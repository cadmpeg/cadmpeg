// SPDX-License-Identifier: Apache-2.0
//! Format-independent container entries.

use std::collections::BTreeMap;
use std::num::NonZeroU64;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! label_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        pub enum $name {
            $(
                #[doc = concat!("The `", $label, "` classification.")]
                #[serde(rename = $label)]
                $variant,
            )*
        }

        impl $name {
            /// Returns the stable summary label.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $label,)* }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

label_enum! {
    /// The closed semantic roles of container summary entries.
    ContainerRole {
        AcisBinary => "acis-binary",
        ActiveBodyIndex => "active-body-index",
        Ancillary => "ancillary",
        Arrangements => "arrangements",
        AssetCatalog => "asset-catalog",
        Auxiliary => "auxiliary",
        Block => "block",
        Brep => "brep",
        BrepSmb => "brep-smb",
        BrepSmbh => "brep-smbh",
        BrepText => "brep-text",
        Bulkstream => "bulkstream",
        CacheCell => "cache-cell",
        CompoundStream => "compound-stream",
        DesignConfig => "design-config",
        Directory => "directory",
        DirectoryEntry => "directory-entry",
        DisplayJt => "display-jt",
        Document => "document",
        EntityRecords => "entity-records",
        ExternalReference => "external-reference",
        ExternalReferences => "external-references",
        FastLoadJt => "fast-load-jt",
        FastLoadStructure => "fast-load-structure",
        FinjplSegment => "finjpl-segment",
        GuiDocument => "gui-document",
        Image => "image",
        InFileAnchors => "in-file-anchors",
        Manifest => "manifest",
        MaterialTexture => "material-texture",
        Metadata => "metadata",
        Metastream => "metastream",
        ModelData => "model-data",
        NamedOpaqueStream => "named-opaque-stream",
        NestedArchive => "nested-archive",
        ObjectClass => "object-class",
        OgsCache => "ogs-cache",
        Opaque => "opaque",
        Other => "other",
        Paramesh => "paramesh",
        ParasolidStream => "parasolid-stream",
        PartAttributes => "part-attributes",
        PartPayload => "part-payload",
        Preview => "preview",
        PreviewImage => "preview-image",
        Properties => "properties",
        Protein => "protein",
        ProteinAssets => "protein-assets",
        PsbGeometry => "psb-geometry",
        RetainedTrailingRecords => "retained-trailing-records",
        RootExchange => "root-exchange",
        RseDatabase => "rse-database",
        RseRevisionTable => "rse-revision-table",
        RseSegmentBulk => "rse-segment-bulk",
        RseSegmentMetadata => "rse-segment-metadata",
        RseSegmentRegistry => "rse-segment-registry",
        RseStorage => "rse-storage",
        SaveToggleInfo => "save-toggle-info",
        Section => "section",
        Signature => "signature",
        Storage => "storage",
        Stream => "stream",
        SubsidiaryExchange => "subsidiary-exchange",
        Table => "table",
        Thumbnail => "thumbnail",
    }
}

label_enum! {
    /// How a container spells "these bytes are stored verbatim".
    VerbatimLabel {
        None => "none",
        Stored => "stored",
    }
}

label_enum! {
    /// Compression methods reported by container summaries.
    CompressionMethod {
        Deflate => "deflate",
        Zstd => "zstd",
        Jpeg => "jpeg",
        UnixCompress => "unix-compress",
        Zlib => "zlib",
    }
}

/// What a container reports about the size of a verbatim payload.
///
/// Every value maps to a distinct declared `(stored, expanded)` pair, so an
/// unreported size is absent rather than spelled zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbatimSize {
    /// Neither the payload nor its stored span is reported.
    Unreported,
    /// The payload size; its stored span is not reported.
    PayloadOnly(NonZeroU64),
    /// The payload occupies exactly this many stored bytes.
    Exact(NonZeroU64),
    /// The payload, plus the container framing counted in its stored span.
    Framed {
        /// Payload size in bytes.
        payload: u64,
        /// Container framing counted in the stored span but not in the payload.
        framing: NonZeroU64,
    },
}

/// How one container summary entry stores its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryStorage {
    /// A container directory node that holds no bytes of its own.
    Directory,
    /// Bytes stored verbatim, with whatever the container reports about their size.
    Verbatim {
        /// How this container spells verbatim storage.
        label: VerbatimLabel,
        /// What the container reports about the size.
        size: VerbatimSize,
    },
    /// Bytes stored compressed; either size is absent when the codec does not report it.
    Compressed {
        /// Compression method.
        method: CompressionMethod,
        /// Stored size in bytes.
        stored: Option<u64>,
        /// Expanded size in bytes.
        expanded: Option<u64>,
    },
}

