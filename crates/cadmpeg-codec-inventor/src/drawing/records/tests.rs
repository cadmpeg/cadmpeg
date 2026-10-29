// SPDX-License-Identifier: Apache-2.0

use super::{
    parse, Family, Fields, Record, ALTERNATE_VIEW, CACHED_VIEW, MODEL_REFERENCE, SHEET, VIEW,
};
use crate::test_support::test_fixtures::{push_u16, push_u32, push_utf16};
use cadmpeg_core::decode::View;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn list(bytes: &mut Vec<u8>, count: u32) {
    push_u16(bytes, 2);
    push_u16(bytes, 0x3000);
    push_u32(bytes, count);
    if count != 0 {
        push_u32(bytes, count);
        push_u32(bytes, 0);
        for index in 0..count {
            push_u32(bytes, 0x8000_0001 + index);
        }
    }
}

fn sheet(properties: u32) -> Vec<u8> {
    let mut bytes = vec![0; 30];
    push_utf16(&mut bytes, "Generated sheet");
    list(&mut bytes, properties);
    bytes.extend_from_slice(&[0; 24]);
    list(&mut bytes, 2);
    bytes.extend_from_slice(&[0; 14]);
    for _ in 0..3 {
        list(&mut bytes, 0);
    }
    bytes.extend_from_slice(&[0x5a; 8]);
    bytes
}

fn view(type_id: [u8; 16], operands: u32) -> Vec<u8> {
    let mut bytes = vec![0; 30];
    list(&mut bytes, operands);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 3);
    if type_id == ALTERNATE_VIEW {
        push_u32(&mut bytes, 0);
    }
    push_utf16(&mut bytes, "Generated view");
    bytes.extend_from_slice(&[0xa5, 0x5a]);
    bytes
}

fn model_reference(path: &str) -> Vec<u8> {
    let mut bytes = vec![0; 49];
    push_utf16(&mut bytes, path);
    bytes.extend_from_slice(&[0x55, 0xaa]);
    bytes
}

fn cached_view() -> Vec<u8> {
    let mut bytes = vec![0; 28];
    push_u32(&mut bytes, 1);
    bytes.extend_from_slice(&[0x33, 0xcc]);
    bytes
}

fn record(
    family: Family,
    type_id: [u8; 16],
    payload: Vec<u8>,
) -> Result<Record, cadmpeg_core::CodecError> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let fields = parse(&ctx, family, 19, type_id, View::over_retained(&payload))?;
    let token = cadmpeg_ir::identity_key!("generated");
    Ok(Record {
        id: Record::identity(&token, 0),
        segment_token: token,
        ordinal: 0,
        segment_version_major: 19,
        type_id: crate::record_identity::RecordTypeId::from_bytes(type_id),
        family,
        payload: payload.into(),
        fields,
    })
}

#[test]
fn sheet_prefix_keeps_both_lists_and_its_opaque_tail() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    let record = record(Family::Sheet, SHEET, sheet(6))?;
    let Fields::Sheet {
        properties,
        views,
        tail,
        label,
        ..
    } = &record.fields
    else {
        return Err("sheet fields".into());
    };
    assert_eq!(label, "Generated sheet");
    assert_eq!(properties.references().len(), 6);
    assert_eq!(views.references().len(), 2);
    assert!(views
        .references()
        .iter()
        .all(|reference| reference.qualified()));
    assert_eq!(tail.len, 46);
    assert!(record.valid(&ctx)?);
    let restored: Record = serde_json::from_value(serde_json::to_value(&record)?)?;
    assert_eq!(restored, record);
    assert!(restored.valid(&ctx)?);
    Ok(())
}

#[test]
fn both_view_forms_keep_every_operand_and_the_extension() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    for type_id in [VIEW, ALTERNATE_VIEW] {
        let record = record(Family::Sheet, type_id, view(type_id, 19))?;
        let Fields::View {
            operands,
            extension,
            text,
            tail,
            ..
        } = &record.fields
        else {
            return Err("view fields".into());
        };
        assert_eq!(operands.references().len(), 19);
        assert_eq!(*extension, (type_id == ALTERNATE_VIEW).then_some(0));
        assert_eq!(text, "Generated view");
        assert_eq!(tail.len, 2);
        assert!(record.valid(&ctx)?);
        let restored: Record = serde_json::from_value(serde_json::to_value(&record)?)?;
        assert_eq!(restored, record);
        assert!(restored.valid(&ctx)?);
    }
    Ok(())
}

