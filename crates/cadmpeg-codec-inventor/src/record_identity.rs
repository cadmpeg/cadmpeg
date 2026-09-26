// SPDX-License-Identifier: Apache-2.0
//! Record identities and located payloads with flat wire fields.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::IdentityKey;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::pmdc::type_id_string;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordIdentity {
    pub(crate) segment_token: IdentityKey,
    pub(crate) record_ordinal: u32,
    pub(crate) type_id: String,
}

impl RecordIdentity {
    fn id(&self, kind: &str) -> String {
        format!(
            "inventor:pmdc:{kind}#{}-{}",
            self.segment_token, self.record_ordinal
        )
    }

    /// The identity key of this record: `{segment_token}-{record_ordinal}`.
    pub(crate) fn key(&self) -> IdentityKey {
        self.segment_token.clone().dash(self.record_ordinal)
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
        type_id: String,
        segment_token: &IdentityKey,
        record_ordinal: u32,
    ) -> Self {
        Self {
            identity: RecordIdentity {
                type_id,
                segment_token: segment_token.clone(),
                record_ordinal,
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
    ctx.charge_collection_items(1, operation)?;
    ctx.charge_retained(32, "retain Inventor PmDc record type id")?;
    ctx.charge_retained(
        segment_token.as_str().len() as u64,
        "retain Inventor PmDc record segment token",
    )?;
    records.push(Located::new(
        value,
        type_id_string(type_id),
        segment_token,
        ordinal,
    ));
    Ok(())
}

impl<T: RecordPayload> Located<T> {
    pub(crate) fn id_len(&self) -> usize {
        "inventor:pmdc:".len()
            + T::KIND.len()
            + 1
            + self.identity.segment_token.as_str().len()
            + 1
            + self.identity.record_ordinal.max(1).ilog10() as usize
            + 1
    }

    pub(crate) fn id(&self) -> String {
        self.identity.id(T::KIND)
    }
}

impl<T> std::ops::Deref for Located<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.payload
    }
}

#[derive(Serialize, Deserialize)]
struct LocatedWire<T> {
    id: String,
    type_id: String,
    segment_token: String,
    record_ordinal: u32,
    #[serde(flatten)]
    value: T,
}

impl<T: RecordPayload + Serialize> Serialize for Located<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LocatedWire {
            id: self.id(),
            type_id: self.identity.type_id.clone(),
            segment_token: self.identity.segment_token.as_str().to_owned(),
            record_ordinal: self.identity.record_ordinal,
            value: &self.payload,
        }
        .serialize(serializer)
    }
}

impl<'de, T: RecordPayload + Deserialize<'de>> Deserialize<'de> for Located<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = LocatedWire::<T>::deserialize(deserializer)?;
        let segment_token = IdentityKey::try_new(wire.segment_token)
            .map_err(|error| serde::de::Error::custom(format!("segment_token: {error}")))?;
        let value = Self::new(
            wire.value,
            wire.type_id,
            &segment_token,
            wire.record_ordinal,
        );
        if wire.id != value.id() {
            return Err(serde::de::Error::custom(
                "id disagrees with segment_token or record_ordinal",
            ));
        }
        Ok(value)
    }
}
