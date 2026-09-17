// SPDX-License-Identifier: Apache-2.0
//! Retained source records without a typed IR interpretation.
#![deny(clippy::disallowed_methods)]

use crate::ids::{Identity, UnknownId};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A format-specific product record.
///
/// The `Deserialize` derive has a live reader: `CadIr::native_unknowns_iter`
/// and `CadIr::all_native_unknowns_iter` read this type out of the reserved
/// `unknowns` arena through `arena_iter_as`, whose bound is
/// `T: DeserializeOwned`.
///
/// The product projection contains only identity and links. Extra source
/// fields require an explicit raw [`UnknownRecord`] read; this reader must not
/// silently discard them.
///
/// Product links carry admitted identities, including identities whose targets
/// will be added later during document assembly.
///
/// ```compile_fail
/// let mut record = cadmpeg_ir::NativeUnknownRecord {
///     id: "test:source:unknown#0".try_into().unwrap(),
///     links: Vec::new(),
/// };
/// record.links.push(String::from("malformed"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NativeUnknownRecord {
    /// Arena id.
    pub id: UnknownId,
    /// Related entity IDs from any document arena.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Identity>,
}

/// Raw source retention facts. Digest text and extents are producer evidence;
/// authoritative sidecar admission checks them before recovery or replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "retention", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RawRetainedBytes {
    Inline {
        #[serde(with = "crate::bytes")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        data: Vec<u8>,
    },
    Digest {
        byte_len: u64,
        sha256: String,
    },
}

impl TryFrom<&UnknownRecord> for NativeUnknownRecord {
    type Error = crate::native::NativeConvertError;

