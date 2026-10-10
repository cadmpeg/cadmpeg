// SPDX-License-Identifier: Apache-2.0
use super::super::{
    boundary_edge_count, complete_counted_face_recipe, counted_face_recipe_frame,
    resolve_face_operand_history_candidate_from, resolve_face_operand_history_candidates,
    resolve_face_operand_support_candidate, unique_preceding_face_boundaries,
    unique_stable_face_slots, valid_preceding_face_boundaries,
};
use super::{
    boundary, face, reference, stable_bounded_face_operand, support, surface_delete_face_operand,
};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_refusal(error: &CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

#[test]
fn counted_matching_node_count_skips_boundary_proofs() {
    let mut operand = stable_bounded_face_operand();
    operand.historical_support_contexts = (0..10_000).map(|_| support(10, &[])).collect();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2; // One node and its end probe.
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            complete_counted_face_recipe(ctx, &operand).unwrap(),
            Some(1)
        );
    });
}

#[test]
fn counted_missing_structure_stops_before_original_node_tail() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_nodes[0].recipe_structure = None;
    let node = operand.recipe_nodes[0].clone();
    operand
        .recipe_nodes
        .extend((0..10_000).map(|_| node.clone()));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(complete_counted_face_recipe(ctx, &operand).unwrap(), None);
    });
}

#[test]
fn counted_node_proofs_refuse_work() {
    let operand = stable_bounded_face_operand();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d counted face structure source",
        0,
        |ctx| complete_counted_face_recipe(ctx, &operand),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d counted face structure source",
    );
    let operand = surface_delete_face_operand();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d counted face frame source",
        0,
        |ctx| counted_face_recipe_frame(ctx, &operand),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d counted face frame source",
    );
}

#[test]
fn preceding_boundaries_preserve_duplicate_expected_set_and_reject_invalid_members() {
    let mut context = support(10, &[(100, 4), (101, 4)]);
    context.preceding_face_slots.push(100);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            valid_preceding_face_boundaries(ctx, &context).unwrap(),
            Some(context.preceding_face_boundaries.as_slice())
        );
    });
    for variant in 0..3 {
        let mut context = context.clone();
        match variant {
            0 => context.preceding_face_boundaries[1].face_slot = 100,
            1 => context.preceding_face_boundaries[1].face_slot = 102,
            _ => {
                context.preceding_face_boundaries.pop();
            }
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            assert_eq!(
                valid_preceding_face_boundaries(ctx, &context).unwrap(),
                None
            );
            drop(
                ctx.reserve_scoped(4096, "test rejected boundary scratch release")
                    .unwrap(),
            );
        });
    }
}

#[test]
fn preceding_empty_loops_stop_before_expected_index_and_boundary_tail() {
    let mut context = support(10, &[(100, 4)]);
    context.preceding_face_boundaries[0].loops.clear();
    context
        .preceding_face_boundaries
        .extend((0..10_000).map(|_| boundary(100, 4)));
    context.preceding_face_slots.extend(0..10_000);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            valid_preceding_face_boundaries(ctx, &context).unwrap(),
            None
        );
    });
}

#[test]
fn preceding_boundary_operations_refuse_work_and_storage() {
    let context = support(10, &[(100, 4), (101, 4)]);
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d preceding boundary source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d preceding expected face source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d preceding expected face lookup",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d preceding expected face index",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d preceding expected face index",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d preceding boundary face index",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d preceding boundary face index",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            valid_preceding_face_boundaries(ctx, &context).map(|_| ())
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn unique_boundaries_keep_first_occurrence_order_and_identical_repeats() {
    let contexts = [
        support(10, &[(101, 4), (100, 4)]),
        support(11, &[(100, 4), (102, 4)]),
    ];
    crate::test_support::with_decode_context(|ctx| {
        let result = unique_preceding_face_boundaries(ctx, &contexts)
            .unwrap()
            .unwrap();
        assert_eq!(
            result
                .boundaries
                .iter()
                .map(|boundary| boundary.face_slot)
                .collect::<Vec<_>>(),
            vec![101, 100, 102]
        );
        assert!(std::ptr::eq(
            result.boundaries[1],
            &raw const contexts[0].preceding_face_boundaries[1]
        ));
    });
}

#[test]
fn unique_boundary_conflict_and_duplicate_active_stop_before_context_tail() {
    for duplicate_active in [false, true] {
        let mut contexts = vec![support(10, &[(100, 4)]), support(11, &[(100, 5)])];
        if duplicate_active {
            contexts[1].active_face_slot = 10;
        }
        contexts.extend((0..10_000).map(|_| support(12, &[(102, 4)])));
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 4096; // Two small contexts, indexes, and one four-row equality.
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            assert!(unique_preceding_face_boundaries(ctx, &contexts)
                .unwrap()
                .is_none());
            drop(
                ctx.reserve_scoped(4096, "test unique boundary scratch release")
                    .unwrap(),
            );
        });
    }
}

