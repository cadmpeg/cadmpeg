// SPDX-License-Identifier: Apache-2.0
//! Protein state and its owned package entries on the native wire.

use cadmpeg_container::ZipCompression;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{de::Error as _, ser::SerializeStruct, Deserialize, Serialize};

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProteinRecordState {
    Absent,
    Empty,
    Package,
}

#[derive(Deserialize)]
struct ProteinRecordWire {
    id: String,
    state: ProteinRecordState,
    directory_id: Option<u32>,
    declared_len: Option<u32>,
    entry_count: u64,
}

impl Serialize for ProteinRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (id, state, directory_id, declared_len, entry_count) = match self {
            Self::Absent { id } => (id, ProteinRecordState::Absent, None, None, 0),
            Self::Empty { id, directory_id } => (
                id,
                ProteinRecordState::Empty,
                Some(*directory_id),
                Some(0),
                0,
            ),
            Self::Package {
                id,
                directory_id,
                declared_len,
                entries,
            } => (
                id,
                ProteinRecordState::Package,
                Some(*directory_id),
                Some(declared_len.get()),
                cadmpeg_core::decode::u64_from_index(entries.len()),
            ),
        };
        let mut fields = serializer.serialize_struct("ProteinRecordWire", 5)?;
        fields.serialize_field("id", id)?;
        fields.serialize_field("state", &state)?;
        fields.serialize_field("directory_id", &directory_id)?;
        fields.serialize_field("declared_len", &declared_len)?;
        fields.serialize_field("entry_count", &entry_count)?;
        fields.end()
    }
}

impl ProteinRecord {
    pub(crate) fn entries(&self) -> &[ProteinEntryRecord] {
        match self {
            Self::Package { entries, .. } => entries,
            Self::Absent { .. } | Self::Empty { .. } => &[],
        }
    }

    pub(crate) fn install(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        namespace: &mut NativeNamespace,
    ) -> Result<(), NativeConvertError> {
        namespace.set_arena(ctx, "protein", std::slice::from_ref(self))?;
        namespace.set_arena(ctx, "protein_entries", self.entries())
    }

    pub(crate) fn read(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        namespace: &NativeNamespace,
    ) -> Result<Self, NativeConvertError> {
        let records = namespace.arena_as_for_decode::<ProteinRecordWire>(ctx, "protein")?;
        let Some([wire]) = <[_; 1]>::try_from(records)
            .map_err(|records: Vec<_>| {
                serde_json::Error::custom(format!(
                    "Inventor native data has {} Protein state records",
                    records.len()
                ))
            })
            .ok()
        else {
            return Ok(Self::Absent {
                id: "inventor:protein:state#root".to_owned(),
            });
        };
        let entries = namespace.arena_as_for_decode(ctx, "protein_entries")?;
        wire.into_record(entries)
            .map_err(|detail| serde_json::Error::custom(detail).into())
    }
}

impl ProteinRecordWire {
    fn into_record(self, entries: Vec<ProteinEntryRecord>) -> Result<ProteinRecord, String> {
        if self.entry_count != cadmpeg_core::decode::u64_from_index(entries.len()) {
            return Err("Protein entry_count does not match its entry arena".into());
        }
        match self.state {
            ProteinRecordState::Absent
                if self.directory_id.is_none()
                    && self.declared_len.is_none()
                    && entries.is_empty() =>
            {
                Ok(ProteinRecord::Absent { id: self.id })
            }
            ProteinRecordState::Empty if self.declared_len == Some(0) && entries.is_empty() => {
                Ok(ProteinRecord::Empty {
                    id: self.id,
                    directory_id: self
                        .directory_id
                        .ok_or("empty Protein requires directory_id")?,
                })
            }
            ProteinRecordState::Package => Ok(ProteinRecord::Package {
                id: self.id,
                directory_id: self
                    .directory_id
                    .ok_or("Protein package requires directory_id")?,
                declared_len: std::num::NonZeroU32::new(self.declared_len.unwrap_or(0))
                    .ok_or("Protein package declared_len must be nonzero")?,
                entries,
            }),
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
    Unsupported(u16),
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "ProteinAssetRecordWire")]
pub(crate) struct ProteinAssetRecord {
    pub(crate) id: String,
    pub(crate) entry_name: InstancePropertiesEntry,
    asset: cadmpeg_protein::DecodedRecord,
}

impl Serialize for ProteinAssetRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_struct("ProteinAssetRecordWire", 4)?;
        fields.serialize_field("id", &self.id)?;
        fields.serialize_field("entry_name", self.entry_name.as_str())?;
        fields.serialize_field("ordinal", &self.asset.ordinal)?;
        fields.serialize_field("asset", &self.asset)?;
        fields.end()
    }
}

