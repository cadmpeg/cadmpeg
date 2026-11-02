// SPDX-License-Identifier: Apache-2.0
//! Semantic fields, source navigation, and failed-record rollback.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::CodecFormat;
use serde_json::Value;

use crate::record_identity::{Located, RecordTypeId};

use super::super::super::{self as presentation, Cursor};

fn rendering(bytes: &[u8], version: u8) -> Value {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("the record fits the service policy");
    let parsed = presentation::parse_rendering_style(&ctx, root, version)
        .expect("the rendering style parses");
    let record = presentation::native::rendering_style(&Located::new(
        parsed,
        RecordTypeId::from_bytes(presentation::RENDERING_STYLE_TYPE),
        cadmpeg_ir::identity_key!("source"),
        0,
    ))
    .expect("the rendering style forms a native record");
    serde_json::to_value(record).expect("the rendering style record serializes")
}

fn primary(bytes: &[u8]) -> Value {
    let parsed = presentation::parse_graphics_primary_color_style(
        cadmpeg_core::decode::View::over_retained(bytes),
        14,
    )
    .expect("the primary color style parses");
    serde_json::to_value(presentation::native::primary_color(&Located::new(
        parsed,
        RecordTypeId::from_bytes(presentation::GRAPHICS_PRIMARY_COLOR_STYLE_TYPE),
        cadmpeg_ir::identity_key!("source"),
        0,
    )))
    .expect("the primary color record serializes")
}

#[test]
fn legacy_rendering_padding_and_comment_state_are_not_semantic_reads() {
    let mut bytes = 77_u32.to_le_bytes().to_vec();
    bytes.extend(3_u16.to_le_bytes());
    bytes.extend([0xa1; 4]);
    bytes.push(2);
    bytes.extend(4_u16.to_le_bytes());
    bytes.extend(5_u16.to_le_bytes());
    bytes.extend(6_u16.to_le_bytes());
    bytes.extend(7_u32.to_le_bytes());
    bytes.extend(8_u32.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    super::text(&mut bytes, "N\0");
    super::text(&mut bytes, "C");
    bytes.extend(0x1234_u16.to_le_bytes());
    super::text(&mut bytes, "L");
    bytes.extend([0xb2; 4]);
    assert_eq!(bytes.len(), 55);
    let record = rendering(&bytes, 14);
    assert_eq!(record["name"], "N");
    assert_eq!(record["comment"], "C");
    assert_eq!(record["long_name"], "L");
    let mut changed = bytes.clone();
    changed[6..10].fill(0xcc);
    changed[43..45].copy_from_slice(&0xabcd_u16.to_le_bytes());
    changed[51..55].fill(0xdd);
    let other = rendering(&changed, 14);
    assert_eq!(other, record);
    changed[35..37].copy_from_slice(&[b'X', 0]);
    let mut text_change = rendering(&changed, 14);
    assert_eq!(text_change["name"], "NX");
    text_change["name"] = record["name"].clone();
    assert_eq!(text_change, record);
}

#[test]
fn primary_color_digest_and_legacy_padding_do_not_claim_semantic_fields() {
    let mut bytes = vec![0xa1; 4];
    bytes.extend(31_u32.to_le_bytes());
    bytes.extend([0xa2; 4]);
    for value in 32_u16..=38 {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0xa3; 4]);
    bytes.extend([39, 40]);
    for color in [[0.1_f32; 4], [0.25; 4], [0.7; 4], [0.9; 4]] {
        for value in color {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0xa4; 4]);
    }
    bytes.extend(41_u16.to_le_bytes());
    bytes.extend(42_u16.to_le_bytes());
    bytes.extend([0xa5; 4]);
    bytes.push(43);
    bytes.extend(44_u16.to_le_bytes());
    bytes.extend(45_u16.to_le_bytes());
    bytes.extend([0xa6; 4]);
    bytes.push(46);
    assert_eq!(bytes.len(), 130);
    let record = primary(&bytes);
    let mut changed = bytes.clone();
    for range in [
        0..4,
        8..12,
        26..30,
        48..52,
        68..72,
        88..92,
        108..112,
        116..120,
        125..129,
    ] {
        changed[range].fill(0xee);
    }
    assert_eq!(primary(&changed), record);
    changed[30..32].copy_from_slice(&[11, 12]);
    let mut other = primary(&changed);
    let mut original = record;
    assert_ne!(
        other["color_header_sha256"],
        original["color_header_sha256"]
    );
    other
        .as_object_mut()
        .expect("the primary color record is an object")
        .remove("color_header_sha256");
    original
        .as_object_mut()
        .expect("the primary color record is an object")
        .remove("color_header_sha256");
    assert_eq!(other, original);
}

