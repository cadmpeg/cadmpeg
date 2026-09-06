// SPDX-License-Identifier: Apache-2.0
//! Wire adapters for closed scalar-pair framing.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FeatureDatumCsysPayloadFixedPairWire {
    /// Globally unique fixed-pair identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub datum_csys_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    #[serde(flatten, with = "crate::om::fixed::pair_wire")]
    pub values: [Q155; 2],
    /// Exact discriminator selecting the pair branch.
    pub discriminator: Vec<u8>,
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the two `30` atom markers.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two `30` atom markers.
    pub value_source_offsets: [u64; 2],
}

impl TryFrom<FeatureDatumCsysPayloadFixedPairWire> for FeatureDatumCsysPayloadFixedPair {
    type Error = String;
    fn try_from(wire: FeatureDatumCsysPayloadFixedPairWire) -> Result<Self, Self::Error> {
        Ok(Self {
            position: PairPosition::from_wire(
                &wire.discriminator,
                wire.payload_offset,
                wire.value_payload_offsets,
            )?,
            id: wire.id,
            operation_label: wire.operation_label,
            datum_csys_payload: wire.datum_csys_payload,
            ordinal: wire.ordinal,
            values: wire.values,
            source_offset: wire.source_offset,
            value_source_offsets: wire.value_source_offsets,
        })
    }
}
impl From<FeatureDatumCsysPayloadFixedPair> for FeatureDatumCsysPayloadFixedPairWire {
    fn from(value: FeatureDatumCsysPayloadFixedPair) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            datum_csys_payload: value.datum_csys_payload,
            ordinal: value.ordinal,
            values: value.values,
            source_offset: value.source_offset,
            value_source_offsets: value.value_source_offsets,
            discriminator: value.position.discriminator().to_vec(),
            payload_offset: value.position.offset(),
            value_payload_offsets: value.position.value_offsets(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FeatureSketchPayloadFixedPairWire {
    /// Globally unique fixed-pair identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    pub construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    #[serde(flatten, with = "crate::om::sketch_scalar::pair_wire")]
    pub values: [SketchScaledAtom; 2],
    /// Exact discriminator and branch prefix selecting the pair layout.
    pub discriminator: Vec<u8>,
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the two atom markers.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    pub value_source_offsets: [u64; 2],
}

impl TryFrom<FeatureSketchPayloadFixedPairWire> for FeatureSketchPayloadFixedPair {
    type Error = String;
    fn try_from(wire: FeatureSketchPayloadFixedPairWire) -> Result<Self, Self::Error> {
        Ok(Self {
            position: PairPosition::from_wire(
                &wire.discriminator,
                wire.payload_offset,
                wire.value_payload_offsets,
            )?,
            id: wire.id,
            operation_label: wire.operation_label,
            construction_payload: wire.construction_payload,
            ordinal: wire.ordinal,
            values: wire.values,
            source_offset: wire.source_offset,
            value_source_offsets: wire.value_source_offsets,
        })
    }
}
impl From<FeatureSketchPayloadFixedPair> for FeatureSketchPayloadFixedPairWire {
    fn from(value: FeatureSketchPayloadFixedPair) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            construction_payload: value.construction_payload,
            ordinal: value.ordinal,
            values: value.values,
            source_offset: value.source_offset,
            value_source_offsets: value.value_source_offsets,
            discriminator: value.position.discriminator().to_vec(),
            payload_offset: value.position.offset(),
            value_payload_offsets: value.position.value_offsets(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FeatureSketchPayloadMixedPairWire {
    /// Globally unique mixed-pair identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    pub construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Exact scaled binary64 and binary32 atoms.
    #[serde(flatten)]
    pub scalars: SketchMixedScalars,
    /// Exact discriminator selecting the mixed pair layout.
    pub discriminator: Vec<u8>,
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the two atom markers.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    pub value_source_offsets: [u64; 2],
}

impl TryFrom<FeatureSketchPayloadMixedPairWire> for FeatureSketchPayloadMixedPair {
    type Error = String;
    fn try_from(wire: FeatureSketchPayloadMixedPairWire) -> Result<Self, Self::Error> {
        Ok(Self {
            position: PairPosition::from_wire(
                &wire.discriminator,
                wire.payload_offset,
                wire.value_payload_offsets,
            )?,
            id: wire.id,
            operation_label: wire.operation_label,
            construction_payload: wire.construction_payload,
            ordinal: wire.ordinal,
            scalars: wire.scalars,
            source_offset: wire.source_offset,
            value_source_offsets: wire.value_source_offsets,
        })
    }
}
impl From<FeatureSketchPayloadMixedPair> for FeatureSketchPayloadMixedPairWire {
    fn from(value: FeatureSketchPayloadMixedPair) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            construction_payload: value.construction_payload,
            ordinal: value.ordinal,
            scalars: value.scalars,
            source_offset: value.source_offset,
            value_source_offsets: value.value_source_offsets,
            discriminator: value.position.discriminator().to_vec(),
            payload_offset: value.position.offset(),
            value_payload_offsets: value.position.value_offsets(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_pair_wire_rejects_unknown_framing_and_inconsistent_offsets() {
        let pair = FeatureSketchPayloadFixedPair {
            id: "pair".into(),
            operation_label: "operation".into(),
            construction_payload: "payload".into(),
            ordinal: 0,
            values: [SketchScaledAtom::from_raw([0; 7]); 2],
            position: PairPosition::new(SketchPairForm::Legacy, 20).unwrap(),
            source_offset: 1020,
            value_source_offsets: [1028, 2037],
        };
        let wire = serde_json::to_value(&pair).unwrap();
        assert_eq!(
            serde_json::from_value::<FeatureSketchPayloadFixedPair>(wire.clone()).unwrap(),
            pair
        );
        for (field, invalid) in [
            ("discriminator", serde_json::json!([4])),
            ("value_payload_offsets", serde_json::json!([28, 38])),
            ("payload_offset", serde_json::json!(u64::MAX)),
        ] {
            let mut invalid_wire = wire.clone();
            invalid_wire[field] = invalid;
            let error =
                serde_json::from_value::<FeatureSketchPayloadFixedPair>(invalid_wire).unwrap_err();
            assert!(error.to_string().contains(field), "{error}");
        }
    }
}
