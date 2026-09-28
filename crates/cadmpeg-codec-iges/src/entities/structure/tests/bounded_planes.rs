// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::loss::IgesLossCode;
use crate::test_support::decode;
use crate::test_support::test_owned::{
    owned_test_file_with_global_and_line_fonts, OwnedTestEntity,
};
use crate::test_support::test_surface_fixtures::bounded_plane_entity_file;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{
    analytic::LineCurve, CompositeCurveSegment, CompositeCurveTransition,
    Curve, CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::index::ModelIndex;
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
    let mut cap = 0_u64;
    let mut reached = false;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        match crate::IgesCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions { policy, ..DecodeOptions::default() }) {
            Err(cadmpeg_ir::codec::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                if limit.operation == "iges bounded plane boundary edges" {
                    reached = true;
                    break;
                }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            _ => panic!("expected bounded plane boundary-edge refusal"),
        }
    }
    assert!(reached, "bounded plane boundary-edge slot was not reached");
}

#[test]
fn plane_nurbs_boundary_points_refuse_collection_limit() {
    let nurbs = NurbsCurve::from_lanes(
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
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::linear_nurbs_boundary_points(&nurbs, [0.0, 4.0], &ctx);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 5
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let points = super::super::linear_nurbs_boundary_points(&nurbs, [0.0, 4.0], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(points.len(), 5);
    assert_eq!(points[0], points[4]);
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
        }].try_into().unwrap(),
        self_intersect: Some(false),
    };
    let index = ModelIndex::new(&ir);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let context = super::super::PlaneBoundarySimplicity {
        index: &index,
        plane: (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        resolution: 0.001,
        transform: Transform::identity(),
        ctx: &ctx,
    };
    let result = super::super::bounded_plane_curve_is_simple(
        &geometry, context, false, None, &mut BTreeSet::new(),
    );
    assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "iges plane boundary child curve ID"));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let context = super::super::PlaneBoundarySimplicity { ctx: &ctx, ..context };
    assert!(!super::super::bounded_plane_curve_is_simple(
        &geometry, context, false, None, &mut BTreeSet::new(),
    ).unwrap());
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
