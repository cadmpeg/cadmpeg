// SPDX-License-Identifier: Apache-2.0
//! ENLARGE native fields without inferred authored construction roles.

use super::{format_feature_history_id, visit_feature_history_operation_records};
use crate::container::Container;
use crate::om::enlarge::{EnlargeEnvelope, PARAMETER_CLASS};
use crate::om::reference_index::{FeatureReferenceToken, PayloadIndexToken};
use crate::om::scalar::{PayloadScalarAtom, PayloadScalarEncoding};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "EnvelopeWire")]
pub(in crate::native) struct FeatureEnlargeConstructionEnvelope {
    id: String,
    operation_label: String,
    field: EnlargeEnvelope,
    source_offset: u64,
}

#[derive(Deserialize)]
struct EnvelopeWire {
    id: String,
    operation_label: String,
    parameter_class_index: NonZeroU8,
    references: [PayloadIndexToken; 6],
    middle_index: FeatureReferenceToken,
    scalars: [f64; 4],
    raw_scalars: [NativeBytes<Vec<u8>>; 4],
    byte_len: u64,
    source_offset: u64,
}

impl TryFrom<EnvelopeWire> for FeatureEnlargeConstructionEnvelope {
    type Error = &'static str;

    fn try_from(wire: EnvelopeWire) -> Result<Self, Self::Error> {
        let mut scalars = [PayloadScalarAtom::Zero; 4];
        for (i, scalar) in scalars.iter_mut().enumerate() {
            let encoding = match wire.raw_scalars[i].len() {
                1 => PayloadScalarEncoding::Zero,
                8 => PayloadScalarEncoding::Binary64,
                _ => return Err("raw_scalars must contain zero or shifted binary64 atoms"),
            };
            *scalar =
                PayloadScalarAtom::from_wire(wire.scalars[i], encoding, &wire.raw_scalars[i])?;
        }
        let field = EnlargeEnvelope {
            parameter_class_index: wire.parameter_class_index,
            references: wire.references,
            middle: wire.middle_index,
            scalars,
        };
        if wire.byte_len != u64_from_index(field.byte_len())
            || wire.source_offset.checked_add(wire.byte_len).is_none()
        {
            return Err(
                "byte_len/source_offset must describe the complete bounded ENLARGE envelope",
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            field,
            source_offset: wire.source_offset,
        })
    }
}

impl Serialize for FeatureEnlargeConstructionEnvelope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct EnvelopeRef<'a> {
            id: &'a str,
            operation_label: &'a str,
            parameter_class_index: NonZeroU8,
            references: &'a [PayloadIndexToken; 6],
            middle_index: &'a FeatureReferenceToken,
            scalars: [f64; 4],
            raw_scalars: [NativeBytes<&'a [u8]>; 4],
            byte_len: u64,
            source_offset: u64,
        }
        EnvelopeRef {
            id: &self.id,
            operation_label: &self.operation_label,
            parameter_class_index: self.field.parameter_class_index,
            references: &self.field.references,
            middle_index: &self.field.middle,
            scalars: self.field.scalars.map(|s| s.value().get()),
            raw_scalars: std::array::from_fn(|i| self.field.scalars[i].raw().into()),
            byte_len: u64_from_index(self.field.byte_len()),
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

pub(in crate::native) fn feature_enlarge_construction_envelopes(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureEnlargeConstructionEnvelope>, CodecError> {
    let mut envelopes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let Some(field) = EnlargeEnvelope::read(record.payload_view()) else {
                return;
            };
            let projected = (|| -> Result<Option<_>, CodecError> {
                ctx.charge_work(
                    u64_from_index(PARAMETER_CLASS.len()),
                    "resolve NX ENLARGE parameter class",
                )?;
                if section
                    .types
                    .get(usize::from(field.parameter_class_index.get()) - 1)
                    .is_none_or(|class| class.name != PARAMETER_CLASS)
                {
                    return Ok(None);
                }
                let Some(source_offset) = entry_offset
                    .checked_add(u64_from_index(record.payload_offset()))
                    .filter(|offset| {
                        offset
                            .checked_add(u64_from_index(field.byte_len()))
                            .is_some()
                    })
                else {
                    return Ok(None);
                };
                let envelope = FeatureEnlargeConstructionEnvelope {
                    id: format_feature_history_id(
                        ctx,
                        "enlarge-construction-envelope",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    operation_label: format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    field,
                    source_offset,
                };
                ctx.reserve_vec(&mut envelopes, 1, "NX ENLARGE construction envelopes")?;
                Ok(Some(envelope))
            })();
            match projected {
                Ok(Some(envelope)) => envelopes.push(envelope),
                Ok(None) => {}
                Err(error) => failure = Some(error),
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(envelopes)
}

#[cfg(test)]
mod tests {
    use super::FeatureEnlargeConstructionEnvelope;

    fn wire() -> serde_json::Value {
        serde_json::json!({
            "id":"nx:test:enlarge#0", "operation_label":"nx:test:operation#0", "parameter_class_index":57,
            "references":[{"object_index":0,"raw_object_index":"f000"},
                {"object_index":256,"raw_object_index":"f10100"},
                {"object_index":2,"raw_object_index":"f002"},
                {"object_index":3,"raw_object_index":"f003"},
                {"object_index":4,"raw_object_index":"f004"},
                {"object_index":5,"raw_object_index":"f005"}],
            "middle_index":{"object_index":0,"raw_object_index":"8000"},
            "scalars":[1.0,0.0,0.0,0.0], "raw_scalars":["2ff0000000000000","00","00","00"],
            "byte_len":63,"source_offset":100
        })
    }

    #[test]
    fn enlarge_wire_checks_atoms_and_complete_span_before_native_admission() {
        let wire = wire();
        let envelope: FeatureEnlargeConstructionEnvelope =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&envelope).unwrap(), wire);
        cadmpeg_test_support::native_serialization::assert_native_limit(&envelope, wire.clone());
        for (field, value) in [
            ("parameter_class_index", serde_json::json!(0)),
            ("byte_len", serde_json::json!(62)),
            ("source_offset", serde_json::json!(u64::MAX)),
        ] {
            let mut invalid = wire.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<FeatureEnlargeConstructionEnvelope>(invalid).is_err());
        }
        let mut invalid = wire.clone();
        invalid["scalars"][0] = serde_json::json!(2.0);
        assert!(serde_json::from_value::<FeatureEnlargeConstructionEnvelope>(invalid).is_err());
        let mut invalid = wire.clone();
        invalid["raw_scalars"][1] = serde_json::json!("0000");
        assert!(serde_json::from_value::<FeatureEnlargeConstructionEnvelope>(invalid).is_err());
        let mut invalid = wire.clone();
        invalid["references"][0]["raw_object_index"] = serde_json::json!("00");
        assert!(serde_json::from_value::<FeatureEnlargeConstructionEnvelope>(invalid).is_err());
    }
}
