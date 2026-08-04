// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write Autodesk Shape Manager record streams.
//!
//! [`sab`] frames binary ASM records. [`asm_header`] parses the binary stream
//! header and locates the solved and construction-history partitions.

pub mod asm_header;
pub mod sab;
