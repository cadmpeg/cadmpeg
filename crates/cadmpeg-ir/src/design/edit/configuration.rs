// SPDX-License-Identifier: Apache-2.0
//! Changing a scope's active configuration (§4, §5.5).
//!
//! The current geometry holds each scope's active configuration. Changing it
//! exchanges, per current state of the scope, that state's extent with the
//! incoming configuration's evaluation at the same position, and keeps every
//! state the document holds that has a spelling. A current state at an
//! unstated rollback is read as built through its container's end; its
//! outgoing state has no spelling as an evaluation (part 10), so it is left
//! out and reported.

use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::states::{reached, remove, upsert, State};
use super::wire::{exchange_constructions, rewrite};
use super::{malformed, ConfigurationSwitch, EditError};
use crate::design::container::Container;
use crate::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use crate::design::ids::EvaluationId;
use crate::design::refs::{Evaluated, Member, Position};
use crate::document::Process;
use crate::features::ConfigurationId;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::process::state::{ProcessBase, ProcessContent};
use crate::CadIr;

/// What one current state exchanges.
struct Exchange {
    /// The current state's index in its scope.
    index: usize,
    /// The outgoing extent.
    outgoing: State,
    /// The incoming configuration's evaluation at the state's position, and
    /// its resolved state.
    incoming: Option<(EvaluationId, State)>,
    /// Whether the state is at an unstated rollback, where its outgoing state
    /// has no spelling as an evaluation.
    unspelled: bool,
}

/// The key part of an id: the text after its `#`.
fn key(id: &str) -> &str {
    id.split_once('#').map_or(id, |(_, key)| key)
}

/// The `<format>:<scope>:` prefix of an id.
fn namespace(id: &str) -> Option<String> {
    let (space, _) = id.split_once('#')?;
    let mut parts = space.splitn(3, ':');
    Some(format!("{}:{}:", parts.next()?, parts.next()?))
}

/// Rebases every process state based on the design state `from` onto `to`,
/// which holds the same resolved state.
fn rebase_process(process: &mut Process, from: &StateBase, to: &StateBase) {
    for state in &mut process.states {
        if let Some(ProcessContent {
            base: ProcessBase::Design(base),
            ..
        }) = &mut state.content
        {
            if base == from {
                base.clone_from(to);
            }
        }
    }
}

fn position_key(at: &Position) -> String {
    match at {
        Position::After(Member::Feature(feature)) => key(feature.as_str()).to_owned(),
        Position::After(Member::Container(container)) => key(container.as_str()).to_owned(),
        Position::Start(container) => format!("start.{}", key(container.as_str())),
    }
}

impl CadIr {
    /// Where `at` is `After(Container(c))` for an ordered container `c`
    /// whose rollback is unstated (§5.3): the position of the state after
    /// `c`'s last member, as which the editor reads it (built through its
    /// end), spelled through its nearest feature member, else `Start(c)`.
    fn unstated_end(&self, at: &Position) -> Option<Position> {
        let Position::After(Member::Container(id)) = at else {
            return None;
        };
        let Container::Ordered {
            members,
            rollback: None,
            ..
        } = self
            .design
            .containers
            .iter()
            .find(|container| container.id() == id)?
        else {
            return None;
        };
        Some(
            members
                .iter()
                .rev()
                .find(|member| matches!(member, Member::Feature(_)))
                .map_or_else(
                    || Position::Start(id.clone()),
                    |feature| Position::After(feature.clone()),
                ),
        )
    }

    /// A new evaluation id for the outgoing configuration's state at a
    /// position, in the configuration id's namespace.
    fn mint_evaluation(
        &self,
        configuration: &ConfigurationId,
        at: &Position,
    ) -> Result<EvaluationId, EditError> {
        let prefix = namespace(configuration.as_str()).ok_or_else(|| {
            malformed(format!("configuration id {configuration} has no namespace"))
        })?;
        let stem = format!(
            "{prefix}evaluation#{}/{}",
            key(configuration.as_str()),
            position_key(at)
        );
        let taken: BTreeSet<&str> = self
            .design
            .evaluations
            .iter()
            .map(|evaluation| evaluation.id.as_str())
            .collect();
        let mut candidate = stem.clone();
        let mut suffix = 0_u32;
        while taken.contains(candidate.as_str()) {
            suffix = suffix
                .checked_add(1)
                .ok_or_else(|| malformed("no evaluation id is free"))?;
            candidate = format!("{stem}.{suffix}");
        }
        EvaluationId::mint(candidate).map_err(malformed)
    }

