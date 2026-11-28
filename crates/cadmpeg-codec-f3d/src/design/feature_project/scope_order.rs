// SPDX-License-Identifier: Apache-2.0
//! Authored scope ordinals, the scope history graph and parameter-scope order.

use crate::design::feature_project::reverse_normal;
use crate::design::feature_project::thread_companions;

use crate::design::design_feature_family;
use crate::design::feature_project::ComponentHistoryNamespace;
use crate::design::feature_project::PARAMETER_ORDER_WORK;
use crate::ids;
use crate::ids::native_stream;
use crate::records::bodies::DesignBodyBinding;
use crate::records::entity_header::DesignFeatureTimeline;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use cadmpeg_core::CodecError;
use std::collections::HashMap;
use std::collections::HashSet;

/// Authored construction ordinal of every parameter scope represented by a
/// neutral top-level feature. All input scopes must share one Design stream.
pub(super) fn authored_scope_ordinals<'a>(
    scopes: &'a [DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let Some(first_scope) = scopes.first() else {
        return Ok(HashMap::new());
    };
    let stream = native_stream(&first_scope.id).unwrap_or(ids::DEFAULT_STREAM);
    if scopes
        .iter()
        .any(|scope| native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM) != stream)
    {
        return Err(CodecError::NotImplemented(
            "independent Design scope streams have no shared authored timeline order".into(),
        ));
    }
    let mut stream_scopes = Vec::new();
    for scope in scopes {
        stream_scopes.push(scope);
    }
    authored_scope_ordinals_for_stream(&stream_scopes, timelines)
}

/// Authored scope ordinals evaluated independently for every Design stream.
pub(crate) fn authored_scope_ordinals_per_stream<'a>(
    scopes: &'a [DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let mut streams = HashMap::<&str, Vec<&DesignParameterScope>>::new();
    for scope in scopes {
        let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
        if !streams.contains_key(stream) {
            streams.insert(stream, Vec::new());
        }
        streams
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("authored stream index lost its key"))?
            .push(scope);
    }
    let mut out = HashMap::new();
    for stream_scopes in streams.into_values() {
        for (key, ordinal) in authored_scope_ordinals_for_stream(&stream_scopes, timelines)? {
            if out.insert(key, ordinal).is_some() {
                return Err(CodecError::Malformed(
                    "Design scope record identity is not unique".into(),
                ));
            }
        }
    }
    Ok(out)
}

