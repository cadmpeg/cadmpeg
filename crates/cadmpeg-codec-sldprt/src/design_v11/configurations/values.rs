// SPDX-License-Identifier: Apache-2.0
//! Configuration-local parameter expressions and stored samples.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};

use cadmpeg_ir::design::configuration::ParameterOverride;
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::edit::{cache_value, evaluate, kind_value};
use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, Neutral, Node};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::features::parameters::DesignConfiguration as ModelConfiguration;
use cadmpeg_ir::features::{ConfigurationId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;

use super::Configurations;
use crate::design_v11::equations;
use crate::design_v11::parameters::{literal_like, same_value, stored_literal, typed_cache};
use crate::history::configuration::ConfigurationSamples;
use crate::loss::{NamedCount, SldprtLossCode};

type Tree<'a> = Cow<'a, Node<ParameterRef>>;

pub(in crate::design_v11) fn project(
    configurations: &mut Configurations,
    decoded: &[ModelConfiguration],
    samples: &ConfigurationSamples,
    parameters: &mut [DesignParameter],
    length_unit: Option<UnitSymbol>,
) -> (HashSet<ConfigurationId>, Vec<LossNote>) {
    let mut cuts = HashSet::new();
    let mut losses = Vec::new();
    let by_configuration = decoded
        .iter()
        .map(|configuration| (&configuration.id, configuration))
        .collect::<HashMap<_, _>>();
    let base = configurations
        .active
        .as_ref()
        .and_then(|id| by_configuration.get(id).copied());
    let Some(base) = base else {
        return (cuts, losses);
    };
    if let Some(observed) = samples.get(&base.id) {
        for parameter in parameters.iter_mut() {
            let Some(value) = sample(parameter, base, observed) else {
                continue;
            };
            if parameter
                .value
                .as_ref()
                .is_some_and(|held| same_value(held, &value))
            {
                continue;
            }
            let ParameterDefinition::Expression(expression) = &mut parameter.definition else {
                continue;
            };
            let literal = literal_like(expression, &value).or_else(|| {
                if matches!(
                    expression,
                    Expression::Neutral(Neutral(Node::Boolean(_) | Node::Text(_)))
                ) {
                    stored_literal(&parameter.kind, &value, length_unit)
                } else {
                    None
                }
            });
            if let Some(node) = literal {
                *expression = Expression::Neutral(Neutral(node));
                parameter.value = Some(value);
            } else if matches!(expression, Expression::Source { .. }) {
                parameter.value = Some(value);
            }
        }
    }
    let graph = Graph::new(parameters, length_unit);
    let base_values = samples
        .get(&base.id)
        .filter(|observed| !observed.is_empty())
        .map(|observed| {
            let mut values = vec![None; parameters.len()];
            for &position in &graph.ordered {
                values[position] = sample(&parameters[position], base, observed)
                    .or_else(|| graph.inherited(position, &values));
            }
            values
        });

    let empty = HashSet::new();
    for record in &mut configurations.records {
        if record.id == base.id {
            continue;
        }
        let Some(state) = by_configuration.get(&record.id).copied() else {
            continue;
        };
        let observed = samples.get(&record.id).unwrap_or(&empty);
        if state.parameter_values.is_empty() && observed.is_empty() {
            continue;
        }
        let mut effective = vec![None; parameters.len()];
        let mut unread = Vec::new();
        for (position, ordered) in graph
            .ordered
            .iter()
            .map(|&position| (position, true))
            .chain(graph.cyclic.iter().map(|&position| (position, false)))
        {
            let parameter = &parameters[position];
            let inherited = ordered
                .then(|| graph.inherited(position, &effective))
                .flatten();
            let Some(value) = sample(parameter, state, observed) else {
                effective[position] = inherited;
                if observed.contains(&parameter.id) {
                    unread.push(&parameter.id);
                }
                continue;
            };
            if inherited
                .as_ref()
                .is_some_and(|base| same_value(base, &value))
            {
                effective[position] = inherited;
                continue;
            }
            let ParameterDefinition::Expression(expression) = &parameter.definition else {
                effective[position] = inherited;
                unread.push(&parameter.id);
                continue;
            };
            let expression = inherited.and_then(|_| {
                literal_like(expression, &value)
                    .or_else(|| stored_literal(&parameter.kind, &value, length_unit))
                    .map(|node| Expression::Neutral(Neutral(node)))
            });
            if expression.is_none() {
                unread.push(&parameter.id);
            }
            effective[position] = Some(value.clone());
            record.overrides.parameters.insert(
                parameter.id.clone(),
                ParameterOverride {
                    expression,
                    value: Some(value),
                },
            );
        }
        if state.parameter_values.iter().any(|(id, value)| {
            if let Some(&position) = graph.by_id.get(id) {
                let matched = typed_cache(&parameters[position].kind, value.clone())
                    .zip(effective[position].as_ref())
                    .is_some_and(|(value, effective)| same_value(&value, effective));
                return !matched
                    && (observed.contains(id) || base.parameter_values.get(id) != Some(value));
            }
            base.parameter_values.get(id) != Some(value)
        }) {
            cuts.insert(record.id.clone());
        }
        if !unread.is_empty() {
            let mut named = NamedCount::default();
            for id in &unread {
                named.push(id.as_str());
            }
            losses.push(
                SldprtLossCode::DesignConfigurationParameterUnread.note(format!(
                    "{} samples in configuration {} lack a determined expression or \
                         compatible value kind. Expression-defined parameters keep compatible \
                         samples as value caches: {named}",
                    named.count(),
                    record.id,
                )),
            );
        }
    }

    drop(graph);
    for (parameter, value) in parameters.iter_mut().zip(base_values.into_iter().flatten()) {
        if value.is_some()
            && matches!(
                parameter.definition,
                ParameterDefinition::Expression(Expression::Neutral(_))
            )
        {
            parameter.value = value;
        }
    }
    (cuts, losses)
}

fn sample(
    parameter: &DesignParameter,
    state: &ModelConfiguration,
    observed: &HashSet<ParameterId>,
) -> Option<ParameterValue> {
    if !observed.contains(&parameter.id) {
        return None;
    }
    typed_cache(
        &parameter.kind,
        state.parameter_values.get(&parameter.id)?.clone(),
    )
}

struct Graph<'a> {
    parameters: &'a [DesignParameter],
    by_id: HashMap<&'a ParameterId, usize>,
    trees: Vec<Option<Tree<'a>>>,
    ordered: Vec<usize>,
    cyclic: Vec<usize>,
}

