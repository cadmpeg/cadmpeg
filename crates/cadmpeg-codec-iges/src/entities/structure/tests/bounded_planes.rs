// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::loss::IgesLossCode;
use crate::test_support::decode;
use crate::test_support::test_owned::{
    owned_test_file_with_global_and_line_fonts, OwnedTestEntity,
};
use crate::test_support::test_surface_fixtures::bounded_plane_entity_file;
use cadmpeg_core::decode::{
    refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{
    analytic::LineCurve, CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry,
    SolvedCurveGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::BTreeSet;
use std::io::Cursor;

const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

#[test]
fn bounded_plane_refuses_boundary_edge_slot_before_draft() {
    let bytes = bounded_plane_entity_file(GLOBAL_V5_0, 100, "100,0,0,0,1,0,1,0;");
    super::assert_structure_refusal(
        &bytes,
        "iges bounded plane boundary edges",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn bounded_plane_identity_copies_refuse_before_retaining_text() {
    let bytes = bounded_plane_entity_file(GLOBAL_V5_0, 100, "100,0,0,0,1,0,1,0;");
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "iges structure identity copy",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            crate::IgesCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        policy,
                        ..DecodeOptions::default()
                    },
                )
                .map_err(|error| match error {
                    cadmpeg_ir::codec::DecodeFailure::Codec(error) => error,
                    other => panic!("{other:?}"),
                })
        },
    );
    decode(bytes);
}

#[test]
fn plane_nurbs_boundary_points_refuse_collection_limit() {
    let nurbs = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges plane NURBS boundary points",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::super::linear_nurbs_boundary_points(
                &nurbs,
                [0.0, 4.0],
                Transform::identity(),
                &ctx,
            )
        },
    );

    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges plane NURBS interior knots",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::super::linear_nurbs_boundary_points(
                &nurbs,
                [0.0, 4.0],
                Transform::identity(),
                &ctx,
            )
        },
    );

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let points =
        super::super::linear_nurbs_boundary_points(&nurbs, [0.0, 4.0], Transform::identity(), &ctx)
            .unwrap()
            .unwrap();
    assert_eq!(points.len(), 5);
    assert_eq!(points[0], points[4]);
    let translated = Transform::affine([
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
    ])
    .unwrap();
    assert_eq!(
        super::super::linear_nurbs_boundary_points(&nurbs, [0.0, 4.0], translated, &ctx)
            .unwrap()
            .unwrap(),
        vec![
            Point3::new(2.0, 3.0, 4.0),
            Point3::new(3.0, 3.0, 4.0),
            Point3::new(3.0, 4.0, 4.0),
            Point3::new(2.0, 4.0, 4.0),
            Point3::new(2.0, 3.0, 4.0),
        ]
    );
    let overflowing = Transform::affine([
        [1e308, 0.0, 0.0, 1e308],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert!(
        super::super::linear_nurbs_boundary_points(&nurbs, [0.0, 4.0], overflowing, &ctx)
            .unwrap()
            .is_none()
    );
}

fn has_entity_projection_loss(result: &cadmpeg_ir::codec::DecodeResult) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
}

#[test]
fn bounded_plane_builds_a_sheet_face_in_v4_and_v5() {
    for (expected_version, global) in [("4.0", GLOBAL_V4), ("5.0", GLOBAL_V5_0)] {
        let result = decode(bounded_plane_entity_file(global, 100, "100,0,0,0,1,0,1,0;"));

        assert_eq!(
            result.report().dialects().unwrap().primary().declared()["effective_version"],
            expected_version
        );
        let face = result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "iges:model:face#bounded-plane-D1")
            .unwrap();
        assert_eq!(face.surface.as_str(), "iges:model:surface#D1");
        assert_eq!(face.loops.len(), 1);
        let loop_ = result
            .ir()
            .model
            .loops
            .iter()
            .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
            .unwrap();
        assert_eq!(
            result
                .ir()
                .model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.loop_role(&loop_.id))
                .unwrap_or_default(),
            cadmpeg_ir::topology::LoopBoundaryRole::Outer
        );
        assert_eq!(loop_.coedges().len(), 1);
        let coedge = result
            .ir()
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.id == loop_.coedges()[0])
            .unwrap();
        assert_eq!(coedge.edge.as_str(), "iges:model:edge#bounded-plane-D1");
        assert!(!has_entity_projection_loss(&result), "{expected_version}");
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(
            validation.is_ok(),
            "{expected_version}: {:#?}",
            validation.findings
        );
    }
}

