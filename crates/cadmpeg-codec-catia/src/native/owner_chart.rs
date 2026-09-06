// SPDX-License-Identifier: Apache-2.0
//! Native owner-chart carriers, bridge references, and alias bindings.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::CatiaAllocationReferenceEncoding;

/// Parameter axis held constant by selectors `0x05` and `0x09` in a
/// consolidated owner chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerChartSideAxis {
    /// First surface parameter.
    FirstParameter,
    /// Second surface parameter.
    SecondParameter,
}

/// Family-and-class carrier production that opens an owner chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerChartCarrier {
    /// B-family class-`0x28` cylinder carrier.
    B28,
    /// B-family class-`0x2b` torus carrier.
    B2b,
    /// A-family class-`0x32` carrier.
    A32,
}

/// Outer alias row selected by a unique width-coded support tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaOwnerChartAliasBinding {
    /// Exact outer alias row.
    pub row: String,
    /// Canonical persistent surface tag selected through the alias row.
    pub canonical_tag: Option<u32>,
}

/// One allocation-local reference in an owner-chart bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "CatiaOwnerChartBridgeReferenceWire",
    into = "CatiaOwnerChartBridgeReferenceWire"
)]
pub struct CatiaOwnerChartBridgeReference {
    /// Decoded allocation-local value.
    pub value: u32,
    /// Addressing form and its optional width-coded alias binding.
    pub address: CatiaOwnerChartAddress,
}

/// Addressing form of an owner-chart reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatiaOwnerChartAddress {
    /// Backward framed-record distance.
    BackwardDistance,
    /// Ordinal in the immediately owned allocation.
    OwnedChild,
    /// Width-coded persistent tag with an optional resolved alias.
    WidthCoded {
        alias: Option<CatiaOwnerChartAliasBinding>,
    },
    /// Untagged selector.
    Selector2,
    /// Tagged one-byte reference.
    TaggedU8,
    /// Tagged two-byte reference.
    TaggedU16,
}

impl CatiaOwnerChartBridgeReference {
    pub fn new(value: u32, encoding: CatiaAllocationReferenceEncoding) -> Self {
        let address = match encoding {
            CatiaAllocationReferenceEncoding::BackwardDistance => {
                CatiaOwnerChartAddress::BackwardDistance
            }
            CatiaAllocationReferenceEncoding::OwnedChild => CatiaOwnerChartAddress::OwnedChild,
            CatiaAllocationReferenceEncoding::WidthCoded => {
                CatiaOwnerChartAddress::WidthCoded { alias: None }
            }
            CatiaAllocationReferenceEncoding::Selector2 => CatiaOwnerChartAddress::Selector2,
            CatiaAllocationReferenceEncoding::TaggedU8 => CatiaOwnerChartAddress::TaggedU8,
            CatiaAllocationReferenceEncoding::TaggedU16 => CatiaOwnerChartAddress::TaggedU16,
        };
        Self { value, address }
    }

    pub fn encoding(&self) -> CatiaAllocationReferenceEncoding {
        match self.address {
            CatiaOwnerChartAddress::BackwardDistance => {
                CatiaAllocationReferenceEncoding::BackwardDistance
            }
            CatiaOwnerChartAddress::OwnedChild => CatiaAllocationReferenceEncoding::OwnedChild,
            CatiaOwnerChartAddress::WidthCoded { .. } => {
                CatiaAllocationReferenceEncoding::WidthCoded
            }
            CatiaOwnerChartAddress::Selector2 => CatiaAllocationReferenceEncoding::Selector2,
            CatiaOwnerChartAddress::TaggedU8 => CatiaAllocationReferenceEncoding::TaggedU8,
            CatiaOwnerChartAddress::TaggedU16 => CatiaAllocationReferenceEncoding::TaggedU16,
        }
    }

