// SPDX-License-Identifier: Apache-2.0
//! Mesh selection search and singleton coordinate topology.

use cadmpeg_core::decode::u64_from_index;

use super::{
    canonical_mesh_boundary_directions, changed_quotient_edges, common_supported_corner_equations,
    copy_mesh_assignment, copy_mesh_boundary_directions, copy_mesh_edge_rows,
    distinct_domain_matching_with_budget, domain_contains, initial_mesh_quotient, least_rotation,
    mesh_candidates_equivalent_with_context, mesh_candidates_identical_with_context,
    orient_face_cycles, reconstruct_mesh_selection, same_unordered_pair, Arc, BTreeMap, BTreeSet,
    BoundaryDraft, CodecError, CoedgeUse, DecodeContext, EdgeRow, FaceTopologyDraft, HashMap,
    HashSet, MeshBoundaryEdgeCandidate, MeshCandidateFailure, MeshCandidateGauge,
    MeshEndpointResolve, MeshFaceBoundaryAssignment, MeshFaceSelection, MeshFixedDirectionOption,
    MeshQuotient, MeshQuotientSignature, MeshSelectionSearch, MeshSelectionStateSignature,
    MeshSolve, SearchOutcome, StandardTopologyDraft, VecDeque, WorkBudget,
    MAX_FACE_EQUATION_CACHE_ENTRIES, MAX_SELECTION_STATE_MEMO_ENTRIES,
};

#[cfg(test)]
use super::{
    deduplicate_mesh_quotient_assignments, domains_have_distinct_matching, largest_fbb_run,
    parse_edge_tables, parse_vertex_table, required_component_roots,
    resolve_standard_mesh_endpoint_candidates, standard_mesh_boundary_assignments, NonZeroUsize,
    UnionFind, MAX_MESH_CONSTRAINT_OPERATIONS,
};

