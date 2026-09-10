// SPDX-License-Identifier: Apache-2.0
//! Format-independent container entries.

use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};

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

/// Rejection for a verbatim entry whose stored span is smaller than its payload.
const VERBATIM_SPAN_UNDER_PAYLOAD: &str =
    "verbatim container entry stores fewer bytes than it expands to";

/// What a container reports about the size of a verbatim payload.
///
/// Every value maps to a distinct declared `(stored, expanded)` pair, and an
/// unreported size is absent on the wire, so a reported zero is an ordinary
/// size rather than a second spelling of [`VerbatimSize::Unreported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbatimSize {
    /// Neither the payload nor its stored span is reported.
    Unreported,
    /// The payload size; its stored span is not reported.
    PayloadOnly(u64),
    /// The stored span; the payload it expands to is not reported.
    StoredOnly(u64),
    /// The payload occupies exactly this many stored bytes.
    Exact(u64),
    /// The payload, plus the container framing counted in its stored span.
    Framed(FramedSpan),
}

/// The byte length of a live allocation.
///
/// Rust guarantees no allocation exceeds `isize::MAX` bytes, so a value read
/// off a slice or a `str` carries that bound with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocatedLen(usize);

impl From<&[u8]> for AllocatedLen {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.len())
    }
}

impl From<&str> for AllocatedLen {
    fn from(text: &str) -> Self {
        Self(text.len())
    }
}

/// The widening in [`FramedSpan::from_parts`] is exact on this target.
const _: () = assert!(usize::BITS <= 64);
/// The stored span in [`FramedSpan::stored`] cannot overflow a `u64`.
const _: () = assert!((isize::MAX as u128) + (u32::MAX as u128) < (u64::MAX as u128));

/// A verbatim payload inside a strictly larger stored span.
///
/// The framing is the declared number and it is non-zero, so a stored span
/// that does not exceed its payload is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramedSpan {
    payload: u64,
    framing: NonZeroU64,
    stored: NonZeroU64,
}

impl FramedSpan {
    /// A `payload`-byte payload wrapped in `framing` bytes of container framing.
    ///
    /// `payload` is the length of a live allocation and so at most `isize::MAX`
    /// and `framing` at most `u32::MAX`, and the module assertions above prove
    /// that sum inside `u64`, so the absent arm never fires.
    #[must_use]
    pub fn from_parts(payload: AllocatedLen, framing: NonZeroU32) -> Option<Self> {
        let payload = payload.0 as u64;
        Self::new(payload, NonZeroU64::from(framing).checked_add(payload)?)
    }

    /// A payload of `payload` bytes inside a `stored`-byte span, absent when the
    /// span does not exceed the payload.
    #[must_use]
    pub const fn new(payload: u64, stored: NonZeroU64) -> Option<Self> {
        match stored.get().checked_sub(payload) {
            Some(framing) => match NonZeroU64::new(framing) {
                Some(framing) => Some(Self {
                    payload,
                    framing,
                    stored,
                }),
                None => None,
            },
            None => None,
        }
    }

    /// Payload size in bytes.
    #[must_use]
    pub const fn payload(self) -> u64 {
        self.payload
    }

    /// Stored span in bytes, framing included: at least `payload + 1`, since
    /// the framing is non-zero.
    #[must_use]
    pub const fn stored(self) -> NonZeroU64 {
        self.stored
    }

    /// Container framing counted in the stored span but not in the payload.
    #[must_use]
    pub const fn framing(self) -> NonZeroU64 {
        self.framing
    }
}

impl VerbatimSize {
    /// The shape declared by a `payload`-byte payload in a `stored`-byte span,
    /// absent when the span is smaller than the payload.
    #[must_use]
    pub const fn declared(payload: u64, stored: u64) -> Option<Self> {
        if stored < payload {
            return None;
        }
        match NonZeroU64::new(stored) {
            None => Some(Self::Exact(0)),
            Some(stored) => match FramedSpan::new(payload, stored) {
                Some(span) => Some(Self::Framed(span)),
                None => Some(Self::Exact(payload)),
            },
        }
    }
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
    ///
    /// The two sizes are independent facts: a compressed stream may occupy
    /// fewer, the same, or more stored bytes than it expands to (an
    /// incompressible deflate member grows), so no pair of reported sizes is
    /// invalid and none is a second spelling of another.
    Compressed {
        /// Compression method.
        method: CompressionMethod,
        /// Stored size in bytes, absent when the codec does not report it.
        stored: Option<u64>,
        /// Expanded size in bytes, absent when the codec does not report it.
        expanded: Option<u64>,
    },
}