#[test]
fn navigation_preserves_truncation_positions_and_zero_validation_without_read_credit() {
    let bytes = [0_u8, 0, 1];
    let arena = DecodeArena::new();
    let (_ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the source fits the service policy");
    {
        let mut cursor = Cursor::new(root);
        cursor
            .zeroes(2, "alignment")
            .expect("the alignment bytes are zero");
        let error = cursor
            .zeroes(1, "suffix")
            .expect_err("a nonzero suffix is refused");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "Inventor presentation suffix is not zero-filled"));
        let error = cursor
            .skip(1, "legacy padding")
            .expect_err("padding past the end is refused");
        let mut expected = root;
        expected
            .req_take_child(3)
            .expect("the source holds three bytes");
        let expected = crate::reader::take(&mut expected, 1, "legacy padding")
            .expect_err("a take past the end is refused");
        assert_eq!(error.to_string(), expected.to_string());
        assert_eq!(cursor.source.position(), 3);
    }
}

#[test]
fn late_rendering_parse_failure_and_native_budget_refusal_discard_actual_source_reads() {
    let mut bytes = super::rendering_style();
    bytes.pop();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the truncated record fits the service policy");
    assert!(presentation::parse_rendering_style(&ctx, root, 18).is_err());

    let bytes = super::rendering_style();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("the record fits the zero entity policy");
    let mut output = PartOutput::new(CodecFormat::Inventor, &ctx);
    let parsed =
        presentation::parse_rendering_style(&ctx, root, 18).expect("the rendering style parses");
    let record = presentation::native::rendering_style(&Located::new(
        parsed,
        RecordTypeId::from_bytes(presentation::RENDERING_STYLE_TYPE),
        cadmpeg_ir::identity_key!("source"),
        0,
    ))
    .expect("the rendering style forms a native record");
    let result = output.insert_native(&ctx, "pm_app_rendering_styles", record);
    let Err(CodecError::ResourceLimit(expected)) = result else {
        panic!("native construction must retain the real entity refusal")
    };
    assert_eq!(expected.dimension, ResourceDimension::Entities);
    assert!(output.native().namespace("inventor").is_none());
}

#[test]
fn late_source_body_failure_keeps_exact_bytes_and_an_independent_rendering_peer() {
    use crate::container::InventorContainer;
    use crate::native::PmAppRenderingStyleRecord;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};

    let peer = super::rendering_style();
    let mut damaged = peer.clone();
    damaged.pop();
    let bytes = crate::test_support::test_fixtures::presentation_envelope_fixture(
        &SegmentKind::PmApp,
        &[
            (presentation::RENDERING_STYLE_TYPE, damaged.clone()),
            (presentation::RENDERING_STYLE_TYPE, peer.clone()),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.kind == SegmentKind::PmApp)
        .expect("the container holds a pm app segment");
    let SegmentBulkState::Framed(bulk) = &segment.bulk else {
        panic!("actual expanded source")
    };
    let RecordFrameState::Framed(table) = &bulk.records else {
        panic!("actual source frames")
    };
    assert_eq!(table.records[0].payload.window(), damaged);
    assert_eq!(table.records[1].payload.window(), peer);
    let bad_id = format!(
        "inventor:presentation:rendering-style#{}-0",
        segment.pair.token.as_str()
    );
    let good_id = format!(
        "inventor:presentation:rendering-style#{}-1",
        segment.pair.token.as_str()
    );
    let failed = crate::records::source_admission::body(bulk.expanded, &table.records[0])
        .expect("the first record frame holds a body");
    let mut output = PartOutput::new(CodecFormat::Inventor, &ctx);
    let inventory = presentation::inventory(&ctx, &container.rse, &mut output)
        .expect("the presentation inventory decodes");
    assert_eq!(inventory.rendering_styles.len(), 1);
    assert_eq!(inventory.rendering_styles[0].identity.record_ordinal, 1);
    let (_, native, fidelity, losses) = output.into_parts();
    let ns = native
        .namespace("inventor")
        .expect("the inventor namespace exists");
    let styles = ns
        .arena_as::<PmAppRenderingStyleRecord>("pm_app_rendering_styles")
        .expect("the rendering styles arena holds rendering style records");
    assert_eq!(styles.len(), 1);
    assert_eq!(styles[0].id, good_id);
    let wire = serde_json::to_value(&styles[0]).expect("a rendering style record serializes");
    assert_eq!(wire["name"], "actual style");
    assert_eq!(wire["long_name"], "long name");
    // #215 rule 1: the given-up part keeps one unknowns home.
    let unknowns = ns.arenas().get("unknowns").expect("one unknowns home");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .keys()
            .next()
            .expect("one retained part")
            .as_str()
    );
    assert!(!ns.arenas().contains_key("presentation_record_issues"));
    assert!(!styles.iter().any(|style| style.id == bad_id));
    let given_up = crate::test_support::given_up_records(&fidelity);
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].data(), Some(failed.window()));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(!losses[0].message.contains(&good_id));
}
