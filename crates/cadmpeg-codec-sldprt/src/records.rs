// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` parametric construction-history records.
#![deny(clippy::disallowed_methods)]

use crate::brep::feature_source::FeatureSourceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod debug;
pub(crate) mod operand_tag;
pub(crate) mod relation_scalars;
pub(crate) mod sketch_code;

/// One semantic product-manufacturing dimension from `PMISemanticDataDB`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmiDimension {
    /// Globally unique source-derived record id.
    pub(crate) id: String,
    /// Source block containing this record.
    pub(crate) parent: String,
    /// Byte offset of the `MessagePack` map within the decompressed block.
    pub(crate) offset: u64,
    /// `UnQLite` record key.
    pub(crate) guid: String,
    /// CAD dimension reference, such as `D1@Sketch4`.
    pub(crate) cad_text: String,
    /// Number of elements in the source `dimItems` array.
    #[serde(default = "default_pmi_item_count", skip_serializing_if = "is_one")]
    pub(crate) item_count: u32,
    /// Native PMI dimension subtype.
    pub(crate) subtype: String,
    /// Stored dimension value.
    pub(crate) value: f64,
    /// Byte offset of the big-endian `f64` value.
    pub(crate) value_offset: u64,
    /// Display precision.
    pub(crate) precision: i64,
    /// Byte offset of the `MessagePack` precision value.
    pub(crate) precision_offset: u64,
    /// Native formatted dimension text and its byte offset.
    #[serde(flatten, with = "pmi_display_text_wire")]
    pub(crate) display_text: Option<(String, u64)>,
    /// Basic-dimension flag.
    pub(crate) basic: bool,
    /// Byte offset of the basic flag.
    pub(crate) basic_offset: u64,
    /// Inspection-dimension flag.
    pub(crate) inspection: bool,
    /// Byte offset of the inspection flag.
    pub(crate) inspection_offset: u64,
    /// Reference-only flag.
    pub(crate) reference_only: bool,
    /// Byte offset of the reference-only flag.
    pub(crate) reference_only_offset: u64,
}

impl PmiDimension {
    pub(crate) fn display_text(&self) -> Option<&str> {
        self.display_text.as_ref().map(|(text, _)| text.as_str())
    }

    pub(crate) fn display_text_offset(&self) -> Option<u64> {
        self.display_text.as_ref().map(|(_, offset)| *offset)
    }
}

mod pmi_display_text_wire {
    use serde::{ser::SerializeMap, Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    pub(super) struct Wire {
        #[serde(default)]
        display_text: Option<String>,
        #[serde(default)]
        display_text_offset: Option<u64>,
    }

    // Serde passes the field by reference to this adapter.
    #[allow(clippy::ref_option)]
    pub(super) fn serialize<S: Serializer>(
        display: &Option<(String, u64)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        if let Some((text, offset)) = display {
            map.serialize_entry("display_text", text)?;
            map.serialize_entry("display_text_offset", offset)?;
        }
        map.end()
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<(String, u64)>, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        match (wire.display_text, wire.display_text_offset) {
            (Some(text), Some(offset)) => Ok(Some((text, offset))),
            (None, None) => Ok(None),
            _ => Err(serde::de::Error::custom(
                "display_text and display_text_offset must be present together",
            )),
        }
    }
}

fn default_pmi_item_count() -> u32 {
    1
}

// Serde's `skip_serializing_if` contract passes the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one(value: &u32) -> bool {
    *value == 1
}

/// A named parametric-model variant (e.g. CAD "configuration") with its own
/// material and property overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Configuration {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Owning feature-history record id.
    pub(crate) parent: String,
    /// Position in the source configuration list.
    #[serde(default)]
    pub(crate) ordinal: u32,
    /// Numeric key used by configuration-scoped container sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_index: Option<u32>,
    /// Source configuration name.
    pub(crate) name: String,
    /// Material assigned in this configuration, when overridden; `None` when the
    /// configuration inherits the part's default material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) material: Option<String>,
    /// Source custom-property name/value pairs local to this configuration.
    #[serde(default)]
    pub(crate) properties: BTreeMap<String, String>,
}

fn default_feature_xml_tag() -> String {
    "Feature".into()
}

/// A native feature-object identifier, or the reserved marker the source writes on records
/// that carry no object identity of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) enum FeatureSource {
    /// The reserved `-1` marker.
    Reserved,
    /// A native feature-object identifier.
    Id(FeatureSourceId),
}

impl FeatureSource {
    /// The native identifier, when this source is not the reserved marker.
    pub(crate) fn id(self) -> Option<FeatureSourceId> {
        match self {
            Self::Reserved => None,
            Self::Id(id) => Some(id),
        }
    }

    /// The native identifier value, when this source is not the reserved marker.
    pub(crate) fn value(self) -> Option<u32> {
        self.id().map(FeatureSourceId::value)
    }

    /// The source for a native identifier value, when the value is a real identifier.
    pub(crate) fn from_value(value: u32) -> Option<Self> {
        FeatureSourceId::try_from(value).ok().map(Self::Id)
    }
}

impl TryFrom<&str> for FeatureSource {
    type Error = &'static str;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value == RESERVED_FEATURE_SOURCE {
            return Ok(Self::Reserved);
        }
        value
            .parse::<u32>()
            .map_err(|_| "source_id is not a native feature-object identifier")
            .and_then(|value| FeatureSourceId::try_from(value).map(Self::Id))
    }
}

impl TryFrom<String> for FeatureSource {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl From<FeatureSource> for String {
    fn from(value: FeatureSource) -> Self {
        match value {
            FeatureSource::Reserved => RESERVED_FEATURE_SOURCE.to_string(),
            FeatureSource::Id(id) => id.value().to_string(),
        }
    }
}

/// The wire spelling of the reserved feature-source marker.
const RESERVED_FEATURE_SOURCE: &str = "-1";

/// A construction-tree parent reference.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TreeParent {
    Record {
        record_id: String,
        source_id: Option<FeatureSource>,
    },
    Source(FeatureSource),
}

impl TreeParent {
    pub(crate) fn record_id(&self) -> Option<&str> {
        match self {
            Self::Record { record_id, .. } => Some(record_id),
            Self::Source(_) => None,
        }
    }

    pub(crate) fn source_id(&self) -> Option<FeatureSource> {
        match self {
            Self::Record { source_id, .. } => *source_id,
            Self::Source(source_id) => Some(*source_id),
        }
    }
}

mod tree_parent_wire {
    use super::TreeParent;
    use serde::{ser::SerializeMap, Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    pub(super) struct Wire {
        #[serde(default)]
        tree_parent: Option<String>,
        #[serde(default)]
        parent_source_id: Option<super::FeatureSource>,
    }

    // Serde's field adapter borrows the complete optional parent field.
    #[allow(clippy::ref_option)]
    pub(super) fn serialize<S: Serializer>(
        parent: &Option<TreeParent>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        if let Some(parent) = parent {
            if let Some(record) = parent.record_id() {
                map.serialize_entry("tree_parent", record)?;
            }
            if let Some(source) = parent.source_id() {
                map.serialize_entry("parent_source_id", &source)?;
            }
        }
        map.end()
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<TreeParent>, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        Ok(match (wire.tree_parent, wire.parent_source_id) {
            (Some(record_id), source_id) => Some(TreeParent::Record {
                record_id,
                source_id,
            }),
            (None, Some(source_id)) => Some(TreeParent::Source(source_id)),
            (None, None) => None,
        })
    }
}

impl Feature {
    pub(crate) fn tree_parent_record_id(&self) -> Option<&str> {
        self.tree_parent.as_ref().and_then(TreeParent::record_id)
    }

    pub(crate) fn parent_source_id(&self) -> Option<FeatureSource> {
        self.tree_parent.as_ref().and_then(TreeParent::source_id)
    }

