// SPDX-License-Identifier: Apache-2.0
//! Versioned FCStd-native records.

pub(crate) mod element_map;
pub(crate) mod frame;
pub(crate) mod joint;
pub(crate) mod object_identity;

use crate::attachment::MapModeIndex;
use cadmpeg_ir::units::FiniteVector;
use frame::FiniteFrame;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroU64;

/// Every content/evidence arena in the `FreeCAD` native namespace. Source byte
/// ledgers live in `SourceFidelity` rather than in this registry.
pub(crate) const ARENAS: &[&str] = &[
    "document",
    "objects",
    "extensions",
    "properties",
    "shape_payloads",
    "carrier_census",
    "string_tables",
    "product_nodes",
    "joints",
    "drawings",
    "annotations",
    "applications",
    "attachments",
    "design_census",
    "entries",
    "gui_documents",
    "gui_view_providers",
    "gui_properties",
    "element_maps",
    "unreadable_entries",
    "unknowns",
];

pub(crate) fn native_id(kind: &str, key: impl AsRef<str>) -> String {
    format!(
        "fcstd:native:{kind}#{}",
        IdentityKey::encode_segment(key.as_ref())
    )
}

pub(crate) fn native_child_id(kind: &str, parent: &str, child: &str) -> String {
    let parent_key = id_key(parent);
    format!(
        "fcstd:native:{kind}#{parent_key}:{}",
        IdentityKey::encode_segment(child)
    )
}

pub(crate) fn model_id(kind: &str, parent: &str, child: impl AsRef<str>) -> String {
    let child = child.as_ref();
    let child_key = if child.is_empty() {
        String::new()
    } else {
        IdentityKey::encode_segment(child).into_string()
    };
    format!("fcstd:model:{kind}#{}:{}", id_key(parent), child_key)
}

pub(crate) fn id_key(id: &str) -> &str {
    id.split_once('#').map_or(id, |(_, key)| key)
}

#[cfg(test)]
mod tests;

/// Machine-derived semantic projection census for one design object.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignCensusRecordWire")]
pub(crate) struct DesignCensusRecord {
    /// Stable census identity derived from the native object.
    pub(crate) id: String,
    /// Native application object being classified.
    pub(crate) object: String,
    /// Persisted runtime type.
    pub(crate) type_name: String,
    /// Neutral history feature projected from the object.
    pub(crate) feature: String,
    /// Stable CADIR feature-definition family name.
    pub(crate) semantic_kind: String,
    /// Whether topology post-processing composition wraps the operation.
    pub(crate) post_processed: bool,
}

impl DesignCensusRecord {
    /// Whether the operation has neutral semantics.
    pub(crate) fn neutral(&self) -> bool {
        self.semantic_kind != "native"
    }
}

#[derive(Deserialize)]
struct DesignCensusRecordWire {
    id: String,
    object: String,
    type_name: String,
    feature: String,
    semantic_kind: String,
    neutral: bool,
    post_processed: bool,
}

#[derive(Serialize)]
struct DesignCensusRecordOut<'a> {
    id: &'a str,
    object: &'a str,
    type_name: &'a str,
    feature: &'a str,
    semantic_kind: &'a str,
    neutral: bool,
    post_processed: bool,
}

impl Serialize for DesignCensusRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCensusRecordOut {
            id: &self.id,
            object: &self.object,
            type_name: &self.type_name,
            feature: &self.feature,
            semantic_kind: &self.semantic_kind,
            neutral: self.neutral(),
            post_processed: self.post_processed,
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignCensusRecordWire> for DesignCensusRecord {
    type Error = String;

    fn try_from(wire: DesignCensusRecordWire) -> Result<Self, Self::Error> {
        let record = Self {
            id: wire.id,
            object: wire.object,
            type_name: wire.type_name,
            feature: wire.feature,
            semantic_kind: wire.semantic_kind,
            post_processed: wire.post_processed,
        };
        if wire.neutral != record.neutral() {
            return Err("neutral disagrees with semantic_kind".to_owned());
        }
        Ok(record)
    }
}

/// Carrier grammar counted by a census record. Empty payloads have no census.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CarrierCensusForm {
    /// Compact text shape-set grammar.
    Text,
    /// Binary shape-set grammar.
    Binary,
}

/// Machine-derived carrier and topology-family census for one exact shape payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CarrierCensusRecord {
    /// Stable census identity derived from the shape payload.
    pub(crate) id: String,
    /// Shape payload being counted.
    pub(crate) payload: String,
    /// `text` or `binary` carrier grammar.
    pub(crate) form: CarrierCensusForm,
    /// Grammar version declared by the shape-set header.
    pub(crate) topology_version: u8,
    /// Recursive 2D-curve family counts.
    pub(crate) curves_2d: BTreeMap<String, u64>,
    /// Recursive 3D-curve family counts.
    pub(crate) curves_3d: BTreeMap<String, u64>,
    /// Recursive surface-family counts.
    pub(crate) surfaces: BTreeMap<String, u64>,
    /// Topological shape-family counts.
    pub(crate) topology: BTreeMap<String, u64>,
    /// Standalone polygon carrier count.
    pub(crate) polygons_3d: u64,
    /// Polygon-on-triangulation carrier count.
    pub(crate) polygons_on_triangulations: u64,
    /// Triangulation carrier count.
    pub(crate) triangulations: u64,
}

/// One support attachment and its distinct persisted frames.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "AttachmentRecordWire")]
pub(crate) struct AttachmentRecord {
    /// Stable attachment identity.
    pub(crate) id: String,
    /// Attached application object.
    pub(crate) object: String,
    /// Ordered support objects and subelements.
    pub(crate) supports: Vec<Option<LinkTarget>>,
    /// Persisted attachment-map mode.
    pub(crate) map_mode: Option<MapModeIndex>,
    /// Persisted resolved object placement.
    placement: Option<FiniteFrame>,
    /// Persisted attachment-local offset.
    offset: Option<FiniteFrame>,
}

impl AttachmentRecord {
    pub(crate) fn try_new(
        id: String,
        object: String,
        supports: Vec<Option<LinkTarget>>,
        map_mode: Option<MapModeIndex>,
        placement: Option<FiniteFrame>,
        offset: Option<FiniteFrame>,
    ) -> Result<Self, String> {
        let record = Self {
            id,
            object,
            supports,
            map_mode,
            placement,
            offset,
        };
        if let (Some(placement), Some(offset)) = (record.placement, record.offset) {
            placement
                .transform()
                .compose(offset.transform())
                .map_err(|error| format!("effective_frame: {error}"))?;
        }
        Ok(record)
    }
    pub(crate) fn placement(&self) -> Option<FiniteFrame> {
        self.placement
    }
    pub(crate) fn offset(&self) -> Option<FiniteFrame> {
        self.offset
    }

    /// Effective frame used for neutral geometry.
    pub(crate) fn effective_frame(&self) -> [[f64; 4]; 4] {
        crate::attachment::effective_frame(
            self.placement().map(FiniteFrame::rows),
            self.offset().map(FiniteFrame::rows),
        )
    }
}

#[derive(Deserialize)]
struct AttachmentRecordWire {
    id: String,
    object: String,
    supports: Vec<Option<LinkTarget>>,
    map_mode: Option<MapModeIndex>,
    placement: Option<[[f64; 4]; 4]>,
    offset: Option<[[f64; 4]; 4]>,
    effective_frame: [[f64; 4]; 4],
}

#[derive(Serialize)]
struct AttachmentRecordOut<'a> {
    id: &'a str,
    object: &'a str,
    supports: &'a [Option<LinkTarget>],
    map_mode: Option<MapModeIndex>,
    placement: Option<[[f64; 4]; 4]>,
    offset: Option<[[f64; 4]; 4]>,
    effective_frame: [[f64; 4]; 4],
}

impl Serialize for AttachmentRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AttachmentRecordOut {
            id: &self.id,
            object: &self.object,
            supports: &self.supports,
            map_mode: self.map_mode,
            placement: self.placement.map(FiniteFrame::rows),
            offset: self.offset.map(FiniteFrame::rows),
            effective_frame: self.effective_frame(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<AttachmentRecordWire> for AttachmentRecord {
    type Error = String;

    fn try_from(wire: AttachmentRecordWire) -> Result<Self, Self::Error> {
        let record = Self::try_new(
            wire.id,
            wire.object,
            wire.supports,
            wire.map_mode,
            wire.placement
                .map(FiniteFrame::try_from)
                .transpose()
                .map_err(|error| format!("placement: {error}"))?,
            wire.offset
                .map(FiniteFrame::try_from)
                .transpose()
                .map_err(|error| format!("offset: {error}"))?,
        )?;
        if wire.effective_frame != record.effective_frame() {
            return Err(
                "attachment effective_frame disagrees with placement and offset".to_owned(),
            );
        }
        Ok(record)
    }
}

/// A nonempty half-open byte interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ByteSpan {
    start: u64,
    end: u64,
}

impl ByteSpan {
    pub(crate) fn try_new(start: u64, end: u64) -> Result<Self, String> {
        if start >= end {
            return Err("byte span start must be less than end".to_owned());
        }
        Ok(Self { start, end })
    }
    pub(crate) fn start(self) -> u64 {
        self.start
    }
    pub(crate) fn end(self) -> u64 {
        self.end
    }
}

/// Exact XML text paired with its nonempty source interval.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RetainedXmlWire")]
pub(crate) struct RetainedXml {
    // Transient scan fields. NativeRecord serializes only the evidence below.
    text: String,
    span: ByteSpan,
    length: u64,
    sha256: String,
}

impl RetainedXml {
    fn try_new(text: String, start: u64, end: u64) -> Result<Self, String> {
        let span = ByteSpan::try_new(start, end)?;
        if end - start != cadmpeg_core::decode::u64_from_index(text.len()) {
            return Err("raw_xml length disagrees with byte_start and byte_end".to_owned());
        }
        let sha256 = sha256_hex(text.as_bytes());
        Ok(Self {
            text,
            span,
            length: end - start,
            sha256,
        })
    }
    pub(crate) fn from_text(text: String, start: u64) -> Result<Self, String> {
        let end = start
            .checked_add(cadmpeg_core::decode::u64_from_index(text.len()))
            .ok_or("raw_xml byte_end overflow")?;
        Self::try_new(text, start, end)
    }
    pub(crate) fn from_source(text: &str, start: u64) -> Result<Self, CodecError> {
        Self::from_text(text.to_owned(), start).map_err(CodecError::Malformed)
    }
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
    pub(crate) fn start(&self) -> u64 {
        self.span.start()
    }
    pub(crate) fn end(&self) -> u64 {
        self.span.end()
    }
    pub(crate) fn length(&self) -> u64 {
        self.length
    }
    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedXmlWire {
    length: u64,
    sha256: String,
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
impl TryFrom<RetainedXmlWire> for RetainedXml {
    type Error = String;
    fn try_from(wire: RetainedXmlWire) -> Result<Self, Self::Error> {
        if wire.length == 0 || !valid_sha256(&wire.sha256) {
            return Err("XML evidence needs positive length and lowercase sha256".into());
        }
        Ok(Self {
            text: String::new(),
            span: ByteSpan {
                start: 0,
                end: wire.length,
            },
            length: wire.length,
            sha256: wire.sha256,
        })
    }
}

#[derive(Serialize)]
struct RetainedXmlOut<'a> {
    length: u64,
    sha256: &'a str,
}

impl Serialize for RetainedXml {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RetainedXmlOut {
            length: self.length,
            sha256: &self.sha256,
        }
        .serialize(serializer)
    }
}

/// Document-level GUI state outside application-object view providers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "GuiDocumentRecordWire")]
pub(crate) struct GuiDocumentRecord {
    /// Stable GUI document identity.
    pub(crate) id: String,
    /// Exact persisted GUI schema declaration when present.
    pub(crate) schema_version: Option<String>,
    /// Exact root attributes.
    pub(crate) attributes: BTreeMap<String, String>,
    /// Ordered camera, active-view, clipping, and other document state.
    pub(crate) states: Vec<GuiStateRecord>,
}

/// One ordered document-level GUI state element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GuiStateRecord {
    /// Stable state identity.
    pub(crate) id: String,
    /// Persisted XML element name.
    pub(crate) kind: String,
    /// Exact element attributes.
    pub(crate) attributes: BTreeMap<String, String>,
    /// Ordered descendant value elements.
    pub(crate) values: Vec<ValueRecord>,
    /// Referenced display assets.
    pub(crate) side_entries: Vec<String>,
}

