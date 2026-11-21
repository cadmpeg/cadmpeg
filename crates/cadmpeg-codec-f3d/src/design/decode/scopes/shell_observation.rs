use super::shared_frames::{exact_fixed_scalar, marked_record_reference};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_guid_end;
use crate::layout::shell_compact_slot_scope_prefix as compact;
use crate::layout::shell_marked_slot_scope_prefix as marked;
use crate::layout::shell_wall_slot_scope_prefix as cornered;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::Serialize;

const REFERENCE_LEN: usize = 11;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::design) enum Form {
    Cornered,
    Compact,
    Marked,
    Counted,
}

#[derive(Serialize)]
pub(in crate::design) struct Slot {
    pub(in crate::design) record: Option<u32>,
    pub(in crate::design) offset: u64,
}

#[derive(Serialize)]
pub(in crate::design) struct ReferenceRun {
    count: Located<u32>,
    pub(in crate::design) entries: Vec<Option<Located<u32>>>,
}

#[derive(Serialize)]
pub(in crate::design) struct Frame {
    pub(in crate::design) form: Form,
    pub(in crate::design) control: Option<Located<u8>>,
    pub(in crate::design) flag: Located<u8>,
    pub(in crate::design) rounded: Option<Located<u8>>,
    pub(in crate::design) slots: [Option<Slot>; 2],
    pub(in crate::design) auxiliary: Option<ReferenceRun>,
    pub(in crate::design) groups: Option<ReferenceRun>,
    pub(in crate::design) aligned: bool,
}

#[derive(Serialize)]
pub(in crate::design) struct Scalar {
    pub(in crate::design) record: u32,
    pub(in crate::design) owner: Option<u32>,
    pub(in crate::design) ordinal: u8,
    pub(in crate::design) value: f64,
    pub(in crate::design) offset: u64,
}

#[derive(Default, Serialize)]
pub(in crate::design) struct Readout {
    prefix: Option<NativeBytes>,
    pub(in crate::design) frames: Vec<Frame>,
    pub(in crate::design) scalars: Vec<Scalar>,
    fragments: Vec<Fragment>,
}

#[derive(Serialize)]
struct Fragment {
    record: u32,
    offset: u64,
    bytes: NativeBytes,
}

fn zeros(bytes: &[u8], cursor: &mut usize, length: usize) -> Option<()> {
    let end = cursor.checked_add(length)?;
    bytes
        .get(*cursor..end)?
        .iter()
        .all(|value| *value == 0)
        .then_some(())?;
    *cursor = end;
    Some(())
}

fn slot(bytes: &[u8], cursor: &mut usize, start: u64) -> Option<Slot> {
    let offset = start.checked_add(u64::try_from(*cursor).ok()?)?;
    let record = match bytes.get(*cursor)? {
        0 => {
            *cursor = cursor.checked_add(1)?;
            None
        }
        1 => {
            let record = marked_record_reference(bytes, *cursor)?;
            *cursor = cursor.checked_add(REFERENCE_LEN)?;
            Some(record)
        }
        _ => return None,
    };
    Some(Slot { record, offset })
}

fn references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    cursor: &mut usize,
    start: u64,
) -> Result<Option<ReferenceRun>, CodecError> {
    let mut view = View::over_retained(bytes.get(*cursor..).unwrap_or_default());
    let Some(count) = view.u32_le() else {
        return Ok(None);
    };
    let Some(length) = view.counted(u64::from(count), REFERENCE_LEN) else {
        return Ok(None);
    };
    let count_offset = start.checked_add(cadmpeg_core::decode::u64_from_index(*cursor));
    let Some(count_offset) = count_offset else {
        return Ok(None);
    };
    let first = cursor.checked_add(4);
    let Some(first) = first else { return Ok(None) };
    let Some(width) = length.get().checked_mul(REFERENCE_LEN) else {
        return Ok(None);
    };
    let Some(end) = first.checked_add(width) else {
        return Ok(None);
    };
    let mut entries = ctx.alloc_filled(length.get(), None, "F3D Shell observed references")?;
    for (ordinal, entry) in entries.iter_mut().enumerate() {
        let at = first + ordinal * REFERENCE_LEN;
        *entry = marked_record_reference(bytes, at).and_then(|value| {
            Some(Located {
                value,
                offset: start.checked_add(cadmpeg_core::decode::u64_from_index(at))?,
            })
        });
    }
    *cursor = end;
    Ok(Some(ReferenceRun {
        count: Located {
            value: count,
            offset: count_offset,
        },
        entries,
    }))
}

