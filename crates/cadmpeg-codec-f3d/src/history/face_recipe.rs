// SPDX-License-Identifier: Apache-2.0
//! Bind bounded-face nodes to complete historical face boundaries.

use super::cache::SnapshotCache;
use crate::history_records::AsmHistoricalTopology;
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::topology::face::{
    prefix_operand_ordinals, DesignFaceOperand, DesignFaceRecipeStructure,
};
use cadmpeg_core::decode::{BudgetExhausted, WorkBudget};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

const MAX_FACE_RECIPE_MATCH_WORK: usize = 1 << 22;
const FAN_MATCH_PASSES: usize = 6;

pub(super) fn bind_operation_operand<'a>(
    operand: &mut DesignFaceOperand,
    kind: &DesignFeatureKind,
    topology: &'a AsmHistoricalTopology,
    indices: &mut SnapshotCache<'a, AsmHistoricalTopology, FaceRecipeIndex>,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            kind,
            DesignFeatureKind::MoveFace | DesignFeatureKind::OffsetFaces | DesignFeatureKind::Shell
        )
    {
        return None;
    }
    let index = indices.get(topology, FaceRecipeIndex::new);
    let face = if matches!(kind, DesignFeatureKind::Shell) {
        match index.resolve_loop_program(operand, &WorkBudget::new(MAX_FACE_RECIPE_MATCH_WORK)) {
            Ok(face) => face,
            Err(BudgetExhausted) => {
                operand.resolved_face_slots.clear();
                return Some(crate::loss::F3dLossCode::FaceRecipeMatchWorkExceeded.note(format!(
                    "Operand {} exhausted MAX_FACE_RECIPE_MATCH_WORK ({MAX_FACE_RECIPE_MATCH_WORK}) while assigning counted face nodes to boundary edges; its source recipe is retained.",
                    operand.id
                )));
            }
        }
    } else {
        index.resolve(operand)
    };
    operand.resolved_face_slots = face.into_iter().collect();
    None
}

struct Corner {
    face: i64,
    edges: [i64; 2],
}

struct BoundaryRecipe<'a> {
    lanes: Vec<BTreeSet<i64>>,
    nodes: Vec<&'a DesignFaceRecipeStructure>,
    candidates: BTreeSet<i64>,
}

pub(super) struct FaceRecipeIndex {
    boundaries: HashMap<i64, Vec<i64>>,
    loops: HashMap<i64, Vec<std::ops::Range<usize>>>,
    edge_faces: HashMap<i64, Vec<i64>>,
    endpoints: HashMap<i64, Option<[i64; 2]>>,
    corners: HashMap<i64, Vec<Corner>>,
    vertex_edges: HashMap<i64, BTreeSet<i64>>,
}

