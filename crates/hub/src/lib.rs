//! glichen's hub: the grants L2 ([`glichen_notes`]: one note per account,
//! payments made atomic by grants, the hub's book) on a lichen hub
//! (`lichen_hub`), which runs the contracts.
//!
//! Its part of a contract ([`Grants`], a [`HubL2`]): the statement's terms
//! (the issuer, the owner, the mint's value), its one-time keys for
//! `correct`, the account opened at the mint once the funding is out, a
//! withdrawal's claim taken into the book, and the dispute's front: when
//! `request` confirms, the hub takes the claim into its book (the account
//! closed) and weighs it by Algorithm 1, citing the last spend of the
//! account if it takes the claimed note or a later one. Owed in full:
//! nothing, `request_timeout` pays her. Otherwise `correct`, its one-time
//! signatures on `b_corr` and `U` (the cited spend's seed). She accepts
//! (after `w_A`, `accept` pays `b_corr`) or countersigns in `ack`; then the
//! hub proves the statement from her four signatures (lichen proves and
//! asserts).
//!
//! Its HTTP routes, the payments (`glichen_notes::proto`): `POST /pay`,
//! `GET /incoming/<owner>`, `POST /half`, `GET /payment/<id>`, `POST
//! /sign`, `POST /cancel`, `GET /account/<owner>`.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{ensure, Context, Result};
use bitcoin::secp256k1::PublicKey;
use bitcoin::{OutPoint, Sequence, Txid};
use glichen_notes::book::Book;
use glichen_notes::core as acc;
use glichen_proto::{AliceKeys, Countersigned, HubKeys, Keys, SetupTerms, U_LEN};
use k256::schnorr::SigningKey;
use lichen_contract::bit_tree;
use lichen_hub::{Bits, Ctx, FrontStep, HubL2, OpeningFault, ProverInput};
use lichen_proto::hp::{Disputed, HpSetup, HpSetupRequest};
use lichen_proto::{Hash, Layer, OpenRequest, Record};
use lichen_wots::{WotsParams, WotsSecret};
use tiny_http::Method;

pub use lichen_hub::{Config, Hub};

/// The hub's statement guest's image id, hex (the build committed in
/// `r0/guests/hp.id`).
pub const IMAGE_ID: &str = include_str!("../../../r0/guests/hp.id");

/// The grants L2 at the hub: its book, and a test's switch.
pub struct Grants {
    pub book: Book,
    withhold_unlocks: AtomicBool,
}

impl Grants {
    /// The book in `dir` (`book.json`), its issuer's key `l2_key`.
    pub fn open(dir: &std::path::Path, l2_key: &SigningKey) -> Result<Grants> {
        let issuer = glichen_notes::keypair(&l2_key.to_bytes().into()).context("the issuer key")?;
        std::fs::create_dir_all(dir)?;
        Ok(Grants { book: Book::open(dir.join("book.json"), issuer)?, withhold_unlocks: AtomicBool::new(false) })
    }
    /// For tests: record spends but answer none (`POST /spend`): the
    /// payer's spend stays in flight, recorded, its unlock unknown to her.
    pub fn set_withhold_unlocks(&self, on: bool) {
        self.withhold_unlocks.store(on, Ordering::Relaxed);
    }
}

/// The hub's one-time key for `b_corr` in contract `cid` (`correct`).
fn hub_corr(ctx: &dyn Ctx, cid: u32) -> WotsSecret {
    WotsSecret::from_entropy(WotsParams::for_bytes(4), ctx.contract_secret(cid, "hp/bc"))
}

/// The hub's one-time key for `U` in contract `cid` (`correct`).
fn hub_u(ctx: &dyn Ctx, cid: u32) -> WotsSecret {
    WotsSecret::from_entropy(WotsParams::for_bytes(U_LEN as u32), ctx.contract_secret(cid, "hp/u"))
}

