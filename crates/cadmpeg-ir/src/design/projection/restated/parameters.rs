// SPDX-License-Identifier: Apache-2.0
//! The fields a design parameter leaves out of the model parameter of its
//! identity, found by comparing the two records.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::design::expr::{Expression, ExpressionToken, Node, UnaryOp};
use crate::design::parameter::{
    DesignParameter, DimensionDisplay, ParameterDefinition, ParameterOwner, ParameterValue,
};
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::features::parameters::{
    DesignParameter as ModelParameter, DimensionDisplay as ModelDisplay,
    ParameterValue as ModelValue,
};
use crate::ids::ProductDefinitionId;

use super::super::fields::{Field, ModelParameterField};
use super::{json_strings, Restatements, Restater};

/// The property spellings of a model parameter's driven flag, each with the
/// value that marks the parameter driven and the one that marks it driving.
const DRIVEN_FLAGS: [(&str, &str, &str); 2] =
    [("is_driving", "0", "1"), ("driven", "true", "false")];

/// The model property that keeps the name a source stores for a parameter
/// whose model name the decoder generated.
const SOURCE_NAME: &str = "source_name";

/// The fields each model parameter of `ir` that `compared` accepts leaves
/// out of the design parameter of its identity.
pub(super) fn parameter_cuts<'ir>(
    ir: &'ir CadIr,
    compared: &dyn Fn(&str) -> bool,
) -> Vec<(&'ir str, BTreeSet<&'static str>)> {
    let design = ir
        .design
        .parameters
        .iter()
        .map(|parameter| (parameter.id.as_str(), parameter))
        .collect::<HashMap<_, _>>();
    let components = ir
        .design
        .features
        .iter()
        .filter_map(|feature| Some((feature.id.as_str(), feature.component.as_ref()?)))
        .collect::<HashMap<_, _>>();
    let pairs = ir
        .model
        .parameters
        .iter()
        .filter(|model| compared(model.id.as_str()))
        .filter_map(|model| Some((model, *design.get(model.id.as_str())?)))
        .collect::<Vec<_>>();
    let cells = ir
        .design
        .spreadsheets
        .iter()
        .flat_map(|sheet| {
            sheet
                .cells()
                .iter()
                .enumerate()
                .map(move |(at, cell)| (cell.parameter.as_str(), (sheet.id.as_str(), at)))
        })
        .collect::<HashMap<_, _>>();
    let misplaced = misplaced(&ir.model.parameters, &cells);
    pairs
        .into_iter()
        .map(|(model, design)| {
            let id = model.id.as_str();
            let component = match &design.owner {
                Some(ParameterOwner::Feature(feature)) => components.get(feature.as_str()).copied(),
                Some(ParameterOwner::Container(_) | ParameterOwner::Attribute(_)) | None => None,
            };
            (
                id,
                parameter_cut(
                    model,
                    design,
                    component,
                    misplaced.contains(id),
                    &ir.model.restated,
                ),
            )
        })
        .collect()
}

/// The parameters of `models` whose position by `ordinal` among the
/// parameters of their definition and owner the design does not hold. Only
/// a spreadsheet orders design parameters (§2.2): `cells` holds the sheet
/// and the position among its cells of each cell parameter. A group of one
/// parameter holds its position; a larger group holds it where each member
/// is a cell, and its order by `ordinal` is its order by cell.
fn misplaced<'ir>(
    models: &'ir [ModelParameter],
    cells: &HashMap<&str, (&str, usize)>,
) -> HashSet<&'ir str> {
    let mut groups = BTreeMap::<_, Vec<&ModelParameter>>::new();
    for model in models {
        let key = (
            model
                .product_definition
                .as_ref()
                .map(crate::ids::ProductDefinitionId::as_str),
            model.owner.as_ref().map(crate::features::FeatureId::as_str),
        );
        groups.entry(key).or_default().push(model);
    }
    let mut misplaced = HashSet::new();
    for members in groups.into_values().filter(|members| members.len() > 1) {
        let Some(placed) = members
            .iter()
            .map(|model| Some((*model, *cells.get(model.id.as_str())?)))
            .collect::<Option<Vec<_>>>()
        else {
            misplaced.extend(members.iter().map(|model| model.id.as_str()));
            continue;
        };
        let mut in_design = placed
            .iter()
            .map(|(model, at)| (*at, model.id.as_str()))
            .collect::<Vec<_>>();
        in_design.sort_unstable();
        let mut by_ordinal = placed
            .iter()
            .map(|(model, at)| (model.ordinal, *at, model.id.as_str()))
            .collect::<Vec<_>>();
        by_ordinal.sort_unstable();
        for ((_, _, at_ordinal), (_, at_design)) in by_ordinal.iter().zip(&in_design) {
            if at_ordinal != at_design {
                misplaced.insert(*at_ordinal);
            }
        }
    }
    misplaced
}

