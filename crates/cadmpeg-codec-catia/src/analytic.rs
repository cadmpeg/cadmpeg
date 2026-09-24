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

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::scalar::{
    Angle, FiniteReal, NonNegativeLength, NonZeroLength, PositiveLength, PositiveReal,
};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

use crate::wire::cursor::Cursor;

/// Keep the surface frame, with the reference reversed when the circle
/// radius is negative: the circle then starts on the opposite side of the axis.
pub(crate) fn signed_reference_frame(
    mut frame: OrthonormalFrame3,
    circle_radius: f64,
) -> OrthonormalFrame3 {
    if circle_radius.is_sign_negative() {
        frame.reverse_reference();
    }
    frame
}

/// Validate an active angular interval and its centered full-turn chart domain.
pub(crate) fn periodic_angular_range_is_valid(range: [f64; 2], domain: [f64; 2]) -> bool {
    const TOLERANCE: f64 = 1.0e-12;
    let range_midpoint = (range[0] + range[1]) * 0.5;
    let domain_midpoint = (domain[0] + domain[1]) * 0.5;
    range[0] < range[1]
        && domain[0] < domain[1]
        && range[0] >= domain[0] - TOLERANCE
        && range[1] <= domain[1] + TOLERANCE
        && range[1] - range[0] <= std::f64::consts::TAU + TOLERANCE
        && ((domain[1] - domain[0]) - std::f64::consts::TAU).abs() <= TOLERANCE
        && (range_midpoint - domain_midpoint).abs() <= TOLERANCE
}

/// Validate the active azimuth and latitude intervals of a sphere chart.
pub(crate) fn sphere_angular_ranges_are_valid(
    azimuth_range: [f64; 2],
    latitude_range: [f64; 2],
) -> bool {
    azimuth_range[0] < azimuth_range[1]
        && azimuth_range[1] - azimuth_range[0] <= std::f64::consts::TAU
        && -std::f64::consts::FRAC_PI_2 <= latitude_range[0]
        && latitude_range[0] < latitude_range[1]
        && latitude_range[1] <= std::f64::consts::FRAC_PI_2
}

/// Cylinder from an already-decoded `origin` plus a direction frame.
///
/// Reads two direction rows `u`, `v` then the radius from `c`, which must be
/// positioned at the first row (the row block is contiguous in every family,
/// but its offset relative to `origin` is not, so the caller reads `origin`
/// and positions `c` itself). The axis is `u × v` normalised and the
/// zero-azimuth reference is `u` normalised; both fail on a degenerate frame.
///
/// The returned radius is the carrier's admitted positive radius.
pub(crate) fn cylinder_uvr(
    c: &mut Cursor,
    origin: FinitePoint3,
) -> Option<(SurfaceGeometry, PositiveLength)> {
    let u = c.vector3()?.get();
    let v = c.vector3()?.get();
    let radius = PositiveLength::new(c.f64()?.get())?;
    let axis = UnitVector3::normalized(u.cross(v))?;
    let ref_direction = UnitVector3::normalized(u)?;
    Some((
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::new(
                origin,
                OrthonormalFrame3::from_units(axis, ref_direction)?,
                radius,
            ),
        )),
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
/// Returns the built [`SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone)`] together with the radius and
/// derived half-angle so callers can apply their own guard. Both are finite:
/// the radius is read finite, and the cone admits only a finite half-angle.
pub(crate) fn cone_ozra(c: &mut Cursor) -> Option<(SurfaceGeometry, FiniteReal, f64)> {
    let origin = c.point3()?;
    let ref_direction = c.unit3()?;
    c.skip(24)?;
    let axis = c.unit3()?;
    let stored_angle = c.f64()?;
    let radius = c.f64()?;
    let half_angle = std::f64::consts::FRAC_PI_2 - stored_angle.get();
    Some((
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::new(
                origin,
                OrthonormalFrame3::from_units(axis, ref_direction)?,
                NonNegativeLength::new(radius.get())?,
                PositiveReal::ONE,
                Angle::new(half_angle)?,
            ),
        )),
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
/// Returns the built [`SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus)`] together with the
/// carrier's admitted radii, a positive major radius and a nonzero minor
/// radius, so callers can apply their own guard.
pub(crate) fn torus_ozrr(
    c: &mut Cursor,
) -> Option<(SurfaceGeometry, PositiveLength, NonZeroLength)> {
    let center = c.point3()?;
    let ref_direction = c.unit3()?;
    c.skip(24)?;
    let axis = c.unit3()?;
    let major_radius = PositiveLength::new(c.f64()?.get())?;
    let minor_radius = NonZeroLength::new(c.f64()?.get())?;
    Some((
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::analytic::TorusSurface::new(
                center,
                OrthonormalFrame3::from_units(axis, ref_direction)?,
                major_radius,
                minor_radius,
            ),
        )),
        major_radius,
        minor_radius,
    ))
}