/// A dispute's front at the hub.
pub enum Front {
    /// `request` confirmed at `at`; `done` once weighed (owed in full, or
    /// too late to correct).
    Requested(u32, bool),
    /// `correct` confirmed at `at`: her `ack` is due within `w_A`, or
    /// `accept` pays `b_corr`.
    Corrected(u32),
}

/// What the hub holds against a claim: what it's owed, the cited spend,
/// the unlock.
pub struct Assessed {
    pub owed: u32,
    pub cited: Option<acc::Cited>,
    pub u: acc::Seed,
}

impl Grants {
    /// Whether `owner` may open a contract: a key fresh for it, no
    /// contract's before and no account's. Its note 0 is its mint.
    fn owner_free(&self, ctx: &dyn Ctx, owner: &Hash) -> Result<()> {
        if let Some((r, _)) = ctx.contracts().into_iter().find(|(r, _)| r.l2_address == *owner) {
            anyhow::bail!("this owner key has contract {} already: a fresh key for each contract", r.cid());
        }
        ensure!(!self.book.knows(owner), "this owner key has an account already: a fresh key for each contract");
        Ok(())
    }

    /// What the hub holds against a claim `q` of `b` units on contract
    /// `record` (by the statement's own count), from the evidence the book
    /// gives: the cited spend and its seed.
    pub fn assess(&self, record: &Record, b: u32, q: Option<&acc::Claim>, ev: Option<glichen_notes::book::Evidence>) -> Result<Assessed> {
        let k = glichen_proto::keys(record)?;
        let (cited, u) = match ev {
            Some(e) => (Some(e.cited), e.seed),
            None => (None, [0; acc::U_LEN]),
        };
        let owed = acc::owed(q, b, &record.l2_address, &k.issuer, record.terms.unit.to_sat(), k.v0, cited.as_ref(), &u);
        // a citation that doesn't lower the claim isn't posted
        let plain = acc::owed(q, b, &record.l2_address, &k.issuer, record.terms.unit.to_sat(), k.v0, None, &[0; acc::U_LEN]);
        Ok(if owed < plain { Assessed { owed, cited, u } } else { Assessed { owed: plain, cited: None, u: [0; acc::U_LEN] } })
    }

    /// The hub's answer to a request: the claim taken into the book (the
    /// account closed) and weighed; owed in full, nothing (`request_timeout`
    /// pays her, None); otherwise `correct`, signing `b_corr` and `U`.
    /// `correct`'s confirmation height.
    fn correct(&self, ctx: &dyn Ctx, d: &Disputed) -> Result<Option<u32>> {
        let record = &d.record;
        let cid = record.cid();
        let (b, q, _, _) = glichen_proto::requested(&glichen_proto::link(ctx.chain(), d, "request")?)?;
        let q = acc::Claim::from_bytes(&q);
        let ev = match &q {
            Some(c) => self.book.take_claim(cid, &record.l2_address, c)?,
            None => None,
        };
        let a = self.assess(record, b, q.as_ref(), ev)?;
        // a lying hub (tests) corrects a claim to nothing
        let (b_corr, u) = if ctx.forging() { (0, [0; acc::U_LEN]) } else { (a.owed, a.u) };
        ctx.note(format!("contract {cid}: the user's exit claim of {b} units taken into the book"));
        if b_corr == b {
            ctx.note(format!("contract {cid}: the claim is owed in full: request_timeout pays her"));
            return Ok(None);
        }
        ctx.note(format!("contract {cid}: the claim of {b} units is owed {b_corr}{}: correcting it", match (&q, &a.cited) {
            (None, _) => " (malformed)",
            (_, Some(_)) => " (her last step cited, its seed in U)",
            _ => "",
        }));
        let wire = Countersigned::correct_wire(&hub_corr(ctx, cid).sign(&b_corr.to_be_bytes())?, &hub_u(ctx, cid).sign(&u)?)?;
        let tx = d.finish("correct", wire)?;
        let at = ctx.post(cid, &tx)?;
        ctx.note(format!("contract {cid}: corrected to {b_corr} units ({} vB)", tx.vsize()));
        Ok(Some(at))
    }

