// SPDX-License-Identifier: Apache-2.0
//! Record identities and located payloads with flat wire fields.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::IdentityKey;
use serde::{Deserialize, Serialize, Serializer};

use crate::pmdc::type_id_string;

pub(crate) fn try_identity_key(
    ctx: &DecodeContext<'_>,
    value: String,
    operation: &'static str,
    field: Option<&str>,
) -> Result<IdentityKey, CodecError> {
    let work = cadmpeg_core::decode::u64_from_index(value.len())
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_work(work, operation)?;
    match IdentityKey::try_new(value) {
        Ok(key) => Ok(key),
        Err(error) => {
            let message = match field {
                Some(field) => ctx.format_retained(
                    format_args!("{field}: {error}"),
                    "format invalid Inventor identity key",
                )?,
                None => ctx.format_retained(
                    format_args!("{error}"),
                    "format invalid Inventor identity key",
                )?,
            };
            Err(CodecError::Malformed(message))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct RecordTypeId(String);

impl RecordTypeId {
    pub(crate) fn from_bytes(
        ctx: &DecodeContext<'_>,
        value: [u8; 16],
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self(type_id_string(ctx, value, operation)?))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RecordTypeId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value))
        } else {
            Err("type_id must contain 32 lowercase hexadecimal digits")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordIdentity {
    pub(crate) segment_token: IdentityKey,
    pub(crate) record_ordinal: u32,
    pub(crate) type_id: RecordTypeId,
}

impl RecordIdentity {
    fn id(&self, ctx: &DecodeContext<'_>, kind: &str) -> Result<String, CodecError> {
        ctx.format_retained(
            format_args!(
                "inventor:pmdc:{kind}#{}-{}",
                self.segment_token, self.record_ordinal
            ),
            "retain Inventor PmDc record identity",
        )
    }

    /// The identity key of this record: `{segment_token}-{record_ordinal}`.
    pub(crate) fn key(&self, ctx: &DecodeContext<'_>) -> Result<IdentityKey, CodecError> {
        let key = ctx.format_retained(
            format_args!("{}-{}", self.segment_token, self.record_ordinal),
            "Inventor record identity key",
        )?;
        try_identity_key(
            ctx,
            key,
            "validate Inventor record identity key",
            None,
        )
    }
}

pub(crate) trait RecordPayload {
    const KIND: &'static str;
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Located<T> {
    pub(crate) identity: RecordIdentity,
    payload: T,
}

impl<T> Located<T> {
    pub(crate) fn new(
        value: T,
        type_id: RecordTypeId,
        segment_token: IdentityKey,
        record_ordinal: u32,
    ) -> Self {
        Self {
            identity: RecordIdentity {
                segment_token,
                record_ordinal,
                type_id,
            },
            payload: value,
        }
    }
}

pub(crate) fn push_record<T>(
    ctx: &DecodeContext<'_>,
    records: &mut Vec<Located<T>>,
    value: T,
    type_id: [u8; 16],
    segment_token: &IdentityKey,
    ordinal: u32,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_entities(1, operation)?;
    let type_id = crate::record_identity::RecordTypeId::from_bytes(
        ctx,
        type_id,
        "retain Inventor PmDc record type id",
    )?;
    let segment_token =
        segment_token.try_clone_for_decode(ctx, "retain Inventor PmDc record segment token")?;
    ctx.push_vec(
        records,
        Located::new(value, type_id, segment_token, ordinal),
        operation,
    )?;
    Ok(())
}

impl<T: RecordPayload> Located<T> {
    pub(crate) fn id_len(&self) -> Option<usize> {
        Some(
            "inventor:pmdc:".len()
                + T::KIND.len()
                + 1
                + self.identity.segment_token.as_str().len()
                + 1
                + usize::try_from(self.identity.record_ordinal.max(1).ilog10()).ok()?
                + 1,
        )
    }

    pub(crate) fn id(&self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        self.identity.id(ctx, T::KIND)
    }
}

impl<T> std::ops::Deref for Located<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.payload
    }
}

#[derive(Deserialize)]
pub(crate) struct LocatedWire<T> {
    pub(crate) id: String,
    pub(crate) type_id: RecordTypeId,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    #[serde(flatten)]
    pub(crate) value: T,
}

struct LocatedId<'a, T: RecordPayload> {
    identity: &'a RecordIdentity,
    payload: std::marker::PhantomData<T>,
}

impl<T: RecordPayload> std::fmt::Display for LocatedId<'_, T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "inventor:pmdc:{}#{}-{}",
            T::KIND,
            self.identity.segment_token,
            self.identity.record_ordinal
        )
    }
}

