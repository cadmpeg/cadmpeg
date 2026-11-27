// SPDX-License-Identifier: Apache-2.0
//! Positions, evaluations, and current geometry (§5.3, §5.4, §5.5, §5.10).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

use super::parameters;
use super::records::{DesignRecords, MemberKey, OfKey, PositionKey};
use super::selections::check_path;
use crate::design::container::Container;
use crate::design::dimension::Dimension;
use crate::design::evaluation::{EvaluationStatus, StateBase};
use crate::design::feature::{Feature, FeatureOperation};
use crate::design::refs::Position;
use crate::design::selection::OccurrencePath;
use crate::features::ConfigurationId;
use crate::products::{ProductDefinition, Storage};
use crate::report::check::Finding;
use crate::tessellation::Meshed;

/// A position names an existing member of a container, an ordered container
/// that is a root of its scope, or the start of an existing ordered
/// container (§5.3). Returns whether it does.
pub(in crate::validate) fn check_position(
    index: &DesignRecords<'_>,
    position: PositionKey<'_>,
    entity: &str,
    findings: &mut Vec<Finding>,
) -> bool {
    match position {
        PositionKey::Start(id) => {
            let ordered = matches!(index.containers.get(id), Some(Container::Ordered { .. }));
            if !ordered {
                super::error(
                    findings,
                    entity,
                    crate::rule!(
                        "design.names-no-ordered-container-design",
                        "{position} names no ordered container of the design"
                    ),
                );
            }
            ordered
        }
        PositionKey::After(MemberKey::Container(id))
            if matches!(index.containers.get(id), Some(Container::Unordered { .. })) =>
        {
            super::error(
                findings,
                entity,
                crate::rule!("design.position-after-unordered-container",
                    "{position} names unordered container {id}, whose members have their own results; it names no state"
                ),
            );
            false
        }
        PositionKey::After(member) => {
            let ordered_root = matches!(
                member,
                MemberKey::Container(id)
                    if index.roots.contains_key(id)
                        && matches!(index.containers.get(id), Some(Container::Ordered { .. }))
            );
            let listed = ordered_root
                || (index.member_exists(member) && index.parents.contains_key(&member));
            if !listed {
                super::error(
                    findings,
                    entity,
                    crate::rule!(
                        "design.names-no-member-container-no-ordered",
                        "{position} names no member of a container and no ordered root container"
                    ),
                );
            }
            listed
        }
    }
}

/// The feature `position` is after, where it is a feature member of an
/// unordered container: `After(m)` is then `m`'s own result (§5.3).
fn unordered_feature<'a>(
    index: &DesignRecords<'a>,
    position: PositionKey<'a>,
) -> Option<&'a Feature> {
    let PositionKey::After(MemberKey::Feature(id)) = position else {
        return None;
    };
    match index.container_of(position) {
        Some(Container::Unordered { .. }) => index.features.get(id).copied(),
        _ => None,
    }
}

/// Whether a feature member of an unordered container has a result of its
/// own in `configuration` (§5.3): it has none where it is stateless there or
/// its operation there places or connects occurrences. `None` where that
/// operation is `Native`, which states nothing to decide it.
fn has_own_result(
    index: &DesignRecords<'_>,
    feature: &Feature,
    configuration: Option<&str>,
) -> Option<bool> {
    let operation = &index.definition_in(feature, configuration, None).operation;
    if matches!(operation, FeatureOperation::Native { .. }) {
        return None;
    }
    if is_stateless(index, feature, configuration) {
        return Some(false);
    }
    Some(!operation.places_occurrences())
}

/// A `Pinned` anchor or a `StateBase::Current` is at no `After(m)` for a
/// feature `m` of an unordered container that has no result of its own in
/// any configuration of its scope (§5.3). Returns whether it is not.
pub(in crate::validate) fn check_resulting_position(
    index: &DesignRecords<'_>,
    position: PositionKey<'_>,
    entity: &str,
    findings: &mut Vec<Finding>,
) -> bool {
    let Some(feature) = unordered_feature(index, position) else {
        return true;
    };
    let id = feature.id.as_str();
    let configurations: Vec<Option<&str>> = match index
        .scope_of(MemberKey::Feature(id))
        .and_then(|scope| index.scope_configurations.get(scope))
    {
        Some(configurations) => configurations
            .iter()
            .map(|configuration| Some(configuration.id.as_str()))
            .collect(),
        None => vec![None],
    };
    let resultless = configurations
        .into_iter()
        .all(|configuration| has_own_result(index, feature, configuration) == Some(false));
    if resultless {
        super::error(
            findings,
            entity,
            crate::rule!("design.position-after-member-without-result",
                "{position} is after {id}, a member of an unordered container with no result of its own in any configuration of its scope; it names no state"
            ),
        );
    }
    !resultless
}

