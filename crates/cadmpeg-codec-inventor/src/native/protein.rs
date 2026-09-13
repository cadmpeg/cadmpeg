// SPDX-License-Identifier: Apache-2.0
//! Protein state and its owned package entries on the native wire.

use cadmpeg_container::ZipCompression;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use cadmpeg_ir::products::NonBlankString;
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
#[serde(try_from = "ProteinAssetRecordWire", into = "ProteinAssetRecordWire")]
pub(crate) struct ProteinAssetRecord {
    pub(crate) id: String,
    pub(crate) entry_name: InstancePropertiesEntry,
    pub(crate) asset: cadmpeg_protein::DecodedRecord,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ProteinAssetRecordWire {
    pub(crate) id: String,
    pub(crate) entry_name: String,
    pub(crate) ordinal: u64,
    pub(crate) asset: cadmpeg_protein::DecodedRecord,
}

impl TryFrom<ProteinAssetRecordWire> for ProteinAssetRecord {
    type Error = String;
    fn try_from(wire: ProteinAssetRecordWire) -> Result<Self, Self::Error> {
        if wire.ordinal != wire.asset.ordinal {
            return Err("ordinal disagrees with asset.ordinal".into());
        }
        Ok(Self {
            id: wire.id,
            entry_name: InstancePropertiesEntry::try_from(wire.entry_name)?,
            asset: wire.asset,
        })
    }
}

impl From<ProteinAssetRecord> for ProteinAssetRecordWire {
    fn from(value: ProteinAssetRecord) -> Self {
        let ordinal = value.ordinal();
        Self {
            id: value.id,
            entry_name: value.entry_name.into(),
            ordinal,
            asset: value.asset,
        }
    }
}

impl ProteinAssetRecord {
    pub(crate) fn ordinal(&self) -> u64 {
        self.asset.ordinal
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "ProteinRejectionRecordWire",
    into = "ProteinRejectionRecordWire"
)]
pub(crate) struct ProteinRejectionRecord {
    pub(crate) id: String,
    pub(crate) entry_name: InstancePropertiesEntry,
    pub(crate) ordinal: u64,
    detail: NonBlankString,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ProteinRejectionRecordWire {
    pub(crate) id: String,
    pub(crate) entry_name: String,
    pub(crate) ordinal: u64,
    pub(crate) detail: String,
}

impl TryFrom<ProteinRejectionRecordWire> for ProteinRejectionRecord {
    type Error = String;
    fn try_from(wire: ProteinRejectionRecordWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            entry_name: InstancePropertiesEntry::try_from(wire.entry_name)?,
            ordinal: wire.ordinal,
            detail: NonBlankString::new(wire.detail).ok_or("detail must not be empty")?,
        })
    }
}

impl From<ProteinRejectionRecord> for ProteinRejectionRecordWire {
    fn from(value: ProteinRejectionRecord) -> Self {
        Self {
            id: value.id,
            entry_name: value.entry_name.into(),
            ordinal: value.ordinal,
            detail: value.detail.as_str().to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct InstancePropertiesEntry(String);

impl TryFrom<String> for InstancePropertiesEntry {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !value.ends_with("InstanceProperties.bin") {
            return Err("entry_name must end with InstanceProperties.bin");
        }
        Ok(Self(value))
    }
}

impl From<InstancePropertiesEntry> for String {
    fn from(value: InstancePropertiesEntry) -> Self {
        value.0
    }
}

impl InstancePropertiesEntry {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{ProteinAssetRecord, ProteinEntryRecord, ProteinRecord, ProteinRejectionRecord};
    use cadmpeg_ir::native::NativeNamespace;
    use std::num::NonZeroU32;

    #[test]
    fn asset_and_rejection_admission_preserves_positions_and_entry_names() {
        let asset = serde_json::json!({
            "id": "asset", "entry_name": "assets/InstanceProperties.bin", "ordinal": 3,
            "asset": { "ordinal": 3, "logical_offset": 0, "schema": "GenericSchema",
                "guid": "asset-guid", "base": "", "asset_lib_id": "", "properties": {} }
        });
        let mut admitted: ProteinAssetRecord =
            serde_json::from_value(asset.clone()).expect("valid asset");
        assert_eq!(serde_json::to_value(&admitted).expect("valid asset"), asset);
        admitted.asset.ordinal = 4;
        assert_eq!(admitted.ordinal(), 4);
        let wire = serde_json::to_value(admitted).expect("valid asset");
        assert_eq!(wire["ordinal"], 4);
        assert_eq!(wire["asset"]["ordinal"], 4);
        let mut inconsistent = asset.clone();
        inconsistent["ordinal"] = serde_json::json!(4);
        assert!(serde_json::from_value::<ProteinAssetRecord>(inconsistent)
            .expect_err("inconsistent ordinal")
            .to_string()
            .contains("ordinal"));
        let rejection = serde_json::json!({
            "id": "rejection", "entry_name": "InstanceProperties.bin", "ordinal": 0, "detail": "unsupported schema"
        });
        let admitted: ProteinRejectionRecord =
            serde_json::from_value(rejection.clone()).expect("valid rejection");
        assert_eq!(
            serde_json::to_value(admitted).expect("valid rejection"),
            rejection
        );
        for entry_name in ["", "InstanceProperties.bin.bak", "instanceproperties.bin"] {
            let mut wire = asset.clone();
            wire["entry_name"] = serde_json::json!(entry_name);
            assert!(serde_json::from_value::<ProteinAssetRecord>(wire)
                .expect_err("invalid entry")
                .to_string()
                .contains("entry_name"));
            let mut wire = rejection.clone();
            wire["entry_name"] = serde_json::json!(entry_name);
            assert!(serde_json::from_value::<ProteinRejectionRecord>(wire)
                .expect_err("invalid entry")
                .to_string()
                .contains("entry_name"));
        }
        let mut wire = rejection;
        wire["detail"] = serde_json::json!("");
        assert!(serde_json::from_value::<ProteinRejectionRecord>(wire)
            .expect_err("empty detail")
            .to_string()
            .contains("detail"));
    }

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
                id: "inventor:protein:entry#0".into(),
                ordinal: 0,
                name: "asset.bin".into(),
                compression,
                crc32: 0,
                compressed_size: 0,
                uncompressed_size: 0,
            };
            let mut wire = serde_json::to_value(&entry).expect("Protein entry fixture serializes");
            assert_eq!(wire["compression"], compression.label());
            assert_eq!(
                serde_json::from_value::<ProteinEntryRecord>(wire.clone())
                    .expect("Protein entry fixture serializes"),
                entry
            );
            wire["compression"] = serde_json::json!("banana");
            let mut namespace = NativeNamespace::default();
            namespace
                .set_arena("protein_entries", &[wire])
                .expect("Protein entry fixture serializes");
            assert!(namespace
                .arena_as::<ProteinEntryRecord>("protein_entries")
                .is_err());
        }
    }
}
