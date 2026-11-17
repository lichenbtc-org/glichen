//! glichen's part of a lichen contract: the statement's terms (the issuer,
//! the owner, the mint's value), the parties' keys for the dispute's front
//! ([`front`]), and how both sides read the front's links. The rest of the
//! contract is lichen's (`lichen_proto`).
//!
//! The statement (`glichen_statement::accounts::check`) is fixed at the
//! opening by its journal `J = c ‖ D`, which binds the issuer, the
//! contract's key, the unit, the mint's value and the user's four one-time
//! keys.

pub mod front;

use anyhow::{ensure, Context, Result};
use glichen_statement::accounts;
use lichen_proto::hp::Disputed;
use lichen_proto::setup::link_args;
use lichen_proto::{Hash, Net, Record};
use lichen_wots::{Wots256Public, WotsParams, WotsPublic};
use serde::{Deserialize, Serialize};

pub use front::{Countersigned, Windows, Q_LEN, U_LEN};

/// glichen's version: what its hub and wallet report (`grants-X.Y.Z`,
/// X.Y the design's, papers/lichen-grants).
pub const VERSION: &str = "grants-0.5.4";

/// The statement's terms in a setup (`HpSetup::l2`): the contract's
/// owner (its L2 key), the mint's value, and the issuer, whose key signs
/// every note. The user asks for the first two; the hub adds the issuer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupTerms {
    #[serde(with = "lichen_babe::hexbytes::fixed")]
    pub owner: Hash,
    pub v0: u64,
    #[serde(default, with = "lichen_babe::hexbytes::opt_fixed")]
    pub issuer: Option<Hash>,
}

/// The user's one-time keys: `K_Q`, `K_b` (in `request`), `A_corr`, `A_U`
/// (in `ack`). SHA-256 chains: the statement verifies them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliceKeys {
    #[serde(with = "lichen_babe::hexbytes::wots")]
    pub q_key: Wots256Public,
    #[serde(with = "lichen_babe::hexbytes::wots")]
    pub b_key: Wots256Public,
    #[serde(with = "lichen_babe::hexbytes::wots")]
    pub a_corr_key: Wots256Public,
    #[serde(with = "lichen_babe::hexbytes::wots")]
    pub a_u_key: Wots256Public,
}

impl AliceKeys {
    /// Of the parameters the front takes.
    pub fn check(&self) -> Result<()> {
        let w = |n: usize| WotsParams::for_bytes(n as u32);
        ensure!(self.q_key.params == w(Q_LEN) && self.q_key.digits.len() == w(Q_LEN).total_digits() as usize, "K_Q of other parameters");
        for (k, n) in [(&self.b_key, 4), (&self.a_corr_key, 4), (&self.a_u_key, U_LEN)] {
            ensure!(k.params == w(n) && k.digits.len() == w(n).total_digits() as usize, "a one-time key of other parameters");
        }
        Ok(())
    }
    /// The journal's keys, in its order.
    pub fn chains(&self) -> [Vec<Hash>; 4] {
        [self.q_key.digits.clone(), self.b_key.digits.clone(), self.a_corr_key.digits.clone(), self.a_u_key.digits.clone()]
    }
}

/// The hub's keys in its offer (`HpOffer::l2`): its one-time keys for
/// `b_corr` and `U` (HASH160: only script reads them), and the front's
/// windows.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HubKeys {
    #[serde(with = "lichen_babe::hexbytes::wots")]
    pub corr_key: WotsPublic,
    #[serde(with = "lichen_babe::hexbytes::wots")]
    pub u_key: WotsPublic,
    pub windows: Windows,
}

impl HubKeys {
    /// Of the parameters the front takes.
    pub fn check(&self) -> Result<()> {
        ensure!(self.corr_key.params == WotsParams::for_bytes(4), "K_corr of other parameters");
        ensure!(self.u_key.params == WotsParams::for_bytes(U_LEN as u32), "K_U of other parameters");
        Ok(())
    }
}

/// A contract's keys (`HpKeys::l2`): both parties', the issuer and the
/// mint's value.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keys {
    pub alice: AliceKeys,
    pub hub: HubKeys,
    #[serde(with = "lichen_babe::hexbytes::fixed")]
    pub issuer: Hash,
    pub v0: u64,
}

