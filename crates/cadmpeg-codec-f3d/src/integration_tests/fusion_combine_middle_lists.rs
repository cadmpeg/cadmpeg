// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of operand-list Combines that pin the created-body and
//! consumed-body lists.

use cadmpeg_core::serde_erase::json;
use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::Value;

use crate::F3dCodec;

/// The native operation of the one Combine scope of a corpus file and the
/// decode loss codes.
fn combine_operation(bytes: &[u8]) -> (Value, Vec<String>) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let codes = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect::<Vec<_>>();
    let ir = json::to_value(result.ir()).expect("serialize the IR");
    let mut combines = ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scope list")
        .iter()
        .filter(|scope| scope["kind"] == "Combine");
    let combine = combines.next().expect("one Combine scope").clone();
    assert!(combines.next().is_none(), "one Combine scope");
    for code in [
        "feature.combine-operand-list-refused",
        "feature.combine-definition-unresolved",
        "feature.combine-prologue-assumed",
        "feature.combine-tool-consumption-assumed",
    ] {
        assert!(!codes.iter().any(|seen| seen == code), "{code}: {codes:?}");
    }
    let operation = combine["combine_operation"].clone();
    assert_eq!(operation["form"], "operand_lists", "{operation}");
    (operation, codes)
}

fn records(list: &Value) -> Vec<u64> {
    list.as_array()
        .map(|entries| entries.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

fn tool_nodes(operation: &Value) -> Vec<u64> {
    operation["tools"]
        .as_array()
        .expect("tool list")
        .iter()
        .map(|tool| {
            tool["body_node"]["record_index"]
                .as_u64()
                .expect("tool browser node")
        })
        .collect()
}

#[test]
fn a_kept_tool_is_neither_created_nor_consumed() {
    let (operation, _) = combine_operation(include_bytes!(
        "../../../../corpus/f3d/combine_keep_tools.f3d"
    ));
    assert_eq!(operation["operation"], "join");
    assert_eq!(operation["keep_tools"], true);
    assert!(records(&operation["created_body_nodes"]).is_empty());
    assert!(records(&operation["consumed_body_nodes"]).is_empty());
}

#[test]
fn an_intersect_of_two_lumps_creates_one_body_and_consumes_its_tool() {
    let (operation, _) = combine_operation(include_bytes!(
        "../../../../corpus/f3d/combine_intersect_two_lumps.f3d"
    ));
    assert_eq!(operation["operation"], "intersect");
    assert_eq!(operation["keep_tools"], false);
    assert_eq!(records(&operation["created_body_nodes"]).len(), 1);
    assert_eq!(
        records(&operation["consumed_body_nodes"]),
        tool_nodes(&operation)
    );
}

#[test]
fn a_cut_that_splits_its_target_reads_the_operand_list_prologue() {
    let (operation, _) = combine_operation(include_bytes!(
        "../../../../corpus/f3d/combine_cut_split.f3d"
    ));
    assert_eq!(operation["operation"], "cut");
    assert_eq!(operation["keep_tools"], false);
    assert_eq!(records(&operation["created_body_nodes"]).len(), 1);
    assert_eq!(
        records(&operation["consumed_body_nodes"]),
        tool_nodes(&operation)
    );
}

#[test]
fn a_new_component_result_is_a_created_body_and_a_companion_consumes_the_tool() {
    let (operation, codes) = combine_operation(include_bytes!(
        "../../../../corpus/f3d/combine_new_component.f3d"
    ));
    assert_eq!(operation["operation"], "join");
    assert_eq!(operation["keep_tools"], false);
    let created = records(&operation["created_body_nodes"]);
    assert_eq!(created.len(), 1);
    assert_eq!(operation["destination"]["kind"], "new_component");
    assert_eq!(
        operation["destination"]["browser_node_record_index"].as_u64(),
        created.first().copied()
    );
    assert!(records(&operation["consumed_body_nodes"]).is_empty());
    assert!(codes
        .iter()
        .any(|code| code == "feature.combine-component-destination-assumed"));
}

#[test]
fn a_companion_consumes_the_tool_from_another_component() {
    let (operation, _) = combine_operation(include_bytes!(
        "../../../../corpus/f3d/combine_cross_component.f3d"
    ));
    assert_eq!(operation["operation"], "join");
    assert_eq!(operation["keep_tools"], false);
    assert!(records(&operation["created_body_nodes"]).is_empty());
    let tools = tool_nodes(&operation);
    assert_eq!(tools.len(), 2);
    assert_eq!(records(&operation["consumed_body_nodes"]), tools[1..]);
}
