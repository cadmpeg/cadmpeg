// SPDX-License-Identifier: Apache-2.0
//! Mesh selection search and singleton coordinate topology.

use super::{
    canonical_mesh_boundary_directions, changed_quotient_edges, common_supported_corner_equations,
    copy_mesh_assignment, copy_mesh_boundary_directions, copy_mesh_edge_rows,
    distinct_domain_matching_with_budget, initial_mesh_quotient,
    mesh_candidates_equivalent_with_context, orient_face_cycles, reconstruct_mesh_selection,
    same_unordered_pair, Arc, Boundary, CodecError, CoedgeUse, DecodeContext, EdgeRow,
    FaceTopology, HashMap, HashSet, MeshBoundaryEdgeCandidate, MeshCandidateFailure,
    MeshCandidateGauge, MeshEndpointResolve, MeshFaceBoundaryAssignment, MeshFaceSelection,
    MeshFixedDirectionOption, MeshQuotient, MeshQuotientSignature, MeshSelectionSearch,
    MeshSelectionStateSignature, MeshSolve, SearchOutcome, StandardTopology, VecDeque, WorkBudget,
    MAX_FACE_EQUATION_CACHE_ENTRIES, MAX_SELECTION_STATE_MEMO_ENTRIES,
};

#[cfg(test)]
use super::{
    deduplicate_mesh_quotient_assignments, domains_have_distinct_matching, largest_fbb_run,
    parse_edge_tables, parse_vertex_table, required_component_roots,
    resolve_standard_mesh_endpoint_candidates, standard_mesh_boundary_assignments, NonZeroUsize,
    UnionFind, MAX_MESH_CONSTRAINT_OPERATIONS,
};

impl MeshSelectionSearch<'_, '_> {
    pub(super) fn should_stop(&self) -> bool {
        self.outcome.is_closed()
    }

    pub(super) fn has_exact_singleton_endpoint_domains(&self) -> bool {
        self.port_identities.is_some()
            && self
                .edge_candidates
                .iter()
                .all(|candidates| candidates.len() == 1)
    }

    pub(super) fn selected_edges(&self) -> Result<HashSet<usize>, CodecError> {
        let mut edges = HashSet::new();
        for (face, selected) in self.selected.iter().enumerate() {
            let Some((index, _)) = selected else {
                continue;
            };
            if let Some(assignment) = self.assignments[face].get(*index) {
                for use_ in assignment.boundaries.iter().flatten() {
                    crate::resource::insert_set(
                        self.ctx,
                        &mut edges,
                        use_.edge,
                        "catia_selection_selected_edges",
                    )?;
                }
            }
        }
        Ok(edges)
    }

