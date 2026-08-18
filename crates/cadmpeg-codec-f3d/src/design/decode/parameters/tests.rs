// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use super::{
    decode_parameters, parse_design_parameter, parse_legacy_parameter_owner_68,
    parse_legacy_parameter_owner_88, parse_parameter_companion, parse_parameter_owner,
};
use crate::design::test_support::{lp_utf16, parameter_owner_frame, parameter_record};
use crate::records::DesignParameterKind;
use crate::test_support::*;

fn compact_owned_parameter_record(
    owner_record_index: u32,
    source_ordinal: u32,
    expression: &str,
    source_kind: &str,
    unit: Option<&str>,
    name: &str,
    evaluated_value: f64,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"328");
    out.extend_from_slice(&(owner_record_index + 1).to_le_bytes());
    out.extend_from_slice(&[0; 15]);
    out.extend_from_slice(&source_ordinal.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&owner_record_index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    lp_utf16(&mut out, expression);
    out.extend_from_slice(&[0; 5]);
    lp_utf16(&mut out, source_kind);
    if let Some(unit) = unit {
        lp_utf16(&mut out, unit);
    } else {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    lp_utf16(&mut out, name);
    out.extend_from_slice(&evaluated_value.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

fn class_287_parameter_record(source_kind: &str, name: &str) -> Vec<u8> {
    class_287_parameter_record_with_expression_trailer(source_kind, name, [0; 5])
}

fn class_287_parameter_record_with_expression_trailer(
    source_kind: &str,
    name: &str,
    expression_trailer: [u8; 5],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"287");
    out.extend_from_slice(&887u32.to_le_bytes());
    out.extend_from_slice(&[0; 15]);
    out.extend_from_slice(&20u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&886u32.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    lp_utf16(&mut out, "0.4375 in");
    out.extend_from_slice(&expression_trailer);
    lp_utf16(&mut out, source_kind);
    lp_utf16(&mut out, "in");
    lp_utf16(&mut out, name);
    out.extend_from_slice(&1.11125f64.to_le_bytes());
    out.extend_from_slice(&[0, 1, 175, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

#[test]
fn class_287_parameter_accepts_the_compact_prefix_with_af_tail() {
    let parameter = parse_design_parameter(&class_287_parameter_record("HoleDepth", "d20"))
        .expect("class-287 parameter");
    assert_eq!(parameter.class_tag, "287");
    assert_eq!(parameter.record_index, 887);
    assert_eq!(parameter.owner_record_index, Some(886));
    assert_eq!(parameter.source_ordinal, 20);
    assert_eq!(parameter.expression, "0.4375 in");
    assert_eq!(parameter.expression_offset, 45);
    assert_eq!(parameter.source_kind, "HoleDepth");
    assert_eq!(parameter.unit.as_deref(), Some("in"));
    assert_eq!(parameter.unit_offset, Some(94));
    assert_eq!(parameter.name, "d20");
    assert_eq!(parameter.evaluated_value_offset, 108);

    let dimension =
        parse_design_parameter(&class_287_parameter_record("Diameter Dimension-2", "d1"))
            .expect("class-287 dimension parameter");
    assert_eq!(dimension.source_kind, "Diameter Dimension-2");
    assert_eq!(dimension.name, "d1");
}

#[test]
fn class_287_parameter_accepts_the_marked_expression_trailer() {
    let parameter = parse_design_parameter(&class_287_parameter_record_with_expression_trailer(
        "OffsetX",
        "d63",
        [0, 0, 0, 1, 0],
    ))
    .expect("class-287 parameter with marked expression trailer");
    assert_eq!(parameter.source_kind, "OffsetX");
    assert_eq!(parameter.name, "d63");

    let malformed =
        class_287_parameter_record_with_expression_trailer("OffsetX", "d63", [0, 0, 0, 2, 0]);
    assert!(parse_design_parameter(&malformed).is_none());
}

#[test]
fn class_287_parameter_requires_its_marker_and_tail() {
    let mut frame = class_287_parameter_record("HoleDepth", "d20");
    frame[30] = 0;
    assert!(parse_design_parameter(&frame).is_none());

    let mut frame = class_287_parameter_record("HoleDepth", "d20");
    let tail = frame.len() - 12;
    frame[tail + 2] = 174;
    assert!(parse_design_parameter(&frame).is_none());
}

#[test]
fn compact_owned_design_parameter_has_no_family_discriminator() {
    let bytes =
        compact_owned_parameter_record(6653, 99, "82.00 mm", "Diameter", Some("mm"), "d99", 8.2);
    let parameter = parse_design_parameter(&bytes).expect("compact owned parameter");
    assert_eq!(parameter.record_index, 6654);
    assert_eq!(parameter.owner_record_index, Some(6653));
    assert_eq!(parameter.source_ordinal, 99);
    assert_eq!(parameter.family_discriminator, None);
    assert_eq!(parameter.family_discriminator_offset, None);
    assert_eq!(parameter.expression, "82.00 mm");
    assert_eq!(parameter.source_kind, "Diameter");
    assert_eq!(parameter.unit.as_deref(), Some("mm"));
    assert_eq!(parameter.name, "d99");
    assert_eq!(parameter.evaluated_value, 8.2);
}

#[test]
fn legacy_owned_design_parameter_uses_the_compact_identity_prefix() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"296");
    bytes.extend_from_slice(&439_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&437_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    lp_utf16(&mut bytes, "0.00 mm");
    bytes.extend_from_slice(&[0; 5]);
    lp_utf16(&mut bytes, "OffsetX");
    lp_utf16(&mut bytes, "mm");
    lp_utf16(&mut bytes, "d5");
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    let parameter = parse_design_parameter(&bytes).expect("legacy owned parameter");
    assert_eq!(parameter.record_index, 439);
    assert_eq!(parameter.owner_record_index, Some(437));
    assert_eq!(parameter.source_ordinal, 5);
    assert_eq!(parameter.source_kind, "OffsetX");
    assert_eq!(parameter.unit.as_deref(), Some("mm"));
    assert_eq!(parameter.name, "d5");
    assert_eq!(parameter.evaluated_value, 0.0);
}

#[test]
fn parameter_variants_have_exact_string_and_scalar_boundaries() {
    let user = parse_design_parameter(&parameter_record(
        None,
        "60 mm",
        "User Parameter",
        Some("mm"),
        "Width",
        6.0,
    ))
    .unwrap();
    assert_eq!(user.kind, DesignParameterKind::User);
    assert_eq!(user.owner_record_index, None);
    assert_eq!(user.unit.as_deref(), Some("mm"));
    assert_eq!(user.evaluated_value, 6.0);

    let feature = parse_design_parameter(&parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    ))
    .unwrap();
    assert_eq!(feature.kind, DesignParameterKind::Feature);
    assert_eq!(feature.owner_record_index, Some(44));
    assert_eq!(feature.expression, "Width / 2");

    let boolean = parse_design_parameter(&parameter_record(
        None,
        "1",
        "User Parameter",
        None,
        "OnOff",
        1.0,
    ))
    .unwrap();
    assert_eq!(boolean.unit, None);
    assert_eq!(boolean.name, "OnOff");

    let mut tangency = parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
    tangency[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tangency = parse_design_parameter(&tangency).expect("prefixed unitless parameter");
    assert_eq!(tangency.family_discriminator, Some(6));
    assert_eq!(tangency.unit, None);
    assert_eq!(tangency.name, "d81");
    assert_eq!(tangency.evaluated_value, 1.0);

    let mut earlier_tangency =
        parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
    earlier_tangency[22..30].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        parse_design_parameter(&earlier_tangency)
            .expect("earlier tangency parameter")
            .family_discriminator,
        Some(0)
    );

    let mut scale_factor = parameter_record(Some(1331), "1", "ScaleFactor", None, "scale", 1.0);
    let scale_factor_tail = scale_factor.len() - 12;
    scale_factor[scale_factor_tail + 2] = 16;
    let scale_factor = parse_design_parameter(&scale_factor).expect("scale-factor parameter");
    assert_eq!(scale_factor.family_discriminator, Some(5));
    assert_eq!(scale_factor.owner_record_index, Some(1331));
    assert_eq!(scale_factor.unit, None);
    assert_eq!(scale_factor.evaluated_value, 1.0);

    for discriminator in [3u64, 4] {
        let mut earlier_distance = parameter_record(
            Some(44),
            "Width / 2",
            "AlongDistance",
            Some("mm"),
            "d12",
            3.0,
        );
        earlier_distance[22..30].copy_from_slice(&discriminator.to_le_bytes());
        assert_eq!(
            parse_design_parameter(&earlier_distance)
                .expect("earlier feature parameter")
                .family_discriminator,
            Some(discriminator)
        );
    }

    let mut invalid_tangency = earlier_tangency;
    invalid_tangency[22..30].copy_from_slice(&5u64.to_le_bytes());
    assert!(parse_design_parameter(&invalid_tangency).is_none());

    let mut revised_distance = parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    );
    revised_distance[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tail = revised_distance.len() - 12;
    revised_distance[tail + 2] = 16;
    assert_eq!(
        parse_design_parameter(&revised_distance)
            .expect("revision-six feature parameter")
            .family_discriminator,
        Some(6)
    );

    let mut invalid_distance = revised_distance.clone();
    invalid_distance[22..30].copy_from_slice(&7u64.to_le_bytes());
    assert!(parse_design_parameter(&invalid_distance).is_none());

    revised_distance[tail + 2] = 19;
    assert!(parse_design_parameter(&revised_distance).is_none());

    let mut sheet_metal =
        parameter_record(Some(301), "50.00 mm", "FlangeHeight", Some("mm"), "d2", 5.0);
    sheet_metal[22..30].copy_from_slice(&6u64.to_le_bytes());
    let (_, expression_end) =
        crate::bytes::lp_utf16_bounded(&sheet_metal, 46, 1..=256).expect("sheet-metal expression");
    sheet_metal.insert(expression_end + 9, 0);
    let tail = sheet_metal.len() - 12;
    sheet_metal[tail + 2] = 16;
    let sheet_metal = parse_design_parameter(&sheet_metal)
        .expect("sheet-metal parameter with ten-byte expression trailer");
    assert_eq!(sheet_metal.source_kind, "FlangeHeight");
    assert_eq!(sheet_metal.owner_record_index, Some(301));
    assert_eq!(sheet_metal.evaluated_value, 5.0);
}

#[test]
fn parameter_record_rejects_noncanonical_tail() {
    let mut record = parameter_record(
        Some(44),
        "45 deg",
        "TaperAngle",
        Some("deg"),
        "d13",
        std::f64::consts::FRAC_PI_4,
    );
    *record.last_mut().unwrap() = 1;
    assert!(parse_design_parameter(&record).is_none());
}

#[test]
fn duplicate_parameter_index_keeps_the_first_serialized_frame() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut bulk = parameter_record(Some(44), "first", "AlongDistance", Some("mm"), "d71", 1.0);
    let second = parameter_record(Some(44), "second", "AlongDistance", Some("mm"), "d71", 2.0);
    bulk.extend_from_slice(&second);

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let parameters = with_scan(&archive, decode_parameters).unwrap();
    let [parameter] = parameters.as_slice() else {
        panic!("expected one canonical parameter");
    };
    assert_eq!(parameter.record_index, 71);
    assert_eq!(parameter.byte_offset, 0);
    assert_eq!(parameter.expression, "first");
    assert_eq!(parameter.evaluated_value, 1.0);
}

fn compact_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 103];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"406");
    frame[7..11].copy_from_slice(&6653u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&6644u32.to_le_bytes());
    frame[35..39].copy_from_slice(&0u32.to_le_bytes());
    frame[40..48].copy_from_slice(&8.2f64.to_le_bytes());
    frame[48] = 1;
    frame[49..53].copy_from_slice(&6654u32.to_le_bytes());
    frame[59..63].copy_from_slice(&4u32.to_le_bytes());
    frame[67] = 1;
    frame[68..72].copy_from_slice(&6644u32.to_le_bytes());
    frame[80] = 1;
    frame[81..85].copy_from_slice(&6655u32.to_le_bytes());
    frame[92] = 1;
    frame[93..97].copy_from_slice(&6644u32.to_le_bytes());
    frame
}

