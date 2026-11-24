// SPDX-License-Identifier: Apache-2.0
//! The history arenas of a document read from CADIR JSON.
//!
//! The CADIR wire omits the arenas of [`ArenaName::WIRE_OMITTED`], and the
//! `design` records restate what a user edits of them. A writer that applies
//! native history edits from those arenas takes them from the decode of the
//! source the document was saved from, and [`restore_history`] applies to
//! them each edit the `design` records state since that decode: a
//! parameter's definition and value, and a feature's suppression and name.
//!
//! The decode's own design records tell an edit from what an edit derives.
//! Each changed parameter definition is replayed on them with
//! [`Editor::set_parameter_expression`](crate::design::edit::Editor::set_parameter_expression),
//! which sets the caches and stale states the edit derives. A design value
//! that differs from the decode's is an edit the history cannot take, and
//! refuses the restore, unless it is the replayed value, or the replay
//! changed it and it still holds the decode's: a cache the edit made stale.
//! An evaluation's `status` is derived and never refuses.
//!
//! [`ArenaName::WIRE_OMITTED`]: crate::document::ArenaName::WIRE_OMITTED

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::CodecError;
use serde_json::Value;

use crate::codec::write::EncodeInput;
use crate::design::dimension::{Dimension, ParameterRef, RefTarget};
use crate::design::expr::{Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp};
use crate::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use crate::design::units::{Quantity, Unit, ValueKind};
use crate::document::{CadIr, Model};
use crate::features::parameters::{
    DesignParameter as HistoryParameter, ParameterValue as HistoryValue,
};
use crate::features::selections::DistinctMembers;
use crate::features::{FeatureId, ParameterId};
use crate::scalar::{Angle, Length};

/// How a codec's history arenas spell a parameter's expression.
pub trait HistoryText {
    /// The dialect of the codec's `Source` expressions.
    fn dialect(&self) -> ExpressionDialect;

    /// The text of the literal `value` in `unit`, for a parameter whose
    /// text was `stored` before the edit; `None` where the codec has no
    /// literal in `unit`.
    fn literal(&self, value: f64, unit: &Unit, stored: &str) -> Option<String>;

    /// The text of a reference to `target` that no stored expression
    /// spells; `None` where the codec has none.
    fn reference(&self, target: &HistoryParameter, model: &Model) -> Option<String>;
}

/// The retained source image `id` of `input`'s document where it was read
/// from CADIR JSON and its write reads the arenas the wire omits: the
/// document is edited, or the write does not replay the source. `replays`
/// states whether the write replays an unedited document.
///
/// # Errors
///
/// The error of the sidecar's edit comparison.
pub fn saved_source<'a>(
    input: EncodeInput<'a>,
    id: &str,
    replays: bool,
) -> Result<Option<&'a [u8]>, CodecError> {
    if input.ir.model.omitted_arenas_held() {
        return Ok(None);
    }
    let Some(fidelity) = input.fidelity else {
        return Ok(None);
    };
    let Some(image) = fidelity
        .retained_record(id)
        .and_then(|record| record.data())
    else {
        return Ok(None);
    };
    if replays && !fidelity.edited(input.ir)? {
        return Ok(None);
    }
    Ok(Some(image))
}