    #[cfg(test)]
    pub(super) fn remaining_equation_merge_capacity(
        &self,
        quotient: &mut MeshQuotient,
    ) -> Result<Option<usize>, CodecError> {
        fn choice_component_reductions(
            choice: &[[usize; 2]],
            quotient: &mut MeshQuotient,
            possible: &mut UnionFind,
        ) -> HashMap<usize, usize> {
            let mut equations = HashMap::<usize, Vec<[usize; 2]>>::new();
            for [left, right] in choice {
                let left = quotient.union.find(*left);
                let right = quotient.union.find(*right);
                let component = possible.find(left);
                if component == possible.find(right) {
                    equations.entry(component).or_default().push([left, right]);
                }
            }
            equations
                .into_iter()
                .map(|(component, equations)| {
                    let mut roots = HashMap::new();
                    for [left, right] in &equations {
                        for root in [left, right] {
                            let next = roots.len();
                            roots.entry(*root).or_insert(next);
                        }
                    }
                    let mut local = UnionFind::new(roots.len());
                    for [left, right] in equations {
                        local.union(roots[&left], roots[&right]);
                    }
                    let remaining = (0..local.len())
                        .filter(|&node| local.find(node) == node)
                        .count();
                    (component, roots.len().saturating_sub(remaining))
                })
                .collect()
        }

        let node_count = quotient.union.len();
        let mut possible = UnionFind::new(node_count);
        for node in 0..node_count {
            let root = quotient.union.find(node);
            possible.union(node, root);
        }
        let before = (0..node_count)
            .filter(|&node| possible.find(node) == node)
            .count();
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_some() {
                continue;
            }
            for [left, right] in &self.possible_face_equations[face] {
                possible.union(*left, *right);
            }
        }
        let after = (0..node_count)
            .filter(|&node| possible.find(node) == node)
            .count();
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
            if quotient.union.find(node) != node {
                continue;
            }
            let component = possible.find(node);
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
                let reductions = choice_component_reductions(choice, quotient, &mut possible);
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
            independent_capacity = independent_capacity.saturating_add(independent_face_capacity);
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
        let universal_required = universal_components
            .iter()
            .map(required_count)
            .fold(0usize, usize::saturating_add);
        let mut domains = possible_domains
            .into_iter()
            .flat_map(|(component, domain)| {
                let required = required_count(&component);
                std::iter::repeat_n(domain, required)
            })
            .collect::<Vec<_>>();
        if universal_required > point_count.saturating_sub(domains.len()) {
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
            if quotient.union.find(node) != node || quotient.domains[node].len() != 1 {
                continue;
            }
            let Some(point) = quotient.domains[node].iter().next() else {
                return Ok(None);
            };
            let point = *point;
            let component = possible.find(node);
            if singleton_component
                .insert(point, component)
                .is_some_and(|previous| previous != component)
            {
                return Ok(None);
            }
        }
        Ok(Some(before.saturating_sub(after).min(independent_capacity)))
    }

    pub(super) fn face_projection_signature(
        &self,
        face: usize,
        quotient: &mut MeshQuotient,
    ) -> Result<MeshQuotientSignature, CodecError> {
        let mut root_set = HashSet::new();
        for use_ in self.assignments[face]
            .iter()
            .flat_map(|assignment| &assignment.boundaries)
            .flatten()
        {
            for node in [use_.edge * 2, use_.edge * 2 + 1] {
                crate::resource::insert_set(
                    self.ctx,
                    &mut root_set,
                    quotient.union.find(node),
                    "catia_face_projection_roots",
                )?;
            }
        }
        let mut roots = Vec::new();
        crate::resource::reserve_vec(
            self.ctx,
            &mut roots,
            root_set.len(),
            "catia_face_projection_root_order",
        )?;
        roots.extend(root_set);
        roots.sort_unstable();
        let mut signature = Vec::new();
        crate::resource::reserve_vec(
            self.ctx,
            &mut signature,
            roots.len(),
            "catia_face_projection_signature_rows",
        )?;
        for root in roots {
            let mut domain = Vec::new();
            crate::resource::reserve_vec(
                self.ctx,
                &mut domain,
                quotient.domains[root].len(),
                "catia_face_projection_domain_points",
            )?;
            domain.extend(quotient.domains[root].iter().copied());
            domain.sort_unstable();
            signature.push((
                crate::resource::copy_slice(
                    self.ctx,
                    quotient.members(root),
                    "catia_face_projection_member_nodes",
                )?,
                domain,
            ));
        }
        signature.sort_unstable();
        Ok(signature)
    }

    #[cfg(test)]
    pub(super) fn propagate_forced_face_equations(
        &self,
        quotient: &mut MeshQuotient,
    ) -> Result<bool, CodecError> {
        let budget = WorkBudget::new(usize::MAX);
        self.propagate_forced_face_equations_from(quotient, None, &budget)
    }

    pub(super) fn propagate_forced_face_equations_from(
        &self,
        quotient: &mut MeshQuotient,
        changed_edges: Option<&HashSet<usize>>,
        budget: &WorkBudget<'_>,
    ) -> Result<bool, CodecError> {
        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        for (face, selected) in self.selected.iter().enumerate() {
            if selected.is_none()
                && changed_edges.is_none_or(|changed_edges| {
                    self.assignments[face]
                        .iter()
                        .flat_map(|assignment| &assignment.boundaries)
                        .flatten()
                        .any(|use_| changed_edges.contains(&use_.edge))
                })
            {
                crate::resource::push_back(
                    self.ctx,
                    &mut queue,
                    face,
                    "catia_forced_equation_queue",
                )?;
                crate::resource::insert_set(
                    self.ctx,
                    &mut queued,
                    face,
                    "catia_forced_equation_queued",
                )?;
            }
        }
        while let Some(face) = queue.pop_front() {
            if !budget.charge() {
                return Ok(true);
            }
            queued.remove(&face);
            if self.selected[face].is_some() {
                continue;
            }
            let before = quotient.clone_charged(self.ctx)?;
            let mut changed = false;
            let deterministic = self.assignments[face].len() == 1
                && self.assignments[face][0]
                    .boundaries
                    .iter()
                    .flatten()
                    .all(|use_| use_.reversed.is_some());
            let equations = if deterministic {
                let [choice] = self.possible_face_choices[face].as_slice() else {
                    return Ok(false);
                };
                crate::resource::copy_slice(
                    self.ctx,
                    choice,
                    "catia_forced_deterministic_equations",
                )?
            } else {
                let cache_key = (face, self.face_projection_signature(face, quotient)?);
                let cached = {
                    let cache = self.face_equation_cache.borrow();
                    cache
                        .get(&cache_key)
                        .map(|equations| {
                            crate::resource::copy_slice(
                                self.ctx,
                                equations,
                                "catia_forced_cached_equations",
                            )
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
                    let mut equations = Vec::new();
                    crate::resource::reserve_vec(
                        self.ctx,
                        &mut equations,
                        common.len(),
                        "catia_forced_common_equations",
                    )?;
                    equations.extend(common);
                    let cached_equations = crate::resource::copy_retained_slice(
                        self.ctx,
                        &equations,
                        "catia_forced_cached_equation_copy",
                    )?;
                    let mut cache = self.face_equation_cache.borrow_mut();
                    if cache.len() >= MAX_FACE_EQUATION_CACHE_ENTRIES {
                        cache.clear();
                    }
                    let Some(key_row_bytes) =
                        cache_key
                            .1
                            .len()
                            .checked_mul(std::mem::size_of::<(Vec<usize>, Vec<usize>)>())
                    else {
                        return Err(self.ctx.refuse_codec_limit(
                            "catia_forced_equation_cache_key",
                            u64::MAX,
                            u64::MAX,
                        ));
                    };
                    let Some(key_bytes) = cache_key
                        .1
                        .iter()
                        .try_fold(key_row_bytes, |total, (members, domain)| {
                            total
                                .checked_add(
                                    members.len().checked_mul(std::mem::size_of::<usize>())?,
                                )?
                                .checked_add(
                                    domain.len().checked_mul(std::mem::size_of::<usize>())?,
                                )
                        })
                        .map(cadmpeg_core::decode::u64_from_index)
                    else {
                        return Err(self.ctx.refuse_codec_limit(
                            "catia_forced_equation_cache_key",
                            u64::MAX,
                            u64::MAX,
                        ));
                    };
                    self.ctx
                        .charge_retained(key_bytes, "catia_forced_equation_cache_key")?;
                    crate::resource::insert_map(
                        self.ctx,
                        &mut cache,
                        cache_key,
                        cached_equations,
                        "catia_forced_equation_cache",
                    )?;
                    equations
                }
            };
            for [left, right] in equations {
                if quotient.union.find(left) == quotient.union.find(right) {
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
            for (dependent, assignments) in self.assignments.iter().enumerate() {
                if self.selected[dependent].is_none()
                    && dependent != face
                    && !queued.contains(&dependent)
                    && assignments
                        .iter()
                        .flat_map(|assignment| &assignment.boundaries)
                        .flatten()
                        .any(|use_| changed_edges.contains(&use_.edge))
                {
                    crate::resource::insert_set(
                        self.ctx,
                        &mut queued,
                        dependent,
                        "catia_forced_equation_queued",
                    )?;
                    crate::resource::push_back(
                        self.ctx,
                        &mut queue,
                        dependent,
                        "catia_forced_equation_queue",
                    )?;
                }
            }
        }
        Ok(true)
    }

    pub(super) fn selection_orientable(
        &self,
        selection: &[MeshFaceSelection],
    ) -> Result<bool, CodecError> {
        let mut constraints = Vec::<Vec<(usize, bool)>>::new();
        let mut edge_uses = HashMap::<usize, Vec<(usize, bool)>>::new();
        for (face, selected) in selection.iter().enumerate() {
            let Some((assignment_index, directions)) = selected else {
                continue;
            };
            let Some(assignment) = self.assignments[face].get(*assignment_index) else {
                return Ok(false);
            };
            if assignment.boundaries.len() != directions.len() {
                return Ok(false);
            }
            for (boundary, directions) in assignment.boundaries.iter().zip(directions) {
                if boundary.len() != directions.len() {
                    return Ok(false);
                }
                let node = constraints.len();
                crate::resource::push(
                    self.ctx,
                    &mut constraints,
                    Vec::new(),
                    "catia_selection_constraint_nodes",
                )?;
                for (use_, &direction) in boundary.iter().zip(directions) {
                    let reversed = use_.reversed.unwrap_or(direction);
                    if use_.reversed.is_some() && reversed != direction {
                        return Ok(false);
                    }
                    crate::resource::admit_map_entry(
                        self.ctx,
                        &mut edge_uses,
                        &use_.edge,
                        "catia_selection_edge_keys",
                    )?;
                    let uses = edge_uses.entry(use_.edge).or_default();
                    if uses.len() == 2 {
                        return Ok(false);
                    }
                    crate::resource::push(
                        self.ctx,
                        uses,
                        (node, reversed),
                        "catia_selection_edge_uses",
                    )?;
                }
            }
        }
        for uses in edge_uses.values() {
            let [(left_node, left_reversed), (right_node, right_reversed)] = uses.as_slice() else {
                continue;
            };
            let parity = left_reversed == right_reversed;
            if left_node == right_node {
                if parity {
                    return Ok(false);
                }
            } else {
                crate::resource::push(
                    self.ctx,
                    &mut constraints[*left_node],
                    (*right_node, parity),
                    "catia_selection_adjacent_constraints",
                )?;
                crate::resource::push(
                    self.ctx,
                    &mut constraints[*right_node],
                    (*left_node, parity),
                    "catia_selection_adjacent_constraints",
                )?;
            }
        }
        let mut flips = self
            .ctx
            .alloc_filled(constraints.len(), None, "catia_selection_flips")?;
        for root in 0..constraints.len() {
            if flips[root].is_some() {
                continue;
            }
            flips[root] = Some(false);
            let mut stack = Vec::new();
            crate::resource::push(
                self.ctx,
                &mut stack,
                root,
                "catia_selection_orientation_stack",
            )?;
            while let Some(node) = stack.pop() {
                self.ctx
                    .charge_work(1, "catia_selection_orientation_work")?;
                let Some(flip) = flips[node] else {
                    return Ok(false);
                };
                for &(neighbor, parity) in &constraints[node] {
                    let required = flip ^ parity;
                    match flips[neighbor] {
                        Some(existing) if existing != required => return Ok(false),
                        Some(_) => {}
                        None => {
                            flips[neighbor] = Some(required);
                            crate::resource::push(
                                self.ctx,
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
        let mut completion = Vec::new();
        crate::resource::reserve_vec(
            self.ctx,
            &mut completion,
            self.selected.len(),
            "catia_selection_completion",
        )?;
        for selected in &self.selected {
            completion.push(match selected {
                Some((index, directions)) => {
                    Some((*index, copy_mesh_boundary_directions(self.ctx, directions)?))
                }
                None => None,
            });
        }
        for (face, selected) in completion.iter_mut().enumerate() {
            if selected.is_some() {
                continue;
            }
            let [assignment] = self.assignments[face].as_slice() else {
                continue;
            };
            self.ctx.charge_collection_items(
                cadmpeg_core::decode::u64_from_index(assignment.boundaries.len()),
                "catia_selection_completion_boundaries",
            )?;
            for boundary in &assignment.boundaries {
                self.ctx.charge_collection_items(
                    cadmpeg_core::decode::u64_from_index(boundary.len()),
                    "catia_selection_completion_directions",
                )?;
            }
            let mut directions = Vec::new();
            crate::resource::reserve_admitted_vec(
                &mut directions,
                assignment.boundaries.len(),
                "catia_selection_completion_boundaries",
            )?;
            let mut complete = true;
            for boundary in &assignment.boundaries {
                if boundary.iter().any(|use_| use_.reversed.is_none()) {
                    complete = false;
                    break;
                }
                let mut row = Vec::new();
                crate::resource::reserve_admitted_vec(
                    &mut row,
                    boundary.len(),
                    "catia_selection_completion_directions",
                )?;
                for use_ in boundary {
                    if let Some(reversed) = use_.reversed {
                        row.push(reversed);
                    }
                }
                directions.push(row);
            }
            if !complete {
                continue;
            }
            *selected = Some((0, directions));
        }
        self.selection_orientable(&completion)
    }

    pub(super) fn prepare_selected_branch(
        &self,
        quotient: &MeshQuotient,
        changed_edges: &HashSet<usize>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<Option<MeshQuotient>, CodecError> {
        let mut measured = quotient.clone_charged(self.ctx)?;
        if !self.has_exact_singleton_endpoint_domains()
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
        let root_count = measured.root_count();
        if root_count < self.vertex_points.len() {
            return Ok(None);
        }
        if root_count == self.vertex_points.len()
            && !self.has_exact_singleton_endpoint_domains()
            && !measured.point_assignment_exists(
                self.ctx,
                self.vertex_points.len(),
                self.edge_candidates,
                Some(propagation_budget),
            )?
        {
            return Ok(None);
        }
        let orientable = if self.has_exact_singleton_endpoint_domains() {
            true
        } else {
            self.fixed_remaining_faces_are_orientable()?
        };
        Ok(orientable.then_some(measured))
    }

    #[cfg(test)]
    pub(crate) fn search(&mut self, quotient: &MeshQuotient) -> Result<(), CodecError> {
        self.search_with_limit(quotient, MAX_MESH_CONSTRAINT_OPERATIONS)
    }

    pub(super) fn search_with_budget(
        &mut self,
        quotient: &MeshQuotient,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        self.search_from_state(quotient, false, budget, propagation_budget)
    }

    pub(super) fn fixed_direction_options(
        &self,
        measured: &MeshQuotient,
        face: usize,
        budget: Option<&WorkBudget<'_>>,
    ) -> Result<Vec<MeshFixedDirectionOption>, CodecError> {
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
        let mut seen = HashSet::<(Vec<Vec<bool>>, Vec<Option<bool>>)>::new();
        let mut output = Vec::new();
        for label_directions in direction_options {
            let mut next_orientations = crate::resource::copy_slice(
                self.ctx,
                &self.fixed_edge_orientations,
                "catia_fixed_next_orientations",
            )?;
            let constrained = assignment
                .boundaries
                .iter()
                .zip(label_directions)
                .flat_map(|(boundary, directions)| boundary.iter().zip(directions))
                .all(|(use_, &label_direction)| {
                    let Some(required) = use_.reversed else {
                        return true;
                    };
                    let Some(orientation) = next_orientations.get_mut(use_.edge) else {
                        return false;
                    };
                    let required_orientation = required ^ label_direction;
                    match *orientation {
                        Some(existing) => existing == required_orientation,
                        None => {
                            *orientation = Some(required_orientation);
                            true
                        }
                    }
                });
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
                crate::resource::copy_slice(
                    self.ctx,
                    &next_orientations,
                    "catia_fixed_orientation_signature",
                )?,
            );
            if crate::resource::insert_set(
                self.ctx,
                &mut seen,
                signature,
                "catia_fixed_direction_signatures",
            )? {
                crate::resource::push(
                    self.ctx,
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
        quotient: &MeshQuotient,
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
        if measured.root_count() < self.vertex_points.len() {
            return Ok(());
        }
        if self.visited_states.len() < MAX_SELECTION_STATE_MEMO_ENTRIES {
            let signature = self.selection_state_signature(&measured, true)?;
            if !crate::resource::insert_set(
                self.ctx,
                &mut self.visited_states,
                signature,
                "catia_selection_state_memo",
            )? {
                return Ok(());
            }
        }
        let selected_edges = self.selected_edges()?;
        let mut impossible = false;
        let face = self
            .selected
            .iter()
            .enumerate()
            .filter(|(_, selected)| selected.is_none())
            .filter_map(|(face, _)| {
                let directions = self.fixed_face_directions.get(face)?.as_ref()?;
                let [assignment] = self.assignments[face].as_slice() else {
                    return None;
                };
                let ready = assignment.boundaries.iter().flatten().all(|use_| {
                    self.fixed_edge_orientations
                        .get(use_.edge)
                        .and_then(Option::as_ref)
                        .is_some()
                        || use_.reversed.is_some()
                        || !self
                            .edge_has_fixed_direction
                            .get(use_.edge)
                            .copied()
                            .unwrap_or(false)
                });
                if !ready {
                    return None;
                }
                let local_fixed = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .filter(|use_| use_.reversed.is_some())
                    .count();
                let adjacent = assignment
                    .boundaries
                    .iter()
                    .flatten()
                    .any(|use_| selected_edges.contains(&use_.edge));
                // The exact quotient options are generated once for the face
                // selected below. This count only orders the search; probing
                // every face here would construct and hash the same large
                // quotient states a second time.
                let viable_options = directions.len();
                if viable_options == 0 {
                    impossible = true;
                    return None;
                }
                let use_count = assignment.boundaries.iter().map(Vec::len).sum::<usize>();
                Some((
                    (
                        !adjacent,
                        viable_options,
                        local_fixed == 0,
                        usize::MAX.saturating_sub(use_count),
                        directions.len(),
                        face,
                    ),
                    face,
                ))
            })
            .min_by_key(|(key, _)| *key)
            .map(|(_, face)| face);
        if impossible {
            return Ok(());
        }
        let Some(face) = face else {
            if let Some(edge) =
                self.edge_has_fixed_direction
                    .iter()
                    .enumerate()
                    .find_map(|(edge, has_fixed)| {
                        (*has_fixed
                            && self
                                .fixed_edge_orientations
                                .get(edge)
                                .is_some_and(Option::is_none))
                        .then_some(edge)
                    })
            {
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
            crate::resource::reserve_vec(
                self.ctx,
                &mut selected_assignments,
                self.selected.len(),
                "catia_fixed_selected_assignments",
            )?;
            crate::resource::reserve_vec(
                self.ctx,
                &mut directions,
                self.selected.len(),
                "catia_fixed_selected_directions",
            )?;
            for (face, selected) in self.selected.iter().enumerate() {
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
            let Some(outcome) = resolve_singleton_mesh_selection(
                self.ctx,
                self.edge_rows,
                self.vertex_points,
                self.edge_candidates,
                &selected_assignments,
                &directions,
                port_identities,
                budget,
                self.candidate_gauge,
            )?
            else {
                return Ok(());
            };
            match outcome {
                MeshSolve::Solved((topology, assignment)) => {
                    let candidate = (topology, assignment);
                    let gauge = self.candidate_gauge;
                    let equivalent = if let SearchOutcome::Solved(previous) = &self.outcome {
                        previous == &candidate
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
        for (directions, next_quotient, next_orientations) in options {
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
        quotient: &MeshQuotient,
        limit: usize,
    ) -> Result<(), CodecError> {
        let budget = WorkBudget::new(limit);
        let propagation_budget = WorkBudget::new(limit);
        self.search_from_state(quotient, false, &budget, &propagation_budget)
    }

    pub(super) fn selection_state_signature(
        &self,
        quotient: &MeshQuotient,
        prepared: bool,
    ) -> Result<MeshSelectionStateSignature, CodecError> {
        let mut quotient = quotient.clone_charged(self.ctx)?;
        let mut selected = Vec::new();
        for choice in &self.selected {
            let copy = match choice {
                Some((assignment, directions)) => Some((
                    *assignment,
                    copy_mesh_boundary_directions(self.ctx, directions)?,
                )),
                None => None,
            };
            crate::resource::push(
                self.ctx,
                &mut selected,
                copy,
                "catia_selection_signature_faces",
            )?;
        }
        Ok((
            prepared,
            selected,
            quotient.signature_charged(self.ctx)?,
            crate::resource::copy_slice(
                self.ctx,
                &self.fixed_edge_orientations,
                "catia_selection_signature_edge_orientations",
            )?,
        ))
    }

    pub(super) fn search_from_state(
        &mut self,
        quotient: &MeshQuotient,
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
            if !crate::resource::insert_set(
                self.ctx,
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
        quotient: &MeshQuotient,
        prepared: bool,
        budget: &WorkBudget<'_>,
        propagation_budget: &WorkBudget<'_>,
    ) -> Result<(), CodecError> {
        let mut measured = quotient.clone_charged(self.ctx)?;
        if !prepared {
            if !self.has_exact_singleton_endpoint_domains()
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
            let root_count = measured.root_count();
            if root_count < self.vertex_points.len() {
                return Ok(());
            }
            if !self.has_exact_singleton_endpoint_domains()
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
            if !self.has_exact_singleton_endpoint_domains()
                && !self.fixed_remaining_faces_are_orientable()?
            {
                return Ok(());
            }
        }
        let selected_edges = self.selected_edges()?;
        let mut adjacent_faces = HashSet::new();
        if !selected_edges.is_empty() {
            for (face, selected) in self.selected.iter().enumerate() {
                if selected.is_none()
                    && self.assignments[face]
                        .iter()
                        .flat_map(|assignment| &assignment.boundaries)
                        .flatten()
                        .any(|use_| selected_edges.contains(&use_.edge))
                {
                    crate::resource::insert_set(
                        self.ctx,
                        &mut adjacent_faces,
                        face,
                        "catia_selection_adjacent_faces",
                    )?;
                }
            }
        }
        let adjacent_faces = (!adjacent_faces.is_empty()).then_some(adjacent_faces);
        let next = self
            .selected
            .iter()
            .enumerate()
            .filter(|(_, selected)| selected.is_none())
            .filter(|(face, _)| {
                adjacent_faces
                    .as_ref()
                    .is_none_or(|adjacent| adjacent.contains(face))
            })
            .filter_map(|(face, _)| {
                if !budget.charge() {
                    return None;
                }
                self.face_work[face]?;
                let assignments = &self.assignments[face];
                if assignments.is_empty() {
                    return Some((0, 0, 0, 0, 0, face));
                }
                let direction_work =
                    direction_work_estimate(assignments.iter().map(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| use_.reversed.is_none())
                            .count()
                    }));
                let Some(direction_work) = direction_work else {
                    // The face states more direction choices than the work
                    // counter can hold, so no search over it can finish.
                    budget.exhaust();
                    return None;
                };
                let can_merge = assignments
                    .iter()
                    .any(|assignment| mesh_assignment_can_merge(assignment, &mut measured));
                let selected_incidence = assignments
                    .iter()
                    .map(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| selected_edges.contains(&use_.edge))
                            .count()
                    })
                    .max()
                    .unwrap_or_default();
                let constrained = assignments
                    .iter()
                    .map(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .filter(|use_| {
                                let left = measured.union.find(use_.edge * 2);
                                let right = measured.union.find(use_.edge * 2 + 1);
                                measured.domains[left].len() < self.vertex_points.len()
                                    || measured.domains[right].len() < self.vertex_points.len()
                            })
                            .count()
                    })
                    .max()
                    .unwrap_or_default();
                Some((
                    if can_merge { 1 } else { 2 },
                    direction_work,
                    assignments.len(),
                    usize::MAX - selected_incidence,
                    usize::MAX - constrained,
                    face,
                ))
            })
            .min();
        if budget.exhausted() {
            self.outcome.exhaust();
            return Ok(());
        }
        let Some((_, supported, _, _, _, face)) = next else {
            let mut selected_assignments = Vec::new();
            let mut directions = Vec::new();
            crate::resource::reserve_vec(
                self.ctx,
                &mut selected_assignments,
                self.selected.len(),
                "catia_search_selected_assignments",
            )?;
            crate::resource::reserve_vec(
                self.ctx,
                &mut directions,
                self.selected.len(),
                "catia_search_selected_directions",
            )?;
            for (face, selected) in self.selected.iter().enumerate() {
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
            if self
                .edge_candidates
                .iter()
                .all(|candidates| candidates.len() == 1)
            {
                if let Some(port_identities) = self.port_identities {
                    let outcome = resolve_singleton_mesh_selection(
                        self.ctx,
                        self.edge_rows,
                        self.vertex_points,
                        self.edge_candidates,
                        &selected_assignments,
                        &directions,
                        port_identities,
                        budget,
                        self.candidate_gauge,
                    )?;
                    if let Some(outcome) = outcome {
                        match outcome {
                            MeshSolve::Solved((topology, assignment)) => {
                                let candidate = (topology, assignment);
                                let gauge = self.candidate_gauge;
                                let equivalent =
                                    if let SearchOutcome::Solved(previous) = &self.outcome {
                                        previous == &candidate
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
                for coedge in topology
                    .faces
                    .iter()
                    .flat_map(|face| &face.boundaries)
                    .flat_map(|boundary| &boundary.coedges)
                {
                    use_counts[coedge.edge_row] += 1;
                }
                if use_counts.iter().any(|count| *count > 2) {
                    break 'candidate None;
                }
                if use_counts.iter().all(|count| *count == 2)
                    && orient_face_cycles(self.ctx, &mut topology.faces)?.is_none()
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
                for (edge, vertices) in edge_vertices.into_iter().enumerate() {
                    for (port, vertex) in vertices.into_iter().enumerate() {
                        let root = quotient.union.find(edge * 2 + port);
                        let Some(&point) = root_points.get(&root) else {
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
                    let closed_ports =
                        quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
                    if !mesh_edge_points_compatible(
                        closed_ports,
                        &self.edge_candidates[edge],
                        points,
                    ) {
                        break 'candidate None;
                    }
                }
                let mut completed_points = Vec::new();
                crate::resource::reserve_vec(
                    self.ctx,
                    &mut completed_points,
                    point_assignment.len(),
                    "catia_search_completed_points",
                )?;
                for point in point_assignment {
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
                    previous == &candidate
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
            if !budget.charge() {
                self.outcome.exhaust();
                return Ok(());
            }
            let remaining = remaining_work.saturating_sub(options.len());
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
            crate::resource::reserve_vec(
                self.ctx,
                &mut options,
                assignment_options.len(),
                "catia_search_assignment_options",
            )?;
            options.extend(
                assignment_options
                    .into_iter()
                    .map(|(directions, next_quotient)| {
                        (assignment_index, directions, next_quotient)
                    }),
            );
        }
        options.retain_mut(|(_, _, quotient)| quotient.root_count() >= self.vertex_points.len());
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
        options.sort_unstable_by(
            |(left_assignment, left_directions, left_quotient),
             (right_assignment, right_directions, right_quotient)| {
                let measure = |quotient: &MeshQuotient| {
                    (0..quotient.union.len())
                        .filter(|&node| quotient.union.root(node) == node)
                        .fold((0usize, 0u128), |(count, freedom), node| {
                            (count + 1, freedom + quotient.domains[node].len() as u128)
                        })
                };
                measure(left_quotient)
                    .cmp(&measure(right_quotient))
                    .then_with(|| left_assignment.cmp(right_assignment))
                    .then_with(|| left_directions.cmp(right_directions))
            },
        );
        for (assignment_index, directions, next_quotient) in options {
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
    mut unknown_uses: impl Iterator<Item = usize>,
) -> Option<usize> {
    unknown_uses.try_fold(0usize, |total, unknown| {
        let choices = u32::try_from(unknown)
            .ok()
            .and_then(|unknown| 1usize.checked_shl(unknown))?;
        total.checked_add(choices)
    })
}

pub(super) fn mesh_assignment_can_merge(
    assignment: &MeshFaceBoundaryAssignment,
    quotient: &mut MeshQuotient,
) -> bool {
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

    assignment.boundaries.iter().any(|boundary| {
        (0..boundary.len()).any(|index| {
            let left = possible_ports(boundary[index], true);
            let right = possible_ports(boundary[(index + 1) % boundary.len()], false);
            left.into_iter().flatten().any(|left| {
                right
                    .into_iter()
                    .flatten()
                    .any(|right| quotient.union.find(left) != quotient.union.find(right))
            })
        })
    })
}

pub(in crate::solve) fn mesh_edge_points_compatible(
    closed_ports: bool,
    candidates: &[[usize; 2]],
    points: [usize; 2],
) -> bool {
    (points[0] != points[1] || closed_ports)
        && (candidates.is_empty()
            || candidates
                .iter()
                .any(|candidate| same_unordered_pair(*candidate, points)))
}

/// Resolve standard trim assignments through their abstract physical-port
/// quotient before binding the quotient bijectively to coordinate rows.
#[cfg(test)]
pub(in crate::solve) fn parse_standard_mesh_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    edge_faces: &[[usize; 2]],
    edge_candidates: &[Vec<[usize; 2]>],
) -> Result<Option<(StandardTopology, Vec<usize>)>, CodecError> {
    let Some(face_run) = largest_fbb_run(bytes) else {
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
        &edge_rows,
        &vertex_points,
        edge_candidates,
        assignments,
        &port_identities,
        None,
        None,
        &budget,
        None,
        None,
        None,
        None,
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
        crate::resource::push(
            ctx,
            &mut directions,
            first_direction,
            "catia_singleton_initial_direction",
        )?;
        let mut valid = true;
        for use_ in &boundary[1..] {
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
            crate::resource::push(
                ctx,
                &mut directions,
                direction,
                "catia_singleton_direction_step",
            )?;
        }
        if valid && current == first_start {
            crate::resource::push(
                ctx,
                &mut solutions,
                directions,
                "catia_singleton_direction_solutions",
            )?;
        }
    }
    solutions.sort_unstable();
    solutions.dedup();
    if solutions.len() == 2
        && boundary.iter().all(|use_| {
            use_.reversed.is_none()
                || !edge_direction_evidence
                    .and_then(|evidence| evidence.get(use_.edge))
                    .copied()
                    .unwrap_or(false)
        })
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
    pub(super) fn canonical_cycle(
        ctx: &DecodeContext<'_>,
        points: &[usize],
    ) -> Result<Vec<usize>, CodecError> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        let value = |reversed: bool, start: usize, offset: usize| {
            let until_wrap = points.len() - start;
            let index = if offset >= until_wrap {
                offset - until_wrap
            } else {
                start + offset
            };
            if reversed {
                points[points.len() - 1 - index]
            } else {
                points[index]
            }
        };
        let mut best = (false, 0usize);
        for reversed in [false, true] {
            for start in 0..points.len() {
                let candidate = (reversed, start);
                if (0..points.len())
                    .map(|offset| value(reversed, start, offset))
                    .cmp((0..points.len()).map(|offset| value(best.0, best.1, offset)))
                    .is_lt()
                {
                    best = candidate;
                }
            }
        }
        let mut canonical = Vec::new();
        crate::resource::reserve_vec(
            ctx,
            &mut canonical,
            points.len(),
            "catia_singleton_canonical_cycle",
        )?;
        canonical.extend((0..points.len()).map(|offset| value(best.0, best.1, offset)));
        Ok(canonical)
    }

    let mut cycles = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut cycles,
        assignment.boundaries.len().min(directions.len()),
        "catia_singleton_cycle_rows",
    )?;
    for (boundary, directions) in assignment.boundaries.iter().zip(directions) {
        if boundary.len() != directions.len() {
            return Ok(None);
        }
        let mut points = Vec::new();
        crate::resource::reserve_vec(
            ctx,
            &mut points,
            boundary.len(),
            "catia_singleton_cycle_points",
        )?;
        for (use_, &reversed) in boundary.iter().zip(directions) {
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
    cycles.sort_unstable();
    Ok(Some(cycles))
}

pub(super) fn reconstruct_singleton_coordinate_topology(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    directions: &[Vec<Vec<bool>>],
) -> Result<Option<StandardTopology>, CodecError> {
    if selected.len() != directions.len() {
        return Ok(None);
    }
    let mut faces = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut faces,
        selected.len(),
        "catia_singleton_topology_faces",
    )?;
    for (assignment, face_directions) in selected.iter().zip(directions) {
        let mut boundaries = Vec::new();
        crate::resource::reserve_vec(
            ctx,
            &mut boundaries,
            assignment.boundaries.len().min(face_directions.len()),
            "catia_singleton_topology_boundaries",
        )?;
        for (boundary, directions) in assignment.boundaries.iter().zip(face_directions) {
            if boundary.len() != directions.len() || boundary.is_empty() {
                return Ok(None);
            }
            let mut coedges = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut coedges,
                boundary.len(),
                "catia_singleton_topology_coedges",
            )?;
            for (use_, &reversed) in boundary.iter().zip(directions) {
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
            let Some(boundary) = Boundary::new(coedges) else {
                return Ok(None);
            };
            boundaries.push(boundary);
        }
        faces.push(FaceTopology { boundaries });
    }
    let topology = StandardTopology {
        faces,
        edge_rows: copy_mesh_edge_rows(ctx, edge_rows)?,
        vertex_points: crate::resource::copy_retained_slice(
            ctx,
            vertex_points,
            "catia_singleton_topology_points",
        )?,
        logical_vertex_count: vertex_points.len(),
    };
    let Some(_) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    Ok(Some(topology))
}

pub(super) fn resolve_mesh_selection_from_quotient(
    ctx: &DecodeContext<'_>,
    topology: StandardTopology,
    mut quotient: MeshQuotient,
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
        || quotient.root_count() != vertex_points.len()
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
    for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
        let mut points = [0; 2];
        for (port, vertex) in [start, end].into_iter().enumerate() {
            let root = quotient.union.find(edge * 2 + port);
            let Some(&point) = root_points.get(&root) else {
                return Ok(None);
            };
            match point_assignment[vertex] {
                Some(stored) if stored != point => return Ok(None),
                Some(_) => {}
                None => point_assignment[vertex] = Some(point),
            }
            points[port] = point;
        }
        let closed_ports = quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
        if !mesh_edge_points_compatible(closed_ports, &edge_candidates[edge], points) {
            return Ok(None);
        }
        for (identity, point) in port_identities[edge].into_iter().zip(points) {
            match crate::resource::insert_map(
                ctx,
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
    let mut completed = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut completed,
        point_assignment.len(),
        "catia_merged_mesh_completed_points",
    )?;
    for point in point_assignment {
        let Some(point) = point else {
            return Ok(None);
        };
        completed.push(point);
    }
    Ok(Some(MeshSolve::Solved((topology, completed))))
}

pub(super) fn reduced_distinct_matching(
    ctx: &DecodeContext<'_>,
    domains: &[Vec<usize>],
    point_count: usize,
    budget: &WorkBudget<'_>,
    excluded: Option<(usize, usize)>,
) -> Result<Option<Vec<usize>>, CodecError> {
    let mut assignment = ctx.alloc_filled(domains.len(), None, "catia_reduced_matching")?;
    let mut used = ctx.alloc_filled(point_count, false, "catia_reduced_matching_used")?;
    let mut remaining = Vec::new();
    for (root, domain) in domains.iter().enumerate() {
        if domain.len() == 1 {
            let point = domain[0];
            if excluded.is_some_and(|(excluded_root, excluded_point)| {
                excluded_root == root && excluded_point == point
            }) || used[point]
            {
                return Ok(None);
            }
            used[point] = true;
            assignment[root] = Some(point);
            continue;
        }
        let mut values = Vec::new();
        crate::resource::reserve_vec(
            ctx,
            &mut values,
            domain.len(),
            "catia_reduced_matching_domain_values",
        )?;
        values.extend(domain.iter().copied().filter(|point| {
            !used[*point]
                && excluded.is_none_or(|(excluded_root, excluded_point)| {
                    excluded_root != root || excluded_point != *point
                })
        }));
        if values.is_empty() {
            return Ok(None);
        }
        crate::resource::push(
            ctx,
            &mut remaining,
            (root, values),
            "catia_reduced_matching_remaining_rows",
        )?;
    }
    let mut remaining_domains = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut remaining_domains,
        remaining.len(),
        "catia_reduced_matching_domain_refs",
    )?;
    remaining_domains.extend(remaining.iter().map(|(_, domain)| domain.as_slice()));
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
    for ((root, _), point) in remaining.into_iter().zip(matching) {
        assignment[root] = Some(point);
    }
    let mut completed = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut completed,
        assignment.len(),
        "catia_reduced_matching_completed",
    )?;
    for point in assignment {
        let Some(point) = point else {
            return Ok(None);
        };
        completed.push(point);
    }
    Ok(Some(completed))
}

// The selection owns the complete quotient inputs and the optional gauge. The
// explicit signature keeps the two bounded materialization paths symmetric.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_singleton_mesh_selection(
    ctx: &DecodeContext<'_>,
    edge_rows: &[EdgeRow],
    vertex_points: &[[f64; 3]],
    edge_candidates: &[Vec<[usize; 2]>],
    selected: &[MeshFaceBoundaryAssignment],
    directions: &[Vec<Vec<bool>>],
    port_identities: &[[u32; 2]],
    budget: &WorkBudget<'_>,
    candidate_gauge: Option<MeshCandidateGauge<'_>>,
) -> Result<Option<MeshEndpointResolve>, CodecError> {
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
    for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
        for (port, vertex) in [(0, start), (1, end)] {
            let node = edge * 2 + port;
            if let Some(previous) = crate::resource::insert_map(
                ctx,
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
    for (edge, candidates) in edge_candidates.iter().enumerate() {
        let &[[left_point, right_point]] = candidates.as_slice() else {
            return Ok(None);
        };
        let left_root = quotient.union.find(edge * 2);
        let right_root = quotient.union.find(edge * 2 + 1);
        if left_root == right_root && left_point != right_point {
            return Ok(None);
        }
        for root in [left_root, right_root] {
            let retained = quotient.domains[root]
                .len()
                .checked_mul(std::mem::size_of::<usize>())
                .and_then(|bytes| bytes.checked_add(std::mem::size_of::<HashSet<usize>>()))
                .map(cadmpeg_core::decode::u64_from_index)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_singleton_root_domain_copy", u64::MAX, u64::MAX)
                })?;
            ctx.charge_retained(retained, "catia_singleton_root_domain_copy")?;
            let mut domain = HashSet::new();
            crate::resource::reserve_set(
                ctx,
                &mut domain,
                quotient.domains[root].len(),
                "catia_singleton_root_domain_copy",
            )?;
            domain.extend(
                quotient.domains[root]
                    .iter()
                    .copied()
                    .filter(|point| *point == left_point || *point == right_point),
            );
            if domain.is_empty() {
                return Ok(None);
            }
            quotient.domains[root] = Arc::new(domain);
        }
    }
    let mut roots = Vec::new();
    for node in 0..quotient.union.len() {
        if quotient.union.find(node) == node {
            crate::resource::push(ctx, &mut roots, node, "catia_singleton_root_rows")?;
        }
    }
    if roots.len() != vertex_points.len() {
        return Ok(None);
    }
    let mut root_indices = HashMap::new();
    for (index, &root) in roots.iter().enumerate() {
        crate::resource::insert_map(
            ctx,
            &mut root_indices,
            root,
            index,
            "catia_singleton_root_indices",
        )?;
    }
    let mut domain_values = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut domain_values,
        roots.len(),
        "catia_singleton_domain_rows",
    )?;
    for &root in &roots {
        let domain = &quotient.domains[root];
        if domain.is_empty() {
            return Ok(None);
        }
        let mut values = Vec::new();
        crate::resource::reserve_vec(
            ctx,
            &mut values,
            domain.len(),
            "catia_singleton_domain_values",
        )?;
        values.extend(domain.iter().copied());
        values.sort_unstable();
        domain_values.push(values);
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
    for use_ in selected
        .iter()
        .flat_map(|assignment| &assignment.boundaries)
        .flatten()
    {
        let Some(count) = edge_use_counts.get_mut(use_.edge) else {
            return Ok(None);
        };
        *count += 1;
    }
    if edge_use_counts.iter().any(|count| *count > 2) {
        return Ok(None);
    }
    let mut materialize =
        |assignment: &[usize]| -> Result<Option<(StandardTopology, Vec<usize>)>, CodecError> {
            if assignment.len() != roots.len() {
                return Ok(None);
            }
            let mut point_assignment = ctx.alloc_filled(
                topology.logical_vertex_count,
                None,
                "catia_selection_singleton_point_assignment",
            )?;
            let mut points_by_identity = HashMap::<u32, usize>::new();
            for (edge, [start, end]) in edge_vertices.iter().copied().enumerate() {
                let mut points = [0; 2];
                for (port, vertex) in [start, end].into_iter().enumerate() {
                    let root = quotient.union.find(edge * 2 + port);
                    let Some(&root) = root_indices.get(&root) else {
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
                let closed_ports =
                    quotient.union.find(edge * 2) == quotient.union.find(edge * 2 + 1);
                if !mesh_edge_points_compatible(closed_ports, &edge_candidates[edge], points) {
                    return Ok(None);
                }
                for (identity, point) in port_identities[edge].into_iter().zip(points) {
                    match crate::resource::insert_map(
                        ctx,
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
            let mut points = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut points,
                point_assignment.len(),
                "catia_singleton_completed_points",
            )?;
            for point in point_assignment {
                let Some(point) = point else {
                    return Ok(None);
                };
                points.push(point);
            }
            Ok(Some((topology.clone_charged(ctx)?, points)))
        };
    let Some(first) = materialize(&first_assignment)? else {
        return Ok(None);
    };
    let mut ambiguous_roots = Vec::new();
    for (root, domain) in domain_values.iter().enumerate() {
        if domain.len() > 1 {
            crate::resource::push(
                ctx,
                &mut ambiguous_roots,
                root,
                "catia_singleton_ambiguous_roots",
            )?;
        }
    }
    for root in ambiguous_roots {
        let Some(alternate) = reduced_distinct_matching(
            ctx,
            &domain_values,
            vertex_points.len(),
            budget,
            Some((root, first_assignment[root])),
        )?
        else {
            if budget.exhausted() {
                return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Exhausted(()))));
            }
            continue;
        };
        let Some(alternate) = materialize(&alternate)? else {
            continue;
        };
        if !mesh_candidates_equivalent_with_context(ctx, &first, &alternate, candidate_gauge)? {
            return Ok(Some(MeshSolve::Failed(MeshCandidateFailure::Ambiguous(()))));
        }
    }
    Ok(Some(MeshSolve::Solved((first.0, first.1))))
}
