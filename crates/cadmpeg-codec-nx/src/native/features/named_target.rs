// SPDX-License-Identifier: Apache-2.0
//! Named references owned by complete pattern payloads and extrusion profiles.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use super::construction_records::charged_unique_offset_data_block;
use super::joined_payload::JoinedPayload;
use super::{
    offset_data_block_bytes, FeatureConstructionPayload, FeatureExtrudeConstructionProfile,
};
use crate::container::Container;
use crate::om::named_target::NamedTargetReference;
use crate::printable_string::PrintableString;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "construction_kind",
    content = "construction",
    rename_all = "snake_case"
)]
enum ConstructionOwner<S> {
    PatternPayload(S),
    ExtrudeProfile(S),
}

/// Complete named-target envelope; a shared target does not assign a seed role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NamedTargetWire")]
pub(in crate::native) struct FeatureConstructionNamedTarget {
    id: String,
    operation_label: String,
    #[serde(flatten)]
    owner: ConstructionOwner<String>,
    ordinal: u32,
    value: PrintableString<String>,
    #[serde(flatten)]
    reference: NamedTargetReference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_block: Option<String>,
    payload_offset: u64,
    source_offsets: [u64; 3],
}

#[derive(Deserialize)]
struct NamedTargetWire {
    id: String,
    operation_label: String,
    #[serde(flatten)]
    owner: ConstructionOwner<String>,
    ordinal: u32,
    value: PrintableString<String>,
    #[serde(flatten)]
    reference: NamedTargetReference,
    #[serde(default, deserialize_with = "deserialize_data_block")]
    data_block: Option<String>,
    payload_offset: u64,
    source_offsets: [u64; 3],
}

cadmpeg_core::named_optional_field!(deserialize_data_block, String, "data_block");

impl TryFrom<NamedTargetWire> for FeatureConstructionNamedTarget {
    type Error = &'static str;

    fn try_from(wire: NamedTargetWire) -> Result<Self, Self::Error> {
        if wire.value.as_str().len() > 253 {
            return Err("value: must fit the one-byte canonical name length");
        }
        if wire.reference.value() == 0 && wire.data_block.is_some() {
            return Err("data_block: control index zero is not a target record");
        }
        wire.payload_offset
            .checked_add(
                12 + u64::from(wire.reference.byte_len())
                    + u64_from_index(wire.value.as_str().len()),
            )
            .ok_or("payload_offset: named-target span overflow")?;
        for offset in wire.source_offsets {
            offset
                .checked_add(1)
                .ok_or("source_offsets: marker span overflow")?;
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            owner: wire.owner,
            ordinal: wire.ordinal,
            value: wire.value,
            reference: wire.reference,
            data_block: wire.data_block,
            payload_offset: wire.payload_offset,
            source_offsets: wire.source_offsets,
        })
    }
}

