// SPDX-License-Identifier: Apache-2.0
//! Decode cached B-spline blocks from spline and procedural SAB records.
//!
//! A `0x0d`-tagged `nubs` marker introduces a non-rational block; `nurbs`
//! introduces a rational block. Surface blocks contain two degrees, closure and
//! singularity enums, U and V knot tables, and a control grid. Curve blocks
//! contain one degree, a closure enum, one knot table, and a control polygon.
//! The token after the first degree distinguishes the two forms.
//!
//! Spline surfaces and procedural curves store solved geometry in these caches.
//! The public decode functions accept one record's bytes, with variants that
//! also follow references through the active slice's subtype table.
//!
//! Endpoint knot multiplicities are stored as `degree` rather than
//! `degree + 1`; the clamped knot vector is recovered by adding one at each
//! end. Control-point x/y/z are model-space lengths converted from centimetres
//! to millimetres; knots and rational weights are not scaled. Surface control
//! grids are stored v-major (v outer, u inner) and are transposed to the IR's
//! u-major order.
//!
//! Integer-family payloads (`0x04` int, `0x0c` ref, `0x15` enum) are 4 bytes in
//! `BinaryFile4` streams and 8 in `BinaryFile8` ([spec §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#3-asm-binary-header)); doubles are always
//! 8. Record bytes omit the stream width, so each decoder tests both layouts
//! and validates tags, degrees, counts, and block extents.

pub mod blend;
pub mod core;
pub mod pcurve;
pub mod proc_curve;
pub mod proc_surface;
pub mod reader;
pub mod subtypes;