#[test]
fn unique_boundary_operations_refuse_work_and_storage() {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(100, 4)])];
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d unique preceding context source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d unique preceding boundary source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d unique preceding boundary equality",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d unique preceding active index",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d unique preceding active index",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d unique preceding boundary index",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d unique preceding boundary index",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d unique preceding boundary output",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d unique preceding boundary output",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            unique_preceding_face_boundaries(ctx, &contexts).map(|_| ())
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn boundary_count_uses_exact_metadata_for_all_coedge_variants() {
    use crate::records::topology::historical_context::{
        DesignHistoricalLoopBoundary as Boundary, DesignHistoricalLoopCoedge,
        DesignHistoricalLoopPoint, DesignHistoricalLoopPosition, DesignHistoricalLoopVertex,
    };
    let coedge = DesignHistoricalLoopCoedge {
        coedge_slot: 1,
        edge_slot: 2,
    };
    let vertex = DesignHistoricalLoopVertex {
        coedge: coedge.clone(),
        vertex_slot: 3,
    };
    let point = DesignHistoricalLoopPoint {
        vertex: vertex.clone(),
        point_slot: 4,
    };
    for value in [
        Boundary::Coedges(vec![coedge; 10_000]),
        Boundary::Vertices(vec![vertex; 10_000]),
        Boundary::Points(vec![point.clone(); 10_000]),
        Boundary::Positions(vec![
            DesignHistoricalLoopPosition {
                point,
                position: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
            };
            10_000
        ]),
    ] {
        let mut boundary = boundary(100, 1);
        boundary.loops[0].boundary = value;
        let mut policy = DecodePolicy::service();
        // One face and one loop each require a source step and an end probe.
        policy.limits.max_work_units = 4;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            assert_eq!(boundary_edge_count(ctx, [&boundary]).unwrap(), Some(10_000));
        });
    }
}

#[test]
fn boundary_count_empty_loop_stops_before_original_tail() {
    let mut boundaries = vec![boundary(100, 0)];
    boundaries.extend((0..10_000).map(|_| boundary(101, 4)));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(boundary_edge_count(ctx, &boundaries).unwrap(), None);
    });
}

#[test]
fn boundary_count_sources_refuse_work() {
    let boundary = boundary(100, 4);
    for operation in [
        "f3d boundary count face source",
        "f3d boundary count loop source",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| boundary_edge_count(ctx, [&boundary]),
        );
        assert_refusal(&error, ResourceDimension::WorkUnits, operation);
    }
}

#[test]
fn direct_history_match_stops_before_reference_tail_without_output_copies() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_kind = crate::records::recipes::ConstructionRecipeKind::Face;
    operand.preceding_candidate_faces = vec![face(10)];
    operand.recipe_references = vec![reference(10, "selected", 1)];
    operand
        .recipe_references
        .extend((0..10_000).map(|_| reference(11, "unused", 2)));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 256; // One reference, one complete ID comparison, and one suffix search.
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolve_face_operand_history_candidates(ctx, &operand).unwrap(),
            Some(10)
        );
    });
}

