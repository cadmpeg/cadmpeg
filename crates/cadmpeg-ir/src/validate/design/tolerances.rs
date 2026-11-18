// SPDX-License-Identifier: Apache-2.0
//! A design parameter's tolerance (§5.12, §8.1): the limits-and-fits
//! designations of a fit, and the general tolerance a `General` tolerance
//! takes from its definition.

use std::collections::BTreeMap;

use super::records::DesignRecords;
use crate::design::parameter::{DesignParameter, Deviations, ParameterValue, Tolerance};
use crate::design::units::{Quantity, ValueKind};
use crate::pmi::LimitsAndFits;
use crate::products::{AngularGeneral, LinearGeneral, LinearGeneralClass};
use crate::report::check::Finding;

pub(super) fn check_tolerances(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    for parameter in &index.ir.design.parameters {
        let id = parameter.id.as_str();
        match &parameter.tolerance {
            Some(Tolerance::Fit { fit, mate, .. }) => {
                check_designation(id, "fit", fit, findings);
                if let Some(mate) = mate {
                    check_designation(id, "mate", mate, findings);
                }
            }
            Some(Tolerance::General) => check_general(index, parameter, findings),
            Some(Tolerance::Deviations(_) | Tolerance::Basic | Tolerance::SingleLimit(_))
            | None => {}
        }
    }
}

/// A designation's `form_variance` holds capital letters only, and its
/// `grade` is not empty.
fn check_designation(
    entity: &str,
    what: &str,
    designation: &LimitsAndFits,
    findings: &mut Vec<Finding>,
) {
    let letters = &designation.form_variance;
    if letters.is_empty() || !letters.chars().all(|letter| letter.is_ascii_uppercase()) {
        super::error(
            findings,
            entity,
            format_args!("parameter's {what} designation {letters:?} is not capital letters only"),
        );
    }
    if designation.grade.is_empty() {
        super::error(
            findings,
            entity,
            format_args!("parameter's {what} designation states an empty grade"),
        );
    }
}

/// ISO 2768-1 table 1's ranges with a value in each class: above 0.5 up to
/// 2000 for `Fine`, to 4000 for `Medium` and `Coarse`, and above 3 up to 4000
/// for `VeryCoarse`, whose first cell is empty (§8.1).
fn iso2768_holds(class: LinearGeneralClass, size: f64) -> bool {
    let (above, up_to) = match class {
        LinearGeneralClass::Fine => (0.5, 2000.0),
        LinearGeneralClass::Medium | LinearGeneralClass::Coarse => (0.5, 4000.0),
        LinearGeneralClass::VeryCoarse => (3.0, 4000.0),
    };
    size.abs() > above && size.abs() <= up_to
}

/// `Tolerance::General` occurs only where the parameter's definition states
/// a general tolerance with the rule for its quantity, `linear` for a length
/// and `angular` for an angle, and that rule states an entry for it: under
/// `ByDecimals` one at its `decimals`, under `Iso2768` a class range that
/// holds the absolute value of its `Number` value.
fn check_general(
    index: &DesignRecords<'_>,
    parameter: &DesignParameter,
    findings: &mut Vec<Finding>,
) {
    let id = parameter.id.as_str();
    let scope = parameter.scope.as_str();
    let general = index
        .definitions
        .get(scope)
        .and_then(|definition| definition.general_tolerance.as_ref());
    let at_decimals = |map: &BTreeMap<u8, Deviations>| {
        parameter.decimals.is_some_and(|key| map.contains_key(&key))
    };
    let entry = match (&parameter.kind, general) {
        (ValueKind::Quantity(Quantity::LENGTH), Some(general)) => {
            general.linear.as_ref().map(|linear| match linear {
                LinearGeneral::Iso2768(class) => match &parameter.value {
                    Some(ParameterValue::Number(size)) => iso2768_holds(*class, size.get()),
                    _ => false,
                },
                LinearGeneral::ByDecimals(map) => at_decimals(map),
                LinearGeneral::Uniform(_) => true,
            })
        }
        (ValueKind::Quantity(Quantity::ANGLE), Some(general)) => {
            general.angular.as_ref().map(|angular| match angular {
                AngularGeneral::ByDecimals(map) => at_decimals(map),
                AngularGeneral::Uniform(_) => true,
            })
        }
        _ => None,
    };
    match entry {
        Some(true) => {}
        Some(false) => super::error(
            findings,
            id,
            format_args!(
                "parameter's tolerance is `general`, but the general tolerance of {scope} states no entry for it"
            ),
        ),
        None => super::error(
            findings,
            id,
            format_args!(
                "parameter's tolerance is `general`, but {scope} states no general tolerance rule for its quantity"
            ),
        ),
    }
}
