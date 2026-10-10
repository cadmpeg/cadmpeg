// SPDX-License-Identifier: Apache-2.0
//! Query backing drops independently of accepted native selections.

use super::super::{sorted_transition_slots, EdgeTreatmentInputs};
use super::*;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{EdgeSelection, FeatureId};

const QUERY_SCRATCH: u64 = 4096;

fn scratch_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = QUERY_SCRATCH;
    policy
}

fn exact_recipe(record: u32, edge: i64) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record, &[], &[]);
    operand.recipe_references = vec![recipe_reference(&[edge])];
    operand
}

fn recipe_bytes(count: usize) -> u64 {
    // The first push reserves four EdgeId slots; each copied text has exact
    // retained storage. Both supplied native IDs have equal byte length.
    u64::try_from(4 * std::mem::size_of::<EdgeId>() + count * "f3d:test:edge#17".len()).unwrap()
}

fn rejected_recipe(second: &DesignEdgeOperand, expected: SurfacePatchRecipeEdges) {
    let first = exact_recipe(10, 17);
    let mut policy = scratch_policy();
    policy.limits.max_retained_bytes = recipe_bytes(2);
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            assert_eq!(
                surface_patch_grouped_recipe_edges(&[&first, second], ctx).unwrap(),
                expected
            );
            drop(
                ctx.reserve_scoped(QUERY_SCRATCH, "reuse rejected recipe query scratch")
                    .unwrap(),
            );
        }
        ctx.charge_retained(recipe_bytes(2), "reuse rejected recipe candidate retention")
            .unwrap();
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn edge_query_lifetime_absent_recipe_refunds_cloned_prefix() {
    rejected_recipe(
        &recipe_edge_operand(11, &[], &[]),
        SurfacePatchRecipeEdges::Absent,
    );
}

#[test]
fn edge_query_lifetime_conflicting_recipe_refunds_cloned_prefix() {
    let mut second = exact_recipe(11, 18);
    second.recipe_references.push(recipe_reference(&[19]));
    rejected_recipe(&second, SurfacePatchRecipeEdges::Inconclusive);
}

#[test]
fn edge_query_lifetime_repeated_recipe_refunds_clones_and_distinct_index() {
    rejected_recipe(&exact_recipe(11, 17), SurfacePatchRecipeEdges::Inconclusive);
}