impl FaceRecipeIndex {
    pub(super) fn new(topology: &AsmHistoricalTopology) -> Self {
        let endpoints = super::topology_cache::endpoint_index(topology);
        let mut boundaries = HashMap::new();
        let mut face_loops = HashMap::new();
        let mut edge_faces = HashMap::<_, Vec<_>>::new();
        let mut corners = HashMap::<_, Vec<_>>::new();
        let mut vertex_edges = HashMap::<_, BTreeSet<_>>::new();
        for row in &topology.edge_vertices {
            for vertex in [row.start_vertex, row.end_vertex] {
                vertex_edges.entry(vertex).or_default().insert(row.edge);
            }
        }
        let mut coedge_rows = HashMap::new();
        for row in &topology.coedge_topology {
            coedge_rows
                .entry(row.coedge)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(row));
        }
        for &face in &topology.faces {
            let Some(loops) = super::face_resolution::relation_members(&topology.face_loops, face)
            else {
                continue;
            };
            let mut boundary = Vec::new();
            let mut loop_boundaries = Vec::new();
            let mut face_corners = Vec::new();
            let complete = loops.iter().all(|&loop_| {
                let Some(coedges) =
                    super::face_resolution::relation_members(&topology.loop_coedges, loop_)
                else {
                    return false;
                };
                if coedges.is_empty() {
                    return false;
                }
                let mut edges = Vec::new();
                for &coedge in coedges {
                    let Some(Some(row)) = coedge_rows.get(&coedge) else {
                        return false;
                    };
                    if row.owner_loop != loop_ {
                        return false;
                    }
                    edges.push(row.edge);
                }
                for (ordinal, &edge) in edges.iter().enumerate() {
                    let next = edges[(ordinal + 1) % edges.len()];
                    let (Some(Some(left)), Some(Some(right))) =
                        (endpoints.get(&edge), endpoints.get(&next))
                    else {
                        return false;
                    };
                    let shared: BTreeSet<_> = left
                        .iter()
                        .filter(|vertex| right.contains(vertex))
                        .copied()
                        .collect();
                    let shared: Vec<_> = shared.into_iter().collect();
                    let vertices = match shared.as_slice() {
                        [vertex] => std::slice::from_ref(vertex),
                        [_, _] if edges.len() == 2 && ordinal == 0 => shared.as_slice(),
                        [_, _] if edges.len() == 2 => continue,
                        _ => return false,
                    };
                    for &vertex in vertices {
                        face_corners.push((
                            vertex,
                            Corner {
                                face,
                                edges: [edge, next],
                            },
                        ));
                    }
                }
                let start = boundary.len();
                boundary.extend(edges);
                loop_boundaries.push(start..boundary.len());
                true
            });
            if complete && !boundary.is_empty() {
                for &edge in &boundary {
                    edge_faces.entry(edge).or_default().push(face);
                }
                for (vertex, corner) in face_corners {
                    corners.entry(vertex).or_default().push(corner);
                }
                boundaries.insert(face, boundary);
                face_loops.insert(face, loop_boundaries);
            }
        }
        Self {
            boundaries,
            loops: face_loops,
            edge_faces,
            endpoints,
            corners,
            vertex_edges,
        }
    }

    /// Whether every edge of the topology that meets `vertex`, including
    /// the edges of faces without a complete boundary, bounds a corner there.
    fn corners_cover_vertex_edges(&self, vertex: i64, corners: &[Corner]) -> bool {
        self.vertex_edges.get(&vertex).is_some_and(|edges| {
            edges
                .iter()
                .all(|edge| corners.iter().any(|corner| corner.edges.contains(edge)))
        })
    }

    fn fan(&self, edge: i64, vertex: i64, first_face: i64) -> Option<Vec<i64>> {
        let corners = self.corners.get(&vertex)?;
        let mut current_edge = edge;
        let mut previous = None;
        let mut visited = HashSet::new();
        let mut fan = Vec::new();
        loop {
            let mut matches = corners.iter().enumerate().filter(|(index, corner)| {
                Some(*index) != previous
                    && corner.edges.contains(&current_edge)
                    && (!fan.is_empty() || corner.face == first_face)
            });
            let (index, corner) = matches.next()?;
            if matches.next().is_some() || !visited.insert(index) {
                return None;
            }
            fan.push(corner.face);
            current_edge = if corner.edges[0] == current_edge {
                corner.edges[1]
            } else {
                corner.edges[0]
            };
            if current_edge == edge {
                return (visited.len() == corners.len()).then_some(fan);
            }
            previous = Some(index);
        }
    }

    /// The face fan at the one vertex of a closed edge: the two faces that
    /// the edge bounds, when no other edge meets that vertex.
    fn closed_edge_fan(&self, edge: i64, vertex: i64) -> Option<Vec<i64>> {
        let corners = self.corners.get(&vertex)?;
        let [first, second] = corners.as_slice() else {
            return None;
        };
        (first.edges == [edge, edge]
            && second.edges == [edge, edge]
            && self.corners_cover_vertex_edges(vertex, corners))
        .then(|| vec![first.face, second.face])
    }

    fn branching_fan_matches(
        &self,
        edge: i64,
        vertex: i64,
        first_face: i64,
        ordinals: &[i32],
        lanes: &[BTreeSet<i64>],
    ) -> bool {
        let Some(corners) = self.corners.get(&vertex) else {
            return false;
        };
        if ordinals.len() != corners.len()
            || !self.corners_cover_vertex_edges(vertex, corners)
            || corners
                .iter()
                .any(|corner| corner.edges[0] == corner.edges[1])
        {
            return false;
        }
        let mut incidence = HashMap::<_, Vec<_>>::new();
        for (index, corner) in corners.iter().enumerate() {
            for &incident in &corner.edges {
                incidence.entry(incident).or_default().push(index);
            }
        }
        if incidence.get(&edge).is_none_or(|uses| uses.len() != 2)
            || incidence.values().filter(|uses| uses.len() == 4).count() != 1
            || incidence.values().any(|uses| !matches!(uses.len(), 2 | 4))
        {
            return false;
        }
        let Some(ordinal_lanes) = ordinals
            .iter()
            .map(|&ordinal| usize::try_from(ordinal).ok().and_then(|i| lanes.get(i)))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        (0..corners.len()).any(|start| {
            [false, true].into_iter().any(|reverse| {
                let pattern = (0..corners.len())
                    .map(|offset| {
                        let index = if reverse {
                            (start + corners.len() - offset) % corners.len()
                        } else {
                            (start + offset) % corners.len()
                        };
                        ordinal_lanes[index]
                    })
                    .collect::<Vec<_>>();
                corner_walk_matches(edge, first_face, corners, &incidence, &pattern)
            })
        })
    }

    fn node_matches(
        &self,
        node: &DesignFaceRecipeStructure,
        edge: i64,
        lanes: &[BTreeSet<i64>],
    ) -> bool {
        let Some(faces) = self.edge_faces.get(&edge) else {
            return false;
        };
        let [left, right] = faces.as_slice() else {
            return false;
        };
        let Some(Some(vertices)) = self.endpoints.get(&edge) else {
            return false;
        };
        let closed = vertices[0] == vertices[1];
        if closed != (node.sides.len() == 1) || node.sides.is_empty() {
            return false;
        }
        let Some(root) = usize::try_from(node.root).ok().and_then(|i| lanes.get(i)) else {
            return false;
        };
        let Some(other) = usize::try_from(node.prelude[0])
            .ok()
            .and_then(|i| lanes.get(i))
        else {
            return false;
        };
        if !(root.contains(left) && other.contains(right)
            || root.contains(right) && other.contains(left))
        {
            return false;
        }
        let ordinals = |side: usize| {
            let side = &node.sides[side];
            std::iter::once(side.header_value)
                .chain(side.scalars.iter().copied())
                .collect::<Vec<_>>()
        };
        if closed {
            return self
                .closed_edge_fan(edge, vertices[0])
                .is_some_and(|fan| fan_matches(&ordinals(0), &fan, lanes));
        }
        let fans = [
            self.fan(edge, vertices[0], *left),
            self.fan(edge, vertices[1], *left),
        ];
        let matches = |side: usize, endpoint: usize| {
            let ordinals = ordinals(side);
            match fans[endpoint].as_deref() {
                Some(fan) => fan_matches(&ordinals, fan, lanes),
                None => {
                    self.branching_fan_matches(edge, vertices[endpoint], *left, &ordinals, lanes)
                }
            }
        };
        matches(0, 0) && matches(1, 1) || matches(1, 0) && matches(0, 1)
    }

    pub(super) fn resolve(&self, operand: &DesignFaceOperand) -> Option<i64> {
        self.resolve_boundary(operand, false)
    }

    pub(super) fn resolve_loop(&self, operand: &DesignFaceOperand) -> Option<i64> {
        if operand
            .recipe_nodes
            .iter()
            .any(|node| !node.has_two_side_clauses())
        {
            return None;
        }
        self.resolve_boundary(operand, true)
    }

    fn prepare<'a>(&self, operand: &'a DesignFaceOperand) -> Option<BoundaryRecipe<'a>> {
        use crate::design::decode::operands::{face_recipe_program_kind, FaceRecipeProgramKind};
        if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace {
            return None;
        }
        let FaceRecipeProgramKind::Counted { header_value } =
            face_recipe_program_kind(&operand.recipe_program)?
        else {
            return None;
        };
        if header_value != operand.recipe_nodes.len() || header_value == 0 {
            return None;
        }
        let ordinals = prefix_operand_ordinals(&operand.recipe_references)?;
        let mut lanes = Vec::<BTreeSet<i64>>::new();
        for (reference, &ordinal) in operand.recipe_references.iter().zip(&ordinals) {
            if ordinal == lanes.len() {
                lanes.push(BTreeSet::new());
            }
            let lane = lanes.get_mut(ordinal)?;
            if reference.design_reference < 0 {
                continue;
            }
            for face in super::face_resolution::effective_faces(reference) {
                if let Some(slot) = super::body_closures::stable_ref(face.as_str()) {
                    if self.boundaries.contains_key(&slot) {
                        lane.insert(slot);
                    }
                }
            }
        }
        let nodes = operand
            .recipe_nodes
            .iter()
            .map(|node| node.recipe_structure.as_ref())
            .collect::<Option<Vec<_>>>()?;
        if nodes
            .iter()
            .any(|node| !node.has_valid_boundary_edge_ordinals(&operand.recipe_references))
        {
            return None;
        }
        let mut candidates = None::<BTreeSet<i64>>;
        for node in &nodes {
            let mut incident = lanes[usize::try_from(node.root).ok()?].clone();
            incident.extend(&lanes[usize::try_from(node.prelude[0]).ok()?]);
            candidates = Some(match candidates {
                Some(previous) => previous.intersection(&incident).copied().collect(),
                None => incident,
            });
        }
        Some(BoundaryRecipe {
            lanes,
            nodes,
            candidates: candidates?,
        })
    }

    fn resolve_boundary(&self, operand: &DesignFaceOperand, single_loop: bool) -> Option<i64> {
        self.resolve_prepared_boundary(self.prepare(operand)?, single_loop)
    }

    fn resolve_prepared_boundary(
        &self,
        recipe: BoundaryRecipe<'_>,
        single_loop: bool,
    ) -> Option<i64> {
        let BoundaryRecipe {
            lanes,
            nodes,
            candidates,
        } = recipe;
        let mut resolved = None;
        for face in candidates {
            let boundary = self.boundaries.get(&face)?;
            let loops = self.loops.get(&face)?;
            let whole = 0..boundary.len();
            let ranges = if single_loop {
                loops.as_slice()
            } else {
                std::slice::from_ref(&whole)
            };
            for range in ranges {
                let boundary = &boundary[range.clone()];
                if boundary.len() != nodes.len()
                    || boundary.iter().collect::<HashSet<_>>().len() != boundary.len()
                {
                    continue;
                }
                let mut covered = HashSet::new();
                let complete = nodes.iter().all(|node| {
                    let mut matches = boundary
                        .iter()
                        .copied()
                        .filter(|edge| self.node_matches(node, *edge, &lanes));
                    let Some(edge) = matches.next() else {
                        return false;
                    };
                    matches.next().is_none() && covered.insert(edge)
                });
                if complete && covered.len() == boundary.len() {
                    if resolved.is_some_and(|previous| previous != face) {
                        return None;
                    }
                    resolved = Some(face);
                }
            }
        }
        resolved
    }

    fn resolve_loop_program(
        &self,
        operand: &DesignFaceOperand,
        work: &WorkBudget,
    ) -> Result<Option<i64>, BudgetExhausted> {
        let Some(recipe) = self.prepare(operand) else {
            return Ok(None);
        };
        let mut resolved = None;
        for face in recipe.candidates {
            let (Some(boundary), Some(ranges)) =
                (self.boundaries.get(&face), self.loops.get(&face))
            else {
                continue;
            };
            for range in ranges {
                let boundary = &boundary[range.clone()];
                if boundary.len() != recipe.nodes.len()
                    || boundary.iter().collect::<HashSet<_>>().len() != boundary.len()
                {
                    continue;
                }
                if self.program_matches(&recipe.nodes, boundary, &recipe.lanes, work)? {
                    if resolved.is_some_and(|previous| previous != face) {
                        return Ok(None);
                    }
                    resolved = Some(face);
                }
            }
        }
        Ok(resolved)
    }

    fn program_matches(
        &self,
        nodes: &[&DesignFaceRecipeStructure],
        boundary: &[i64],
        lanes: &[BTreeSet<i64>],
        work: &WorkBudget,
    ) -> Result<bool, BudgetExhausted> {
        let mut choices = Vec::new();
        for node in nodes {
            let mut edges = Vec::new();
            for &edge in boundary {
                let cost = self.match_work(edge).ok_or(BudgetExhausted)?;
                if !work.charge_by(cost) {
                    return Err(BudgetExhausted);
                }
                if self.node_matches(node, edge, lanes) {
                    edges.push(edge);
                }
            }
            if edges.is_empty() {
                return Ok(false);
            }
            choices.push(edges);
        }
        complete_assignment(&choices, work)
    }

    fn match_work(&self, edge: i64) -> Option<usize> {
        let Some(Some(vertices)) = self.endpoints.get(&edge) else {
            return Some(1);
        };
        vertices.iter().try_fold(1usize, |total, vertex| {
            let corners = self.corners.get(vertex).map_or(0, Vec::len);
            total.checked_add(
                corners
                    .checked_mul(corners)?
                    .checked_mul(FAN_MATCH_PASSES)?,
            )
        })
    }
}