#[test]
fn bounded_plane_requires_a_closed_boundary_curve() {
    let result = decode(bounded_plane_entity_file(
        GLOBAL_V5_0,
        110,
        "110,0,0,0,1,0,0;",
    ));

    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "iges:model:surface#D1"));
    assert!(result.ir().model.faces.is_empty());
    assert!(has_entity_projection_loss(&result));
}

#[test]
fn bounded_plane_requires_the_boundary_curve_to_lie_in_the_plane() {
    let result = decode(bounded_plane_entity_file(
        GLOBAL_V5_0,
        100,
        "100,1,0,0,1,0,1,0;",
    ));

    assert!(result.ir().model.faces.is_empty());
    assert!(has_entity_projection_loss(&result));
}

#[test]
fn bounded_plane_accepts_a_simple_piecewise_linear_nurbs_boundary() {
    let result = decode(bounded_plane_entity_file(
        GLOBAL_V5_0,
        126,
        "126,4,1,1,1,1,0,0,0,1,2,3,4,4,1,1,1,1,1,0,0,0,1,0,0,1,1,0,0,1,0,0,0,0,0,4,0,0,1;",
    ));

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#bounded-plane-D1")
        .expect("piecewise-linear NURBS boundary face");
    assert_eq!(face.loops.len(), 1);
    assert!(
        !has_entity_projection_loss(&result),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn bounded_plane_rejects_a_self_intersecting_piecewise_linear_nurbs_boundary() {
    let result = decode(bounded_plane_entity_file(
        GLOBAL_V5_0,
        126,
        "126,4,1,1,1,1,0,0,0,1,2,3,4,4,1,1,1,1,1,0,0,0,1,1,0,0,1,0,1,0,0,0,0,0,0,4,0,0,1;",
    ));

    assert!(result.ir().model.faces.is_empty());
    assert!(has_entity_projection_loss(&result));
}

#[test]
fn bounded_plane_rejects_a_discontinuous_piecewise_linear_nurbs_boundary() {
    let result = decode(bounded_plane_entity_file(
        GLOBAL_V5_0,
        126,
        "126,4,1,1,1,1,0,0,0,1,1,1,2,2,1,1,1,1,1,0,0,0,1,0,0,1,1,0,0,1,0,0,0,0,0,2,0,0,1;",
    ));

    assert!(result.ir().model.faces.is_empty());
    assert!(has_entity_projection_loss(&result));
}

#[test]
fn bounded_plane_accepts_a_simple_composite_line_boundary() {
    let result = decode(owned_test_file_with_global_and_line_fonts(
        &[
            OwnedTestEntity {
                entity_type: 108,
                form: 1,
                label: "PLANE".into(),
                status: "00010000",
                parameters: "108,0,0,1,0,3,0,0,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 102,
                form: 0,
                label: "BOUNDARY".into(),
                status: "00010000",
                parameters: "102,4,5,7,9,11;".into(),
            },
            OwnedTestEntity {
                entity_type: 110,
                form: 0,
                label: "EDGE0".into(),
                status: "00010000",
                parameters: "110,0,0,0,1,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 110,
                form: 0,
                label: "EDGE1".into(),
                status: "00010000",
                parameters: "110,1,0,0,1,1,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 110,
                form: 0,
                label: "EDGE2".into(),
                status: "00010000",
                parameters: "110,1,1,0,0,1,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 110,
                form: 0,
                label: "EDGE3".into(),
                status: "00010000",
                parameters: "110,0,1,0,0,0,0;".into(),
            },
        ],
        GLOBAL_V5_0,
        &[(1, 1), (3, 1), (5, 1), (7, 1), (9, 1), (11, 1)],
    ));

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#bounded-plane-D1")
        .expect("composite line boundary face");
    assert_eq!(face.loops.len(), 1);
    assert!(
        !has_entity_projection_loss(&result),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn bounded_plane_refuses_recursive_child_curve_identity_copy() {
    let child = CurveId::mint("test:model:curve#child").unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: child.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        )),
        source_object: None,
    });
    let geometry = SolvedCurveGeometry::Composite {
        segments: vec![CompositeCurveSegment {
            curve: child,
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        }]
        .try_into()
        .unwrap(),
        self_intersect: Some(false),
    };
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let context = super::super::PlaneBoundarySimplicity {
        index: &index,
        plane: (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        resolution: 0.001,
        transform: Transform::identity(),
        ctx: &ctx,
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges plane boundary child curve ID",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            crate::test_support::with_policy_context(&[], &policy, |ctx| {
                let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
                super::super::bounded_plane_curve_is_simple(
                    &geometry,
                    super::super::PlaneBoundarySimplicity {
                        index: &index,
                        ctx,
                        ..context
                    },
                    false,
                    None,
                    &mut BTreeSet::new(),
                )
            })
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "iges plane boundary child curve ID")
    );

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let context = super::super::PlaneBoundarySimplicity {
        ctx: &ctx,
        ..context
    };
    assert!(!super::super::bounded_plane_curve_is_simple(
        &geometry,
        context,
        false,
        None,
        &mut BTreeSet::new(),
    )
    .unwrap());
}

