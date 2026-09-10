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

/// How the on-disk span of a verbatim payload relates to the payload itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbatimSpan {
    /// The stored span is exactly the payload.
    Payload,
    /// The stored span is the payload plus this much container framing.
    Framed(NonZeroU64),
    /// This codec does not report the stored span.
    Unreported,
}

/// How one container summary entry stores its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryStorage {
    /// A container directory node that holds no bytes of its own.
    Directory,
    /// Bytes stored verbatim: one payload size, plus how its stored span relates to it.
    Verbatim {
        /// How this container spells verbatim storage.
        label: VerbatimLabel,
        /// Payload size in bytes.
        payload: u64,
        /// Relation of the stored span to the payload.
        span: VerbatimSpan,
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
    /// Verbatim bytes whose stored span is exactly the payload.
    #[must_use]
    pub const fn verbatim(label: VerbatimLabel, payload: u64) -> Self {
        Self::Verbatim {
            label,
            payload,
            span: VerbatimSpan::Payload,
        }
    }

    /// Verbatim bytes whose stored span may include container framing.
    #[must_use]
    pub fn framed(label: VerbatimLabel, payload: u64, stored_span: u64) -> Self {
        Self::Verbatim {
            label,
            payload,
            span: match NonZeroU64::new(stored_span.saturating_sub(payload)) {
                Some(overhead) => VerbatimSpan::Framed(overhead),
                None => VerbatimSpan::Payload,
            },
        }
    }

    /// Verbatim bytes whose stored span this codec does not report.
    #[must_use]
    pub const fn unreported_span(label: VerbatimLabel, payload: u64) -> Self {
        Self::Verbatim {
            label,
            payload,
            span: VerbatimSpan::Unreported,
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
        match label {
            Ok(label) => {
                let span =
                    if stored == 0 && expanded != 0 {
                        VerbatimSpan::Unreported
                    } else if stored == expanded {
                        VerbatimSpan::Payload
                    } else {
                        match NonZeroU64::new(stored.wrapping_sub(expanded)) {
                            Some(overhead) if stored > expanded => VerbatimSpan::Framed(overhead),
                            _ => return Err(
                                "verbatim container entry stores fewer bytes than it expands to",
                            ),
                        }
                    };
                Ok(Self::Verbatim {
                    label,
                    payload: expanded,
                    span,
                })
            }
            Err(method) => Ok(Self::Compressed {
                method,
                stored: (stored != 0).then_some(stored),
                expanded: (expanded != 0).then_some(expanded),
            }),
        }
    }

    /// Stored span in bytes, absent when the entry holds no bytes or does not report it.
    #[must_use]
    pub const fn stored_size(&self) -> Option<u64> {
        match self {
            Self::Directory => None,
            Self::Verbatim { payload, span, .. } => match span {
                VerbatimSpan::Payload => Some(*payload),
                VerbatimSpan::Framed(overhead) => Some(*payload + overhead.get()),
                VerbatimSpan::Unreported => None,
            },
            Self::Compressed { stored, .. } => *stored,
        }
    }

    /// Expanded size in bytes, absent when the entry holds no bytes or does not report it.
    #[must_use]
    pub const fn expanded_size(&self) -> Option<u64> {
        match self {
            Self::Directory => None,
            Self::Verbatim { payload, .. } => Some(*payload),
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
