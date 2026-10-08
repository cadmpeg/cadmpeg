// SPDX-License-Identifier: Apache-2.0
use super::*;
use super::super::super::composite::{CompositeCurveError, CompositeIndex};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::topology::{Edge, EdgeCarrier, Point, Vertex};

const SOURCE_CURVE_ID: &str = "iges:model:curve#D5";
const BOUNDED_EDGE_START_ID: &str = "a:b:v#s";
const BOUNDED_EDGE_END_ID: &str = "a:b:v#e";

fn pcurve_source_ir() -> CadIr {
    let surface_id = SurfaceId::mint("iges:model:surface#D1").unwrap();
    let curve_id = CurveId::mint(SOURCE_CURVE_ID).unwrap();
    let nurbs = crate::test_support::with_service_context(&[], |ctx| {
        NurbsCurve::new(
            ctx,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            NurbsPoles3::Polynomial {
                points: vec![
                    FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                    FinitePoint3::new(Point3::new(1.0, 1.0, 0.0)).unwrap(),
                ],
            },
            false,
        )
        .unwrap()
        .unwrap()
    });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: surface_id,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
        source_object: None,
    });
    let start_point = PointId::mint("test:model:point#bounded-start").unwrap();
    let end_point = PointId::mint("test:model:point#bounded-end").unwrap();
    ir.model.points.extend([
        Point::new(
            start_point.clone(),
            FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            None,
        ),
        Point::new(
            end_point.clone(),
            FinitePoint3::new(Point3::new(1.0, 1.0, 0.0)).unwrap(),
            None,
        ),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId::mint(BOUNDED_EDGE_START_ID).unwrap(),
            point: start_point,
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint(BOUNDED_EDGE_END_ID).unwrap(),
            point: end_point,
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:edge#bounded").unwrap(),
        carrier: EdgeCarrier::new(Some(curve_id), Some([0.0, 1.0])).unwrap(),
        start: VertexId::mint(BOUNDED_EDGE_START_ID).unwrap(),
        end: VertexId::mint(BOUNDED_EDGE_END_ID).unwrap(),
        tolerance: None,
    });
    ir
}

fn composite_error_as_codec(error: CompositeCurveError) -> CodecError {
    match error {
        CompositeCurveError::Budget(error) => error,
        CompositeCurveError::Carrier(cadmpeg_ir::geometry::nurbs::NurbsError::ResourceLimit(
            limit,
        )) => limit.into(),
        error => CodecError::malformed(error.to_string()),
    }
}

fn project_source_pcurve(
    ir: &CadIr,
    index: &CompositeIndex,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(PcurveGeometry, [f64; 2])>, CodecError> {
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D1")
        .expect("synthetic support plane");
    super::super::pcurve_geometry(
        ir,
        None,
        5,
        &super::super::PcurveSupport {
            surface_id: &surface.id,
            geometry: &surface.geometry,
            factor: 1.0,
        },
        Some(0.001),
        ctx,
        index,
    )
    .map_err(composite_error_as_codec)
}

fn composite_index(ir: &CadIr) -> CompositeIndex {
    crate::test_support::with_service_context(&[], |ctx| {
        CompositeIndex::from_ir(ir, ctx).unwrap()
    })
}

fn assert_source_pcurve_exists(ir: &CadIr, index: &CompositeIndex) {
    crate::test_support::with_service_context(&[], |ctx| {
        let Some((PcurveGeometry::Nurbs { nurbs }, range)) =
            project_source_pcurve(ir, index, ctx).unwrap()
        else {
            panic!("the synthetic bounded 3D curve must produce a pcurve NURBS");
        };
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(nurbs.degree(), 1);
        assert_eq!(nurbs.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
    });
}

fn bounded_source_peak_bytes(ir: &CadIr) -> u64 {
    let curve = &ir.model.curves[0];
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = curve.geometry.solved() else {
        panic!("source carrier is a solved NURBS curve");
    };
    let pole_count = nurbs.pole_count();
    let knot_count = nurbs.knots().len();

    // This fixture uses the full clamped domain, so trimming inserts no knots.
    // While the 3D source is live, the scratch holds its homogeneous controls,
    // a source-knot copy, the bounded-knot copy, and the bounded 3D poles.
    let bytes = pole_count * std::mem::size_of::<[f64; 4]>()
        + 2 * knot_count * std::mem::size_of::<f64>()
        + pole_count * std::mem::size_of::<FinitePoint3>();
    // The generated source identity stays live while the bounded 3D lanes are
    // copied and mapped inside the outer pcurve source scope.
    u64::try_from(
        bytes
            + SOURCE_CURVE_ID.len()
            + BOUNDED_EDGE_START_ID.len()
            + BOUNDED_EDGE_END_ID.len(),
    )
    .unwrap()
}

#[test]
fn bounded_source_storage_is_live_during_mapping_and_released_afterward() {
    let ir = pcurve_source_ir();
    let index = composite_index(&ir);
    assert_source_pcurve_exists(&ir, &index);

    let source_knot_copy = {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = u64::MAX;
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::MaterializedBytes,
            "iges composite trimmed knots",
            None,
        );
        match crate::test_support::with_policy_context(&[], &policy, |ctx| {
            project_source_pcurve(&ir, &index, ctx)
        }) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(limit.operation, "iges composite trimmed knots");
                limit.additional
            }
            Err(error) => panic!("unexpected bounded-source refusal: {error:?}"),
            Ok(_) => panic!("missing live bounded-source knot allocation"),
        }
    };
    assert_eq!(
        source_knot_copy,
        4 * u64::try_from(std::mem::size_of::<f64>()).unwrap()
    );

    let peak = bounded_source_peak_bytes(&ir);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = peak;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let Some((PcurveGeometry::Nurbs { nurbs }, range)) =
            project_source_pcurve(&ir, &index, ctx).unwrap()
        else {
            panic!("bounded source curve produces a pcurve NURBS");
        };
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(nurbs.degree(), 1);
        assert_eq!(nurbs.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);

        // The cap is the derived peak for the complete source scratch. The
        // mapped knot vector is retained output; the same full scratch fits
        // again only after pcurve_geometry drops its source reservation.
        ctx.reserve_scoped(peak, "verify released bounded pcurve source storage")
            .unwrap();
        assert_eq!(nurbs.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
    });
}

#[test]
fn mapped_pcurve_knots_remain_retained_after_source_storage_is_released() {
    let ir = pcurve_source_ir();
    let index = composite_index(&ir);
    assert_source_pcurve_exists(&ir, &index);
    let additional = {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RetainedBytes,
            "iges pcurve output knots",
            None,
        );
        match crate::test_support::with_policy_context(&[], &policy, |ctx| {
            project_source_pcurve(&ir, &index, ctx)
        }) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.operation, "iges pcurve output knots");
                limit.additional
            }
            Err(error) => panic!("unexpected mapped-knot refusal: {error:?}"),
            Ok(_) => panic!("missing retained mapped-knot boundary"),
        }
    };
    assert_eq!(additional, 4 * u64::try_from(std::mem::size_of::<f64>()).unwrap());

    crate::test_support::with_service_context(&[], |ctx| {
        let Some((PcurveGeometry::Nurbs { nurbs }, range)) =
            project_source_pcurve(&ir, &index, ctx).unwrap()
        else {
            panic!("bounded source curve produces a pcurve NURBS");
        };
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(nurbs.degree(), 1);
        assert_eq!(nurbs.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
    });
}
