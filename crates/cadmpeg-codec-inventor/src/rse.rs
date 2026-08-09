// SPDX-License-Identifier: Apache-2.0
//! `RSe` storage navigation and stable governing types.

use std::collections::BTreeMap;

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot, CompoundStreamId};

/// A validated `V<n>` storage-band number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StorageBand(u32);

impl StorageBand {
    pub(crate) fn parse(component: &str) -> Option<Self> {
        let (prefix, digits) = component.split_at_checked(1)?;
        if !prefix.eq_ignore_ascii_case("V") {
            return None;
        }
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().map(Self)
    }

    pub(crate) const fn value(self) -> u32 {
        self.0
    }
}

/// Exact suffix shared by one `RSe` metadata and bulk stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SegmentToken(String);

impl SegmentToken {
    fn parse(name: &str) -> Option<(char, Self)> {
        let (prefix, token) = name.split_at_checked(1)?;
        let prefix = prefix.chars().next()?;
        if !matches!(prefix, 'M' | 'B') || token.is_empty() {
            return None;
        }
        Some((prefix, Self(token.into())))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// One exact M/B stream pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentPair {
    pub(crate) token: SegmentToken,
    pub(crate) metadata: CompoundStreamId,
    pub(crate) bulk: CompoundStreamId,
}

/// `RSe` paths established from the compound directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RseInventory {
    pub(crate) storage_bands: Vec<(StorageBand, CompoundStreamId)>,
    pub(crate) segments: Vec<SegmentPair>,
    pub(crate) unpaired_metadata: Vec<SegmentToken>,
    pub(crate) unpaired_bulk: Vec<SegmentToken>,
}

impl RseInventory {
    pub(crate) fn build(snapshot: &CompoundSnapshot<'_>) -> Self {
        let mut databases = Vec::new();
        let mut metadata = BTreeMap::new();
        let mut bulk = BTreeMap::new();
        for entry in snapshot.entries() {
            let CompoundEntry::Stream(stream) = entry else {
                continue;
            };
            let path = stream.path();
            if let Some(band) = database_band(path) {
                databases.push((band, stream.id()));
                continue;
            }
            let Some(name) = direct_rse_child(path) else {
                continue;
            };
            let Some((prefix, token)) = SegmentToken::parse(name) else {
                continue;
            };
            match prefix {
                'M' => {
                    metadata.insert(token, stream.id());
                }
                'B' => {
                    bulk.insert(token, stream.id());
                }
                _ => unreachable!("validated segment prefix"),
            }
        }
        databases.sort_by_key(|(band, _)| *band);
        let segments = metadata
            .iter()
            .filter_map(|(token, metadata)| {
                bulk.get(token).map(|bulk| SegmentPair {
                    token: token.clone(),
                    metadata: *metadata,
                    bulk: *bulk,
                })
            })
            .collect();
        let unpaired_metadata = metadata
            .keys()
            .filter(|token| !bulk.contains_key(*token))
            .cloned()
            .collect();
        let unpaired_bulk = bulk
            .keys()
            .filter(|token| !metadata.contains_key(*token))
            .cloned()
            .collect();
        Self {
            storage_bands: databases,
            segments,
            unpaired_metadata,
            unpaired_bulk,
        }
    }
}

pub(crate) fn direct_rse_child(path: &str) -> Option<&str> {
    let mut components = path.split('/');
    let storage = components.next()?;
    let child = components.next()?;
    (storage.eq_ignore_ascii_case("RSeStorage") && components.next().is_none()).then_some(child)
}

pub(crate) fn database_band(path: &str) -> Option<StorageBand> {
    let mut components = path.split('/');
    let storage = components.next()?;
    let band = components.next()?;
    let name = components.next()?;
    (storage.eq_ignore_ascii_case("RSeStorage")
        && name.eq_ignore_ascii_case("RSeDb")
        && components.next().is_none())
    .then(|| StorageBand::parse(band))
    .flatten()
}
