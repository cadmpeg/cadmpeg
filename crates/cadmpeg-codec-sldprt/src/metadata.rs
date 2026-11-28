// SPDX-License-Identifier: Apache-2.0
//! Typed SW Objects document metadata.

pub(crate) mod configuration_manager;
mod configuration_properties;
pub(crate) mod default_properties;
pub(crate) mod mass_properties;
pub(crate) mod part;

use std::cell::RefCell;
use std::rc::Rc;

use crate::container::{ContainerScan, Section};
use cadmpeg_core::decode::{u64_from_index, ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{DecodeParts as _, GivenUpPart, Part};
use cadmpeg_ir::Exactness;

use crate::layout::{
    transformed_reference_enclosure_extent as enclosure_extent,
    transformed_reference_plane_metadata as trans_plane,
};

pub(crate) fn attributes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    properties: Option<&PropertyReads>,
    annotations: &mut Annotations,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    given_up: &mut Vec<GivenUpPart>,
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
        configuration_manager::append(section, &mut out, annotations, losses)?;
        scan_transformed_reference_plane(section, &mut out, annotations)?;
        if properties.is_none() || !is_saved_property_owner(section) {
            scan_units_xml(ctx, section, &mut out, annotations)?;
        }
        scan_length_user_units(ctx, section, &mut out, annotations)?;
    }
    if let Some(properties) = properties {
        let saved = properties.read(ctx, scan)?;
        out.extend(saved.rows.iter().cloned());
        annotations
            .append(saved.annotations.clone())
            .map_err(CodecError::malformed)?;
        losses.extend(saved.losses.iter().cloned());
        given_up.extend(saved.given_up.iter().cloned());
    } else {
        default_properties::append(ctx, scan, &mut out, annotations, losses, given_up)?;
        configuration_properties::append(ctx, scan, &mut out, annotations, losses, given_up)?;
    }
    Ok(out)
}

#[derive(PartialEq, Eq)]
struct CarrierIdentity {
    id: cadmpeg_ir::ids::UnknownId,
    stream: cadmpeg_ir::StreamName,
    stored: Sha256Digest,
    payload: Sha256Digest,
}

impl CarrierIdentity {
    fn of(section: Section<'_>) -> Self {
        let payload = Sha256Digest::digest(section.payload());
        let stored = if std::ptr::eq(section.payload(), section.stored_payload()) {
            payload.clone()
        } else {
            Sha256Digest::digest(section.stored_payload())
        };
        Self {
            id: section.native_id(),
            stream: section.source_stream().clone(),
            stored,
            payload,
        }
    }
}

#[derive(Default)]
pub(crate) struct PropertyReads {
    projections: RefCell<Vec<(Vec<CarrierIdentity>, Rc<SavedPropertyProjection>)>>,
}

impl PropertyReads {
    pub(crate) fn read(
        &self,
        ctx: &DecodeContext<'_>,
        scan: &ContainerScan<'_>,
    ) -> Result<Rc<SavedPropertyProjection>, CodecError> {
        let roster = scan
            .sections()
            .filter(|section| is_saved_property_owner(*section))
            .map(CarrierIdentity::of)
            .collect::<Vec<_>>();
        let mut reads = self.projections.borrow_mut();
        if let Some((_, projection)) = reads.iter().find(|(held, _)| held == &roster) {
            return Ok(Rc::clone(projection));
        }
        let projection = Rc::new(saved_property_projection(ctx, scan)?);
        reads.push((roster, Rc::clone(&projection)));
        Ok(projection)
    }
}

#[derive(Default)]
pub(crate) struct SavedPropertyProjection {
    pub(crate) rows: Vec<SourceAttribute>,
    annotations: Annotations,
    losses: Vec<LossNote>,
    given_up: Vec<GivenUpPart>,
}

#[cfg(test)]
pub(crate) fn saved_property_rows(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Vec<SourceAttribute>, CodecError> {
    saved_property_projection(ctx, scan).map(|projection| projection.rows)
}

fn saved_property_projection(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<SavedPropertyProjection, CodecError> {
    let mut projection = SavedPropertyProjection::default();
    let SavedPropertyProjection {
        rows,
        annotations,
        losses,
        given_up,
    } = &mut projection;
    for section in scan
        .sections()
        .filter(|section| is_saved_property_owner(*section))
    {
        scan_units_xml(ctx, section, rows, annotations)?;
    }
    default_properties::append(ctx, scan, rows, annotations, losses, given_up)?;
    configuration_properties::append(ctx, scan, rows, annotations, losses, given_up)?;
    Ok(projection)
}

/// The part a saved-property carrier is when none of its properties is read.
const PROPERTY_CARRIER_PART: &str = "property-carrier";

/// Gives up the saved-property carrier `owner`, whose stored content is
/// malformed: none of its properties is read. The source record holds the
/// carrier's bytes, so the part keeps only their digest.
fn give_up_carrier(
    ctx: &DecodeContext<'_>,
    owner: Section<'_>,
    error: CodecError,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<(), CodecError> {
    let stream = owner.source_stream();
    let part = Part::at(
        "sldprt",
        PROPERTY_CARRIER_PART,
        0,
        owner.stored_payload(),
        [ArenaClass::Model],
    )
    .named(stream.as_str())
    .in_stream(stream.clone())
    .digest_only();
    let cause = cadmpeg_ir::unknown::part::Cause::Unreadable.of(&error);
    given_up.push(ctx.give_up_part(part, cause, error)?);
    Ok(())
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
    for offset in memchr::memmem::find_iter(section.payload(), TOKEN) {
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
    for offset in memchr::memmem::find_iter(payload, TOKEN) {
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
            document.source_range(node.range()).start,
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
    let finder = memchr::memmem::Finder::new(token);
    let mut cursor = 0;
    while let Some(rel) = finder.find(&payload[cursor..]) {
        let offset = cursor + rel;
        cursor = offset + 1;
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
            cadmpeg_ir::codec::FormatId::new(crate::dialect::FORMAT),
            &cadmpeg_ir::identity_component!("metadata"),
            name,
        ),
        cadmpeg_ir::ids::IdentityKey::from(section.ordinal()).colon(offset),
    );
    crate::annotations::note(
        annotations,
        &id,
        section.source_stream(),
        u64_from_index(offset),
        std::str::from_utf8(token).unwrap_or(name.as_str()),
        Exactness::ByteExact,
    )?;
    Ok(SourceAttribute {
        id,
        target: AttributeTarget::Document.into(),
        name: name.as_str().into(),
        values: values.into_iter().map(Some).collect(),
        native_ref: None,
    })
}

#[cfg(test)]
mod tests;
