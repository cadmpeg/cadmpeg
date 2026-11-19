// SPDX-License-Identifier: Apache-2.0
//! The records of design parameters and spreadsheets (§5.12): names, owners,
//! kinds, units, cached values, and the sheets that own cell parameters.

use std::collections::{BTreeMap, BTreeSet};

use super::records::{DesignRecords, MemberKey};
use crate::attributes::AttributeValue;
use crate::design::container::Container;
use crate::design::edit::unit_factor;
use crate::design::feature::FeatureOperation;
use crate::design::parameter::{DesignParameter, ParameterOwner, ParameterValue};
use crate::design::spreadsheet::Spreadsheet;
use crate::design::units::ValueKind;
use crate::report::check::Finding;

/// A parameter name's scope, owner kind and id, and the name.
type NameKey<'a> = (&'a str, Option<(&'static str, &'a str)>, &'a str);

pub(super) fn check_parameter_records(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let design = &index.ir.design;
    let cells: BTreeSet<&str> = design
        .spreadsheets
        .iter()
        .flat_map(Spreadsheet::cells)
        .map(|cell| cell.parameter.as_str())
        .collect();
    let mut names: BTreeMap<NameKey<'_>, usize> = BTreeMap::new();
    for parameter in &design.parameters {
        let id = parameter.id.as_str();
        match &parameter.name {
            Some(name) => {
                *names
                    .entry((
                        parameter.scope.as_str(),
                        parameter.owner.as_ref().map(owner_key),
                        name.as_str(),
                    ))
                    .or_default() += 1;
            }
            None if !cells.contains(id) => super::error(
                findings,
                id,
                format_args!("parameter states no name but is no spreadsheet cell's parameter"),
            ),
            None => {}
        }
        check_owner(index, parameter, findings);
        check_kind(parameter, findings);
    }
    for ((scope, owner, name), count) in names {
        if count > 1 {
            let owner = owner.map_or_else(
                || "no owner".to_owned(),
                |(kind, owner)| format!("owner {kind} {owner}"),
            );
            super::error(
                findings,
                scope,
                format_args!("{count} parameters of the scope with {owner} are named {name:?}"),
            );
        }
    }
    check_spreadsheets(index, findings);
}

/// An owner's kind and id.
fn owner_key(owner: &ParameterOwner) -> (&'static str, &str) {
    match owner {
        ParameterOwner::Feature(feature) => ("feature", feature.as_str()),
        ParameterOwner::Container(container) => ("container", container.as_str()),
        ParameterOwner::Attribute(attribute) => ("attribute", attribute.as_str()),
    }
}

/// A parameter's owner is a feature or an ordered container of its scope, or
/// a model attribute one of whose values names the parameter.
fn check_owner(
    index: &DesignRecords<'_>,
    parameter: &DesignParameter,
    findings: &mut Vec<Finding>,
) {
    let id = parameter.id.as_str();
    let scope = parameter.scope.as_str();
    match &parameter.owner {
        None => {}
        Some(ParameterOwner::Feature(feature)) => {
            let feature = feature.as_str();
            if !index.features.contains_key(feature) {
                super::error(
                    findings,
                    id,
                    format_args!("parameter's owner {feature} is no feature of the design"),
                );
            } else if index.scope_of(MemberKey::Feature(feature)) != Some(scope) {
                super::error(
                    findings,
                    id,
                    format_args!("parameter's owner {feature} is no feature of its scope {scope}"),
                );
            }
        }
        Some(ParameterOwner::Container(container)) => {
            let container = container.as_str();
            match index.containers.get(container) {
                None => super::error(
                    findings,
                    id,
                    format_args!("parameter's owner {container} is no container of the design"),
                ),
                Some(Container::Unordered { .. }) => super::error(
                    findings,
                    id,
                    format_args!("parameter's owner {container} is an unordered container"),
                ),
                Some(Container::Ordered { .. })
                    if index.scope_of(MemberKey::Container(container)) != Some(scope) =>
                {
                    super::error(
                        findings,
                        id,
                        format_args!(
                            "parameter's owner {container} is no container of its scope {scope}"
                        ),
                    );
                }
                Some(Container::Ordered { .. }) => {}
            }
        }
        Some(ParameterOwner::Attribute(attribute)) => {
            let names = index
                .ir
                .model
                .attributes
                .iter()
                .find(|record| record.id == *attribute)
                .is_some_and(|record| {
                    record.values.iter().any(|value| {
                        matches!(value, Some(AttributeValue::Parameter(named)) if *named == parameter.id)
                    })
                });
            if !names {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "parameter's owner {} is no model attribute with a value naming the parameter",
                        attribute.as_str()
                    ),
                );
            }
        }
    }
}

