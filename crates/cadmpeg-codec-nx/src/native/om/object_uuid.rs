// SPDX-License-Identifier: Apache-2.0
//! UUID frames and the nonempty record set they intersect.

use crate::container::Container;
use crate::om::nonempty::NonEmpty;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Canonical UUID text spanning one or more contiguous bounded OM records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectUuidValue {
    /// Globally unique value identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Exact UUID text.
    pub uuid: crate::canonical_uuid::CanonicalUuid<String>,
    /// Bounded OM records intersected by the complete UUID frame.
    #[serde(
        serialize_with = "serialize_records",
        deserialize_with = "deserialize_records"
    )]
    pub records: NonEmpty<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the `03 26` marker.
    pub source_offset: u64,
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
pub fn object_uuid_values(container: &Container) -> Vec<ObjectUuidValue> {
    const FRAME_LEN: usize = 2 + 36 + 1;
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(records) = section.as_fixed() else {
                return Vec::new();
            };
            let Some(first) = records.first() else {
                return Vec::new();
            };
            let Some(last) = records.last() else {
                return Vec::new();
            };
            if records.windows(2).any(|window| {
                window[0].offset.checked_add(window[0].bytes.len()) != Some(window[1].offset)
            }) {
                return Vec::new();
            }
            let Some(end) = last.offset.checked_add(last.bytes.len()) else {
                return Vec::new();
            };
            let Some((entry_offset, _)) = entry.file_span() else {
                return Vec::new();
            };
            let Ok(entry_offset_usize) = usize::try_from(entry_offset) else {
                return Vec::new();
            };
            let Some(storage_start) = entry_offset_usize.checked_add(first.offset) else {
                return Vec::new();
            };
            let Some(storage_end) = entry_offset_usize.checked_add(end) else {
                return Vec::new();
            };
            let Some(storage) = container.data.get(storage_start..storage_end) else {
                return Vec::new();
            };
            crate::om::uuid_string_values(storage, first.offset)
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
                        section_ordinal: section_ordinal as u32,
                        uuid: value.value.into_owned(),
                        records,
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + value.offset as u64,
                    })
                })
                .collect()
        })
        .collect()
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