#[derive(Deserialize)]
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

impl ProteinAssetRecord {
    pub(crate) fn ordinal(&self) -> u64 {
        self.asset.ordinal
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct InstancePropertiesEntry(String);

impl Serialize for InstancePropertiesEntry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl TryFrom<String> for InstancePropertiesEntry {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !value.ends_with("InstanceProperties.bin") {
            return Err("entry_name must end with InstanceProperties.bin");
        }
        Ok(Self(value))
    }
}

impl InstancePropertiesEntry {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{ProteinAssetRecord, ProteinEntryRecord, ProteinRecord};
    use cadmpeg_ir::native::NativeNamespace;
    use cadmpeg_test_support::native_serialization::assert_native_limit;
    use std::num::NonZeroU32;

    #[test]
    fn protein_package_streams_once_with_retained_limit() {
        let record = ProteinRecord::Package {
            id: "inventor:protein:state#root".into(),
            directory_id: 3,
            declared_len: NonZeroU32::new(128).expect("valid fixture"),
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
        assert_native_limit(
            &record,
            serde_json::json!({
                "id": "inventor:protein:state#root", "state": "package",
                "directory_id": 3, "declared_len": 128, "entry_count": 1,
            }),
        );
    }

    #[test]
    fn protein_asset_streams_once_with_retained_limit() {
        let expected = serde_json::json!({
            "id": "inventor:protein:asset#0", "entry_name": "assets/InstanceProperties.bin", "ordinal": 3,
            "asset": { "ordinal": 3, "logical_offset": 0, "schema": "GenericSchema",
                "guid": "asset-guid", "base": "", "asset_lib_id": "", "properties": {} }
        });
        let record: ProteinAssetRecord =
            serde_json::from_value(expected.clone()).expect("valid fixture");
        assert_native_limit(&record, expected);
    }

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
        for entry_name in ["", "InstanceProperties.bin.bak", "instanceproperties.bin"] {
            let mut wire = asset.clone();
            wire["entry_name"] = serde_json::json!(entry_name);
            assert!(serde_json::from_value::<ProteinAssetRecord>(wire)
                .expect_err("invalid entry")
                .to_string()
                .contains("entry_name"));
        }
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
        record
            .install(&crate::native::test_ctx(), &mut namespace)
            .expect("valid test fixture");
        let mut wire = namespace
            .arena_as::<serde_json::Value>("protein")
            .expect("valid test fixture");
        assert_eq!(wire[0]["entry_count"], 1);
        assert_eq!(
            ProteinRecord::read(&crate::native::test_ctx(), &namespace)
                .expect("valid test fixture"),
            record
        );
        wire[0]["entry_count"] = serde_json::json!(0);
        namespace
            .set_arena(&crate::native::test_ctx(), "protein", &wire)
            .expect("valid test fixture");
        assert!(ProteinRecord::read(&crate::native::test_ctx(), &namespace)
            .expect_err("invalid test fixture")
            .to_string()
            .contains("entry_count"));
        let absent = ProteinRecord::Absent {
            id: "inventor:protein:state#root".into(),
        };
        namespace
            .set_arena(&crate::native::test_ctx(), "protein", &[absent])
            .expect("valid test fixture");
        assert!(ProteinRecord::read(&crate::native::test_ctx(), &namespace).is_err());
    }

    #[test]
    fn empty_and_absent_states_have_no_package_entries() {
        for record in [
            ProteinRecord::Absent {
                id: "inventor:protein:state#root".into(),
            },
            ProteinRecord::Empty {
                id: "inventor:protein:state#root".into(),
                directory_id: 3,
            },
        ] {
            let mut namespace = NativeNamespace::default();
            record
                .install(&crate::native::test_ctx(), &mut namespace)
                .expect("valid test fixture");
            assert!(record.entries().is_empty());
            assert_eq!(
                ProteinRecord::read(&crate::native::test_ctx(), &namespace)
                    .expect("valid test fixture"),
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
                .set_arena(&crate::native::test_ctx(), "protein_entries", &[wire])
                .expect("Protein entry fixture serializes");
            assert!(namespace
                .arena_as::<ProteinEntryRecord>("protein_entries")
                .is_err());
        }
    }
}
