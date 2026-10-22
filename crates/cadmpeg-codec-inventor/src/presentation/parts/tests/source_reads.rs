// SPDX-License-Identifier: Apache-2.0
//! Semantic fields, source navigation, and failed-record read rollback.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::CodecFormat;
use serde_json::Value;
use std::ops::Range;

use crate::record_identity::{Located, RecordTypeId};

use super::super::super::{self as presentation, Cursor};

fn rendering(bytes: &[u8], version: u8) -> (Value, Vec<Range<u64>>) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let record = ctx
        .record(
            "inventor:presentation:rendering-style#source-0",
            root,
            || {
                let parsed = presentation::parse_rendering_style(&ctx, root, version)?;
                presentation::native::rendering_style(&Located::new(
                    parsed,
                    RecordTypeId::from_bytes(presentation::RENDERING_STYLE_TYPE),
                    cadmpeg_ir::identity_key!("source"),
                    0,
                ))
            },
        )
        .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    let (_, _, _, _, reads) = captures.into_iter().next().unwrap().into_parts();
    (serde_json::to_value(record).unwrap(), reads["file"].clone())
}

fn primary(bytes: &[u8]) -> (Value, Vec<Range<u64>>) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let record = ctx
        .record(
            "inventor:presentation:graphics-primary-color#source-0",
            root,
            || {
                let parsed = presentation::parse_graphics_primary_color_style(root, 14)?;
                Ok(presentation::native::primary_color(&Located::new(
                    parsed,
                    RecordTypeId::from_bytes(presentation::GRAPHICS_PRIMARY_COLOR_STYLE_TYPE),
                    cadmpeg_ir::identity_key!("source"),
                    0,
                )))
            },
        )
        .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    let (_, _, _, _, reads) = captures.into_iter().next().unwrap().into_parts();
    (serde_json::to_value(record).unwrap(), reads["file"].clone())
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
    let (record, reads) = rendering(&bytes, 14);
    assert_eq!(record["name"], "N");
    assert_eq!(record["comment"], "C");
    assert_eq!(record["long_name"], "L");
    assert_eq!(reads, [0..6, 10..43, 45..51]);
    let mut changed = bytes.clone();
    changed[6..10].fill(0xcc);
    changed[43..45].copy_from_slice(&0xabcd_u16.to_le_bytes());
    changed[51..55].fill(0xdd);
    let (other, other_reads) = rendering(&changed, 14);
    assert_eq!(other, record);
    assert_eq!(other_reads, reads);
    changed[35..37].copy_from_slice(&[b'X', 0]);
    let (mut text_change, text_reads) = rendering(&changed, 14);
    assert_eq!(text_change["name"], "NX");
    text_change["name"] = record["name"].clone();
    assert_eq!(text_change, record);
    assert_eq!(text_reads, reads);
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
    let (record, reads) = primary(&bytes);
    assert_eq!(
        reads,
        [
            4..8,
            12..26,
            32..48,
            52..68,
            72..88,
            92..108,
            112..116,
            120..125,
            129..130
        ]
    );
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
    assert_eq!(primary(&changed), (record.clone(), reads.clone()));
    changed[30..32].copy_from_slice(&[11, 12]);
    let (mut other, other_reads) = primary(&changed);
    let mut original = record;
    assert_ne!(
        other["color_header_sha256"],
        original["color_header_sha256"]
    );
    other.as_object_mut().unwrap().remove("color_header_sha256");
    original
        .as_object_mut()
        .unwrap()
        .remove("color_header_sha256");
    assert_eq!(other, original);
    assert_eq!(other_reads, reads);
}

#[test]
fn navigation_preserves_truncation_positions_and_zero_validation_without_read_credit() {
    let bytes = [0_u8, 0, 1];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    ctx.record("inventor:test:navigation#0", root, || {
        let mut cursor = Cursor::new(root);
        cursor.zeroes(2, "alignment")?;
        let error = cursor.zeroes(1, "suffix").unwrap_err();
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "Inventor presentation suffix is not zero-filled"));
        let error = cursor.skip(1, "legacy padding").unwrap_err();
        let mut expected = root;
        expected.req_take_child(3).unwrap();
        let expected = crate::reader::take(&mut expected, 1, "legacy padding").unwrap_err();
        assert_eq!(error.to_string(), expected.to_string());
        assert_eq!(cursor.source.position(), 3);
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    let (_, _, _, _, reads) = captures.into_iter().next().unwrap().into_parts();
    assert!(reads.is_empty());
}

#[test]
fn late_rendering_parse_failure_and_native_budget_refusal_discard_actual_source_reads() {
    let mut bytes = super::rendering_style();
    bytes.pop();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(ctx
        .record(
            "inventor:presentation:rendering-style#source-0",
            root,
            || { presentation::parse_rendering_style(&ctx, root, 18) }
        )
        .is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());

    let bytes = super::rendering_style();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let result = ctx.record(
        "inventor:presentation:rendering-style#source-0",
        root,
        || {
            let parsed = presentation::parse_rendering_style(&ctx, root, 18)?;
            let record = presentation::native::rendering_style(&Located::new(
                parsed,
                RecordTypeId::from_bytes(presentation::RENDERING_STYLE_TYPE),
                cadmpeg_ir::identity_key!("source"),
                0,
            ))?;
            output.insert_native(&ctx, "pm_app_rendering_styles", record)
        },
    );
    let Err(CodecError::ResourceLimit(expected)) = result else {
        panic!("native construction must retain the real entity refusal")
    };
    assert_eq!(expected.dimension, ResourceDimension::Entities);
    assert!(output.native().namespace("inventor").is_none());
    assert!(
        matches!(ctx.take_record_captures(), Err(CodecError::ResourceLimit(actual))
        if actual == expected)
    );
}

#[test]
fn late_source_body_failure_keeps_exact_bytes_and_an_independent_rendering_peer() {
    use crate::container::InventorContainer;
    use crate::native::PmAppRenderingStyleRecord;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use cadmpeg_ir::NativeUnknownRecord;

    let peer = super::rendering_style();
    let mut damaged = peer.clone();
    damaged.pop();
    let bytes = crate::test_support::test_fixtures::presentation_envelope_fixture(
        SegmentKind::PmApp,
        &[
            (presentation::RENDERING_STYLE_TYPE, damaged.clone()),
            (presentation::RENDERING_STYLE_TYPE, peer.clone()),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.kind == SegmentKind::PmApp)
        .unwrap();
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
    let failed =
        crate::records::source_admission::body(&ctx, bulk.expanded, &table.records[0]).unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let inventory = presentation::inventory(&ctx, &container.rse, &mut output).unwrap();
    assert_eq!(inventory.rendering_styles.len(), 1);
    assert_eq!(inventory.rendering_styles[0].identity.record_ordinal, 1);
    let (_, native, mut fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let ns = native.namespace("inventor").unwrap();
    let styles = ns
        .arena_as::<PmAppRenderingStyleRecord>("pm_app_rendering_styles")
        .unwrap();
    assert_eq!(styles.len(), 1);
    assert_eq!(styles[0].id, good_id);
    let wire = serde_json::to_value(&styles[0]).unwrap();
    assert_eq!(wire["name"], "actual style");
    assert_eq!(wire["long_name"], "long name");
    let unknowns = ns.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(failed.window())
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(losses[0].message.contains(&bad_id));
    assert!(!losses[0].message.contains(&good_id));
    fidelity.install_mapped_record_captures(&ctx, Ok).unwrap();
    assert!(!fidelity.record_layout().contains_key(&bad_id));
    assert!(fidelity.record_layout().contains_key(&good_id));
}
