//! glichen's dispute front (papers/lichen-grants §5–6): every value the
//! hub's statement reads at exit time is under one of the user's one-time
//! keys. Then BABE's tail ([`lichen_babe::graph`]).
//!
//! ```text
//! request (the user: Q and b, her SHA-256 keys)
//!  ├─ request_timeout (after w_H)       → the user: b
//!  └─ correct (the hub: b_corr and U, its HASH160 keys)
//!      ├─ accept (after w_A)            → the user: b_corr (by the hub's
//!      │                                   signature), her reserve; the
//!      │                                   rest the hub's
//!      └─ ack (the user: the hub's two signatures again, and hers on
//!          │    the same values, digit by digit equal)
//!          ├─ ack_timeout (after w_H)   → the user: b
//!          └─ assert … (BABE's tail; success pays b_corr)
//! ```
//!
//! The payouts: `b`'s bit outputs read her signature under `K_b` (SHA-256:
//! the statement checks it too), `b_corr`'s the hub's under `K_corr`
//! ([`corr_bit_tree`]). The keys whose values no bit output reads (`K_Q`,
//! `K_U`, `A_corr`, `A_U`) have no equivocation leaf.
//!
//! `ack` is the countersignature: script verifies the hub's signature on a
//! value and hers, and checks their digits equal, so she can only sign
//! what the hub posted. The hub's statement reads `b_corr` and `U` from
//! her signatures, not its own: a second signature of its own, off chain,
//! no one would see.

use anyhow::{ensure, Context, Result};
use lichen_babe::graph::{link_leaf, verify_drop, Front, FrontLink, Payout};
use lichen_btc::bitcoin::opcodes::all::*;
use lichen_btc::bitcoin::script::Builder;
use lichen_btc::bitcoin::{Amount, TxOut};
use lichen_btc::script::BuilderExt;
use lichen_btc::taptree::{Leaf, TapTree};
use lichen_btc::tx::Timelock;
use lichen_contract::{wots_shape, ContractCtx, ContractParams, Core, PartyPubKeys, Role, Terms};
use lichen_wots::{Chain, Wots256Public, Wots256Sig, WotsExt, WotsParams, WotsPublic, WotsPublicOf, WotsSig, WotsSigOf};

pub use glichen_statement::accounts::{Q_LEN, U_LEN};

/// The front's windows, relative, in blocks: `w_H` for the hub's moves
/// (`correct`, and `assert`, which must cover proving), `w_A` for her
/// `ack`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Windows {
    pub w_h: u16,
    pub w_a: u16,
}

fn ok_leaf(name: &str, b: Builder, timelock: Timelock) -> Leaf {
    Leaf::new(name, b.push_int(1).into_script(), timelock)
}

/// Drop `n` stack items.
fn drop_n(b: Builder, n: usize) -> Builder {
    lichen_babe::graph::drop_n(b, n)
}

/// The countersignature: verify a signature under `first` (the hub's),
/// then one under `second` (hers), and check their message digits equal,
/// one by one. Consumes both; leaves nothing.
pub fn digits_equal<H1: Chain, H2: Chain>(b: Builder, first: &WotsPublicOf<H1>, second: &WotsPublicOf<H2>) -> Builder {
    let m = first.params.message_digits as usize;
    assert_eq!(second.params.message_digits as usize, m, "both keys sign the same length");
    // the first's digits to the altstack: d_0 ends on top
    let mut b = b.wots_verify(first);
    for _ in 0..m {
        b = b.push_opcode(OP_TOALTSTACK);
    }
    // the second's digits d'_0 … d'_{m-1}, d'_{m-1} on top; before step
    // j, d'_j … d'_{m-1} remain, and d_j, from the altstack, sits on them:
    // d'_j at depth m - j
    b = b.wots_verify(second);
    for j in 0..m {
        b = b.push_opcode(OP_FROMALTSTACK).push_int((m - j) as i64).push_opcode(OP_ROLL).push_opcode(OP_EQUALVERIFY);
    }
    b
}

