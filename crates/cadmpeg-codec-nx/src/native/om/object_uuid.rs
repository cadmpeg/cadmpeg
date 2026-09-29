// SPDX-License-Identifier: Apache-2.0
//! UUID frames and the nonempty record set they intersect.

use crate::container::Container;
use crate::om::nonempty::NonEmpty;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Write;

fn decimal_digits(mut value: usize) -> usize {
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

fn uuid_value_id(
    ctx: &DecodeContext<'_>,
    section_ordinal: usize,
    offset: usize,
) -> Result<String, CodecError> {
    let length = "nx:om-object-uuid-values-:value#"
        .len()
        .checked_add(decimal_digits(section_ordinal))
        .and_then(|length| length.checked_add(decimal_digits(offset)))
        .ok_or_else(|| ctx.refuse_codec_limit("NX OM UUID identity length", 0, 1))?;
    let mut id = ctx.retained_string(length, "retain NX OM UUID identity")?;
    write!(
        id,
        "nx:om-object-uuid-values-{section_ordinal}:value#{offset}"
    )
    .map_err(|_| ctx.refuse_codec_limit("write NX OM UUID identity", 0, 1))?;
    Ok(id)
}

fn uuid_record_id(
    ctx: &DecodeContext<'_>,
    section_ordinal: usize,
    record_ordinal: usize,
) -> Result<String, CodecError> {
    let length = "nx:om-record-directory-:entry#"
        .len()
        .checked_add(decimal_digits(section_ordinal))
        .and_then(|length| length.checked_add(decimal_digits(record_ordinal)))
        .ok_or_else(|| ctx.refuse_codec_limit("NX OM UUID record identity length", 0, 1))?;
    let mut id = ctx.retained_string(length, "retain NX OM UUID record identity")?;
    write!(
        id,
        "nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"
    )
    .map_err(|_| ctx.refuse_codec_limit("write NX OM UUID record identity", 0, 1))?;
    Ok(id)
}

/// Canonical UUID text spanning one or more contiguous bounded OM records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct ObjectUuidValue {
    /// Globally unique value identity.
    pub(in crate::native) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(in crate::native) section_ordinal: u32,
    /// Exact UUID text.
    pub(in crate::native) uuid: crate::canonical_uuid::CanonicalUuid<String>,
    /// Bounded OM records intersected by the complete UUID frame.
    #[serde(
        serialize_with = "serialize_records",
        deserialize_with = "deserialize_records"
    )]
    pub(in crate::native) records: NonEmpty<String>,
    /// Directory entry containing the OM section.
    pub(in crate::native) source_entry: String,
    /// Absolute file offset of the `03 26` marker.
    pub(in crate::native) source_offset: u64,
}

fn serialize_records<S: Serializer>(
    records: &NonEmpty<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(records.iter())
}

fn deserialize_records<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<NonEmpty<String>, D::Error> {
    NonEmpty::new(Vec::<String>::deserialize(deserializer)?).ok_or_else(|| {
        serde::de::Error::custom("records must contain at least one intersected record")
    })
}

