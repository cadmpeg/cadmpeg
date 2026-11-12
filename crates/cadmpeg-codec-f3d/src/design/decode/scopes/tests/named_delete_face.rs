use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::design::test_support::{indexed_record_offsets_for_test, with_test_decode_context};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::test_support::lp_utf16;
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

fn named_delete_scope(kind: &str, label: &str) -> Vec<u8> {
    let mut bytes = vec![0; 127];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"361");
    bytes[7..11].copy_from_slice(&77u32.to_le_bytes());
    bytes[20] = 1;
    bytes[33..37].copy_from_slice(&1u32.to_le_bytes());
    bytes[37] = 1;
    bytes[38..42].copy_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [100u32, 200] {
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, kind);
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
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&77u32.to_le_bytes());
    bytes
}

fn face_group() -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:group#100",
        "scope_record_index": 77,
        "scope_reference_ordinal": 0,
        "record_index": 100,
        "byte_offset": 1000,
        "class_tag": "300",
        "members": [200],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": 68719476736u64,
        "role_offset": 1054,
        "paired_class_tag": "258",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

#[test]
fn named_delete_faces_project_with_their_healing_mode_and_display_label() {
    let group = face_group();
    for (kind, heal) in [("DeleteFace", true), ("SurfaceDeleteFace", false)] {
        for label in ["Remove selected faces", "Face \u{1f30a}"] {
            let bytes = named_delete_scope(kind, label);
            let mut scope = parse_parameter_scope(
                &bytes,
                &indexed_record_offsets_for_test(&bytes),
                77,
                &"361".to_owned().try_into().unwrap(),
                0,
            )
            .unwrap()
            .unwrap();
            scope.id = "f3d:Design/BulkStream.dat:scope#77".into();
            let scopes = [scope];
            let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
            let projected = with_test_decode_context(|ctx| {
                project_parameter_design_with_edge_identities(
                    ctx,
                    &ProjectInputs {
                        scopes: &scopes,
                        timelines: &timelines,
                        construction_groups: std::slice::from_ref(&group),
                        ..Default::default()
                    },
                )
                .unwrap()
            });
            assert_eq!(projected.features.len(), 1);
            assert_eq!(projected.features[0].name.as_deref(), Some(label));
            assert!(matches!(projected.features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::DeleteFace {
                    faces: FaceSelection::Native(native), heal: actual,
                }) if native == &group.id && *actual == heal));
        }
    }
}

#[test]
fn fixed_surface_delete_faces_project_and_charge_the_wide_prologue() {
    for count_offset in [123, 128, 134] {
        let named = named_delete_scope("SurfaceDeleteFace", "Unused");
        let mut bytes = named[..127].to_vec();
        bytes.resize(count_offset, 0);
        bytes.extend_from_slice(&named[127..195]);
        let mut tail = [0; 76];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[30..34].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"258");
        bytes.extend_from_slice(&77u32.to_le_bytes());
        let mut scope = parse_parameter_scope(
            &bytes,
            &indexed_record_offsets_for_test(&bytes),
            77,
            &"361".to_owned().try_into().unwrap(),
            0,
        )
        .unwrap()
        .unwrap();
        scope.id = "f3d:Design/BulkStream.dat:scope#77".into();
        let scopes = [scope];
        let groups = [face_group()];
        let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
        let projected = with_test_decode_context(|ctx| {
            project_parameter_design_with_edge_identities(
                ctx,
                &ProjectInputs {
                    scopes: &scopes,
                    timelines: &timelines,
                    construction_groups: &groups,
                    ..Default::default()
                },
            )
            .unwrap()
        });
        assert_eq!(projected.features.len(), 1);
        assert!(matches!(projected.features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DeleteFace {
                faces: FaceSelection::Native(native), heal: false,
            }) if native == &groups[0].id));
        assert_eq!(
            projected
                .projection_losses
                .iter()
                .filter(|loss| loss.code
                    == crate::loss::F3dLossCode::SurfaceDeleteFaceWidePrologueAssumed.kind())
                .count(),
            usize::from(count_offset == 128)
        );
    }
}
