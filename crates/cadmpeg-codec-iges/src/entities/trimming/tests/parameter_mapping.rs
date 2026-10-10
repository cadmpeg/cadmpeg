// SPDX-License-Identifier: Apache-2.0
//! Procedural parameter charts, repeated lookup, and work admission.

use super::super::{
    pcurve_parameter_map, procedural_source_parameter_map, PcurveSupport,
    ProceduralSourceParameterMap,
};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_ir::geometry::{
    analytic::{LineCurve, PlaneSurface},
    surface_payloads::ExtrusionSurfaceConstruction,
    CacheContract, Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition,
    RecordBounds, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;

fn extrusions(count: usize) -> CadIr {
    let mut ir = CadIr::empty();
    let directrix = CurveId::mint("test:model:curve#line").unwrap();
    ir.model.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        )),
        parameter_range: None,
        source_object: None,
    });
    for ordinal in 0..count {
        let id = ProceduralSurfaceId::mint(format!("test:model:construction#{ordinal}")).unwrap();
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            id.clone(),
            ProceduralSurfaceDefinition::Extrusion(
                ExtrusionSurfaceConstruction::try_new(
                    directrix.clone(),
                    None,
                    Vector3::new(0.0, 1.0, 0.0),
                    None,
                    CacheContract::legacy(),
                )
                .unwrap(),
            ),
            Some(RecordBounds::try_new([Some(2.0), Some(5.0), None, None]).unwrap()),
        ));
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("test:model:surface#{ordinal}")).unwrap(),
            geometry: SurfaceGeometry::Procedural {
                construction: id,
                cache: None,
            },
            source_object: None,
        });
    }
    ir
}

fn plane() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ))
}

#[test]
fn repeated_parameter_charts_preserve_normalized_line_and_length_coordinates() {
    let ir = extrusions(1024);
    let geometry = plane();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2_000_000;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let index = ModelIndex::new_model_only(&ir, ctx).unwrap();
        for surface in &ir.model.surfaces {
            let support = PcurveSupport {
                surface_id: &surface.id,
                geometry: &geometry,
                factor: 2.0,
            };
            let source = procedural_source_parameter_map(&index, &support, ctx).unwrap();
            // The normalized source interval [0,1] maps to the retained [2,5].
            let ProceduralSourceParameterMap::Mapped(map) = source else {
                panic!("missing chart")
            };
            assert_eq!(map, (3.0, 2.0, 1.0, 0.0));
            assert_eq!(
                pcurve_parameter_map(source, &support),
                Some((1.5, 2.0, 0.5, 0.0))
            );
        }
    });
}

#[test]
fn parameter_chart_queries_propagate_work_refusal() {
    let ir = extrusions(1);
    let geometry = plane();
    crate::test_support::with_service_context(&[], |build_ctx| {
        let index = ModelIndex::new_model_only(&ir, build_ctx).unwrap();
        let support = PcurveSupport {
            surface_id: &ir.model.surfaces[0].id,
            geometry: &geometry,
            factor: 1.0,
        };
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        crate::test_support::with_policy_context(&[], &policy, |ctx| {
            assert!(
                matches!(procedural_source_parameter_map(&index, &support, ctx),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits)
            );
        });
    });
}

#[test]
fn missing_procedural_chart_does_not_become_a_solved_parameter_map() {
    let mut ir = extrusions(1);
    ir.model.procedural_surfaces.clear();
    crate::test_support::with_service_context(&[], |ctx| {
        let index = ModelIndex::new_model_only(&ir, ctx).unwrap();
        let surface = &ir.model.surfaces[0];
        let support = PcurveSupport {
            surface_id: &surface.id,
            geometry: &surface.geometry,
            factor: 1.0,
        };
        let source = procedural_source_parameter_map(&index, &support, ctx).unwrap();
        assert!(matches!(source, ProceduralSourceParameterMap::Unavailable));
        assert_eq!(pcurve_parameter_map(source, &support), None);
    });
}