/// Whether `position` is `After(Container(c))` for an ordered container `c`.
fn after_ordered_container(index: &DesignRecords<'_>, position: PositionKey<'_>) -> bool {
    matches!(
        position,
        PositionKey::After(MemberKey::Container(id))
            if matches!(index.containers.get(id), Some(Container::Ordered { .. }))
    )
}

/// The positions of a scope's current states.
fn current_positions<'a>(index: &DesignRecords<'a>, scope: &str) -> BTreeSet<PositionKey<'a>> {
    index
        .scopes
        .get(scope)
        .map(|scope| {
            scope
                .current
                .iter()
                .map(|current| PositionKey::of(&current.at))
                .collect()
        })
        .unwrap_or_default()
}

/// The evaluation rules. `max_chain_depth` is the caller's ceiling on a chain
/// of state bases, `ResourceLimits::max_state_chain_depth`; a longer chain
/// refuses the session.
pub(super) fn check_evaluations(
    index: &DesignRecords<'_>,
    max_chain_depth: u64,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let StateChains { cyclic, depths } = state_chains(index).unwrap_or_else(|id| {
        super::error(
            findings,
            id,
            crate::rule!(
                "design.depth-evaluation-chain-state-bases-overflows",
                "the depth of the evaluation's chain of state bases overflows"
            ),
        );
        StateChains::default()
    });
    if let Some(refusal) = depths
        .values()
        .max()
        .and_then(|&deepest| crate::validate::state_chain_refusal(max_chain_depth, deepest))
    {
        return Err(CodecError::ResourceLimit(refusal));
    }
    let mut keys = BTreeMap::new();
    for evaluation in &index.ir.design.evaluations {
        let id = evaluation.id.as_str();
        let scope = evaluation.scope.as_str();
        let design_scope = index.scopes.get(scope).copied();
        if design_scope.is_none() {
            super::error(
                findings,
                id,
                crate::rule!(
                    "design.evaluation-scope-no-design-scope",
                    "evaluation's scope {scope} has no design scope"
                ),
            );
        }
        let configuration = evaluation
            .configuration
            .as_ref()
            .map(ConfigurationId::as_str);
        match (configuration, index.has_configurations(scope)) {
            (None, true) => super::error(
                findings,
                id,
                crate::rule!(
                    "design.evaluation-states-no-configuration-scope-configurations",
                    "evaluation states no configuration, but its scope has configurations"
                ),
            ),
            (Some(configuration), false) => super::error(
                findings,
                id,
                crate::rule!(
                    "design.evaluation-names-configuration-scope-none",
                    "evaluation names configuration {configuration}, but its scope has none"
                ),
            ),
            (Some(configuration), true) => {
                let of_scope = index
                    .configurations
                    .get(configuration)
                    .is_some_and(|c| c.scope.as_str() == scope);
                if !of_scope {
                    super::error(
                        findings,
                        id,
                        crate::rule!("design.evaluation-names-no-configuration-scope", "evaluation names {configuration}, which is no configuration of its scope"),
                    );
                }
            }
            (None, false) => {}
        }
        let of = OfKey::of(&evaluation.of);
        let feature = match of {
            OfKey::State(position) => {
                check_position(index, position, id, findings);
                if after_ordered_container(index, position) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("design.evaluation-evaluates-state-ordered-container-rollback", "evaluation evaluates {position}, which is the state at that ordered container's rollback position"),
                    );
                }
                let active = design_scope.is_some_and(|s| {
                    s.active_configuration.as_ref().map(ConfigurationId::as_str) == configuration
                });
                if active && current_positions(index, scope).contains(&position) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("design.evaluation-active-configuration-repeats-current-state", "evaluation of the active configuration repeats the current state at {position}"),
                    );
                }
                match position {
                    PositionKey::After(MemberKey::Feature(feature)) => Some(feature),
                    PositionKey::After(MemberKey::Container(_)) | PositionKey::Start(_) => None,
                }
            }
            OfKey::Tool(feature) => {
                if !index.features.contains_key(feature) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("design.evaluation-evaluates-tool-body-no-feature", "evaluation evaluates the tool body of {feature}, which is no feature of the design"),
                    );
                }
                Some(feature)
            }
        };
        if let OfKey::State(position) = of {
            let resultless = unordered_feature(index, position).filter(|feature| {
                !is_stateless(index, feature, configuration)
                    && has_own_result(index, feature, configuration) == Some(false)
            });
            if let Some(member) = resultless {
                super::error(
                    findings,
                    id,
                    crate::rule!("design.evaluation-after-member-without-result",
                        "evaluation evaluates {position}, but {} has no result of its own in the evaluation's configuration",
                        member.id.as_str()
                    ),
                );
            }
        }
        if let Some(feature) = feature.and_then(|feature| index.features.get(feature)) {
            if is_stateless(index, feature, configuration) {
                super::error(
                    findings,
                    id,
                    crate::rule!("design.feature-stateless-evaluation-configuration-no-evaluation", 
                        "feature {} is stateless in the evaluation's configuration and has no evaluation there",
                        feature.id.as_str()
                    ),
                );
            } else if matches!(of, OfKey::Tool(_))
                && super::ops::builds_tool_body(
                    &index.definition_in(feature, configuration, None).operation,
                ) == Some(false)
            {
                super::error(
                    findings,
                    id,
                    crate::rule!("design.evaluation-evaluates-tool-body-operation-evaluation", 
                        "evaluation evaluates the tool body of {}, whose operation in the evaluation's configuration builds none",
                        feature.id.as_str()
                    ),
                );
            }
        }
        let key = (scope, configuration, of);
        if let Some(first) = keys.insert(key, id) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "design.evaluation-repeats-scope-configuration-evaluation",
                    "evaluation repeats the scope, configuration, and `of` of evaluation {first}"
                ),
            );
        }
        let Some(content) = &evaluation.content else {
            if evaluation.status == EvaluationStatus::Current {
                super::error(
                    findings,
                    id,
                    crate::rule!("design.evaluation-no-content-status-current-stale", "evaluation with no content has status `current`; it is `stale` or `failed`"),
                );
            }
            continue;
        };
        match &content.base {
            StateBase::Empty => {}
            StateBase::Current(position) => {
                let position = PositionKey::of(position);
                if check_position(index, position, id, findings)
                    && check_resulting_position(index, position, id, findings)
                {
                    if matches!(position, PositionKey::After(MemberKey::Container(_)))
                        && !index.at_unstated_rollback(position)
                    {
                        super::error(
                            findings,
                            id,
                            crate::rule!("design.state-base-names-stated-rollback-position", "state base names {position}; a stated rollback position is spelled through its member"),
                        );
                    }
                    if !current_positions(index, scope).contains(&position) {
                        super::error(
                            findings,
                            id,
                            crate::rule!("design.state-base-names-position-no-current", "state base names {position}, which is the position of no current state of the evaluation's scope"),
                        );
                    }
                }
            }
            StateBase::Evaluation(base) => match index.evaluations.get(base.as_str()) {
                None if index.ir.fragment => super::error(
                    findings,
                    id,
                    crate::rule!(
                        "design.fragment-does-not-hold-evaluation-base",
                        "the fragment does not hold the evaluation's base {}",
                        base.as_str()
                    ),
                ),
                None => super::error(
                    findings,
                    id,
                    crate::rule!(
                        "design.state-base-no-evaluation-design",
                        "state base {} is no evaluation of the design",
                        base.as_str()
                    ),
                ),
                Some(base) if base.content.is_none() => super::error(
                    findings,
                    id,
                    crate::rule!(
                        "design.state-base-evaluation-no-content",
                        "state base {} is an evaluation with no content",
                        base.id.as_str()
                    ),
                ),
                Some(_) => {}
            },
        }
        if cyclic.contains(id) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "design.evaluation-own-ancestor-through-state-bases",
                    "evaluation is its own ancestor through its state bases; bases form a forest"
                ),
            );
        }
    }
    Ok(())
}

