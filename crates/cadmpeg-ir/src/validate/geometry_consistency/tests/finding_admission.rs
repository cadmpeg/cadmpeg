// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
use crate::math::{Point3, Vector3};
use crate::report::check::Finding;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn refuses(
    ir: &CadIr,
    checker: impl Fn(&DecodeContext<'_>, &CadIr, &mut Vec<Finding>) -> Result<(), CodecError>,
    message: &str,
) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    checker(&ctx, ir, &mut findings).unwrap();
    assert!(!findings.is_empty());
    assert!(findings[0].message.starts_with(message), "{findings:?}");
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = checker(&ctx, ir, &mut findings) else {
            panic!("finding must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn support_side_finding_preserves_output_refusals() {
    let ir = super::mapped_surface_curve([3.0, 2.0]);
    refuses(
        &ir,
        super::super::check_procedural_support_consistency,
        "procedural support side 0 misses",
    );
}

#[test]
fn surface_offset_distance_finding_preserves_output_refusals() {
    let mut ir = super::mapped_surface_offset();
    ir.model.curves[1].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        crate::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 2.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    refuses(
        &ir,
        super::super::check_procedural_support_consistency,
        "surface-offset solved curve misses",
    );
}

#[test]
fn edge_endpoint_finding_preserves_output_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let id = ir.model.edges[0].curve().unwrap().clone();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == id)
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        crate::geometry::analytic::LineCurve::try_new(
            Point3::new(20.0, 20.0, 20.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    refuses(
        &ir,
        super::super::check_edge_endpoint_consistency,
        "edge curve endpoints miss",
    );
}

#[test]
fn coedge_use_curve_finding_preserves_output_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let mut curve = ir.model.curves[0].clone();
    curve.id = "test:model:curve#use".try_into().unwrap();
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        crate::geometry::analytic::LineCurve::try_new(
            Point3::new(20.0, 20.0, 20.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    ir.model.coedges[0].use_curve = Some(crate::topology::CoedgeUseCurve {
        curve: curve.id.clone(),
        parameter_range: crate::topology::ParameterInterval::new([0.0, 1.0]).unwrap(),
    });
    ir.model.curves.push(curve);
    refuses(
        &ir,
        super::super::check_edge_endpoint_consistency,
        "coedge use-curve endpoints miss",
    );
}

#[test]
fn mapped_pcurve_finding_preserves_output_refusals() {
    let mut ir = super::untrimmed_surface_curve();
    let crate::geometry::pcurve::PcurveGeometry::Circle(circle) = &ir.model.pcurves[0].geometry
    else {
        panic!("circle fixture");
    };
    ir.model.pcurves[0].geometry = crate::geometry::pcurve::PcurveGeometry::Circle(
        crate::geometry::pcurve::CirclePcurve::try_new(
            circle.center().get(),
            circle.x_axis().get(),
            circle.y_axis().get(),
            2.0,
        )
        .unwrap(),
    );
    refuses(
        &ir,
        super::super::check_pcurve_surface_consistency,
        "pcurve mapped through the face surface misses",
    );
}

fn charted(with_supports: bool) -> CadIr {
    use crate::geometry::{
        ProceduralCurve, ProceduralCurveDefinition, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    let mut ir = CadIr::empty();
    let supports: [crate::ids::SurfaceId; 2] =
        ["test:model:surface#first", "test:model:surface#second"].map(|id| id.try_into().unwrap());
    if with_supports {
        for (id, normal) in supports
            .iter()
            .zip([Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, -1.0, 0.0)])
        {
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                source_object: None,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    crate::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        normal,
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
            });
        }
    }
    let curve: crate::ids::CurveId = "test:model:curve#charted".try_into().unwrap();
    ir.model.curves.push(crate::geometry::Curve {
        id: curve.clone(),
        source_object: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
    });
    let pcurve = crate::geometry::pcurve::PcurveGeometry::Line(
        crate::geometry::pcurve::LinePcurve::try_new(
            crate::math::Point2::new(0.0, 0.0),
            crate::math::Point2::new(1.0, 0.0),
        )
        .unwrap(),
    );
    ir.model
        .add_procedural_curve(
            &crate::document::admission::StandardAdmission,
            &curve,
            ProceduralCurve::new(
                "test:model:procedural#charted".try_into().unwrap(),
                ProceduralCurveDefinition::TolerantIntersection {
                    construction: crate::geometry::TolerantIntersectionConstruction::try_new(
                        supports,
                        [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                        0.01,
                    )
                    .unwrap(),
                    parameterization: Some(
                        crate::geometry::TolerantIntersectionParameterization::try_new(
                            [pcurve.clone(), pcurve],
                            [0.0, 1.0],
                        )
                        .unwrap(),
                    ),
                    cache: None,
                },
            ),
        )
        .unwrap()
        .unwrap();
    ir
}

#[test]
fn charted_intersection_missing_evaluation_finding_preserves_output_refusals() {
    refuses(
        &charted(false),
        super::super::check_procedural_support_consistency,
        "charted tolerant intersection does not evaluate",
    );
}

#[test]
fn charted_intersection_witness_finding_preserves_output_refusals() {
    refuses(
        &charted(true),
        super::super::check_procedural_support_consistency,
        "charted tolerant intersection misses its endpoint witnesses",
    );
}