pub(crate) fn authored_scope_ordinals_for_stream<'a>(
    scopes: &[&'a DesignParameterScope],
    timelines: &[DesignFeatureTimeline],
) -> Result<HashMap<(&'a str, u32), u64>, CodecError> {
    let mut out = HashMap::new();
    let Some(first_scope) = scopes.first().copied() else {
        return Ok(out);
    };
    let stream = native_stream(&first_scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let mut scopes_by_record = HashMap::<u32, &DesignParameterScope>::new();
    for scope in scopes {
        if scopes_by_record
            .insert(scope.record_index, *scope)
            .is_some()
        {
            return Err(CodecError::Malformed(
                "Design scope record identity is not unique".into(),
            ));
        }
    }
    for scope in scopes {
        let Some(target_record_index) = scope
            .assembly_alignment()
            .and_then(crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index)
        else {
            continue;
        };
        let Some(target) = scopes_by_record.get(&target_record_index) else {
            return Err(CodecError::Malformed(
                "Design assembly datum envelope has no JointOrigin target".into(),
            ));
        };
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Assemble
            || target.kind() != crate::records::feature::scope::DesignFeatureKind::JointOrigin
            || target.joint_origin_transform().is_none()
        {
            return Err(CodecError::Malformed(
                "Design assembly datum envelope has an invalid JointOrigin target".into(),
            ));
        }
    }

    let mut stream_timelines = Vec::new();
    for timeline in timelines
        .iter()
        .filter(|timeline| native_stream(timeline.id()).unwrap_or(ids::DEFAULT_STREAM) == stream)
    {
        stream_timelines.push(timeline);
    }
    stream_timelines.sort_by_key(|left| left.source_ordinal);
    if stream_timelines.is_empty() {
        let first_family = design_feature_family(&first_scope.kind());
        let homogeneous = scopes.iter().all(|scope| {
            first_family.map_or_else(
                || scope.kind() == first_scope.kind(),
                |family| design_feature_family(&scope.kind()) == Some(family),
            )
        });
        let mut ordered = Vec::new();
        for &scope in scopes {
            ordered.push(scope);
        }
        ordered.sort_by_key(|left| left.feature_ordinal);
        let complete_ordinals = ordered.iter().enumerate().all(|(ordinal, scope)| {
            u32::try_from(ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_add(1))
                == Some(scope.feature_ordinal.get())
        });
        if !homogeneous || !complete_ordinals {
            return Err(CodecError::NotImplemented(
                "Design scopes have no complete authored timeline order".into(),
            ));
        }
        for (ordinal, scope) in ordered.into_iter().enumerate() {
            let ordinal = u64::try_from(ordinal)
                .map_err(|_| CodecError::Malformed("Design feature ordinal exceeds u64".into()))?;
            if out
                .insert(
                    (
                        native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM),
                        scope.record_index,
                    ),
                    ordinal,
                )
                .is_some()
            {
                return Err(CodecError::Malformed(
                    "Design scope record identity is not unique".into(),
                ));
            }
        }
        return Ok(out);
    }

    if !stream_timelines
        .iter()
        .enumerate()
        .all(|(ordinal, timeline)| u32::try_from(ordinal).ok() == Some(timeline.source_ordinal))
    {
        return Err(CodecError::Malformed(
            "Design timeline-record ordinals are not contiguous".into(),
        ));
    }
    if stream_timelines
        .iter()
        .filter(|timeline| !timeline.frame().items().is_empty())
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(
            "multiple nonempty Design timelines have no shared authored order".into(),
        ));
    }
    let mut items = Vec::new();
    for timeline in stream_timelines {
        items.extend(timeline.frame().items().iter().map(|item| item.value));
    }
    let item_ordinals = timeline_item_ordinals(scopes, &scopes_by_record, &items)?;
    for scope in scopes {
        if let Some(ordinal) = item_ordinals.get(&u64::from(scope.record_index)).copied() {
            // discarded-value: each scope is visited once in this stream.
            let _ = out.insert((stream, scope.record_index), ordinal);
        }
    }
    for scope in scopes {
        let Some(owner) = scope.base_feature_construction().and_then(
            crate::records::feature::base_feature::DesignBaseFeatureConstruction::timeline_owner,
        ) else {
            continue;
        };
        if let Some(ordinal) = item_ordinals.get(&u64::from(owner)).copied() {
            // discarded-value: an owner that also forms a scope yields its position.
            let _ = out.remove(&(stream, owner));
            // discarded-value: each owned scope is visited once in this stream.
            let _ = out.insert((stream, scope.record_index), ordinal);
        }
    }
    crate::records::feature::derived_base::take_owner_positions(
        stream,
        scopes,
        &item_ordinals,
        &mut out,
    );
    for scope in scopes {
        let Some(target_record_index) = scope
            .assembly_alignment()
            .and_then(crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index)
        else {
            continue;
        };
        let target = scopes_by_record[&target_record_index];
        let source_key = (stream, scope.record_index);
        let Some(source_ordinal) = out.remove(&source_key) else {
            continue;
        };
        let target_key = (stream, target.record_index);
        if item_ordinals.contains_key(&u64::from(target.record_index)) {
            continue;
        }
        if out.insert(target_key, source_ordinal).is_some() {
            return Err(CodecError::Malformed(
                "Design JointOrigin target has multiple authored timeline positions".into(),
            ));
        }
    }
    thread_companions::insert_positions(stream, &scopes_by_record, &mut out)?;
    Ok(out)
}