/// The chains of state bases: the evaluations on a cycle, and the depth of
/// every other evaluation as a reader counts it. An evaluation without
/// content has depth zero; one with content has depth one more than the
/// evaluation its base names, or one where its base is another arm or names
/// no evaluation. The walk is iterative.
#[derive(Default)]
pub(in crate::validate) struct StateChains<'a> {
    pub(in crate::validate) cyclic: BTreeSet<&'a str>,
    pub(in crate::validate) depths: BTreeMap<&'a str, u64>,
}

/// Walks every chain of state bases. Returns the evaluation whose depth
/// overflows, if one does.
pub(in crate::validate) fn state_chains<'a>(
    index: &DesignRecords<'a>,
) -> Result<StateChains<'a>, &'a str> {
    #[derive(Clone, Copy)]
    enum Mark {
        OnChain,
        Depth(u64),
    }
    enum Link<'a> {
        Unstored,
        Base(&'a str),
        Below,
    }
    let link = |id: &str| -> Link<'a> {
        let Some(content) = index
            .evaluations
            .get(id)
            .and_then(|evaluation| evaluation.content.as_ref())
        else {
            return Link::Unstored;
        };
        match &content.base {
            StateBase::Evaluation(base) => index
                .evaluations
                .get_key_value(base.as_str())
                .map_or(Link::Below, |(key, _)| Link::Base(key)),
            StateBase::Empty | StateBase::Current(_) => Link::Below,
        }
    };
    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();
    let mut cyclic = BTreeSet::new();
    for &start in index.evaluations.keys() {
        let mut chain = Vec::new();
        let mut at = start;
        let tail = loop {
            match marks.get(at) {
                Some(Mark::Depth(depth)) => break Some(*depth),
                Some(Mark::OnChain) => {
                    let from = chain
                        .iter()
                        .position(|listed| *listed == at)
                        .unwrap_or_default();
                    cyclic.extend(chain[from..].iter().copied());
                    break None;
                }
                None => {}
            }
            match link(at) {
                Link::Unstored => {
                    marks.insert(at, Mark::Depth(0));
                    break Some(0);
                }
                Link::Below => {
                    chain.push(at);
                    break Some(0);
                }
                Link::Base(base) => {
                    marks.insert(at, Mark::OnChain);
                    chain.push(at);
                    at = base;
                }
            }
        };
        let mut depth = tail.unwrap_or_default();
        for id in chain.into_iter().rev() {
            depth = depth.checked_add(1).ok_or(id)?;
            marks.insert(id, Mark::Depth(if tail.is_some() { depth } else { 0 }));
        }
    }
    let depths = marks
        .into_iter()
        .filter_map(|(id, mark)| match mark {
            Mark::Depth(depth) if !cyclic.contains(id) => Some((id, depth)),
            Mark::Depth(_) | Mark::OnChain => None,
        })
        .collect();
    Ok(StateChains { cyclic, depths })
}

