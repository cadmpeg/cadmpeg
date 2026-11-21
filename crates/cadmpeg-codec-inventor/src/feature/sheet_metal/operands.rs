// SPDX-License-Identifier: Apache-2.0
//! Attached-plate dimensions and retained construction controls.

use cadmpeg_ir::features::sheet_metal_forms::{
    SheetMetalRelief, SheetMetalReliefForm, SheetMetalThicknessSide, SheetMetalTransition,
    SheetMetalTransitionForm,
};
use cadmpeg_ir::scalar::{Length, NonNegativeLength, PositiveLength};
use cadmpeg_ir::units::UnitVector3;

use super::refusal::{require, PlateRefusal, Refuse};
use super::{style::Style, SheetMetalProperty};
use crate::feature::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily,
};
use crate::pmdc::PmDcReference;

const EPS_PLATE_DIRECTION: f64 = 1.0e-10;
const EPS_ATTACHMENT_DIRECTION: f64 = 1.0e-10;
/// Smallest V component of an oblique second attachment direction; a smaller
/// component leaves the plate plane parallel to the attached face.
const EPS_OBLIQUE_SECOND_DIRECTION: f64 = 1.0e-6;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlateOperands {
    pub(crate) thickness: PositiveLength,
    pub(crate) side: SheetMetalThicknessSide,
    pub(crate) inside_radius: PositiveLength,
    pub(crate) relief: SheetMetalRelief,
    pub(crate) transition: SheetMetalTransition,
}

/// The property layout of an attached plate.
///
/// The form fixes the plate property count, the slot that holds the result,
/// and the paired bend layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlateForm {
    /// 15 properties; slot 6 holds the result surface body.
    Body,
    /// 17 properties; slot 15 holds the result object collection and slot 16
    /// the part operation.
    Collection,
}

impl PlateForm {
    /// The form with `count` plate properties.
    pub(crate) const fn of_count(count: usize) -> Option<Self> {
        match count {
            15 => Some(Self::Body),
            17 => Some(Self::Collection),
            _ => None,
        }
    }

    /// The form of an attached plate record.
    pub(crate) fn of(source: &PmDcFeature) -> Option<Self> {
        Self::of_count(source.properties.references().len())
    }

    /// The plate slot that holds the result.
    pub(crate) const fn result_slot(self) -> usize {
        match self {
            Self::Body => 6,
            Self::Collection => 15,
        }
    }

    /// The paired bend slot that holds the result.
    pub(crate) const fn bend_result_slot(self) -> usize {
        match self {
            Self::Body => 5,
            Self::Collection => 9,
        }
    }

    /// The paired bend property count.
    pub(crate) const fn bend_count(self) -> usize {
        match self {
            Self::Body => 9,
            Self::Collection => 11,
        }
    }
}

pub(crate) fn same(a: PmDcReference, b: PmDcReference) -> bool {
    a.index() == b.index()
}

pub(crate) fn sheet(property: &PmDcFeatureProperty) -> Option<&SheetMetalProperty> {
    match &property.kind {
        PmDcFeaturePropertyKind::SheetMetal { definition } => Some(definition),
        _ => None,
    }
}

pub(crate) fn boolean(property: &PmDcFeatureProperty) -> Option<bool> {
    match property.kind {
        PmDcFeaturePropertyKind::Boolean { value, .. } => Some(value),
        _ => None,
    }
}

pub(crate) fn enumeration(property: &PmDcFeatureProperty) -> Option<(i16, u16)> {
    match sheet(property)? {
        SheetMetalProperty::Enumeration { type_value, value } => Some((*type_value, *value)),
        _ => None,
    }
}

fn flange_side(property: &PmDcFeatureProperty) -> Option<SheetMetalThicknessSide> {
    match property.kind {
        PmDcFeaturePropertyKind::Boolean { value: false, .. }
        | PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PlateSide,
            type_value: 2,
            value: 0,
        } => Some(SheetMetalThicknessSide::Forward),
        _ => None,
    }
}