#[derive(Deserialize)]
struct GuiStateRecordWire {
    order: usize,
    #[serde(flatten)]
    state: GuiStateRecord,
}

#[derive(Deserialize)]
struct GuiDocumentRecordWire {
    id: String,
    schema_version: Option<String>,
    attributes: BTreeMap<String, String>,
    states: Vec<GuiStateRecordWire>,
}

#[derive(Serialize)]
struct GuiStateRecordOut<'a> {
    order: usize,
    #[serde(flatten)]
    state: &'a GuiStateRecord,
}

/// Writes each state beside the order its collection position states.
struct GuiStatesOut<'a>(&'a [GuiStateRecord]);

impl Serialize for GuiStatesOut<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .iter()
                .enumerate()
                .map(|(order, state)| GuiStateRecordOut { order, state }),
        )
    }
}

#[derive(Serialize)]
struct GuiDocumentRecordOut<'a> {
    id: &'a str,
    schema_version: Option<&'a str>,
    attributes: &'a BTreeMap<String, String>,
    states: GuiStatesOut<'a>,
}

impl Serialize for GuiDocumentRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        GuiDocumentRecordOut {
            id: &self.id,
            schema_version: self.schema_version.as_deref(),
            attributes: &self.attributes,
            states: GuiStatesOut(&self.states),
        }
        .serialize(serializer)
    }
}
impl TryFrom<GuiDocumentRecordWire> for GuiDocumentRecord {
    type Error = String;
    fn try_from(wire: GuiDocumentRecordWire) -> Result<Self, Self::Error> {
        let states = wire
            .states
            .into_iter()
            .enumerate()
            .map(|(order, wire)| {
                if wire.order != order {
                    return Err("GUI state order disagrees with its states position".to_owned());
                }
                Ok(wire.state)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id: wire.id,
            schema_version: wire.schema_version,
            attributes: wire.attributes,
            states,
        })
    }
}

/// A supported semantic annotation runtime type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnnotationRuntimeType {
    /// The `App::Annotation` runtime type.
    Annotation,
    /// The `App::AnnotationLabel` runtime type.
    AnnotationLabel,
    /// The `TechDraw::DrawViewAnnotation` runtime type.
    DrawViewAnnotation,
    /// The `TechDraw::DrawViewAnnotationPython` runtime type.
    DrawViewAnnotationPython,
    /// The `TechDraw::DrawRichAnno` runtime type.
    DrawRichAnno,
    /// The `TechDraw::DrawRichAnnoPython` runtime type.
    DrawRichAnnoPython,
    /// The `TechDraw::DrawViewDimension` runtime type.
    DrawViewDimension,
    /// The `TechDraw::DrawViewDimExtent` runtime type.
    DrawViewDimExtent,
    /// The `TechDraw::LandmarkDimension` runtime type.
    LandmarkDimension,
    /// The `TechDraw::DrawViewBalloon` runtime type.
    DrawViewBalloon,
    /// The `TechDraw::DrawLeaderLine` runtime type.
    DrawLeaderLine,
    /// The `TechDraw::DrawLeaderLinePython` runtime type.
    DrawLeaderLinePython,
    /// The `TechDraw::DrawViewSymbol` runtime type.
    DrawViewSymbol,
    /// The `TechDraw::DrawViewSymbolPython` runtime type.
    DrawViewSymbolPython,
    /// The `TechDraw::DrawWeldSymbol` runtime type.
    DrawWeldSymbol,
    /// The `TechDraw::DrawWeldSymbolPython` runtime type.
    DrawWeldSymbolPython,
}

impl AnnotationRuntimeType {
    /// Returns the persisted runtime type name.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Annotation => "App::Annotation",
            Self::AnnotationLabel => "App::AnnotationLabel",
            Self::DrawViewAnnotation => "TechDraw::DrawViewAnnotation",
            Self::DrawViewAnnotationPython => "TechDraw::DrawViewAnnotationPython",
            Self::DrawRichAnno => "TechDraw::DrawRichAnno",
            Self::DrawRichAnnoPython => "TechDraw::DrawRichAnnoPython",
            Self::DrawViewDimension => "TechDraw::DrawViewDimension",
            Self::DrawViewDimExtent => "TechDraw::DrawViewDimExtent",
            Self::LandmarkDimension => "TechDraw::LandmarkDimension",
            Self::DrawViewBalloon => "TechDraw::DrawViewBalloon",
            Self::DrawLeaderLine => "TechDraw::DrawLeaderLine",
            Self::DrawLeaderLinePython => "TechDraw::DrawLeaderLinePython",
            Self::DrawViewSymbol => "TechDraw::DrawViewSymbol",
            Self::DrawViewSymbolPython => "TechDraw::DrawViewSymbolPython",
            Self::DrawWeldSymbol => "TechDraw::DrawWeldSymbol",
            Self::DrawWeldSymbolPython => "TechDraw::DrawWeldSymbolPython",
        }
    }

    pub(crate) fn from_label(label: &str) -> Option<Self> {
        match label {
            "App::Annotation" => Some(Self::Annotation),
            "App::AnnotationLabel" => Some(Self::AnnotationLabel),
            "TechDraw::DrawViewAnnotation" => Some(Self::DrawViewAnnotation),
            "TechDraw::DrawViewAnnotationPython" => Some(Self::DrawViewAnnotationPython),
            "TechDraw::DrawRichAnno" => Some(Self::DrawRichAnno),
            "TechDraw::DrawRichAnnoPython" => Some(Self::DrawRichAnnoPython),
            "TechDraw::DrawViewDimension" => Some(Self::DrawViewDimension),
            "TechDraw::DrawViewDimExtent" => Some(Self::DrawViewDimExtent),
            "TechDraw::LandmarkDimension" => Some(Self::LandmarkDimension),
            "TechDraw::DrawViewBalloon" => Some(Self::DrawViewBalloon),
            "TechDraw::DrawLeaderLine" => Some(Self::DrawLeaderLine),
            "TechDraw::DrawLeaderLinePython" => Some(Self::DrawLeaderLinePython),
            "TechDraw::DrawViewSymbol" => Some(Self::DrawViewSymbol),
            "TechDraw::DrawViewSymbolPython" => Some(Self::DrawViewSymbolPython),
            "TechDraw::DrawWeldSymbol" => Some(Self::DrawWeldSymbol),
            "TechDraw::DrawWeldSymbolPython" => Some(Self::DrawWeldSymbolPython),
            _ => None,
        }
    }
}

impl Serialize for AnnotationRuntimeType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AnnotationRuntimeType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let label = String::deserialize(deserializer)?;
        Self::from_label(&label).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "semantic annotation kind: unsupported runtime type {label}"
            ))
        })
    }
}

/// One semantic annotation object kept distinct from drawing presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SemanticAnnotationRecord {
    /// Stable semantic annotation identity.
    pub(crate) id: String,
    /// Owning application object.
    pub(crate) object: String,
    /// Persisted annotation runtime type.
    pub(crate) kind: AnnotationRuntimeType,
    /// Ordered user-visible text fragments.
    pub(crate) text: Vec<String>,
    /// Object and subelement references grouped by source property.
    pub(crate) references: BTreeMap<String, Vec<Option<LinkTarget>>>,
    /// Typed or exactly framed annotation properties grouped by source name.
    pub(crate) parameters: BTreeMap<String, String>,
    /// Referenced symbol, image, or other side entries.
    pub(crate) side_entries: Vec<String>,
}

/// Persisted page runtime type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TechDrawPageKind {
    /// Native page.
    Page,
    /// Python page.
    Python,
}

/// Persisted non-page runtime type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TechDrawNonPageKind(String);

/// Drawing runtime type and its page payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TechDrawKind {
    /// Page with ordered views and optional template.
    Page {
        /// Persisted page class.
        runtime: TechDrawPageKind,
        /// Ordered page views.
        views: Vec<String>,
        /// Page template object.
        template: Option<String>,
    },
    /// Non-page drawing object.
    Other(TechDrawNonPageKind),
}

impl TechDrawKind {
    /// Admits a runtime type and its page payload.
    pub(crate) fn try_new(
        kind: String,
        views: Vec<String>,
        template: Option<String>,
    ) -> Result<Self, String> {
        let runtime = match kind.as_str() {
            "TechDraw::DrawPage" => Some(TechDrawPageKind::Page),
            "TechDraw::DrawPagePython" => Some(TechDrawPageKind::Python),
            _ => None,
        };
        if let Some(runtime) = runtime {
            Ok(Self::Page {
                runtime,
                views,
                template,
            })
        } else if views.is_empty() && template.is_none() {
            Ok(Self::Other(TechDrawNonPageKind(kind)))
        } else {
            Err("non-page drawing record cannot carry views or a template".to_owned())
        }
    }

    /// Persisted runtime type name.
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Page {
                runtime: TechDrawPageKind::Page,
                ..
            } => "TechDraw::DrawPage",
            Self::Page {
                runtime: TechDrawPageKind::Python,
                ..
            } => "TechDraw::DrawPagePython",
            Self::Other(kind) => &kind.0,
        }
    }
}