impl EntryStorage {
    /// Verbatim bytes occupying exactly `size` stored bytes.
    #[must_use]
    pub const fn verbatim(label: VerbatimLabel, size: u64) -> Self {
        Self::Verbatim {
            label,
            size: VerbatimSize::Exact(size),
        }
    }

    /// Verbatim bytes whose payload is known and whose stored span is not reported.
    #[must_use]
    pub const fn payload_only(label: VerbatimLabel, payload: u64) -> Self {
        Self::Verbatim {
            label,
            size: VerbatimSize::PayloadOnly(payload),
        }
    }

    /// Verbatim bytes whose stored span may include container framing.
    ///
    /// A stored span smaller than the payload is rejected with the message
    /// [`EntryStorage::from_declared`] uses.
    pub fn framed(
        label: VerbatimLabel,
        payload: u64,
        stored_span: u64,
    ) -> Result<Self, &'static str> {
        VerbatimSize::declared(payload, stored_span)
            .map(|size| Self::Verbatim { label, size })
            .ok_or(VERBATIM_SPAN_UNDER_PAYLOAD)
    }

    /// Verbatim bytes whose stored span is the payload plus `framing` bytes of
    /// container framing the producer knows.
    ///
    /// Absent when the stored span would not fit a `u64`.
    #[must_use]
    pub fn framed_by(
        label: VerbatimLabel,
        payload: AllocatedLen,
        framing: NonZeroU32,
    ) -> Option<Self> {
        Some(Self::Verbatim {
            label,
            size: VerbatimSize::Framed(FramedSpan::from_parts(payload, framing)?),
        })
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
    /// An absent declared size is one the producer did not report; a zero one
    /// is a reported zero. A verbatim entry whose stored span is smaller than
    /// its payload is rejected.
    pub fn from_declared(
        label: Result<VerbatimLabel, CompressionMethod>,
        stored: Option<u64>,
        expanded: Option<u64>,
    ) -> Result<Self, &'static str> {
        let label = match label {
            Ok(label) => label,
            Err(method) => {
                return Ok(Self::Compressed {
                    method,
                    stored,
                    expanded,
                })
            }
        };
        let size = match (stored, expanded) {
            (None, None) => VerbatimSize::Unreported,
            (None, Some(payload)) => VerbatimSize::PayloadOnly(payload),
            (Some(stored), None) => VerbatimSize::StoredOnly(stored),
            (Some(stored), Some(payload)) => {
                VerbatimSize::declared(payload, stored).ok_or(VERBATIM_SPAN_UNDER_PAYLOAD)?
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
                VerbatimSize::StoredOnly(size) | VerbatimSize::Exact(size) => Some(*size),
                VerbatimSize::Framed(span) => Some(span.stored().get()),
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
                VerbatimSize::Unreported | VerbatimSize::StoredOnly(_) => None,
                VerbatimSize::PayloadOnly(payload) | VerbatimSize::Exact(payload) => Some(*payload),
                VerbatimSize::Framed(span) => Some(span.payload()),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compressed_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uncompressed_size: Option<u64>,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
}

impl From<ContainerEntry> for ContainerEntryWire {
    fn from(entry: ContainerEntry) -> Self {
        let stored = entry.stored_size();
        let expanded = entry.expanded_size();
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
                if wire.compressed_size.is_some() || wire.uncompressed_size.is_some() {
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
    use std::collections::BTreeMap;
    use std::num::{NonZeroU32, NonZeroU64};

    fn wire(
        compression: &str,
        compressed: Option<u64>,
        uncompressed: Option<u64>,
    ) -> serde_json::Value {
        let mut value = serde_json::json!({
            "name": "entry",
            "role": "stream",
            "compression": compression,
        });
        if let Some(compressed) = compressed {
            value["compressed_size"] = compressed.into();
        }
        if let Some(uncompressed) = uncompressed {
            value["uncompressed_size"] = uncompressed.into();
        }
        value
    }

    fn admit(
        compression: &str,
        compressed: Option<u64>,
        uncompressed: Option<u64>,
    ) -> ContainerEntry {
        serde_json::from_value(wire(compression, compressed, uncompressed))
            .expect("the declared sizes are admissible")
    }

    fn reject(compression: &str, compressed: Option<u64>, uncompressed: Option<u64>) -> String {
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
            (None, None, VerbatimSize::Unreported),
            (None, Some(12), VerbatimSize::PayloadOnly(12)),
            (None, Some(0), VerbatimSize::PayloadOnly(0)),
            (Some(5), None, VerbatimSize::StoredOnly(5)),
            (Some(0), None, VerbatimSize::StoredOnly(0)),
            (Some(7), Some(7), VerbatimSize::Exact(7)),
            (Some(0), Some(0), VerbatimSize::Exact(0)),
            (
                Some(30),
                Some(10),
                VerbatimSize::declared(10, 30).expect("a framed span"),
            ),
            (
                Some(5),
                Some(0),
                VerbatimSize::declared(0, 5).expect("a framed span"),
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
    fn declared_compressed_sizes_keep_absence_and_zero_apart() {
        assert_eq!(
            EntryStorage::from_declared(Err(CompressionMethod::Zlib), None, Some(99)),
            Ok(EntryStorage::Compressed {
                method: CompressionMethod::Zlib,
                stored: None,
                expanded: Some(99),
            })
        );
        assert_eq!(
            EntryStorage::from_declared(Err(CompressionMethod::Jpeg), Some(44), None),
            Ok(EntryStorage::Compressed {
                method: CompressionMethod::Jpeg,
                stored: Some(44),
                expanded: None,
            })
        );
    }

    #[test]
    fn compressed_sizes_spell_unreported_only_as_absence() {
        for (compressed, uncompressed, stored, expanded) in [
            (None, None, None, None),
            (None, Some(99), None, Some(99)),
            (Some(44), None, Some(44), None),
            (Some(4), Some(16), Some(4), Some(16)),
            (Some(0), Some(0), Some(0), Some(0)),
        ] {
            let storage = EntryStorage::Compressed {
                method: CompressionMethod::Deflate,
                stored,
                expanded,
            };
            assert_eq!(
                EntryStorage::from_declared(
                    Err(CompressionMethod::Deflate),
                    compressed,
                    uncompressed
                ),
                Ok(storage.clone())
            );
            let entry = ContainerEntry {
                name: "entry".to_string(),
                role: ContainerRole::Stream,
                storage: storage.clone(),
                attributes: BTreeMap::new(),
            };
            let mut expected = wire("deflate", compressed, uncompressed);
            expected["attributes"] = serde_json::json!({});
            let serialized = serde_json::to_value(&entry).expect("a container entry serializes");
            assert_eq!(serialized, expected);
            let read_back: ContainerEntry =
                serde_json::from_value(serialized).expect("the wire value re-admits");
            assert_eq!(read_back.storage, storage);
        }
    }

    #[test]
    fn a_verbatim_entry_cannot_store_fewer_bytes_than_it_expands_to() {
        assert_eq!(
            EntryStorage::from_declared(Ok(VerbatimLabel::Stored), Some(5), Some(9)),
            Err("verbatim container entry stores fewer bytes than it expands to")
        );
        assert_eq!(
            EntryStorage::framed(VerbatimLabel::Stored, 9, 5),
            Err("verbatim container entry stores fewer bytes than it expands to")
        );
        assert_eq!(VerbatimSize::declared(9, 5), None);
        assert_eq!(
            EntryStorage::framed(VerbatimLabel::Stored, 9, 9),
            Ok(EntryStorage::verbatim(VerbatimLabel::Stored, 9))
        );
        assert_eq!(super::FramedSpan::new(5, nonzero(5)), None);
        let span = super::FramedSpan::new(0, nonzero(1)).expect("a framed span");
        assert_eq!(span.payload(), 0);
        assert_eq!(span.framing(), nonzero(1));
        assert_eq!(span.stored(), nonzero(1));
        let span = super::FramedSpan::new(u64::MAX - 1, nonzero(u64::MAX)).expect("a framed span");
        assert_eq!(span.stored(), nonzero(u64::MAX));
        assert_eq!(span.framing(), nonzero(1));
        assert_eq!(
            EntryStorage::Verbatim {
                label: VerbatimLabel::Stored,
                size: VerbatimSize::Framed(span),
            }
            .stored_size(),
            Some(u64::MAX)
        );
        for label in ["none", "stored"] {
            assert!(reject(label, Some(5), Some(9))
                .contains("verbatim container entry stores fewer bytes than it expands to"));
        }
    }

    #[test]
    fn a_minted_framed_span_always_exceeds_its_payload() {
        let empty: &[u8] = &[];
        let span = super::FramedSpan::from_parts(empty.into(), NonZeroU32::MAX)
            .expect("the sum fits a u64");
        assert_eq!(span.payload(), 0);
        assert_eq!(span.framing(), nonzero(u64::from(u32::MAX)));
        assert_eq!(span.stored(), nonzero(u64::from(u32::MAX)));
        assert!(span.stored().get() > span.payload());
        let span = super::FramedSpan::from_parts(empty.into(), NonZeroU32::MIN)
            .expect("the sum fits a u64");
        assert_eq!(span.stored(), nonzero(1));
        let body = vec![0u8; 12];
        let span = super::FramedSpan::from_parts(body.as_slice().into(), NonZeroU32::MIN)
            .expect("the sum fits a u64");
        assert_eq!(span.payload(), body.len() as u64);
        assert_eq!(span.stored(), nonzero(13));
        assert_eq!(span.framing(), nonzero(1));
        let span = super::FramedSpan::from_parts("target.CATPart".into(), NonZeroU32::MAX)
            .expect("the sum fits a u64");
        assert_eq!(span.payload(), "target.CATPart".len() as u64);
        assert_eq!(
            span.stored(),
            nonzero("target.CATPart".len() as u64 + u64::from(u32::MAX))
        );
    }

    #[test]
    fn a_reported_zero_size_is_not_an_unreported_one() {
        let reported = admit("none", Some(0), Some(0));
        let unreported = admit("none", None, None);
        assert_eq!(
            reported.storage,
            EntryStorage::Verbatim {
                label: VerbatimLabel::None,
                size: VerbatimSize::Exact(0),
            }
        );
        assert_eq!(
            unreported.storage,
            EntryStorage::Verbatim {
                label: VerbatimLabel::None,
                size: VerbatimSize::Unreported,
            }
        );
        assert_ne!(reported.storage, unreported.storage);
        assert_eq!(reported.stored_size(), Some(0));
        assert_eq!(reported.expanded_size(), Some(0));
        assert_eq!(unreported.stored_size(), None);
        assert_eq!(unreported.expanded_size(), None);
        let serialized = serde_json::to_value(&unreported).expect("a container entry serializes");
        assert_eq!(serialized.get("compressed_size"), None);
        assert_eq!(serialized.get("uncompressed_size"), None);
        let serialized = serde_json::to_value(&reported).expect("a container entry serializes");
        assert_eq!(
            serialized.get("uncompressed_size"),
            Some(&serde_json::json!(0))
        );
    }

    #[test]
    fn a_storage_label_cannot_declare_a_byte_size() {
        for (compressed, uncompressed) in [
            (Some(4), None),
            (None, Some(7)),
            (Some(4), Some(7)),
            (Some(0), Some(0)),
        ] {
            assert!(reject("storage", compressed, uncompressed)
                .contains("container entry compression \"storage\" declares a byte size"));
        }
        assert_eq!(
            admit("storage", None, None).storage,
            EntryStorage::Directory
        );
    }

    #[test]
    fn every_legal_shape_round_trips_through_the_wire() {
        for (compression, compressed, uncompressed) in [
            ("storage", None, None),
            ("none", None, None),
            ("none", None, Some(12)),
            ("none", None, Some(0)),
            ("none", Some(5), None),
            ("none", Some(0), None),
            ("none", Some(7), Some(7)),
            ("none", Some(0), Some(0)),
            ("none", Some(30), Some(10)),
            ("none", Some(5), Some(0)),
            ("stored", Some(9), Some(9)),
            ("deflate", Some(4), Some(16)),
            ("zlib", None, Some(99)),
            ("jpeg", Some(44), None),
            ("zstd", Some(8), Some(20)),
            ("unix-compress", Some(15), Some(18)),
            ("deflate", Some(0), Some(0)),
        ] {
            let entry = admit(compression, compressed, uncompressed);
            let mut expected = wire(compression, compressed, uncompressed);
            expected["attributes"] = serde_json::json!({});
            assert_eq!(
                serde_json::to_value(&entry).expect("a container entry serializes"),
                expected,
                "{compression} {compressed:?}/{uncompressed:?}"
            );
        }
    }

    #[test]
    fn reported_sizes_are_absent_rather_than_zero() {
        assert_eq!(admit("none", None, None).stored_size(), None);
        assert_eq!(admit("none", None, None).expanded_size(), None);
        assert_eq!(admit("none", None, Some(12)).stored_size(), None);
        assert_eq!(admit("none", None, Some(12)).expanded_size(), Some(12));
        assert_eq!(admit("none", Some(30), Some(10)).stored_size(), Some(30));
        assert_eq!(admit("none", Some(30), Some(10)).expanded_size(), Some(10));
        assert_eq!(admit("zlib", None, Some(99)).stored_size(), None);
        let directory = admit("storage", None, None);
        assert_eq!(directory.stored_size(), None);
        assert_eq!(directory.expanded_size(), None);
        assert_eq!(directory.role, ContainerRole::Stream);
    }
}
