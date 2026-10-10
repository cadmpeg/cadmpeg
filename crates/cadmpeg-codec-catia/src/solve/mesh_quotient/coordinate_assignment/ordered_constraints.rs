//! Indexed direction compatibility at ordered boundary corners.

use crate::solve::mesh_quotient::{domain_contains, domains_disjoint};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryDomain};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

const OPERATION: &str = "catia_coordinate_ordered_viability";
const IDENTITY: u8 = 0b1001;

/// Two direction states per use. A product joins consecutive corners.
fn product(left: u8, right: u8) -> u8 {
    let mut result = 0;
    for from in 0..2 {
        for to in 0..2 {
            for middle in 0..2 {
                if left & (1 << (2 * from + middle)) != 0 && right & (1 << (2 * middle + to)) != 0 {
                    result |= 1 << (2 * from + to);
                }
            }
        }
    }
    result
}

#[derive(Clone, Copy)]
struct Corner {
    left: [Option<usize>; 2],
    right: [Option<usize>; 2],
}

impl Corner {
    fn matrix(
        self,
        ctx: &DecodeContext<'_>,
        domains: &[Vec<usize>],
        assigned: &[Option<usize>],
        candidate: Option<(usize, usize)>,
    ) -> Result<u8, CodecError> {
        let value = |root: usize| {
            candidate
                .filter(|&(other, _)| other == root)
                .map(|(_, point)| point)
                .or(assigned[root])
        };
        let mut matrix = 0;
        for (from, left) in self.left.into_iter().enumerate() {
            for (to, right) in self.right.into_iter().enumerate() {
                let (Some(left), Some(right)) = (left, right) else {
                    continue;
                };
                let compatible = if left == right {
                    true
                } else {
                    match (value(left), value(right)) {
                        (Some(left), Some(right)) => left == right,
                        (Some(point), None) => {
                            domain_contains(ctx, &domains[right], point, OPERATION)?
                        }
                        (None, Some(point)) => {
                            domain_contains(ctx, &domains[left], point, OPERATION)?
                        }
                        (None, None) => {
                            !domains_disjoint(ctx, &domains[left], &domains[right], OPERATION)?
                        }
                    }
                };
                if compatible {
                    matrix |= 1 << (2 * from + to);
                }
            }
        }
        Ok(matrix)
    }
}

struct Cycle {
    corners: Vec<Corner>,
    matrices: Vec<u8>,
    leaves: usize,
    assignment: usize,
}

impl Cycle {
    fn valid(&self) -> bool {
        !self.corners.is_empty() && self.matrices[1] & IDENTITY != 0
    }

    fn update(
        &mut self,
        ctx: &DecodeContext<'_>,
        corner: usize,
        matrix: u8,
    ) -> Result<(), CodecError> {
        let leaf = self.leaves + corner;
        if self.matrices[leaf] == matrix {
            return Ok(());
        }
        self.matrices[leaf] = matrix;
        if self.leaves == 1 {
            return Ok(());
        }
        if self.leaves == 2 {
            self.matrices[1] = product(self.matrices[2], self.matrices[3]);
            return Ok(());
        }
        let mut nodes =
            std::iter::successors(Some(leaf / 2), |node| (node / 2 > 0).then_some(node / 2));
        while let Some(node) = ctx.next_charged(&mut nodes, OPERATION)? {
            self.matrices[node] = product(self.matrices[2 * node], self.matrices[2 * node + 1]);
        }
        Ok(())
    }
}

pub(super) struct OrderedConstraints {
    cycles: Vec<Cycle>,
    root_corners: Vec<Vec<(usize, usize)>>,
    assignment_faces: Vec<usize>,
    bad_boundaries: Vec<usize>,
    valid_assignments: Vec<usize>,
}