/// One `TechDraw` page, template, view, dimension, or annotation record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DrawingRecordWire")]
pub(crate) struct DrawingRecord {
    /// Stable drawing-record identity.
    pub(crate) id: String,
    /// Owning application object.
    pub(crate) object: String,
    /// Persisted `TechDraw` runtime type.
    pub(crate) kind: TechDrawKind,
    /// Ordered source object and subelement references for a view or dimension.
    pub(crate) sources: Vec<Option<LinkTarget>>,
    /// All drawing relationships grouped by their persisted property name.
    pub(crate) relationships: BTreeMap<String, Vec<Option<LinkTarget>>>,
    /// Typed scalar/vector/string drawing fields retained by property name.
    pub(crate) parameters: BTreeMap<String, String>,
    /// Referenced template or drawing side entries.
    pub(crate) side_entries: Vec<String>,
}

#[derive(Deserialize)]
struct DrawingRecordWire {
    id: String,
    object: String,
    kind: String,
    views: Vec<String>,
    template: Option<String>,
    sources: Vec<Option<LinkTarget>>,
    relationships: BTreeMap<String, Vec<Option<LinkTarget>>>,
    parameters: BTreeMap<String, String>,
    side_entries: Vec<String>,
}

#[derive(Serialize)]
struct DrawingRecordOut<'a> {
    id: &'a str,
    object: &'a str,
    kind: &'a str,
    views: &'a [String],
    template: Option<&'a str>,
    sources: &'a [Option<LinkTarget>],
    relationships: &'a BTreeMap<String, Vec<Option<LinkTarget>>>,
    parameters: &'a BTreeMap<String, String>,
    side_entries: &'a [String],
}

impl Serialize for DrawingRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // A non-page drawing object carries no views and no template.
        let (views, template) = match &self.kind {
            TechDrawKind::Page {
                views, template, ..
            } => (views.as_slice(), template.as_deref()),
            TechDrawKind::Other(_) => (&[][..], None),
        };
        DrawingRecordOut {
            id: &self.id,
            object: &self.object,
            kind: self.kind.as_str(),
            views,
            template,
            sources: &self.sources,
            relationships: &self.relationships,
            parameters: &self.parameters,
            side_entries: &self.side_entries,
        }
        .serialize(serializer)
    }
}

impl TryFrom<DrawingRecordWire> for DrawingRecord {
    type Error = String;

    fn try_from(wire: DrawingRecordWire) -> Result<Self, Self::Error> {
        let kind = TechDrawKind::try_new(wire.kind, wire.views, wire.template)?;
        Ok(Self {
            id: wire.id,
            object: wire.object,
            kind,
            sources: wire.sources,
            relationships: wire.relationships,
            parameters: wire.parameters,
            side_entries: wire.side_entries,
        })
    }
}

/// One product container, prototype, or placed link occurrence.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "ProductNodeRecordWire")]
pub(crate) struct ProductNodeRecord {
    /// Stable record identity.
    pub(crate) id: String,
    /// Owning application object.
    pub(crate) object: String,
    /// Structural family and family-specific payload.
    pub(crate) node: ProductNode,
}

/// Structural family of a product node.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProductNode {
    /// `App::DocumentObjectGroup`.
    Group(ContainerNode),
    /// `App::Part` or assembly container.
    Part(ContainerNode),
    /// `App::LinkGroup`.
    LinkGroup {
        /// Group placement and any explicit `Group` membership.
        container: ContainerNode,
        /// Ordered `ElementList` application objects.
        element_objects: Vec<String>,
    },
    /// `App::Link` or `App::LinkElement`.
    Occurrence(Box<LinkOccurrence>),
}

/// Shared payload of a non-occurrence product container.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContainerNode {
    /// Ordered contained application objects.
    pub(crate) members: Vec<String>,
    /// Local placement as a row-major affine matrix.
    pub(crate) local_transform: Option<FiniteFrame>,
    /// Property supplying the placement.
    pub(crate) placement_property: Option<String>,
}

/// Link occurrence payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinkOccurrence {
    /// Ordered contained application objects.
    pub(crate) members: Vec<String>,
    /// Linked prototype object.
    pub(crate) prototype: Option<String>,
    /// External document token when the prototype is not local.
    pub(crate) external_document: Option<ExternalDocument>,
    /// Local occurrence placement as a row-major affine matrix.
    pub(crate) local_transform: Option<FiniteFrame>,
    /// Property supplying the placement.
    pub(crate) placement_property: Option<String>,
    /// Admitted link-array values.
    pub(crate) array: LinkArray,
    /// Whether the prototype transform participates in occurrence placement.
    pub(crate) link_transform: Option<bool>,
    /// Subelement paths selected on the linked prototype.
    pub(crate) linked_subelements: Vec<String>,
    /// Whether the link claims its prototype as a tree child.
    pub(crate) claim_child: Option<bool>,
    /// Copy-on-change policy and its payload.
    pub(crate) copy_on_change: Option<CopyOnChange>,
    /// Base scale vector applied to every occurrence element.
    pub(crate) scale: Option<FiniteVector<3>>,
}

/// Independently optional array carriers with a common element count.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinkArray {
    count: Option<u64>,
    transforms: Vec<FiniteFrame>,
    scales: Vec<FiniteVector<3>>,
    visibility: Vec<bool>,
    objects: Vec<String>,
}

/// The element cardinality a link states.
///
/// A present zero `ElementCount` requires every array-valued field to be empty
/// and the link retains its single scalar occurrence. An absent `ElementCount`
/// permits one scalar link occurrence or infers a nonzero count from the
/// populated array-valued fields, including per-element visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkArrayCardinality {
    /// The link is not an array and carries one occurrence.
    Scalar,
    /// The link is an array of this many elements.
    Elements(NonZeroU64),
}

impl LinkArray {
    pub(crate) fn try_new(
        count: Option<u64>,
        transforms: Vec<FiniteFrame>,
        scales: Vec<FiniteVector<3>>,
        visibility: Vec<bool>,
        objects: Vec<String>,
    ) -> Result<Self, String> {
        let lengths = [
            cadmpeg_core::decode::u64_from_index(transforms.len()),
            cadmpeg_core::decode::u64_from_index(scales.len()),
            cadmpeg_core::decode::u64_from_index(visibility.len()),
            cadmpeg_core::decode::u64_from_index(objects.len()),
        ];
        // With no stated count the longest populated carrier establishes the
        // cardinality; every populated carrier must agree with it. A stated
        // zero therefore requires every carrier to be empty.
        let effective = count.unwrap_or(lengths[0].max(lengths[1]).max(lengths[2]).max(lengths[3]));
        if lengths
            .into_iter()
            .any(|length| length != 0 && length != effective)
        {
            return Err("element_count has inconsistent link-array counts".to_owned());
        }
        Ok(Self {
            count,
            transforms,
            scales,
            visibility,
            objects,
        })
    }

    /// Element cardinality: the stated count, else the one the carriers establish.
    fn cardinality(&self) -> LinkArrayCardinality {
        let elements = self.count.unwrap_or_else(|| {
            (cadmpeg_core::decode::u64_from_index(self.transforms.len()))
                .max(cadmpeg_core::decode::u64_from_index(self.scales.len()))
                .max(cadmpeg_core::decode::u64_from_index(self.visibility.len()))
                .max(cadmpeg_core::decode::u64_from_index(self.objects.len()))
        });
        match NonZeroU64::new(elements) {
            None => LinkArrayCardinality::Scalar,
            Some(elements) => LinkArrayCardinality::Elements(elements),
        }
    }
}

/// A source `LinkCopyOnChange` enumeration index with its exact spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopyOnChangePolicy {
    raw: String,
    index: i64,
}

impl CopyOnChangePolicy {
    pub(crate) fn from_raw(raw: String) -> Result<Self, String> {
        let index = raw
            .parse::<i64>()
            .map_err(|_| "copy-on-change policy must be a signed integer".to_owned())?;
        Ok(Self { raw, index })
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.raw
    }

    pub(crate) fn index(&self) -> i64 {
        self.index
    }
}

/// Copy-on-change policy and its dependent payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CopyOnChange {
    /// Admitted persisted policy index.
    policy: CopyOnChangePolicy,
    /// Original tracked object.
    pub(crate) source: Option<LinkTarget>,
    /// Internal ownership group.
    pub(crate) group: Option<LinkTarget>,
    /// Whether the tracked source changed.
    touched: Option<bool>,
}

impl CopyOnChange {
    fn from_wire(
        policy: Option<String>,
        source: Option<LinkTarget>,
        group: Option<LinkTarget>,
        touched: Option<bool>,
    ) -> Result<Option<Self>, String> {
        let policy = policy.map(CopyOnChangePolicy::from_raw).transpose()?;
        Self::from_admitted(policy, source, group, touched)
    }

    pub(crate) fn from_admitted(
        policy: Option<CopyOnChangePolicy>,
        source: Option<LinkTarget>,
        group: Option<LinkTarget>,
        touched: Option<bool>,
    ) -> Result<Option<Self>, String> {
        match policy {
            Some(policy) => Ok(Some(Self {
                policy,
                source,
                group,
                touched,
            })),
            None if source.is_none() && group.is_none() && touched.is_none() => Ok(None),
            None => Err("copy_on_change payload requires copy_on_change policy".to_owned()),
        }
    }
}