/// `read`, a document read from CADIR JSON, with the arenas the wire omits
/// taken from `decoded`, the decode of the source `read` was saved from, and
/// each edit of its `design` records since that decode applied to them. Its
/// `design` holds the caches and states those edits derive.
///
/// # Errors
///
/// [`CodecError::NotImplemented`] that names each design edit the history
/// arenas cannot state, and [`CodecError::Malformed`] where a design arena
/// has no JSON form.
pub fn restore_history(
    read: &CadIr,
    decoded: &CadIr,
    text: &dyn HistoryText,
) -> Result<CadIr, CodecError> {
    let mut unmapped = Vec::new();
    let mut replayed = decoded.clone();
    let stored = decoded
        .design
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    for parameter in &read.design.parameters {
        let Some(before) = stored.get(&parameter.id) else {
            continue;
        };
        if before.definition == parameter.definition {
            continue;
        }
        if let ParameterDefinition::Expression(expression) = &parameter.definition {
            if let Err(error) = replayed.set_parameter_expression(&parameter.id, expression.clone())
            {
                unmapped.push(format!(
                    "design.parameters[{}].definition: {error}",
                    parameter.id
                ));
            }
        }
    }
    for feature in &read.design.features {
        let Some(target) = replayed
            .design
            .features
            .iter_mut()
            .find(|candidate| candidate.id == feature.id)
        else {
            continue;
        };
        if !matches!(
            feature.suppressed,
            Some(Dimension::Parameter(_) | Dimension::Negated(_))
        ) {
            target.suppressed.clone_from(&feature.suppressed);
        }
        target.name.clone_from(&feature.name);
    }
    let json = |design: &crate::document::Design| {
        serde_json::to_value(design).map_err(|error| {
            CodecError::malformed(format!("the design records have no JSON form: {error}"))
        })
    };
    residue(
        &json(&read.design)?,
        &json(&decoded.design)?,
        &json(&replayed.design)?,
        "design",
        &mut unmapped,
    );
    refuse(&unmapped)?;

    let mut restored = read.clone();
    restored.model.adopt_omitted_arenas(&decoded.model);
    let spellings = spellings(decoded, &stored);
    for parameter in &replayed.design.parameters {
        let Some(before) = stored.get(&parameter.id) else {
            continue;
        };
        let definition_edited = before.definition != parameter.definition;
        if !definition_edited && before.value == parameter.value {
            continue;
        }
        let Some(at) = restored
            .model
            .parameters
            .iter()
            .position(|history| history.id == parameter.id)
        else {
            unmapped.push(format!(
                "design.parameters[{}]: no history parameter restates it",
                parameter.id
            ));
            continue;
        };
        if definition_edited {
            let history = &restored.model.parameters[at];
            match history_expression(
                &parameter.definition,
                history,
                text,
                &spellings,
                &restored.model,
            ) {
                Ok((expression, dependencies)) => {
                    let history = &mut restored.model.parameters[at];
                    history.expression = expression;
                    if history.dependencies.iter().collect::<BTreeSet<_>>()
                        != dependencies.iter().collect::<BTreeSet<_>>()
                    {
                        let mut members = DistinctMembers::default();
                        for dependency in dependencies {
                            members.insert(dependency);
                        }
                        history.dependencies = members;
                    }
                }
                Err(why) => unmapped.push(format!(
                    "design.parameters[{}].definition: {why}",
                    parameter.id
                )),
            }
        }
        let history = &mut restored.model.parameters[at];
        history.value = history_value(
            parameter.value.as_ref(),
            history.value.as_ref(),
            &parameter.kind,
        );
    }
    let stored_features = decoded
        .design
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    for feature in &replayed.design.features {
        let Some(before) = stored_features.get(&feature.id) else {
            continue;
        };
        if before.suppressed == feature.suppressed && before.name == feature.name {
            continue;
        }
        let Some(history) = restored
            .model
            .features
            .iter_mut()
            .find(|history| history.id == feature.id)
        else {
            unmapped.push(format!(
                "design.features[{}]: no history feature restates it",
                feature.id
            ));
            continue;
        };
        if before.suppressed != feature.suppressed {
            match &feature.suppressed {
                None => history.suppressed = None,
                Some(Dimension::Literal(flag)) => history.suppressed = Some(*flag),
                Some(Dimension::Parameter(_) | Dimension::Negated(_)) => unmapped.push(format!(
                    "design.features[{}].suppressed: a parameter binds it",
                    feature.id
                )),
            }
        }
        if before.name != feature.name {
            history.name.clone_from(&feature.name);
        }
    }
    refuse(&unmapped)?;
    restored.design = replayed.design;
    Ok(restored)
}

fn refuse(unmapped: &[String]) -> Result<(), CodecError> {
    if unmapped.is_empty() {
        return Ok(());
    }
    Err(CodecError::NotImplemented(format!(
        "the document was read from CADIR, and its history arenas cannot state these design \
         edits: {}",
        unmapped.join("; ")
    )))
}