/// The authored position of every record of a Design stream's timeline run:
/// each item in run order, a reverse-normal linked scope that is no item
/// directly after the item that links it. `items` are the item records of the
/// stream's timelines in order.
pub(crate) fn timeline_item_ordinals(
    scopes: &[&DesignParameterScope],
    scopes_by_record: &HashMap<u32, &DesignParameterScope>,
    items: &[u64],
) -> Result<HashMap<u64, u64>, CodecError> {
    let mut linked_scopes = reverse_normal::linked_scopes_outside(scopes, items);
    let shared_links = reverse_normal::shared_timeline_links(scopes_by_record, items);
    let mut item_ordinals = HashMap::<u64, u64>::new();
    let mut next_ordinal = 0_u64;
    for &item in items {
        let followers = u32::try_from(item)
            .ok()
            .and_then(|record| scopes_by_record.get(&record))
            .and_then(|scope| reverse_normal::timeline_link(scope))
            .filter(|link| !shared_links.contains(link))
            .and_then(|link| linked_scopes.remove(link))
            .unwrap_or_default();
        for record in std::iter::once(item).chain(followers) {
            if item_ordinals.insert(record, next_ordinal).is_some() {
                return Err(CodecError::Malformed(
                    "Design timeline item identity is not unique".into(),
                ));
            }
            next_ordinal = next_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("Design feature ordinal exceeds u64".into())
            })?;
        }
    }
    Ok(item_ordinals)
}

/// Result of following one scope's preceding state through internal scopes.
pub(crate) enum ScopeHistoryPredecessor<'a> {
    /// The state chain reaches no projected parameter scope.
    None,
    /// The chain reaches one projected parameter scope.
    Scope(&'a DesignParameterScope),
    /// The state or history identity does not select one scope.
    Ambiguous,
}

pub(super) enum ScopeHistoryBinding {
    Absent,
    Bound(HashMap<String, String>),
}

/// History-state index qualified by component-local Design and ASM history.
pub(crate) struct ScopeHistoryGraph<'a> {
    pub(super) binding: ScopeHistoryBinding,
    pub(super) component_namespaces: HashMap<String, ComponentHistoryNamespace>,
    pub(super) scopes_by_state:
        HashMap<(String, ComponentHistoryNamespace, String, i64), Vec<&'a DesignParameterScope>>,
}

impl<'a> ScopeHistoryGraph<'a> {
    pub(crate) fn new(
        scopes: &'a [DesignParameterScope],
        body_bindings: &[DesignBodyBinding],
        body_recipe_operands: &[DesignBodyRecipeOperand],
        edge_recipes: crate::history::edge_recipe_scope::Sources<'_>,
        component_naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
        histories: &[crate::history_records::AsmHistory],
    ) -> Self {
        let binding = if histories.is_empty() {
            ScopeHistoryBinding::Absent
        } else {
            ScopeHistoryBinding::Bound(crate::history::scope_histories::bind_scope_histories(
                scopes,
                body_bindings,
                body_recipe_operands,
                edge_recipes,
                histories,
                &mut Vec::new(),
            ))
        };
        let mut component_namespaces = HashMap::new();
        for scope in scopes {
            if let Some(namespace) = Self::component_namespace(scope, component_naming_spaces) {
                // discarded-value: each scope identity has one namespace.
                let _ = component_namespaces.insert(scope.id.clone(), namespace);
            }
        }
        let mut scopes_by_state: HashMap<_, Vec<_>> = HashMap::new();
        for scope in scopes {
            let (Some(stream), Some(state_id)) =
                (native_stream(&scope.id), scope.history_state_id())
            else {
                continue;
            };
            let history_id = match &binding {
                ScopeHistoryBinding::Absent => String::new(),
                ScopeHistoryBinding::Bound(bound) => {
                    let Some(history_id) = bound.get(&scope.id) else {
                        continue;
                    };
                    history_id.to_owned()
                }
            };
            let Some(component_namespace) = component_namespaces.get(&scope.id) else {
                continue;
            };
            let key = (
                stream.to_owned(),
                *component_namespace,
                history_id,
                state_id,
            );
            scopes_by_state.entry(key).or_default().push(scope);
        }
        Self {
            binding,
            component_namespaces,
            scopes_by_state,
        }
    }

    pub(crate) fn component_namespace(
        scope: &DesignParameterScope,
        component_naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
    ) -> Option<ComponentHistoryNamespace> {
        Self::component_namespace_at(&scope.id, scope.record_index, component_naming_spaces)
    }

    pub(crate) fn component_namespace_at(
        id: &str,
        record_index: u32,
        component_naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
    ) -> Option<ComponentHistoryNamespace> {
        let stream = native_stream(id)?;
        let mut stream_spaces = component_naming_spaces
            .iter()
            .filter(|space| native_stream(&space.id) == Some(stream))
            .peekable();
        if stream_spaces.peek().is_none() {
            return Some(ComponentHistoryNamespace::Aggregate);
        }
        stream_spaces
            .filter(|space| space.component_record_index <= u64::from(record_index))
            .max_by_key(|space| space.component_record_index)
            .map(|space| ComponentHistoryNamespace::Component(space.component_record_index))
    }

