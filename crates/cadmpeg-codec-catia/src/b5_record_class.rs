// SPDX-License-Identifier: Apache-2.0
//! CATIA `b5 03` record-class codes.

/// `b5 03 5f` face node.
pub const FACE: u8 = 0x5f;
/// `b5 03 62` loop node.
pub const LOOP: u8 = 0x62;
/// `b5 03 21` parametric-curve node.
pub const PCURVE: u8 = 0x21;
/// `b5 03 18` straight-line pcurve node.
pub const LINE_PCURVE: u8 = 0x18;
/// `b5 03 5e` edge node.
pub const EDGE: u8 = 0x5e;
/// `b5 03 27` planar surface.
pub const SURFACE_PLANE: u8 = 0x27;
/// `b5 03 28` cylindrical surface.
pub const SURFACE_CYLINDER: u8 = 0x28;
/// `b5 03 2d` surface of revolution.
pub const SURFACE_REVOLUTION: u8 = 0x2d;
/// `b5 03 34` NURBS surface.
pub const SURFACE_NURBS: u8 = 0x34;
/// `b5 03 0e` straight-line profile.
pub const PROFILE_LINE: u8 = 0x0e;
/// `b5 03 0f` circular-arc profile.
pub const PROFILE_ARC: u8 = 0x0f;

/// Whether the class code names a surface accepted by the topology binder.
#[must_use]
pub fn is_surface(code: u8) -> bool {
    matches!(
        code,
        SURFACE_PLANE | SURFACE_CYLINDER | SURFACE_REVOLUTION | SURFACE_NURBS
    )
}