#[test]
fn edge_query_lifetime_allowed_recipe_keeps_only_actual_retained_backing() {
    let first = exact_recipe(10, 17);
    let mut policy = scratch_policy();
    policy.limits.max_retained_bytes = recipe_bytes(1);
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let recipe = surface_patch_grouped_recipe_edges(&[&first], ctx).unwrap();
        assert_eq!(
            recipe,
            SurfacePatchRecipeEdges::Resolved(vec![EdgeId::mint("f3d:test:edge#17").unwrap()])
        );
        drop(
            ctx.reserve_scoped(
                QUERY_SCRATCH,
                "reuse recipe distinct index scratch while output lives",
            )
            .unwrap(),
        );
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_retained(1, "live accepted recipe edge backing")
        else {
            panic!("accepted output keeps its actual backing");
        };
        assert_eq!((original.used, original.additional), (recipe_bytes(1), 1));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        drop(recipe);
        assert!(
            matches!(surface_patch_grouped_recipe_edges(&[], ctx), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn edge_query_lifetime_recipe_retained_one_below_preserves_original_refusal() {
    let first = exact_recipe(10, 17);
    let mut policy = scratch_policy();
    policy.limits.max_retained_bytes = recipe_bytes(1) - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            surface_patch_grouped_recipe_edges(&[&first], ctx)
        else {
            panic!("actual recipe vector growth must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.operation, "f3d surface patch recipe edge");
        assert_eq!(
            (original.used, original.additional),
            (
                u64::try_from("f3d:test:edge#17".len()).unwrap(),
                u64::try_from(4 * std::mem::size_of::<EdgeId>()).unwrap()
            )
        );
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(surface_patch_grouped_recipe_edges(&[], ctx), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn edge_query_lifetime_distinct_index_refusal_does_not_escape_candidate() {
    let first = exact_recipe(10, 17);
    let mut policy = scratch_policy();
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            surface_patch_grouped_recipe_edges(&[&first], ctx)
        else {
            panic!("actual distinct index growth must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(original.operation, "f3d surface patch distinct recipe edge");
        assert_eq!(original.used, 0);
        assert!(original.additional > 0);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(surface_patch_grouped_recipe_edges(&[], ctx), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

fn pair_group() -> DesignConstructionOperandGroup {
    let mut group = group(2, 10);
    group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    group
}

#[test]
fn edge_query_lifetime_incomplete_identity_prefix_keeps_only_native_output() {
    let group = pair_group();
    let identities = [identity(10, &[])];
    let recipes = [
        recipe_edge_operand(10, &[], &[]),
        recipe_edge_operand(11, &[], &[]),
    ];
    let feature = FeatureId::mint("f3d:model:feature#query-lifetime").unwrap();
    let native_bytes = u64::try_from(group.id.len()).unwrap();
    let mut policy = scratch_policy();
    policy.limits.max_retained_bytes = 7 * native_bytes;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut selections = Vec::new();
        for _ in 0..7 {
            let selection = resolved_edge_group(
                &group,
                std::slice::from_ref(&group),
                &recipes,
                &identities,
                None,
                &feature,
                ctx,
            )
            .unwrap();
            assert!(matches!(&selection, EdgeSelection::Native(id) if id == &group.id));
            selections.push(selection);
            drop(
                ctx.reserve_scoped(
                    QUERY_SCRATCH,
                    "reuse incomplete identity and recipe queries",
                )
                .unwrap(),
            );
        }
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_retained(1, "seven live native selections")
        else {
            panic!("only seven copied native IDs exhaust this ceiling");
        };
        assert_eq!((original.used, original.additional), (7 * native_bytes, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
        drop(selections);
    });
}

#[test]
fn edge_query_lifetime_identity_pointer_backing_refuses_one_below() {
    let group = group(2, 10);
    let identity = identity(10, &[]);
    let feature = FeatureId::mint("f3d:model:feature#query-lifetime").unwrap();
    let bytes = u64::try_from(4 * std::mem::size_of::<&DesignEdgeIdentityOperand>()).unwrap();
    let mut policy = scratch_policy();
    policy.limits.max_materialized_bytes = bytes - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            std::slice::from_ref(&identity),
            None,
            &feature,
            ctx,
        ) else {
            panic!("actual matched identity backing must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(original.operation, "f3d edge group matched identity");
        assert_eq!((original.used, original.additional), (0, bytes));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(sorted_transition_slots(&[], ctx, "later empty transition"), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

fn accepted_query(route: &str, ctx: &DecodeContext<'_>) -> EdgeSelection {
    let group = pair_group();
    let feature = FeatureId::mint("f3d:model:feature#query-lifetime").unwrap();
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.resolved_edge_slot = Some(17);
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.resolved_edge_slot = Some(18);
    if route == "surface" {
        let structure = crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure {
            clauses: std::array::from_fn(|_| {
                crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                    fields: Vec::new(),
                    face_reference_ordinals: [0, 0],
                    edge_reference_ordinals: [0, 0],
                    entries: Vec::new(),
                }
            }),
        };
        first.surface_patch_recipe_structure = Some(structure.clone());
        second.surface_patch_recipe_structure = Some(structure);
    }
    let recipes = [first, second];
    let mut first_identity = identity(10, &[(18, 5.0), (17, 3.0)]);
    let mut second_identity = identity(11, &[(17, 3.0), (18, 5.0)]);
    if route == "identity" {
        first_identity.resolved_edge_slot = Some(17);
        second_identity.resolved_edge_slot = Some(18);
    }
    let identities = [first_identity, second_identity];
    match route {
        "transition" => resolved_edge_treatment_group_with_corners(
            &group,
            EdgeTreatmentInputs {
                groups: std::slice::from_ref(&group),
                operands: &[],
                identity_operands: &identities,
                vertex_operands: &[],
                histories: &[],
                previous_state_id: Some(7),
                feature_id: &feature,
                treatment_radius: None,
            },
            ctx,
        )
        .unwrap(),
        "identity" => resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            &identities,
            Some(7),
            &feature,
            ctx,
        )
        .unwrap(),
        _ => resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            &recipes,
            &[],
            Some(7),
            &feature,
            ctx,
        )
        .unwrap(),
    }
}

fn assert_accepted_query(route: &str) {
    crate::test_support::with_decode_policy(&scratch_policy(), |ctx| {
        let selection = accepted_query(route, ctx);
        let EdgeSelection::Historical { edges, native, .. } = &selection else {
            panic!("complete native query proof");
        };
        assert_eq!(native.as_str(), "f3d:test:group-2");
        let prefix =
            crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("query-lifetime"), 7);
        assert_eq!(
            edges.as_slice(),
            [
                crate::ids::history_input_edge_id(&prefix, 17),
                crate::ids::history_input_edge_id(&prefix, 18)
            ]
        );
        drop(
            ctx.reserve_scoped(
                QUERY_SCRATCH,
                "reuse all query scratch while historical selection lives",
            )
            .unwrap(),
        );
        assert_eq!(ctx.resource_refusal(), None);
        drop(selection);
    });
}

#[test]
fn edge_query_lifetime_exact_selection_refunds_query_backing() {
    assert_accepted_query("exact");
}

#[test]
fn edge_query_lifetime_surface_selection_refunds_query_backing() {
    assert_accepted_query("surface");
}

#[test]
fn edge_query_lifetime_identity_selection_refunds_query_backing() {
    assert_accepted_query("identity");
}

#[test]
fn edge_query_lifetime_uniform_transition_refunds_each_comparison_backing() {
    assert_accepted_query("transition");
}

#[test]
fn edge_query_lifetime_single_unresolved_recipe_skips_impossible_partial_query() {
    let group = group(2, 10);
    let recipe = recipe_edge_operand(10, &[17, 18], &[]);
    let feature = FeatureId::mint("f3d:model:feature#query-lifetime").unwrap();
    let mut policy = scratch_policy();
    policy.limits.max_collection_items = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::CollectionItems,
            "f3d combined edge group slot",
            None,
        );
        let selection = resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            std::slice::from_ref(&recipe),
            &[],
            Some(7),
            &feature,
            ctx,
        )
        .unwrap();
        assert!(matches!(selection, EdgeSelection::Native(id) if id == group.id));
        assert_eq!(ctx.resource_refusal(), None);
        drop(
            ctx.reserve_scoped(QUERY_SCRATCH, "reuse singleton unresolved recipe scratch")
                .unwrap(),
        );
    });
}