    /// The statement's input for an acked dispute: the contract, her four
    /// signatures from `request` and `ack`, the spend `correct` cited.
    fn input(&self, ctx: &dyn Ctx, d: &Disputed) -> Result<acc::Input> {
        let record = &d.record;
        let chain = ctx.chain();
        let (_, q, q_sig, b_sig) = glichen_proto::requested(&glichen_proto::link(chain, d, "request")?)?;
        let (c_sig, u_sig) = glichen_proto::acked(&glichen_proto::link(chain, d, "ack")?)?;
        let (_, u) = glichen_proto::corrected(&glichen_proto::link(chain, d, "correct")?)?;
        let cited = match acc::Claim::from_bytes(&q) {
            Some(c) if u != [0; acc::U_LEN] => self.book.evidence(&record.l2_address, c.k).filter(|e| e.seed == u).map(|e| e.cited),
            _ => None,
        };
        Ok(acc::Input { contract: glichen_proto::record_contract(record)?, sigs: [q_sig, b_sig, c_sig, u_sig], cited })
    }

    /// The link `name`'s output spent by other than `next`: a terminal (its
    /// payout) is out.
    fn ended(ctx: &dyn Ctx, d: &Disputed, name: &str, next: &str) -> Result<bool> {
        let op = OutPoint { txid: d.txid(name), vout: 0 };
        Ok(!ctx.chain().unspent(&op)? && ctx.chain().get_tx(&d.txid(next)).is_none())
    }

    /// Sweep its bit outputs of a payout the user won: for each bit clear
    /// in her signed `b`, the output of `u 2^i`, by its leaf `hub_bit_i`
    /// (her signature on `b`, as her claim posted it, and the hub's).
    fn sweep_bits(ctx: &dyn Ctx, d: &Disputed) -> Result<Bits> {
        let req = glichen_proto::link(ctx.chain(), d, "request")?;
        let (bsig, _) = Countersigned::read_request(&lichen_proto::setup::link_args(&req))?;
        let b = u32::from_be_bytes(bsig.message().try_into().map_err(|_| anyhow::anyhow!("b's signature signs 4 bytes"))?);
        let k = glichen_proto::keys(&d.record)?;
        let core = d.graph.core();
        let (t, ctx2) = (core.terms, core.ctx());
        let mut trees = vec![];
        for i in (t.low_bits..t.bits).filter(|i| (b >> i) & 1 == 0) {
            let tree = bit_tree(&ctx2, i, &k.alice.b_key)?;
            trees.push((tree.script_pubkey(), tree, format!("hub_bit_{i}")));
        }
        ctx.sweep(&d.record, trees, &Countersigned::word_wire(&bsig), Sequence::ENABLE_RBF_NO_LOCKTIME)
    }

    /// Sweep its bit outputs of a payout of `b_corr` (`accept`, spending
    /// `correct`'s output, or `success`, `assert`'s: `link`): for each bit
    /// clear in the hub's signed `b_corr` (in `correct`), by its leaf
    /// `hub_corr_bit_i`, after `Δ` (until then, not yet).
    fn sweep_corr_bits(ctx: &dyn Ctx, d: &Disputed, link: Txid) -> Result<Bits> {
        let correct = glichen_proto::link(ctx.chain(), d, "correct")?;
        let (sig, _) = Countersigned::read_correct(&lichen_proto::setup::link_args(&correct))?;
        let b_corr = u32::from_be_bytes(sig.message().try_into().map_err(|_| anyhow::anyhow!("b_corr's signature signs 4 bytes"))?);
        let k = glichen_proto::keys(&d.record)?;
        let core = d.graph.core();
        let (t, cctx) = (core.terms, core.ctx());
        let delta = cctx.params.delta;
        // the payout's txid is known once posted with its fee coin
        let a_op = OutPoint { txid: link, vout: 0 };
        let from = ctx.confirmed_at(&a_op.txid)?.context("the payout's link")?;
        let Some(payout) = ctx.chain().spender(&a_op, from)? else { return Ok(Bits::NotYet) };
        if ctx.chain().confirmations(&payout)? <= u32::from(delta) {
            return Ok(Bits::NotYet);
        }
        let mut trees = vec![];
        for i in (t.low_bits..t.bits).filter(|i| (b_corr >> i) & 1 == 0) {
            let tree = glichen_proto::front::corr_bit_tree(&cctx, i, &k.hub.corr_key)?;
            trees.push((tree.script_pubkey(), tree, format!("hub_corr_bit_{i}")));
        }
        ctx.sweep(&d.record, trees, &Countersigned::word_wire(&sig), Sequence::from_height(delta))
    }
}

