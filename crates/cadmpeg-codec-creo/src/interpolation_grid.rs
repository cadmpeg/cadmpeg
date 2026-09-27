// SPDX-License-Identifier: Apache-2.0

/// A complete bicubic interpolation grid: points, ordered parameters,
/// boundary derivatives and corner mixed derivatives that agree in length.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InterpolationGrid {
    points: Vec<[f64; 3]>,
    u_parameters: Vec<f64>,
    v_parameters: Vec<f64>,
    u_derivatives: Vec<[f64; 3]>,
    v_derivatives: Vec<[f64; 3]>,
    mixed_derivatives: [[f64; 3]; 4],
}

impl InterpolationGrid {
    /// Admits a complete finite grid with increasing parameters and matching
    /// point and boundary-derivative counts.
    pub(crate) fn try_new(
        points: Vec<[f64; 3]>,
        u_parameters: Vec<f64>,
        v_parameters: Vec<f64>,
        u_derivatives: Vec<[f64; 3]>,
        v_derivatives: Vec<[f64; 3]>,
        mixed_derivatives: [[f64; 3]; 4],
    ) -> Option<Self> {
        let u_count = u_parameters.len();
        let v_count = v_parameters.len();
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
            && vectors_finite(&u_derivatives)
            && vectors_finite(&v_derivatives)
            && mixed_derivatives
                .iter()
                .flatten()
                .all(|value| value.is_finite())
            && points.len() == u_count.checked_mul(v_count)?
            && u_derivatives.len() == v_count.checked_mul(2)?
            && v_derivatives.len() == u_count.checked_mul(2)?)
        .then_some(())?;
        Some(Self {
            points,
            u_parameters,
            v_parameters,
            u_derivatives,
            v_derivatives,
            mixed_derivatives,
        })
    }

    /// Selects boundary derivatives from a complete finite source grid.
    pub(crate) fn from_full_tangent_grid(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        points: Vec<[f64; 3]>,
        u_parameters: Vec<f64>,
        v_parameters: Vec<f64>,
        u_tangents: &[[f64; 3]],
        v_tangents: &[[f64; 3]],
        mixed_derivatives: &[[f64; 3]],
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let u_count = u_parameters.len();
        let v_count = v_parameters.len();
        let Some(point_count) = u_count.checked_mul(v_count) else {
            return Ok(None);
        };
        let vectors_finite =
            |vectors: &[[f64; 3]]| vectors.iter().flatten().all(|value| value.is_finite());
        if !(u_count >= 2
            && v_count >= 2
            && points.len() == point_count
            && vectors_finite(u_tangents)
            && vectors_finite(v_tangents)
            && vectors_finite(mixed_derivatives)
            && u_tangents.len() == point_count
            && v_tangents.len() == point_count
            && mixed_derivatives.len() == point_count)
        {
            return Ok(None);
        }

        let Some(upper_u) = (u_count - 1).checked_mul(v_count) else {
            return Ok(None);
        };
        let upper_v = v_count - 1;
        let Some(u_derivative_count) = v_count.checked_mul(2) else {
            return Ok(None);
        };
        let mut u_derivatives = Vec::new();
        ctx.try_reserve_items(
            &mut u_derivatives,
            u_derivative_count,
            "creo legacy spline u derivatives",
        )?;
        u_derivatives.extend(
            (0..v_count)
                .map(|v| u_tangents[v])
                .chain((0..v_count).map(|v| u_tangents[upper_u + v])),
        );
        let Some(v_derivative_count) = u_count.checked_mul(2) else {
            return Ok(None);
        };
        let mut v_derivatives = Vec::new();
        ctx.try_reserve_items(
            &mut v_derivatives,
            v_derivative_count,
            "creo legacy spline v derivatives",
        )?;
        v_derivatives.extend(
            (0..u_count)
                .map(|u| v_tangents[u * v_count])
                .chain((0..u_count).map(|u| v_tangents[u * v_count + upper_v])),
        );
        let mixed_derivatives = [
            mixed_derivatives[0],
            mixed_derivatives[upper_v],
            mixed_derivatives[upper_u],
            mixed_derivatives[upper_u + upper_v],
        ];
        Ok(Self::try_new(
            points,
            u_parameters,
            v_parameters,
            u_derivatives,
            v_derivatives,
            mixed_derivatives,
        ))
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
    use super::InterpolationGrid;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn from_full_tangent_grid(
        points: Vec<[f64; 3]>,
        u_parameters: Vec<f64>,
        v_parameters: Vec<f64>,
        u_tangents: &[[f64; 3]],
        v_tangents: &[[f64; 3]],
        mixed_derivatives: &[[f64; 3]],
    ) -> Option<InterpolationGrid> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
        InterpolationGrid::from_full_tangent_grid(
            &ctx,
            points,
            u_parameters,
            v_parameters,
            u_tangents,
            v_tangents,
            mixed_derivatives,
        )
        .expect("service admits grid derivatives")
    }

    fn derivative_limit_result(
        limit: u64,
    ) -> Result<Option<InterpolationGrid>, cadmpeg_core::CodecError> {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
        InterpolationGrid::from_full_tangent_grid(
            &ctx,
            vec![[0.0; 3]; 4],
            vec![0.0, 1.0],
            vec![0.0, 1.0],
            &[[0.0; 3]; 4],
            &[[0.0; 3]; 4],
            &[[0.0; 3]; 4],
        )
    }

    #[test]
    fn legacy_spline_u_derivatives_refuse_collection_limit() {
        assert!(derivative_limit_result(8)
            .expect("service admits derivatives")
            .is_some());
        assert!(matches!(
            derivative_limit_result(3),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo legacy spline u derivatives"
        ));
    }

    #[test]
    fn legacy_spline_v_derivatives_refuse_collection_limit() {
        assert!(matches!(
            derivative_limit_result(7),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo legacy spline v derivatives"
        ));
    }

    fn valid_grid() -> InterpolationGrid {
        InterpolationGrid::try_new(
            vec![[0.0; 3]; 4],
            vec![0.0, 1.0],
            vec![0.0, 1.0],
            vec![[0.0; 3]; 4],
            vec![[0.0; 3]; 4],
            [[0.0; 3]; 4],
        )
        .expect("valid grid fixture")
    }

    fn readmit(grid: InterpolationGrid) -> Option<InterpolationGrid> {
        InterpolationGrid::try_new(
            grid.points,
            grid.u_parameters,
            grid.v_parameters,
            grid.u_derivatives,
            grid.v_derivatives,
            grid.mixed_derivatives,
        )
    }

    #[test]
    fn grid_admission_rejects_nonfinite_points() {
        let mut grid = valid_grid();
        grid.points[0][0] = f64::NAN;
        assert!(readmit(grid).is_none());
    }

    #[test]
    fn grid_admission_rejects_nonfinite_parameters() {
        let mut grid = valid_grid();
        grid.u_parameters[0] = f64::INFINITY;
        assert!(readmit(grid).is_none());
        let mut grid = valid_grid();
        grid.v_parameters[1] = f64::NAN;
        assert!(readmit(grid).is_none());
    }

    #[test]
    fn grid_admission_rejects_unordered_parameters() {
        let mut grid = valid_grid();
        grid.u_parameters[1] = 0.0;
        assert!(readmit(grid).is_none());
        let mut grid = valid_grid();
        grid.v_parameters.swap(0, 1);
        assert!(readmit(grid).is_none());
    }

    #[test]
    fn grid_admission_rejects_nonfinite_boundary_derivatives() {
        let mut grid = valid_grid();
        grid.u_derivatives[0][1] = f64::NAN;
        assert!(readmit(grid).is_none());
        let mut grid = valid_grid();
        grid.v_derivatives[3][2] = f64::INFINITY;
        assert!(readmit(grid).is_none());
    }

    #[test]
    fn grid_admission_rejects_nonfinite_mixed_derivatives() {
        let mut grid = valid_grid();
        grid.mixed_derivatives[2][0] = f64::NAN;
        assert!(readmit(grid).is_none());
    }

    #[test]
    fn grid_admission_rejects_single_parameter_axes() {
        let mut grid = valid_grid();
        grid.u_parameters.pop();
        grid.points.truncate(2);
        grid.v_derivatives.truncate(2);
        assert!(readmit(grid).is_none());
    }

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
            assert!(from_full_tangent_grid(
                points,
                u.clone(),
                v.clone(),
                &u_tangents,
                &v_tangents,
                &mixed
            )
            .is_none());
        }
        assert!(from_full_tangent_grid(
            points.clone(),
            vec![0.0, 1.0, 0.5],
            v,
            &points,
            &points,
            &points
        )
        .is_none());
    }

    #[test]
    fn grid_admission_rejects_disagreeing_lengths() {
        let points = vec![[0.0; 3]; 6];
        let u = vec![0.0, 0.5, 1.0];
        let v = vec![0.0, 1.0];
        let u_derivatives = vec![[0.0; 3]; 4];
        let v_derivatives = vec![[0.0; 3]; 6];
        assert!(InterpolationGrid::try_new(
            points.clone(),
            u.clone(),
            v.clone(),
            u_derivatives.clone(),
            v_derivatives.clone(),
            [[0.0; 3]; 4],
        )
        .is_some());
        for grid in [
            InterpolationGrid::try_new(
                vec![[0.0; 3]; 5],
                u.clone(),
                v.clone(),
                u_derivatives.clone(),
                v_derivatives.clone(),
                [[0.0; 3]; 4],
            ),
            InterpolationGrid::try_new(
                points.clone(),
                u.clone(),
                v.clone(),
                vec![[0.0; 3]; 3],
                v_derivatives,
                [[0.0; 3]; 4],
            ),
            InterpolationGrid::try_new(
                points,
                u,
                v,
                u_derivatives,
                vec![[0.0; 3]; 5],
                [[0.0; 3]; 4],
            ),
        ] {
            assert!(grid.is_none());
        }
    }
}
