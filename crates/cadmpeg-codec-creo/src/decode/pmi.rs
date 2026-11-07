// SPDX-License-Identifier: Apache-2.0
//! Geometric tolerances from neutral attributes and annotation elements.

mod attributes;
mod dimensions;
mod elements;
pub(super) mod tags;
mod targets;

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    DatumReference, DatumReferences, GeometricToleranceKind, PmiAnnotation, PmiDefinition,
    PmiMagnitude, PmiQuantity, PmiTarget, PmiValue,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;
use crate::loss::CreoLossCode;

use self::attributes::{Attribute, Value};

#[derive(Default)]
struct Tolerance<'a> {
    kind: Option<&'a [u8]>,
    magnitude: Option<f64>,
    secondary: Option<&'a [u8]>,
    datums: [[Option<DatumLabel<'a>>; 3]; 2],
    issues: BTreeSet<ToleranceIssue>,
    offset: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ToleranceIssue {
    Kind,
    Magnitude,
    Secondary,
    Datum(usize),
    Conflict,
    Unit,
}

#[derive(Clone, Copy)]
struct DatumLabel<'a> {
    text: &'a [u8],
    offset: usize,
}

#[derive(Default)]
struct DatumState {
    ids: BTreeMap<String, PmiId>,
    unresolved: usize,
}

pub(super) fn transfer(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(neutral) = section(scan, "NeuPrtSld") else {
        return Ok(());
    };
    let has_pmi_attributes = [b"PTC_GTOL_TYPE\0".as_slice(), b"PTC_DIM_TYPE\0".as_slice()]
        .into_iter()
        .any(|marker| {
            neutral
                .window()
                .windows(marker.len())
                .any(|bytes| bytes == marker)
        });
    if !has_pmi_attributes {
        return Ok(());
    }
    let Some(attributes) = attributes::attributes(ctx, neutral)? else {
        losses.push(
            CreoLossCode::PmiTableIncomplete
                .note("The neutral attribute table containing PMI did not parse completely."),
        );
        return Ok(());
    };
    let mut tolerances = BTreeMap::<(u32, u32), Tolerance<'_>>::new();
    let mut unresolved = 0usize;
    for attribute in attributes.iter().flatten().copied() {
        let (Some(owner), Some(owner_type)) = (attribute.owner, attribute.owner_type) else {
            if attribute.name == b"PTC_GTOL_TYPE" {
                unresolved += 1;
            }
            continue;
        };
        if matches!(
            attribute.name,
            b"PTC_GTOL_TYPE" | b"PTC_GTOL_PRIMARY_TOL" | b"PTC_GTOL_2_TOL_STRING"
        ) || datum_field(attribute.name).is_some()
        {
            let tolerance = tolerances.entry((owner, owner_type)).or_default();
            add_attribute(tolerance, attribute);
        }
    }
    let mut owners = BTreeMap::<(u32, u32), Option<(elements::Element, &str)>>::new();
    let mut element_tables = Vec::new();
    for name in ["BasFullData", "FullMData"] {
        let Some(section) = section(scan, name) else {
            continue;
        };
        let Some(elements) = elements::elements(ctx, section)? else {
            continue;
        };
        for element in elements.iter().flatten().copied() {
            if element.annotation_type != Some(32) || element.annotation.is_none() {
                continue;
            }
            let key = (element.owner, element.owner_type);
            match owners.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some((element, name)));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        element_tables.push(elements);
    }
    let scale = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm);
    dimensions::transfer(
        ctx,
        &attributes,
        &element_tables,
        scale.map(cadmpeg_ir::scalar::PositiveReal::get),
        ir,
        annotations,
        losses,
    )?;
    let targets = targets::flatness_targets(ctx, scan, &owners, &tolerances, ir)?;
    let mut emitted = 0usize;
    let mut text_magnitudes = 0usize;
    let mut conflicting = 0usize;
    let mut unresolved_units = 0usize;
    let mut datum_state = DatumState::default();
    let mut inferred_targets = 0usize;
    for ((owner, owner_type), tolerance) in tolerances {
        if !matches!(owners.get(&(owner, owner_type)), Some(Some(_))) {
            unresolved += 1;
            continue;
        }
        conflicting += usize::from(tolerance.issues.contains(&ToleranceIssue::Conflict));
        unresolved_units += usize::from(
            tolerance.issues.contains(&ToleranceIssue::Unit)
                || (scale.is_none()
                    && (tolerance.magnitude.is_some()
                        || tolerance.secondary.is_some_and(|text| !text.is_empty()))),
        );
        let Some(kind) = tolerance
            .kind
            .filter(|_| !tolerance.issues.contains(&ToleranceIssue::Kind))
            .and_then(kind)
        else {
            unresolved += 1;
            continue;
        };
        let first_key = format!("{owner_type}:{owner}");
        let target = targets.get(&(owner, owner_type)).cloned();
        let references = if tolerance.issues.contains(&ToleranceIssue::Datum(0)) {
            unresolved += 1;
            None
        } else {
            datum_system(
                ctx,
                ir,
                annotations,
                &mut datum_state,
                &tolerance.datums[0],
                &first_key,
            )?
        };
        let second_key = format!("{owner_type}:{owner}:segment2");
        let second_references = if tolerance.issues.contains(&ToleranceIssue::Datum(1)) {
            unresolved += 1;
            None
        } else {
            datum_system(
                ctx,
                ir,
                annotations,
                &mut datum_state,
                &tolerance.datums[1],
                &second_key,
            )?
        };
        if let Some(value) = tolerance
            .magnitude
            .filter(|_| !tolerance.issues.contains(&ToleranceIssue::Magnitude))
            .and_then(|value| PmiValue::new(value * scale?.get(), PmiQuantity::Length))
            .and_then(PmiMagnitude::new)
        {
            push_tolerance(
                ctx,
                ir,
                annotations,
                (&first_key, tolerance.offset),
                (kind.clone(), value),
                references,
                target.clone().into_iter().collect(),
            )?;
            emitted += 1;
            inferred_targets += usize::from(target.is_some());
        } else {
            unresolved += 1;
        }
        if tolerance.issues.contains(&ToleranceIssue::Secondary)
            || tolerance.secondary.is_some_and(|text| !text.is_empty())
            || tolerance.datums[1]
                .iter()
                .flatten()
                .any(|label| !label.text.is_empty())
        {
            if let Some(secondary) = tolerance
                .secondary
                .filter(|_| !tolerance.issues.contains(&ToleranceIssue::Secondary))
                .and_then(text_magnitude)
                .and_then(|value| PmiValue::new(value * scale?.get(), PmiQuantity::Length))
                .and_then(PmiMagnitude::new)
            {
                push_tolerance(
                    ctx,
                    ir,
                    annotations,
                    (&second_key, tolerance.offset),
                    (kind, secondary),
                    second_references,
                    Vec::new(),
                )?;
                emitted += 1;
                text_magnitudes += 1;
            } else {
                unresolved += 1;
            }
        }
    }
    ir.model.pmi.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    if unresolved != 0 {
        losses.push(CreoLossCode::PmiRecordUnresolved.note(format!(
            "{unresolved} geometric-tolerance value(s) or annotation-element joins are unresolved."
        )));
    }
    if conflicting != 0 {
        losses.push(CreoLossCode::PmiAttributeConflict.note(format!(
            "{conflicting} geometric tolerance(s) have conflicting attributes; affected values were withheld."
        )));
    }
    if unresolved_units != 0 {
        losses.push(CreoLossCode::PmiUnitUnresolved.note(format!(
            "{unresolved_units} geometric tolerance(s) have unresolved units; affected values were withheld."
        )));
    }
    if emitted != 0 {
        losses.push(CreoLossCode::PmiAssociationsUnresolved.note(
            "Geometric-tolerance datum-feature and geometric-target associations remain incomplete."
        ));
        losses.push(CreoLossCode::PmiModifiersUnresolved.note(format!(
            "{emitted} geometric tolerance(s) retain unresolved modifiers and zone constraints."
        )));
    }
    if inferred_targets != 0 {
        losses.push(CreoLossCode::PmiToleranceTargetInferred.note(format!(
            "{inferred_targets} geometric tolerance(s) use the unique face of the referenced surface as the target."
        )));
    }
    if text_magnitudes != 0 {
        losses.push(CreoLossCode::PmiTextMagnitudeInferred.note(format!("{text_magnitudes} composite tolerance segment(s) use the leading numeric value of the formatted magnitude. Trailing text and segment grouping are unresolved.")));
    }
    if !datum_state.ids.is_empty() {
        losses.push(
            CreoLossCode::PmiDatumIdentityInferred
                .note("Datum references with the same label use an inferred shared identity."),
        );
    }
    if datum_state.unresolved != 0 {
        losses.push(CreoLossCode::PmiDatumLabelUnresolved.note(format!(
            "{} datum-reference label(s) could not be decoded.",
            datum_state.unresolved
        )));
    }
    Ok(())
}

