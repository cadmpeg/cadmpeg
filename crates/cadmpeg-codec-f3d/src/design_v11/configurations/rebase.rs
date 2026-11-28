// SPDX-License-Identifier: Apache-2.0
//! The base of a design whose active configurations resolve parameter
//! overrides, spelled as Fusion stores it: the Design stream holds the state
//! of the active row (f3d.md §1.2), so the active configuration is the base
//! and states no overrides (cadir-v11.md §5.11).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::codec::write::value_unwritable;
use cadmpeg_ir::design::configuration::ParameterOverride;
use cadmpeg_ir::design::dimension::RefTarget;
use cadmpeg_ir::design::expr::{Expression, ExpressionToken, Neutral, Node};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::{
    DesignParameter as LegacyParameter, ParameterValue as LegacyValue,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{ConfigurationId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::units::Tolerances;

use super::{agree, chain, legacy_value, resolved, spelled, Values};

/// A document whose parameters hold, as their own, the values that the
/// active configuration of their scope resolves.
pub(crate) struct Rebased {
    pub(crate) ir: CadIr,
    /// The v10 form of each parameter that takes the active configuration's
    /// value.
    pub(crate) parameters: BTreeMap<ParameterId, Own>,
    pub(crate) losses: Vec<LossNote>,
}

/// A parameter's own Fusion text, where it changes, and its own value.
#[derive(Debug, Clone)]
pub(crate) struct Own {
    expression: Option<(String, DistinctMembers<ParameterId>)>,
    value: Option<LegacyValue>,
}

impl Rebased {
    /// Write each rebased parameter's own expression and value into
    /// `parameters`, v10 parameters.
    pub(crate) fn apply(&self, parameters: &mut [LegacyParameter]) {
        apply(&self.parameters, parameters);
    }
}

fn apply(rebased: &BTreeMap<ParameterId, Own>, parameters: &mut [LegacyParameter]) {
    for parameter in parameters {
        let Some(own) = rebased.get(&parameter.id) else {
            continue;
        };
        if let Some((text, dependencies)) = &own.expression {
            parameter.expression.clone_from(text);
            parameter.dependencies.clone_from(dependencies);
        }
        parameter.value.clone_from(&own.value);
    }
}

/// One parameter that takes the active configuration's expression or value.
struct Taken<'a> {
    id: &'a ParameterId,
    /// The expression the active configuration gives it, where that is
    /// another expression than its own.
    expression: Option<Expression>,
    value: Option<ParameterValue>,
    own: Own,
}

/// `ir` with each parameter that the active configuration of its scope
/// resolves otherwise than its own taking that configuration's expression
/// and value as its own; `None` where no parameter does.
///
/// The active configuration's own override of such a parameter is removed.
/// A configuration that does not inherit from the active one, and resolves
/// the parameter to its own expression, states its previous value as an
/// override: its own value where nothing it inherits states one, and the
/// previous expression with the value of the override it inherits where
/// that override states no expression. A parameter whose expression has no
/// Fusion text, or whose value is of another kind, keeps its own. The
/// losses name each parameter whose previous own value no configuration
/// resolves.
pub(crate) fn rebased(ir: &CadIr) -> Option<Rebased> {
    let design = &ir.design;
    let records = design
        .configurations
        .iter()
        .map(|configuration| (&configuration.id, configuration))
        .collect::<BTreeMap<_, _>>();
    let mut definitions =
        BTreeMap::<&ParameterId, (Option<Expression>, Option<ParameterValue>)>::new();
    let mut legacy = BTreeMap::new();
    let mut removed = BTreeSet::<(&ConfigurationId, &ParameterId)>::new();
    let mut added = Vec::<(&ConfigurationId, &ParameterId, ParameterOverride)>::new();
    let mut uncarried = BTreeSet::new();
    for scope in &design.scopes {
        let Some(active) = scope
            .active_configuration
            .as_ref()
            .and_then(|id| records.get(id).copied())
            .filter(|active| active.scope == scope.definition)
        else {
            continue;
        };
        let stated = resolved(&chain(active, &records)).parameters;
        if stated.is_empty() {
            continue;
        }
        let scoped = design
            .parameters
            .iter()
            .filter(|parameter| parameter.scope == scope.definition)
            .map(|parameter| (&parameter.id, parameter))
            .collect::<HashMap<_, _>>();
        let taken = taken(&stated, &scoped, &ir.tolerances);
        if taken.is_empty() {
            continue;
        }
        let mut carried = BTreeSet::new();
        for others in design
            .configurations
            .iter()
            .filter(|record| record.scope == scope.definition && record.id != active.id)
        {
            let chain = chain(others, &records);
            if chain.iter().any(|link| link.id == active.id) {
                continue;
            }
            let inherited = resolved(&chain).parameters;
            let mut values = Values::new(&inherited, &scoped);
            for taken in taken.iter().filter(|taken| stated.contains_key(taken.id)) {
                let previous = match &scoped[taken.id].definition {
                    ParameterDefinition::Expression(own) => Some(own),
                    _ => None,
                };
                let restated = match inherited.get(taken.id) {
                    Some(link) if link.expression.is_some() => {
                        if link.expression.as_ref() == previous {
                            carried.insert(taken.id);
                        }
                        None
                    }
                    Some(link) => {
                        carried.insert(taken.id);
                        taken.expression.as_ref().map(|_| ParameterOverride {
                            expression: previous.cloned(),
                            value: link.value.clone(),
                        })
                    }
                    None => {
                        carried.insert(taken.id);
                        Some(ParameterOverride {
                            expression: taken.expression.as_ref().and(previous.cloned()),
                            value: values.value(taken.id),
                        })
                    }
                };
                if let Some(restated) =
                    restated.filter(|over| over.expression.is_some() || over.value.is_some())
                {
                    added.push((&others.id, taken.id, restated));
                }
            }
        }
        for taken in taken {
            if stated.contains_key(taken.id) && !carried.contains(taken.id) {
                uncarried.insert(taken.id);
            }
            if active.overrides.parameters.contains_key(taken.id) {
                removed.insert((&active.id, taken.id));
            }
            legacy.insert(taken.id.clone(), taken.own);
            definitions.insert(taken.id, (taken.expression, taken.value));
        }
    }
    if definitions.is_empty() {
        return None;
    }
    let mut out = ir.clone();
    for parameter in &mut out.design.parameters {
        let Some((expression, value)) = definitions.get(&parameter.id) else {
            continue;
        };
        if let Some(expression) = expression {
            parameter.definition = ParameterDefinition::Expression(expression.clone());
        }
        parameter.value.clone_from(value);
    }
    apply(&legacy, &mut out.model.parameters);
    for record in &mut out.design.configurations {
        record
            .overrides
            .parameters
            .retain(|id, _| !removed.contains(&(&record.id, id)));
        for (_, id, over) in added.iter().filter(|(at, _, _)| **at == record.id) {
            record
                .overrides
                .parameters
                .insert((*id).clone(), over.clone());
        }
    }
    let losses = value_unwritable(
        "F3D",
        "writes these parameters with the expression and value that the active configuration of their design scope resolves, and no configuration resolves their own: the Design stream stores the state of the active row (f3d.md §1.2), which is the base",
        uncarried.iter().map(|id| format!("design.parameters[{id}]")),
    )
    .into_iter()
    .collect();
    Some(Rebased {
        ir: out,
        parameters: legacy,
        losses,
    })
}

/// The parameters of `scoped` whose expression or value in the
/// configuration that resolves `stated` differs from their own, with their
/// Fusion text and value there: each that `stated` names, where it has
/// Fusion text and a value of its kind, and then each other one whose
/// expression evaluates otherwise once those take their values.
fn taken<'a>(
    stated: &BTreeMap<ParameterId, ParameterOverride>,
    scoped: &HashMap<&'a ParameterId, &'a DesignParameter>,
    tolerances: &Tolerances,
) -> Vec<Taken<'a>> {
    let names = scoped
        .values()
        .filter_map(|parameter| Some((&parameter.id, parameter.name.as_ref()?.as_str())))
        .collect::<HashMap<_, _>>();
    let vocabulary = scoped
        .values()
        .filter_map(|parameter| Some((parameter.name.as_ref()?.as_str(), &parameter.id)))
        .collect::<HashMap<&str, &ParameterId>>();
    let differs = |parameter: &DesignParameter, value: &Option<ParameterValue>| {
        let typed = |value: &Option<ParameterValue>| {
            value
                .as_ref()
                .map(|value| legacy_value(value, &parameter.kind))
        };
        match (typed(value), typed(&parameter.value)) {
            (Some(Some(now)), Some(Some(then))) => !agree(&now, &then, tolerances),
            _ => value != &parameter.value,
        }
    };
    let mut values = Values::new(stated, scoped);
    let mut taken = Vec::new();
    for (id, over) in stated {
        let Some((&id, &parameter)) = scoped.get_key_value(id) else {
            continue;
        };
        let expression = match (&over.expression, &parameter.definition) {
            (Some(stated), ParameterDefinition::Expression(own)) if stated != own => {
                Some(stated.clone())
            }
            (Some(_), ParameterDefinition::Expression(_)) | (None, _) => None,
            (Some(_), _) => continue,
        };
        let value = values.value(id);
        if expression.is_none() && !differs(parameter, &value) {
            continue;
        }
        let text = match &expression {
            Some(expression) => {
                let Some(text) = spelled(expression, &names).filter(|text| {
                    &super::expression(text, &parameter.kind, &vocabulary) == expression
                }) else {
                    continue;
                };
                Some((text, references(expression)))
            }
            None => None,
        };
        let legacy = match &value {
            Some(value) => match legacy_value(value, &parameter.kind) {
                Some(legacy) => Some(legacy),
                None => continue,
            },
            None => None,
        };
        taken.push(Taken {
            id,
            expression,
            value,
            own: Own {
                expression: text,
                value: legacy,
            },
        });
    }
    let applied = taken
        .iter()
        .map(|taken| (taken.id.clone(), stated[taken.id].clone()))
        .collect::<BTreeMap<_, _>>();
    let mut after = Values::new(&applied, scoped);
    let base = BTreeMap::new();
    let mut before = Values::new(&base, scoped);
    let mut dependents = scoped
        .iter()
        .filter(|(id, _)| !stated.contains_key(**id))
        .collect::<Vec<_>>();
    dependents.sort_by_key(|(id, _)| **id);
    for (id, parameter) in dependents {
        let (Some(now), Some(then)) = (after.evaluation(id), before.evaluation(id)) else {
            continue;
        };
        let (Some(legacy), Some(previous)) = (
            legacy_value(&now, &parameter.kind),
            legacy_value(&then, &parameter.kind),
        ) else {
            continue;
        };
        if agree(&legacy, &previous, tolerances) {
            continue;
        }
        taken.push(Taken {
            id,
            expression: None,
            value: Some(now),
            own: Own {
                expression: None,
                value: Some(legacy),
            },
        });
    }
    taken
}

/// The parameters `expression` reads, in order, each once.
fn references(expression: &Expression) -> DistinctMembers<ParameterId> {
    let mut read = Vec::new();
    let mut push = |target: &RefTarget| {
        if let RefTarget::Parameter(id) = target {
            if !read.contains(id) {
                read.push(id.clone());
            }
        }
    };
    match expression {
        Expression::Neutral(Neutral(Node::Reference(reference))) => push(&reference.target),
        Expression::Neutral(_) => {}
        Expression::Source { tokens, .. } => {
            for token in tokens {
                if let ExpressionToken::Reference(reference) = token {
                    push(&reference.target);
                }
            }
        }
    }
    DistinctMembers::try_from(read).unwrap_or_default()
}
