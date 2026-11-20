// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs, ProjectedParameterDesign,
};
use crate::design::test_support::{indexed_record_offsets_for_test, with_test_decode_context};
use crate::loss::F3dLossCode;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::test_support::lp_utf16;
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#77";
const GROUP_ID: &str = "f3d:Design/BulkStream.dat:group#100";

#[derive(Clone, Copy)]
enum Prologue {
    Compact,
    Marked,
    Extended,
}

fn marked_reference(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

fn unstitch_scope(prologue: Prologue, members: &[u32], sources: &[u32], label: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"396");
    bytes.extend_from_slice(&77u32.to_le_bytes());
    let flag = u8::from(!sources.is_empty());
    match prologue {
        Prologue::Compact => {
            bytes.extend_from_slice(&[0; 10]);
            bytes.push(flag);
        }
        Prologue::Marked | Prologue::Extended => {
            bytes.extend_from_slice(&[0; 9]);
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&[0, flag]);
            if matches!(prologue, Prologue::Extended) {
                bytes.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }
    bytes.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
    for ordinal in 0..u32::try_from(members.len()).unwrap() {
        marked_reference(&mut bytes, 300 + ordinal);
    }
    bytes.extend_from_slice(&u32::try_from(sources.len()).unwrap().to_le_bytes());
    for source in sources {
        marked_reference(&mut bytes, *source);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    marked_reference(&mut bytes, 100);
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&u32::try_from(members.len() + 1).unwrap().to_le_bytes());
    marked_reference(&mut bytes, 100);
    for member in members {
        marked_reference(&mut bytes, *member);
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "UnStitch");
    if label.is_empty() {
        let mut tail = [0; 76];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[30..34].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
    } else {
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        lp_utf16(&mut bytes, label);
        bytes.extend_from_slice(&[0; 7]);
        bytes.extend_from_slice(&[1, 90]);
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&257u32.to_le_bytes());
        bytes.extend_from_slice(&0.25f64.to_le_bytes());
        bytes.extend_from_slice(&257u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 89]);
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&[0, 1, 0, 0, 1, 88]);
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 3]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&77u32.to_le_bytes());
    bytes
}

fn face_group(members: &[u32]) -> DesignConstructionOperandGroup {
    let member_offsets = (0..members.len())
        .map(|ordinal| 1026 + 11 * ordinal)
        .collect::<Vec<_>>();
    serde_json::from_value(serde_json::json!({
        "id": GROUP_ID,
        "scope_record_index": 77,
        "scope_reference_ordinal": 0,
        "record_index": 100,
        "byte_offset": 1000,
        "class_tag": "300",
        "members": members,
        "member_offsets": member_offsets,
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": 68_719_476_736_u64,
        "role_offset": 1054,
        "paired_class_tag": "258",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn project(bytes: &[u8], groups: &[DesignConstructionOperandGroup]) -> ProjectedParameterDesign {
    let mut scope = parse_parameter_scope(
        bytes,
        &indexed_record_offsets_for_test(bytes),
        77,
        &"396".to_owned().try_into().unwrap(),
        0,
    )
    .unwrap()
    .unwrap();
    scope.id = SCOPE_ID.into();
    let scopes = [scope];
    let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
    with_test_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes: &scopes,
                timelines: &timelines,
                construction_groups: groups,
                ..Default::default()
            },
        )
        .unwrap()
    })
}

fn loss_count(projected: &ProjectedParameterDesign, code: F3dLossCode) -> usize {
    projected
        .projection_losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

fn assert_unstitch(projected: &ProjectedParameterDesign, source: &str) {
    assert_eq!(projected.features.len(), 1);
    assert!(matches!(
        projected.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::UnstitchFaces {
            faces: FaceSelection::Native(native),
        }) if native == source
    ));
    assert_eq!(
        loss_count(projected, F3dLossCode::UnstitchPrologueControlsAssumed),
        1
    );
}

#[test]
fn every_unstitch_prologue_form_projects_the_face_group() {
    for prologue in [Prologue::Compact, Prologue::Marked, Prologue::Extended] {
        for (members, sources) in [
            (&[200u32][..], &[][..]),
            (&[200, 201, 202, 203][..], &[400u32][..]),
            (&[200, 201, 202, 203, 204, 205][..], &[400, 401][..]),
        ] {
            let bytes = unstitch_scope(prologue, members, sources, "");
            let projected = project(&bytes, &[face_group(members)]);
            assert_unstitch(&projected, GROUP_ID);
            assert_eq!(loss_count(&projected, F3dLossCode::UnstitchFrameAssumed), 0);
            assert_eq!(
                loss_count(&projected, F3dLossCode::UnstitchGroupSelectionAssumed),
                0
            );
        }
    }
}

#[test]
fn a_named_unstitch_keeps_its_label_and_face_group() {
    for label in ["Separate faces", "Face \u{1f30a}"] {
        let bytes = unstitch_scope(Prologue::Extended, &[200], &[], label);
        let projected = project(&bytes, &[face_group(&[200])]);
        assert_unstitch(&projected, GROUP_ID);
        assert_eq!(projected.features[0].name.as_deref(), Some(label));
        assert_eq!(loss_count(&projected, F3dLossCode::UnstitchFrameAssumed), 0);
    }
}

#[test]
fn an_unstitch_frame_outside_the_prologue_forms_charges_the_frame_assumption() {
    let mut bytes = unstitch_scope(Prologue::Compact, &[200], &[], "");
    bytes.splice(11..11, [0; 2]);
    let projected = project(&bytes, &[face_group(&[200])]);
    assert_unstitch(&projected, GROUP_ID);
    assert_eq!(loss_count(&projected, F3dLossCode::UnstitchFrameAssumed), 1);
}

#[test]
fn an_unstitch_without_a_consistent_face_group_retains_its_scope_references() {
    let bytes = unstitch_scope(Prologue::Marked, &[200, 201], &[400], "");
    for groups in [Vec::new(), vec![face_group(&[201, 200])]] {
        let projected = project(&bytes, &groups);
        assert_unstitch(&projected, SCOPE_ID);
        assert_eq!(
            loss_count(&projected, F3dLossCode::UnstitchGroupSelectionAssumed),
            1
        );
        assert_eq!(loss_count(&projected, F3dLossCode::UnstitchFrameAssumed), 0);
    }
}