impl ProductNodeRecord {
    /// Structural family string retained on the CADIR wire.
    pub(crate) fn kind(&self) -> &'static str {
        match self.node {
            ProductNode::Group(_) => "group",
            ProductNode::Part(_) => "part",
            ProductNode::LinkGroup { .. } => "link_group",
            ProductNode::Occurrence(_) => "occurrence",
        }
    }

    /// Ordered contained application objects.
    pub(crate) fn members(&self) -> &[String] {
        match &self.node {
            ProductNode::Group(node)
            | ProductNode::Part(node)
            | ProductNode::LinkGroup {
                container: node, ..
            } => &node.members,
            ProductNode::Occurrence(node) => &node.members,
        }
    }

    /// Local placement matrix when stored on the node.
    pub(crate) fn local_transform(&self) -> Option<FiniteFrame> {
        match &self.node {
            ProductNode::Group(node)
            | ProductNode::Part(node)
            | ProductNode::LinkGroup {
                container: node, ..
            } => node.local_transform,
            ProductNode::Occurrence(node) => node.local_transform,
        }
    }

    /// Property supplying the placement.
    pub(crate) fn placement_property(&self) -> Option<&str> {
        match &self.node {
            ProductNode::Group(node)
            | ProductNode::Part(node)
            | ProductNode::LinkGroup {
                container: node, ..
            } => node.placement_property.as_deref(),
            ProductNode::Occurrence(node) => node.placement_property.as_deref(),
        }
    }

    /// Occurrence payload when this node is a link.
    pub(crate) fn occurrence(&self) -> Option<&LinkOccurrence> {
        match &self.node {
            ProductNode::Occurrence(node) => Some(node),
            _ => None,
        }
    }

    /// Linked prototype object for an occurrence.
    pub(crate) fn prototype(&self) -> Option<&str> {
        self.occurrence().and_then(|node| node.prototype.as_deref())
    }

    /// External document token when the prototype is not local.
    pub(crate) fn external_document(&self) -> Option<&ExternalDocument> {
        self.occurrence()
            .and_then(|node| node.external_document.as_ref())
    }

    /// Number of array elements requested by the link.
    pub(crate) fn element_count(&self) -> Option<u64> {
        self.occurrence().and_then(|node| node.array.count)
    }

    /// Element cardinality of a link occurrence. A non-occurrence node has none.
    pub(crate) fn element_cardinality(&self) -> Option<LinkArrayCardinality> {
        self.occurrence().map(|node| node.array.cardinality())
    }

    /// Whether the prototype transform participates in occurrence placement.
    pub(crate) fn link_transform(&self) -> Option<bool> {
        self.occurrence().and_then(|node| node.link_transform)
    }

    /// Ordered per-element placements for a link array.
    pub(crate) fn element_transforms(&self) -> &[FiniteFrame] {
        self.occurrence()
            .map_or(&[], |node| node.array.transforms.as_slice())
    }

    /// Ordered per-element scale vectors for a link array.
    pub(crate) fn element_scales(&self) -> &[FiniteVector<3>] {
        self.occurrence()
            .map_or(&[], |node| node.array.scales.as_slice())
    }

    /// Ordered per-element visibility values after `FreeCAD` bit-order decoding.
    pub(crate) fn element_visibility(&self) -> &[bool] {
        self.occurrence()
            .map_or(&[], |node| node.array.visibility.as_slice())
    }

    /// Subelement paths selected on the linked prototype.
    pub(crate) fn linked_subelements(&self) -> &[String] {
        self.occurrence()
            .map_or(&[], |node| node.linked_subelements.as_slice())
    }

    /// Whether the link claims its prototype as a tree child.
    pub(crate) fn claim_child(&self) -> Option<bool> {
        self.occurrence().and_then(|node| node.claim_child)
    }

    /// Persisted copy-on-change policy name or numeric code.
    pub(crate) fn copy_on_change(&self) -> Option<&str> {
        self.occurrence()
            .and_then(|node| node.copy_on_change.as_ref())
            .map(|copy| copy.policy.as_str())
    }

    /// Admitted copy-on-change policy, including its parsed source index.
    pub(crate) fn copy_on_change_policy(&self) -> Option<&CopyOnChangePolicy> {
        self.occurrence()
            .and_then(|node| node.copy_on_change.as_ref())
            .map(|copy| &copy.policy)
    }

    /// Original object tracked by copy-on-change.
    pub(crate) fn copy_on_change_source(&self) -> Option<&LinkTarget> {
        self.occurrence()
            .and_then(|node| node.copy_on_change.as_ref())
            .and_then(|copy| copy.source.as_ref())
    }

    /// Internal ownership group for copy-on-change copies.
    pub(crate) fn copy_on_change_group(&self) -> Option<&LinkTarget> {
        self.occurrence()
            .and_then(|node| node.copy_on_change.as_ref())
            .and_then(|copy| copy.group.as_ref())
    }

    /// Whether the tracked source has changed.
    pub(crate) fn copy_on_change_touched(&self) -> Option<bool> {
        self.occurrence()
            .and_then(|node| node.copy_on_change.as_ref())
            .and_then(|copy| copy.touched)
    }

    /// Base scale vector applied to every occurrence element.
    pub(crate) fn scale(&self) -> Option<[f64; 3]> {
        self.occurrence()
            .and_then(|node| node.scale)
            .map(FiniteVector::get)
    }

    /// Explicit per-element application objects in array order.
    pub(crate) fn element_objects(&self) -> &[String] {
        match &self.node {
            ProductNode::LinkGroup {
                element_objects, ..
            } => element_objects,
            ProductNode::Occurrence(node) => &node.array.objects,
            ProductNode::Group(_) | ProductNode::Part(_) => &[],
        }
    }
}

#[derive(Deserialize)]
struct ProductNodeRecordWire {
    id: String,
    object: String,
    kind: String,
    members: Vec<String>,
    prototype: Option<String>,
    external_document: Option<String>,
    external_document_attribute: Option<String>,
    local_transform: Option<[[f64; 4]; 4]>,
    placement_property: Option<String>,
    element_count: Option<u64>,
    link_transform: Option<bool>,
    element_transforms: Vec<[[f64; 4]; 4]>,
    element_scales: Vec<[f64; 3]>,
    linked_subelements: Vec<String>,
    claim_child: Option<bool>,
    copy_on_change: Option<String>,
    copy_on_change_source: Option<LinkTarget>,
    copy_on_change_group: Option<LinkTarget>,
    copy_on_change_touched: Option<bool>,
    scale: Option<[f64; 3]>,
    element_visibility: Vec<bool>,
    element_objects: Vec<String>,
}

/// Writes admitted frames as the row-major matrices the wire carries.
struct FrameRowsOut<'a>(&'a [FiniteFrame]);

impl Serialize for FrameRowsOut<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().copied().map(FiniteFrame::rows))
    }
}

/// Writes admitted scale vectors as the component triples the wire carries.
struct Vec3ValuesOut<'a>(&'a [FiniteVector<3>]);

impl Serialize for Vec3ValuesOut<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().copied().map(FiniteVector::get))
    }
}

#[derive(Serialize)]
struct ProductNodeRecordOut<'a> {
    id: &'a str,
    object: &'a str,
    kind: &'static str,
    members: &'a [String],
    prototype: Option<&'a str>,
    external_document: Option<&'a str>,
    external_document_attribute: Option<&'static str>,
    local_transform: Option<[[f64; 4]; 4]>,
    placement_property: Option<&'a str>,
    element_count: Option<u64>,
    link_transform: Option<bool>,
    element_transforms: FrameRowsOut<'a>,
    element_scales: Vec3ValuesOut<'a>,
    linked_subelements: &'a [String],
    claim_child: Option<bool>,
    copy_on_change: Option<&'a str>,
    copy_on_change_source: Option<&'a LinkTarget>,
    copy_on_change_group: Option<&'a LinkTarget>,
    copy_on_change_touched: Option<bool>,
    scale: Option<[f64; 3]>,
    element_visibility: &'a [bool],
    element_objects: &'a [String],
}

impl Serialize for ProductNodeRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ProductNodeRecordOut {
            id: &self.id,
            object: &self.object,
            kind: self.kind(),
            members: self.members(),
            prototype: self.prototype(),
            external_document: self.external_document().map(ExternalDocument::as_str),
            external_document_attribute: self
                .external_document()
                .and_then(ExternalDocument::attribute),
            local_transform: self.local_transform().map(FiniteFrame::rows),
            placement_property: self.placement_property(),
            element_count: self.element_count(),
            link_transform: self.link_transform(),
            element_transforms: FrameRowsOut(self.element_transforms()),
            element_scales: Vec3ValuesOut(self.element_scales()),
            linked_subelements: self.linked_subelements(),
            claim_child: self.claim_child(),
            copy_on_change: self.copy_on_change(),
            copy_on_change_source: self.copy_on_change_source(),
            copy_on_change_group: self.copy_on_change_group(),
            copy_on_change_touched: self.copy_on_change_touched(),
            scale: self.scale(),
            element_visibility: self.element_visibility(),
            element_objects: self.element_objects(),
        }
        .serialize(serializer)
    }
}

impl ProductNodeRecordWire {
    fn has_occurrence_fields(&self) -> bool {
        self.prototype.is_some()
            || self.external_document.is_some()
            || self.external_document_attribute.is_some()
            || self.element_count.is_some()
            || self.link_transform.is_some()
            || !self.element_transforms.is_empty()
            || !self.element_scales.is_empty()
            || !self.element_visibility.is_empty()
            || !self.linked_subelements.is_empty()
            || self.claim_child.is_some()
            || self.copy_on_change.is_some()
            || self.copy_on_change_source.is_some()
            || self.copy_on_change_group.is_some()
            || self.copy_on_change_touched.is_some()
            || self.scale.is_some()
    }
}

impl TryFrom<ProductNodeRecordWire> for ProductNodeRecord {
    type Error = String;

    fn try_from(wire: ProductNodeRecordWire) -> Result<Self, Self::Error> {
        if wire.kind != "occurrence" && wire.has_occurrence_fields() {
            return Err("container product node carries occurrence-only link fields".to_owned());
        }
        if matches!(wire.kind.as_str(), "group" | "part") && !wire.element_objects.is_empty() {
            return Err("non-link container product node carries element_objects".to_owned());
        }
        let node = match wire.kind.as_str() {
            "group" | "part" | "link_group" => {
                let container = ContainerNode {
                    members: wire.members,
                    local_transform: wire
                        .local_transform
                        .map(FiniteFrame::try_from)
                        .transpose()
                        .map_err(|error| format!("local_transform: {error}"))?,
                    placement_property: wire.placement_property,
                };
                match wire.kind.as_str() {
                    "group" => ProductNode::Group(container),
                    "part" => ProductNode::Part(container),
                    _ => ProductNode::LinkGroup {
                        container,
                        element_objects: wire.element_objects,
                    },
                }
            }
            "occurrence" => ProductNode::Occurrence(Box::new(LinkOccurrence {
                members: wire.members,
                prototype: wire.prototype,
                external_document: ExternalDocument::from_wire(
                    wire.external_document,
                    wire.external_document_attribute.as_deref(),
                )?,
                local_transform: wire
                    .local_transform
                    .map(FiniteFrame::try_from)
                    .transpose()
                    .map_err(|error| format!("local_transform: {error}"))?,
                placement_property: wire.placement_property,
                array: LinkArray::try_new(
                    wire.element_count,
                    wire.element_transforms
                        .into_iter()
                        .map(FiniteFrame::try_from)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| format!("element_transforms: {error}"))?,
                    wire.element_scales
                        .into_iter()
                        .map(|values| {
                            FiniteVector::new(values)
                                .ok_or("scale vector components must be finite")
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| format!("element_scales: {error}"))?,
                    wire.element_visibility,
                    wire.element_objects,
                )?,
                link_transform: wire.link_transform,
                linked_subelements: wire.linked_subelements,
                claim_child: wire.claim_child,
                copy_on_change: CopyOnChange::from_wire(
                    wire.copy_on_change,
                    wire.copy_on_change_source,
                    wire.copy_on_change_group,
                    wire.copy_on_change_touched,
                )?,
                scale: wire
                    .scale
                    .map(|values| {
                        FiniteVector::new(values).ok_or("scale vector components must be finite")
                    })
                    .transpose()
                    .map_err(|error| format!("scale: {error}"))?,
            })),
            _ => return Err("unknown product node kind".to_owned()),
        };
        Ok(Self {
            id: wire.id,
            object: wire.object,
            node,
        })
    }
}

/// One persisted GUI view provider linked to an application object when available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GuiViewProviderRecord {
    /// Stable native identity.
    pub(crate) id: String,
    /// Application object identity, or `None` for a GUI-only provider.
    pub(crate) object: Option<NonBlankString>,
    /// Persisted provider name.
    pub(crate) name: String,
    /// Persisted tree-expansion state.
    pub(crate) expanded: Option<bool>,
    /// Source order in `ViewProviderData`.
    pub(crate) order: usize,
}

