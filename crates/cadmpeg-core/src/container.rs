// SPDX-License-Identifier: Apache-2.0
//! Format-independent container entries.

use std::collections::BTreeMap;

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
    /// Compression and storage labels reported by container summaries.
    EntryCompression {
        None => "none",
        Stored => "stored",
        Deflate => "deflate",
        Zstd => "zstd",
        Jpeg => "jpeg",
        UnixCompress => "unix-compress",
        Zlib => "zlib",
    }
}

/// How one container summary entry stores its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryStorage {
    /// A container directory node that holds no bytes of its own.
    Directory,
    /// A byte payload with its stored and expanded sizes.
    Bytes {
        /// Compression method label (for example, `"stored"` or `"deflate"`).
        compression: EntryCompression,
        /// Stored size in bytes.
        compressed_size: u64,
        /// Expanded size in bytes.
        uncompressed_size: u64,
    },
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
    /// Compression method of the stored bytes, absent for a directory node.
    #[must_use]
    pub const fn compression(&self) -> Option<EntryCompression> {
        match self.storage {
            EntryStorage::Directory => None,
            EntryStorage::Bytes { compression, .. } => Some(compression),
        }
    }

    /// Stored size in bytes, absent for a directory node.
    #[must_use]
    pub const fn compressed_size(&self) -> Option<u64> {
        match self.storage {
            EntryStorage::Directory => None,
            EntryStorage::Bytes {
                compressed_size, ..
            } => Some(compressed_size),
        }
    }

    /// Expanded size in bytes, absent for a directory node.
    #[must_use]
    pub const fn expanded_size(&self) -> Option<u64> {
        match self.storage {
            EntryStorage::Directory => None,
            EntryStorage::Bytes {
                uncompressed_size, ..
            } => Some(uncompressed_size),
        }
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
    fn method(self) -> Option<EntryCompression> {
        Some(match self {
            Self::None => EntryCompression::None,
            Self::Stored => EntryCompression::Stored,
            Self::Deflate => EntryCompression::Deflate,
            Self::Zstd => EntryCompression::Zstd,
            Self::Jpeg => EntryCompression::Jpeg,
            Self::UnixCompress => EntryCompression::UnixCompress,
            Self::Zlib => EntryCompression::Zlib,
            Self::Storage => return None,
        })
    }

    fn from_method(compression: EntryCompression) -> Self {
        match compression {
            EntryCompression::None => Self::None,
            EntryCompression::Stored => Self::Stored,
            EntryCompression::Deflate => Self::Deflate,
            EntryCompression::Zstd => Self::Zstd,
            EntryCompression::Jpeg => Self::Jpeg,
            EntryCompression::UnixCompress => Self::UnixCompress,
            EntryCompression::Zlib => Self::Zlib,
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
        let (compression, compressed_size, uncompressed_size) = match entry.storage {
            EntryStorage::Directory => (EntryCompressionWire::Storage, 0, 0),
            EntryStorage::Bytes {
                compression,
                compressed_size,
                uncompressed_size,
            } => (
                EntryCompressionWire::from_method(compression),
                compressed_size,
                uncompressed_size,
            ),
        };
        Self {
            name: entry.name,
            role: entry.role,
            compression,
            compressed_size,
            uncompressed_size,
            attributes: entry.attributes,
        }
    }
}

impl TryFrom<ContainerEntryWire> for ContainerEntry {
    type Error = String;

    fn try_from(wire: ContainerEntryWire) -> Result<Self, Self::Error> {
        let storage = match wire.compression.method() {
            None => {
                if wire.compressed_size != 0 || wire.uncompressed_size != 0 {
                    return Err(
                        "container entry compression \"storage\" declares a byte size".to_string(),
                    );
                }
                EntryStorage::Directory
            }
            Some(compression) => EntryStorage::Bytes {
                compression,
                compressed_size: wire.compressed_size,
                uncompressed_size: wire.uncompressed_size,
            },
        };
        Ok(Self {
            name: wire.name,
            role: wire.role,
            storage,
            attributes: wire.attributes,
        })
    }
}