/// `CadIr::is_stateless(f, c)` (§5.4, D34, D35), decided where the document
/// states what decides it: the feature's `suppressed` resolves to true in
/// `configuration`, or the operation of the definition that applies there
/// cannot change a state. A suppression bound to a parameter resolves through
/// the parameter's value in `configuration`.
pub(super) fn is_stateless(
    index: &DesignRecords<'_>,
    feature: &Feature,
    configuration: Option<&str>,
) -> bool {
    let chain = index.configuration_chain(configuration);
    let suppressed = chain
        .iter()
        .find_map(|configuration| {
            configuration
                .overrides
                .features
                .get(&feature.id)?
                .suppressed
        })
        .or_else(|| match feature.suppressed.as_ref()? {
            Dimension::Literal(value) => Some(*value),
            Dimension::Parameter(parameter) => parameters::boolean_in(index, &chain, parameter),
            Dimension::Negated(parameter) => {
                parameters::boolean_in(index, &chain, parameter).map(|value| !value)
            }
        });
    if suppressed == Some(true) {
        return true;
    }
    index
        .definition_in(feature, configuration, None)
        .operation
        .cannot_change_state()
}

/// The bodies a definition lists in `bodies` and in its `instance_bodies`.
fn listed_bodies(definition: &ProductDefinition) -> impl Iterator<Item = &crate::ids::BodyId> {
    definition.bodies.iter().chain(
        definition
            .instance_bodies
            .iter()
            .flat_map(|entry| &entry.bodies),
    )
}

