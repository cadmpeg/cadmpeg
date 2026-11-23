// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::sketch_profiles::bind_sketch_profiles;
use crate::design::sketch_text_extrude::SketchTextExtrudeCandidate;
use crate::layout::sketch_profile_text_selection as text_selection;
use crate::test_support::indexed_header;
use crate::test_support::multiple_profiles::fixture_with_nested_records;

/// How the nested records after one sketch-profile operand differ from a
/// text selection that names text record `N+900`.
#[derive(Clone, Copy)]
enum Frame {
    Text,
    NonzeroPadding,
    TrailingByte,
    Truncated,
    SelectionIndex(u32),
    FirstNestedIndex(u32),
}

fn nested(bytes: &mut Vec<u8>, record: u32, frame: Frame) {
    let first = match frame {
        Frame::FirstNestedIndex(offset) => record + offset,
        _ => record + 1,
    };
    indexed_header(bytes, *b"308", first);
    bytes.extend_from_slice(&[0; 20]);
    indexed_header(bytes, *b"321", record + 2);
    bytes.extend_from_slice(&[0; 22]);
    let selection = match frame {
        Frame::SelectionIndex(offset) => record + offset,
        _ => record + 3,
    };
    let mut payload = [0u8; text_selection::LEN];
    payload[..text_selection::ZERO_RUN_10].copy_from_slice(&{
        let mut header = Vec::new();
        indexed_header(&mut header, *b"363", selection);
        header
    });
    payload[text_selection::TEXT_RECORD_INDEX..]
        .copy_from_slice(&u64::from(record + 900).to_le_bytes());
    if let Frame::NonzeroPadding = frame {
        payload[text_selection::ZERO_RUN_10 + 4] = 1;
    }
    match frame {
        Frame::Truncated => bytes.extend_from_slice(&payload[..text_selection::LEN - 1]),
        _ => bytes.extend_from_slice(&payload),
    }
    if let Frame::TrailingByte = frame {
        bytes.push(0);
    }
    indexed_header(bytes, *b"263", selection);
}

fn bind(
    count: u32,
    frame: impl Fn(u32) -> Frame,
) -> (Vec<String>, Vec<SketchTextExtrudeCandidate>) {
    let (archive, mut scope, headers, entities) =
        fixture_with_nested_records(count, None, &|bytes, record| {
            nested(bytes, record, frame(record));
        });
    let mut candidates = Vec::new();
    let losses = crate::test_support::zip_test::with_scan(&archive, |scan| {
        bind_sketch_profiles(
            scan,
            std::slice::from_mut(&mut scope),
            &headers,
            &entities,
            &mut candidates,
        )
        .unwrap()
    });
    (
        losses
            .iter()
            .map(|loss| loss.code.local_code().to_owned())
            .collect(),
        candidates,
    )
}

fn picks(candidate: &SketchTextExtrudeCandidate) -> Vec<(u64, u32)> {
    candidate
        .picks
        .iter()
        .map(|pick| (pick.sketch_entity, pick.text_record))
        .collect()
}

#[test]
fn one_text_selection_records_its_text_pick() {
    let (losses, candidates) = bind(1, |_| Frame::Text);
    assert!(losses.is_empty());
    let [candidate] = candidates.as_slice() else {
        panic!("one candidate");
    };
    assert_eq!(picks(candidate), [(172, 1000)]);
}

#[test]
fn text_selections_of_a_profile_group_defer_the_group_loss() {
    let (losses, candidates) = bind(2, |_| Frame::Text);
    assert!(losses.is_empty());
    let [candidate] = candidates.as_slice() else {
        panic!("one candidate");
    };
    assert_eq!(picks(candidate), [(172, 1000), (173, 1100)]);
}

#[test]
fn a_malformed_text_selection_is_not_a_pick() {
    for frame in [
        Frame::NonzeroPadding,
        Frame::TrailingByte,
        Frame::Truncated,
        Frame::SelectionIndex(4),
        Frame::FirstNestedIndex(2),
    ] {
        let (losses, candidates) = bind(1, |_| frame);
        assert!(losses.is_empty());
        assert!(candidates.is_empty());
        let (losses, candidates) =
            bind(2, |record| if record == 200 { frame } else { Frame::Text });
        assert_eq!(losses, ["feature.extrude-profile-group-native"]);
        assert!(candidates.is_empty());
    }
}
