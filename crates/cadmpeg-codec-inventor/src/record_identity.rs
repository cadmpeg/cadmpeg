use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordIdentity {
    pub(crate) segment_token: String,
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
}

pub(crate) trait RecordPayload {
    const KIND: &'static str;
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Located<T> {
    pub(crate) identity: RecordIdentity,
    pub(crate) payload: T,
}

impl<T: RecordPayload> Located<T> {
    pub(crate) fn new(value: T, type_id: String, segment_token: &str, record_ordinal: u32) -> Self {
        Self {
            identity: RecordIdentity {
                type_id,
                segment_token: segment_token.into(),
                record_ordinal,
            },
            payload: value,
        }
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
            segment_token: self.identity.segment_token.clone(),
            record_ordinal: self.identity.record_ordinal,
            value: &self.payload,
        }
        .serialize(serializer)
    }
}

impl<'de, T: RecordPayload + Deserialize<'de>> Deserialize<'de> for Located<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = LocatedWire::<T>::deserialize(deserializer)?;
        let value = Self::new(
            wire.value,
            wire.type_id,
            &wire.segment_token,
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

impl<T> std::ops::DerefMut for Located<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.payload
    }
}
