// SPDX-License-Identifier: Apache-2.0
//! Composed placements and their classification.

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

use crate::failure::{Measure, MeasureFailure};
use crate::reason::UnavailableReason;
use crate::report::PlacementClass;
use crate::tolerance::SIMILARITY_RELATIVE_TOLERANCE;

/// A composed affine placement and its classification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
    transform: Transform,
    inverse: Option<Transform>,
    pub(crate) class: PlacementClass,
    /// The uniform scale `s`, `sqrt(trace(LᵀL)/3)`.
    pub(crate) scale: f64,
    /// The relative Gram deviation `δ` from a similarity.
    pub(crate) deviation: f64,
    /// Whether the linear part reverses orientation.
    pub(crate) reflection: bool,
}

const IDENTITY_ROWS: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

impl Placement {
    pub(crate) fn identity() -> Self {
        Self {
            transform: Transform::identity(),
            inverse: Some(Transform::identity()),
            class: PlacementClass::Identity,
            scale: 1.0,
            deviation: 0.0,
            reflection: false,
        }
    }

    /// Classify a transform.
    pub(crate) fn of(transform: Transform) -> Measure<Self> {
        let rows = transform.affine_rows();
        if rows
            .iter()
            .flatten()
            .zip(IDENTITY_ROWS.iter().flatten())
            .all(|(value, identity)| value.to_bits() == identity.to_bits())
        {
            return Ok(Self::identity());
        }
        let column = |index: usize| Vector3::new(rows[0][index], rows[1][index], rows[2][index]);
        let columns = [column(0), column(1), column(2)];
        let gram: [[f64; 3]; 3] =
            std::array::from_fn(|i| std::array::from_fn(|j| columns[i].dot(columns[j])));
        let squared = (gram[0][0] + gram[1][1] + gram[2][2]) / 3.0;
        let scale = squared.sqrt();
        if !scale.is_finite() || scale <= 0.0 {
            return Err(non_finite());
        }
        let mut deviation: f64 = 0.0;
        for (i, row) in gram.iter().enumerate() {
            for (j, value) in row.iter().enumerate() {
                let expected = if i == j { squared } else { 0.0 };
                deviation = deviation.max((value - expected).abs() / squared);
            }
        }
        if !deviation.is_finite() {
            return Err(non_finite());
        }
        let class = if deviation > SIMILARITY_RELATIVE_TOLERANCE {
            PlacementClass::Affine
        } else if (scale - 1.0).abs() <= SIMILARITY_RELATIVE_TOLERANCE {
            PlacementClass::Rigid
        } else {
            PlacementClass::Similarity
        };
        let reflection = transform.orientation().is_some_and(|sign| sign < 0.0);
        Ok(Self {
            transform,
            inverse: transform.try_inverse_affine().ok(),
            class,
            scale,
            deviation,
            reflection,
        })
    }

    /// The placement `outer ∘ inner`.
    pub(crate) fn then(self, outer: Self) -> Measure<Self> {
        if self.class == PlacementClass::Identity {
            return Ok(outer);
        }
        if outer.class == PlacementClass::Identity {
            return Ok(self);
        }
        let composed = outer.transform.compose(self.transform).map_err(|error| {
            MeasureFailure::unavailable(
                UnavailableReason::PlacementNonFinite,
                None,
                format!("placement composition failed: {error:?}"),
            )
        })?;
        Self::of(composed)
    }

    /// Whether radii and angles are invariant up to the uniform scale.
    pub(crate) fn is_similarity(self) -> bool {
        self.class != PlacementClass::Affine
    }

    /// Map a point.
    pub(crate) fn point(self, point: Point3) -> Point3 {
        let rows = self.transform.affine_rows();
        let [x, y, z] = std::array::from_fn(|row| {
            let r = rows[row];
            r[0] * point.x + r[1] * point.y + r[2] * point.z + r[3]
        });
        Point3::new(x, y, z)
    }