fn section<'a>(scan: &'a ContainerScan<'_>, name: &str) -> Option<View<'a>> {
    let mut sections = scan
        .framing
        .sections
        .iter()
        .filter(|section| section.name() == name);
    let (Some(section), None) = (sections.next(), sections.next()) else {
        return None;
    };
    View::over_retained(&scan.framing.data).child(section.offset(), section.end())
}

fn add_attribute<'a>(tolerance: &mut Tolerance<'a>, attribute: Attribute<'a>) {
    match (attribute.name, attribute.value) {
        (b"PTC_GTOL_TYPE", Value::Text(value)) => {
            if let Some(previous) = tolerance.kind {
                if previous != value {
                    tolerance.issues.insert(ToleranceIssue::Kind);
                    tolerance.issues.insert(ToleranceIssue::Conflict);
                }
            } else {
                tolerance.kind = Some(value);
                tolerance.offset = attribute.offset;
            }
        }
        (b"PTC_GTOL_PRIMARY_TOL", Value::Real(Some(value))) => {
            if !attribute.unit.is_empty() {
                tolerance.issues.insert(ToleranceIssue::Magnitude);
                tolerance.issues.insert(ToleranceIssue::Unit);
            }
            if let Some(previous) = tolerance.magnitude {
                if previous.to_bits() != value.to_bits() {
                    tolerance.issues.insert(ToleranceIssue::Magnitude);
                    tolerance.issues.insert(ToleranceIssue::Conflict);
                }
            } else {
                tolerance.magnitude = Some(value);
            }
        }
        (b"PTC_GTOL_2_TOL_STRING", Value::Text(value)) => {
            if !attribute.unit.is_empty() && !value.is_empty() {
                tolerance.issues.insert(ToleranceIssue::Secondary);
                tolerance.issues.insert(ToleranceIssue::Unit);
            }
            if let Some(previous) = tolerance.secondary {
                if previous != value {
                    tolerance.issues.insert(ToleranceIssue::Secondary);
                    tolerance.issues.insert(ToleranceIssue::Conflict);
                }
            } else {
                tolerance.secondary = Some(value);
            }
        }
        (name, Value::Text(value)) => {
            if let Some((segment, compartment)) = datum_field(name) {
                let label = DatumLabel {
                    text: value,
                    offset: attribute.offset,
                };
                let field = &mut tolerance.datums[segment][compartment];
                if let Some(previous) = field {
                    if previous.text != value {
                        tolerance.issues.insert(ToleranceIssue::Datum(segment));
                        tolerance.issues.insert(ToleranceIssue::Conflict);
                    }
                } else {
                    *field = Some(label);
                }
            }
        }
        (name, _) => match name {
            b"PTC_GTOL_TYPE" => { tolerance.issues.insert(ToleranceIssue::Kind); }
            b"PTC_GTOL_PRIMARY_TOL" => { tolerance.issues.insert(ToleranceIssue::Magnitude); }
            b"PTC_GTOL_2_TOL_STRING" => { tolerance.issues.insert(ToleranceIssue::Secondary); }
            _ => {
                if let Some((segment, _)) = datum_field(name) {
                    tolerance.issues.insert(ToleranceIssue::Datum(segment));
                }
            }
        },
    }
}

