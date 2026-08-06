// SPDX-License-Identifier: Apache-2.0
//! Closed-form geometry used while writing a build123d program.
//!
//! The encoder has no geometry kernel, so everything here is analytic. That is
//! a deliberate constraint rather than an omission: an exporter runs inside the
//! CLI, where evaluating a surface is not an option, and every value it writes
//! has to be derivable from the IR's own carriers.

use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

/// Distances below this are treated as coincident when comparing directions.
pub(crate) const DIRECTION_TOLERANCE: f64 = 1e-9;

/// A vector in model space, kept separate from the IR types so the arithmetic
/// stays free of identity and serialization concerns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub(crate) const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub(crate) fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub(crate) fn scale(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }

    pub(crate) fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub(crate) fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub(crate) fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Unit vector, or the input unchanged when it is too short to normalize.
    pub(crate) fn unit(self) -> Self {
        let length = self.length();
        if length > DIRECTION_TOLERANCE {
            self.scale(1.0 / length)
        } else {
            self
        }
    }

    /// Component of `self` perpendicular to `axis`, which must be a unit vector.
    pub(crate) fn reject(self, axis: Self) -> Self {
        self.sub(axis.scale(self.dot(axis)))
    }

    /// True when the two directions are parallel or antiparallel.
    pub(crate) fn is_parallel_to(self, other: Self) -> bool {
        self.unit().cross(other.unit()).length() < 1e-6
    }
}

impl From<Point3> for Vec3 {
    fn from(value: Point3) -> Self {
        Self::new(value.x, value.y, value.z)
    }
}

impl From<Vector3> for Vec3 {
    fn from(value: Vector3) -> Self {
        Self::new(value.x, value.y, value.z)
    }
}

/// The IR convention for a derived frame axis: the normalized projection of the
/// global axis with the smallest absolute dot product against the carrier axis.
pub(crate) fn derived_ref_direction(axis: Vec3) -> Vec3 {
    const AXES: [Vec3; 3] = [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ];
    let axis = axis.unit();
    let mut best = AXES[0];
    let mut best_alignment = f64::INFINITY;
    for candidate in AXES {
        let alignment = candidate.dot(axis).abs();
        if alignment < best_alignment {
            best_alignment = alignment;
            best = candidate;
        }
    }
    best.reject(axis).unit()
}

/// The axis, origin, and reference direction of an analytic surface of
/// revolution, or `None` for carriers that have no single axis.
pub(crate) fn surface_frame(geometry: &SurfaceGeometry) -> Option<(Vec3, Vec3, Vec3)> {
    let (origin, axis, reference) = match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => (origin, normal, u_axis),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            ..
        }
        | SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ..
        } => (origin, axis, ref_direction),
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            ..
        }
        | SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            ..
        } => (center, axis, ref_direction),
        _ => return None,
    };
    let axis = Vec3::from(*axis).unit();
    let mut reference = Vec3::from(*reference);
    if reference.length() < DIRECTION_TOLERANCE {
        reference = derived_ref_direction(axis);
    }
    Some((Vec3::from(*origin), axis, reference.reject(axis).unit()))
}

/// Signed distance from a point to an analytic surface.
///
/// Used to decide, before the kernel ever sees it, whether a reconstructed
/// boundary really lies on the carrier it is about to trim. OCC aborts the
/// process rather than reporting failure when handed a wire that is off its
/// surface, so this question has to be settled here.
pub(crate) fn distance_to_surface(geometry: &SurfaceGeometry, point: Vec3) -> Option<f64> {
    let (origin, axis, _) = surface_frame(geometry)?;
    let delta = point.sub(origin);
    let along = delta.dot(axis);
    let radial = delta.reject(axis).length();
    let distance = match geometry {
        SurfaceGeometry::Plane { .. } => along,
        SurfaceGeometry::Cylinder { radius, .. } => radial - *radius,
        SurfaceGeometry::Sphere { radius, .. } => delta.length() - *radius,
        SurfaceGeometry::Cone {
            radius, half_angle, ..
        } => (radial - (*radius + along * half_angle.tan())) * half_angle.cos(),
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } => (radial - *major_radius).hypot(along) - minor_radius.abs(),
        _ => return None,
    };
    Some(distance)
}

/// Formats a float so the emitted program is byte-identical across runs and
/// platforms, and reads as a number rather than as machine output.
pub(crate) fn number(value: f64) -> String {
    if !value.is_finite() {
        return format!("float({:?})", value.to_string());
    }
    let rounded = (value * 1e9).round() / 1e9;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    if rounded == rounded.trunc() && rounded.abs() < 1e15 {
        return format!("{}", rounded as i64);
    }
    let mut text = format!("{rounded}");
    if text.contains('e') {
        text = format!("{rounded:?}");
    }
    text
}

/// Formats a point or direction as a Python tuple.
pub(crate) fn tuple(value: Vec3) -> String {
    format!(
        "({}, {}, {})",
        number(value.x),
        number(value.y),
        number(value.z)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cylinder(radius: f64) -> SurfaceGeometry {
        SurfaceGeometry::Cylinder {
            origin: Point3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            axis: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            ref_direction: Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            radius,
        }
    }

    #[test]
    fn distance_to_a_cylinder_is_radial() {
        let surface = cylinder(3.75);
        let on_surface = distance_to_surface(&surface, Vec3::new(3.75, 12.0, 0.0))
            .expect("a cylinder has a closed-form distance");
        assert!(on_surface.abs() < 1e-12, "got {on_surface}");
        let outside = distance_to_surface(&surface, Vec3::new(4.75, 0.0, 0.0))
            .expect("a cylinder has a closed-form distance");
        assert!((outside - 1.0).abs() < 1e-12, "got {outside}");
    }

    #[test]
    fn derived_reference_is_perpendicular_to_the_axis() {
        let axis = Vec3::new(0.0, 1.0, 0.0);
        let reference = derived_ref_direction(axis);
        assert!(reference.dot(axis).abs() < 1e-12);
        assert!((reference.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn numbers_round_trip_without_exponent_noise() {
        assert_eq!(number(4.0), "4");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(3.35), "3.35");
        assert_eq!(number(0.750_000_000_000_000_1), "0.75");
    }
}
