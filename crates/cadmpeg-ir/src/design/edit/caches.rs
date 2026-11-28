// SPDX-License-Identifier: Apache-2.0
//! The edits of a parameter or a feature definition, and the caches they keep
//! honest (§4, Edit helpers).
//!
//! An edit marks `Stale` every evaluation and current state that depends on
//! the edited feature or parameter, and the solve status of every sketch
//! whose dimensions, parameters, or selection referents it edits. It then
//! recomputes the cache of every dependent neutral expression that has no
//! uncached measure, and clears every dependent cache it cannot recompute: a
//! parameter's value, the `Cell` and `Unchecked` deviations of a fit, a
//! measure's cached value, an override's value, and the `resolved` of a
//! transformation whose steps bind a dependent parameter. An override with
//! no expression whose value it clears states nothing, so it is removed.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::eval::{
    cache_value, check_types, evaluate, kind_type, kind_value, EvalError, ExpressionSite, Type,
    Value,
};
use super::history::{currents, Context, Currents, Dependents, Graph, History, Node};
use super::resolve::ParameterOverrides;
use super::wire::{clear_measures, clear_transformations, reads, rewrite};
use super::{EditError, Literal, ParameterDrivers};
use crate::design::configuration::{
    ArrangementRef, DesignConfiguration, Overrides, ParameterOverride, Stated,
};
use crate::design::dimension::{Dimension, ParameterRef, RefTarget};
use crate::design::evaluation::EvaluationStatus;
use crate::design::expr::{print, Expression, ExpressionToken, Neutral, Node as ExprNode, UnaryOp};
use crate::design::feature::FeatureDefinition;
use crate::design::parameter::{
    FitDeviations, ParameterDefinition, ParameterOwner, ParameterValue, Tolerance,
};
use crate::design::refs::{ConfigurationRef, Evaluated, Member};
use crate::design::scope::DesignScope;
use crate::design::units::ValueKind;
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::ids::OccurrenceId;
use crate::pmi::PmiDefinition;
use crate::scalar::NonNegativeReal;
use crate::CadIr;

/// The outcome of one attempt to recompute a cache.
enum Attempt {
    /// A reference names a cache that is itself still to be recomputed.
    Blocked,
    /// The recomputed cache, or `None` where it cannot be recomputed.
    Done(Option<ParameterValue>),
}

/// The references of an expression, in order of first use.
fn references(expression: &Expression) -> Result<Vec<ParameterRef>, EditError> {
    Ok(match expression {
        Expression::Neutral(Neutral(node)) => print(node).map_err(super::malformed)?.refs,
        Expression::Source { tokens, .. } => tokens
            .iter()
            .filter_map(|token| match token {
                ExpressionToken::Reference(reference) => Some(reference.clone()),
                ExpressionToken::Text(_) => None,
            })
            .collect(),
    })
}

fn literal_node(literal: Literal) -> Result<ExprNode<ParameterRef>, String> {
    Ok(match literal {
        Literal::Number { value, unit } => {
            let magnitude = NonNegativeReal::new(value.get().abs())
                .ok_or_else(|| "the value is not finite".to_owned())?;
            let number = ExprNode::Number {
                value: magnitude,
                unit,
            };
            if value.get() < 0.0 {
                ExprNode::Unary {
                    op: UnaryOp::Negate,
                    operand: Box::new(number),
                }
            } else {
                number
            }
        }
        Literal::Boolean(value) => ExprNode::Boolean(value),
        Literal::Text(text) => ExprNode::Text(text),
    })
}

/// Evaluates an expression of the declared kind. `value_of` gives a
/// reference's cache; a reference to a parameter of `pending` blocks. A
/// reference `through` an occurrence path has no value: the edit resolves no
/// path, so it clears what reads one.
fn attempt(
    expression: &Expression,
    kind: &ValueKind,
    pending: &BTreeSet<ParameterId>,
    value_of: &dyn Fn(&ParameterId) -> Option<Value>,
) -> Attempt {
    let Expression::Neutral(Neutral(node)) = expression else {
        return Attempt::Done(None);
    };
    let mut blocked = false;
    let result = evaluate(
        node,
        &mut |reference: &ParameterRef| match &reference.target {
            RefTarget::Parameter(_) if reference.through.is_some() => None,
            RefTarget::Parameter(target) if pending.contains(target) => {
                blocked = true;
                None
            }
            RefTarget::Parameter(target) => value_of(target),
            RefTarget::External { .. } => None,
        },
    );
    if blocked {
        return Attempt::Blocked;
    }
    Attempt::Done(result.ok().and_then(|value| kind_value(kind, value).ok()))
}

