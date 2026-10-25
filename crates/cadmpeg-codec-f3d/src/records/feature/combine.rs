// SPDX-License-Identifier: Apache-2.0
//! Combine operations, their tool bodies and the external body identity they may name.

use cadmpeg_core::decode::u64_from_index;

use crate::records::identity::Sourced;
use crate::records::mesh::DesignRelaxedGuidText;
use serde::{Deserialize, Deserializer, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_external_identity,
    DesignCombineExternalBodyIdentity,
    "external_identity"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_property_key,
    DesignRelaxedGuidText,
    "external_property_key"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_version_urn,
    String,
    "external_version_urn"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_property_key_offset,
    u64,
    "external_property_key_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_version_urn_offset,
    u64,
    "external_version_urn_offset"
);
/// Serialized prologue form of a `Combine` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCombineForm {
    /// Nine zero bytes followed by the operation at offset 20.
    Standard,
    /// Class-387 form with the operation at offset 21.
    Compact,
    /// Eighteen-zero reference form with the operation at offset 31.
    ExtendedReference,
}

/// Version identity carried by a cross-document reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignExternalVersion {
    pub(crate) property_key: Sourced<DesignRelaxedGuidText>,
    pub(crate) version_urn: Sourced<String>,
}

/// Cross-document persistent body identity carried by a `Combine` tool selector.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignCombineExternalBodyIdentityDocumentWire")]
pub(crate) struct DesignCombineExternalBodyIdentity {
    /// Asset GUID of the enclosing body selector.
    selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id` of a located scope.
    selector_asset_id_offset: Option<u64>,
    /// Context GUID of the enclosing body selector.
    selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id` of a located scope.
    selector_context_id_offset: Option<u64>,
    /// Same-segment occurrence reference preceding the external body reference.
    occurrence_reference: u64,
    /// Byte offset of `occurrence_reference` of a located scope.
    occurrence_reference_offset: Option<u64>,
    /// Entity reference of the body in the referenced document.
    external_body_reference: u64,
    /// Byte offset of `external_body_reference` of a located scope.
    external_body_reference_offset: Option<u64>,
    /// Segment carried by the cross-document body reference.
    external_segment: u32,
    /// Byte offset of `external_segment` of a located scope.
    external_segment_offset: Option<u64>,
    /// Byte offset of `external_asset_id` of a located scope.
    external_asset_id_offset: Option<u64>,
    /// Link name carried by the cross-document body reference.
    external_link_name: String,
    /// Byte offset of `external_link_name` of a located scope.
    external_link_name_offset: Option<u64>,
    /// Property key and referenced-document version identity.
    external_version: Option<DesignExternalVersion>,
    /// Raw selection stamp and binary64 evaluation-scalar bits, separated by the epoch.
    #[serde(default)]
    tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order of a located scope.
    #[serde(skip)]
    tail_value_offsets: Option<[u64; 2]>,
}

#[cfg(test)]
thread_local! {
    static COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCombineExternalBodyIdentity {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            selector_asset_id: self.selector_asset_id.clone(),
            selector_asset_id_offset: self.selector_asset_id_offset,
            selector_context_id: self.selector_context_id.clone(),
            selector_context_id_offset: self.selector_context_id_offset,
            occurrence_reference: self.occurrence_reference,
            occurrence_reference_offset: self.occurrence_reference_offset,
            external_body_reference: self.external_body_reference,
            external_body_reference_offset: self.external_body_reference_offset,
            external_segment: self.external_segment,
            external_segment_offset: self.external_segment_offset,
            external_asset_id_offset: self.external_asset_id_offset,
            external_link_name: self.external_link_name.clone(),
            external_link_name_offset: self.external_link_name_offset,
            external_version: self.external_version.clone(),
            tail_values: self.tail_values,
            tail_value_offsets: self.tail_value_offsets,
        }
    }
}