/// What the front is built from.
#[derive(Clone, Debug)]
pub struct Spec {
    pub terms: Terms,
    pub params: ContractParams,
    /// The user's keys, then the hub's.
    pub pubs: [PartyPubKeys; 2],
    /// The user's one-time keys in `request`: `K_b` (the payout's bit
    /// outputs read it) and `K_Q`; in `ack`, `A_corr` and `A_U`. SHA-256
    /// chains: the hub's statement verifies them.
    pub b_key: Wots256Public,
    pub q_key: Wots256Public,
    pub a_corr_key: Wots256Public,
    pub a_u_key: Wots256Public,
    /// The hub's one-time keys in `correct`: `K_corr` (the bit outputs of
    /// `accept` and `success` read it) and `K_U`. HASH160: only script
    /// reads them.
    pub corr_key: WotsPublic,
    pub u_key: WotsPublic,
    pub windows: Windows,
}

/// The front: `request`, `correct`, `ack`.
pub struct Countersigned {
    pub spec: Spec,
}

impl Countersigned {
    pub fn new(spec: Spec) -> Result<Countersigned> {
        let word = WotsParams::for_bytes(4);
        ensure!(spec.b_key.params == word && spec.corr_key.params == word && spec.a_corr_key.params == word, "the keys for b and b_corr sign a 4-byte word");
        ensure!(spec.q_key.params == WotsParams::for_bytes(Q_LEN as u32), "K_Q signs {Q_LEN} bytes");
        let u = WotsParams::for_bytes(U_LEN as u32);
        ensure!(spec.u_key.params == u && spec.a_u_key.params == u, "the keys for U sign {U_LEN} bytes");
        Ok(Countersigned { spec })
    }
    fn ctx(&self) -> ContractCtx<'_> {
        self.core().ctx()
    }
    /// The outputs paying `b` in bits and both reserves to the user.
    pub fn pay_alice(&self, available: Amount) -> Result<Vec<TxOut>> {
        self.core().pay_alice(&self.spec.b_key, available)
    }
    /// `accept`'s and `success`'s outputs: for each bit `i` an output of
    /// `u 2^i`, the user's iff bit `i` of the hub's signed `b_corr` is set
    /// ([`corr_bit_tree`]); her reserve back to her; the rest the hub's
    /// (it bears the dispute's floors).
    pub fn pay_corrected(&self, available: Amount) -> Result<Vec<TxOut>> {
        let core = self.core();
        let (t, ctx) = (core.terms, core.ctx());
        let mut outs: Vec<TxOut> = (t.low_bits..t.bits).map(|i| Ok(TxOut { value: t.unit * (1u64 << i), script_pubkey: corr_bit_tree(&ctx, i, &self.spec.corr_key)?.script_pubkey() })).collect::<Result<_>>()?;
        let paid: Amount = outs.iter().map(|o| o.value).sum();
        outs.push(TxOut { value: t.reserve_alice, script_pubkey: self.spec.pubs[0].payout_spk.clone() });
        let rest = available.checked_sub(paid + t.reserve_alice).context("a payout of b_corr can't pay V_max and the user's reserve")?;
        if rest >= ctx.params.dust {
            outs.push(TxOut { value: rest, script_pubkey: self.spec.pubs[1].payout_spk.clone() });
        }
        Ok(outs)
    }

    // ----- the links' witnesses, below both signatures -----

    /// One-time signatures as a leaf takes them, `sigs` in the order its
    /// script verifies them: the wire lists the last-verified bottom
    /// first.
    fn wire_of(sigs: &[Vec<Vec<u8>>]) -> Vec<Vec<u8>> {
        sigs.iter().rev().flat_map(|c| c.iter().rev().cloned()).collect()
    }
    /// Read `n` signatures' consumption orders off `args` (consumption
    /// order, below both signatures), of `params` each.
    fn read_sigs(args: &[Vec<u8>], params: &[WotsParams]) -> Result<Vec<Vec<Vec<u8>>>> {
        let need: usize = params.iter().map(|p| 2 * p.total_digits() as usize).sum();
        ensure!(args.len() == need, "{} arguments, not {need}", args.len());
        let mut at = 0;
        Ok(params
            .iter()
            .map(|p| {
                let n = 2 * p.total_digits() as usize;
                at += n;
                args[at - n..at].to_vec()
            })
            .collect())
    }
    fn sig_of<H: Chain>(params: WotsParams, items: &[Vec<u8>]) -> Result<WotsSigOf<H>> {
        WotsSigOf::<H>::from_consumption_order(params, items)
    }

    /// `request`'s: her signatures on `b` and on `Q`.
    pub fn request_wire(b: &Wots256Sig, q: &Wots256Sig) -> Result<Vec<Vec<u8>>> {
        ensure!(q.message().len() == Q_LEN, "a Q of {} bytes", q.message().len());
        Ok(Self::wire_of(&[b.consumption_order(), q.consumption_order()]))
    }
    /// Read `request`'s arguments: her signatures on `b` and on `Q`.
    pub fn read_request(args: &[Vec<u8>]) -> Result<(Wots256Sig, Wots256Sig)> {
        let (w, q) = (WotsParams::for_bytes(4), WotsParams::for_bytes(Q_LEN as u32));
        let s = Self::read_sigs(args, &[w, q])?;
        Ok((Self::sig_of(w, &s[0])?, Self::sig_of(q, &s[1])?))
    }
    /// `correct`'s: the hub's signatures on `b_corr` and on `U`.
    pub fn correct_wire(b_corr: &WotsSig, u: &WotsSig) -> Result<Vec<Vec<u8>>> {
        ensure!(u.message().len() == U_LEN, "a U of {} bytes", u.message().len());
        Ok(Self::wire_of(&[b_corr.consumption_order(), u.consumption_order()]))
    }
    /// Read `correct`'s arguments: the hub's signatures on `b_corr` and
    /// on `U`.
    pub fn read_correct(args: &[Vec<u8>]) -> Result<(WotsSig, WotsSig)> {
        let (w, u) = (WotsParams::for_bytes(4), WotsParams::for_bytes(U_LEN as u32));
        let s = Self::read_sigs(args, &[w, u])?;
        Ok((Self::sig_of(w, &s[0])?, Self::sig_of(u, &s[1])?))
    }
    /// `ack`'s: the hub's signature on `b_corr` and hers, then the hub's
    /// on `U` and hers.
    pub fn ack_wire(hub_corr: &WotsSig, a_corr: &Wots256Sig, hub_u: &WotsSig, a_u: &Wots256Sig) -> Vec<Vec<u8>> {
        Self::wire_of(&[hub_corr.consumption_order(), a_corr.consumption_order(), hub_u.consumption_order(), a_u.consumption_order()])
    }
    /// Read `ack`'s arguments: her signatures on `b_corr` and on `U`.
    pub fn read_ack(args: &[Vec<u8>]) -> Result<(Wots256Sig, Wots256Sig)> {
        let (w, u) = (WotsParams::for_bytes(4), WotsParams::for_bytes(U_LEN as u32));
        let s = Self::read_sigs(args, &[w, w, u, u])?;
        Ok((Self::sig_of(w, &s[1])?, Self::sig_of(u, &s[3])?))
    }
    /// A word's signature as a bit output's leaf takes it (bottom first).
    pub fn word_wire<H: Chain>(sig: &WotsSigOf<H>) -> Vec<Vec<u8>> {
        sig.consumption_order().into_iter().rev().collect()
    }
}

