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
        EntityRecords => "entity_records",
        ExternalReference => "external-reference",
        ExternalReferences => "external-references",
        StepExternalReferences => "external_references",
        FastLoadJt => "fast-load-jt",
        FastLoadStructure => "fast-load-structure",
        FinjplSegment => "finjpl-segment",
        GuiDocument => "gui-document",
        Image => "image",
        InFileAnchors => "in_file_anchors",
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
        Storage => "storage",
        Jpeg => "jpeg",
        UnixCompress => "unix-compress",
        CompoundFile => "compound-file",
        Zlib => "zlib",
    }
}

/// One stream or segment in a container summary.
///
/// `role` and `attributes` are codec-defined. The ordered attribute map keeps
/// the format-independent summary deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ContainerEntry {
    /// Entry name/path within the container.
    pub name: String,
    /// Codec-defined role classification.
    pub role: ContainerRole,
    /// Compression method label (for example, `"stored"` or `"deflate"`).
    pub compression: EntryCompression,
    /// Compressed size in bytes.
    pub compressed_size: u64,
    /// Uncompressed size in bytes.
    pub uncompressed_size: u64,
    /// Extra codec-extracted attributes, sorted by key.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}
