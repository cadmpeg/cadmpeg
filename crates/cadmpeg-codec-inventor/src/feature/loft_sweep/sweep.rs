// SPDX-License-Identifier: Apache-2.0
//! Solid sweep operands and first-refusal losses.

use cadmpeg_ir::features::parameters::ParameterValue;
use cadmpeg_ir::features::sweeps::{SolidSweepOperation, SweepSection, SweepShape};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use crate::design::PmDcParameter;
use crate::feature::profile::selection::{SectionWire, SelectedPath};
use crate::feature::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectionIndex,
};
use crate::pmdc::{type_id_string, PmDcReference};

use super::{projection, SWEEP_MODE_TYPE, SWEEP_ORIENTATION_TYPE, SWEEP_SCALING_TYPE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Slots,
    Slot(usize),
    OrientationMajor(u8),
    ResultCollection,
    Angle,
    Dimension,
    SectionSelection,
    Operation,
    Section,
    PathSelection,
    Path,
    PathCorner,
    ResultTopology,
    ResultIdentity,
}

impl NativeReason {
    pub(crate) fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        let reason = match self {
            Self::Slots => "property vector lacks the sweep operand slots".to_owned(),
            Self::Slot(slot) => format!("property slot {slot} differs from the solid sweep form"),
            Self::OrientationMajor(major) => format!(
                "the type of the slot 9 orientation enumeration at segment major {major} is not read"
            ),
            Self::ResultCollection => "slot 15 does not select one result body".to_owned(),
            Self::Angle => {
                "slot 3 does not resolve to zero nominal and model angular values".to_owned()
            }
            Self::Dimension => "slot 7 dimension does not select the slot 3 parameter".to_owned(),
            Self::SectionSelection => "slot 0 does not select one zero-control profile".to_owned(),
            Self::Operation => {
                "slot 2 selects an operation other than new body, cut, or join".to_owned()
            }
            Self::Section => {
                "section does not resolve to one closed profile of whole lines and arcs".to_owned()
            }
            Self::PathSelection => "slot 1 does not select a zero-control path profile".to_owned(),
            Self::Path => "path does not resolve to one complete ordered open wire".to_owned(),
            Self::PathCorner => {
                "path has a corner or a curve that is neither a line nor an arc".to_owned()
            }
            Self::ResultTopology => {
                "generator-owned result faces do not match the section and path curve pairs"
                    .to_owned()
            }
            Self::ResultIdentity => "result body identity does not resolve".to_owned(),
        };
        crate::loss::InventorLossCode::FeatureSweepControlsUnresolved
            .note(format!("Retained Inventor sweep natively: {reason}."))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Operands {
    section: PmDcReference,
    path: PmDcReference,
    op: SolidSweepOperation,
}

/// The type value of the orientation enumeration at a segment major: type 1
/// at majors 19 and 23, type 2 at majors 25 and 31. No other major has an
/// evidenced form.
fn orientation_type(major: u8) -> Option<i16> {
    match major {
        19 | 23 => Some(1),
        25 | 31 => Some(2),
        _ => None,
    }
}

pub(crate) fn operands<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
    dimension: impl Fn(PmDcReference) -> Option<&'a crate::feature::revolve::Dimension>,
) -> Result<Operands, NativeReason> {
    let slots = source.properties.references();
    if source.properties.marker() != 2 || slots.len() < 16 {
        return Err(NativeReason::Slots);
    }
    let result = property(slots[15]).ok_or(NativeReason::ResultCollection)?;
    if result.identity.type_id.as_str() != type_id_string(crate::feature::OBJECT_COLLECTION_TYPE)
        || !matches!(&result.kind, PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection, items,
    } if items.references().len() == 1 && items.references()[0].index() != 0)
    {
        return Err(NativeReason::ResultCollection);
    }
    for (slot, reference) in slots.iter().enumerate() {
        if reference.index() != 0 && ![0, 1, 2, 3, 4, 7, 8, 9, 10, 15].contains(&slot) {
            return Err(NativeReason::Slot(slot));
        }
    }
    let operation = property(slots[2]).ok_or(NativeReason::Slot(2))?;
    let op = match operation.kind {
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PartOperation,
            type_value: 5,
            value,
        } if operation.identity.type_id.as_str()
            == type_id_string(crate::feature::PART_OPERATION_TYPE) =>
        {
            match value {
                1 => SolidSweepOperation::NewBody,
                2 => SolidSweepOperation::Cut,
                3 => SolidSweepOperation::Join,
                _ => return Err(NativeReason::Operation),
            }
        }
        _ => return Err(NativeReason::Slot(2)),
    };
    let extent = property(slots[4]).ok_or(NativeReason::Slot(4))?;
    let extent_type = if source.save_version_major >= crate::feature::EXTENT_TYPE_12_FIRST_MAJOR {
        12
    } else {
        11
    };
    if extent.identity.type_id.as_str() != type_id_string(crate::feature::EXTENT_TYPE)
        || !matches!(extent.kind, PmDcFeaturePropertyKind::Enumeration { family: PmDcFeatureEnumFamily::Extent, type_value, value: 3 }
            if type_value == extent_type)
    {
        return Err(NativeReason::Slot(4));
    }
    for (slot, type_id, expected_type) in [
        (8, SWEEP_MODE_TYPE, Some(3)),
        (
            9,
            SWEEP_ORIENTATION_TYPE,
            orientation_type(source.save_version_major),
        ),
        (10, SWEEP_SCALING_TYPE, Some(2)),
    ] {
        let Some(expected_type) = expected_type else {
            return Err(NativeReason::OrientationMajor(source.save_version_major));
        };
        if !projection::zero_control(
            property(slots[slot]).ok_or(NativeReason::Slot(slot))?,
            type_id,
            expected_type,
        ) {
            return Err(NativeReason::Slot(slot));
        }
    }
    let (raw, value) = parameter(slots[3]).ok_or(NativeReason::Angle)?;
    if raw.identity.type_id.as_str() != type_id_string(crate::design::PARAMETER_FULL_TYPE)
        || raw.nominal_value.get() != 0.0
        || raw.model_value.get() != 0.0
        || !matches!(value, ParameterValue::Angle(value) if value.get() == 0.0)
    {
        return Err(NativeReason::Angle);
    }
    if slots[7].index() != 0 {
        let p = property(slots[7]).ok_or(NativeReason::Dimension)?;
        if p.identity.type_id.as_str() != type_id_string(crate::feature::FEATURE_DIMENSIONS_TYPE) {
            return Err(NativeReason::Dimension);
        }
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            items,
        } = &p.kind
        else {
            return Err(NativeReason::Dimension);
        };
        let [item] = items.references() else {
            return Err(NativeReason::Dimension);
        };
        if dimension(*item)
            .ok_or(NativeReason::Dimension)?
            .parameter
            .index()
            != slots[3].index()
        {
            return Err(NativeReason::Dimension);
        }
    }
    let boundary = property(slots[0]).ok_or(NativeReason::SectionSelection)?;
    if boundary.identity.type_id.as_str() != type_id_string(crate::feature::BOUNDARY_PATCH_TYPE) {
        return Err(NativeReason::SectionSelection);
    }
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::BoundaryPatch,
        items,
    } = &boundary.kind
    else {
        return Err(NativeReason::SectionSelection);
    };
    let [selection] = items.references() else {
        return Err(NativeReason::SectionSelection);
    };
    let section = selection_link(property(*selection).ok_or(NativeReason::SectionSelection)?)
        .ok_or(NativeReason::SectionSelection)?;
    let path = selection_link(property(slots[1]).ok_or(NativeReason::PathSelection)?)
        .ok_or(NativeReason::PathSelection)?;
    Ok(Operands { section, path, op })
}

