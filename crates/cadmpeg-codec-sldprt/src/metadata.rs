// SPDX-License-Identifier: Apache-2.0
//! Typed SW Objects document metadata.

use crate::container::{ContainerScan, Section};
use cadmpeg_core::decode::View;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::Exactness;

use crate::layout::transformed_reference_plane_metadata as trans_plane;

pub fn attributes(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<SourceAttribute> {
    let mut out = Vec::new();
    for section in scan.sections() {
        scan_vectors(
            section,
            b"moBBoxCenterData_c",
            "bounding_envelope",
            4,
            4,
            true,
            &mut out,
            annotations,
        );
        scan_vectors(
            section,
            b"moDefaultRefPlnData_c",
            "default_reference_plane",
            9,
            0,
            false,
            &mut out,
            annotations,
        );
        scan_part(section, &mut out, annotations);
        scan_configuration_manager(section, &mut out, annotations);
        scan_transformed_reference_plane(section, &mut out, annotations);
        scan_units_xml(section, &mut out, annotations);
        scan_length_user_units(section, &mut out, annotations);
    }
    out
}

fn scan_transformed_reference_plane(
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) {
    const TOKEN: &[u8] = b"moTransRefPlaneData_c";
    const PREFIX: &[u8] = &trans_plane::PREFIX_VALUE;
    let payload = section.payload();
    for offset in payload
        .windows(TOKEN.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == TOKEN).then_some(at))
    {
        let prefix = offset + TOKEN.len();
        if payload.get(prefix..prefix + PREFIX.len()) != Some(PREFIX) {
            continue;
        }
        let start = prefix + trans_plane::CENTER;
        let Some(values) = (0..9)
            .map(|index| View::f64_le_at(payload, start + index * 8))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if !values
            .iter()
            .all(|value| value.is_finite() && value.abs() < 1_000.0)
            || values[3] <= 1.0e-5
            || values[4] <= 1.0e-5
        {
            continue;
        }
        out.push(attribute(
            section,
            offset,
            "transformed_reference_plane",
            TOKEN,
            vec![
                AttributeValue::Vector(values[..3].iter().map(|value| value * 1000.0).collect()),
                AttributeValue::Vector(values[3..5].iter().map(|value| value * 1000.0).collect()),
                AttributeValue::Vector(values[5..8].to_vec()),
                AttributeValue::Float(values[8] * 1000.0),
            ],
            annotations,
        ));
    }
}

fn scan_length_user_units(
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) {
    const TOKEN: &[u8] = b"moLengthUserUnits_c";
    const STRING_MARKER: &[u8] = &[0xff, 0xfe, 0xff];
    let payload = section.payload();
    for offset in payload
        .windows(TOKEN.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == TOKEN).then_some(at))
    {
        let search = offset + TOKEN.len();
        let Some(marker_end) = search.checked_add(STRING_MARKER.len()) else {
            continue;
        };
        if payload.get(search..marker_end) != Some(STRING_MARKER) {
            continue;
        }
        let marker = search;
        let Some(length) = payload.get(marker + 3).copied().map(usize::from) else {
            continue;
        };
        let start = marker + 4;
        let Some(bytes) = payload.get(start..start.saturating_add(length)) else {
            continue;
        };
        if bytes.is_empty() || bytes.len() % 2 != 0 {
            continue;
        }
        let mut view = View::over_retained(bytes);
        let mut units = Vec::new();
        while let Some(unit) = view.u16_le() {
            units.push(unit);
        }
        let value = String::from_utf16_lossy(&units);
        if value.trim().is_empty() {
            continue;
        }
        out.push(attribute(
            section,
            offset,
            "source_linear_unit_name",
            TOKEN,
            vec![AttributeValue::String(value)],
            annotations,
        ));
    }
}

fn scan_units_xml(
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) {
    let Some(text) = crate::container::xml_text(section.payload()) else {
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
            section,
            node.range().start,
            "source_linear_unit_code",
            b"SW_UnitsLinear",
            vec![AttributeValue::Integer(code)],
            annotations,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_vectors(
    section: Section<'_>,
    token: &[u8],
    name: &str,
    count: usize,
    skip: usize,
    all_lengths: bool,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) {
    let payload = section.payload();
    for offset in payload
        .windows(token.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == token).then_some(at))
    {
        let start = offset + token.len() + skip;
        let Some(values) = (0..count)
            .map(|index| View::f64_le_at(payload, start + index * 8))
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
        out.push(attribute(section, offset, name, token, values, annotations));
    }
}

fn scan_part(section: Section<'_>, out: &mut Vec<SourceAttribute>, annotations: &mut Annotations) {
    const TOKEN: &[u8] = b"moPart_c";
    let payload = section.payload();
    for offset in payload
        .windows(TOKEN.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == TOKEN).then_some(at))
    {
        let start = offset + TOKEN.len();
        let (Some(id), Some(version)) = (
            View::u32_le_at(payload, start),
            View::u32_le_at(payload, start + 8),
        ) else {
            continue;
        };
        out.push(attribute(
            section,
            offset,
            "part_record",
            TOKEN,
            vec![
                AttributeValue::Integer(id as i64),
                AttributeValue::Integer(version as i64),
            ],
            annotations,
        ));
    }
}

fn scan_configuration_manager(
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) {
    const TOKEN: &[u8] = b"moConfigurationMgr_c";
    let payload = section.payload();
    for offset in payload
        .windows(TOKEN.len())
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == TOKEN).then_some(at))
    {
        let start = offset + TOKEN.len();
        let (Some(minor), Some(states), Some(filetime)) = (
            View::u32_le_at(payload, start + 66),
            payload.get(start + 107),
            View::u64_le_at(payload, start + 117),
        ) else {
            continue;
        };
        if filetime > i64::MAX as u64 {
            continue;
        }
        out.push(attribute(
            section,
            offset,
            "configuration_manager",
            TOKEN,
            vec![
                AttributeValue::Integer(minor as i64),
                AttributeValue::Integer(*states as i64),
                AttributeValue::Integer(filetime as i64),
            ],
            annotations,
        ));
    }
}

fn attribute(
    section: Section<'_>,
    offset: usize,
    name: &str,
    token: &[u8],
    values: Vec<AttributeValue>,
    annotations: &mut Annotations,
) -> SourceAttribute {
    let id = AttributeId(format!(
        "sldprt:metadata:{name}#{}:{offset}",
        section.ordinal()
    ));
    crate::annotations::note(
        annotations,
        id.0.clone(),
        section.display_name(),
        offset as u64,
        std::str::from_utf8(token).unwrap_or(name),
        Exactness::ByteExact,
    );
    SourceAttribute {
        id,
        target: AttributeTarget::Document,
        name: name.into(),
        values,
    }
}

#[cfg(test)]
mod tests;