impl EntryStorage {
    /// Verbatim bytes occupying exactly `size` stored bytes.
    ///
    /// A zero `size` is the container reporting nothing, which is the only
    /// reading the wire admits: an empty payload and an unreported one are the
    /// same declared pair.
    #[must_use]
    pub fn verbatim(label: VerbatimLabel, size: u64) -> Self {
        Self::Verbatim {
            label,
            size: NonZeroU64::new(size).map_or(VerbatimSize::Unreported, VerbatimSize::Exact),
        }
    }

    /// Verbatim bytes whose payload is known and whose stored span is not reported.
    #[must_use]
    pub fn payload_only(label: VerbatimLabel, payload: u64) -> Self {
        Self::Verbatim {
            label,
            size: NonZeroU64::new(payload)
                .map_or(VerbatimSize::Unreported, VerbatimSize::PayloadOnly),
        }
    }

    /// Verbatim bytes whose stored span may include container framing.
    #[must_use]
    pub fn framed(label: VerbatimLabel, payload: u64, stored_span: u64) -> Self {
        match NonZeroU64::new(stored_span.saturating_sub(payload)) {
            Some(framing) => Self::Verbatim {
                label,
                size: VerbatimSize::Framed { payload, framing },
            },
            None => Self::verbatim(label, payload),
        }
    }

    /// Verbatim bytes whose size the container does not report.
    #[must_use]
    pub const fn unreported(label: VerbatimLabel) -> Self {
        Self::Verbatim {
            label,
            size: VerbatimSize::Unreported,
        }
    }

    /// Admit storage from a declared method label and the two declared sizes.
    ///
    /// A zero declared size means the producer did not report it. A verbatim
    /// entry whose stored span is smaller than its payload is rejected.
    pub fn from_declared(
        label: Result<VerbatimLabel, CompressionMethod>,
        stored: u64,
        expanded: u64,
    ) -> Result<Self, &'static str> {
        let label = match label {
            Ok(label) => label,
            Err(method) => {
                return Ok(Self::Compressed {
                    method,
                    stored: (stored != 0).then_some(stored),
                    expanded: (expanded != 0).then_some(expanded),
                })
            }
        };
        let size = match (NonZeroU64::new(stored), NonZeroU64::new(expanded)) {
            (None, None) => VerbatimSize::Unreported,
            (None, Some(payload)) => VerbatimSize::PayloadOnly(payload),
            (Some(stored), None) => VerbatimSize::Framed {
                payload: 0,
                framing: stored,
            },
            (Some(stored), Some(payload)) => {
                let Some(framing) = stored.get().checked_sub(payload.get()) else {
                    return Err("verbatim container entry stores fewer bytes than it expands to");
                };
                match NonZeroU64::new(framing) {
                    None => VerbatimSize::Exact(payload),
                    Some(framing) => VerbatimSize::Framed {
                        payload: payload.get(),
                        framing,
                    },
                }
            }
        };
        Ok(Self::Verbatim { label, size })
    }

    /// Stored span in bytes, absent when the container reports none.
    #[must_use]
    pub const fn stored_size(&self) -> Option<u64> {
        match self {
            Self::Directory => None,
            Self::Verbatim { size, .. } => match size {
                VerbatimSize::Unreported | VerbatimSize::PayloadOnly(_) => None,
                VerbatimSize::Exact(size) => Some(size.get()),
                VerbatimSize::Framed { payload, framing } => Some(*payload + framing.get()),
            },
            Self::Compressed { stored, .. } => *stored,
        }
    }

    /// Expanded size in bytes, absent when the container reports none.
    #[must_use]
    pub const fn expanded_size(&self) -> Option<u64> {
        match self {
            Self::Directory => None,
            Self::Verbatim { size, .. } => match size {
                VerbatimSize::Unreported => None,
                VerbatimSize::PayloadOnly(payload) | VerbatimSize::Exact(payload) => {
                    Some(payload.get())
                }
                VerbatimSize::Framed { payload, .. } => Some(*payload),
            },
            Self::Compressed { expanded, .. } => *expanded,
        }
    }
}