impl<T: RecordPayload> Serialize for LocatedId<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Serialize)]
#[serde(bound(serialize = "T: Serialize"))]
struct LocatedWireView<'a, T: RecordPayload> {
    id: LocatedId<'a, T>,
    type_id: &'a str,
    segment_token: &'a str,
    record_ordinal: u32,
    #[serde(flatten)]
    value: &'a T,
}

impl<T: RecordPayload + Serialize> Serialize for Located<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LocatedWireView {
            id: LocatedId {
                identity: &self.identity,
                payload: std::marker::PhantomData,
            },
            type_id: self.identity.type_id.as_str(),
            segment_token: self.identity.segment_token.as_str(),
            record_ordinal: self.identity.record_ordinal,
            value: &self.payload,
        }
        .serialize(serializer)
    }
}

impl<T: RecordPayload> LocatedWire<T> {
    pub(crate) fn into_record(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Located<T>, CodecError> {
        let segment_token_text = self.segment_token;
        let segment_token = try_identity_key(
            ctx,
            segment_token_text,
            "validate Inventor PmDc segment token",
            Some("segment_token"),
        )?;
        let record = Located::new(
            self.value,
            self.type_id,
            segment_token,
            self.record_ordinal,
        );
        let (expected_id, _expected_id_storage) =
            ctx.with_scoped_storage("validate Inventor PmDc record identity", || {
                record.id(ctx)
            })?;
        if !ctx.equal(&self.id, &expected_id, "validate Inventor PmDc record identity")? {
            return Err(CodecError::Malformed(
                "id disagrees with segment_token or record_ordinal".into(),
            ));
        }
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn located_records_reject_invalid_type_guids() {
        #[derive(serde::Deserialize)]
        struct Payload {}
        impl super::RecordPayload for Payload {
            const KIND: &'static str = "test";
        }
        for type_id in [
            "not-a-guid",
            "",
            "0000000000000000000000000000000",
            "ABCDEF0123456789abcdef0123456789ab",
        ] {
            assert!(super::RecordTypeId::try_from(type_id.to_owned()).is_err());
            let wire = serde_json::json!({"id": "inventor:pmdc:test#segment-0", "type_id": type_id, "segment_token": "segment", "record_ordinal": 0});
            assert!(serde_json::from_value::<super::LocatedWire<Payload>>(wire).is_err());
        }
    }

    #[test]
    fn located_wire_converts_with_the_decode_context() {
        #[derive(serde::Deserialize)]
        struct Payload {}
        impl super::RecordPayload for Payload {
            const KIND: &'static str = "test";
        }

        let ctx = cadmpeg_test_support::service_decode_context();
        let wire = serde_json::from_value::<super::LocatedWire<Payload>>(serde_json::json!({
            "id": "inventor:pmdc:test#segment-0",
            "type_id": "00000000000000000000000000000000",
            "segment_token": "segment",
            "record_ordinal": 0
        }))
        .expect("valid located wire");
        let record = wire.into_record(&ctx).expect("valid located identity");
        assert_eq!(
            record.id(&ctx).expect("retained located id"),
            "inventor:pmdc:test#segment-0"
        );
    }

    #[test]
    fn located_record_id_streams_once_with_retained_limit() {
        #[derive(serde::Serialize)]
        struct Payload {
            value: u32,
        }

        impl super::RecordPayload for Payload {
            const KIND: &'static str = "test";
        }

        let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
        let ctx = cadmpeg_test_support::service_decode_context();
        let record = super::Located::new(
            Payload { value: 7 },
            crate::record_identity::RecordTypeId::from_bytes(
                &ctx,
                [0; 16],
                "retain Inventor PmDc record type id",
            )
            .expect("service fixture type id"),
            token
                .try_clone_for_decode(&ctx, "Inventor located fixture token")
                .expect("service fixture token"),
            1,
        );
        assert_native_limit(
            &record,
            serde_json::json!({
                "id": "inventor:pmdc:test#segment-1", "type_id": "00000000000000000000000000000000",
                "segment_token": "segment", "record_ordinal": 1,
                "value": 7
            }),
        );
    }

    #[test]
    fn parsed_record_refuses_entity_limit_before_collection_push() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
        let token = cadmpeg_ir::ids::IdentityKey::encode_segment("segment");
        let mut records = Vec::new();
        assert!(matches!(
            super::push_record(
                &limited,
                &mut records,
                7_u32,
                [0; 16],
                &token,
                1,
                "admit parsed record",
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit parsed record"
        ));
        assert!(records.is_empty());

        let (service, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service context");
        super::push_record(
            &service,
            &mut records,
            7_u32,
            [0; 16],
            &token,
            1,
            "admit parsed record",
        )
        .expect("service admission");
        assert_eq!(records.len(), 1);
    }
}
