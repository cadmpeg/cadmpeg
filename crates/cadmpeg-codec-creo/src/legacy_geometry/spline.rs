// SPDX-License-Identifier: Apache-2.0

/// Complete interpolation data selected from a legacy spline grid.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LegacySpline {
    points: Vec<[f64; 3]>,
    u_parameters: Vec<f64>,
    v_parameters: Vec<f64>,
    u_derivatives: Vec<[f64; 3]>,
    v_derivatives: Vec<[f64; 3]>,
    mixed_derivatives: [[f64; 3]; 4],
}

impl LegacySpline {
    /// Admits complete finite source grids and selects boundary derivatives.
    pub(crate) fn from_grid(
        points: Vec<[f64; 3]>,
        u_parameters: Vec<f64>,
        v_parameters: Vec<f64>,
        u_tangents: &[[f64; 3]],
        v_tangents: &[[f64; 3]],
        mixed_derivatives: &[[f64; 3]],
    ) -> Option<Self> {
        let u_count = u_parameters.len();
        let v_count = v_parameters.len();
        let point_count = u_count.checked_mul(v_count)?;
        let ordered_finite = |parameters: &[f64]| {
            parameters.iter().all(|value| value.is_finite())
                && parameters.windows(2).all(|pair| pair[0] < pair[1])
        };
        let vectors_finite =
            |vectors: &[[f64; 3]]| vectors.iter().flatten().all(|value| value.is_finite());
        (u_count >= 2
            && v_count >= 2
            && ordered_finite(&u_parameters)
            && ordered_finite(&v_parameters)
            && vectors_finite(&points)
            && points.len() == point_count
            && vectors_finite(u_tangents)
            && vectors_finite(v_tangents)
            && vectors_finite(mixed_derivatives)
            && u_tangents.len() == point_count
            && v_tangents.len() == point_count
            && mixed_derivatives.len() == point_count)
            .then_some(())?;

        let upper_u = (u_count - 1).checked_mul(v_count)?;
        let upper_v = v_count - 1;
        let u_derivatives = (0..v_count)
            .map(|v| u_tangents[v])
            .chain((0..v_count).map(|v| u_tangents[upper_u + v]))
            .collect();
        let v_derivatives = (0..u_count)
            .map(|u| v_tangents[u * v_count])
            .chain((0..u_count).map(|u| v_tangents[u * v_count + upper_v]))
            .collect();
        let mixed_derivatives = [
            mixed_derivatives[0],
            mixed_derivatives[upper_v],
            mixed_derivatives[upper_u],
            mixed_derivatives[upper_u + upper_v],
        ];
        Some(Self {
            points,
            u_parameters,
            v_parameters,
            u_derivatives,
            v_derivatives,
            mixed_derivatives,
        })
    }

    /// Interpolation points in u-major order.
    pub(crate) fn points(&self) -> &[[f64; 3]] {
        &self.points
    }

    /// Ordered parameters in the first direction.
    pub(crate) fn u_parameters(&self) -> &[f64] {
        &self.u_parameters
    }

    /// Ordered parameters in the second direction.
    pub(crate) fn v_parameters(&self) -> &[f64] {
        &self.v_parameters
    }

    /// Lower-u and upper-u boundary derivatives.
    pub(crate) fn u_derivatives(&self) -> &[[f64; 3]] {
        &self.u_derivatives
    }

    /// Lower-v and upper-v boundary derivatives.
    pub(crate) fn v_derivatives(&self) -> &[[f64; 3]] {
        &self.v_derivatives
    }

    /// Mixed derivatives at the four corners.
    pub(crate) fn mixed_derivatives(&self) -> &[[f64; 3]; 4] {
        &self.mixed_derivatives
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_grid_admission_rejects_mismatched_and_unordered_fields() {
        let points = vec![[0.0; 3]; 6];
        let u = vec![0.0, 0.5, 1.0];
        let v = vec![0.0, 1.0];
        for missing in 0..4 {
            let mut grids = [
                points.clone(),
                points.clone(),
                points.clone(),
                points.clone(),
            ];
            grids[missing].pop();
            let [points, u_tangents, v_tangents, mixed] = grids;
            assert!(LegacySpline::from_grid(
                points,
                u.clone(),
                v.clone(),
                &u_tangents,
                &v_tangents,
                &mixed
            )
            .is_none());
        }
        assert!(LegacySpline::from_grid(
            points.clone(),
            vec![0.0, 1.0, 0.5],
            v,
            &points,
            &points,
            &points
        )
        .is_none());
    }
}