    pub(super) fn history_id(&self, scope: &DesignParameterScope) -> Option<&str> {
        match &self.binding {
            ScopeHistoryBinding::Absent => Some(""),
            ScopeHistoryBinding::Bound(bound) => bound.get(&scope.id).map(String::as_str),
        }
    }

    pub(super) fn state_key(
        &self,
        scope: &DesignParameterScope,
        state_id: i64,
    ) -> Option<(String, ComponentHistoryNamespace, String, i64)> {
        let (Some(stream), Some(namespace), Some(history_id)) = (
            native_stream(&scope.id),
            self.component_namespaces.get(&scope.id),
            self.history_id(scope),
        ) else {
            return None;
        };
        Some((
            stream.to_owned(),
            *namespace,
            history_id.to_owned(),
            state_id,
        ))
    }

    /// Follow `scope.previous_history_state_id()` until a scope accepted by
    /// `projected` is reached. Internal scopes preserve state continuity but
    /// are not themselves authored top-level features.
    pub(crate) fn predecessor<F>(
        &self,
        scope: &DesignParameterScope,
        projected: F,
    ) -> Result<ScopeHistoryPredecessor<'a>, CodecError>
    where
        F: Fn(&DesignParameterScope) -> bool,
    {
        let Some(mut state_id) = scope.previous_history_state_id() else {
            return Ok(ScopeHistoryPredecessor::None);
        };
        let Some(stream) = native_stream(&scope.id) else {
            return Ok(ScopeHistoryPredecessor::Ambiguous);
        };
        let Some(history_id) = self.history_id(scope) else {
            return Ok(ScopeHistoryPredecessor::Ambiguous);
        };
        let mut visited = HashSet::new();
        loop {
            let Some(component_namespace) = self.component_namespaces.get(&scope.id) else {
                return Ok(ScopeHistoryPredecessor::Ambiguous);
            };
            let Some(candidates) = self.scopes_by_state.get(&(
                stream.to_owned(),
                *component_namespace,
                history_id.to_owned(),
                state_id,
            )) else {
                return Ok(ScopeHistoryPredecessor::None);
            };
            let [candidate] = candidates.as_slice() else {
                return Ok(ScopeHistoryPredecessor::Ambiguous);
            };
            if candidate.id == scope.id {
                if visited.is_empty() {
                    return Ok(ScopeHistoryPredecessor::None);
                }
                return Err(CodecError::Malformed(
                    "Design scope history-state dependency is cyclic".into(),
                ));
            }
            if projected(candidate) {
                return Ok(ScopeHistoryPredecessor::Scope(candidate));
            }
            if !visited.insert(candidate.id.as_str()) {
                return Err(CodecError::Malformed(
                    "Design scope history-state dependency is cyclic".into(),
                ));
            }
            let Some(previous_state_id) = candidate.previous_history_state_id() else {
                return Ok(ScopeHistoryPredecessor::None);
            };
            if candidate.history_state_id() == Some(previous_state_id) {
                return Ok(ScopeHistoryPredecessor::None);
            }
            state_id = previous_state_id;
        }
    }
}

