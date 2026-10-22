// SPDX-License-Identifier: Apache-2.0
//! Equal-length stored-content edits with source-preserving CRC updates.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};

use super::layout::u32_at;
use super::{append, frame};

/// One equal-length replacement in the source's absolute coordinates.
pub struct Edit<'a> {
    /// Interval wholly inside one directly stored unencrypted member payload.
    pub range: Range<u64>,
    /// Replacement bytes of exactly the interval length.
    pub replacement: &'a [u8],
}

/// Copy a ZIP source and reseal only its edited stored members.
///
/// Lengths, offsets and all metadata stay byte-identical. The governing central
/// CRC and a present descriptor CRC are recomputed. A deferred zero local CRC
/// stays zero; an actual local CRC is recomputed. Opaque directory signatures
/// are preserved only while their covered central headers stay byte-identical.
/// This function neither verifies nor renews a signature. Compression/encryption and
/// length-changing edits need their owning rebuilders and are refused here.
///
/// # Errors
/// Returns unsupported/overlapping/out-of-member edits, invalid physical ZIP
/// framing/provenance or a caller-budget/allocation refusal.
pub fn rebuild(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    edits: &[Edit<'_>],
) -> Result<Vec<u8>, String> {
    let mut parts = Vec::new();
    let entries = frame(ctx, source, &mut parts)?;
    let mut order = Vec::new();
    for edit in edits {
        append(ctx, &mut order, edit)?;
    }
    order.sort_by_key(|edit| edit.range.start);
    let mut previous = u64_from_index(source.start());
    let mut touched = ctx
        .alloc_filled(entries.len(), false, "ZIP edited member flags")
        .map_err(|error| error.to_string())?;
    ctx.retain_bytes(u64_from_index(entries.len()), "ZIP edited member flags")
        .map_err(|error| error.to_string())?;
    for edit in &order {
        let length = edit
            .range
            .end
            .checked_sub(edit.range.start)
            .ok_or("ZIP edit is reversed")?;
        if edit.range.start < previous
            || edit.range.end > u64_from_index(source.end())
            || length != u64_from_index(edit.replacement.len())
        {
            return Err("ZIP edit overlaps, exceeds source or changes length".into());
        }
        previous = edit.range.end;
        if length == 0 {
            continue;
        }
        let (index, entry) = entries
            .iter()
            .enumerate()
            .find(|(_, entry)| {
                u64_from_index(entry.payload.start()) <= edit.range.start
                    && edit.range.end <= u64_from_index(entry.payload.end())
            })
            .ok_or("ZIP edit is not inside an actual member payload")?;
        if entry.method != 0
            || entry.flags & 1 != 0
            || entry.local_flags & 1 != 0
            || u64_from_index(entry.payload.window().len()) != entry.uncompressed
        {
            return Err(
                "ZIP edit requires an unencrypted directly stored member with matching sizes"
                    .into(),
            );
        }
        touched[index] = true;
    }
    ctx.retain_bytes(u64_from_index(source.window().len()), "ZIP rebuilt source")
        .map_err(|error| error.to_string())?;
    let mut result = ctx
        .alloc_filled(source.window().len(), 0_u8, "ZIP rebuilt source")
        .map_err(|error| error.to_string())?;
    result.copy_from_slice(source.window());
    let relative = |absolute: u64| {
        usize::try_from(absolute - u64_from_index(source.start()))
            .map_err(|_| "ZIP rebuilt source index exceeds usize")
    };
    for edit in order {
        result[relative(edit.range.start)?..relative(edit.range.end)?]
            .copy_from_slice(edit.replacement);
    }
    for (index, entry) in entries.iter().enumerate() {
        if !touched[index] {
            continue;
        }
        let start = entry.payload.start() - source.start();
        let end = entry.payload.end() - source.start();
        let crc = crate::zip_fixture::crc32(&result[start..end]);
        let put = |bytes: &mut [u8], offset: usize| -> Result<(), String> {
            let field = bytes
                .get_mut(offset..super::layout::add(offset, 4)?)
                .ok_or("ZIP CRC field exceeds source")?;
            field.copy_from_slice(&crc.to_le_bytes());
            Ok(())
        };
        put(&mut result, entry.central_crc)?;
        if entry.local_flags & 8 == 0 || u32_at(source.window(), entry.local_crc)? != 0 {
            put(&mut result, entry.local_crc)?;
        }
        if let Some(descriptor) = entry.descriptor_crc {
            put(&mut result, descriptor)?;
        }
    }
    if let Some(signature) = parts
        .iter()
        .find(|part| part.level().as_str() == "zip.digital_signature")
    {
        let directory = parts
            .iter()
            .find(|part| part.level().as_str() == "zip.central_directory")
            .ok_or("signed ZIP has no directory frame")?;
        let start = directory.view().start() - source.start();
        let end = signature.view().start() - source.start();
        if result.get(start..end) != source.window().get(start..end) {
            return Err(
                "ZIP edit changes signed central-directory bytes; signature renewal is unsupported"
                    .into(),
            );
        }
    }
    Ok(result)
}