/// Whether a new expression refuses its kind outright: its tree does not
/// type-check, or its root or the value it evaluates to is not of the kind;
/// as opposed to lacking a value it reads. An integer-kind value that is not
/// whole is not refused: that rule is a Warning, and the value has no cache
/// (§4, §5.13).
fn kind_refusal(
    parameter: &ParameterId,
    expression: &Expression,
    kind: &ValueKind,
    type_of: &dyn Fn(&ParameterRef) -> Result<Type, String>,
    value_of: &dyn Fn(&ParameterId) -> Option<Value>,
) -> Result<(), EditError> {
    let Expression::Neutral(Neutral(node)) = expression else {
        return Ok(());
    };
    let refusal = |reason: String| EditError::KindMismatch {
        parameter: parameter.clone(),
        reason,
    };
    check_types(node, ExpressionSite::Parameter, type_of)
        .and_then(|found| kind_type(kind, found))
        .map_err(refusal)?;
    let result = evaluate(
        node,
        &mut |reference: &ParameterRef| match &reference.target {
            RefTarget::Parameter(_) if reference.through.is_some() => None,
            RefTarget::Parameter(target) => value_of(target),
            RefTarget::External { .. } => None,
        },
    );
    match result {
        Ok(_) if *kind == ValueKind::Integer => Ok(()),
        Ok(value) => kind_value(kind, value).map(|_| ()).map_err(refusal),
        Err(EvalError::Type(reason)) => Err(refusal(reason)),
        Err(EvalError::Unbound(_) | EvalError::Domain(_)) => Ok(()),
    }
}

impl CadIr {
    fn parameter_index(&self) -> BTreeMap<ParameterId, usize> {
        self.design
            .parameters
            .iter()
            .enumerate()
            .map(|(at, parameter)| (parameter.id.clone(), at))
            .collect()
    }

    /// A parameter's cache in a configuration as an expression value.
    fn cache_in(
        &self,
        parameter: &ParameterId,
        configuration: Option<&ConfigurationId>,
    ) -> Option<Value> {
        let record = self.design_parameter(parameter).ok()?;
        let cache = self.parameter_cache(parameter, configuration).ok()??;
        cache_value(&record.kind, cache)
    }

    /// A parameter's cache as an expression value, where `levels` are the
    /// parameter overrides that apply, nearest first, and `stated` what else
    /// they state (§5.13).
    fn cache_over_levels(
        &self,
        parameter: &ParameterId,
        levels: &[&ParameterOverrides],
        stated: Stated,
    ) -> Option<Value> {
        let record = self.design_parameter(parameter).ok()?;
        cache_value(&record.kind, self.cache_over(record, levels, stated)?)
    }

    /// A configuration and its ancestors, nearest first; none for the base.
    /// `None` where the configuration does not resolve.
    fn chain_of(
        &self,
        configuration: Option<&ConfigurationId>,
    ) -> Option<Vec<&DesignConfiguration>> {
        match configuration {
            Some(configuration) => self.design.configuration_chain(configuration).ok(),
            None => Some(Vec::new()),
        }
    }

