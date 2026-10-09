// SPDX-License-Identifier: Apache-2.0
//! Pattern source vector shells are temporary; projected children survive.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::chunks::{ArchiveVersion, FramingError};
use crate::presentation::{
    parse_hatch_pattern, parse_linetype, HatchLineRecord, LinetypeSegment,
    PatternTransferError, SourceHatchLine, SourceLinetypeSegment,
};
use crate::settings::{MillimeterScale, UnitBinding};
use crate::test_support::test_dump::utf16_bytes;

fn linetype() -> Vec<u8> {
    let mut body = 7_i32.to_le_bytes().to_vec();
    body.extend(utf16_bytes(""));
    body.extend(1_i32.to_le_bytes());
    body.extend(2.5_f64.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    super::anonymous(0, &body)
}

fn hatch(modern: bool) -> Vec<u8> {
    let mut line = Vec::new();
    for value in [0.5_f64, 1.0, 2.0, 3.0, 4.0] {
        line.extend(value.to_le_bytes());
    }
    line.extend(1_i32.to_le_bytes());
    line.extend(5.0_f64.to_le_bytes());
    if modern {
        let component = super::anonymous(0, &0_u32.to_le_bytes());
        let mut body = component;
        body.extend(1_i32.to_le_bytes());
        body.extend(utf16_bytes(""));
        let mut lines = 1_i32.to_le_bytes().to_vec();
        lines.extend(super::anonymous(0, &line));
        body.extend(super::anonymous_body(&lines));
        super::anonymous(0, &body)
    } else {
        let mut body = vec![0x10];
        body.extend(7_i32.to_le_bytes());
        body.extend(1_i32.to_le_bytes());
        body.extend(utf16_bytes(""));
        body.extend(utf16_bytes(""));
        body.extend(1_i32.to_le_bytes());
        body.push(0x10);
        body.extend(line);
        body
    }
}

#[test]
fn linetype_source_shell_releases_with_two_projected_records_live() {
    let bytes = linetype();
    let expected_id = "rhino:presentation:linetype#record-23";
    let output_bytes = expected_id.len() + std::mem::size_of::<LinetypeSegment>();
    let source_bytes = u64::try_from(std::mem::size_of::<SourceLinetypeSegment>()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(2 * output_bytes).unwrap();
    policy.limits.max_materialized_bytes = source_bytes;
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let parse = || parse_linetype(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V5,
        UnitBinding::Millimeters(MillimeterScale::IDENTITY), 23).unwrap();
    let first = parse();
    let second = parse();
    for record in [&first, &second] {
        assert_eq!(record.id, expected_id);
        assert_eq!(record.name, "");
        assert_eq!(record.archive_index, Some(7));
        assert_eq!(record.source_uuid, None);
        assert_eq!(record.segments.len(), 1);
        assert_eq!(record.segments[0].length_millimeters.get(), 2.5);
        assert_eq!(record.segments[0].segment_type, 0);
    }
    let released = ctx.reserve_scoped(source_bytes, "linetype source shell reuse").unwrap();
    drop(released);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

fn assert_hatch_source_released(modern: bool) {
    let bytes = hatch(modern);
    let expected_id = "rhino:presentation:hatch_pattern#record-23";
    // The projected line moves the parsed one-element dash Vec unchanged.
    let output_bytes = expected_id.len() + std::mem::size_of::<HatchLineRecord>()
        + std::mem::size_of::<cadmpeg_ir::scalar::FiniteReal>();
    let source_bytes = u64::try_from(std::mem::size_of::<SourceHatchLine>()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(2 * output_bytes).unwrap();
    policy.limits.max_materialized_bytes = source_bytes;
    policy.limits.max_collection_items = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let parse = || parse_hatch_pattern(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V5,
        UnitBinding::Millimeters(MillimeterScale::IDENTITY), 23).unwrap();
    let first = parse();
    let second = parse();
    for record in [&first, &second] {
        assert_eq!(record.id, expected_id);
        assert_eq!(record.name, "");
        assert_eq!(record.description, "");
        assert_eq!(record.source_uuid, None);
        assert_eq!(record.archive_index, if modern { None } else { Some(7) });
        assert_eq!(record.lines.len(), 1);
        let line = &record.lines[0];
        assert_eq!(line.angle_radians.get(), 0.5);
        assert_eq!(line.base_millimeters.map(|value| value.get()), [1.0, 2.0]);
        assert_eq!(line.offset_millimeters.map(|value| value.get()), [3.0, 4.0]);
        assert_eq!(line.dashes_millimeters.len(), 1);
        assert_eq!(line.dashes_millimeters[0].get(), 5.0);
    }
    let released = ctx.reserve_scoped(source_bytes, "hatch source shell reuse").unwrap();
    drop(released);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn legacy_hatch_source_shell_releases_with_transferred_dashes_live() {
    assert_hatch_source_released(false);
}

#[test]
fn modern_hatch_source_shell_releases_with_transferred_dashes_live() {
    assert_hatch_source_released(true);
}

fn assert_original_scratch_refusal(modern: Option<bool>) {
    let bytes = modern.map_or_else(linetype, hatch);
    let (operation, bytes_needed) = match modern {
        None => ("Rhino linetype segments", std::mem::size_of::<SourceLinetypeSegment>()),
        Some(false) => ("Rhino legacy hatch lines", std::mem::size_of::<SourceHatchLine>()),
        Some(true) => ("Rhino modern hatch lines", std::mem::size_of::<SourceHatchLine>()),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = match modern {
        None => parse_linetype(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V5,
            UnitBinding::Millimeters(MillimeterScale::IDENTITY), 23).unwrap_err(),
        Some(_) => parse_hatch_pattern(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V5,
            UnitBinding::Millimeters(MillimeterScale::IDENTITY), 23).unwrap_err(),
    };
    let PatternTransferError::Framing(FramingError::Resource(original)) = error
        else { panic!("source shell refusal"); };
    assert_eq!(original.operation, operation);
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(original.used, 0);
    assert_eq!(original.additional, u64::try_from(bytes_needed).unwrap());
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn linetype_source_shell_keeps_original_backing_refusal() {
    assert_original_scratch_refusal(None);
}

#[test]
fn legacy_hatch_source_shell_keeps_original_backing_refusal() {
    assert_original_scratch_refusal(Some(false));
}

#[test]
fn modern_hatch_source_shell_keeps_original_backing_refusal() {
    assert_original_scratch_refusal(Some(true));
}