#[test]
fn direct_history_referenced_lane_prevents_fallback_and_empty_parents_admit_before_fallback() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_kind = crate::records::recipes::ConstructionRecipeKind::Face;
    operand.preceding_candidate_faces = vec![face(10)];
    operand.candidate_faces = vec![face(10)];
    operand.recipe_references = vec![reference(11, "context", 1)];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            resolve_face_operand_history_candidates(ctx, &operand).unwrap(),
            None
        );
    });
    operand.recipe_references[0].candidate_faces.clear();
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            resolve_face_operand_history_candidates(ctx, &operand).unwrap(),
            Some(10)
        );
    });
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d direct face reference source",
        0,
        |ctx| resolve_face_operand_history_candidates(ctx, &operand),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d direct face reference source",
    );
}

#[test]
fn direct_history_queries_admit_candidate_membership() {
    let mut operand = stable_bounded_face_operand();
    operand.preceding_candidate_faces = vec![face(10)];
    operand.candidate_faces = vec![face(10)];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d direct face candidate membership",
        0,
        |ctx| resolve_face_operand_history_candidates(ctx, &operand),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d direct face candidate membership",
    );
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d direct supplied face membership",
        0,
        |ctx| resolve_face_operand_history_candidate_from(ctx, &operand, &operand.candidate_faces),
    );
    assert_refusal(
        &error,
        ResourceDimension::WorkUnits,
        "f3d direct supplied face membership",
    );
}

#[test]
fn support_ambiguity_stops_before_active_face_and_context_tails() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_references = vec![reference(10, "selected", 1)];
    operand.recipe_references[0]
        .candidate_faces
        .extend((0..10_000).map(|_| face(12)));
    operand.historical_support_contexts = vec![support(10, &[(100, 4)]), support(10, &[(101, 4)])];
    operand
        .historical_support_contexts
        .extend((0..10_000).map(|_| support(12, &[(102, 4)])));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 256; // Two contexts use the cached first active slot.
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolve_face_operand_support_candidate(ctx, &operand).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(4096, "test support index release")
                .unwrap(),
        );
    });
}

#[test]
fn support_cache_reuses_parsed_slots_for_later_contexts() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_references = vec![reference(10, "selected", 1)];
    operand.recipe_references[0].candidate_faces.push(face(11));
    operand.historical_support_contexts = vec![support(11, &[(100, 4)]), support(10, &[(100, 4)])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2; // One cache entry per distinct parsed active slot.
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolve_face_operand_support_candidate(ctx, &operand).unwrap(),
            Some(100)
        );
    });
}

#[test]
fn support_sources_and_cache_refuse_limits() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_references = vec![reference(10, "selected", 1)];
    operand.recipe_references[0].candidate_faces.push(face(11));
    operand.historical_support_contexts = vec![support(11, &[(100, 4)])];
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d support initial face source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d support active face source",
        ),
        (ResourceDimension::WorkUnits, "f3d support context source"),
        (
            ResourceDimension::WorkUnits,
            "f3d support predecessor source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d support active face lookup",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d support active face index",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d support active face index",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            resolve_face_operand_support_candidate(ctx, &operand)
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn stable_slots_and_surface_delete_rejections_release_scratch_without_retained_copies() {
    let faces = [face(11), face(10), face(11)];
    let mut operand = surface_delete_face_operand();
    operand.changed_candidate_faces = vec![face(10), face(12)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(unique_stable_face_slots(ctx, &faces).unwrap().is_none());
        assert_eq!(
            super::super::resolve_surface_delete_face_history_set(ctx, &operand).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(4096, "test stable and SurfaceDeleteFace release")
                .unwrap(),
        );
    });
}

#[test]
fn bounded_cardinality_sources_comparisons_and_accepted_output_refuse_limits() {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(100, 4)])];
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d bounded cardinality context source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d bounded face cardinality equality",
        ),
        (
            ResourceDimension::RetainedBytes,
            "f3d bounded accepted face slots",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            super::super::bounded_face_candidate_by_boundary_cardinality(ctx, 4, &contexts)
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn bounded_cardinality_conflict_keeps_no_output_and_releases_candidate_storage() {
    let mut contexts = vec![support(10, &[(100, 4)]), support(11, &[(101, 4)])];
    contexts.extend((0..10_000).map(|_| support(12, &[(102, 4)])));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    // Two one-face boundaries, numeric indexes, counts, sorts and one comparison fit.
    policy.limits.max_work_units = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            super::super::bounded_face_candidate_by_boundary_cardinality(ctx, 4, &contexts)
                .unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(4096, "test cardinality candidate storage release")
                .unwrap(),
        );
    });
}