    #[cfg(test)]
    pub fn alias(&self) -> Option<&CatiaOwnerChartAliasBinding> {
        match &self.address {
            CatiaOwnerChartAddress::WidthCoded { alias } => alias.as_ref(),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CatiaOwnerChartBridgeReferenceWire {
    value: u32,
    encoding: CatiaAllocationReferenceEncoding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alias_row: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    canonical_surface_tag: Option<u32>,
}

impl From<CatiaOwnerChartBridgeReference> for CatiaOwnerChartBridgeReferenceWire {
    fn from(value: CatiaOwnerChartBridgeReference) -> Self {
        let encoding = value.encoding();
        let (alias_row, canonical_surface_tag) = match value.address {
            CatiaOwnerChartAddress::WidthCoded {
                alias: Some(binding),
            } => (Some(binding.row), binding.canonical_tag),
            _ => (None, None),
        };
        Self {
            value: value.value,
            encoding,
            alias_row,
            canonical_surface_tag,
        }
    }
}

impl TryFrom<CatiaOwnerChartBridgeReferenceWire> for CatiaOwnerChartBridgeReference {
    type Error = String;

    fn try_from(wire: CatiaOwnerChartBridgeReferenceWire) -> Result<Self, Self::Error> {
        let alias = match (wire.alias_row, wire.canonical_surface_tag) {
            (None, None) => None,
            (None, Some(_)) => {
                return Err("owner-chart canonical_surface_tag requires alias_row".to_owned());
            }
            (Some(row), canonical_tag) => Some(CatiaOwnerChartAliasBinding { row, canonical_tag }),
        };
        let mut reference = Self::new(wire.value, wire.encoding);
        match &mut reference.address {
            CatiaOwnerChartAddress::WidthCoded { alias: slot } => *slot = alias,
            _ if alias.is_some() => {
                return Err("owner-chart alias binding requires width-coded addressing".to_owned());
            }
            _ => {}
        }
        Ok(reference)
    }
}

/// Structurally complete class-`0x37` owner-chart bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaOwnerChartBridge {
    /// Five-reference supported-surface construction.
    SupportedSurface {
        /// Record byte offset.
        byte_offset: u64,
        /// Constructed carrier surface.
        carrier_surface: CatiaOwnerChartBridgeReference,
        /// Two supporting surfaces.
        support_surfaces: [CatiaOwnerChartBridgeReference; 2],
        /// Pcurves on the supporting surfaces.
        support_pcurves: [CatiaOwnerChartBridgeReference; 2],
        /// Six construction controls in storage order.
        controls: [u8; 6],
        /// Positive construction radius.
        construction_radius: f64,
    },
    /// Eight-reference A-family production without an assigned object role.
    Extended {
        /// Record byte offset.
        byte_offset: u64,
        /// Counted allocation references in storage order.
        references: [CatiaOwnerChartBridgeReference; 8],
        /// Four controls before the zero lane.
        controls: [u8; 4],
        /// Two terminal controls after the zero lane.
        terminal_controls: [u8; 2],
    },
}

/// Source-closed carrier chart terminated by an owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "CatiaOwnerChartRelationWire",
    into = "CatiaOwnerChartRelationWire"
)]
pub struct CatiaOwnerChartRelation {
    /// Carrier record byte offset.
    pub carrier_byte_offset: u64,
    /// Family-and-class carrier production.
    pub carrier: CatiaOwnerChartCarrier,
    /// Immediately following class-`0x37` bridge record.
    pub bridge: CatiaOwnerChartBridge,
    /// Byte offsets of selectors `0x05`, `0x09`, `0x0d`, and `0x11`.
    pub parameter_point_byte_offsets: [u64; 4],
}