    /// The native identifier of this feature, when it carries a real one.
    pub(crate) fn source_value(&self) -> Option<u32> {
        self.source_id.and_then(FeatureSource::value)
    }
}

/// One parametric construction-history feature (e.g. an extrude or fillet operation).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Feature {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Owning feature-history record id.
    pub(crate) parent: String,
    /// XML element name carrying this feature record.
    #[serde(default = "default_feature_xml_tag")]
    pub(crate) xml_tag: String,
    /// Containing feature, identified by its record or legacy source id.
    #[serde(flatten, with = "tree_parent_wire")]
    pub(crate) tree_parent: Option<TreeParent>,
    /// Native identifier of this feature, when the source assigned one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_id: Option<FeatureSource>,
    /// Position of this feature in the construction-history timeline, in
    /// regeneration order.
    pub(crate) ordinal: u32,
    /// Feature display name.
    pub(crate) name: String,
    /// Native feature-type tag (e.g. `"Extrude"`, `"Fillet"`).
    pub(crate) kind: String,
    /// Serialized feature-input object class owning this feature, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) input_class: Option<String>,
    /// Whether this feature is suppressed and excluded from regeneration.
    #[serde(default)]
    pub(crate) suppressed: bool,
    /// Source parametric input values keyed by parameter name.
    #[serde(default)]
    pub(crate) parameters: BTreeMap<String, String>,
    /// Source attributes on each named dimension, excluding its `Name` key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) dimension_properties: BTreeMap<String, BTreeMap<String, String>>,
    /// Source custom-property name/value pairs local to this feature.
    #[serde(default)]
    pub(crate) properties: BTreeMap<String, String>,
    /// Text content of a native leaf feature element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
    /// Source order of dimensions, nested feature nodes, and text content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) content: Vec<FeatureContent>,
}

/// One ordered item inside a native feature XML element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum FeatureContent {
    /// Named dimension child.
    Dimension(String),
    /// Native record id of a nested feature child.
    Feature(String),
    /// Non-whitespace text content.
    Text(String),
}

/// One ordered item inside the native `Keywords` root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum HistoryContent {
    /// Native configuration record id.
    Configuration(String),
    /// Native top-level feature record id.
    Feature(String),
    /// Non-whitespace root text content.
    Text(String),
}

/// The full parametric construction-history timeline for a part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FeatureHistory {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Source part display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) part_name: Option<String>,
    /// Source attributes on the `Keywords` root, excluding its `Name` key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) properties: BTreeMap<String, String>,
    /// Source order of configurations, top-level features, and root text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) content: Vec<HistoryContent>,
    /// Named parametric-model variants defined on this part.
    #[serde(default)]
    pub(crate) configurations: Vec<Configuration>,
    /// Ordered construction-history features, in regeneration order.
    #[serde(default)]
    pub(crate) features: Vec<Feature>,
}

/// Native feature-input stream retained for parametric replay and rewrite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FeatureInputLaneWire")]
pub(crate) struct FeatureInputLane {
    /// Stable source-derived identifier for this feature-input record.
    pub(crate) id: String,
    /// Configuration this input lane applies to, when the source scoped inputs
    /// per configuration; `None` when the lane applies to all configurations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) configuration: Option<String>,
    /// Complete native feature-input byte stream, retained undecoded for
    /// parametric replay and native rewrite.
    #[serde(with = "cadmpeg_ir::bytes")]
    pub(crate) native_payload: Vec<u8>,
    /// Class declarations used by object instances in this lane.
    #[serde(default)]
    pub(crate) classes: Vec<FeatureInputClass>,
    /// Serialized object names in this lane.
    #[serde(default)]
    pub(crate) names: Vec<FeatureInputName>,
    /// Named scalar values in this lane.
    #[serde(default)]
    pub(crate) scalars: Vec<FeatureInputScalar>,
    /// Relation-class declarations bound to their attached scalar records.
    #[serde(default)]
    pub(crate) relation_bindings: Vec<FeatureInputRelationBinding>,
    /// Compact relation instances grouped by feature and operand identity.
    #[serde(default)]
    pub(crate) relation_instances: Vec<FeatureInputRelationInstance>,
    /// Compact body-selection vectors owned by feature objects in this lane.
    #[serde(default)]
    pub(crate) body_selections: Vec<FeatureInputBodySelection>,
    /// Compact edge-selection vectors owned by feature objects in this lane.
    #[serde(default)]
    pub(crate) edge_selections: Vec<FeatureInputEdgeSelection>,
    /// Compact surface-component selections owned by feature objects in this lane.
    #[serde(default)]
    pub(crate) surface_selections: Vec<FeatureInputSurfaceSelection>,
    /// Persistent identities of surfaces produced by regenerated features.
    #[serde(default)]
    pub(crate) generated_surface_identities: Vec<FeatureInputGeneratedSurfaceIdentity>,
    /// Native entity-reference cells in byte order.
    #[serde(default)]
    pub(crate) references: Vec<FeatureInputReference>,
    /// Typed sketch-entity markers located within `native_payload`.
    #[serde(default)]
    pub(crate) sketch_entities: Vec<SketchInputEntity>,
}

/// Deserialization mirror admitting every sketch-entity marker against this lane's payload.
#[derive(Deserialize)]
struct FeatureInputLaneWire {
    /// Stable source-derived identifier for this feature-input record.
    id: String,
    /// Configuration this input lane applies to, when the source scoped inputs
    /// per configuration; `None` when the lane applies to all configurations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    configuration: Option<String>,
    /// Complete native feature-input byte stream, retained undecoded for
    /// parametric replay and native rewrite.
    #[serde(with = "cadmpeg_ir::bytes")]
    native_payload: Vec<u8>,
    /// Class declarations used by object instances in this lane.
    #[serde(default)]
    classes: Vec<FeatureInputClass>,
    /// Serialized object names in this lane.
    #[serde(default)]
    names: Vec<FeatureInputName>,
    /// Named scalar values in this lane.
    #[serde(default)]
    scalars: Vec<FeatureInputScalar>,
    /// Relation-class declarations bound to their attached scalar records.
    #[serde(default)]
    relation_bindings: Vec<FeatureInputRelationBinding>,
    /// Compact relation instances grouped by feature and operand identity.
    #[serde(default)]
    relation_instances: Vec<FeatureInputRelationInstance>,
    /// Compact body-selection vectors owned by feature objects in this lane.
    #[serde(default)]
    body_selections: Vec<FeatureInputBodySelection>,
    /// Compact edge-selection vectors owned by feature objects in this lane.
    #[serde(default)]
    edge_selections: Vec<FeatureInputEdgeSelection>,
    /// Compact surface-component selections owned by feature objects in this lane.
    #[serde(default)]
    surface_selections: Vec<FeatureInputSurfaceSelection>,
    /// Persistent identities of surfaces produced by regenerated features.
    #[serde(default)]
    generated_surface_identities: Vec<FeatureInputGeneratedSurfaceIdentity>,
    /// Native entity-reference cells in byte order.
    #[serde(default)]
    references: Vec<FeatureInputReference>,
    /// Typed sketch-entity markers located within `native_payload`.
    #[serde(default)]
    sketch_entities: Vec<SketchInputEntityWire>,
}

impl TryFrom<FeatureInputLaneWire> for FeatureInputLane {
    type Error = String;
    fn try_from(wire: FeatureInputLaneWire) -> Result<Self, Self::Error> {
        let sketch_entities = wire
            .sketch_entities
            .into_iter()
            .map(|entity| SketchInputEntity::try_from_wire(entity, &wire.native_payload))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id: wire.id,
            configuration: wire.configuration,
            native_payload: wire.native_payload,
            classes: wire.classes,
            names: wire.names,
            scalars: wire.scalars,
            relation_bindings: wire.relation_bindings,
            relation_instances: wire.relation_instances,
            body_selections: wire.body_selections,
            edge_selections: wire.edge_selections,
            surface_selections: wire.surface_selections,
            generated_surface_identities: wire.generated_surface_identities,
            references: wire.references,
            sketch_entities,
        })
    }
}

