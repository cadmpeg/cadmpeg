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
    let Ok(document) = serde_json::to_value(&ir.model) else {
        debug_assert!(false, "the CADIR model serializes");
        return Ok(losses);
    };
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
        for (name, xml) in &mut object.properties {
            let Ok(parsed) = roxmltree::Document::parse(xml) else {
                debug_assert!(false, "generated property XML parses");
                continue;
            };
            let mut missing = Vec::new();
            for node in parsed.descendants().filter(roxmltree::Node::is_element) {
                if !matches!(node.tag_name().name(), "Link" | "LinkSub" | "XLink")
                    || node.attribute("file").is_some_and(|file| !file.is_empty())
                {
                    continue;
                }
                let target = ["value", "name", "obj"]
                    .into_iter()
                    .find_map(|key| node.attribute_node(key));
                if let Some(target) = target
                    .filter(|target| !target.value().is_empty() && !live.contains(target.value()))
                {
                    missing.push((target.range_value(), target.value().to_owned()));
                }
            }
            for (range, target) in missing.into_iter().rev() {
                xml.replace_range(range, "");
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "object {} property {name} link to omitted object {target} is cleared",
                    object.name
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_link_clears_its_slot_and_keeps_siblings() {
        let mut owner = Object::new("Owner".into(), "App::Feature", "Owner");
        owner.set(
            "Links",
            r#"<Property name="Links" type="App::PropertyLinkList"><LinkList count="3"><Link value="First"/><Link value="Missing"/><Link value="Second"/></LinkList></Property>"#.into(),
        );
        let mut objects = vec![
            owner,
            Object::new("First".into(), "App::Feature", "First"),
            Object::new("Second".into(), "App::Feature", "Second"),
        ];
        let mut losses = Vec::new();
        close(&mut objects, &mut losses).unwrap();
        let xml = roxmltree::Document::parse(&objects[0].properties["Links"]).unwrap();
        let list = xml
            .descendants()
            .find(|node| node.has_tag_name("LinkList"))
            .unwrap();
        assert_eq!(list.attribute("count"), Some("3"));
        assert_eq!(
            list.children()
                .filter(|node| node.has_tag_name("Link"))
                .map(|node| node.attribute("value").unwrap())
                .collect::<Vec<_>>(),
            ["First", "", "Second"]
        );
        assert_eq!(losses.len(), 1);
    }
}