fn legacy_parameter_owner_68_frame(class_tag: &str) -> Vec<u8> {
    let mut frame = vec![0; 68];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(class_tag.as_bytes());
    frame[7..11].copy_from_slice(&100u32.to_le_bytes());
    frame[19] = 1;
    frame[33] = 1;
    frame[34..38].copy_from_slice(&101u32.to_le_bytes());
    frame[44..48].copy_from_slice(&290u32.to_le_bytes());
    frame[55] = 1;
    frame[56..60].copy_from_slice(&102u32.to_le_bytes());
    frame
}

fn legacy_parameter_owner_88_frame(class_tag: &str) -> Vec<u8> {
    let mut frame = legacy_parameter_owner_68_frame(class_tag);
    frame.resize(88, 0);
    frame[48..52].fill(0);
    frame[52] = 1;
    frame[53..57].copy_from_slice(&77u32.to_le_bytes());
    frame[65] = 1;
    frame[66..70].copy_from_slice(&102u32.to_le_bytes());
    frame[77] = 1;
    frame[78..82].copy_from_slice(&77u32.to_le_bytes());
    frame
}

fn counted_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 101];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"316");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40] = 1;
    frame[41..45].copy_from_slice(&6u32.to_le_bytes());
    frame[45] = 1;
    frame[46..50].copy_from_slice(&45u32.to_le_bytes());
    frame[56..60].copy_from_slice(&9u32.to_le_bytes());
    frame[64] = 1;
    frame[65..69].copy_from_slice(&12u32.to_le_bytes());
    frame[75] = 1;
    frame[76] = 1;
    frame[78] = 1;
    frame[79..83].copy_from_slice(&46u32.to_le_bytes());
    frame[90] = 1;
    frame[91..95].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn compact_typed_counted_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 100];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"320");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[40] = 1;
    frame[41..45].copy_from_slice(&19u32.to_le_bytes());
    frame[45] = 1;
    frame[46..50].copy_from_slice(&46u32.to_le_bytes());
    frame[56..60].copy_from_slice(&9u32.to_le_bytes());
    frame[64] = 1;
    frame[65..69].copy_from_slice(&12u32.to_le_bytes());
    frame[77] = 1;
    frame[78..82].copy_from_slice(&45u32.to_le_bytes());
    frame[89] = 1;
    frame[90..94].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn compact_counted_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 99];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"457");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40..44].copy_from_slice(&6u32.to_le_bytes());
    frame[44] = 1;
    frame[45..49].copy_from_slice(&45u32.to_le_bytes());
    frame[55..59].copy_from_slice(&9u32.to_le_bytes());
    frame[63] = 1;
    frame[64..68].copy_from_slice(&12u32.to_le_bytes());
    frame[76] = 1;
    frame[77..81].copy_from_slice(&46u32.to_le_bytes());
    frame[88] = 1;
    frame[89..93].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn tagged_scalar_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 107];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"406");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[39] = 1;
    frame[44..52].copy_from_slice(&6.0f64.to_le_bytes());
    frame[52] = 1;
    frame[53..57].copy_from_slice(&45u32.to_le_bytes());
    frame[63..67].copy_from_slice(&9u32.to_le_bytes());
    frame[71] = 1;
    frame[72..76].copy_from_slice(&12u32.to_le_bytes());
    frame[84] = 1;
    frame[85..89].copy_from_slice(&46u32.to_le_bytes());
    frame[96] = 1;
    frame[97..101].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn tagged_scalar_variant_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 108];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"299");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&1u32.to_le_bytes());
    frame[39] = 1;
    frame[44..52].copy_from_slice(&0.8f64.to_le_bytes());
    frame[52] = 1;
    frame[53..57].copy_from_slice(&45u32.to_le_bytes());
    frame[63..67].copy_from_slice(&73u32.to_le_bytes());
    frame[71] = 1;
    frame[72..76].copy_from_slice(&12u32.to_le_bytes());
    frame[82] = 1;
    frame[85] = 1;
    frame[86..90].copy_from_slice(&46u32.to_le_bytes());
    frame[97] = 1;
    frame[98..102].copy_from_slice(&12u32.to_le_bytes());
    frame
}

