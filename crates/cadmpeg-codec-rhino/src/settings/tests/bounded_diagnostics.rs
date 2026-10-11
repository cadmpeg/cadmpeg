// SPDX-License-Identifier: Apache-2.0
//! Bounded scalar diagnostics preserve structural recovery without budget work.

use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::settings;
use crate::test_support::test_dump::crc_chunk;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn nonfinite_point_diagnostic_has_no_budget_fee() {
    let bytes: Vec<_> = [f64::NAN, 1.0, 2.0]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = settings::point(
        &ctx,
        &mut BoundedReader::new(&bytes, 0, bytes.len()).unwrap(),
    )
    .unwrap_err();
    assert!(
        matches!(error, FramingError::Structural { offset: 0, message } if message == "point contains a nonfinite value")
    );
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
    let error =
        settings::parse_setting(&ctx, &[], &record, &mut values, ArchiveVersion::V8).unwrap_err();
    assert!(
        matches!(error, FramingError::Structural { message, .. } if message == "current layer is not a valid short index")
    );
    assert_eq!(values.current_layer, None);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn bounded_anonymous_diagnostics_keep_the_integration_text() {
    let bytes = crc_chunk(ArchiveVersion::V8, 0x4000_8000, &[2, 0, 0, 0, 0, 0, 0, 0]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    let (mut payload, _) =
        settings::anonymous_payload(&bytes, &mut reader, ArchiveVersion::V8, "IO settings")
            .unwrap();
    assert!(
        matches!(settings::anonymous_version(&mut payload, "IO settings"), Err(FramingError::Structural { message, .. }) if message == "IO settings version is unsupported")
    );
    let bytes = crate::test_support::test_dump::crc_chunk(ArchiveVersion::V8, 0x4000_8001, &[]);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert!(
        matches!(settings::anonymous_payload(&bytes, &mut reader, ArchiveVersion::V8, "IO settings"), Err(FramingError::Structural { message, .. }) if message == "IO settings must be a long anonymous chunk")
    );
    assert!(
        matches!(settings::begin_direct_object(&bytes, &mut reader, ArchiveVersion::V8, "embedded linetype"), Err(FramingError::Structural { message, .. }) if message == "embedded linetype must be an object chunk")
    );
}

#[test]
fn layer_class_diagnostic_has_no_text_budget_fee() {
    use crate::loss::Diagnostics;
    use crate::test_support::test_dump::{class_wrapper, metadata_record};

    let archive = ArchiveVersion::V8;
    let (bytes, record) = metadata_record(
        settings::LAYER_RECORD,
        class_wrapper(archive, [0x44; 16], &[]),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = settings::parse_layer(
        &ctx,
        &bytes,
        &record,
        archive,
        None,
        &mut Diagnostics::new(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        matches!(error, FramingError::Structural { message, .. } if message == "layer record has class UUID 44444444-4444-4444-4444-444444444444")
    );
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

fn layer_before_rendering(minor: u8) -> Vec<u8> {
    use crate::test_support::test_dump::utf16_bytes;

    let mut payload = vec![0x10 | minor];
    for value in [0_i32, 7, -1, -1, 0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend([0, 0, 0, 255]);
    payload.extend([0; 4]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(utf16_bytes(""));
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0, 0, 0, 255]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend([0; 16]);
    payload
}

fn assert_layer_wrapper_without_admission(payload: &[u8], label: &str, needed: usize) {
    use crate::loss::Diagnostics;
    use crate::test_support::test_dump::{class_wrapper, metadata_record};

    let archive = ArchiveVersion::V8;
    let (bytes, record) = metadata_record(
        settings::LAYER_RECORD,
        class_wrapper(archive, settings::ON_LAYER_UUID.to_wire(), payload),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "Rhino parse_layer text",
        None,
    );
    let mut warnings = Diagnostics::new();
    let mut losses = Vec::new();
    let error = settings::parse_layer(
        &ctx,
        &bytes,
        &record,
        archive,
        Some(200_505_000),
        &mut warnings,
        &mut losses,
    )
    .unwrap_err();
    drop(probe);
    let FramingError::Structural { offset, message } = error else {
        panic!("bounded layer framing error");
    };
    let expected_offset = crate::objects::parse_class_wrapper(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        record.body(),
        archive,
        &mut Diagnostics::new(),
    )
    .unwrap()
    .class_data_range
    .end;
    assert_eq!(offset, expected_offset);
    let source = FramingError::OutOfBounds {
        offset: expected_offset,
        end: expected_offset + needed,
        bound: expected_offset,
    };
    assert_eq!(message, format!("{label}: {source}"));
    assert!(warnings.is_empty());
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn truncated_layer_rendering_diagnostic_has_no_budget_fee() {
    assert_layer_wrapper_without_admission(&layer_before_rendering(7), "rendering", 4);
}

#[test]
fn truncated_layer_display_material_diagnostic_has_no_budget_fee() {
    let mut payload = layer_before_rendering(8);
    let mut rendering = 1_i32.to_le_bytes().to_vec();
    rendering.extend(0_i32.to_le_bytes());
    rendering.extend(0_i32.to_le_bytes());
    payload.extend(crc_chunk(ArchiveVersion::V8, 0x4000_8000, &rendering));
    assert_layer_wrapper_without_admission(&payload, "display material", 16);
}