/// One compact feature-local body-selection vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputBodySelection {
    /// Globally unique deterministic identifier for this vector.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Position among compact body-selection vectors in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the schema word opening the vector.
    pub(crate) offset: u64,
    /// Feature-input name record owning this vector.
    pub(crate) object_name_ref: String,
    /// Native history feature owning this vector.
    pub(crate) feature_ref: String,
    /// Ordered feature-local body identifiers.
    pub(crate) local_body_ids: Vec<u32>,
    /// Ordered body-state records stored before the selection vector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) body_state_ids: Vec<u32>,
    /// Retention mode carried by the delete-body data record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mode: Option<cadmpeg_ir::features::BodyRetentionMode>,
}

/// One compact feature-local edge-selection vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputEdgeSelection {
    /// Globally unique deterministic identifier for this vector.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Position among compact edge-selection vectors in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the vector marker.
    pub(crate) offset: u64,
    /// Feature-input name record owning this vector.
    pub(crate) object_name_ref: String,
    /// Native history feature owning this vector.
    pub(crate) feature_ref: String,
    /// Ordered feature-local edge identifiers.
    pub(crate) local_edge_ids: Vec<u32>,
    /// Complete typed path entries when this is an entry-form vector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) components: Vec<FeatureInputComponentPathEntry>,
    /// Ordered persistent references carried by a reference-list vector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) references: Vec<Vec<FeatureInputComponentPathEntry>>,
    /// Ordered history features traversed by the persistent edge path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) producer_feature_refs: Vec<String>,
    /// History feature owning the terminal edge component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) terminal_feature_ref: Option<String>,
}

/// One compact feature-local surface-component selection.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputSurfaceSelection {
    /// Globally unique deterministic identifier.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Position among surface selections in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the vector marker.
    pub(crate) offset: u64,
    /// Low selector subtype stored in the vector header.
    #[serde(default)]
    pub(crate) selector: u8,
    /// Component selection form; extrusion endpoints carry their opaque selector.
    #[serde(flatten, with = "surface_selection_kind_wire")]
    pub(crate) kind: FeatureInputSurfaceSelectionKind,
    /// Feature-input name record owning this selection.
    pub(crate) object_name_ref: String,
    /// Native history feature owning this selection.
    pub(crate) feature_ref: String,
    /// Ordered native history features traversed by the persistent surface path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) producer_feature_refs: Vec<String>,
    /// Native history feature owning the terminal face component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) terminal_feature_ref: Option<String>,
    /// Ordered typed entries in the persistent surface-component path.
    #[serde(default)]
    pub(crate) components: Vec<FeatureInputComponentPathEntry>,
}

/// Form of a retained surface-component selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureInputSurfaceSelectionKind {
    Component,
    ExtrusionEndpoint { endpoint_selector: u32 },
}

impl FeatureInputSurfaceSelection {
    pub(crate) fn endpoint_selector(&self) -> Option<u32> {
        match self.kind {
            FeatureInputSurfaceSelectionKind::Component => None,
            FeatureInputSurfaceSelectionKind::ExtrusionEndpoint { endpoint_selector } => {
                Some(endpoint_selector)
            }
        }
    }
}

mod surface_selection_kind_wire {
    use super::FeatureInputSurfaceSelectionKind;
    use serde::{ser::SerializeMap, Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    pub(super) struct Wire {
        #[serde(default)]
        endpoint_selector: Option<u32>,
    }

    // Serde field adapters borrow the field even when its type is Copy.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn serialize<S: Serializer>(
        kind: &FeatureInputSurfaceSelectionKind,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        if let FeatureInputSurfaceSelectionKind::ExtrusionEndpoint { endpoint_selector } = kind {
            map.serialize_entry("endpoint_selector", endpoint_selector)?;
        }
        map.end()
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<FeatureInputSurfaceSelectionKind, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        Ok(match wire.endpoint_selector {
            Some(endpoint_selector) => {
                FeatureInputSurfaceSelectionKind::ExtrusionEndpoint { endpoint_selector }
            }
            None => FeatureInputSurfaceSelectionKind::Component,
        })
    }
}

/// One persistent identity of a surface produced by a regenerated feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputGeneratedSurfaceIdentity {
    /// Globally unique deterministic identifier.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Position among generated surface identities in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the first component type signature.
    pub(crate) offset: u64,
    /// Four-byte serialized surface identity type family.
    pub(crate) type_prefix: [u8; 4],
    /// Source identifier of the feature that produced the terminal surface.
    pub(crate) feature_source_id: crate::brep::feature_source::FeatureSourceId,
    /// Opaque feature-local identity of the terminal surface.
    pub(crate) local_identity: u32,
    /// Ordered typed entries in the persistent generated-surface path.
    #[serde(default)]
    pub(crate) components: Vec<FeatureInputComponentPathEntry>,
}

/// One typed node in a persistent feature-input component path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputComponentPathEntry {
    /// Serialized component instance tag; absent on anonymous path nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) instance: Option<u16>,
    /// Twelve-byte serialized component type identity.
    pub(crate) type_signature: [u8; 12],
    /// Feature-local identifier carried by terminal selection nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) local_id: Option<u32>,
}

/// A declared sketch-relation family and its attached scalar record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputRelationBinding {
    /// Globally unique deterministic identifier for this binding.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Position among relation bindings in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the relation class declaration.
    pub(crate) offset: u64,
    /// Declared class record.
    pub(crate) class_ref: String,
    /// Native relation family.
    pub(crate) family: FeatureInputRelationFamily,
    /// Scalar record attached to the declaration.
    pub(crate) scalar_ref: String,
    /// Native history feature owning the relation, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) feature_ref: Option<String>,
}

/// One compact sketch-relation instance represented by related scalar records.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputRelationInstance {
    /// Globally unique deterministic identifier for this relation instance.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Position among relation instances in scalar stream order.
    pub(crate) ordinal: u32,
    /// First participating scalar's byte offset.
    pub(crate) offset: u64,
    /// Native relation family.
    pub(crate) family: FeatureInputRelationFamily,
    /// Class declaration defining the relation family.
    pub(crate) class_ref: String,
    /// Native sketch feature owning the relation.
    pub(crate) feature_ref: String,
    /// Scalar members and their selected parameter and display roles.
    #[serde(flatten)]
    pub(crate) scalars: relation_scalars::RelationScalars,
    /// Operand cells shared by the participating scalar records.
    pub(crate) operands: Vec<FeatureInputOperand>,
}

impl FeatureInputRelationInstance {
    pub(crate) fn scalar_refs(&self) -> &[String] {
        self.scalars.refs()
    }

    pub(crate) fn parameter_scalar_ref(&self) -> Option<&str> {
        self.scalars.parameter()
    }

    pub(crate) fn display_scalar_ref(&self) -> Option<&str> {
        self.scalars.display()
    }
}

/// Native sketch-relation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FeatureInputRelationFamily {
    /// Diameter of one circular sketch entity.
    CircleDiameter,
    /// Distance between two line loci.
    LineLineDistance,
    /// Distance between two point loci.
    PointPointDistance,
    /// Distance between a point locus and a line locus.
    PointLineDistance,
    /// Horizontal distance between two point loci.
    PointPointHorizontalDistance,
    /// Vertical distance between two point loci.
    PointPointVerticalDistance,
    /// Angle between two entity loci.
    Angle,
}

