// SPDX-License-Identifier: Apache-2.0
//! Shared Fusion `MetaStream` segment framing.

use cadmpeg_core::le::u32_at;
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_hyphenated, is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded};
use crate::records::SegmentType;

/// One primary record-index entry locating a sibling `BulkStream` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordIndexEntry {
    pub(crate) entity_id: u64,
    pub(crate) bulk_offset: u64,
}

/// One completely framed `MetaStream` segment.
pub(crate) struct MetaStream {
    pub(crate) types: Vec<SegmentType>,
    pub(crate) records: Vec<RecordIndexEntry>,
}

/// One exact sibling-BulkStream extent from the primary record index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrimaryRecordFrame {
    pub(crate) entity_id: u64,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Resolve the primary index to nonempty, strictly ordered sibling-BulkStream
/// extents.
pub(crate) fn primary_record_frames(
    meta: &MetaStream,
    bulk_len: usize,
) -> Result<Vec<PrimaryRecordFrame>, CodecError> {
    let mut frames = Vec::with_capacity(meta.records.len());
    for (ordinal, record) in meta.records.iter().enumerate() {
        let start = usize::try_from(record.bulk_offset)
            .map_err(|_| CodecError::Malformed("F3D primary record offset exceeds usize".into()))?;
        let end = meta.records.get(ordinal + 1).map_or_else(
            || Ok(bulk_len),
            |next| {
                usize::try_from(next.bulk_offset).map_err(|_| {
                    CodecError::Malformed("F3D primary record end exceeds usize".into())
                })
            },
        )?;
        if start >= end || end > bulk_len {
            return Err(CodecError::Malformed(
                "F3D primary record extents are not nonempty and strictly increasing within the BulkStream"
                    .into(),
            ));
        }
        frames.push(PrimaryRecordFrame {
            entity_id: record.entity_id,
            start,
            end,
        });
    }
    Ok(frames)
}

fn take_counted_run(bytes: &[u8], at: &mut usize, stride: usize) -> Option<()> {
    let count = usize::try_from(u32_at(bytes, *at)?).ok()?;
    let start = at.checked_add(4)?;
    let end = count.checked_mul(stride)?.checked_add(start)?;
    bytes.get(start..end)?;
    *at = end;
    Some(())
}

