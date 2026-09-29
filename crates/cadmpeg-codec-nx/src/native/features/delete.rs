// SPDX-License-Identifier: Apache-2.0
//! Native DELETE reference fields and construction payloads.

use super::payload_content::{FeaturePayloadBlock, FeaturePayloadContent};
use super::{
    charged_unique_offset_data_block, format_feature_history_id,
    offset_data_block_bytes, visit_feature_history_operation_records,
};
use crate::container::Container;
use crate::om::delete_references::DeleteReferences;
use crate::om::reference_index::PayloadIndexToken;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Exact counted nullable reference field carried by a `DELETE` payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DeleteReferenceFieldWire")]
pub(in crate::native) struct FeatureDeleteReferenceField {
    /// Globally unique field identity.
    pub(in crate::native) id: String,
    /// Owning `DELETE` operation label.
    pub(in crate::native) operation_label: String,
    references: DeleteReferences<Option<String>>,
}

/// Exact logical payload reconstructed from a complete non-null `DELETE` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct FeatureDeleteConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub(in crate::native) id: String,
    /// Owning `DELETE` operation label.
    pub(in crate::native) operation_label: String,
    /// Complete five-slot reference field selecting the source blocks.
    reference_field: String,
    /// Ordered source blocks and the hash of their concatenated bytes.
    #[serde(flatten)]
    content: FeaturePayloadContent<[FeaturePayloadBlock; 5]>,
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

impl Serialize for FeatureDeleteReferenceField {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        const NULL_TOKEN: &[u8] = &[0xff];
        let slots = self.references.slots();
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("control", &self.references.control())?;
        wire.serialize_entry(
            "object_indices",
            &slots
                .each_ref()
                .map(|slot| slot.as_ref().map(|target| target.0.value())),
        )?;
        wire.serialize_entry(
            "raw_object_indices",
            &slots
                .each_ref()
                .map(|slot| slot.as_ref().map_or(NULL_TOKEN, |target| target.0.raw())),
        )?;
        wire.serialize_entry(
            "data_blocks",
            &slots
                .each_ref()
                .map(|slot| slot.as_ref().and_then(|target| target.1.as_deref())),
        )?;
        wire.serialize_entry("source_offset", &self.references.offset())?;
        wire.serialize_entry(
            "object_index_source_offsets",
            &self.references.reference_offsets(),
        )?;
        wire.end()
    }
}

#[cfg(test)]
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
pub(in crate::native) fn feature_delete_reference_fields(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureDeleteReferenceField>, cadmpeg_core::CodecError> {
    let indexed = container.indexed_om_sections(ctx)?;
    let mut fields = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let projected =
                (|| -> Result<Option<FeatureDeleteReferenceField>, cadmpeg_core::CodecError> {
                    let Some(field) = DeleteReferences::read(record.payload_view()) else {
                        return Ok(None);
                    };
                    let Some(references) = field.resolve(entry_offset, |token| {
                        charged_unique_offset_data_block(ctx, &indexed, token.value())
                    })?
                    else {
                        return Ok(None);
                    };
                    let id = format_feature_history_id(
                        ctx,
                        "delete-reference-field",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    let operation_label = format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?;
                    ctx.charge_collection_items(1, "NX DELETE reference fields")?;
                    ctx.charge_retained(
                        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                            FeatureDeleteReferenceField,
                        >()),
                        "NX DELETE reference field",
                    )?;
                    fields.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit("allocate NX DELETE reference fields", 0, 1)
                    })?;
                    Ok(Some(FeatureDeleteReferenceField {
                        id,
                        operation_label,
                        references,
                    }))
                })();
            match projected {
                Ok(Some(field)) => fields.push(field),
                Ok(None) => {}
                Err(error) => failure = Some(error),
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(fields)
}

