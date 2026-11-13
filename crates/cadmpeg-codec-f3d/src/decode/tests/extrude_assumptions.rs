// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use super::super::report_design_projection_gaps;
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
};
use crate::records::feature::scope::{
    DesignExtrudeScope, DesignParameterScope, DesignParameterScopeDraft, DesignScopePayload,
};
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::sketch_profile::{
    DesignSketchProfileOperand, DesignSketchProfileOperandDraft,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";
const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#12";
const FEATURE_ID: &str = "test:model:feature#extrude";

fn class_tag(tag: &str) -> DesignClassTag {
    DesignClassTag::try_from(tag.to_owned()).unwrap()
}

fn scope(extent: DesignExtrudeExtent, start: DesignExtrudeStart) -> DesignParameterScope {
    scope_with(extent, start, None, None)
}

fn scope_with(
    extent: DesignExtrudeExtent,
    start: DesignExtrudeStart,
    extrude_profile: Option<DesignSketchProfileOperand>,
    fixed_extrude_parameters: Option<DesignFixedExtrudeParameters>,
) -> DesignParameterScope {
    let (direction, sides) = match extent {
        DesignExtrudeExtent::OneSidedDistance => (1, [1, 0]),
        DesignExtrudeExtent::OneSidedToFace => (1, [2, 0]),
        DesignExtrudeExtent::TwoSidedToFaces => (2, [2, 0]),
        DesignExtrudeExtent::TwoSidedDistance => (2, [1, 1]),
        DesignExtrudeExtent::TwoSidedDistanceToFace
        | DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne => (2, [1, 2]),
        DesignExtrudeExtent::TwoSidedDistanceThroughAll => (2, [1, 4]),
        DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero => (2, [2, 1]),
        DesignExtrudeExtent::TwoSidedToTargetsOptionZero
        | DesignExtrudeExtent::TwoSidedToTargetsOptionTwo => (2, [2, 2]),
        DesignExtrudeExtent::SymmetricDistance => (3, [1, 0]),
        _ => panic!("extent outside these tests"),
    };
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: SCOPE_ID.into(),
            byte_offset: 100,
            class_tag: class_tag("301"),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            payload: DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                    reference: None,
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 128,
                    direction_face_extend_values: [direction, 2],
                    side_extent_discriminators: sides,
                    side_extent_discriminator_offsets: [177, 190],
                    first_side_target_ordinal: None,
                    distance_tail: None,
                    extent,
                    direction_face_extend_offsets: [132, 136],
                    direction_reversed: false,
                    direction_reversed_offset: 140,
                    solid_operation: true,
                    solid_operation_offset: 141,
                    start,
                    start_offset: 142,
                }),
                fixed_extrude_parameters,
                extrude_profile,
                ..Default::default()
            })),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            label: None,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100, 101],
                vec![185, 196],
                "reference_members",
            )
            .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            deleted_members: None,
            paired_class_tag: class_tag("261"),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

fn group(index: u32, role: DesignConstructionOperandRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:operand-group#{index}"),
        scope_record_index: 12,
        scope_reference_ordinal: 1,
        record_index: index,
        byte_offset: 1000,
        class_tag: class_tag("332"),
        members: vec![crate::records::identity::Located {
            value: index + 100,
            offset: 1026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1072,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1076,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: role,
        role_offset: 1054,
        paired_class_tag: class_tag("259"),
        paired_byte_offset: 1125,
    })
    .unwrap()
}

fn sketch_profile() -> DesignSketchProfileOperand {
    DesignSketchProfileOperand::try_new(DesignSketchProfileOperandDraft {
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 300,
        class_tag: class_tag("308"),
        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 330,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned()).unwrap(),
        entity_reference_offset: 420,
        compact_tail_flags: None,
        region_selection: None,
        paired_class_tag: class_tag("259"),
        paired_byte_offset: 520,
    })
    .unwrap()
}

