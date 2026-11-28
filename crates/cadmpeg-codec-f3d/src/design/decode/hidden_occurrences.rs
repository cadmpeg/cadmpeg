// SPDX-License-Identifier: Apache-2.0
//! Hidden-occurrence records: the display visibility of a component
//! occurrence.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::container::ContainerScan;
use crate::records::entity_header::SegmentType;
use crate::records::feature::assembly_features::DesignComponentOccurrence;
use crate::records::xref::XrefReference;

use super::meta::stream_types_by_entity;
use super::sketch::IndexedRecordOffsets;

/// Type GUID of the hidden-occurrence record.
pub(crate) const HIDDEN_OCCURRENCE_TYPE_GUID: &str = "2BB3BB69-B0FD-4D56-B4CD-DEBD30847BE3";
/// Bytes between the indexed header and the marker of the reference count.
const ZERO_RUN: usize = 8;
/// Bytes of a marked reference that follow its marker and u32 index.
const REFERENCE_ZEROS: usize = 4;

/// One hidden-occurrence record resolved against the occurrence carriers of
/// its stream.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignHiddenOccurrence {
    pub(crate) id: String,
    /// Native scope of the Design `BulkStream` that holds the record.
    pub(crate) stream: String,
    /// Record index of the hidden-occurrence record.
    pub(crate) record_index: u32,
    /// Record index of the hidden occurrence carrier.
    pub(crate) target: u32,
    /// Record indexes of the carriers that the path names before the target,
    /// in record order without repeats.
    pub(crate) ancestors: Vec<u32>,
}

/// Decode every hidden-occurrence record of every Design `BulkStream`.
pub(crate) fn decode_hidden_occurrences(
    scan: &ContainerScan<'_>,
    types: &[SegmentType],
    carriers: &[DesignComponentOccurrence],
    references: &[XrefReference],
) -> Result<Vec<DesignHiddenOccurrence>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream_types = stream_types_by_entity(types, &entry.name);
        let entities = stream_types
            .iter()
            .filter(|(_, (guid, _))| guid.eq_ignore_ascii_case(HIDDEN_OCCURRENCE_TYPE_GUID))
            .filter_map(|(entity, _)| u32::try_from(*entity).ok())
            .collect::<BTreeSet<_>>();
        if entities.is_empty() {
            continue;
        }
        let stream = crate::ids::native_scope(&entry.name);
        let occurrences = carriers
            .iter()
            .filter(|carrier| crate::ids::native_stream(&carrier.id) == Some(stream.as_str()))
            .map(|carrier| carrier.record_index)
            .chain(
                references
                    .iter()
                    .filter_map(|reference| reference.placement_source.as_ref())
                    .filter(|source| source.stream == stream)
                    .filter_map(|source| u32::try_from(source.carrier_record_index).ok()),
            )
            .chain(
                stream_types
                    .iter()
                    .filter(|(_, (guid, _))| {
                        guid.eq_ignore_ascii_case(
                    crate::records::feature::assembly_features::OCCURRENCE_PLACEMENT_TYPE_GUID,
                )
                    })
                    .filter_map(|(entity, _)| u32::try_from(*entity).ok()),
            )
            .collect::<BTreeSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = scan.design_record_offsets(&entry.name)?;
        out.extend(decode_stream_with_records(
            bytes,
            &records,
            &stream,
            &entities,
            &occurrences,
        ));
    }
    Ok(out)
}

/// The hidden-occurrence records of one stream. `entities` holds the record
/// indexes of the type's entities; `occurrences` the record indexes of the
/// stream's occurrence carriers.
#[cfg(test)]
pub(super) fn decode_stream(
    bytes: &[u8],
    stream: &str,
    entities: &BTreeSet<u32>,
    occurrences: &BTreeSet<u32>,
) -> Vec<DesignHiddenOccurrence> {
    let records = IndexedRecordOffsets::build(bytes);
    decode_stream_with_records(bytes, &records, stream, entities, occurrences)
}

fn decode_stream_with_records(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    entities: &BTreeSet<u32>,
    occurrences: &BTreeSet<u32>,
) -> Vec<DesignHiddenOccurrence> {
    let mut out = Vec::new();
    for &entity in entities {
        let Some(&start) = records.offsets(entity).first() else {
            continue;
        };
        let end = records.next_offset(start + 1).unwrap_or(bytes.len());
        let Some(body) = bytes.get(start + 11..end) else {
            continue;
        };
        let Some(path) = occurrence_path(body, occurrences) else {
            continue;
        };
        let Some((&target, ancestors)) = path.split_last() else {
            continue;
        };
        let mut distinct = Vec::new();
        for &ancestor in ancestors {
            if ancestor != target && !distinct.contains(&ancestor) {
                distinct.push(ancestor);
            }
        }
        out.push(DesignHiddenOccurrence {
            id: format!("{stream}:design-hidden-occurrence#{entity}"),
            stream: stream.to_owned(),
            record_index: entity,
            target,
            ancestors: distinct,
        });
    }
    out
}

