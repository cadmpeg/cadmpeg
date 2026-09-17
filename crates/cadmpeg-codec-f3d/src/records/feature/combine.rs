// SPDX-License-Identifier: Apache-2.0
//! Combine operations, their tool bodies and the external body identity they may name.

use crate::records::identity::Located;
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
    deserialize_external_property_key_offset,
    u64,
    "external_property_key_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_version_urn,
    String,
    "external_version_urn"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_version_urn_offset,
    u64,
    "external_version_urn_offset"
);
/// Serialized prologue form of a `Combine` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignCombineForm {
    /// Nine zero bytes followed by the operation at offset 20.
    Standard,
    /// Class-387 form with the operation at offset 21.
    Compact,
    /// Eighteen-zero reference form with the operation at offset 31.
    ExtendedReference,
}

/// Version identity carried by a cross-document reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignExternalVersion {
    pub property_key: Located<DesignRelaxedGuidText>,
    pub version_urn: Located<String>,
}

/// Cross-document persistent body identity carried by a `Combine` tool selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCombineExternalBodyIdentityWire",
    into = "DesignCombineExternalBodyIdentityWire"
)]
pub struct DesignCombineExternalBodyIdentity {
    /// Asset GUID of the enclosing body selector.
    selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    external_segment: u32,
    /// Byte offset of `external_segment`.
    external_segment_offset: u64,
    /// Byte offset of `external_asset_id`.
    external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    external_link_name: String,
    /// Byte offset of `external_link_name`.
    external_link_name_offset: u64,
    /// Located property key and referenced-document version identity.
    external_version: Option<DesignExternalVersion>,
    /// Retained u64 values around the fixed `u32 48` member in the selector tail.
    #[serde(default)]
    tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    tail_value_offsets: [u64; 2],
}

/// Wire fields for an external Combine body identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignCombineExternalBodyIdentityWire {
    /// Asset GUID of the enclosing body selector.
    pub selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    pub selector_asset_id_offset: u64,
    /// Context GUID of the enclosing body selector.
    pub selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    pub selector_context_id_offset: u64,
    /// Same-segment occurrence reference preceding the external body reference.
    pub occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    pub occurrence_reference_offset: u64,
    /// Entity reference of the body in the referenced document.
    pub external_body_reference: u64,
    /// Byte offset of `external_body_reference`.
    pub external_body_reference_offset: u64,
    /// Segment carried by the cross-document body reference.
    pub external_segment: u32,
    /// Byte offset of `external_segment`.
    pub external_segment_offset: u64,
    /// Asset GUID carried by the cross-document body reference.
    pub external_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `external_asset_id`.
    pub external_asset_id_offset: u64,
    /// Link name carried by the cross-document body reference.
    pub external_link_name: String,
    /// Byte offset of `external_link_name`.
    pub external_link_name_offset: u64,
    /// Optional property key preceding the version identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_property_key"
    )]
    pub external_property_key: Option<DesignRelaxedGuidText>,
    /// Byte offset of `external_property_key` when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_property_key_offset"
    )]
    pub external_property_key_offset: Option<u64>,
    /// Optional referenced-document version identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_version_urn"
    )]
    pub external_version_urn: Option<String>,
    /// Byte offset of `external_version_urn` when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_version_urn_offset"
    )]
    pub external_version_urn_offset: Option<u64>,
    /// Retained u64 values around the fixed `u32 48` member in the selector tail.
    #[serde(default)]
    pub tail_values: [u64; 2],
    /// Byte offsets of `tail_values` in source order.
    #[serde(default)]
    pub tail_value_offsets: [u64; 2],
}