fn parameter(
    record_index: u32,
    source_kind: &str,
    evaluated_value: f64,
) -> crate::records::parameters::DesignParameter {
    crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("{STREAM}:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: class_tag("305"),
            record_index,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                source_kind.into(),
                Some(record_index + 1),
                None,
            )
            .unwrap(),
            expression: "2 mm".into(),
            expression_offset: 40,
            source_kind_offset: 60,
            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 90,
            }),
            name: "d1".into(),
            name_offset: 100,
            evaluated_value,
            evaluated_value_offset: 110,
        },
    )
    .unwrap()
}

fn owner(parameter_record_index: u32) -> crate::records::parameters::DesignParameterOwner {
    crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: format!(
                "{STREAM}:design-parameter-owner#{}",
                parameter_record_index + 1
            ),
            byte_offset: 120,
            frame_length: 104,
            class_tag: class_tag("292"),
            record_index: parameter_record_index + 1,
            scope_record_index: 12,
            local_ordinal: 0,
            evaluated_value: 0.2,
            evaluated_value_offset: 160,
            parameter_record_index,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: parameter_record_index + 2,
        },
    )
    .unwrap()
}

fn definition(
    profile: &str,
    start: serde_json::Value,
    extent: serde_json::Value,
) -> cadmpeg_ir::features::FeatureDefinition {
    let mut value = serde_json::json!({
        "definition": "extrude",
        "profile": {"kind": "native", "value": profile},
        "direction": {"kind": "profile_normal"},
        "op": "new_body",
        "solid": true
    });
    value["start"] = start;
    value["extent"] = extent;
    serde_json::from_value(value).unwrap()
}

fn blind(length: f64) -> serde_json::Value {
    serde_json::json!({"termination": {"kind": "blind", "length": length}})
}

fn one_sided() -> serde_json::Value {
    serde_json::json!({"kind": "one_sided", "side": blind(2.0)})
}

fn two_sided() -> serde_json::Value {
    serde_json::json!({"kind": "two_sided", "first": blind(2.0), "second": blind(3.0)})
}

fn profile_plane() -> serde_json::Value {
    serde_json::json!({"kind": "profile_plane"})
}

fn notes(
    native: &F3dNative,
    definition: cadmpeg_ir::features::FeatureDefinition,
    code: F3dLossCode,
) -> Vec<String> {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint(FEATURE_ID).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(SCOPE_ID.into()),
    });
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        native,
    )
    .unwrap();
    report
        .losses
        .into_iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message)
        .collect()
}

fn native_with(scope: DesignParameterScope) -> F3dNative {
    let mut native = F3dNative::default();
    native.design_parameter_scopes.push(scope);
    native
}

#[test]
fn wall_thickness_owner_reports_its_native_inputs() {
    let code = F3dLossCode::FeatureExtrudeWallThicknessNative;
    let mut native = native_with(scope(
        DesignExtrudeExtent::OneSidedDistance,
        DesignExtrudeStart::ProfilePlane,
    ));
    native
        .design_parameters
        .push(parameter(40, "Side1WallThickness", 0.2));
    native.design_parameter_owners.push(owner(40));
    let extrude = || definition(SCOPE_ID, profile_plane(), one_sided());
    assert_eq!(
        notes(&native, extrude(), code),
        [format!(
            "Extrude {FEATURE_ID} retains its wall-thickness inputs in the native scope."
        )]
    );

    native.design_parameters[0] = parameter(40, "AlongDistance", 0.2);
    assert!(notes(&native, extrude(), code).is_empty());
}

#[test]
fn native_scope_profile_reports_its_unresolved_sketch_placement() {
    let code = F3dLossCode::FeatureExtrudeSketchPlacementNative;
    let native = native_with(scope_with(
        DesignExtrudeExtent::OneSidedDistance,
        DesignExtrudeStart::ProfilePlane,
        Some(sketch_profile()),
        None,
    ));
    assert_eq!(
        notes(&native, definition(SCOPE_ID, profile_plane(), one_sided()), code),
        [format!("Extrude {FEATURE_ID} retains its sketch profile in the native scope because its placement is unresolved.")]
    );

    let group_profile = format!("{STREAM}:operand-group#13");
    assert!(notes(
        &native,
        definition(&group_profile, profile_plane(), one_sided()),
        code
    )
    .is_empty());
}