#[test]
fn bounded_plane_requires_a_resolvable_boundary_pointer() {
    let result = decode(
        crate::test_support::test_owned::owned_test_file_with_global_and_line_fonts(
            &[OwnedTestEntity {
                entity_type: 108,
                form: 1,
                label: "PLANE".into(),
                status: "00010000",
                parameters: "108,0,0,1,0,99,0,0,0,0;".into(),
            }],
            GLOBAL_V5_0,
            &[(1, 1)],
        ),
    );

    assert!(result.ir().model.surfaces.is_empty());
    assert!(result.ir().model.faces.is_empty());
    assert!(has_entity_projection_loss(&result));
}

#[test]
fn negative_bounded_plane_without_an_owner_is_not_invented_as_a_face() {
    let result = decode(
        crate::test_support::test_owned::owned_test_file_with_global_and_line_fonts(
            &[
                OwnedTestEntity {
                    entity_type: 108,
                    form: -1,
                    label: "NEGPLANE".into(),
                    status: "00010000",
                    parameters: "108,0,0,1,0,3,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 100,
                    form: 0,
                    label: "BOUNDARY".into(),
                    status: "00010000",
                    parameters: "100,0,0,0,1,0,1,0;".into(),
                },
            ],
            GLOBAL_V5_0,
            &[(1, 1), (3, 1)],
        ),
    );

    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "iges:model:surface#D1"));
    assert!(result.ir().model.faces.is_empty());
    assert!(has_entity_projection_loss(&result));
}

#[test]
fn bounded_plane_linear_nurbs_proofs_propagate_work_refusals() {
    let bytes = bounded_plane_entity_file(
        GLOBAL_V5_0,
        126,
        "126,4,1,1,1,1,0,0,0,1,2,3,4,4,1,1,1,1,1,0,0,0,1,0,0,1,1,0,0,1,0,0,0,0,0,4,0,0,1;",
    );
    for operation in [
        "iges closed polyline duplicate comparisons",
        "iges planar self-intersection comparisons",
    ] {
        super::assert_structure_refusal(&bytes, operation, ResourceDimension::WorkUnits);
    }
}

