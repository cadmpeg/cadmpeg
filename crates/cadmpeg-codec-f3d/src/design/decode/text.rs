// SPDX-License-Identifier: Apache-2.0
//! Charged text reads shared by Design record decoders.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Validate a borrowed three-digit class tag before making its fixed-size copy.
pub(in crate::design::decode) fn class_tag_from_view(
    ctx: &DecodeContext<'_>,
    value: &str,
) -> Result<Result<crate::records::references::DesignClassTag, String>, CodecError> {
    if value.len() != 3 || !ctx.admit_iter(value.as_bytes(), "validate F3D class tag digits")?.all(|byte| byte.is_ascii_digit()) {
        return Ok(Err("class_tag must contain three ASCII digits".into()));
    }
    Ok(crate::records::references::DesignClassTag::try_from(value.to_owned()))
}

/// Compose a native scope and record suffix under the retained text budget.
pub(in crate::design::decode) fn design_record_id_charged(
    ctx: &DecodeContext<'_>,
    stream: &str,
    suffix: &'static str,
    offset: u64,
    charge_operation: &'static str,
) -> Result<String, CodecError> {
    let mut id = super::sketch::native_scope_charged(ctx, stream)?;
    ctx.append_formatted_retained(&mut id, format_args!("{suffix}{offset}"), charge_operation)?;
    Ok(id)
}

/// Validate an exact 36-code-unit relaxed GUID into a fixed ASCII array.
pub(in crate::design::decode) fn fixed_guid_ascii(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    count_at: usize,
) -> Result<Option<([u8; 36], usize)>, CodecError> {
    let Some((units, end)) = (|| {
        (View::u32_le_at(bytes, count_at)? == 36).then_some(())?;
        let start = count_at.checked_add(4)?;
        let end = start.checked_add(72)?;
        Some((bytes.get(start..end)?, end))
    })() else { return Ok(None); };
    let width = std::num::NonZeroUsize::new(2)
        .ok_or_else(|| CodecError::malformed("F3D GUID code unit width is zero"))?;
    let mut guid = [0; 36];
    for (unit, slot) in ctx.admit_iter(units, "validate F3D relaxed GUID code units")?
        .chunks(width).zip(guid.iter_mut()) {
        if unit[1] != 0 || !(unit[0].is_ascii_alphanumeric() || matches!(unit[0], b'-' | b'_')) {
            return Ok(None);
        }
        *slot = unit[0];
    }
    Ok(Some((guid, end)))
}

/// Read a fixed-width relaxed GUID into its native value after code-unit validation.
pub(in crate::design::decode) fn fixed_relaxed_guid_text(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    count_at: usize,
) -> Result<Option<(crate::records::mesh::DesignRelaxedGuidText, usize)>, CodecError> {
    let Some((guid, end)) = fixed_guid_ascii(ctx, bytes, count_at)? else {
        return Ok(None);
    };
    // UTF-8 validation, copy, GUID validation, and identity-key validation.
    ctx.charge_work(36 * 4, "copy and admit F3D relaxed GUID")?;
    let guid = std::str::from_utf8(&guid)
        .map_err(|_| CodecError::malformed("validated F3D relaxed GUID is not ASCII"))?;
    let text = ctx.copy_retained_text(guid, "retain F3D relaxed GUID")?;
    let value = crate::records::mesh::DesignRelaxedGuidText::try_from(text)
        .map_err(CodecError::malformed)?;
    Ok(Some((value, end)))
}

/// Validate an exact 36-code-unit relaxed GUID in UTF-16LE without copying it.
pub(in crate::design::decode) fn fixed_guid_end(
    ctx: &DecodeContext<'_>, bytes: &[u8], count_at: usize,
) -> Result<Option<usize>, CodecError> {
    Ok(fixed_guid_ascii(ctx, bytes, count_at)?.map(|(_, end)| end))
}

/// Validate a counted relaxed GUID without allocating its text.
pub(in crate::design::decode) fn relaxed_guid_end(
    ctx: &DecodeContext<'_>, bytes: &[u8], count_at: usize,
) -> Result<Option<usize>, CodecError> {
    let Some((units, end)) = (|| {
        let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
        if !(36..=38).contains(&count) { return None; }
        let start = count_at.checked_add(4)?;
        let end = start.checked_add(count.checked_mul(2)?)?;
        Some((bytes.get(start..end)?, end))
    })() else { return Ok(None); };
    let width = std::num::NonZeroUsize::new(2)
        .ok_or_else(|| CodecError::malformed("F3D GUID code unit width is zero"))?;
    Ok(ctx.admit_iter(units, "validate F3D counted relaxed GUID")?.chunks(width)
        .all(|unit| unit[1] == 0 && (unit[0].is_ascii_alphanumeric() || matches!(unit[0], b'-' | b'_')))
        .then_some(end))
}

