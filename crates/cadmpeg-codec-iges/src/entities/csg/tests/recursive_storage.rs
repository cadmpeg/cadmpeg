// SPDX-License-Identifier: Apache-2.0
use super::*;
use super::super::{boolean_tree_is_valid, BooleanTerm, BooleanValidation};

fn nested_boolean_input() -> CsgProjectionInput {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 180, form: 0, label: "ROOT".into(), status: "00000000",
            parameters: "180,3,-3,-5,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 180, form: 0, label: "CHILD".into(), status: "00000000",
            parameters: "180,3,-5,-7,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 150, form: 0, label: "BLOCK1".into(), status: "00000000",
            parameters: "150,2,3,4,1,2,3,1,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 150, form: 0, label: "BLOCK2".into(), status: "00000000",
            parameters: "150,2,3,4,1,2,3,1,0,0,0,0,1;".into(),
        },
    ]);
    super::parse_csg_projection_input(&bytes)
}

fn nested_boolean_definitions() -> BTreeMap<u32, Vec<BooleanTerm>> {
    // Each postfix definition has two operands followed by one binary operation.
    BTreeMap::from([
        (1, vec![BooleanTerm::Operand(3), BooleanTerm::Operand(5), BooleanTerm::Operation]),
        (3, vec![BooleanTerm::Operand(5), BooleanTerm::Operand(7), BooleanTerm::Operation]),
    ])
}

#[test]
fn boolean_child_depth_refusal_destroys_path_before_frame_storage() {
    let input = nested_boolean_input();
    let entries = input.directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let definitions = nested_boolean_definitions();
    for cap in [1, 2] {
        let mut policy = DecodePolicy::service();
        // Root D1 inserts its key; child D3 needs the second frame.
        policy.limits.max_recursion_depth = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut validation = BooleanValidation {
            path: BTreeSet::new(), memo: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "iges boolean validity memo").unwrap(),
        };
        let result = boolean_tree_is_valid(1, &entries, &definitions, &mut validation, &ctx);
        assert!(validation.path.is_empty());
        if cap == 1 {
            let Err(CodecError::ResourceLimit(first)) = result else {
                panic!("expected child depth refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
            assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
            assert_eq!(first.operation, "iges boolean tree validation");
            assert!(validation.memo.is_empty());
            drop(validation);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert!(result.unwrap());
            assert_eq!(validation.memo, BTreeMap::from([(1, true), (3, true)]));
            drop(validation);
            ctx.finish_session().unwrap();
        }
    }
}

fn assert_boolean_post_insertion_refusal(operation: &str) {
    let input = nested_boolean_input();
    let entries = input.directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let definitions = nested_boolean_definitions();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation, |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut validation = BooleanValidation {
            path: BTreeSet::new(), memo: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "iges boolean validity memo").unwrap(),
        };
        let result = boolean_tree_is_valid(1, &entries, &definitions, &mut validation, &ctx);
        assert!(validation.path.is_empty());
        let Err(CodecError::ResourceLimit(first)) = &result else {
            panic!("expected post-insertion refusal")
        };
        drop(validation);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == *first));
        result
    });
}

#[test]
fn boolean_directory_refusal_destroys_path_before_frame_storage() {
    assert_boolean_post_insertion_refusal("iges boolean directory lookup");
}

#[test]
fn boolean_definition_refusal_destroys_path_before_frame_storage() {
    assert_boolean_post_insertion_refusal("iges boolean definition lookup");
}

#[test]
fn boolean_term_refusal_destroys_path_before_frame_storage() {
    assert_boolean_post_insertion_refusal("iges boolean term validation");
}

#[test]
fn boolean_operand_refusal_destroys_path_before_frame_storage() {
    assert_boolean_post_insertion_refusal("iges boolean operand directory lookup");
}

#[test]
fn boolean_removal_refusal_destroys_path_before_frame_storage() {
    assert_boolean_post_insertion_refusal("iges boolean path removal");
}

#[test]
fn boolean_memo_refusal_destroys_path_before_frame_storage() {
    assert_boolean_post_insertion_refusal("iges boolean validity memo");
}

#[test]
fn boolean_cycle_remains_invalid_without_losing_ancestor_cleanup() {
    let input = nested_boolean_input();
    let entries = input.directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let mut definitions = nested_boolean_definitions();
    definitions.get_mut(&3).unwrap()[0] = BooleanTerm::Operand(1);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut validation = BooleanValidation {
        path: BTreeSet::new(), memo: BTreeMap::new(),
        storage: ctx.reserve_scoped(0, "iges boolean validity memo").unwrap(),
    };
    assert!(!boolean_tree_is_valid(1, &entries, &definitions, &mut validation, &ctx).unwrap());
    assert!(validation.path.is_empty());
    assert_eq!(validation.memo, BTreeMap::from([(1, false), (3, false)]));
    drop(validation);
    ctx.finish_session().unwrap();
}

fn node_bytes<K, V>() -> u64 {
    u64::try_from(11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
        + 16 * std::mem::size_of::<usize>() + 2 * std::mem::align_of::<K>()
            .max(std::mem::align_of::<V>()).max(std::mem::align_of::<usize>())).unwrap()
}

#[test]
fn boolean_success_destroys_empty_root_and_keeps_only_actual_memo_storage() {
    let input = nested_boolean_input();
    let entries = input.directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let definitions = nested_boolean_definitions();
    let path_node = node_bytes::<u32, ()>();
    let memo_node = node_bytes::<u32, bool>();
    // Both frames share one path node. Two memo keys need one memo node.
    for cap in [path_node + memo_node - 1, path_node + memo_node] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 4;
        policy.limits.max_recursion_depth = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut validation = BooleanValidation {
            path: BTreeSet::new(), memo: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "test Boolean memo storage").unwrap(),
        };
        let result = boolean_tree_is_valid(1, &entries, &definitions, &mut validation, &ctx);
        assert!(validation.path.is_empty());
        if cap == path_node + memo_node {
            assert!(result.unwrap());
            assert_eq!(validation.memo, BTreeMap::from([(1, true), (3, true)]));
            let path_released = ctx.reserve_scoped(path_node, "test destroyed Boolean root").unwrap();
            drop(path_released);
            drop(validation);
            let all_released = ctx.reserve_scoped(cap, "test destroyed Boolean memo").unwrap();
            drop(all_released);
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(first)) = result else { panic!("expected child memo refusal") };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "iges boolean validity memo");
            assert_eq!((first.limit, first.used, first.additional), (cap, path_node, memo_node));
            assert!(validation.memo.is_empty());
            drop(validation);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
}

#[test]
fn boolean_missing_entry_and_definition_destroy_empty_roots_without_memo() {
    let input = nested_boolean_input();
    let entries = input.directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let definitions = BTreeMap::new();
    let node = node_bytes::<u32, ()>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = node;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 2;
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut validation = BooleanValidation {
        path: BTreeSet::new(), memo: BTreeMap::new(),
        storage: ctx.reserve_scoped(0, "test absent Boolean memo").unwrap(),
    };
    // D99 is absent; D1 exists but has no definition. Each root is destroyed.
    for sequence in [99, 1] {
        assert!(!boolean_tree_is_valid(sequence, &entries, &definitions, &mut validation, &ctx).unwrap());
        assert!(validation.path.is_empty());
        assert!(validation.memo.is_empty());
        let released = ctx.reserve_scoped(node, "test destroyed absent Boolean root").unwrap();
        drop(released);
    }
    drop(validation);
    ctx.finish_session().unwrap();
}