fn take_record_index(bytes: &[u8], at: &mut usize) -> Option<Vec<RecordIndexEntry>> {
    let count = usize::try_from(u32_at(bytes, *at)?).ok()?;
    let records_at = at.checked_add(4)?;
    let records_end = count.checked_mul(16)?.checked_add(records_at)?;
    let raw_records = bytes.get(records_at..records_end)?;
    let mut records = Vec::with_capacity(count);
    for raw_record in raw_records.chunks_exact(16) {
        records.push(RecordIndexEntry {
            entity_id: u64::from_le_bytes(raw_record[..8].try_into().ok()?),
            bulk_offset: u64::from_le_bytes(raw_record[8..].try_into().ok()?),
        });
    }
    *at = records_end;
    Some(records)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParseFailure {
    field: &'static str,
    offset: usize,
}

fn require<T>(value: Option<T>, field: &'static str, offset: usize) -> Result<T, ParseFailure> {
    value.ok_or(ParseFailure { field, offset })
}

fn take_version_context(bytes: &[u8], at: &mut usize) -> Result<(), ParseFailure> {
    let present_at = *at;
    let present = require(u32_at(bytes, *at), "version-context presence", *at)?;
    *at = require(at.checked_add(4), "version-context presence", *at)?;
    match present {
        0 => Ok(()),
        1 => {
            let token_end = require(at.checked_add(8), "version-context token", *at)?;
            require(bytes.get(*at..token_end), "version-context token", *at)?;
            *at = token_end;
            for field in [
                "version-context asset GUID",
                "version-context revision GUID",
            ] {
                let (guid, next) = require(lp_utf16_bounded(bytes, *at, 36..=36), field, *at)?;
                if !is_guid_hyphenated(&guid) {
                    return Err(ParseFailure { field, offset: *at });
                }
                *at = next;
            }
            let (version_urn, next) = require(
                lp_utf16_bounded(bytes, *at, 1..=1024),
                "version-context version URN",
                *at,
            )?;
            let version_urn = version_urn.as_bytes();
            if version_urn.len() <= 4
                || !version_urn[..4].eq_ignore_ascii_case(b"urn:")
                || !version_urn[4..].iter().all(u8::is_ascii_graphic)
            {
                return Err(ParseFailure {
                    field: "version-context version URN",
                    offset: *at,
                });
            }
            *at = next;
            let (guid, next) = require(
                lp_utf16_bounded(bytes, *at, 36..=36),
                "version-context asset revision GUID",
                *at,
            )?;
            if !is_guid_hyphenated(&guid) {
                return Err(ParseFailure {
                    field: "version-context asset revision GUID",
                    offset: *at,
                });
            }
            *at = next;
            require(u32_at(bytes, *at), "version-context revision", *at)?;
            *at = require(at.checked_add(4), "version-context revision", *at)?;
            Ok(())
        }
        _ => Err(ParseFailure {
            field: "version-context presence",
            offset: present_at,
        }),
    }
}

fn parse_inner(bytes: &[u8]) -> Result<MetaStream, ParseFailure> {
    // Header: short segment type name, segment id, asset GUID, serializer
    // magic and its magic-gated integer group, full segment type name, add-in
    // name, and the segment type code.
    let (_, at) = require(
        lp_ascii_filtered(bytes, 0, 1..=256, u8::is_ascii_graphic),
        "short segment type name",
        0,
    )?;
    let at = require(at.checked_add(4), "segment id", at)?;
    let (_, at) = require(lp_utf16_bounded(bytes, at, 0..=256), "asset GUID", at)?;
    let magic = require(u32_at(bytes, at), "serializer magic", at)?;
    let at = require(
        at.checked_add(if magic == 1234 { 16 } else { 8 }),
        "serializer integer group",
        at,
    )?;
    require(
        bytes.get(..at),
        "serializer integer group",
        at.min(bytes.len()),
    )?;
    let (_, at) = require(
        lp_ascii_filtered(bytes, at, 1..=256, u8::is_ascii_graphic),
        "full segment type name",
        at,
    )?;
    let (_, at) = require(
        lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic),
        "add-in name",
        at,
    )?;
    let mut at = require(at.checked_add(8), "segment type code", at)?;
    require(bytes.get(..at), "segment type code", at.min(bytes.len()))?;

    let count = require(u32_at(bytes, at), "type count", at)?;
    at = require(at.checked_add(4), "type count", at)?;
    let mut types = Vec::new();
    for _ in 0..count {
        let entry_at = at;
        let type_guid_offset = require(at.checked_add(4), "type GUID", at)?;
        let (type_guid, next) = require(
            lp_ascii_filtered(bytes, at, 1..=256, u8::is_ascii_graphic)
                .filter(|(guid, _)| is_guid_relaxed(guid)),
            "type GUID",
            at,
        )?;
        at = next;
        let base_type_guid_offset = require(at.checked_add(4), "base type GUID", at)?;
        let (base_type_guid, next) = require(
            lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)
                .filter(|(guid, _)| guid.is_empty() || is_guid_relaxed(guid)),
            "base type GUID",
            at,
        )?;
        at = next;
        let version_offset = at;
        let version = require(u32_at(bytes, at), "type version", at)?;
        at = require(at.checked_add(4), "type version", at)?;
        let (module, next) = require(
            lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic),
            "type module",
            at,
        )?;
        at = next;
        let id_count = usize::try_from(require(u32_at(bytes, at), "type entity count", at)?)
            .map_err(|_| ParseFailure {
                field: "type entity count",
                offset: at,
            })?;
        let ids_at = require(at.checked_add(4), "type entity count", at)?;
        let ids_end = require(
            id_count
                .checked_mul(8)
                .and_then(|length| length.checked_add(ids_at)),
            "type entity ids",
            ids_at,
        )?;
        let raw_ids = require(bytes.get(ids_at..ids_end), "type entity ids", ids_at)?;
        at = ids_end;
        types.push(SegmentType {
            id: String::new(),
            byte_offset: entry_at as u64,
            type_guid,
            type_guid_offset: type_guid_offset as u64,
            base_type_guid_offset: (!base_type_guid.is_empty())
                .then_some(base_type_guid_offset as u64),
            base_type_guid: (!base_type_guid.is_empty()).then_some(base_type_guid),
            version,
            version_offset: version_offset as u64,
            module,
            entity_ids: raw_ids
                .chunks_exact(8)
                .map(|raw| {
                    u64::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: chunks_exact(8) yields 8-byte slices"),
                    )
                })
                .collect(),
            entity_id_offsets: (0..id_count)
                .map(|index| (ids_at + index * 8) as u64)
                .collect(),
        });
    }

    // Named entities, the primary record index, and the secondary index.
    let named_entities_at = at;
    require(
        take_counted_run(bytes, &mut at, 8),
        "named-entity index",
        named_entities_at,
    )?;
    let primary_index_at = at;
    let records = require(
        take_record_index(bytes, &mut at),
        "primary record index",
        primary_index_at,
    )?;
    let secondary_index_at = at;
    require(
        take_counted_run(bytes, &mut at, 16),
        "secondary record index",
        secondary_index_at,
    )?;

    // A legacy segment can end after the secondary index, after the
    // next-entity counter, or after the version-context/property suffix.
    if at < bytes.len() {
        let end = require(at.checked_add(8), "next-entity counter", at)?;
        require(bytes.get(at..end), "next-entity counter", at)?;
        at += 8;
    }
    if at < bytes.len() {
        take_version_context(bytes, &mut at)?;
        let properties = require(u32_at(bytes, at), "property count", at)?;
        at = require(at.checked_add(4), "property count", at)?;
        for _ in 0..properties {
            let (_, next) = require(
                lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic),
                "property name",
                at,
            )?;
            at = require(next.checked_add(4), "property value", next)?;
            require(bytes.get(..at), "property value", at.min(bytes.len()))?;
        }
    }
    if at != bytes.len() {
        return Err(ParseFailure {
            field: "trailing bytes",
            offset: at,
        });
    }
    Ok(MetaStream { types, records })
}