#[derive(Serialize)]
struct DesignCombineExternalBodyIdentityRef<'a> {
    selector_asset_id: &'a str,
    selector_context_id: &'a str,
    occurrence_reference: u64,
    external_body_reference: u64,
    external_segment: u32,
    external_asset_id: &'a str,
    external_link_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_property_key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_version_urn: Option<&'a str>,
    tail_values: [u64; 2],
}

impl Serialize for DesignCombineExternalBodyIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCombineExternalBodyIdentityRef {
            selector_asset_id: self.selector_asset_id.as_str(),
            selector_context_id: self.selector_context_id.as_str(),
            occurrence_reference: self.occurrence_reference,
            external_body_reference: self.external_body_reference,
            external_segment: self.external_segment,
            external_asset_id: self.selector_asset_id.as_str(),
            external_link_name: &self.external_link_name,
            external_property_key: self
                .external_version
                .as_ref()
                .map(|version| version.property_key.value.as_str()),
            external_version_urn: self
                .external_version
                .as_ref()
                .map(|version| version.version_urn.value.as_str()),
            tail_values: self.tail_values,
        }
        .serialize(serializer)
    }
}

/// Document wire fields for an external Combine body identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignCombineExternalBodyIdentityDocumentWire {
    /// Asset GUID of the enclosing body selector.
    pub(crate) selector_asset_id: DesignRelaxedGuidText,
    /// Context GUID of the enclosing body selector.
    pub(crate) selector_context_id: DesignRelaxedGuidText,
    /// Same-segment occurrence reference preceding the external body reference.
    pub(crate) occurrence_reference: u64,
    /// Entity reference of the body in the referenced document.
    pub(crate) external_body_reference: u64,
    /// Segment carried by the cross-document body reference.
    pub(crate) external_segment: u32,
    /// Asset GUID carried by the cross-document body reference.
    pub(crate) external_asset_id: DesignRelaxedGuidText,
    /// Link name carried by the cross-document body reference.
    pub(crate) external_link_name: String,
    /// Optional property key preceding the version identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_property_key"
    )]
    pub(crate) external_property_key: Option<DesignRelaxedGuidText>,
    /// Optional referenced-document version identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_version_urn"
    )]
    pub(crate) external_version_urn: Option<String>,
    /// Raw selection stamp and binary64 evaluation-scalar bits, separated by the epoch.
    #[serde(default)]
    pub(crate) tail_values: [u64; 2],
}

/// Decoded fields of an external Combine body identity with their source
/// offsets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct DesignCombineExternalBodyIdentityDraft {
    /// Asset GUID of the enclosing body selector.
    pub(crate) selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    pub(crate) selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    pub(crate) selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    pub(crate) selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    pub(crate) occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    pub(crate) occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    pub(crate) external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    pub(crate) external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    pub(crate) external_segment: u32,
    /// Byte offset of `external_segment`.
    pub(crate) external_segment_offset: u64,
    /// Asset GUID carried by the cross-document body reference.
    pub(crate) external_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `external_asset_id`.
    pub(crate) external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    pub(crate) external_link_name: String,
    /// Byte offset of `external_link_name`.
    pub(crate) external_link_name_offset: u64,
    /// Optional property key preceding the version identity.
    #[serde(default, deserialize_with = "deserialize_external_property_key")]
    pub(crate) external_property_key: Option<DesignRelaxedGuidText>,
    /// Byte offset of `external_property_key` when present.
    #[serde(default, deserialize_with = "deserialize_external_property_key_offset")]
    pub(crate) external_property_key_offset: Option<u64>,
    /// Optional referenced-document version identity.
    #[serde(default, deserialize_with = "deserialize_external_version_urn")]
    pub(crate) external_version_urn: Option<String>,
    /// Byte offset of `external_version_urn` when present.
    #[serde(default, deserialize_with = "deserialize_external_version_urn_offset")]
    pub(crate) external_version_urn_offset: Option<u64>,
    /// Raw selection stamp and binary64 evaluation-scalar bits, separated by the epoch.
    #[serde(default)]
    pub(crate) tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    pub(crate) tail_value_offsets: [u64; 2],
}