    /// [`Editor::set_active_configuration`](super::Editor::set_active_configuration)
    /// on the document itself.
    pub(super) fn set_active_configuration(
        &mut self,
        scope: &ProductDefinitionId,
        configuration: &ConfigurationId,
    ) -> Result<ConfigurationSwitch, EditError> {
        let record = self
            .design
            .design_scope(scope)
            .ok_or_else(|| EditError::UnknownScope(scope.clone()))?;
        if !self
            .design
            .configurations
            .iter()
            .any(|candidate| &candidate.id == configuration && &candidate.scope == scope)
        {
            return Err(EditError::UnknownConfiguration(configuration.clone()));
        }
        let outgoing = record
            .active_configuration
            .clone()
            .ok_or_else(|| EditError::UnknownConfiguration(configuration.clone()))?;
        if &outgoing == configuration {
            return Ok(ConfigurationSwitch::default());
        }
        let mut exchanges = Vec::new();
        for (index, current) in record.current.iter().enumerate() {
            let end = self.unstated_end(&current.at);
            let incoming = self
                .design
                .evaluations
                .iter()
                .find(|evaluation| {
                    &evaluation.scope == scope
                        && evaluation.configuration.as_ref() == Some(configuration)
                        && (evaluation.of == Evaluated::State(current.at.clone())
                            || end
                                .as_ref()
                                .is_some_and(|end| evaluation.of == Evaluated::State(end.clone())))
                        && evaluation.content.is_some()
                })
                .map(|evaluation| evaluation.id.clone());
            let incoming = match incoming {
                Some(id) => {
                    let state = self.resolved(&id)?;
                    Some((id, state))
                }
                None => None,
            };
            exchanges.push(Exchange {
                index,
                outgoing: self.extent(scope, current)?,
                incoming,
                unspelled: end.is_some(),
            });
        }
        let leaving: BTreeSet<BodyId> = exchanges
            .iter()
            .filter(|exchange| exchange.incoming.is_some())
            .flat_map(|exchange| {
                exchange
                    .outgoing
                    .records
                    .bodies
                    .iter()
                    .map(|body| body.id.clone())
            })
            .collect();
        let staying: Vec<BodyId> = self
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .filter(|body| !leaving.contains(body))
            .collect();
        let kept = reached(&self.geometry, &staying)?;
        let scope_at = self
            .design
            .scopes
            .iter()
            .position(|candidate| &candidate.definition == scope)
            .ok_or_else(|| EditError::UnknownScope(scope.clone()))?;
        let mut switch = ConfigurationSwitch::default();
        for exchange in exchanges.iter().filter(|exchange| exchange.unspelled) {
            let at = self.design.scopes[scope_at].current[exchange.index]
                .at
                .clone();
            self.rebase_off_current(scope, &at)?;
            switch.unspelled.push(at);
        }
        for exchange in &exchanges {
            self.retire(scope, scope_at, &outgoing, exchange, &kept)?;
        }
        for exchange in exchanges {
            self.install(scope, scope_at, exchange);
        }
        self.exchange_scope_constructions(scope, &outgoing, configuration)?;
        self.design.scopes[scope_at].active_configuration = Some(configuration.clone());
        self.refresh_process_for_configuration(scope)?;
        self.geometry.finalize();
        self.design.finalize();
        Ok(switch)
    }

    /// Writes one current state's outgoing extent into an evaluation of the
    /// outgoing configuration, rebases the evaluations based on that state
    /// onto it, and, where an incoming state replaces it, removes from the
    /// geometry the outgoing records that no staying body reaches, with
    /// their names and owners. A state at an unstated rollback writes no
    /// evaluation: the caller has rebased what was based on it.
    fn retire(
        &mut self,
        scope: &ProductDefinitionId,
        scope_at: usize,
        outgoing: &ConfigurationId,
        exchange: &Exchange,
        kept: &BTreeSet<String>,
    ) -> Result<(), EditError> {
        let current = self.design.scopes[scope_at].current[exchange.index].clone();
        if !exchange.unspelled {
            self.write_outgoing(scope, outgoing, exchange, &current)?;
        }
        if exchange.incoming.is_none() {
            self.design.scopes[scope_at].current[exchange.index].status = EvaluationStatus::Stale;
            return Ok(());
        }
        let leaving: BTreeSet<String> = super::states::ids(&exchange.outgoing.records)
            .into_iter()
            .filter(|record| !kept.contains(record))
            .collect();
        remove(&mut self.geometry, &leaving);
        let mut names = self.design.names.entries().clone();
        names.retain(|entity, _| !leaving.contains(entity.id()));
        self.design.names = names.into();
        let outgoing_bodies: BTreeSet<&BodyId> = exchange
            .outgoing
            .records
            .bodies
            .iter()
            .map(|body| &body.id)
            .collect();
        for definition in &mut self.model.product_definitions {
            definition.bodies.retain(|body| {
                !(outgoing_bodies.contains(body) && leaving.contains(body.as_str()))
            });
            for entry in &mut definition.instance_bodies {
                entry.bodies.retain(|body| !leaving.contains(body.as_str()));
            }
            definition
                .instance_bodies
                .retain(|entry| !entry.bodies.is_empty());
        }
        Ok(())
    }