/// Parse one complete `MetaStream` segment and reject any unframed remainder.
pub(crate) fn parse(bytes: &[u8], stream: &str) -> Result<MetaStream, CodecError> {
    parse_inner(bytes).map_err(|failure| {
        CodecError::Malformed(format!(
            "invalid F3D MetaStream {} at byte {}: {stream}",
            failure.field, failure.offset
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{parse, primary_record_frames, MetaStream, RecordIndexEntry};

    fn lp_ascii(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn stream_prefix() -> Vec<u8> {
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "ACT");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
        bytes.extend_from_slice(&1234u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 12]);
        lp_ascii(&mut bytes, "FusionACTSegmentType");
        lp_ascii(&mut bytes, "Fusion");
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&[0; 16]);
        bytes
    }

    #[test]
    fn rejects_unframed_empty_input() {
        assert!(parse(&[], "empty").is_err());
    }

    #[test]
    fn parses_present_version_context_before_properties() {
        let mut bytes = stream_prefix();
        bytes.extend_from_slice(&15u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
        for value in [
            "11111111-2222-3333-4444-555555555555",
            "66666666-7777-8888-9999-aaaaaaaaaaaa",
        ] {
            lp_utf16(&mut bytes, value);
        }
        lp_utf16(&mut bytes, "urn:synthetic:version:2");
        lp_utf16(&mut bytes, "bbbbbbbb-cccc-dddd-eeee-ffffffffffff");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for (name, value) in [("Application", 1u32), ("Server", 1)] {
            lp_ascii(&mut bytes, name);
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        let parsed = parse(&bytes, "version-context").expect("framed version context");
        assert!(parsed.types.is_empty());
        assert!(parsed.records.is_empty());

        let mut invalid_presence = stream_prefix();
        invalid_presence.extend_from_slice(&15u64.to_le_bytes());
        invalid_presence.extend_from_slice(&2u32.to_le_bytes());
        assert!(parse(&invalid_presence, "invalid-version-context").is_err());
        assert!(parse(&bytes[..bytes.len() - 1], "truncated-version-context").is_err());
    }

    #[test]
    fn rejects_primary_index_count_beyond_the_stream_extent() {
        let mut bytes = stream_prefix();
        let primary_count = bytes.len() - 8;
        bytes[primary_count..primary_count + 4].copy_from_slice(&u32::MAX.to_le_bytes());

        assert!(parse(&bytes, "oversized-primary-index").is_err());
    }

    #[test]
    fn primary_index_frames_end_at_the_next_primary_offset() {
        let meta = MetaStream {
            types: Vec::new(),
            records: vec![
                RecordIndexEntry {
                    entity_id: 11,
                    bulk_offset: 3,
                },
                RecordIndexEntry {
                    entity_id: 12,
                    bulk_offset: 9,
                },
            ],
        };

        let frames = primary_record_frames(&meta, 14).expect("ordered primary extents");
        assert_eq!(frames[0].start, 3);
        assert_eq!(frames[0].end, 9);
        assert_eq!(frames[1].start, 9);
        assert_eq!(frames[1].end, 14);

        let empty = MetaStream {
            types: Vec::new(),
            records: Vec::new(),
        };
        assert!(primary_record_frames(&empty, 0)
            .expect("an empty primary index has no frames")
            .is_empty());

        let empty_last = MetaStream {
            types: Vec::new(),
            records: vec![RecordIndexEntry {
                entity_id: 11,
                bulk_offset: 14,
            }],
        };
        assert!(primary_record_frames(&empty_last, 14).is_err());

        let repeated_offset = MetaStream {
            types: Vec::new(),
            records: vec![
                RecordIndexEntry {
                    entity_id: 11,
                    bulk_offset: 3,
                },
                RecordIndexEntry {
                    entity_id: 12,
                    bulk_offset: 3,
                },
            ],
        };
        assert!(primary_record_frames(&repeated_offset, 14).is_err());
    }
}