pub(crate) fn plate<'a>(
    source: &PmDcFeature,
    style: Option<&Style>,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: impl Fn(PmDcReference) -> Option<Length>,
    direction: impl Fn(PmDcReference) -> Option<UnitVector3>,
    normal: UnitVector3,
    u_axis: UnitVector3,
) -> Result<PlateOperands, PlateRefusal> {
    let style = style.refuse(PlateRefusal::Style)?;
    let slots = source.properties.references();
    let form = PlateForm::of(source).refuse(PlateRefusal::PropertyCount)?;
    require(
        style.save_version_major == source.save_version_major,
        PlateRefusal::StyleVersion,
    )?;
    let reserved: &[usize] = match form {
        PlateForm::Body => &[5, 14],
        PlateForm::Collection => &[5, 6, 14],
    };
    require(
        reserved.iter().all(|&slot| slots[slot].index() == 0),
        PlateRefusal::ReservedSlot,
    )?;
    match form {
        PlateForm::Body => require(
            matches!(
                property(slots[6]).map(|p| &p.kind),
                Some(PmDcFeaturePropertyKind::SurfaceBody { .. })
            ),
            PlateRefusal::ResultBody,
        )?,
        PlateForm::Collection => require(
            matches!(
                property(slots[16]).map(|p| &p.kind),
                Some(PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::PartOperation,
                    type_value: 5,
                    value: 3
                })
            ),
            PlateRefusal::PartOperation,
        )?,
    }
    let selection = property(slots[0]).refuse(PlateRefusal::ProfileSelection)?;
    require(
        selection.identity.type_id.as_str()
            == crate::pmdc::type_id_string(crate::feature::PLATE_PROFILE_SELECTION_TYPE)
            && matches!(
                selection.kind,
                PmDcFeaturePropertyKind::ProfileSelection { value: 0, .. }
            ),
        PlateRefusal::ProfileSelection,
    )?;
    require(
        matches!(
            property(slots[7]).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items,
            }) if items.references().is_empty()
        ),
        PlateRefusal::EdgeCollection,
    )?;
    require(
        property(slots[9]).is_some_and(|p| corner(p, &property, &length)),
        PlateRefusal::CornerControl,
    )?;
    require(
        matches!(
            property(slots[8]).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                ..
            })
        ),
        PlateRefusal::FeatureDimensions,
    )?;
    require(
        property(slots[13])
            .and_then(sheet)
            .is_some_and(|kind| matches!(kind, SheetMetalProperty::PlateControl { value: 0 })),
        PlateRefusal::PlateControl,
    )?;
    let SheetMetalProperty::Attachment {
        control,
        thickness,
        first_mode,
        directions,
        radius,
        second_mode,
        relief_width,
        minimum_remnant,
        first_option,
        states,
        second_option,
        extension,
    } = property(slots[4])
        .and_then(sheet)
        .refuse(PlateRefusal::AttachmentControl)?
    else {
        return Err(PlateRefusal::AttachmentControl);
    };
    let plate_direction = direction(slots[1]).refuse(PlateRefusal::Direction)?;
    let first_direction = direction(directions[0]).refuse(PlateRefusal::Direction)?;
    let second_direction = direction(directions[1]).refuse(PlateRefusal::Direction)?;
    let v_axis = UnitVector3::normalized(normal.as_raw().cross(*u_axis.as_raw()))
        .refuse(PlateRefusal::SecondDirection)?;
    require(
        plate_direction.as_raw().dot(*first_direction.as_raw()) <= -1.0 + EPS_PLATE_DIRECTION,
        PlateRefusal::PlateDirection,
    )?;
    require(
        first_direction.as_raw().dot(*u_axis.as_raw()).abs() >= 1.0 - EPS_ATTACHMENT_DIRECTION,
        PlateRefusal::FirstDirection,
    )?;
    // The second direction is perpendicular to the first direction. It has a
    // positive component along V, or it is antiparallel to V when the first
    // direction is antiparallel to U. An oblique second direction belongs to
    // a plate whose sketch plane is not perpendicular to the attached face.
    let along_v = second_direction.as_raw().dot(*v_axis.as_raw());
    let first_reversed = first_direction.as_raw().dot(*u_axis.as_raw()) < 0.0;
    require(
        second_direction
            .as_raw()
            .dot(*first_direction.as_raw())
            .abs()
            <= EPS_ATTACHMENT_DIRECTION
            && (along_v >= EPS_OBLIQUE_SECOND_DIRECTION
                || (first_reversed && along_v <= -1.0 + EPS_ATTACHMENT_DIRECTION)),
        PlateRefusal::SecondDirection,
    )?;
    require(same(*thickness, slots[3]), PlateRefusal::ThicknessReference)?;
    require(
        [*control, *first_option, *second_option]
            .iter()
            .all(|&reference| property(reference).and_then(boolean) == Some(false)),
        PlateRefusal::AttachmentBoolean,
    )?;
    require(
        [*first_mode, *second_mode]
            .iter()
            .all(|&reference| property(reference).and_then(enumeration) == Some((9, 1))),
        PlateRefusal::AttachmentMode,
    )?;
    require(states.as_ref() == [1, 1, 1], PlateRefusal::AttachmentStates)?;
    if let Some(reference) = extension {
        require(
            extension_repeats(
                property(*reference),
                slots[4],
                [*thickness, *radius, *relief_width, *minimum_remnant],
                [*first_option, *second_option],
            ),
            PlateRefusal::AttachmentExtension,
        )?;
    }
    let relief = relief(
        property(slots[10]),
        style,
        [*relief_width, *minimum_remnant],
        &property,
        &length,
    )?;
    let transition = transition(property(slots[11]), style, &property, &length)?;
    Ok(PlateOperands {
        thickness: length(*thickness)
            .and_then(|v| PositiveLength::try_from(v).ok())
            .refuse(PlateRefusal::Length)?,
        side: property(slots[2])
            .and_then(flange_side)
            .refuse(PlateRefusal::Side)?,
        inside_radius: length(*radius)
            .and_then(|v| PositiveLength::try_from(v).ok())
            .refuse(PlateRefusal::Length)?,
        relief,
        transition,
    })
}