#[test]
fn bounded_plane_polyline_proofs_propagate_work_refusals() {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples};
    let points = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
    ];
    let geometry = SolvedCurveGeometry::Polyline(
        PolylineCurve::new(
            PolylineSamples::Unparameterized {
                points: points.try_into().unwrap(),
            },
            0.0,
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("polyline construction admission")
        .unwrap(),
    );
    let ir = CadIr::empty();
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    for operation in [
        "iges closed polyline duplicate comparisons",
        "iges planar self-intersection comparisons",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    let context = super::super::PlaneBoundarySimplicity {
                        index: &index,
                        plane: (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
                        resolution: 0.001,
                        transform: Transform::identity(),
                        ctx,
                    };
                    super::super::bounded_plane_curve_is_simple(
                        &geometry,
                        context,
                        false,
                        None,
                        &mut BTreeSet::new(),
                    )
                })
            },
        );
    }
}

#[test]
fn plane_nurbs_weight_validation_charges_only_rational_poles() {
    for weights in [None, Some(vec![2.0; 5])] {
        let operation = if weights.is_some() {
            "iges plane NURBS weights"
        } else {
            "iges plane NURBS knot validation"
        };
        let nurbs = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            weights,
            false,
        )
        .expect("fixture constructor admission")
        .unwrap();
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    super::super::linear_nurbs_boundary_points(
                        &nurbs,
                        [0.0, 4.0],
                        Transform::identity(),
                        ctx,
                    )
                })
            },
        );
        let points = crate::test_support::with_service_context(&[], |ctx| {
            super::super::linear_nurbs_boundary_points(
                &nurbs,
                [0.0, 4.0],
                Transform::identity(),
                ctx,
            )
        })
        .unwrap()
        .unwrap();
        assert_eq!(points.len(), 5);
        assert_eq!(points[0], points[4]);
    }
}

