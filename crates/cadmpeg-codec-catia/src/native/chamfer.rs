// SPDX-License-Identifier: Apache-2.0
//! Exact two-support chamfer syntax and contextual compact contact identities.

use cadmpeg_core::decode::u64_from_index;
use serde::{Deserialize, Serialize};

use super::CatiaConsolidatedParameterPoint;
use crate::families::{b2, b5, chamfer::ChamferParameters};
use crate::wire::compact_unsigned::CompactUnsigned;
use crate::wire::object_reference::ObjectReference;
use crate::wire::records::ConsolidatedRecord;

/// Compact construction with an implicit result reference to the preceding frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaConsolidatedChamfer {
    pub(super) id: String,
    pub(super) byte_offset: u64,
    pub(super) byte_len: u64,
    pub(super) support_surfaces: [CompactUnsigned; 2],
    pub(super) support_pcurves: [CompactUnsigned; 2],
    pub(super) parameters: ChamferParameters,
    pub(crate) parameter_points: Vec<String>,
}

/// Object-id construction retaining all five source references and their encoding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaObjectChamfer {
    pub(super) id: String,
    pub(super) byte_offset: u64,
    pub(super) byte_len: u64,
    pub(super) object_id: u32,
    pub(super) references: [ObjectReference; 5],
    pub(super) parameters: ChamferParameters,
}

pub(super) fn consolidated_chamfers(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    parameter_points: &[CatiaConsolidatedParameterPoint],
) -> Vec<CatiaConsolidatedChamfer> {
    let constructions = b2::chamfer::records(bytes);
    if constructions.is_empty() {
        return Vec::new();
    }
    let mut point_ids = std::collections::HashMap::new();
    for point in parameter_points {
        point_ids.insert(point.byte_offset, point.id.as_str());
    }
    let mut output = Vec::new();
    for (index, construction) in constructions.into_iter().enumerate() {
        let positions = b2::chamfer::parameter_positions(bytes, records, &construction);
        let mut contacts = Vec::new();
        for position in positions {
            let Some(id) = point_ids.get(&u64_from_index(position)) else {
                contacts.clear();
                break;
            };
            let id = (id).to_owned();
            contacts.push(id.to_owned());
        }
        let value = CatiaConsolidatedChamfer {
            id: format!("catia:consolidated:chamfer#{index}"),
            byte_offset: u64_from_index(construction.pos),
            byte_len: u64_from_index(construction.end - construction.pos),
            support_surfaces: construction.support_surfaces,
            support_pcurves: construction.support_pcurves,
            parameters: construction.parameters,
            parameter_points: contacts,
        };
        output.push(value);
    }
    output
}

pub(super) fn object_chamfers(bytes: &[u8]) -> Vec<CatiaObjectChamfer> {
    let mut output = Vec::new();
    for (index, construction) in b5::chamfer::records(bytes).into_iter().enumerate() {
        let value = CatiaObjectChamfer {
            id: format!("catia:object:chamfer#{index}"),
            byte_offset: u64_from_index(construction.pos),
            byte_len: u64_from_index(construction.end - construction.pos),
            object_id: construction.object_id,
            references: construction.references,
            parameters: construction.parameters,
        };
        output.push(value);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{object_chamfers, CatiaObjectChamfer};
    use crate::test_support::with_service_context;

    #[test]
    fn object_chamfer_retains_sparse_references_and_angle_to_support() {
        let mut payload = vec![
            0x85, 0x28, 0x34, 0x02, 0x83, 0x84, 0x85, 0x86, 0x05, 0x05, 0x01, 0x03, 0x03, 0x11,
        ];
        payload.extend_from_slice(&0.2_f64.to_le_bytes());
        payload.extend_from_slice(&std::f64::consts::FRAC_PI_6.to_le_bytes());
        let mut bytes = vec![
            0xb5,
            0x03,
            0x3b,
            u8::try_from(payload.len()).expect("valid bounded chamfer fixture"),
        ];
        bytes.extend_from_slice(&7_u32.to_le_bytes());
        bytes.extend_from_slice(&payload);
        let rows = with_service_context(|_ctx| object_chamfers(&bytes));
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .references
                .map(crate::wire::object_reference::ObjectReference::value),
            [0x02_0034, 3, 4, 5, 6]
        );
        let wire = serde_json::to_value(&rows[0]).expect("valid bounded chamfer fixture");
        assert_eq!(wire["references"][0]["encoding"], "low_high_u8");
        let loaded: CatiaObjectChamfer =
            serde_json::from_value(wire.clone()).expect("valid bounded chamfer fixture");
        assert_eq!(loaded, rows[0]);
        let mut invalid = wire.clone();
        invalid["references"][0]["value"] = serde_json::json!(0x02_0134);
        assert!(serde_json::from_value::<CatiaObjectChamfer>(invalid).is_err());
        let mut invalid = wire;
        invalid["parameters"]["definition"]["angle"] =
            serde_json::json!(std::f64::consts::FRAC_PI_2);
        assert!(serde_json::from_value::<CatiaObjectChamfer>(invalid).is_err());
        let at = bytes.len() - 22;
        bytes[at + 2] = 3;
        assert!(with_service_context(|_ctx| object_chamfers(&bytes)).is_empty());
    }
}