fn datum_field(name: &[u8]) -> Option<(usize, usize)> {
    match name {
        b"PTC_GTOL_1_DTM_PRIM_STRING" => Some((0, 0)),
        b"PTC_GTOL_1_DTM_SEC_STRING" => Some((0, 1)),
        b"PTC_GTOL_1_DTM_TERT_STRING" => Some((0, 2)),
        b"PTC_GTOL_2_DTM_PRIM_STRING" => Some((1, 0)),
        b"PTC_GTOL_2_DTM_SEC_STRING" => Some((1, 1)),
        b"PTC_GTOL_2_DTM_TERT_STRING" => Some((1, 2)),
        _ => None,
    }
}

fn datum_system(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    state: &mut DatumState,
    labels: &[Option<DatumLabel<'_>>; 3],
    key: &str,
) -> Result<Option<PmiId>, CodecError> {
    let mut references = Vec::new();
    let mut offset = None;
    for (index, field) in labels.iter().enumerate() {
        let Some(field) = field else { continue };
        if field.text.is_empty() {
            continue;
        }
        let Some(text) = std::str::from_utf8(field.text).ok() else {
            state.unresolved += 1;
            continue;
        };
        let parts = text.split('-');
        let count = parts.clone().count();
        if count == 0 || !parts.clone().all(|part| datum_part(part).is_some()) {
            state.unresolved += 1;
            continue;
        }
        let Some(precedence) = u32::try_from(index + 1).ok().and_then(NonZeroU32::new) else {
            state.unresolved += 1;
            continue;
        };
        offset.get_or_insert(field.offset);
        for part in parts {
            let Some((label, modifier)) = datum_part(part) else {
                continue;
            };
            let datum = if let Some(id) = state.ids.get(label) {
                id.clone()
            } else {
                let id = PmiId::try_from(format!("creo:annotation:pmi#datum:{label}"))
                    .map_err(|_| CodecError::malformed("invalid Creo datum identity"))?;
                ctx.charge_entities(1, "Creo datum identification")?;
                super::native::annotate(
                    annotations,
                    &id,
                    "NeuPrtSld",
                    cadmpeg_core::decode::u64_from_index(field.offset),
                    "datum label",
                    Exactness::Inferred,
                )?;
                ir.model.pmi.push(PmiAnnotation {
                    id: id.clone(),
                    name: None,
                    visible: None,
                    targets: Vec::new(),
                    definition: PmiDefinition::Datum {
                        identification: label.to_owned(),
                    },
                });
                state.ids.insert(label.to_owned(), id.clone());
                id
            };
            references.push(DatumReference {
                datum,
                precedence,
                common_group: (count > 1).then_some(precedence.get()),
                modifiers: modifier
                    .map(|value| vec![value.to_owned()])
                    .unwrap_or_default(),
            });
        }
    }
    let Some(offset) = offset else {
        return Ok(None);
    };
    let references = DatumReferences::try_from(references)
        .map_err(|_| CodecError::malformed("invalid Creo datum-reference compartments"))?;
    let id = PmiId::try_from(format!("creo:annotation:pmi#system:{key}"))
        .map_err(|_| CodecError::malformed("invalid Creo datum-system identity"))?;
    ctx.charge_entities(1, "Creo datum system")?;
    super::native::annotate(
        annotations,
        &id,
        "NeuPrtSld",
        cadmpeg_core::decode::u64_from_index(offset),
        "datum-reference labels",
        Exactness::Inferred,
    )?;
    ir.model.pmi.push(PmiAnnotation {
        id: id.clone(),
        name: None,
        visible: None,
        targets: Vec::new(),
        definition: PmiDefinition::DatumSystem { references },
    });
    Ok(Some(id))
}

fn datum_part(text: &str) -> Option<(&str, Option<&'static str>)> {
    let (label, modifier) = if let Some(label) = text.strip_suffix('Ⓜ') {
        (label, Some("maximum_material_requirement"))
    } else if let Some(label) = text.strip_suffix('Ⓛ') {
        (label, Some("least_material_requirement"))
    } else {
        (text, None)
    };
    (!label.is_empty()
        && label
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
    .then_some((label, modifier))
}

fn text_magnitude(mut text: &[u8]) -> Option<f64> {
    if let Some(rest) = text.strip_prefix(b"\xe8") {
        text = rest;
    }
    if let Some(rest) = text.strip_prefix("⌀".as_bytes()) {
        text = rest;
    }
    let count = text
        .iter()
        .take_while(|byte| byte.is_ascii_digit() || **byte == b'.')
        .count();
    let value = std::str::from_utf8(text.get(..count)?)
        .ok()?
        .parse::<f64>()
        .ok()?;
    value.is_finite().then_some(value)
}

fn push_tolerance(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source: (&str, usize),
    definition: (GeometricToleranceKind, PmiMagnitude),
    datum_system: Option<PmiId>,
    targets: Vec<PmiTarget>,
) -> Result<(), CodecError> {
    let (key, offset) = source;
    let id = PmiId::try_from(format!("creo:annotation:pmi#{key}"))
        .map_err(|_| CodecError::malformed("invalid Creo PMI identity"))?;
    ctx.charge_entities(1, "Creo geometric tolerance")?;
    super::native::annotate(
        annotations,
        &id,
        "NeuPrtSld",
        cadmpeg_core::decode::u64_from_index(offset),
        "geometric tolerance attributes",
        Exactness::Derived,
    )?;
    ir.model.pmi.push(PmiAnnotation {
        id,
        name: None,
        visible: None,
        targets,
        definition: PmiDefinition::GeometricTolerance {
            tolerance: definition.0,
            magnitude: definition.1,
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system,
            modifiers: Vec::new(),
            constraints: cadmpeg_ir::pmi::zones::ToleranceConstraints::default(),
        },
    });
    Ok(())
}

fn kind(value: &[u8]) -> Option<GeometricToleranceKind> {
    Some(match value {
        b"POSITION" => GeometricToleranceKind::Position,
        b"FLATNESS" => GeometricToleranceKind::Flatness,
        b"SURFACE" => GeometricToleranceKind::SurfaceProfile,
        b"LINE" => GeometricToleranceKind::LineProfile,
        b"PERPENDICULAR" => GeometricToleranceKind::Perpendicularity,
        b"ANGULAR" => GeometricToleranceKind::Angularity,
        b"PARALLEL" => GeometricToleranceKind::Parallelism,
        b"STRAIGHTNESS" => GeometricToleranceKind::Straightness,
        b"CIRCULAR" => GeometricToleranceKind::Roundness,
        b"CYLINDRICITY" => GeometricToleranceKind::Cylindricity,
        b"CONCENTRICITY" => GeometricToleranceKind::Concentricity,
        b"SYMMETRY" => GeometricToleranceKind::Symmetry,
        b"CIRCULAR RUNOUT" => GeometricToleranceKind::CircularRunout,
        b"TOTAL RUNOUT" => GeometricToleranceKind::TotalRunout,
        _ => GeometricToleranceKind::Other(std::str::from_utf8(value).ok()?.to_owned()),
    })
}

#[cfg(test)]
mod tests;
