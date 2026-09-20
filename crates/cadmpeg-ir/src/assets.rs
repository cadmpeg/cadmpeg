// SPDX-License-Identifier: Apache-2.0
//! Embedded and externally referenced document resources.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_core::text::NonBlankString;

crate::ids::id_type!(
    /// Stable identity of one document asset.
    AssetId
);

/// Nonempty embedded asset bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AssetData(#[serde(with = "crate::bytes")] Vec<u8>);

#[cfg(feature = "schema")]
impl JsonSchema for AssetData {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AssetData".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // The final alphabet symbols before padding have zero unused bits.
        // The separate alphabet check also rejects a trailing line terminator,
        // which some regular-expression engines permit before `$`.
        schemars::json_schema!({
            "type": "string",
            "minLength": 4,
            "contentEncoding": "base64",
            "pattern": "^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/][AQgw]==|[A-Za-z0-9+/]{2}[AEIMQUYcgkosw048]=)?$",
            "not": {"pattern": "[^A-Za-z0-9+/=]"}
        })
    }
}

impl AssetData {
    /// Construct nonempty embedded bytes.
    pub fn new(data: Vec<u8>) -> Option<Self> {
        (!data.is_empty()).then_some(Self(data))
    }
}

impl<'de> Deserialize<'de> for AssetData {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(crate::bytes::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("asset data must not be empty"))
    }
}

/// Bytes or location supplying an asset's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum AssetContent {
    /// Content embedded in the decoded document.
    Embedded {
        /// Exact resource bytes.
        data: AssetData,
    },
    /// Content resolved outside the decoded document.
    External {
        /// Stable external resource identifier or URI.
        #[serde(deserialize_with = "deserialize_uri")]
        uri: NonBlankString,
    },
}

/// A document resource referenced by model, drawing, or presentation entities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "AssetWire")]
pub struct Asset {
    /// Stable asset identity.
    pub id: AssetId,
    /// Source display name or basename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<NonBlankString>,
    /// IANA media type when identified from the source container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<NonBlankString>,
    /// Embedded bytes or an external resource location.
    pub content: AssetContent,
    /// Full-fidelity source record or container-entry identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

fn deserialize_uri<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<NonBlankString, D::Error> {
    NonBlankString::new(String::deserialize(deserializer)?)
        .ok_or_else(|| serde::de::Error::custom("asset uri must not be empty"))
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct AssetWire {
    /// Stable asset identity.
    id: AssetId,
    /// Source display name or basename.
    #[serde(default, deserialize_with = "deserialize_asset_wire_name")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<NonBlankString>"))]
    name: Option<String>,
    /// IANA media type when identified from the source container.
    #[serde(default, deserialize_with = "deserialize_asset_wire_media_type")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<NonBlankString>"))]
    media_type: Option<String>,
    /// Embedded bytes or an external resource location.
    content: AssetContent,
    /// Full-fidelity source record or container-entry identity.
    #[serde(default, deserialize_with = "deserialize_native_ref")]
    native_ref: Option<String>,
}

impl Asset {
    /// Construct an asset with nonempty optional metadata.
    pub fn try_new(
        id: AssetId,
        name: Option<String>,
        media_type: Option<String>,
        content: AssetContent,
        native_ref: Option<String>,
    ) -> Result<Self, String> {
        let name = name
            .map(|name| {
                NonBlankString::new(name).ok_or_else(|| "asset name must not be empty".to_owned())
            })
            .transpose()?;
        let media_type = media_type
            .map(|media_type| {
                NonBlankString::new(media_type)
                    .ok_or_else(|| "asset media_type must not be empty".to_owned())
            })
            .transpose()?;
        Ok(Self {
            id,
            name,
            media_type,
            content,
            native_ref,
        })
    }
}

impl TryFrom<AssetWire> for Asset {
    type Error = String;

    fn try_from(wire: AssetWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.id,
            wire.name,
            wire.media_type,
            wire.content,
            wire.native_ref,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Asset, AssetContent, AssetData, AssetId};

    #[test]
    fn complete_document_admission_requires_nonempty_canonical_asset_bytes() {
        let empty = serde_json::to_value(crate::CadIr::empty()).unwrap();
        let document = |data: &str| {
            let mut wire = empty.clone();
            wire["model"]["assets"] = serde_json::json!([{
                "id": "test:assets:asset#embedded",
                "content": {"kind": "embedded", "data": data}
            }]);
            wire
        };
        for (encoded, bytes) in [
            ("AA==", vec![0]),
            ("AAA=", vec![0, 0]),
            ("AAAA", vec![0, 0, 0]),
        ] {
            let wire = document(encoded);
            let ir: crate::CadIr = serde_json::from_value(wire.clone()).unwrap();
            let AssetContent::Embedded { data } = &ir.model.assets[0].content else {
                panic!("embedded fixture changed content family");
            };
            assert_eq!(data.0, bytes);
            assert_eq!(serde_json::to_value(ir).unwrap(), wire);
        }
        for encoded in ["", "AA", "AA=", "AAAA=", "AB==", "AAB=", "AA==\n", "__8="] {
            assert!(
                serde_json::from_value::<crate::CadIr>(document(encoded)).is_err(),
                "accepted invalid embedded bytes {encoded:?}"
            );
        }
    }

    #[test]
    #[cfg(feature = "schema")]
    fn asset_byte_schema_requires_at_least_one_encoded_byte() {
        let schema = serde_json::to_value(schemars::schema_for!(AssetData)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["minLength"], 4);
        assert_eq!(schema["contentEncoding"], "base64");
    }

    #[test]
    fn asset_admission_rejects_empty_content_and_metadata() {
        assert!(AssetData::new(Vec::new()).is_none());
        for (field, content) in [
            ("data", serde_json::json!({"kind": "embedded", "data": ""})),
            ("uri", serde_json::json!({"kind": "external", "uri": ""})),
        ] {
            let error = serde_json::from_value::<AssetContent>(content).expect_err("empty content");
            assert!(error.to_string().contains(field));
        }
        for field in ["name", "media_type"] {
            let mut value = serde_json::json!({"id": "synthetic:test:asset#asset", "content": {"kind": "embedded", "data": "AA=="}});
            value[field] = serde_json::json!("");
            let error = serde_json::from_value::<Asset>(value).expect_err("empty metadata");
            assert!(error.to_string().contains(field));
        }
        let content = AssetContent::Embedded {
            data: AssetData::new(vec![0]).expect("nonempty data"),
        };
        assert!(Asset::try_new(
            AssetId::mint("synthetic:test:asset#asset").expect("id"),
            Some(String::new()),
            None,
            content.clone(),
            None
        )
        .is_err());
        assert!(Asset::try_new(
            AssetId::mint("synthetic:test:asset#asset").expect("id"),
            None,
            Some(String::new()),
            content,
            None
        )
        .is_err());
        for content in [
            serde_json::json!({"kind": "embedded", "data": "AA=="}),
            serde_json::json!({"kind": "external", "uri": " u "}),
        ] {
            let value = serde_json::json!({"id": "synthetic:test:asset#asset", "name": " n ", "media_type": " m ", "content": content});
            let asset: Asset = serde_json::from_value(value.clone()).expect("valid asset");
            assert_eq!(serde_json::to_value(asset).expect("serialize asset"), value);
        }
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_asset_wire_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_asset_wire_media_type, String, "media_type");
