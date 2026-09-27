// SPDX-License-Identifier: Apache-2.0
//! Derived wire lengths and hashes for complete deltas tail encodings.

use super::{ParasolidDeltasTermUseNumericTail, ParasolidDeltasTerminalNullReferences};
use crate::deltas::tails::{NullTailForm, NumericTailValues};
use crate::native::hex::Sha256WireDigest;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(super) struct NullTailWire {
    id: String,
    stream_ordinal: u32,
    references: Vec<u32>,
    byte_len: u64,
    sha256: crate::native::hex::Sha256Hex,
    inflated_offset: u64,
}

#[derive(Serialize)]
struct NullTailRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    references: &'static [u32],
    byte_len: u64,
    sha256: Sha256WireDigest,
    inflated_offset: u64,
}

impl Serialize for ParasolidDeltasTerminalNullReferences {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        NullTailRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            references: self.form.references(),
            byte_len: self.form.raw().len() as u64,
            sha256: Sha256WireDigest::of(self.form.raw()),
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<ParasolidDeltasTerminalNullReferences> for NullTailWire {
    fn from(value: ParasolidDeltasTerminalNullReferences) -> Self {
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            references: value.form.references().to_vec(),
            byte_len: value.form.raw().len() as u64,
            sha256: crate::native::hex::Sha256Hex::digest(value.form.raw()),
            inflated_offset: value.inflated_offset,
        }
    }
}
impl TryFrom<NullTailWire> for ParasolidDeltasTerminalNullReferences {
    type Error = &'static str;
    fn try_from(wire: NullTailWire) -> Result<Self, Self::Error> {
        let form = NullTailForm::from_references(&wire.references)?;
        if wire.byte_len != form.raw().len() as u64 {
            return Err("byte_len: does not match null-reference encoding");
        }
        if wire.sha256 != crate::native::hex::Sha256Hex::digest(form.raw()) {
            return Err("sha256: does not match null-reference bytes");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            form,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(super) struct NumericTailWire {
    id: String,
    stream_ordinal: u32,
    term_use_xmt: u32,
    term_use_count: u32,
    values: Vec<f64>,
    byte_len: u64,
    sha256: crate::native::hex::Sha256Hex,
    inflated_offset: u64,
}

#[derive(Serialize)]
struct NumericTailRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    term_use_xmt: u32,
    term_use_count: u32,
    values: &'a [cadmpeg_ir::scalar::FiniteReal],
    byte_len: u64,
    sha256: Sha256WireDigest,
    inflated_offset: u64,
}

impl Serialize for ParasolidDeltasTermUseNumericTail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (bytes, len) = self.values.encoded_bytes();
        NumericTailRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            term_use_xmt: self.term_use_xmt,
            term_use_count: self.values.term_use_count(),
            values: self.values.values(),
            byte_len: self.values.byte_len() as u64,
            sha256: Sha256WireDigest::of(&bytes[..len]),
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<ParasolidDeltasTermUseNumericTail> for NumericTailWire {
    fn from(value: ParasolidDeltasTermUseNumericTail) -> Self {
        let byte_len = value.values.byte_len() as u64;
        let sha256 = crate::native::hex::Sha256Hex::digest(&value.values.bytes());
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            term_use_xmt: value.term_use_xmt,
            term_use_count: value.values.term_use_count(),
            values: value.values.into_values(),
            byte_len,
            sha256,
            inflated_offset: value.inflated_offset,
        }
    }
}
impl TryFrom<NumericTailWire> for ParasolidDeltasTermUseNumericTail {
    type Error = &'static str;
    fn try_from(wire: NumericTailWire) -> Result<Self, Self::Error> {
        let values = NumericTailValues::new(wire.term_use_count, wire.values)?;
        if wire.byte_len != values.byte_len() as u64 {
            return Err("byte_len: does not match numeric-tail encoding");
        }
        let (bytes, len) = values.encoded_bytes();
        if wire.sha256 != crate::native::hex::Sha256Hex::digest(&bytes[..len]) {
            return Err("sha256: does not match numeric-tail bytes");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            term_use_xmt: wire.term_use_xmt,
            values,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParasolidDeltasTermUseNumericTail;
    use super::super::ParasolidDeltasTerminalNullReferences;

    #[test]
    fn null_tail_wire_is_derived_from_its_complete_form() {
        for (references, bytes) in [
            ("[1,1]", &[0, 1, 0, 1][..]),
            ("[1,1,1,1]", &[0, 1, 0, 1, 0, 1, 0, 1][..]),
        ] {
            let sha256 = crate::native::hex::Sha256Hex::digest(bytes);
            let byte_len = bytes.len();
            let json = format!(
                r#"{{"id":"tail","stream_ordinal":0,"references":{references},"byte_len":{byte_len},"sha256":"{sha256}","inflated_offset":10}}"#
            );
            let tail: ParasolidDeltasTerminalNullReferences = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&tail).unwrap(), json);
            assert_eq!(
                serde_json::to_vec(&tail).unwrap(),
                serde_json::to_vec(&super::NullTailWire::from(tail.clone())).unwrap()
            );
            for (field, invalid) in [
                ("references", serde_json::json!([1, 2])),
                ("byte_len", serde_json::json!(1)),
                ("sha256", serde_json::json!("wrong")),
            ] {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire[field] = invalid;
                let error = serde_json::from_value::<ParasolidDeltasTerminalNullReferences>(wire)
                    .unwrap_err();
                assert!(error.to_string().contains(field), "{error}");
            }
        }
    }

