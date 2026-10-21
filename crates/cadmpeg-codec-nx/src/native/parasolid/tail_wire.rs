// SPDX-License-Identifier: Apache-2.0
//! Derived wire lengths and hashes for complete deltas tail encodings.

use super::ParasolidDeltasTerminalNullReferences;
use crate::deltas::tails::NullTailForm;
use cadmpeg_ir::hash::{sha256, LowerHex};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(super) struct NullTailWire {
    id: String,
    stream_ordinal: u32,
    references: Vec<u32>,
    byte_len: u64,
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    inflated_offset: u64,
}

#[derive(Serialize)]
struct NullTailRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    references: &'static [u32],
    byte_len: u64,
    sha256: LowerHex<'a>,
    inflated_offset: u64,
}

impl Serialize for ParasolidDeltasTerminalNullReferences {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        NullTailRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            references: self.form.references(),
            byte_len: cadmpeg_core::decode::u64_from_index(self.form.raw().len()),
            sha256: LowerHex(&sha256(self.form.raw())),
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
            byte_len: cadmpeg_core::decode::u64_from_index(value.form.raw().len()),
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(value.form.raw()),
            inflated_offset: value.inflated_offset,
        }
    }
}
impl TryFrom<NullTailWire> for ParasolidDeltasTerminalNullReferences {
    type Error = &'static str;
    fn try_from(wire: NullTailWire) -> Result<Self, Self::Error> {
        let form = NullTailForm::from_references(&wire.references)?;
        if wire.byte_len != cadmpeg_core::decode::u64_from_index(form.raw().len()) {
            return Err("byte_len: does not match null-reference encoding");
        }
        if wire.sha256 != cadmpeg_ir::hash::digest::Sha256Digest::digest(form.raw()) {
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

#[cfg(test)]
mod tests {
    use super::super::ParasolidDeltasTerminalNullReferences;

    #[test]
    fn null_tail_wire_is_derived_from_its_complete_form() {
        for (references, bytes) in [
            ("[1,1]", &[0, 1, 0, 1][..]),
            ("[1,1,1,1]", &[0, 1, 0, 1, 0, 1, 0, 1][..]),
        ] {
            let sha256 = cadmpeg_ir::hash::digest::Sha256Digest::digest(bytes);
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
    fn null_tail_native_limit_refuses_before_hash_text_allocation() {
        let bytes = [0, 1, 0, 1];
        let digest = cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes);
        let json = format!(
            r#"{{"id":"nx:parasolid:null-tail#0","stream_ordinal":0,"references":[1,1],"byte_len":4,"sha256":"{digest}","inflated_offset":10}}"#
        );
        let tail: ParasolidDeltasTerminalNullReferences = serde_json::from_str(&json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &tail,
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        );
    }
}
