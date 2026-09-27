// SPDX-License-Identifier: Apache-2.0
//! Flat wire fields for complete support-UV tuples.

use super::ParasolidSupportUvRecord;
use crate::intersection::support_uv_values::{SupportUvPacking, SupportUvValues};
use crate::intersection::SupportUvFraming;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(super) struct SupportUvWire {
    id: String,
    stream_ordinal: u32,
    xmt: u32,
    count: u32,
    marker: u8,
    values: Vec<f64>,
    framing: SupportUvFraming,
    inflated_offset: u64,
}

#[derive(Serialize)]
struct SupportUvRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    xmt: u32,
    count: u32,
    marker: u8,
    values: &'a [cadmpeg_ir::scalar::FiniteReal],
    framing: SupportUvFraming,
    inflated_offset: u64,
}

impl Serialize for ParasolidSupportUvRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SupportUvRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            xmt: self.xmt,
            count: self.values.count(),
            marker: self.values.marker(),
            values: self.values.values(),
            framing: self.framing,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<ParasolidSupportUvRecord> for SupportUvWire {
    fn from(value: ParasolidSupportUvRecord) -> Self {
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt,
            count: value.values.count(),
            marker: value.values.marker(),
            values: value.values.into_values(),
            framing: value.framing,
            inflated_offset: value.inflated_offset,
        }
    }
}
impl TryFrom<SupportUvWire> for ParasolidSupportUvRecord {
    type Error = &'static str;
    fn try_from(wire: SupportUvWire) -> Result<Self, Self::Error> {
        let values = SupportUvValues::new(SupportUvPacking::try_from(wire.marker)?, wire.values)?;
        if wire.count != values.count() {
            return Err("count: does not match values length");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: wire.xmt,
            values,
            framing: wire.framing,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParasolidSupportUvRecord;

    #[test]
    fn support_tuples_preserve_wire_and_reject_invalid_count_or_packing() {
        for (marker, values) in [
            (2, "[0.0,1.0,2.0,3.0]"),
            (3, "[0.0,1.0,2.0,3.0]"),
            (4, "[0.0,1.0,2.0,3.0,4.0,5.0,6.0,7.0]"),
        ] {
            let count = if marker == 4 { 8 } else { 4 };
            let json = format!(
                r#"{{"id":"uv","stream_ordinal":0,"xmt":1,"count":{count},"marker":{marker},"values":{values},"framing":"direct","inflated_offset":10}}"#
            );
            let record: ParasolidSupportUvRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), json);
            assert_eq!(
                serde_json::to_vec(&record).unwrap(),
                serde_json::to_vec(&super::SupportUvWire::from(record.clone())).unwrap()
            );
            for (field, value) in [
                ("count", serde_json::json!(1)),
                ("marker", serde_json::json!(0)),
                ("values", serde_json::json!([1.0, 2.0, 3.0])),
            ] {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire[field] = value;
                assert!(serde_json::from_value::<ParasolidSupportUvRecord>(wire)
                    .unwrap_err()
                    .to_string()
                    .contains(field));
            }
        }
    }

    #[test]
    fn support_uv_native_limit_refuses_before_values_copy() {
        let json = r#"{"id":"nx:parasolid:support-uv#0","stream_ordinal":0,"xmt":1,"count":4,"marker":2,"values":[0.0,1.0,2.0,3.0],"framing":"direct","inflated_offset":10}"#;
        let record: ParasolidSupportUvRecord = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