pub(super) fn check_current_geometry(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let ir = index.ir;
    let moved = super::selections::moved_occurrences(ir);
    let bodies: BTreeSet<&str> = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    let mut listed: BTreeMap<&str, usize> = BTreeMap::new();
    for scope in &ir.design.scopes {
        let entity = scope.definition.as_str();
        let spanned = index.spanned_definitions(entity);
        let spanned: Vec<&ProductDefinition> = spanned
            .iter()
            .filter_map(|definition| index.definitions.get(definition).copied())
            .collect();
        let owned: BTreeSet<&str> = spanned
            .iter()
            .flat_map(|definition| listed_bodies(definition))
            .map(crate::ids::BodyId::as_str)
            .collect();
        // §5.1 stops the span at a definition stored as a file.
        let required: BTreeSet<&str> = spanned
            .iter()
            .filter(|definition| {
                definition.id.as_str() == entity || matches!(definition.storage, Storage::Embedded)
            })
            .flat_map(|definition| listed_bodies(definition))
            .map(crate::ids::BodyId::as_str)
            .collect();
        let mut posed: BTreeMap<&OccurrencePath, usize> = BTreeMap::new();
        let active = scope
            .active_configuration
            .as_ref()
            .map(ConfigurationId::as_str);
        for current in &scope.current {
            check_current_position(index, &current.at, entity, active, findings);
            for body in &current.bodies {
                *listed.entry(body.as_str()).or_default() += 1;
                if !bodies.contains(body.as_str()) {
                    super::error(
                        findings,
                        entity,
                        crate::rule!(
                            "design.current-state-lists-no-body-current",
                            "current state lists {}, which is no body of the current geometry",
                            body.as_str()
                        ),
                    );
                }
            }
            for path in current.poses.keys() {
                *posed.entry(path).or_default() += 1;
                check_path(index, Some(entity), path, &moved, entity, findings);
            }
        }
        for body in &scope.unproduced {
            *listed.entry(body.as_str()).or_default() += 1;
            if !bodies.contains(body.as_str()) {
                super::error(
                    findings,
                    entity,
                    crate::rule!(
                        "design.unproduced-entry-must-body-current-geometry",
                        "unproduced entry {} must be a body of the current geometry",
                        body.as_str()
                    ),
                );
            }
            if !owned.contains(body.as_str()) {
                super::error(
                    findings,
                    entity,
                    crate::rule!(
                        "design.unproduced-body-must-owned-definition-scope",
                        "unproduced body {} must be owned by a definition the scope spans",
                        body.as_str()
                    ),
                );
            }
        }
        let here: BTreeSet<&str> = scope
            .current
            .iter()
            .flat_map(|current| &current.bodies)
            .chain(&scope.unproduced)
            .map(crate::ids::BodyId::as_str)
            .collect();
        for body in required.difference(&here) {
            super::error(
                findings,
                body,
                crate::rule!("design.body-definition-scope-spans-belongs-no", "body of a definition scope {entity} spans belongs to no current state or unproduced set of that scope"),
            );
        }
        for count in posed.into_values() {
            if count > 1 {
                super::error(
                    findings,
                    entity,
                    crate::rule!("design.instance-path-pose-current-states-scope", "an instance path has a pose in {count} current states of the scope; it has one in at most one"),
                );
            }
        }
    }
    for (body, count) in &listed {
        if *count > 1 {
            super::error(
                findings,
                body,
                crate::rule!("design.body-belongs-current-states-unproduced-sets", "body belongs to {count} current states or unproduced sets; it belongs to exactly one"),
            );
        }
    }
    check_stored_features(index, findings);
}