/// Adds to `out` the path of each value where `read` differs from `stored`,
/// the decode's design, and is not `replayed`, the decode's design with the
/// mapped edits replayed. Records of an arena match by `id`.
fn residue(read: &Value, stored: &Value, replayed: &Value, path: &str, out: &mut Vec<String>) {
    if read == stored || read == replayed {
        return;
    }
    match (read, stored, replayed) {
        (Value::Object(read), Value::Object(stored), Value::Object(replayed)) => {
            let keys = read.keys().chain(stored.keys()).collect::<BTreeSet<_>>();
            for key in keys.into_iter().filter(|key| key.as_str() != "status") {
                residue(
                    read.get(key).unwrap_or(&Value::Null),
                    stored.get(key).unwrap_or(&Value::Null),
                    replayed.get(key).unwrap_or(&Value::Null),
                    &format!("{path}.{key}"),
                    out,
                );
            }
        }
        (Value::Array(read), Value::Array(stored), Value::Array(replayed)) => {
            if let (Some(read), Some(stored), Some(replayed)) =
                (by_id(read), by_id(stored), by_id(replayed))
            {
                let ids = read.keys().chain(stored.keys()).collect::<BTreeSet<_>>();
                for id in ids {
                    residue(
                        read.get(id).copied().unwrap_or(&Value::Null),
                        stored.get(id).copied().unwrap_or(&Value::Null),
                        replayed.get(id).copied().unwrap_or(&Value::Null),
                        &format!("{path}[{id}]"),
                        out,
                    );
                }
            } else if read.len() == stored.len() && stored.len() == replayed.len() {
                for (at, ((read, stored), replayed)) in
                    read.iter().zip(stored).zip(replayed).enumerate()
                {
                    residue(read, stored, replayed, &format!("{path}[{at}]"), out);
                }
            } else {
                out.push(path.to_owned());
            }
        }
        _ => out.push(path.to_owned()),
    }
}

/// The records of an arena by `id`; `None` where one has no `id`.
fn by_id(records: &[Value]) -> Option<BTreeMap<&str, &Value>> {
    records
        .iter()
        .map(|record| Some((record.get("id")?.as_str()?, record)))
        .collect()
}

/// The text by which each stored expression of a parameter that `owner`
/// holds spells a reference to a parameter.
type Spellings = HashMap<(Option<FeatureId>, ParameterId), String>;

fn spellings(decoded: &CadIr, stored: &HashMap<&ParameterId, &DesignParameter>) -> Spellings {
    let mut spellings = Spellings::new();
    for history in &decoded.model.parameters {
        let Some(ParameterDefinition::Expression(expression)) =
            stored.get(&history.id).map(|design| &design.definition)
        else {
            continue;
        };
        let spelled = match expression {
            Expression::Neutral(Neutral(Node::Reference(reference))) => {
                vec![(reference, history.expression.trim())]
            }
            Expression::Source { tokens, .. } => {
                aligned(tokens, &history.expression).unwrap_or_default()
            }
            Expression::Neutral(_) => Vec::new(),
        };
        for (reference, raw) in spelled {
            if let (RefTarget::Parameter(target), false) = (&reference.target, raw.is_empty()) {
                spellings
                    .entry((history.owner.clone(), target.clone()))
                    .or_insert_with(|| raw.to_owned());
            }
        }
    }
    spellings
}

/// Each reference token of `tokens` with the part of `text` it stands for,
/// where the text tokens split `text` around them.
fn aligned<'a, 't>(
    tokens: &'a [ExpressionToken<ParameterRef>],
    text: &'t str,
) -> Option<Vec<(&'a ParameterRef, &'t str)>> {
    let mut rest = text;
    let mut spelled = Vec::new();
    let mut pending = None;
    for token in tokens {
        match token {
            ExpressionToken::Text(part) => {
                if let Some(reference) = pending.take() {
                    let at = rest.find(part.as_str()).filter(|at| *at > 0)?;
                    spelled.push((reference, &rest[..at]));
                    rest = &rest[at..];
                }
                rest = rest.strip_prefix(part.as_str())?;
            }
            ExpressionToken::Reference(reference) => {
                if pending.replace(reference).is_some() {
                    return None;
                }
            }
        }
    }
    if let Some(reference) = pending {
        spelled.push((reference, rest));
        rest = "";
    }
    rest.is_empty().then_some(spelled)
}

