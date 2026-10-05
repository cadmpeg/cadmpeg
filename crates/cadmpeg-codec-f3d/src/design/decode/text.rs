// SPDX-License-Identifier: Apache-2.0
//! Charged text reads shared by Design record decoders.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Copy a borrowed three-digit class tag into retained storage. Text that is
/// not three ASCII digits is no class tag; the test reads at most three bytes.
pub(in crate::design::decode) fn class_tag_from_view(
    ctx: &DecodeContext<'_>,
    value: &str,
) -> Result<Option<crate::records::references::DesignClassTag>, CodecError> {
    let Some(digits) = value.as_bytes().first_chunk::<3>() else {
        return Ok(None);
    };
    if value.len() != 3 || !digits.iter().all(u8::is_ascii_digit) {
        return Ok(None);
    }
    Ok(crate::records::references::DesignClassTag::try_from(
        ctx.copy_retained_text(value, "copy F3D class tag")?,
    )
    .ok())
}

/// Split `text` at its last ASCII `separator` without allocating either half.
/// The byte search admits the text and the one-byte pattern.
pub(in crate::design::decode) fn rsplit_once_ascii<'text>(
    ctx: &DecodeContext<'_>,
    text: &'text str,
    separator: u8,
    operation: &'static str,
) -> Result<Option<(&'text str, &'text str)>, CodecError> {
    debug_assert!(separator.is_ascii());
    let Some(at) = ctx.rfind_bytes(text.as_bytes(), &[separator], operation)? else {
        return Ok(None);
    };
    // An ASCII byte is a character boundary on both sides.
    Ok(text.get(..at).zip(text.get(at + 1..)))
}

/// Copy a three-digit class tag that an indexed-header read already
/// validated into retained storage.
pub(in crate::design::decode) fn retain_class_tag(
    ctx: &DecodeContext<'_>,
    value: [u8; 3],
    operation: &'static str,
) -> Result<crate::records::references::DesignClassTag, CodecError> {
    let text = std::str::from_utf8(&value)
        .map_err(|_| CodecError::malformed("F3D class tag must be three ASCII digits"))?;
    crate::records::references::DesignClassTag::try_from(ctx.copy_retained_text(text, operation)?)
        .map_err(CodecError::Malformed)
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

/// Whether a UTF-16LE code unit holds a relaxed GUID character.
fn relaxed_guid_unit(unit: &[u8]) -> bool {
    unit[1] == 0 && (unit[0].is_ascii_alphanumeric() || matches!(unit[0], b'-' | b'_'))
}

/// Validate an exact 36-code-unit relaxed GUID into a fixed ASCII array. The
/// test reads at most the 76-byte counted field.
pub(in crate::design::decode) fn fixed_guid_ascii(
    bytes: &[u8],
    count_at: usize,
) -> Option<([u8; 36], usize)> {
    (View::u32_le_at(bytes, count_at)? == 36).then_some(())?;
    let start = count_at.checked_add(4)?;
    let units = super::byte_fields::bytes_at::<72>(bytes, start)?;
    let mut guid = [0; 36];
    for (unit, slot) in units.chunks_exact(2).zip(guid.iter_mut()) {
        if !relaxed_guid_unit(unit) {
            return None;
        }
        *slot = unit[0];
    }
    Some((guid, start + 72))
}

/// Read a fixed-width relaxed GUID into its native value after code-unit validation.
pub(in crate::design::decode) fn fixed_relaxed_guid_text(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    count_at: usize,
) -> Result<Option<(crate::records::mesh::DesignRelaxedGuidText, usize)>, CodecError> {
    let Some((guid, end)) = fixed_guid_ascii(bytes, count_at) else {
        return Ok(None);
    };
    let guid = std::str::from_utf8(&guid)
        .map_err(|_| CodecError::malformed("validated F3D relaxed GUID is not ASCII"))?;
    let text = ctx.copy_retained_text(guid, "retain F3D relaxed GUID")?;
    let value = crate::records::mesh::DesignRelaxedGuidText::try_from(text)
        .map_err(CodecError::malformed)?;
    Ok(Some((value, end)))
}

/// Validate an exact 36-code-unit relaxed GUID in UTF-16LE without copying it.
pub(in crate::design::decode) fn fixed_guid_end(bytes: &[u8], count_at: usize) -> Option<usize> {
    fixed_guid_ascii(bytes, count_at).map(|(_, end)| end)
}

/// Validate a counted relaxed GUID of 36 to 38 code units without allocating
/// its text. The test reads at most the 80-byte counted field.
pub(in crate::design::decode) fn relaxed_guid_end(bytes: &[u8], count_at: usize) -> Option<usize> {
    let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
    if !(36..=38).contains(&count) {
        return None;
    }
    let start = count_at.checked_add(4)?;
    let end = start.checked_add(count * 2)?;
    bytes
        .get(start..end)?
        .chunks_exact(2)
        .all(relaxed_guid_unit)
        .then_some(end)
}

/// Match an ASCII literal encoded as a counted UTF-16LE field without copying
/// it. The test reads at most the literal's code units.
pub(in crate::design::decode) fn fixed_utf16_ascii_eq(
    bytes: &[u8],
    count_at: usize,
    expected: &'static str,
) -> Option<usize> {
    if !expected.is_ascii()
        || usize::try_from(View::u32_le_at(bytes, count_at)?).ok()? != expected.len()
    {
        return None;
    }
    let start = count_at.checked_add(4)?;
    let end = start.checked_add(expected.len().checked_mul(2)?)?;
    bytes
        .get(start..end)?
        .chunks_exact(2)
        .zip(expected.as_bytes())
        .all(|(unit, byte)| unit == [*byte, 0])
        .then_some(end)
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
            assert_eq!(relaxed_guid_end(&bytes, 0), owned);
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
            let current = fixed_guid_ascii(&bytes, 0)
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
            assert_eq!(fixed_utf16_ascii_eq(&bytes, 0, expected), decoded);
            bytes.pop();
            assert_eq!(fixed_utf16_ascii_eq(&bytes, 0, expected), None);
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
                    let borrowed = crate::test_support::with_decode_context(|ctx| {
                        lp_ascii_filtered_view(ctx, &bytes, 0, bounds.clone(), allowed).unwrap()
                    })
                    .map(|(value, end)| (value.to_owned(), end));
                    assert_eq!(borrowed, original);
                    bytes.pop();
                    assert_eq!(
                        crate::test_support::with_decode_context(|ctx| {
                            lp_ascii_filtered_view(ctx, &bytes, 0, bounds.clone(), allowed).unwrap()
                        }),
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
                class_tag_from_view(&cadmpeg_test_support::service_decode_context(), value)
                    .unwrap(),
                crate::records::references::DesignClassTag::try_from(value.to_owned()).ok()
            );
        }
    }

    #[test]
    fn borrowed_class_tag_refuses_retained_text_before_copy() {
        use cadmpeg_core::decode::ResourceDimension;

        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::RetainedBytes,
            "copy F3D class tag",
            0,
            |ctx| class_tag_from_view(ctx, "123").map(|_| ()),
        );
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "copy F3D class tag"
                    && limit.additional == 3
        ));
    }
}
