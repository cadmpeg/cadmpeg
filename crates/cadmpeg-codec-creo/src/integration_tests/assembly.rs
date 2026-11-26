// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::products::Storage;
use cadmpeg_ir::unknown::part::record_unreadable_kind;
use cadmpeg_test_support::EditableDecodeResult;

use crate::loss::CreoLossCode;
use crate::test_support::{build_asm, build_asm_with, build_prt_raw, members_table};
use crate::CreoCodec;

fn decode(bytes: Vec<u8>) -> EditableDecodeResult {
    EditableDecodeResult::from(
        CreoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("synthesized Creo assembly should decode"),
    )
}

/// The record each `record.unreadable` names, as its message's second token.
fn unreadable_records(result: &EditableDecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .filter_map(|loss| loss.message.split_whitespace().nth(1).map(str::to_owned))
        .collect()
}

fn loss_messages(result: &EditableDecodeResult, code: CreoLossCode) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message.clone())
        .collect()
}

#[test]
fn component_rows_place_external_children_under_the_root() {
    let result = decode(build_asm());
    let model = &result.ir().model;
    // The root and one external definition per distinct child file.
    assert_eq!(model.product_definitions.len(), 3);
    let root = model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .expect("root definition");
    assert_eq!(root.label.as_deref(), Some("TOP_ASSY"));
    let placed = root
        .occurrences
        .iter()
        .enumerate()
        .map(|(ordinal, id)| {
            let occurrence = model
                .occurrences
                .iter()
                .find(|occurrence| &occurrence.id == id)
                .expect("owned occurrence");
            let prototype = model
                .product_definitions
                .iter()
                .find(|definition| definition.id == occurrence.prototype)
                .expect("prototype");
            let Storage::External(document) = &prototype.storage else {
                panic!("external prototype: {:?}", prototype.storage);
            };
            assert!(document.object.is_none());
            (
                ordinal,
                document.path.as_ref().expect("path").as_str().to_owned(),
                occurrence.transform.unwrap().affine_rows(),
                occurrence.native_ref.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        placed,
        [
            (
                0,
                "SUBASM.asm".to_owned(),
                [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0]
                ],
                Some("NeuAsmSld:members:42".to_owned()),
            ),
            (
                1,
                "BRACKET.prt".to_owned(),
                [
                    [1.0, 0.0, 0.0, 340.0],
                    [0.0, 0.0, -1.0, 408.0],
                    [0.0, 1.0, 0.0, 0.0]
                ],
                Some("NeuAsmSld:members:2575".to_owned()),
            ),
            (
                2,
                "BRACKET.prt".to_owned(),
                [
                    [0.0, -1.0, 0.0, -355.0],
                    [1.0, 0.0, 0.0, 2.5],
                    [0.0, 0.0, 1.0, 619.0]
                ],
                Some("NeuAsmSld:members:2576".to_owned()),
            ),
        ]
    );
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyComponentsExternal),
        [
            "3 component occurrence(s) reference 2 external model file(s); \
          the referenced files are not opened"
        ]
    );
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyConstraintsNotTransferred).len(),
        1
    );
    assert!(loss_messages(&result, CreoLossCode::AssemblyMemberTableIncomplete).is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn an_assembly_header_without_a_component_table_reports_the_drop() {
    let part = build_prt_raw(
        "test",
        &[("BasBasData", b"\xe0\x0amodel_name\0TOP_ASSY\0".to_vec())],
    );
    let header_end = part
        .iter()
        .position(|&byte| byte == b'\n')
        .expect("header line");
    let mut assembly = b"#UGC:2 ASSEMBLY 2562 2120".to_vec();
    assembly.extend_from_slice(&part[header_end..]);
    let result = decode(assembly);
    assert!(result.ir().model.occurrences.is_empty());
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyMemberTableIncomplete).len(),
        1
    );
    assert!(loss_messages(&result, CreoLossCode::AssemblyComponentsExternal).is_empty());
}

#[test]
fn a_part_without_a_component_table_reports_no_assembly_loss() {
    let result = decode(build_prt_raw(
        "test",
        &[("BasBasData", b"\xe0\x0amodel_name\0PART_A\0".to_vec())],
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.code.local_code().starts_with("assembly.")));
}

#[test]
fn the_golden_fixture_is_the_synthetic_assembly() {
    let fixture = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/assembly_member_table.prt"
    ))
    .expect("read the golden fixture");
    assert_eq!(fixture, build_asm());
}