#[test]
fn parameter_owner_frame_has_repeated_scope_and_both_record_orders() {
    let parsed = parse_parameter_owner(&parameter_owner_frame()).unwrap();
    assert_eq!(parsed.frame_length, 104);
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.scope_record_index, 12);
    assert_eq!(parsed.local_ordinal, 2);
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.owned_ordinal, 9);
    assert_eq!(parsed.variant, Some(1));
    assert_eq!(parsed.companion_record_index, 46);

    let mut parameter_first = parameter_owner_frame();
    parameter_first[49..53].copy_from_slice(&43u32.to_le_bytes());
    parameter_first[82..86].copy_from_slice(&45u32.to_le_bytes());
    let parsed = parse_parameter_owner(&parameter_first).expect("parameter-first owner frame");
    assert_eq!(parsed.parameter_record_index, 43);
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.companion_record_index, 45);

    let mut malformed = parameter_owner_frame();
    malformed[94..98].copy_from_slice(&13u32.to_le_bytes());
    assert!(parse_parameter_owner(&malformed).is_none());
}

#[test]
fn parameter_owner_requires_its_complete_structural_suffix() {
    for build in [
        parameter_owner_frame as fn() -> Vec<u8>,
        compact_parameter_owner_frame,
        counted_parameter_owner_frame,
        compact_typed_counted_parameter_owner_frame,
        compact_counted_parameter_owner_frame,
        tagged_scalar_parameter_owner_frame,
        tagged_scalar_variant_parameter_owner_frame,
    ] {
        let frame = build();
        assert!(parse_parameter_owner(&frame).is_some());
        let mut longer = frame.clone();
        longer.push(0);
        assert!(parse_parameter_owner(&longer).is_none());
        assert!(parse_parameter_owner(&frame[..frame.len() - 1]).is_none());
    }

    assert_eq!(
        parse_parameter_owner(&parameter_owner_frame())
            .expect("owner frame")
            .evaluated_value_offset,
        40
    );
    assert_eq!(
        parse_parameter_owner(&compact_parameter_owner_frame())
            .expect("compact owner frame")
            .evaluated_value_offset,
        40
    );
}

