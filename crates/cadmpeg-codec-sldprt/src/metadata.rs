// SPDX-License-Identifier: Apache-2.0
//! Typed SW Objects document metadata.

mod configuration_properties;
pub(crate) mod default_properties;
pub(crate) mod part;

use crate::container::{ContainerScan, Section};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::Exactness;

use crate::layout::{
    transformed_reference_enclosure_extent as enclosure_extent,
    transformed_reference_plane_metadata as trans_plane,
};

pub(crate) fn attributes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Vec<SourceAttribute>, CodecError> {
    let mut out = Vec::new();
    for section in scan.sections() {
        scan_vectors(
            section,
            &VectorScan {
                token: b"moBBoxCenterData_c",
                name: &cadmpeg_ir::identity_component!("bounding_envelope"),
                count: 4,
                skip: 4,
                all_lengths: true,
            },
            &mut out,
            annotations,
        )?;
        scan_vectors(
            section,
            &VectorScan {
                token: b"moDefaultRefPlnData_c",
                name: &cadmpeg_ir::identity_component!("default_reference_plane"),
                count: 9,
                skip: 0,
                all_lengths: false,
            },
            &mut out,
            annotations,
        )?;
        scan_part(section, &mut out, annotations, losses)?;
        note_configuration_manager(section, losses);
        scan_transformed_reference_plane(section, &mut out, annotations)?;
        scan_units_xml(ctx, section, &mut out, annotations)?;
        scan_length_user_units(ctx, section, &mut out, annotations)?;
    }
    default_properties::append(ctx, scan, &mut out, annotations, losses)?;
    configuration_properties::append(ctx, scan, &mut out, annotations, losses)?;
    Ok(out)
}

pub(crate) fn is_saved_property_attribute(attribute: &SourceAttribute) -> bool {
    default_properties::is_attribute(attribute) || configuration_properties::is_attribute(attribute)
}

pub(crate) fn is_saved_property_owner(owner: Section<'_>) -> bool {
    default_properties::is_owner(owner) || configuration_properties::is_owner(owner)
}

fn scan_transformed_reference_plane(
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) -> Result<(), CodecError> {
    const TOKEN: &[u8] = b"moTransRefPlaneData_c";
    for (offset, bytes) in section.payload().windows(TOKEN.len()).enumerate() {
        if bytes != TOKEN {
            continue;
        }
        let Some(values) = transformed_enclosure(section.payload(), offset) else {
            continue;
        };
        let (Some(center), Some(upper), Some(lower), Some(radius)) = (
            millimetres(&values[..3]),
            millimetres(&values[3..6]),
            millimetres(&values[6..9]),
            AttributeValue::float(values[9] * 1000.0),
        ) else {
            continue;
        };
        out.push(attribute(
            section,
            offset,
            &cadmpeg_ir::identity_component!("transformed_reference_enclosure"),
            TOKEN,
            vec![center, upper, lower, radius],
            annotations,
        )?);
    }
    Ok(())
}

pub(crate) fn transformed_enclosure(payload: &[u8], token: usize) -> Option<[f64; 10]> {
    let declaration = token.checked_sub(6)?;
    if payload.get(declaration..token) != Some(&[0xff, 0xff, 1, 0, 21, 0]) {
        return None;
    }
    let body = token.checked_add(b"moTransRefPlaneData_c".len())?;
    payload.get(body..body.checked_add(trans_plane::LEN)?)?;
    if payload.get(body + trans_plane::EXTENT_PRESENT) != Some(&1) {
        return None;
    }
    let start = body.checked_add(trans_plane::LEN)?;
    payload.get(start..start.checked_add(enclosure_extent::LEN)?)?;
    let values: [Option<f64>; 10] =
        std::array::from_fn(|index| View::f64_le_at(payload, start + index * 8));
    let values: Vec<_> = values.into_iter().collect::<Option<Vec<_>>>()?;
    let values: [f64; 10] = values.try_into().ok()?;
    valid_transformed_enclosure(&values).then_some(values)
}

pub(crate) fn valid_transformed_enclosure(values: &[f64; 10]) -> bool {
    values.iter().all(|value| value.is_finite())
        && values[9] >= 0.0
        && (0..3).all(|axis| values[axis + 6] <= values[axis] && values[axis] <= values[axis + 3])
}