/// One persisted property owned by a GUI view provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GuiPropertyRecord {
    /// Stable native identity.
    pub(crate) id: String,
    /// Owning view-provider identity.
    pub(crate) owner: String,
    /// Persisted property name.
    pub(crate) name: String,
    /// Runtime property type.
    pub(crate) type_name: String,
    /// Native status bits.
    pub(crate) status: Option<u64>,
    /// Source order within the provider.
    pub(crate) order: usize,
    /// Ordered value elements.
    pub(crate) values: Vec<ValueRecord>,
    /// Referenced archive entries.
    pub(crate) side_entries: Vec<String>,
}

/// ZIP physical-ledger role stored on one archive span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArchiveSpanRole {
    /// ZIP local-header signature for the named entry.
    LocalSignature(String),
    /// ZIP local-header fields for the named entry.
    LocalFields(String),
    /// ZIP local-header name for the named entry.
    LocalName(String),
    /// ZIP local-header extra data for the named entry.
    LocalExtra(String),
    /// ZIP compressed payload for the named entry.
    CompressedPayload(String),
    /// ZIP data descriptor for the named entry.
    DataDescriptor(String),
    /// ZIP padding following the named entry.
    EntryArchivePadding(String),
    /// ZIP central-header signature for the named entry.
    CentralSignature(String),
    /// ZIP central-header fields for the named entry.
    CentralFields(String),
    /// ZIP central-header name for the named entry.
    CentralName(String),
    /// ZIP central-header extra data for the named entry.
    CentralExtra(String),
    /// ZIP central-header comment for the named entry.
    CentralComment(String),
    /// ZIP64 end-of-central-directory record.
    Zip64EndRecord,
    /// ZIP64 end-of-central-directory locator.
    Zip64EndLocator,
    /// ZIP end-of-central-directory record.
    EndRecord,
    /// ZIP padding not owned by an entry.
    ArchivePadding,
}

impl From<&cadmpeg_container::ZipSpanRole> for ArchiveSpanRole {
    fn from(role: &cadmpeg_container::ZipSpanRole) -> Self {
        use cadmpeg_container::ZipSpanRole;
        match role {
            ZipSpanRole::LocalSignature(entry) => Self::LocalSignature(entry.clone()),
            ZipSpanRole::LocalFields(entry) => Self::LocalFields(entry.clone()),
            ZipSpanRole::LocalName(entry) => Self::LocalName(entry.clone()),
            ZipSpanRole::LocalExtra(entry) => Self::LocalExtra(entry.clone()),
            ZipSpanRole::CompressedPayload(entry) => Self::CompressedPayload(entry.clone()),
            ZipSpanRole::DataDescriptor(entry) => Self::DataDescriptor(entry.clone()),
            ZipSpanRole::CentralSignature(entry) => Self::CentralSignature(entry.clone()),
            ZipSpanRole::CentralFields(entry) => Self::CentralFields(entry.clone()),
            ZipSpanRole::CentralName(entry) => Self::CentralName(entry.clone()),
            ZipSpanRole::CentralExtra(entry) => Self::CentralExtra(entry.clone()),
            ZipSpanRole::CentralComment(entry) => Self::CentralComment(entry.clone()),
            ZipSpanRole::Padding { entry: Some(entry) } => Self::EntryArchivePadding(entry.clone()),
            ZipSpanRole::Padding { entry: None } => Self::ArchivePadding,
            ZipSpanRole::Zip64EndRecord => Self::Zip64EndRecord,
            ZipSpanRole::Zip64EndLocator => Self::Zip64EndLocator,
            ZipSpanRole::EndRecord => Self::EndRecord,
        }
    }
}

impl ArchiveSpanRole {
    /// Stable physical-ledger label retained on the CADIR wire.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::LocalSignature(_) => "local-signature",
            Self::LocalFields(_) => "local-fields",
            Self::LocalName(_) => "local-name",
            Self::LocalExtra(_) => "local-extra",
            Self::CompressedPayload(_) => "compressed-payload",
            Self::DataDescriptor(_) => "data-descriptor",
            Self::EntryArchivePadding(_) | Self::ArchivePadding => "archive-padding",
            Self::CentralSignature(_) => "central-signature",
            Self::CentralFields(_) => "central-fields",
            Self::CentralName(_) => "central-name",
            Self::CentralExtra(_) => "central-extra",
            Self::CentralComment(_) => "central-comment",
            Self::Zip64EndRecord => "zip64-end-record",
            Self::Zip64EndLocator => "zip64-end-locator",
            Self::EndRecord => "end-record",
        }
    }

    /// Owning entry, when the role is entry-owned.
    fn entry(&self) -> Option<&str> {
        match self {
            Self::LocalSignature(entry)
            | Self::LocalFields(entry)
            | Self::LocalName(entry)
            | Self::LocalExtra(entry)
            | Self::CompressedPayload(entry)
            | Self::DataDescriptor(entry)
            | Self::EntryArchivePadding(entry)
            | Self::CentralSignature(entry)
            | Self::CentralFields(entry)
            | Self::CentralName(entry)
            | Self::CentralExtra(entry)
            | Self::CentralComment(entry) => Some(entry),
            Self::Zip64EndRecord
            | Self::Zip64EndLocator
            | Self::EndRecord
            | Self::ArchivePadding => None,
        }
    }

    fn from_label(label: &str, entry: Option<String>) -> Result<Self, String> {
        let named = |entry: Option<String>, ctor: fn(String) -> Self| {
            entry
                .map(ctor)
                .ok_or_else(|| format!("archive span role {label} requires an owning entry"))
        };
        let unit = |entry: Option<String>, role: Self| {
            if entry.is_some() {
                Err(format!(
                    "archive span role {label} cannot carry an owning entry"
                ))
            } else {
                Ok(role)
            }
        };
        match label {
            "local-signature" => named(entry, Self::LocalSignature),
            "local-fields" => named(entry, Self::LocalFields),
            "local-name" => named(entry, Self::LocalName),
            "local-extra" => named(entry, Self::LocalExtra),
            "compressed-payload" => named(entry, Self::CompressedPayload),
            "data-descriptor" => named(entry, Self::DataDescriptor),
            "archive-padding" => match entry {
                Some(entry) => Ok(Self::EntryArchivePadding(entry)),
                None => Ok(Self::ArchivePadding),
            },
            "central-signature" => named(entry, Self::CentralSignature),
            "central-fields" => named(entry, Self::CentralFields),
            "central-name" => named(entry, Self::CentralName),
            "central-extra" => named(entry, Self::CentralExtra),
            "central-comment" => named(entry, Self::CentralComment),
            "zip64-end-record" => unit(entry, Self::Zip64EndRecord),
            "zip64-end-locator" => unit(entry, Self::Zip64EndLocator),
            "end-record" => unit(entry, Self::EndRecord),
            _ => Err(format!("unknown archive span role {label}")),
        }
    }
}

/// One physical archive span.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ArchiveSpanWire")]
pub(crate) struct ArchiveSpan {
    /// Stable span identity.
    pub(crate) id: String,
    /// Nonempty byte interval.
    pub(crate) span: ByteSpan,
    /// Structural role.
    pub(crate) role: ArchiveSpanRole,
}

#[derive(Deserialize)]
struct ArchiveSpanWire {
    id: String,
    start: u64,
    end: u64,
    role: String,
    entry: Option<String>,
}

#[derive(Serialize)]
struct ArchiveSpanOut<'a> {
    id: &'a str,
    start: u64,
    end: u64,
    role: &'static str,
    entry: Option<&'a str>,
}

impl Serialize for ArchiveSpan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ArchiveSpanOut {
            id: &self.id,
            start: self.span.start(),
            end: self.span.end(),
            role: self.role.as_str(),
            entry: self.role.entry(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<ArchiveSpanWire> for ArchiveSpan {
    type Error = String;

    fn try_from(wire: ArchiveSpanWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            span: ByteSpan::try_new(wire.start, wire.end)?,
            role: ArchiveSpanRole::from_label(&wire.role, wire.entry)?,
        })
    }
}

/// Structural document-kind classification from declared object domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DocumentKind {
    /// At least one `Assembly::` object.
    Assembly,
    /// At least one `TechDraw::` object and no assembly objects.
    Drawing,
    /// At least one `PartDesign::` object and no assembly or drawing objects.
    PartDesign,
    /// At least one `Part::` object and no assembly, drawing, or part-design objects.
    Part,
    /// No declared application objects.
    Empty,
    /// Declared objects with none of the classified domains.
    ApplicationDocument,
}

impl DocumentKind {
    /// Stable document-kind label retained on the CADIR wire.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Assembly => "assembly",
            Self::Drawing => "drawing",
            Self::PartDesign => "part-design",
            Self::Part => "part",
            Self::Empty => "empty",
            Self::ApplicationDocument => "application-document",
        }
    }
}

/// File-version declaration with its exact spelling and optional interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileVersion {
    spelling: String,
    value: Option<usize>,
}
impl From<String> for FileVersion {
    fn from(spelling: String) -> Self {
        let value = spelling.parse().ok();
        Self { spelling, value }
    }
}
impl FileVersion {
    pub(crate) fn value(&self) -> Option<usize> {
        self.value
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.spelling
    }
}

/// Metadata read from the persistence document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DocumentFactsWire")]
pub(crate) struct DocumentFacts {
    /// Stable document-record identity.
    pub(crate) id: String,
    /// Persistence file version.
    pub(crate) file_version: FileVersion,
    /// Producing application version, when carried.
    pub(crate) program_version: Option<String>,
    /// XML document element name.
    pub(crate) root_name: String,
    /// Number of declared application objects.
    pub(crate) object_count: usize,
    /// Application domains present in object declarations.
    pub(crate) domains: Vec<String>,
}

impl DocumentFacts {
    /// Structural document-kind classification.
    pub(crate) fn document_kind(&self) -> DocumentKind {
        if self.domains.iter().any(|domain| domain == "Assembly") {
            DocumentKind::Assembly
        } else if self.domains.iter().any(|domain| domain == "TechDraw") {
            DocumentKind::Drawing
        } else if self.domains.iter().any(|domain| domain == "PartDesign") {
            DocumentKind::PartDesign
        } else if self.domains.iter().any(|domain| domain == "Part") {
            DocumentKind::Part
        } else if self.object_count == 0 {
            DocumentKind::Empty
        } else {
            DocumentKind::ApplicationDocument
        }
    }
}

#[derive(Deserialize)]
struct DocumentFactsWire {
    id: String,
    file_version: String,
    program_version: Option<String>,
    root_name: String,
    object_count: usize,
    domains: Vec<String>,
    document_kind: DocumentKind,
}