impl<'a> Graph<'a> {
    fn new(parameters: &'a [DesignParameter], length_unit: Option<UnitSymbol>) -> Self {
        let by_id = parameters
            .iter()
            .enumerate()
            .map(|(position, parameter)| (&parameter.id, position))
            .collect::<HashMap<_, _>>();
        let trees = parameters
            .iter()
            .map(|parameter| match &parameter.definition {
                ParameterDefinition::Expression(Expression::Neutral(Neutral(node))) => {
                    Some(Cow::Borrowed(node))
                }
                ParameterDefinition::Expression(Expression::Source {
                    dialect: ExpressionDialect::SolidWorks,
                    tokens,
                }) => {
                    equations::source_tree(tokens, parameters, &parameter.kind, length_unit, &by_id)
                        .map(Cow::Owned)
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut incoming = vec![0; parameters.len()];
        let mut dependents = vec![Vec::new(); parameters.len()];
        for (position, tree) in trees.iter().enumerate() {
            let Some(tree) = tree else { continue };
            let mut pending = vec![tree.as_ref()];
            while let Some(node) = pending.pop() {
                if let Node::Reference(reference) = node {
                    if let Some(dependency) = local(reference).and_then(|id| by_id.get(id)) {
                        incoming[position] += 1;
                        dependents[*dependency].push(position);
                    }
                }
                pending.extend(node.operands());
            }
        }
        let mut ready = incoming
            .iter()
            .enumerate()
            .filter_map(|(position, &count)| (count == 0).then_some(position))
            .collect::<VecDeque<_>>();
        let mut ordered = Vec::with_capacity(parameters.len());
        while let Some(position) = ready.pop_front() {
            ordered.push(position);
            for &dependent in &dependents[position] {
                incoming[dependent] -= 1;
                if incoming[dependent] == 0 {
                    ready.push_back(dependent);
                }
            }
        }
        let cyclic = incoming
            .into_iter()
            .enumerate()
            .filter_map(|(position, count)| (count > 0).then_some(position))
            .collect();
        Self {
            parameters,
            by_id,
            trees,
            ordered,
            cyclic,
        }
    }

    fn inherited(
        &self,
        position: usize,
        effective: &[Option<ParameterValue>],
    ) -> Option<ParameterValue> {
        let parameter = &self.parameters[position];
        if matches!(parameter.definition, ParameterDefinition::Driven) {
            return parameter.value.clone();
        }
        let result = evaluate(self.trees[position].as_deref()?, &mut |reference| {
            let &position = self.by_id.get(local(reference)?)?;
            cache_value(
                &self.parameters[position].kind,
                effective[position].as_ref()?,
            )
        })
        .ok()?;
        kind_value(&parameter.kind, result).ok()
    }
}

fn local(reference: &ParameterRef) -> Option<&ParameterId> {
    match reference {
        ParameterRef {
            target: RefTarget::Parameter(id),
            through: None,
        } => Some(id),
        _ => None,
    }
}
