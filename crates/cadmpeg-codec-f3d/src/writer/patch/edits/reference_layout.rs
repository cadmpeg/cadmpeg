// SPDX-License-Identifier: Apache-2.0
//! Semantic edits use actual baseline positions after a CADIR reload.

use super::*;

#[test]
fn reloaded_material_edit_keeps_actual_baseline_string_positions() {
    let mut before: DesignMaterialAssignment = serde_json::from_value(serde_json::json!({
        "id": "f3d:design:material-assignment#100",
        "body_binding_id": "f3d:design:design-body-binding#25",
        "asm_body_key": 42,
        "entity_id": "0_985",
        "visual_guid": "11111111-2222-3333-4444-555555555555",
        "physical_token": "Prism-002"
    }))
    .unwrap();
    before.entity_suffix_offset = Some(100);
    before.entity_id_offset = Some(110);
    before.visual_guid_offset = Some(130);
    before.physical_token.as_mut().unwrap().offset = Some(210);
    let mut after: DesignMaterialAssignment =
        serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
    after.physical_token.as_mut().unwrap().value = "Prism-003".into();
    let baseline = F3dNative {
        design_material_assignments: vec![before],
        ..Default::default()
    };
    let target = F3dNative {
        design_material_assignments: vec![after],
        ..Default::default()
    };
    let edits = validate_material_assignment_edits(PatchNatives {
        baseline: Some(&baseline),
        target: Some(&target),
    })
    .unwrap();
    let actual = &edits.values().next().unwrap()[0];
    assert_eq!(actual.entity_suffix_offset, Some(100));
    assert_eq!(actual.entity_id_offset, Some(110));
    assert_eq!(actual.visual_guid_offset, Some(130));
    let token = actual.physical_token.as_ref().unwrap();
    assert_eq!(token.offset, Some(210));
    assert_eq!(token.value, "Prism-003");
}

#[test]
fn reloaded_lost_edge_edit_keeps_actual_baseline_header_position() {
    let before = LostEdgeReference::new(
        "f3d:design:lost-edge-reference#100".into(),
        100,
        "419".into(),
        7,
        "420".into(),
        8,
    )
    .unwrap();
    let mut after: LostEdgeReference =
        serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
    after.record_index = 9;
    let baseline = F3dNative {
        lost_edge_references: vec![before],
        ..Default::default()
    };
    let target = F3dNative {
        lost_edge_references: vec![after],
        ..Default::default()
    };
    let edits = validate_lost_edge_edits(PatchNatives {
        baseline: Some(&baseline),
        target: Some(&target),
    })
    .unwrap();
    let actual = &edits.values().next().unwrap()[0];
    assert_eq!(actual.record_byte_offset(), Some(100));
    assert_eq!(actual.record_index, 9);
}

#[test]
fn persistent_value_edit_uses_baseline_position_and_declines_when_absent() {
    let before = crate::records::references::PersistentReference {
        id: "f3d:design:persistent-reference#100".into(),
        byte_offset: Some(100),
        value_offset: Some(32),
        kind: crate::records::references::PersistentReferenceKind::Point,
        value: 42,
    };
    let mut after: crate::records::references::PersistentReference =
        serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
    after.value = 43;
    let mut baseline = F3dNative {
        persistent_references: vec![before],
        ..Default::default()
    };
    let target = F3dNative {
        persistent_references: vec![after],
        ..Default::default()
    };
    let edits = validate_persistent_reference_edits(PatchNatives {
        baseline: Some(&baseline),
        target: Some(&target),
    })
    .unwrap();
    let actual = &edits.values().next().unwrap()[0];
    assert_eq!(actual.offset, 100);
    assert_eq!(actual.identity_offset, 32);
    assert_eq!(actual.identity, 43);
    baseline.persistent_references[0].byte_offset = None;
    assert!(matches!(
        validate_persistent_reference_edits(PatchNatives {
            baseline: Some(&baseline),
            target: Some(&target),
        }),
        Err(CodecError::UnsupportedContent(_))
    ));
}

#[test]
fn reloaded_sketch_curve_edit_requires_actual_baseline_geometry_position() {
    use crate::records::sketch_geometry::SketchCurveIdentity;
    let line = |end| {
        SketchCurveGeometry::line(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(end, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        )
        .unwrap()
    };
    let before = SketchCurveIdentity {
        id: "f3d:Design/BulkStream.dat:sketch-curve-identity#100".into(),
        record_index: 7,
        owner_reference: Some(42),
        class_tag: "259".to_owned().try_into().unwrap(),
        byte_offset: Some(100),
        geometry_offset: Some(133),
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(10).unwrap(),
        secondary_id: 11,
        geometry: Some(line(10.0)),
        planar_z_noise: None,
    };
    let mut after: SketchCurveIdentity =
        serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
    after.geometry = Some(line(20.0));
    let mut baseline = F3dNative {
        sketch_curve_identities: vec![before],
        ..Default::default()
    };
    let target = F3dNative {
        sketch_curve_identities: vec![after],
        ..Default::default()
    };
    let edits = validate_sketch_curve_edits(PatchNatives {
        baseline: Some(&baseline),
        target: Some(&target),
    })
    .unwrap();
    let actual = &edits.values().next().unwrap()[0];
    assert_eq!(actual.offset, 100);
    assert_eq!(actual.geometry_offset, 133);
    assert_eq!(actual.geometry, line(20.0));
    baseline.sketch_curve_identities[0].geometry_offset = None;
    assert!(matches!(
        validate_sketch_curve_edits(PatchNatives {
            baseline: Some(&baseline),
            target: Some(&target),
        }),
        Err(CodecError::UnsupportedContent(_))
    ));
}