impl DesignCombineExternalBodyIdentity {
    pub(crate) fn selector_asset_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_asset_id
    }
    /// Byte offset of `selector_asset_id` of a located scope.
    pub(crate) fn selector_asset_id_offset(&self) -> Option<u64> {
        self.selector_asset_id_offset
    }
    pub(crate) fn selector_context_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_context_id
    }
    pub(crate) fn occurrence_reference(&self) -> u64 {
        self.occurrence_reference
    }
    pub(crate) fn external_body_reference(&self) -> u64 {
        self.external_body_reference
    }
    pub(crate) fn external_segment(&self) -> u32 {
        self.external_segment
    }
    pub(crate) fn external_asset_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_asset_id
    }
    pub(crate) fn external_link_name(&self) -> &str {
        &self.external_link_name
    }
    pub(crate) fn external_version(&self) -> Option<&DesignExternalVersion> {
        self.external_version.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn tail_values(&self) -> [u64; 2] {
        self.tail_values
    }
    #[cfg(test)]
    pub(crate) fn tail_value_offsets(&self) -> Option<[u64; 2]> {
        self.tail_value_offsets
    }
    #[cfg(test)]
    pub(crate) fn external_asset_id_offset(&self) -> Option<u64> {
        self.external_asset_id_offset
    }

    /// Admit the document fields of an identity.
    fn from_fields(wire: DesignCombineExternalBodyIdentityDocumentWire) -> Result<Self, String> {
        let external_version = match (wire.external_property_key, wire.external_version_urn) {
            (None, None) => None,
            (Some(key), Some(urn)) => {
                if urn.is_empty() {
                    return Err("external_version_urn must not be empty".into());
                }
                Some(DesignExternalVersion {
                    property_key: Sourced {
                        value: key,
                        offset: None,
                    },
                    version_urn: Sourced {
                        value: urn,
                        offset: None,
                    },
                })
            }
            _ => {
                return Err(
                    "external_property_key and external_version_urn must occur together".into(),
                )
            }
        };
        if wire.external_asset_id != wire.selector_asset_id {
            return Err("external_asset_id must match selector_asset_id".into());
        }
        if wire.occurrence_reference == 0 {
            return Err("occurrence_reference must be nonzero".into());
        }
        if wire.external_body_reference == 0 {
            return Err("external_body_reference must be nonzero".into());
        }
        if wire.external_link_name.is_empty() {
            return Err("external_link_name must not be empty".into());
        }
        Ok(Self {
            selector_asset_id: wire.selector_asset_id,
            selector_asset_id_offset: None,
            selector_context_id: wire.selector_context_id,
            selector_context_id_offset: None,
            occurrence_reference: wire.occurrence_reference,
            occurrence_reference_offset: None,
            external_body_reference: wire.external_body_reference,
            external_body_reference_offset: None,
            external_segment: wire.external_segment,
            external_segment_offset: None,
            external_asset_id_offset: None,
            external_link_name: wire.external_link_name,
            external_link_name_offset: None,
            external_version,
            tail_values: wire.tail_values,
            tail_value_offsets: None,
        })
    }

    /// Check the source offset chain of a located identity: each field
    /// follows the previous one by its encoding length and fixed gap. An
    /// absent offset fails the check.
    pub(crate) fn offset_chain_valid(&self) -> Result<(), String> {
        let utf16_end = |offset: u64, text: &str| -> Option<u64> {
            offset.checked_add(
                u64::try_from(text.encode_utf16().count())
                    .ok()?
                    .checked_mul(2)?,
            )
        };
        let after_text = |offset: Option<u64>, text: &str, delta: u64| {
            offset
                .and_then(|offset| utf16_end(offset, text))
                .and_then(|end| end.checked_add(delta))
        };
        let prefix = u64_from_index(crate::layout::combine_external_selector_prefix::LEN + 4);
        if self
            .selector_asset_id_offset
            .is_none_or(|offset| offset < prefix)
        {
            return Err("selector_asset_id_offset must follow the selector header".into());
        }
        let tail_value_offsets = self
            .tail_value_offsets
            .map_or([None, None], |offsets| [Some(offsets[0]), Some(offsets[1])]);
        for (field, actual, expected) in [
            (
                "selector_context_id_offset",
                self.selector_context_id_offset,
                after_text(
                    self.selector_asset_id_offset,
                    self.selector_asset_id.as_str(),
                    4,
                ),
            ),
            (
                "occurrence_reference_offset",
                self.occurrence_reference_offset,
                after_text(
                    self.selector_context_id_offset,
                    self.selector_context_id.as_str(),
                    13,
                ),
            ),
            (
                "external_body_reference_offset",
                self.external_body_reference_offset,
                self.occurrence_reference_offset
                    .and_then(|offset| offset.checked_add(15)),
            ),
            (
                "external_segment_offset",
                self.external_segment_offset,
                self.external_body_reference_offset
                    .and_then(|offset| offset.checked_add(9)),
            ),
            (
                "external_asset_id_offset",
                self.external_asset_id_offset,
                self.external_segment_offset
                    .and_then(|offset| offset.checked_add(8)),
            ),
            (
                "external_link_name_offset",
                self.external_link_name_offset,
                after_text(
                    self.external_asset_id_offset,
                    self.selector_asset_id.as_str(),
                    5,
                ),
            ),
            (
                "tail_value_offsets[1]",
                tail_value_offsets[1],
                tail_value_offsets[0].and_then(|offset| offset.checked_add(12)),
            ),
        ] {
            if actual.is_none() || expected != actual {
                return Err(format!(
                    "{field} disagrees with the external identity offset chain"
                ));
            }
        }
        let tail_offset = match &self.external_version {
            None => after_text(self.external_link_name_offset, &self.external_link_name, 7),
            Some(version) => {
                if after_text(self.external_link_name_offset, &self.external_link_name, 5)
                    .is_none_or(|expected| version.property_key.offset != Some(expected))
                {
                    return Err(
                        "external_property_key_offset disagrees with external_link_name".into(),
                    );
                }
                if after_text(
                    version.property_key.offset,
                    version.property_key.value.as_str(),
                    4,
                )
                .is_none_or(|expected| version.version_urn.offset != Some(expected))
                {
                    return Err(
                        "external_version_urn_offset disagrees with external_property_key".into(),
                    );
                }
                after_text(version.version_urn.offset, &version.version_urn.value, 6)
            }
        };
        if tail_offset.is_none() || tail_offset != tail_value_offsets[0] {
            return Err(
                "tail_value_offsets[0] disagrees with the external identity offset chain".into(),
            );
        }
        Ok(())
    }
}

