// SPDX-License-Identifier: Apache-2.0

use super::{
    derived_base_groups, derived_base_output_order, derived_base_scopes_property,
    feature_derived_base_scopes, DesignDerivedBaseOwner,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use std::collections::HashMap;

const STREAM: &str = "f3d:Design/BulkStream.dat";

/// A scope of `kind` at `record_index` on history states `previous -> state`
/// whose frame names `owner` at its reference offsets.
fn scope(
    kind: DesignFeatureKind,
    record_index: u32,
    previous: i64,
    state: i64,
    owner: Option<u32>,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{record_index}"),
        kind,
        record_index,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 100;
            draft.history_state_id = Some(state);
            draft.previous_history_state_id = Some(previous);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let paired = scope.paired_byte_offset();
    scope.derived_base_owner = owner.map(|owner| DesignDerivedBaseOwner {
        record_index: Located {
            value: owner,
            offset: 160,
        },
        repeat_offset: paired - 12,
    });
    scope
}

fn derived(
    record_index: u32,
    previous: i64,
    state: i64,
    owner: Option<u32>,
) -> DesignParameterScope {
    scope(
        DesignFeatureKind::native("Derived Base Feature"),
        record_index,
        previous,
        state,
        owner,
    )
}

fn grouped(scopes: &[DesignParameterScope]) -> Vec<Vec<u32>> {
    let mut groups = derived_base_groups(scopes)
        .into_values()
        .map(|group| group.iter().map(|scope| scope.record_index).collect())
        .collect::<Vec<Vec<u32>>>();
    groups.sort();
    groups
}

#[test]
fn derived_base_scopes_group_by_owner_when_their_states_chain() {
    let chained = [
        derived(88, 78, 79, Some(50)),
        derived(70, 62, 77, Some(50)),
        derived(79, 77, 78, Some(50)),
        derived(10, 2, 3, Some(5)),
    ];
    assert_eq!(grouped(&chained), [vec![10], vec![70, 79, 88]]);
}

#[test]
fn a_derived_base_group_beside_an_unreadable_sibling_does_not_hold() {
    for (scopes, reason) in [
        (
            vec![
                derived(70, 62, 77, Some(50)),
                derived(79, 77, 78, Some(50)),
                derived(88, 78, 79, None),
            ],
            "a scope without an owner continues the group",
        ),
        (
            vec![derived(61, 55, 62, None), derived(70, 62, 77, Some(50))],
            "a scope without an owner precedes the group",
        ),
        (
            vec![derived(70, 62, 77, Some(50)), derived(79, 70, 78, Some(50))],
            "the second scope does not continue the first",
        ),
    ] {
        assert!(grouped(&scopes).is_empty(), "{reason}");
    }
    let other_kind = [scope(DesignFeatureKind::Extrude, 70, 62, 77, Some(50))];
    assert!(
        grouped(&other_kind).is_empty(),
        "only derived base scopes group"
    );
}

fn feature(native_ref: &str, group: Option<&str>) -> cadmpeg_ir::features::Feature {
    let mut feature = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("f3d:test:feature#1").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native {
                    kind: "Derived Base Feature".into(),
                    parameters: std::collections::BTreeMap::default(),
                },
            ),
        ),
        native_ref: Some(native_ref.to_owned()),
    };
    if let Some(group) = group {
        feature
            .source_properties
            .insert(derived_base_scopes_property(), group.to_owned());
    }
    feature
}

#[test]
fn a_projected_derive_feature_lists_its_scopes_and_orders_its_outputs_by_scope() {
    let scopes = [derived(70, 62, 77, Some(50)), derived(79, 77, 78, Some(50))];
    let leader = feature(&scopes[0].id, Some("70,79"));
    let members = feature_derived_base_scopes(&leader, &scopes).expect("group");
    assert_eq!(
        members
            .iter()
            .map(|scope| scope.record_index)
            .collect::<Vec<_>>(),
        [70, 79]
    );
    let states = HashMap::from([((0, 77), Some(vec![5])), ((0, 78), Some(vec![3]))]);
    assert_eq!(
        derived_base_output_order(&leader, &scopes, 0, &states, vec![3, 5]),
        [5, 3]
    );

    let plain = feature(&scopes[0].id, None);
    assert!(feature_derived_base_scopes(&plain, &scopes).is_none());
    assert_eq!(
        derived_base_output_order(&plain, &scopes, 0, &states, vec![3, 5]),
        [3, 5],
        "a feature without the property keeps its outputs"
    );
    let unknown = feature(&scopes[0].id, Some("70,99"));
    assert!(
        feature_derived_base_scopes(&unknown, &scopes).is_none(),
        "a listed record without a scope voids the group"
    );
}

#[test]
fn a_derived_base_owner_survives_cadir_and_must_sit_in_its_frame() {
    let scope = derived(70, 62, 77, Some(50));
    let wire = serde_json::to_value(&scope).unwrap();
    let read = serde_json::from_value::<DesignParameterScope>(wire.clone()).unwrap();
    assert_eq!(read.derived_base_owner, scope.derived_base_owner);

    let mut moved = wire.clone();
    moved["derived_base_owner"]["repeat_offset"] = serde_json::json!(0);
    assert!(serde_json::from_value::<DesignParameterScope>(moved).is_err());

    let extrude = scope_with_owner_on_extrude();
    let wire = serde_json::to_value(&extrude).unwrap();
    assert!(serde_json::from_value::<DesignParameterScope>(wire).is_err());
}

fn scope_with_owner_on_extrude() -> DesignParameterScope {
    scope(DesignFeatureKind::Extrude, 70, 62, 77, Some(50))
}
