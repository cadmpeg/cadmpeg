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
//! sanctioned exception is `crate::math`, reserved for the future shared
//! cursor; the model must not leak downward into the byte platform.

pub mod be;
pub mod compression;
pub mod hash;
pub mod le;
pub(crate) mod read;