/// The history text of `definition` and the parameters it references, for
/// the history parameter `history`.
fn history_expression(
    definition: &ParameterDefinition,
    history: &HistoryParameter,
    text: &dyn HistoryText,
    spellings: &Spellings,
    model: &Model,
) -> Result<(String, Vec<ParameterId>), String> {
    let spell = |reference: &ParameterRef| -> Result<(ParameterId, String), String> {
        let RefTarget::Parameter(target) = &reference.target else {
            return Err("it references a parameter of a document that was not read".into());
        };
        if reference.through.is_some() {
            return Err(format!("it references {target} through an occurrence"));
        }
        if let Some(spelled) = spellings.get(&(history.owner.clone(), target.clone())) {
            return Ok((target.clone(), spelled.clone()));
        }
        let referenced = model
            .parameters
            .iter()
            .find(|parameter| parameter.id == *target)
            .ok_or_else(|| format!("it references {target}, which no history parameter holds"))?;
        text.reference(referenced, model)
            .map(|spelled| (target.clone(), spelled))
            .ok_or_else(|| format!("the history has no name for {target}"))
    };
    let ParameterDefinition::Expression(expression) = definition else {
        return Err("it is no expression".into());
    };
    match expression {
        Expression::Neutral(Neutral(node)) => {
            if let Some((value, unit)) = literal(node) {
                return text
                    .literal(value, unit, &history.expression)
                    .map(|spelled| (spelled, Vec::new()))
                    .ok_or_else(|| "the history has no literal in its unit".to_owned());
            }
            match node {
                Node::Boolean(flag) => Ok((flag.to_string(), Vec::new())),
                Node::Reference(reference) => {
                    let (target, spelled) = spell(reference)?;
                    Ok((spelled, vec![target]))
                }
                _ => Err("it is a neutral expression other than a literal or one reference".into()),
            }
        }
        Expression::Source { dialect, tokens } if *dialect == text.dialect() => {
            let mut spelled = String::new();
            let mut references = Vec::new();
            for token in tokens {
                match token {
                    ExpressionToken::Text(part) => spelled.push_str(part),
                    ExpressionToken::Reference(reference) => {
                        let (target, name) = spell(reference)?;
                        spelled.push_str(&name);
                        if !references.contains(&target) {
                            references.push(target);
                        }
                    }
                }
            }
            Ok((spelled, references))
        }
        Expression::Source { .. } => Err("it is source text of another dialect".into()),
    }
}

/// The signed number and unit of a literal node.
fn literal<R>(node: &Node<R>) -> Option<(f64, &Unit)> {
    match node {
        Node::Number { value, unit } => Some((value.get(), unit)),
        Node::Unary {
            op: UnaryOp::Negate,
            operand,
        } => match operand.as_ref() {
            Node::Number { value, unit } => Some((-value.get(), unit)),
            _ => None,
        },
        _ => None,
    }
}

/// The history value of the cache `value`, in the variant of `stored`, the
/// history value before the edit, or else of `kind`.
fn history_value(
    value: Option<&ParameterValue>,
    stored: Option<&HistoryValue>,
    kind: &ValueKind,
) -> Option<HistoryValue> {
    Some(match value? {
        ParameterValue::Number(number) => match (stored, kind) {
            (Some(HistoryValue::Length(_)), _) | (None, ValueKind::Quantity(Quantity::LENGTH)) => {
                HistoryValue::Length(Length::new(number.get())?)
            }
            (Some(HistoryValue::Angle(_)), _) | (None, ValueKind::Quantity(Quantity::ANGLE)) => {
                HistoryValue::Angle(Angle::new(number.get())?)
            }
            _ => HistoryValue::Real(*number),
        },
        ParameterValue::Integer(integer) => HistoryValue::Integer(*integer),
        ParameterValue::Boolean(flag) => HistoryValue::Boolean(*flag),
        ParameterValue::Text(text) => HistoryValue::String(text.clone()),
    })
}

#[cfg(test)]
mod tests;