impl DesignCombineExternalBodyIdentity {
    pub(crate) fn selector_asset_id(&self) -> &DesignRelaxedGuidText {
        &self.selector_asset_id
    }
    pub(crate) fn selector_asset_id_offset(&self) -> u64 {
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
    pub(crate) fn tail_value_offsets(&self) -> [u64; 2] {
        self.tail_value_offsets
    }
    #[cfg(test)]
    pub(crate) fn external_asset_id_offset(&self) -> u64 {
        self.external_asset_id_offset
    }
}

impl TryFrom<DesignCombineExternalBodyIdentityWire> for DesignCombineExternalBodyIdentity {
    type Error = String;
    fn try_from(wire: DesignCombineExternalBodyIdentityWire) -> Result<Self, Self::Error> {
        let external_version = match (wire.external_property_key, wire.external_property_key_offset, wire.external_version_urn, wire.external_version_urn_offset) {
            (None, None, None, None) => None,
            (Some(key), Some(key_offset), Some(urn), Some(urn_offset)) => Some(DesignExternalVersion { property_key: Located { value: key, offset: key_offset }, version_urn: Located { value: urn, offset: urn_offset } }),
            _ => return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into()),
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
        let utf16_end = |offset: u64, text: &str| -> Option<u64> {
            offset.checked_add(
                u64::try_from(text.encode_utf16().count())
                    .ok()?
                    .checked_mul(2)?,
            )
        };
        let after_text = |offset: u64, text: &str, delta: u64| {
            utf16_end(offset, text).and_then(|end| end.checked_add(delta))
        };
        let prefix = (crate::layout::combine_external_selector_prefix::LEN + 4) as u64;
        if wire.selector_asset_id_offset < prefix {
            return Err("selector_asset_id_offset must follow the selector header".into());
        }
        for (field, actual, expected) in [
            (
                "selector_context_id_offset",
                wire.selector_context_id_offset,
                after_text(
                    wire.selector_asset_id_offset,
                    wire.selector_asset_id.as_str(),
                    4,
                ),
            ),
            (
                "occurrence_reference_offset",
                wire.occurrence_reference_offset,
                after_text(
                    wire.selector_context_id_offset,
                    wire.selector_context_id.as_str(),
                    13,
                ),
            ),
            (
                "external_body_reference_offset",
                wire.external_body_reference_offset,
                wire.occurrence_reference_offset.checked_add(15),
            ),
            (
                "external_segment_offset",
                wire.external_segment_offset,
                wire.external_body_reference_offset.checked_add(9),
            ),
            (
                "external_asset_id_offset",
                wire.external_asset_id_offset,
                wire.external_segment_offset.checked_add(8),
            ),
            (
                "external_link_name_offset",
                wire.external_link_name_offset,
                after_text(
                    wire.external_asset_id_offset,
                    wire.external_asset_id.as_str(),
                    5,
                ),
            ),
            (
                "tail_value_offsets[1]",
                wire.tail_value_offsets[1],
                wire.tail_value_offsets[0].checked_add(12),
            ),
        ] {
            if expected != Some(actual) {
                return Err(format!(
                    "{field} disagrees with the external identity offset chain"
                ));
            }
        }
        let tail_offset = match &external_version {
            None => after_text(wire.external_link_name_offset, &wire.external_link_name, 7),
            Some(version) => {
                if version.version_urn.value.is_empty() {
                    return Err("external_version_urn must not be empty".into());
                }
                if after_text(wire.external_link_name_offset, &wire.external_link_name, 5)
                    != Some(version.property_key.offset)
                {
                    return Err(
                        "external_property_key_offset disagrees with external_link_name".into(),
                    );
                }
                if after_text(
                    version.property_key.offset,
                    version.property_key.value.as_str(),
                    4,
                ) != Some(version.version_urn.offset)
                {
                    return Err(
                        "external_version_urn_offset disagrees with external_property_key".into(),
                    );
                }
                after_text(version.version_urn.offset, &version.version_urn.value, 6)
            }
        };
        if tail_offset != Some(wire.tail_value_offsets[0]) {
            return Err(
                "tail_value_offsets[0] disagrees with the external identity offset chain".into(),
            );
        }
        Ok(Self {
            selector_asset_id: wire.selector_asset_id,
            selector_asset_id_offset: wire.selector_asset_id_offset,
            selector_context_id: wire.selector_context_id,
            selector_context_id_offset: wire.selector_context_id_offset,
            occurrence_reference: wire.occurrence_reference,
            occurrence_reference_offset: wire.occurrence_reference_offset,
            external_body_reference: wire.external_body_reference,
            external_body_reference_offset: wire.external_body_reference_offset,
            external_segment: wire.external_segment,
            external_segment_offset: wire.external_segment_offset,
            external_asset_id_offset: wire.external_asset_id_offset,
            external_link_name: wire.external_link_name,
            external_link_name_offset: wire.external_link_name_offset,
            external_version,
            tail_values: wire.tail_values,
            tail_value_offsets: wire.tail_value_offsets,
        })
    }
}

impl From<DesignCombineExternalBodyIdentity> for DesignCombineExternalBodyIdentityWire {
    fn from(record: DesignCombineExternalBodyIdentity) -> Self {
        Self {
            selector_asset_id: record.selector_asset_id.clone(),
            selector_asset_id_offset: record.selector_asset_id_offset,
            selector_context_id: record.selector_context_id,
            selector_context_id_offset: record.selector_context_id_offset,
            occurrence_reference: record.occurrence_reference,
            occurrence_reference_offset: record.occurrence_reference_offset,
            external_body_reference: record.external_body_reference,
            external_body_reference_offset: record.external_body_reference_offset,
            external_segment: record.external_segment,
            external_segment_offset: record.external_segment_offset,
            external_asset_id: record.selector_asset_id,
            external_asset_id_offset: record.external_asset_id_offset,
            external_link_name: record.external_link_name,
            external_link_name_offset: record.external_link_name_offset,
            external_property_key: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.value.clone()),
            external_property_key_offset: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.offset),
            external_version_urn: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.value.clone()),
            external_version_urn_offset: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.offset),
            tail_values: record.tail_values,
            tail_value_offsets: record.tail_value_offsets,
        }
    }
}