#[test]
fn modeled_document_reference_keeps_each_path_form() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    for path in ["generated.ipt", "generated.iam", "C:\\generated\\part.ipt"] {
        let record = record(Family::Browser, MODEL_REFERENCE, model_reference(path))?;
        let Fields::ModelReference {
            path: stored, tail, ..
        } = &record.fields
        else {
            return Err("reference fields".into());
        };
        assert_eq!(stored, path);
        assert_eq!(tail.len, 2);
        assert!(record.valid(&ctx)?);
        let restored: Record = serde_json::from_value(serde_json::to_value(&record)?)?;
        assert!(restored.valid(&ctx)?);
    }
    for path in [
        "",
        "generated.idw",
        "generated.ipt.extra",
        "generated.IPT",
        "generated.IAM",
        "generated",
    ] {
        assert!(record(Family::Browser, MODEL_REFERENCE, model_reference(path)).is_err());
    }
    Ok(())
}

#[test]
fn decoder_and_validator_refuse_adjacent_counts_families_and_versions() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (type_id, count) in [
        (VIEW, 18),
        (VIEW, 20),
        (ALTERNATE_VIEW, 18),
        (ALTERNATE_VIEW, 20),
    ] {
        assert!(parse(
            &ctx,
            Family::Sheet,
            19,
            type_id,
            View::over_retained(&view(type_id, count))
        )
        .is_err());
    }
    for count in [0, 5, 7] {
        assert!(parse(
            &ctx,
            Family::Sheet,
            19,
            SHEET,
            View::over_retained(&sheet(count))
        )
        .is_err());
    }
    for (family, type_id, bytes) in [
        (Family::Sheet, SHEET, sheet(6)),
        (Family::Sheet, VIEW, view(VIEW, 19)),
        (Family::Sheet, ALTERNATE_VIEW, view(ALTERNATE_VIEW, 19)),
        (
            Family::Browser,
            MODEL_REFERENCE,
            model_reference("generated.ipt"),
        ),
        (Family::SheetCache, CACHED_VIEW, cached_view()),
    ] {
        let valid = record(family, type_id, bytes)?;
        for version in [18, 20] {
            assert!(parse(
                &ctx,
                family,
                version,
                type_id,
                View::over_retained(&valid.payload)
            )
            .is_err());
            let mut invalid = valid.clone();
            invalid.segment_version_major = version;
            assert!(!invalid.valid(&ctx)?);
        }
        let mut invalid = valid.clone();
        invalid.family = Family::Document;
        assert!(!invalid.valid(&ctx)?);
        invalid = valid.clone();
        invalid.id.push('x');
        assert!(!invalid.valid(&ctx)?);
        invalid = valid.clone();
        invalid.payload.pop();
        assert!(!invalid.valid(&ctx)?);
        invalid = valid.clone();
        match &mut invalid.fields {
            Fields::CachedView { view, .. } => *view = crate::pmdc::PmDcReference::from_packed(0),
            Fields::Sheet { label, .. } => label.push('x'),
            Fields::View { text, .. } => text.push('x'),
            Fields::ModelReference { path, .. } => path.push('x'),
        }
        assert!(!invalid.valid(&ctx)?);
    }
    Ok(())
}

#[test]
fn short_prefixes_and_impossible_counts_are_located_refusals() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (family, type_id, bytes, prefix_len) in [
        (Family::Sheet, SHEET, sheet(6), 30),
        (Family::Sheet, VIEW, view(VIEW, 19), 30),
        (Family::Sheet, ALTERNATE_VIEW, view(ALTERNATE_VIEW, 19), 30),
        (
            Family::Browser,
            MODEL_REFERENCE,
            model_reference("generated.ipt"),
            49,
        ),
        (Family::SheetCache, CACHED_VIEW, cached_view(), 32),
    ] {
        for end in 0..prefix_len {
            assert!(parse(
                &ctx,
                family,
                19,
                type_id,
                View::over_retained(&bytes[..end])
            )
            .is_err());
        }
    }
    let mut bytes = view(VIEW, 19);
    bytes[34..38].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse(&ctx, Family::Sheet, 19, VIEW, View::over_retained(&bytes)).is_err());
}