/// One native entity-reference cell in a feature-input stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputReference {
    /// Globally unique deterministic identifier for this cell.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Native history feature enclosing this cell, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) feature_ref: Option<String>,
    /// Position among reference cells in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the reference cell.
    pub(crate) offset: u64,
    /// Native reference-cell family.
    pub(crate) kind: FeatureInputOperandKind,
    /// Class declaration assigned to this lane-local token, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) class_ref: Option<String>,
    /// Local object index carried by the cell.
    pub(crate) object_index: u16,
}

/// One serialized UTF-16 object name in a feature-input stream.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FeatureInputName {
    /// Globally unique deterministic identifier for this name record.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Position among serialized names in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the name marker.
    pub(crate) offset: u64,
    /// Native object identifier stored after the UTF-16 name; `None` when the
    /// record has no identifier trailer at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) object_id: Option<ObjectId>,
    /// Decoded object name.
    pub(crate) value: String,
}

/// The native object identifier trailing a serialized object name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum ObjectId {
    /// The trailer holds the wire's absent-identifier marker.
    Absent,
    /// The trailer holds a native object identifier.
    Id(FeatureSourceId),
}

impl ObjectId {
    /// The identifier, when the trailer names a native object.
    pub(crate) fn id(self) -> Option<FeatureSourceId> {
        match self {
            Self::Absent => None,
            Self::Id(id) => Some(id),
        }
    }

    /// The identifier value, when the trailer names a native object.
    pub(crate) fn value(self) -> Option<u32> {
        self.id().map(FeatureSourceId::value)
    }

    /// The trailer for a raw identifier value.
    #[cfg(test)]
    pub(crate) fn from_value(value: u32) -> Option<Self> {
        FeatureSourceId::try_from(value).ok().map(Self::Id)
    }
}

impl TryFrom<u32> for ObjectId {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == u32::MAX {
            return Ok(Self::Absent);
        }
        FeatureSourceId::try_from(value)
            .map(Self::Id)
            .map_err(|_| "object_id is not a native object identifier")
    }
}

impl From<ObjectId> for u32 {
    fn from(value: ObjectId) -> Self {
        match value {
            ObjectId::Absent => u32::MAX,
            ObjectId::Id(id) => id.value(),
        }
    }
}

/// One named scalar serialized in native SI units.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FeatureInputScalar {
    /// Globally unique deterministic identifier for this scalar record.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Native history feature enclosing this scalar, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) feature_ref: Option<String>,
    /// Position among named scalars in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the little-endian f64 value.
    pub(crate) offset: u64,
    /// Native object identifier carried by the scalar record.
    pub(crate) object_id: u32,
    /// Name record attached to this scalar.
    pub(crate) name: String,
    /// Scalar value in native SI units.
    pub(crate) value: f64,
    /// Function of this scalar in the dimension record.
    pub(crate) role: FeatureInputScalarRole,
    /// Typed native operand cells attached to this scalar.
    #[serde(flatten, with = "scalar_operands_wire")]
    pub(crate) operands: Vec<FeatureInputOperand>,
}

impl FeatureInputScalar {
    /// Local sketch-entity indices carried by D6 dimension operands.
    pub(crate) fn entity_indices(&self) -> Vec<u16> {
        scalar_operands_wire::entity_indices(&self.operands)
    }
}

mod scalar_operands_wire {
    use super::{FeatureInputOperand, FeatureInputOperandKind};
    use serde::{ser::SerializeMap, Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    pub(super) struct Wire {
        #[serde(default)]
        entity_indices: Option<Vec<u16>>,
        #[serde(default)]
        operands: Vec<FeatureInputOperand>,
    }

    pub(super) fn entity_indices(operands: &[FeatureInputOperand]) -> Vec<u16> {
        operands
            .iter()
            .filter(|operand| operand.kind == FeatureInputOperandKind::D6)
            .map(|operand| operand.entity_index)
            .collect()
    }

    pub(super) fn serialize<S: Serializer>(
        operands: &[FeatureInputOperand],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        let indices = entity_indices(operands);
        if !indices.is_empty() {
            map.serialize_entry("entity_indices", &indices)?;
        }
        if !operands.is_empty() {
            map.serialize_entry("operands", operands)?;
        }
        map.end()
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<FeatureInputOperand>, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        if wire
            .entity_indices
            .is_some_and(|indices| indices != entity_indices(&wire.operands))
        {
            return Err(serde::de::Error::custom(
                "entity_indices must match the D6 operands",
            ));
        }
        Ok(wire.operands)
    }
}

/// One native entity-reference cell attached to a feature-input scalar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeatureInputOperand {
    /// Byte offset of the reference cell within the feature-input stream.
    pub(crate) offset: u64,
    /// Reference-cell record at this byte offset.
    pub(crate) reference_ref: String,
    /// Native reference-cell family.
    pub(crate) kind: FeatureInputOperandKind,
    /// Local entity index carried by the cell.
    pub(crate) entity_index: u16,
    /// Resolved sketch-input entity in the same feature object, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) entity_ref: Option<String>,
}

/// Native feature-input entity-reference cell family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FeatureInputOperandKind {
    /// `d6 80` reference cell.
    D6,
    /// `e1 80` reference cell.
    E1,
    /// Other two-byte reference-cell tag, stored as a little-endian u16.
    Native(operand_tag::NativeOperandTag),
}

/// Function of a named scalar in its dimension record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FeatureInputScalarRole {
    /// Value consumed during model regeneration.
    Driving,
    /// Dimension-label placement or display value.
    Display,
    /// Scalar from a different native record layout.
    Native,
}

/// One class declaration in a native feature-input stream.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FeatureInputClass {
    /// Globally unique deterministic identifier for this declaration.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Position among class declarations in stream order.
    pub(crate) ordinal: u32,
    /// Byte offset of the `ff ff 01 00` declaration marker.
    pub(crate) offset: u64,
    /// Declared native class name.
    #[serde(flatten, with = "feature_class_wire")]
    pub(crate) name: String,
}

impl FeatureInputClass {
    pub(crate) fn role(&self) -> FeatureInputClassRole {
        crate::classification::native_object_class(&self.name).role()
    }
}

mod feature_class_wire {
    use super::FeatureInputClassRole;
    use crate::classification::native_object_class;
    use serde::{ser::SerializeMap, Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    pub(super) struct Wire {
        name: String,
        #[serde(default)]
        role: Option<FeatureInputClassRole>,
    }

    pub(super) fn serialize<S: Serializer>(name: &str, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("name", name)?;
        map.serialize_entry("role", &native_object_class(name).role())?;
        map.end()
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<String, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        if wire
            .role
            .is_some_and(|role| role != native_object_class(&wire.name).role())
        {
            return Err(serde::de::Error::custom(
                "role must match the native class name",
            ));
        }
        Ok(wire.name)
    }
}

/// Design-intent role declared by a feature-input class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FeatureInputClassRole {
    /// Modeling operation or construction feature.
    Feature,
    /// Sketch container.
    Sketch,
    /// Sketch geometry handle.
    SketchEntity,
    /// Geometric sketch relation.
    SketchConstraint,
    /// Driving or driven dimension.
    Dimension,
    /// Scalar feature parameter.
    Parameter,
    /// Reference to another model object.
    Reference,
    /// Supporting serialization object.
    Auxiliary,
    /// Class with no typed role.
    #[default]
    Native,
}

/// One typed sketch-entity marker inside a native feature-input stream.
#[derive(Clone, PartialEq, Serialize)]
pub(crate) struct SketchInputEntity {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Native history feature whose serialized object interval contains this marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) feature_ref: Option<String>,
    /// Position of this marker within the owning `FeatureInputLane`, in stream order.
    ordinal: u32,
    /// Byte offset of this marker within `FeatureInputLane::native_payload`.
    offset: u64,
    /// Feature-local object index stored immediately before the marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_index: Option<u32>,
    /// Feature-local object identifier stored in the marker trailer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_id: Option<u32>,
    /// Sketch-entity kind this marker identifies.
    pub(crate) kind: SketchInputKind,
    /// Finite little-endian state scalar at the marker layout's state slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) state_value: Option<f64>,
    /// Two little-endian coordinate fields stored by geometry-handle marker families, in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) coordinates_m: Option<[f64; 2]>,
    /// Resolved links and their selector from the reference-bearing layout.
    #[serde(flatten, with = "sketch_input_links_wire")]
    pub(crate) links: Option<SketchInputLinks>,
}