/// The front's windows on a network: `w_H` for the hub's moves, `w_A` for
/// her `ack`.
pub fn windows(net: Net) -> Windows {
    let (w_h, w_a) = match net {
        // blocks every 150 ms in the tests
        Net::Regtest => (40, 30),
        // 30 s blocks: 2 h, a day
        Net::Mutinynet => (240, 2880),
        // 10 min blocks: 6 h, a day
        Net::Signet => (36, 144),
        Net::Testnet4 => (72, 288),
    };
    Windows { w_h, w_a }
}

/// The contract as the statement's journal binds it.
pub fn contract(cid: u32, issuer: &Hash, owner: &Hash, unit: u64, v0: u64, keys: &AliceKeys) -> accounts::Contract {
    accounts::Contract { c: cid, issuer: *issuer, owner: *owner, unit, v0, keys: keys.chains() }
}

/// A record's keys, typed.
pub fn keys(record: &Record) -> Result<Keys> {
    record.dispute.l2()
}

/// The contract as the journal binds it, from its record.
pub fn record_contract(record: &Record) -> Result<accounts::Contract> {
    let k = keys(record)?;
    Ok(contract(record.cid(), &k.issuer, &record.l2_address, record.terms.unit.to_sat(), k.v0, &k.alice))
}

/// The journal of a setup's terms and the user's keys.
pub fn setup_journal(cid: u32, unit: u64, terms: &SetupTerms, keys: &AliceKeys) -> Result<Vec<u8>> {
    let issuer = terms.issuer.context("the setup names no issuer")?;
    Ok(contract(cid, &issuer, &terms.owner, unit, terms.v0, keys).journal().to_vec())
}

/// The front of a contract's graph, from its record ([`lichen_proto::Layer::front`]).
pub fn front(record: &Record) -> Result<Box<dyn lichen_babe::graph::Front>> {
    let k = keys(record)?;
    Ok(Box::new(Countersigned::new(front::Spec {
        terms: record.terms,
        params: lichen_proto::params(record.network),
        pubs: record.pubs.clone(),
        b_key: k.alice.b_key.clone(),
        q_key: k.alice.q_key.clone(),
        a_corr_key: k.alice.a_corr_key.clone(),
        a_u_key: k.alice.a_u_key.clone(),
        corr_key: k.hub.corr_key.clone(),
        u_key: k.hub.u_key.clone(),
        windows: k.hub.windows,
    })?))
}

/// The statement's journal for a contract ([`lichen_proto::Layer::journal`]).
pub fn journal(record: &Record) -> Result<Vec<u8>> {
    Ok(record_contract(record)?.journal().to_vec())
}

/// A one-time signature as the statement takes it.
pub fn wots(s: &lichen_wots::Wots256Sig) -> accounts::Wots {
    accounts::Wots { hashes: s.hashes.clone(), digits: s.digits.clone() }
}

/// `request`'s: `b`, the posted claim's bytes, and her signatures on
/// them.
pub fn requested(request: &bitcoin::Transaction) -> Result<(u32, Vec<u8>, accounts::Wots, accounts::Wots)> {
    let (b, q) = Countersigned::read_request(&link_args(request))?;
    let bv = u32::from_be_bytes(b.message().try_into().map_err(|_| anyhow::anyhow!("b's signature signs 4 bytes"))?);
    Ok((bv, q.message(), wots(&q), wots(&b)))
}

/// `correct`'s: `b_corr` and `U`, as the hub signed them.
pub fn corrected(correct: &bitcoin::Transaction) -> Result<(u32, accounts::Seed)> {
    let (c, u) = Countersigned::read_correct(&link_args(correct))?;
    let b_corr = u32::from_be_bytes(c.message().try_into().map_err(|_| anyhow::anyhow!("b_corr's signature signs 4 bytes"))?);
    Ok((b_corr, u.message().try_into().map_err(|_| anyhow::anyhow!("U's signature signs 16 bytes"))?))
}

/// `ack`'s: her countersignatures on `b_corr` and `U`.
pub fn acked(ack: &bitcoin::Transaction) -> Result<(accounts::Wots, accounts::Wots)> {
    let (c, u) = Countersigned::read_ack(&link_args(ack))?;
    Ok((wots(&c), wots(&u)))
}

/// A dispute's link `name`, from the chain.
pub fn link(chain: &lichen_chain::Chain, d: &Disputed, name: &str) -> Result<bitcoin::Transaction> {
    chain.get_tx(&d.txid(name)).with_context(|| format!("{name}, on chain"))
}