#[test]
fn compact_parameter_owner_omits_the_variant_slot() {
    let parsed =
        parse_parameter_owner(&compact_parameter_owner_frame()).expect("compact parameter owner");
    assert_eq!(parsed.frame_length, 103);
    assert_eq!(parsed.record_index, 6653);
    assert_eq!(parsed.scope_record_index, 6644);
    assert_eq!(parsed.parameter_record_index, 6654);
    assert_eq!(parsed.companion_record_index, 6655);
    assert_eq!(parsed.owned_ordinal, 4);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.evaluated_value, 8.2);
}

#[test]
fn counted_parameter_owner_uses_typed_u32_scalar() {
    let parsed =
        parse_parameter_owner(&counted_parameter_owner_frame()).expect("counted parameter owner");
    assert_eq!(parsed.frame_length, 101);
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.evaluated_value_offset, 41);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn legacy_counted_parameter_owner_uses_zero_typed_u32_scalar() {
    let mut frame = counted_parameter_owner_frame();
    frame[40] = 0;
    let parsed = parse_parameter_owner(&frame)
        .expect("legacy counted parameter owner with zero scalar marker");
    assert_eq!(parsed.frame_length, 101);
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.evaluated_value_offset, 41);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn legacy_parameter_owner_68_uses_parameter_scalar_and_zero_scope() {
    let parsed = parse_legacy_parameter_owner_68(&legacy_parameter_owner_68_frame("284"), 0.0)
        .expect("legacy 68-byte parameter owner");
    assert_eq!(parsed.frame_length, 68);
    assert_eq!(parsed.class_tag, "284");
    assert_eq!(parsed.record_index, 100);
    assert_eq!(parsed.parameter_record_index, 101);
    assert_eq!(parsed.companion_record_index, 102);
    assert_eq!(parsed.scope_record_index, 0);
    assert_eq!(parsed.local_ordinal, 0);
    assert_eq!(parsed.owned_ordinal, 290);
    assert_eq!(parsed.evaluated_value, 0.0);

    for class_tag in ["268", "282", "336", "325", "297"] {
        assert!(
            parse_legacy_parameter_owner_68(&legacy_parameter_owner_68_frame(class_tag), 1.25)
                .is_some()
        );
    }
}

