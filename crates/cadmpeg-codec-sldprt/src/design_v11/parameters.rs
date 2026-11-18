// SPDX-License-Identifier: Apache-2.0
//! The part's parameters (§5.12): each dimension of a feature-tree node is a
//! model parameter owned by its feature, and each global variable of the
//! equations folder is a user parameter.
//!
//! A dimension whose text is one literal holds a `Neutral` expression that
//! keeps the literal's authored unit. Any other text is a `Source`
//! expression in the SolidWorks dialect, its references to parameters as
//! reference tokens. A value is the cache the file stores: the literal, or
//! the dimension's stored `Value`.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::{
    DesignParameter, DimensionDisplay, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};

use super::tree::Timeline;
use crate::history::literals::dimension_display;
use crate::history::parameters::eval::exact_integer_f64;
use crate::history::parameters::{
    expression_identifier_tokens, native_parameter_is_length, parse_native_parameter_literal,
};
use crate::history::project::{neutral_parameter_id, projected_parameter_names};
use crate::records::{Feature, FeatureHistory};

/// The kind of a dimension whose quantity the decoder has not read: its
/// expression references other parameters and the file stores no value.
const UNREAD_KIND: &str = "sldprt:dimension";

/// One parameter of the source, before its expression is read.
struct Entry<'a> {
    feature: &'a Feature,
    owner: Option<FeatureId>,
    name: NonBlankString,
    id: ParameterId,
    text: &'a str,
}

/// The parameters of the timeline's features and of the equations folders.
pub(super) fn parameters(
    histories: &[FeatureHistory],
    timeline: &Timeline,
    scope: &ProductDefinitionId,
) -> Result<Vec<DesignParameter>, CodecError> {
    let mut entries = Vec::new();
    let mut user_names = HashSet::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        let owner = match timeline.features_by_record.get(&feature.id) {
            Some(owner) => Some(owner.clone()),
            None if timeline.equation_records.contains(&feature.id) => None,
            None => continue,
        };
        for (ordinal, name) in projected_parameter_names(feature).into_iter().enumerate() {
            let Some(text) = feature.parameters.get(name.as_str()) else {
                continue;
            };
            if owner.is_none() && !user_names.insert(name.clone()) {
                continue;
            }
            let Some(key) = NonBlankString::new(name) else {
                continue;
            };
            entries.push(Entry {
                feature,
                owner: owner.clone(),
                name: key,
                id: neutral_parameter_id(feature, ordinal)?,
                text,
            });
        }
    }
    let aliases = Aliases::new(&entries);
    Ok(entries
        .iter()
        .map(|entry| parameter(entry, &aliases, scope))
        .collect())
}

fn parameter(entry: &Entry<'_>, aliases: &Aliases, scope: &ProductDefinitionId) -> DesignParameter {
    let reading = read(entry, aliases);
    DesignParameter {
        id: entry.id.clone(),
        scope: scope.clone(),
        owner: entry.owner.clone().map(ParameterOwner::Feature),
        name: Some(entry.name.clone()),
        kind: reading.kind,
        unit: None,
        definition: ParameterDefinition::Expression(reading.expression),
        value: reading.value,
        display: reading.display,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    }
}

struct Reading {
    kind: ValueKind,
    expression: Expression,
    value: Option<ParameterValue>,
    display: Option<DimensionDisplay>,
}

fn read(entry: &Entry<'_>, aliases: &Aliases) -> Reading {
    let user = entry.owner.is_none();
    let (feature, name, text) = (entry.feature, entry.name.as_str(), entry.text);
    if let Some((node, kind, value)) = literal(feature, name, text, user) {
        return Reading {
            kind,
            expression: Expression::Neutral(Neutral(node)),
            value: Some(value),
            display: None,
        };
    }
    let tokens = source_tokens(text, entry.owner.as_ref(), aliases);
    let references = tokens
        .iter()
        .any(|token| matches!(token, ExpressionToken::Reference(_)));
    let parsed = parse_native_parameter_literal(feature, name, text);
    let display = dimension_display(text);
    if parsed.is_none() && display.is_none() && !references && is_text(text) {
        return Reading {
            kind: ValueKind::Text,
            expression: Expression::Neutral(Neutral(Node::Text(text.to_owned()))),
            value: Some(ParameterValue::Text(text.to_owned())),
            display: None,
        };
    }
    let stored = feature
        .dimension_properties
        .get(name)
        .and_then(|properties| properties.get("Value"));
    let parsed = parsed
        .or_else(|| stored.and_then(|value| parse_native_parameter_literal(feature, name, value)));
    let (kind, value) = match parsed.and_then(|value| cache(value, user)) {
        Some((kind, value)) => (kind, Some(value)),
        None if user => (ValueKind::Quantity(Quantity::REAL), None),
        None if native_parameter_is_length(feature, name, Some(text)) => {
            (ValueKind::Quantity(Quantity::LENGTH), None)
        }
        None => (
            ValueKind::Native(cadmpeg_core::nonblank_const!(UNREAD_KIND)),
            None,
        ),
    };
    let display = display.filter(|_| kind == ValueKind::Quantity(Quantity::LENGTH));
    Reading {
        kind,
        expression: Expression::Source {
            dialect: ExpressionDialect::SolidWorks,
            tokens,
        },
        value,
        display,
    }
}