/// The fields `design` leaves out of `model`. `component`: the definition
/// the design states for the feature that owns `design`. `misplaced`: the
/// design does not hold `model`'s position.
pub(super) fn parameter_cut(
    model: &ModelParameter,
    design: &DesignParameter,
    component: Option<&ProductDefinitionId>,
    misplaced: bool,
    restated: &Restatements,
) -> BTreeSet<&'static str> {
    let mut cut = BTreeSet::new();
    let mut leave = |field: ModelParameterField, carried: bool| {
        if !carried {
            cut.insert(field.key());
        }
    };
    leave(
        ModelParameterField::ProductDefinition,
        model
            .product_definition
            .as_ref()
            .is_none_or(|definition| *definition == design.scope || Some(definition) == component),
    );
    leave(
        ModelParameterField::Owner,
        owner_carried(model, design, restated),
    );
    leave(ModelParameterField::Ordinal, !misplaced);
    let source_name = model
        .properties
        .iter()
        .find(|(key, _)| key.as_str() == SOURCE_NAME)
        .map(|(_, name)| name.as_str());
    leave(
        ModelParameterField::Name,
        model.name.trim().is_empty()
            || design.name.as_ref().is_some_and(|name| {
                name.as_str() == model.name || Some(name.as_str()) == source_name
            }),
    );
    leave(
        ModelParameterField::Display,
        model.display.as_ref().is_none_or(|display| {
            design.display
                == Some(match display {
                    ModelDisplay::Diameter => DimensionDisplay::Diameter,
                    ModelDisplay::Radius => DimensionDisplay::Radius,
                })
        }),
    );
    leave(
        ModelParameterField::Value,
        model
            .value
            .as_ref()
            .is_none_or(|value| value_carried(value, design)),
    );
    let expression = expression_carried(model, design);
    leave(ModelParameterField::ExpressionText, expression);
    leave(
        ModelParameterField::Dependencies,
        expression || model.dependencies.is_empty(),
    );
    leave(
        ModelParameterField::Properties,
        properties_carried(model, design),
    );
    leave(ModelParameterField::Pmi, model.pmi.is_none());
    leave(
        ModelParameterField::NativeRef,
        model.native_ref.is_none() || model.native_ref == design.native_ref,
    );
    cut
}

/// Whether `design` states `model`'s owner: the feature itself, or the
/// container that restates it under another identity.
fn owner_carried(
    model: &ModelParameter,
    design: &DesignParameter,
    restated: &Restatements,
) -> bool {
    let Some(owner) = &model.owner else {
        return true;
    };
    match &design.owner {
        Some(ParameterOwner::Feature(feature)) => feature == owner,
        Some(ParameterOwner::Container(container)) => {
            restated.get(owner.as_str()).is_some_and(|restating| {
                restating.restaters().any(|(by, _)| {
                    matches!(by, Restater::Record(id) if id.as_str() == container.as_str())
                })
            })
        }
        Some(ParameterOwner::Attribute(_)) | None => false,
    }
}

/// Whether two reals are equal to 15 significant digits (§5.12).
fn same_number(left: f64, right: f64) -> bool {
    format!("{left:.14e}") == format!("{right:.14e}")
}

/// Whether `design` states the value `value` in the quantity it implies.
fn value_carried(value: &ModelValue, design: &DesignParameter) -> bool {
    let quantity = |quantity: Quantity, number: f64| {
        design.kind == ValueKind::Quantity(quantity)
            && matches!(&design.value, Some(ParameterValue::Number(stated)) if same_number(stated.get(), number))
    };
    match value {
        ModelValue::Length(length) => quantity(Quantity::LENGTH, length.get()),
        ModelValue::Angle(angle) => quantity(Quantity::ANGLE, angle.get()),
        ModelValue::Real(real) => quantity(Quantity::REAL, real.get()),
        ModelValue::Integer(integer) => {
            matches!(&design.value, Some(ParameterValue::Integer(stated)) if stated == integer)
        }
        ModelValue::Boolean(flag) => {
            matches!(&design.value, Some(ParameterValue::Boolean(stated)) if stated == flag)
        }
        ModelValue::String(text) => {
            matches!(&design.value, Some(ParameterValue::Text(stated)) if stated == text)
        }
    }
}