#[test]
fn distance_and_through_all_reports_its_auxiliary_selection() {
    let code = F3dLossCode::FeatureExtrudeAuxiliarySelectionNative;
    let extent = serde_json::json!({
        "kind": "two_sided",
        "first": blind(2.0),
        "second": {"termination": {"kind": "through_all"}}
    });
    let mut native = native_with(scope(
        DesignExtrudeExtent::TwoSidedDistanceThroughAll,
        DesignExtrudeStart::ProfilePlane,
    ));
    native.design_construction_operand_groups.push(group(
        13,
        DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5),
    ));
    assert_eq!(
        notes(
            &native,
            definition(SCOPE_ID, profile_plane(), extent.clone()),
            code
        ),
        [format!(
            "Extrude {FEATURE_ID} retains an auxiliary selection in its native scope (DR-328)."
        )]
    );

    native.design_construction_operand_groups.clear();
    assert!(notes(&native, definition(SCOPE_ID, profile_plane(), extent), code).is_empty());
}

#[test]
fn target_options_zero_and_two_report_the_assumed_terminations() {
    let code = F3dLossCode::FeatureExtrudeTargetOptionAssumed;
    let expected = [format!(
        "Extrude {FEATURE_ID} assumes the target option preserves the selected side terminations (DR-117)."
    )];
    for extent in [
        DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero,
        DesignExtrudeExtent::TwoSidedToTargetsOptionZero,
        DesignExtrudeExtent::TwoSidedToTargetsOptionTwo,
    ] {
        let native = native_with(scope(extent, DesignExtrudeStart::ProfilePlane));
        assert_eq!(
            notes(
                &native,
                definition(SCOPE_ID, profile_plane(), two_sided()),
                code
            ),
            expected
        );
    }

    let native = native_with(scope(
        DesignExtrudeExtent::TwoSidedToFaces,
        DesignExtrudeStart::ProfilePlane,
    ));
    assert!(notes(
        &native,
        definition(SCOPE_ID, profile_plane(), two_sided()),
        code
    )
    .is_empty());
}

#[test]
fn second_side_control_one_reports_the_assumed_face_termination() {
    let code = F3dLossCode::FeatureExtrudeFaceControlAssumed;
    let native = native_with(scope(
        DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne,
        DesignExtrudeStart::ProfilePlane,
    ));
    assert_eq!(
        notes(&native, definition(SCOPE_ID, profile_plane(), two_sided()), code),
        [format!(
            "Extrude {FEATURE_ID} assumes the second-side control preserves the face termination (DR-329)."
        )]
    );

    let native = native_with(scope(
        DesignExtrudeExtent::TwoSidedDistanceToFace,
        DesignExtrudeStart::ProfilePlane,
    ));
    assert!(notes(
        &native,
        definition(SCOPE_ID, profile_plane(), two_sided()),
        code
    )
    .is_empty());
}

#[test]
fn selected_face_start_read_as_offset_plane_reports_the_absent_start_group() {
    let code = F3dLossCode::FeatureExtrudeStartFaceAbsentAssumed;
    let native = native_with(scope(
        DesignExtrudeExtent::OneSidedToFace,
        DesignExtrudeStart::FromFace,
    ));
    let offset_plane = serde_json::json!({"kind": "offset_profile_plane", "offset": 5.0});
    assert_eq!(
        notes(&native, definition(SCOPE_ID, offset_plane.clone(), one_sided()), code),
        [format!("Extrude {FEATURE_ID} reads its selected-face start without a start group as the offset profile plane (DR-322).")]
    );

    let from_face = serde_json::json!({
        "kind": "from_face",
        "face": {"kind": "native", "value": format!("{STREAM}:operand-group#13")}
    });
    assert!(notes(&native, definition(SCOPE_ID, from_face, one_sided()), code).is_empty());

    let native = native_with(scope(
        DesignExtrudeExtent::OneSidedToFace,
        DesignExtrudeStart::OffsetProfilePlane,
    ));
    assert!(notes(
        &native,
        definition(SCOPE_ID, offset_plane, one_sided()),
        code
    )
    .is_empty());
}

