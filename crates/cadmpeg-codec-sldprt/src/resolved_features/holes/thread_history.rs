// SPDX-License-Identifier: Apache-2.0
//! First-use history lookups for selected cosmetic-thread producers.

use crate::history::project::solid::HoleProfileKey;
use crate::records::{Feature, FeatureSource};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap};

const OPERATION: &str = "enrich SLDPRT cosmetic thread diameters";
const RECORDS: &str = "index SLDPRT hole profile records";

struct UniqueRecords<'features, 'ctx> {
    entries: HashMap<&'features str, Option<&'features Feature>>,
    _storage: ScopedReservation<'ctx>,
}

/// Borrowed tables die before their backing leases. Each source is walked
/// only when its first query requires that table; later queries reuse it.
pub(super) struct ThreadHistoryLookups<'features, 'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    features: &'features [Feature],
    by_id: Option<HashMap<&'features str, &'features Feature>>,
    by_source: Option<BTreeMap<FeatureSource, &'features Feature>>,
    records: Option<UniqueRecords<'features, 'ctx>>,
    storage: ScopedReservation<'ctx>,
}

impl<'features, 'ctx, 'arena> ThreadHistoryLookups<'features, 'ctx, 'arena> {
    pub(super) fn new(
        context: &'ctx DecodeContext<'arena>,
        features: &'features [Feature],
    ) -> Result<Self, CodecError> {
        Ok(Self {
            context,
            features,
            by_id: None,
            by_source: None,
            records: None,
            storage: context.reserve_scoped(0, OPERATION)?,
        })
    }

    pub(super) fn by_id(&mut self, id: &str) -> Result<Option<&'features Feature>, CodecError> {
        if let Some(entries) = &self.by_id {
            return Ok(self.context.get_hash_map(entries, id, "resolve SLDPRT holes keys")?.copied());
        }
        let mut entries = HashMap::new();
        let mut source = self.features.iter();
        while let Some(feature) = self.context.next_charged(&mut source, OPERATION)? {
            self.storage.with_storage(|| {
                self.context.insert_hash_map(&mut entries, feature.id.as_str(), feature, OPERATION)
            })?;
        }
        let found = self.context.get_hash_map(&entries, id, "resolve SLDPRT holes keys")?.copied();
        self.by_id = Some(entries);
        Ok(found)
    }

    pub(super) fn profile(&mut self, key: HoleProfileKey<'_>) -> Result<Option<&'features Feature>, CodecError> {
        match key {
            HoleProfileKey::Source(source) => {
                if let Some(entries) = &self.by_source {
                    return Ok(self.context.get_btree_map(entries, &source, "look up SLDPRT feature source")?.copied());
                }
                let mut entries = BTreeMap::new();
                let mut features = self.features.iter();
                while let Some(feature) = self.context.next_charged(&mut features, OPERATION)? {
                    if let Some(source) = feature.source_id {
                        self.storage.with_storage(|| {
                            self.context.insert_btree_map(&mut entries, source, feature, OPERATION)
                        })?;
                    }
                }
                let found = self.context.get_btree_map(&entries, &source, "look up SLDPRT feature source")?.copied();
                self.by_source = Some(entries);
                Ok(found)
            }
            HoleProfileKey::Record(id) => {
                if let Some(records) = &self.records {
                    return Ok(self.context.get_hash_map(&records.entries, id, "find SLDPRT hole profile record")?.copied().flatten());
                }
                let (entries, storage) = self.context.unique_index(
                    self.features.iter().map(|feature| (feature.id.as_str(), feature)),
                    RECORDS,
                )?;
                let records = UniqueRecords { entries, _storage: storage };
                let found = self.context.get_hash_map(&records.entries, id, "find SLDPRT hole profile record")?.copied().flatten();
                self.records = Some(records);
                Ok(found)
            }
        }
    }
}

#[cfg(test)]
mod tests;
