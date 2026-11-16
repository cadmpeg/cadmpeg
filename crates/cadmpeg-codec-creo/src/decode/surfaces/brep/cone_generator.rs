// SPDX-License-Identifier: Apache-2.0
//! Source-chart witnesses for cone generators.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::decode::analytic::pcurves::{unique_oriented_native_pcurve, NativePcurveCandidates};
use crate::decode::analytic::vertices::{finite_model_point, model_points_agree};

const EPS_ANGLE: f64 = 1.0e-9;

pub(super) fn carrier(
    ctx: &DecodeContext<'_>,
    curve: u32,
    faces: [u32; 2],
    surfaces: [&SurfaceGeometry; 2],
    candidates: &NativePcurveCandidates,
    endpoints: [[f64; 3]; 2],
) -> Result<Option<CurveGeometry>, CodecError> {
    let Some(midpoint) = finite_model_point(std::array::from_fn(|i| {
        (endpoints[0][i] + endpoints[1][i]) * 0.5
    })) else {
        return Ok(None);
    };
    for (face, surface) in faces.into_iter().zip(surfaces) {
        if !matches!(
            surface,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_))
        ) {
            return Ok(None);
        }
        let Some(paths) = candidates.get(&(curve, face)) else {
            return Ok(None);
        };
        let Some(path) = unique_oriented_native_pcurve(ctx, surface, paths, endpoints)? else {
            return Ok(None);
        };
        let [start, end] = path.endpoints;
        if (start[0] - end[0]).abs() > EPS_ANGLE || start[1] == end[1] {
            return Ok(None);
        }
        let mapped = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::surface_point(
                ctx,
                surface,
                (start[0] + end[0]) * 0.5,
                (start[1] + end[1]) * 0.5,
            ),
        )?)?;
        if !mapped.is_some_and(|point| model_points_agree(point, midpoint)) {
            return Ok(None);
        }
    }
    let Some(direction) =
        crate::vecmath::normalize(std::array::from_fn(|i| endpoints[1][i] - endpoints[0][i]))
    else {
        return Ok(None);
    };
    Ok(cadmpeg_ir::geometry::analytic::LineCurve::try_new(
        Point3::from(endpoints[0]),
        Vector3::from(direction),
    )
    .ok()
    .map(|line| CurveGeometry::Solved(SolvedCurveGeometry::Line(line))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::analytic::ConeSurface;

    #[test]
    fn accepts_only_agreeing_constant_angle_cone_paths() {
        let cone = ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            0.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("cone");
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone));
        let faces = [2, 3];
        let endpoints = [[0.0, 0.0, 0.0], [2.0, 0.0, 2.0]];
        let mut candidates = NativePcurveCandidates::new();
        candidates.insert((1, 2), vec![([[0.0, 0.0], [0.0, 2.0]], 0)]);
        candidates.insert(
            (1, 3),
            vec![(
                [[std::f64::consts::TAU, 0.0], [std::f64::consts::TAU, 2.0]],
                0,
            )],
        );
        let evaluate = |candidates: &NativePcurveCandidates, endpoints| {
            crate::decode::with_service_context(|ctx| {
                carrier(ctx, 1, faces, [&surface, &surface], candidates, endpoints)
            })
            .expect("evaluate cone charts")
        };
        assert!(matches!(
            evaluate(&candidates, endpoints),
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(_)))
        ));
        assert!(matches!(
            evaluate(&candidates, [endpoints[1], endpoints[0]]),
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(_)))
        ));
        assert!(evaluate(&candidates, [endpoints[0], [0.0, 2.0, 2.0]]).is_none());
        candidates.insert(
            (1, 3),
            vec![([[0.0, 0.0], [std::f64::consts::TAU, 2.0]], 0)],
        );
        assert!(evaluate(&candidates, endpoints).is_none());
    }
}
