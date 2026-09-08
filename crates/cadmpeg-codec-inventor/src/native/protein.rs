// SPDX-License-Identifier: Apache-2.0
//! Protein state and its owned package entries on the native wire.

use cadmpeg_container::ZipCompression;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{de::Error as _, Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProteinRecord {
    Absent {
        id: String,
    },
    Empty {
        id: String,
        directory_id: u32,
    },
    Package {
        id: String,
        directory_id: u32,
        declared_len: std::num::NonZeroU32,
        entries: Vec<ProteinEntryRecord>,
    },
    Malformed {
        id: String,
        directory_id: u32,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProteinRecordState {
    Absent,
    Empty,
    Package,
    Malformed,
}

#[derive(Serialize, Deserialize)]
struct ProteinRecordWire {
    id: String,
    state: ProteinRecordState,
    directory_id: Option<u32>,
    declared_len: Option<u32>,
    entry_count: u64,
    detail: Option<String>,
}

impl Serialize for ProteinRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ProteinRecordWire::from(self).serialize(serializer)
    }
}

impl From<&ProteinRecord> for ProteinRecordWire {
    fn from(value: &ProteinRecord) -> Self {
        match value {
            ProteinRecord::Absent { id } => Self {
                id: id.clone(),
                state: ProteinRecordState::Absent,
                directory_id: None,
                declared_len: None,
                entry_count: 0,
                detail: None,
            },
            ProteinRecord::Empty { id, directory_id } => Self {
                id: id.clone(),
                state: ProteinRecordState::Empty,
                directory_id: Some(*directory_id),
                declared_len: Some(0),
                entry_count: 0,
                detail: None,
            },
            ProteinRecord::Package {
                id,
                directory_id,
                declared_len,
                entries,
            } => Self {
                id: id.clone(),
                state: ProteinRecordState::Package,
                directory_id: Some(*directory_id),
                declared_len: Some(declared_len.get()),
                entry_count: entries.len() as u64,
                detail: None,
            },
            ProteinRecord::Malformed {
                id,
                directory_id,
                detail,
            } => Self {
                id: id.clone(),
                state: ProteinRecordState::Malformed,
                directory_id: Some(*directory_id),
                declared_len: None,
                entry_count: 0,
                detail: Some(detail.clone()),
            },
        }
    }
}

impl ProteinRecord {
    pub(crate) fn entries(&self) -> &[ProteinEntryRecord] {
        match self {
            Self::Package { entries, .. } => entries,
            Self::Absent { .. } | Self::Empty { .. } | Self::Malformed { .. } => &[],
        }
    }

    pub(crate) fn install(
        &self,
        namespace: &mut NativeNamespace,
    ) -> Result<(), NativeConvertError> {
        namespace.set_arena("protein", std::slice::from_ref(self))?;
        namespace.set_arena("protein_entries", self.entries())
    }

    pub(crate) fn read(namespace: &NativeNamespace) -> Result<Self, NativeConvertError> {
        let [wire] = <[_; 1]>::try_from(namespace.arena_as::<ProteinRecordWire>("protein")?)
            .map_err(|records: Vec<_>| {
                serde_json::Error::custom(format!(
                    "Inventor native data has {} Protein state records",
                    records.len()
                ))
            })?;
        let entries = namespace.arena_as("protein_entries")?;
        wire.into_record(entries)
            .map_err(|detail| serde_json::Error::custom(detail).into())
    }
}

impl ProteinRecordWire {
    fn into_record(self, entries: Vec<ProteinEntryRecord>) -> Result<ProteinRecord, String> {
        if self.entry_count != entries.len() as u64 {
            return Err("Protein entry_count does not match its entry arena".into());
        }
        match self.state {
            ProteinRecordState::Absent
                if self.directory_id.is_none()
                    && self.declared_len.is_none()
                    && self.detail.is_none()
                    && entries.is_empty() =>
            {
                Ok(ProteinRecord::Absent { id: self.id })
            }
            ProteinRecordState::Empty
                if self.declared_len == Some(0) && self.detail.is_none() && entries.is_empty() =>
            {
                Ok(ProteinRecord::Empty {
                    id: self.id,
                    directory_id: self
                        .directory_id
                        .ok_or("empty Protein requires directory_id")?,
                })
            }
            ProteinRecordState::Package if self.detail.is_none() => Ok(ProteinRecord::Package {
                id: self.id,
                directory_id: self
                    .directory_id
                    .ok_or("Protein package requires directory_id")?,
                declared_len: std::num::NonZeroU32::new(self.declared_len.unwrap_or(0))
                    .ok_or("Protein package declared_len must be nonzero")?,
                entries,
            }),
            ProteinRecordState::Malformed if self.declared_len.is_none() && entries.is_empty() => {
                Ok(ProteinRecord::Malformed {
                    id: self.id,
                    directory_id: self
                        .directory_id
                        .ok_or("malformed Protein requires directory_id")?,
                    detail: self.detail.ok_or("malformed Protein requires detail")?,
                })
            }
            _ => Err("Protein state carries incompatible fields or entries".into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ZipCompression", rename_all = "lowercase")]
enum ZipCompressionWire {
    Stored,
    Deflate,
    Zstd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProteinEntryRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) name: String,
    #[serde(with = "ZipCompressionWire")]
    pub(crate) compression: ZipCompression,
    pub(crate) crc32: u32,
    pub(crate) compressed_size: u64,
    pub(crate) uncompressed_size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProteinAssetRecord {
    pub(crate) id: String,
    pub(crate) entry_name: String,
    pub(crate) ordinal: u64,
    pub(crate) asset: cadmpeg_protein::DecodedRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProteinRejectionRecord {
    pub(crate) id: String,
    pub(crate) entry_name: String,
    pub(crate) ordinal: u64,
    pub(crate) detail: String,
}

#[cfg(test)]
mod tests {
    use super::{ProteinEntryRecord, ProteinRecord};
    use cadmpeg_ir::native::NativeNamespace;
    use std::num::NonZeroU32;

    #[test]
    fn package_count_comes_from_owned_entries_and_the_wire_must_agree() {
        let record = ProteinRecord::Package {
            id: "inventor:protein:state#root".into(),
            directory_id: 3,
            declared_len: NonZeroU32::new(128).expect("valid test fixture"),
            entries: vec![ProteinEntryRecord {
                id: "inventor:protein:entry#0".into(),
                ordinal: 0,
                name: "asset.bin".into(),
                compression: super::ZipCompression::Stored,
                crc32: 0,
                compressed_size: 0,
                uncompressed_size: 0,
            }],
        };
        let mut namespace = NativeNamespace::default();
        record.install(&mut namespace).expect("valid test fixture");
        let mut wire = namespace
            .arena_as::<serde_json::Value>("protein")
            .expect("valid test fixture");
        assert_eq!(wire[0]["entry_count"], 1);
        assert_eq!(
            ProteinRecord::read(&namespace).expect("valid test fixture"),
            record
        );
        wire[0]["entry_count"] = serde_json::json!(0);
        namespace
            .set_arena("protein", &wire)
            .expect("valid test fixture");
        assert!(ProteinRecord::read(&namespace)
            .expect_err("invalid test fixture")
            .to_string()
            .contains("entry_count"));
        let absent = ProteinRecord::Absent {
            id: "inventor:protein:state#root".into(),
        };
        namespace
            .set_arena("protein", &[absent])
            .expect("valid test fixture");
        assert!(ProteinRecord::read(&namespace).is_err());
    }

    #[test]
    fn empty_and_malformed_states_have_no_package_entries() {
        for record in [
            ProteinRecord::Absent {
                id: "inventor:protein:state#root".into(),
            },
            ProteinRecord::Empty {
                id: "inventor:protein:state#root".into(),
                directory_id: 3,
            },
            ProteinRecord::Malformed {
                id: "inventor:protein:state#root".into(),
                directory_id: 3,
                detail: "truncated".into(),
            },
        ] {
            let mut namespace = NativeNamespace::default();
            record.install(&mut namespace).expect("valid test fixture");
            assert!(record.entries().is_empty());
            assert_eq!(
                ProteinRecord::read(&namespace).expect("valid test fixture"),
                record
            );
        }
    }
    #[test]
    fn protein_compression_wire_uses_the_archive_vocabulary() {
        for compression in [
            super::ZipCompression::Stored,
            super::ZipCompression::Deflate,
            super::ZipCompression::Zstd,
        ] {
            let entry = ProteinEntryRecord {
                id: "entry".into(),
                ordinal: 0,
                name: "asset.bin".into(),
                compression,
                crc32: 0,
                compressed_size: 0,
                uncompressed_size: 0,
            };
            let mut wire = serde_json::to_value(&entry).unwrap();
            assert_eq!(wire["compression"], compression.label());
            assert_eq!(
                serde_json::from_value::<ProteinEntryRecord>(wire.clone()).unwrap(),
                entry
            );
            wire["compression"] = serde_json::json!("banana");
            let mut namespace = NativeNamespace::default();
            namespace.set_arena("protein_entries", &[wire]).unwrap();
            assert!(namespace
                .arena_as::<ProteinEntryRecord>("protein_entries")
                .is_err());
        }
    }
}
