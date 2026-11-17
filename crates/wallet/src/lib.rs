//! glichen's wallet: a lichen wallet (`lichen_wallet`) with the grants L2
//! ([`Grants`]): its accounts ([`notes`], one note per key, payments by
//! joint spends: [`Payments`]), its part of each contract (the statement's
//! terms, the user's four one-time keys, the account opened at the mint,
//! a withdrawal's claim of the account's live note), and the dispute's
//! front ([`front`]).

pub mod exit;
pub mod front;
pub mod notes;

use std::collections::BTreeMap;

use anyhow::{ensure, Context, Result};
use bitcoin::secp256k1::SecretKey;
use bitcoin::{OutPoint, Sequence, Transaction};
use glichen_proto::{AliceKeys, Countersigned, HubKeys, Keys, SetupTerms};
use lichen_chain::Chain;
use lichen_contract::{bit_tree, reserve_tree};
use lichen_proto::hp::{Disputed, HpOffer, HpSetup};
use lichen_proto::{Hash, Info, Layer, OpenOffer, Record, Statement};
use lichen_wallet::engine::{ContractSecrets, Engine, Outcome};
use lichen_wallet::{FrontEnd, Quote, SweepIn, Wallet, WalletL2};
use serde::{Deserialize, Serialize};

pub use exit::ExitPlan;
pub use lichen_wallet::{dispute, engine, hub_call, hub_info, Ask, Deposit, Held, Opening, PAYOUT_FEERATE};
pub use notes::{Acct, Payments, Pending, Step};

/// The hub's statement guest's image id, hex (glichen's build,
/// `r0/guests/hp.id`): the hub must prove with the same.
pub const IMAGE_ID: &str = include_str!("../../../r0/guests/hp.id");

/// A glichen wallet.
pub type GrantsWallet = Wallet<Grants>;

/// The grants L2 in the wallet file: the hub's issuer (its deployment,
/// pinned when the wallet was made) and our accounts, by owner (hex): each
/// one's live note and a payment pending.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grants {
    #[serde(with = "glichen_notes::hex32")]
    pub issuer: Hash,
    #[serde(default)]
    pub accounts: BTreeMap<String, Acct>,
}

impl Layer for Grants {
    fn front(&self, record: &Record) -> Result<Box<dyn lichen_babe::graph::Front>> {
        glichen_proto::front(record)
    }
    fn journal(&self, record: &Record) -> Result<Vec<u8>> {
        glichen_proto::journal(record)
    }
}

fn info_field<'a>(info: &'a Info, name: &str) -> Result<&'a str> {
    info.l2.get(name).and_then(|v| v.as_str()).with_context(|| format!("the hub names no {name}: it runs no glichen of this wallet's version ({})", glichen_proto::VERSION))
}

fn plan(v: &serde_json::Value) -> Result<ExitPlan> {
    serde_json::from_value(v.clone()).context("the exit plan")
}

impl WalletL2 for Grants {
    fn create(info: &Info) -> Result<Grants> {
        let issuer = hex::decode(info_field(info, "issuer")?)?.try_into().map_err(|_| anyhow::anyhow!("an issuer of other than 32 bytes"))?;
        let g = Grants { issuer, accounts: BTreeMap::new() };
        g.check_info(info)?;
        Ok(g)
    }
    fn check_info(&self, info: &Info) -> Result<()> {
        let issuer = info_field(info, "issuer")?;
        ensure!(issuer == hex::encode(self.issuer), "the hub is in another deployment (issuer {issuer}) than this wallet's (issuer {})", hex::encode(self.issuer));
        let hp = info_field(info, "hp")?;
        let commit = info.commit.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
        ensure!(hp == IMAGE_ID.trim(), "the hub proves with another guest than this wallet's: it runs glichen {}{commit}, this wallet {}; use a wallet of the hub's version", if info.version.is_empty() { "?" } else { &info.version }, glichen_proto::VERSION);
        Ok(())
    }
    fn version() -> String {
        glichen_proto::VERSION.to_string()
    }

