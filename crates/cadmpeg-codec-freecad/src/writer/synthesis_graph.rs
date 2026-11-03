// SPDX-License-Identifier: Apache-2.0
//! Close the output graph after individual source records have been omitted.

use super::xml::Object;
use crate::loss::FreecadLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::ExportLossCode;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, HashSet};

/// Name every neutral record the synthesis below cannot admit.
///
/// A record with a duplicate identity, or one whose typed references name
/// absent records, is dropped where it is consumed. Each such removal is a
/// W1 cascade removal, so this scan charges `export:native.records-omitted`
/// naming the record before dispatch (#232 §5.1(a), #217 §5.0). A document
/// whose references all resolve and whose identities are all distinct
/// charges nothing.
///
/// The identity universe comes from the serialized model itself, so every
/// arena the document holds is covered without a per-arena mirror.
pub(super) fn admission(
    ir: &CadIr,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let document = serde_json::to_value(&ir.model).map_err(CodecError::malformed)?;
    // Only the identity of each arena row counts. References echo record
    // identities inside nested objects; a deep walk would mistake every
    // referenced record for a duplicate.
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    if let serde_json::Value::Object(model) = &document {
        for records in model.values() {
            let serde_json::Value::Array(rows) = records else {
                continue;
            };
            for row in rows {
                if let serde_json::Value::String(id) = &row["id"] {
                    *counts.entry(id.as_str()).or_default() += 1;
                }
            }
        }
    }
    for (id, count) in &counts {
        if *count > 1 {
            losses.push(ExportLossCode::NativeRecordsOmitted.note(format!(
                "neutral record {id} occurs {count} times and its duplicates are omitted"
            )));
        }
    }
    let mut universe: HashSet<&str> = counts.keys().copied().collect();
    // Typed neutral references may name retained native records, which the
    // model walk does not cover; those resolve through the own native arenas.
    if let Some(namespace) = ir.native.namespace("fcstd") {
        for records in namespace.arenas().values() {
            for record in records {
                universe.insert(record.id());
            }
        }
    }
    for (owner, targets) in ir.neutral_needs(ctx)? {
        for target in &targets {
            if !universe.contains(target.as_str()) {
                losses.push(ExportLossCode::NativeRecordsOmitted.note(format!(
                    "neutral record {owner} refers to absent record {target} and is omitted"
                )));
            }
        }
    }
    Ok(losses)
}

pub(super) fn close(
    objects: &mut Vec<Object>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let live: HashSet<_> = objects.iter().map(|object| object.name.clone()).collect();
    for object in objects {
        let before = object.dependencies.len();
        object
            .dependencies
            .retain(|dependency| live.contains(dependency));
        if before != object.dependencies.len() {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "object {} dependencies on omitted records are removed",
                object.name
            )));
        }
        let mut omitted = Vec::new();
        for (name, xml) in &object.properties {
            let parsed = roxmltree::Document::parse(xml).map_err(CodecError::malformed)?;
            if parsed
                .descendants()
                .filter(roxmltree::Node::is_element)
                .any(|node| {
                    if !matches!(node.tag_name().name(), "Link" | "LinkSub" | "XLink")
                        || node.attribute("file").is_some_and(|file| !file.is_empty())
                    {
                        return false;
                    }
                    node.attribute("value")
                        .or_else(|| node.attribute("name"))
                        .or_else(|| node.attribute("obj"))
                        .is_some_and(|target| !target.is_empty() && !live.contains(target))
                })
            {
                omitted.push(name.clone());
            }
        }
        for name in omitted {
            object.properties.remove(&name);
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "object {} property {name} refers to an omitted record and is removed",
                object.name
            )));
        }
    }
    Ok(())
}