#[derive(Serialize)]
struct DocumentFactsOut<'a> {
    id: &'a str,
    file_version: &'a str,
    program_version: Option<&'a str>,
    root_name: &'a str,
    object_count: usize,
    domains: &'a [String],
    document_kind: DocumentKind,
}

impl Serialize for DocumentFacts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DocumentFactsOut {
            id: &self.id,
            file_version: self.file_version.as_str(),
            program_version: self.program_version.as_deref(),
            root_name: &self.root_name,
            object_count: self.object_count,
            domains: &self.domains,
            document_kind: self.document_kind(),
        }
        .serialize(serializer)
    }
}
impl TryFrom<DocumentFactsWire> for DocumentFacts {
    type Error = String;
    fn try_from(wire: DocumentFactsWire) -> Result<Self, Self::Error> {
        let value = Self {
            id: wire.id,
            file_version: wire.file_version.into(),
            program_version: wire.program_version,
            root_name: wire.root_name,
            object_count: wire.object_count,
            domains: wire.domains,
        };
        if wire.document_kind != value.document_kind() {
            return Err("document_kind disagrees with domains and object_count".to_owned());
        }
        Ok(value)
    }
}

/// One declared application object and its persistence state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ObjectRecordWire")]
pub(crate) struct ObjectRecord {
    /// Checked identity and its persisted source name.
    pub(crate) identity: object_identity::ObjectIdentity,
    /// Runtime type name.
    pub(crate) type_name: String,
    /// Persisted numeric identity, when present.
    pub(crate) persistent_id: Option<i64>,
    /// Optional custom view-provider type.
    pub(crate) view_type: Option<String>,
    /// Declaration attributes not projected into dedicated fields.
    pub(crate) attributes: BTreeMap<String, String>,
    /// Ordered dependency identities.
    pub(crate) dependencies: Vec<String>,
    /// Positive partial-load capability from the dependency record.
    pub(crate) dependency_allow_partial: Option<std::num::NonZeroU64>,
    /// Source-order index.
    pub(crate) order: usize,
}

impl ObjectRecord {
    pub(crate) fn id(&self) -> &String {
        self.identity.id()
    }

    pub(crate) fn name(&self) -> &String {
        self.identity.name()
    }
}

#[derive(Deserialize)]
struct ObjectRecordWire {
    id: String,
    name: String,
    type_name: String,
    persistent_id: Option<i64>,
    view_type: Option<String>,
    attributes: BTreeMap<String, String>,
    dependencies: Vec<String>,
    dependency_allow_partial: Option<i128>,
    order: usize,
}

#[derive(Serialize)]
struct ObjectRecordOut<'a> {
    id: &'a str,
    name: &'a str,
    type_name: &'a str,
    persistent_id: Option<i64>,
    view_type: Option<&'a str>,
    attributes: &'a BTreeMap<String, String>,
    dependencies: &'a [String],
    dependency_allow_partial: Option<i128>,
    order: usize,
}

impl Serialize for ObjectRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ObjectRecordOut {
            id: self.id(),
            name: self.name(),
            type_name: &self.type_name,
            persistent_id: self.persistent_id,
            view_type: self.view_type.as_deref(),
            attributes: &self.attributes,
            dependencies: &self.dependencies,
            dependency_allow_partial: self
                .dependency_allow_partial
                .map(|value| i128::from(value.get())),
            order: self.order,
        }
        .serialize(serializer)
    }
}

impl TryFrom<ObjectRecordWire> for ObjectRecord {
    type Error = String;

    fn try_from(wire: ObjectRecordWire) -> Result<Self, Self::Error> {
        Ok(Self {
            identity: object_identity::ObjectIdentity::try_new(wire.id, wire.name)?,
            type_name: wire.type_name,
            persistent_id: wire.persistent_id,
            view_type: wire.view_type,
            attributes: wire.attributes,
            dependencies: wire.dependencies,
            dependency_allow_partial: wire
                .dependency_allow_partial
                .map(|value| {
                    u64::try_from(value)
                        .ok()
                        .and_then(std::num::NonZeroU64::new)
                        .ok_or_else(|| "dependency_allow_partial must be positive".to_owned())
                })
                .transpose()?,
            order: wire.order,
        })
    }
}

/// One dynamic object extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExtensionRecord {
    /// Stable extension identity.
    pub(crate) id: String,
    /// Owning object identity.
    pub(crate) owner: String,
    /// Persisted extension name.
    pub(crate) name: String,
    /// Runtime extension type.
    pub(crate) type_name: String,
    /// Source-order index.
    pub(crate) order: usize,
}

/// Dynamic-property persistence metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DynamicPropertyMeta {
    /// Display group.
    pub(crate) group: String,
    /// Documentation string.
    pub(crate) documentation: Option<String>,
    /// Native attribute flags.
    pub(crate) attributes: Option<i64>,
    /// Read-only state.
    pub(crate) read_only: Option<bool>,
    /// Hidden state.
    pub(crate) hidden: Option<bool>,
}

/// External document named by a file path or a document identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExternalDocument {
    /// Path carried by a `file` attribute.
    File(NonBlankString),
    /// Document identity carried without a `file` attribute.
    Name(NonBlankString),
}

impl ExternalDocument {
    /// Document token retained on the CADIR wire.
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::File(value) | Self::Name(value) => value.as_str(),
        }
    }

    /// Attribute spelling retained on the CADIR wire.
    pub(crate) fn attribute(&self) -> Option<&'static str> {
        match self {
            Self::File(_) => Some("file"),
            Self::Name(_) => None,
        }
    }

    fn from_wire(
        document: Option<String>,
        attribute: Option<&str>,
    ) -> Result<Option<Self>, String> {
        match (document, attribute) {
            (None, None | Some("file")) => Ok(None),
            (Some(path), Some("file")) => NonBlankString::new(path)
                .map(Self::File)
                .map(Some)
                .ok_or_else(|| "external document file path must not be empty".to_owned()),
            (Some(name), None) => NonBlankString::new(name)
                .map(Self::Name)
                .map(Some)
                .ok_or_else(|| "external document name must not be empty".to_owned()),
            (_, Some(attribute)) => Err(format!(
                "external document attribute must be \"file\", not {attribute}"
            )),
        }
    }
}

/// One `XLink`, `PropertyLink`, or `PropertyLinkSub` target.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "LinkTargetWire")]
pub(crate) struct LinkTarget {
    /// External document, when the target is not local.
    document: Option<ExternalDocument>,
    /// Target object identity. Empty source names are absent.
    object: Option<NonBlankString>,
    /// Ordered subelement selectors.
    subelements: Vec<String>,
}

impl LinkTarget {
    /// Admits a target from a parsed link element, or absence when the element
    /// selects no document, object, or subelement.
    pub(crate) fn optional_from_wire(wire: LinkTargetWire) -> Result<Option<Self>, String> {
        let document =
            ExternalDocument::from_wire(wire.document, wire.document_attribute.as_deref())?;
        let object = wire.object.and_then(NonBlankString::new);
        if document.is_none() && object.is_none() && wire.subelements.is_empty() {
            return Ok(None);
        }
        Self::try_new(document, object, wire.subelements).map(Some)
    }

    /// Admits a target with a document, object, or subelement selection.
    fn try_new(
        document: Option<ExternalDocument>,
        object: Option<NonBlankString>,
        subelements: Vec<String>,
    ) -> Result<Self, String> {
        if document.is_none() && object.is_none() && subelements.is_empty() {
            return Err("link target requires document, object, or subelements".to_owned());
        }
        Ok(Self {
            document,
            object,
            subelements,
        })
    }

    /// External document when the target is not local.
    pub(crate) fn document(&self) -> Option<&ExternalDocument> {
        self.document.as_ref()
    }

    /// Ordered subelement selectors.
    pub(crate) fn subelements(&self) -> &[String] {
        &self.subelements
    }

    /// Sets a nonempty object identity.
    pub(crate) fn set_object(&mut self, object: NonBlankString) {
        self.object = Some(object);
    }

    /// Document token retained on the CADIR wire.
    pub(crate) fn document_name(&self) -> Option<&str> {
        self.document.as_ref().map(ExternalDocument::as_str)
    }

    /// Attribute spelling retained on the CADIR wire.
    pub(crate) fn document_attribute(&self) -> Option<&'static str> {
        self.document.as_ref().and_then(ExternalDocument::attribute)
    }

    /// Target object identity, omitting empty source names.
    pub(crate) fn object(&self) -> Option<&str> {
        self.object.as_ref().map(NonBlankString::as_str)
    }
}

/// The persisted link target fields.
#[derive(Deserialize)]
pub(crate) struct LinkTargetWire {
    pub(crate) document: Option<String>,
    pub(crate) document_attribute: Option<String>,
    pub(crate) object: Option<String>,
    pub(crate) subelements: Vec<String>,
}

#[derive(Serialize)]
struct LinkTargetOut<'a> {
    document: Option<&'a str>,
    document_attribute: Option<&'static str>,
    object: Option<&'a str>,
    subelements: &'a [String],
}

impl Serialize for LinkTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LinkTargetOut {
            document: self.document_name(),
            document_attribute: self.document_attribute(),
            // An absent object identity writes the empty name the source carried.
            object: Some(self.object().unwrap_or("")),
            subelements: &self.subelements,
        }
        .serialize(serializer)
    }
}

impl TryFrom<LinkTargetWire> for LinkTarget {
    type Error = String;

    fn try_from(wire: LinkTargetWire) -> Result<Self, Self::Error> {
        let document =
            ExternalDocument::from_wire(wire.document, wire.document_attribute.as_deref())?;
        let object = wire.object.and_then(NonBlankString::new);
        Self::try_new(document, object, wire.subelements)
    }
}

/// A decoded JSON value. The wrapper distinguishes a held JSON null from an
/// absent decoded value without giving the optional key two wire spellings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecodedJsonValue {
    pub(crate) value: serde_json::Value,
}

impl From<serde_json::Value> for DecodedJsonValue {
    fn from(value: serde_json::Value) -> Self {
        Self { value }
    }
}

cadmpeg_core::named_optional_field!(deserialize_decoded_json, DecodedJsonValue, "decoded_json");

/// One property value element retained in source order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct ValueRecord {
    /// Value element tag.
    pub(crate) tag: String,
    /// Ordered position among value elements.
    pub(crate) order: usize,
    /// Parent value index; absent for a direct property child.
    pub(crate) parent: Option<usize>,
    /// Value attributes.
    pub(crate) attributes: BTreeMap<String, String>,
    /// Decoded JSON-mode Python value; encoded source attributes are layout only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_decoded_json"
    )]
    pub(crate) decoded_json: Option<DecodedJsonValue>,
    /// Direct text content.
    pub(crate) text: Option<String>,
}

