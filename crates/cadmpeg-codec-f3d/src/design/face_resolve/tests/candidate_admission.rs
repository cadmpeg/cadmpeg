// SPDX-License-Identifier: Apache-2.0
use super::super::{
    historical_face_operand_candidates, legacy_face_recipe_reference_candidates,
    nested_bounded_face_history_candidates, resolved_extrude_profile_active_faces,
    split_face_complete_candidate_slots,
};
use super::borrowed_candidates::{explicit_bounded_face_candidates, resolved_face_operand};
use super::{face, legacy_face_candidate_limit_fixture, reference, stable_bounded_face_operand};
use crate::records::recipes::ConstructionRecipeKind;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_refusal(error: &CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

fn referenced_operand(nested: bool) -> crate::records::topology::face::DesignFaceOperand {
    let mut operand = stable_bounded_face_operand();
    operand.candidate_faces.clear();
    operand.unreferenced_candidate_faces.clear();
    operand.alternate_selector_candidate_faces.clear();
    if !nested {
        operand.recipe_kind = ConstructionRecipeKind::Face;
    }
    operand.recipe_references = vec![reference(20, "second", 1), reference(10, "first", 2)];
    operand.recipe_references[0].alternate_selector_faces = vec![face(10), face(20)];
    operand.recipe_references[1]
        .candidate_faces
        .extend((0..1024).map(|_| face(10)));
    operand
}

#[test]
fn historical_and_nested_candidates_copy_only_sorted_distinct_ids() {
    for nested in [false, true] {
        let operand = referenced_operand(nested);
        let mut policy = DecodePolicy::service();
        // Two admitted identities and the four-slot Vec growth fit; duplicate copies cannot fit.
        policy.limits.max_retained_bytes = u64::try_from(
            face(10).as_str().len()
                + face(20).as_str().len()
                + 4 * std::mem::size_of::<cadmpeg_ir::ids::FaceId>(),
        )
        .unwrap();
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let result = if nested {
                nested_bounded_face_history_candidates(ctx, &operand)
                    .unwrap()
                    .unwrap()
            } else {
                historical_face_operand_candidates(ctx, &operand).unwrap()
            };
            assert_eq!(result, [face(10), face(20)]);
        });
    }
}

#[test]
fn historical_reference_parents_with_empty_children_refuse_original_work() {
    let mut operand = referenced_operand(false);
    for reference in &mut operand.recipe_references {
        reference.candidate_faces.clear();
        reference.alternate_selector_faces.clear();
    }
    let reference = operand.recipe_references[0].clone();
    operand.recipe_references = vec![reference; 1024];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let error = historical_face_operand_candidates(ctx, &operand).unwrap_err();
        assert_refusal(
            &error,
            ResourceDimension::WorkUnits,
            "f3d historical reference source",
        );
    });
}

#[test]
fn historical_candidate_sources_and_ordered_index_refuse_limits() {
    let operand = referenced_operand(false);
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d historical referenced candidate source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d historical referenced candidate index",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d historical referenced candidate index",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d historical referenced candidate index",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d historical accepted candidate source",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            historical_face_operand_candidates(ctx, &operand)
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn historical_fallback_keeps_original_order_and_duplicates() {
    let mut operand = stable_bounded_face_operand();
    operand.candidate_faces = vec![face(20), face(10), face(20)];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            historical_face_operand_candidates(ctx, &operand).unwrap(),
            operand.candidate_faces
        );
    });
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d historical fallback candidate source",
        0,
        |ctx| historical_face_operand_candidates(ctx, &operand),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d historical fallback candidate source",
    );
}

#[test]
fn nested_active_lane_rejection_skips_counted_node_tail() {
    let mut operand = stable_bounded_face_operand();
    let node = operand.recipe_nodes[0].clone();
    operand
        .recipe_nodes
        .extend((0..10_000).map(|_| node.clone()));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            nested_bounded_face_history_candidates(ctx, &operand).unwrap(),
            None
        );
    });
}

#[test]
fn legacy_mismatched_active_set_keeps_no_ids_and_releases_proof_storage() {
    let mut operand = legacy_face_candidate_limit_fixture();
    operand.candidate_faces = vec![face(20)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            legacy_face_recipe_reference_candidates(ctx, &operand, 201).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(4096, "test legacy proof release")
                .unwrap(),
        );
    });
}

#[test]
fn legacy_original_filtered_parents_and_member_proofs_refuse_work() {
    let operand = legacy_face_candidate_limit_fixture();
    for operation in [
        "f3d legacy face reference source",
        "f3d legacy selected candidate source",
        "f3d legacy selected face index",
        "f3d legacy active candidate source",
        "f3d legacy face set comparison",
        "f3d legacy face id comparison",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| legacy_face_recipe_reference_candidates(ctx, &operand, 201),
        );
        assert_refusal(&error, ResourceDimension::WorkUnits, operation);
    }
}

#[test]
fn resolved_missing_late_slot_keeps_no_ids_and_releases_cache() {
    let mut operand = stable_bounded_face_operand();
    operand.candidate_faces = vec![face(10)];
    operand.resolved_face_slots = vec![10, 11];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(resolved_face_operand(ctx, &operand).unwrap(), None);
        drop(
            ctx.reserve_scoped(4096, "test missing slot cache release")
                .unwrap(),
        );
    });
}

