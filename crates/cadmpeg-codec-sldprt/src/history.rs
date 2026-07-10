// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

use crate::container::ContainerScan;
use cadmpeg_ir::history::{Configuration, Feature, FeatureHistory};
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};
use std::collections::BTreeMap;

pub fn histories(scan: &ContainerScan) -> Vec<FeatureHistory> {
    scan.blocks
        .iter()
        .filter_map(|block| {
            let text = xml_text(&block.payload)?;
            let doc = roxmltree::Document::parse(&text).ok()?;
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                return None;
            }
            let meta = |offset, tag: &str| EntityMeta {
                provenance: Provenance {
                    format: "sldprt".into(),
                    stream: block
                        .section
                        .clone()
                        .unwrap_or_else(|| format!("block@{}", block.offset)),
                    offset,
                    tag: Some(tag.into()),
                },
                exactness: Exactness::ByteExact,
            };
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .map(|node| Configuration {
                    name: node.attribute("Name").unwrap_or("").into(),
                    material: node
                        .attribute("Material")
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    properties: node
                        .attributes()
                        .filter(|attribute| !matches!(attribute.name(), "Name" | "Material"))
                        .map(|attribute| {
                            (attribute.name().to_string(), attribute.value().to_string())
                        })
                        .collect(),
                })
                .collect();
            let features = root
                .descendants()
                .filter(|node| {
                    node.is_element()
                        && !matches!(
                            node.tag_name().name(),
                            "Keywords" | "Configuration" | "Dimension"
                        )
                })
                .enumerate()
                .map(|(ordinal, node)| Feature {
                    source_id: node
                        .attribute("id")
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    parent_source_id: node
                        .ancestors()
                        .skip(1)
                        .find_map(|parent| parent.attribute("id").map(str::to_string)),
                    ordinal: ordinal as u32,
                    name: node.attribute("Name").unwrap_or("").into(),
                    kind: node
                        .attribute("Type")
                        .unwrap_or_else(|| node.tag_name().name())
                        .into(),
                    suppressed: node
                        .attribute("Suppressed")
                        .is_some_and(|value| matches!(value, "1" | "true" | "True")),
                    parameters: node
                        .children()
                        .filter(|child| {
                            child.is_element() && child.tag_name().name() == "Dimension"
                        })
                        .filter_map(|dimension| {
                            Some((
                                dimension.attribute("Name")?.into(),
                                dimension.text()?.trim().into(),
                            ))
                        })
                        .collect::<BTreeMap<_, _>>(),
                    properties: node
                        .attributes()
                        .filter(|attribute| {
                            !matches!(attribute.name(), "id" | "Name" | "Type" | "Suppressed")
                        })
                        .map(|attribute| {
                            (attribute.name().to_string(), attribute.value().to_string())
                        })
                        .collect(),
                    meta: meta(node.range().start as u64, node.tag_name().name()),
                })
                .collect();
            Some(FeatureHistory {
                part_name: root
                    .attribute("Name")
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                configurations,
                features,
                meta: meta(0, "Keywords"),
            })
        })
        .collect()
}

fn xml_text(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_prefix(&[0x86]).unwrap_or(bytes);
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        Some(String::from_utf16_lossy(&units))
    } else {
        std::str::from_utf8(bytes).ok().map(str::to_string)
    }
}