/// Whether `design` states `model`'s expression text and every parameter
/// it references.
fn expression_carried(model: &ModelParameter, design: &DesignParameter) -> bool {
    let text = model.expression.trim();
    let references = json_strings(&design.definition);
    if !model
        .dependencies
        .iter()
        .all(|dependency| references.contains(dependency.as_str()))
    {
        return false;
    }
    if text.is_empty() {
        return true;
    }
    match &design.definition {
        ParameterDefinition::Expression(Expression::Source { tokens, .. }) => spells(tokens, text),
        ParameterDefinition::Expression(Expression::Neutral(neutral)) => {
            match (literal(&neutral.0), literal_number(text)) {
                (Some(Literal::Number(stated)), Some(number)) => same_number(stated, number),
                (Some(Literal::Flag(stated)), _) => text.eq_ignore_ascii_case(&stated.to_string()),
                (Some(Literal::Text(stated)), _) => {
                    stated == text
                        || text.strip_prefix('"').and_then(|t| t.strip_suffix('"')) == Some(stated)
                }
                (Some(Literal::Number(_)), None) => false,
                (None, number) => number.is_none(),
            }
        }
        ParameterDefinition::Driven
        | ParameterDefinition::Assigned(_)
        | ParameterDefinition::Unset => {
            let held = json_strings(&design.native_options)
                .into_iter()
                .chain(json_strings(&design.source_settings))
                .any(|held| held.trim() == text);
            held || bare_value(text, model.value.as_ref())
        }
    }
}

/// Whether `text` is a bare number that equals the value `value` holds.
fn bare_value(text: &str, value: Option<&ModelValue>) -> bool {
    let number = |stated: f64| {
        text.parse::<f64>()
            .is_ok_and(|number| same_number(stated, number))
    };
    match value {
        Some(ModelValue::Length(length)) => number(length.get()),
        Some(ModelValue::Angle(angle)) => number(angle.get()),
        Some(ModelValue::Real(real)) => number(real.get()),
        Some(ModelValue::Integer(integer)) => text.parse::<i64>() == Ok(*integer),
        Some(ModelValue::Boolean(_) | ModelValue::String(_)) | None => false,
    }
}

/// A one-literal neutral tree.
enum Literal<'a> {
    Number(f64),
    Flag(bool),
    Text(&'a str),
}

fn literal<R>(node: &Node<R>) -> Option<Literal<'_>> {
    match node {
        Node::Number { value, .. } => Some(Literal::Number(value.get())),
        Node::Unary {
            op: UnaryOp::Negate,
            operand,
        } => match operand.as_ref() {
            Node::Number { value, .. } => Some(Literal::Number(-value.get())),
            _ => None,
        },
        Node::Boolean(flag) => Some(Literal::Flag(*flag)),
        Node::Text(text) => Some(Literal::Text(text)),
        _ => None,
    }
}

/// The number of a literal's text: a number with an optional unit after
/// it.
fn literal_number(text: &str) -> Option<f64> {
    let (number, unit) = text
        .char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()))
        .rev()
        .find_map(|at| {
            let number = text[..at]
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())?;
            Some((number, &text[at..]))
        })?;
    unit.trim()
        .chars()
        .all(|unit| {
            unit.is_alphanumeric() || matches!(unit, '°' | '"' | '\'' | '[' | ']' | '^' | '/')
        })
        .then_some(number)
}

/// Whether the source tokens `tokens` spell `text`, white space aside: each
/// text token in turn, with at least one character for each reference
/// before the next text token or the end.
fn spells<R>(tokens: &[ExpressionToken<R>], text: &str) -> bool {
    fn squeeze(text: &str) -> String {
        text.chars().filter(|unit| !unit.is_whitespace()).collect()
    }
    if tokens.is_empty() {
        return false;
    }
    let text = squeeze(text);
    let mut rest = text.as_str();
    let mut references = 0;
    for token in tokens {
        match token {
            ExpressionToken::Reference(_) => references += 1,
            ExpressionToken::Text(part) => {
                let part = squeeze(part);
                if part.is_empty() {
                    continue;
                }
                let Some(skip) = rest
                    .char_indices()
                    .map(|(at, _)| at)
                    .chain(std::iter::once(rest.len()))
                    .nth(references)
                else {
                    return false;
                };
                let Some(at) = rest[skip..].find(&part) else {
                    return false;
                };
                if references == 0 && at != 0 {
                    return false;
                }
                rest = &rest[skip + at + part.len()..];
                references = 0;
            }
        }
    }
    if references == 0 {
        rest.is_empty()
    } else {
        rest.chars().count() >= references
    }
}

/// Whether `design` holds the properties of `model`: a native record either
/// names holds them, and `design` states the driven flag they hold.
fn properties_carried(model: &ModelParameter, design: &DesignParameter) -> bool {
    let driven = DRIVEN_FLAGS.iter().find_map(|(key, driven, driving)| {
        let value = model
            .properties
            .iter()
            .find(|(name, _)| name.as_str() == *key)
            .map(|(_, value)| value.as_str())?;
        if value == *driven {
            Some(true)
        } else if value == *driving {
            Some(false)
        } else {
            None
        }
    });
    let flag = driven
        .is_none_or(|driven| driven == matches!(design.definition, ParameterDefinition::Driven));
    flag && (model.properties.is_empty()
        || model.native_ref.is_some()
        || design.native_ref.is_some())
}

#[cfg(test)]
mod tests;