    /// The cache of a prototype parameter in an occurrence of `scope` whose
    /// own parameter overrides are `entries`: the value `entries` holds, else
    /// the value in the configuration of its scope that the occurrence uses,
    /// and then in the arrangements in force there, of each kind the one the
    /// occurrence names where it names one (§5.1, §5.11, §5.13). `None` where
    /// the occurrence uses a configuration or an arrangement that the
    /// parameter's scope does not hold.
    fn occurrence_cache(
        &self,
        scope: &DesignScope,
        occurrence: &OccurrenceId,
        parameter: &ParameterId,
        entries: &ParameterOverrides,
    ) -> Option<Value> {
        let record = self.design_parameter(parameter).ok()?;
        let prototype = &record.scope;
        let configuration = match self.occurrence_configuration(scope, occurrence).ok()? {
            None => self.active_configuration_of(prototype).cloned(),
            Some(ConfigurationRef::Local(configuration)) => {
                self.design.configurations.iter().find(|candidate| {
                    candidate.id == configuration && &candidate.scope == prototype
                })?;
                Some(configuration)
            }
            Some(ConfigurationRef::Named(_)) => return None,
        };
        let mut in_force = Vec::new();
        for named in &scope.occurrences.get(occurrence)?.arrangements {
            let ArrangementRef::Local(id) = named else {
                return None;
            };
            in_force.push(
                self.design
                    .arrangements
                    .iter()
                    .find(|candidate| &candidate.id == id && &candidate.scope == prototype)?,
            );
        }
        let own = in_force.len();
        for arrangement in self
            .design
            .arrangements_in_force(prototype, configuration.as_ref())
            .ok()?
        {
            if !in_force[..own]
                .iter()
                .any(|held| held.kind == arrangement.kind)
            {
                in_force.push(arrangement);
            }
        }
        let chain = self.chain_of(configuration.as_ref())?;
        let mut levels = vec![entries];
        levels.extend(
            in_force
                .iter()
                .map(|arrangement| &arrangement.overrides.parameters),
        );
        levels.extend(parameter_levels(&chain));
        let stated = Stated {
            anything: true,
            occurrences: Stated::of(&chain).occurrences
                || in_force
                    .iter()
                    .any(|arrangement| !arrangement.overrides.occurrences.is_empty()),
        };
        self.cache_over_levels(parameter, &levels, stated)
    }

    /// [`Editor::set_dimension`](super::Editor::set_dimension) on the
    /// document itself.
    pub(super) fn set_dimension(
        &mut self,
        parameter: &ParameterId,
        value: Literal,
    ) -> Result<(), EditError> {
        let record = self.design_parameter(parameter)?;
        let drivers = match &record.definition {
            ParameterDefinition::Unset => None,
            ParameterDefinition::Driven => Some(ParameterDrivers::Geometry),
            ParameterDefinition::Expression(expression) => {
                let found = references(expression)?;
                (!found.is_empty()).then_some(ParameterDrivers::References(found))
            }
        };
        if let Some(drivers) = drivers {
            return Err(EditError::DrivenParameter {
                parameter: parameter.clone(),
                drivers,
            });
        }
        let refusal = |reason: String| EditError::KindMismatch {
            parameter: parameter.clone(),
            reason,
        };
        if record.kind == ValueKind::Integer {
            if let Literal::Number { value, unit } = &value {
                if !unit.0.is_empty() {
                    return Err(refusal("an integer takes a number with no unit".into()));
                }
                if value.get().fract() != 0.0 {
                    return Err(refusal(format!("{} is not a whole number", value.get())));
                }
            }
        }
        let node = literal_node(value).map_err(refusal)?;
        self.set_parameter_expression(parameter, Expression::Neutral(Neutral(node)))
    }

    /// [`Editor::set_parameter_expression`](super::Editor::set_parameter_expression)
    /// on the document itself.
    pub(crate) fn set_parameter_expression(
        &mut self,
        parameter: &ParameterId,
        expression: Expression,
    ) -> Result<(), EditError> {
        let kind = self.design_parameter(parameter)?.kind.clone();
        for reference in references(&expression)? {
            if let RefTarget::Parameter(target) = &reference.target {
                self.design_parameter(target)?;
            }
        }
        let type_of = |reference: &ParameterRef| match &reference.target {
            RefTarget::Parameter(target) => {
                let kind = &self
                    .design_parameter(target)
                    .map_err(|error| error.to_string())?
                    .kind;
                Type::of_kind(kind)
                    .ok_or_else(|| format!("{target} has the kind {kind:?}, with no neutral value"))
            }
            RefTarget::External { .. } => Ok(Type::Unknown),
        };
        kind_refusal(parameter, &expression, &kind, &type_of, &|target| {
            if target == parameter {
                None
            } else {
                self.cache_in(target, None)
            }
        })?;
        let at = self.parameter_index()[parameter];
        self.design.parameters[at].definition = ParameterDefinition::Expression(expression);
        self.after_edit([Node::Parameter(parameter.clone())])
    }