/// `value`'s variant follows `kind`, and `unit` occurs only with a quantity
/// kind and has its exponents.
fn check_kind(parameter: &DesignParameter, findings: &mut Vec<Finding>) {
    let id = parameter.id.as_str();
    if let Some(value) = &parameter.value {
        let matches = match (&parameter.kind, value) {
            (ValueKind::Quantity(_), ParameterValue::Number(_))
            | (ValueKind::Integer, ParameterValue::Integer(_))
            | (ValueKind::Boolean, ParameterValue::Boolean(_))
            | (ValueKind::Text, ParameterValue::Text(_))
            | (ValueKind::Native(_), _) => true,
            (
                ValueKind::Quantity(_) | ValueKind::Integer | ValueKind::Boolean | ValueKind::Text,
                _,
            ) => false,
        };
        if !matches {
            super::error(
                findings,
                id,
                format_args!(
                    "parameter's value {value:?} does not follow its kind {:?}",
                    parameter.kind
                ),
            );
        }
    }
    let Some(unit) = &parameter.unit else {
        return;
    };
    let ValueKind::Quantity(kind) = parameter.kind else {
        super::error(
            findings,
            id,
            format_args!("parameter states a unit but its kind is no quantity"),
        );
        return;
    };
    match unit_factor(unit) {
        Ok((_, quantity)) if quantity == kind => {}
        Ok((_, quantity)) => super::error(
            findings,
            id,
            format_args!("parameter's unit has the quantity {quantity:?}, not its kind's {kind:?}"),
        ),
        Err(_) => super::error(
            findings,
            id,
            format_args!("parameter's unit has exponents outside the quantity range"),
        ),
    }
}

/// Every sheet is named by exactly one feature's `Spreadsheet` operation,
/// and each of its cells' parameters is owned by that feature.
fn check_spreadsheets(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let design = &index.ir.design;
    let mut naming: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for feature in &design.features {
        if let FeatureOperation::Spreadsheet(sheet) = &feature.definition.operation {
            naming
                .entry(sheet.as_str())
                .or_default()
                .push(feature.id.as_str());
        }
    }
    let sheets: BTreeSet<&str> = design
        .spreadsheets
        .iter()
        .map(|sheet| sheet.id.as_str())
        .collect();
    for (sheet, features) in &naming {
        if !sheets.contains(sheet) {
            for feature in features {
                super::error(
                    findings,
                    feature,
                    format_args!("feature's spreadsheet {sheet} is no spreadsheet of the design"),
                );
            }
        }
    }
    for sheet in &design.spreadsheets {
        let id = sheet.id.as_str();
        let owner = match naming.get(id).map(Vec::as_slice) {
            Some([feature]) => *feature,
            Some(features) => {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "spreadsheet is named by {} features' spreadsheet operations; one names it",
                        features.len()
                    ),
                );
                continue;
            }
            None => {
                super::error(
                    findings,
                    id,
                    format_args!("spreadsheet is named by no feature's spreadsheet operation"),
                );
                continue;
            }
        };
        for cell in sheet.cells() {
            let parameter = cell.parameter.as_str();
            let owned = index.parameters.get(parameter).is_some_and(|record| {
                matches!(&record.owner, Some(ParameterOwner::Feature(feature)) if feature.as_str() == owner)
            });
            if !owned {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "spreadsheet cell's parameter {parameter} is no parameter owned by the sheet's feature {owner}"
                    ),
                );
            }
        }
    }
}