impl Layer for Grants {
    fn front(&self, record: &Record) -> Result<Box<dyn lichen_babe::graph::Front>> {
        glichen_proto::front(record)
    }
    fn journal(&self, record: &Record) -> Result<Vec<u8>> {
        glichen_proto::journal(record)
    }
}

fn setup_terms(v: &serde_json::Value) -> Result<SetupTerms> {
    serde_json::from_value(v.clone()).context("the setup's terms")
}

impl HubL2 for Grants {
    type Front = Front;

    fn info(&self) -> serde_json::Value {
        serde_json::json!({ "issuer": hex::encode(self.book.issuer()), "hp": IMAGE_ID.trim() })
    }
    fn version(&self) -> String {
        glichen_proto::VERSION.to_string()
    }
    fn route(&self, _ctx: &dyn Ctx, method: &Method, path: &str, body: &str) -> Option<Result<String>> {
        let owner = |rest: &str| -> Result<Hash> { hex::decode(rest).ok().and_then(|v| v.try_into().ok()).context("an owner: 64 hex digits") };
        let release = !self.withhold_unlocks.load(Ordering::Relaxed);
        use glichen_notes::proto::{CancelRequest, HalfRequest, PayRequest, SignRequest};
        let r = || -> Result<Option<String>> {
            if let (Method::Get, Some(rest)) = (method, path.strip_prefix("/incoming/")) {
                return Ok(Some(serde_json::to_string(&self.book.incoming(&owner(rest)?))?));
            }
            if let (Method::Get, Some(rest)) = (method, path.strip_prefix("/account/")) {
                return Ok(Some(serde_json::to_string(&self.book.account(&owner(rest)?))?));
            }
            if let (Method::Get, Some(id)) = (method, path.strip_prefix("/payment/")) {
                return Ok(Some(serde_json::to_string(&self.book.payment(id).with_context(|| format!("no payment {id}"))?)?));
            }
            Ok(match (method, path) {
                (Method::Post, "/pay") => {
                    let r: PayRequest = serde_json::from_str(body)?;
                    Some(serde_json::to_string(&self.book.pay(r.half, r.to, r.amount)?)?)
                }
                (Method::Post, "/half") => {
                    let r: HalfRequest = serde_json::from_str(body)?;
                    Some(serde_json::to_string(&self.book.half(&r.id, r.half)?)?)
                }
                (Method::Post, "/sign") => {
                    let r: SignRequest = serde_json::from_str(body)?;
                    Some(serde_json::to_string(&self.book.sign(&r.id, &r.owner, &r.psi, release)?)?)
                }
                (Method::Post, "/cancel") => {
                    let r: CancelRequest = serde_json::from_str(body)?;
                    Some(serde_json::to_string(&self.book.cancel(&r.id, &r.owner, &r.sig, release)?)?)
                }
                _ => None,
            })
        };
        r().transpose()
    }

