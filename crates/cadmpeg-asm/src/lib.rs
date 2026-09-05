// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write Autodesk Shape Manager record streams.
//!
//! [`sab`] frames binary ASM and admitted ACIS records, and [`sat`] frames
//! text-encoded records into the same token model. [`asm_header`] and
//! [`acis_header`] parse their binary headers and locate the solved and
//! construction-history partitions. [`kernel_header`] owns their shared
//! metadata.
//! [`dialect`] owns the `acis:` kernel-layer rows and the save-format band the
//! Spatial ACIS record decoders are verified against.
//! [`nurbs`] decodes cached B-spline blocks and procedural curve and surface
//! definitions from spline SAB records. [`brep`] decodes a framed record slice
//! into the neutral B-rep graph. [`ids`] carries the format component
//! of emitted entity IDs.

pub mod acis_header;
pub mod asm_header;
pub mod brep;
pub mod dialect;
pub mod edit;
pub mod ids;
pub mod kernel_header;
/// Byte-offset constants generated from `docs/layouts/asm.toml`.
pub(crate) mod layout;
pub mod nurbs;
pub mod sab;
pub mod sat;

pub mod stream_error;