impl Front for Countersigned {
    fn core(&self) -> Core<'_> {
        Core { terms: &self.spec.terms, params: &self.spec.params, pubs: &self.spec.pubs }
    }
    fn links(&self) -> Result<Vec<FrontLink>> {
        let (s, ctx) = (&self.spec, self.ctx());
        Ok(vec![
            FrontLink {
                name: "request",
                poster: Role::User,
                leaf: link_leaf(&ctx, "request", true, |b| verify_drop(verify_drop(b, &s.b_key), &s.q_key)),
                shape: wots_shape([&s.q_key, &s.b_key]),
                terminal: "request_timeout",
                window: s.windows.w_h,
                pays: Payout::User,
            },
            FrontLink {
                name: "correct",
                poster: Role::Hub,
                leaf: link_leaf(&ctx, "correct", false, |b| verify_drop(verify_drop(b, &s.corr_key), &s.u_key)),
                shape: wots_shape([&s.u_key, &s.corr_key]),
                terminal: "accept",
                window: s.windows.w_a,
                pays: Payout::Other("corrected"),
            },
            FrontLink {
                name: "ack",
                poster: Role::User,
                leaf: link_leaf(&ctx, "ack", false, |b| digits_equal(digits_equal(b, &s.corr_key, &s.a_corr_key), &s.u_key, &s.a_u_key)),
                shape: wots_shape([&s.u_key, &s.corr_key]).into_iter().chain(wots_shape([&s.a_u_key, &s.a_corr_key])).collect(),
                terminal: "ack_timeout",
                window: s.windows.w_h,
                pays: Payout::User,
            },
        ])
    }
    fn pay(&self, which: Payout, available: Amount) -> Result<Vec<TxOut>> {
        match which {
            Payout::User => self.pay_alice(available),
            Payout::Hub => Ok(self.core().pay_hub(available)),
            Payout::Other("corrected") => self.pay_corrected(available),
            Payout::Other(o) => anyhow::bail!("no payout {o}"),
        }
    }
    fn success(&self) -> Payout {
        Payout::Other("corrected")
    }
}

