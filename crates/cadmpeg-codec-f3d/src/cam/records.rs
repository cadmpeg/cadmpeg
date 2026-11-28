// SPDX-License-Identifier: Apache-2.0
//! Source manufacturing definitions with ordered native parameters.

use cadmpeg_ir::design::ids::{OperationId, SetupId, ToolId};

use serde::{Deserialize, Serialize};

/// Manufacturing definitions retained before geometry selection resolution.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manufacturing {
    /// Setups in source order.
    pub setups: Vec<ManufacturingSetup>,
    /// Operations in source order.
    pub operations: Vec<ManufacturingOperation>,
    /// Cutting tools in source order.
    pub tools: Vec<ManufacturingTool>,
}

impl Manufacturing {
    /// True when the section holds no records.
    pub(crate) fn is_empty(&self) -> bool {
        self.setups.is_empty() && self.operations.is_empty() && self.tools.is_empty()
    }
}

/// A named parameter with its exact stored representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManufacturingParameter {
    /// Source parameter name.
    pub name: String,
    /// Source value text, including expressions.
    pub value: String,
    /// Source unit symbol; absent units remain absent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unit"
    )]
    pub unit: Option<String>,
    /// Source value type.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_kind"
    )]
    pub kind: Option<String>,
    /// Source XML element that carries this value, including additional attributes.
    pub native_xml: String,
}

/// Byte provenance of one native XML record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManufacturingSource {
    /// Exactness of the source record interpretation.
    pub exactness: cadmpeg_ir::Exactness,
    /// Archive member path, including an enclosing document when present.
    pub member: String,
    /// Start byte in the XML stream.
    pub start: u64,
    /// Exclusive end byte in the XML stream.
    pub end: u64,
    /// Complete source record XML.
    pub native_xml: String,
}

/// Unresolved source geometry selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManufacturingSelection {
    /// Native selection element name.
    pub kind: String,
    /// Exact XML containing the native references.
    pub native_xml: String,
}

/// Stock dimensions and offsets in source units.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManufacturingStock {
    /// Source stock mode or shape selector.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_kind"
    )]
    pub kind: Option<String>,
    /// Box sizes, round diameters, and stock lengths, with units.
    pub dimensions: Vec<ManufacturingParameter>,
    /// Stock offsets, with units.
    pub offsets: Vec<ManufacturingParameter>,
    /// Complete stock parameters in source order.
    pub parameters: Vec<ManufacturingParameter>,
}

/// A source work coordinate system definition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManufacturingWcs {
    /// Orientation mode, axes, and frame parameters in source order.
    pub orientation: Vec<ManufacturingParameter>,
    /// Source origin mode.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_origin_mode"
    )]
    pub origin_mode: Option<String>,
    /// Origin point and placement parameters, with units.
    pub origin: Vec<ManufacturingParameter>,
    /// Complete WCS parameters in source order.
    pub parameters: Vec<ManufacturingParameter>,
}

/// A source setup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManufacturingSetup {
    /// Typed setup identity.
    pub id: SetupId,
    /// Source name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Stock shape, dimensions, and offsets.
    pub stock: ManufacturingStock,
    /// WCS orientation, origin mode, and point.
    pub wcs: ManufacturingWcs,
    /// Native model and fixture selections.
    pub model_refs: Vec<ManufacturingSelection>,
    /// All source parameters, in order.
    pub parameters: Vec<ManufacturingParameter>,
    /// Source byte range and exact XML.
    pub source: ManufacturingSource,
}

/// Normalized machining strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
pub(crate) enum ManufacturingStrategy {
    /// Planar face milling.
    Face,
    /// Planar adaptive clearing.
    Adaptive2d,
    /// Planar pocket milling.
    Pocket2d,
    /// Planar contour milling.
    Contour2d,
    /// Drilling cycles.
    Drilling,
    /// Spatial adaptive clearing.
    Adaptive3d,
    /// Parallel finishing.
    Parallel,
    /// Scallop finishing.
    Scallop,
    /// Another source strategy.
    Other,
}

/// A source machining operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManufacturingOperation {
    /// Typed operation identity.
    pub id: OperationId,
    /// Owning setup.
    pub setup: SetupId,
    /// Zero-based order within the setup.
    pub order: u64,
    /// Source name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Strategy name as stored.
    pub strategy: String,
    /// Normalized strategy family.
    pub family: ManufacturingStrategy,
    /// Referenced cutting tool, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tool"
    )]
    pub tool: Option<ToolId>,
    /// All source parameters in order.
    pub parameters: Vec<ManufacturingParameter>,
    /// Native geometry selections.
    pub geometry: Vec<ManufacturingSelection>,
    /// A computed-path descriptor is present.
    pub computed_path_present: bool,
    /// Source computed-path validity flag.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_computed_path_valid"
    )]
    pub computed_path_valid: Option<bool>,
    /// Source byte range and exact XML.
    pub source: ManufacturingSource,
}

/// A source cutting tool and holder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManufacturingTool {
    /// Typed tool identity.
    pub id: ToolId,
    /// Source tool number text.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_number"
    )]
    pub number: Option<String>,
    /// Source tool type.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tool_type"
    )]
    pub tool_type: Option<String>,
    /// Tool geometry, cutting data, and remaining source parameters.
    pub parameters: Vec<ManufacturingParameter>,
    /// Holder geometry and source parameters.
    pub holder: Vec<ManufacturingParameter>,
    /// Vendor text.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vendor"
    )]
    pub vendor: Option<String>,
    /// Product identifier.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_product"
    )]
    pub product: Option<String>,
    /// Source byte range and exact XML.
    pub source: ManufacturingSource,
}

cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_unit, String, "unit");
cadmpeg_core::named_optional_field!(deserialize_kind, String, "kind");
cadmpeg_core::named_optional_field!(deserialize_origin_mode, String, "origin_mode");
cadmpeg_core::named_optional_field!(deserialize_tool, ToolId, "tool");
cadmpeg_core::named_optional_field!(deserialize_computed_path_valid, bool, "computed_path_valid");
cadmpeg_core::named_optional_field!(deserialize_number, String, "number");
cadmpeg_core::named_optional_field!(deserialize_tool_type, String, "tool_type");
cadmpeg_core::named_optional_field!(deserialize_vendor, String, "vendor");
cadmpeg_core::named_optional_field!(deserialize_product, String, "product");
