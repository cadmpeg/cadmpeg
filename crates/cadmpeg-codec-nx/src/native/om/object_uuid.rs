// SPDX-License-Identifier: Apache-2.0
//! UUID frames and the nonempty record set they intersect.

use crate::container::Container;
use crate::om::nonempty::NonEmpty;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
        container.indexed_om_sections().into_iter().enumerate()
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
        let section_ordinal_u32 = u32::try_from(section_ordinal).map_err(|_| {
            ctx.refuse_codec_limit("nx OM UUID section ordinal", 0, u64::MAX)
        })?;
        values.extend(
            crate::om::uuid_string_values(ctx, storage, first.offset)?
                .into_iter()
                .filter_map(|value| {
                    let frame_end = value.offset.checked_add(FRAME_LEN)?;
                    let records = NonEmpty::new(
                        records
                            .iter()
                            .enumerate()
                            .filter(|(_, record)| {
                                record.offset < frame_end
                                    && record
                                        .offset
                                        .checked_add(record.bytes.len())
                                        .is_some_and(|record_end| value.offset < record_end)
                            })
                            .map(|(record_ordinal, _)| {
                                format!(
                                    "nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"
                                )
                            }),
                    )?;
                    Some(ObjectUuidValue {
                        id: format!(
                            "nx:om-object-uuid-values-{section_ordinal}:value#{}",
                            value.offset
                        ),
                        section_ordinal: section_ordinal_u32,
                        uuid: value.value.into_owned(),
                        records,
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset
                            .checked_add(cadmpeg_core::decode::u64_from_index(value.offset))?,
                    })
                }),
        );
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::ObjectUuidValue;

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
