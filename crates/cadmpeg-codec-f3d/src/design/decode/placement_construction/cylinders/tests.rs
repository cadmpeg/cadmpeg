// SPDX-License-Identifier: Apache-2.0

use crate::loss::F3dLossCode;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::placement_construction::DesignPlacementConstruction;

fn placement() -> DesignPlacementConstruction {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"317", 255);
    bytes.extend_from_slice(&[0; 10]);
    for value in [0.0_f64; 3] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 10]);
    let rows = [
        [1.0_f64, 0.0, 0.0, 0.1],
        [0.0, 0.0, 1.0, 0.2],
        [0.0, -1.0, 0.0, 0.3],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for value in rows.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::placement_construction::decode_frame(
            ctx,
            "f3d:Design/BulkStream.dat:design-placement-construction#0".into(),
            cadmpeg_core::decode::View::over_retained(&bytes),
            9,
        )
        .unwrap()
        .unwrap()
    })
}

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::CylinderPrimitive,
        1,
    );
    let primitive = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
        "height": 2.6, "height_record_index": 259,
        "height_offset": 1000, "diameter": 0.5, "diameter_record_index": 262,
        "diameter_offset": 1100, "operation": "join", "operation_offset": 20,
    }))
    .unwrap();
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![251, 255, 259, 262, 264]);
            draft.payload =
                crate::records::feature::primitives::DesignSolidPrimitive::Cylinder(primitive)
                    .into();
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn named_cylinder_binds_decoded_version_nine_frame() {
    let placement = placement();
    let expected = placement.fields.matrix.unwrap();
    let mut scope = scope();
    assert!(super::bind(std::slice::from_mut(&mut scope), &[placement]).is_empty());
    let crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(cylinder)) =
        scope.payload()
    else {
        panic!("expected cylinder");
    };
    assert_eq!(cylinder.transform, Some(expected));
    assert_eq!(
        expected.value.rows(),
        [
            [1.0, 0.0, 0.0, 0.1],
            [0.0, 0.0, 1.0, 0.2],
            [0.0, -1.0, 0.0, 0.3],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    let restored: DesignParameterScope = cadmpeg_core::serde_erase::json::from_value(
        cadmpeg_core::serde_erase::json::to_value(&scope).unwrap(),
    )
    .unwrap();
    assert_eq!(restored, scope);
    assert!(super::bind(std::slice::from_mut(&mut scope), &[]).is_empty());
    assert_eq!(scope, restored);
}

#[test]
fn named_cylinder_reports_unbound_placement() {
    let placement = placement();
    let mut foreign = placement.clone();
    foreign.id = "f3d:Other/BulkStream.dat:design-placement-construction#0".into();
    let mut flipped = placement.clone();
    flipped.fields.axis_flipped.as_mut().unwrap().value = 1;
    let mut other_version = placement.clone();
    other_version.type_version = 7;
    let mut inferred_x = placement.clone();
    inferred_x.fields.inferred_x_axis.value = 1;
    let mut inferred_z = placement.clone();
    inferred_z.fields.inferred_z_axis.value = 1;
    for placements in [
        vec![],
        vec![foreign],
        vec![placement.clone(), placement.clone()],
        vec![flipped],
        vec![other_version],
        vec![inferred_x],
        vec![inferred_z],
    ] {
        let mut scope = scope();
        let losses = super::bind(std::slice::from_mut(&mut scope), &placements);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::CylinderPlacementUnbound
                .note("")
                .code
        );
        let crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(cylinder)) =
            scope.payload()
        else {
            panic!("expected cylinder");
        };
        assert!(cylinder.transform.is_none());
    }
}

#[test]
fn named_cylinder_binds_version_eight_solved_frame() {
    let mut placement = crate::test_support::placement_construction::grammar_placement(8, 0);
    placement.record_index = 255;
    let expected = placement.fields.matrix;
    let mut scope = scope();
    assert!(super::bind(std::slice::from_mut(&mut scope), &[placement]).is_empty());
    let crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(cylinder)) =
        scope.payload()
    else {
        panic!("expected cylinder");
    };
    assert_eq!(cylinder.transform, expected);
    assert_eq!(
        cylinder.transform.unwrap().value.rows(),
        crate::test_support::placement_construction::GRAMMAR_FRAME_ROWS
    );
}

#[test]
fn named_cylinder_version_eight_reflection_keeps_dimensions_unplaced() {
    let mut placement = placement();
    placement.type_version = 8;
    let mut matrix = placement.fields.matrix.unwrap();
    let mut rows = matrix.value.rows();
    for row in &mut rows {
        row[0] = -row[0];
    }
    matrix.value = rows.try_into().unwrap();
    placement.fields.matrix = Some(matrix);
    let mut scope = scope();
    assert_eq!(
        super::bind(std::slice::from_mut(&mut scope), &[placement]).len(),
        1
    );
    let crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(cylinder)) =
        scope.payload()
    else {
        panic!("expected cylinder");
    };
    assert!(cylinder.transform.is_none());
    assert_eq!(cylinder.height.get(), 2.6);
    assert_eq!(cylinder.diameter.get(), 0.5);
}

