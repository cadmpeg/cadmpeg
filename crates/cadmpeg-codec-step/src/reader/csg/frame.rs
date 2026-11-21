// SPDX-License-Identifier: Apache-2.0
//! Right-handed frames and rigid motions for CSG primitives.

use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;

/// A right-handed orthonormal frame in model millimetres.
#[derive(Debug, Clone, Copy)]
pub(super) struct Frame {
    pub(super) origin: Point3,
    pub(super) x: Vector3,
    pub(super) y: Vector3,
    pub(super) z: Vector3,
}

impl Frame {
    /// The frame of a decoded placement: its location, its axis as z, and
    /// its reference direction as x.
    pub(super) fn from_placement(
        (origin, axis, reference): (FinitePoint3, UnitVector3, UnitVector3),
    ) -> Self {
        let z = *axis.as_raw();
        let x = *reference.as_raw();
        Self {
            origin: origin.get(),
            x,
            y: z.cross(x),
            z,
        }
    }

    /// The frame at `origin` whose z axis is `axis`, with an x axis chosen
    /// perpendicular to it.
    pub(super) fn about(origin: Point3, axis: Vector3) -> Option<Self> {
        let z = unit(axis)?;
        let x = perpendicular(z)?;
        Some(Self {
            origin,
            x,
            y: z.cross(x),
            z,
        })
    }

    /// The model point at frame coordinates `(x, y, z)`.
    pub(super) fn point(&self, x: f64, y: f64, z: f64) -> Point3 {
        self.origin
            .translated(self.x, x)
            .translated(self.y, y)
            .translated(self.z, z)
    }

    /// The frame moved by `distance` along its own z axis.
    pub(super) fn shifted(&self, distance: f64) -> Self {
        Self {
            origin: self.origin.translated(self.z, distance),
            ..*self
        }
    }
}

/// `vector` scaled to unit length, or `None` for a zero or non-finite one.
pub(super) fn unit(vector: Vector3) -> Option<Vector3> {
    vector.unit().filter(Vector3::is_finite)
}

/// A unit direction perpendicular to the unit vector `axis`.
pub(super) fn perpendicular(axis: Vector3) -> Option<Vector3> {
    let seed = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    unit(axis.cross(seed))
}

/// `point` turned by `angle` radians about the line through `center` along
/// the unit vector `axis`, counterclockwise about `axis`.
pub(super) fn rotate_about(point: Point3, center: Point3, axis: Vector3, angle: f64) -> Point3 {
    center.translated(turn(point.vector_from(center), axis, angle), 1.0)
}

/// `vector` turned by `angle` radians counterclockwise about the unit vector
/// `axis` (Rodrigues' rotation formula).
pub(super) fn turn(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let (sin, cos) = angle.sin_cos();
    vector.scale(cos) + axis.cross(vector).scale(sin) + axis.scale(axis.dot(vector) * (1.0 - cos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_turn_about_z_carries_x_to_y() {
        let turned = turn(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            std::f64::consts::FRAC_PI_2,
        );
        assert!(turned.x.abs() < 1.0e-15);
        assert!((turned.y - 1.0).abs() < 1.0e-15);
    }

    #[test]
    fn a_frame_about_an_axis_is_right_handed() {
        let frame =
            Frame::about(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 3.0, 4.0)).expect("a frame");
        let z = frame.x.cross(frame.y);
        assert!((z.dot(frame.z) - 1.0).abs() < 1.0e-12);
        assert!(frame.x.dot(frame.z).abs() < 1.0e-12);
    }
}
