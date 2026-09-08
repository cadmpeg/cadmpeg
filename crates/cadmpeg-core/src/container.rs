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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Summary {
        entries: Vec<ContainerEntry>,
    }

    #[test]
    fn native_summary_labels_distinguish_ranges_storages_and_streams() {
        let rhino: Summary = serde_json::from_str(include_str!(
            "../../cadmpeg-codec-rhino/tests/golden/inspect/point.json"
        ))
        .expect("native summary witness");
        let table = rhino
            .entries
            .iter()
            .find(|entry| entry.role == ContainerRole::Table)
            .expect("native summary witness");
        assert_eq!(table.compression, EntryCompression::None);
        let body_offset: u64 = table.attributes["body_offset"]
            .parse()
            .expect("native summary witness");
        let offset: u64 = table.attributes["offset"]
            .parse()
            .expect("native summary witness");
        assert_eq!(
            table.compressed_size - table.uncompressed_size,
            body_offset - offset
        );
        assert!(body_offset > offset);

        let inventor: Summary = serde_json::from_str(include_str!(
            "../../cadmpeg-codec-inventor/tests/golden/inspect/structural.json"
        ))
        .expect("native summary witness");
        let storage = inventor
            .entries
            .iter()
            .find(|entry| entry.name == "RSeStorage")
            .expect("native summary witness");
        assert_eq!(storage.compression, EntryCompression::Storage);
        let stream = inventor
            .entries
            .iter()
            .find(|entry| entry.name == "RSeStorage/RSeSegInfo")
            .expect("native summary witness");
        assert_eq!(stream.compression, EntryCompression::Stored);
        assert_eq!(stream.compressed_size, stream.uncompressed_size);

        let step: Summary = serde_json::from_str(include_str!(
            "../../cadmpeg-codec-step/tests/golden/inspect/ap242_ed3_sections.json"
        ))
        .expect("native summary witness");
        let references = step
            .entries
            .iter()
            .find(|entry| entry.name == "REFERENCE")
            .expect("native summary witness");
        assert_eq!(references.role, ContainerRole::StepExternalReferences);
        assert_eq!(references.compression, EntryCompression::None);
        assert_eq!(references.attributes["external_count"], "1");
    }
}
