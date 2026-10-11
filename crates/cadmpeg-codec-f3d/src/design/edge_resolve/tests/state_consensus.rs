// SPDX-License-Identifier: Apache-2.0

use super::super::{
    resolved_edge_group, resolved_surface_patch_edge_group, shared_recipe_state_id,
};
use super::{group, recipe_edge_operand, recipe_reference};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{EdgeSelection, FeatureId};

fn operands(states: &[Option<i64>]) -> Vec<super::DesignEdgeOperand> {
    states
        .iter()
        .enumerate()
        .map(|(i, &state)| {
            let mut operand = recipe_edge_operand(10 + u32::try_from(i).unwrap(), &[], &[]);
            operand.recipe_state_id = state;
            operand
        })
        .collect()
}

fn measure(states: &[Option<i64>]) -> (Option<i64>, u64) {
    let operands = operands(states);
    crate::test_support::with_decode_context(|ctx| {
        let result = shared_recipe_state_id(&operands.iter().collect::<Vec<_>>(), ctx).unwrap();
        let Err(CodecError::ResourceLimit(usage)) = ctx.charge_work(
            DecodePolicy::service().limits.max_work_units + 1,
            "measure edge recipe state work",
        ) else {
            panic!("finite ceiling exposes cumulative work");
        };
        assert_eq!(usage.dimension, ResourceDimension::WorkUnits);
        (result, usage.used)
    })
}

#[test]
fn edge_recipe_state_empty_uses_only_metadata() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(shared_recipe_state_id(&[], ctx).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn edge_recipe_state_singleton_uses_the_fixed_first_record() {
    assert_eq!(measure(&[Some(7)]), (Some(7), 0));
}

#[test]
fn edge_recipe_state_missing_first_skips_the_suffix() {
    let short = [None];
    let mut long = short.to_vec();
    long.extend([Some(7); 1024]);
    assert_eq!(measure(&short), (None, 0));
    assert_eq!(measure(&long), measure(&short));
}

#[test]
fn edge_recipe_state_conflict_stops_at_the_first_reached_record() {
    let short = [Some(7), Some(8)];
    let mut long = short.to_vec();
    long.extend([Some(7); 1024]);
    assert_eq!(measure(&short), (None, 1));
    assert_eq!(measure(&long), measure(&short));
}

#[test]
fn edge_recipe_state_missing_later_stops_at_that_record() {
    let short = [Some(7), None];
    let mut long = short.to_vec();
    long.extend([Some(7); 1024]);
    assert_eq!(measure(&short), (None, 1));
    assert_eq!(measure(&long), measure(&short));
}

#[test]
fn edge_recipe_state_late_conflict_pays_only_the_reached_suffix() {
    assert_eq!(measure(&[Some(7), Some(8)]), (None, 1));
    assert_eq!(
        measure(&[Some(7), Some(7), Some(7), Some(7), Some(8)]),
        (None, 4)
    );
}

#[test]
fn edge_recipe_state_equal_records_are_scanned_once() {
    assert_eq!(
        measure(&[Some(7), Some(7), Some(7), Some(7), Some(7)]),
        (Some(7), 4)
    );
}

#[test]
fn edge_recipe_state_step_refuses_before_reading_the_next_state() {
    let operands = operands(&[Some(7), Some(7)]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(refusal)) =
            shared_recipe_state_id(&[&operands[0], &operands[1]], ctx)
        else {
            panic!("the next state must be admitted");
        };
        assert_eq!(refusal.operation, "scan F3D shared edge recipe states");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
    policy.limits.max_work_units = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            shared_recipe_state_id(&[&operands[0], &operands[1]], ctx).unwrap(),
            Some(7)
        );
    });
}

fn selection(
    route: &str,
    states: [Option<i64>; 2],
    previous: Option<i64>,
    ctx: &DecodeContext<'_>,
) -> Result<EdgeSelection, CodecError> {
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
    let mut operands = operands(&states);
    for (operand, slot) in operands.iter_mut().zip([17, 18]) {
        operand.resolved_edge_slot = Some(slot);
        operand.recipe_references = vec![recipe_reference(&[slot])];
        if route == "generic_surface" {
            operand.surface_patch_recipe_structure = Some(
                crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure {
                    clauses: std::array::from_fn(|_| {
                        crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                            fields: Vec::new(),
                            face_reference_ordinals: [0, 0],
                            edge_reference_ordinals: [0, 0],
                            entries: Vec::new(),
                        }
                    }),
                },
            );
        }
    }
    let feature = FeatureId::mint("f3d:model:feature#state-consensus").unwrap();
    if route == "surface" {
        resolved_surface_patch_edge_group(
            &group,
            std::slice::from_ref(&group),
            &operands,
            &[],
            previous,
            &feature,
            ctx,
        )
    } else {
        resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            &operands,
            &[],
            previous,
            &feature,
            ctx,
        )
    }
}

fn assert_historical(selection: EdgeSelection) {
    let EdgeSelection::Historical { edges, .. } = selection else {
        panic!("complete state and edge proof");
    };
    let prefix = crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("state-consensus"), 7);
    assert_eq!(
        edges.as_slice(),
        [
            crate::ids::history_input_edge_id(&prefix, 17),
            crate::ids::history_input_edge_id(&prefix, 18)
        ]
    );
}

fn assert_caller(route: &str) {
    crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "scan F3D shared edge recipe states",
        0,
        |ctx| selection(route, [Some(7), Some(7)], None, ctx),
    );
    crate::test_support::with_decode_context(|ctx| {
        assert_historical(selection(route, [Some(7), Some(7)], None, ctx).unwrap());
    });
}

#[test]
fn edge_recipe_state_surface_caller_preserves_native_identity_and_refusal() {
    assert_caller("surface");
}

#[test]
fn edge_recipe_state_generic_surface_caller_preserves_native_identity_and_refusal() {
    assert_caller("generic_surface");
}

#[test]
fn edge_recipe_state_generic_caller_preserves_native_identity_and_refusal() {
    assert_caller("generic");
}

#[test]
fn edge_recipe_state_supplied_previous_state_skips_all_recipe_queries() {
    for route in ["surface", "generic_surface", "generic"] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                "scan F3D shared edge recipe states",
                None,
            );
            assert_historical(selection(route, [Some(8), Some(9)], Some(7), ctx).unwrap());
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn edge_recipe_state_missing_proof_preserves_readable_native_edges() {
    crate::test_support::with_decode_context(|ctx| {
        let EdgeSelection::Edges(edges) = selection("surface", [Some(7), None], None, ctx).unwrap()
        else {
            panic!("exact native edges stay readable without a common state");
        };
        assert_eq!(
            edges.as_slice(),
            [
                cadmpeg_ir::ids::EdgeId::mint("f3d:test:edge#17").unwrap(),
                cadmpeg_ir::ids::EdgeId::mint("f3d:test:edge#18").unwrap(),
            ]
        );
        for route in ["generic_surface", "generic"] {
            assert!(
                matches!(selection(route, [Some(7), None], None, ctx).unwrap(),
                EdgeSelection::Native(id) if id == "f3d:test:group-2")
            );
        }
    });
}