impl<'storage> MeshSelectionSearch<'storage, '_> {
    pub(super) fn should_stop(&self) -> bool {
        self.outcome.is_closed()
    }

    /// Whether every edge holds exactly one candidate pair.
    pub(super) fn has_singleton_edge_candidates(&self) -> Result<bool, CodecError> {
        self.ctx.all_by(
            self.edge_candidates,
            |candidates| Ok(candidates.len() == 1),
            "catia_selection_singleton_candidates",
        )
    }

    pub(super) fn has_exact_singleton_endpoint_domains(&self) -> Result<bool, CodecError> {
        Ok(self.port_identities.is_some() && self.has_singleton_edge_candidates()?)
    }

    pub(super) fn selected_edges(&self) -> Result<BTreeSet<usize>, CodecError> {
        const OPERATION: &str = "catia_selection_selected_edges";
        let ctx = self.ctx;
        let mut edges = BTreeSet::new();
        for (face, selected) in ctx.admit_iter(&self.selected, OPERATION)?.enumerate() {
            let Some((index, _)) = selected else {
                continue;
            };
            if let Some(assignment) = self.assignments[face].get(*index) {
                for boundary in ctx.admit_iter(&assignment.boundaries, OPERATION)? {
                    for use_ in ctx.admit_iter(boundary, OPERATION)? {
                        ctx.insert_btree_set(&mut edges, use_.edge, OPERATION)?;
                    }
                }
            }
        }
        Ok(edges)
    }

    #[cfg(test)]
    pub(super) fn remaining_equation_merge_capacity(
        &self,
        quotient: &mut MeshQuotient<'storage>,
    ) -> Result<Option<usize>, CodecError> {
        fn choice_component_reductions(
            ctx: &DecodeContext<'_>,
            choice: &[[usize; 2]],
            quotient: &mut MeshQuotient<'_>,
            possible: &mut UnionFind<'_>,
        ) -> Result<HashMap<usize, usize>, CodecError> {
            let mut equations = HashMap::<usize, Vec<[usize; 2]>>::new();
            for [left, right] in choice {
                let left = quotient.union.find(ctx, *left)?;
                let right = quotient.union.find(ctx, *right)?;
                let component = possible.find(ctx, left)?;
                if component == possible.find(ctx, right)? {
                    equations.entry(component).or_default().push([left, right]);
                }
            }
            let mut reductions = HashMap::new();
            for (component, equations) in equations {
                let mut roots = HashMap::new();
                for [left, right] in &equations {
                    for root in [left, right] {
                        let next = roots.len();
                        roots.entry(*root).or_insert(next);
                    }
                }
                let mut local = UnionFind::new(roots.len());
                for [left, right] in equations {
                    local.union(ctx, roots[&left], roots[&right])?;
                }
                let mut remaining = 0;
                for node in 0..local.len() {
                    if local.find(ctx, node)? == node {
                        remaining += 1;
                    }
                }
                reductions.insert(component, roots.len() - remaining);
            }
            Ok(reductions)
        }

        let node_count = quotient.union.len();
        let mut possible = UnionFind::new(node_count);
        for node in 0..node_count {
            let root = quotient.union.find(self.ctx, node)?;
            possible.union(self.ctx, node, root)?;
        }
        let mut before = 0usize;
        for node in 0..node_count {
            if possible.find(self.ctx, node)? == node {
                before += 1;
            }
        }
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_some() {
                continue;
            }
            for [left, right] in &self.possible_face_equations[face] {
                possible.union(self.ctx, *left, *right)?;
            }
        }
        let mut after = 0usize;
        for node in 0..node_count {
            if possible.find(self.ctx, node)? == node {
                after += 1;
            }
        }
        let point_count = if self.vertex_points.is_empty() {
            quotient
                .domains
                .iter()
                .flat_map(|domain| domain.iter())
                .max()
                .map_or(0, |point| point + 1)
        } else {
            self.vertex_points.len()
        };
        let mut possible_domains = HashMap::<usize, HashSet<usize>>::new();
        let mut universal_components = HashSet::new();
        let mut possible_root_counts = HashMap::<usize, NonZeroUsize>::new();
        for node in 0..node_count {
            if quotient.union.find(self.ctx, node)? != node {
                continue;
            }
            let component = possible.find(self.ctx, node)?;
            if quotient.domains[node].len() == point_count {
                universal_components.insert(component);
                possible_domains.remove(&component);
            } else if !universal_components.contains(&component) {
                possible_domains
                    .entry(component)
                    .or_default()
                    .extend(quotient.domains[node].iter());
            }
            // The node itself is the component's first root, so the count is
            // never zero.
            let roots = match possible_root_counts.get(&component) {
                Some(roots) => {
                    let Some(next) = roots.checked_add(1) else {
                        return Ok(None);
                    };
                    next
                }
                None => NonZeroUsize::MIN,
            };
            possible_root_counts.insert(component, roots);
        }
        let mut component_merge_capacity = HashMap::<usize, usize>::new();
        let mut independent_capacity = 0usize;
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_some() {
                continue;
            }
            let mut face_capacity = HashMap::<usize, usize>::new();
            let mut independent_face_capacity = 0usize;
            for choice in &self.possible_face_choices[face] {
                let reductions =
                    choice_component_reductions(self.ctx, choice, quotient, &mut possible)?;
                independent_face_capacity = independent_face_capacity.max(
                    reductions
                        .values()
                        .copied()
                        .fold(0usize, usize::saturating_add),
                );
                for (component, reduction) in reductions {
                    face_capacity
                        .entry(component)
                        .and_modify(|capacity| *capacity = (*capacity).max(reduction))
                        .or_insert(reduction);
                }
            }
            let Some(capacity) = independent_capacity.checked_add(independent_face_capacity) else {
                return Ok(None);
            };
            independent_capacity = capacity;
            for (component, capacity) in face_capacity {
                *component_merge_capacity.entry(component).or_default() += capacity;
            }
        }
        let required_root_count = possible_root_counts
            .iter()
            .map(|(component, roots)| {
                required_component_roots(
                    *roots,
                    component_merge_capacity
                        .get(component)
                        .copied()
                        .unwrap_or(0),
                )
            })
            .sum::<usize>();
        if required_root_count > point_count {
            return Ok(None);
        }
        let required_count = |component: &usize| {
            required_component_roots(
                possible_root_counts[component],
                component_merge_capacity
                    .get(component)
                    .copied()
                    .unwrap_or(0),
            )
        };
        let Some(universal_required) = universal_components
            .iter()
            .map(required_count)
            .try_fold(0usize, usize::checked_add)
        else {
            return Ok(None);
        };
        let mut domains = possible_domains
            .into_iter()
            .flat_map(|(component, domain)| {
                let required = required_count(&component);
                std::iter::repeat_with(move || domain.clone()).take(required)
            })
            .collect::<Vec<_>>();
        let Some(available_points) = point_count.checked_sub(domains.len()) else {
            return Ok(None);
        };
        if universal_required > available_points {
            return Ok(None);
        }
        domains.sort_unstable_by_key(HashSet::len);
        let domains = domains
            .into_iter()
            .map(|domain| domain.into_iter().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        if !domains_have_distinct_matching(
            self.ctx,
            domains.iter().map(Vec::as_slice),
            point_count,
        )? {
            return Ok(None);
        }
        let mut singleton_component = HashMap::new();
        for node in 0..node_count {
            if quotient.union.find(self.ctx, node)? != node || quotient.domains[node].len() != 1 {
                continue;
            }
            let Some(point) = quotient.domains[node].iter().next() else {
                return Ok(None);
            };
            let point = *point;
            let component = possible.find(self.ctx, node)?;
            if singleton_component
                .insert(point, component)
                .is_some_and(|previous| previous != component)
            {
                return Ok(None);
            }
        }
        let Some(reduction) = before.checked_sub(after) else {
            return Ok(None);
        };
        Ok(Some(reduction.min(independent_capacity)))
    }

    pub(super) fn face_projection_signature(
        &self,
        face: usize,
        quotient: &mut MeshQuotient<'storage>,
    ) -> Result<MeshQuotientSignature, CodecError> {
        let ctx = self.ctx;
        let mut roots = Vec::new();
        for assignment in ctx.admit_iter(&self.assignments[face], "catia_face_projection_roots")? {
            for boundary in ctx.admit_iter(&assignment.boundaries, "catia_face_projection_roots")? {
                for use_ in ctx.admit_iter(boundary, "catia_face_projection_roots")? {
                    for node in [use_.edge * 2, use_.edge * 2 + 1] {
                        ctx.push_vec(
                            &mut roots,
                            quotient.union.find(ctx, node)?,
                            "catia_face_projection_roots",
                        )?;
                    }
                }
            }
        }
        ctx.sort_unstable_by(
            &mut roots,
            |value| value,
            Ord::cmp,
            "catia_face_projection_root_order_sort",
        )?;
        ctx.dedup_vec(&mut roots, "catia_face_projection_root_order")?;
        let mut signature =
            ctx.collection_vec(roots.len(), "catia_face_projection_signature_rows")?;
        for &root in ctx.admit_iter(&roots, "catia_face_projection_signature_rows")? {
            signature.push((
                ctx.copy_slice(quotient.members(root), "catia_face_projection_member_nodes")?,
                ctx.copy_slice(
                    &quotient.domains[root],
                    "catia_face_projection_domain_points",
                )?,
            ));
        }
        ctx.sort_unstable_by(
            &mut signature,
            |value| value,
            Ord::cmp,
            "catia_face_projection_signature_rows_sort",
        )?;
        Ok(signature)
    }

    /// Whether any assignment of `face` uses an edge of `edges`.
    fn face_uses_any_edge(
        &self,
        face: usize,
        edges: &HashSet<usize>,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        let ctx = self.ctx;
        ctx.any_by(
            &self.assignments[face],
            |assignment| {
                ctx.any_by(
                    &assignment.boundaries,
                    |boundary| {
                        ctx.any_by(
                            boundary,
                            |use_| ctx.contains_hash_set(edges, &use_.edge, operation),
                            operation,
                        )
                    },
                    operation,
                )
            },
            operation,
        )
    }

    #[cfg(test)]
    pub(super) fn propagate_forced_face_equations(
        &self,
        quotient: &mut MeshQuotient<'storage>,
    ) -> Result<bool, CodecError> {
        let budget = WorkBudget::new(usize::MAX);
        self.propagate_forced_face_equations_from(quotient, None, &budget)
    }

    pub(super) fn propagate_forced_face_equations_from(
        &self,
        quotient: &mut MeshQuotient<'storage>,
        changed_edges: Option<&HashSet<usize>>,
        budget: &WorkBudget<'_>,
    ) -> Result<bool, CodecError> {
        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        for (face, selected) in self
            .ctx
            .admit_iter(&self.selected, "catia_forced_equation_queue")?
            .enumerate()
        {
            if selected.is_none()
                && changed_edges.map_or(Ok(true), |changed_edges| {
                    self.face_uses_any_edge(face, changed_edges, "catia_forced_equation_queue")
                })?
            {
                self.ctx
                    .push_back(&mut queue, face, "catia_forced_equation_queue")?;
                self.ctx
                    .insert_hash_set(&mut queued, face, "catia_forced_equation_queued")?;
            }
        }
        while let Some(face) = queue.pop_front() {
            self.ctx
                .charge_work(1, "catia_selection_search_iteration")?;
            if !budget.charge() {
                return Ok(true);
            }
            self.ctx
                .remove_hash_set(&mut queued, &face, "catia_forced_equation_queued")?;
            if self.selected[face].is_some() {
                continue;
            }
            let before = quotient.clone_charged(self.ctx)?;
            let mut changed = false;
            let deterministic = self.assignments[face].len() == 1
                && self.ctx.all_by(
                    &self.assignments[face][0].boundaries,
                    |boundary| {
                        self.ctx.all_by(
                            boundary,
                            |use_| Ok(use_.reversed.is_some()),
                            "catia_forced_deterministic_equations",
                        )
                    },
                    "catia_forced_deterministic_equations",
                )?;
            let equations = if deterministic {
                let [choice] = self.possible_face_choices[face].as_slice() else {
                    return Ok(false);
                };
                self.ctx
                    .copy_slice(choice, "catia_forced_deterministic_equations")?
            } else {
                let cache_key = (face, self.face_projection_signature(face, quotient)?);
                let cached = {
                    let cache = self.face_equation_cache.borrow();
                    self.ctx
                        .get_hash_map(&cache, &cache_key, "catia_forced_equation_cache")?
                        .map(|equations| {
                            self.ctx
                                .copy_slice(equations, "catia_forced_cached_equations")
                        })
                        .transpose()?
                };
                if let Some(cached) = cached {
                    cached
                } else {
                    let Some(common) = common_supported_corner_equations(
                        self.ctx,
                        quotient,
                        &self.assignments[face],
                        budget,
                    )?
                    else {
                        return Ok(budget.exhausted());
                    };
                    let equations = common;
                    let mut cache = self.face_equation_cache.borrow_mut();
                    if cache.len() < MAX_FACE_EQUATION_CACHE_ENTRIES {
                        let cached_equations = self
                            .ctx
                            .copy_slice(&equations, "catia_forced_cached_equation_copy")?;
                        self.ctx.insert_hash_map(
                            &mut cache,
                            cache_key,
                            cached_equations,
                            "catia_forced_equation_cache",
                        )?;
                    }
                    equations
                }
            };
            for [left, right] in self
                .ctx
                .admit_iter(equations, "catia_forced_equation_merges")?
            {
                if quotient.union.find(self.ctx, left)? == quotient.union.find(self.ctx, right)? {
                    continue;
                }
                let Some(root) = quotient.merge_charged(self.ctx, left, right)? else {
                    return Ok(false);
                };
                if !quotient.propagate_component_edge_domains(
                    self.ctx,
                    root,
                    self.edge_candidates,
                    None,
                )? {
                    return Ok(false);
                }
                changed = true;
            }
            if !changed {
                continue;
            }
            let changed_edges = changed_quotient_edges(self.ctx, &before, quotient)?;
            for dependent in self
                .ctx
                .admit_iter(0..self.assignments.len(), "catia_forced_equation_queue")?
            {
                if self.selected[dependent].is_none()
                    && dependent != face
                    && !self.ctx.contains_hash_set(
                        &queued,
                        &dependent,
                        "catia_forced_equation_queued",
                    )?
                    && self.face_uses_any_edge(
                        dependent,
                        &changed_edges,
                        "catia_forced_equation_queue",
                    )?
                {
                    self.ctx.insert_hash_set(
                        &mut queued,
                        dependent,
                        "catia_forced_equation_queued",
                    )?;
                    self.ctx
                        .push_back(&mut queue, dependent, "catia_forced_equation_queue")?;
                }
            }
        }
        Ok(true)
    }

    pub(super) fn selection_orientable(
        &self,
        selection: &[MeshFaceSelection],
    ) -> Result<bool, CodecError> {
        let ctx = self.ctx;
        let mut constraints = Vec::<Vec<(usize, bool)>>::new();
        let mut edge_uses = BTreeMap::<usize, Vec<(usize, bool)>>::new();
        for (face, selected) in ctx
            .admit_iter(selection, "catia_selection_constraint_nodes")?
            .enumerate()
        {
            let Some((assignment_index, directions)) = selected else {
                continue;
            };
            let Some(assignment) = self.assignments[face].get(*assignment_index) else {
                return Ok(false);
            };
            if assignment.boundaries.len() != directions.len() {
                return Ok(false);
            }
            for (boundary, directions) in ctx
                .admit_iter(&assignment.boundaries, "catia_selection_constraint_nodes")?
                .zip(directions)
            {
                if boundary.len() != directions.len() {
                    return Ok(false);
                }
                let node = constraints.len();
                self.ctx.push_vec(
                    &mut constraints,
                    Vec::new(),
                    "catia_selection_constraint_nodes",
                )?;
                for (use_, &direction) in ctx
                    .admit_iter(boundary, "catia_selection_edge_uses")?
                    .zip(directions)
                {
                    let reversed = use_.reversed.unwrap_or(direction);
                    if use_.reversed.is_some() && reversed != direction {
                        return Ok(false);
                    }
                    let uses = ctx
                        .entry_btree_map(&mut edge_uses, use_.edge, "catia_selection_edge_keys")?
                        .or_default();
                    if uses.len() == 2 {
                        return Ok(false);
                    }
                    self.ctx
                        .push_vec(uses, (node, reversed), "catia_selection_edge_uses")?;
                }
            }
        }
        for (_, uses) in ctx.admit_iter(&edge_uses, "catia_selection_adjacent_constraints")? {
            let [(left_node, left_reversed), (right_node, right_reversed)] = uses.as_slice() else {
                continue;
            };
            let parity = left_reversed == right_reversed;
            if left_node == right_node {
                if parity {
                    return Ok(false);
                }
            } else {
                self.ctx.push_vec(
                    &mut constraints[*left_node],
                    (*right_node, parity),
                    "catia_selection_adjacent_constraints",
                )?;
                self.ctx.push_vec(
                    &mut constraints[*right_node],
                    (*left_node, parity),
                    "catia_selection_adjacent_constraints",
                )?;
            }
        }
        let mut flips = self
            .ctx
            .alloc_filled(constraints.len(), None, "catia_selection_flips")?;
        for root in ctx.admit_iter(0..constraints.len(), "catia_selection_flips")? {
            if flips[root].is_some() {
                continue;
            }
            flips[root] = Some(false);
            let mut stack = Vec::new();
            self.ctx
                .push_vec(&mut stack, root, "catia_selection_orientation_stack")?;
            while let Some(node) = stack.pop() {
                self.ctx
                    .charge_work(1, "catia_selection_orientation_work")?;
                let Some(flip) = flips[node] else {
                    return Ok(false);
                };
                for &(neighbor, parity) in
                    ctx.admit_iter(&constraints[node], "catia_selection_orientation_work")?
                {
                    let required = flip ^ parity;
                    match flips[neighbor] {
                        Some(existing) if existing != required => return Ok(false),
                        Some(_) => {}
                        None => {
                            flips[neighbor] = Some(required);
                            self.ctx.push_vec(
                                &mut stack,
                                neighbor,
                                "catia_selection_orientation_stack",
                            )?;
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    pub(super) fn selected_orientable(&self) -> Result<bool, CodecError> {
        self.selection_orientable(&self.selected)
    }

    pub(super) fn fixed_remaining_faces_are_orientable(&self) -> Result<bool, CodecError> {
        let ctx = self.ctx;
        let mut completion =
            ctx.collection_vec(self.selected.len(), "catia_selection_completion")?;
        for selected in ctx.admit_iter(&self.selected, "catia_selection_completion")? {
            completion.push(match selected {
                Some((index, directions)) => {
                    Some((*index, copy_mesh_boundary_directions(ctx, directions)?))
                }
                None => None,
            });
        }
        for (face, selected) in ctx
            .admit_iter(&mut completion, "catia_selection_completion_boundaries")?
            .enumerate()
        {
            if selected.is_some() {
                continue;
            }
            let [assignment] = self.assignments[face].as_slice() else {
                continue;
            };
            // A face with a single assignment whose uses are all reversed has
            // exactly one direction row per boundary.
            if !ctx.all_by(
                &assignment.boundaries,
                |boundary| {
                    ctx.all_by(
                        boundary,
                        |use_| Ok(use_.reversed.is_some()),
                        "catia_selection_completion_directions",
                    )
                },
                "catia_selection_completion_directions",
            )? {
                continue;
            }
            let directions = ctx.collect_indexed_vec(
                assignment.boundaries.len(),
                "catia_selection_completion_boundaries",
                |boundary| {
                    ctx.collect_vec(
                        assignment.boundaries[boundary]
                            .iter()
                            .map(|use_| use_.reversed.unwrap_or(false)),
                        "catia_selection_completion_directions",
                    )
                },
            )?;
            *selected = Some((0, directions));
        }
        self.selection_orientable(&completion)
    }

    pub(super) fn prepare_selected_branch(
        &self,
        quotient: &MeshQuotient<'storage>,
        changed_edges: &HashSet<usize>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<Option<MeshQuotient<'storage>>, CodecError> {
        let mut measured = quotient.clone_charged(self.ctx)?;
        if !self.has_exact_singleton_endpoint_domains()?
            && !self.propagate_forced_face_equations_from(
                &mut measured,
                Some(changed_edges),
                propagation_budget,
            )?
        {
            return Ok(None);
        }
        if !measured.merge_singleton_coordinate_roots(self.ctx, self.edge_candidates)? {
            return Ok(None);
        }
        let root_count = measured.root_count(self.ctx)?;
        if root_count < self.vertex_points.len() {
            return Ok(None);
        }
        if root_count == self.vertex_points.len()
            && !self.has_exact_singleton_endpoint_domains()?
            && !measured.point_assignment_exists(
                self.ctx,
                self.vertex_points.len(),
                self.edge_candidates,
                Some(propagation_budget),
            )?
        {
            return Ok(None);
        }
        let orientable = if self.has_exact_singleton_endpoint_domains()? {
            true
        } else {
            self.fixed_remaining_faces_are_orientable()?
        };
        Ok(orientable.then_some(measured))
    }

    #[cfg(test)]
    pub(crate) fn search(&mut self, quotient: &MeshQuotient<'storage>) -> Result<(), CodecError> {
        self.search_with_limit(quotient, MAX_MESH_CONSTRAINT_OPERATIONS)
    }

    pub(super) fn search_with_budget(
        &mut self,
        quotient: &MeshQuotient<'storage>,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        self.search_from_state(quotient, false, budget, propagation_budget)
    }

    pub(super) fn fixed_direction_options(
        &self,
        measured: &MeshQuotient<'storage>,
        face: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Vec<MeshFixedDirectionOption<'storage>>, CodecError> {
        let Some(direction_options) = self
            .fixed_face_directions
            .get(face)
            .and_then(Option::as_ref)
        else {
            return Ok(Vec::new());
        };
        let [assignment] = self.assignments[face].as_slice() else {
            return Ok(Vec::new());
        };
        let ctx = self.ctx;
        let mut seen = HashSet::<(Vec<Vec<bool>>, Vec<Option<bool>>)>::new();
        let mut output = Vec::new();
        for label_directions in
            ctx.admit_iter(direction_options, "catia_fixed_direction_options")?
        {
            let mut next_orientations = ctx.copy_slice(
                &self.fixed_edge_orientations,
                "catia_fixed_next_orientations",
            )?;
            let mut orient = |use_: &MeshBoundaryEdgeCandidate, label_direction: bool| {
                let Some(required) = use_.reversed else {
                    return Ok(true);
                };
                let Some(orientation) = next_orientations.get_mut(use_.edge) else {
                    return Ok(false);
                };
                let required_orientation = required ^ label_direction;
                Ok(match *orientation {
                    Some(existing) => existing == required_orientation,
                    None => {
                        *orientation = Some(required_orientation);
                        true
                    }
                })
            };
            let constrained = ctx.all_by(
                assignment.boundaries.iter().zip(label_directions),
                |(boundary, directions)| {
                    ctx.all_by(
                        boundary.iter().zip(directions),
                        |(use_, &label_direction)| orient(use_, label_direction),
                        "catia_fixed_next_orientations",
                    )
                },
                "catia_fixed_next_orientations",
            )?;
            if !constrained {
                continue;
            }
            let option = measured.assignment_option_for_label_directions(
                self.ctx,
                assignment,
                label_directions,
                &next_orientations,
                budget,
            )?;
            let Some((directions, quotient)) = option else {
                continue;
            };
            let signature = (
                canonical_mesh_boundary_directions(self.ctx, &directions)?,
                self.ctx
                    .copy_slice(&next_orientations, "catia_fixed_orientation_signature")?,
            );
            if self
                .ctx
                .insert_hash_set(&mut seen, signature, "catia_fixed_direction_signatures")?
            {
                self.ctx.push_vec(
                    &mut output,
                    (directions, quotient, next_orientations),
                    "catia_fixed_direction_options",
                )?;
            }
        }
        Ok(output)
    }

    pub(super) fn search_fixed_direction_with_budget(
        &mut self,
        quotient: &MeshQuotient<'storage>,
        budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        if self.should_stop() {
            return Ok(());
        }
        if !budget.charge() {
            self.outcome.exhaust();
            return Ok(());
        }
        let mut measured = quotient.clone_charged(self.ctx)?;
        if !measured.merge_singleton_coordinate_roots(self.ctx, self.edge_candidates)? {
            return Ok(());
        }
        if measured.root_count(self.ctx)? < self.vertex_points.len() {
            return Ok(());
        }
        if self.visited_states.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let signature = self.selection_state_signature(&measured, true)?;
            if !self.ctx.insert_hash_set(
                &mut self.visited_states,
                signature,
                "catia_selection_state_memo",
            )? {
                return Ok(());
            }
        }
        let selected_edges = self.selected_edges()?;
        let ctx = self.ctx;
        let mut impossible = false;
        // The ready face with the least search key: adjacent to the selection
        // first, then fewer options, any fixed use, more uses.
        let mut face = None;
        for (face_index, selected) in ctx
            .admit_iter(&self.selected, "catia_fixed_face_order")?
            .enumerate()
        {
            if selected.is_some() {
                continue;
            }
            let Some(directions) = self
                .fixed_face_directions
                .get(face_index)
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let [assignment] = self.assignments[face_index].as_slice() else {
                continue;
            };
            let mut ready = true;
            let mut local_fixed = 0usize;
            let mut adjacent = false;
            let mut use_count = 0usize;
            for boundary in ctx.admit_iter(&assignment.boundaries, "catia_fixed_face_order")? {
                for use_ in ctx.admit_iter(boundary, "catia_fixed_face_order")? {
                    ready &= self
                        .fixed_edge_orientations
                        .get(use_.edge)
                        .and_then(Option::as_ref)
                        .is_some()
                        || use_.reversed.is_some()
                        || !self
                            .edge_has_fixed_direction
                            .get(use_.edge)
                            .copied()
                            .unwrap_or(false);
                    local_fixed += usize::from(use_.reversed.is_some());
                    adjacent |= ctx.contains_btree_set(
                        &selected_edges,
                        &use_.edge,
                        "catia_fixed_face_order",
                    )?;
                    use_count += 1;
                }
            }
            if !ready {
                continue;
            }
            // The exact quotient options are generated once for the face
            // selected below. This count only orders the search; probing
            // every face here would construct and hash the same large
            // quotient states a second time.
            let viable_options = directions.len();
            if viable_options == 0 {
                impossible = true;
                continue;
            }
            let key = (
                !adjacent,
                viable_options,
                local_fixed == 0,
                usize::MAX - use_count,
                directions.len(),
                face_index,
            );
            if face.is_none_or(|(least, _)| key < least) {
                face = Some((key, face_index));
            }
        }
        let face = face.map(|(_, face)| face);
        if impossible {
            return Ok(());
        }
        let Some(face) = face else {
            if let Some(edge) = ctx.find_map(
                self.edge_has_fixed_direction.iter().enumerate(),
                |(edge, has_fixed)| {
                    Ok((*has_fixed
                        && self
                            .fixed_edge_orientations
                            .get(edge)
                            .is_some_and(Option::is_none))
                    .then_some(edge))
                },
                "catia_fixed_unoriented_edges",
            )? {
                for orientation in [false, true] {
                    if self.should_stop() {
                        return Ok(());
                    }
                    self.fixed_edge_orientations[edge] = Some(orientation);
                    self.search_fixed_direction_with_budget(&measured, budget)?;
                }
                self.fixed_edge_orientations[edge] = None;
                return Ok(());
            }
            let mut selected_assignments = Vec::new();
            let mut directions = Vec::new();
            self.ctx.reserve_vec(
                &mut selected_assignments,
                self.selected.len(),
                "catia_fixed_selected_assignments",
            )?;
            self.ctx.reserve_vec(
                &mut directions,
                self.selected.len(),
                "catia_fixed_selected_directions",
            )?;
            for (face, selected) in ctx
                .admit_iter(&self.selected, "catia_fixed_selected_assignments")?
                .enumerate()
            {
                let Some((assignment, selected_directions)) = selected else {
                    return Ok(());
                };
                if *assignment != 0 {
                    return Ok(());
                }
                let Some(source) = self.assignments[face].get(*assignment) else {
                    return Ok(());
                };
                selected_assignments.push(copy_mesh_assignment(self.ctx, source)?);
                directions.push(copy_mesh_boundary_directions(
                    self.ctx,
                    selected_directions,
                )?);
            }
            let Some(port_identities) = self.port_identities else {
                return Ok(());
            };
            let Some(outcome) = resolve_singleton_mesh_selection(self.ctx, crate::solve::mesh_quotient::selection_search::ResolveSingletonMeshSelectionInputs { edge_rows: self.edge_rows, vertex_points: self.vertex_points, edge_candidates: self.edge_candidates, selected: &selected_assignments, directions: &directions, port_identities, budget, candidate_gauge: self.candidate_gauge })?
            else {
                return Ok(());
            };
            match outcome {
                MeshSolve::Solved((topology, assignment)) => {
                    let candidate = (topology, assignment);
                    let gauge = self.candidate_gauge;
                    let equivalent = if let SearchOutcome::Solved(previous) = &self.outcome {
                        mesh_candidates_identical_with_context(self.ctx, previous, &candidate)?
                            || mesh_candidates_equivalent_with_context(
                                self.ctx, previous, &candidate, gauge,
                            )?
                    } else {
                        false
                    };
                    self.outcome.record_solved(candidate, |_, _| equivalent);
                }
                MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                    self.outcome.mark_ambiguous();
                }
                MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => self.outcome.exhaust(),
                MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
            }
            return Ok(());
        };
        let options = self.fixed_direction_options(&measured, face, Some(budget))?;
        if budget.exhausted() {
            self.outcome.exhaust();
            return Ok(());
        }
        for (directions, next_quotient, next_orientations) in
            ctx.admit_iter(options, "catia_fixed_direction_options")?
        {
            if self.should_stop() {
                return Ok(());
            }
            let previous_orientations =
                std::mem::replace(&mut self.fixed_edge_orientations, next_orientations);
            self.selected[face] = Some((0, directions));
            self.search_fixed_direction_with_budget(&next_quotient, budget)?;
            self.selected[face] = None;
            self.fixed_edge_orientations = previous_orientations;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn search_with_limit(
        &mut self,
        quotient: &MeshQuotient<'storage>,
        limit: usize,
    ) -> Result<(), CodecError> {
        let budget = WorkBudget::new(limit);
        let propagation_budget = WorkBudget::new(limit);
        self.search_from_state(quotient, false, &budget, &propagation_budget)
    }

    pub(super) fn selection_state_signature(
        &self,
        quotient: &MeshQuotient<'storage>,
        prepared: bool,
    ) -> Result<MeshSelectionStateSignature, CodecError> {
        let mut selected = Vec::new();
        for choice in self
            .ctx
            .admit_iter(&self.selected, "catia_selection_signature_faces")?
        {
            let copy = match choice {
                Some((assignment, directions)) => Some((
                    *assignment,
                    copy_mesh_boundary_directions(self.ctx, directions)?,
                )),
                None => None,
            };
            self.ctx
                .push_vec(&mut selected, copy, "catia_selection_signature_faces")?;
        }
        Ok((
            prepared,
            selected,
            quotient.signature_charged(self.ctx)?,
            self.ctx.copy_slice(
                &self.fixed_edge_orientations,
                "catia_selection_signature_edge_orientations",
            )?,
        ))
    }

    pub(super) fn search_from_state(
        &mut self,
        quotient: &MeshQuotient<'storage>,
        prepared: bool,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        if self.should_stop() {
            return Ok(());
        }
        if !budget.charge() {
            self.outcome.exhaust();
            return Ok(());
        }
        if self.visited_states.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let signature = self.selection_state_signature(quotient, prepared)?;
            if !self.ctx.insert_hash_set(
                &mut self.visited_states,
                signature,
                "catia_selection_state_memo",
            )? {
                return Ok(());
            }
        }
        self.search_state(quotient, prepared, budget, propagation_budget)
    }

    pub(super) fn search_state(
        &mut self,
        quotient: &MeshQuotient<'storage>,
        prepared: bool,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        let mut measured = quotient.clone_charged(self.ctx)?;
        if !prepared {
            if !self.has_exact_singleton_endpoint_domains()?
                && !self.propagate_forced_face_equations_from(
                    &mut measured,
                    None,
                    propagation_budget,
                )?
            {
                return Ok(());
            }
            if !measured.merge_singleton_coordinate_roots(self.ctx, self.edge_candidates)? {
                return Ok(());
            }
            let root_count = measured.root_count(self.ctx)?;
            if root_count < self.vertex_points.len() {
                return Ok(());
            }
            if !self.has_exact_singleton_endpoint_domains()?
                && root_count == self.vertex_points.len()
                && !measured.point_assignment_exists(
                    self.ctx,
                    self.vertex_points.len(),
                    self.edge_candidates,
                    Some(propagation_budget),
                )?
            {
                if propagation_budget.exhausted() {
                    self.outcome.exhaust();
                }
                return Ok(());
            }
            if !self.has_exact_singleton_endpoint_domains()?
                && !self.fixed_remaining_faces_are_orientable()?
            {
                return Ok(());
            }
        }
        let selected_edges = self.selected_edges()?;
        let ctx = self.ctx;
        let mut adjacent_faces = HashSet::new();
        if !selected_edges.is_empty() {
            for (face, selected) in ctx
                .admit_iter(&self.selected, "catia_selection_adjacent_faces")?
                .enumerate()
            {
                if selected.is_none()
                    && ctx.any_by(
                        &self.assignments[face],
                        |assignment| {
                            ctx.any_by(
                                &assignment.boundaries,
                                |boundary| {
                                    ctx.any_by(
                                        boundary,
                                        |use_| {
                                            ctx.contains_btree_set(
                                                &selected_edges,
                                                &use_.edge,
                                                "catia_selection_adjacent_faces",
                                            )
                                        },
                                        "catia_selection_adjacent_faces",
                                    )
                                },
                                "catia_selection_adjacent_faces",
                            )
                        },
                        "catia_selection_adjacent_faces",
                    )?
                {
                    self.ctx.insert_hash_set(
                        &mut adjacent_faces,
                        face,
                        "catia_selection_adjacent_faces",
                    )?;
                }
            }
        }
        let adjacent_faces = (!adjacent_faces.is_empty()).then_some(adjacent_faces);
        let mut next = None;
        for face in ctx.admit_iter(0..self.selected.len(), "catia_selection_next_face")? {
            if self.selected[face].is_some()
                || adjacent_faces
                    .as_ref()
                    .map_or(Ok::<_, CodecError>(false), |adjacent| {
                        Ok(!ctx.contains_hash_set(
                            adjacent,
                            &face,
                            "catia_selection_adjacent_faces",
                        )?)
                    })?
            {
                continue;
            }
            if !budget.charge() {
                break;
            }
            if self.face_work[face].is_none() {
                continue;
            }
            let assignments = &self.assignments[face];
            let key = if assignments.is_empty() {
                (0, 0, 0, 0, 0, face)
            } else {
                let mut unknown_uses = Vec::new();
                for assignment in ctx.admit_iter(assignments, "catia_selection_direction_work")? {
                    let unknown = ctx.fold(
                        &assignment.boundaries,
                        0usize,
                        |total, boundary| {
                            Ok(total
                                + ctx.fold(
                                    boundary,
                                    0usize,
                                    |count, use_| Ok(count + usize::from(use_.reversed.is_none())),
                                    "catia_selection_direction_work",
                                )?)
                        },
                        "catia_selection_direction_work",
                    )?;
                    ctx.push_vec(&mut unknown_uses, unknown, "catia_selection_direction_work")?;
                }
                let Some(direction_work) = direction_work_estimate(ctx, &unknown_uses)? else {
                    budget.exhaust();
                    break;
                };
                let mut can_merge = false;
                let mut selected_incidence = 0;
                let mut constrained = 0;
                for assignment in ctx.admit_iter(assignments, "catia_selection_face_key")? {
                    if !can_merge {
                        can_merge = mesh_assignment_can_merge(self.ctx, assignment, &mut measured)?;
                    }
                    let mut used = 0;
                    let mut constrained_uses = 0;
                    for use_ in assignment.boundaries.iter().flatten() {
                        ctx.charge_work(1, "catia_selection_face_key")?;
                        if ctx.contains_btree_set(
                            &selected_edges,
                            &use_.edge,
                            "catia_selection_face_key",
                        )? {
                            used += 1;
                        }
                        let left = measured.union.find(self.ctx, use_.edge * 2)?;
                        let right = measured.union.find(self.ctx, use_.edge * 2 + 1)?;
                        if measured.domains[left].len() < self.vertex_points.len()
                            || measured.domains[right].len() < self.vertex_points.len()
                        {
                            constrained_uses += 1;
                        }
                    }
                    if used > selected_incidence {
                        selected_incidence = used;
                    }
                    if constrained_uses > constrained {
                        constrained = constrained_uses;
                    }
                }
                (
                    if can_merge { 1 } else { 2 },
                    direction_work,
                    assignments.len(),
                    usize::MAX - selected_incidence,
                    usize::MAX - constrained,
                    face,
                )
            };
            if next.is_none_or(|previous| key < previous) {
                next = Some(key);
            }
        }

        if budget.exhausted() {
            self.outcome.exhaust();
            return Ok(());
        }
        let Some((_, supported, _, _, _, face)) = next else {
            let mut selected_assignments = Vec::new();
            let mut directions = Vec::new();
            self.ctx.reserve_vec(
                &mut selected_assignments,
                self.selected.len(),
                "catia_search_selected_assignments",
            )?;
            self.ctx.reserve_vec(
                &mut directions,
                self.selected.len(),
                "catia_search_selected_directions",
            )?;
            for (face, selected) in ctx
                .admit_iter(&self.selected, "catia_search_selected_assignments")?
                .enumerate()
            {
                let Some((index, selected_directions)) = selected else {
                    return Ok(());
                };
                let Some(assignment) = self.assignments[face].get(*index) else {
                    return Ok(());
                };
                selected_assignments.push(copy_mesh_assignment(self.ctx, assignment)?);
                directions.push(copy_mesh_boundary_directions(
                    self.ctx,
                    selected_directions,
                )?);
            }
            if self.has_singleton_edge_candidates()? {
                if let Some(port_identities) = self.port_identities {
                    let outcome = resolve_singleton_mesh_selection(self.ctx, crate::solve::mesh_quotient::selection_search::ResolveSingletonMeshSelectionInputs { edge_rows: self.edge_rows, vertex_points: self.vertex_points, edge_candidates: self.edge_candidates, selected: &selected_assignments, directions: &directions, port_identities, budget, candidate_gauge: self.candidate_gauge })?;
                    if let Some(outcome) = outcome {
                        match outcome {
                            MeshSolve::Solved((topology, assignment)) => {
                                let candidate = (topology, assignment);
                                let gauge = self.candidate_gauge;
                                let equivalent =
                                    if let SearchOutcome::Solved(previous) = &self.outcome {
                                        mesh_candidates_identical_with_context(
                                            self.ctx, previous, &candidate,
                                        )? || mesh_candidates_equivalent_with_context(
                                            self.ctx, previous, &candidate, gauge,
                                        )?
                                    } else {
                                        false
                                    };
                                self.outcome.record_solved(candidate, |_, _| equivalent);
                            }
                            MeshSolve::Failed(MeshCandidateFailure::Ambiguous(())) => {
                                self.outcome.mark_ambiguous();
                            }
                            MeshSolve::Failed(MeshCandidateFailure::Exhausted(())) => {
                                self.outcome.exhaust();
                            }
                            MeshSolve::Failed(MeshCandidateFailure::Rejected(())) => {}
                        }
                        if self.should_stop() {
                            return Ok(());
                        }
                    }
                }
            }
            let mut quotient = measured.clone_charged(self.ctx)?;
            let Some(root_points) = quotient.close_coordinate_roots(
                self.ctx,
                self.vertex_points.len(),
                self.edge_candidates,
                Some(budget),
            )?
            else {
                if budget.exhausted() {
                    self.outcome.exhaust();
                }
                return Ok(());
            };
            let candidate = 'candidate: {
                let Some(mut topology) = reconstruct_mesh_selection(
                    self.ctx,
                    self.edge_rows,
                    self.vertex_points,
                    &selected_assignments,
                    &directions,
                )?
                else {
                    break 'candidate None;
                };
                let mut use_counts = self.ctx.alloc_filled(
                    topology.edge_rows.len(),
                    0usize,
                    "catia_search_edge_uses",
                )?;
                for face in ctx.admit_iter(&topology.faces, "catia_search_edge_uses")? {
                    for boundary in ctx.admit_iter(&face.boundaries, "catia_search_edge_uses")? {
                        for coedge in
                            ctx.admit_iter(&boundary.coedges[..], "catia_search_edge_uses")?
                        {
                            use_counts[coedge.edge_row] += 1;
                        }
                    }
                }
                if ctx.any_by(
                    &use_counts,
                    |count| Ok(*count > 2),
                    "catia_search_edge_uses",
                )? {
                    break 'candidate None;
                }
                if ctx.all_by(
                    &use_counts,
                    |count| Ok(*count == 2),
                    "catia_search_edge_uses",
                )? && orient_face_cycles(self.ctx, &mut topology.faces)?.is_none()
                {
                    break 'candidate None;
                }
                let Some(edge_vertices) = topology.edge_vertices(self.ctx)? else {
                    break 'candidate None;
                };
                let mut point_assignment = self.ctx.alloc_filled(
                    topology.logical_vertex_count,
                    None,
                    "catia_search_point_assignment",
                )?;
                for (edge, vertices) in ctx
                    .admit_iter(edge_vertices, "catia_search_point_assignment")?
                    .enumerate()
                {
                    for (port, vertex) in vertices.into_iter().enumerate() {
                        let root = quotient.union.find(self.ctx, edge * 2 + port)?;
                        let Some(&point) =
                            ctx.get_hash_map(&root_points, &root, "catia_search_point_assignment")?
                        else {
                            break 'candidate None;
                        };
                        match point_assignment[vertex] {
                            Some(stored) if stored != point => break 'candidate None,
                            Some(_) => {}
                            None => point_assignment[vertex] = Some(point),
                        }
                    }
                    let [Some(start), Some(end)] = vertices.map(|vertex| point_assignment[vertex])
                    else {
                        break 'candidate None;
                    };
                    let points = [start, end];
                    let closed_ports = quotient.union.find(self.ctx, edge * 2)?
                        == quotient.union.find(self.ctx, edge * 2 + 1)?;
                    if !mesh_edge_points_compatible(
                        ctx,
                        closed_ports,
                        &self.edge_candidates[edge],
                        points,
                    )? {
                        break 'candidate None;
                    }
                }
                let mut completed_points =
                    ctx.collection_vec(point_assignment.len(), "catia_search_completed_points")?;
                for point in ctx.admit_iter(point_assignment, "catia_search_completed_points")? {
                    let Some(point) = point else {
                        break 'candidate None;
                    };
                    completed_points.push(point);
                }
                Some((topology, completed_points))
            };
            if let Some(candidate) = candidate {
                let gauge = self.candidate_gauge;
                let equivalent = if let SearchOutcome::Solved(previous) = &self.outcome {
                    mesh_candidates_identical_with_context(self.ctx, previous, &candidate)?
                        || mesh_candidates_equivalent_with_context(
                            self.ctx, previous, &candidate, gauge,
                        )?
                } else {
                    false
                };
                self.outcome.record_solved(candidate, |_, _| equivalent);
            }
            return Ok(());
        };
        if supported == 0 {
            return Ok(());
        }
        let remaining_work = budget.remaining();
        if remaining_work == 0 {
            self.outcome.exhaust();
            return Ok(());
        }
        let mut options = Vec::new();
        for assignment_index in 0..self.assignments[face].len() {
            ctx.charge_work(1, "catia_search_assignment_options")?;
            if !budget.charge() {
                self.outcome.exhaust();
                return Ok(());
            }
            let Some(remaining) = remaining_work.checked_sub(options.len()) else {
                self.outcome.exhaust();
                return Ok(());
            };
            if remaining == 0 {
                break;
            }
            let assignment = &self.assignments[face][assignment_index];
            let assignment_options = if let Some(direction_options) = self
                .fixed_face_directions
                .get(face)
                .and_then(Option::as_ref)
            {
                if assignment_index != 0 {
                    continue;
                }
                measured.assignment_options_for_directions(
                    self.ctx,
                    assignment,
                    direction_options,
                    remaining,
                    Some(budget),
                )
            } else {
                measured.assignment_options_limited(
                    self.ctx,
                    assignment,
                    self.edge_candidates,
                    &selected_edges,
                    remaining,
                    Some(budget),
                )
            }?;
            if budget.exhausted() {
                self.outcome.exhaust();
                return Ok(());
            }
            for (directions, next_quotient) in
                ctx.admit_iter(assignment_options, "catia_search_assignment_options")?
            {
                ctx.push_vec(
                    &mut options,
                    (assignment_index, directions, next_quotient),
                    "catia_search_assignment_options",
                )?;
            }
        }
        ctx.retain_vec(
            &mut options,
            |(_, _, quotient)| Ok(quotient.root_count(ctx)? >= self.vertex_points.len()),
            "catia_selection_search_iteration",
        )?;
        if options.is_empty() {
            return Ok(());
        }
        if let [(assignment_index, directions, next_quotient)] = options.as_slice() {
            let changed_edges = changed_quotient_edges(self.ctx, &measured, next_quotient)?;
            self.selected[face] = Some((
                *assignment_index,
                copy_mesh_boundary_directions(self.ctx, directions)?,
            ));
            if self.selected_orientable()? {
                if let Some(next_quotient) =
                    self.prepare_selected_branch(next_quotient, &changed_edges, propagation_budget)?
                {
                    // The branch preflight has already run. Continue the
                    // forced suffix without another memo entry or preflight.
                    self.search_state(&next_quotient, true, budget, propagation_budget)?;
                } else if budget.exhausted() || propagation_budget.exhausted() {
                    self.outcome.exhaust();
                }
            }
            self.selected[face] = None;
            return Ok(());
        }
        let mut ranked_options = Vec::new();
        for (assignment, directions, quotient) in
            ctx.admit_iter(options, "catia_search_ranked_options")?
        {
            let mut count = 0usize;
            let mut freedom = 0u128;
            for node in ctx.admit_iter(0..quotient.union.len(), "catia_search_ranked_options")? {
                if quotient.union.root(self.ctx, node)? == node {
                    count += 1;
                    freedom += u128::from(u64_from_index(quotient.domains[node].len()));
                }
            }
            self.ctx.push_vec(
                &mut ranked_options,
                (((count, freedom), assignment, directions), quotient),
                "catia_search_ranked_options",
            )?;
        }
        self.ctx.sort_unstable_by(
            &mut ranked_options,
            |value| &value.0,
            Ord::cmp,
            "catia_search_assignment_options_sort",
        )?;

        for ((_, assignment_index, directions), next_quotient) in
            ctx.admit_iter(ranked_options, "catia_search_ranked_options")?
        {
            let changed_edges = changed_quotient_edges(self.ctx, &measured, &next_quotient)?;
            self.selected[face] = Some((assignment_index, directions));
            if self.selected_orientable()? {
                if let Some(next_quotient) = self.prepare_selected_branch(
                    &next_quotient,
                    &changed_edges,
                    propagation_budget,
                )? {
                    // `prepare_selected_branch` has already applied the recursive
                    // entry preflight to this quotient.
                    self.search_from_state(&next_quotient, true, budget, propagation_budget)?;
                } else if budget.exhausted() || propagation_budget.exhausted() {
                    self.outcome.exhaust();
                }
            }
            self.selected[face] = None;
            if self.should_stop() {
                return Ok(());
            }
        }
        Ok(())
    }
}

/// The direction choices a face's endpoint assignments state, summed.
///
/// Each assignment states `2^unknown` choices for its `unknown` boundary uses
/// with no stated reversal. `None` states a figure the work counter cannot
/// hold: more choices than any budget can enumerate.
pub(super) fn direction_work_estimate(
    ctx: &DecodeContext<'_>,
    unknown_uses: &[usize],
) -> Result<Option<usize>, CodecError> {
    ctx.fold(
        unknown_uses,
        Some(0usize),
        |total, &unknown| {
            Ok(total.and_then(|total| {
                let choices = u32::try_from(unknown)
                    .ok()
                    .and_then(|unknown| 1usize.checked_shl(unknown))?;
                total.checked_add(choices)
            }))
        },
        "catia_selection_direction_work",
    )
}

pub(super) fn mesh_assignment_can_merge(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    quotient: &mut MeshQuotient<'_>,
) -> Result<bool, CodecError> {
    pub(super) fn possible_ports(use_: MeshBoundaryEdgeCandidate, end: bool) -> [Option<usize>; 2] {
        let port = |reversed: bool| {
            use_.edge
                .checked_mul(2)?
                .checked_add(usize::from(reversed != end))
        };
        match use_.reversed {
            Some(reversed) => [port(reversed), None],
            None => [port(false), port(true)],
        }
    }

    for boundary in &assignment.boundaries {
        ctx.charge_work(1, "catia_selection_assignment_merge")?;
        for index in 0..boundary.len() {
            ctx.charge_work(1, "catia_selection_assignment_merge")?;
            let left = possible_ports(boundary[index], true);
            let right = possible_ports(boundary[(index + 1) % boundary.len()], false);
            for left in left.into_iter().flatten() {
                for right in right.into_iter().flatten() {
                    if quotient.union.find(ctx, left)? != quotient.union.find(ctx, right)? {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

pub(in crate::solve) fn mesh_edge_points_compatible(
    ctx: &DecodeContext<'_>,
    closed_ports: bool,
    candidates: &[[usize; 2]],
    points: [usize; 2],
) -> Result<bool, CodecError> {
    Ok((points[0] != points[1] || closed_ports)
        && (candidates.is_empty()
            || ctx.any_by(
                candidates,
                |candidate| Ok(same_unordered_pair(*candidate, points)),
                "catia_mesh_edge_point_candidates",
            )?))
}

/// Resolve standard trim assignments through their abstract physical-port
/// quotient before binding the quotient bijectively to coordinate rows.
#[cfg(test)]
pub(in crate::solve) fn parse_standard_mesh_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Result<Option<(StandardTopologyDraft, Vec<usize>)>, CodecError> {
    let Some(face_run) = largest_fbb_run(ctx, bytes)? else {
        return Ok(None);
    };
    let face_count = face_run.face_count();
    let after_faces = face_run.after_faces();
    let Some((edge_rows, vertex_header)) = parse_edge_tables(ctx, bytes, after_faces)? else {
        return Ok(None);
    };
    let Some(vertex_points) = parse_vertex_table(ctx, bytes, vertex_header)? else {
        return Ok(None);
    };
    if edge_rows.len() != edge_faces.len() || edge_rows.len() != edge_candidates.len() {
        return Ok(None);
    }
    let Some(mut assignments) =
        standard_mesh_boundary_assignments(ctx, bytes, edge_faces, Some(edge_candidates))?
    else {
        return Ok(None);
    };
    if assignments.len() != face_count {
        return Ok(None);
    }
    deduplicate_mesh_quotient_assignments(ctx, &mut assignments)?;
    // Standard-row occurrence direction is a face-quotient choice. Complete
    // FBB tables retain their scoped handle equalities in these local ports.
    let Some(port_identities) = crate::solve::missing_edge::edge_port_identities(ctx, bytes)?
    else {
        return Ok(None);
    };
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    resolve_standard_mesh_endpoint_candidates(
        ctx,
        crate::solve::mesh_quotient::ResolveStandardMeshEndpointCandidatesInputs {
            edge_rows: &edge_rows,
            vertex_points: &vertex_points,
            edge_candidates,
            assignments,
            port_identities: &port_identities,
            prepared_quotient: None,
            edge_direction_evidence: None,
            budget: &budget,
            partial_solution_valid: None,
            complete_solution_valid: None,
            candidate_gauge: None,
            priority_edges: None,
        },
    )
    .map(MeshSolve::into_option)
}

pub(super) fn singleton_mesh_boundary_directions(
    ctx: &DecodeContext<'_>,
    boundary: &[MeshBoundaryEdgeCandidate],
    edge_candidates: &[Vec<[usize; 2]>],
    edge_direction_evidence: Option<&[bool]>,
) -> Result<Option<Vec<bool>>, CodecError> {
    if boundary.is_empty() {
        return Ok(None);
    }
    let first = boundary[0];
    let Some(first_pair) = edge_candidates
        .get(first.edge)
        .and_then(|candidates| candidates.first())
        .copied()
    else {
        return Ok(None);
    };
    let first_required = first.reversed.filter(|_| {
        edge_direction_evidence
            .and_then(|evidence| evidence.get(first.edge))
            .copied()
            .unwrap_or(false)
    });
    let mut solutions = Vec::new();
    for first_direction in [false, true] {
        if first_required.is_some_and(|required| required != first_direction)
            || (first_required.is_none() && first_pair[0] == first_pair[1] && first_direction)
        {
            continue;
        }
        let first_start = if first_direction {
            first_pair[1]
        } else {
            first_pair[0]
        };
        let mut current = if first_direction {
            first_pair[0]
        } else {
            first_pair[1]
        };
        let mut directions = Vec::new();
        ctx.push_vec(
            &mut directions,
            first_direction,
            "catia_singleton_initial_direction",
        )?;
        let mut valid = true;
        for use_ in &boundary[1..] {
            ctx.charge_work(1, "catia_singleton_direction_step")?;
            let Some(pair) = edge_candidates
                .get(use_.edge)
                .and_then(|candidates| candidates.first())
                .copied()
            else {
                return Ok(None);
            };
            let required = use_.reversed.filter(|_| {
                edge_direction_evidence
                    .and_then(|evidence| evidence.get(use_.edge))
                    .copied()
                    .unwrap_or(false)
            });
            let false_direction = pair[0] == current && required.is_none_or(|direction| !direction);
            let true_direction = pair[0] != pair[1]
                && pair[1] == current
                && required.is_none_or(|direction| direction);
            let direction = match (false_direction, true_direction) {
                (true, false) => false,
                (false, true) => true,
                _ => {
                    valid = false;
                    break;
                }
            };
            current = if direction { pair[0] } else { pair[1] };
            ctx.push_vec(&mut directions, direction, "catia_singleton_direction_step")?;
        }
        if valid && current == first_start {
            ctx.push_vec(
                &mut solutions,
                directions,
                "catia_singleton_direction_solutions",
            )?;
        }
    }
    ctx.sort_unstable_by(
        &mut solutions,
        |value| value,
        Ord::cmp,
        "catia_singleton_direction_solutions_sort",
    )?;
    ctx.dedup_vec(&mut solutions, "catia_singleton_direction_solutions")?;
    if solutions.len() == 2
        && ctx.all_by(
            boundary,
            |use_| {
                Ok(use_.reversed.is_none()
                    || !edge_direction_evidence
                        .and_then(|evidence| evidence.get(use_.edge))
                        .copied()
                        .unwrap_or(false))
            },
            "catia_singleton_direction_solutions",
        )?
    {
        solutions.truncate(1);
    }
    Ok((solutions.len() == 1).then(|| solutions.remove(0)))
}

pub(super) fn canonical_singleton_coordinate_cycles(
    ctx: &DecodeContext<'_>,
    assignment: &MeshFaceBoundaryAssignment,
    directions: &[Vec<bool>],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    /// The least rotation of the cycle read forward or backward.
    pub(super) fn canonical_cycle(
        ctx: &DecodeContext<'_>,
        points: &[usize],
    ) -> Result<Vec<usize>, CodecError> {
        const OPERATION: &str = "catia_singleton_canonical_cycle";
        let reversed = ctx.collect_vec(points.iter().rev().copied(), OPERATION)?;
        let rotate = |values: &[usize]| -> Result<Vec<usize>, CodecError> {
            let start = least_rotation(ctx, values, OPERATION)?;
            let mut rotated = ctx.collection_vec(values.len(), OPERATION)?;
            rotated.extend_from_slice(&values[start..]);
            rotated.extend_from_slice(&values[..start]);
            Ok(rotated)
        };
        let forward = rotate(points)?;
        let reversed = rotate(&reversed)?;
        Ok(if ctx.compare(&reversed, &forward, OPERATION)?.is_lt() {
            reversed
        } else {
            forward
        })
    }

    let mut cycles = Vec::new();
    ctx.reserve_vec(
        &mut cycles,
        assignment.boundaries.len().min(directions.len()),
        "catia_singleton_cycle_rows",
    )?;
    for (boundary, directions) in ctx
        .admit_iter(&assignment.boundaries, "catia_singleton_cycle_rows")?
        .zip(directions)
    {
        if boundary.len() != directions.len() {
            return Ok(None);
        }
        let mut points = Vec::new();
        ctx.reserve_vec(&mut points, boundary.len(), "catia_singleton_cycle_points")?;
        for (use_, &reversed) in ctx
            .admit_iter(boundary, "catia_singleton_cycle_points")?
            .zip(directions)
        {
            let Some(pair) = edge_candidates
                .get(use_.edge)
                .and_then(|candidates| candidates.first())
                .copied()
            else {
                return Ok(None);
            };
            points.push(if reversed { pair[1] } else { pair[0] });
        }
        cycles.push(canonical_cycle(ctx, &points)?);
    }
    ctx.sort_unstable_by(
        &mut cycles,
        |value| value,
        Ord::cmp,
        "catia_singleton_cycle_rows_sort",
    )?;
    Ok(Some(cycles))
}

pub(super) fn reconstruct_singleton_coordinate_topology(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    directions: &[Vec<Vec<bool>>],
) -> Result<Option<StandardTopologyDraft>, CodecError> {
    if selected.len() != directions.len() {
        return Ok(None);
    }
    let mut faces = Vec::new();
    ctx.reserve_vec(&mut faces, selected.len(), "catia_singleton_topology_faces")?;
    for (assignment, face_directions) in ctx
        .admit_iter(selected, "catia_singleton_topology_faces")?
        .zip(directions)
    {
        let mut boundaries = Vec::new();
        ctx.reserve_vec(
            &mut boundaries,
            assignment.boundaries.len().min(face_directions.len()),
            "catia_singleton_topology_boundaries",
        )?;
        for (boundary, directions) in ctx
            .admit_iter(
                &assignment.boundaries,
                "catia_singleton_topology_boundaries",
            )?
            .zip(face_directions)
        {
            if boundary.len() != directions.len() || boundary.is_empty() {
                return Ok(None);
            }
            let mut coedges = Vec::new();
            ctx.reserve_vec(
                &mut coedges,
                boundary.len(),
                "catia_singleton_topology_coedges",
            )?;
            for (use_, &reversed) in ctx
                .admit_iter(boundary, "catia_singleton_topology_coedges")?
                .zip(directions)
            {
                let Some(pair) = edge_candidates
                    .get(use_.edge)
                    .and_then(|candidates| candidates.first())
                    .copied()
                else {
                    return Ok(None);
                };
                let [start_vertex, end_vertex] = if reversed { [pair[1], pair[0]] } else { pair };
                coedges.push(CoedgeUse {
                    edge_row: use_.edge,
                    reversed,
                    start_vertex,
                    end_vertex,
                });
            }
            let Some(boundary) = BoundaryDraft::new(coedges) else {
                return Ok(None);
            };
            boundaries.push(boundary);
        }
        faces.push(FaceTopologyDraft { boundaries });
    }
    let topology = StandardTopologyDraft {
        faces,
        edge_rows: copy_mesh_edge_rows(ctx, edge_rows)?,
        vertex_points: ctx.copy_slice(vertex_points, "catia_singleton_topology_points")?,
        logical_vertex_count: vertex_points.len(),
    };
    let Some(_) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    Ok(Some(topology))
}

pub(super) fn resolve_mesh_selection_from_quotient<'storage>(
    ctx: &'storage DecodeContext<'_>,
    topology: StandardTopologyDraft,
    mut quotient: MeshQuotient<'storage>,
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
) -> Result<Option<MeshEndpointResolve>, CodecError> {
    let Some(port_count) = edge_candidates.len().checked_mul(2) else {
        return Ok(None);
    };
    if quotient.union.len() != port_count
        || port_identities.len() != edge_candidates.len()
        || quotient.root_count(ctx)? != vertex_points.len()
    {
        return Ok(None);
    }
    let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    if edge_vertices.len() != edge_candidates.len() {
        return Ok(None);
    }
    // The direct path is a fast path only for a unique coordinate matching.
    // Non-unique matchings defer to the full search, which applies the mesh gauge.
    let Some(root_points) =
        quotient.point_assignment(ctx, vertex_points.len(), edge_candidates, Some(budget))?
    else {
        return Ok(None);
    };
    let mut point_assignment = ctx.alloc_filled(
        topology.logical_vertex_count,
        None,
        "catia_merged_mesh_point_assignment",
    )?;
    let mut points_by_identity = HashMap::<u32, usize>::new();
    for (edge, &[start, end]) in ctx
        .admit_iter(&edge_vertices, "catia_merged_mesh_point_assignment")?
        .enumerate()
    {
        let mut points = [0; 2];
        for (port, vertex) in [start, end].into_iter().enumerate() {
            let root = quotient.union.find(ctx, edge * 2 + port)?;
            let Some(&point) =
                ctx.get_hash_map(&root_points, &root, "catia_merged_mesh_point_assignment")?
            else {
                return Ok(None);
            };
            match point_assignment[vertex] {
                Some(stored) if stored != point => return Ok(None),
                Some(_) => {}
                None => point_assignment[vertex] = Some(point),
            }
            points[port] = point;
        }
        let closed_ports =
            quotient.union.find(ctx, edge * 2)? == quotient.union.find(ctx, edge * 2 + 1)?;
        if !mesh_edge_points_compatible(ctx, closed_ports, &edge_candidates[edge], points)? {
            return Ok(None);
        }
        for (identity, point) in port_identities[edge].into_iter().zip(points) {
            match ctx.insert_hash_map(
                &mut points_by_identity,
                identity,
                point,
                "catia_merged_mesh_identity_points",
            )? {
                Some(previous) if previous != point => return Ok(None),
                _ => {}
            }
        }
    }
    let mut completed =
        ctx.collection_vec(point_assignment.len(), "catia_merged_mesh_completed_points")?;
    for point in ctx.admit_iter(point_assignment, "catia_merged_mesh_completed_points")? {
        let Some(point) = point else {
            return Ok(None);
        };
        completed.push(point);
    }
    Ok(Some(MeshSolve::Solved((topology, completed))))
}

/// A complete assignment of distinct, in-range point indices.
struct DistinctAssignment {
    points: Vec<usize>,
}

impl DistinctAssignment {
    fn new(
        ctx: &DecodeContext<'_>,
        points: Vec<usize>,
        point_count: usize,
    ) -> Result<Option<Self>, CodecError> {
        let (mut seen, _storage) = ctx.temporary_vec(point_count, "catia_distinct_assignment")?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(point_count),
            "catia_distinct_assignment",
        )?;
        seen.resize(point_count, false);
        for &point in &points {
            ctx.charge_work(1, "catia_distinct_assignment")?;
            let Some(used) = seen.get_mut(point) else {
                return Ok(None);
            };
            if *used {
                return Ok(None);
            }
            *used = true;
        }
        Ok(Some(Self { points }))
    }
}

fn reduced_distinct_matching(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<usize>],
    point_count: usize,
    budget: &WorkBudget<'_>,
    excluded: Option<(usize, usize)>,
) -> Result<Option<DistinctAssignment>, CodecError> {
    let mut assignment = ctx.alloc_filled(domains.len(), None, "catia_reduced_matching")?;
    let mut used = ctx.alloc_filled(point_count, false, "catia_reduced_matching_used")?;
    let mut remaining = Vec::new();
    for (root, domain) in ctx
        .admit_iter(domains, "catia_reduced_matching")?
        .enumerate()
    {
        if domain.len() == 1 {
            let point = domain[0];
            if point >= point_count {
                return Ok(None);
            }
            if excluded.is_some_and(|(excluded_root, excluded_point)| {
                excluded_root == root && excluded_point == point
            }) || used[point]
            {
                return Ok(None);
            }
            used[point] = true;
            assignment[root] = Some(point);
        }
    }
    for (root, domain) in ctx
        .admit_iter(domains, "catia_reduced_matching_remaining_rows")?
        .enumerate()
    {
        if domain.len() == 1 {
            continue;
        }
        let mut values = Vec::new();
        for &point in ctx.admit_iter(domain, "catia_reduced_matching_domain_values")? {
            if point >= point_count {
                return Ok(None);
            }
            if !used[point]
                && excluded.is_none_or(|(excluded_root, excluded_point)| {
                    excluded_root != root || excluded_point != point
                })
            {
                ctx.push_vec(&mut values, point, "catia_reduced_matching_domain_values")?;
            }
        }
        if values.is_empty() {
            return Ok(None);
        }
        ctx.push_vec(
            &mut remaining,
            (root, values),
            "catia_reduced_matching_remaining_rows",
        )?;
    }
    let remaining_domains = ctx.collect_vec(
        remaining.iter().map(|(_, domain)| domain.as_slice()),
        "catia_reduced_matching_domain_refs",
    )?;
    let Some(matching) = distinct_domain_matching_with_budget(
        ctx,
        remaining_domains,
        point_count,
        Some(budget),
        None,
    )?
    else {
        return Ok(None);
    };
    for ((root, _), point) in ctx
        .admit_iter(remaining, "catia_reduced_matching")?
        .zip(matching)
    {
        assignment[root] = Some(point);
    }
    let mut completed = ctx.collection_vec(assignment.len(), "catia_reduced_matching_completed")?;
    for point in ctx.admit_iter(assignment, "catia_reduced_matching_completed")? {
        let Some(point) = point else {
            return Ok(None);
        };
        completed.push(point);
    }
    DistinctAssignment::new(ctx, completed, point_count)
}

// The selection owns the complete quotient inputs and the optional gauge. The
// explicit signature keeps the two bounded materialization paths symmetric.
#[derive(Clone, Copy)]
pub(super) struct ResolveSingletonMeshSelectionInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input6,
    'input7,
    'input8,
> {
    pub(super) edge_rows: &'input0 [EdgeRow],
    pub(super) vertex_points: &'input1 [[f64; 3]],
    pub(super) edge_candidates: &'input2 [Vec<[usize; 2]>],
    pub(super) selected: &'input3 [MeshFaceBoundaryAssignment],
    pub(super) directions: &'input4 [Vec<Vec<bool>>],
    pub(super) port_identities: &'input5 [[u32; 2]],
    pub(super) budget: &'input7 WorkBudget<'input6>,
    pub(super) candidate_gauge: Option<MeshCandidateGauge<'input8>>,
}

pub(super) fn resolve_singleton_mesh_selection(
    ctx: &DecodeContext<'_>,
    inputs: ResolveSingletonMeshSelectionInputs<'_, '_, '_, '_, '_, '_, '_, '_, '_>,
) -> Result<Option<MeshEndpointResolve>, CodecError> {
    let ResolveSingletonMeshSelectionInputs {
        edge_rows,
        vertex_points,
        edge_candidates,
        selected,
        directions,
        port_identities,
        budget,
        candidate_gauge,
    } = inputs;

    if selected.len() != directions.len()
        || edge_candidates.len() != edge_rows.len()
        || port_identities.len() != edge_rows.len()
    {
        return Ok(None);
    }
    let Some(topology) =
        reconstruct_mesh_selection(ctx, edge_rows, vertex_points, selected, directions)?
    else {
        return Ok(None);
    };
    let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    let Some(mut quotient) =
        initial_mesh_quotient(ctx, edge_candidates, vertex_points.len(), port_identities)?
    else {
        return Ok(None);
    };
    let mut port_by_vertex = HashMap::<usize, usize>::new();
    for (edge, &[start, end]) in ctx
        .admit_iter(&edge_vertices, "catia_mesh_port_by_vertex")?
        .enumerate()
    {
        for (port, vertex) in [(0, start), (1, end)] {
            let node = edge * 2 + port;
            if let Some(previous) = ctx.insert_hash_map(
                &mut port_by_vertex,
                vertex,
                node,
                "catia_mesh_port_by_vertex",
            )? {
                if quotient.merge_charged(ctx, previous, node)?.is_none() {
                    return Ok(None);
                }
            }
        }
    }
    for (edge, candidates) in ctx
        .admit_iter(edge_candidates, "catia_singleton_root_domain_copy")?
        .enumerate()
    {
        let &[[left_point, right_point]] = candidates.as_slice() else {
            return Ok(None);
        };
        let left_root = quotient.union.find(ctx, edge * 2)?;
        let right_root = quotient.union.find(ctx, edge * 2 + 1)?;
        if left_root == right_root && left_point != right_point {
            return Ok(None);
        }
        for root in [left_root, right_root] {
            let current = Arc::clone(&quotient.domains[root]);
            let domain = quotient.new_domain("catia_singleton_root_domain_copy", || {
                let mut domain = Vec::new();
                for point in [left_point.min(right_point), left_point.max(right_point)] {
                    if domain.last() != Some(&point)
                        && domain_contains(
                            ctx,
                            &current,
                            point,
                            "catia_singleton_root_domain_copy",
                        )?
                    {
                        ctx.push_vec(&mut domain, point, "catia_singleton_root_domain_copy")?;
                    }
                }
                Ok(domain)
            })?;
            if domain.is_empty() {
                return Ok(None);
            }
            quotient.domains[root] = domain;
        }
    }
    let mut roots = Vec::new();
    let mut root_indices =
        ctx.alloc_filled(quotient.union.len(), None, "catia_singleton_root_indices")?;
    for node in ctx.admit_iter(0..quotient.union.len(), "catia_singleton_root_rows")? {
        if quotient.union.find(ctx, node)? == node {
            root_indices[node] = Some(roots.len());
            ctx.push_vec(&mut roots, node, "catia_singleton_root_rows")?;
        }
    }
    if roots.len() != vertex_points.len() {
        return Ok(None);
    }
    let mut domain_values = ctx.collection_vec(roots.len(), "catia_singleton_domain_rows")?;
    for &root in ctx.admit_iter(&roots, "catia_singleton_domain_rows")? {
        let domain = &quotient.domains[root];
        if domain.is_empty() {
            return Ok(None);
        }
        domain_values.push(ctx.copy_slice(domain, "catia_singleton_domain_values")?);
    }
    let first_assignment =
        reduced_distinct_matching(ctx, &domain_values, vertex_points.len(), budget, None)?;
    let Some(first_assignment) = first_assignment else {
        return Ok(budget
            .exhausted()
            .then_some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
    };
    let mut edge_use_counts =
        ctx.alloc_filled(edge_rows.len(), 0usize, "catia_mesh_edge_use_counts")?;
    for assignment in ctx.admit_iter(selected, "catia_mesh_edge_use_counts")? {
        for boundary in ctx.admit_iter(&assignment.boundaries, "catia_mesh_edge_use_counts")? {
            for use_ in ctx.admit_iter(boundary, "catia_mesh_edge_use_counts")? {
                let Some(count) = edge_use_counts.get_mut(use_.edge) else {
                    return Ok(None);
                };
                *count += 1;
            }
        }
    }
    if ctx.any_by(
        &edge_use_counts,
        |count| Ok(*count > 2),
        "catia_mesh_edge_use_counts",
    )? {
        return Ok(None);
    }
    let mut materialize =
        |assignment: &[usize]| -> Result<Option<(StandardTopologyDraft, Vec<usize>)>, CodecError> {
            if assignment.len() != roots.len() {
                return Ok(None);
            }
            let mut point_assignment = ctx.alloc_filled(
                topology.logical_vertex_count,
                None,
                "catia_selection_singleton_point_assignment",
            )?;
            let mut points_by_identity = HashMap::<u32, usize>::new();
            for (edge, &[start, end]) in ctx
                .admit_iter(&edge_vertices, "catia_selection_singleton_point_assignment")?
                .enumerate()
            {
                let mut points = [0; 2];
                for (port, vertex) in [start, end].into_iter().enumerate() {
                    let root = quotient.union.find(ctx, edge * 2 + port)?;
                    let Some(root) = root_indices[root] else {
                        return Ok(None);
                    };
                    let Some(&point) = assignment.get(root) else {
                        return Ok(None);
                    };
                    match point_assignment[vertex] {
                        Some(stored) if stored != point => return Ok(None),
                        Some(_) => {}
                        None => point_assignment[vertex] = Some(point),
                    }
                    points[port] = point;
                }
                let closed_ports = quotient.union.find(ctx, edge * 2)?
                    == quotient.union.find(ctx, edge * 2 + 1)?;
                if !mesh_edge_points_compatible(ctx, closed_ports, &edge_candidates[edge], points)?
                {
                    return Ok(None);
                }
                for (identity, point) in port_identities[edge].into_iter().zip(points) {
                    match ctx.insert_hash_map(
                        &mut points_by_identity,
                        identity,
                        point,
                        "catia_singleton_identity_points",
                    )? {
                        Some(previous) if previous != point => return Ok(None),
                        _ => {}
                    }
                }
            }
            let mut points =
                ctx.collection_vec(point_assignment.len(), "catia_singleton_completed_points")?;
            for point in ctx.admit_iter(point_assignment, "catia_singleton_completed_points")? {
                let Some(point) = point else {
                    return Ok(None);
                };
                points.push(point);
            }
            Ok(Some((topology.clone_charged(ctx)?, points)))
        };
    let Some(first) = materialize(&first_assignment.points)? else {
        return Ok(None);
    };
    let mut ambiguous_roots = Vec::new();
    for (root, domain) in ctx
        .admit_iter(&domain_values, "catia_singleton_ambiguous_roots")?
        .enumerate()
    {
        if domain.len() > 1 {
            ctx.push_vec(
                &mut ambiguous_roots,
                root,
                "catia_singleton_ambiguous_roots",
            )?;
        }
    }
    for root in ctx.admit_iter(ambiguous_roots, "catia_singleton_ambiguous_roots")? {
        let Some(alternate) = reduced_distinct_matching(
            ctx,
            &domain_values,
            vertex_points.len(),
            budget,
            Some((root, first_assignment.points[root])),
        )?
        else {
            if budget.exhausted() {
                return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
            }
            continue;
        };
        let Some(alternate) = materialize(&alternate.points)? else {
            continue;
        };
        if !mesh_candidates_equivalent_with_context(ctx, &first, &alternate, candidate_gauge)? {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))));
        }
    }
    Ok(Some(MeshSolve::Solved((first.0, first.1))))
}

#[cfg(test)]
mod reduced_matching_tests {
    use super::{reduced_distinct_matching, DistinctAssignment};

    #[test]
    fn distinct_assignment_initialization_refuses_work() {
        crate::test_support::with_work_limit(0, |ctx| {
            let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
                super::DistinctAssignment::new(ctx, vec![], 64)
            else {
                panic!("resource refusal required")
            };
            assert_eq!(limit.operation, "catia_distinct_assignment");
            assert_eq!(limit.additional, 64);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }

    #[test]
    fn reduced_matching_reserves_later_singletons_before_domains() {
        crate::test_support::with_service_context(|ctx| {
            let budget = ctx.work_budget(100);
            let domains = [vec![0, 1], vec![0]];
            let assignment = reduced_distinct_matching(ctx, &domains, 2, &budget, None)
                .expect("admitted matching")
                .expect("distinct solution");
            assert_eq!(assignment.points, [1, 0]);
            assert!(
                reduced_distinct_matching(ctx, &domains, 2, &budget, Some((0, 1)))
                    .expect("admitted exclusion")
                    .is_none()
            );
        });
    }

    #[test]
    fn reduced_matching_rejects_duplicate_and_out_of_range_assignments() {
        crate::test_support::with_service_context(|ctx| {
            assert!(DistinctAssignment::new(ctx, vec![0, 0], 2)
                .expect("admitted")
                .is_none());
            assert!(DistinctAssignment::new(ctx, vec![2], 2)
                .expect("admitted")
                .is_none());
            let budget = ctx.work_budget(100);
            assert!(reduced_distinct_matching(ctx, &[vec![2]], 2, &budget, None)
                .expect("admitted")
                .is_none());
        });
    }
}