/// Reconstruct one ordered logical payload from each complete same-store
/// non-null `DELETE` reference field.
pub(in crate::native) fn feature_delete_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    fields: &[FeatureDeleteReferenceField],
) -> Result<Vec<FeatureDeleteConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let mut output = Vec::new();
    for field in fields {
        let Some(payload) = delete_construction_payload_from_field(ctx, field, &blocks)? else {
            continue;
        };
        ctx.charge_collection_items(1, "NX DELETE construction payloads")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                FeatureDeleteConstructionPayload,
            >()),
            "NX DELETE construction payload",
        )?;
        output.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX DELETE construction payloads", 0, 1)
        })?;
        output.push(payload);
    }
    Ok(output)
}

fn delete_construction_payload_from_field(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    field: &FeatureDeleteReferenceField,
    blocks: &BTreeMap<String, (&[u8], u64)>,
) -> Result<Option<FeatureDeleteConstructionPayload>, cadmpeg_core::CodecError> {
    let slots = field.references.slots();
    if slots.iter().any(|reference| {
        reference
            .as_ref()
            .and_then(|(_, block)| block.as_ref())
            .is_none()
    }) {
        return Ok(None);
    }
    let text_bytes = slots.iter().try_fold(0usize, |total, reference| {
        let length = reference
            .as_ref()
            .and_then(|(_, block)| block.as_ref())
            .map_or(0, String::len);
        total
            .checked_add(length)
            .ok_or_else(|| ctx.refuse_codec_limit("NX DELETE source block references", 0, 1))
    })?;
    let slot_bytes = slots
        .len()
        .checked_mul(std::mem::size_of::<String>())
        .and_then(|bytes| bytes.checked_add(text_bytes))
        .ok_or_else(|| ctx.refuse_codec_limit("NX DELETE source block references", 0, 1))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(slots.len()),
        "NX DELETE source block references",
    )?;
    let _source_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(slot_bytes),
        "NX DELETE source block references",
    )?;
    let mut data_blocks = Vec::new();
    data_blocks
        .try_reserve_exact(slots.len())
        .map_err(|_| ctx.refuse_codec_limit("allocate NX DELETE source block references", 0, 1))?;
    for reference in slots {
        let Some((_, Some(block))) = reference else {
            return Ok(None);
        };
        let mut id = String::new();
        id.try_reserve_exact(block.len()).map_err(|_| {
            ctx.refuse_codec_limit("allocate NX DELETE source block reference", 0, 1)
        })?;
        id.push_str(block);
        data_blocks.push(id);
    }
    let Some(store) = data_blocks
        .first()
        .and_then(|id| id.rsplit_once(":block#").map(|(store, _)| store))
    else {
        return Ok(None);
    };
    if data_blocks.iter().any(|block| {
        block
            .rsplit_once(":block#")
            .is_none_or(|(prefix, _)| prefix != store)
    }) {
        return Ok(None);
    }
    let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, blocks)? else {
        return Ok(None);
    };
    let Some(operation_key) = field
        .operation_label
        .strip_prefix("nx:feature-history:operation-label#")
    else {
        return Ok(None);
    };
    let prefix = "nx:feature-history:delete-construction-payload#";
    let id_len = prefix
        .len()
        .checked_add(operation_key.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX DELETE construction identity", 0, 1))?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(id_len),
        "NX DELETE construction identity",
    )?;
    let mut id = String::new();
    id.try_reserve_exact(id_len)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX DELETE construction identity", 0, 1))?;
    write!(&mut id, "{prefix}{operation_key}")
        .map_err(|_| ctx.refuse_codec_limit("write NX DELETE construction identity", 0, 1))?;
    Ok(Some(FeatureDeleteConstructionPayload {
        id,
        operation_label: ctx.copy_retained_text(&field.operation_label, "NX DELETE construction operation")?,
        reference_field: ctx.copy_retained_text(&field.id, "NX DELETE construction reference")?,
        content,
    }))
}

#[cfg(test)]
mod tests {
    use super::{DeleteReferenceFieldWire, FeatureDeleteReferenceField};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeMap;

