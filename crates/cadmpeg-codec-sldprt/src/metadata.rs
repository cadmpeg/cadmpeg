// SPDX-License-Identifier: Apache-2.0
//! Typed SW Objects document metadata.

use crate::container::{ContainerScan, Section};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::Exactness;

use crate::layout::transformed_reference_plane_metadata as trans_plane;

pub(crate) fn attributes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> Result<Vec<SourceAttribute>, CodecError> {
    let mut out = Vec::new();
    for section in scan.sections(ctx)? {
        scan_vectors(
            ctx,
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
            ctx,
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
        scan_part(ctx, section, &mut out, annotations)?;
        scan_configuration_manager(ctx, section, &mut out, annotations)?;
        scan_transformed_reference_plane(ctx, section, &mut out, annotations)?;
        scan_units_xml(ctx, section, &mut out, annotations)?;
        scan_length_user_units(ctx, section, &mut out, annotations)?;
    }
    Ok(out)
}

fn scan_transformed_reference_plane(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) -> Result<(), CodecError> {
    const TOKEN: &[u8] = b"moTransRefPlaneData_c";
    const PREFIX: &[u8] = &trans_plane::PREFIX_VALUE;
    let payload = section.payload();
    for offset in ctx.admit_iter(payload, "scan SLDPRT metadata token markers")?
        .windows(std::num::NonZeroUsize::new(TOKEN.len()).ok_or_else(|| CodecError::malformed("zero metadata token width"))?)
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
        let (Some(center), Some(extents), Some(auxiliary), Some(diagonal)) = (
            millimetres(&values[..3]),
            millimetres(&values[3..5]),
            AttributeValue::vector(values[5..8].iter().copied()),
            AttributeValue::float(values[8] * 1000.0),
        ) else {
            continue;
        };
        ctx.reserve_vec(out, 1, "collect SLDPRT document attributes")?;
        out.push(attribute(
            ctx,
            section,
            offset,
            &cadmpeg_ir::identity_component!("transformed_reference_plane"),
            TOKEN,
            vec![center, extents, auxiliary, diagonal],
            annotations,
        )?);
    }
    Ok(())
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
    for offset in ctx.admit_iter(payload, "scan SLDPRT metadata token markers")?
        .windows(std::num::NonZeroUsize::new(TOKEN.len()).ok_or_else(|| CodecError::malformed("zero metadata token width"))?)
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
        let units = ctx.admit_iter(bytes, "validate SLDPRT linear unit name")?
            .chunks(std::num::NonZeroUsize::new(2).ok_or_else(|| CodecError::malformed("zero UTF-16 unit width"))?)
            .filter_map(|unit| View::u16_le_at(unit, 0));
        if char::decode_utf16(units).map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER)).all(char::is_whitespace) {
            continue;
        }
        let value = ctx.utf16le_lossy_text(
            bytes,
            bytes.len() / 2,
            false,
            "retain SLDPRT linear unit name",
        )?;
        ctx.reserve_vec(out, 1, "collect SLDPRT document attributes")?;
        out.push(attribute(
            ctx,
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
    let Some(text) = crate::container::xml_text_charged(
        ctx,
        section.payload(),
        "materialize SLDPRT document metadata XML",
    )?
    else {
        return Ok(());
    };
    let admitted_document = match ctx.parse_xml(text.as_str(), "decode XML tree") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(());
        }
    };
    let document = admitted_document.document();
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
        ctx.reserve_vec(out, 1, "collect SLDPRT document attributes")?;
        out.push(attribute(
            ctx,
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
    ctx: &DecodeContext<'_>,
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
    for offset in ctx.admit_iter(payload, "scan SLDPRT metadata vector markers")?
        .windows(std::num::NonZeroUsize::new(token.len()).ok_or_else(|| CodecError::malformed("zero metadata token width"))?)
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
        ctx.reserve_vec(out, 1, "collect SLDPRT document attributes")?;
        out.push(attribute(
            ctx,
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
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) -> Result<(), CodecError> {
    const TOKEN: &[u8] = b"moPart_c";
    let payload = section.payload();
    for offset in ctx.admit_iter(payload, "scan SLDPRT metadata token markers")?
        .windows(std::num::NonZeroUsize::new(TOKEN.len()).ok_or_else(|| CodecError::malformed("zero metadata token width"))?)
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
        ctx.reserve_vec(out, 1, "collect SLDPRT document attributes")?;
        out.push(attribute(
            ctx,
            section,
            offset,
            &cadmpeg_ir::identity_component!("part_record"),
            TOKEN,
            vec![
                AttributeValue::Integer(i64::from(id)),
                AttributeValue::Integer(i64::from(version)),
            ],
            annotations,
        )?);
    }
    Ok(())
}

fn scan_configuration_manager(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
) -> Result<(), CodecError> {
    const TOKEN: &[u8] = b"moConfigurationMgr_c";
    let payload = section.payload();
    for offset in ctx.admit_iter(payload, "scan SLDPRT metadata token markers")?
        .windows(std::num::NonZeroUsize::new(TOKEN.len()).ok_or_else(|| CodecError::malformed("zero metadata token width"))?)
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
        let Ok(filetime) = i64::try_from(filetime) else {
            continue;
        };
        ctx.reserve_vec(out, 1, "collect SLDPRT document attributes")?;
        out.push(attribute(
            ctx,
            section,
            offset,
            &cadmpeg_ir::identity_component!("configuration_manager"),
            TOKEN,
            vec![
                AttributeValue::Integer(i64::from(minor)),
                AttributeValue::Integer(i64::from(*states)),
                AttributeValue::Integer(filetime),
            ],
            annotations,
        )?);
    }
    Ok(())
}

/// Source lengths in metres as a millimetre vector, or `None` when a value
/// is not finite in millimetres.
fn millimetres(values: &[f64]) -> Option<AttributeValue> {
    AttributeValue::vector(values.iter().map(|value| value * 1000.0))
}

fn attribute(
    ctx: &DecodeContext<'_>,
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
        ctx,
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