impl OrderedConstraints {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        domains: &[Vec<usize>],
        assigned: &[Option<usize>],
        edge_by_id: &HashMap<usize, usize>,
        edges: &[[usize; 2]],
        faces: &[&MeshFaceBoundaryDomain],
        closed: &[bool],
    ) -> Result<Self, CodecError> {
        let mut index = Self {
            cycles: Vec::new(),
            root_corners: ctx.collect_indexed_vec(domains.len(), OPERATION, |_| Ok(Vec::new()))?,
            assignment_faces: Vec::new(),
            bad_boundaries: Vec::new(),
            valid_assignments: ctx.alloc_filled(faces.len(), 0, OPERATION)?,
        };
        let roots = |use_: MeshBoundaryEdgeCandidate,
                     end: bool|
         -> Result<[Option<usize>; 2], CodecError> {
            let Some(&edge) = ctx.get_hash_map(edge_by_id, &use_.edge, OPERATION)? else {
                return Ok([None; 2]);
            };
            Ok(std::array::from_fn(|direction| {
                let reversed = direction != 0;
                use_.reversed
                    .is_none_or(|fixed| fixed == reversed)
                    .then_some(edges[edge][usize::from(if end { !reversed } else { reversed })])
            }))
        };
        for (face, domain) in ctx.admit_iter(faces, OPERATION)?.enumerate() {
            if !closed[face] {
                continue;
            }
            let MeshFaceBoundaryDomain::Ordered(assignments) = domain else {
                continue;
            };
            for assignment in ctx.admit_iter(assignments, OPERATION)? {
                let assignment_id = index.assignment_faces.len();
                ctx.push_vec(&mut index.assignment_faces, face, OPERATION)?;
                let mut bad = 0;
                for boundary in ctx.admit_iter(&assignment.boundaries, OPERATION)? {
                    let leaves = boundary
                        .len()
                        .max(1)
                        .checked_next_power_of_two()
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX, u64::MAX))?;
                    let count = leaves
                        .checked_mul(2)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX, u64::MAX))?;
                    let mut cycle = Cycle {
                        corners: Vec::new(),
                        matrices: ctx.alloc_filled(count, IDENTITY, OPERATION)?,
                        leaves,
                        assignment: assignment_id,
                    };
                    let cycle_id = index.cycles.len();
                    for corner_id in ctx.admit_iter(0..boundary.len(), OPERATION)? {
                        let corner = Corner {
                            left: roots(boundary[corner_id], true)?,
                            right: roots(boundary[(corner_id + 1) % boundary.len()], false)?,
                        };
                        cycle.matrices[leaves + corner_id] =
                            corner.matrix(ctx, domains, assigned, None)?;
                        // Equal roots stay equal under every assignment. Only unequal
                        // root comparisons are dependencies of this corner.
                        let mut dependencies = [None; 4];
                        for left in corner.left.into_iter().flatten() {
                            for right in corner.right.into_iter().flatten() {
                                if left == right {
                                    continue;
                                }
                                for root in [left, right] {
                                    if dependencies.contains(&Some(root)) {
                                        continue;
                                    }
                                    let Some(slot) =
                                        dependencies.iter_mut().find(|slot| slot.is_none())
                                    else {
                                        return Err(CodecError::malformed(
                                            "ordered corner has more than four roots",
                                        ));
                                    };
                                    *slot = Some(root);
                                }
                            }
                        }
                        for root in dependencies.into_iter().flatten() {
                            ctx.push_vec(
                                &mut index.root_corners[root],
                                (cycle_id, corner_id),
                                OPERATION,
                            )?;
                        }
                        ctx.push_vec(&mut cycle.corners, corner, OPERATION)?;
                    }
                    if leaves == 2 {
                        cycle.matrices[1] = product(cycle.matrices[2], cycle.matrices[3]);
                    } else if leaves > 2 {
                        for node in ctx.admit_iter(1..leaves, OPERATION)?.rev() {
                            cycle.matrices[node] =
                                product(cycle.matrices[2 * node], cycle.matrices[2 * node + 1]);
                        }
                    }
                    bad += usize::from(!cycle.valid());
                    ctx.push_vec(&mut index.cycles, cycle, OPERATION)?;
                }
                ctx.push_vec(&mut index.bad_boundaries, bad, OPERATION)?;
                index.valid_assignments[face] += usize::from(bad == 0);
            }
        }
        Ok(index)
    }

    pub(super) fn update(
        &mut self,
        ctx: &DecodeContext<'_>,
        root: usize,
        domains: &[Vec<usize>],
        assigned: &[Option<usize>],
        candidate: Option<usize>,
    ) -> Result<(), CodecError> {
        for &(cycle_id, corner_id) in ctx.admit_iter(&self.root_corners[root], OPERATION)? {
            let cycle = &mut self.cycles[cycle_id];
            let old_valid = cycle.valid();
            let matrix = cycle.corners[corner_id].matrix(
                ctx,
                domains,
                assigned,
                candidate.map(|point| (root, point)),
            )?;
            cycle.update(ctx, corner_id, matrix)?;
            let new_valid = cycle.valid();
            if old_valid != new_valid {
                let bad = &mut self.bad_boundaries[cycle.assignment];
                let old_assignment = *bad == 0;
                if new_valid {
                    *bad -= 1;
                } else {
                    *bad += 1;
                }
                let new_assignment = *bad == 0;
                if old_assignment != new_assignment {
                    let valid =
                        &mut self.valid_assignments[self.assignment_faces[cycle.assignment]];
                    if new_assignment {
                        *valid += 1;
                    } else {
                        *valid -= 1;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn face_valid(&self, face: usize) -> bool {
        self.valid_assignments[face] > 0
    }

    pub(super) fn affected_roots(
        &self,
        ctx: &DecodeContext<'_>,
        root: usize,
        output: &mut Vec<usize>,
    ) -> Result<(), CodecError> {
        // Immediate propagation visits roots at changed corners. The closure
        // performs a complete scan when this local worklist reaches a fixed point.
        for &(cycle, corner) in ctx.admit_iter(&self.root_corners[root], OPERATION)? {
            let corner = self.cycles[cycle].corners[corner];
            for other in corner.left.into_iter().chain(corner.right).flatten() {
                ctx.push_vec(output, other, OPERATION)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solve::missing_edge::MeshFaceBoundaryAssignment;

    #[test]
    fn ordered_root_dependencies_visit_only_changed_corners() {
        const EDGES: usize = 128;
        crate::test_support::with_service_context(|ctx| {
            let edges = (0..EDGES)
                .map(|edge| [2 * edge, 2 * edge + 1])
                .collect::<Vec<_>>();
            let domains = vec![vec![0, 1]; 2 * EDGES];
            let assigned = vec![None; 2 * EDGES];
            let edge_ids = (0..EDGES).map(|edge| (edge, edge)).collect();
            let face = MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
                boundaries: vec![(0..EDGES)
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: Some(false),
                    })
                    .collect()],
            }]);
            let index = OrderedConstraints::new(
                ctx,
                &domains,
                &assigned,
                &edge_ids,
                &edges,
                &[&face],
                &[true],
            )
            .expect("ordered index");
            let mut affected = Vec::new();
            index
                .affected_roots(ctx, 0, &mut affected)
                .expect("changed corner");
            affected.sort_unstable();
            assert_eq!(affected, vec![0, 2 * EDGES - 1]);
        });
    }

    #[test]
    fn indexed_ordered_corners_match_complete_direction_enumeration() {
        crate::test_support::with_service_context(|ctx| {
            let edges = [[0, 1], [2, 3], [4, 5]];
            let domains = vec![vec![0, 1]; 6];
            let edge_ids = HashMap::from([(0, 0), (1, 1), (2, 2)]);
            for fixed in [None, Some(false), Some(true)] {
                let uses = (0..3)
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: if edge == 1 { fixed } else { None },
                    })
                    .collect::<Vec<_>>();
                let domain = MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
                    boundaries: vec![uses],
                }]);
                let mut assigned = vec![None; 6];
                let mut index = OrderedConstraints::new(
                    ctx,
                    &domains,
                    &assigned,
                    &edge_ids,
                    &edges,
                    &[&domain],
                    &[true],
                )
                .expect("index");
                for values in 0..64 {
                    for (root, value) in assigned.iter_mut().enumerate() {
                        *value = Some((values >> root) & 1);
                    }
                    for root in 0..6 {
                        index
                            .update(ctx, root, &domains, &assigned, None)
                            .expect("assignment update");
                    }
                    let viable = |candidate: Option<(usize, usize)>| {
                        let value = |root: usize| {
                            candidate
                                .filter(|&(other, _)| other == root)
                                .map(|(_, point)| point)
                                .unwrap_or(assigned[root].expect("assigned"))
                        };
                        (0..8).any(|directions| {
                            if fixed.is_some_and(|fixed| fixed != (directions & 2 != 0)) {
                                return false;
                            }
                            (0..3).all(|edge| {
                                let next = (edge + 1) % 3;
                                let end = usize::from(directions & (1 << edge) == 0);
                                let start = usize::from(directions & (1 << next) != 0);
                                value(edges[edge][end]) == value(edges[next][start])
                            })
                        })
                    };
                    assert_eq!(index.face_valid(0), viable(None));
                    for root in 0..6 {
                        let point = 1 - assigned[root].expect("assigned");
                        index
                            .update(ctx, root, &domains, &assigned, Some(point))
                            .expect("probe");
                        assert_eq!(index.face_valid(0), viable(Some((root, point))));
                        index
                            .update(ctx, root, &domains, &assigned, None)
                            .expect("rollback");
                        assert_eq!(index.face_valid(0), viable(None));
                    }
                }
            }
        });
    }
}
