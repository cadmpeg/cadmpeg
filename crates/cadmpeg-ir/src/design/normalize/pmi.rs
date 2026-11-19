// SPDX-License-Identifier: Apache-2.0
//! The tolerance and PMI spellings of §8.
//!
//! A limits-and-fits designation states its letter and its grade apart, so
//! the combined form (`G6` whole in `form_variance`, with an empty `grade`)
//! is split, its letter in capitals. A `DimensionModifier::Other` or
//! `ToleranceZoneForm::Other` whose text is one of its type's own names,
//! compared without regard to ASCII case, is that name's arm.

use crate::design::parameter::Tolerance;
use crate::document::{Design, Model};
use crate::pmi::zones::ToleranceZoneForm;
use crate::pmi::{DimensionModifier, LimitsAndFits, PmiDefinition};

/// Puts every design parameter's tolerance into its one spelling.
pub(super) fn normalize_design(design: &mut Design) {
    for parameter in &mut design.parameters {
        if let Some(tolerance) = &mut parameter.tolerance {
            normalize_tolerance(tolerance);
        }
    }
}

/// Puts every PMI dimension's tolerance and modifiers, and every tolerance
/// zone's form, into its one spelling.
pub(super) fn normalize_model(model: &mut Model) {
    for annotation in &mut model.pmi {
        match &mut annotation.definition {
            PmiDefinition::Dimension(dimension) => {
                if let Some(tolerance) = &mut dimension.tolerance {
                    normalize_tolerance(tolerance);
                }
                normalize_modifiers(&mut dimension.modifiers);
            }
            PmiDefinition::GeometricTolerance { zones, .. } => {
                for zone in zones {
                    normalize_zone_form(&mut zone.form);
                }
            }
            PmiDefinition::Datum { .. }
            | PmiDefinition::DatumSystem { .. }
            | PmiDefinition::DatumTarget { .. }
            | PmiDefinition::Presentation { .. } => {}
        }
    }
}

fn normalize_tolerance(tolerance: &mut Tolerance) {
    if let Tolerance::Fit { fit, mate, .. } = tolerance {
        split_designation(fit);
        if let Some(mate) = mate {
            split_designation(mate);
        }
    }
}

/// Splits a designation stated whole in `form_variance`, letters then digits
/// with an empty `grade`, into its letter, in capitals, and its grade.
fn split_designation(fit: &mut LimitsAndFits) {
    if !fit.grade.is_empty() {
        return;
    }
    let Some(split) = fit.form_variance.find(|c: char| c.is_ascii_digit()) else {
        return;
    };
    let (letters, grade) = fit.form_variance.split_at(split);
    if letters.is_empty()
        || !letters.chars().all(|c| c.is_ascii_alphabetic())
        || !grade.chars().all(|c| c.is_ascii_digit())
    {
        return;
    }
    let (letters, grade) = (letters.to_ascii_uppercase(), grade.to_owned());
    fit.form_variance = letters;
    fit.grade = grade;
}

/// Replaces each `Other` modifier that names an arm by that arm. A dimension
/// states each modifier once, so a replaced modifier that the list already
/// states is dropped.
fn normalize_modifiers(modifiers: &mut Vec<DimensionModifier>) {
    if !modifiers
        .iter()
        .any(|modifier| matches!(modifier, DimensionModifier::Other(text) if modifier_named(text).is_some()))
    {
        return;
    }
    let mut kept: Vec<DimensionModifier> = Vec::with_capacity(modifiers.len());
    for modifier in modifiers.drain(..) {
        let modifier = match &modifier {
            DimensionModifier::Other(text) => match modifier_named(text) {
                Some(arm) => arm,
                None => {
                    kept.push(modifier);
                    continue;
                }
            },
            DimensionModifier::Reference
            | DimensionModifier::Statistical
            | DimensionModifier::ControlledRadius => modifier,
        };
        if !kept.contains(&modifier) {
            kept.push(modifier);
        }
    }
    *modifiers = kept;
}

/// The modifier arm a text names: `Reference`, which STEP names
/// `auxiliary`, `Statistical`, or `ControlledRadius`.
fn modifier_named(text: &str) -> Option<DimensionModifier> {
    [
        ("reference", DimensionModifier::Reference),
        ("auxiliary", DimensionModifier::Reference),
        ("statistical", DimensionModifier::Statistical),
        ("controlled radius", DimensionModifier::ControlledRadius),
    ]
    .into_iter()
    .find_map(|(name, arm)| text.eq_ignore_ascii_case(name).then_some(arm))
}

fn normalize_zone_form(form: &mut ToleranceZoneForm) {
    let ToleranceZoneForm::Other(text) = form else {
        return;
    };
    if text.eq_ignore_ascii_case("cylindrical or circular") {
        *form = ToleranceZoneForm::CylindricalOrCircular;
    } else if text.eq_ignore_ascii_case("spherical") {
        *form = ToleranceZoneForm::Spherical;
    }
}