    fn check_owner(&self, ctx: &dyn Ctx, owner: &Hash) -> Result<()> {
        self.owner_free(ctx, owner)
    }
    fn setup_terms(&self, ctx: &dyn Ctx, req: &HpSetupRequest) -> Result<serde_json::Value> {
        let mut t = setup_terms(&req.l2)?;
        ensure!(t.v0 < acc::VALUE_LIMIT, "a mint too large");
        self.owner_free(ctx, &t.owner)?;
        t.issuer = Some(self.book.issuer());
        Ok(serde_json::to_value(t)?)
    }
    fn same_setup(&self, setup: &HpSetup, req: &HpSetupRequest) -> bool {
        match (setup_terms(&setup.l2), setup_terms(&req.l2)) {
            (Ok(a), Ok(b)) => a.owner == b.owner && a.v0 == b.v0,
            _ => false,
        }
    }
    fn check_keys(&self, keys: &serde_json::Value) -> Result<()> {
        let k: AliceKeys = serde_json::from_value(keys.clone()).context("the user's one-time keys")?;
        k.check()
    }
    fn setup_journal(&self, cid: u32, setup: &HpSetup, keys: &serde_json::Value) -> Result<Vec<u8>> {
        let k: AliceKeys = serde_json::from_value(keys.clone()).context("the user's one-time keys")?;
        glichen_proto::setup_journal(cid, setup.unit, &setup_terms(&setup.l2)?, &k)
    }
    fn open_keys(&self, ctx: &dyn Ctx, setup: &HpSetup, keys: &serde_json::Value, req: &OpenRequest) -> Result<(serde_json::Value, serde_json::Value)> {
        let t = setup_terms(&setup.l2)?;
        let v0 = req.deposit - req.fee;
        ensure!(t.owner == req.l2_address && t.v0 == v0, "the setup is for another owner or mint than the opening's");
        self.owner_free(ctx, &t.owner)?;
        let alice: AliceKeys = serde_json::from_value(keys.clone()).context("the user's one-time keys")?;
        let cid = setup.cid;
        let hub = HubKeys { corr_key: hub_corr(ctx, cid).public(), u_key: hub_u(ctx, cid).public(), windows: glichen_proto::windows(ctx.cfg().network) };
        let issuer = t.issuer.context("the setup names no issuer")?;
        let k = Keys { alice, hub: hub.clone(), issuer, v0 };
        Ok((serde_json::to_value(k)?, serde_json::to_value(hub)?))
    }
    fn offered(&self, _ctx: &dyn Ctx, record: &Record, _mint_point: &PublicKey, fault: OpeningFault) -> Result<serde_json::Value> {
        ensure!(fault != OpeningFault::ShortMint || record.minted() == 0, "contract {}: a short mint (a test's fault) has no meaning on accounts", record.cid());
        Ok(serde_json::Value::Null)
    }
    fn funded(&self, _ctx: &dyn Ctx, record: &Record) -> Result<()> {
        // its account, opened at the mint: the funding is out
        self.book.open_account(record.l2_address, record.minted())
    }
    fn take_claim(&self, ctx: &dyn Ctx, record: &Record, b: u32, claim: &str, rest_to: Option<Hash>) -> Result<()> {
        let posted = hex::decode(claim).context("the claim's hex")?;
        let cid = record.cid();
        let q = acc::Claim::from_bytes(&posted).context("not a claim's bytes")?;
        let owner = record.l2_address;
        let live = self.book.account(&owner);
        ensure!(!live.closed, "the account is closed");
        ensure!(live.busy.is_none(), "a payment is pending on the account: finish or cancel it first");
        // the note, not the signature's bytes: hers came from the grant's
        // pre-signature, the book's is signed afresh; Algorithm 1 checks hers
        let n = live.live.note;
        ensure!(q.k == n.k && (q.k == 0 || q.v == n.v), "the claim isn't of the account's live note");
        let unit = record.terms.unit.to_sat();
        let owed = self.assess(record, b, Some(&q), self.book.evidence(&owner, q.k))?.owed;
        ensure!(owed == b, "the claim is owed {owed} units, not {b}");
        self.book.take_claim(cid, &owner, &q)?;
        let rest = live.live.note.v - u64::from(b) * unit;
        if let (Some(to), true) = (rest_to, rest > 0) {
            self.book.credit(&to, rest)?;
        }
        ctx.note(format!("contract {cid}: the user's withdrawal claim of {b} units taken (note {} of {} sats{})", q.k, live.live.note.v, if rest > 0 { format!(", {rest} sats left as credit") } else { String::new() }));
        Ok(())
    }

