// SPDX-License-Identifier: Apache-2.0
//! Chamfer projection: the edge groups the source states and the parameter
//! lanes they pair with.
use super::{localized_fillet_group, localized_fillet_owner, localized_fillet_parameter};
use crate::design::feature_project::project_parameter_design;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

fn localized_chamfer_scope() -> DesignParameterScope {
    DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:native/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
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
            payload: crate::records::feature::scope::DesignFeatureKind::Chamfer
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}
#[test]
fn a_chamfer_that_states_no_edge_group_refuses_a_one_element_distance_lane() {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        EdgeSelection,
    };

    let scope = localized_chamfer_scope();
    let parameters = [localized_fillet_parameter(
        10,
        11,
        "Distance",
        Some("mm"),
        0.1,
    )];
    let owners = [localized_fillet_owner(10, 11, 0)];

    let (refused, _) = project_parameter_design(
        &parameters,
        &owners,
        std::slice::from_ref(&scope),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        refused[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Chamfer,
            parameters,
        }) if parameters.len() == 1
    ));

    let group = localized_fillet_group(100, 0, vec![200]);
    let (accepted, _) = project_parameter_design(
        &parameters,
        &owners,
        std::slice::from_ref(&scope),
        std::slice::from_ref(&group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        accepted[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                edges: EdgeSelection::Native(selection),
                spec: ChamferSpec::Distance { distance },
            }] if selection == &group.id && distance.get() == 1.0)
    ));
}

fn assert_chamfer_collection_refusal(limit: u64, operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let scope = localized_chamfer_scope();
    let parameter = localized_fillet_parameter(10, 11, "Distance", Some("mm"), 0.1);
    let group = localized_fillet_group(100, 0, vec![200]);
    let inputs = crate::design::feature_project::ProjectInputs {
        native: &[],
        owners: &[],
        scopes: &[],
        timelines: &[],
        construction_groups: std::slice::from_ref(&group),
        fillet_radius_groups: &[],
        edge_operands: &[],
        edge_identity_operands: &[],
        edge_treatment_vertex_operands: &[],
        entity_selection_operands: &[],
        curve_identities: &[],
        face_operands: &[],
        body_recipe_operands: &[],
        legacy_loft_body_carriers: &[],
        placements: &[],
        body_bindings: &[],
        component_naming_spaces: &[],
        histories: &[],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(crate::design::feature_project::project_chamfer(&scope, &[(0, &parameter)],
        &inputs, &ctx), Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems && failure.operation == operation)
    );
}

#[test]
fn chamfer_edge_groups_refuse_collection_limit() {
    assert_chamfer_collection_refusal(0, "f3d chamfer edge groups");
}

#[test]
fn chamfer_ordered_parameter_entries_refuse_collection_limit() {
    assert_chamfer_collection_refusal(1, "f3d chamfer ordered parameter entries");
}

#[test]
fn chamfer_ordered_parameter_output_refuses_collection_limit() {
    assert_chamfer_collection_refusal(2, "f3d chamfer ordered parameter output");
}

#[test]
fn chamfer_specifications_refuse_collection_limit() {
    assert_chamfer_collection_refusal(3, "f3d chamfer specifications");
}

#[test]
fn chamfer_output_groups_refuse_collection_limit() {
    // Four chamfer entries and one edge-member identity precede the output group.
    assert_chamfer_collection_refusal(5, "f3d chamfer output groups");
}
