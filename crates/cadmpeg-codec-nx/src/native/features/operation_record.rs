// SPDX-License-Identifier: Apache-2.0
//! Labeled operation records with one checked record and payload span.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::native) struct OperationRecordSpan {
    source_offset: u64,
    payload_source_offset: u64,
    payload_byte_len: u64,
}

impl OperationRecordSpan {
    pub(in crate::native) fn new(
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

    pub(in crate::native) fn source_offset(self) -> u64 {
        self.source_offset
    }
    pub(super) fn byte_len(self) -> u64 {
        self.payload_source_offset - self.source_offset + self.payload_byte_len
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "OperationRecordWire")]
pub(in crate::native) struct FeatureOperationRecord {
    pub(in crate::native) id: String,
    pub(in crate::native) operation_label: String,
    pub(in crate::native) ordinal: u32,
    pub(in crate::native) sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    pub(in crate::native) payload_sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    pub(in crate::native) stable_identity: Option<String>,
    pub(in crate::native) span: OperationRecordSpan,
    pub(in crate::native) core: Option<crate::om::operation_core::OperationCore>,
    pub(in crate::native) linked_face_payload: Option<crate::om::linked_face::LinkedFacePayload>,
}

#[derive(serde::Serialize)]
struct OperationRecordRef<'a> {
    id: &'a str,
    operation_label: &'a str,
    ordinal: u32,
    byte_len: u64,
    sha256: &'a cadmpeg_ir::hash::digest::Sha256Digest,
    payload_byte_len: u64,
    payload_sha256: &'a cadmpeg_ir::hash::digest::Sha256Digest,
    #[serde(skip_serializing_if = "Option::is_none")]
    stable_identity: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    core: Option<&'a crate::om::operation_core::OperationCore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linked_face_payload: Option<&'a crate::om::linked_face::LinkedFacePayload>,
    payload_source_offset: u64,
    source_offset: u64,
}

