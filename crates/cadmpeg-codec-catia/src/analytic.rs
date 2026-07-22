//! Canonical frame readers for analytic surface carriers.
//!
//! The little-endian analytic surfaces (`e5` and `zero_entity`) stamp out the
//! same cylinder/cone/torus field sequences at different base offsets. Each
//! family positions a [`Cursor`] over its record payload and calls one of the
//! readers here; the reader decodes the canonical field sequence, builds the
//! [`SurfaceGeometry`] variant, and returns the magnitude-bearing scalars so
//! the caller can apply its own validation guard (the guards differ per
//! family and must stay at the call site).
//!
//! These readers are little-endian. The big-endian inline analytic block
//! decoded by `crate::families::standard::records::decode_curved` has a different layout, endianness,
//! and axis reconstruction, and keeps its own reader.

use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::Point3;

use crate::wire::cursor::Cursor;

/// Cylinder from an already-decoded `origin` plus a direction frame.
///
/// Reads two direction rows `u`, `v` then the radius from `c`, which must be
/// positioned at the first row (the row block is contiguous in every family,
/// but its offset relative to `origin` is not, so the caller reads `origin`
/// and positions `c` itself). The axis is `u × v` normalised and the
/// zero-azimuth reference is `u` normalised; both fail on a degenerate frame.
///
/// The returned radius is finite but otherwise unvalidated: callers apply
/// their own magnitude guard.
pub(crate) fn cylinder_uvr(c: &mut Cursor, origin: Point3) -> Option<(SurfaceGeometry, f64)> {
    let u = c.vector3()?;
    let v = c.vector3()?;
    let radius = c.f64()?;
    let axis = u.cross(v).unit()?;
    let ref_direction = u.unit()?;
    Some((
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        },
        radius,
    ))
}

/// Cone from origin, reference direction, axis, a stored polar angle, and a
/// radius, in that canonical order.
///
/// `c` is positioned at the origin. The reference direction immediately
/// follows the origin; a 24-byte block separates it from the axis. The stored
/// angle is the complement of the half-angle: `half_angle = π/2 − stored`.
///
/// Returns the built [`SurfaceGeometry::Cone`] together with the radius and
/// derived half-angle so callers can apply their own guard. Both are finite.
pub(crate) fn cone_ozra(c: &mut Cursor) -> Option<(SurfaceGeometry, f64, f64)> {
    let origin = c.point3()?;
    let ref_direction = c.unit3()?;
    c.skip(24)?;
    let axis = c.unit3()?;
    let stored_angle = c.f64()?;
    let radius = c.f64()?;
    let half_angle = std::f64::consts::FRAC_PI_2 - stored_angle;
    Some((
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio: 1.0,
            half_angle,
        },
        radius,
        half_angle,
    ))
}

/// Torus from center, reference direction, axis, major radius, and minor
/// radius, in that canonical order.
///
/// `c` is positioned at the center. The field layout matches [`cone_ozra`]:
/// the reference direction follows the center, a 24-byte block separates it
/// from the axis, and the two radii follow the axis.
///
/// Returns the built [`SurfaceGeometry::Torus`] together with the major and
/// minor radii so callers can apply their own guard. Both are finite.
pub(crate) fn torus_ozrr(c: &mut Cursor) -> Option<(SurfaceGeometry, f64, f64)> {
    let center = c.point3()?;
    let ref_direction = c.unit3()?;
    c.skip(24)?;
    let axis = c.unit3()?;
    let major_radius = c.f64()?;
    let minor_radius = c.f64()?;
    Some((
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        },
        major_radius,
        minor_radius,
    ))
}