/// One target or tool body selector owned by a `Combine` operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignCombineBodySelection {
    /// Body-selection record index.
    pub record_index: u32,
    /// Complete external body identity when the selector crosses a document boundary.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_identity"
    )]
    pub external_identity: Option<DesignCombineExternalBodyIdentity>,
}

/// Exact Boolean construction carried by a `Combine` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCombineOperationWire",
    into = "DesignCombineOperationWire"
)]
pub struct DesignCombineOperation {
    /// Serialized scope-prologue form.
    pub form: DesignCombineForm,
    /// Join, cut, or intersect operation.
    pub operation: cadmpeg_ir::features::BooleanKind,
    /// Byte offset of the operation u32.
    pub operation_offset: u64,
    /// Whether the source operation retains its tool bodies.
    pub keep_tools: bool,
    /// Byte offset of the keep-tools Boolean.
    pub keep_tools_offset: u64,
    /// Boolean target body selector.
    pub target_record_index: u32,
    /// Boolean tool body selectors in source order.
    pub tools: DesignCombineTools,
}

/// Ordered nonempty tools of a Combine operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignCombineTools {
    pub first: DesignCombineBodySelection,
    pub additional: Vec<DesignCombineBodySelection>,
}

impl DesignCombineTools {
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &DesignCombineBodySelection> {
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
    /// Byte offset of the operation u32.
    operation_offset: u64,
    /// Whether the source operation retains its tool bodies.
    keep_tools: bool,
    /// Byte offset of the keep-tools Boolean.
    keep_tools_offset: u64,
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
            operation_offset: wire.operation_offset,
            keep_tools: wire.keep_tools,
            keep_tools_offset: wire.keep_tools_offset,
            target_record_index: wire.target.record_index,
            tools: DesignCombineTools {
                first,
                additional: tools.collect(),
            },
        })
    }
}

impl From<DesignCombineOperation> for DesignCombineOperationWire {
    fn from(operation: DesignCombineOperation) -> Self {
        Self {
            form: operation.form,
            operation: operation.operation,
            operation_offset: operation.operation_offset,
            keep_tools: operation.keep_tools,
            keep_tools_offset: operation.keep_tools_offset,
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
