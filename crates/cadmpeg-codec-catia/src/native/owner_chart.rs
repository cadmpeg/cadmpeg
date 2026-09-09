// SPDX-License-Identifier: Apache-2.0
//! Native owner-chart carriers, bridge references, and alias bindings.

use serde::{Deserialize, Serialize};

use super::CatiaAllocationReferenceEncoding;

/// Parameter axis held constant by selectors `0x05` and `0x09` in a
/// consolidated owner chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerChartSideAxis {
    /// First surface parameter.
    FirstParameter,
    /// Second surface parameter.
    SecondParameter,
}

/// Family-and-class carrier production that opens an owner chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct CatiaOwnerChartAliasBinding {
    /// Exact outer alias row.
    pub row: String,
    /// Canonical persistent surface tag selected through the alias row.
    pub canonical_tag: Option<u32>,
}

/// One allocation-local reference in an owner-chart bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Middle construction control in a five-reference owner-chart bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatiaOwnerChartMiddleControl {
    /// Control byte `0x03`.
    Control03,
    /// Control byte `0x05`.
    Control05,
}

impl CatiaOwnerChartMiddleControl {
    pub(crate) fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x03 => Some(Self::Control03),
            0x05 => Some(Self::Control05),
            _ => None,
        }
    }

    pub(crate) fn as_byte(self) -> u8 {
        match self {
            Self::Control03 => 0x03,
            Self::Control05 => 0x05,
        }
    }
}

/// Terminal construction control in a five-reference owner-chart bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatiaOwnerChartTerminalControl {
    /// Control byte `0x01`.
    Control01,
    /// Control byte `0x05`.
    Control05,
}

impl CatiaOwnerChartTerminalControl {
    pub(crate) fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::Control01),
            0x05 => Some(Self::Control05),
            _ => None,
        }
    }

    pub(crate) fn as_byte(self) -> u8 {
        match self {
            Self::Control01 => 0x01,
            Self::Control05 => 0x05,
        }
    }
}

/// Structurally complete class-`0x37` owner-chart bridge.
#[derive(Debug, Clone, PartialEq)]
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
        /// Independent middle construction controls.
        middle_controls: [CatiaOwnerChartMiddleControl; 2],
        /// Independent terminal control.
        terminal_control: CatiaOwnerChartTerminalControl,
        /// Positive construction radius.
        construction_radius: f64,
    },
    /// Eight-reference A-family production without an assigned object role.
    Extended {
        /// Record byte offset.
        byte_offset: u64,
        /// Counted allocation references in storage order.
        references: [CatiaOwnerChartBridgeReference; 8],
    },
}

/// Compatibility representation with explicit framing controls.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatiaOwnerChartBridgeWire {
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

impl CatiaOwnerChartCarrier {
    fn selector(self) -> u8 {
        match self {
            Self::B28 => 0x05,
            Self::B2b => 0x09,
            Self::A32 => 0x11,
        }
    }
}

impl CatiaOwnerChartBridgeWire {
    fn from_bridge(bridge: CatiaOwnerChartBridge, carrier: CatiaOwnerChartCarrier) -> Self {
        match bridge {
            CatiaOwnerChartBridge::SupportedSurface {
                byte_offset,
                carrier_surface,
                support_surfaces,
                support_pcurves,
                middle_controls,
                terminal_control,
                construction_radius,
            } => Self::SupportedSurface {
                byte_offset,
                carrier_surface,
                support_surfaces,
                support_pcurves,
                controls: [
                    carrier.selector(),
                    0x05,
                    middle_controls[0].as_byte(),
                    middle_controls[1].as_byte(),
                    terminal_control.as_byte(),
                    0x05,
                ],
                construction_radius,
            },
            CatiaOwnerChartBridge::Extended {
                byte_offset,
                references,
            } => Self::Extended {
                byte_offset,
                references,
                controls: [carrier.selector(), 0x09, 0x05, 0x05],
                terminal_controls: [0x01, 0x05],
            },
        }
    }

