// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use crate::wire::records::{consolidated_records_in_sources, parse_consolidated_record};

fn group(reference: u8) -> Vec<u8> {
    let mut payload = vec![0x81, reference, 5, 1];
    for value in [-1.0_f32, 1.0, -1.0, 1.0, -1.0, 1.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.push(3);
    let mut bytes = vec![
        0xb2,
        0x13,
        0x5b,
        u8::try_from(payload.len()).expect("short root"),
        5,
    ];
    bytes.extend(payload);
    bytes
}
fn run(data: &[u8], sources: &[Vec<SourceExtent>], end: usize) -> Sheets {
    let records = consolidated_records_in_sources(data, sources).expect("records");
    let selection = crate::container::body_selection::BodySelection {
        source: 0,
        allocation_ends: vec![end],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )
    .expect("context");
    patches(
        &ctx,
        &CadIr::empty(),
        &Stream {
            data,
            sources,
            records: &records,
            selection: Some(&selection),
        },
        &BTreeMap::new(),
        &mut crate::nurbs::LaneRefusals::new(),
    )
    .expect("sheet membership")
}
#[test]
fn backward_sheet_roster_accepts_compact_family() {
    let mut bytes = vec![0xb2, 3, 0x5f, 0, 5];
    bytes.extend([0xb2, 3, 0x61, 5, 5, 0x81, 5, 0, 0x13, 3]);
    let start = bytes.len();
    bytes.extend(group(5));
    let allocations = Allocations::read(&bytes, 0, Some(bytes.len()));
    let root = parse_consolidated_record(&bytes, start, bytes.len()).expect("root");
    assert_eq!(
        allocations.backward_faces(&bytes, &root),
        Some(vec![(1, 0, true)])
    );
}
#[test]
fn governing_sheet_root_accepts_four_byte_allocation_header() {
    let ordinary = group(3);
    let mut data = vec![0xb2, 3, 0x25, 0, 5];
    data.extend([0xb5, 0x13, 0x5b, ordinary[3], 0x0c, 0, 0, 1]);
    data.extend_from_slice(&ordinary[5..]);
    let shell = data.len();
    data.extend([0xb2, 3, 0x61, 5, 5, 0x81, 3, 0, 0x13, 3]);
    let face = data.len();
    data.extend([0xb2, 3, 0x5f, 0, 5]);
    let sources = vec![vec![
        SourceExtent::within(&data, 0, data.len()).expect("source")
    ]];
    let records = consolidated_records_in_sources(&data, &sources).expect("records");
    assert!(!records.iter().any(|frame| frame.class() == 0x5b));
    let sheets = run(&data, &sources, 65_537);
    assert_eq!(
        sheets.members.get(&(0, face)),
        Some(&vec![Member {
            body: (0, shell),
            forward: true
        }])
    );
    assert!(sheets.roots[&(0, shell)].contains(&5));
}
#[test]
fn governing_sheet_root_survives_extent_split_with_physical_identity() {
    let mut logical = vec![0xb2, 3, 0x25, 0, 5];
    logical.extend(group(3));
    let shell = logical.len();
    logical.extend([0xb2, 3, 0x61, 5, 5, 0x81, 3, 0, 0x13, 3]);
    let face = logical.len();
    logical.extend([0xb2, 3, 0x5f, 0, 5]);
    for split in [3, 5, 6, 9, 17, 30] {
        let gap = [0xff; 11];
        let mut data = logical[..split].to_vec();
        data.extend(gap);
        data.extend_from_slice(&logical[split..]);
        let sources = vec![vec![
            SourceExtent::within(&data, 0, split).expect("first extent"),
            SourceExtent::within(&data, split + gap.len(), data.len()).expect("second extent"),
        ]];
        let sheets = run(&data, &sources, 2);
        assert_eq!(
            sheets.members.get(&(0, face)),
            Some(&vec![Member {
                body: (0, shell + gap.len()),
                forward: true
            }])
        );
        let physical_root = 5 + if split <= 5 { gap.len() } else { 0 };
        assert!(sheets.roots[&(0, shell + gap.len())].contains(&physical_root));
    }
}
#[test]
fn governing_sheet_roots_require_complete_rosters_and_exact_allocation_ends() {
    let mut data = vec![0xb2, 3, 0x25, 0, 5];
    data.extend(group(3));
    data.extend([0xb2, 3, 0x61, 5, 5, 0x81, 3, 0, 0x13, 3]);
    data.extend([0xb2, 3, 0x5f, 0, 5]);
    let sources = vec![vec![
        SourceExtent::within(&data, 0, data.len()).expect("source")
    ]];
    let invalid_end = run(&data, &sources, 99);
    assert!(invalid_end.selection.is_none());
    data[10] = 0x82;
    assert!(run(&data, &sources, 2).members.is_empty());
}
