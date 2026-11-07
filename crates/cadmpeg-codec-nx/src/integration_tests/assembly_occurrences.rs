// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts for native-only NX assembly placement frames.

use std::fmt::Write;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use serde_json::Value;

use crate::loss::NxLossCode;
use crate::NxCodec;

const FRAME_UUID: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn append_fastload_string(bytes: &mut Vec<u8>, tag: u8, value: &str) {
    bytes.push(tag);
    bytes.push(u8::try_from(value.len() + 2).unwrap());
    bytes.extend(value.as_bytes());
    bytes.push(0);
}

fn fastload_structure_with_two_same_prototype_uses(uuid: &str) -> Vec<u8> {
    let mut bytes = vec![1, 2, 0x42, 0];
    bytes.extend([1, 2]);
    append_fastload_string(&mut bytes, 4, "MODEL");
    bytes.extend([1, 3, 0, 0, 1, 3, b'9', b'9']);
    bytes.extend([1, 2, 0xff, 0xff, 0xff, 0xff, 1, 2]);
    append_fastload_string(&mut bytes, 4, "pin");
    bytes.extend([1, 3, 1, 1]);
    bytes.extend([1, 2, 3, 38]);
    bytes.extend(uuid.as_bytes());
    bytes.push(0);
    bytes.extend([1, 3, 1, 1]);

    let mut structure = vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0];
    structure.extend(u32::try_from(bytes.len()).unwrap().to_be_bytes());
    structure.extend(bytes);
    structure
}

fn placement_with_uuid(uuid: &str) -> Vec<u8> {
    let mut frame = crate::layout::assembly_placement_prefix::MAGIC_VALUE.to_vec();
    frame.extend([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]);
    frame.extend([0, 0, 0, 1, 1, 0xf0, 0, 0, 0]);
    append_fastload_string(&mut frame, 3, "instance");
    append_fastload_string(&mut frame, 3, uuid);
    frame
}

fn assert_frame_loss(result: &cadmpeg_ir::codec::DecodeResult, placements: usize, uses: usize) {
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == NxLossCode::AssemblyPlacementFrameUnresolved.kind())
        .expect("unproven placement frames have a specific loss");
    assert!(loss
        .message
        .contains(&format!("Withheld {placements} NX placement(s)")));
    assert!(loss
        .message
        .contains(&format!("{uses} FastLoad component use(s)")));
    assert!(loss.message.contains("placement-to-component association"));
    assert!(loss.message.contains("assembly root frame"));
    assert!(loss.message.contains("translation unit"));
    assert!(loss.message.contains("remain in the NX native namespace"));
}

#[test]
fn decode_withholds_unproven_frames_and_retains_native_placement_and_use_records() {
    let placement = placement_with_uuid(FRAME_UUID);
    let mut expected_raw_frame = String::new();
    for byte in &placement {
        write!(expected_raw_frame, "{byte:02x}").unwrap();
    }
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", placement),
        (
            "/Root/FastLoad/Structure",
            fastload_structure_with_two_same_prototype_uses(FRAME_UUID),
        ),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();

    assert!(result.ir().model.occurrences.is_empty());
    assert!(result.ir().model.product_definitions.is_empty());
    let namespace = result.ir().native.namespace("nx").unwrap();
    let placements: Vec<Value> = namespace.arena_as("assembly_placements").unwrap();
    let uses: Vec<Value> = namespace
        .arena_as("fast_load_component_occurrences")
        .unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(
        placements[0]["raw_frame"].as_str(),
        Some(expected_raw_frame.as_str())
    );
    assert_eq!(uses.len(), 2);
    assert_frame_loss(&result, 1, 2);
}

#[test]
fn decode_withholds_mtc_placement_frames_and_retains_all_native_records() {
    let bytes = include_bytes!("../../../../corpus/nist-pmi/nx/NIST mtc crada assembly.prt");
    let result = NxCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result.ir().model.occurrences.is_empty());
    assert!(result.ir().model.product_definitions.is_empty());
    let namespace = result.ir().native.namespace("nx").unwrap();
    let placements: Vec<Value> = namespace.arena_as("assembly_placements").unwrap();
    let uses: Vec<Value> = namespace
        .arena_as("fast_load_component_occurrences")
        .unwrap();
    assert_eq!(placements.len(), 36);
    assert_eq!(uses.len(), 36);
    assert!(placements.iter().all(|placement| placement["raw_frame"]
        .as_str()
        .is_some_and(|frame| !frame.is_empty())));
    assert_frame_loss(&result, 36, 36);
}