impl Serialize for ValueRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct ValueOut<'a> {
            tag: &'a str,
            order: usize,
            parent: Option<usize>,
            attributes: BTreeMap<&'a str, &'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            decoded_json: Option<&'a DecodedJsonValue>,
            text: Option<&'a str>,
        }
        let python = self.tag == "Python";
        ValueOut {
            tag: &self.tag,
            order: self.order,
            parent: self.parent,
            attributes: self
                .attributes
                .iter()
                .filter(|(key, _)| {
                    !python
                        || !matches!(
                            key.as_str(),
                            "value" | "Value" | "data" | "encoded" | "json"
                        )
                })
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect(),
            decoded_json: self.decoded_json.as_ref(),
            text: self.text.as_deref(),
        }
        .serialize(serializer)
    }
}

/// Persisted values of a property, or a status-only transient declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PropertyBody {
    /// Persisted payload is unreadable; only its exact XML is authoritative.
    Unreadable(String),
    /// Status-only transient property declaration.
    Transient,
    /// Persisted property payload.
    Persisted {
        /// Ordered value elements.
        values: Vec<ValueRecord>,
        /// Generically recovered ordered link targets; `None` is a null link.
        links: Vec<Option<LinkTarget>>,
        /// Referenced archive entries.
        side_entries: Vec<String>,
        /// Dynamic-property metadata, when carried.
        dynamic: Option<DynamicPropertyMeta>,
    },
}

/// One persisted property.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "PropertyRecordWire")]
pub(crate) struct PropertyRecord {
    /// Stable native identity.
    pub(crate) id: String,
    /// Owning native object or document identity.
    pub(crate) owner: String,
    /// Persisted property name.
    pub(crate) name: String,
    /// Runtime property type.
    pub(crate) type_name: String,
    /// Broad persistence value family selected by the property type.
    pub(crate) family: PropertyFamily,
    /// Native status bits.
    pub(crate) status: Option<u64>,
    /// Transient declaration or persisted payload.
    pub(crate) body: PropertyBody,
    /// Source-order index within the owner.
    pub(crate) order: usize,
    /// Retained XML and its byte span.
    pub(crate) xml: RetainedXml,
}

/// More than one property matches a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DuplicateProperty;

/// Returns the sole property selected by a predicate.
pub(crate) fn unique_property<P>(
    properties: impl IntoIterator<Item = P>,
    predicate: impl Fn(&PropertyRecord) -> bool,
) -> Result<Option<P>, DuplicateProperty>
where
    P: std::ops::Deref<Target = PropertyRecord>,
{
    let mut matches = properties
        .into_iter()
        .filter(|property| predicate(property));
    let property = matches.next();
    if matches.next().is_some() {
        return Err(DuplicateProperty);
    }
    Ok(property)
}

/// Returns the sole property with a name.
///
/// `owner` is the noun that names the property carrier in the duplicate
/// diagnostic.
pub(crate) fn sole_named_property<'a>(
    owner: &str,
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    match unique_property(properties.iter().copied(), |property| property.name == name) {
        Ok(property) => Ok(property),
        Err(_) => Err(CodecError::malformed(format_args!(
            "{owner} property {name} occurs more than once"
        ))),
    }
}

/// Wraps a decode message in the malformed-source error variant.
pub(crate) fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}

/// Reads a `FreeCAD` boolean property text, which is `true`, `false`, `1` or `0`.
pub(crate) fn parse_bool(value: &str) -> Option<bool> {
    if value == "1" || value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value == "0" || value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

impl PropertyRecord {
    fn payload_error(&self) -> Option<&str> {
        match &self.body {
            PropertyBody::Unreadable(detail) => Some(detail),
            _ => None,
        }
    }

    /// Whether this is a status-only transient property declaration.
    pub(crate) fn is_transient(&self) -> bool {
        matches!(self.body, PropertyBody::Transient)
    }

    /// Ordered value elements; empty for a transient declaration.
    pub(crate) fn values(&self) -> &[ValueRecord] {
        match &self.body {
            PropertyBody::Persisted { values, .. } => values,
            PropertyBody::Transient | PropertyBody::Unreadable(_) => &[],
        }
    }

    /// Recovered link targets; empty for a transient declaration.
    pub(crate) fn links(&self) -> &[Option<LinkTarget>] {
        match &self.body {
            PropertyBody::Persisted { links, .. } => links,
            PropertyBody::Transient | PropertyBody::Unreadable(_) => &[],
        }
    }

    /// Referenced archive entries; empty for a transient declaration.
    pub(crate) fn side_entries(&self) -> &[String] {
        match &self.body {
            PropertyBody::Persisted { side_entries, .. } => side_entries,
            PropertyBody::Transient | PropertyBody::Unreadable(_) => &[],
        }
    }

    /// Dynamic-property metadata; absent for a transient declaration.
    fn dynamic(&self) -> Option<&DynamicPropertyMeta> {
        match &self.body {
            PropertyBody::Persisted { dynamic, .. } => dynamic.as_ref(),
            PropertyBody::Transient | PropertyBody::Unreadable(_) => None,
        }
    }

    pub(crate) fn values_mut(&mut self) -> Option<&mut Vec<ValueRecord>> {
        match &mut self.body {
            PropertyBody::Persisted { values, .. } => Some(values),
            PropertyBody::Transient | PropertyBody::Unreadable(_) => None,
        }
    }
}

#[derive(Deserialize)]
struct PropertyRecordWire {
    id: String,
    owner: String,
    name: String,
    type_name: String,
    family: PropertyFamily,
    status: Option<u64>,
    transient: bool,
    payload_error: Option<String>,
    dynamic: Option<DynamicPropertyMeta>,
    order: usize,
    values: Vec<ValueRecord>,
    links: Vec<Option<LinkTarget>>,
    side_entries: Vec<String>,
    #[serde(flatten)]
    evidence: RetainedXml,
}

#[derive(Serialize)]
struct PropertyRecordOut<'a> {
    id: &'a str,
    owner: &'a str,
    name: &'a str,
    type_name: &'a str,
    family: PropertyFamily,
    status: Option<u64>,
    transient: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_error: Option<&'a str>,
    dynamic: Option<&'a DynamicPropertyMeta>,
    order: usize,
    values: &'a [ValueRecord],
    links: &'a [Option<LinkTarget>],
    side_entries: &'a [String],
    #[serde(flatten)]
    evidence: &'a RetainedXml,
}

impl Serialize for PropertyRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PropertyRecordOut {
            id: &self.id,
            owner: &self.owner,
            name: &self.name,
            type_name: &self.type_name,
            family: self.family,
            status: self.status,
            transient: self.is_transient(),
            payload_error: self.payload_error(),
            // A transient declaration writes no payload: the accessors state its
            // empty tables and absent metadata.
            dynamic: self.dynamic(),
            order: self.order,
            values: self.values(),
            links: self.links(),
            side_entries: self.side_entries(),
            evidence: &self.xml,
        }
        .serialize(serializer)
    }
}

impl TryFrom<PropertyRecordWire> for PropertyRecord {
    type Error = String;

    fn try_from(wire: PropertyRecordWire) -> Result<Self, Self::Error> {
        for (index, value) in wire.values.iter().enumerate() {
            if value.order != index || value.parent.is_some_and(|parent| parent >= index) {
                return Err(
                    "property values require contiguous order and backward parent references"
                        .into(),
                );
            }
        }
        let body = if let Some(detail) = wire.payload_error {
            if detail.trim().is_empty()
                || wire.transient
                || !wire.values.is_empty()
                || !wire.links.is_empty()
                || !wire.side_entries.is_empty()
                || wire.dynamic.is_some()
            {
                return Err(
                    "unreadable property requires a diagnostic and no admitted payload".to_owned(),
                );
            }
            PropertyBody::Unreadable(detail)
        } else if wire.transient {
            if !wire.values.is_empty()
                || !wire.links.is_empty()
                || !wire.side_entries.is_empty()
                || wire.dynamic.is_some()
            {
                return Err(
                    "transient property cannot carry values, links, side entries, or dynamic metadata"
                        .to_owned(),
                );
            }
            PropertyBody::Transient
        } else {
            PropertyBody::Persisted {
                values: wire.values,
                links: wire.links,
                side_entries: wire.side_entries,
                dynamic: wire.dynamic,
            }
        };
        Ok(Self {
            id: wire.id,
            owner: wire.owner,
            name: wire.name,
            type_name: wire.type_name,
            family: wire.family,
            status: wire.status,
            body,
            order: wire.order,
            xml: wire.evidence,
        })
    }
}

/// Format-level property value families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PropertyFamily {
    /// Boolean, integer, or floating scalar.
    Scalar,
    /// Unit-bearing quantity.
    Quantity,
    /// Enumeration value and option table.
    Enumeration,
    /// Two-, three-, or four-component vector.
    Vector,
    /// Matrix value.
    Matrix,
    /// Placement or transform value.
    Placement,
    /// Text, path, UUID, or byte-string value.
    String,
    /// Key/value map.
    Map,
    /// Ordered list.
    List,
    /// Object, subobject, or external link.
    Link,
    /// Expression bindings.
    Expression,
    /// Embedded or external file reference.
    File,
    /// Exact geometry, mesh, or point carrier.
    Geometry,
    /// Inert serialized Python value.
    PythonObject,
    /// Type without a settled family mapping.
    Unknown,
}

/// One logical archive entry and its graph ownership.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "EntryRecordWire")]
pub(crate) struct EntryRecord {
    id: cadmpeg_ir::ids::Identity,
    name: ArchiveEntryName,
    pub(crate) role: cadmpeg_core::container::ContainerRole,
    referenced_by: EntryReferences,
    data: NativeBytes,
    length: u64,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveEntryName(String);

impl TryFrom<String> for ArchiveEntryName {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_safe_entry_name(&value) {
            Ok(Self(value))
        } else {
            Err("unsafe ZIP entry path")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryReferences(Vec<String>);

impl TryFrom<Vec<String>> for EntryReferences {
    type Error = String;
    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
        if value
            .iter()
            .all(|id| cadmpeg_ir::ids::is_valid_identity(id))
        {
            Ok(Self(value))
        } else {
            Err("entry reference identity is invalid".into())
        }
    }
}

/// Check the exact ZIP name used by source scans and retained entry records.
pub(crate) fn is_safe_entry_name(name: &str) -> bool {
    !name.contains('\\')
        && !name
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
}

impl EntryRecord {
    pub(crate) fn new(
        id: String,
        name: String,
        role: cadmpeg_core::container::ContainerRole,
        referenced_by: Vec<String>,
        data: Vec<u8>,
    ) -> Result<Self, CodecError> {
        let id = cadmpeg_ir::ids::Identity::new(id).map_err(CodecError::malformed)?;
        let name = ArchiveEntryName::try_from(name).map_err(CodecError::malformed)?;
        let referenced_by =
            EntryReferences::try_from(referenced_by).map_err(CodecError::malformed)?;
        let sha256 = sha256_hex(&data);
        let length = cadmpeg_core::decode::u64_from_index(data.len());
        Ok(Self {
            id,
            name,
            role,
            referenced_by,
            data: data.into(),
            length,
            sha256,
        })
    }