#[test]
fn legacy_parameter_owner_68_requires_its_admitted_class_and_shape() {
    assert!(
        parse_legacy_parameter_owner_68(&legacy_parameter_owner_68_frame("291"), 1.0).is_none()
    );

    let mut malformed = legacy_parameter_owner_68_frame("284");
    malformed[55] = 0;
    assert!(parse_legacy_parameter_owner_68(&malformed, 1.0).is_none());
}

#[test]
fn legacy_parameter_owner_88_repeats_a_nonzero_scope_without_a_scalar_lane() {
    let parsed = parse_legacy_parameter_owner_88(&legacy_parameter_owner_88_frame("284"), 2.5)
        .expect("legacy 88-byte parameter owner");
    assert_eq!(parsed.frame_length, 88);
    assert_eq!(parsed.scope_record_index, 77);
    assert_eq!(parsed.local_ordinal, 0);
    assert_eq!(parsed.owned_ordinal, 290);
    assert_eq!(parsed.parameter_record_index, 101);
    assert_eq!(parsed.companion_record_index, 102);
    assert_eq!(parsed.evaluated_value, 2.5);

    let mut mismatched = legacy_parameter_owner_88_frame("284");
    mismatched[78..82].copy_from_slice(&78u32.to_le_bytes());
    assert!(parse_legacy_parameter_owner_88(&mismatched, 2.5).is_none());
}

