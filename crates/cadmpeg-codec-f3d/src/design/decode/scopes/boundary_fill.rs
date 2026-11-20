// SPDX-License-Identifier: Apache-2.0

use crate::layout::{
    boundary_fill_scope_prefix_plain as plain, boundary_fill_scope_prefix_property as property,
};
use crate::records::feature::boundary_fill::{
    DesignBoundaryFill, DesignBoundaryFillOperands, DesignBoundaryFillRemoval,
};
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{BoundedCount, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::HashMap;
use std::num::NonZeroU32;

const CELL_TYPE_GUID: &str = "233D4E1B-1C2F-4622-96A0-0027622078CB";
const SETTINGS_TYPE_GUID: &str = "5AC5B5CD-44C7-45B7-9CDC-F79B215A825D";

pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    stream: View<'_>,
    scope: &DesignParameterScope,
    types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<DesignBoundaryFill>, CodecError> {
    let bounds = usize::try_from(scope.byte_offset())
        .ok()
        .zip(usize::try_from(scope.reference_count_offset()).ok());
    let Some(prefix) = bounds.and_then(|(start, end)| stream.child(start, end)) else {
        return Ok(None);
    };
    decode_prefix(ctx, prefix, types)
}

fn decode_prefix(
    ctx: &DecodeContext<'_>,
    prefix: View<'_>,
    types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<DesignBoundaryFill>, CodecError> {
    let Some((operation_word, lists)) = operation_prefix(prefix) else {
        return Ok(None);
    };
    let standard = read_operands(lists, false, types);
    let removal = read_operands(lists, true, types);
    let operands = match (standard, removal) {
        (Some(operands), None) | (None, Some(operands)) => Some(operands.retain(ctx)?),
        _ => None,
    };
    Ok(Some(DesignBoundaryFill {
        operation_word,
        operands,
    }))
}

fn operation_prefix(mut prefix: View<'_>) -> Option<(u32, View<'_>)> {
    prefix.take(plain::ZERO_RUN_9)?;
    if prefix.take(plain::PROPERTY_FLAG - plain::ZERO_RUN_9)? != [0; 9] {
        return None;
    }
    let operation_at = match prefix.u8()? {
        0 => plain::OPERATION,
        1 if prefix.u32_le()? == property::OPTIONAL_PROPERTY_WORD_VALUE => property::OPERATION,
        _ => return None,
    };
    prefix.seek(prefix.start().checked_add(operation_at)?)?;
    Some((prefix.u32_le()?, prefix))
}

type LocalReference = Option<NonZeroU32>;

struct ReferenceList<'a> {
    count: BoundedCount,
    references: View<'a>,
}

impl<'a> ReferenceList<'a> {
    fn read(view: &mut View<'a>, admits: impl Fn(LocalReference) -> bool) -> Option<Self> {
        let count = u64::from(view.u32_le()?);
        let count = view.counted(count, 1)?;
        let start = view.position();
        for _ in 0..count.get() {
            if !admits(read_reference(view)?) {
                return None;
            }
        }
        Some(Self {
            count,
            references: view.child(start, view.position())?,
        })
    }

    fn retain(self, ctx: &DecodeContext<'_>) -> Result<Vec<LocalReference>, CodecError> {
        let mut references = self.references;
        let mut out = ctx.alloc_filled(self.count.get(), None, "BoundaryFill references")?;
        for entry in &mut out {
            *entry = read_reference(&mut references).ok_or_else(|| {
                CodecError::Malformed("invalid validated BoundaryFill reference".into())
            })?;
        }
        Ok(out)
    }
}

struct Operands<'a> {
    body_groups: ReferenceList<'a>,
    plane_tools: ReferenceList<'a>,
    settings: NonZeroU32,
    cells: ReferenceList<'a>,
    removal: Option<([ReferenceList<'a>; 2], bool)>,
    suffix: View<'a>,
}

impl Operands<'_> {
    fn retain(self, ctx: &DecodeContext<'_>) -> Result<DesignBoundaryFillOperands, CodecError> {
        let removal = match self.removal {
            Some(([first, second], remove_tools)) => Some(DesignBoundaryFillRemoval {
                references: [first.retain(ctx)?, second.retain(ctx)?],
                remove_tools,
            }),
            None => None,
        };
        let mut suffix = ctx.alloc_filled(self.suffix.remaining(), 0, "BoundaryFill suffix")?;
        suffix.copy_from_slice(self.suffix.unread());
        Ok(DesignBoundaryFillOperands {
            body_groups: self.body_groups.retain(ctx)?,
            plane_tools: self.plane_tools.retain(ctx)?,
            settings: self.settings,
            cells: self.cells.retain(ctx)?,
            removal,
            suffix: cadmpeg_ir::native::bytes::NativeBytes::new(suffix),
        })
    }
}

fn read_operands<'a>(
    mut view: View<'a>,
    has_removal: bool,
    types: &HashMap<u64, (&str, u32)>,
) -> Option<Operands<'a>> {
    let has_type = |index: NonZeroU32, guid: &str| {
        types
            .get(&u64::from(index.get()))
            .is_some_and(|(registered, version)| {
                *version == 0 && registered.eq_ignore_ascii_case(guid)
            })
    };
    let removal = if has_removal {
        let first = ReferenceList::read(&mut view, |_| true)?;
        let second = ReferenceList::read(&mut view, |_| true)?;
        let remove_tools = match view.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        Some(([first, second], remove_tools))
    } else {
        None
    };
    let body_groups = ReferenceList::read(&mut view, |_| true)?;
    let plane_tools = ReferenceList::read(&mut view, |_| true)?;
    let settings = read_reference(&mut view)??;
    if !has_type(settings, SETTINGS_TYPE_GUID) {
        return None;
    }
    let cells = ReferenceList::read(&mut view, |index| {
        index.is_none_or(|index| has_type(index, CELL_TYPE_GUID))
    })?;
    Some(Operands {
        body_groups,
        plane_tools,
        settings,
        cells,
        removal,
        suffix: view,
    })
}

fn read_reference(view: &mut View<'_>) -> Option<LocalReference> {
    match view.u8()? {
        0 => Some(None),
        1 => {
            let target = u32::try_from(view.u64_le()?)
                .ok()
                .and_then(NonZeroU32::new)?;
            (view.take(2)? == [0; 2]).then_some(Some(target))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
