// SPDX-License-Identifier: Apache-2.0

use super::{reference_names, FeatureRecipe, FeatureReferenceName};

fn operation_states(payload: &[u8]) -> Vec<crate::feature::operations::FeatureOperationState> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::feature::operations::operation_states(ctx, payload)
    })
    .expect("feature operation states are admitted")
}

fn operations(payload: &[u8]) -> Vec<crate::feature::operations::FeatureOperation> {
    crate::decode::with_test_decode_ctx(|ctx| crate::feature::operations::operations(ctx, payload))
        .expect("feature operations are admitted")
}

#[test]
fn decodes_mdlstatus_recipe_discriminators_within_their_records() {
    let payload = b"\xe3icon\0protextrude\0Protrusion id 40\0\xe2\xe3\
            icon\0protrevolve\0Revolve id 41\0\xe2\xe3\
            icon\0cutextrude\0Cut id 42\0\xe2\xe3\
            icon\0cutrevolve\0Cut id 43\0\xe2\xe3Datum Plane id 44\0\xe3K\xc3\xb6rper ID 45\0";
    let operations = operations(payload);
    assert_eq!(operations.len(), 6);
    assert_eq!(
        operations[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(
        operations[1].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeRevolve)
    );
    assert_eq!(
        operations[2].recipe.resolved(),
        Some(FeatureRecipe::CutExtrude)
    );
    assert_eq!(
        operations[3].recipe.resolved(),
        Some(FeatureRecipe::CutRevolve)
    );
    assert_eq!(
        operations[4].recipe,
        crate::feature::operations::RecipeResolution::None
    );
    assert_eq!(operations[5].kind.as_str(), "Körper");
    assert_eq!(operations[5].feature_id, 45);
}

#[test]
fn preserves_mdlstatus_name_prefixes_without_using_them_as_state_selectors() {
    let payload = b"\xe3oExtrude id 7\0\xe3xExtrude id 7\0\xe3yExtrude id 7\0\xe3zExtrude ID 7\0";

    let states = operation_states(payload);
    assert_eq!(states.len(), 4);
    for (state, (prefix, expected_name)) in states.iter().zip([
        (b'o', "oExtrude id 7"),
        (b'x', "xExtrude id 7"),
        (b'y', "yExtrude id 7"),
        (b'z', "zExtrude ID 7"),
    ]) {
        assert_eq!(state.feature_id, 7);
        assert_eq!(state.kind.as_str(), "Extrude");
        assert_eq!(state.stored_name_prefix(), Some(prefix));
        assert!(state.display_state_conflict);
        assert_eq!(state.state_offset + 1, state.offset);
        assert_eq!(state.stored_name().as_deref(), Some(expected_name));
    }
    assert_eq!(states[3].name.identifier_keyword(), Some("ID"));

    let current_operations = operations(payload);
    let [current] = current_operations.as_slice() else {
        panic!("one current operation");
    };
    assert_eq!(current.kind.as_str(), "Extrude");
    assert!(!current.display_name_stored());
    assert_eq!(current.stored_name(), None);
    assert_eq!(current.name.stored_name_bytes(), None);
    assert_eq!(current.name.identifier_keyword(), None);
    assert_eq!(current.stored_name_prefix(), None);
    assert!(current.display_state_conflict);
}

#[test]
fn conflicting_inline_recipes_across_display_states_remain_conflicting() {
    let payload = b"\xe3protextrude\0Extrude id 7\0\xe3cutextrude\0Extrude id 7\0";

    let states = operation_states(payload);
    assert_eq!(states.len(), 2);
    assert_eq!(
        states[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(states[1].recipe.resolved(), Some(FeatureRecipe::CutExtrude));

    let current_operations = operations(payload);
    let [current] = current_operations.as_slice() else {
        panic!("one consensus operation");
    };
    assert_eq!(current.kind.as_str(), "Extrude");
    assert!(current.display_state_conflict);
    assert!(current.recipe.is_conflicting());
    assert_eq!(
        current.recipe,
        crate::feature::operations::RecipeResolution::Conflicting
    );
}

#[test]
fn binds_depdb_recipe_records_to_compact_feature_ids() {
    let payload = b"\xe3K\xc3\xb6rper ID 247\0\xe3\
            \xf7\x3b\x80\xf7\x83\x95\xf6\x20Drehen 1\0\xf6\0protrevolve\0\
            \xe3Body ID 8053\0\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

    let operations = operations(payload);
    assert_eq!(operations.len(), 2);
    assert_eq!(operations[0].feature_id, 247);
    assert_eq!(
        operations[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeRevolve)
    );
    assert_eq!(
        operations[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(operations[0].parent_feature_id(), Some(32));
    assert_eq!(operations[1].feature_id, 8053);
    assert_eq!(
        operations[1].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(
        operations[1]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(operations[1].parent_feature_id(), Some(8051));
}

#[test]
fn preserves_competing_depdb_recipe_bindings() {
    let payload = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0";

    let states = operation_states(payload);
    assert_eq!(states.len(), 2);
    assert_eq!(states[0].feature_id, 8053);
    assert_eq!(
        states[0].recipe.candidate(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert!(states[0].recipe.is_conflicting());
    assert_eq!(
        states[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(states[1].feature_id, 8053);
    assert_eq!(
        states[1].recipe.candidate(),
        Some(FeatureRecipe::CutExtrude)
    );
    assert!(states[1].recipe.is_conflicting());
    assert_eq!(
        states[1]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(916)
    );

    let current = operations(payload);
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].feature_id, 8053);
    assert_eq!(current[0].kind.as_str(), "Native Feature");
    assert_eq!(
        current[0].recipe,
        crate::feature::operations::RecipeResolution::Conflicting
    );
    assert!(current[0].recipe.is_conflicting());
    assert_eq!(
        current[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        None
    );
    assert_eq!(current[0].parent_feature_id(), None);

    let repeated = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 2\0\xf6\0protextrude\0";
    let repeated_states = operation_states(repeated);
    assert_eq!(repeated_states.len(), 2);
    assert_eq!(repeated_states[0].recipe, repeated_states[1].recipe);
    assert_eq!(
        repeated_states[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_ne!(repeated_states[0].offset, repeated_states[1].offset);
    let repeated_current = operations(repeated);
    assert_eq!(repeated_current.len(), 1);
    assert_eq!(repeated_current[0].kind.as_str(), "Extrude");
    assert_eq!(
        repeated_current[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(
        repeated_current[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(repeated_current[0].parent_feature_id(), Some(8051));
}

#[test]
fn conflicting_bindings_do_not_use_an_inline_recipe_fallback() {
    let payload = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0\
            \xe3icon\0protextrude\0Extrude id 8053\0";

    let states = operation_states(payload);
    let display = states
        .iter()
        .find(|state| state.display_name_stored())
        .expect("stored display state");
    assert_eq!(display.kind.as_str(), "Extrude");
    assert_eq!(display.recipe.candidate(), None);
    assert!(display.recipe.is_conflicting());
    assert_eq!(
        display
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        None
    );
    assert_eq!(display.parent_feature_id(), None);

    let current = operations(payload);
    let [current] = current.as_slice() else {
        panic!("one current operation");
    };
    assert_eq!(current.kind.as_str(), "Extrude");
    assert!(current.display_name_stored());
    assert_eq!(
        current.recipe,
        crate::feature::operations::RecipeResolution::Conflicting
    );
    assert!(current.recipe.is_conflicting());
}

#[test]
fn leaves_inline_recipe_conflicts_unresolved() {
    let payload = b"\xe3icon\0protextrude\0cutextrude\0Extrude id 9\0";

    let states = operation_states(payload);
    let [state] = states.as_slice() else {
        panic!("one operation state");
    };
    assert_eq!(state.feature_id, 9);
    assert_eq!(state.kind.as_str(), "Extrude");
    assert_eq!(state.recipe.candidate(), None);
    assert!(state.recipe.is_conflicting());
    assert_eq!(
        state
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        None
    );
    assert_eq!(state.parent_feature_id(), None);
}

#[test]
fn promotes_depdb_recipe_without_operation_display_name() {
    let payload = b"\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

    let operations = operations(payload);
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].feature_id, 8053);
    assert_eq!(operations[0].kind.as_str(), "Extrude");
    assert_eq!(
        operations[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(
        operations[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(operations[0].parent_feature_id(), Some(8051));
    assert_eq!(operations[0].offset, 1);
}

#[test]
fn model_reference_entry_joins_feature_name_to_feature_id() {
    let payload = b"\0\xf7\x71\x2a\x05\x29Datum Plane id 41\0\x2a\x2a\x10\0\
            \xf7\x71\x30\x05\x2fBroken\0\x30\x31";

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root input is admitted");
    let names = reference_names(&ctx, payload).expect("reference names");
    assert_eq!(
        names,
        [FeatureReferenceName {
            feature_id: 41,
            name_bytes: b"Datum Plane id 41".to_vec(),
            own_reference_id: 42,
            reference_type: 5,
            offset: 1,
        }]
    );
    assert_eq!(names[0].name(), "Datum Plane id 41");
}