    #[test]
    fn numeric_tail_wire_preserves_signed_zero_and_rejects_derived_copies() {
        let bytes = [-0.0_f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
            .into_iter()
            .flat_map(f64::to_be_bytes)
            .collect::<Vec<_>>();
        let sha256 = crate::native::hex::Sha256Hex::digest(&bytes);
        let json = format!(
            r#"{{"id":"tail","stream_ordinal":0,"term_use_xmt":20,"term_use_count":1,"values":[-0.0,1.0,2.0,3.0,4.0,5.0,6.0,7.0],"byte_len":64,"sha256":"{sha256}","inflated_offset":10}}"#
        );
        let tail: ParasolidDeltasTermUseNumericTail = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&tail).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&tail).unwrap(),
            serde_json::to_vec(&super::NumericTailWire::from(tail.clone())).unwrap()
        );
        for (field, invalid) in [
            ("term_use_count", serde_json::json!(2)),
            ("byte_len", serde_json::json!(1)),
            ("sha256", serde_json::json!("wrong")),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
            wire[field] = invalid;
            let error =
                serde_json::from_value::<ParasolidDeltasTermUseNumericTail>(wire).unwrap_err();
            assert!(error.to_string().contains(field), "{error}");
        }
    }

    #[test]
    fn null_tail_native_limit_refuses_before_hash_text_allocation() {
        let bytes = [0, 1, 0, 1];
        let digest = crate::native::hex::Sha256Hex::digest(&bytes);
        let json = format!(
            r#"{{"id":"nx:parasolid:null-tail#0","stream_ordinal":0,"references":[1,1],"byte_len":4,"sha256":"{digest}","inflated_offset":10}}"#
        );
        let tail: ParasolidDeltasTerminalNullReferences = serde_json::from_str(&json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &tail,
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        );
    }

    #[test]
    fn numeric_tail_native_limit_refuses_before_values_copy() {
        let bytes = [-0.0_f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
            .into_iter()
            .flat_map(f64::to_be_bytes)
            .collect::<Vec<_>>();
        let digest = crate::native::hex::Sha256Hex::digest(&bytes);
        let json = format!(
            r#"{{"id":"nx:parasolid:numeric-tail#0","stream_ordinal":0,"term_use_xmt":20,"term_use_count":1,"values":[-0.0,1.0,2.0,3.0,4.0,5.0,6.0,7.0],"byte_len":64,"sha256":"{digest}","inflated_offset":10}}"#
        );
        let tail: ParasolidDeltasTermUseNumericTail = serde_json::from_str(&json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &tail,
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        );
    }
}