/// Whether an attachment extension is the state-10 record that repeats the
/// attachment's thickness, radius, relief width, minimum remnant, and option
/// references, and names `attachment`.
pub(crate) fn extension_repeats(
    extension: Option<&PmDcFeatureProperty>,
    attachment: PmDcReference,
    lengths: [PmDcReference; 4],
    options: [PmDcReference; 2],
) -> bool {
    let Some(SheetMetalProperty::AttachmentState {
        state: 10,
        thickness,
        radius,
        attachment: other_attachment,
        relief_width,
        minimum_remnant,
        options: other_options,
    }) = extension.and_then(sheet)
    else {
        return false;
    };
    same(*other_attachment, attachment)
        && [*thickness, *radius, *relief_width, *minimum_remnant]
            .iter()
            .zip(lengths)
            .all(|(a, b)| same(*a, b))
        && other_options.iter().zip(options).all(|(a, b)| same(*a, b))
}

/// The bend relief of a plate: a relief record whose width and minimum
/// remnant repeat the attachment references `[width, minimum_remnant]` and
/// whose form is the document-style bend-relief form.
pub(crate) fn relief<'a>(
    source: Option<&PmDcFeatureProperty>,
    style: &Style,
    attachment: [PmDcReference; 2],
    property: &impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: &impl Fn(PmDcReference) -> Option<Length>,
) -> Result<SheetMetalRelief, PlateRefusal> {
    let Some(SheetMetalProperty::Relief {
        control,
        width,
        depth,
        form,
        minimum_remnant,
        seam_gap,
    }) = source.and_then(sheet)
    else {
        return Err(PlateRefusal::Relief);
    };
    let style_relief = property(style.bend_relief_form).and_then(enumeration);
    require(
        property(*control).and_then(boolean).is_some()
            && same(*width, attachment[0])
            && same(*minimum_remnant, attachment[1])
            && property(*form).and_then(enumeration).is_some()
            && property(*form).and_then(enumeration) == style_relief,
        PlateRefusal::Relief,
    )?;
    Ok(SheetMetalRelief {
        form: SheetMetalReliefForm::StyleDefault,
        width: length(*width)
            .and_then(|v| PositiveLength::try_from(v).ok())
            .refuse(PlateRefusal::Length)?,
        depth: length(*depth)
            .and_then(|v| PositiveLength::try_from(v).ok())
            .refuse(PlateRefusal::Length)?,
        minimum_remnant: length(*minimum_remnant)
            .and_then(|v| NonNegativeLength::try_from(v).ok())
            .refuse(PlateRefusal::Length)?,
        seam_gap: length(*seam_gap)
            .and_then(|v| NonNegativeLength::try_from(v).ok())
            .refuse(PlateRefusal::Length)?,
    })
}