pub(super) fn ensure_feature_dependencies_precede(
    features: &[cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    let mut ordinals = HashMap::new();
    for feature in features {
        ordinals.insert(&feature.id, feature.ordinal);
    }
    if ordinals.len() != features.len() {
        return Err(CodecError::Malformed(
            "projected Design feature identity is not unique".into(),
        ));
    }
    let mut unique_ordinals = HashSet::new();
    for feature in features {
        if !unique_ordinals.insert(feature.ordinal) {
            return Err(CodecError::Malformed(
                "projected Design feature ordinal is not unique".into(),
            ));
        }
        if let Some((dependency, dependency_ordinal)) =
            feature.dependencies.iter().find_map(|dependency| {
                ordinals
                    .get(dependency)
                    .filter(|ordinal| **ordinal >= feature.ordinal)
                    .map(|ordinal| (dependency, ordinal))
            })
        {
            return Err(crate::design::text::malformed_design(format_args!(
                "Design feature dependency does not precede its authored timeline position: {dependency} at ordinal {dependency_ordinal} -> {} at ordinal {}",
                feature.id, feature.ordinal,
            )));
        }
    }
    Ok(())
}

/// Order every owner scope's parameters and return how many scopes exceeded
/// [`PARAMETER_ORDER_WORK`].
pub(super) fn normalize_parameter_ordinals(
    parameters: &mut [cadmpeg_ir::features::parameters::DesignParameter],
    owners: &HashMap<cadmpeg_ir::features::ParameterId, Option<cadmpeg_ir::features::FeatureId>>,
) -> Result<usize, CodecError> {
    let mut groups = HashMap::<Option<cadmpeg_ir::features::FeatureId>, Vec<usize>>::new();
    for (index, parameter) in parameters.iter().enumerate() {
        if let Some(indices) = groups.get_mut(&parameter.owner) {
            indices.push(index);
        } else {
            let owner = parameter.owner.clone();
            let indices = vec![index];
            // discarded-value: a new owner group has no previous member list.
            let _ = groups.insert(owner, indices);
        }
    }
    let mut unordered_scopes = 0;
    for (owner, indices) in groups {
        if !order_parameter_scope(
            parameters,
            owners,
            owner.as_ref(),
            &indices,
            &cadmpeg_core::decode::WorkBudget::new(PARAMETER_ORDER_WORK),
        )? {
            unordered_scopes += 1;
        }
    }
    Ok(unordered_scopes)
}

/// Reassign the ordinals of one owner scope so each parameter follows its
/// same-scope dependencies, breaking cycles at their lowest-ordinal member.
///
/// Returns `false` when the ordering exhausts `budget`. The scope then keeps
/// its unique ordinal slots and loses its same-scope dependency edges, so the
/// projection stays valid. Decode reports the skipped scope as
/// `parameter.order-budget-exceeded` and the dropped edges as unbound
/// expression symbols.
pub(super) fn order_parameter_scope(
    parameters: &mut [cadmpeg_ir::features::parameters::DesignParameter],
    owners: &HashMap<cadmpeg_ir::features::ParameterId, Option<cadmpeg_ir::features::FeatureId>>,
    owner: Option<&cadmpeg_ir::features::FeatureId>,
    indices: &[usize],
    budget: &cadmpeg_core::decode::WorkBudget,
) -> Result<bool, CodecError> {
    let mut source_order = indices.to_vec();
    source_order.sort_unstable_by(|a, b| {
        let a = &parameters[*a];
        let b = &parameters[*b];
        (a.ordinal, &a.id).cmp(&(b.ordinal, &b.id))
    });
    let mut ordinals = Vec::new();
    let mut previous: Option<u32> = None;
    for index in source_order {
        let parameter = &mut parameters[index];
        if let Some(previous) = previous {
            if parameter.ordinal <= previous {
                parameter.ordinal = previous
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("Design parameter ordinal exceeds u32"))?;
            }
        }
        ordinals.push(parameter.ordinal);
        previous = Some(parameter.ordinal);
    }
    let mut unresolved = HashSet::new();
    for index in indices {
        // discarded-value: each parameter index occurs once in its owner group.
        let _ = unresolved.insert(*index);
    }
    let mut resolved = HashSet::<cadmpeg_ir::features::ParameterId>::new();
    let mut order = Vec::new();
    while !unresolved.is_empty() {
        let mut ready = Vec::new();
        for index in &unresolved {
            let dependencies = &parameters[*index].dependencies;
            // The round charge also covers the component search of a
            // cycle-breaking round, which walks the same graph.
            if !budget.charge_by(1 + dependencies.len()) {
                break;
            }
            if dependencies.iter().all(|dependency| {
                owners.get(dependency).map(Option::as_ref) != Some(owner)
                    || resolved.contains(dependency)
            }) {
                ready.push(*index);
            }
        }
        if budget.exhausted() {
            for index in indices {
                parameters[*index]
                    .dependencies
                    .retain(|dependency| owners.get(dependency).map(Option::as_ref) != Some(owner));
            }
            return Ok(false);
        }
        ready.sort_by(|a, b| {
            let pa = &parameters[*a];
            let pb = &parameters[*b];
            pa.ordinal.cmp(&pb.ordinal).then_with(|| pa.id.cmp(&pb.id))
        });
        if ready.is_empty() {
            // Every blocked parameter has an unresolved dependency, so the
            // remaining graph contains a cycle. Break its lowest-ordinal
            // member, then resume ordering before selecting another cycle.
            let cycle = cyclic_parameter_components(parameters, &unresolved)
                .into_iter()
                .map(|(first, remaining)| {
                    let breaker = remaining.iter().copied().fold(first, |best, index| {
                        let candidate = &parameters[index];
                        let current = &parameters[best];
                        if (candidate.ordinal, &candidate.id) < (current.ordinal, &current.id) {
                            index
                        } else {
                            best
                        }
                    });
                    (breaker, first, remaining)
                })
                .min_by(|(a, _, _), (b, _, _)| {
                    let a = &parameters[*a];
                    let b = &parameters[*b];
                    (a.ordinal, &a.id).cmp(&(b.ordinal, &b.id))
                });
            if let Some((breaker, first, remaining)) = cycle {
                let mut members = HashSet::new();
                for index in std::iter::once(first).chain(remaining) {
                    let id = parameters[index].id.clone();
                    // discarded-value: component indices are distinct.
                    let _ = members.insert(id);
                }
                parameters[breaker]
                    .dependencies
                    .retain(|dependency| !members.contains(dependency));
            }
            continue;
        }
        for index in ready {
            unresolved.remove(&index);
            let id = parameters[index].id.clone();
            // discarded-value: a parameter is resolved once.
            let _ = resolved.insert(id);
            order.push(index);
        }
    }
    for (index, ordinal) in order.into_iter().zip(ordinals) {
        parameters[index].ordinal = ordinal;
    }
    Ok(true)
}