    /// Writes one current state's outgoing extent into an evaluation of the
    /// outgoing configuration at the state's position, and rebases the
    /// evaluations and process states based on that state onto it.
    fn write_outgoing(
        &mut self,
        scope: &ProductDefinitionId,
        outgoing: &ConfigurationId,
        exchange: &Exchange,
        current: &crate::design::evaluation::CurrentState,
    ) -> Result<(), EditError> {
        let id = self.mint_evaluation(outgoing, &current.at)?;
        let installed = exchange.incoming.is_some();
        let owners = exchange
            .outgoing
            .owners
            .iter()
            .filter(|_| installed)
            .map(|(body, owner)| (body.clone(), Some(owner.clone())))
            .collect();
        let written = Evaluation {
            id: id.clone(),
            scope: scope.clone(),
            configuration: Some(outgoing.clone()),
            of: Evaluated::State(current.at.clone()),
            status: current.status,
            content: Some(StateContent {
                base: StateBase::Empty,
                delta: StateDelta {
                    records: exchange.outgoing.records.clone(),
                    removed: Vec::new(),
                    names: exchange
                        .outgoing
                        .names
                        .iter()
                        .map(|(entity, entry)| (entity.clone(), Some(entry.clone())))
                        .collect(),
                    owners,
                    members: std::collections::BTreeMap::new(),
                    instance_bodies: exchange
                        .outgoing
                        .instance_bodies
                        .iter()
                        .map(|(path, bodies)| (path.clone(), Some(bodies.clone())))
                        .collect(),
                    poses: current
                        .poses
                        .iter()
                        .map(|(path, pose)| (path.clone(), Some(*pose)))
                        .collect(),
                },
            }),
            native_ref: current.native_ref.clone(),
        };
        let from_current = StateBase::Current(current.at.clone());
        for evaluation in &mut self.design.evaluations {
            if &evaluation.scope != scope {
                continue;
            }
            if let Some(content) = &mut evaluation.content {
                if content.base == from_current {
                    content.base = StateBase::Evaluation(id.clone());
                }
            }
        }
        rebase_process(
            &mut self.process,
            &from_current,
            &StateBase::Evaluation(id.clone()),
        );
        self.design.evaluations.push(written);
        Ok(())
    }

    /// Installs the incoming state of one current state: its records into
    /// the geometry, its names, its bodies' owners, and its poses; lists its
    /// bodies in the current state; removes its evaluation; and rebases the
    /// evaluations based on that evaluation onto the current state.
    fn install(&mut self, scope: &ProductDefinitionId, scope_at: usize, exchange: Exchange) {
        let Some((incoming, state)) = exchange.incoming else {
            return;
        };
        let from_current = StateBase::Current(
            self.design.scopes[scope_at].current[exchange.index]
                .at
                .clone(),
        );
        let bodies: Vec<BodyId> = state
            .records
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect();
        upsert(&mut self.geometry, state.records);
        let mut names = self.design.names.entries().clone();
        names.extend(state.names);
        self.design.names = names.into();
        let instance_bodies: BTreeSet<&BodyId> = state.instance_bodies.values().flatten().collect();
        for body in &bodies {
            if instance_bodies.contains(body) {
                continue;
            }
            let owner = state.owners.get(body).unwrap_or(scope);
            if let Some(definition) = self
                .model
                .product_definitions
                .iter_mut()
                .find(|definition| &definition.id == owner)
            {
                if !definition.bodies.contains(body) {
                    definition.bodies.push(body.clone());
                }
            }
        }
        for (path, incoming_bodies) in state.instance_bodies {
            let owner = path
                .0
                .first()
                .and_then(|step| match &step.occurrence {
                    crate::design::selection::StepRef::Occurrence(id) => {
                        state.members.get(id).cloned().or_else(|| {
                            crate::products::owner(&self.model.product_definitions, id)
                                .map(|owner| owner.id.clone())
                        })
                    }
                    crate::design::selection::StepRef::Native(_) => None,
                })
                .unwrap_or_else(|| scope.clone());
            if let Some(definition) = self
                .model
                .product_definitions
                .iter_mut()
                .find(|definition| definition.id == owner)
            {
                if let Some(entry) = definition
                    .instance_bodies
                    .iter_mut()
                    .find(|entry| entry.occurrence == path)
                {
                    for body in incoming_bodies {
                        if !entry.bodies.contains(&body) {
                            entry.bodies.push(body);
                        }
                    }
                } else {
                    definition
                        .instance_bodies
                        .push(crate::products::InstanceBodies {
                            occurrence: path,
                            bodies: incoming_bodies,
                        });
                }
            }
        }
        let status = self
            .design
            .evaluations
            .iter()
            .find(|evaluation| evaluation.id == incoming)
            .map_or(EvaluationStatus::Stale, |evaluation| evaluation.status);
        let native_ref = self
            .design
            .evaluations
            .iter()
            .find(|evaluation| evaluation.id == incoming)
            .and_then(|evaluation| evaluation.native_ref.clone());
        self.design
            .evaluations
            .retain(|evaluation| evaluation.id != incoming);
        let to_current = StateBase::Evaluation(incoming);
        for evaluation in &mut self.design.evaluations {
            if let Some(content) = &mut evaluation.content {
                if content.base == to_current {
                    content.base = from_current.clone();
                }
            }
        }
        rebase_process(&mut self.process, &to_current, &from_current);
        let target = &mut self.design.scopes[scope_at].current[exchange.index];
        target.bodies = bodies;
        target.poses = state.poses;
        target.status = status;
        target.native_ref = native_ref;
    }

