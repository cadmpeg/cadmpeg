// SPDX-License-Identifier: Apache-2.0
//! Surface allocation identities and complete ordered construction graphs.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde_json::Value;

use super::values::*;

pub(super) fn validate(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
) -> Result<(), NativeConvertError> {
    let payloads =
        namespace.arena_as_for_decode::<Value>(ctx, "feature_surface_construction_payloads")?;
    if payloads.is_empty() {
        return Ok(());
    }
    let references =
        namespace.arena_as_for_decode::<Value>(ctx, "feature_surface_construction_references")?;
    let post =
        namespace.arena_as_for_decode::<Value>(ctx, "feature_studio_post_branch_references")?;
    let branches =
        namespace.arena_as_for_decode::<Value>(ctx, "feature_surface_construction_branches")?;
    for payload in payloads {
        let operation: String = field(&payload, "operation_label")?;
        let label = unique(namespace, "feature_operation_labels", &operation)?.fields()?;
        let kind: String = field(&payload, "operation_kind")?;
        let expected = match kind.as_str() {
            "skin" => "SKIN",
            "studio_surface" => "Studio Surface",
            _ => {
                return Err(fail(
                    &payload,
                    "surface allocation operation_kind is not SKIN or Studio Surface",
                ))
            }
        };
        if label.get("value").and_then(Value::as_str) != Some(expected) {
            return Err(fail(
                &payload,
                "surface allocation operation_kind disagrees with operation_label",
            ));
        }
        if references
            .iter()
            .filter(|item| item["operation_label"] == operation)
            .count()
            != 14
        {
            return Err(fail(
                &payload,
                "surface allocation requires fourteen owned construction references",
            ));
        }
        let mut rows = Vec::new();
        for (ordinal, id) in sized_array(&payload, "construction_references", 14)?
            .iter()
            .enumerate()
        {
            rows.push(reference(namespace, &payload, &references, id, ordinal)?);
        }
        let owned = array(&payload, "construction_branches")?;
        if !(1..=255).contains(&owned.len())
            || branches
                .iter()
                .filter(|item| item["operation_label"] == operation)
                .count()
                != owned.len()
        {
            return Err(fail(
                &payload,
                "construction_branches must contain every owned branch exactly once",
            ));
        }
        for (ordinal, id) in owned.iter().enumerate() {
            let branch = select(&payload, &branches, id)?;
            if branch["operation_label"] != operation
                || uint(branch, "ordinal", 254)? != cadmpeg_core::decode::u64_from_index(ordinal)
            {
                return Err(fail(
                    &payload,
                    "construction_branches owner or ordinal disagrees",
                ));
            }
            field::<crate::om::surface_branches::SurfaceFamily>(branch, "family")?;
            field::<crate::om::discriminators::SurfaceBranchMode>(branch, "mode")?;
            field::<bool>(branch, "witnessed")?;
            uint(branch, "header_code", 255)?;
            let members = array(branch, "members")?;
            if !(1..=254).contains(&members.len())
                || uint(branch, "declared_count", 255)?
                    != cadmpeg_core::decode::u64_from_index(members.len()) + 1
            {
                return Err(fail(
                    &payload,
                    "declared_count must equal members length plus one",
                ));
            }
            for (ordinal, reference) in members
                .iter()
                .chain(std::iter::once(&branch["terminal"]))
                .enumerate()
            {
                if uint(reference, "ordinal", u64::from(u32::MAX))?
                    != cadmpeg_core::decode::u64_from_index(ordinal)
                {
                    return Err(fail(
                        &payload,
                        "branch member/terminal ordinal disagrees with its order",
                    ));
                }
                rows.push(block(namespace, &payload, reference)?);
            }
        }
        let post_count = if expected == "Studio Surface" { 8 } else { 0 };
        if post
            .iter()
            .filter(|item| item["operation_label"] == operation)
            .count()
            != post_count
        {
            return Err(fail(
                &payload,
                "surface allocation post-branch reference count disagrees",
            ));
        }
        if post_count != 0 {
            for (ordinal, id) in sized_array(&payload, "post_branch_references", post_count)?
                .iter()
                .enumerate()
            {
                rows.push(reference(namespace, &payload, &post, id, ordinal)?);
            }
        } else if payload.get("post_branch_references").is_some() {
            return Err(fail(&payload, "SKIN cannot own post_branch_references"));
        }
        let ids = array(&payload, "data_blocks")?;
        let lengths: Vec<u64> = field(&payload, "block_byte_lengths")?;
        if ids.len() != rows.len() || lengths.len() != rows.len() {
            return Err(fail(&payload, "surface allocation must match its complete ordered reference graph and physical blocks"));
        }
        let mut total = 0u64;
        for ((id, length), (expected_id, token)) in ids.iter().zip(lengths).zip(rows) {
            if id.as_str() != Some(expected_id.as_str()) {
                return Err(fail(
                    &payload,
                    "data_blocks order disagrees with the complete construction graph",
                ));
            }
            let fields = unique(namespace, "data_blocks", &expected_id)?.fields()?;
            if fields.get("block_ordinal").and_then(Value::as_u64) != Some(token)
                || fields.get("byte_len").and_then(Value::as_u64) != Some(length)
            {
                return Err(fail(
                    &payload,
                    "data_blocks object index or block_byte_lengths disagree",
                ));
            }
            total = total
                .checked_add(length)
                .ok_or_else(|| fail(&payload, "block_byte_lengths overflow byte_len"))?;
        }
        if uint(&payload, "byte_len", u64::MAX)? != total {
            return Err(fail(&payload, "block_byte_lengths disagree with byte_len"));
        }
        field::<cadmpeg_ir::hash::digest::Sha256Digest>(&payload, "sha256")?;
        consecutive(&payload, ids)?;
    }
    Ok(())
}

