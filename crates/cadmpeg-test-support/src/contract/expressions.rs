// SPDX-License-Identifier: Apache-2.0
//! Source-name equality for expressions crossing a design-depth boundary.

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::{Expression, ParameterId};
use cadmpeg_ir::{ArenaClass, CadIr};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// A codec's actual source symbols, independent of identifier spellings in IDs.
pub trait ExpressionSourceResolver {
    /// Return one unambiguous source name for this genuinely held parameter.
    fn source_name(&self, parameter: &ParameterId) -> Result<String, String>;
    /// Resolve only identifier spans in the supplied text, using this attribute's scope.
    /// Unresolved text is left outside the returned spans; ambiguous names are errors.
    fn reference_spans(
        &self,
        attribute: &SourceAttribute,
        text: &str,
    ) -> Result<Vec<(Range<usize>, ParameterId)>, String>;
}

/// Compare model expressions through Full's actual source symbols before dropping design.
///
/// Only `AttributeValue::Expression.expression` changes representation. All other
/// attribute fields and values, including order, remain exact comparison content.
pub fn neutral_views(
    full: &CadIr,
    geometry: &CadIr,
    exclude: &[ArenaClass],
    resolver: &dyn ExpressionSourceResolver,
) -> Result<(Value, Value), String> {
    if !exclude.contains(&ArenaClass::Design) || exclude.contains(&ArenaClass::Model) {
        return Ok((
            super::neutral_view(full, exclude),
            super::neutral_view(geometry, exclude),
        ));
    }
    let held = full
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<BTreeSet<_>>();
    let mut names = BTreeMap::<String, ParameterId>::new();
    let mut canonicalize = |ir: &CadIr| -> Result<Value, String> {
        let mut current = ir.clone();
        for attribute in &mut current.model.attributes {
            if matches!(
                attribute.target,
                AttributeTarget::Feature(_)
                    | AttributeTarget::Parameter(_)
                    | AttributeTarget::Sketch(_)
            ) || matches!(attribute.target, AttributeTarget::Record { arena, .. } if arena.class() == ArenaClass::Design)
            {
                continue;
            }
            let mut replacements = Vec::new();
            for (index, value) in attribute.values.iter().enumerate() {
                if let AttributeValue::Expression(value) = value {
                    replacements.push((
                        index,
                        expression(attribute, &value.expression, &held, &mut names, resolver)?,
                    ));
                }
            }
            for (index, expression) in replacements {
                if let AttributeValue::Expression(value) = &mut attribute.values[index] {
                    value.expression = expression;
                }
            }
        }
        Ok(super::neutral_view(&current, exclude))
    };
    Ok((canonicalize(full)?, canonicalize(geometry)?))
}

fn expression(
    attribute: &SourceAttribute,
    expression: &Expression,
    held: &BTreeSet<ParameterId>,
    names: &mut BTreeMap<String, ParameterId>,
    resolver: &dyn ExpressionSourceResolver,
) -> Result<Expression, String> {
    for id in expression.dependencies() {
        let name = source_name(id, held, names, resolver)?;
        let rebound = resolver.reference_spans(attribute, &name)?;
        if rebound.as_slice() != [(0..name.len(), id.clone())] {
            return Err(format!(
                "source name {name:?} does not resolve uniquely to {id}"
            ));
        }
    }
    let mut error = None;
    let rendered = expression
        .render_with(|id| match source_name(id, held, names, resolver) {
            Ok(name) => Some(name),
            Err(reason) => {
                error = Some(reason);
                None
            }
        })
        .ok_or_else(|| error.unwrap_or_else(|| "expression source-name rendering failed".into()))?;
    let spans = resolver.reference_spans(attribute, &rendered)?;
    for (_, id) in &spans {
        source_name(id, held, names, resolver)?;
    }
    let resolved = Expression::from_spans(&rendered, spans).map_err(|error| error.to_string())?;
    let mut error = None;
    let rendered = resolved
        .render_with(|id| match source_name(id, held, names, resolver) {
            Ok(name) => Some(name),
            Err(reason) => {
                error = Some(reason);
                None
            }
        })
        .ok_or_else(|| {
            error.unwrap_or_else(|| "resolved expression source-name rendering failed".into())
        })?;
    Ok(rendered.into())
}

fn source_name(
    id: &ParameterId,
    held: &BTreeSet<ParameterId>,
    names: &mut BTreeMap<String, ParameterId>,
    resolver: &dyn ExpressionSourceResolver,
) -> Result<String, String> {
    if !held.contains(id) {
        return Err(format!("expression references an unheld parameter {id}"));
    }
    let name = resolver.source_name(id)?;
    if name.is_empty() {
        return Err(format!("parameter {id} has an empty source name"));
    }
    if names.get(&name).is_some_and(|other| other != id) {
        return Err(format!(
            "source name {name:?} identifies more than one parameter"
        ));
    }
    names.insert(name.clone(), id.clone());
    Ok(name)
}

#[cfg(test)]
mod tests;