/// One stream or segment in a container summary.
///
/// `role` and `attributes` are codec-defined. The ordered attribute map keeps
/// the format-independent summary deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "ContainerEntryWire"))]
#[serde(try_from = "ContainerEntryWire", into = "ContainerEntryWire")]
pub struct ContainerEntry {
    /// Entry name/path within the container.
    pub name: String,
    /// Codec-defined role classification.
    pub role: ContainerRole,
    /// Byte storage of this entry.
    pub storage: EntryStorage,
    /// Extra codec-extracted attributes, sorted by key.
    pub attributes: BTreeMap<String, String>,
}

impl ContainerEntry {
    /// Stored span in bytes, absent when the entry reports none.
    #[must_use]
    pub const fn stored_size(&self) -> Option<u64> {
        self.storage.stored_size()
    }

    /// Expanded size in bytes, absent when the entry reports none.
    #[must_use]
    pub const fn expanded_size(&self) -> Option<u64> {
        self.storage.expanded_size()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
enum EntryCompressionWire {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "stored")]
    Stored,
    #[serde(rename = "deflate")]
    Deflate,
    #[serde(rename = "zstd")]
    Zstd,
    #[serde(rename = "storage")]
    Storage,
    #[serde(rename = "jpeg")]
    Jpeg,
    #[serde(rename = "unix-compress")]
    UnixCompress,
    #[serde(rename = "zlib")]
    Zlib,
}

impl EntryCompressionWire {
    fn label(self) -> Option<Result<VerbatimLabel, CompressionMethod>> {
        Some(match self {
            Self::None => Ok(VerbatimLabel::None),
            Self::Stored => Ok(VerbatimLabel::Stored),
            Self::Deflate => Err(CompressionMethod::Deflate),
            Self::Zstd => Err(CompressionMethod::Zstd),
            Self::Jpeg => Err(CompressionMethod::Jpeg),
            Self::UnixCompress => Err(CompressionMethod::UnixCompress),
            Self::Zlib => Err(CompressionMethod::Zlib),
            Self::Storage => return None,
        })
    }

    fn from_verbatim(label: VerbatimLabel) -> Self {
        match label {
            VerbatimLabel::None => Self::None,
            VerbatimLabel::Stored => Self::Stored,
        }
    }

