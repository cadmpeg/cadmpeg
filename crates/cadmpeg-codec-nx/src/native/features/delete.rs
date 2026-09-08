// SPDX-License-Identifier: Apache-2.0
//! Native DELETE reference fields and construction payloads.

use super::payload_content::{FeaturePayloadBlock, FeaturePayloadContent};
use super::{
    offset_data_block_bytes, unique_offset_data_block, visit_feature_history_operation_records,
};
use crate::container::Container;
use crate::om::delete_references::DeleteReferences;
use crate::om::reference_index::PayloadIndexToken;
use serde::{Deserialize, Serialize};

/// Exact counted nullable reference field carried by a `DELETE` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DeleteReferenceFieldWire",
    into = "DeleteReferenceFieldWire"
)]
pub struct FeatureDeleteReferenceField {
    /// Globally unique field identity.
    pub id: String,
    /// Owning `DELETE` operation label.
    pub operation_label: String,
    pub references: DeleteReferences<Option<String>>,
}

/// Exact logical payload reconstructed from a complete non-null `DELETE` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDeleteConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DELETE` operation label.
    pub operation_label: String,
    /// Complete five-slot reference field selecting the source blocks.
    pub reference_field: String,
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    pub content: FeaturePayloadContent<[FeaturePayloadBlock; 5]>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DeleteReferenceFieldWire {
    id: String,
    operation_label: String,
    control: u8,
    object_indices: [Option<u32>; 5],
    raw_object_indices: [Vec<u8>; 5],
    data_blocks: [Option<String>; 5],
    source_offset: u64,
    object_index_source_offsets: [u64; 5],
}

impl From<FeatureDeleteReferenceField> for DeleteReferenceFieldWire {
    fn from(value: FeatureDeleteReferenceField) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            control: value.references.control(),
            object_indices: value
                .references
                .slots()
                .each_ref()
                .map(|slot| slot.as_ref().map(|target| target.0.value())),
            raw_object_indices: value.references.slots().each_ref().map(|slot| {
                slot.as_ref()
                    .map_or_else(|| vec![0xff], |target| target.0.raw().to_vec())
            }),
            data_blocks: value
                .references
                .slots()
                .each_ref()
                .map(|slot| slot.as_ref().and_then(|target| target.1.clone())),
            source_offset: value.references.offset(),
            object_index_source_offsets: value.references.reference_offsets(),
        }
    }
}

impl TryFrom<DeleteReferenceFieldWire> for FeatureDeleteReferenceField {
    type Error = String;

    fn try_from(wire: DeleteReferenceFieldWire) -> Result<Self, Self::Error> {
        let slots = std::array::from_fn::<_, 5, _>(|slot| {
            let target = match wire.object_indices[slot] {
                Some(index) => Some((
                    PayloadIndexToken::from_wire(index, &wire.raw_object_indices[slot]).map_err(
                        |error| format!("object_indices/raw_object_indices[{slot}]: {error}"),
                    )?,
                    wire.data_blocks[slot].clone(),
                )),
                None => {
                    if wire.raw_object_indices[slot] != [0xff] {
                        return Err(format!(
                            "raw_object_indices[{slot}]: null reference requires ff"
                        ));
                    }
                    if wire.data_blocks[slot].is_some() {
                        return Err(format!(
                            "data_blocks[{slot}]: null reference cannot have a target"
                        ));
                    }
                    None
                }
            };
            Ok::<_, String>(target)
        });
        let [first, second, third, fourth, fifth] = slots;
        let references = DeleteReferences::new(
            wire.source_offset,
            wire.control,
            [first?, second?, third?, fourth?, fifth?],
        )?;
        if references.reference_offsets() != wire.object_index_source_offsets {
            return Err(
                "object_index_source_offsets: must follow the DELETE header and token widths"
                    .into(),
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            references,
        })
    }
}

/// Decode exact `DELETE` payload reference fields and independently resolve
/// their non-null slots without assigning a target object family.
pub fn feature_delete_reference_fields(container: &Container) -> Vec<FeatureDeleteReferenceField> {
    let indexed = container.indexed_om_sections();
    let mut fields = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(field) = DeleteReferences::read(record.payload_view()) else {
                return;
            };
            let Ok(references) = field.resolve(entry_offset, |token| {
                unique_offset_data_block(&indexed, token.value())
            }) else {
                return;
            };
            fields.push(FeatureDeleteReferenceField {
                id: format!(
                    "nx:feature-history:delete-reference-field#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                references,
            });
        },
    );
    fields
}

/// Reconstruct one ordered logical payload from each complete same-store
/// non-null `DELETE` reference field.
pub fn feature_delete_construction_payloads(
    container: &Container,
    fields: &[FeatureDeleteReferenceField],
) -> Vec<FeatureDeleteConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    fields
        .iter()
        .filter_map(|field| {
            let data_blocks = field
                .references
                .slots()
                .iter()
                .map(|reference| reference.as_ref()?.1.clone())
                .collect::<Option<Vec<_>>>()?;
            let store = data_blocks.first()?.rsplit_once(":block#")?.0;
            if data_blocks.iter().any(|block| {
                block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != store)
            }) {
                return None;
            }
            let (_, content) = FeaturePayloadContent::from_source(data_blocks, &blocks)?;
            let operation_key = field
                .operation_label
                .strip_prefix("nx:feature-history:operation-label#")?;
            Some(FeatureDeleteConstructionPayload {
                id: format!("nx:feature-history:delete-construction-payload#{operation_key}"),
                operation_label: field.operation_label.clone(),
                reference_field: field.id.clone(),
                content,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::FeatureDeleteReferenceField;

    #[test]
    fn delete_wire_derives_mixed_width_and_null_positions() -> Result<(), Box<dyn std::error::Error>>
    {
        let json = r#"{"id":"delete","operation_label":"operation","control":255,"object_indices":[32,null,520,521,null],"raw_object_indices":[[240,32],[255],[241,2,8],[241,2,9],[255]],"data_blocks":["block-32",null,"block-520",null,null],"source_offset":100,"object_index_source_offsets":[107,109,110,113,116]}"#;
        let field: FeatureDeleteReferenceField = serde_json::from_str(json)?;
        assert_eq!(serde_json::to_string(&field)?, json);
        for (key, value) in [
            (
                "object_index_source_offsets",
                serde_json::json!([107, 109, 110, 114, 116]),
            ),
            ("source_offset", serde_json::json!(u64::MAX)),
            (
                "raw_object_indices",
                serde_json::json!([[32], [255], [241, 2, 8], [241, 2, 9], [255]]),
            ),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json)?;
            wire[key] = value;
            let error = serde_json::from_value::<FeatureDeleteReferenceField>(wire)
                .err()
                .ok_or("malformed DELETE field was accepted")?
                .to_string();
            assert!(error.contains(key), "{error}");
        }
        Ok(())
    }
}