/// Match an ASCII literal encoded as a counted UTF-16LE field without copying it.
pub(in crate::design::decode) fn fixed_utf16_ascii_eq(
    ctx: &DecodeContext<'_>, bytes: &[u8], count_at: usize, expected: &str,
) -> Result<Option<usize>, CodecError> {
    let Some((units, end)) = (|| {
        if !expected.is_ascii()
            || usize::try_from(View::u32_le_at(bytes, count_at)?).ok()? != expected.len() {
            return None;
        }
        let start = count_at.checked_add(4)?;
        let end = start.checked_add(expected.len().checked_mul(2)?)?;
        Some((bytes.get(start..end)?, end))
    })() else { return Ok(None); };
    let width = std::num::NonZeroUsize::new(2)
        .ok_or_else(|| CodecError::malformed("F3D ASCII code unit width is zero"))?;
    Ok(ctx.admit_iter(units, "match F3D UTF-16 ASCII field")?.chunks(width)
        .zip(ctx.admit_iter(expected.as_bytes(), "scan F3D UTF-16 ASCII literal bytes")?)
        .all(|(unit, byte)| unit == [*byte, 0]).then_some(end))
}

#[cfg(test)]
mod tests {
    use super::{class_tag_from_view, fixed_guid_ascii, fixed_utf16_ascii_eq, relaxed_guid_end};
    use crate::bytes::lp_ascii_filtered_view;