fn frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: u64,
    form: Form,
) -> Result<Option<Frame>, CodecError> {
    let (zero_start, zero_end, flag_at, mut cursor) = match form {
        Form::Cornered => (
            cornered::ZERO_RUN_10,
            cornered::TANGENT_CHAIN,
            cornered::TANGENT_CHAIN,
            cornered::LEN,
        ),
        Form::Compact => (
            compact::ZERO_RUN_10,
            compact::FLAG,
            compact::FLAG,
            compact::LEN,
        ),
        Form::Marked => (
            marked::ZERO_RUN_9,
            marked::FEATURE_FORM,
            marked::FLAG,
            marked::MARKED_SLOTS,
        ),
        Form::Counted => (
            marked::ZERO_RUN_9,
            marked::FEATURE_FORM,
            marked::FLAG,
            marked::LEN,
        ),
    };
    let mut zero_at = zero_start;
    if zeros(bytes, &mut zero_at, zero_end - zero_start).is_none() {
        return Ok(None);
    }
    let Some(&flag) = bytes.get(flag_at).filter(|value| **value <= 1) else {
        return Ok(None);
    };
    if matches!(form, Form::Marked | Form::Counted) {
        let mut at = marked::ZERO_RUN_3;
        if bytes.get(marked::FEATURE_FORM) != Some(&marked::FEATURE_FORM_VALUE)
            || bytes.get(marked::SLOT_MARKER) != Some(&marked::SLOT_MARKER_VALUE)
            || zeros(bytes, &mut at, marked::SLOT_MARKER - marked::ZERO_RUN_3).is_none()
        {
            return Ok(None);
        }
    }
    let located = |at: usize, value| {
        Some(Located {
            value,
            offset: start.checked_add(cadmpeg_core::decode::u64_from_index(at))?,
        })
    };
    let Some(flag) = located(flag_at, flag) else {
        return Ok(None);
    };
    let rounded = if form == Form::Cornered {
        let Some(&value) = bytes
            .get(cornered::ROUNDED_CORNERS)
            .filter(|value| **value <= 1)
        else {
            return Ok(None);
        };
        located(cornered::ROUNDED_CORNERS, value)
    } else {
        None
    };
    let control = if form == Form::Counted {
        let Some(&value) = bytes.get(marked::MARKED_SLOTS) else {
            return Ok(None);
        };
        located(marked::MARKED_SLOTS, value)
    } else {
        None
    };
    let inside = slot(bytes, &mut cursor, start);
    let outside = inside
        .as_ref()
        .and_then(|_| slot(bytes, &mut cursor, start));
    let mut result = Frame {
        form,
        control,
        flag,
        rounded,
        slots: [inside, outside],
        auxiliary: None,
        groups: None,
        aligned: false,
    };
    if result.slots.iter().any(Option::is_none) {
        return Ok(Some(result));
    }
    match form {
        Form::Counted => {
            if zeros(bytes, &mut cursor, 4).is_none() {
                return Ok(Some(result));
            }
            result.auxiliary = references(ctx, bytes, &mut cursor, start)?;
            if result.auxiliary.is_none() || zeros(bytes, &mut cursor, 4).is_none() {
                return Ok(Some(result));
            }
        }
        Form::Cornered => {
            if zeros(bytes, &mut cursor, 12).is_none() {
                return Ok(Some(result));
            }
        }
        Form::Compact | Form::Marked => {
            if zeros(bytes, &mut cursor, 8).is_none() {
                return Ok(Some(result));
            }
        }
    }
    result.groups = references(ctx, bytes, &mut cursor, start)?;
    if result.groups.is_none() {
        return Ok(Some(result));
    }
    if let Some(end) = fixed_guid_end(bytes, cursor) {
        cursor = end;
        result.aligned = zeros(bytes, &mut cursor, 3).is_some() && cursor == bytes.len();
    }
    Ok(Some(result))
}

pub(in crate::design) fn read(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Readout, CodecError> {
    let mut result = Readout::default();
    if let (Ok(start), Ok(end)) = (
        usize::try_from(scope.byte_offset()),
        usize::try_from(scope.reference_count_offset()),
    ) {
        if let Some(prefix) = bytes.get(start..end) {
            result.prefix = Some(NativeBytes::new(prefix.to_vec()));
            for form in [Form::Cornered, Form::Compact, Form::Marked, Form::Counted] {
                if let Some(frame) = frame(ctx, prefix, scope.byte_offset(), form)? {
                    result.frames.push(frame);
                }
            }
        }
    }
    let mut references = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    references.extend(
        result
            .frames
            .iter()
            .flat_map(|frame| frame.slots.iter().flatten().filter_map(|slot| slot.record)),
    );
    references.extend(result.frames.iter().flat_map(|frame| {
        frame
            .auxiliary
            .iter()
            .flat_map(|run| run.entries.iter().flatten().map(|entry| entry.value))
    }));
    references.sort_unstable();
    references.dedup();
    let mut headers = records
        .records()
        .flat_map(|(_, offsets)| offsets.iter().copied())
        .collect::<Vec<_>>();
    headers.sort_unstable();
    headers.dedup();
    for record in references {
        if let Some(scalar) = exact_fixed_scalar(bytes, records, record) {
            result.scalars.push(Scalar {
                record,
                owner: scalar.owner_record_index,
                ordinal: scalar.ordinal,
                value: scalar.value.get(),
                offset: scalar.value_offset,
            });
        }
        for &offset in records.offsets(record) {
            let end = headers
                .get(headers.partition_point(|value| *value <= offset))
                .copied()
                .unwrap_or(bytes.len());
            if let Some(fragment) = bytes.get(offset..end) {
                result.fragments.push(Fragment {
                    record,
                    offset: cadmpeg_core::decode::u64_from_index(offset),
                    bytes: NativeBytes::new(fragment.to_vec()),
                });
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