#[test]
fn cached_view_requires_a_nonnull_external_reference() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    let valid = record(Family::SheetCache, CACHED_VIEW, cached_view())?;
    assert!(valid.valid(&ctx)?);
    let Fields::CachedView { view, tail, .. } = &valid.fields else {
        return Err("cached view fields".into());
    };
    assert_eq!(view.index(), 1);
    assert!(!view.qualified());
    assert_eq!(tail.len, 2);
    for value in [0_u32, 0x8000_0000, 0x8000_0001] {
        let mut bytes = vec![0; 28];
        push_u32(&mut bytes, value);
        assert!(record(Family::SheetCache, CACHED_VIEW, bytes.clone()).is_err());
        let mut invalid = valid.clone();
        invalid.payload = bytes.into();
        assert!(!invalid.valid(&ctx)?);
    }
    Ok(())
}

#[test]
fn sheet_form_requires_segment_major_version_nineteen() -> TestResult {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut record = record(Family::Sheet, SHEET, sheet(6))?;
    assert!(record.valid(&ctx)?);
    for version in [18, 20, 23] {
        record.segment_version_major = version;
        assert!(!record.valid(&ctx)?);
    }
    Ok(())
}

#[test]
fn every_view_operand_requires_its_qualified_local_target_class() -> TestResult {
    let record = record(Family::Sheet, VIEW, view(VIEW, 19))?;
    let expected = [
        "2e187506d2111f5e6000b3856c2387b0",
        "9ff49f1dd11101e96000a8ade3bc87b0",
        "3edf52ced011d0d20008ccbc0663dc09",
        "3edf52ced011d0d20008ccbc0663dc09",
        "4debf5fad41120726000bebead9287b0",
        "35df52ced011d0d20008ccbc0663dc09",
        "474dd06b2047478c9da481bcfae149d5",
        "229630f7d111a466600027a5602d6bb0",
        "5c42cb22d111c557600037bd861c3cb0",
        "284d8790d011f8d10008cabc0663dc09",
        "284d8790d011f8d10008cabc0663dc09",
        "ebf44026d111e457600037bd861c3cb0",
        "264d8790d011f8d10008cabc0663dc09",
        "264d8790d011f8d10008cabc0663dc09",
        "e9ee786bd11195a460009dade3bc87b0",
        "e9ee786bd11195a460009dade3bc87b0",
        "2e187506d2111f5e6000b3856c2387b0",
        "9ff49f1dd11101e96000a8ade3bc87b0",
        "35df52ced011d0d20008ccbc0663dc09",
    ];
    assert!(record.operands_are_valid(|ordinal| usize::try_from(ordinal)
        .ok()
        .and_then(|index| expected.get(index).copied())));
    for changed in 0..19_u32 {
        assert!(!record.operands_are_valid(|ordinal| {
            if ordinal == changed {
                Some("00000000000000000000000000000000")
            } else {
                usize::try_from(ordinal)
                    .ok()
                    .and_then(|index| expected.get(index).copied())
            }
        }));
        assert!(!record.operands_are_valid(|ordinal| {
            if ordinal == changed {
                None
            } else {
                usize::try_from(ordinal)
                    .ok()
                    .and_then(|index| expected.get(index).copied())
            }
        }));
    }
    let mut invalid = record.clone();
    let Fields::View { operands, .. } = &mut invalid.fields else {
        return Err("view fields".into());
    };
    let (marker, metadata, mut refs) = operands.clone().into_parts();
    refs[0] = crate::pmdc::PmDcReference::from_packed(1);
    *operands =
        crate::pmdc::PmDcReferenceList::new(marker, metadata, refs).ok_or("operand list")?;
    assert!(
        !invalid.operands_are_valid(|ordinal| usize::try_from(ordinal)
            .ok()
            .and_then(|index| expected.get(index).copied()))
    );
    Ok(())
}

#[test]
fn sheet_property_targets_require_their_local_record_class() -> TestResult {
    let record = record(Family::Sheet, SHEET, sheet(6))?;
    assert!(record.operands_are_valid(|_| Some("754d8790d011f8d10008cabc0663dc09")));
    assert!(!record.operands_are_valid(|_| Some("00000000000000000000000000000000")));
    assert!(!record.operands_are_valid(|_| None));
    Ok(())
}

#[test]
fn short_prefix_error_keeps_the_input_space_and_offset() -> TestResult {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    let bytes = [0_u8; 9];
    let arena = DecodeArena::new();
    let (ctx, mut root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())?;
    crate::reader::take(&mut root, 7, "generated prefix")?;
    let expected = root.location();
    let error = parse(&ctx, Family::Browser, 19, MODEL_REFERENCE, root)
        .expect_err("short model-reference prefix");
    let CodecError::Truncated { location, .. } = error else {
        return Err("located truncation".into());
    };
    assert_eq!(location, expected);
    Ok(())
}