impl TryFrom<DesignCombineExternalBodyIdentityDocumentWire> for DesignCombineExternalBodyIdentity {
    type Error = String;
    fn try_from(wire: DesignCombineExternalBodyIdentityDocumentWire) -> Result<Self, Self::Error> {
        Self::from_fields(wire)
    }
}

impl TryFrom<DesignCombineExternalBodyIdentityDraft> for DesignCombineExternalBodyIdentity {
    type Error = String;
    fn try_from(draft: DesignCombineExternalBodyIdentityDraft) -> Result<Self, Self::Error> {
        let versioned = match (
            draft.external_property_key_offset,
            draft.external_version_urn_offset,
        ) {
            (None, None) => None,
            (Some(key), Some(urn)) => Some((key, urn)),
            _ => {
                return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into())
            }
        };
        if versioned.is_some()
            != (draft.external_property_key.is_some() && draft.external_version_urn.is_some())
        {
            return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into());
        }
        let mut identity = Self::from_fields(DesignCombineExternalBodyIdentityDocumentWire {
            selector_asset_id: draft.selector_asset_id,
            selector_context_id: draft.selector_context_id,
            occurrence_reference: draft.occurrence_reference,
            external_body_reference: draft.external_body_reference,
            external_segment: draft.external_segment,
            external_asset_id: draft.external_asset_id,
            external_link_name: draft.external_link_name,
            external_property_key: draft.external_property_key,
            external_version_urn: draft.external_version_urn,
            tail_values: draft.tail_values,
        })?;
        identity.selector_asset_id_offset = Some(draft.selector_asset_id_offset);
        identity.selector_context_id_offset = Some(draft.selector_context_id_offset);
        identity.occurrence_reference_offset = Some(draft.occurrence_reference_offset);
        identity.external_body_reference_offset = Some(draft.external_body_reference_offset);
        identity.external_segment_offset = Some(draft.external_segment_offset);
        identity.external_asset_id_offset = Some(draft.external_asset_id_offset);
        identity.external_link_name_offset = Some(draft.external_link_name_offset);
        identity.tail_value_offsets = Some(draft.tail_value_offsets);
        if let (Some(version), Some((key, urn))) = (&mut identity.external_version, versioned) {
            version.property_key.offset = Some(key);
            version.version_urn.offset = Some(urn);
        }
        identity.offset_chain_valid()?;
        Ok(identity)
    }
}