    fn delete_construction_fixture() -> (
        FeatureDeleteReferenceField,
        BTreeMap<String, (&'static [u8], u64)>,
    ) {
        let field = serde_json::from_str(r#"{"id":"field","operation_label":"nx:feature-history:operation-label#0-0000000001","control":255,"object_indices":[32,33,34,35,36],"raw_object_indices":[[240,32],[240,33],[240,34],[240,35],[240,36]],"data_blocks":["nx:om-data-blocks-0:block#32","nx:om-data-blocks-0:block#33","nx:om-data-blocks-0:block#34","nx:om-data-blocks-0:block#35","nx:om-data-blocks-0:block#36"],"source_offset":100,"object_index_source_offsets":[107,109,111,113,115]}"#)
            .expect("complete DELETE field");
        let blocks = (32u32..=36)
            .map(|ordinal| {
                (
                    format!("nx:om-data-blocks-0:block#{ordinal}"),
                    (b"A".as_slice(), u64::from(ordinal)),
                )
            })
            .collect();
        (field, blocks)
    }

    fn delete_construction_limit_error(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> CodecError {
        let (field, blocks) = delete_construction_fixture();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        configure(&mut policy);
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test root");
        super::delete_construction_payload_from_field(&ctx, &field, &blocks)
            .expect_err("DELETE construction limit refusal")
    }

    #[test]
    fn delete_construction_refuses_collection_limit() {
        let error =
            delete_construction_limit_error(|policy| policy.limits.max_collection_items = 0);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn delete_construction_refuses_scoped_limit() {
        let error =
            delete_construction_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn delete_construction_refuses_retained_limit() {
        let error = delete_construction_limit_error(|policy| policy.limits.max_retained_bytes = 0);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
    }

    #[test]
    fn delete_construction_refuses_work_limit() {
        let error = delete_construction_limit_error(|policy| policy.limits.max_work_units = 0);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
    }

    #[test]
    fn delete_construction_preserves_complete_source_order() {
        let (field, blocks) = delete_construction_fixture();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test root");
        let payload = super::delete_construction_payload_from_field(&ctx, &field, &blocks)
            .expect("admitted source blocks")
            .expect("complete DELETE payload");
        assert_eq!(
            payload.id,
            "nx:feature-history:delete-construction-payload#0-0000000001"
        );
        assert_eq!(payload.content.blocks().len(), 5);
        assert_eq!(
            payload.content.blocks()[0].id,
            "nx:om-data-blocks-0:block#32"
        );
        assert_eq!(
            payload.content.blocks()[4].id,
            "nx:om-data-blocks-0:block#36"
        );
    }

    #[test]
    fn delete_reference_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let json = r#"{"id":"nx:feature:delete-reference#0","operation_label":"operation","control":255,"object_indices":[32,null,520,521,null],"raw_object_indices":[[240,32],[255],[241,2,8],[241,2,9],[255]],"data_blocks":["block-32",null,"block-520",null,null],"source_offset":100,"object_index_source_offsets":[107,109,110,113,116]}"#;
        let record: FeatureDeleteReferenceField = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&DeleteReferenceFieldWire::from(record.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }

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

    // The substring assertion above admits any message that names the key.
    // `raw_object_indices` is the case where the key appears only inside a
    // composite path, so this pins the whole spelling the route emits:
    // `TryFrom<DeleteReferenceFieldWire>` wraps the slot token error of
    // `PayloadIndexToken::from_wire` in `object_indices/raw_object_indices[{slot}]: `.
    #[test]
    fn a_refused_delete_slot_token_states_its_whole_path() -> Result<(), Box<dyn std::error::Error>>
    {
        let json = r#"{"id":"delete","operation_label":"operation","control":255,"object_indices":[32,null,520,521,null],"raw_object_indices":[[240,32],[255],[241,2,8],[241,2,9],[255]],"data_blocks":["block-32",null,"block-520",null,null],"source_offset":100,"object_index_source_offsets":[107,109,110,113,116]}"#;
        let mut wire: serde_json::Value = serde_json::from_str(json)?;
        wire["raw_object_indices"] =
            serde_json::json!([[32], [255], [241, 2, 8], [241, 2, 9], [255]]);

        let error = serde_json::from_value::<FeatureDeleteReferenceField>(wire)
            .err()
            .ok_or("malformed DELETE field was accepted")?
            .to_string();

        assert_eq!(
            error,
            "object_indices/raw_object_indices[0]: raw_object_index: \
             invalid payload reference token"
        );
        Ok(())
    }
}