#[test]
fn compact_extended_cylinder_reports_the_support_frame_assumption() {
    let support = placement();
    let expected = support.fields.matrix;
    let mut scope = scope();
    let DesignScopePayloadMut::CylinderPrimitive(Some(cylinder)) = scope.payload_mut() else {
        panic!("expected cylinder");
    };
    cylinder.construction = Some(
        crate::records::feature::primitives::DesignCylinderConstruction {
            placement_record_index: u32::try_from(support.record_index).unwrap(),
            reference: crate::records::identity::Located {
                value: 40,
                offset: 80,
            },
            local_frame: None,
        },
    );

    let losses = super::bind(std::slice::from_mut(&mut scope), &[support]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::CylinderLocalFrameAssumed.note("").code
    );
    let DesignScopePayloadMut::CylinderPrimitive(Some(cylinder)) = scope.payload_mut() else {
        panic!("expected cylinder");
    };
    assert_eq!(cylinder.transform, expected);
}

#[test]
fn extended_cylinder_composes_the_explicit_same_stream_support_on_the_left() {
    let mut support = placement();
    support.type_version = 6;
    let selected = u32::try_from(support.record_index).unwrap();
    let mut decoy = support.clone();
    decoy.record_index = 251;
    let mut foreign = support.clone();
    foreign.id = "f3d:Other/BulkStream.dat:design-placement-construction#0".into();
    let local = crate::records::identity::Located {
        value: [
            [0.0, -1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 0.0, 1.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
        offset: 76,
    };
    let mut scope = scope();
    let DesignScopePayloadMut::CylinderPrimitive(Some(cylinder)) = scope.payload_mut() else {
        panic!("expected cylinder");
    };
    cylinder.construction = Some(
        crate::records::feature::primitives::DesignCylinderConstruction {
            placement_record_index: selected,
            reference: crate::records::identity::Located {
                value: 40,
                offset: 212,
            },
            local_frame: Some(local),
        },
    );
    let mut ambiguous = scope.clone();
    let losses = super::bind(
        std::slice::from_mut(&mut ambiguous),
        &[support.clone(), support.clone()],
    );
    assert_eq!(
        losses[0].code,
        F3dLossCode::CylinderPlacementUnbound.note("").code
    );
    let losses = super::bind(std::slice::from_mut(&mut scope), &[decoy, foreign, support]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::CylinderLocalFrameAssumed.note("").code
    );
    let DesignScopePayloadMut::CylinderPrimitive(Some(cylinder)) = scope.payload_mut() else {
        panic!("expected cylinder");
    };
    assert_eq!(cylinder.construction.unwrap().local_frame, Some(local));
    assert_eq!(
        cylinder.transform.unwrap().value.rows(),
        [
            [0.0, -1.0, 0.0, 1.1],
            [0.0, 0.0, 1.0, 3.2],
            [-1.0, 0.0, 0.0, -1.7],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
    let restored: DesignParameterScope = cadmpeg_core::serde_erase::json::from_value(
        cadmpeg_core::serde_erase::json::to_value(&scope).unwrap(),
    )
    .unwrap();
    assert_eq!(restored, scope);
}

#[test]
fn named_cylinder_binds_version_sixteen_frames() {
    let placements = crate::test_support::placement_construction::modern_placements();
    for mut placement in placements
        .into_iter()
        .filter(|placement| placement.type_version == 16)
    {
        placement.record_index = 255;
        let expected = super::super::checked_solved_frame(&placement).unwrap();
        let mut scope = scope();
        assert!(super::bind(std::slice::from_mut(&mut scope), &[placement]).is_empty());
        let crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(cylinder)) =
            scope.payload()
        else {
            panic!("expected cylinder");
        };
        assert_eq!(cylinder.transform, Some(expected));
    }
}

#[test]
fn named_cylinder_binds_version_sixteen_identity_marker() {
    let mut placement = crate::test_support::placement_construction::modern_placements()
        .into_iter()
        .find(|placement| {
            placement.type_version == 16
                && placement.fields.matrix.is_none()
                && placement
                    .fields
                    .identity
                    .as_ref()
                    .is_some_and(|identity| identity.value)
        })
        .unwrap();
    placement.record_index = 255;
    let mut scope = scope();
    assert!(super::bind(std::slice::from_mut(&mut scope), &[placement]).is_empty());
    let crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(cylinder)) =
        scope.payload()
    else {
        panic!("expected cylinder");
    };
    assert_eq!(
        cylinder.transform.unwrap().value.rows(),
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
}
