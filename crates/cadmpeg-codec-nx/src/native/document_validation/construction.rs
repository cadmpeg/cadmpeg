// SPDX-License-Identifier: Apache-2.0
//! Construction content admission without reconstructing scanner positions.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde_json::Value;

use super::values::*;

pub(super) fn validate(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
) -> Result<(), NativeConvertError> {
    for arena in [
        "feature_operation_records",
        "feature_construction_named_targets",
        "feature_swp104_leading_branches",
        "feature_enlarge_construction_envelopes",
        "feature_extract_face_construction_envelopes",
        "feature_symbolic_threads",
        "feature_operation_body_writes",
        "feature_unlabeled_operation_body_writes",
        "assembly_placements",
        "rmfastload_physical_storages",
    ] {
        for record in namespace.arena_iter_as_for_decode::<Value>(ctx, arena) {
            let record = record?;
            match arena {
                "feature_operation_records" => operation_record(&record)?,
                "feature_construction_named_targets" => named_target(namespace, &record)?,
                "feature_swp104_leading_branches" => swp104(namespace, &record)?,
                "feature_enlarge_construction_envelopes" => enlarge(&record)?,
                "feature_extract_face_construction_envelopes" => extract_face(&record)?,
                "feature_symbolic_threads" => symbolic_thread(namespace, &record)?,
                "feature_operation_body_writes" | "feature_unlabeled_operation_body_writes" => {
                    body_write(
                        namespace,
                        &record,
                        arena == "feature_unlabeled_operation_body_writes",
                    )?
                }
                "assembly_placements" => super::values::placement(&record)?,
                "rmfastload_physical_storages" => super::values::storage(&record)?,
                _ => return Err(fail(&record, "unknown construction arena")),
            }
            if !matches!(
                arena,
                "assembly_placements"
                    | "rmfastload_physical_storages"
                    | "feature_unlabeled_operation_body_writes"
            ) {
                link(
                    namespace,
                    &record,
                    "operation_label",
                    "feature_operation_labels",
                )?;
            } else {
                optional_link(
                    namespace,
                    &record,
                    "operation_label",
                    "feature_operation_labels",
                )?;
            }
        }
    }
    Ok(())
}