#[test]
fn stable_slot_and_surface_delete_proofs_refuse_work_and_accepted_output() {
    let faces = [face(10), face(11)];
    for operation in [
        "f3d stable face slot source",
        "f3d stable face slot uniqueness",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| unique_stable_face_slots(ctx, &faces).map(|_| ()),
        );
        assert_refusal(&error, ResourceDimension::WorkUnits, operation);
    }
    let operand = surface_delete_face_operand();
    for (dimension, operation) in [
        (
            ResourceDimension::WorkUnits,
            "f3d SurfaceDeleteFace changed equality",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d SurfaceDeleteFace context source",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d SurfaceDeleteFace active membership",
        ),
        (
            ResourceDimension::RetainedBytes,
            "f3d SurfaceDeleteFace accepted slots",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            super::super::resolve_surface_delete_face_history_set(ctx, &operand)
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn retained_operand_ambiguity_stops_before_tail_without_mutation() {
    let (mut operand, group, _) = super::start_geometry_fixture();
    operand.candidate_faces = vec![face(10)];
    let mut operands = vec![operand.clone(), operand.clone()];
    operands.extend((0..10_000).map(|_| operand.clone()));
    let original = operands.clone();
    let mut policy = DecodePolicy::service();
    // Two operands, two one-member group queries and two full face comparisons fit.
    policy.limits.max_work_units = 1024;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(!super::super::retain_face_operand_resolution(
            ctx,
            &group,
            &mut operands,
            &face(10)
        )
        .unwrap());
    });
    assert_eq!(operands, original);
}

#[test]
fn retained_operand_sources_membership_and_kept_slot_refuse_limits() {
    let (mut operand, group, _) = super::start_geometry_fixture();
    operand.candidate_faces = vec![face(10)];
    for (dimension, operation) in [
        (ResourceDimension::WorkUnits, "visit F3D retained face operand"),
        (
            ResourceDimension::WorkUnits,
            "find F3D retained face group member",
        ),
        (
            ResourceDimension::WorkUnits,
            "find F3D retained face operand candidate",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d retained operand face slot",
        ),
        (
            ResourceDimension::RetainedBytes,
            "f3d retained operand face slot",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
            let mut operand = operand.clone();
            super::super::retain_face_operand_resolution(
                ctx,
                &group,
                std::slice::from_mut(&mut operand),
                &face(10),
            )
        });
        assert_refusal(&error, dimension, operation);
    }
}

#[test]
fn support_and_cardinality_empty_sources_skip_all_proof_work() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_references = vec![reference(10, "unused", 1)];
    operand.recipe_references[0]
        .candidate_faces
        .extend((0..10_000).map(|_| face(11)));
    operand.historical_support_contexts.clear();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolve_face_operand_support_candidate(ctx, &operand).unwrap(),
            None
        );
        assert_eq!(
            super::super::bounded_face_candidate_by_boundary_cardinality(ctx, 1, &[]).unwrap(),
            None
        );
    });
    operand.recipe_references[0].candidate_faces.clear();
    operand.historical_support_contexts = vec![support(10, &[(100, 4)])];
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            resolve_face_operand_support_candidate(ctx, &operand).unwrap(),
            None
        );
    });
}

#[test]
fn surface_delete_empty_changed_lane_skips_original_active_face_tail() {
    let mut operand = surface_delete_face_operand();
    operand
        .preceding_candidate_faces
        .extend((0..10_000).map(|_| face(12)));
    operand.changed_candidate_faces.clear();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2; // The one-node frame proof and its end probe.
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            super::super::resolve_surface_delete_face_history_set(ctx, &operand).unwrap(),
            None
        );
    });
}