/// Decode canonical UUID frames across the contiguous storage of ID-bounded
/// OM records. A value retains every physical record intersected by its frame.
pub(in crate::native) fn object_uuid_values(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<ObjectUuidValue>, CodecError> {
    const FRAME_LEN: usize = 2 + 36 + 1;
    let mut values = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections(ctx)?.into_iter().enumerate()
    {
        let Some(records) = section.as_fixed() else {
            continue;
        };
        let Some(first) = records.first() else {
            continue;
        };
        let Some(last) = records.last() else {
            continue;
        };
        if records.windows(2).any(|window| {
            window[0].offset.checked_add(window[0].bytes.len()) != Some(window[1].offset)
        }) {
            continue;
        }
        let Some(end) = last.offset.checked_add(last.bytes.len()) else {
            continue;
        };
        let Some((entry_offset, _)) = entry.file_span() else {
            continue;
        };
        let Ok(entry_offset_usize) = usize::try_from(entry_offset) else {
            continue;
        };
        let Some(storage_start) = entry_offset_usize.checked_add(first.offset) else {
            continue;
        };
        let Some(storage_end) = entry_offset_usize.checked_add(end) else {
            continue;
        };
        let Some(storage) = container.data.get(storage_start..storage_end) else {
            continue;
        };
        let section_ordinal_u32 = u32::try_from(section_ordinal)
            .map_err(|_| ctx.refuse_codec_limit("nx OM UUID section ordinal", 0, u64::MAX))?;
        for value in crate::om::uuid_string_values(ctx, storage, first.offset)? {
            let Some(frame_end) = value.offset.checked_add(FRAME_LEN) else {
                continue;
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(records.len()),
                "scan NX OM UUID record overlap",
            )?;
            let mut record_ids = Vec::new();
            for (record_ordinal, record) in records.iter().enumerate() {
                if record.offset >= frame_end
                    || record
                        .offset
                        .checked_add(record.bytes.len())
                        .is_none_or(|record_end| value.offset >= record_end)
                {
                    continue;
                }
                ctx.reserve_retained_vec(&mut record_ids, 1, "NX OM UUID records")?;
                record_ids.push(uuid_record_id(ctx, section_ordinal, record_ordinal)?);
            }
            let Some(records) = NonEmpty::from_vec(record_ids) else {
                continue;
            };
            let Some(source_offset) =
                entry_offset.checked_add(cadmpeg_core::decode::u64_from_index(value.offset))
            else {
                continue;
            };
            ctx.reserve_retained_vec(&mut values, 1, "NX OM UUID values")?;
            let id = uuid_value_id(ctx, section_ordinal, value.offset)?;
            let uuid = crate::canonical_uuid::CanonicalUuid::new(
                ctx.copy_retained_text(value.value.as_str(), "retain NX OM UUID text")?,
            )
            .map_err(|error| CodecError::InvalidInput(error.to_owned()))?;
            let source_entry =
                ctx.copy_retained_text(&entry.name, "retain NX OM UUID source entry")?;
            values.push(ObjectUuidValue {
                id,
                section_ordinal: section_ordinal_u32,
                uuid,
                records,
                source_entry,
                source_offset,
            });
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::{object_uuid_values, ObjectUuidValue};
    use crate::container::{Container, DirEntry, DirEntryBody, IndexedSectionCache, Region};
    use crate::om::{FixedEntityRecord, IndexedSection, IndexedStore};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::borrow::Cow;
    use std::collections::BTreeMap;
    use std::sync::{Arc, OnceLock};

    const UUID_FRAME: &[u8] = b"\x03\x2601234567-89ab-cdef-0123-456789abcdef\0";

    fn container() -> Container<'static> {
        let section = IndexedSection {
            base: 0,
            entity_index_offset: 0,
            object_id_table_offset: 0,
            types: Arc::from([]),
            fields: Arc::from([]),
            store: IndexedStore::Fixed {
                records: Arc::from([FixedEntityRecord {
                    object_id: (1, 0),
                    offset: 0,
                    bytes: UUID_FRAME,
                }]),
            },
        };
        let indexed_section_layouts = OnceLock::new();
        indexed_section_layouts
            .set(IndexedSectionCache::Borrowed {
                sections: vec![(0, section)],
                blocks: BTreeMap::new(),
            })
            .expect("test cache is empty");
        Container {
            data: Cow::Borrowed(UUID_FRAME),
            physical_size: cadmpeg_core::decode::u64_from_index(UUID_FRAME.len()),
            layout: crate::container::test_modern_layout(6),
            entries: vec![DirEntry {
                name: "/Root/UG_PART/UG_PART".to_owned(),
                region: Region::Header,
                body: DirEntryBody::File {
                    offset: 0,
                    len: cadmpeg_core::decode::u64_from_index(UUID_FRAME.len()),
                },
            }],
            fastload_table: None,
            indexed_section_layouts,
            om_section_cache: OnceLock::new(),
        }
    }

    fn assert_limit(configure: impl FnOnce(&mut DecodePolicy), dimension: ResourceDimension) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        configure(&mut policy);
        let (ctx, _) = DecodeContext::from_root_bytes(UUID_FRAME, &arena, &policy).unwrap();
        let error = object_uuid_values(&ctx, &container()).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
    }

    #[test]
    fn object_uuid_values_refuse_collection_limit() {
        assert_limit(
            |policy| policy.limits.max_collection_items = 0,
            ResourceDimension::CollectionItems,
        );
    }

    #[test]
    fn object_uuid_values_refuse_retained_limit() {
        assert_limit(
            |policy| policy.limits.max_retained_bytes = 0,
            ResourceDimension::RetainedBytes,
        );
    }

    #[test]
    fn object_uuid_values_refuse_work_limit() {
        assert_limit(
            |policy| policy.limits.max_work_units = 0,
            ResourceDimension::WorkUnits,
        );
    }

    #[test]
    fn object_uuid_values_preserve_wire_after_admission() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(UUID_FRAME, &arena, &policy).unwrap();
        let values = object_uuid_values(&ctx, &container()).unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(
            values[0].uuid.as_str(),
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        assert_eq!(
            values[0]
                .records
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["nx:om-record-directory-0:entry#0"]
        );
    }

    #[test]
    fn uuid_records_preserve_order_and_reject_empty_ownership() {
        let json = r#"{"id":"u","section_ordinal":0,"uuid":"00000000-0000-0000-0000-000000000000","records":["record#1","record#2"],"source_entry":"om","source_offset":10}"#;
        let value: ObjectUuidValue = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["records"] = serde_json::json!([]);
        assert!(serde_json::from_value::<ObjectUuidValue>(wire)
            .unwrap_err()
            .to_string()
            .contains("records"));
    }
}
