// SPDX-License-Identifier: Apache-2.0
//! The parameters of a Fusion design (§5.12) with their expressions (§5.13).
//!
//! Each parameter the decoder reads is a parameter of the scope of the
//! document that holds it: a model parameter owned by the feature of its
//! timeline item, or a user parameter. Its value is the cache the decoder
//! read, in canonical units. An expression that is one number with a unit,
//! or a bare number of a dimensionless parameter, is a `Neutral` literal in
//! the unit the text states, and a name of one parameter alone is a `Neutral`
//! reference. Any other text is a `Source` expression in the
//! `Fusion` dialect whose names of the parameters it reads are reference
//! tokens. The role Fusion stores on a model parameter is the source setting
//! `f3d:role`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::features::{
    DesignParameter as LegacyParameter, FeatureId, ParameterId, ParameterValue as LegacyValue,
};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};

use super::timeline::document_prefix;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;

/// The v10 property that holds the role Fusion stores on a model parameter.
const ROLE: &str = "source_kind";
/// The v10 property that holds the source's comment.
const COMMENT: &str = "comment";
/// The v10 property that holds a unit the v10 value does not type.
const UNIT: &str = "unit";
/// The v10 property that marks a model parameter whose owner is unresolved.
const UNRESOLVED_OWNER: &str = "owner_record_index";

/// The parameters of the scopes keyed by document prefix in `scopes`.
/// `features` holds the ids of the scopes' features. A parameter owned by a
/// document's parameter scope, the root's or a referenced design's, is a user
/// parameter of that document's scope. A model parameter whose owner is no
/// such feature has no owner the scope states, so it is no parameter here.
/// A document in `hosted`, or one `scopes` does not key, has no scope of its
/// own, so its user parameters stay v10 parameters and one loss in `losses`
/// names each of them.
pub(super) fn project(
    legacy: &[LegacyParameter],
    scopes: &BTreeMap<String, ProductDefinitionId>,
    hosted: &BTreeSet<String>,
    features: &BTreeSet<&str>,
    losses: &mut Vec<LossNote>,
) -> Vec<DesignParameter> {
    let document_parameters = crate::ids::document_parameter_scope_id();
    let referenced_document_parameters = document_parameters
        .as_str()
        .strip_prefix("f3d:")
        .map(|local| format!("/{local}"));
    let is_document_parameters = |owner: &FeatureId| {
        owner == &document_parameters
            || owner
                .as_str()
                .strip_prefix("f3d:xref/")
                .zip(referenced_document_parameters.as_deref())
                .is_some_and(|(path, local)| path.ends_with(local))
    };
    let mut unscoped = Vec::new();
    let scoped = legacy
        .iter()
        .filter_map(|parameter| {
            let stream = parameter.native_ref.as_deref().and_then(native_stream)?;
            let prefix = document_prefix(stream);
            let owner = match &parameter.owner {
                Some(owner) if is_document_parameters(owner) => None,
                Some(owner) if features.contains(owner.as_str()) => Some(owner),
                Some(_) => return None,
                None if parameter.properties.contains_key(UNRESOLVED_OWNER) => return None,
                None => None,
            };
            let scope = match scopes.get(prefix) {
                Some(scope) if owner.is_some() || !hosted.contains(prefix) => scope,
                Some(_) | None => {
                    if owner.is_none() {
                        unscoped.push(parameter.id.as_str());
                    }
                    return None;
                }
            };
            let name = NonBlankString::new(parameter.name.clone())?;
            Some((parameter, scope, owner, name))
        })
        .collect::<Vec<_>>();
    if !unscoped.is_empty() {
        losses.push(F3dLossCode::ParameterReferencedDesignUnscoped.note(format!(
            "{} user parameter(s) of documents with no design scope of their own stay v10 parameters: {}",
            unscoped.len(),
            unscoped.join(" ")
        )));
    }
    let names = scoped
        .iter()
        .map(|(parameter, ..)| (&parameter.id, parameter.name.as_str()))
        .collect::<HashMap<_, _>>();
    scoped
        .iter()
        .map(|(parameter, scope, owner, name)| {
            record(parameter, (*scope).clone(), *owner, name.clone(), &names)
        })
        .collect()
}

fn record(
    parameter: &LegacyParameter,
    scope: ProductDefinitionId,
    owner: Option<&FeatureId>,
    name: NonBlankString,
    names: &HashMap<&ParameterId, &str>,
) -> DesignParameter {
    let property = |key: &str| {
        parameter
            .properties
            .iter()
            .find(|(name, _)| name.as_str() == key)
            .map(|(_, value)| value.as_str())
    };
    let (kind, value) = parameter.value.as_ref().map_or_else(
        || {
            let kind = property(UNIT)
                .and_then(|unit| NonBlankString::new(format!("f3d:{unit}")))
                .map_or(ValueKind::Quantity(Quantity::REAL), ValueKind::Native);
            (kind, None)
        },
        cache,
    );
    let vocabulary = parameter
        .dependencies
        .iter()
        .filter_map(|id| Some((*names.get(id)?, id)))
        .collect::<HashMap<_, _>>();
    let expression = expression(&parameter.expression, &kind, &vocabulary);
    let mut source_settings = NativeOptions::new();
    if let Some(role) = property(ROLE).and_then(|role| NonBlankString::new(role.to_owned())) {
        source_settings.insert(
            cadmpeg_core::nonblank_literal!("f3d:role"),
            NativeOption::Text(Dimension::Literal(role)),
        );
    }
    DesignParameter {
        id: parameter.id.clone(),
        scope,
        owner: owner.cloned().map(ParameterOwner::Feature),
        name: Some(name),
        kind,
        unit: None,
        definition: ParameterDefinition::Expression(expression),
        value,
        display: None,
        tolerance: None,
        decimals: None,
        comment: property(COMMENT).map(str::to_owned),
        native_options: NativeOptions::new(),
        source_settings,
        native_ref: parameter.native_ref.clone(),
    }
}

