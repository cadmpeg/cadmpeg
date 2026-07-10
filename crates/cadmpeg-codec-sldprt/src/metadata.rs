// SPDX-License-Identifier: Apache-2.0
//! Typed SW Objects document metadata.

use crate::container::ContainerScan;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};

fn f64_le(bytes: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

pub fn attributes(scan: &ContainerScan) -> Vec<SourceAttribute> {
    let mut out = Vec::new();
    for block in &scan.blocks {
        scan_vectors(
            block,
            b"moBBoxCenterData_c",
            "bounding_envelope",
            4,
            4,
            true,
            &mut out,
        );
        scan_vectors(
            block,
            b"moDefaultRefPlnData_c",
            "default_reference_plane",
            9,
            0,
            false,
            &mut out,
        );
        scan_part(block, &mut out);
        scan_configuration_manager(block, &mut out);
        scan_units_xml(block, &mut out);
    }
    out
}

fn scan_units_xml(block: &crate::container::Block, out: &mut Vec<SourceAttribute>) {
    let Some(text) = xml_text(&block.payload) else {
        return;
    };
    let Ok(document) = roxmltree::Document::parse(&text) else {
        return;
    };
    for node in document.descendants().filter(roxmltree::Node::is_element) {
        let value = if node.tag_name().name() == "SW_UnitsLinear" {
            node.text()
        } else if node.attribute("Name") == Some("SW_UnitsLinear") {
            node.attribute("Value").or_else(|| node.text())
        } else {
            node.attribute("SW_UnitsLinear")
        };
        let Some(code) = value.and_then(|value| value.trim().parse::<i64>().ok()) else {
            continue;
        };
        out.push(attribute(
            block,
            node.range().start,
            "source_linear_unit_code",
            b"SW_UnitsLinear",
            vec![AttributeValue::Integer(code)],
        ));
    }
}

fn xml_text(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_prefix(&[0x86]).unwrap_or(bytes);
    if bytes.starts_with(&[0xff, 0xfe]) {
        Some(String::from_utf16_lossy(
            &bytes[2..]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        ))
    } else {
        std::str::from_utf8(bytes).ok().map(str::to_string)
    }
}

fn scan_vectors(
    block: &crate::container::Block,
    token: &[u8],
    name: &str,
    count: usize,
    skip: usize,
    all_lengths: bool,
    out: &mut Vec<SourceAttribute>,
) {
    for offset in block
        .payload
        .windows(token.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == token).then_some(at))
    {
        let start = offset + token.len() + skip;
        let Some(values) = (0..count)
            .map(|index| f64_le(&block.payload, start + index * 8))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !values.iter().all(|value| value.is_finite()) {
            continue;
        }
        let values = if all_lengths {
            vec![AttributeValue::Vector(
                values.into_iter().map(|value| value * 1000.0).collect(),
            )]
        } else {
            vec![
                AttributeValue::Vector(values[..3].iter().map(|value| value * 1000.0).collect()),
                AttributeValue::Vector(values[3..].to_vec()),
            ]
        };
        out.push(attribute(block, offset, name, token, values));
    }
}

fn scan_part(block: &crate::container::Block, out: &mut Vec<SourceAttribute>) {
    const TOKEN: &[u8] = b"moPart_c";
    for offset in block
        .payload
        .windows(TOKEN.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == TOKEN).then_some(at))
    {
        let start = offset + TOKEN.len();
        let (Some(id), Some(version)) = (
            u32_le(&block.payload, start),
            u32_le(&block.payload, start + 8),
        ) else {
            continue;
        };
        out.push(attribute(
            block,
            offset,
            "part_record",
            TOKEN,
            vec![
                AttributeValue::Integer(id as i64),
                AttributeValue::Integer(version as i64),
            ],
        ));
    }
}

fn scan_configuration_manager(block: &crate::container::Block, out: &mut Vec<SourceAttribute>) {
    const TOKEN: &[u8] = b"moConfigurationMgr_c";
    for offset in block
        .payload
        .windows(TOKEN.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == TOKEN).then_some(at))
    {
        let start = offset + TOKEN.len();
        let (Some(minor), Some(states), Some(filetime)) = (
            u32_le(&block.payload, start + 66),
            block.payload.get(start + 107),
            u64_le(&block.payload, start + 117),
        ) else {
            continue;
        };
        if filetime > i64::MAX as u64 {
            continue;
        }
        out.push(attribute(
            block,
            offset,
            "configuration_manager",
            TOKEN,
            vec![
                AttributeValue::Integer(minor as i64),
                AttributeValue::Integer(*states as i64),
                AttributeValue::Integer(filetime as i64),
            ],
        ));
    }
}

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
fn u64_le(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn attribute(
    block: &crate::container::Block,
    offset: usize,
    name: &str,
    token: &[u8],
    values: Vec<AttributeValue>,
) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId(format!("sldprt:{name}:{}:{offset}", block.offset)),
        target: AttributeTarget::Document,
        name: name.into(),
        values,
        meta: EntityMeta {
            provenance: Provenance {
                format: "sldprt".into(),
                stream: block
                    .section
                    .clone()
                    .unwrap_or_else(|| format!("block@{}", block.offset)),
                offset: offset as u64,
                tag: Some(token.escape_ascii().to_string()),
            },
            exactness: Exactness::ByteExact,
        },
    }
}
