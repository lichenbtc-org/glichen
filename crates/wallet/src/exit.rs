//! The wallet's exit: a claim of its account's note posted through the
//! contract's dispute (lichen's engine plays it, [`crate::front`] its
//! front), for when the hub won't fold or has gone. The claim is the note
//! with the hub's signature (`Q`), so it needs nothing from the hub. After
//! a cooperative withdrawal the hub didn't fold, the exit posts that
//! withdrawal's claim.

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{GrantsWallet, Payments};

/// A claim made and kept: `b` units, the claim's bytes (`Q`, hex: what a
/// cooperative withdrawal sends), and what `request` posts (hex: `Q`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitPlan {
    pub b: u32,
    pub claim: String,
    pub posted: String,
}

impl ExitPlan {
    pub fn claim(&self) -> Result<glichen_notes::core::Claim> {
        glichen_notes::core::Claim::from_bytes(&hex::decode(&self.claim)?).context("not a claim's bytes")
    }
    pub fn posted(&self) -> Result<Vec<u8>> {
        Ok(hex::decode(&self.posted)?)
    }
}

/// The claim an exit of contract `cid` posts: all of its account's live
/// note the cap allows, in whole units (made now: the account is synced
/// first, if the hub answers).
pub fn exit_claim(w: &mut GrantsWallet, _chain: &lichen_chain::Chain, cid: u32) -> Result<ExitPlan> {
    let k = w.contracts.iter().position(|h| h.record.cid() == cid).with_context(|| format!("no contract {cid}"))?;
    let record = w.contracts[k].record.clone();
    let (t, owner) = (record.terms, record.l2_address);
    let v0 = glichen_proto::keys(&record)?.v0;
    let unit = t.unit.to_sat();
    // a hub that won't answer mustn't hold off the claim
    if let Err(e) = w.sync() {
        w.say(format!("claiming without a sync: {e:#}"));
    }
    let live = w.account(&owner).live;
    let held = if live.note.k == 0 { v0 } else { live.note.v };
    let units = (held.min(t.v_max().to_sat()) / unit) as u32;
    ensure!(units > 0, "nothing to claim: {held} sats in the contract's account, a unit of {unit}");
    w.claim_for(cid, units)
}

/// The unlock `correct` posted on contract `cid`'s dispute, if any.
pub fn unlock_of(w: &GrantsWallet, chain: &lichen_chain::Chain, statement: &lichen_proto::Statement, cid: u32) -> Result<Option<glichen_notes::Seed>> {
    let h = w.contracts.iter().find(|h| h.record.cid() == cid).with_context(|| format!("no contract {cid}"))?;
    let d = lichen_proto::hp::Disputed::new(&h.record, statement, &w.l2)?;
    let Some(correct) = chain.get_tx(&d.txid("correct")) else { return Ok(None) };
    let (_, u) = glichen_proto::corrected(&correct)?;
    Ok((u != [0; glichen_notes::U_LEN]).then_some(u))
}

/// Complete our payment pending with a seed `z` (from a dispute, or the
/// counterparty).
pub fn complete_with(w: &mut GrantsWallet, z: &glichen_notes::Seed) -> Result<crate::notes::Step> {
    let me = w.address();
    ensure!(w.opens(&me, z), "the seed doesn't open our payment pending");
    w.complete(&me, z)
}

/// [`exit_claim`], kept in the contract for its exit (as an exit or a
/// withdrawal the hub doesn't fold would keep it).
pub fn keep_exit_claim(w: &mut GrantsWallet, chain: &lichen_chain::Chain, cid: u32) -> Result<ExitPlan> {
    let p = exit_claim(w, chain, cid)?;
    let k = w.contracts.iter().position(|h| h.record.cid() == cid).with_context(|| format!("no contract {cid}"))?;
    w.contracts[k].exit = Some(serde_json::to_value(&p)?);
    w.save()?;
    Ok(p)
}
