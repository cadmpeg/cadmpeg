// SPDX-License-Identifier: Apache-2.0
//! Transient genuine archive-entry sources for decoder interpretation.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

use crate::native::EntryRecord;

pub(crate) struct EntrySources<'source> {
    entries: &'source BTreeMap<String, View<'source>>,
    owners: Option<&'source crate::container::parts::EntryOwners<'source>>,
    parents: Option<&'source BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>>,
}

impl<'source> EntrySources<'source> {
    pub(crate) fn new(
        context: &DecodeContext<'_>,
        entries: &'source BTreeMap<String, View<'source>>,
    ) -> Result<Self, CodecError> {
        for source in entries.values() {
            context.require_source(*source)?;
        }
        Ok(Self {
            entries,
            owners: None,
            parents: None,
        })
    }

    pub(crate) fn with_owners(
        mut self,
        owners: &'source crate::container::parts::EntryOwners<'source>,
    ) -> Self {
        self.owners = Some(owners);
        self
    }

    pub(crate) fn owner(
        &self,
        entry: &EntryRecord,
    ) -> Result<&cadmpeg_core::decode::RecordOwner<'source>, CodecError> {
        self.owners
            .and_then(|owners| owners.get(entry.name()))
            .map(|owner| &***owner)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "archive entry {} has no genuine header record owner",
                    entry.name()
                ))
            })
    }

    pub(crate) fn with_parents(
        mut self,
        parents: &'source BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>,
    ) -> Self {
        self.parents = Some(parents);
        self
    }

    pub(crate) fn parent(
        &self,
        entry: &EntryRecord,
    ) -> Option<&cadmpeg_ir::source_fidelity::parts::PartParent> {
        self.parents.and_then(|parents| parents.get(entry.name()))
    }

    pub(crate) fn contains(&self, entry: &EntryRecord) -> bool {
        self.optional_view(entry).is_some()
    }

    pub(crate) fn optional_view(&self, entry: &EntryRecord) -> Option<View<'source>> {
        self.entries.get(entry.name()).copied()
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