    fn try_from(record: &UnknownRecord) -> Result<Self, Self::Error> {
        let links = record
            .links()
            .iter()
            .enumerate()
            .map(|(index, link)| {
                Identity::new(link.clone()).map_err(|error| {
                    Self::Error::InvalidCollection(format!(
                        "native unknown {} link {index}: {error}",
                        record.id()
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            id: record.id().clone(),
            links,
        })
    }
}

impl From<&NativeUnknownRecord> for crate::native::NativeRecord {
    fn from(record: &NativeUnknownRecord) -> Self {
        let mut fields = serde_json::Map::new();
        if !record.links.is_empty() {
            fields.insert(
                "links".into(),
                serde_json::Value::Array(
                    record
                        .links
                        .iter()
                        .map(|link| serde_json::Value::String(link.as_str().to_owned()))
                        .collect(),
                ),
            );
        }
        Self::from_identity(record.id.clone(), fields)
    }
}

/// A recognized source record represented by location, retained image, and
/// links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UnknownRecord {
    /// Arena id.
    id: UnknownId,
    /// Byte offset of the record within its source stream.
    offset: u64,
    /// Retained image of the record bytes: the bytes themselves, or the
    /// length and digest of bytes that are not retained. One or the other,
    /// never both, so no record can state an extent or a digest that
    /// contradicts the bytes beside it.
    retention: RawRetainedBytes,
    /// Related entity IDs from any document arena.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    links: Vec<String>,
}

impl UnknownRecord {
    /// Retains source bytes and derives their length and SHA-256 digest.
    #[must_use]
    pub fn retained(id: UnknownId, offset: u64, data: Vec<u8>, links: Vec<String>) -> Self {
        Self {
            id,
            offset,
            retention: RawRetainedBytes::Inline { data },
            links,
        }
    }

    /// Records unavailable source bytes by their producer-supplied length and digest.
    #[must_use]
    pub fn unavailable(
        id: UnknownId,
        offset: u64,
        byte_len: u64,
        sha256: impl Into<String>,
        links: Vec<String>,
    ) -> Self {
        Self {
            id,
            offset,
            retention: RawRetainedBytes::Digest {
                byte_len,
                sha256: sha256.into(),
            },
            links,
        }
    }

    pub(crate) fn into_parts(self) -> (UnknownId, u64, RawRetainedBytes, Vec<String>) {
        (self.id, self.offset, self.retention, self.links)
    }

    /// Returns the arena id.
    #[must_use]
    pub fn id(&self) -> &UnknownId {
        &self.id
    }

    /// Replaces the arena id during namespace composition.
    pub fn set_id(&mut self, id: UnknownId) {
        self.id = id;
    }

    /// Returns the byte offset within the source stream.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// Returns the byte length of the record span.
    #[must_use]
    pub fn byte_len(&self) -> u64 {
        match &self.retention {
            RawRetainedBytes::Inline { data } => data.len() as u64,
            RawRetainedBytes::Digest { byte_len, .. } => *byte_len,
        }
    }

    /// Return the measured inline digest or the producer-supplied digest text.
    #[must_use]
    pub fn sha256(&self) -> String {
        match &self.retention {
            RawRetainedBytes::Inline { data } => crate::hash::sha256_hex(data),
            RawRetainedBytes::Digest { sha256, .. } => sha256.clone(),
        }
    }

    /// Returns the retained bytes when available.
    #[must_use]
    pub fn data(&self) -> Option<&[u8]> {
        match &self.retention {
            RawRetainedBytes::Inline { data } => Some(data),
            RawRetainedBytes::Digest { .. } => None,
        }
    }

    /// Retains source bytes. Their length and digest become functions of them.
    pub fn retain_data(&mut self, data: Vec<u8>) {
        self.retention = RawRetainedBytes::Inline { data };
    }

    /// Returns the related entity IDs.
    #[must_use]
    pub fn links(&self) -> &[String] {
        &self.links
    }

    /// Returns the related entity IDs for reference resolution.
    #[must_use]
    pub fn links_mut(&mut self) -> &mut Vec<String> {
        &mut self.links
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_projection_checks_raw_link_identity_without_changing_evidence() {
        for link in [
            "",
            "missing",
            "test:body#0",
            "test:model:body#",
            "test:model:body#with space",
        ] {
            let raw = UnknownRecord::retained(
                UnknownId::mint("test:source:unknown#0").unwrap(),
                0,
                vec![1],
                vec![link.to_owned()],
            );
            let error = NativeUnknownRecord::try_from(&raw).unwrap_err();
            assert!(error.to_string().contains(raw.id().as_str()), "{error}");
            assert!(error.to_string().contains("link 0"), "{error}");
            let wire = serde_json::to_value(&raw).unwrap();
            assert_eq!(serde_json::from_value::<UnknownRecord>(wire).unwrap(), raw);
        }
        let raw = UnknownRecord::retained(
            UnknownId::mint("test:source:unknown#0").unwrap(),
            0,
            vec![1],
            vec!["test:model:body#unresolved".into()],
        );
        let product = NativeUnknownRecord::try_from(&raw).unwrap();
        assert_eq!(product.links[0].as_str(), "test:model:body#unresolved");
    }

    #[test]
    fn raw_source_facts_survive_document_admission_without_becoming_a_product_projection() {
        let record = UnknownRecord::unavailable(
            UnknownId::mint("synthetic:source:unknown#0").unwrap(),
            u64::MAX,
            1,
            "wire-value",
            vec![],
        );
        let wire = serde_json::to_value(&record).unwrap();
        assert_eq!(
            serde_json::from_value::<UnknownRecord>(wire).unwrap(),
            record
        );
        let mut ir = crate::CadIr::empty();
        ir.native
            .namespace_mut("synthetic")
            .set_arena("unknowns", std::slice::from_ref(&record))
            .unwrap();
        let parsed = crate::CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
        assert_eq!(parsed, ir);
        let raw = parsed
            .native
            .namespace("synthetic")
            .unwrap()
            .arena_as::<UnknownRecord>("unknowns")
            .unwrap();
        assert_eq!(raw, std::slice::from_ref(&record));
        let error = parsed.native_unknowns("synthetic").unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{error}");

        let mut inline = record;
        inline.retain_data(vec![1]);
        assert_eq!(inline.offset(), u64::MAX);
        assert_eq!(inline.byte_len(), 1);
        assert_eq!(inline.sha256(), crate::hash::sha256_hex(&[1]));
        let wire = serde_json::to_value(&inline).unwrap();
        assert_eq!(
            serde_json::from_value::<UnknownRecord>(wire).unwrap(),
            inline
        );
    }

    #[test]
    fn product_unknown_projection_refuses_extra_fields_and_accepts_absent_links() {
        for links in [None, Some(serde_json::json!([]))] {
            let mut wire = serde_json::json!({"id": "synthetic:source:unknown#0"});
            if let Some(links) = links {
                wire["links"] = links;
            }
            let record = serde_json::from_value::<NativeUnknownRecord>(wire.clone()).unwrap();
            assert!(record.links.is_empty());
            wire["zz_bogus"] = true.into();
            let error = serde_json::from_value::<NativeUnknownRecord>(wire).unwrap_err();
            assert!(error.to_string().contains("zz_bogus"), "{error}");
        }
    }

    #[test]
    fn retained_record_derives_extent_and_digest() {
        let record = UnknownRecord::retained(
            UnknownId::mint("synthetic:model:unknown#0").expect("valid identity"),
            7,
            vec![1, 2, 3],
            vec!["synthetic:model:point#0".into()],
        );

        assert_eq!(record.byte_len(), 3);
        assert_eq!(record.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
        assert_eq!(record.data(), Some([1, 2, 3].as_slice()));
    }

    #[test]
    fn an_unretained_record_states_its_extent_and_digest() {
        let wire = serde_json::json!({
            "id": "synthetic:model:unknown#0",
            "offset": 7,
            "retention": {
                "retention": "digest",
                "byte_len": 99,
                "sha256": "wire-value"
            },
            "links": ["synthetic:model:point#0"]
        });

        let record: UnknownRecord =
            serde_json::from_value(wire.clone()).expect("deserialize unknown-record wire");

        assert_eq!(record.byte_len(), 99);
        assert_eq!(record.sha256(), "wire-value");
        assert_eq!(record.data(), None);
        assert_eq!(
            serde_json::to_value(record).expect("serialize unknown-record wire"),
            wire
        );
    }

    /// An extent or a digest beside the retained bytes has no place on the
    /// wire, and an unknown key is refused rather than dropped.
    #[test]
    fn a_retained_record_cannot_restate_its_extent_or_digest() {
        let wire = serde_json::json!({
            "id": "synthetic:model:unknown#0",
            "offset": 7,
            "retention": {"retention": "inline", "data": "AQID"}
        });
        let record: UnknownRecord =
            serde_json::from_value(wire.clone()).expect("deserialize unknown-record wire");
        assert_eq!(record.byte_len(), 3);
        assert_eq!(record.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
        assert_eq!(
            serde_json::to_value(record).expect("serialize unknown-record wire"),
            wire
        );

        for (key, value) in [
            ("byte_len", serde_json::json!(99)),
            ("sha256", serde_json::json!("not-the-digest-of-the-bytes")),
        ] {
            let mut restated = wire.clone();
            restated["retention"][key] = value;
            let error = serde_json::from_value::<UnknownRecord>(restated)
                .expect_err("the retained bytes own their extent and digest")
                .to_string();
            assert_eq!(
                error,
                format!("unknown field `{key}`, expected `data`"),
                "{key}"
            );
        }

        let mut bogus = wire;
        bogus["zz_bogus"] = serde_json::json!(true);
        let error = serde_json::from_value::<UnknownRecord>(bogus)
            .expect_err("an unknown key is refused")
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }
}
