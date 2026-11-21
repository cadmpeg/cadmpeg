// SPDX-License-Identifier: Apache-2.0
//! The retained members that own SLDPRT metadata records, sorted by what a
//! semantic write can do with each.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::Annotations;

use super::{Held, Unwritten};
use crate::container::ContainerScan;
use crate::SourceRecord;

/// The retained metadata owners of one write, by stream.
#[derive(Debug, Default)]
pub(super) struct MetadataOwners {
    /// Members the patch writes into: one retained record with bytes, equal
    /// to its named source envelope member. Sorted by name.
    pub(super) patchable: Vec<(String, Vec<u8>)>,
    /// Members written as stored: several retained records, an unnamed
    /// member, or bytes unlike the envelope member.
    pub(super) carried: BTreeSet<String>,
    /// Members whose bytes the source fidelity does not hold.
    pub(super) unavailable: BTreeSet<String>,
    /// Members that are generated partition sections.
    pub(super) regenerated: BTreeSet<String>,
}

impl MetadataOwners {
    /// Whether retained members hold the metadata records: the write keeps
    /// them and generates no `SWObjects` member of its own.
    pub(super) fn replay(&self) -> bool {
        !self.patchable.is_empty() || !self.carried.is_empty()
    }

    /// Why the patch writes no attribute stored in `stream`, and what the
    /// output holds for it; `None` when `stream` is patchable.
    pub(super) fn unpatched(&self, stream: &str) -> Option<(Unwritten, Held)> {
        if self.patchable.iter().any(|(name, _)| name == stream) {
            None
        } else if self.carried.contains(stream) {
            Some((Unwritten::OwnerCarried, Held::Source))
        } else if self.regenerated.contains(stream) {
            Some((Unwritten::OwnerRegenerated, Held::Nothing))
        } else {
            Some((Unwritten::OwnerUnavailable, Held::Nothing))
        }
    }
}

/// Sort every stream that owns a metadata record: each stream whose name
/// holds `swobjects`, and each stream a metadata attribute's provenance
/// names.
pub(super) fn metadata_owners(
    ir: &CadIr,
    records: &[SourceRecord<'_>],
    annotations: &Annotations,
    scan: Option<&ContainerScan<'_>>,
    generated_partitions: &HashSet<String>,
) -> MetadataOwners {
    let metadata_streams = super::metadata_attributes(ir)
        .into_iter()
        .filter(|attribute| attribute.name != "source_linear_unit_code")
        .filter_map(|attribute| {
            annotations
                .provenance
                .get(attribute.id.as_str())
                .map(cadmpeg_ir::Provenance::stream)
        })
        .collect::<BTreeSet<_>>();
    let mut owned = BTreeMap::<&str, Vec<&SourceRecord<'_>>>::new();
    for stream in &metadata_streams {
        owned.entry(stream).or_default();
    }
    for record in records {
        let Some(provenance) = annotations.provenance.get(record.id.as_str()) else {
            continue;
        };
        let stream = provenance.stream();
        if stream.to_ascii_lowercase().contains("swobjects") || metadata_streams.contains(stream) {
            let retained = owned.entry(stream).or_default();
            if retained.iter().all(|other| other.id != record.id) {
                retained.push(record);
            }
        }
    }
    let mut owners = MetadataOwners::default();
    for (stream, retained) in owned {
        if generated_partitions.contains(stream) {
            owners.regenerated.insert(stream.to_owned());
            continue;
        }
        match retained.as_slice() {
            [] | [SourceRecord { data: None, .. }] => {
                owners.unavailable.insert(stream.to_owned());
            }
            [SourceRecord {
                data: Some(bytes), ..
            }] if scan.is_none_or(|scan| named_member_holds(scan, stream, bytes)) => {
                owners.patchable.push((stream.to_owned(), bytes.to_vec()));
            }
            _ => {
                owners.carried.insert(stream.to_owned());
            }
        }
    }
    owners
}

/// Whether the source envelope holds exactly one member of `stream`, named
/// `stream`, with `bytes` as its payload.
fn named_member_holds(scan: &ContainerScan<'_>, stream: &str, bytes: &[u8]) -> bool {
    let mut members = scan
        .sections()
        .filter(|section| section.source_stream().as_str() == stream);
    matches!(
        (members.next(), members.next()),
        (Some(member), None) if member.name() == Some(stream) && member.payload() == bytes
    )
}