#[test]
fn symmetric_distance_without_a_measurement_word_reports_its_reading() {
    let code = F3dLossCode::FeatureExtrudeSymmetricDistanceAssumed;
    let symmetric = || {
        definition(
            SCOPE_ID,
            profile_plane(),
            serde_json::json!({"kind": "symmetric", "side": blind(2.0)}),
        )
    };
    let indexed = native_with(scope(
        DesignExtrudeExtent::SymmetricDistance,
        DesignExtrudeStart::ProfilePlane,
    ));
    assert_eq!(
        notes(&indexed, symmetric(), code),
        [format!(
            "Extrude {FEATURE_ID} assumes the symmetric distance applies to the whole travel (DR-78)."
        )]
    );

    let fixed = scope_with(
        DesignExtrudeExtent::SymmetricDistance,
        DesignExtrudeStart::ProfilePlane,
        None,
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: cadmpeg_ir::scalar::NonZeroReal::new(0.2).unwrap(),
                    record_index: 11,
                    value_offset: 1100,
                },
            )),
            ..Default::default()
        }),
    );
    assert_eq!(
        notes(&native_with(fixed), symmetric(), code),
        [format!(
            "Extrude {FEATURE_ID} assumes the symmetric distance applies to each side (DR-78)."
        )]
    );

    let mut two_lanes = scope(
        DesignExtrudeExtent::SymmetricDistance,
        DesignExtrudeStart::ProfilePlane,
    );
    if let Some(DesignExtrudePrologue::ReferenceAware {
        side_extent_discriminators,
        ..
    }) = two_lanes.extrude_prologue_mut()
    {
        *side_extent_discriminators = [1, 1];
    }
    assert!(notes(&native_with(two_lanes), symmetric(), code).is_empty());
}

#[test]
fn role_43_profile_group_reports_the_assumed_surface_profile() {
    let code = F3dLossCode::FeatureExtrudeSurfaceProfileAssumed;
    let group_id = format!("{STREAM}:operand-group#13");
    let mut native = native_with(scope(
        DesignExtrudeExtent::OneSidedDistance,
        DesignExtrudeStart::ProfilePlane,
    ));
    native.design_construction_operand_groups.push(group(
        13,
        DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X43),
    ));
    assert_eq!(
        notes(
            &native,
            definition(&group_id, profile_plane(), one_sided()),
            code
        ),
        [format!(
            "Extrude {FEATURE_ID} takes native group {group_id} as its surface profile (DR-328)."
        )]
    );

    native.design_construction_operand_groups[0] =
        group(13, DesignConstructionOperandRole::ExtrudeProfile);
    assert!(notes(
        &native,
        definition(&group_id, profile_plane(), one_sided()),
        code
    )
    .is_empty());
}

#[test]
fn zero_first_side_reports_its_ignored_taper() {
    let code = F3dLossCode::FeatureExtrudeZeroSideTaperIgnored;
    let mut native = native_with(scope(
        DesignExtrudeExtent::TwoSidedDistance,
        DesignExtrudeStart::ProfilePlane,
    ));
    native
        .design_parameters
        .push(parameter(40, "AlongDistance", 0.0));
    native
        .design_parameters
        .push(parameter(50, "TaperAngle", 0.1));
    native.design_parameter_owners.push(owner(40));
    native.design_parameter_owners.push(owner(50));
    let extrude = || definition(SCOPE_ID, profile_plane(), one_sided());
    assert_eq!(
        notes(&native, extrude(), code),
        [format!(
            "Extrude {FEATURE_ID} ignores the first-side taper because its first side has zero length."
        )]
    );

    native.design_parameters[1] = parameter(50, "TaperAngle", 0.0);
    assert!(notes(&native, extrude(), code).is_empty());

    native.design_parameters[0] = parameter(40, "AlongDistance", 0.3);
    native.design_parameters[1] = parameter(50, "TaperAngle", 0.1);
    assert!(notes(&native, extrude(), code).is_empty());
}
