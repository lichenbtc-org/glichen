//! The user's side of glichen's dispute front (papers/lichen-grants §6),
//! from her request to the hub's assertion (lichen plays the rest). She
//! requests, posting her claim `Q` and `b`; the hub corrects it
//! (`correct`: `b_corr` and the unlock `U`) or after `w_H` her timeout pays
//! her. If `U` opens her spend in flight, she completes it: her new note
//! settles, and the seed is public for the counterparty. Then she accepts a
//! correction to what she is owed (after `w_A`, `accept` pays it), or
//! countersigns it in `ack`, and the hub must prove.
//!
//! Also her one-time keys for `Q`, `b`, `b_corr` and `U`, from her
//! contract's seed, so that nothing of them need be kept.

use anyhow::{Context, Result};
use bitcoin::OutPoint;
use glichen_proto::{AliceKeys, Countersigned, Q_LEN, U_LEN};
use lichen_chain::Chain;
use lichen_proto::hp::Disputed;
use lichen_proto::setup::link_args;
use lichen_wallet::engine::{confirmed_at, wait_link, ContractSecrets, Engine, Next, Outcome};
use lichen_wallet::FrontEnd;
use lichen_wots::{Wots256Secret, WotsParams};

use crate::exit::ExitPlan;

fn ots(s: &ContractSecrets, label: &str, n: usize) -> Wots256Secret {
    Wots256Secret::from_entropy(WotsParams::for_bytes(n as u32), s.wots(label))
}

/// Her one-time key for `b`.
pub fn hp_b(s: &ContractSecrets) -> Wots256Secret {
    ots(s, "grants/b", 4)
}
/// Her one-time key for `Q`.
pub fn hp_q(s: &ContractSecrets) -> Wots256Secret {
    ots(s, "grants/q", Q_LEN)
}
/// Her one-time key for her countersignature on `b_corr`.
pub fn hp_a_corr(s: &ContractSecrets) -> Wots256Secret {
    ots(s, "grants/a_corr", 4)
}
/// Her one-time key for her countersignature on `U`.
pub fn hp_a_u(s: &ContractSecrets) -> Wots256Secret {
    ots(s, "grants/a_u", U_LEN)
}

/// Her four one-time keys, public.
pub fn alice_keys(s: &ContractSecrets) -> AliceKeys {
    AliceKeys { q_key: hp_q(s).public(), b_key: hp_b(s).public(), a_corr_key: hp_a_corr(s).public(), a_u_key: hp_a_u(s).public() }
}

/// `request`'s witness: her signatures on `b` and on `Q`.
pub fn request_wire(s: &ContractSecrets, b: u32, q: &[u8]) -> Result<Vec<Vec<u8>>> {
    Countersigned::request_wire(&hp_b(s).sign(&b.to_be_bytes())?, &hp_q(s).sign(q)?)
}

/// Her signature on `b` as the payout's bit outputs take it.
pub fn b_wire(s: &ContractSecrets, b: u32) -> Result<Vec<Vec<u8>>> {
    Ok(Countersigned::word_wire(&hp_b(s).sign(&b.to_be_bytes())?))
}

/// The front, from her request of `plan`'s claim.
pub fn run(e: &mut Engine<'_>, chain: &Chain, d: &Disputed, plan: &ExitPlan) -> Result<FrontEnd> {
    let record = &d.record;
    let w = glichen_proto::keys(record)?.hub.windows;
    let b = plan.b;
    let q = plan.posted()?;
    let wire = request_wire(e.signer.contract(), b, &q)?;
    let tx = d.finish("request", wire)?;
    e.say(format!("requesting {b} units ({} sats), {} ({} vB)", u64::from(b) * record.terms.unit.to_sat(), d.txid("request"), tx.vsize()));
    let at = e.link(chain, "request", tx)?;
    if e.opts.pause_after == Some(1) {
        return Ok(FrontEnd::Paused);
    }
    e.say(format!("the hub may correct the claim until block {}", at + u32::from(w.w_h)));
    let correct = match wait_link(chain, &d.txid("correct"), &OutPoint { txid: d.txid("request"), vout: 0 }, at + u32::from(w.w_h))? {
        Next::Link(tx) => tx,
        Next::Deadline => {
            e.say("the hub didn't correct the claim: request_timeout pays the user".into());
            let txid = e.terminal(chain, d, "request_timeout")?;
            return Ok(FrontEnd::Done(e.pay(Outcome::HubTimedOut(txid))?));
        }
        Next::Taken => return Ok(FrontEnd::Done(e.lose(d.txid("request"))?)),
    };
    let (b_corr, u) = glichen_proto::corrected(&correct)?;
    let corrected_at = confirmed_at(chain, &d.txid("correct"))?.context("correct")?;
    // the unlock: if it opens our spend in flight, that step counts, and
    // its new note is ours (and the counterparty's, from the same seed)
    let unit = record.terms.unit.to_sat();
    let expected = match (u != [0; U_LEN]).then(|| e.store.unlocked(&u)).transpose()?.flatten() {
        Some(v) => {
            e.say(format!("correct's unlock completes our payment in flight: our new note holds {v} sats"));
            b.min(u32::try_from(v / unit).unwrap_or(u32::MAX))
        }
        None => b,
    };
    e.say(format!("the hub corrected the claim to {b_corr} units (we reckon {expected})"));
    if b_corr >= expected && !e.opts.contest {
        // accept: after w_A, accept pays b_corr
        e.say(format!("accepting: accept pays {b_corr} units from block {}", corrected_at + u32::from(w.w_a)));
        let paid = e.await_terminal(chain, d, "correct", "accept", corrected_at + u32::from(w.w_a))?;
        return Ok(FrontEnd::Done(e.pay(Outcome::Corrected { success: paid, b_corr })?));
    }
    // contest: countersign the hub's values in ack, digit by digit
    let (hub_corr, hub_u) = Countersigned::read_correct(&link_args(&correct))?;
    let s = e.signer.contract();
    let wire = Countersigned::ack_wire(&hub_corr, &hp_a_corr(s).sign(&b_corr.to_be_bytes())?, &hub_u, &hp_a_u(s).sign(&u)?);
    let tx = d.finish("ack", wire)?;
    e.say(format!("contesting the correction: ack, {} ({} vB)", d.txid("ack"), tx.vsize()));
    let acked_at = e.link(chain, "ack", tx)?;
    if e.opts.pause_after == Some(2) {
        return Ok(FrontEnd::Paused);
    }
    e.say(format!("the hub may prove its correction until block {}", acked_at + u32::from(w.w_h)));
    Ok(FrontEnd::Asserting { at: acked_at, b_corr: Some(b_corr) })
}
