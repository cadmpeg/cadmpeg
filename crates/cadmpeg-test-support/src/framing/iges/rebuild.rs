// SPDX-License-Identifier: Apache-2.0
//! Equal-length edits of fully framed physical ASCII leaf content.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};

use super::frame;

mod fields;

pub use fields::{rebuild_fields, FieldEdit};

/// One replacement in the source view's absolute byte coordinates.
pub struct Edit<'a> {
    /// Exact source interval; it must be wholly inside one supported leaf.
    pub range: Range<u64>,
    /// Replacement of exactly the interval's byte length.
    pub replacement: &'a [u8],
}

/// Rebuild only equal-length Start/Directory-field/unmarked/tail content edits.
///
/// Every other byte, including line endings, sequence fields, section counts
/// and Parameter Data locators, is copied unchanged. Parameter/Global payload
/// edits need their finer framers and are refused here. IGES has no checksum.
///
/// # Errors
/// Returns overlapping/unequal/out-of-source edits, edits of known framing or
/// unsupported leaves, invalid source provenance, or allocation/budget failure.
pub fn rebuild(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    edits: &[Edit<'_>],
) -> Result<Vec<u8>, String> {
    let mut parts = Vec::new();
    frame(ctx, source, &mut parts)?;
    let mut order = Vec::new();
    for edit in edits {
        super::append(ctx, &mut order, edit)?;
    }
    order.sort_by_key(|edit| edit.range.start);
    let mut cursor = u64_from_index(source.start());
    let end = u64_from_index(source.end());
    for edit in &order {
        let length = edit
            .range
            .end
            .checked_sub(edit.range.start)
            .ok_or("IGES edit has a reversed interval")?;
        if edit.range.start < cursor
            || edit.range.end > end
            || length != u64_from_index(edit.replacement.len())
        {
            return Err("IGES edit overlaps, lies outside source or changes length".into());
        }
        if length == 0 {
            cursor = edit.range.end;
            continue;
        }
        if parts.iter().any(|part| {
            part.framing()
                .iter()
                .any(|range| edit.range.start < range.end && range.start < edit.range.end)
        }) {
            return Err("IGES edit intersects physical framing".into());
        }
        if !parts.iter().any(|part| {
            matches!(
                part.level().as_str(),
                "iges.start_card"
                    | "iges.directory_field"
                    | "iges.unmarked_line"
                    | "iges.post_terminate_line"
            ) && u64_from_index(part.view().start()) <= edit.range.start
                && edit.range.end <= u64_from_index(part.view().end())
        }) {
            return Err("IGES edit needs a finer source framer".into());
        }
        if edit
            .replacement
            .iter()
            .any(|byte| matches!(byte, b'\r' | b'\n'))
        {
            return Err("IGES content edit introduces a physical line ending".into());
        }
        cursor = edit.range.end;
    }
    apply_edits(ctx, source, edits)
}

pub(super) fn apply_edits(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    edits: &[Edit<'_>],
) -> Result<Vec<u8>, String> {
    ctx.retain_bytes(u64_from_index(source.window().len()), "IGES rebuilt source")
        .map_err(|error| error.to_string())?;
    let mut result = ctx
        .alloc_filled(source.window().len(), 0_u8, "IGES rebuilt source")
        .map_err(|error| error.to_string())?;
    result.copy_from_slice(source.window());
    for edit in edits {
        let start = usize::try_from(edit.range.start - u64_from_index(source.start()))
            .map_err(|_| "IGES edit index exceeds usize")?;
        let end = usize::try_from(edit.range.end - u64_from_index(source.start()))
            .map_err(|_| "IGES edit index exceeds usize")?;
        result[start..end].copy_from_slice(edit.replacement);
    }
    if super::cards::scan(ctx, source.window())? != super::cards::scan(ctx, &result)? {
        return Err("IGES edit changes physical card boundaries or dispatch".into());
    }
    Ok(result)
}