    pub(crate) fn id(&self) -> &str {
        self.id.as_str()
    }
    pub(crate) fn name(&self) -> &str {
        &self.name.0
    }
    pub(crate) fn referenced_by(&self) -> &[String] {
        &self.referenced_by.0
    }
    pub(crate) fn data(&self) -> &[u8] {
        self.data.as_ref()
    }
    pub(crate) fn byte_len(&self) -> u64 {
        self.length
    }
    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }

    pub(crate) fn add_reference(&mut self, owner: &str) -> Result<(), CodecError> {
        if !cadmpeg_ir::ids::is_valid_identity(owner) {
            return Err(CodecError::malformed("entry reference identity is invalid"));
        }
        for candidate in &self.referenced_by.0 {
            if candidate == owner {
                return Ok(());
            }
        }

        let owner = owner.to_owned();
        self.referenced_by.0.push(owner);
        Ok(())
    }
}

#[derive(Deserialize)]
struct EntryRecordWire {
    id: String,
    name: String,
    role: String,
    byte_len: u64,
    sha256: String,
    referenced_by: Vec<String>,
}

#[derive(Serialize)]
struct EntryRecordOut<'a> {
    id: &'a str,
    name: &'a str,
    role: &'static str,
    byte_len: u64,
    sha256: &'a str,
    referenced_by: &'a [String],
}

impl Serialize for EntryRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EntryRecordOut {
            id: self.id(),
            name: self.name(),
            role: self.role.as_str(),
            byte_len: self.byte_len(),
            sha256: self.sha256(),
            referenced_by: self.referenced_by(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<EntryRecordWire> for EntryRecord {
    type Error = String;

    fn try_from(wire: EntryRecordWire) -> Result<Self, Self::Error> {
        let id = cadmpeg_ir::ids::Identity::new(wire.id).map_err(|error| error.to_string())?;
        let name = ArchiveEntryName::try_from(wire.name).map_err(str::to_owned)?;
        let referenced_by = EntryReferences::try_from(wire.referenced_by)?;
        if !valid_sha256(&wire.sha256) {
            return Err("entry sha256 must use lowercase hexadecimal".to_owned());
        }
        let record = Self {
            id,
            name,
            role: serde_json::from_value(serde_json::Value::String(wire.role))
                .map_err(|error| format!("entry role: {error}"))?,
            referenced_by,
            data: Vec::new().into(),
            length: wire.byte_len,
            sha256: wire.sha256,
        };
        Ok(record)
    }
}

/// One non-overlapping logical-entry byte span.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "LogicalSpanWire")]
pub(crate) struct LogicalSpan {
    /// Stable span identity.
    pub(crate) id: String,
    /// Owning archive entry.
    pub(crate) entry: String,
    /// Nonempty byte interval.
    pub(crate) span: ByteSpan,
    /// Span family and owner.
    pub(crate) classification: LogicalClassification,
}

/// Logical-entry family. Typed and named-opaque spans own a native record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LogicalClassification {
    /// Framing bytes with no native owner.
    Structural,
    /// Typed native record bytes.
    Typed {
        /// Native record that owns the bytes.
        owner: String,
    },
    /// Named opaque payload bytes.
    NamedOpaque {
        /// Native record that owns the bytes.
        owner: String,
    },
}

impl LogicalClassification {
    /// Classification string retained on the CADIR wire.
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Structural => "structural",
            Self::Typed { .. } => "typed",
            Self::NamedOpaque { .. } => "named_opaque",
        }
    }

    /// Native record that owns typed or opaque bytes.
    pub(crate) fn owner(&self) -> Option<&str> {
        match self {
            Self::Structural => None,
            Self::Typed { owner } | Self::NamedOpaque { owner } => Some(owner.as_str()),
        }
    }
}

#[derive(Deserialize)]
struct LogicalSpanWire {
    id: String,
    entry: String,
    start: u64,
    end: u64,
    classification: String,
    owner: Option<String>,
}

#[derive(Serialize)]
struct LogicalSpanOut<'a> {
    id: &'a str,
    entry: &'a str,
    start: u64,
    end: u64,
    classification: &'static str,
    owner: Option<&'a str>,
}

impl Serialize for LogicalSpan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LogicalSpanOut {
            id: &self.id,
            entry: &self.entry,
            start: self.span.start(),
            end: self.span.end(),
            classification: self.classification.as_str(),
            owner: self.classification.owner(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<LogicalSpanWire> for LogicalSpan {
    type Error = String;

    fn try_from(wire: LogicalSpanWire) -> Result<Self, Self::Error> {
        let classification = match (wire.classification.as_str(), wire.owner) {
            ("structural", None) => LogicalClassification::Structural,
            ("typed", Some(owner)) => LogicalClassification::Typed { owner },
            ("named_opaque", Some(owner)) => LogicalClassification::NamedOpaque { owner },
            ("structural", Some(_)) => {
                return Err("structural logical span cannot carry an owner".to_owned());
            }
            ("typed" | "named_opaque", None) => {
                return Err("typed and named_opaque logical spans require an owner".to_owned());
            }
            _ => return Err("unknown logical classification".to_owned()),
        };
        Ok(Self {
            id: wire.id,
            entry: wire.entry,
            span: ByteSpan::try_new(wire.start, wire.end)?,
            classification,
        })
    }
}

/// Deterministic whole-archive byte-accounting summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ByteCoverageRecord {
    /// Stable report identity.
    pub(crate) id: String,
    /// Complete physical archive length.
    pub(crate) physical_byte_len: u64,
    /// Number of physical partition spans.
    pub(crate) physical_span_count: usize,
    /// Number of logical archive entries.
    pub(crate) logical_entry_count: usize,
    /// Sum of complete logical entry lengths.
    pub(crate) logical_byte_len: u64,
    /// Number of logical partition spans.
    pub(crate) logical_span_count: usize,
    /// Byte count for each closed classification.
    pub(crate) classification_bytes: BTreeMap<String, u64>,
    /// Sorted logical entries containing named opaque bytes.
    pub(crate) named_opaque_entries: Vec<String>,
    /// Whether both physical and logical partitions close exactly.
    pub(crate) exact: bool,
}

/// Persistent string tables in contiguous numeric `HasherIndex` order.
///
/// Construction requires the records already in that order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<StringTableRecord>")]
pub(crate) struct StringTables(Vec<StringTableRecord>);

impl StringTables {
    pub(crate) fn as_slice(&self) -> &[StringTableRecord] {
        &self.0
    }
}

impl TryFrom<Vec<StringTableRecord>> for StringTables {
    type Error = cadmpeg_ir::native::NativeConvertError;

    fn try_from(records: Vec<StringTableRecord>) -> Result<Self, Self::Error> {
        for (position, record) in records.iter().enumerate() {
            if record.index != position {
                return Err(Self::Error::InvalidCollection(format!(
                    "string_tables[{position}].index must equal {position}, got {}",
                    record.index
                )));
            }
        }
        Ok(Self(records))
    }
}

/// One document-wide persistent string table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "StringTableRecordWire")]
pub(crate) struct StringTableRecord {
    /// Zero-based document table index referenced by shape properties.
    pub(crate) index: usize,
    /// Owning property when the table is serialized beside its first use.
    pub(crate) owner_property: Option<String>,
    /// Whether all strings were persisted; `None` for unreadable metadata.
    pub(crate) save_all: Option<bool>,
    /// Native hashing threshold; `None` for unreadable metadata.
    pub(crate) threshold: Option<i64>,
    /// Referenced side entry, or `None` for inline data.
    pub(crate) source_entry: Option<String>,
    /// Parsed records in serialized order; `None` when the payload is source-only.
    entries: Option<Vec<StringTableEntry>>,
}

impl StringTableRecord {
    pub(crate) fn try_new(
        index: usize,
        owner_property: Option<String>,
        save_all: Option<bool>,
        threshold: Option<i64>,
        source_entry: Option<String>,
        entries: Option<Vec<StringTableEntry>>,
    ) -> Result<Self, String> {
        let mut seen = std::collections::HashSet::new();
        for entry in entries.as_deref().unwrap_or_default() {
            if entry.components.iter().any(|id| !seen.contains(id)) {
                return Err("entries.components must reference earlier string_id values".to_owned());
            }
            if !seen.insert(entry.string_id) {
                return Err("entries.string_id values must be distinct".to_owned());
            }
        }
        Ok(Self {
            index,
            owner_property,
            save_all,
            threshold,
            source_entry,
            entries,
        })
    }

    /// Stable table identity derived from the document table index.
    pub(crate) fn id(&self) -> String {
        native_id("string-table", self.index.to_string())
    }

    pub(crate) fn entries(&self) -> Option<&[StringTableEntry]> {
        self.entries.as_deref()
    }

    /// Admitted serialized count; absent exactly when the table is unreadable.
    fn declared_count(&self) -> Option<usize> {
        self.entries().map(<[StringTableEntry]>::len)
    }
}

#[derive(Deserialize)]
struct StringTableRecordWire {
    id: String,
    index: usize,
    owner_property: Option<String>,
    save_all: Option<bool>,
    threshold: Option<i64>,
    declared_count: Option<usize>,
    source_entry: Option<String>,
    entries: Option<Vec<StringTableEntry>>,
}

#[derive(Serialize)]
struct StringTableRecordOut<'a> {
    id: String,
    index: usize,
    owner_property: Option<&'a str>,
    save_all: Option<bool>,
    threshold: Option<i64>,
    declared_count: Option<usize>,
    source_entry: Option<&'a str>,
    entries: Option<&'a [StringTableEntry]>,
}

impl Serialize for StringTableRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StringTableRecordOut {
            // The identity is the written value, derived from the table index.
            id: self.id(),
            index: self.index,
            owner_property: self.owner_property.as_deref(),
            save_all: self.save_all,
            threshold: self.threshold,
            declared_count: self.declared_count(),
            source_entry: self.source_entry.as_deref(),
            entries: self.entries(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<StringTableRecordWire> for StringTableRecord {
    type Error = String;

    fn try_from(wire: StringTableRecordWire) -> Result<Self, Self::Error> {
        let expected_id = native_id("string-table", wire.index.to_string());
        if wire.id != expected_id {
            return Err(format!(
                "string table id must be {expected_id} for index {}",
                wire.index
            ));
        }
        if wire.declared_count != wire.entries.as_ref().map(Vec::len) {
            return Err("string table declared_count must equal entries.len()".to_owned());
        }
        Self::try_new(
            wire.index,
            wire.owner_property,
            wire.save_all,
            wire.threshold,
            wire.source_entry,
            wire.entries,
        )
    }
}

/// One relative-coded record in a persistent string table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StringTableEntry {
    /// Restored numeric string identity.
    pub(crate) string_id: i64,
    /// Native flag word.
    pub(crate) flags: u64,
    /// Restored referenced string identities.
    pub(crate) components: Vec<i64>,
    /// Exact payload following the numeric header.
    pub(crate) payload: String,
}