#[test]
fn compact_typed_counted_parameter_owner_omits_variant_slot() {
    let parsed = parse_parameter_owner(&compact_typed_counted_parameter_owner_frame())
        .expect("compact typed counted parameter owner");
    assert_eq!(parsed.frame_length, 100);
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.scope_record_index, 12);
    assert_eq!(parsed.local_ordinal, 0);
    assert_eq!(parsed.evaluated_value, 19.0);
    assert_eq!(parsed.evaluated_value_offset, 41);
    assert_eq!(parsed.parameter_record_index, 46);
    assert_eq!(parsed.owned_ordinal, 9);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 45);
}

#[test]
fn compact_counted_parameter_owner_omits_type_and_variant_markers() {
    let mut frame = compact_counted_parameter_owner_frame();
    frame[45..49].copy_from_slice(&46u32.to_le_bytes());
    frame[77..81].copy_from_slice(&45u32.to_le_bytes());
    let parsed = parse_parameter_owner(&frame).expect("compact counted parameter owner");
    assert_eq!(parsed.frame_length, 99);
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.evaluated_value_offset, 40);
    assert_eq!(parsed.parameter_record_index, 46);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 45);
}

#[test]
fn tagged_scalar_parameter_owner_carries_a_scalar_type_prefix() {
    let parsed = parse_parameter_owner(&tagged_scalar_parameter_owner_frame())
        .expect("tagged scalar parameter owner");
    assert_eq!(parsed.frame_length, 107);
    assert_eq!(parsed.evaluated_value, 6.0);
    assert_eq!(parsed.evaluated_value_offset, 44);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn tagged_scalar_parameter_owner_can_carry_a_variant_slot() {
    let parsed = parse_parameter_owner(&tagged_scalar_variant_parameter_owner_frame())
        .expect("tagged scalar variant parameter owner");
    assert_eq!(parsed.frame_length, 108);
    assert_eq!(parsed.evaluated_value, 0.8);
    assert_eq!(parsed.evaluated_value_offset, 44);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.owned_ordinal, 73);
    assert_eq!(parsed.variant, Some(0));
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn parameter_companion_prefix_has_owner_backlink_and_timestamp() {
    let mut prefix = vec![0; 58];
    prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
    prefix[4..7].copy_from_slice(b"408");
    prefix[7..11].copy_from_slice(&46u32.to_le_bytes());
    prefix[31] = 1;
    prefix[32..36].copy_from_slice(&44u32.to_le_bytes());
    prefix[42..50].copy_from_slice(&1_678_000_000_000_000u64.to_le_bytes());

    let parsed = parse_parameter_companion(&prefix).unwrap();
    assert_eq!(parsed.record_index, 46);
    assert_eq!(parsed.owner_record_index, 44);
    assert_eq!(parsed.timestamp_micros, 1_678_000_000_000_000);
    assert_eq!(parsed.timestamp_micros_offset, 42);

    prefix[32..36].copy_from_slice(&45u32.to_le_bytes());
    assert_eq!(
        parse_parameter_companion(&prefix)
            .unwrap()
            .owner_record_index,
        45
    );
    prefix[42..50].fill(0);
    assert!(parse_parameter_companion(&prefix).is_none());
}

#[test]
fn parameter_owner_uses_the_paired_same_index_header_as_its_boundary() {
    fn owner_frame() -> Vec<u8> {
        let mut frame = vec![0; 104];
        frame[0..4].copy_from_slice(&3u32.to_le_bytes());
        frame[4..7].copy_from_slice(b"292");
        frame[7..11].copy_from_slice(&44u32.to_le_bytes());
        frame[19] = 1;
        frame[20..24].copy_from_slice(&1u32.to_le_bytes());
        frame[24] = 1;
        frame[25..29].copy_from_slice(&12u32.to_le_bytes());
        frame[35..39].copy_from_slice(&2u32.to_le_bytes());
        frame[40..48].copy_from_slice(&6.0f64.to_le_bytes());
        frame[48] = 1;
        frame[49..53].copy_from_slice(&45u32.to_le_bytes());
        frame[59..63].copy_from_slice(&9u32.to_le_bytes());
        frame[67] = 1;
        frame[68..72].copy_from_slice(&12u32.to_le_bytes());
        frame[78] = 1;
        frame[79] = 1;
        frame[81] = 1;
        frame[82..86].copy_from_slice(&46u32.to_le_bytes());
        frame[93] = 1;
        frame[94..98].copy_from_slice(&12u32.to_le_bytes());
        frame
    }
    fn paired_header() -> [u8; 11] {
        let mut header = [0; 11];
        header[0..4].copy_from_slice(&3u32.to_le_bytes());
        header[4..7].copy_from_slice(b"293");
        header[7..11].copy_from_slice(&44u32.to_le_bytes());
        header
    }
    fn archive(stream: &str, bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(stream, stored).unwrap();
        zip.write_all(bulk).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let parameter = crate::records::DesignParameter {
        id: crate::ids::native_design_parameter_id(stream, 200),
        byte_offset: 200,
        class_tag: "305".into(),
        record_index: 45,
        family_discriminator: Some(0),
        family_discriminator_offset: Some(222),
        source_ordinal: 0,
        owner_record_index: Some(44),
        expression: "6 cm".into(),
        expression_offset: 240,
        source_kind: "Distance".into(),
        source_kind_offset: 260,
        kind: crate::records::DesignParameterKind::Feature,
        unit: Some("cm".into()),
        unit_offset: Some(280),
        name: "distance".into(),
        name_offset: 300,
        evaluated_value: 6.0,
        evaluated_value_offset: 320,
    };
    let header = crate::records::DesignRecordHeader {
        id: crate::ids::native_design_record_header_id(stream, 0),
        record_index: 44,
        class_tag: "292".into(),
        byte_offset: 0,
    };

    let mut exact = owner_frame();
    exact.extend_from_slice(&paired_header());
    let owners = with_scan(&archive(stream, &exact), |scan| {
        crate::design::decode::parameters::decode_parameter_owners(
            scan,
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&header),
        )
    })
    .expect("exact owner frame");
    let [owner] = owners.as_slice() else {
        panic!("expected one parameter owner");
    };
    assert_eq!(owner.frame_length, 104);
    assert_eq!(owner.evaluated_value_offset, 40);

    let unresolved_parameter = crate::records::DesignParameter {
        class_tag: "287".into(),
        ..parameter.clone()
    };
    let unresolved = with_scan(&archive(stream, &[]), |scan| {
        crate::design::decode::parameters::decode_parameter_owners(
            scan,
            std::slice::from_ref(&unresolved_parameter),
            &[],
        )
    })
    .expect("missing owner frame is retained as an unresolved binding");
    assert!(unresolved.is_empty());

    let error = with_scan(&archive(stream, &[]), |scan| {
        crate::design::decode::parameters::decode_parameter_owners(
            scan,
            std::slice::from_ref(&parameter),
            &[],
        )
    })
    .expect_err("an unadmitted parameter family must retain the missing-owner refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));

    let mut extended = owner_frame();
    extended.push(0);
    extended.extend_from_slice(&paired_header());
    let error = with_scan(&archive(stream, &extended), |scan| {
        crate::design::decode::parameters::decode_parameter_owners(
            scan,
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&header),
        )
    })
    .expect_err("an owner-shaped prefix must not shorten the exact frame");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}