/// Deserialization mirror of a sketch-entity marker, re-admitted against its lane payload.
#[derive(Deserialize)]
pub(crate) struct SketchInputEntityWire {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Owning feature-input lane record id.
    pub(crate) parent: String,
    /// Native history feature whose serialized object interval contains this marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    feature_ref: Option<String>,
    /// Position of this marker within the owning `FeatureInputLane`, in stream order.
    ordinal: u32,
    /// Byte offset of this marker within `FeatureInputLane::native_payload`.
    offset: u64,
    /// Feature-local object index stored immediately before the marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_index: Option<u32>,
    /// Feature-local object identifier stored in the marker trailer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_id: Option<u32>,
    /// Sketch-entity kind this marker identifies.
    kind: SketchInputKind,
    /// Finite little-endian state scalar at the marker layout's state slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state_value: Option<f64>,
    /// Two little-endian coordinate fields stored by geometry-handle marker families, in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coordinates_m: Option<[f64; 2]>,
    /// Resolved links and their selector from the reference-bearing layout.
    #[serde(flatten, with = "sketch_input_links_wire")]
    links: Option<SketchInputLinks>,
}

/// A selector paired with a nonempty collection of resolved marker links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SketchInputLinks {
    selector: u16,
    entries: Vec<SketchInputLink>,
}

impl SketchInputLinks {
    pub(crate) fn new(selector: u16, entries: Vec<SketchInputLink>) -> Option<Self> {
        (!entries.is_empty()).then_some(Self { selector, entries })
    }

    /// The layout selector this marker's links were read under.
    pub(crate) fn selector(&self) -> u16 {
        self.selector
    }

    pub(crate) fn entries(&self) -> &[SketchInputLink] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn entries_mut(&mut self) -> &mut [SketchInputLink] {
        &mut self.entries
    }
}

mod sketch_input_links_wire {
    use super::{SketchInputLink, SketchInputLinks};
    use serde::{ser::SerializeMap, Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    pub(super) struct Wire {
        #[serde(default)]
        links: Vec<SketchInputLink>,
        #[serde(default)]
        link_selector: Option<u16>,
    }

    // Serde field adapters borrow the complete optional links field.
    #[allow(clippy::ref_option)]
    pub(super) fn serialize<S: Serializer>(
        links: &Option<SketchInputLinks>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(if links.is_some() { 2 } else { 0 }))?;
        if let Some(links) = links {
            map.serialize_entry("links", links.entries())?;
            map.serialize_entry("link_selector", &links.selector())?;
        }
        map.end()
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<SketchInputLinks>, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        match (wire.links.is_empty(), wire.link_selector) {
            (true, None) => Ok(None),
            (false, Some(selector)) => Ok(SketchInputLinks::new(selector, wire.links)),
            _ => Err(serde::de::Error::custom(
                "links and link_selector must be present together, with nonempty links",
            )),
        }
    }
}

impl SketchInputEntity {
    pub(crate) fn links(&self) -> &[SketchInputLink] {
        self.links.as_ref().map_or(&[], SketchInputLinks::entries)
    }

    pub(crate) fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    /// Returns the feature-local object index.
    pub(crate) fn object_index(&self) -> Option<u32> {
        self.object_index
    }

    /// Returns the feature-local object identifier.
    pub(crate) fn local_id(&self) -> Option<u32> {
        self.local_id
    }

    #[cfg(test)]
    /// Sets the identity fields on a cloned fixture.
    pub(crate) fn with_test_identity(
        &self,
        object_index: Option<u32>,
        local_id: Option<u32>,
    ) -> Self {
        let mut updated = self.clone();
        updated.object_index = object_index;
        updated.local_id = local_id;
        updated
    }

    pub(crate) fn try_from_wire(
        wire: SketchInputEntityWire,
        payload: &[u8],
    ) -> Result<Self, String> {
        let mut entity = Self::try_new(
            wire.id,
            wire.parent,
            wire.ordinal,
            wire.offset,
            wire.kind,
            payload,
        )
        .map_err(str::to_string)?;
        if wire.object_index != entity.object_index {
            return Err(
                "SolidWorks feature-input object index does not match its native payload".into(),
            );
        }
        if wire.local_id != entity.local_id {
            return Err(
                "SolidWorks feature-input local object id does not match its native payload".into(),
            );
        }
        entity.feature_ref = wire.feature_ref;
        entity.state_value = wire.state_value;
        entity.coordinates_m = wire.coordinates_m;
        entity.links = wire.links;
        Ok(entity)
    }

