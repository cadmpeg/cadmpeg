// SPDX-License-Identifier: Apache-2.0
//! Attached-plate dimensions and retained construction controls.

use cadmpeg_ir::features::{
    SheetMetalRelief, SheetMetalReliefForm, SheetMetalThicknessSide, SheetMetalTransition,
    SheetMetalTransitionForm,
};
use cadmpeg_ir::scalar::{Length, NonNegativeLength, PositiveLength};
use cadmpeg_ir::units::UnitVector3;

use super::{style::Style, SheetMetalProperty};
use crate::feature::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily,
};
use crate::pmdc::PmDcReference;

const EPS_PLATE_DIRECTION: f64 = 1.0e-10;
const EPS_ATTACHMENT_DIRECTION: f64 = 1.0e-10;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlateOperands {
    pub(crate) thickness: PositiveLength,
    pub(crate) side: SheetMetalThicknessSide,
    pub(crate) inside_radius: PositiveLength,
    pub(crate) relief: SheetMetalRelief,
    pub(crate) transition: SheetMetalTransition,
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

fn boolean(property: &PmDcFeatureProperty) -> Option<bool> {
    match property.kind {
        PmDcFeaturePropertyKind::Boolean { value, .. } => Some(value),
        _ => None,
    }
}

fn enumeration(property: &PmDcFeatureProperty) -> Option<(i16, u16)> {
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
    style: &Style,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: impl Fn(PmDcReference) -> Option<Length>,
    direction: impl Fn(PmDcReference) -> Option<UnitVector3>,
    normal: UnitVector3,
    u_axis: UnitVector3,
) -> Option<PlateOperands> {
    let slots = source.properties.references();
    if slots.len() != 17
        || style.save_version_major != source.save_version_major
        || [5, 6, 14].iter().any(|&slot| slots[slot].index() != 0)
        || !matches!(
            property(slots[16])?.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 3
            }
        )
        || property(slots[0])?.identity.type_id.as_str()
            != crate::pmdc::type_id_string(crate::feature::PLATE_PROFILE_SELECTION_TYPE)
        || !matches!(
            property(slots[0])?.kind,
            PmDcFeaturePropertyKind::ProfileSelection { value: 0, .. }
        )
        || !matches!(
            &property(slots[7])?.kind,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items,
            } if items.references().is_empty()
        )
        || !corner(property(slots[9])?, &property, &length)
        || !matches!(
            property(slots[8])?.kind,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                ..
            }
        )
        || !matches!(
            sheet(property(slots[13])?)?,
            SheetMetalProperty::PlateControl { value: 0 }
        )
    {
        return None;
    }
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
    } = sheet(property(slots[4])?)?
    else {
        return None;
    };
    let plate_direction = direction(slots[1])?;
    let first_direction = direction(directions[0])?;
    let second_direction = direction(directions[1])?;
    let v_axis = UnitVector3::normalized(normal.as_raw().cross(*u_axis.as_raw()))?;
    if plate_direction.as_raw().dot(*first_direction.as_raw()) > -1.0 + EPS_PLATE_DIRECTION
        || first_direction.as_raw().dot(*u_axis.as_raw()).abs() < 1.0 - EPS_ATTACHMENT_DIRECTION
        || second_direction.as_raw().dot(*v_axis.as_raw()) < 1.0 - EPS_ATTACHMENT_DIRECTION
        || !same(*thickness, slots[3])
        || boolean(property(*control)?)?
        || boolean(property(*first_option)?)?
        || boolean(property(*second_option)?)?
        || enumeration(property(*first_mode)?)? != (9, 1)
        || enumeration(property(*second_mode)?)? != (9, 1)
        || states.as_ref() != [1, 1, 1]
    {
        return None;
    }
    if let Some(reference) = extension {
        let SheetMetalProperty::AttachmentState {
            state: 10,
            thickness: other_thickness,
            radius: other_radius,
            attachment,
            relief_width: other_width,
            minimum_remnant: other_remnant,
            options,
        } = sheet(property(*reference)?)?
        else {
            return None;
        };
        if !same(*other_thickness, *thickness)
            || !same(*other_radius, *radius)
            || !same(*attachment, slots[4])
            || !same(*other_width, *relief_width)
            || !same(*other_remnant, *minimum_remnant)
            || !same(options[0], *first_option)
            || !same(options[1], *second_option)
        {
            return None;
        }
    }
    let SheetMetalProperty::Relief {
        control,
        width,
        depth,
        form,
        minimum_remnant: relief_remnant,
        seam_gap,
    } = sheet(property(slots[10])?)?
    else {
        return None;
    };
    if boolean(property(*control)?) != Some(true)
        || !same(*width, *relief_width)
        || !same(*relief_remnant, *minimum_remnant)
        || enumeration(property(*form)?)? != enumeration(property(style.bend_relief_form)?)?
    {
        return None;
    }
    let relief = SheetMetalRelief {
        form: SheetMetalReliefForm::StyleDefault,
        width: PositiveLength::try_from(length(*width)?).ok()?,
        depth: PositiveLength::try_from(length(*depth)?).ok()?,
        minimum_remnant: NonNegativeLength::try_from(length(*minimum_remnant)?).ok()?,
        seam_gap: NonNegativeLength::try_from(length(*seam_gap)?).ok()?,
    };
    let SheetMetalProperty::Transition {
        form,
        control,
        radius: transition_radius,
    } = sheet(property(slots[11])?)?
    else {
        return None;
    };
    if boolean(property(*control)?) != Some(true)
        || enumeration(property(*form)?)? != enumeration(property(style.transition_form)?)?
    {
        return None;
    }
    Some(PlateOperands {
        thickness: PositiveLength::try_from(length(*thickness)?).ok()?,
        side: flange_side(property(slots[2])?)?,
        inside_radius: PositiveLength::try_from(length(*radius)?).ok()?,
        relief,
        transition: SheetMetalTransition {
            form: SheetMetalTransitionForm::StyleDefault,
            radius: NonNegativeLength::try_from(length(*transition_radius)?).ok()?,
        },
    })
}

fn corner<'a>(
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
    if p.len() != 17
        || b.len() != 11
        || plate.identity.segment_token != bend.identity.segment_token
        || b[5].index() != 0
        || ![(7, 0), (3, 2), (4, 3), (10, 4), (11, 7), (15, 9)]
            .iter()
            .all(|&(left, right)| same(p[left], b[right]))
    {
        return false;
    }
    let Some(SheetMetalProperty::Attachment { radius, .. }) = property(p[4]).and_then(sheet) else {
        return false;
    };
    same(*radius, b[1])
        && property(b[8]).and_then(sheet).is_some_and(|kind| {
            matches!(
                kind,
                SheetMetalProperty::BendControl {
                    value: 6,
                    state: Some(1)
                }
            )
        })
        && property(b[10]).and_then(boolean) == Some(true)
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
