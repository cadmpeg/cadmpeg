// SPDX-License-Identifier: Apache-2.0
//! Native body scalar triples with derived contiguous positions.

use crate::om::body_scalar_triple::ScalarTriple;
use crate::om::scalar::{PayloadScalarAtom, PayloadScalarEncoding};
use serde::{Deserialize, Serialize};

/// Three typed scalars anchored to an ordered operation body reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "FeatureOperationBodyScalarTripleWire",
    into = "FeatureOperationBodyScalarTripleWire"
)]
pub struct FeatureOperationBodyScalarTriple {
    /// Globally unique scalar-clause identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Three checked scalar atoms and their absolute source offsets.
    pub scalars: ScalarTriple,
}

#[derive(Serialize, Deserialize)]
struct FeatureOperationBodyScalarTripleWire {
    /// Globally unique scalar-clause identity.
    id: String,
    /// Owning operation label.
    operation_label: String,
    /// Zero-based body-reference occurrence order.
    body_reference_ordinal: u32,
    /// Serialized body object index.
    body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    branch: u8,
    /// Ordered finite scalar values.
    values: [f64; 3],
    /// Ordered serialized width forms.
    encodings: [PayloadScalarEncoding; 3],
    /// Exact serialized scalar atoms in value order.
    raw_values: [Vec<u8>; 3],
    /// Absolute file offsets of the three scalar markers.
    source_offsets: [u64; 3],
}

impl From<FeatureOperationBodyScalarTriple> for FeatureOperationBodyScalarTripleWire {
    fn from(value: FeatureOperationBodyScalarTriple) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            body_reference_ordinal: value.body_reference_ordinal,
            body_object_index: value.body_object_index,
            branch: value.branch,
            values: value.scalars.atoms().map(PayloadScalarAtom::value),
            encodings: value.scalars.atoms().map(PayloadScalarAtom::encoding),
            raw_values: value.scalars.atoms().map(|atom| atom.raw().to_vec()),
            source_offsets: value.scalars.source_offsets(),
        }
    }
}

impl TryFrom<FeatureOperationBodyScalarTripleWire> for FeatureOperationBodyScalarTriple {
    type Error = String;

    fn try_from(wire: FeatureOperationBodyScalarTripleWire) -> Result<Self, Self::Error> {
        let [a, b, c] = std::array::from_fn::<_, 3, _>(|i| {
            PayloadScalarAtom::from_wire(wire.values[i], wire.encodings[i], &wire.raw_values[i])
                .map_err(|error| format!("scalar[{i}]: {error}"))
        });
        let scalars = ScalarTriple::new(wire.source_offsets[0], [a?, b?, c?])
            .ok_or("source_offsets: scalar triple span overflow")?;
        if scalars.source_offsets() != wire.source_offsets {
            return Err("source_offsets: inconsistent contiguous scalar positions".into());
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            body_reference_ordinal: wire.body_reference_ordinal,
            body_object_index: wire.body_object_index,
            branch: wire.branch,
            scalars,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_triple_wire_requires_contiguous_positions_and_complete_span() {
        let mut wire = serde_json::json!({
            "id": "triple", "operation_label": "operation", "body_reference_ordinal": 0,
            "body_object_index": 10, "branch": 28, "values": [0.0, 3.0, 1.0],
            "encodings": ["zero", "binary32", "binary64"],
            "raw_values": [[0], [80, 64, 0, 0], [47, 240, 0, 0, 0, 0, 0, 0]],
            "source_offsets": [100, 101, 105]
        });
        for offsets in [
            [100, 102, 105],
            [100, 101, 106],
            [u64::MAX - 12, u64::MAX - 11, u64::MAX - 7],
        ] {
            wire["source_offsets"] = serde_json::json!(offsets);
            let error = serde_json::from_value::<FeatureOperationBodyScalarTriple>(wire.clone())
                .unwrap_err();
            assert!(error.to_string().contains("source_offsets"));
        }
        wire["source_offsets"] = serde_json::json!([u64::MAX - 13, u64::MAX - 12, u64::MAX - 8]);
        let triple: FeatureOperationBodyScalarTriple =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&triple).unwrap(), wire);
        assert!(triple.scalars.relocate(1).is_none());
    }
}
