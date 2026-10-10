// SPDX-License-Identifier: Apache-2.0
//! Bounded scalar diagnostics preserve structural recovery without budget work.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::settings;
use crate::test_support::test_dump::{crc_chunk};

#[test]
fn nonfinite_point_diagnostic_has_no_budget_fee() {
    let bytes: Vec<_> = [f64::NAN, 1.0, 2.0].into_iter().flat_map(f64::to_le_bytes).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = settings::point(&ctx, &mut BoundedReader::new(&bytes, 0, bytes.len()).unwrap()).unwrap_err();
    assert!(matches!(error, FramingError::Structural { offset: 0, message } if message == "point contains a nonfinite value"));
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn invalid_short_index_diagnostic_has_no_budget_fee() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let record = crate::container::Record::short(settings::CURRENT_LAYER, 0..0, -2);
    let mut values = settings::DocumentSettings::default();
    let error = settings::parse_setting(&ctx, &[], &record, &mut values, ArchiveVersion::V8).unwrap_err();
    assert!(matches!(error, FramingError::Structural { message, .. } if message == "current layer is not a valid short index"));
    assert_eq!(values.current_layer, None);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn bounded_anonymous_diagnostics_keep_the_integration_text() {
    let bytes = crc_chunk(ArchiveVersion::V8, 0x4000_8000, &[2, 0, 0, 0, 0, 0, 0, 0]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    let (mut payload, _) = settings::anonymous_payload(&bytes, &mut reader, ArchiveVersion::V8, "IO settings").unwrap();
    assert!(matches!(settings::anonymous_version(&mut payload, "IO settings"), Err(FramingError::Structural { message, .. }) if message == "IO settings version is unsupported"));
    let bytes = crate::test_support::test_dump::crc_chunk(ArchiveVersion::V8, 0x4000_8001, &[]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert!(matches!(settings::anonymous_payload(&bytes, &mut reader, ArchiveVersion::V8, "IO settings"), Err(FramingError::Structural { message, .. }) if message == "IO settings must be a long anonymous chunk"));
    assert!(matches!(settings::begin_direct_object(&bytes, &mut reader, ArchiveVersion::V8, "embedded linetype"), Err(FramingError::Structural { message, .. }) if message == "embedded linetype must be an object chunk"));
}