/// The kind and canonical cache of a value the v10 literal reader parsed. A
/// user parameter has no declared quantity, so a whole number there is a
/// dimensionless real.
fn cache(
    value: cadmpeg_ir::features::ParameterValue,
    user: bool,
) -> Option<(ValueKind, ParameterValue)> {
    use cadmpeg_ir::features::ParameterValue as Parsed;
    let number = |quantity, value: f64| {
        Some((
            ValueKind::Quantity(quantity),
            ParameterValue::Number(FiniteReal::new(value)?),
        ))
    };
    match value {
        Parsed::Length(length) => number(Quantity::LENGTH, length.get()),
        Parsed::Angle(angle) => number(Quantity::ANGLE, angle.get()),
        Parsed::Real(real) => number(Quantity::REAL, real.get()),
        Parsed::Integer(integer) if user => number(Quantity::REAL, exact_integer_f64(integer)?),
        Parsed::Integer(integer) => Some((ValueKind::Integer, ParameterValue::Integer(integer))),
        Parsed::Boolean(flag) => Some((ValueKind::Boolean, ParameterValue::Boolean(flag))),
        Parsed::String(text) => Some((ValueKind::Text, ParameterValue::Text(text))),
    }
}

/// Text that is a word or phrase, not an expression: no operator, no quoted
/// name, and no qualified name.
fn is_text(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && !text.contains([
            '+', '-', '*', '/', '^', '=', '<', '>', '(', ')', ',', '"', '@',
        ])
        && text.parse::<f64>().is_err()
}

/// The units a SolidWorks literal may carry, longest suffix first.
const LITERAL_UNITS: [(&str, UnitSymbol); 11] = [
    ("deg", UnitSymbol::Deg),
    ("rad", UnitSymbol::Rad),
    ("mm", UnitSymbol::Mm),
    ("cm", UnitSymbol::Cm),
    ("km", UnitSymbol::Km),
    ("um", UnitSymbol::Um),
    ("nm", UnitSymbol::Nm),
    ("in", UnitSymbol::In),
    ("ft", UnitSymbol::Ft),
    ("\u{00b0}", UnitSymbol::Deg),
    ("m", UnitSymbol::M),
];

/// A literal of one number, with a unit the vocabulary names or none, or a
/// Boolean: its tree, kind, and canonical value.
///
/// A number with no unit is a literal only where the parameter is not a
/// length: a unit-blind length takes the definition's authoring unit, which
/// the decoder does not read (§5.13, §5.15).
fn literal(
    feature: &Feature,
    name: &str,
    text: &str,
    user: bool,
) -> Option<(Node<ParameterRef>, ValueKind, ParameterValue)> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("false") {
        let flag = text.eq_ignore_ascii_case("true");
        return Some((Node::Boolean(flag), ValueKind::Boolean, ParameterValue::Boolean(flag)));
    }
    let (number, unit) = LITERAL_UNITS
        .iter()
        .find_map(|(suffix, symbol)| Some((text.strip_suffix(suffix)?, Some(*symbol))))
        .unwrap_or((text, None));
    let number = number.trim();
    if number.is_empty()
        || !number
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+' | b'e' | b'E'))
    {
        return None;
    }
    let signed = number.parse::<f64>().ok().filter(|value| value.is_finite())?;
    let magnitude = NonNegativeReal::new(signed.abs())?;
    let (kind, value) = match unit {
        Some(symbol) => {
            let (quantity, scale) = canonical(symbol)?;
            (
                ValueKind::Quantity(quantity),
                ParameterValue::Number(FiniteReal::new(signed * scale)?),
            )
        }
        None if native_parameter_is_length(feature, name, Some(text)) => return None,
        None => match number.parse::<i64>() {
            Ok(integer) if !user => (ValueKind::Integer, ParameterValue::Integer(integer)),
            _ => (
                ValueKind::Quantity(Quantity::REAL),
                ParameterValue::Number(FiniteReal::new(signed)?),
            ),
        },
    };
    let unit = match unit {
        Some(symbol) => Unit(vec![UnitFactor {
            symbol,
            power: NonZeroI8::new(1)?,
        }]),
        None => Unit::default(),
    };
    let number = Node::Number {
        value: magnitude,
        unit,
    };
    let node = if signed.is_sign_negative() && signed != 0.0 {
        Node::Unary {
            op: UnaryOp::Negate,
            operand: Box::new(number),
        }
    } else {
        number
    };
    Some((node, kind, value))
}