impl FeatureConstructionNamedTarget {
    pub(in crate::native) fn validate_links(
        &self,
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::native::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::native::NativeConvertError> {
        use cadmpeg_ir::native::NativeConvertError;
        let (arena, id) = match &self.owner {
            ConstructionOwner::PatternPayload(id) => ("feature_pattern_construction_payloads", id),
            ConstructionOwner::ExtrudeProfile(id) => ("feature_extrude_construction_profiles", id),
        };
        let owner = unique_record(ctx, namespace, arena, id)?;
        if owner
            .fields()
            .get("operation_label")
            .and_then(serde_json::Value::as_str)
            != Some(self.operation_label.as_str())
        {
            return Err(NativeConvertError::InvalidCollection(
                "construction owner and named target must share an operation_label".into(),
            ));
        }
        let blocks = owner
            .fields()
            .get("data_blocks")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                NativeConvertError::InvalidCollection(
                    "construction owner must retain ordered data_blocks".into(),
                )
            })?;
        let markers = [
            self.payload_offset,
            self.payload_offset + 5,
            self.payload_offset + 7 + u64::from(self.reference.byte_len()),
        ];
        let mut matched = [false; 3];
        let mut cursor = 0_u64;
        for block in blocks {
            let id = block.as_str().ok_or_else(|| {
                NativeConvertError::InvalidCollection("data_blocks must contain identities".into())
            })?;
            let row = unique_record(ctx, namespace, "data_blocks", id)?;
            let position = ["source_offset", "byte_len"]
                .map(|field| row.fields().get(field).and_then(serde_json::Value::as_u64));
            let [Some(source), Some(length)] = position else {
                return Err(NativeConvertError::InvalidCollection(
                    "construction data block requires its physical span".into(),
                ));
            };
            let end = cursor.checked_add(length).ok_or_else(|| {
                NativeConvertError::InvalidCollection("construction payload span overflow".into())
            })?;
            if source.checked_add(length).is_none() {
                return Err(NativeConvertError::InvalidCollection(
                    "construction data block span overflow".into(),
                ));
            }
            for (slot, marker) in markers.iter().enumerate() {
                if *marker >= cursor && *marker < end {
                    if self.source_offsets[slot] != source + (*marker - cursor) {
                        return Err(NativeConvertError::InvalidCollection(
                            "source_offsets must follow the construction block order".into(),
                        ));
                    }
                    matched[slot] = true;
                }
            }
            cursor = end;
        }
        let envelope_end = self.payload_offset
            + 12
            + u64::from(self.reference.byte_len())
            + u64_from_index(self.value.as_str().len());
        if !matched.into_iter().all(|value| value) || envelope_end > cursor {
            return Err(NativeConvertError::InvalidCollection(
                "named-target envelope must fit its construction payload".into(),
            ));
        }
        if let Some(id) = &self.data_block {
            let target = unique_record(ctx, namespace, "data_blocks", id)?;
            let rows = namespace
                .arenas()
                .get("data_blocks")
                .map_or(&[][..], Vec::as_slice);
            ctx.charge_work(u64_from_index(rows.len()), "resolve NX named-target store")?;
            let mut candidates = rows.iter().filter(|row| {
                row.fields().get("role").and_then(serde_json::Value::as_str) == Some("column")
                    && row
                        .fields()
                        .get("block_ordinal")
                        .and_then(serde_json::Value::as_u64)
                        == Some(u64::from(self.reference.value()))
            });
            if candidates.next().map(cadmpeg_ir::native::NativeRecord::id) != Some(target.id())
                || candidates.next().is_some()
            {
                return Err(NativeConvertError::InvalidCollection(
                    "data_block must uniquely match the named target object_index across stores"
                        .into(),
                ));
            }
        }
        Ok(())
    }
}

fn unique_record<'a>(
    ctx: &DecodeContext<'_>,
    namespace: &'a cadmpeg_ir::native::NativeNamespace,
    arena: &str,
    id: &str,
) -> Result<&'a cadmpeg_ir::native::NativeRecord, cadmpeg_ir::native::NativeConvertError> {
    use cadmpeg_ir::native::NativeConvertError;
    let rows = namespace.arenas().get(arena).map_or(&[][..], Vec::as_slice);
    ctx.charge_work(
        u64_from_index(rows.len()),
        "resolve NX named-target native link",
    )?;
    let mut matched = rows.iter().filter(|row| row.id() == id);
    let row = matched.next().ok_or_else(|| {
        NativeConvertError::InvalidCollection(
            "named-target reference must select one native record".into(),
        )
    })?;
    if matched.next().is_some() {
        return Err(NativeConvertError::InvalidCollection(
            "named-target reference is ambiguous".into(),
        ));
    }
    Ok(row)
}

