// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::products::{ExternalDocument, OccurrenceParent, PrototypeReference};
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
    assert_eq!(model.product_definitions.len(), 1);
    let root = model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.parent == OccurrenceParent::Root {})
        .expect("root occurrence");
    assert_eq!(root.name.as_deref(), Some("TOP_ASSY"));
    let mut children = model
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.parent
                == OccurrenceParent::Occurrence {
                    occurrence: root.id.clone(),
                }
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|occurrence| occurrence.ordinal);
    let placed = children
        .iter()
        .map(|occurrence| {
            let PrototypeReference::External {
                document: ExternalDocument::Path { path },
                object: None,
            } = &occurrence.prototype
            else {
                panic!("external path prototype: {:?}", occurrence.prototype);
            };
            (
                occurrence.ordinal,
                path.as_str().to_owned(),
                occurrence.transform.affine_rows(),
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
    assert_eq!(result.ir().model.occurrences.len(), 1);
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
    assert_eq!(result.ir().model.occurrences.len(), 3);
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyPlacementUnresolved),
        [
            "1 component row(s) store a frame that is not a proper rigid placement; \
          those occurrences did not transfer"
        ]
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
        .filter(|occurrence| occurrence.prototype == PrototypeReference::Unresolved {})
        .count();
    assert_eq!(unresolved, 1);
    assert_eq!(
        loss_messages(&result, CreoLossCode::AssemblyMemberReferenceUnresolved).len(),
        1
    );
}