    /// Exchanges the cached `resolved` value of every construction of a
    /// scope's design: its features, containers, sketches, sheet-metal
    /// rules, and the overrides of its configurations and arrangements.
    fn exchange_scope_constructions(
        &mut self,
        scope: &ProductDefinitionId,
        outgoing: &ConfigurationId,
        incoming: &ConfigurationId,
    ) -> Result<(), EditError> {
        fn all<'a, T: Serialize + DeserializeOwned + 'a>(
            records: impl IntoIterator<Item = &'a mut T>,
            outgoing: &str,
            incoming: &str,
        ) -> Result<(), EditError> {
            for record in records {
                rewrite(record, |json| {
                    Ok(exchange_constructions(json, outgoing, incoming))
                })?;
            }
            Ok(())
        }
        let history = super::history::History::new(&self.design);
        let in_scope = |member: Member| history.scope_of(&member) == Some(scope);
        let features: BTreeSet<_> = self
            .design
            .features
            .iter()
            .filter(|feature| in_scope(Member::Feature(feature.id.clone())))
            .map(|feature| feature.id.clone())
            .collect();
        let containers: BTreeSet<_> = self
            .design
            .containers
            .iter()
            .filter(|container| in_scope(Member::Container(container.id().clone())))
            .map(|container| container.id().clone())
            .collect();
        let sketches: BTreeSet<String> = self
            .design
            .sketches
            .iter()
            .filter(|sketch| features.contains(&sketch.owner))
            .map(|sketch| sketch.id.as_str().to_owned())
            .chain(
                self.design
                    .spatial_sketches
                    .iter()
                    .filter(|sketch| features.contains(&sketch.owner))
                    .map(|sketch| sketch.id.as_str().to_owned()),
            )
            .collect();
        let (outgoing, incoming) = (outgoing.as_str(), incoming.as_str());
        let design = &mut self.design;
        all(
            design
                .features
                .iter_mut()
                .filter(|feature| features.contains(&feature.id)),
            outgoing,
            incoming,
        )?;
        all(
            design
                .containers
                .iter_mut()
                .filter(|container| containers.contains(container.id())),
            outgoing,
            incoming,
        )?;
        all(
            design
                .sketches
                .iter_mut()
                .filter(|sketch| sketches.contains(sketch.id.as_str())),
            outgoing,
            incoming,
        )?;
        all(
            design
                .sketch_entities
                .iter_mut()
                .filter(|entity| sketches.contains(entity.sketch.as_str())),
            outgoing,
            incoming,
        )?;
        all(
            design
                .sketch_constraints
                .iter_mut()
                .filter(|constraint| sketches.contains(constraint.sketch.as_str())),
            outgoing,
            incoming,
        )?;
        all(
            design
                .spatial_sketches
                .iter_mut()
                .filter(|sketch| sketches.contains(sketch.id.as_str())),
            outgoing,
            incoming,
        )?;
        all(
            design
                .spatial_sketch_entities
                .iter_mut()
                .filter(|entity| sketches.contains(entity.sketch.as_str())),
            outgoing,
            incoming,
        )?;
        all(
            design
                .spatial_sketch_constraints
                .iter_mut()
                .filter(|constraint| sketches.contains(constraint.sketch.as_str())),
            outgoing,
            incoming,
        )?;
        all(
            design
                .sheet_metal_rules
                .iter_mut()
                .filter(|rule| &rule.scope == scope),
            outgoing,
            incoming,
        )?;
        all(
            design
                .configurations
                .iter_mut()
                .filter(|configuration| &configuration.scope == scope)
                .map(|configuration| &mut configuration.overrides),
            outgoing,
            incoming,
        )?;
        all(
            design
                .arrangements
                .iter_mut()
                .filter(|arrangement| &arrangement.scope == scope),
            outgoing,
            incoming,
        )
    }
}
