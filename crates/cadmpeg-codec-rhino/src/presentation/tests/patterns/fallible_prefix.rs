// SPDX-License-Identifier: Apache-2.0
//! Hatch projection stops before unvisited coordinates and lines.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::chunks::{ArchiveVersion, FramingError};
use crate::presentation::{parse_hatch_pattern, PatternTransferError, SourceHatchLine};
use crate::settings::{MillimeterScale, StandardUnit, UnitBinding};
use crate::test_support::finite;

#[test]
fn invalid_fixed_hatch_coordinate_does_not_admit_any_dash() {
    let line = SourceHatchLine {
        angle_radians: finite(0.0),
        base: [finite(f64::MAX), finite(0.0)],
        offset: [finite(0.0); 2],
        dashes: vec![finite(1.0); 1024],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = line.into_millimeters(&ctx, StandardUnit::Centimeters.into(), 42).unwrap_err();
    assert!(matches!(error, FramingError::Structural { offset: 42, message } if message == "scaled hatch line is invalid"));
    ctx.finish_session().unwrap();
}

#[test]
fn invalid_first_hatch_dash_does_not_admit_the_tail() {
    let line = SourceHatchLine {
        angle_radians: finite(0.0),
        base: [finite(0.0); 2],
        offset: [finite(0.0); 2],
        dashes: vec![finite(f64::MAX); 1024],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = line.into_millimeters(&ctx, StandardUnit::Centimeters.into(), 42).unwrap_err();
    assert!(matches!(error, FramingError::Structural { offset: 42, message } if message == "scaled hatch line is invalid"));
    ctx.finish_session().unwrap();
}

#[test]
fn empty_hatch_dash_source_has_no_exhaustion_work() {
    let line = SourceHatchLine {
        angle_radians: finite(0.5),
        base: [finite(1.0), finite(2.0)],
        offset: [finite(3.0), finite(4.0)],
        dashes: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let projected = line.into_millimeters(&ctx, MillimeterScale::IDENTITY, 42).unwrap();
    assert_eq!(projected.base_millimeters, [finite(1.0), finite(2.0)]);
    assert_eq!(projected.offset_millimeters, [finite(3.0), finite(4.0)]);
    assert!(projected.dashes_millimeters.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn hatch_pattern_projection_stops_after_first_invalid_line() {
    let line_count = 1024_i32;
    let mut bytes = vec![0x10];
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(1_i32.to_le_bytes());
    // Empty name and description have no text to scan or copy.
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(line_count.to_le_bytes());
    for _ in 0..line_count {
        bytes.push(0x10);
        for value in [0.0_f64, f64::MAX, 0.0, 0.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0_i32.to_le_bytes());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Parsing executes one visit per line. Projection executes only the first line.
    policy.limits.max_work_units = u64::try_from(line_count).unwrap() + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = parse_hatch_pattern(&ctx, &bytes, 0..bytes.len(), ArchiveVersion::V5,
        UnitBinding::Millimeters(StandardUnit::Centimeters.into()), 42).unwrap_err();
    assert!(matches!(error, PatternTransferError::Framing(FramingError::Structural { offset: 42, message })
        if message == "scaled hatch line is invalid"));
    ctx.finish_session().unwrap();
}

#[test]
fn linetype_projection_stops_after_first_missing_unit_binding() {
    let segment_count = 1024_i32;
    let component = super::super::anonymous(0, &0_u32.to_le_bytes());
    let mut payload = 2_i32.to_le_bytes().to_vec();
    payload.extend(3_i32.to_le_bytes());
    payload.extend(component);
    payload.extend(segment_count.to_le_bytes());
    for _ in 0..segment_count {
        payload.extend(1.0_f64.to_le_bytes());
        payload.extend(0_u32.to_le_bytes());
    }
    payload.extend([6, 1, 0]);
    let bytes = super::super::anonymous_body(&payload);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Parsing visits each stored segment; conversion reaches only the first.
    policy.limits.max_work_units = u64::try_from(segment_count).unwrap() + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = crate::presentation::parse_linetype(
        &ctx, &bytes, 0..bytes.len(), ArchiveVersion::V8, UnitBinding::Native, 42,
    ).unwrap_err();
    assert!(matches!(error, PatternTransferError::NativeDocumentUnits));
    ctx.finish_session().unwrap();
}
