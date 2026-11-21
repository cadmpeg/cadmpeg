// SPDX-License-Identifier: Apache-2.0

use super::{merge_operand_list_bindings, tool_replacement_loss, unbound_refusals};
use crate::history::CombineOperandSide;
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::BodyId;

fn bodies(name: &str) -> BodySelection {
    BodySelection::Bodies(
        DistinctMembers::try_from(vec![BodyId::mint(format!("f3d:test:body#{name}")).unwrap()])
            .unwrap(),
    )
}

fn native(name: &str) -> BodySelection {
    BodySelection::Native(format!("f3d:test:selection#{name}"))
}

fn historical(slot: i64) -> BodySelection {
    let prefix =
        crate::ids::feature_input_prefix(&FeatureId::mint("f3d:test:feature#combine").unwrap(), 4);
    BodySelection::historical(
        crate::ids::history_input_state_id(&prefix),
        vec![crate::ids::history_input_body_id(&prefix, slot)],
        "f3d:test:selection#tools".to_owned(),
    )
    .unwrap()
}

#[test]
fn an_operand_that_its_slot_leaves_native_takes_its_recipe_binding() {
    assert_eq!(
        merge_operand_list_bindings(
            (bodies("target"), native("tools")),
            (native("target"), bodies("tool")),
        ),
        (bodies("target"), bodies("tool"))
    );
    assert_eq!(
        merge_operand_list_bindings(
            (native("target"), historical(2)),
            (bodies("target"), native("tools")),
        ),
        (bodies("target"), historical(2))
    );
    assert_eq!(
        merge_operand_list_bindings(
            (native("target"), native("tools")),
            (native("target"), native("tools")),
        ),
        (native("target"), native("tools"))
    );
}

#[test]
fn recipe_tools_that_name_model_bodies_replace_slot_tools() {
    assert_eq!(
        merge_operand_list_bindings(
            (bodies("target"), historical(2)),
            (native("target"), bodies("tool")),
        ),
        (bodies("target"), bodies("tool"))
    );
}

#[test]
fn a_recipe_tool_replacement_charges_a_loss_that_names_both_tools() {
    let feature = FeatureId::mint("f3d:test:feature#combine").unwrap();
    let (_, merged) = merge_operand_list_bindings(
        (bodies("target"), historical(2)),
        (native("target"), bodies("tool")),
    );
    let loss = tool_replacement_loss(&feature, &historical(2), &merged)
        .expect("a replacement of bound slot tools");
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureCombineToolRecipeReplacementAssumed.kind()
    );
    assert!(
        loss.message.contains("f3d:test:feature#combine"),
        "{}",
        loss.message
    );
    assert!(
        loss.message.contains("f3d:test:body#tool"),
        "{}",
        loss.message
    );
    let BodySelection::Historical { bodies: slot, .. } = historical(2) else {
        unreachable!("a historical selection")
    };
    assert!(
        loss.message.contains(slot.as_slice()[0].as_str()),
        "{}",
        loss.message
    );
    assert_eq!(
        tool_replacement_loss(&feature, &historical(2), &historical(2)),
        None
    );
    assert_eq!(
        tool_replacement_loss(&feature, &native("tools"), &bodies("tool")),
        None
    );
}

#[test]
fn a_recipe_binding_that_overlaps_the_other_operand_is_refused() {
    assert_eq!(
        merge_operand_list_bindings(
            (bodies("one"), native("tools")),
            (native("target"), bodies("one")),
        ),
        (bodies("one"), native("tools"))
    );
    assert_eq!(
        merge_operand_list_bindings(
            (native("target"), bodies("one")),
            (bodies("one"), bodies("one")),
        ),
        (native("target"), bodies("one"))
    );
}

#[test]
fn only_operands_that_stay_native_keep_their_slot_refusals() {
    let refusals = || {
        vec![
            (
                CombineOperandSide::Target,
                F3dLossCode::FeatureCombineOperandBodyUnbound.note("target"),
            ),
            (
                CombineOperandSide::Tools,
                F3dLossCode::FeatureCombineOperandBodyUnbound.note("tools"),
            ),
        ]
    };
    let messages = |target: BodySelection, tools: BodySelection| {
        unbound_refusals((&target, &tools), refusals())
            .into_iter()
            .map(|loss| loss.message)
            .collect::<Vec<_>>()
    };
    assert_eq!(messages(bodies("target"), native("tools")), ["tools"]);
    assert_eq!(messages(native("target"), bodies("tool")), ["target"]);
    assert_eq!(
        messages(native("target"), native("tools")),
        ["target", "tools"]
    );
    assert!(messages(bodies("target"), bodies("tool")).is_empty());
}