    /// [`Editor::set_feature_definition`](super::Editor::set_feature_definition)
    /// on the document itself.
    pub(super) fn set_feature_definition(
        &mut self,
        feature: &FeatureId,
        definition: FeatureDefinition,
    ) -> Result<(), EditError> {
        let record = self
            .design
            .features
            .iter_mut()
            .find(|candidate| &candidate.id == feature)
            .ok_or_else(|| EditError::UnknownFeature(feature.clone()))?;
        record.definition = definition;
        self.after_edit([Node::Feature(feature.clone())])
    }

    /// Marks and refreshes what depends on the seeds.
    pub(super) fn after_edit(
        &mut self,
        seeds: impl IntoIterator<Item = Node>,
    ) -> Result<(), EditError> {
        let history = History::new(&self.design);
        let graph = Graph::new(&self.design, &history)?;
        let currents = currents(&self.design);
        let dependents = graph.dependents(&history, &currents, seeds);
        self.mark_states_stale(&history, &dependents.features);
        self.mark_sketches_stale(&dependents.features);
        self.refresh_caches(&history, &currents, &dependents)?;
        self.clear_bound_fits(&dependents);
        Ok(())
    }

    /// Clears cached fit deviations bound to an edited parameter.
    fn clear_bound_fits(&mut self, dependents: &Dependents) {
        for annotation in &mut self.model.pmi {
            let PmiDefinition::Dimension(dimension) = &mut annotation.definition else {
                continue;
            };
            let bound = match &dimension.nominal {
                Some(Dimension::Parameter(parameter) | Dimension::Negated(parameter)) => {
                    dependents.parameters.contains(parameter)
                }
                Some(Dimension::Literal(_)) | None => false,
            };
            if let (
                true,
                Some(Tolerance::Fit {
                    deviations:
                        deviations @ Some(FitDeviations::Cell(_) | FitDeviations::Unchecked(_)),
                    ..
                }),
            ) = (bound, &mut dimension.tolerance)
            {
                *deviations = None;
            }
        }
    }

    /// Marks `Stale` every evaluation and current state that depends on any
    /// of `features`.
    pub(super) fn mark_states_stale(&mut self, history: &History, features: &BTreeSet<FeatureId>) {
        for evaluation in &mut self.design.evaluations {
            let stale = match &evaluation.of {
                Evaluated::State(at) => history.state_depends(at, features),
                Evaluated::Tool(feature) => features.contains(feature),
            };
            if stale {
                evaluation.status = EvaluationStatus::Stale;
            }
        }
        for scope in &mut self.design.scopes {
            for current in &mut scope.current {
                if history.state_depends(&current.at, features) {
                    current.status = EvaluationStatus::Stale;
                }
            }
        }
    }

    /// Marks `Stale` the solve status of every sketch whose feature is one of
    /// `features`.
    fn mark_sketches_stale(&mut self, features: &BTreeSet<FeatureId>) {
        for sketch in &mut self.design.sketches {
            if features.contains(&sketch.owner) {
                sketch.solve.status = EvaluationStatus::Stale;
            }
        }
        for sketch in &mut self.design.spatial_sketches {
            if features.contains(&sketch.owner) {
                sketch.solve.status = EvaluationStatus::Stale;
            }
        }
    }

    fn refresh_caches(
        &mut self,
        history: &History,
        currents: &Currents,
        dependents: &Dependents,
    ) -> Result<(), EditError> {
        self.clear_stale_measures(history, currents, &dependents.features)?;
        self.clear_stale_transformations(&dependents.parameters)?;
        let targets = &dependents.parameters;
        self.recompute_values(targets);
        for parameter in &mut self.design.parameters {
            if !targets.contains(&parameter.id) {
                continue;
            }
            if let Some(Tolerance::Fit { deviations, .. }) = &mut parameter.tolerance {
                if matches!(
                    deviations,
                    Some(FitDeviations::Cell(_) | FitDeviations::Unchecked(_))
                ) {
                    *deviations = None;
                }
            }
        }
        self.recompute_overrides(targets);
        Ok(())
    }