fn complete_assignment(choices: &[Vec<i64>], work: &WorkBudget) -> Result<bool, BudgetExhausted> {
    let mut edge_nodes = HashMap::new();
    let mut node_edges = HashMap::new();
    for root in 0..choices.len() {
        let mut pending = VecDeque::from([root]);
        let mut parents = HashMap::new();
        let mut free = None;
        'search: while let Some(node) = pending.pop_front() {
            for &edge in &choices[node] {
                if !work.charge() {
                    return Err(BudgetExhausted);
                }
                if let std::collections::hash_map::Entry::Vacant(entry) = parents.entry(edge) {
                    entry.insert(node);
                } else {
                    continue;
                }
                if let Some(&owner) = edge_nodes.get(&edge) {
                    pending.push_back(owner);
                } else {
                    free = Some(edge);
                    break 'search;
                }
            }
        }
        let Some(mut edge) = free else {
            return Ok(false);
        };
        loop {
            let Some(node) = parents.remove(&edge) else {
                return Ok(false);
            };
            edge_nodes.insert(edge, node);
            let Some(previous) = node_edges.insert(node, edge) else {
                break;
            };
            edge = previous;
        }
    }
    Ok(true)
}

fn corner_walk_matches(
    edge: i64,
    first_face: i64,
    corners: &[Corner],
    incidence: &HashMap<i64, Vec<usize>>,
    pattern: &[&BTreeSet<i64>],
) -> bool {
    let mut stack = vec![(edge, 0)];
    let mut path = Vec::new();
    let mut visited = HashSet::new();
    while let Some((current, next)) = stack.last_mut() {
        if path.len() == corners.len() && *current == edge {
            return true;
        }
        let Some(&index) = incidence
            .get(current)
            .and_then(|choices| choices.get(*next))
        else {
            stack.pop();
            if let Some(previous) = path.pop() {
                visited.remove(&previous);
            }
            continue;
        };
        *next += 1;
        let corner = &corners[index];
        if visited.contains(&index)
            || path.is_empty() && corner.face != first_face
            || !pattern[path.len()].contains(&corner.face)
        {
            continue;
        }
        let following = if corner.edges[0] == *current {
            corner.edges[1]
        } else {
            corner.edges[0]
        };
        if (following == edge) != (path.len() + 1 == corners.len()) {
            continue;
        }
        visited.insert(index);
        path.push(index);
        stack.push((following, 0));
    }
    false
}

fn fan_matches(ordinals: &[i32], fan: &[i64], lanes: &[BTreeSet<i64>]) -> bool {
    ordinals.len() == fan.len()
        && (0..fan.len()).any(|start| {
            [false, true].into_iter().any(|reverse| {
                ordinals.iter().enumerate().all(|(offset, ordinal)| {
                    let index = if reverse {
                        (start + fan.len() - offset) % fan.len()
                    } else {
                        (start + offset) % fan.len()
                    };
                    usize::try_from(*ordinal)
                        .ok()
                        .and_then(|ordinal| lanes.get(ordinal))
                        .is_some_and(|lane| lane.contains(&fan[index]))
                })
            })
        })
}

#[cfg(test)]
mod tests;