/// The digits of a word's one-time signature, as `wots_verify` leaves
/// them: whether bit `i` of the word is `want`, the digits dropped.
fn bit_check(b: Builder, i: u32, key: &WotsPublic, want: bool) -> Builder {
    let m = key.params.message_digits as usize;
    // digit k (0 the most significant nibble) sits at depth m - 1 - k
    let (nibble, j) = (7 - i / 4, i % 4);
    let mut b = b.wots_verify(key).push_int((m - 1 - nibble as usize) as i64).push_opcode(OP_PICK);
    // reduce the nibble mod 2^(j+1), then compare with 2^j
    for h in (j + 1..4).rev() {
        b = b.push_opcode(OP_DUP).push_int(1 << h).push_opcode(OP_GREATERTHANOREQUAL).push_opcode(OP_IF).push_int(1 << h).push_opcode(OP_SUB).push_opcode(OP_ENDIF);
    }
    b = b.push_int(1 << j).push_opcode(OP_GREATERTHANOREQUAL);
    if !want {
        b = b.push_opcode(OP_NOT);
    }
    b = b.push_opcode(OP_VERIFY);
    drop_n(b, m)
}

/// A payout of `b_corr`'s output for bit `i`, signed under the hub's
/// `key`: `alice_corr_bit_i` (her signature, the bit set), `hub_corr_bit_i`
/// (after `delta`, its signature, the bit clear: time for her to show an
/// equivocation), and `equiv_corr_i` (her signature and two different
/// signatures under `key`: the hub signed two values).
pub fn corr_bit_tree(ctx: &ContractCtx, i: u32, key: &WotsPublic) -> Result<TapTree> {
    assert_eq!(key.params.message_digits, 8, "a word's key signs 4 bytes");
    let alice = bit_check(Builder::new().checksigverify(&ctx.key(Role::User).payment), i, key, true);
    let delta = ctx.params.delta;
    let hub = bit_check(Builder::new().csv(delta).checksigverify(&ctx.key(Role::Hub).payment), i, key, false);
    let m = key.params.message_digits as usize;
    let mut e = Builder::new().push_int(i64::from(i)).push_opcode(OP_DROP).checksigverify(&ctx.key(Role::User).payment).wots_verify(key);
    for _ in 0..m {
        e = e.push_opcode(OP_TOALTSTACK);
    }
    e = e.wots_verify(key);
    for _ in 0..m {
        e = e.push_opcode(OP_FROMALTSTACK);
    }
    // count the digits that differ; at least one must
    e = e.push_int(0);
    for j in 0..m {
        e = e.push_int((m - j) as i64).push_opcode(OP_PICK).push_int((2 * m - j + 1) as i64).push_opcode(OP_PICK).push_opcode(OP_SUB).push_opcode(OP_0NOTEQUAL).push_opcode(OP_ADD);
    }
    e = e.push_opcode(OP_VERIFY);
    for _ in 0..m {
        e = e.push_opcode(OP_2DROP);
    }
    TapTree::new(vec![
        ok_leaf(&format!("alice_corr_bit_{i}"), alice, Timelock::NONE),
        ok_leaf(&format!("hub_corr_bit_{i}"), hub, Timelock::csv(delta)),
        ok_leaf(&format!("equiv_corr_{i}"), e, Timelock::NONE),
    ])
}