    fn front_start(&self, ctx: &dyn Ctx, d: &Disputed, at: u32) -> Result<FrontStep<Front>> {
        if let Some(a) = ctx.confirmed_at(&d.txid("ack"))? {
            return Ok(FrontStep::Prove { at: a });
        }
        if let Some(c) = ctx.confirmed_at(&d.txid("correct"))? {
            return Ok(FrontStep::Wait(Front::Corrected(c)));
        }
        Ok(FrontStep::Wait(Front::Requested(at, false)))
    }
    fn front_tick(&self, ctx: &dyn Ctx, d: &Disputed, s: Front) -> Result<FrontStep<Front>> {
        let cid = d.record.cid();
        let w = glichen_proto::keys(&d.record)?.hub.windows;
        match s {
            Front::Requested(at, done) => {
                if let Some(c) = ctx.confirmed_at(&d.txid("correct"))? {
                    return Ok(FrontStep::Wait(Front::Corrected(c)));
                }
                if Self::ended(ctx, d, "request", "correct")? {
                    ctx.note(format!("contract {cid}: request_timeout paid the user her claim"));
                    return Ok(FrontStep::Paid { link: d.txid("request"), terminal: "request_timeout".into() });
                }
                if done {
                    return Ok(FrontStep::Wait(Front::Requested(at, true)));
                }
                let last_chance = at + u32::from(w.w_h) - (u32::from(w.w_h) / 4).max(2);
                if ctx.chain().height()? >= last_chance {
                    ctx.note_once(format!("contract {cid}: too late to correct the claim; request_timeout will pay the user"));
                    return Ok(FrontStep::Wait(Front::Requested(at, true)));
                }
                Ok(FrontStep::Wait(match self.correct(ctx, d)? {
                    None => Front::Requested(at, true),
                    Some(c) => Front::Corrected(c),
                }))
            }
            Front::Corrected(at) => {
                if let Some(a) = ctx.confirmed_at(&d.txid("ack"))? {
                    ctx.note(format!("contract {cid}: the user countersigned the correction (ack): the statement is due"));
                    return Ok(FrontStep::Prove { at: a });
                }
                if Self::ended(ctx, d, "correct", "ack")? {
                    ctx.note(format!("contract {cid}: accept paid the user b_corr"));
                    return Ok(FrontStep::Paid { link: d.txid("correct"), terminal: "accept".into() });
                }
                if ctx.chain().height()? >= at + u32::from(w.w_a) {
                    let txid = ctx.post_terminal(d, "accept", vec![])?;
                    ctx.note(format!("contract {cid}: the user accepted the correction: accept {txid} pays her b_corr"));
                    return Ok(FrontStep::Paid { link: d.txid("correct"), terminal: "accept".into() });
                }
                Ok(FrontStep::Wait(Front::Corrected(at)))
            }
        }
    }
    fn prover_input(&self, ctx: &dyn Ctx, d: &Disputed) -> Result<ProverInput> {
        let input = self.input(ctx, d)?;
        match acc::check(&input) {
            Ok(j) => {
                ensure!(j[..] == glichen_proto::journal(&d.record)?[..], "the statement's journal isn't the contract's");
                Ok(ProverInput::Holds(input.to_bytes()))
            }
            Err(e) => Ok(ProverInput::Fails(format!("{e:?}"))),
        }
    }
    fn collect(&self, ctx: &dyn Ctx, d: &Disputed, link: Txid, terminal: &str) -> Result<Bits> {
        match terminal {
            "accept" | "success" => Self::sweep_corr_bits(ctx, d, link),
            _ => Self::sweep_bits(ctx, d),
        }
    }
    fn describe(&self, s: &Front) -> String {
        match s {
            Front::Requested(_, false) => "requested: the hub weighs the claim".into(),
            Front::Requested(_, true) => "requested: owed in full, request_timeout pays the user".into(),
            Front::Corrected(..) => "corrected: the user accepts or countersigns".into(),
        }
    }
}
