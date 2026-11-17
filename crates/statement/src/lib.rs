//! glichen's statement: the hub's, that a claim the user signed is owed at
//! most the `b_corr` she countersigned ([`accounts`], papers/lichen-grants
//! §6), with the hashes and encodings the grants L2's notes, halves, spends
//! and grants are made of. One implementation, for the hub and the wallet
//! (`glichen_notes`) and the guest (`r0/hp-guest`).
//!
//! Integers are big-endian in the encodings.

#![no_std]

extern crate alloc;

pub mod accounts;
mod codec;

/// A 32-byte hash, key or seed.
pub type Hash = [u8; 32];

/// Why the statement rejects its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// A signature that doesn't verify.
    BadSignature,
    /// The input isn't the encoding of one.
    Encoding,
    /// The claim is owed more than the correction: the statement holds
    /// for the user, not the hub.
    Holds,
}