fn selection_link(property: &PmDcFeatureProperty) -> Option<PmDcReference> {
    if ![
        crate::feature::PROFILE_SELECTION_TYPE,
        crate::feature::PLATE_PROFILE_SELECTION_TYPE,
    ]
    .into_iter()
    .any(|type_id| property.identity.type_id.as_str() == type_id_string(type_id))
    {
        return None;
    }
    match property.kind {
        PmDcFeaturePropertyKind::ProfileSelection {
            entity_link,
            value: 0,
        } if entity_link.index() != 0 => Some(entity_link),
        _ => None,
    }
}

/// Check the generator-owned faces of the result against the section and the
/// path.
///
/// A new body of one whole circle section keeps its own form: the path can
/// stay a native reference, and the faces are checked against the circle
/// radius and the path curves that the keyed wire joins. The line and arc
/// partition needs the model-space span of every path edge and tangent
/// continuity between them.
fn prove(
    result: &cadmpeg_ir::CadIr,
    generator: u32,
    op: SolidSweepOperation,
    circle: Option<(f64, i64)>,
    path: &SelectedPath,
    section: Option<&SectionWire>,
) -> Result<(), NativeReason> {
    if let Some((radius, name)) = circle {
        return projection::evidence::sweep(
            result,
            generator,
            radius,
            name,
            &path
                .names
                .iter()
                .enumerate()
                .map(|(at, name)| (*name, path.curves.get(at).copied()))
                .collect::<Vec<_>>(),
            path.endpoints,
        )
        .ok_or(NativeReason::ResultTopology);
    }
    let section = section.ok_or(NativeReason::Section)?;
    if path.spans.is_empty() {
        return Err(NativeReason::Path);
    }
    if !path.tangent {
        return Err(NativeReason::PathCorner);
    }
    projection::evidence::sweep_partition(
        result,
        generator,
        op,
        &section.curves,
        section.plane,
        &path.spans,
    )
    .ok_or(NativeReason::ResultTopology)
}

