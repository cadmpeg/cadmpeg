// SPDX-License-Identifier: Apache-2.0
//! Wire adapters for closed scalar-pair framing.
use super::{
    Deserialize, FeatureDatumCsysPayloadFixedPair, FeatureSketchPayloadFixedPair,
    FeatureSketchPayloadMixedPair, PairPosition, Serialize, SketchMixedScalars, SketchScaledAtom,
    Q155,
};

#[derive(Serialize)]
struct FixedPairRef<'a, T: Serialize> {
    id: &'a str,
    operation_label: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    datum_csys_payload: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    construction_payload: Option<&'a str>,
    ordinal: u32,
    #[serde(flatten)]
    values: T,
    discriminator: &'a [u8],
    payload_offset: u64,
    value_payload_offsets: [u64; 2],
    source_offset: u64,
    value_source_offsets: [u64; 2],
}

#[derive(Serialize)]
struct FixedValues {
    values: [f64; 2],
    raw_values: [[u8; 7]; 2],
}

impl Serialize for FeatureDatumCsysPayloadFixedPair {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FixedPairRef {
            id: &self.id,
            operation_label: &self.operation_label,
            datum_csys_payload: Some(&self.datum_csys_payload),
            construction_payload: None,
            ordinal: self.ordinal,
            values: FixedValues {
                values: self.values.map(Q155::value),
                raw_values: self.values.map(Q155::raw),
            },
            discriminator: self.position.discriminator(),
            payload_offset: self.position.offset(),
            value_payload_offsets: self.position.value_offsets(),
            source_offset: self.source_offset,
            value_source_offsets: self.value_source_offsets,
        }
        .serialize(serializer)
    }
}

impl Serialize for FeatureSketchPayloadFixedPair {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FixedPairRef {
            id: &self.id,
            operation_label: &self.operation_label,
            datum_csys_payload: None,
            construction_payload: Some(&self.construction_payload),
            ordinal: self.ordinal,
            values: FixedValues {
                values: self.values.map(SketchScaledAtom::value),
                raw_values: self.values.map(SketchScaledAtom::raw),
            },
            discriminator: self.position.discriminator(),
            payload_offset: self.position.offset(),
            value_payload_offsets: self.position.value_offsets(),
            source_offset: self.source_offset,
            value_source_offsets: self.value_source_offsets,
        }
        .serialize(serializer)
    }
}