    pub(crate) fn try_new(
        id: String,
        parent: String,
        ordinal: u32,
        offset: u64,
        kind: SketchInputKind,
        payload: &[u8],
    ) -> Result<Self, &'static str> {
        let position = usize::try_from(offset).map_err(|_| "sketch entity offset exceeds usize")?;
        if position >= payload.len()
            || !crate::resolved_features::markers::sketch_marker_at(payload, position)
        {
            return Err("sketch entity offset is not a marker in native_payload");
        }
        Ok(Self {
            id,
            parent,
            feature_ref: None,
            ordinal,
            offset,
            object_index: crate::resolved_features::markers::marker_object_index(payload, position),
            local_id: crate::resolved_features::markers::marker_local_id(payload, position),
            kind,
            state_value: None,
            coordinates_m: None,
            links: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(
        id: impl Into<String>,
        parent: impl Into<String>,
        ordinal: u32,
        offset: u64,
        kind: SketchInputKind,
    ) -> Self {
        let position = usize::try_from(offset).unwrap();
        let mut payload = cadmpeg_core::decode::alloc_filled(
            position.checked_add(39).unwrap(),
            0,
            "SLDPRT sketch marker fixture",
        )
        .unwrap();
        if position >= 4 {
            payload[position - 4..position].fill(0xff);
        }
        payload[position..position + 5].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
        payload[position + 5..position + 13].fill(0xff);
        payload[position + 13..position + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        Self::try_new(id.into(), parent.into(), ordinal, offset, kind, &payload).unwrap()
    }

    #[cfg(test)]
    pub(crate) fn with_test_position(&self, ordinal: u32, offset: u64) -> Self {
        let mut updated = self.clone();
        updated.ordinal = ordinal;
        updated.offset = offset;
        updated
    }
}

/// One marker-local reference resolved within its owning feature object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SketchInputLink {
    /// Feature-local object identifier stored in the marker payload.
    pub(crate) local_id: u16,
    /// Typed sketch-input marker with this local identifier.
    pub(crate) entity_ref: String,
}

/// Kind of sketch entity referenced by a native feature-input marker.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SketchInputKindWire", into = "SketchInputKindWire")]
pub(crate) enum SketchInputKind {
    /// A sketch point.
    Point,
    /// A sketch line or circle from the shared native family.
    LineOrCircle,
    /// A sketch arc.
    Arc,
    /// A sketch point bound by a geometric constraint.
    ConstrainedPoint,
    /// A sketch relation handle.
    Relation(SketchRelationKind),
    /// A native extension code in the unclassified marker namespace.
    Native(sketch_code::NativeSketchCode),
    /// A low code retained under a native handle layout, such as a slot handle.
    NativeHandle(sketch_code::LowMarkerCode),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SketchInputKindWire {
    Point,
    #[serde(alias = "curve")]
    LineOrCircle,
    Arc,
    ConstrainedPoint,
    Relation(SketchRelationKind),
    Native(u32),
}

impl From<SketchInputKindWire> for SketchInputKind {
    fn from(wire: SketchInputKindWire) -> Self {
        match wire {
            SketchInputKindWire::Point => Self::Point,
            SketchInputKindWire::LineOrCircle => Self::LineOrCircle,
            SketchInputKindWire::Arc => Self::Arc,
            SketchInputKindWire::ConstrainedPoint => Self::ConstrainedPoint,
            SketchInputKindWire::Relation(kind) => Self::Relation(kind),
            SketchInputKindWire::Native(code) => Self::from_handle_code(code),
        }
    }
}

impl From<SketchInputKind> for SketchInputKindWire {
    fn from(kind: SketchInputKind) -> Self {
        match kind {
            SketchInputKind::Point => Self::Point,
            SketchInputKind::LineOrCircle => Self::LineOrCircle,
            SketchInputKind::Arc => Self::Arc,
            SketchInputKind::ConstrainedPoint => Self::ConstrainedPoint,
            SketchInputKind::Relation(kind) => Self::Relation(kind),
            SketchInputKind::Native(code) => Self::Native(code.value()),
            SketchInputKind::NativeHandle(code) => Self::Native(code.value()),
        }
    }
}

impl SketchInputKind {
    /// Maps a code in the geometry-marker namespace.
    pub(crate) fn from_native_code(code: u32) -> Self {
        use sketch_code::{LowMarkerCode, NativeSketchCode};
        match NativeSketchCode::try_from(code) {
            Ok(code) => Self::Native(code),
            Err(LowMarkerCode::Zero) => Self::Point,
            Err(LowMarkerCode::One) => Self::LineOrCircle,
            Err(LowMarkerCode::Two) => Self::Arc,
            Err(LowMarkerCode::Three) => Self::ConstrainedPoint,
        }
    }

    /// Retains a code whose handle layout does not assign geometry semantics.
    pub(crate) fn from_handle_code(code: u32) -> Self {
        match sketch_code::NativeSketchCode::try_from(code) {
            Ok(code) => Self::Native(code),
            Err(code) => Self::NativeHandle(code),
        }
    }

    /// Maps a marker code using its layout to separate geometry and relation handles.
    pub(crate) fn from_native_code_and_layout(code: u32, coordinate_bearing: bool) -> Self {
        if code == 0 || (coordinate_bearing && code <= 3) {
            return Self::from_native_code(code);
        }
        SketchRelationKind::from_native_code(code)
            .map_or_else(|| Self::from_native_code(code), Self::Relation)
    }

    /// Returns the stored code; the marker layout selects its namespace.
    pub(crate) fn native_code(self) -> u32 {
        match self {
            Self::Point => 0,
            Self::LineOrCircle => 1,
            Self::Arc => 2,
            Self::ConstrainedPoint => 3,
            Self::Relation(relation) => relation.native_code(),
            Self::Native(value) => value.value(),
            Self::NativeHandle(value) => value.value(),
        }
    }

    /// Whether this marker owns constraint semantics that require a neutral
    /// projection. Dimensional marker handles are operands of scalar-bearing
    /// relation instances and do not independently encode a constraint.
    pub(crate) fn owns_constraint(self) -> bool {
        match self {
            Self::Relation(
                SketchRelationKind::Distance
                | SketchRelationKind::Angle
                | SketchRelationKind::Radius
                | SketchRelationKind::Diameter,
            ) => false,
            Self::Relation(_) | Self::Native(_) | Self::NativeHandle(_) => true,
            Self::Point | Self::LineOrCircle | Self::Arc | Self::ConstrainedPoint => false,
        }
    }
}

/// Relation kind carried by a non-coordinate sketch marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SketchRelationKind {
    /// Linear distance.
    Distance,
    /// Angular distance.
    Angle,
    /// Radius dimension.
    Radius,
    /// Horizontal entity or point alignment.
    Horizontal,
    /// Vertical entity or point alignment.
    Vertical,
    /// Tangency.
    Tangent,
    /// Parallelism.
    Parallel,
    /// Perpendicularity.
    Perpendicular,
    /// Point-on-entity coincidence.
    Coincident,
    /// Shared center.
    Concentric,
    /// Symmetry about a centerline.
    Symmetric,
    /// Midpoint incidence.
    Midpoint,
    /// Intersection incidence.
    AtIntersection,
    /// Equal length or radius.
    Equal,
    /// Diameter dimension.
    Diameter,
    /// Offset-edge relation.
    OffsetEdge,
    /// Fixed geometry.
    Fixed,
    /// Arc angle fixed at 90 degrees.
    ArcAngle90,
    /// Arc angle fixed at 180 degrees.
    ArcAngle180,
    /// Arc angle fixed at 270 degrees.
    ArcAngle270,
    /// Arc constrained to the top cardinal position.
    ArcAngleTop,
    /// Arc constrained to the bottom cardinal position.
    ArcAngleBottom,
    /// Arc constrained to the left cardinal position.
    ArcAngleLeft,
    /// Arc constrained to the right cardinal position.
    ArcAngleRight,
    /// Horizontal point alignment.
    HorizontalPoints,
    /// Vertical point alignment.
    VerticalPoints,
    /// Collinearity.
    Collinear,
    /// Circular arcs share a center and radius.
    Coradial,
    /// Point snapped to the sketch grid.
    SnapGrid,
    /// Length snapped to an increment.
    SnapLength,
    /// Angle snapped to an increment.
    SnapAngle,
    /// Geometry converted from an external edge.
    UseEdge,
    /// Ellipse angle fixed at 90 degrees.
    EllipseAngle90,
    /// Ellipse angle fixed at 180 degrees.
    EllipseAngle180,
    /// Ellipse angle fixed at 270 degrees.
    EllipseAngle270,
    /// Ellipse constrained to the top cardinal position.
    EllipseAngleTop,
    /// Ellipse constrained to the bottom cardinal position.
    EllipseAngleBottom,
    /// Ellipse constrained to the left cardinal position.
    EllipseAngleLeft,
    /// Ellipse constrained to the right cardinal position.
    EllipseAngleRight,
    /// Point pierces a referenced entity.
    AtPierce,
    /// Doubled distance display relation.
    DoubleDistance,
    /// Point merge relation.
    MergePoints,
    /// Three-point angular dimension.
    AngleThreePoint,
    /// Arc-length dimension.
    ArcLength,
    /// Entity normal relation.
    Normal,
    /// Point normal relation.
    NormalPoints,
    /// Offset relation between entities in one sketch.
    SketchOffset,
    /// Entity aligned with the X axis.
    AlongX,
    /// Entity aligned with the Y axis.
    AlongY,
    /// Entity aligned with the Z axis.
    AlongZ,
    /// Points aligned with the X axis.
    AlongXPoints,
    /// Points aligned with the Y axis.
    AlongYPoints,
    /// Points aligned with the Z axis.
    AlongZPoints,
    /// Entity parallel to the YZ plane.
    ParallelYz,
    /// Entity parallel to the ZX plane.
    ParallelZx,
    /// Intersection relation.
    Intersection,
    /// Pattern membership relation.
    Patterned,
    /// Isoparametric curve controlled by an external point.
    IsoByPoint,
    /// Common isoparametric relation.
    SameIsoparametric,
    /// Fit-spline relation.
    FitSpline,
    /// Equal-curvature relation.
    EqualCurvature,
    /// Equal-tangent relation.
    EqualTangent,
    /// Tangency to a face.
    TangentFace,
    /// 3D entity aligned with the X axis.
    AlongX3d,
    /// 3D entity aligned with the Y axis.
    AlongY3d,
    /// 3D points aligned with the X axis.
    AlongXPoints3d,
    /// 3D points aligned with the Y axis.
    AlongYPoints3d,
    /// Traction relation.
    Traction,
    /// Belt-traction relation.
    BeltTraction,
    /// Two blocks locked together.
    BlockFixedLock,
    /// Blocks locked normal to one another.
    BlockNormalLock,
    /// Blocks locked for relative rotation.
    BlockRotateLock,
    /// Display-only slot relation.
    FakeSlotConstraint,
    /// Fixed-slot relation.
    FixedSlot,
    /// Slots have equal dimensions.
    SameSlots,
    /// Linear-pattern count relation.
    LinearPatternCount,
    /// Circular-pattern count relation.
    CircularPatternCount,
    /// Radial routing offset.
    RadialOffset,
    /// Planar routing offset.
    PlanarOffset,
    /// Aligned equal curvature between 3D splines.
    EqualCurvature3dAligned,
    /// Virtual-point distance to a flange face.
    FlangeFaceDistance,
    /// Conic rho relation.
    ConicRho,
    /// Third-order continuity relation.
    C3Touch,
    /// Doubled angle display relation.
    DoubleAngle,
    /// Equal arc or spline length.
    SameCurveLength,
}

impl SketchRelationKind {
    /// Decodes relation codes `1..85`.
    pub(crate) fn from_native_code(code: u32) -> Option<Self> {
        Some(match code {
            1 => Self::Distance,
            2 => Self::Angle,
            3 => Self::Radius,
            4 => Self::Horizontal,
            5 => Self::Vertical,
            6 => Self::Tangent,
            7 => Self::Parallel,
            8 => Self::Perpendicular,
            9 => Self::Coincident,
            10 => Self::Concentric,
            11 => Self::Symmetric,
            12 => Self::Midpoint,
            13 => Self::AtIntersection,
            14 => Self::Equal,
            15 => Self::Diameter,
            16 => Self::OffsetEdge,
            17 => Self::Fixed,
            18 => Self::ArcAngle90,
            19 => Self::ArcAngle180,
            20 => Self::ArcAngle270,
            21 => Self::ArcAngleTop,
            22 => Self::ArcAngleBottom,
            23 => Self::ArcAngleLeft,
            24 => Self::ArcAngleRight,
            25 => Self::HorizontalPoints,
            26 => Self::VerticalPoints,
            27 => Self::Collinear,
            28 => Self::Coradial,
            29 => Self::SnapGrid,
            30 => Self::SnapLength,
            31 => Self::SnapAngle,
            32 => Self::UseEdge,
            33 => Self::EllipseAngle90,
            34 => Self::EllipseAngle180,
            35 => Self::EllipseAngle270,
            36 => Self::EllipseAngleTop,
            37 => Self::EllipseAngleBottom,
            38 => Self::EllipseAngleLeft,
            39 => Self::EllipseAngleRight,
            40 => Self::AtPierce,
            41 => Self::DoubleDistance,
            42 => Self::MergePoints,
            43 => Self::AngleThreePoint,
            44 => Self::ArcLength,
            45 => Self::Normal,
            46 => Self::NormalPoints,
            47 => Self::SketchOffset,
            48 => Self::AlongX,
            49 => Self::AlongY,
            50 => Self::AlongZ,
            51 => Self::AlongXPoints,
            52 => Self::AlongYPoints,
            53 => Self::AlongZPoints,
            54 => Self::ParallelYz,
            55 => Self::ParallelZx,
            56 => Self::Intersection,
            57 => Self::Patterned,
            58 => Self::IsoByPoint,
            59 => Self::SameIsoparametric,
            60 => Self::FitSpline,
            61 => Self::EqualCurvature,
            62 => Self::EqualTangent,
            63 => Self::TangentFace,
            64 => Self::AlongX3d,
            65 => Self::AlongY3d,
            66 => Self::AlongXPoints3d,
            67 => Self::AlongYPoints3d,
            68 => Self::Traction,
            69 => Self::BeltTraction,
            70 => Self::BlockFixedLock,
            71 => Self::BlockNormalLock,
            72 => Self::BlockRotateLock,
            73 => Self::FakeSlotConstraint,
            74 => Self::FixedSlot,
            75 => Self::SameSlots,
            76 => Self::LinearPatternCount,
            77 => Self::CircularPatternCount,
            78 => Self::RadialOffset,
            79 => Self::PlanarOffset,
            80 => Self::EqualCurvature3dAligned,
            81 => Self::FlangeFaceDistance,
            82 => Self::ConicRho,
            83 => Self::C3Touch,
            84 => Self::DoubleAngle,
            85 => Self::SameCurveLength,
            _ => return None,
        })
    }

    /// Returns the serialized relation code.
    pub(crate) fn native_code(self) -> u32 {
        match self {
            Self::Distance => 1,
            Self::Angle => 2,
            Self::Radius => 3,
            Self::Horizontal => 4,
            Self::Vertical => 5,
            Self::Tangent => 6,
            Self::Parallel => 7,
            Self::Perpendicular => 8,
            Self::Coincident => 9,
            Self::Concentric => 10,
            Self::Symmetric => 11,
            Self::Midpoint => 12,
            Self::AtIntersection => 13,
            Self::Equal => 14,
            Self::Diameter => 15,
            Self::OffsetEdge => 16,
            Self::Fixed => 17,
            Self::ArcAngle90 => 18,
            Self::ArcAngle180 => 19,
            Self::ArcAngle270 => 20,
            Self::ArcAngleTop => 21,
            Self::ArcAngleBottom => 22,
            Self::ArcAngleLeft => 23,
            Self::ArcAngleRight => 24,
            Self::HorizontalPoints => 25,
            Self::VerticalPoints => 26,
            Self::Collinear => 27,
            Self::Coradial => 28,
            Self::SnapGrid => 29,
            Self::SnapLength => 30,
            Self::SnapAngle => 31,
            Self::UseEdge => 32,
            Self::EllipseAngle90 => 33,
            Self::EllipseAngle180 => 34,
            Self::EllipseAngle270 => 35,
            Self::EllipseAngleTop => 36,
            Self::EllipseAngleBottom => 37,
            Self::EllipseAngleLeft => 38,
            Self::EllipseAngleRight => 39,
            Self::AtPierce => 40,
            Self::DoubleDistance => 41,
            Self::MergePoints => 42,
            Self::AngleThreePoint => 43,
            Self::ArcLength => 44,
            Self::Normal => 45,
            Self::NormalPoints => 46,
            Self::SketchOffset => 47,
            Self::AlongX => 48,
            Self::AlongY => 49,
            Self::AlongZ => 50,
            Self::AlongXPoints => 51,
            Self::AlongYPoints => 52,
            Self::AlongZPoints => 53,
            Self::ParallelYz => 54,
            Self::ParallelZx => 55,
            Self::Intersection => 56,
            Self::Patterned => 57,
            Self::IsoByPoint => 58,
            Self::SameIsoparametric => 59,
            Self::FitSpline => 60,
            Self::EqualCurvature => 61,
            Self::EqualTangent => 62,
            Self::TangentFace => 63,
            Self::AlongX3d => 64,
            Self::AlongY3d => 65,
            Self::AlongXPoints3d => 66,
            Self::AlongYPoints3d => 67,
            Self::Traction => 68,
            Self::BeltTraction => 69,
            Self::BlockFixedLock => 70,
            Self::BlockNormalLock => 71,
            Self::BlockRotateLock => 72,
            Self::FakeSlotConstraint => 73,
            Self::FixedSlot => 74,
            Self::SameSlots => 75,
            Self::LinearPatternCount => 76,
            Self::CircularPatternCount => 77,
            Self::RadialOffset => 78,
            Self::PlanarOffset => 79,
            Self::EqualCurvature3dAligned => 80,
            Self::FlangeFaceDistance => 81,
            Self::ConicRho => 82,
            Self::C3Touch => 83,
            Self::DoubleAngle => 84,
            Self::SameCurveLength => 85,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_operand_wire_rejects_reserved_tags() {
        for tag in [0x0000, 0xffff, 0x80d6, 0x80e1] {
            assert!(serde_json::from_value::<super::FeatureInputOperandKind>(
                serde_json::json!({"native": tag})
            )
            .is_err());
        }
        let wire = serde_json::json!({"native": 0x812a});
        let kind: super::FeatureInputOperandKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(kind).unwrap(), wire);
    }

    #[test]
    fn surface_selection_kind_preserves_the_endpoint_wire() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Selection {
            #[serde(flatten, with = "super::surface_selection_kind_wire")]
            kind: super::FeatureInputSurfaceSelectionKind,
        }
        for wire in [
            serde_json::json!({}),
            serde_json::json!({"endpoint_selector": 0}),
        ] {
            let selection: Selection = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(
                matches!(
                    selection.kind,
                    super::FeatureInputSurfaceSelectionKind::ExtrusionEndpoint { .. }
                ),
                wire.get("endpoint_selector").is_some(),
            );
            assert_eq!(serde_json::to_value(selection).unwrap(), wire);
        }
    }

    #[test]
    fn pmi_display_text_wire_keeps_text_and_offset_together() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Display {
            #[serde(flatten, with = "super::pmi_display_text_wire")]
            value: Option<(String, u64)>,
        }
        let wire = serde_json::json!({"display_text": "25 mm", "display_text_offset": 17});
        let display: Display = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(display).unwrap(), wire);
        for field in ["display_text", "display_text_offset"] {
            let mut split = wire.clone();
            split.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<Display>(split).is_err());
        }
        let absent: Display = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(serde_json::to_value(absent).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn class_role_wire_is_derived_from_name() {
        let wire = serde_json::json!({
            "id": "class", "parent": "lane", "ordinal": 0, "offset": 0,
            "name": "sgEntHandle", "role": "sketch_entity"
        });
        let class: super::FeatureInputClass = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(class.role(), super::FeatureInputClassRole::SketchEntity);
        assert_eq!(serde_json::to_value(class).unwrap(), wire);
        let mut inconsistent = wire;
        inconsistent["role"] = serde_json::json!("feature");
        assert!(serde_json::from_value::<super::FeatureInputClass>(inconsistent).is_err());
    }

    #[test]
    fn scalar_wire_derives_indices_from_d6_operands() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Operands {
            #[serde(flatten, with = "super::scalar_operands_wire")]
            operands: Vec<super::FeatureInputOperand>,
        }
        let wire = serde_json::json!({
            "entity_indices": [7],
            "operands": [
                {"offset": 0, "reference_ref": "a", "kind": "d6", "entity_index": 7},
                {"offset": 12, "reference_ref": "b", "kind": "e1", "entity_index": 9}
            ]
        });
        let operands: Operands = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(operands).unwrap(), wire);
        let mut inconsistent = wire;
        inconsistent["entity_indices"] = serde_json::json!([7, 9]);
        assert!(serde_json::from_value::<Operands>(inconsistent).is_err());
        let empty: Operands = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(serde_json::to_value(empty).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn tree_parent_preserves_record_and_source_wire_forms() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Parent {
            #[serde(flatten, with = "super::tree_parent_wire")]
            parent: Option<super::TreeParent>,
        }
        for wire in [
            serde_json::json!({}),
            serde_json::json!({"tree_parent": "record"}),
            serde_json::json!({"parent_source_id": "7"}),
            serde_json::json!({"tree_parent": "record", "parent_source_id": "7"}),
        ] {
            let parent: Parent = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(
                parent.parent.is_none(),
                wire.as_object().unwrap().is_empty()
            );
            assert_eq!(serde_json::to_value(parent).unwrap(), wire);
        }
    }