/// The occurrence carriers a record body names, in order. The body starts
/// with eight zero bytes, `u8 1` and a u32 reference count; every marked
/// reference after them is `u8 1`, a u32 record index and four zero bytes.
fn occurrence_path(body: &[u8], occurrences: &BTreeSet<u32>) -> Option<Vec<u32>> {
    if body.get(..ZERO_RUN)? != [0; ZERO_RUN] || *body.get(ZERO_RUN)? != 1 {
        return None;
    }
    View::u32_le_at(body, ZERO_RUN + 1)?;
    let mut path = Vec::new();
    let mut at = ZERO_RUN + 5;
    while at + 5 + REFERENCE_ZEROS <= body.len() {
        if body[at] == 1 && body[at + 5..at + 5 + REFERENCE_ZEROS] == [0; REFERENCE_ZEROS] {
            if let Some(index) = View::u32_le_at(body, at + 1).filter(|i| occurrences.contains(i)) {
                path.push(index);
                at += 5 + REFERENCE_ZEROS;
                continue;
            }
        }
        at += 1;
    }
    (!path.is_empty()).then_some(path)
}

/// Mark each occurrence that a hidden-occurrence record names. An occurrence
/// is hidden when it instantiates the record's target carrier, a local
/// occurrence carrier or the placement carrier of an external reference, and,
/// for each ancestor the record names, one of its containing occurrences
/// instantiates that ancestor carrier. A record with an ancestor that has no
/// admitted occurrence applies to nothing.
/// `parents` names the occurrence each nested occurrence sits in.
/// Return the number of paths that retain unbound carriers.
pub(crate) fn apply_hidden_occurrences(
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
    parents_of: &std::collections::BTreeMap<
        cadmpeg_ir::ids::OccurrenceId,
        cadmpeg_ir::ids::OccurrenceId,
    >,
    carriers: &[DesignComponentOccurrence],
    references: &[XrefReference],
    hidden: &[DesignHiddenOccurrence],
    components: &std::collections::BTreeMap<String, super::components::StreamComponents>,
) -> usize {
    if hidden.is_empty() {
        return 0;
    }
    let by_id: HashMap<&str, (&str, u32)> = carriers
        .iter()
        .filter_map(|carrier| {
            Some((
                carrier.id.as_str(),
                (
                    crate::ids::native_stream(&carrier.id)?,
                    carrier.record_index,
                ),
            ))
        })
        .chain(references.iter().filter_map(|reference| {
            let source = reference.placement_source.as_ref()?;
            Some((
                reference.id.as_str(),
                (
                    source.stream.as_str(),
                    u32::try_from(source.carrier_record_index).ok()?,
                ),
            ))
        }))
        .collect();
    let present: BTreeSet<_> = occurrences
        .iter()
        .map(|occurrence| &occurrence.id)
        .collect();
    let roots: HashMap<_, Vec<_>> = components
        .iter()
        .filter_map(|(stream, entries)| {
            let root = entries.roots.get(&entries.root_component?)?;
            let id = crate::design::components::root_occurrence_id(components, stream, root);
            if !present.contains(&id) {
                return None;
            }
            Some((
                id,
                entries
                    .root_instances
                    .iter()
                    .filter_map(|index| u32::try_from(*index).ok())
                    .map(|index| (stream.as_str(), index))
                    .collect(),
            ))
        })
        .collect();
    let carrier_of = |occurrence: &cadmpeg_ir::products::Occurrence| {
        occurrence
            .native_ref
            .as_deref()
            .and_then(|native| by_id.get(native).copied())
            .into_iter()
            .chain(roots.get(&occurrence.id).into_iter().flatten().copied())
            .collect::<Vec<_>>()
    };
    let parents: HashMap<cadmpeg_ir::ids::OccurrenceId, usize> = occurrences
        .iter()
        .enumerate()
        .map(|(position, occurrence)| (occurrence.id.clone(), position))
        .collect();
    let mut chosen = Vec::new();
    let mut bound = BTreeSet::new();
    for (position, occurrence) in occurrences.iter().enumerate() {
        let identities = carrier_of(occurrence);
        if identities.is_empty() {
            continue;
        }
        let mut ancestors = Vec::new();
        let mut current = occurrence;
        for _ in 0..occurrences.len() {
            let Some(parent) = parents_of.get(&current.id) else {
                break;
            };
            let Some(&next) = parents.get(parent) else {
                break;
            };
            current = &occurrences[next];
            ancestors.extend(carrier_of(current));
        }
        let mut named = false;
        for (index, entry) in hidden.iter().enumerate() {
            if identities
                .iter()
                .any(|&(stream, record)| entry.stream == stream && entry.target == record)
                && entry
                    .ancestors
                    .iter()
                    .all(|ancestor| ancestors.contains(&(entry.stream.as_str(), *ancestor)))
            {
                bound.insert(index);
                named = true;
            }
        }
        if named {
            chosen.push(position);
        }
    }
    for position in chosen {
        occurrences[position].visible = Some(false);
    }
    hidden.len() - bound.len()
}

#[cfg(test)]
mod tests;
