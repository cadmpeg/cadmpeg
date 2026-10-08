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
