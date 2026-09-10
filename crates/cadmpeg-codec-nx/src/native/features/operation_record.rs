// SPDX-License-Identifier: Apache-2.0
//! Labeled operation records with one checked record and payload span.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationRecordSpan {
    source_offset: u64,
    payload_source_offset: u64,
    payload_byte_len: u64,
}

impl OperationRecordSpan {
    pub(crate) fn new(
        source_offset: u64,
        payload_source_offset: u64,
        payload_byte_len: u64,
    ) -> Option<Self> {
        payload_source_offset.checked_sub(source_offset)?;
        payload_source_offset.checked_add(payload_byte_len)?;
        Some(Self {
            source_offset,
            payload_source_offset,
            payload_byte_len,
        })
    }

    pub(crate) fn source_offset(self) -> u64 {
        self.source_offset
    }
    pub(crate) fn byte_len(self) -> u64 {
        self.payload_source_offset - self.source_offset + self.payload_byte_len
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "OperationRecordWire", into = "OperationRecordWire")]
pub(crate) struct FeatureOperationRecord {
    pub(crate) id: String,
    pub(crate) operation_label: String,
    pub(crate) ordinal: u32,
    pub(crate) sha256: crate::native::hex::Sha256Hex,
    pub(crate) payload_sha256: crate::native::hex::Sha256Hex,
    pub(crate) stable_identity: Option<String>,
    pub(crate) span: OperationRecordSpan,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OperationRecordWire {
    id: String,
    operation_label: String,
    ordinal: u32,
    byte_len: u64,
    sha256: crate::native::hex::Sha256Hex,
    payload_byte_len: u64,
    payload_sha256: crate::native::hex::Sha256Hex,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stable_identity: Option<String>,
    payload_source_offset: u64,
    source_offset: u64,
}

impl From<FeatureOperationRecord> for OperationRecordWire {
    fn from(value: FeatureOperationRecord) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            ordinal: value.ordinal,
            byte_len: value.span.byte_len(),
            sha256: value.sha256,
            payload_byte_len: value.span.payload_byte_len,
            payload_sha256: value.payload_sha256,
            stable_identity: value.stable_identity,
            payload_source_offset: value.span.payload_source_offset,
            source_offset: value.span.source_offset,
        }
    }
}

impl TryFrom<OperationRecordWire> for FeatureOperationRecord {
    type Error = &'static str;

    fn try_from(wire: OperationRecordWire) -> Result<Self, Self::Error> {
        let span = OperationRecordSpan::new(wire.source_offset, wire.payload_source_offset, wire.payload_byte_len)
            .ok_or("source_offset/payload_source_offset/payload_byte_len: reversed or overflowing record span")?;
        if span.byte_len() != wire.byte_len {
            return Err("byte_len: length disagrees with record and payload span");
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            ordinal: wire.ordinal,
            sha256: wire.sha256,
            payload_sha256: wire.payload_sha256,
            stable_identity: wire.stable_identity,
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FeatureOperationRecord, OperationRecordSpan};

    #[test]
    fn operation_record_wire_preserves_optional_identity_and_exact_span() {
        for identity in ["", r#","stable_identity":"stable""#] {
            let wire = format!(
                r#"{{"id":"record","operation_label":"label","ordinal":0,"byte_len":70,"sha256":"record-hash","payload_byte_len":40,"payload_sha256":"payload-hash"{identity},"payload_source_offset":120,"source_offset":90}}"#
            );
            let record: FeatureOperationRecord = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), wire);
            for (field, value) in [
                ("byte_len", 80),
                ("payload_source_offset", 89),
                ("payload_byte_len", u64::MAX),
                ("source_offset", 121),
            ] {
                let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
                invalid[field] = serde_json::json!(value);
                assert!(serde_json::from_value::<FeatureOperationRecord>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains(field));
            }
        }
    }

    #[test]
    fn operation_record_span_accepts_the_last_representable_end() {
        let span = OperationRecordSpan::new(u64::MAX - 70, u64::MAX - 40, 40).unwrap();
        assert_eq!(span.byte_len(), 70);
        assert!(OperationRecordSpan::new(u64::MAX - 70, u64::MAX - 40, 41).is_none());
        assert_eq!(OperationRecordSpan::new(5, 5, 0).unwrap().byte_len(), 0);
        assert!(OperationRecordSpan::new(6, 5, 0).is_none());
    }
}