impl CatiaOwnerChartRelation {
    pub fn side_axis(&self) -> CatiaOwnerChartSideAxis {
        match self.carrier {
            CatiaOwnerChartCarrier::B28 => CatiaOwnerChartSideAxis::FirstParameter,
            CatiaOwnerChartCarrier::B2b | CatiaOwnerChartCarrier::A32 => {
                CatiaOwnerChartSideAxis::SecondParameter
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CatiaOwnerChartRelationWire {
    carrier_byte_offset: u64,
    carrier: CatiaOwnerChartCarrier,
    bridge: CatiaOwnerChartBridge,
    side_axis: CatiaOwnerChartSideAxis,
    parameter_point_byte_offsets: [u64; 4],
}

impl From<CatiaOwnerChartRelation> for CatiaOwnerChartRelationWire {
    fn from(value: CatiaOwnerChartRelation) -> Self {
        let side_axis = value.side_axis();
        Self {
            carrier_byte_offset: value.carrier_byte_offset,
            carrier: value.carrier,
            bridge: value.bridge,
            side_axis,
            parameter_point_byte_offsets: value.parameter_point_byte_offsets,
        }
    }
}

impl TryFrom<CatiaOwnerChartRelationWire> for CatiaOwnerChartRelation {
    type Error = String;

    fn try_from(wire: CatiaOwnerChartRelationWire) -> Result<Self, Self::Error> {
        let value = Self {
            carrier_byte_offset: wire.carrier_byte_offset,
            carrier: wire.carrier,
            bridge: wire.bridge,
            parameter_point_byte_offsets: wire.parameter_point_byte_offsets,
        };
        if wire.side_axis != value.side_axis() {
            return Err("owner-chart side axis does not match carrier".to_owned());
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn relation_wire_checks_the_carrier_derived_axis() {
        for carrier_class in [0x28, 0x2b, 0x32] {
            let bytes = crate::test_support::b2_owner_chart_stream(carrier_class);
            let native = crate::native::CatiaNative::decode(&bytes);
            let relation = native.consolidated_owner_packets[0]
                .owner_chart()
                .expect("source-closed owner chart");
            let mut wire = serde_json::to_value(relation).expect("serialize owner chart");
            let decoded: CatiaOwnerChartRelation =
                serde_json::from_value(wire.clone()).expect("valid owner chart");
            assert_eq!(&decoded, relation);
            wire["side_axis"] = json!(if carrier_class == 0x28 {
                "second_parameter"
            } else {
                "first_parameter"
            });
            assert!(serde_json::from_value::<CatiaOwnerChartRelation>(wire).is_err());
        }
    }

    #[test]
    fn reference_wire_preserves_all_addressing_forms_and_width_coded_aliases() {
        for encoding in [
            "backward_distance",
            "owned_child",
            "width_coded",
            "selector2",
            "tagged_u8",
            "tagged_u16",
        ] {
            let wire = json!({ "value": 17, "encoding": encoding });
            let reference: CatiaOwnerChartBridgeReference = serde_json::from_value(wire.clone())
                .expect("unresolved reference accepts every allocation encoding");
            assert_eq!(
                serde_json::to_value(reference).expect("serialize reference"),
                wire
            );
        }
        for canonical_tag in [None, Some(23)] {
            let mut wire =
                json!({ "value": 17, "encoding": "width_coded", "alias_row": "alias-17" });
            if let Some(tag) = canonical_tag {
                wire["canonical_surface_tag"] = json!(tag);
            }
            let reference: CatiaOwnerChartBridgeReference = serde_json::from_value(wire.clone())
                .expect("width-coded alias may lack a canonical target");
            assert_eq!(
                serde_json::to_value(reference).expect("serialize alias"),
                wire
            );
        }
    }

    #[test]
    fn reference_wire_rejects_aliases_on_other_encodings_and_targets_without_rows() {
        for encoding in [
            "backward_distance",
            "owned_child",
            "selector2",
            "tagged_u8",
            "tagged_u16",
        ] {
            let wire = json!({ "value": 17, "encoding": encoding, "alias_row": "alias-17" });
            assert!(serde_json::from_value::<CatiaOwnerChartBridgeReference>(wire).is_err());
        }
        let wire = json!({ "value": 17, "encoding": "width_coded", "canonical_surface_tag": 23 });
        assert!(serde_json::from_value::<CatiaOwnerChartBridgeReference>(wire).is_err());
    }
}
