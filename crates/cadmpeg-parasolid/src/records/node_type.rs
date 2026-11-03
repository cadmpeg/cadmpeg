// SPDX-License-Identifier: Apache-2.0
//! Node types of the topology, point, B-spline, and procedural surface nodes.

/// A body, `BODY`.
pub const BODY: u16 = 12;
/// A shell, `SHELL`.
pub const SHELL: u16 = 13;
/// A face, `FACE`.
pub const FACE: u16 = 14;
/// A loop of fins, `LOOP`.
pub const LOOP: u16 = 15;
/// An edge, `EDGE`.
pub const EDGE: u16 = 16;
/// A fin, one use of an edge by a loop, `FIN`.
pub const FIN: u16 = 17;
/// A vertex, `VERTEX`.
pub const VERTEX: u16 = 18;
/// A region, `REGION`.
pub const REGION: u16 = 19;
/// A point, `POINT`.
pub const POINT: u16 = 29;
/// The control-point values of a B-spline, `BSPLINE_VERTICES`.
pub const BSPLINE_VERTICES: u16 = 45;
/// A rolling-ball blend surface, `BLEND_SURF`.
pub const BLEND_SURF: u16 = 56;
/// One boundary of a blend surface, `BLEND_BOUND`.
pub const BLEND_BOUND: u16 = 59;
/// An offset surface, `OFFSET_SURF`.
pub const OFFSET_SURF: u16 = 60;
/// A swept surface, `SWEPT_SURF`.
pub const SWEPT_SURF: u16 = 67;
/// A spun surface, `SPUN_SURF`.
pub const SPUN_SURF: u16 = 68;
/// A B-spline surface, `B_SURFACE`.
pub const B_SURFACE: u16 = 124;
/// The NURBS definition of a B-spline surface, `NURBS_SURF`.
pub const NURBS_SURF: u16 = 126;
/// Knot multiplicities, `KNOT_MULT`.
pub const KNOT_MULT: u16 = 127;
/// Distinct knot values, `KNOT_SET`.
pub const KNOT_SET: u16 = 128;
/// A B-spline curve, `B_CURVE`.
pub const B_CURVE: u16 = 134;
/// The NURBS definition of a B-spline curve, `NURBS_CURVE`.
pub const NURBS_CURVE: u16 = 136;
/// A curve given by a parameter-space B-curve on a surface, `SP_CURVE`.
pub const SP_CURVE: u16 = 137;