/// The quantity of a literal unit and its factor to the canonical unit.
fn canonical(symbol: UnitSymbol) -> Option<(Quantity, f64)> {
    Some(match symbol {
        UnitSymbol::Nm => (Quantity::LENGTH, 1.0e-6),
        UnitSymbol::Um => (Quantity::LENGTH, 1.0e-3),
        UnitSymbol::Mm => (Quantity::LENGTH, 1.0),
        UnitSymbol::Cm => (Quantity::LENGTH, 10.0),
        UnitSymbol::M => (Quantity::LENGTH, 1000.0),
        UnitSymbol::Km => (Quantity::LENGTH, 1.0e6),
        UnitSymbol::In => (Quantity::LENGTH, 25.4),
        UnitSymbol::Ft => (Quantity::LENGTH, 304.8),
        UnitSymbol::Rad => (Quantity::ANGLE, 1.0),
        UnitSymbol::Deg => (Quantity::ANGLE, std::f64::consts::PI / 180.0),
        _ => return None,
    })
}

/// The source text as tokens, each name that resolves to one parameter a
/// reference.
fn source_tokens(
    text: &str,
    owner: Option<&FeatureId>,
    aliases: &Aliases,
) -> Vec<ExpressionToken<ParameterRef>> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    for identifier in expression_identifier_tokens(text).unwrap_or_default() {
        if identifier.is_syntax() {
            continue;
        }
        let Some(parameter) = aliases.resolve(identifier.value(), owner) else {
            continue;
        };
        let end = text.len() - identifier.following().len();
        let start = end - identifier.raw().len();
        if start > cursor {
            tokens.push(ExpressionToken::Text(text[cursor..start].to_owned()));
        }
        tokens.push(ExpressionToken::Reference(ParameterRef {
            target: RefTarget::Parameter(parameter.clone()),
            through: None,
        }));
        cursor = end;
    }
    if cursor < text.len() {
        tokens.push(ExpressionToken::Text(text[cursor..].to_owned()));
    }
    tokens
}

/// The names by which an expression reaches a parameter: `name@feature`
/// anywhere, a dimension's bare name within its own feature, and a global
/// variable's name anywhere. A name that reaches two parameters resolves to
/// none.
struct Aliases {
    qualified: HashMap<String, Option<ParameterId>>,
    local: HashMap<(FeatureId, String), Option<ParameterId>>,
    global: HashMap<String, Option<ParameterId>>,
}

impl Aliases {
    fn new(entries: &[Entry<'_>]) -> Self {
        fn insert<K: Eq + std::hash::Hash>(
            map: &mut HashMap<K, Option<ParameterId>>,
            key: K,
            id: &ParameterId,
        ) {
            map.entry(key)
                .and_modify(|existing| {
                    if existing.as_ref() != Some(id) {
                        *existing = None;
                    }
                })
                .or_insert_with(|| Some(id.clone()));
        }
        let mut aliases = Self {
            qualified: HashMap::new(),
            local: HashMap::new(),
            global: HashMap::new(),
        };
        for entry in entries {
            let name = entry.name.as_str().to_owned();
            match &entry.owner {
                Some(owner) => {
                    if !entry.feature.name.is_empty() {
                        let qualified = format!("{name}@{}", entry.feature.name);
                        insert(&mut aliases.qualified, qualified, &entry.id);
                    }
                    insert(&mut aliases.local, (owner.clone(), name), &entry.id);
                }
                None => insert(&mut aliases.global, name, &entry.id),
            }
        }
        aliases
    }

    fn resolve(&self, name: &str, owner: Option<&FeatureId>) -> Option<&ParameterId> {
        if let Some(found) = self.qualified.get(name) {
            return found.as_ref();
        }
        if let Some(found) =
            owner.and_then(|owner| self.local.get(&(owner.clone(), name.to_owned())))
        {
            return found.as_ref();
        }
        self.global.get(name)?.as_ref()
    }
}
