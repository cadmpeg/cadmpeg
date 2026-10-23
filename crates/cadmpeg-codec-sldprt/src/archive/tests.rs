// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::container::{make_block, outer_header, scan};

use super::*;

fn declaration(name: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xff, 0x01, 0x00];
    bytes.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

fn tag(index: u16) -> [u8; 2] {
    (0x8000 | index).to_le_bytes()
}

fn end_count(payload: &[u8], resolved_count: Option<u32>) -> i64 {
    walk(payload, resolved_count).unwrap().end
}

/// `moPart_c` and `moNodeName_c` declared, then one more name object. The
/// name tag `04 80 ff fe ff` puts the name class at 4 and the map's first
/// index at 2: the part takes 2 and 3, the name class 4 and 5, the object 6.
fn named_part() -> Vec<u8> {
    let mut stream = declaration("moPart_c");
    stream.extend_from_slice(&[0, 0]);
    stream.extend(declaration("moNodeName_c"));
    stream.extend_from_slice(&[0xff, 0xfe, 0xff, 0x01, b'A', 0]);
    stream.extend_from_slice(&tag(4));
    stream.extend_from_slice(&[0xff, 0xfe, 0xff, 0x01, b'B', 0]);
    stream
}

#[test]
fn declarations_need_a_class_name() {
    let mut stream = declaration("moPart_c");
    stream.extend(declaration("9bad"));
    stream.extend(declaration("ab"));
    stream.extend(declaration("sgLineHandle"));
    let found = declarations(&stream);
    let names = found.iter().map(|found| found.name).collect::<Vec<_>>();
    assert_eq!(names, ["moPart_c", "sgLineHandle"]);
    assert_eq!(found[0].body, 6 + "moPart_c".len());
}

#[test]
fn first_index_is_two_below_the_name_class() {
    assert_eq!(first_index(&named_part()), 2);
    let mut five = vec![0x05, 0x80, 0xff, 0xfe, 0xff, 0x00];
    five.extend_from_slice(&[0x05, 0x80, 0xff, 0xfe, 0xff, 0x00]);
    five.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, 0x00]);
    assert_eq!(first_index(&five), 3);
    assert_eq!(first_index(&[0; 16]), FALLBACK_FIRST_INDEX);
}

#[test]
fn words_are_classified_by_their_first_bytes() {
    assert_eq!(word_at(&[0xff, 0xff], 0), Word::Declaration);
    assert_eq!(word_at(&[0xff, 0xfe, 0xff], 0), Word::String);
    assert_eq!(word_at(&[0x04, 0x80], 0), Word::Tag);
    assert_eq!(word_at(&[0xff, 0x0f], 0), Word::Small);
    assert_eq!(word_at(&[0x34, 0x12], 0), Word::Data);
    assert_eq!(word_at(&[0x34], 0), Word::End);
}

#[test]
fn a_tag_opens_an_object_that_takes_the_next_index() {
    assert_eq!(end_count(&named_part(), None), 7);
}

#[test]
fn a_root_class_tag_opens_no_object() {
    let mut stream = named_part();
    stream.extend_from_slice(&tag(2));
    stream.extend_from_slice(&[0x01, 0x00]);
    assert_eq!(end_count(&stream, None), 7);
}

/// `moColour_c`'s body opens with a small word and has a `CString` at body
/// offset 4, its anchor. A tag whose next word is data opens an object only
/// when a `CString` lies at that anchor.
#[test]
fn a_data_led_body_opens_an_object_at_its_anchor() {
    let mut stream = declaration("moPart_c");
    stream.extend_from_slice(&[0, 0]);
    stream.extend(declaration("moColour_c"));
    stream.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0xff, 0xfe, 0xff, 0x00]);
    stream.extend_from_slice(&tag(4));
    stream.extend_from_slice(&[0x22, 0x33, 0x00, 0x00, 0xff, 0xfe, 0xff, 0x00]);
    stream.extend_from_slice(&tag(4));
    stream.extend_from_slice(&[0x22, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    // moPart_c 2, 3; moColour_c 4, 5; the anchored object 6.
    assert_eq!(end_count(&stream, None), 7);
}

#[test]
fn an_extended_handle_cell_opens_an_object() {
    let mut stream = declaration("moPart_c");
    stream.extend_from_slice(&[0, 0]);
    stream.extend(declaration("sgLineHandle"));
    stream.extend_from_slice(&[0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
    stream.extend_from_slice(&tag(4));
    stream.extend_from_slice(&[0x7f, 0x77, 0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    stream.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    // moPart_c 2, 3; sgLineHandle 4, 5; the cell 6.
    assert_eq!(end_count(&stream, None), 7);
}

#[test]
fn coverage_counts_reconciled_configurations() {
    let mut file = outer_header();
    file.extend(make_block(0x20, "Contents/Config-0", &named_part()));
    let mut resolved = 7_u32.to_le_bytes().to_vec();
    resolved.extend_from_slice(&[0, 0]);
    file.extend(make_block(
        0x21,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let mut wrong = 9_u32.to_le_bytes().to_vec();
    wrong.extend_from_slice(&[0, 0]);
    file.extend(make_block(0x22, "Contents/Config-1", &named_part()));
    file.extend(make_block(
        0x23,
        "Contents/Config-1-ResolvedFeatures",
        &wrong,
    ));
    let scan = scan(&file);
    let coverage = coverage(&scan).unwrap();
    assert_eq!(coverage.get(WALKED_CONFIGURATION_COUNT.as_str()), Some(&2));
    assert_eq!(
        coverage.get(RECONCILED_CONFIGURATION_COUNT.as_str()),
        Some(&1)
    );
    assert_eq!(coverage.get(CLASS_DECLARATION_COUNT.as_str()), Some(&4));
}

#[test]
fn a_resolved_features_stream_that_opens_with_a_declaration_carries_no_count() {
    let mut compound = vec![0x0d, 0x00];
    compound.extend(declaration("moSensorFolder_c"));
    assert_eq!(resolved_count(&compound), None);
    let mut block = 7_u32.to_le_bytes().to_vec();
    block.extend_from_slice(&[0, 0]);
    block.extend(declaration("moSensorFolder_c"));
    assert_eq!(resolved_count(&block), Some(7));
}