/// Strongly connected components of the unresolved parameter graph that contain a cycle.
/// Each component retains one member separately from its remaining members.
pub(super) fn cyclic_parameter_components(
    parameters: &[cadmpeg_ir::features::parameters::DesignParameter],
    unresolved: &HashSet<usize>,
) -> Vec<(usize, Vec<usize>)> {
    enum Visit {
        Enter(usize),
        Leave(usize),
    }

    let mut indices = Vec::new();
    for index in unresolved {
        indices.push(*index);
    }
    let mut local_by_id = HashMap::new();
    for (local, index) in indices.iter().enumerate() {
        // discarded-value: distinct parameter IDs retain one local index.
        let _ = local_by_id.insert(&parameters[*index].id, local);
    }
    let mut edges = Vec::new();
    for index in &indices {
        let mut dependencies = Vec::new();
        for dependency in &parameters[*index].dependencies {
            if let Some(local) = local_by_id.get(dependency) {
                dependencies.push(*local);
            }
        }
        edges.push(dependencies);
    }
    let mut incoming = Vec::new();
    for _ in &indices {
        incoming.push(Vec::new());
    }
    for (source, dependencies) in edges.iter().enumerate() {
        for &target in dependencies {
            incoming[target].push(source);
        }
    }

    // Iterative graph walks keep long source dependency chains off the call stack.
    let mut visited = HashSet::new();
    let mut finished = Vec::new();
    for root in 0..indices.len() {
        if visited.contains(&root) {
            continue;
        }
        let mut pending = Vec::new();
        pending.push(Visit::Enter(root));
        while let Some(visit) = pending.pop() {
            match visit {
                Visit::Enter(node) => {
                    if !visited.insert(node) {
                        continue;
                    }
                    pending.push(Visit::Leave(node));
                    for dependency in &edges[node] {
                        pending.push(Visit::Enter(*dependency));
                    }
                }
                Visit::Leave(node) => {
                    finished.push(node);
                }
            }
        }
    }

    let mut assigned = HashSet::new();
    let mut cycles = Vec::new();
    for root in finished.into_iter().rev() {
        if !assigned.insert(root) {
            continue;
        }
        let mut remaining_members = Vec::new();
        let mut pending = Vec::new();
        pending.push(root);
        while let Some(node) = pending.pop() {
            for &source in &incoming[node] {
                if assigned.insert(source) {
                    remaining_members.push(indices[source]);
                    pending.push(source);
                }
            }
        }
        if !remaining_members.is_empty() || edges[root].contains(&root) {
            cycles.push((indices[root], remaining_members));
        }
    }
    cycles
}