    /// Map a vector by the linear part.
    pub(crate) fn vector(self, vector: Vector3) -> Vector3 {
        let rows = self.transform.affine_rows();
        let [x, y, z] = std::array::from_fn(|row| {
            let r = rows[row];
            r[0] * vector.x + r[1] * vector.y + r[2] * vector.z
        });
        Vector3::new(x, y, z)
    }

    /// Map a model point back to the placed geometry's own coordinates.
    pub(crate) fn inverse_point(self, point: Point3) -> Option<Point3> {
        self.inverse?
            .apply_point(point)
            .map(cadmpeg_ir::features::FinitePoint3::get)
    }

    /// Map a direction by the rotation part `L/s`.
    pub(crate) fn rotate(self, vector: Vector3) -> Vector3 {
        self.vector(vector).scale(1.0 / self.scale)
    }

    /// The model term of a quantity of dimension `dimension`: `d·δ/2·|value|`.
    pub(crate) fn model_term(self, dimension: u8, value: f64) -> f64 {
        f64::from(dimension) * self.deviation / 2.0 * value.abs()
    }
}

fn non_finite() -> MeasureFailure {
    MeasureFailure::unavailable(
        UnavailableReason::PlacementNonFinite,
        None,
        "the placement's linear part is degenerate or not finite",
    )
}

#[cfg(test)]
mod tests {
    use super::{Placement, PlacementClass};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::transform::Transform;

    fn transform(rows: [[f64; 4]; 3]) -> Placement {
        Placement::of(Transform::affine(rows).unwrap()).unwrap()
    }

    #[test]
    fn placements_classify_by_their_gram_matrix() {
        assert_eq!(
            Placement::of(Transform::identity()).unwrap().class,
            PlacementClass::Identity
        );
        let (sine, cosine) = 0.3_f64.sin_cos();
        let rotation = transform([
            [cosine, -sine, 0.0, 5.0],
            [sine, cosine, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]);
        assert_eq!(rotation.class, PlacementClass::Rigid);
        assert!(!rotation.reflection);
        let scaled = transform([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
        ]);
        assert_eq!(scaled.class, PlacementClass::Similarity);
        assert!((scaled.scale - 2.0).abs() < 1.0e-15);
        let reflection = transform([
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]);
        assert_eq!(reflection.class, PlacementClass::Rigid);
        assert!(reflection.reflection);
        let shear = transform([
            [1.0, 0.5, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]);
        assert_eq!(shear.class, PlacementClass::Affine);
    }

    #[test]
    fn four_admitted_frames_compose_to_a_rigid_placement() {
        // Each frame deviates from orthonormal by about 2e-9 per Gram entry,
        // the admission bound of a unit frame.
        let skew = 1.0e-9;
        let frame = transform([
            [1.0 + skew, skew, 0.0, 1.0],
            [0.0, 1.0, skew, 0.0],
            [skew, 0.0, 1.0 - skew, 0.0],
        ]);
        let composed = frame
            .then(frame)
            .unwrap()
            .then(frame)
            .unwrap()
            .then(frame)
            .unwrap();
        assert_eq!(composed.class, PlacementClass::Rigid);
        assert!(composed.deviation > 0.0);
    }

    #[test]
    fn placements_map_points_and_directions() {
        let placement = transform([
            [0.0, -2.0, 0.0, 1.0],
            [2.0, 0.0, 0.0, 2.0],
            [0.0, 0.0, 2.0, 3.0],
        ]);
        let point = placement.point(Point3::new(1.0, 0.0, 0.0));
        assert_eq!([point.x, point.y, point.z], [1.0, 4.0, 3.0]);
        let direction = placement.rotate(Vector3::new(1.0, 0.0, 0.0));
        assert!((direction.y - 1.0).abs() < 1.0e-15);
        assert_eq!(placement.model_term(2, 10.0), 0.0);
    }
}