#[cfg(test)]
impl From<DesignCombineExternalBodyIdentity> for DesignCombineExternalBodyIdentityDocumentWire {
    fn from(record: DesignCombineExternalBodyIdentity) -> Self {
        Self {
            selector_asset_id: record.selector_asset_id.clone(),
            selector_context_id: record.selector_context_id,
            occurrence_reference: record.occurrence_reference,
            external_body_reference: record.external_body_reference,
            external_segment: record.external_segment,
            external_asset_id: record.selector_asset_id,
            external_link_name: record.external_link_name,
            external_property_key: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.value.clone()),
            external_version_urn: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.value.clone()),
            tail_values: record.tail_values,
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignCombineExternalBodyIdentity {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset(
            "selector_asset_id_offset",
            &mut self.selector_asset_id_offset,
        );
        offsets.offset(
            "selector_context_id_offset",
            &mut self.selector_context_id_offset,
        );
        offsets.offset(
            "occurrence_reference_offset",
            &mut self.occurrence_reference_offset,
        );
        offsets.offset(
            "external_body_reference_offset",
            &mut self.external_body_reference_offset,
        );
        offsets.offset("external_segment_offset", &mut self.external_segment_offset);
        offsets.offset(
            "external_asset_id_offset",
            &mut self.external_asset_id_offset,
        );
        offsets.offset(
            "external_link_name_offset",
            &mut self.external_link_name_offset,
        );
        if let Some(version) = &mut self.external_version {
            offsets.sourced("external_property_key_offset", &mut version.property_key);
            offsets.sourced("external_version_urn_offset", &mut version.version_urn);
        }
        offsets.offsets("tail_value_offsets", &mut self.tail_value_offsets);
    }
}

/// One target or tool body selector owned by a `Combine` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignCombineBodySelection {
    /// Body-selection record index.
    pub(crate) record_index: u32,
    /// Complete external body identity when the selector crosses a document boundary.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_identity"
    )]
    pub(crate) external_identity: Option<DesignCombineExternalBodyIdentity>,
}

