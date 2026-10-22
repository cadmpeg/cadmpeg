// SPDX-License-Identifier: Apache-2.0
//! Transient genuine archive-entry sources for decoder interpretation.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

use crate::native::EntryRecord;

pub(crate) struct EntrySources<'source> {
    entries: &'source BTreeMap<String, View<'source>>,
}

impl<'source> EntrySources<'source> {
    pub(crate) fn new(
        context: &DecodeContext<'_>,
        entries: &'source BTreeMap<String, View<'source>>,
    ) -> Result<Self, CodecError> {
        for source in entries.values() {
            context.capture_part_extent(*source)?;
        }
        Ok(Self { entries })
    }

    pub(crate) fn view(&self, entry: &EntryRecord) -> Result<View<'source>, CodecError> {
        self.entries.get(entry.name()).copied().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "archive entry {} has no genuine source View",
                entry.name()
            ))
        })
    }

    pub(crate) fn byte_len(&self, entry: &EntryRecord) -> Result<u64, CodecError> {
        self.view(entry)
            .map(|source| cadmpeg_core::decode::u64_from_index(source.window().len()))
    }
}

#[cfg(test)]
mod tests;
