// SPDX-License-Identifier: Apache-2.0
//! Exact two-support chamfer syntax and contextual compact contact identities.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    parameter_points: &[CatiaConsolidatedParameterPoint],
) -> Result<Vec<CatiaConsolidatedChamfer>, CodecError> {
    let constructions = b2::chamfer::records(ctx, bytes)?;
    if constructions.is_empty() {
        return Ok(Vec::new());
    }
    let mut point_ids = std::collections::HashMap::new();
    for point in parameter_points {
        ctx.insert_hash_map(
            &mut point_ids,
            point.byte_offset,
            point.id.as_str(),
            "catia_chamfer_point_index",
        )?;
    }
    let mut output = Vec::new();
    for (index, construction) in constructions.into_iter().enumerate() {
        let positions = b2::chamfer::parameter_positions(ctx, bytes, records, &construction)?;
        let mut contacts = Vec::new();
        for position in positions {
            let Some(id) = point_ids.get(&u64_from_index(position)) else {
                contacts.clear();
                break;
            };
            let id = ctx.copy_retained_text(id, "catia_chamfer_parameter_id")?;
            ctx.push_vec(&mut contacts, id, "catia_chamfer_parameter_points")?;
        }
        let value = CatiaConsolidatedChamfer {
            id: ctx.format_retained(
                format_args!("catia:consolidated:chamfer#{index}"),
                "catia_native_chamfer_id",
            )?,
            byte_offset: u64_from_index(construction.pos),
            byte_len: u64_from_index(construction.end - construction.pos),
            support_surfaces: construction.support_surfaces,
            support_pcurves: construction.support_pcurves,
            parameters: construction.parameters,
            parameter_points: contacts,
        };
        ctx.push_vec(&mut output, value, "catia_native_chamfers")?;
    }
    Ok(output)
}

pub(super) fn object_chamfers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<CatiaObjectChamfer>, CodecError> {
    let mut output = Vec::new();
    for (index, construction) in b5::chamfer::records(ctx, bytes)?.into_iter().enumerate() {
        let value = CatiaObjectChamfer {
            id: ctx.format_retained(
                format_args!("catia:object:chamfer#{index}"),
                "catia_native_object_chamfer_id",
            )?,
            byte_offset: u64_from_index(construction.pos),
            byte_len: u64_from_index(construction.end - construction.pos),
            object_id: construction.object_id,
            references: construction.references,
            parameters: construction.parameters,
        };
        ctx.push_vec(&mut output, value, "catia_native_object_chamfers")?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{consolidated_chamfers, object_chamfers, CatiaObjectChamfer};
    use crate::test_support::{test_b2::b2_chamfer_stream, with_service_context};

    #[test]
    fn native_chamfer_identity_and_output_obey_budgets() {
        let bytes = b2_chamfer_stream();
        let inventory = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(1, |ctx| {
            consolidated_chamfers(ctx, &bytes, &inventory, &[])
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error)) if error.operation == "catia_native_chamfers")
        );
        let limited =
            crate::test_support::with_retained_refusal(&[], "catia_native_chamfer_id", |ctx| {
                consolidated_chamfers(ctx, &bytes, &inventory, &[])
            });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error)) if error.operation == "catia_native_chamfer_id")
        );
    }

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
        let rows = with_service_context(|ctx| object_chamfers(ctx, &bytes))
            .expect("valid bounded chamfer fixture");
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
        assert!(with_service_context(|ctx| object_chamfers(ctx, &bytes))
            .expect("valid bounded chamfer fixture")
            .is_empty());
    }
}
