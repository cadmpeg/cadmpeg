// SPDX-License-Identifier: Apache-2.0
//! Transient genuine archive-entry sources for decoder interpretation.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

use crate::native::EntryRecord;

pub(crate) struct EntrySources<'source> {
    entries: &'source BTreeMap<String, View<'source>>,
}

impl<'source> EntrySources<'source> {
    pub(crate) fn new(entries: &'source BTreeMap<String, View<'source>>) -> Self {
        Self { entries }
    }

    pub(crate) fn view(&self, entry: &EntryRecord) -> Result<View<'source>, CodecError> {
        self.entries.get(entry.name()).copied().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "archive entry {} has no genuine source View",
                entry.name()
            ))
        })
    }
}

#[cfg(test)]
mod tests;