    #[test]
    fn sketch_links_preserve_flat_wire_and_reject_split_pairs() {
        use super::{
            FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks,
        };
        let mut payload = vec![0u8; 39];
        payload[..5].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        let mut entity = SketchInputEntity::try_new(
            "marker".into(),
            "lane".into(),
            0,
            0,
            SketchInputKind::Point,
            &payload,
        )
        .expect("marker fixture");
        entity.links = SketchInputLinks::new(
            3,
            vec![SketchInputLink {
                local_id: 7,
                entity_ref: "target".into(),
            }],
        );
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![entity],
        };
        let wire = serde_json::to_value(&lane).expect("lane JSON");
        assert_eq!(
            serde_json::from_value::<FeatureInputLane>(wire.clone()).expect("lane round trip"),
            lane
        );
        assert_eq!(wire["sketch_entities"][0]["link_selector"], 3);
        for missing in ["links", "link_selector"] {
            let mut split = wire.clone();
            split["sketch_entities"][0]
                .as_object_mut()
                .expect("entity object")
                .remove(missing);
            let error = serde_json::from_value::<FeatureInputLane>(split).unwrap_err();
            assert!(error.to_string().contains("links and link_selector"));
        }
        let mut empty = wire.clone();
        empty["sketch_entities"][0]["links"] = serde_json::json!([]);
        assert!(serde_json::from_value::<FeatureInputLane>(empty).is_err());
        let mut moved = wire.clone();
        moved["sketch_entities"][0]["offset"] = serde_json::json!(3);
        let error = serde_json::from_value::<FeatureInputLane>(moved).unwrap_err();
        assert!(error
            .to_string()
            .contains("is not a marker in native_payload"));
        let mut renamed = wire;
        renamed["sketch_entities"][0]["local_id"] = serde_json::json!(4_242);
        let error = serde_json::from_value::<FeatureInputLane>(renamed).unwrap_err();
        assert!(error.to_string().contains("local object id does not match"));
        assert!(SketchInputLinks::new(3, Vec::new()).is_none());
    }

    use super::{SketchInputKind, SketchRelationKind};

    #[test]
    fn low_native_handles_keep_their_layout_namespace_and_wire() {
        for code in 0..=3 {
            let geometry = SketchInputKind::from_native_code(code);
            let handle = SketchInputKind::from_handle_code(code);
            assert_ne!(geometry, handle);
            assert!(matches!(handle, SketchInputKind::NativeHandle(_)));
            assert_eq!(handle.native_code(), code);
            let wire = serde_json::json!({"native": code});
            assert_eq!(serde_json::to_value(handle).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<SketchInputKind>(wire).unwrap(),
                handle
            );
        }
    }

    #[test]
    fn marker_layout_disambiguates_geometry_and_relation_codes() {
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(1, true),
            SketchInputKind::LineOrCircle
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(1, false),
            SketchInputKind::Relation(SketchRelationKind::Distance)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(9, false),
            SketchInputKind::Relation(SketchRelationKind::Coincident)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(4, true),
            SketchInputKind::Relation(SketchRelationKind::Horizontal)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(10, true),
            SketchInputKind::Relation(SketchRelationKind::Concentric)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(27, false),
            SketchInputKind::Relation(SketchRelationKind::Collinear)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(28, false),
            SketchInputKind::Relation(SketchRelationKind::Coradial)
        );
        assert_eq!(
            SketchInputKind::from_native_code_and_layout(86, false),
            SketchInputKind::from_native_code(86)
        );
        for code in 1..=85 {
            let relation = SketchRelationKind::from_native_code(code).expect("required invariant");
            assert_eq!(relation.native_code(), code);
        }
    }

    #[test]
    fn scalar_bearing_instances_own_dimensional_constraints() {
        for relation in [
            SketchRelationKind::Distance,
            SketchRelationKind::Angle,
            SketchRelationKind::Radius,
            SketchRelationKind::Diameter,
        ] {
            assert!(!SketchInputKind::Relation(relation).owns_constraint());
        }
        assert!(SketchInputKind::Relation(SketchRelationKind::Horizontal).owns_constraint());
        assert!(SketchInputKind::from_native_code(86).owns_constraint());
        assert!(!SketchInputKind::Point.owns_constraint());
    }
}
