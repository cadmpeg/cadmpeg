// SPDX-License-Identifier: Apache-2.0
//! Caller-budget admission for copied decoded geometry.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::geometry::analytic::{LineCurve, PlaneSurface};
use crate::geometry::nurbs::{
    NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
};
use crate::geometry::pcurve::{
    LinePcurve, PcurveGeometry, PcurveNurbs, PcurveNurbsPoles, PlacedPcurve,
};
use crate::geometry::sampled::{PolygonalSurface, PolylineCurve, PolylineSamples};
use crate::features::NonEmptyMembers;
use crate::geometry::CompositeCurveSegments;
use crate::ids::CurveId;
use crate::geometry::{PlacedCurve, PlacedSurface, SolvedCurveGeometry, SolvedSurfaceGeometry};
use crate::math::{Point2, Point3, Vector3};
use crate::transform::{Transform, Transform2};

fn context_with_limit<'a>(arena: &'a DecodeArena, policy: &'a DecodePolicy) -> DecodeContext<'a> {
    DecodeContext::from_root_bytes(b"", arena, policy)
        .expect("empty root fits policy")
        .0
}

fn line() -> SolvedCurveGeometry {
    SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("valid line"),
    )
}

fn plane() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid plane"),
    )
}

macro_rules! curve_copy_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let geometry = SolvedCurveGeometry::Transformed(
                PlacedCurve::try_new(Box::new(line()), Transform::identity())
                    .expect("valid placement"),
            );
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let ctx = context_with_limit(&arena, &policy);
            assert!(matches!(
                geometry.try_clone_for_decode(&ctx, $operation),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation
            ));
        }
    };
}

curve_copy_limit_test!(
    curve_replica_basis_copy_refuses_collection_limit,
    "step_curve_replica_basis"
);
curve_copy_limit_test!(
    trim_curve_carrier_copy_refuses_collection_limit,
    "step_trim_curve_carrier"
);
curve_copy_limit_test!(
    offset_curve_carrier_copy_refuses_collection_limit,
    "step_offset_curve_carrier"
);

macro_rules! surface_copy_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let geometry = SolvedSurfaceGeometry::Transformed(
                PlacedSurface::try_new(Box::new(plane()), Transform::identity())
                    .expect("valid placement"),
            );
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let ctx = context_with_limit(&arena, &policy);
            assert!(matches!(
                geometry.try_clone_for_decode(&ctx, $operation),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation
            ));
        }
    };
}

surface_copy_limit_test!(
    trim_surface_carrier_copy_refuses_collection_limit,
    "step_trim_surface_carrier"
);
surface_copy_limit_test!(
    curve_bounded_surface_carrier_copy_refuses_collection_limit,
    "step_curve_bounded_surface_carrier"
);
surface_copy_limit_test!(
    surface_replica_basis_copy_refuses_collection_limit,
    "step_surface_replica_basis"
);