fn select<'a>(
    payload: &Value,
    records: &'a [Value],
    id: &Value,
) -> Result<&'a Value, NativeConvertError> {
    let id = id
        .as_str()
        .ok_or_else(|| fail(payload, "construction graph identity is not text"))?;
    let mut matches = records.iter().filter(|record| record["id"] == id);
    let record = matches
        .next()
        .ok_or_else(|| fail(payload, "construction graph reference does not resolve"))?;
    if matches.next().is_some() {
        return Err(fail(payload, "construction graph identity is duplicated"));
    }
    Ok(record)
}

fn reference(
    namespace: &NativeNamespace,
    payload: &Value,
    records: &[Value],
    id: &Value,
    ordinal: usize,
) -> Result<(String, u64), NativeConvertError> {
    let reference = select(payload, records, id)?;
    if reference["operation_label"] != payload["operation_label"]
        || uint(reference, "ordinal", u64::from(u32::MAX))?
            != cadmpeg_core::decode::u64_from_index(ordinal)
    {
        return Err(fail(
            payload,
            "construction reference owner or ordinal disagrees",
        ));
    }
    block(namespace, payload, reference)
}

fn block(
    namespace: &NativeNamespace,
    payload: &Value,
    reference: &Value,
) -> Result<(String, u64), NativeConvertError> {
    let id: String = field(reference, "data_block")?;
    let object_index = index(reference)?;
    let fields = unique(namespace, "data_blocks", &id)?.fields()?;
    if fields.get("block_ordinal").and_then(Value::as_u64) != Some(object_index) {
        return Err(fail(
            payload,
            "construction data_block disagrees with object_index",
        ));
    }
    Ok((id, object_index))
}

fn consecutive(payload: &Value, ids: &[Value]) -> Result<(), NativeConvertError> {
    let first = ids
        .first()
        .and_then(Value::as_str)
        .and_then(|id| id.rsplit_once(":block#"))
        .and_then(|(store, index)| index.parse::<u32>().ok().map(|index| (store, index)))
        .filter(|(_, index)| *index != 0)
        .ok_or_else(|| {
            fail(
                payload,
                "data_blocks must name one consecutive positive object-index run",
            )
        })?;
    for (ordinal, id) in ids.iter().enumerate() {
        let index = u32::try_from(ordinal)
            .ok()
            .and_then(|ordinal| first.1.checked_add(ordinal))
            .ok_or_else(|| fail(payload, "data_blocks object-index run overflows"))?;
        if id.as_str() != Some(format!("{}:block#{index}", first.0).as_str()) {
            return Err(fail(
                payload,
                "data_blocks must name one consecutive positive object-index run",
            ));
        }
    }
    Ok(())
}
