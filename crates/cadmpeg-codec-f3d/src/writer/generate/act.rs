// SPDX-License-Identifier: Apache-2.0
//! ACT segments authored from current typed tables and their class registries.

use std::collections::{BTreeMap, BTreeSet};

use super::records::{native_lp_ascii, native_lp_utf16};
use crate::native::F3dNative;
use crate::records::entity_header::SegmentType;
use cadmpeg_core::CodecError;

pub(super) struct Segment {
    pub(super) folder: String,
    pub(super) bulk: Vec<u8>,
    pub(super) meta: Vec<u8>,
}

pub(super) fn encode(native: &F3dNative) -> Result<Vec<Segment>, CodecError> {
    let mut segments = Vec::new();
    for table in &native.act_tables {
        let scope = crate::ids::native_stream(&table.id)
            .ok_or_else(|| CodecError::malformed("ACT table has no native segment identity"))?;
        let prefix = scope
            .strip_suffix("BulkStream.dat")
            .ok_or_else(|| CodecError::malformed("ACT table identity must name a BulkStream"))?;
        let meta_scope = format!("{prefix}MetaStream.dat");
        let mut types = native
            .act_types
            .iter()
            .filter(|record| crate::ids::native_stream(record.id()) == Some(meta_scope.as_str()))
            .collect::<Vec<_>>();
        types.sort_by(|left, right| left.class_tag.as_str().cmp(right.class_tag.as_str()));
        let mut bulk = Vec::new();
        let mut records = Vec::new();
        let mut registrations = BTreeMap::<u32, usize>::new();
        let type_for = |index: u32| -> Result<usize, CodecError> {
            let mut matching = types.iter().enumerate().filter(|(_, record)| {
                record
                    .entities
                    .values()
                    .any(|value| *value == u64::from(index))
            });
            let Some((ordinal, _)) = matching.next() else {
                return Err(CodecError::UnsupportedContent(format!(
                    "ACT record {index} has no typed class registration"
                )));
            };
            if matching.next().is_some() {
                return Err(CodecError::malformed(
                    "ACT record belongs to multiple class registrations",
                ));
            }
            Ok(ordinal)
        };
        let mut header = |out: &mut Vec<u8>, index: u32| -> Result<(), CodecError> {
            let ordinal = type_for(index)?;
            if registrations.insert(index, ordinal).is_some() {
                return Err(CodecError::malformed(
                    "ACT typed records repeat a segment-local index",
                ));
            }
            records.push((
                index,
                u64::try_from(out.len()).map_err(|_| {
                    CodecError::InvalidInput("ACT stream exceeds u64 byte offsets".into())
                })?,
            ));
            let tag = ordinal
                .checked_add(256)
                .filter(|value| *value <= 999)
                .ok_or_else(|| {
                    CodecError::InvalidInput("ACT registry exceeds three-digit class tags".into())
                })?;
            native_lp_ascii(out, &tag.to_string())?;
            out.extend_from_slice(&index.to_le_bytes());
            Ok(())
        };
        header(&mut bulk, table.record_index)?;
        bulk.extend_from_slice(&0_u32.to_le_bytes());
        native_lp_ascii(&mut bulk, "ACTTable")?;
        bulk.extend_from_slice(&0_u16.to_le_bytes());
        let entities = native
            .act_entities
            .iter()
            .filter(|record| record.stream() == scope)
            .collect::<Vec<_>>();
        count(
            &mut bulk,
            entities.iter().filter(|record| record.in_table()).count(),
        )?;
        for entity in entities.iter().filter(|record| record.in_table()) {
            reference(&mut bulk, entity.record_index(), 6);
            native_lp_utf16(&mut bulk, entity.entity_id())?;
        }
        let mut guids = native
            .act_guids
            .iter()
            .filter(|record| record.stream() == scope)
            .collect::<Vec<_>>();
        guids.sort_by_key(|record| record.ordinal);
        for guid in guids {
            native_lp_utf16(&mut bulk, guid.guid.as_str())?;
        }
        let mut references = native
            .act_table_references
            .iter()
            .filter(|record| record.stream() == scope)
            .collect::<Vec<_>>();
        references.sort_by_key(|record| record.ordinal);
        count(&mut bulk, references.len())?;
        for record in references {
            reference(&mut bulk, record.target_record, 6);
        }
        let mut channels = native
            .act_registry_channels
            .iter()
            .filter(|record| record.stream() == scope)
            .collect::<Vec<_>>();
        channels.sort_by_key(|record| record.ordinal);
        count(&mut bulk, channels.len())?;
        for channel in channels {
            native_lp_ascii(&mut bulk, channel.name())?;
            native_lp_utf16(&mut bulk, channel.guid.as_str())?;
        }
        for root in native
            .act_root_components
            .iter()
            .filter(|record| record.stream() == scope)
        {
            header(&mut bulk, root.record_index)?;
            bulk.extend_from_slice(&[0; 10]);
            reference(&mut bulk, root.instance_root_record, 6);
            native_lp_utf16(&mut bulk, root.layout().entity_id())?;
            reference(&mut bulk, 3, 5);
            reference(&mut bulk, root.registry_flag.code(), 0);
            native_lp_utf16(&mut bulk, root.layout().display_name())?;
            bulk.push(0);
            reference(&mut bulk, root.components_root_record, 6);
        }
        for entity in entities {
            header(&mut bulk, entity.record_index())?;
            bulk.extend_from_slice(&[0; 10]);
            count(&mut bulk, entity.channel_group().channels().len())?;
            for (name, channel) in entity.channel_group().channels() {
                native_lp_ascii(&mut bulk, name)?;
                native_lp_utf16(&mut bulk, channel.value.as_str())?;
            }
            native_lp_utf16(&mut bulk, entity.entity_id())?;
        }
        let known = registrations.keys().copied().collect::<BTreeSet<_>>();
        if native
            .act_table_references
            .iter()
            .filter(|record| record.stream() == scope)
            .any(|record| !known.contains(&record.target_record))
        {
            return Err(CodecError::UnsupportedContent(
                "ACT table reference has no typed target record".into(),
            ));
        }
        let meta = metastream(&types, &records, &registrations)?;
        segments.push(Segment {
            folder: format!(
                "{}/FusionACTSegmentType{}",
                crate::manifest::GENERATED_DESIGN_ASSET_FOLDER,
                segments.len() + 1
            ),
            bulk,
            meta,
        });
    }
    Ok(segments)
}