/// A current state names an existing position, and one inside an ordered
/// container is at that container's rollback position: spelled through its
/// member where the rollback is stated, and as `After(Container(c))` where
/// it is not (§5.3). In an unordered container it is after a member that
/// has a result of its own in the active configuration.
fn check_current_position(
    index: &DesignRecords<'_>,
    at: &Position,
    entity: &str,
    active: Option<&str>,
    findings: &mut Vec<Finding>,
) {
    let at = PositionKey::of(at);
    if !check_position(index, at, entity, findings) {
        return;
    }
    if let Some(member) = unordered_feature(index, at) {
        if has_own_result(index, member, active) == Some(false) {
            super::error(
                findings,
                entity,
                crate::rule!("design.current-state-after-member-without-result",
                    "current state is at {at}, but {} has no result of its own in the active configuration",
                    member.id.as_str()
                ),
            );
        }
    }
    if matches!(at, PositionKey::After(MemberKey::Container(_))) {
        if !index.at_unstated_rollback(at) {
            super::error(
                findings,
                entity,
                crate::rule!("design.current-state-stated-rollback-position-spelled", 
                    "current state is at {at}; a stated rollback position is spelled through its member"
                ),
            );
        }
        return;
    }
    let Some(container) = index.container_of(at) else {
        return;
    };
    if let Some(rollback) = DesignRecords::rollback_position(container) {
        if rollback != at {
            super::error(
                findings,
                entity,
                crate::rule!("design.current-state-not-rollback-position-container", 
                    "current state is at {at}, not at the rollback position of container {}, {rollback}",
                    container.id().as_str()
                ),
            );
        }
    }
}

/// A stored-geometry definition's tessellations exist, and a current state
/// after a feature whose definition in its scope's active configuration is
/// stored holds each body that definition names (§5.10).
fn check_stored_features(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let ir = index.ir;
    let tessellations: BTreeMap<&str, &Meshed> = ir
        .model
        .tessellations
        .iter()
        .map(|tessellation| (tessellation.id.as_str(), &tessellation.of))
        .collect();
    for (holder, feature, definition) in index.feature_definitions() {
        let FeatureOperation::Stored {
            tessellations: named,
            ..
        } = &definition.operation
        else {
            continue;
        };
        let spanned = index
            .scope_of(MemberKey::Feature(feature))
            .map(|scope| index.spanned_definitions(scope));
        for tessellation in named {
            let id = tessellation.as_str();
            match tessellations.get(id) {
                None => super::error(
                    findings,
                    holder,
                    crate::rule!(
                        "design.stored-feature-names-no-tessellation-model",
                        "stored feature names {id}, which is no tessellation of the model"
                    ),
                ),
                Some(Meshed::Definition { definition, .. }) => {
                    if spanned
                        .as_ref()
                        .is_some_and(|spanned| !spanned.contains(definition.as_str()))
                    {
                        super::error(
                            findings,
                            holder,
                            crate::rule!(
                                "design.stored-feature-tessellation-definition-not-spanned",
                                "stored feature names {id}, a mesh of {}, which the feature's scope does not span",
                                definition.as_str()
                            ),
                        );
                    }
                }
                Some(
                    Meshed::Body { .. }
                    | Meshed::Display { .. }
                    | Meshed::Pmi { .. }
                    | Meshed::Unresolved,
                ) => super::error(
                    findings,
                    holder,
                    crate::rule!(
                        "design.stored-feature-tessellation-not-definition-mesh",
                        "stored feature names {id}, whose `of` is not `Definition`"
                    ),
                ),
            }
        }
    }
    for feature in &ir.design.features {
        let id = feature.id.as_str();
        let after = PositionKey::After(MemberKey::Feature(id));
        let Some(scope) = index
            .scope_of(MemberKey::Feature(id))
            .and_then(|scope| index.scopes.get(scope))
        else {
            continue;
        };
        let active = scope
            .active_configuration
            .as_ref()
            .map(ConfigurationId::as_str);
        let FeatureOperation::Stored { bodies, .. } =
            &index.definition_in(feature, active, None).operation
        else {
            continue;
        };
        for current in scope
            .current
            .iter()
            .filter(|current| PositionKey::of(&current.at) == after)
        {
            for body in bodies.iter().flatten() {
                if !current.bodies.contains(body) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("design.stored-feature-names-no-body-current", "stored feature names {}, which is no body of the current state after it", body.as_str()),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
