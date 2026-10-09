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

fn item(error: &CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
}

fn retained(error: &CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == operation));
}

#[test]
fn recipe_binding_refuses_before_vec_growth() {
    item(
        &run(
            BINDING,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo recipe bindings"),
                |cap| {
                    run(BINDING, cap, u64::MAX, |ctx| {
                        super::super::recipe_bindings(ctx, BINDING)
                    })
                },
            ),
            u64::MAX,
            |ctx| super::super::recipe_bindings(ctx, BINDING),
        )
        .expect_err("one recipe binding needs an item"),
        "creo recipe bindings",
    );
}

#[test]
fn recipe_binding_count_refuses_before_btree_insertion() {
    item(
        &run(
            BINDING,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo recipe binding counts"),
                |cap| {
                    run(BINDING, cap, u64::MAX, |ctx| {
                        super::super::operation_states(ctx, BINDING)
                    })
                },
            ),
            u64::MAX,
            |ctx| super::super::operation_states(ctx, BINDING),
        )
        .expect_err("binding count node needs admission"),
        "creo recipe binding counts",
    );
}

#[test]
fn operation_family_refuses_before_retained_text_copy() {
    retained(
        &run(
            DISPLAY,
            u64::MAX,
            crate::test_support::allocation_limit_at(
                ResourceDimension::RetainedBytes,
                Some("creo operation family name"),
                |cap| {
                    run(DISPLAY, u64::MAX, cap, |ctx| {
                        super::super::operation_states(ctx, DISPLAY)
                    })
                },
            ),
            |ctx| super::super::operation_states(ctx, DISPLAY),
        )
        .expect_err("family text needs retained bytes"),
        "creo operation family name",
    );
}

#[test]
fn operation_stored_name_refuses_before_retained_byte_copy() {
    retained(
        &run(
            DISPLAY,
            u64::MAX,
            crate::test_support::allocation_limit_at(
                ResourceDimension::RetainedBytes,
                Some("creo operation stored name bytes"),
                |cap| {
                    run(DISPLAY, u64::MAX, cap, |ctx| {
                        super::super::operation_states(ctx, DISPLAY)
                    })
                },
            ),
            |ctx| super::super::operation_states(ctx, DISPLAY),
        )
        .expect_err("stored name bytes need retained admission"),
        "creo operation stored name bytes",
    );
}

#[test]
fn operation_state_refuses_before_vec_growth() {
    item(
        &run(
            DISPLAY,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo feature operation states"),
                |cap| {
                    run(DISPLAY, cap, u64::MAX, |ctx| {
                        super::super::operation_states(ctx, DISPLAY)
                    })
                },
            ),
            u64::MAX,
            |ctx| super::super::operation_states(ctx, DISPLAY),
        )
        .expect_err("one state needs a vector item"),
        "creo feature operation states",
    );
}

#[test]
fn operation_display_count_refuses_before_btree_insertion() {
    item(
        &run(
            DISPLAY,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo operation display counts"),
                |cap| {
                    run(DISPLAY, cap, u64::MAX, |ctx| {
                        super::super::operation_states(ctx, DISPLAY)
                    })
                },
            ),
            u64::MAX,
            |ctx| super::super::operation_states(ctx, DISPLAY),
        )
        .expect_err("one display count needs a map node"),
        "creo operation display counts",
    );
}

#[test]
fn operation_feature_node_refuses_before_btree_insertion() {
    item(
        &run(
            DISPLAY,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo operation feature nodes"),
                |cap| {
                    run(DISPLAY, cap, u64::MAX, |ctx| {
                        super::super::operations(ctx, DISPLAY)
                    })
                },
            ),
            u64::MAX,
            |ctx| super::super::operations(ctx, DISPLAY),
        )
        .expect_err("operation map needs a node"),
        "creo operation feature nodes",
    );
}

#[test]
fn operation_feature_state_refuses_before_inner_vec_growth() {
    item(
        &run(
            DISPLAY,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo operation feature states"),
                |cap| {
                    run(DISPLAY, cap, u64::MAX, |ctx| {
                        super::super::operations(ctx, DISPLAY)
                    })
                },
            ),
            u64::MAX,
            |ctx| super::super::operations(ctx, DISPLAY),
        )
        .expect_err("inner state vector needs one item"),
        "creo operation feature states",
    );
}

#[test]
fn current_operation_projection_refuses_before_vec_growth() {
    assert_eq!(
        run(
            DISPLAY,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                None,
                |cap| run(DISPLAY, cap, u64::MAX, |ctx| {
                    super::super::operations(ctx, DISPLAY)
                })
            ),
            u64::MAX,
            |ctx| { super::super::operations(ctx, DISPLAY) }
        )
        .expect("one operation admitted")
        .len(),
        1
    );
    item(
        &run(
            DISPLAY,
            crate::test_support::allocation_limit_at(
                ResourceDimension::CollectionItems,
                Some("creo current operation projections"),
                |cap| {
                    run(DISPLAY, cap, u64::MAX, |ctx| {
                        super::super::operations(ctx, DISPLAY)
                    })
                },
            ),
            u64::MAX,
            |ctx| super::super::operations(ctx, DISPLAY),
        )
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

#[test]
fn operation_family_utf8_refuses_before_invalid_identity() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::super::operation_states(ctx, b"\xff id x\0"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn operation_identity_utf8_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::super::operation_states(ctx, DISPLAY),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn operation_scans_refuse_at_work_boundaries() {
    crate::test_support::assert_work_boundaries(
        &[
            "creo recipe binding scan",
            "creo operation display scan",
            "creo operation family start",
            "creo operation identity end",
            "creo operation identity digits",
            "creo operation record start",
            "creo inline recipe scan",
            "creo operation grouping",
            "creo operation state consensus",
            "creo feature operation kind comparison",
        ],
        |ctx| {
            let operations = super::super::operations(ctx, CONFLICTING_DISPLAYS)?;
            // Both display records have empty recipe slices. Exercise an actual
            // recipe window after preserving the original projection route.
            assert_eq!(super::super::inline_recipe_resolution(ctx, b"protextrude\0")?,
                super::super::RecipeState::Resolved(super::super::FeatureRecipe::ProtrudeExtrude));
            Ok(operations)
        },
    );
}

#[test]
fn operation_projection_retains_only_the_selected_family() {
    let cap =
        crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, None, |cap| {
            run(DISPLAY, u64::MAX, cap, |ctx| {
                super::super::operations(ctx, DISPLAY)
            })
        });
    let repeated = DISPLAY.repeat(2_000);
    let operations = run(&repeated, u64::MAX, cap, |ctx| {
        super::super::operations(ctx, &repeated)
    })
    .expect("discarded display names need no retained bytes");
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].feature_id, 7);
    assert_eq!(operations[0].kind.as_str(), "Extrude");
    assert!(!operations[0].display_name_stored());
    assert!(operations[0].display_state_conflict);
}