fn operation_record(record: &Value) -> Result<(), NativeConvertError> {
    uint(record, "ordinal", u64::from(u32::MAX))?;
    let length = uint(record, "byte_len", u64::MAX)?;
    let payload_length = uint(record, "payload_byte_len", u64::MAX)?;
    field::<cadmpeg_ir::hash::digest::Sha256Digest>(record, "sha256")?;
    let hash: cadmpeg_ir::hash::digest::Sha256Digest = field(record, "payload_sha256")?;
    if length < payload_length {
        return Err(fail(record, "byte_len must contain payload_byte_len"));
    }
    if record.get("core").is_some() && record.get("linked_face_payload").is_some() {
        return Err(fail(
            record,
            "linked_face_payload/core: two representations cannot own one payload",
        ));
    }
    let core: Option<crate::om::operation_core::OperationCore> = record
        .get("core")
        .map(|_| field(record, "core"))
        .transpose()?;
    let linked: Option<crate::om::linked_face::LinkedFacePayload> = record
        .get("linked_face_payload")
        .map(|_| field(record, "linked_face_payload"))
        .transpose()?;
    for bytes in [
        core.as_ref()
            .map(crate::om::operation_core::OperationCore::as_bytes),
        linked
            .as_ref()
            .map(crate::om::linked_face::LinkedFacePayload::as_bytes),
    ]
    .into_iter()
    .flatten()
    {
        if cadmpeg_core::decode::u64_from_index(bytes.len()) != payload_length {
            return Err(fail(
                record,
                "core/linked_face_payload: complete payload lengths disagree",
            ));
        }
        if cadmpeg_ir::hash::digest::Sha256Digest::digest(bytes) != hash {
            return Err(fail(
                record,
                "core/linked_face_payload: bytes disagree with payload_sha256",
            ));
        }
    }
    if record.get("stable_identity").is_some()
        && !record["stable_identity"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    {
        return Err(fail(record, "stable_identity must be nonempty text"));
    }
    Ok(())
}

fn named_target(namespace: &NativeNamespace, record: &Value) -> Result<(), NativeConvertError> {
    let value: crate::printable_string::PrintableString<String> = field(record, "value")?;
    if value.as_str().len() > 253 {
        return Err(fail(
            record,
            "value: must fit the one-byte canonical name length",
        ));
    }
    uint(record, "ordinal", u64::from(u32::MAX))?;
    data_block(namespace, record)?;
    let arena = match record["construction_kind"].as_str() {
        Some("pattern_payload") => "feature_pattern_construction_payloads",
        Some("extrude_profile") => "feature_extrude_construction_profiles",
        _ => {
            return Err(fail(
                record,
                "construction_kind is not a named-target owner",
            ))
        }
    };
    let owner: String = field(record, "construction")?;
    let owner = unique(namespace, arena, &owner)?;
    let owner = owner.fields()?;
    if owner.get("operation_label") != record.get("operation_label") {
        return Err(fail(
            record,
            "construction owner and named target must share an operation_label",
        ));
    }
    let blocks = owner
        .get("data_blocks")
        .and_then(Value::as_array)
        .ok_or_else(|| fail(record, "construction owner must retain ordered data_blocks"))?;
    for block in blocks {
        unique(
            namespace,
            "data_blocks",
            block
                .as_str()
                .ok_or_else(|| fail(record, "data_blocks must contain identities"))?,
        )?;
    }
    if record.get("data_block").is_some() {
        let object_index = index(record)?;
        let mut candidates = Vec::new();
        for candidate in namespace.arenas().get("data_blocks").into_iter().flatten() {
            let fields = candidate.fields()?;
            if fields.get("role").and_then(Value::as_str) == Some("column")
                && fields.get("block_ordinal").and_then(Value::as_u64) == Some(object_index)
            {
                candidates.push(candidate.id());
            }
        }
        if candidates.as_slice()
            != [record["data_block"]
                .as_str()
                .ok_or_else(|| fail(record, "data_block must name a target"))?]
        {
            return Err(fail(
                record,
                "data_block must uniquely match the named target object_index across stores",
            ));
        }
    }
    Ok(())
}

fn swp104(namespace: &NativeNamespace, record: &Value) -> Result<(), NativeConvertError> {
    nonzero_byte(record, "parameter_class_index")?;
    nonzero_byte(record, "profile_class_index")?;
    field::<bool>(record, "leading_zero")?;
    for value in sized_array(record, "scalars", 4)? {
        finite(record, value, "scalars")?;
    }
    let members = array(record, "members")?;
    if !(1..=254).contains(&members.len())
        || uint(record, "declared_count", 255)?
            != cadmpeg_core::decode::u64_from_index(members.len()) + 1
    {
        return Err(fail(
            record,
            "declared_count must equal members length plus one",
        ));
    }
    let mut previous = None;
    for (ordinal, item) in members
        .iter()
        .chain(std::iter::once(&record["successor"]))
        .enumerate()
    {
        if uint(item, "ordinal", u64::from(u32::MAX))?
            != cadmpeg_core::decode::u64_from_index(ordinal)
        {
            return Err(fail(
                record,
                "members/successor ordinal does not match serialized order",
            ));
        }
        let current = index(item)?;
        if current == 0 || previous.is_some_and(|index| index + 1 != current) {
            return Err(fail(
                record,
                "members and successor must be non-null consecutive column indices",
            ));
        }
        previous = Some(current);
        data_block(namespace, item)?;
    }
    if record.get("witnessed_count").is_some() && uint(record, "witnessed_count", 255)? < 2 {
        return Err(fail(record, "witnessed_count must be at least two"));
    }
    Ok(())
}

fn enlarge(record: &Value) -> Result<(), NativeConvertError> {
    nonzero_byte(record, "parameter_class_index")?;
    for reference in sized_array(record, "references", 6)? {
        index(reference)?;
    }
    index(&record["middle_index"])?;
    for value in sized_array(record, "scalars", 4)? {
        finite(record, value, "scalars")?;
    }
    Ok(())
}

fn extract_face(record: &Value) -> Result<(), NativeConvertError> {
    nonzero_byte(record, "parameter_class_index")?;
    field::<[u32; 4]>(record, "control_words")?;
    for lane in sized_array(record, "lanes", 2)? {
        let lane = lane
            .as_array()
            .filter(|lane| lane.len() <= 254)
            .ok_or_else(|| fail(record, "EXTRACT_FACE lane must fit its nonzero count byte"))?;
        for member in lane {
            uint(member, "key", 255)?;
            index(&member["left"])?;
            index(&member["right"])?;
        }
    }
    index(&record["terminal"])?;
    let text: String = field(record, "navigator_text")?;
    if text.len() > 253 {
        return Err(fail(
            record,
            "EXTRACT_FACE navigator text must fit its length byte",
        ));
    }
    Ok(())
}

fn symbolic_thread(namespace: &NativeNamespace, record: &Value) -> Result<(), NativeConvertError> {
    link(
        namespace,
        record,
        "operation_record",
        "feature_operation_records",
    )?;
    for frame in array(record, "text_frames")? {
        field::<crate::payload_text::PayloadText<String>>(frame, "value")?;
        uint(frame, "ordinal", u64::from(u32::MAX))?;
        if uint(frame, "marker", 255)? != 3 {
            return Err(fail(
                record,
                "marker must be 3 for a symbolic-thread text frame",
            ));
        }
        if frame["symbolic_thread"] != record["id"] {
            return Err(fail(
                record,
                "symbolic_thread text frame names a different owner",
            ));
        }
    }
    if let Some(construction) = record.get("construction") {
        if construction.is_null() {
            return Err(fail(
                record,
                "construction: this key states a value; omit it to state absence",
            ));
        }
        nonzero_byte(construction, "parameter_class_index")?;
        for coordinate in sized_array(construction, "coordinates", 3)? {
            finite(record, &coordinate["value"], "coordinates")?;
        }
        for reference in sized_array(construction, "tail_references", 3)? {
            index(reference)?;
        }
        field::<bool>(construction, "tail_sentinel")?;
    }
    Ok(())
}

fn body_write(
    namespace: &NativeNamespace,
    record: &Value,
    unlabeled: bool,
) -> Result<(), NativeConvertError> {
    link(
        namespace,
        record,
        "operation_record",
        if unlabeled {
            "feature_unlabeled_operation_records"
        } else {
            "feature_operation_records"
        },
    )?;
    optional_link(namespace, record, "body_image_data_block", "data_blocks")?;
    uint(record, "ordinal", u64::from(u32::MAX))?;
    uint(record, "body_identity", 255)?;
    uint(record, "group_node", 0x0f_ffff)?;
    nonzero_byte(record, "endpoint_class_index")?;
    let image = uint(record, "body_image_object_index", 0x0f_ffff)?;
    if let Some(id) = record["body_image_data_block"].as_str() {
        let block = unique(namespace, "data_blocks", id)?.fields()?;
        if block.get("block_ordinal").and_then(Value::as_u64) != Some(image) {
            return Err(fail(
                record,
                "body_image_data_block disagrees with body_image_object_index",
            ));
        }
    }
    Ok(())
}