fn scan_length_user_units(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) -> Result<(), CodecError> {
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
        let Some(bytes) = start
            .checked_add(length)
            .and_then(|end| payload.get(start..end))
        else {
            continue;
        };
        if bytes.is_empty() || bytes.len() % 2 != 0 {
            continue;
        }
        let scalars = || {
            char::decode_utf16(
                (0..bytes.len() / 2).filter_map(|index| View::u16_le_at(bytes, index * 2)),
            )
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        };
        if scalars().all(char::is_whitespace) {
            continue;
        }
        let value = ctx.utf16le_lossy_text(
            bytes,
            bytes.len() / 2,
            false,
            "retain SLDPRT linear unit name",
        )?;
        out.push(attribute(
            section,
            offset,
            &cadmpeg_ir::identity_component!("source_linear_unit_name"),
            TOKEN,
            vec![AttributeValue::String(value)],
            annotations,
        )?);
    }
    Ok(())
}

fn scan_units_xml(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) -> Result<(), CodecError> {
    let Some(text) = crate::container::xml_text(section.payload()) else {
        return Ok(());
    };
    let document = match ctx.parse_xml(text.as_str(), "decode XML tree") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(());
        }
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
            &cadmpeg_ir::identity_component!("source_linear_unit_code"),
            b"SW_UnitsLinear",
            vec![AttributeValue::Integer(code)],
            annotations,
        )?);
    }
    Ok(())
}

/// One fixed-width float vector located by its record token.
struct VectorScan<'a> {
    token: &'a [u8],
    name: &'a cadmpeg_ir::ids::IdentityComponent,
    count: usize,
    skip: usize,
    all_lengths: bool,
}

fn scan_vectors(
    section: Section<'_>,
    scan: &VectorScan<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) -> Result<(), CodecError> {
    let VectorScan {
        token,
        name,
        count,
        skip,
        all_lengths,
    } = *scan;
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
        // A source number that is not finite, or a length that is not finite
        // in millimetres, admits no attribute.
        let values = if all_lengths {
            millimetres(&values).map(|lengths| vec![lengths])
        } else {
            millimetres(&values[..3])
                .zip(AttributeValue::vector(values[3..].iter().copied()))
                .map(|(lengths, ratios)| vec![lengths, ratios])
        };
        let Some(values) = values else {
            continue;
        };
        out.push(attribute(
            section,
            offset,
            name,
            token,
            values,
            annotations,
        )?);
    }
    Ok(())
}

fn scan_part(
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    match part::parse(section.payload()) {
        part::Root::Absent => {}
        part::Root::Unsupported => losses.push(
            crate::loss::SldprtLossCode::MetadataPartRootUnsupported.note(format!(
                "Part root in {} has an unsupported scalar prefix or continuation",
                section.source_stream().as_str()
            )),
        ),
        part::Root::Supported(prefix) => {
            let mut values = vec![AttributeValue::Integer(i64::from(prefix.first_word))];
            if let Some(word) = prefix.trailing_word {
                values.push(AttributeValue::Integer(i64::from(word)));
            }
            out.push(attribute(
                section,
                prefix.token_offset,
                &cadmpeg_ir::identity_component!("part_record"),
                part::TOKEN,
                values,
                annotations,
            )?);
        }
    }
    Ok(())
}

fn note_configuration_manager(
    section: Section<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    if part::declaration(section.payload(), 0)
        .is_some_and(|(_, name, _)| name == b"moConfigurationMgr_c")
    {
        losses.push(crate::loss::SldprtLossCode::MetadataConfigurationManagerOpaque.note(
            "Configuration-manager fields and serializer bounds are unresolved; retained bytes are preserved without editable scalar metadata.",
        ));
    }
}

/// Source lengths in metres as a millimetre vector, or `None` when a value
/// is not finite in millimetres.
fn millimetres(values: &[f64]) -> Option<AttributeValue> {
    AttributeValue::vector(values.iter().map(|value| value * 1000.0))
}

fn attribute(
    section: Section<'_>,
    offset: usize,
    name: &cadmpeg_ir::ids::IdentityComponent,
    token: &[u8],
    values: Vec<AttributeValue>,
    annotations: &mut Annotations,
) -> Result<SourceAttribute, CodecError> {
    let id = AttributeId::compose(
        &cadmpeg_ir::ids::IdentityNamespace::from_components(
            &cadmpeg_ir::identity_component!("sldprt"),
            &cadmpeg_ir::identity_component!("metadata"),
            name,
        ),
        cadmpeg_ir::ids::IdentityKey::from(section.ordinal()).colon(offset),
    );
    crate::annotations::note(
        annotations,
        id.as_str(),
        section.source_stream(),
        u64_from_index(offset),
        std::str::from_utf8(token).unwrap_or(name.as_str()),
        Exactness::ByteExact,
    )?;
    Ok(SourceAttribute {
        id,
        target: AttributeTarget::Document,
        name: name.as_str().into(),
        values,
    })
}

#[cfg(test)]
mod tests;