/// Exact Boolean construction carried by a `Combine` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignCombineOperationWire")]
pub(crate) struct DesignCombineOperation {
    /// Serialized scope-prologue form.
    pub(crate) form: DesignCombineForm,
    /// Join, cut, or intersect operation.
    pub(crate) operation: cadmpeg_ir::features::BooleanKind,
    /// Byte offset of the operation u32 of a located scope.
    pub(crate) operation_offset: Option<u64>,
    /// Whether the source operation retains its tool bodies.
    pub(crate) keep_tools: bool,
    /// Byte offset of the keep-tools Boolean of a located scope.
    pub(crate) keep_tools_offset: Option<u64>,
    /// Boolean target body selector.
    pub(crate) target_record_index: u32,
    /// Boolean tool body selectors in source order.
    pub(crate) tools: DesignCombineTools,
}

#[cfg(test)]
thread_local! {
    static COMBINE_OPERATION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCombineOperation {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COMBINE_OPERATION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            form: self.form,
            operation: self.operation,
            operation_offset: self.operation_offset,
            keep_tools: self.keep_tools,
            keep_tools_offset: self.keep_tools_offset,
            target_record_index: self.target_record_index,
            tools: self.tools.clone(),
        }
    }
}

struct DesignCombineToolsRef<'a>(&'a DesignCombineTools);

impl Serialize for DesignCombineToolsRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter())
    }
}

#[derive(Serialize)]
struct DesignCombineOperationRef<'a> {
    form: DesignCombineForm,
    operation: cadmpeg_ir::features::BooleanKind,
    keep_tools: bool,
    target: DesignCombineBodySelection,
    tools: DesignCombineToolsRef<'a>,
}

impl Serialize for DesignCombineOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCombineOperationRef {
            form: self.form,
            operation: self.operation,
            keep_tools: self.keep_tools,
            target: DesignCombineBodySelection {
                record_index: self.target_record_index,
                external_identity: None,
            },
            tools: DesignCombineToolsRef(&self.tools),
        }
        .serialize(serializer)
    }
}

/// Ordered nonempty tools of a Combine operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignCombineTools {
    pub(crate) first: DesignCombineBodySelection,
    pub(crate) additional: Vec<DesignCombineBodySelection>,
}

impl DesignCombineTools {
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &DesignCombineBodySelection> {
        std::iter::once(&self.first).chain(&self.additional)
    }
}

/// Exact Boolean construction carried by a `Combine` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCombineOperationWire {
    /// Serialized scope-prologue form.
    form: DesignCombineForm,
    /// Join, cut, or intersect operation.
    #[serde(deserialize_with = "deserialize_combine_operation_kind")]
    operation: cadmpeg_ir::features::BooleanKind,
    /// Whether the source operation retains its tool bodies.
    keep_tools: bool,
    /// Boolean target body selector.
    target: DesignCombineBodySelection,
    /// Boolean tool body selectors in source order.
    tools: Vec<DesignCombineBodySelection>,
}

fn deserialize_combine_operation_kind<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<cadmpeg_ir::features::BooleanKind, D::Error> {
    cadmpeg_ir::features::BooleanKind::deserialize(deserializer)
        .map_err(|error| serde::de::Error::custom(format!("operation: {error}")))
}

impl TryFrom<DesignCombineOperationWire> for DesignCombineOperation {
    type Error = String;
    fn try_from(wire: DesignCombineOperationWire) -> Result<Self, Self::Error> {
        if wire.target.external_identity.is_some() {
            return Err("target.external_identity must be absent".into());
        }
        let mut tools = wire.tools.into_iter();
        let first = tools
            .next()
            .ok_or("tools must contain at least one body selection")?;
        Ok(Self {
            form: wire.form,
            operation: wire.operation,
            operation_offset: None,
            keep_tools: wire.keep_tools,
            keep_tools_offset: None,
            target_record_index: wire.target.record_index,
            tools: DesignCombineTools {
                first,
                additional: tools.collect(),
            },
        })
    }
}