    fn into_bridge(self, carrier: CatiaOwnerChartCarrier) -> Result<CatiaOwnerChartBridge, String> {
        match self {
            Self::SupportedSurface {
                byte_offset,
                carrier_surface,
                support_surfaces,
                support_pcurves,
                controls,
                construction_radius,
            } => {
                if [controls[0], controls[1], controls[5]] != [carrier.selector(), 0x05, 0x05] {
                    return Err(
                        "owner-chart bridge framing controls do not match carrier".to_owned()
                    );
                }
                if !construction_radius.is_finite() || construction_radius <= 0.0 {
                    return Err("construction_radius must be finite and positive".to_owned());
                }
                let middle_controls = [
                    CatiaOwnerChartMiddleControl::from_byte(controls[2])
                        .ok_or("invalid first owner-chart middle control")?,
                    CatiaOwnerChartMiddleControl::from_byte(controls[3])
                        .ok_or("invalid second owner-chart middle control")?,
                ];
                let terminal_control = CatiaOwnerChartTerminalControl::from_byte(controls[4])
                    .ok_or("invalid owner-chart terminal control")?;
                Ok(CatiaOwnerChartBridge::SupportedSurface {
                    byte_offset,
                    carrier_surface,
                    support_surfaces,
                    support_pcurves,
                    middle_controls,
                    terminal_control,
                    construction_radius,
                })
            }
            Self::Extended {
                byte_offset,
                references,
                controls,
                terminal_controls,
            } => {
                if controls != [carrier.selector(), 0x09, 0x05, 0x05]
                    || terminal_controls != [0x01, 0x05]
                {
                    return Err(
                        "extended owner-chart bridge framing controls do not match carrier"
                            .to_owned(),
                    );
                }
                Ok(CatiaOwnerChartBridge::Extended {
                    byte_offset,
                    references,
                })
            }
        }
    }
}

/// Source-closed carrier chart terminated by an owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
struct CatiaOwnerChartRelationWire {
    carrier_byte_offset: u64,
    carrier: CatiaOwnerChartCarrier,
    bridge: CatiaOwnerChartBridgeWire,
    side_axis: CatiaOwnerChartSideAxis,
    parameter_point_byte_offsets: [u64; 4],
}

impl From<CatiaOwnerChartRelation> for CatiaOwnerChartRelationWire {
    fn from(value: CatiaOwnerChartRelation) -> Self {
        let side_axis = value.side_axis();
        Self {
            carrier_byte_offset: value.carrier_byte_offset,
            carrier: value.carrier,
            bridge: CatiaOwnerChartBridgeWire::from_bridge(value.bridge, value.carrier),
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
            bridge: wire.bridge.into_bridge(wire.carrier)?,
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
    fn bridge_wire_checks_framing_and_variable_controls() {
        for bytes in [
            crate::test_support::b2_owner_chart_stream(0x28),
            crate::test_support::b2_owner_chart_stream(0x2b),
            crate::test_support::b2_owner_chart_stream(0x32),
            crate::test_support::b2_owner_chart_stream_with_extended_bridge(),
        ] {
            let native = crate::native::CatiaNative::decode(&bytes);
            let relation = native.consolidated_owner_packets[0]
                .owner_chart()
                .expect("owner chart");
            let wire = serde_json::to_value(relation).expect("serialize owner chart");
            let decoded: CatiaOwnerChartRelation =
                serde_json::from_value(wire.clone()).expect("valid bridge controls");
            assert_eq!(&decoded, relation);
            for field in ["controls", "terminal_controls"] {
                if let Some(controls) = wire["bridge"][field].as_array() {
                    for index in 0..controls.len() {
                        let mut invalid = wire.clone();
                        invalid["bridge"][field][index] = json!(0);
                        assert!(serde_json::from_value::<CatiaOwnerChartRelation>(invalid).is_err());
                    }
                }
            }
        }
    }

    #[test]
    fn bridge_wire_rejects_invalid_construction_radius() {
        let native =
            crate::native::CatiaNative::decode(&crate::test_support::b2_owner_chart_stream(0x28));
        let relation = native.consolidated_owner_packets[0].owner_chart().unwrap();
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut wire =
                CatiaOwnerChartBridgeWire::from_bridge(relation.bridge.clone(), relation.carrier);
            let CatiaOwnerChartBridgeWire::SupportedSurface {
                construction_radius,
                ..
            } = &mut wire
            else {
                panic!("supported surface");
            };
            *construction_radius = radius;
            assert_eq!(
                wire.into_bridge(relation.carrier).unwrap_err(),
                "construction_radius must be finite and positive"
            );
        }
    }

    #[test]
    fn bridge_preserves_independent_middle_control_bytes() {
        let native =
            crate::native::CatiaNative::decode(&crate::test_support::b2_owner_chart_stream(0x28));
        let relation = native.consolidated_owner_packets[0]
            .owner_chart()
            .expect("source-closed owner chart");
        let original = serde_json::to_value(relation).expect("serialize owner chart");
        for first in [0x03, 0x05] {
            for second in [0x03, 0x05] {
                let mut wire = original.clone();
                wire["bridge"]["controls"][2] = json!(first);
                wire["bridge"]["controls"][3] = json!(second);
                let decoded: CatiaOwnerChartRelation =
                    serde_json::from_value(wire.clone()).expect("independent middle controls");
                assert_eq!(
                    serde_json::to_value(decoded).expect("serialize admitted middle controls"),
                    wire
                );
            }
        }
    }

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