fn replace_once(bytes: &mut [u8], from: &[u8], to: &[u8]) {
    assert_eq!(from.len(), to.len());
    let at = bytes
        .windows(from.len())
        .position(|window| window == from)
        .expect("pattern in the component table");
    bytes[at..at + to.len()].copy_from_slice(to);
}

#[test]
fn a_frame_that_is_not_rigid_drops_its_occurrence() {
    let mut table = members_table();
    replace_once(&mut table, b"\x18\x10\x18\x2d", b"\x18\x0f\x18\x2d");
    let result = decode(build_asm_with(table));
    // The root definition owns the two component rows that stay.
    assert_eq!(result.ir().model.occurrences.len(), 2);
    let dropped = "creo:model:occurrence#member-1";
    assert_eq!(unreadable_records(&result), [dropped.to_owned()]);
    let charges = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.message.contains("proper rigid placement"))
        .collect::<Vec<_>>();
    assert_eq!(charges.len(), 1, "one entry states the drop: {charges:#?}");
    assert_eq!(charges[0].code, record_unreadable_kind());
    assert!(result
        .ir()
        .model
        .occurrences
        .iter()
        .all(|occurrence| occurrence.id.as_str() != dropped));
}

#[test]
fn a_pinned_decode_refuses_a_frame_that_is_not_rigid() {
    let mut table = members_table();
    replace_once(&mut table, b"\x18\x10\x18\x2d", b"\x18\x0f\x18\x2d");
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let failure = CreoCodec
        .decode(&mut Cursor::new(build_asm_with(table)), &options)
        .expect_err("a pinned decode refuses a dropped component row");
    let DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(rejection.loss().code, record_unreadable_kind());
    assert!(rejection
        .loss()
        .message
        .split_whitespace()
        .any(|token| token == "creo:model:occurrence#member-1"));
}

#[test]
fn an_unread_component_row_is_given_up_with_the_rows_after_it() {
    let mut table = members_table();
    replace_once(
        &mut table,
        b"\x8a\x0f\x02BRACKET\0BRACKET\0\xe1",
        b"\x8a\x0f\x02BRACKET\0BRACKET\0\xe2",
    );
    let bytes = build_asm_with(table);
    let row = bytes
        .windows(3)
        .position(|window| window == b"\x8a\x0f\x02")
        .expect("the unread row");
    let end = crate::container::scan_bytes_ok(bytes.clone())
        .framing
        .sections
        .iter()
        .find(|section| section.name() == "NeuAsmSld")
        .expect("the component section")
        .end();
    let result = decode(bytes.clone());
    assert_eq!(result.ir().model.occurrences.len(), 1);
    assert!(loss_messages(&result, CreoLossCode::AssemblyMemberTableIncomplete).is_empty());
    let charges = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(charges.len(), 1, "{charges:#?}");
    assert!(
        charges[0].contains("does not match the component row grammar"),
        "{}",
        charges[0]
    );
    assert!(
        charges[0].starts_with(&format!(
            "part component-rows stream=\"\" offset={row} length={} record=creo:source:part#",
            end - row
        )),
        "{}",
        charges[0]
    );
    let stub = result
        .ir()
        .native_unknowns("creo")
        .expect("native unknowns")
        .into_iter()
        .find(|record| charges[0].contains(&format!(" record={}: ", record.id)))
        .expect("the charge names its part stub");
    assert!(stub.links.is_empty());
    assert_eq!(
        result
            .source_fidelity()
            .retained_record(stub.id.as_str())
            .and_then(|record| record.data()),
        Some(&bytes[row..end])
    );
}

#[test]
fn a_family_instance_row_has_an_unresolved_prototype() {
    let mut table = members_table();
    replace_once(
        &mut table,
        b"\x8a\x10\x02BRACKET\0BRACKET\0",
        b"\x8a\x10\x02BRACKET\0BRACKEX\0",
    );
    let result = decode(build_asm_with(table));
    let unresolved = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            result
                .ir()
                .model
                .product_definitions
                .iter()
                .any(|definition| {
                    definition.id == occurrence.prototype
                        && definition.storage
                            == Storage::External(cadmpeg_ir::products::ExternalDocument::default())
                })
        })
        .count();
    assert_eq!(unresolved, 1);
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyMemberReferenceUnresolved).len(),
        1
    );
}