#[test]
fn repeated_plane_boundaries_reuse_the_complete_geometric_proof() {
    let bytes = bounded_plane_entity_file(GLOBAL_V5_0, 100, "100,0,0,0,1,0,1,0;");
    let decoded = decode(bytes.clone());
    let scan = crate::test_support::scan(&bytes).unwrap();
    let (directory, quarantined) = crate::test_support::with_service_context(&bytes, |ctx| {
        crate::directory::parse(&scan, crate::global::GlobalTable::V5_0, ctx)
    })
    .unwrap();
    assert!(quarantined.is_empty());
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    let directory_entries =
        u64::try_from(directory.len()).expect("directory length fits work units");
    let directory_search_work = directory_entries
        .checked_mul(
            u64::try_from(std::mem::size_of::<u32>()).expect("u32 size fits work units"),
        )
        .and_then(|work| work.checked_mul(3))
        .expect("three directory searches fit work units");
    let mut policy = DecodePolicy::service();
    // The former 10,000-unit cap covers the fixture's directory/index and
    // geometric proof work. The 20,000 loop calls are bounded by one-key
    // 60-byte cache searches; the two later misses cost 60 and 120. Each of
    // the three proof misses checks one u32 directory key, with at most one
    // comparison per directory entry.
    policy.limits.max_work_units = 10_000 + 20_000 * 60 + 60 + 2 * 60 + directory_search_work;
    crate::test_support::with_policy_context(&bytes, &policy, |ctx| {
        let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
        let plane = super::super::plane_carrier(&index, 1, ctx)
            .unwrap()
            .unwrap();
        let mut proofs = super::super::PlaneBoundaryProofs {
            proven: std::collections::BTreeMap::new(),
            storage: ctx
                .reserve_scoped(0, "iges plane boundary proof cache")
                .unwrap(),
        };
        for _ in 0..20_000 {
            assert!(super::super::plane_boundary_edge(
                &index,
                plane,
                3,
                &entries,
                0.001,
                ctx,
                &mut proofs
            )
            .is_ok());
        }
        assert_eq!(proofs.proven.len(), 1);
        assert!(super::super::plane_boundary_edge(
            &index,
            plane,
            3,
            &entries,
            0.002,
            ctx,
            &mut proofs
        )
        .is_ok());
        assert_eq!(proofs.proven.len(), 2);
        let shifted = (Point3::new(0.0, 0.0, 1.0), plane.1);
        assert!(matches!(
            super::super::plane_boundary_edge(
                &index,
                shifted,
                3,
                &entries,
                0.001,
                ctx,
                &mut proofs
            ),
            Err(super::super::PlaneBoundaryError::NotCoplanar)
        ));
        assert_eq!(proofs.proven.len(), 2);
    });
    let run_cache_lookup = |work_cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work_cap;
        crate::test_support::with_policy_context(&bytes, &policy, |ctx| {
            let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
            let plane = super::super::plane_carrier(&index, 1, ctx)?.unwrap();
            let mut proofs = super::super::PlaneBoundaryProofs {
                proven: std::collections::BTreeMap::new(),
                storage: ctx.reserve_scoped(0, "iges plane boundary proof cache")?,
            };
            super::super::plane_boundary_edge(
                &index,
                plane,
                3,
                &entries,
                0.001,
                ctx,
                &mut proofs,
            )
            .map(|_| ())
            .map_err(|error| {
                error
                    .message()
                    .expect_err("initial plane boundary proof must succeed")
            })?;
            super::super::plane_boundary_edge(
                &index,
                plane,
                3,
                &entries,
                0.001,
                ctx,
                &mut proofs,
            )
            .map(|_| ())
            .map_err(|error| {
                error
                    .message()
                    .expect_err("plane boundary proof cache lookup is the tested boundary")
            })
        })
    };
    let operation = "iges plane boundary proof cache lookup";
    let probed = {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(60));
        run_cache_lookup(u64::MAX)
    };
    let limit = match probed {
        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => limit,
        Err(error) => panic!("unexpected refusal while probing {operation}: {error:?}"),
        Ok(()) => panic!("missing proof-cache lookup work charge"),
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 60);
    let need = limit
        .used
        .checked_add(limit.additional)
        .expect("proof-cache lookup work need fits");
    assert!(matches!(
        run_cache_lookup(need - 1),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref replay))
            if replay.dimension == ResourceDimension::WorkUnits
                && replay.operation == operation
                && replay.used + replay.additional == need
    ));
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges plane boundary proof cache",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            crate::test_support::with_policy_context(&bytes, &policy, |ctx| {
                let index = ModelIndex::build(decoded.ir(), cadmpeg_ir::index::StandardIndex);
                let plane = super::super::plane_carrier(&index, 1, ctx)?.unwrap();
                let mut proofs = super::super::PlaneBoundaryProofs {
                    proven: std::collections::BTreeMap::new(),
                    storage: ctx.reserve_scoped(0, "iges plane boundary proof cache")?,
                };
                super::super::plane_boundary_edge(
                    &index,
                    plane,
                    3,
                    &entries,
                    0.001,
                    ctx,
                    &mut proofs,
                )
                .map(|_| ())
                .map_err(|error| {
                    error
                        .message()
                        .expect_err("expected cache resource refusal")
                })
            })
        },
    );
}

#[test]
fn plane_nurbs_scalar_rejections_do_not_visit_lanes() {
    for (degree, knots, range) in [
        (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], [0.0, 1.0]),
        (1, vec![0.0, 0.0, 1.0, 2.0, 2.0], [0.0, 3.0]),
    ] {
        let nurbs = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            degree,
            knots,
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            Some(vec![1.0, 1.0, 1.0]),
            false,
        )
        .unwrap()
        .unwrap();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(super::super::linear_nurbs_boundary_points(
            &nurbs,
            range,
            Transform::identity(),
            &ctx
        )
        .unwrap()
        .is_none());
    }
}

mod recursive_storage;