    /// Clears the cached value of every measure whose anchored state depends
    /// on any of `features`.
    fn clear_stale_measures(
        &mut self,
        history: &History,
        currents: &Currents,
        features: &BTreeSet<FeatureId>,
    ) -> Result<(), EditError> {
        if features.is_empty() {
            return Ok(());
        }
        let check = MeasureCheck {
            history,
            currents,
            features,
        };
        let contexts: BTreeMap<ParameterId, Context> = self
            .design
            .parameters
            .iter()
            .map(|parameter| (parameter.id.clone(), parameter_context(parameter)))
            .collect();
        let feature_context = |feature: &FeatureId| {
            let member = Member::Feature(feature.clone());
            Context {
                scope: history.scope_of(&member).cloned(),
                feature: Some(member),
            }
        };
        for parameter in &mut self.design.parameters {
            let context = parameter_context(parameter);
            rewrite(&mut parameter.definition, |json| {
                Ok(check.clear(json, &context))
            })?;
        }
        let overrides = self
            .design
            .configurations
            .iter_mut()
            .map(|configuration| &mut configuration.overrides)
            .chain(
                self.design
                    .arrangements
                    .iter_mut()
                    .map(|arrangement| &mut arrangement.overrides),
            );
        for overrides in overrides {
            for (parameter, value) in &mut overrides.parameters {
                let context = contexts.get(parameter).cloned().unwrap_or_default();
                rewrite(value, |json| Ok(check.clear(json, &context)))?;
            }
            for (feature, state) in &mut overrides.features {
                let context = feature_context(feature);
                rewrite(state, |json| Ok(check.clear(json, &context)))?;
            }
        }
        for scope in &mut self.design.scopes {
            for occurrence in scope.occurrences.values_mut() {
                for (parameter, value) in &mut occurrence.parameters {
                    let context = contexts.get(parameter).cloned().unwrap_or_default();
                    rewrite(value, |json| Ok(check.clear(json, &context)))?;
                }
            }
        }
        for feature in &mut self.design.features {
            let context = feature_context(&feature.id);
            rewrite(&mut feature.definition, |json| {
                Ok(check.clear(json, &context))
            })?;
        }
        Ok(())
    }

    /// Clears the `resolved` of every `Transformation` one of whose steps
    /// binds a parameter of `parameters` (§4, §6.5), in the features and in
    /// the definitions the configurations and arrangements override.
    fn clear_stale_transformations(
        &mut self,
        parameters: &BTreeSet<ParameterId>,
    ) -> Result<(), EditError> {
        if parameters.is_empty() {
            return Ok(());
        }
        let bound: BTreeSet<String> = parameters
            .iter()
            .map(|parameter| parameter.as_str().to_owned())
            .collect();
        for feature in &mut self.design.features {
            rewrite(&mut feature.definition, |json| {
                Ok(clear_transformations(json, &bound))
            })?;
        }
        let overrides = self
            .design
            .configurations
            .iter_mut()
            .map(|configuration| &mut configuration.overrides)
            .chain(
                self.design
                    .arrangements
                    .iter_mut()
                    .map(|arrangement| &mut arrangement.overrides),
            );
        for overrides in overrides {
            for state in overrides.features.values_mut() {
                rewrite(state, |json| Ok(clear_transformations(json, &bound)))?;
            }
        }
        Ok(())
    }

    /// Recomputes the value of every parameter of `targets` whose definition
    /// is a neutral expression it can evaluate, in dependency order, and
    /// clears the value of every other one.
    fn recompute_values(&mut self, targets: &BTreeSet<ParameterId>) {
        let index = self.parameter_index();
        let mut pending: BTreeSet<ParameterId> = targets
            .iter()
            .filter(|parameter| index.contains_key(*parameter))
            .cloned()
            .collect();
        loop {
            let mut done = Vec::new();
            for parameter in &pending {
                let record = &self.design.parameters[index[parameter]];
                let outcome = match &record.definition {
                    ParameterDefinition::Expression(expression) => {
                        attempt(expression, &record.kind, &pending, &|target| {
                            self.cache_in(target, None)
                        })
                    }
                    ParameterDefinition::Driven | ParameterDefinition::Unset => Attempt::Done(None),
                };
                if let Attempt::Done(value) = outcome {
                    done.push((parameter.clone(), value));
                }
            }
            if done.is_empty() {
                break;
            }
            for (parameter, value) in done {
                self.design.parameters[index[&parameter]].value = value;
                pending.remove(&parameter);
            }
        }
        for parameter in pending {
            self.design.parameters[index[&parameter]].value = None;
        }
    }

