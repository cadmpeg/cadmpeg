// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const DISPLAY: &[u8] = b"\xe3Extrude id 7\0";
const BINDING: &[u8] = b"\xe3\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";
const CONFLICTING_BINDINGS: &[u8] =
    b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
    \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0";
const CONFLICTING_DISPLAYS: &[u8] = b"\xe3oExtrude id 7\0\xe3xExtrude id 7\0";

fn run<T>(
    payload: &[u8],
    items: u64,
    retained: u64,
    parse: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root operation input is admitted");
    parse(&ctx)
}

fn item(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
}

fn retained(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == operation));
}

#[test]
fn recipe_binding_refuses_before_vec_growth() {
    item(
        run(BINDING, 0, u64::MAX, |ctx| {
            super::super::recipe_bindings(ctx, BINDING)
        })
        .expect_err("one recipe binding needs an item"),
        "creo recipe bindings",
    );
}

#[test]
fn recipe_feature_node_refuses_before_btree_insertion() {
    let binding = super::super::FeatureRecipeBinding {
        recipe: super::super::FeatureRecipe::ProtrudeExtrude,
        root_schema_class: crate::feature::schema::SchemaClass::from(917),
        parent_feature_id: 1,
        offset: 0,
    };
    item(
        run(&[], 0, u64::MAX, |ctx| {
            super::super::conflicting_recipe_features(ctx, &[(7, binding)])
        })
        .expect_err("one feature map node needs admission"),
        "creo recipe feature nodes",
    );
}

#[test]
fn recipe_feature_binding_refuses_before_inner_vec_growth() {
    let binding = super::super::FeatureRecipeBinding {
        recipe: super::super::FeatureRecipe::ProtrudeExtrude,
        root_schema_class: crate::feature::schema::SchemaClass::from(917),
        parent_feature_id: 1,
        offset: 0,
    };
    item(
        run(&[], 1, u64::MAX, |ctx| {
            super::super::conflicting_recipe_features(ctx, &[(7, binding)])
        })
        .expect_err("inner binding vector needs admission"),
        "creo recipe feature bindings",
    );
}

#[test]
fn conflicting_recipe_feature_refuses_before_btree_set_insertion() {
    let first = super::super::FeatureRecipeBinding {
        recipe: super::super::FeatureRecipe::ProtrudeExtrude,
        root_schema_class: crate::feature::schema::SchemaClass::from(917),
        parent_feature_id: 1,
        offset: 0,
    };
    let second = super::super::FeatureRecipeBinding {
        recipe: super::super::FeatureRecipe::CutExtrude,
        ..first
    };
    item(
        run(&[], 3, u64::MAX, |ctx| {
            super::super::conflicting_recipe_features(ctx, &[(7, first), (7, second)])
        })
        .expect_err("conflict set node needs admission"),
        "creo conflicting recipe features",
    );
}

#[test]
fn recipe_binding_count_refuses_before_btree_insertion() {
    item(
        run(BINDING, 3, u64::MAX, |ctx| {
            super::super::operation_states(ctx, BINDING)
        })
        .expect_err("binding count node needs admission"),
        "creo recipe binding counts",
    );
}

#[test]
fn operation_family_refuses_before_retained_text_copy() {
    retained(
        run(DISPLAY, u64::MAX, 0, |ctx| {
            super::super::operation_states(ctx, DISPLAY)
        })
        .expect_err("family text needs retained bytes"),
        "creo operation family name",
    );
}

#[test]
fn operation_stored_name_refuses_before_retained_byte_copy() {
    retained(
        run(DISPLAY, u64::MAX, 7, |ctx| {
            super::super::operation_states(ctx, DISPLAY)
        })
        .expect_err("stored name bytes need retained admission"),
        "creo operation stored name bytes",
    );
}

#[test]
fn operation_state_refuses_before_vec_growth() {
    item(
        run(DISPLAY, 0, u64::MAX, |ctx| {
            super::super::operation_states(ctx, DISPLAY)
        })
        .expect_err("one state needs a vector item"),
        "creo feature operation states",
    );
}

#[test]
fn operation_display_count_refuses_before_btree_insertion() {
    item(
        run(DISPLAY, 1, u64::MAX, |ctx| {
            super::super::operation_states(ctx, DISPLAY)
        })
        .expect_err("one display count needs a map node"),
        "creo operation display counts",
    );
}

#[test]
fn conflicting_operation_display_refuses_before_btree_set_insertion() {
    item(
        run(CONFLICTING_DISPLAYS, 3, u64::MAX, |ctx| {
            super::super::operation_states(ctx, CONFLICTING_DISPLAYS)
        })
        .expect_err("conflict set needs a node"),
        "creo conflicting operation displays",
    );
}

#[test]
fn operation_feature_node_refuses_before_btree_insertion() {
    item(
        run(DISPLAY, 2, u64::MAX, |ctx| {
            super::super::operations(ctx, DISPLAY)
        })
        .expect_err("operation map needs a node"),
        "creo operation feature nodes",
    );
}

#[test]
fn operation_feature_state_refuses_before_inner_vec_growth() {
    item(
        run(DISPLAY, 3, u64::MAX, |ctx| {
            super::super::operations(ctx, DISPLAY)
        })
        .expect_err("inner state vector needs one item"),
        "creo operation feature states",
    );
}

#[test]
fn current_operation_projection_refuses_before_vec_growth() {
    assert_eq!(
        run(DISPLAY, 5, u64::MAX, |ctx| {
            super::super::operations(ctx, DISPLAY)
        })
        .expect("one operation admitted")
        .len(),
        1
    );
    item(
        run(DISPLAY, 4, u64::MAX, |ctx| {
            super::super::operations(ctx, DISPLAY)
        })
        .expect_err("current projection needs one item"),
        "creo current operation projections",
    );
}

#[test]
fn competing_recipe_bindings_retain_service_result() {
    assert_eq!(
        run(CONFLICTING_BINDINGS, u64::MAX, u64::MAX, |ctx| {
            super::super::operation_states(ctx, CONFLICTING_BINDINGS)
        })
        .expect("competing source states remain admitted")
        .len(),
        2
    );
}