#[cfg(test)]
impl From<DesignCombineOperation> for DesignCombineOperationWire {
    fn from(operation: DesignCombineOperation) -> Self {
        Self {
            form: operation.form,
            operation: operation.operation,
            keep_tools: operation.keep_tools,
            target: DesignCombineBodySelection {
                record_index: operation.target_record_index,
                external_identity: None,
            },
            tools: std::iter::once(operation.tools.first)
                .chain(operation.tools.additional)
                .collect(),
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignCombineOperation {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("operation_offset", &mut self.operation_offset);
        offsets.offset("keep_tools_offset", &mut self.keep_tools_offset);
        let tools = std::iter::once(&mut self.tools.first).chain(&mut self.tools.additional);
        for (index, tool) in tools.enumerate() {
            if let Some(identity) = &mut tool.external_identity {
                offsets.record(&format!("tools.{index}.external_identity"), identity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DesignCombineExternalBodyIdentity, DesignCombineExternalBodyIdentityDocumentWire,
        DesignCombineOperation, DesignCombineOperationWire, COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT,
        COMBINE_OPERATION_CLONE_COUNT,
    };

    #[derive(serde::Serialize)]
    struct NestedRecord<'a, T: serde::Serialize> {
        id: &'static str,
        value: &'a T,
    }

    fn external_identity(version: bool) -> DesignCombineExternalBodyIdentity {
        let mut wire = serde_json::json!({
            "selector_asset_id": "00000004-1111-4111-8111-111111111111",
            "selector_asset_id_offset": 44,
            "selector_context_id": "00000005-1111-4111-8111-111111111111",
            "selector_context_id_offset": 120,
            "occurrence_reference": 1,
            "occurrence_reference_offset": 205,
            "external_body_reference": 2,
            "external_body_reference_offset": 220,
            "external_segment": 1,
            "external_segment_offset": 229,
            "external_asset_id": "00000004-1111-4111-8111-111111111111",
            "external_asset_id_offset": 237,
            "external_link_name": "link",
            "external_link_name_offset": 314,
            "tail_values": [0, 0],
            "tail_value_offsets": [329, 341]
        });
        if version {
            wire["external_property_key"] =
                serde_json::json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
            wire["external_property_key_offset"] = serde_json::json!(327);
            wire["external_version_urn"] = serde_json::json!("urn");
            wire["external_version_urn_offset"] = serde_json::json!(403);
            wire["tail_value_offsets"] = serde_json::json!([415, 427]);
        }
        serde_json::from_value(wire).unwrap()
    }

    fn operation(version: bool) -> DesignCombineOperation {
        serde_json::from_value(serde_json::json!({
            "form": "standard",
            "operation": "join",
            "operation_offset": 20,
            "keep_tools": false,
            "keep_tools_offset": 25,
            "target": {"record_index": 1},
            "tools": [{"record_index": 2, "external_identity": external_identity(version)}]
        }))
        .unwrap()
    }

    #[test]
    fn combine_external_identity_borrowed_wire_matches_owned_wire_bytes() {
        for identity in [external_identity(false), external_identity(true)] {
            let owned = DesignCombineExternalBodyIdentityDocumentWire::from(identity.clone());
            assert_eq!(
                serde_json::to_vec(&identity).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn combine_external_identity_native_retained_limit_refuses_before_clone() {
        let identity = external_identity(true);
        let record = NestedRecord {
            id: "f3d:native:combine-external-identity#0",
            value: &identity,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT.with(|count| count.set(0)),
            || COMBINE_EXTERNAL_IDENTITY_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }

    #[test]
    fn combine_operation_borrowed_wire_matches_owned_wire_bytes() {
        for record in [operation(false), operation(true)] {
            let owned = DesignCombineOperationWire::from(record.clone());
            assert_eq!(
                serde_json::to_vec(&record).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn combine_operation_native_retained_limit_refuses_before_clone() {
        let operation = operation(true);
        let record = NestedRecord {
            id: "f3d:native:combine-operation#0",
            value: &operation,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || COMBINE_OPERATION_CLONE_COUNT.with(|count| count.set(0)),
            || COMBINE_OPERATION_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }
}

mod identity_rewrite;