/// The kind and canonical cache of a value the decoder read.
pub(super) fn cache(value: &LegacyValue) -> (ValueKind, Option<ParameterValue>) {
    match value {
        LegacyValue::Length(length) => (
            ValueKind::Quantity(Quantity::LENGTH),
            FiniteReal::new(length.get()).map(ParameterValue::Number),
        ),
        LegacyValue::Angle(angle) => (
            ValueKind::Quantity(Quantity::ANGLE),
            FiniteReal::new(angle.get()).map(ParameterValue::Number),
        ),
        LegacyValue::Real(real) => (
            ValueKind::Quantity(Quantity::REAL),
            Some(ParameterValue::Number(*real)),
        ),
        LegacyValue::Integer(integer) => {
            (ValueKind::Integer, Some(ParameterValue::Integer(*integer)))
        }
        LegacyValue::Boolean(flag) => (ValueKind::Boolean, Some(ParameterValue::Boolean(*flag))),
        LegacyValue::String(text) => (ValueKind::Text, Some(ParameterValue::Text(text.clone()))),
    }
}

/// The expression `text` states for a parameter of `kind`, whose names in
/// `vocabulary` are references to those parameters.
pub(super) fn expression(
    text: &str,
    kind: &ValueKind,
    vocabulary: &HashMap<&str, &ParameterId>,
) -> Expression {
    if let Some(node) = literal(text, kind) {
        return Expression::Neutral(Neutral(node));
    }
    match <[_; 1]>::try_from(tokens(text, vocabulary)) {
        Ok([ExpressionToken::Reference(reference)]) => {
            Expression::Neutral(Neutral(Node::Reference(reference)))
        }
        Ok(token) => Expression::Source {
            dialect: ExpressionDialect::Fusion,
            tokens: token.into(),
        },
        Err(tokens) => Expression::Source {
            dialect: ExpressionDialect::Fusion,
            tokens,
        },
    }
}

/// The literal `text` states, where it is one number with a unit of the
/// parameter's quantity, or a bare number of a dimensionless parameter.
fn literal(text: &str, kind: &ValueKind) -> Option<Node<ParameterRef>> {
    let text = text.trim();
    let split = text
        .find(|character: char| character.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let (number, unit) = (text[..split].trim(), text[split..].trim());
    if number.is_empty()
        || !number
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return None;
    }
    let signed = number
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())?;
    let unit = if unit.is_empty() {
        (kind == &ValueKind::Quantity(Quantity::REAL)).then(Unit::default)?
    } else {
        let symbol = UnitSymbol::ALL
            .into_iter()
            .find(|symbol| symbol.name() == unit)?;
        let quantity = match symbol {
            UnitSymbol::Nm
            | UnitSymbol::Um
            | UnitSymbol::Mm
            | UnitSymbol::Cm
            | UnitSymbol::M
            | UnitSymbol::Km
            | UnitSymbol::In
            | UnitSymbol::Ft
            | UnitSymbol::Yd
            | UnitSymbol::Mi => Quantity::LENGTH,
            UnitSymbol::Rad
            | UnitSymbol::Deg
            | UnitSymbol::Grad
            | UnitSymbol::Arcmin
            | UnitSymbol::Arcsec
            | UnitSymbol::Rev => Quantity::ANGLE,
            UnitSymbol::S
            | UnitSymbol::Min
            | UnitSymbol::H
            | UnitSymbol::G
            | UnitSymbol::Kg
            | UnitSymbol::Lb => return None,
        };
        if kind != &ValueKind::Quantity(quantity) {
            return None;
        }
        Unit(vec![UnitFactor {
            symbol,
            power: NonZeroI8::new(1)?,
        }])
    };
    let number = Node::Number {
        value: NonNegativeReal::new(signed.abs())?,
        unit,
    };
    Some(if signed.is_sign_negative() && signed != 0.0 {
        Node::Unary {
            op: UnaryOp::Negate,
            operand: Box::new(number),
        }
    } else {
        number
    })
}

/// The tokens of an expression's source text, each name of a parameter it
/// reads a reference token.
fn tokens(
    text: &str,
    references: &HashMap<&str, &ParameterId>,
) -> Vec<ExpressionToken<ParameterRef>> {
    let mut tokens = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let starts_name = (character.is_alphabetic() || character == '_')
            && !plain.chars().last().is_some_and(|previous| {
                previous.is_alphanumeric() || matches!(previous, '_' | '.')
            });
        if starts_name {
            let end = rest
                .find(|next: char| !(next.is_alphanumeric() || next == '_'))
                .unwrap_or(rest.len());
            let name = &rest[..end];
            if let Some(id) = references.get(name) {
                if !plain.is_empty() {
                    tokens.push(ExpressionToken::Text(std::mem::take(&mut plain)));
                }
                tokens.push(ExpressionToken::Reference(ParameterRef {
                    target: RefTarget::Parameter((*id).clone()),
                    through: None,
                }));
            } else {
                plain.push_str(name);
            }
            rest = &rest[end..];
        } else {
            plain.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    if !plain.is_empty() {
        tokens.push(ExpressionToken::Text(plain));
    }
    tokens
}