pub(in crate::native) fn feature_construction_named_targets(
    ctx: &DecodeContext<'_>,
    container: &Container,
    patterns: &[FeatureConstructionPayload],
    profiles: &[FeatureExtrudeConstructionProfile],
) -> Result<Vec<FeatureConstructionNamedTarget>, CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let indexed = container.indexed_om_sections(ctx)?;
    let mut targets = Vec::new();
    let mut collect = |id: &str,
                       operation_label: &str,
                       owner: ConstructionOwner<&str>,
                       joined: &JoinedPayload<'_>|
     -> Result<(), CodecError> {
        for (ordinal, parsed) in crate::om::named_target::named_targets(ctx, joined.bytes())?
            .into_iter()
            .enumerate()
        {
            let positions = [
                parsed.offset,
                parsed.reference_offset(),
                parsed.name_offset(),
            ]
            .map(u64_from_index);
            let [Some(first), Some(reference), Some(name)] =
                positions.map(|offset| joined.source_offset(offset))
            else {
                continue;
            };
            let owner = match owner {
                ConstructionOwner::PatternPayload(value) => ConstructionOwner::PatternPayload(
                    ctx.copy_retained_text(value, "NX named-target payload owner")?,
                ),
                ConstructionOwner::ExtrudeProfile(value) => ConstructionOwner::ExtrudeProfile(
                    ctx.copy_retained_text(value, "NX named-target profile owner")?,
                ),
            };
            let target = FeatureConstructionNamedTarget {
                id: super::format_feature_child_id(ctx, id, "-named-target-", ordinal)?,
                operation_label: ctx
                    .copy_retained_text(operation_label, "NX named-target operation")?,
                owner,
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX named-target ordinal", 0, 1))?,
                value: PrintableString::new(
                    ctx.copy_retained_text(parsed.value.as_str(), "NX named-target name")?,
                )
                .map_err(CodecError::malformed)?,
                reference: parsed.reference,
                data_block: charged_unique_offset_data_block(
                    ctx,
                    &indexed,
                    parsed.reference.value(),
                )?,
                payload_offset: u64_from_index(parsed.offset),
                source_offsets: [first, reference, name],
            };
            ctx.reserve_vec(&mut targets, 1, "NX construction named targets")?;
            targets.push(target);
        }
        Ok(())
    };
    for payload in patterns {
        if let Some(joined) = JoinedPayload::from_source(ctx, payload.content.block_ids(), &blocks)?
        {
            collect(
                &payload.id,
                &payload.operation_label,
                ConstructionOwner::PatternPayload(&payload.id),
                &joined,
            )?;
        }
    }
    for profile in profiles {
        if let Some(joined) = JoinedPayload::from_source(
            ctx,
            profile
                .references
                .iter()
                .map(|reference| &reference.data_block),
            &blocks,
        )? {
            collect(
                &profile.id,
                &profile.operation_label,
                ConstructionOwner::ExtrudeProfile(&profile.id),
                &joined,
            )?;
        }
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::FeatureConstructionNamedTarget;

    const WIRE: &str = r#"{"id":"nx:test:target#0","operation_label":"nx:test:op#0","construction_kind":"pattern_payload","construction":"nx:test:payload#0","ordinal":0,"value":"Name","object_index":128,"raw_object_index":"8080","data_block":"nx:test:block#0","payload_offset":0,"source_offsets":[10,20,30]}"#;

    #[test]
    fn named_target_wire_preserves_canonical_reference_and_ownership() {
        let record: FeatureConstructionNamedTarget = serde_json::from_str(WIRE).unwrap();
        assert_eq!(
            serde_json::to_value(record).unwrap(),
            serde_json::from_str::<serde_json::Value>(WIRE).unwrap()
        );
        for (field, value) in [
            ("raw_object_index", serde_json::json!("900080")),
            ("value", serde_json::json!("")),
            ("value", serde_json::json!("x".repeat(254))),
            ("payload_offset", serde_json::json!(u64::MAX)),
            ("source_offsets", serde_json::json!([10, 20, u64::MAX])),
            ("construction_kind", serde_json::json!("nx:test:other#0")),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(WIRE).unwrap();
            wire[field] = value;
            assert!(serde_json::from_value::<FeatureConstructionNamedTarget>(wire).is_err());
        }
    }

    #[test]
    fn named_target_rejects_missing_owner_during_native_check() {
        let record: FeatureConstructionNamedTarget = serde_json::from_str(WIRE).unwrap();
        crate::test_support::with_decode_context(|ctx| {
            assert!(record
                .validate_links(ctx, &cadmpeg_ir::native::NativeNamespace::default())
                .is_err());
        });
    }

    #[test]
    fn named_target_optional_block_uses_omission_instead_of_null() {
        let mut wire: serde_json::Value = serde_json::from_str(WIRE).unwrap();
        wire.as_object_mut().unwrap().remove("data_block");
        let record: FeatureConstructionNamedTarget = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
        wire["data_block"] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<FeatureConstructionNamedTarget>(wire)
                .unwrap_err()
                .to_string()
                .contains("data_block: this key states a value")
        );
    }

    #[test]
    fn named_target_check_enforces_owner_block_order_and_target_identity() {
        crate::test_support::with_decode_context(|ctx| {
            let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
            namespace.set_arena(ctx, "feature_pattern_construction_payloads", &[
                serde_json::json!({"id":"nx:test:payload#0","operation_label":"nx:test:op#0","data_blocks":["nx:test:first#0","nx:test:second#0"]}),
            ]).unwrap();
            namespace.set_arena(ctx, "data_blocks", &[
                serde_json::json!({"id":"nx:test:first#0","role":"column","block_ordinal":1,"source_offset":10,"byte_len":6}),
                serde_json::json!({"id":"nx:test:second#0","role":"column","block_ordinal":2,"source_offset":100,"byte_len":12}),
                serde_json::json!({"id":"nx:test:block#0","role":"column","block_ordinal":128,"source_offset":200,"byte_len":1}),
            ]).unwrap();
            let mut wire: serde_json::Value = serde_json::from_str(WIRE).unwrap();
            wire["source_offsets"] = serde_json::json!([10, 15, 103]);
            let record: FeatureConstructionNamedTarget =
                serde_json::from_value(wire.clone()).unwrap();
            record.validate_links(ctx, &namespace).unwrap();
            for (field, value) in [
                ("operation_label", serde_json::json!("nx:test:other#0")),
                ("source_offsets", serde_json::json!([10, 15, 19])),
                ("payload_offset", serde_json::json!(1)),
                ("data_block", serde_json::json!("nx:test:second#0")),
            ] {
                let mut wrong = wire.clone();
                wrong[field] = value;
                let record: FeatureConstructionNamedTarget = serde_json::from_value(wrong).unwrap();
                assert!(record.validate_links(ctx, &namespace).is_err());
            }
            wire["object_index"] = serde_json::json!(0);
            wire["raw_object_index"] = serde_json::json!("00");
            assert!(
                serde_json::from_value::<FeatureConstructionNamedTarget>(wire.clone()).is_err()
            );
            wire.as_object_mut().unwrap().remove("data_block");
            wire["source_offsets"] = serde_json::json!([10, 15, 102]);
            let record: FeatureConstructionNamedTarget = serde_json::from_value(wire).unwrap();
            record.validate_links(ctx, &namespace).unwrap();
            let mut rows: Vec<serde_json::Value> = namespace.arenas()["data_blocks"]
                .iter()
                .map(|row| serde_json::to_value(row).unwrap())
                .collect();
            rows.push(serde_json::json!({"id":"nx:test:other-store#128","role":"column","block_ordinal":128,"source_offset":300,"byte_len":1}));
            namespace.set_arena(ctx, "data_blocks", &rows).unwrap();
            let mut wire: serde_json::Value = serde_json::from_str(WIRE).unwrap();
            wire["source_offsets"] = serde_json::json!([10, 15, 103]);
            let record: FeatureConstructionNamedTarget = serde_json::from_value(wire).unwrap();
            assert!(record.validate_links(ctx, &namespace).is_err());
        });
    }

    #[test]
    fn named_target_retention_refuses_before_unbudgeted_clone() {
        let record: FeatureConstructionNamedTarget = serde_json::from_str(WIRE).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(WIRE).unwrap(),
        );
    }
}