    fn setup_terms(_w: &Wallet<Self>, _cid: u32, quote: &Quote, owner: &Hash) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(SetupTerms { owner: *owner, v0: quote.minted, issuer: None })?)
    }
    fn check_setup(w: &Wallet<Self>, _chain: &Chain, setup: &HpSetup, asked: &serde_json::Value) -> Result<()> {
        let (got, asked): (SetupTerms, SetupTerms) = (serde_json::from_value(setup.l2.clone())?, serde_json::from_value(asked.clone())?);
        ensure!(got.owner == asked.owner && got.v0 == asked.v0, "the setup is for another owner or mint");
        ensure!(got.issuer == Some(w.l2.issuer), "the setup's issuer isn't the deployment's");
        Ok(())
    }
    fn alice_keys(s: &ContractSecrets) -> serde_json::Value {
        serde_json::to_value(front::alice_keys(s)).expect("keys serialize")
    }
    fn setup_journal(cid: u32, setup: &HpSetup, keys: &serde_json::Value) -> Result<Vec<u8>> {
        let k: AliceKeys = serde_json::from_value(keys.clone())?;
        glichen_proto::setup_journal(cid, setup.unit, &serde_json::from_value(setup.l2.clone())?, &k)
    }
    fn record_keys(w: &Wallet<Self>, setup: &HpSetup, keys: &serde_json::Value, offer: &HpOffer, quote: &Quote) -> Result<serde_json::Value> {
        let hub: HubKeys = serde_json::from_value(offer.l2.clone()).context("the hub's keys in its offer")?;
        hub.check()?;
        let t: SetupTerms = serde_json::from_value(setup.l2.clone())?;
        let issuer = t.issuer.context("the setup names no issuer")?;
        ensure!(issuer == w.l2.issuer && t.v0 == quote.minted, "the setup's issuer or mint isn't the deployment's or the quote's");
        Ok(serde_json::to_value(Keys { alice: serde_json::from_value(keys.clone())?, hub, issuer, v0: t.v0 })?)
    }
    fn check_offer(_w: &Wallet<Self>, record: &Record, _offer: &OpenOffer) -> Result<()> {
        let k = glichen_proto::keys(record)?;
        ensure!(k.hub.windows == glichen_proto::windows(record.network), "the hub's windows {:?}, not the network's", k.hub.windows);
        Ok(())
    }
    fn minted(w: &mut Wallet<Self>, held: &Held, _t: &SecretKey) -> Result<()> {
        // the account opens at the mint, (owner, 0, D − fee), which the
        // contract's journal binds
        let (owner, v0) = (held.record.l2_address, held.record.minted());
        w.l2.accounts.entry(hex::encode(owner)).or_insert_with(|| Acct { live: glichen_notes::Settled::first(owner, v0), pending: None, closed: false });
        Ok(())
    }

    fn withdrawal(w: &mut Wallet<Self>, _chain: &Chain, cid: u32, units: u32) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(w.claim_for(cid, units)?)?)
    }
    fn plan_b(p: &serde_json::Value) -> Result<u32> {
        Ok(plan(p)?.b)
    }
    fn plan_claim(p: &serde_json::Value) -> Result<String> {
        Ok(plan(p)?.claim)
    }
    fn rest_to(w: &Wallet<Self>) -> Option<Hash> {
        w.owners().first().copied()
    }
    fn folded(w: &mut Wallet<Self>, cid: u32, units: u32) -> Result<()> {
        if units > 0 {
            w.close_account(cid)?;
        }
        // the rest of the account's note, as credit
        let _ = w.sync();
        Ok(())
    }
    fn exit_plan(w: &mut Wallet<Self>, chain: &Chain, cid: u32) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(exit::exit_claim(w, chain, cid)?)?)
    }
    fn exited(w: &mut Wallet<Self>, cid: u32) -> Result<()> {
        // the hub took the claim at request: the account is closed
        w.close_account(cid)
    }

    fn unlocked(w: &mut Wallet<Self>, k: usize, z: &[u8]) -> Result<Option<u64>> {
        let z: glichen_notes::Seed = z.try_into().map_err(|_| anyhow::anyhow!("an unlock of other than 16 bytes"))?;
        let owner = w.contracts[k].record.l2_address;
        if !w.opens(&owner, &z) {
            return Ok(None);
        }
        match w.complete(&owner, &z)? {
            Step::Done(_, s) => Ok(Some(s.note.v)),
            _ => Ok(None),
        }
    }
    fn run_front(e: &mut Engine<'_>, chain: &Chain, d: &Disputed, p: &serde_json::Value) -> Result<FrontEnd> {
        front::run(e, chain, d, &plan(p)?)
    }
    fn payout_inputs(w: &Wallet<Self>, chain: &Chain, held: &Held, statement: &Statement, outcome: &Outcome, payout: &Transaction, b: u32) -> Result<(Vec<SweepIn>, Vec<Vec<u8>>, u16)> {
        let txid = payout.compute_txid();
        let k = glichen_proto::keys(&held.record)?;
        let g = glichen_proto::front(&held.record)?;
        let core = g.core();
        let (t, ctx) = (core.terms, core.ctx());
        let delta = core.params.delta;
        let mut ins: Vec<SweepIn> = vec![];
        let at_delta = Sequence::from_height(delta);
        let (b_wire, wait) = match outcome {
            // accept, success: the bits set in the hub's b_corr, by its
            // signature (in correct), at once; her reserve came to her
            // payout address
            Outcome::Corrected { b_corr, .. } => {
                let built = held.record.build(statement, &w.l2)?;
                let correct_id = lichen_babe::graph::txid_in(&built.presign(held.record.funding)?, "correct")?;
                let correct = chain.get_tx(&correct_id).context("correct")?;
                let (sig, _) = Countersigned::read_correct(&lichen_proto::setup::link_args(&correct))?;
                for i in (t.low_bits..t.bits).filter(|i| (b_corr >> i) & 1 == 1) {
                    let vout = i - t.low_bits;
                    ins.push((OutPoint { txid, vout }, payout.output[vout as usize].clone(), Sequence::ENABLE_RBF_NO_LOCKTIME, Some((glichen_proto::front::corr_bit_tree(&ctx, i, &k.hub.corr_key)?, format!("alice_corr_bit_{i}"), true))));
                }
                (Countersigned::word_wire(&sig), 0)
            }
            // her outputs: the bits set in b, then the reserves
            _ => {
                for i in t.low_bits..t.bits {
                    if (b >> i) & 1 == 1 {
                        let vout = i - t.low_bits;
                        ins.push((OutPoint { txid, vout }, payout.output[vout as usize].clone(), at_delta, Some((bit_tree(&ctx, i, &k.alice.b_key)?, format!("alice_bit_{i}"), true))));
                    }
                }
                let vout = t.bits - t.low_bits;
                ins.push((OutPoint { txid, vout }, payout.output[vout as usize].clone(), at_delta, Some((reserve_tree(&ctx, t, &k.alice.b_key)?, "alice_reserve".to_string(), false))));
                (front::b_wire(&w.secrets(&held.nonce), b)?, delta)
            }
        };
        Ok((ins, b_wire, wait))
    }
}
