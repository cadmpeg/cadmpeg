// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::{
    bounded_plane_entity_file, owned_test_file_with_global_and_line_fonts, OwnedTestEntity,
};
use crate::IgesCodec;

const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
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
            .find(|face| face.id.0 == "iges:model:face#bounded-plane-D1")
            .unwrap();
        assert_eq!(face.surface.0, "iges:model:surface#D1");
        assert_eq!(face.loops.len(), 1);
        let loop_ = result
            .ir()
            .model
            .loops
            .iter()
            .find(|loop_| loop_.id == face.loops[0])
            .unwrap();
        assert_eq!(
            loop_.boundary_role,
            cadmpeg_ir::topology::LoopBoundaryRole::Outer
        );
        assert_eq!(loop_.coedges.len(), 1);
        let coedge = result
            .ir()
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.id == loop_.coedges[0])
            .unwrap();
        assert_eq!(coedge.edge.0, "iges:model:edge#bounded-plane-D1");
        assert!(!has_entity_projection_loss(&result), "{expected_version}");
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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
        .any(|surface| surface.id.0 == "iges:model:surface#D1"));
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
        .find(|face| face.id.0 == "iges:model:face#bounded-plane-D1")
        .expect("piecewise-linear NURBS boundary face");
    assert_eq!(face.loops.len(), 1);
    assert!(
        !has_entity_projection_loss(&result),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
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
        .find(|face| face.id.0 == "iges:model:face#bounded-plane-D1")
        .expect("composite line boundary face");
    assert_eq!(face.loops.len(), 1);
    assert!(
        !has_entity_projection_loss(&result),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn bounded_plane_requires_a_resolvable_boundary_pointer() {
    let result = decode(
        crate::test_support::owned_test_file_with_global_and_line_fonts(
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
        crate::test_support::owned_test_file_with_global_and_line_fonts(
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
        .any(|surface| surface.id.0 == "iges:model:surface#D1"));
    assert!(result.ir().model.faces.is_empty());
    assert!(has_entity_projection_loss(&result));
}
