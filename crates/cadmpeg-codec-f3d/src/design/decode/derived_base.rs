// SPDX-License-Identifier: Apache-2.0
//! The owner record that a `Derived Base Feature` frame names.

use crate::container::ContainerScan;
use crate::layout::derived_base_owner_reference as owner;
use crate::records::feature::derived_base::{
    is_derived_base, DesignDerivedBaseOwner, REPEAT_FROM_END,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::CodecError;

/// Record the owner of each `Derived Base Feature` scope whose frame names it.
pub(crate) fn mark_derived_base_owners(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    for stream_entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream = crate::ids::native_scope(&stream_entry.name);
        let mut bytes = None;
        for scope in scopes.iter_mut().filter(|scope| {
            crate::ids::native_stream(&scope.id) == Some(stream.as_str()) && is_derived_base(scope)
        }) {
            let bytes = match bytes {
                Some(bytes) => bytes,
                None => *bytes.insert(scan.entry_bytes(&stream_entry.name)?),
            };
            let (Ok(start), Ok(end)) = (
                usize::try_from(scope.byte_offset()),
                usize::try_from(scope.paired_byte_offset()),
            ) else {
                continue;
            };
            scope.derived_base_owner = bytes
                .get(start..end)
                .and_then(|frame| derived_base_owner(frame, start));
        }
    }
    Ok(())
}

/// The owner that `frame`, which starts at stream offset `start`, names at
/// its owner-reference offset and again twelve bytes before its end, with a
/// zero last byte. `None` when either reference is malformed or they differ.
pub(crate) fn derived_base_owner(frame: &[u8], start: usize) -> Option<DesignDerivedBaseOwner> {
    let reference = frame.get(owner::MARKER..owner::LEN)?;
    let repeat_at = frame.len().checked_sub(REPEAT_FROM_END)?;
    if repeat_at < owner::LEN
        || frame.get(repeat_at..repeat_at.checked_add(owner::LEN - owner::MARKER)?)? != reference
        || frame.last() != Some(&0)
        || reference.first() != Some(&owner::MARKER_VALUE)
        || frame.get(owner::ZERO_RUN_2..owner::LEN) != Some(&[0; 2])
    {
        return None;
    }
    let record_index = u32::try_from(View::u64_le_at(frame, owner::RECORD_INDEX)?).ok()?;
    Some(DesignDerivedBaseOwner {
        record_index: Located {
            value: record_index,
            offset: u64_from_index(start.checked_add(owner::RECORD_INDEX)?),
        },
        repeat_offset: u64_from_index(start.checked_add(repeat_at)?),
    })
}

#[cfg(test)]
mod tests;