#[test]
fn nurbs_curve_copy_refuses_knot_limit() {
    let geometry = SolvedCurveGeometry::Nurbs(
        NurbsCurve::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            NurbsPoles3::Polynomial {
                points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            },
            false,
        )
        .expect("valid NURBS"),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let ctx = context_with_limit(&arena, &policy);
    assert!(
        matches!(geometry.try_clone_for_decode(&ctx, "step_trim_curve_carrier"),
        Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn polygonal_surface_copy_refuses_vertex_limit() {
    let geometry = SolvedSurfaceGeometry::Polygonal(
        PolygonalSurface::new(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            vec![[0, 1, 2]],
            0.0,
        )
        .expect("valid polygon"),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let ctx = context_with_limit(&arena, &policy);
    assert!(
        matches!(geometry.try_clone_for_decode(&ctx, "step_trim_surface_carrier"),
        Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn nurbs_surface_copy_refuses_inner_row_limit() {
    let points = [
        [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        [Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
    ];
    let geometry = SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::new(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsPoleGrid::Polynomial {
                rows: points.map(Vec::from).into(),
            },
            false,
        )
        .expect("valid NURBS surface"),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 11;
    let ctx = context_with_limit(&arena, &policy);
    assert!(
        matches!(geometry.try_clone_for_decode(&ctx, "step_trim_surface_carrier"),
        Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_trim_surface_carrier")
    );
}

#[test]
fn pcurve_nurbs_copy_refuses_pole_limit() {
    let geometry = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            PcurveNurbsPoles::Polynomial {
                points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            },
            false,
        )
        .expect("valid pcurve NURBS"),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 5;
    let ctx = context_with_limit(&arena, &policy);
    assert!(
        matches!(geometry.try_clone_for_decode(&ctx, "step_pcurve_carrier_copy"),
        Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_pcurve_carrier_copy")
    );
}

#[test]
fn pcurve_carrier_copy_refuses_collection_limit() {
    let geometry = PcurveGeometry::Transformed(
        PlacedPcurve::try_new(
            Box::new(PcurveGeometry::Line(LinePcurve::U_AXIS)),
            Transform2::identity(),
        )
        .expect("valid placement"),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let ctx = context_with_limit(&arena, &policy);
    assert!(
        matches!(geometry.try_clone_for_decode(&ctx, "step_pcurve_carrier_copy"),
        Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_pcurve_carrier_copy")
    );
}

#[test]
fn pcurve_coordinate_scale_uses_no_collection_items() {
    let nurbs = PcurveNurbs::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        PcurveNurbsPoles::Polynomial {
            points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        },
        false,
    )
    .expect("valid pcurve NURBS");
    let mut geometry = PcurveGeometry::Nurbs { nurbs };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let ctx = context_with_limit(&arena, &policy);
    assert!(matches!(
        geometry.try_scale_coordinates_for_decode([2.0, 3.0], &ctx, "step_pcurve_coordinate_scale"),
        Ok(true)
    ));
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("the pcurve form stays NURBS");
    };
    assert_eq!(
        nurbs.control_points(),
        vec![Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)]
    );
}

#[test]
fn solved_nurbs_copy_admits_knots_and_poles() {
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("test setup");
    let geometry = SolvedCurveGeometry::Nurbs(curve);
    for (cap, operation) in [
        (0, "iges solved curve copied knots"),
        (4, "iges solved curve copied poles"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test setup");
        let result = geometry.try_clone_for_decode(&ctx, operation);
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test setup");
    assert_eq!(
        geometry.try_clone_for_decode(&ctx, "iges solved curve copied knots").expect("test setup"),
        geometry
    );
}

#[test]
fn solved_polyline_copy_admits_sample_lane() {
    let samples = PolylineSamples::Unparameterized {
        points: NonEmptyMembers::try_from(vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ])
        .expect("test setup"),
    };
    let geometry =
        SolvedCurveGeometry::Polyline(PolylineCurve::new(samples, 0.0).expect("test setup"));
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test setup");
    assert!(
        matches!(geometry.try_clone_for_decode(&ctx, "iges solved curve copied polyline samples"), Err(CodecError::ResourceLimit(limit)) if limit.operation == "iges solved curve copied polyline samples")
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test setup");
    assert_eq!(
        geometry.try_clone_for_decode(&ctx, "iges solved curve copied polyline samples").expect("test setup"),
        geometry
    );
}

#[test]
fn solved_composite_copy_admits_nested_curve_id() {
    let segment = crate::geometry::CompositeCurveSegment {
        curve: CurveId::mint("test:model:curve#child").expect("test setup"),
        same_sense: true,
        transition: crate::geometry::CompositeCurveTransition::Discontinuous,
    };
    let geometry = SolvedCurveGeometry::Composite {
        segments: CompositeCurveSegments::try_from(vec![segment]).expect("test setup"),
        self_intersect: None,
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test setup");
    assert!(
        matches!(geometry.try_clone_for_decode(&ctx, "iges solved curve copied composite ID"), Err(CodecError::ResourceLimit(limit)) if limit.operation == "iges solved curve copied composite ID")
    );
}

#[test]
fn polygonal_support_copy_admits_vertices_and_triangles() {
    let polygon = PolygonalSurface::new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2]],
        0.0,
    )
    .expect("test setup");
    for (cap, operation) in [
        (0, "iges copied support polygon vertices"),
        (3, "iges copied support polygon triangles"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test setup");
        assert!(
            matches!(polygon.try_clone_for_decode(&ctx, operation), Err(CodecError::ResourceLimit(limit)) if limit.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test setup");
    assert_eq!(
        polygon.try_clone_for_decode(&ctx, "iges copied support polygon vertices").expect("test setup"),
        polygon
    );
}
