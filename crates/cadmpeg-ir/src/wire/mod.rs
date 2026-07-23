// SPDX-License-Identifier: Apache-2.0
//! The byte platform shared by the format codecs.
//!
//! The wire layer holds the primitives that turn untrusted input bytes into
//! values: checked big- and little-endian primitive readers, bounded
//! decompression, and content hashing. Every codec crate decodes container
//! bytes through these helpers, so they are written to fail closed on
//! truncated, malformed, or adversarial input rather than panic or over-read.
//!
//! Layering rule: `wire/*` sits below the IR model. Its modules may depend
//! only on the standard library and their external compression and hash
//! dependencies, never on the IR model modules (`crate::geometry`,
//! `crate::topology`, `crate::document`, and their peers). The single
//! sanctioned exception is `crate::math`, which [`cursor`] reads into
//! [`Point3`](crate::math::Point3)/[`Vector3`](crate::math::Vector3)
//! compounds; the model must not otherwise leak downward into the byte
//! platform.
//!
//! Adoption pattern: [`cursor::Cursor`] is total and self-poisoning. Decode a
//! frame with it, then call [`cursor::Cursor::finish`] **before** interpreting
//! any decoded value. This is the single terminal check: a poisoned cursor
//! returns zero values indistinguishable from real data, so a truncated
//! length field reads back as `0` and a `for _ in 0..count` loop would treat
//! "zero items" as success while dropping the record. Checking `finish` before
//! interpreting the count closes that poison-masking hazard.

pub mod be;
pub mod compression;
pub mod cursor;
pub mod hash;
pub mod le;
pub(crate) mod read;