/// The loss of the slot 9 orientation law for a typed sweep whose section is
/// not one whole circle.
pub(super) fn orientation_law_loss() -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::InventorLossCode::FeatureSweepControlsUnresolved.note(
        "Typed Inventor sweep leaves its orientation law unstated: slot 9 value 0 turns the section with the path, and the neutral sweep carries no orientation for a non-circular section."
            .to_owned(),
    )
}

pub(super) fn project(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Result<(FeatureOperation, Option<cadmpeg_ir::report::loss::LossNote>), NativeReason> {
    let token = source.identity.segment_token.as_str();
    let links = operands(
        source,
        |r| crate::feature::resolve_property(token, r.index(), index),
        |r| {
            let raw = index
                .parameters
                .get(&(token, r.record_ordinal()?))?
                .as_ref()?;
            let value = index
                .parameter_values
                .get(raw.id().as_str())?
                .as_ref()?
                .value
                .as_ref()?;
            Some((*raw, value))
        },
        |r| {
            index
                .dimensions
                .get(&(token, r.record_ordinal()?))?
                .as_ref()
                .copied()
        },
    )?;
    let sketches = projection::participant_sketches(source, label, index);
    let section_link = links
        .section
        .record_ordinal()
        .ok_or(NativeReason::Section)?;
    let path_property =
        crate::feature::slot_property(source, 1, index).ok_or(NativeReason::PathSelection)?;
    let path = index
        .profiles
        .selected_path(
            token,
            links.path.record_ordinal().ok_or(NativeReason::Path)?,
            path_property.id(),
            &sketches,
        )
        .ok_or(NativeReason::Path)?;
    let result = index.result_model.ok_or(NativeReason::ResultTopology)?;
    // A new body whose section is one whole circle keeps its form, whatever
    // the path.
    let mut circles = sketches.iter().filter_map(|(ordinal, sketch)| {
        index.profiles.circle(token, *ordinal, sketch, section_link)
    });
    let circular = circles
        .next()
        .filter(|_| circles.next().is_none() && links.op == SolidSweepOperation::NewBody);
    let circle = circular.as_ref().map(|(_, radius, name)| (*radius, *name));
    let (profile, section) = match circular {
        Some((profile, ..)) => (profile, None),
        None => {
            let regions = sketches
                .iter()
                .filter_map(|(ordinal, sketch)| {
                    Some((
                        *ordinal,
                        *sketch,
                        index
                            .profiles
                            .regions(token, *ordinal, sketch, &[section_link])?,
                    ))
                })
                .collect::<Vec<_>>();
            let [(section_ordinal, section_sketch, profile)] = regions.as_slice() else {
                return Err(NativeReason::Section);
            };
            let section = index
                .profiles
                .section_curves(token, section_link, *section_ordinal, section_sketch)
                .ok_or(NativeReason::Section)?;
            (profile.clone(), Some(section))
        }
    };
    prove(
        result,
        source.header.source_index,
        links.op,
        circle,
        &path,
        section.as_ref(),
    )?;
    // `orientation: None` states no orientation rule. The IR has no variant for
    // "turns with the path", so a non-circular section, whose built shape
    // depends on that law, reports it as dropped. A whole circle is unchanged
    // by the in-plane turn about its normal.
    let law_loss = circle.is_none().then(orientation_law_loss);
    let operation = FeatureOperation::Sweep {
        shape: SweepShape::Solid {
            op: links.op,
            section: SweepSection::Profile(profile),
            sections: Vec::new(),
        },
        path: Some(path.reference),
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    };
    Ok((operation, law_loss))
}

pub(crate) fn validates<'a>(
    source: &PmDcFeature,
    definition: &FeatureDefinition,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
    dimension: impl Fn(PmDcReference) -> Option<&'a crate::feature::revolve::Dimension>,
) -> bool {
    matches!(definition, FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape: SweepShape::Solid { op, section: SweepSection::Profile(_), sections },
        path: Some(_), orientation: None, transition: None, transformation: None,
        path_tangent: false, linearize: false, twist: None, path_extent: None, guide_rail: None,
        taper: None, scale: None, allow_multi_profile_faces: None,
    }) if sections.is_empty()
        && operands(source, property, parameter, dimension).is_ok_and(|operands| operands.op == *op))
}

#[cfg(test)]
mod tests;
