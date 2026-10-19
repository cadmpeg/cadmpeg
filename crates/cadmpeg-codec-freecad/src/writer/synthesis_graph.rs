// SPDX-License-Identifier: Apache-2.0
//! Close the output graph after individual source records have been omitted.

use super::xml::Object;
use crate::loss::FreecadLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashSet;

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