    #[test]
    fn fixed_relaxed_guid_text_refuses_retained_limit_and_preserves_bytes() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let value = "ABCDEF12-3456-7890-ABCD-EF1234567890";
        let bytes = crate::bytes::lp_utf16_bytes(value).unwrap();
        for retained in [0, 35, 36] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = retained;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::fixed_relaxed_guid_text(&ctx, &bytes, 0);
            if retained < 36 {
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == "retain F3D relaxed GUID" && limit.additional == 36));
            } else {
                let (guid, end) = result.unwrap().unwrap();
                assert_eq!(guid.as_str(), value);
                assert_eq!(end, bytes.len());
                assert!(ctx.charge_retained(1, "after GUID copy").is_err());
            }
        }
    }

    #[test]
    fn fixed_relaxed_guid_text_refuses_work_before_scanning() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let bytes = crate::bytes::lp_utf16_bytes("ABCDEF12-3456-7890-ABCD-EF1234567890").unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(super::fixed_relaxed_guid_text(&ctx, &bytes, 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits));
    }

    #[test]
    fn relaxed_guid_scan_matches_owned_validation_at_each_admitted_length() {
        for value in [
            "00000000-0000-0000-0000-000000000000",
            "00000000-0000-0000-0000-000000000000A",
            "00000000-0000-0000-0000-000000000000AB",
            "00000000-0000-0000-0000-000000000000ABC",
            "00000000-0000-0000-0000-00000000000!",
        ] {
            let mut bytes = u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes()
                .to_vec();
            for unit in value.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            let owned = crate::test_support::with_decode_context(|ctx| {
                crate::bytes::lp_utf16_bounded_charged(
                    ctx,
                    &bytes,
                    0,
                    1..=256,
                    "retain F3D UTF-16 string",
                )
                .unwrap()
            })
            .and_then(|(value, end)| crate::bytes::is_guid_relaxed(&value).then_some(end));
            assert_eq!(relaxed_guid_end(&cadmpeg_test_support::service_decode_context(), &bytes, 0).unwrap(), owned);
        }
    }

    #[test]
    fn fixed_guid_ascii_preserves_decoded_text() {
        for value in [
            "ABCDEF12-3456-7890-ABCD-EF1234567890",
            "00000000-0000-0000-0000-000000000000",
            "00000000-0000-0000-0000-00000000000!",
            "é0000000-0000-0000-0000-000000000000",
        ] {
            let mut bytes = u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes()
                .to_vec();
            for unit in value.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            let prior = crate::test_support::with_decode_context(|ctx| {
                crate::bytes::lp_utf16_bounded_charged(
                    ctx,
                    &bytes,
                    0,
                    36..=36,
                    "retain F3D UTF-16 string",
                )
                .unwrap()
            })
            .filter(|(text, _)| crate::bytes::is_guid_relaxed(text));
            let current = fixed_guid_ascii(&cadmpeg_test_support::service_decode_context(), &bytes, 0).unwrap()
                .map(|(guid, end)| (String::from_utf8(guid.to_vec()).unwrap(), end));
            assert_eq!(current, prior);
        }
    }

    #[test]
    fn fixed_utf16_ascii_match_agrees_with_decoded_text() {
        for (value, expected) in [
            ("Thicken", "Thicken"),
            ("Thicken", "Shell"),
            ("Shell", "Shell"),
            ("Thickén", "Thicken"),
        ] {
            let mut bytes = u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes()
                .to_vec();
            for unit in value.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            let decoded = crate::test_support::with_decode_context(|ctx| {
                crate::bytes::lp_utf16_bounded_charged(
                    ctx,
                    &bytes,
                    0,
                    expected.len()..=expected.len(),
                    "retain F3D UTF-16 string",
                )
                .unwrap()
            })
            .and_then(|(text, end)| (text == expected).then_some(end));
            assert_eq!(fixed_utf16_ascii_eq(&cadmpeg_test_support::service_decode_context(), &bytes, 0, expected).unwrap(), decoded);
            bytes.pop();
            assert_eq!(fixed_utf16_ascii_eq(&cadmpeg_test_support::service_decode_context(), &bytes, 0, expected).unwrap(), None);
        }
    }

    #[test]
    fn borrowed_ascii_reader_matches_owned_reader() {
        let fields: [&[u8]; 6] = [b"", b"123", b"EntityGenesis", b"a-b_", b"\0", b"\x80"];
        for field in fields {
            let mut bytes = u32::try_from(field.len()).unwrap().to_le_bytes().to_vec();
            bytes.extend_from_slice(field);
            for bounds in [0..=2000, 3..=3] {
                let predicates: [fn(&u8) -> bool; 2] = [u8::is_ascii_graphic, u8::is_ascii_digit];
                for allowed in predicates {
                    let original = crate::test_support::with_decode_context(|ctx| {
                        crate::bytes::lp_ascii_strict_charged(ctx, &bytes, 0, bounds.clone())
                            .unwrap()
                    })
                    .filter(|(text, _)| text.as_bytes().iter().all(allowed));
                    let borrowed = lp_ascii_filtered_view(&bytes, 0, bounds.clone(), allowed)
                        .map(|(value, end)| (value.to_owned(), end));
                    assert_eq!(borrowed, original);
                    bytes.pop();
                    assert_eq!(
                        lp_ascii_filtered_view(&bytes, 0, bounds.clone(), allowed),
                        None
                    );
                    bytes.push(*field.last().unwrap_or(&0));
                }
            }
        }
    }

    #[test]
    fn borrowed_class_tag_conversion_matches_owned_conversion() {
        for value in ["123", "000", "12", "1234", "12a", "éé"] {
            assert_eq!(
                class_tag_from_view(&cadmpeg_test_support::service_decode_context(), value).unwrap(),
                crate::records::references::DesignClassTag::try_from(value.to_owned())
            );
        }
    }
    #[test]
    fn borrowed_class_tag_refuses_work_before_digit_validation() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(class_tag_from_view(&ctx, "123"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "validate F3D class tag digits"
                    && limit.additional == 3));
    }

    #[test]
    fn fixed_guid_ascii_refuses_work_before_code_unit_validation() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let bytes = crate::bytes::lp_utf16_bytes("ABCDEF12-3456-7890-ABCD-EF1234567890").unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(fixed_guid_ascii(&ctx, &bytes, 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "validate F3D relaxed GUID code units"
                    && limit.additional == 72));
    }

    #[test]
    fn relaxed_guid_end_refuses_work_before_code_unit_validation() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let bytes = crate::bytes::lp_utf16_bytes("ABCDEF12-3456-7890-ABCD-EF1234567890").unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(relaxed_guid_end(&ctx, &bytes, 0),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "validate F3D counted relaxed GUID"
                    && limit.additional == 72));
    }

    #[test]
    fn fixed_utf16_ascii_eq_refuses_work_before_code_unit_comparison() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let bytes = crate::bytes::lp_utf16_bytes("Thicken").unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(fixed_utf16_ascii_eq(&ctx, &bytes, 0, "Thicken"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "match F3D UTF-16 ASCII field"
                    && limit.additional == 14));
    }

    #[test]
    fn fixed_utf16_ascii_literal_iterator_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = crate::bytes::lp_utf16_bytes("Thicken").unwrap();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "scan F3D UTF-16 ASCII literal bytes", 0,
            |ctx| fixed_utf16_ascii_eq(ctx, &bytes, 0, "Thicken"),
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "scan F3D UTF-16 ASCII literal bytes"
                && limit.additional == 7));
    }

}