fn count(out: &mut Vec<u8>, value: usize) -> Result<(), CodecError> {
    let value = u32::try_from(value)
        .map_err(|_| CodecError::InvalidInput("ACT count exceeds u32::MAX".into()))?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}
fn reference(out: &mut Vec<u8>, value: u32, trailing_zeros: usize) {
    out.push(1);
    out.extend_from_slice(&value.to_le_bytes());
    out.resize(out.len() + trailing_zeros, 0);
}
fn metastream(
    types: &[&SegmentType],
    records: &[(u32, u64)],
    registrations: &BTreeMap<u32, usize>,
) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    native_lp_ascii(&mut out, "FusionACTSegmentType")?;
    out.extend_from_slice(&0_u32.to_le_bytes());
    native_lp_utf16(&mut out, "00000000-0000-0000-0000-000000000000")?;
    out.extend_from_slice(&crate::metastream::MODERN_SERIALIZER_MAGIC.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    native_lp_ascii(&mut out, "FusionACTSegmentType")?;
    native_lp_ascii(&mut out, "Fusion")?;
    out.extend_from_slice(&[0; 8]);
    count(&mut out, types.len())?;
    for (ordinal, record) in types.iter().enumerate() {
        native_lp_ascii(&mut out, record.type_guid.as_str())?;
        native_lp_ascii(
            &mut out,
            record
                .base_type_guid
                .value()
                .map_or("", |guid| guid.as_str()),
        )?;
        out.extend_from_slice(&record.version.to_le_bytes());
        native_lp_ascii(&mut out, &record.module)?;
        let entities = registrations
            .iter()
            .filter_map(|(index, owner)| (*owner == ordinal).then_some(*index))
            .collect::<Vec<_>>();
        count(&mut out, entities.len())?;
        for entity in entities {
            out.extend_from_slice(&u64::from(entity).to_le_bytes());
        }
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    count(&mut out, records.len())?;
    for (index, offset) in records {
        out.extend_from_slice(&u64::from(*index).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    let next = records
        .iter()
        .map(|(index, _)| u64::from(*index) + 1)
        .max()
        .unwrap_or(1);
    out.extend_from_slice(&next.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests;
