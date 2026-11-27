// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::products::Storage;
use cadmpeg_ir::unknown::part::record_unreadable_kind;
use cadmpeg_test_support::EditableDecodeResult;

use crate::loss::CreoLossCode;
use crate::test_support::{
    build_asm, build_asm_sections, build_asm_with, build_prt_raw, members_table, neu_asm_payload,
};
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
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyPlacementUnresolved),
        [
            "1 component row(s) store a frame that is not a proper rigid placement; \
          those occurrences did not transfer"
        ]
    );
    let dropped = "creo:model:occurrence#member-1";
    assert_eq!(unreadable_records(&result), [dropped.to_owned()]);
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
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyMemberTableIncomplete).len(),
        1
    );
    let charges = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(charges.len(), 1, "{charges:#?}");
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

/// The `record.unreadable` messages, in report order.
fn charges(result: &EditableDecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .map(|loss| loss.message.clone())
        .collect()
}

/// The start and end of each `NeuAsmSld` section of `bytes`.
fn component_sections(bytes: &[u8]) -> Vec<(usize, usize)> {
    crate::container::scan_bytes_ok(bytes.to_vec())
        .framing
        .sections
        .iter()
        .filter(|section| section.name() == "NeuAsmSld")
        .map(|section| (section.offset(), section.end()))
        .collect()
}

/// The offset of each `members` field in `bytes`.
fn member_fields(bytes: &[u8]) -> Vec<usize> {
    let field = b"\xe0\x00members\0";
    bytes
        .windows(field.len())
        .enumerate()
        .filter(|(_, window)| window == field)
        .map(|(offset, _)| offset)
        .collect()
}

/// Asserts that `charge` gives up `bytes[start..end]` as a `component-table`
/// part for `reason`, and that the decode retains those bytes on its stub.
fn assert_table_given_up(
    result: &EditableDecodeResult,
    charge: &str,
    bytes: &[u8],
    (start, end): (usize, usize),
    reason: &str,
) {
    assert!(
        charge.starts_with(&format!(
            "part component-table stream=\"\" offset={start} length={} record=creo:source:part#",
            end - start
        )),
        "{charge}"
    );
    assert!(
        charge.ends_with(&format!("{reason}; dropped=[]")),
        "{charge}"
    );
    let stub = result
        .ir()
        .native_unknowns("creo")
        .expect("native unknowns")
        .into_iter()
        .find(|record| charge.contains(&format!(" record={}: ", record.id)))
        .expect("the charge names its part stub");
    assert_eq!(
        result
            .source_fidelity()
            .retained_record(stub.id.as_str())
            .and_then(|record| record.data()),
        Some(&bytes[start..end])
    );
}

/// A component table whose array header does not frame a table.
fn unframed_table() -> Vec<u8> {
    let mut table = members_table();
    replace_once(&mut table, b"members\0\xf1\xf8", b"members\0\xf1\xf9");
    table
}

#[test]
fn a_component_table_the_reader_cannot_frame_is_given_up() {
    let bytes = build_asm_with(unframed_table());
    let [field] = member_fields(&bytes)[..] else {
        panic!("one members field");
    };
    let [(_, end)] = component_sections(&bytes)[..] else {
        panic!("one component section");
    };
    let result = decode(bytes.clone());
    assert!(result.ir().model.occurrences.is_empty());
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyMemberTableIncomplete).len(),
        1
    );
    let charges = charges(&result);
    assert_eq!(charges.len(), 1, "{charges:#?}");
    assert_table_given_up(
        &result,
        &charges[0],
        &bytes,
        (field, end),
        "the members field header is not the table array form the reader reads",
    );
}

#[test]
fn a_pinned_decode_refuses_a_component_table_the_reader_cannot_frame() {
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let failure = CreoCodec
        .decode(&mut Cursor::new(build_asm_with(unframed_table())), &options)
        .expect_err("a pinned decode refuses a dropped component table");
    let DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(rejection.loss().code, record_unreadable_kind());
    assert!(
        rejection
            .loss()
            .message
            .starts_with("part component-table "),
        "{}",
        rejection.loss().message
    );
}

#[test]
fn a_section_with_two_members_fields_is_given_up_from_the_first() {
    let mut tables = members_table();
    tables.extend(members_table());
    let bytes = build_asm_with(tables);
    let fields = member_fields(&bytes);
    assert_eq!(fields.len(), 2);
    let [(_, end)] = component_sections(&bytes)[..] else {
        panic!("one component section");
    };
    let result = decode(bytes.clone());
    assert!(result.ir().model.occurrences.is_empty());
    let charges = charges(&result);
    assert_eq!(charges.len(), 1, "{charges:#?}");
    assert_table_given_up(
        &result,
        &charges[0],
        &bytes,
        (fields[0], end),
        "the section holds more than one members field",
    );
}

#[test]
fn component_tables_in_two_sections_are_each_given_up() {
    let bytes = build_asm_sections(&[
        ("NeuAsmSld", neu_asm_payload(members_table())),
        ("NeuAsmSld", neu_asm_payload(members_table())),
    ]);
    let fields = member_fields(&bytes);
    let sections = component_sections(&bytes);
    assert_eq!((fields.len(), sections.len()), (2, 2));
    let result = decode(bytes.clone());
    assert!(result.ir().model.occurrences.is_empty());
    let charges = charges(&result);
    assert_eq!(charges.len(), 2, "{charges:#?}");
    for (charge, (field, (_, end))) in charges.iter().zip(fields.into_iter().zip(sections)) {
        assert_table_given_up(
            &result,
            charge,
            &bytes,
            (field, end),
            "another NeuAsmSld section also holds a component table",
        );
    }
}

#[test]
fn an_unframed_table_beside_the_transferred_one_is_given_up() {
    let bytes = build_asm_sections(&[
        ("NeuAsmSld", neu_asm_payload(members_table())),
        ("NeuAsmSld", neu_asm_payload(unframed_table())),
    ]);
    let fields = member_fields(&bytes);
    let sections = component_sections(&bytes);
    assert_eq!((fields.len(), sections.len()), (2, 2));
    let result = decode(bytes.clone());
    assert_eq!(result.ir().model.occurrences.len(), 3);
    let charges = charges(&result);
    assert_eq!(charges.len(), 1, "{charges:#?}");
    assert_table_given_up(
        &result,
        &charges[0],
        &bytes,
        (fields[1], sections[1].1),
        "the members field header is not the table array form the reader reads",
    );
}

#[test]
fn a_table_whose_next_row_would_start_at_the_section_end_gives_up_nothing() {
    // The last row's terminator says another row follows, and the section
    // ends there: no row bytes are left unread.
    let mut table = members_table();
    let last = table.len() - 1;
    assert_eq!(table[last], 0xf4);
    table[last] = 0xe3;
    let mut payload = b"\xe0\x00Sld_NeutAssem\0\xe3".to_vec();
    payload.extend(table);
    let bytes = build_asm_sections(&[("NeuAsmSld", payload)]);
    let [(_, end)] = component_sections(&bytes)[..] else {
        panic!("one component section");
    };
    assert_eq!(end, bytes.len());
    let result = decode(bytes);
    assert_eq!(result.ir().model.occurrences.len(), 3);
    let notes = loss_messages(&result, CreoLossCode::AssemblyMemberTableIncomplete);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert!(
        notes[0].contains(&format!("before offset {end}:")),
        "{}",
        notes[0]
    );
    assert!(charges(&result).is_empty(), "{:#?}", charges(&result));
}