    fn from_method(method: CompressionMethod) -> Self {
        match method {
            CompressionMethod::Deflate => Self::Deflate,
            CompressionMethod::Zstd => Self::Zstd,
            CompressionMethod::Jpeg => Self::Jpeg,
            CompressionMethod::UnixCompress => Self::UnixCompress,
            CompressionMethod::Zlib => Self::Zlib,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ContainerEntryWire {
    name: String,
    role: ContainerRole,
    compression: EntryCompressionWire,
    compressed_size: u64,
    uncompressed_size: u64,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
}

impl From<ContainerEntry> for ContainerEntryWire {
    fn from(entry: ContainerEntry) -> Self {
        let stored = entry.stored_size().unwrap_or(0);
        let expanded = entry.expanded_size().unwrap_or(0);
        let compression = match entry.storage {
            EntryStorage::Directory => EntryCompressionWire::Storage,
            EntryStorage::Verbatim { label, .. } => EntryCompressionWire::from_verbatim(label),
            EntryStorage::Compressed { method, .. } => EntryCompressionWire::from_method(method),
        };
        Self {
            name: entry.name,
            role: entry.role,
            compression,
            compressed_size: stored,
            uncompressed_size: expanded,
            attributes: entry.attributes,
        }
    }
}

impl TryFrom<ContainerEntryWire> for ContainerEntry {
    type Error = String;

    fn try_from(wire: ContainerEntryWire) -> Result<Self, Self::Error> {
        let storage = match wire.compression.label() {
            None => {
                if wire.compressed_size != 0 || wire.uncompressed_size != 0 {
                    return Err(
                        "container entry compression \"storage\" declares a byte size".to_string(),
                    );
                }
                EntryStorage::Directory
            }
            Some(label) => {
                EntryStorage::from_declared(label, wire.compressed_size, wire.uncompressed_size)
                    .map_err(str::to_string)?
            }
        };
        Ok(Self {
            name: wire.name,
            role: wire.role,
            storage,
            attributes: wire.attributes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompressionMethod, ContainerEntry, ContainerRole, EntryStorage, VerbatimLabel, VerbatimSize,
    };
    use std::num::NonZeroU64;

    fn wire(compression: &str, compressed: u64, uncompressed: u64) -> serde_json::Value {
        serde_json::json!({
            "name": "entry",
            "role": "stream",
            "compression": compression,
            "compressed_size": compressed,
            "uncompressed_size": uncompressed,
        })
    }

    fn admit(compression: &str, compressed: u64, uncompressed: u64) -> ContainerEntry {
        serde_json::from_value(wire(compression, compressed, uncompressed))
            .expect("the declared sizes are admissible")
    }

    fn reject(compression: &str, compressed: u64, uncompressed: u64) -> String {
        serde_json::from_value::<ContainerEntry>(wire(compression, compressed, uncompressed))
            .expect_err("the declared sizes are inadmissible")
            .to_string()
    }

    fn nonzero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).expect("a nonzero test size")
    }

    #[test]
    fn declared_verbatim_sizes_admit_one_shape_each() {
        for (compressed, uncompressed, expected) in [
            (0, 0, VerbatimSize::Unreported),
            (0, 12, VerbatimSize::PayloadOnly(nonzero(12))),
            (7, 7, VerbatimSize::Exact(nonzero(7))),
            (
                30,
                10,
                VerbatimSize::Framed {
                    payload: 10,
                    framing: nonzero(20),
                },
            ),
            (
                5,
                0,
                VerbatimSize::Framed {
                    payload: 0,
                    framing: nonzero(5),
                },
            ),
        ] {
            assert_eq!(
                EntryStorage::from_declared(Ok(VerbatimLabel::None), compressed, uncompressed),
                Ok(EntryStorage::Verbatim {
                    label: VerbatimLabel::None,
                    size: expected,
                })
            );
        }
    }

    #[test]
    fn declared_compressed_sizes_drop_the_unreported_zero() {
        assert_eq!(
            EntryStorage::from_declared(Err(CompressionMethod::Zlib), 0, 99),
            Ok(EntryStorage::Compressed {
                method: CompressionMethod::Zlib,
                stored: None,
                expanded: Some(99),
            })
        );
        assert_eq!(
            EntryStorage::from_declared(Err(CompressionMethod::Jpeg), 44, 0),
            Ok(EntryStorage::Compressed {
                method: CompressionMethod::Jpeg,
                stored: Some(44),
                expanded: None,
            })
        );
    }

    #[test]
    fn a_verbatim_entry_cannot_store_fewer_bytes_than_it_expands_to() {
        assert_eq!(
            EntryStorage::from_declared(Ok(VerbatimLabel::Stored), 5, 9),
            Err("verbatim container entry stores fewer bytes than it expands to")
        );
        for label in ["none", "stored"] {
            assert!(reject(label, 5, 9)
                .contains("verbatim container entry stores fewer bytes than it expands to"));
        }
    }

    #[test]
    fn a_storage_label_cannot_declare_a_byte_size() {
        for (compressed, uncompressed) in [(4, 0), (0, 7), (4, 7)] {
            assert!(reject("storage", compressed, uncompressed)
                .contains("container entry compression \"storage\" declares a byte size"));
        }
        assert_eq!(admit("storage", 0, 0).storage, EntryStorage::Directory);
    }

    #[test]
    fn every_legal_shape_round_trips_through_the_wire() {
        for (compression, compressed, uncompressed) in [
            ("storage", 0, 0),
            ("none", 0, 0),
            ("none", 0, 12),
            ("none", 7, 7),
            ("none", 30, 10),
            ("none", 5, 0),
            ("stored", 9, 9),
            ("deflate", 4, 16),
            ("zlib", 0, 99),
            ("jpeg", 44, 0),
            ("zstd", 8, 20),
            ("unix-compress", 15, 18),
        ] {
            let entry = admit(compression, compressed, uncompressed);
            let mut expected = wire(compression, compressed, uncompressed);
            expected["attributes"] = serde_json::json!({});
            assert_eq!(
                serde_json::to_value(&entry).expect("a container entry serializes"),
                expected,
                "{compression} {compressed}/{uncompressed}"
            );
        }
    }

    #[test]
    fn reported_sizes_are_absent_rather_than_zero() {
        assert_eq!(admit("none", 0, 0).stored_size(), None);
        assert_eq!(admit("none", 0, 0).expanded_size(), None);
        assert_eq!(admit("none", 0, 12).stored_size(), None);
        assert_eq!(admit("none", 0, 12).expanded_size(), Some(12));
        assert_eq!(admit("none", 30, 10).stored_size(), Some(30));
        assert_eq!(admit("none", 30, 10).expanded_size(), Some(10));
        assert_eq!(admit("zlib", 0, 99).stored_size(), None);
        let directory = admit("storage", 0, 0);
        assert_eq!(directory.stored_size(), None);
        assert_eq!(directory.expanded_size(), None);
        assert_eq!(directory.role, ContainerRole::Stream);
    }
}