impl serde::Serialize for FeatureOperationRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        OperationRecordRef {
            id: &self.id,
            operation_label: &self.operation_label,
            ordinal: self.ordinal,
            byte_len: self.span.byte_len(),
            sha256: &self.sha256,
            payload_byte_len: self.span.payload_byte_len,
            payload_sha256: &self.payload_sha256,
            stable_identity: self.stable_identity.as_deref(),
            core: self.core.as_ref(),
            linked_face_payload: self.linked_face_payload.as_ref(),
            payload_source_offset: self.span.payload_source_offset,
            source_offset: self.span.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(serde::Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct OperationRecordWire {
    id: String,
    operation_label: String,
    ordinal: u32,
    byte_len: u64,
    sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    payload_byte_len: u64,
    payload_sha256: cadmpeg_ir::hash::digest::Sha256Digest,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_stable_identity"
    )]
    stable_identity: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_core"
    )]
    core: Option<crate::om::operation_core::OperationCore>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_linked_face_payload"
    )]
    linked_face_payload: Option<crate::om::linked_face::LinkedFacePayload>,
    payload_source_offset: u64,
    source_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static OPERATION_RECORD_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<FeatureOperationRecord> for OperationRecordWire {
    fn from(value: FeatureOperationRecord) -> Self {
        OPERATION_RECORD_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: value.id,
            operation_label: value.operation_label,
            ordinal: value.ordinal,
            byte_len: value.span.byte_len(),
            sha256: value.sha256,
            payload_byte_len: value.span.payload_byte_len,
            payload_sha256: value.payload_sha256,
            stable_identity: value.stable_identity,
            core: value.core,
            linked_face_payload: value.linked_face_payload,
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
        if wire.core.is_some() && wire.linked_face_payload.is_some() {
            return Err("linked_face_payload/core: two representations cannot own one payload");
        }
        for bytes in [
            wire.core
                .as_ref()
                .map(crate::om::operation_core::OperationCore::as_bytes),
            wire.linked_face_payload
                .as_ref()
                .map(crate::om::linked_face::LinkedFacePayload::as_bytes),
        ]
        .into_iter()
        .flatten()
        {
            if cadmpeg_core::decode::u64_from_index(bytes.len()) != wire.payload_byte_len {
                return Err("core/linked_face_payload: complete payload lengths disagree");
            }
            if cadmpeg_ir::hash::digest::Sha256Digest::digest(bytes) != wire.payload_sha256 {
                return Err("core/linked_face_payload: bytes disagree with payload_sha256");
            }
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            ordinal: wire.ordinal,
            sha256: wire.sha256,
            payload_sha256: wire.payload_sha256,
            stable_identity: wire.stable_identity,
            core: wire.core,
            linked_face_payload: wire.linked_face_payload,
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FeatureOperationRecord, OperationRecordSpan, OperationRecordWire,
        OPERATION_RECORD_INTO_WIRE_COUNT,
    };

    #[test]
    fn operation_record_wire_preserves_optional_identity_and_exact_span() {
        for identity in ["", r#","stable_identity":"stable""#] {
            let wire = format!(
                r#"{{"id":"record","operation_label":"label","ordinal":0,"byte_len":70,"sha256":"e3435e1ec46c3583cddf3562de1ac4b15f5cf950be3f42d3dd273d6f5b756b95","payload_byte_len":40,"payload_sha256":"47ac2ba87d3f6c174479809b0a1ea8f32a654ec0044301278e6c822375d33e75"{identity},"payload_source_offset":120,"source_offset":90}}"#
            );
            let record: FeatureOperationRecord = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), wire);
            assert_eq!(
                serde_json::to_vec(&record).unwrap(),
                serde_json::to_vec(&OperationRecordWire::from(record.clone())).unwrap()
            );
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
    fn operation_record_native_limit_refuses_before_owned_wire_conversion() {
        let wire = serde_json::json!({
            "id": "nx:feature-history:operation-record#0", "operation_label": "label", "ordinal": 0,
            "byte_len": 70,
            "sha256": "e3435e1ec46c3583cddf3562de1ac4b15f5cf950be3f42d3dd273d6f5b756b95",
            "payload_byte_len": 40,
            "payload_sha256": "47ac2ba87d3f6c174479809b0a1ea8f32a654ec0044301278e6c822375d33e75",
            "payload_source_offset": 120, "source_offset": 90
        });
        let record: FeatureOperationRecord = serde_json::from_value(wire.clone()).unwrap();
        OPERATION_RECORD_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(&record, wire);
        OPERATION_RECORD_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn operation_record_core_requires_the_payload_digest_and_length() {
        let bytes = crate::test_support::move_face::core(2);
        let length = bytes.len();
        let digest = cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes);
        let wire = serde_json::json!({
            "id": "nx:feature-history:operation-record#0", "operation_label": "label", "ordinal": 0,
            "byte_len": length + 30,
            "sha256": "e3435e1ec46c3583cddf3562de1ac4b15f5cf950be3f42d3dd273d6f5b756b95",
            "payload_byte_len": length, "payload_sha256": digest,
            "core": {"kind": "move_face", "bytes": cadmpeg_ir::native::bytes::NativeBytes::from(bytes)},
            "payload_source_offset": 120, "source_offset": 90
        });
        let record: FeatureOperationRecord = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&record).unwrap(), wire);
        cadmpeg_test_support::native_serialization::assert_native_limit(&record, wire.clone());
        let mut changed = wire.clone();
        changed["payload_sha256"] =
            serde_json::json!(cadmpeg_ir::hash::digest::Sha256Digest::digest(b"unrelated"));
        assert!(serde_json::from_value::<FeatureOperationRecord>(changed)
            .unwrap_err()
            .to_string()
            .contains("payload_sha256"));
        let mut changed = wire;
        changed["payload_byte_len"] = serde_json::json!(length + 1);
        changed["byte_len"] = serde_json::json!(length + 31);
        assert!(serde_json::from_value::<FeatureOperationRecord>(changed)
            .unwrap_err()
            .to_string()
            .contains("lengths disagree"));
    }

    #[test]
    fn operation_record_core_is_admitted_by_native_check() {
        use cadmpeg_ir::codec::CodecBackend;

        let bytes = crate::test_support::move_face::core(2);
        let length = bytes.len();
        let wire = serde_json::json!({
            "id": "nx:feature-history:operation-record#0", "operation_label": "label", "ordinal": 0,
            "byte_len": length + 30,
            "sha256": "e3435e1ec46c3583cddf3562de1ac4b15f5cf950be3f42d3dd273d6f5b756b95",
            "payload_byte_len": length,
            "payload_sha256": cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes),
            "core": {"kind": "move_face", "bytes": cadmpeg_ir::native::bytes::NativeBytes::from(bytes)},
            "payload_source_offset": 120, "source_offset": 90
        });
        crate::test_support::with_decode_context(|ctx| {
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.native
                .namespace_mut("nx")
                .set_arena(
                    ctx,
                    "feature_operation_records",
                    std::slice::from_ref(&wire),
                )
                .unwrap();
            assert!(crate::NxCodec::validate_native(ctx, &ir)
                .unwrap()
                .is_empty());
            for (field, value) in [
                (
                    "core",
                    serde_json::json!({"kind": "move_face", "bytes": "00"}),
                ),
                (
                    "payload_sha256",
                    serde_json::json!(cadmpeg_ir::hash::digest::Sha256Digest::digest(b"unrelated")),
                ),
                ("byte_len", serde_json::json!(length + 31)),
            ] {
                let mut bad = wire.clone();
                bad[field] = value;
                ir.native
                    .namespace_mut("nx")
                    .set_arena(ctx, "feature_operation_records", &[bad])
                    .unwrap();
                let findings = crate::NxCodec::validate_native(ctx, &ir).unwrap();
                assert_eq!(findings.len(), 1, "{field}");
                assert!(findings[0].message.contains(field), "{field}: {findings:?}");
            }
        });
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

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_stable_identity, String, "stable_identity");

cadmpeg_core::named_optional_field!(
    deserialize_core,
    crate::om::operation_core::OperationCore,
    "core"
);

cadmpeg_core::named_optional_field!(
    deserialize_linked_face_payload,
    crate::om::linked_face::LinkedFacePayload,
    "linked_face_payload"
);
