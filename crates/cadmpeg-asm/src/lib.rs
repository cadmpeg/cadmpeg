// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write Autodesk Shape Manager record streams.
//!
//! [`sab`] frames binary ASM records and [`sat`] frames text-encoded
//! records into the same token model. [`asm_header`] parses the binary stream
//! header and locates the solved and construction-history partitions.
//! [`nurbs`] decodes cached B-spline blocks and procedural curve and surface
//! definitions from spline SAB records. [`brep`] decodes a framed record slice
//! into the neutral B-rep graph. [`ids`] carries the format component
//! of emitted entity IDs.

pub mod asm_header;
pub mod brep;
pub mod ids;
pub mod nurbs;
pub mod sab;
pub mod sat;
