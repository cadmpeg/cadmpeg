// SPDX-License-Identifier: Apache-2.0
//! Parameter source fields join one actual member-local owner capture.

use std::collections::BTreeMap;

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::container::ContainerScan;
use crate::records::parameters::DesignParameter;

pub(crate) fn capture(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    parameters: &mut [DesignParameter],
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let source = scan.entry_view(&entry.name).ok_or_else(|| {
            CodecError::malformed("parameter layout has no original BulkStream View")
        })?;
        for parameter in parameters.iter_mut() {
            let Some(offset) = parameter.byte_offset() else {
                continue;
            };
            if parameter.id != crate::ids::native_design_parameter_id(&entry.name, offset) {
                continue;
            }
            let start = usize::try_from(offset)
                .map_err(|_| CodecError::malformed("parameter layout offset exceeds usize"))?;
            let next = start
                .checked_add(11)
                .ok_or_else(|| CodecError::malformed("parameter layout header overflows"))?;
            let end = super::next_indexed_record_offset(source.window(), next)
                .unwrap_or(source.window().len());
            let begin = source
                .start()
                .checked_add(start)
                .ok_or_else(|| CodecError::malformed("parameter layout start overflows"))?;
            let end = source
                .start()
                .checked_add(end)
                .ok_or_else(|| CodecError::malformed("parameter layout end overflows"))?;
            let view = source
                .child(begin, end)
                .ok_or_else(|| CodecError::malformed("parameter layout exceeds its source"))?;
            capture_record(ctx, view, &entry.name, offset, parameter, pointers)?;
        }
    }
    Ok(())
}

pub(crate) fn capture_record(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    stream: &str,
    offset: u64,
    parameter: &mut DesignParameter,
    pointers: &mut BTreeMap<String, BTreeMap<String, u64>>,
) -> Result<(), CodecError> {
    ctx.capture_part_extent(view)?;
    let parse = || {
        let parsed = super::parse_design_parameter(view.window())?
            .ok_or_else(|| CodecError::malformed("parameter source no longer admits its fields"))?;
        super::locate_design_parameter(
            parsed,
            stream,
            usize::try_from(offset)
                .map_err(|_| CodecError::malformed("parameter layout offset exceeds usize"))?,
        )
    };
    let mut actual = if pointers.contains_key(&parameter.id) {
        parse()?
    } else {
        ctx.record(&parameter.id, view, parse)?
    };
    if actual.id != parameter.id || actual.record_index != parameter.record_index {
        return Err(CodecError::malformed(
            "parameter layout has a different actual source owner",
        ));
    }
    let fields = actual.source_offsets();
    if let Some(existing) = pointers.get(&actual.id) {
        for (name, offset) in fields
            .iter()
            .filter_map(|(name, offset)| offset.map(|offset| (*name, offset)))
        {
            if existing
                .get(name)
                .is_some_and(|existing| *existing != offset)
            {
                return Err(CodecError::malformed(
                    "parameter layout pointer differs from its actual source field",
                ));
            }
        }
    } else {
        ctx.retain_bytes(u64_from_index(actual.id.len()), "parameter layout owner")?;
    }
    let target = pointers.entry(actual.id.clone()).or_default();
    for (name, offset) in fields
        .into_iter()
        .filter_map(|(name, offset)| offset.map(|offset| (name, offset)))
    {
        if !target.contains_key(name) {
            let bytes = name
                .len()
                .checked_add(size_of::<(String, u64)>())
                .ok_or_else(|| {
                    CodecError::malformed("parameter layout pointer storage overflows")
                })?;
            ctx.retain_bytes(u64_from_index(bytes), "parameter layout pointer")?;
            target.insert(name.to_owned(), offset);
        }
    }
    if let Some(state) = parameter.take_parameter_state() {
        actual.set_parameter_state(state);
    }
    *parameter = actual;
    Ok(())
}

#[cfg(test)]
mod tests;