impl Serialize for FeatureSketchPayloadMixedPair {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FixedPairRef {
            id: &self.id,
            operation_label: &self.operation_label,
            datum_csys_payload: None,
            construction_payload: Some(&self.construction_payload),
            ordinal: self.ordinal,
            values: &self.scalars,
            discriminator: self.position.discriminator(),
            payload_offset: self.position.offset(),
            value_payload_offsets: self.position.value_offsets(),
            source_offset: self.source_offset,
            value_source_offsets: self.value_source_offsets,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FeatureDatumCsysPayloadFixedPairWire {
    /// Globally unique fixed-pair identity.
    id: String,
    /// Owning `DATUM_CSYS` operation label.
    operation_label: String,
    /// Reconstructed payload carrying the frame.
    datum_csys_payload: String,
    /// Zero-based frame order within the payload.
    ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    #[serde(flatten, with = "crate::om::fixed::pair_wire")]
    values: [Q155; 2],
    /// Exact discriminator selecting the pair branch.
    discriminator: Vec<u8>,
    /// Payload-relative offset of the discriminator.
    payload_offset: u64,
    /// Payload-relative offsets of the two `30` atom markers.
    value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    source_offset: u64,
    /// Absolute source offsets of the two `30` atom markers.
    value_source_offsets: [u64; 2],
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
#[cfg(test)]
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
    id: String,
    /// Owning `SKETCH` operation label.
    operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    construction_payload: String,
    /// Zero-based frame order within the payload.
    ordinal: u32,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    #[serde(flatten, with = "crate::om::sketch_scalar::pair_wire")]
    values: [SketchScaledAtom; 2],
    /// Exact discriminator and branch prefix selecting the pair layout.
    discriminator: Vec<u8>,
    /// Payload-relative offset of the discriminator.
    payload_offset: u64,
    /// Payload-relative offsets of the two atom markers.
    value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    value_source_offsets: [u64; 2],
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
#[cfg(test)]
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
    id: String,
    /// Owning `SKETCH` operation label.
    operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    construction_payload: String,
    /// Zero-based frame order within the payload.
    ordinal: u32,
    /// Exact scaled binary64 and binary32 atoms.
    #[serde(flatten)]
    scalars: SketchMixedScalars,
    /// Exact discriminator selecting the mixed pair layout.
    discriminator: Vec<u8>,
    /// Payload-relative offset of the discriminator.
    payload_offset: u64,
    /// Payload-relative offsets of the two atom markers.
    value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    value_source_offsets: [u64; 2],
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
#[cfg(test)]
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
    use super::super::FeatureSketchPayloadFixedPair;
    use super::super::PairPosition;
    use super::super::SketchScaledAtom;
    use super::super::{FeatureDatumCsysPayloadFixedPair, FeatureSketchPayloadMixedPair};
    use super::{
        FeatureDatumCsysPayloadFixedPairWire, FeatureSketchPayloadFixedPairWire,
        FeatureSketchPayloadMixedPairWire,
    };
    use crate::om::scalar_pair::SketchPairForm;

    #[test]
    fn datum_fixed_pair_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let scalar = crate::om::fixed::Q155::from_wire(0.0, [0; 7]).unwrap();
        let pair = FeatureDatumCsysPayloadFixedPair {
            id: "nx:feature:datum-pair#0".into(),
            operation_label: "operation".into(),
            datum_csys_payload: "payload".into(),
            ordinal: 0,
            values: [scalar; 2],
            position: PairPosition::new(crate::om::scalar_pair::DatumPairForm::Initial, 20)
                .unwrap(),
            source_offset: 1020,
            value_source_offsets: [1035, 1044],
        };
        assert_eq!(
            serde_json::to_vec(&pair).unwrap(),
            serde_json::to_vec(&FeatureDatumCsysPayloadFixedPairWire::from(pair.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &pair,
            serde_json::to_value(&pair).unwrap(),
        );
    }

    #[test]
    fn sketch_fixed_pair_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let pair = FeatureSketchPayloadFixedPair {
            id: "nx:feature:sketch-fixed-pair#0".into(),
            operation_label: "operation".into(),
            construction_payload: "payload".into(),
            ordinal: 0,
            values: [SketchScaledAtom::from_raw([0; 7]); 2],
            position: PairPosition::new(SketchPairForm::Legacy, 20).unwrap(),
            source_offset: 1020,
            value_source_offsets: [1028, 1037],
        };
        assert_eq!(
            serde_json::to_vec(&pair).unwrap(),
            serde_json::to_vec(&FeatureSketchPayloadFixedPairWire::from(pair.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &pair,
            serde_json::to_value(&pair).unwrap(),
        );
    }

    #[test]
    fn sketch_mixed_pair_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let scalars = serde_json::from_str(r#"{"fixed_value":0.5,"binary32_value":0.5,"fixed_raw_value":[0,0,0,0,0,0,0],"binary32_raw_value":[79,0,0,0]}"#).unwrap();
        let pair = FeatureSketchPayloadMixedPair {
            id: "nx:feature:sketch-mixed-pair#0".into(),
            operation_label: "operation".into(),
            construction_payload: "payload".into(),
            ordinal: 0,
            scalars,
            position: PairPosition::new(crate::om::scalar_pair::MixedPairForm, 20).unwrap(),
            source_offset: 1020,
            value_source_offsets: [1028, 1037],
        };
        assert_eq!(
            serde_json::to_vec(&pair).unwrap(),
            serde_json::to_vec(&FeatureSketchPayloadMixedPairWire::from(pair.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &pair,
            serde_json::to_value(&pair).unwrap(),
        );
    }

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
