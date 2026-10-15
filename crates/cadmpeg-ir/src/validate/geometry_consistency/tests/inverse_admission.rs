// SPDX-License-Identifier: Apache-2.0

use crate::geometry::pcurve::{LinePcurve, PcurveGeometry};
use crate::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn fixture() -> (crate::CadIr, crate::ids::SurfaceId, PcurveGeometry) {
    let id: crate::ids::SurfaceId = "test:model:surface#inverse-admission".try_into().unwrap();
    let mut ir = crate::CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            crate::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
    });
    (
        ir,
        id,
        PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
        ),
    )
}

#[test]
fn mapped_pcurve_inverse_admits_exact_linear_step_without_storage() {
    let (ir, id, geometry) = fixture();
    let index = crate::index::ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    let context = super::super::SurfacePcurveContext {
        index: &index,
        surface_id: &id,
        geometry: &ir.model.surfaces[0].geometry,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        super::super::mapped_pcurve_parameter_near_point(
            &ctx,
            &context,
            &geometry,
            Point3::new(1.0, 0.0, 0.0),
            FiniteReal::ZERO,
            0.0
        )
        .unwrap(),
        Some(FiniteReal::ONE)
    );
    ctx.finish_session().unwrap();
}
