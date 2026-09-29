// SPDX-License-Identifier: Apache-2.0
//! Borrowed JSON wire views for consolidated CATIA native records.

use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;
use serde::Serialize;

use super::owner_chart::CatiaOwnerChartRelation;
use super::{
    slice_is_empty, CatiaCircleLayout, CatiaConsolidatedCircle, CatiaConsolidatedCylinder,
    CatiaConsolidatedCylinderPayload, CatiaConsolidatedCylinderPayloadWire,
    CatiaConsolidatedOwnerPacket, CatiaConsolidatedParameterPoint,
    CatiaConsolidatedParameterPointPayload, CatiaConsolidatedPlaneCarrier,
    CatiaConsolidatedPlaneCarrierPayload, CatiaFaceNodeRelation, CatiaOwnerBoundaryCycle,
    CatiaOwnerIdentityEncoding, CatiaOwnerIdentityTarget, CatiaOwnerNumericTail,
    CatiaOwnerPacketPayload, CatiaOwnerReferenceEncoding, ConsolidatedFrameFlag,
    ConsolidatedFrameWidth,
};

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatiaOwnerPacketPayloadWireRef<'a> {
    FixedNine {
        reference_encoding: CatiaOwnerReferenceEncoding,
        references: &'a [u32; 9],
        identity_encodings: &'a [CatiaOwnerIdentityEncoding; 9],
        numeric_tail: &'a CatiaOwnerNumericTail,
    },
    Counted {
        references: &'a [u32],
        #[serde(with = "cadmpeg_ir::bytes")]
        tail: &'a [u8],
    },
}

#[derive(Serialize)]
struct CatiaConsolidatedOwnerPacketWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    source_index: usize,
    header_token: u32,
    payload: CatiaOwnerPacketPayloadWireRef<'a>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    identity_targets: &'a [CatiaOwnerIdentityTarget],
    #[serde(skip_serializing_if = "Option::is_none")]
    face_node: &'a Option<CatiaFaceNodeRelation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_chart: Option<&'a CatiaOwnerChartRelation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    boundary_cycle: Option<&'a CatiaOwnerBoundaryCycle>,
}

impl Serialize for CatiaConsolidatedOwnerPacket {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (payload, identity_targets, owner_chart, boundary_cycle) = match &self.payload {
            CatiaOwnerPacketPayload::FixedNine {
                reference_encoding,
                references,
                identity_encodings,
                numeric_tail,
                identity_targets,
                owner_chart,
                boundary_cycle,
            } => (
                CatiaOwnerPacketPayloadWireRef::FixedNine {
                    reference_encoding: *reference_encoding,
                    references,
                    identity_encodings,
                    numeric_tail,
                },
                identity_targets.as_slice(),
                owner_chart.as_ref(),
                boundary_cycle.as_ref(),
            ),
            CatiaOwnerPacketPayload::Counted { references, tail } => (
                CatiaOwnerPacketPayloadWireRef::Counted { references, tail },
                &[][..],
                None,
                None,
            ),
        };
        CatiaConsolidatedOwnerPacketWireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            source_index: self.source_index,
            header_token: self.header_token,
            payload,
            identity_targets,
            face_node: &self.face_node,
            owner_chart,
            boundary_cycle,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct CatiaConsolidatedCircleWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    layout: CatiaCircleLayout,
    record_id: u32,
    frame_token: u8,
    center_pair: &'a FiniteVector<2>,
    radius: &'a cadmpeg_ir::scalar::PositiveLength,
    range: &'a cadmpeg_ir::topology::IncreasingParameterInterval,
    full_circle: bool,
    chart_shift: &'a FiniteReal,
}
impl Serialize for CatiaConsolidatedCircle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CatiaConsolidatedCircleWireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            layout: self.layout,
            record_id: self.record_id,
            frame_token: self.frame_token,
            center_pair: &self.center_pair,
            radius: &self.radius,
            range: &self.range,
            full_circle: self.full_circle(),
            chart_shift: &self.chart_shift,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct CatiaConsolidatedCylinderWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    layout: u8,
    origin: &'a FiniteVector<3>,
    radius: &'a cadmpeg_ir::scalar::PositiveLength,
    u_range: &'a cadmpeg_ir::topology::IncreasingParameterInterval,
    v_range: &'a cadmpeg_ir::topology::IncreasingParameterInterval,
    payload: CatiaConsolidatedCylinderPayloadWire,
}

impl Serialize for CatiaConsolidatedCylinder {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let payload = match &self.payload {
            CatiaConsolidatedCylinderPayload::Layout52 {
                frame_token,
                axis,
                reference_direction,
            }
            | CatiaConsolidatedCylinderPayload::Layout5a {
                frame_token,
                axis,
                reference_direction,
            } => CatiaConsolidatedCylinderPayloadWire::Resolved {
                frame_token: *frame_token,
                axis: *axis,
                reference_direction: *reference_direction,
            },
            CatiaConsolidatedCylinderPayload::RangeOrigin {
                stored_vector,
                axis,
                reference_direction,
                range_origin,
            } => CatiaConsolidatedCylinderPayloadWire::RangeOrigin {
                stored_vector: *stored_vector,
                axis: *axis,
                reference_direction: *reference_direction,
                range_origin: *range_origin,
            },
        };
        CatiaConsolidatedCylinderWireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            layout: self.payload.layout(),
            origin: &self.origin,
            radius: &self.radius,
            u_range: &self.u_range,
            v_range: &self.v_range,
            payload,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct CatiaConsolidatedParameterPointWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    byte_len: u64,
    layout: u8,
    prefix: u8,
    control: u8,
    payload: &'a CatiaConsolidatedParameterPointPayload,
}

impl Serialize for CatiaConsolidatedParameterPoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CatiaConsolidatedParameterPointWireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            byte_len: self.byte_len,
            layout: self.payload.layout(),
            prefix: self.prefix.as_u8(),
            control: self.control,
            payload: &self.payload,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct CatiaConsolidatedPlaneCarrierWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    byte_len: u64,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    header_token: u32,
    selector: u8,
    payload: &'a CatiaConsolidatedPlaneCarrierPayload,
}

impl Serialize for CatiaConsolidatedPlaneCarrier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CatiaConsolidatedPlaneCarrierWireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            byte_len: self.byte_len,
            width: self.width,
            flag: self.flag,
            header_token: self.header_token,
            selector: self.payload.selector(),
            payload: &self.payload,
        }
        .serialize(serializer)
    }
}
