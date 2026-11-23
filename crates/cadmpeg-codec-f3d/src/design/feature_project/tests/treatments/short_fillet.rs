// SPDX-License-Identifier: Apache-2.0

use super::{
    decode_fillet_radius_groups, localized_fillet_group, localized_fillet_owner,
    localized_fillet_scope_without_owners, named_fillet_parameter,
};
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs, ProjectedParameterDesign,
};
use crate::design::test_support::{name_scope_references, synthetic_feature_timelines};
use crate::loss::F3dLossCode;
use crate::records::feature::fillet_set::{
    DesignFilletSet, DesignFilletSetLaw, DesignFilletSetList, DesignFilletSetListForm,
};
use crate::records::feature::scope::DesignScopePayloadMut;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use cadmpeg_ir::features::selections::EdgeSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

const PREVIOUS_STATE: i64 = 7;
const TOLERANCE: f64 = 1e-12;

fn project_short_fillet(
    identities: &[DesignEdgeIdentityOperand],
    name_weight_owner: bool,
) -> ProjectedParameterDesign {
    let mut scope = localized_fillet_scope_without_owners();
    name_scope_references(&mut scope, [10]);
    if name_weight_owner {
        name_scope_references(&mut scope, [20]);
    }
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(PREVIOUS_STATE);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let DesignScopePayloadMut::Fillet(payload) = scope.payload_mut() else {
        panic!("Fillet scope");
    };
    payload.sets = Some(DesignFilletSetList {
        form: DesignFilletSetListForm::Compact,
        rolling_ball_corner: true,
        sets: vec![DesignFilletSet {
            record_index: 99,
            byte_offset: 700,
            frame_length: u64::try_from(crate::layout::fillet_set_short_constant::LEN).unwrap(),
            group_record_index: 100,
            tangency_weight_owner_record_index: None,
            tangent_chain: None,
            curvature_continuity: None,
            law: DesignFilletSetLaw::Constant {
                radius_owner_record_index: 10,
            },
        }],
    });
    let scopes = [scope];
    let timelines = synthetic_feature_timelines(&scopes);
    let construction_groups = [localized_fillet_group(100, 0, vec![200])];
    let parameters = [
        named_fillet_parameter(10, 11, "Radius", Some("cm"), 0.3),
        named_fillet_parameter(20, 21, "TangencyWeight", None, 0.75),
    ];
    let owners = [
        localized_fillet_owner(10, 11, 0),
        localized_fillet_owner(20, 21, 1),
    ];
    let assignments =
        decode_fillet_radius_groups(&scopes, &construction_groups, &owners, &parameters);
    crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                native: &parameters,
                owners: &owners,
                scopes: &scopes,
                construction_groups: &construction_groups,
                edge_identity_operands: identities,
                fillet_radius_groups: &assignments,
                timelines: &timelines,
                ..ProjectInputs::default()
            },
        )
        .unwrap()
    })
}

#[test]
fn short_fillet_pairs_unclaimed_scope_tangency_weight() {
    for name_weight_owner in [true, false] {
        let design = project_short_fillet(&[], name_weight_owner);
        let [feature] = design.features.as_slice() else {
            panic!("one Fillet feature");
        };
        let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
            feature.evaluation.definition()
        else {
            panic!("short constant Fillet");
        };
        let [group] = groups.as_slice() else {
            panic!("one Fillet group");
        };
        let weight = group.tangency_weight.expect("scope-owned tangency weight");
        assert!((weight.get() - 0.75).abs() < TOLERANCE);
        assert_eq!(
            design
                .projection_losses
                .iter()
                .filter(|loss| loss.code == F3dLossCode::FeatureFilletSetInputUnresolved.kind())
                .count(),
            1,
            "the inferred weight requires a Fillet input loss"
        );
    }
}

#[test]
fn short_fillet_resolves_edges_from_its_constant_radius() {
    let identity = serde_json::from_value(serde_json::json!({
        "id": "f3d:native/BulkStream.dat:edge-identity#200",
        "scope_record_index": 12,
        "group_record_index": 100,
        "group_member_ordinal": 0,
        "record_index": 200,
        "byte_offset": 0,
        "class_tag": "277",
        "compact_layout": true,
        "local_id": 200,
        "local_id_offset": 23,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "asset_id_offset": 41,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
        "context_id_offset": 117,
        "treatment_radius_candidates": [
            { "edge_slot": 17, "radius": 3.0 },
            { "edge_slot": 18, "radius": 5.0 }
        ]
    }))
    .unwrap();
    let design = project_short_fillet(&[identity], true);
    let [feature] = design.features.as_slice() else {
        panic!("one Fillet feature");
    };
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        feature.evaluation.definition()
    else {
        panic!("short constant Fillet");
    };
    let EdgeSelection::Historical { edges, .. } = &groups[0].edges else {
        panic!(
            "expected the radius-qualified edge, got {:?}",
            groups[0].edges
        );
    };
    assert_eq!(edges.len(), 1);
    assert_eq!(
        crate::ids::feature_input_edge_slot(&feature.id, PREVIOUS_STATE, &edges[0]),
        Some(17)
    );
}