#[test]
fn resolved_slot_cache_keeps_first_identity_and_output_slot_order() {
    let mut operand = stable_bounded_face_operand();
    let first = cadmpeg_ir::ids::FaceId::mint("first:model:face#10").unwrap();
    let duplicate = cadmpeg_ir::ids::FaceId::mint("second:model:face#10").unwrap();
    operand.candidate_faces = vec![first.clone(), duplicate, face(11)];
    operand.resolved_face_slots = vec![11, 10, 11];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            resolved_face_operand(ctx, &operand).unwrap(),
            Some(vec![face(11), first, face(11)])
        );
    });
}

#[test]
fn resolved_slot_cache_stops_before_candidate_tail_and_admits_sources() {
    let mut operand = stable_bounded_face_operand();
    operand.candidate_faces = vec![face(10)];
    operand
        .candidate_faces
        .extend((0..10_000).map(|_| face(11)));
    operand.resolved_face_slots = vec![10, 10];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1024;
    policy.limits.max_collection_items = 5; // Cache and selected proofs fit; the candidate tail cannot fit.
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolved_face_operand(ctx, &operand).unwrap(),
            Some(vec![face(10), face(10)])
        );
    });
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d resolved face slot source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d resolved slot candidate source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d resolved face slot lookup",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d resolved face slot index",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d resolved face slot index",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d resolved face slot selection",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            resolved_face_operand(ctx, &operand)
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn resolved_reference_selection_keeps_first_occurrence_order() {
    let operand = referenced_operand(false);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            resolved_face_operand(ctx, &operand).unwrap(),
            Some(vec![face(20), face(10)])
        );
    });
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d referenced face reference source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d referenced face candidate source",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d referenced face membership",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d referenced face membership",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d referenced face selection",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            resolved_face_operand(ctx, &operand)
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn owned_explicit_and_resolved_candidate_lanes_admit_original_work() {
    for alternate in [false, true] {
        let mut operand = stable_bounded_face_operand();
        operand.historical_support_contexts.clear();
        let operation = if alternate {
            operand.alternate_selector_candidate_faces = vec![face(10), face(11)];
            "f3d resolved candidate source"
        } else {
            "f3d explicit bounded owned candidate source"
        };
        crate::test_support::with_decode_context(|ctx| {
            let selected = if alternate {
                resolved_face_operand(ctx, &operand)
            } else {
                explicit_bounded_face_candidates(ctx, &operand)
            }
            .unwrap()
            .unwrap();
            assert_eq!(selected, super::super::face_operand_candidates(&operand));
        });
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                if alternate {
                    resolved_face_operand(ctx, &operand)
                } else {
                    explicit_bounded_face_candidates(ctx, &operand)
                }
            },
        );
        assert_refusal(&error, ResourceDimension::WorkUnits, operation);
    }
}

#[test]
fn split_candidate_rejections_keep_no_slots_and_release_storage() {
    let mut operand = stable_bounded_face_operand();
    operand.resolved_active_face = Some(face(20));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            split_face_complete_candidate_slots(ctx, &operand).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(4096, "test split candidate proof release")
                .unwrap(),
        );
    });
}

#[test]
fn split_candidate_original_sources_containment_and_output_refuse_limits() {
    let mut operand = stable_bounded_face_operand();
    operand.resolved_active_face = Some(face(10));
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d SplitFace complete candidate source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d SplitFace preceding candidate source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d SplitFace complete containment",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d SplitFace preceding membership",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d SplitFace complete candidate slot",
        ),
        (
            ResourceDimension::RetainedBytes,
            "f3d SplitFace accepted candidate slots",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            split_face_complete_candidate_slots(ctx, &operand)
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn extrude_active_invalid_later_operand_keeps_no_ids_and_releases_storage() {
    let operand = legacy_face_candidate_limit_fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolved_extrude_profile_active_faces(ctx, &[0, 1], &[operand]).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(4096, "test active profile proof release")
                .unwrap(),
        );
    });
}

#[test]
fn extrude_active_sources_membership_and_selection_refuse_limits() {
    let operand = legacy_face_candidate_limit_fixture();
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d Extrude active operand source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d Extrude active candidate source",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d Extrude active face membership",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d Extrude active face membership",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d Extrude active face selection",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            resolved_extrude_profile_active_faces(ctx, &[0], std::slice::from_ref(&operand))
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn empty_extrude_active_index_source_skips_all_proof_work() {
    let operand = legacy_face_candidate_limit_fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolved_extrude_profile_active_faces(ctx, &[], &[operand]).unwrap(),
            None
        );
    });
}

#[test]
fn generated_resolved_slots_admit_original_source_work() {
    let mut operand = stable_bounded_face_operand();
    operand.candidate_faces.clear();
    operand.resolved_face_slots = vec![10];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d generated face slot source",
        0,
        |ctx| resolved_face_operand(ctx, &operand),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d generated face slot source",
    );
}