/// The transition of a plate: a transition record with a true control and
/// the document-style transition form.
pub(crate) fn transition<'a>(
    source: Option<&PmDcFeatureProperty>,
    style: &Style,
    property: &impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: &impl Fn(PmDcReference) -> Option<Length>,
) -> Result<SheetMetalTransition, PlateRefusal> {
    let Some(SheetMetalProperty::Transition {
        form,
        control,
        radius,
    }) = source.and_then(sheet)
    else {
        return Err(PlateRefusal::Transition);
    };
    let style_transition = property(style.transition_form).and_then(enumeration);
    require(
        property(*control).and_then(boolean).is_some()
            && property(*form).and_then(enumeration).is_some()
            && property(*form).and_then(enumeration) == style_transition,
        PlateRefusal::Transition,
    )?;
    Ok(SheetMetalTransition {
        form: SheetMetalTransitionForm::StyleDefault,
        radius: length(*radius)
            .and_then(|v| NonNegativeLength::try_from(v).ok())
            .refuse(PlateRefusal::Length)?,
    })
}

pub(crate) fn corner<'a>(
    source: &PmDcFeatureProperty,
    property: &impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: &impl Fn(PmDcReference) -> Option<Length>,
) -> bool {
    let Some(SheetMetalProperty::Corner {
        control,
        gap,
        size,
        relief_form,
        corner_form,
        secondary_form,
        jacobi_radius,
        value: 0,
        option,
    }) = sheet(source)
    else {
        return false;
    };
    // The corner child feature owns these values; the plate only requires the
    // shared control to be well formed.
    [*control, *option]
        .iter()
        .all(|reference| property(*reference).and_then(boolean).is_some())
        && [*relief_form, *corner_form, *secondary_form]
            .iter()
            .all(|reference| property(*reference).and_then(enumeration).is_some())
        && [*gap, *size, *jacobi_radius]
            .iter()
            .all(|reference| length(*reference).is_some_and(|value| value.get() >= 0.0))
}

pub(crate) fn bend<'a>(
    plate: &PmDcFeature,
    bend: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    let p = plate.properties.references();
    let b = bend.properties.references();
    let Some(form) = PlateForm::of(plate) else {
        return false;
    };
    if b.len() != form.bend_count() || plate.identity.segment_token != bend.identity.segment_token {
        return false;
    }
    let control = property(b[8]).and_then(sheet);
    let shared: &[(usize, usize)] = match form {
        PlateForm::Body => &[(7, 0), (3, 2), (4, 3), (10, 4), (6, 5), (11, 7)],
        PlateForm::Collection => &[(7, 0), (3, 2), (4, 3), (10, 4), (11, 7), (15, 9)],
    };
    let form_controls = match form {
        PlateForm::Body => {
            matches!(
                control,
                Some(SheetMetalProperty::BendControl {
                    value: 0,
                    state: None
                })
            )
        }
        PlateForm::Collection => {
            b[5].index() == 0
                && matches!(
                    control,
                    Some(SheetMetalProperty::BendControl {
                        value: 6,
                        state: Some(1)
                    })
                )
                && property(b[10]).and_then(boolean) == Some(true)
        }
    };
    if !form_controls || !shared.iter().all(|&(left, right)| same(p[left], b[right])) {
        return false;
    }
    let Some(SheetMetalProperty::Attachment { radius, .. }) = property(p[4]).and_then(sheet) else {
        return false;
    };
    same(*radius, b[1])
        && property(b[6]).is_some_and(|property| {
            matches!(
                property.kind,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FeatureDimensions,
                    ..
                }
            )
        })
}

#[cfg(test)]
mod tests;

pub(crate) fn selection_sources<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> Option<Vec<u32>> {
    let root = property(*source.properties.references().get(12)?)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections,
        items,
    } = &root.kind
    else {
        return None;
    };
    let mut sources = Vec::new();
    for reference in items.references() {
        let SheetMetalProperty::EdgeSelection { edges } = sheet(property(*reference)?)? else {
            return None;
        };
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::EdgeCollection,
            items,
        } = &property(*edges)?.kind
        else {
            return None;
        };
        for reference in items.references() {
            let PmDcFeaturePropertyKind::EdgeItem {
                index_references,
                index_reference_value: -1,
                value: 0,
            } = &property(*reference)?.kind
            else {
                return None;
            };
            if index_references.values().is_empty() {
                return None;
            }
            for source in index_references.values() {
                if sources.contains(source) {
                    return None;
                }
                sources.push(*source);
            }
        }
    }
    (!sources.is_empty()).then_some(sources)
}

pub(crate) fn selection_count<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> Option<usize> {
    Some(selection_sources(source, property)?.len())
}
