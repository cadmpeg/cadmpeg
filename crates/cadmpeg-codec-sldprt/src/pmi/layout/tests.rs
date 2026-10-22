// SPDX-License-Identifier: Apache-2.0
//! Native PMI content and its separate, physically proven source slots.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::SourceFidelity;

const FIELDS: [&str; 7] = [
    "offset",
    "value_offset",
    "precision_offset",
    "display_text_offset",
    "basic_offset",
    "inspection_offset",
    "reference_only_offset",
];

fn parse(source: &[u8]) -> (Vec<crate::records::PmiDimension>, SourceFidelity) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::service()).expect("source");
    let mut losses = Vec::new();
    let parsed = crate::pmi::parse_payload(&ctx, root, &mut losses).expect("PMI decode");
    assert!(losses.is_empty(), "{losses:?}");
    let mut fidelity = SourceFidelity::default();
    super::install(&ctx, parsed.pointers, &mut fidelity).expect("captured layouts");
    (parsed.records, fidelity)
}

fn assert_slots(source: &[u8], fields: &std::collections::BTreeMap<String, u64>) {
    assert_eq!(fields.len(), FIELDS.len());
    let position = |field: &str| usize::try_from(fields[field]).expect("source pointer");
    assert_eq!(source[position("offset")], 0x87);
    assert_eq!(
        &source[position("value_offset")..position("value_offset") + 8],
        &0.025_f64.to_be_bytes()
    );
    assert_eq!(source[position("precision_offset")], 3);
    assert_eq!(source[position("basic_offset")], 0xc3);
    assert_eq!(source[position("inspection_offset")], 0xc2);
    assert_eq!(source[position("reference_only_offset")], 0xc3);
    assert_eq!(
        &source[position("display_text_offset")..position("display_text_offset") + 9],
        b"25.000 mm"
    );
}

#[test]
fn seven_raw_pmi_offsets_are_layout_pointers_and_native_wire_keeps_exact_text() {
    let source = crate::test_support::pmi::pmi_semantic_payload();
    let (records, fidelity) = parse(&source);
    let [record] = records.as_slice() else {
        panic!("one dimension")
    };
    let layout = &fidelity.record_layout()[&record.id];
    assert_eq!(layout.stream(), "file");
    assert_eq!(layout.offset(), 100);
    assert_eq!(layout.pointers()["offset"], 100);
    assert_slots(&source, layout.pointers());
    let wire = serde_json::to_value(record).expect("native wire");
    for field in FIELDS {
        assert!(wire.get(field).is_none(), "{field}");
    }
    assert_eq!(wire["display_text"], "25.000 mm");
    let value = layout.pointers()["value_offset"];
    assert!(layout.reads()["file"]
        .iter()
        .any(|range| range.start <= value && value + 8 <= range.end));
    let restored: SourceFidelity =
        serde_json::from_value(serde_json::to_value(&fidelity).expect("fidelity wire"))
            .expect("fidelity readback");
    assert_eq!(restored.record_layout(), fidelity.record_layout());
}

#[test]
fn inline_database_pointers_keep_actual_database_coordinates_and_cell_extent() {
    let raw = crate::test_support::pmi::pmi_semantic_payload();
    let key = std::str::from_utf8(&raw[64..100]).expect("GUID").to_owned();
    let source = crate::pmi::database::encode(&[(key, raw[100..].to_vec())]).expect("database");
    let (records, fidelity) = parse(&source);
    let layout = &fidelity.record_layout()[&records[0].id];
    assert!(layout.stream().starts_with("sldprt-pmi-cell:"));
    assert_eq!(layout.offset(), 36);
    assert_eq!(layout.pointers()["offset"], 2 * 4096 + 38 + 36);
    assert_slots(&source, layout.pointers());
}

#[test]
fn overflow_database_keeps_slots_and_does_not_claim_opaque_bulk_as_semantic_reads() {
    let raw = crate::test_support::pmi::pmi_semantic_payload();
    let key = std::str::from_utf8(&raw[64..100]).expect("GUID").to_owned();
    let mut value = raw[100..].to_vec();
    value[0] = 0x88;
    value.extend_from_slice(b"\xa4bulk\xc5\x13\x88");
    let bulk_start = value.len();
    value.resize(bulk_start + 5000, b'a');
    let source = crate::pmi::database::encode(&[(key, value)]).expect("overflow database");
    let (records, fidelity) = parse(&source);
    let layout = &fidelity.record_layout()[&records[0].id];
    assert!(layout.stream().starts_with("sldprt-pmi-cell:"));
    assert_eq!(layout.offset(), 36);
    assert_eq!(layout.pointers()["offset"], 3 * 4096 + 18 + 36);
    let mut fields = layout.pointers().clone();
    let map = usize::try_from(fields.remove("offset").expect("map pointer")).expect("map offset");
    assert_eq!(source[map], 0x88);
    let raw_value = &source[usize::try_from(fields["value_offset"]).expect("value position")..];
    assert_eq!(&raw_value[..8], &0.025_f64.to_be_bytes());
    let bulk = u64::try_from(36 + bulk_start).expect("bulk position");
    assert!(layout.reads()[layout.stream()]
        .iter()
        .all(|range| range.end <= bulk));
}

#[test]
fn nonzero_section_view_keeps_old_relative_pointers_and_absolute_record_extent() {
    let payload = crate::test_support::pmi::pmi_semantic_payload();
    let mut source = b"prefix".to_vec();
    let start = source.len();
    source.extend_from_slice(&payload);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).expect("source");
    let parsed = crate::pmi::parse_payload(
        &ctx,
        root.child(start, source.len()).expect("section"),
        &mut Vec::new(),
    )
    .expect("section PMI");
    let mut fidelity = SourceFidelity::default();
    super::install(&ctx, parsed.pointers, &mut fidelity).expect("layouts");
    let layout = &fidelity.record_layout()[&parsed.records[0].id];
    assert_eq!(layout.stream(), "file");
    assert_eq!(
        layout.offset(),
        u64::try_from(start + 100).expect("physical map")
    );
    assert_slots(&payload, layout.pointers());
    let physical_value =
        layout.pointers()["value_offset"] + u64::try_from(start).expect("section origin");
    assert!(layout.reads()["file"]
        .iter()
        .any(|range| range.start <= physical_value && physical_value + 8 <= range.end));
}