    /// The value each design parameter with no cache and a neutral
    /// expression evaluates to in the base, in dependency order: a
    /// reference reads the cache of its parameter or the value this
    /// evaluation gave it. A parameter it cannot evaluate has no entry.
    pub(crate) fn uncached_values(
        &self,
        written: &HashSet<&str>,
    ) -> BTreeMap<ParameterId, ParameterValue> {
        let index = self
            .design
            .parameters
            .iter()
            .filter(|parameter| written.contains(parameter.id.as_str()))
            .map(|parameter| (&parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        let mut pending: BTreeSet<ParameterId> = index
            .values()
            .filter(|parameter| {
                parameter.value.is_none()
                    && matches!(
                        parameter.definition,
                        ParameterDefinition::Expression(Expression::Neutral(_))
                    )
            })
            .map(|parameter| parameter.id.clone())
            .collect();
        let mut values = BTreeMap::new();
        loop {
            let mut done = Vec::new();
            for parameter in &pending {
                let record = index[parameter];
                let ParameterDefinition::Expression(expression) = &record.definition else {
                    continue;
                };
                let value_of = |target: &ParameterId| {
                    let target_record = index.get(target)?;
                    let value = values.get(target).or(target_record.value.as_ref())?;
                    cache_value(&target_record.kind, value)
                };
                if let Attempt::Done(value) = attempt(expression, &record.kind, &pending, &value_of)
                {
                    done.push((parameter.clone(), value));
                }
            }
            if done.is_empty() {
                return values;
            }
            for (parameter, value) in done {
                pending.remove(&parameter);
                if let Some(value) = value {
                    values.insert(parameter, value);
                }
            }
        }
    }

    /// Recomputes the value of every override of a parameter of `targets`
    /// that has a neutral expression it can evaluate, clears the value of
    /// every other one, and removes an override left with neither.
    fn recompute_overrides(&mut self, targets: &BTreeSet<ParameterId>) {
        let mut order: Vec<(usize, usize)> = Vec::new();
        for (at, configuration) in self.design.configurations.iter().enumerate() {
            let depth = self
                .design
                .configuration_chain(&configuration.id)
                .map_or(0, |chain| chain.len());
            order.push((depth, at));
        }
        order.sort_unstable();
        for (_, at) in order {
            let configuration = &self.design.configurations[at];
            let chain = self.chain_of(Some(&configuration.id));
            let updated =
                self.settle_overrides(&configuration.overrides, targets, &|target, entries| {
                    let (_, parents) = chain.as_deref()?.split_first()?;
                    let levels = stack(entries, &parameter_levels(parents));
                    self.cache_over_levels(target, &levels, Stated::of(chain.as_deref()?))
                });
            self.design.configurations[at].overrides.parameters = updated;
        }
        for at in 0..self.design.arrangements.len() {
            let arrangement = &self.design.arrangements[at];
            let chain = self.chain_of(arrangement.configuration.as_ref());
            let updated =
                self.settle_overrides(&arrangement.overrides, targets, &|target, entries| {
                    let chain = chain.as_deref()?;
                    let stated = Stated {
                        anything: true,
                        occurrences: Stated::of(chain).occurrences
                            || !arrangement.overrides.occurrences.is_empty(),
                    };
                    let levels = stack(entries, &parameter_levels(chain));
                    self.cache_over_levels(target, &levels, stated)
                });
            self.design.arrangements[at].overrides.parameters = updated;
        }
        for scope in 0..self.design.scopes.len() {
            let occurrences: Vec<_> = self.design.scopes[scope]
                .occurrences
                .keys()
                .cloned()
                .collect();
            for occurrence in occurrences {
                let overrides = Overrides {
                    parameters: self.design.scopes[scope].occurrences[&occurrence]
                        .parameters
                        .clone(),
                    ..Overrides::default()
                };
                let holder = &self.design.scopes[scope];
                let updated = self.settle_overrides(&overrides, targets, &|target, own| {
                    self.occurrence_cache(holder, &occurrence, target, own)
                });
                if let Some(design) = self.design.scopes[scope].occurrences.get_mut(&occurrence) {
                    design.parameters = updated;
                }
            }
        }
    }

    /// The parameter overrides of `overrides` after recomputing those of the
    /// parameters of `targets`. A reference reads `cache`, given the map as
    /// it stands: the map's own entry where it states the parameter, else the
    /// value the map's level resolves over (§5.13).
    fn settle_overrides(
        &self,
        overrides: &Overrides,
        targets: &BTreeSet<ParameterId>,
        cache: &dyn Fn(&ParameterId, &ParameterOverrides) -> Option<Value>,
    ) -> BTreeMap<ParameterId, ParameterOverride> {
        let mut map = overrides.parameters.clone();
        let mut pending: BTreeSet<ParameterId> = map
            .keys()
            .filter(|parameter| targets.contains(*parameter))
            .cloned()
            .collect();
        let kinds: BTreeMap<ParameterId, ValueKind> = pending
            .iter()
            .filter_map(|parameter| {
                Some((
                    parameter.clone(),
                    self.design_parameter(parameter).ok()?.kind.clone(),
                ))
            })
            .collect();
        loop {
            let mut done = Vec::new();
            for parameter in &pending {
                let Some(kind) = kinds.get(parameter) else {
                    done.push((parameter.clone(), None));
                    continue;
                };
                let Some(expression) = map
                    .get(parameter)
                    .and_then(|entry| entry.expression.as_ref())
                else {
                    done.push((parameter.clone(), None));
                    continue;
                };
                let value_of = |target: &ParameterId| cache(target, &map);
                if let Attempt::Done(value) = attempt(expression, kind, &pending, &value_of) {
                    done.push((parameter.clone(), value));
                }
            }
            if done.is_empty() {
                break;
            }
            for (parameter, value) in done {
                if let Some(entry) = map.get_mut(&parameter) {
                    entry.value = value;
                }
                pending.remove(&parameter);
            }
            map.retain(|_, entry| entry.expression.is_some() || entry.value.is_some());
        }
        for parameter in pending {
            if let Some(entry) = map.get_mut(&parameter) {
                entry.value = None;
            }
        }
        map.retain(|_, entry| entry.expression.is_some() || entry.value.is_some());
        map
    }
}

/// `own` above `below`, nearest first.
fn stack<'a>(
    own: &'a ParameterOverrides,
    below: &[&'a ParameterOverrides],
) -> Vec<&'a ParameterOverrides> {
    let mut levels = vec![own];
    levels.extend_from_slice(below);
    levels
}

/// The parameter overrides of a configuration chain, nearest first.
fn parameter_levels<'a>(chain: &[&'a DesignConfiguration]) -> Vec<&'a ParameterOverrides> {
    chain
        .iter()
        .map(|level| &level.overrides.parameters)
        .collect()
}

/// Clears the cached value of every measure whose anchored state depends on
/// any of `features`.
struct MeasureCheck<'a> {
    history: &'a History,
    currents: &'a Currents,
    features: &'a BTreeSet<FeatureId>,
}

impl MeasureCheck<'_> {
    fn clear(&self, json: &mut serde_json::Value, context: &Context) -> bool {
        let is_container = |id: &str| self.history.members(id).is_some();
        clear_measures(json, &mut |measure| {
            let states = reads(measure, &is_container).states;
            context.reads_depend(&states, self.history, self.currents, self.features)
        })
    }
}

fn parameter_context(parameter: &crate::design::parameter::DesignParameter) -> Context {
    Context {
        feature: match &parameter.owner {
            Some(ParameterOwner::Feature(feature)) => Some(Member::Feature(feature.clone())),
            _ => None,
        },
        scope: Some(parameter.scope.clone()),
    }
}
