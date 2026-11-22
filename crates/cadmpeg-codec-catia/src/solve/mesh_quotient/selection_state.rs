// SPDX-License-Identifier: Apache-2.0
//! Selection search state, orientation options, and face choices.

use super::quotient::MeshQuotient;
use super::{
    port, MeshFaceDirectionOptions, MeshFaceEquationCache, MeshFaceSelection,
    MeshOrientationOption, MeshSelectionStateSignature, SearchOutcome,
};
use crate::families::standard::topology::{EdgeRow, StandardTopologyDraft};
use crate::solve::mesh_gauge::MeshCandidateGauge;
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
#[cfg(test)]
use std::sync::Arc;

pub(super) fn canonical_direction_bit(row: &[bool], index: usize) -> bool {
    row[index] ^ row.first().copied().unwrap_or(false)
}

pub(super) fn orientation_fingerprint(
    quotient: &MeshQuotient,
    directions: &[Vec<bool>],
) -> Result<u64, CodecError> {
    let mut hasher = DefaultHasher::new();
    quotient.union.len().hash(&mut hasher);
    for node in 0..quotient.union.len() {
        let root = quotient.union.root(node)?;
        quotient.members(root).iter().min().hash(&mut hasher);
        let domain = &quotient.domains[root];
        domain.len().hash(&mut hasher);
        let mut xor = 0u64;
        let mut sum = 0u128;
        for point in domain.iter() {
            let mut point_hasher = DefaultHasher::new();
            point.hash(&mut point_hasher);
            let value = point_hasher.finish();
            xor ^= value;
            sum += u128::from(value);
        }
        xor.hash(&mut hasher);
        sum.hash(&mut hasher);
    }
    directions.len().hash(&mut hasher);
    for row in directions {
        row.len().hash(&mut hasher);
        for index in 0..row.len() {
            canonical_direction_bit(row, index).hash(&mut hasher);
        }
    }
    Ok(hasher.finish())
}

pub(super) fn orientation_options_equivalent(
    left_quotient: &MeshQuotient,
    left_directions: &[Vec<bool>],
    right_quotient: &MeshQuotient,
    right_directions: &[Vec<bool>],
) -> Result<bool, CodecError> {
    if left_quotient.union.len() != right_quotient.union.len()
        || left_directions.len() != right_directions.len()
    {
        return Ok(false);
    }
    for node in 0..left_quotient.union.len() {
        let left_root = left_quotient.union.root(node)?;
        let right_root = right_quotient.union.root(node)?;
        if left_quotient.members(left_root).iter().min()
            != right_quotient.members(right_root).iter().min()
            || left_quotient.domains[left_root] != right_quotient.domains[right_root]
        {
            return Ok(false);
        }
    }
    Ok(left_directions
        .iter()
        .zip(right_directions)
        .all(|(left, right)| {
            left.len() == right.len()
                && (0..left.len()).all(|index| {
                    canonical_direction_bit(left, index) == canonical_direction_bit(right, index)
                })
        }))
}

pub(super) fn admit_orientation_option(
    seen: &mut HashMap<u64, Vec<usize>>,
    output: &[MeshOrientationOption],
    directions: &[Vec<bool>],
    quotient: &MeshQuotient,
) -> Result<bool, CodecError> {
    let fingerprint = orientation_fingerprint(quotient, directions)?;
    if let Some(indices) = seen.get(&fingerprint) {
        for &index in indices {
            let (prior_directions, prior_quotient) = &output[index];
            if orientation_options_equivalent(
                quotient,
                directions,
                prior_quotient,
                prior_directions,
            )? {
                return Ok(false);
            }
        }
    }

    seen.entry(fingerprint).or_default().push(output.len());
    Ok(true)
}

/// Search-order information for same-class rows with identical endpoint
/// domains. The dependency order reduces branching overhead without assigning
/// endpoint values to row positions.
pub(super) struct EdgeClassSearchConstraint {
    pub(super) active: Vec<bool>,
    pub(super) ordered: Vec<(usize, usize)>,
}

pub(super) fn edge_class_search_constraint(
    edge_classes: &[usize],
    choices: &[Vec<[usize; 2]>],
) -> Option<EdgeClassSearchConstraint> {
    if edge_classes.len() != choices.len() {
        return None;
    }
    let mut normalized = choices.to_vec();
    for row in &mut normalized {
        for pair in row.iter_mut() {
            pair.sort_unstable_by(Ord::cmp);
        }
        row.sort_unstable_by(Ord::cmp);
        row.dedup();
    }
    let mut active = vec![false; choices.len()];
    let mut ordered = Vec::new();
    for left in 0..choices.len() {
        for right in left + 1..choices.len() {
            if edge_classes[left] != edge_classes[right]
                || normalized[left].len() < 2
                || normalized[left] != normalized[right]
            {
                continue;
            }
            active[left] = true;
            active[right] = true;
            ordered.push((left, right));
        }
    }
    Some(EdgeClassSearchConstraint { active, ordered })
}

pub(super) fn changed_quotient_edges(
    left: &MeshQuotient,
    right: &MeshQuotient,
) -> Result<HashSet<usize>, CodecError> {
    let mut left = left.clone();
    let mut right = right.clone();
    let mut changed = HashSet::new();
    for node in 0..left.union.len() {
        let left_root = left.union.find(node)?;
        let right_root = right.union.find(node)?;
        if left_root != right_root
            || left.members(left_root) != right.members(right_root)
            || left.domains[left_root] != right.domains[right_root]
        {
            changed.insert(node / 2);
        }
    }
    Ok(changed)
}

pub(super) struct MeshSelectionSearch<'a, 'ctx> {
    pub(super) ctx: &'a DecodeContext<'ctx>,
    pub(super) assignments: &'a [Vec<MeshFaceBoundaryAssignment>],
    #[cfg(test)]
    pub(super) possible_face_equations: Vec<Vec<[usize; 2]>>,
    pub(super) possible_face_choices: Vec<Vec<Vec<[usize; 2]>>>,
    pub(super) face_work: Vec<Option<usize>>,
    pub(super) edge_candidates: &'a [Vec<[usize; 2]>],
    pub(super) edge_rows: &'a [EdgeRow],
    pub(super) vertex_points: &'a [[f64; 3]],
    pub(super) candidate_gauge: Option<MeshCandidateGauge<'a>>,
    pub(super) port_identities: Option<&'a [[u32; 2]]>,
    pub(super) fixed_face_directions: Vec<Option<MeshFaceDirectionOptions>>,
    pub(super) fixed_edge_orientations: Vec<Option<bool>>,
    pub(super) edge_has_fixed_direction: Vec<bool>,
    pub(super) selected: Vec<MeshFaceSelection>,
    pub(super) visited_states: HashSet<MeshSelectionStateSignature>,
    pub(super) outcome: SearchOutcome<(StandardTopologyDraft, Vec<usize>)>,
    pub(super) face_equation_cache: MeshFaceEquationCache,
}

pub(super) fn possible_face_equations(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
) -> Vec<Vec<[usize; 2]>> {
    fn ports(use_: MeshBoundaryEdgeCandidate, end: bool) -> [Option<usize>; 2] {
        let port = |reversed: bool| {
            use_.edge.checked_mul(2)?.checked_add(usize::from(if end {
                !reversed
            } else {
                reversed
            }))
        };
        match use_.reversed {
            Some(reversed) => [port(reversed), None],
            None => [port(false), port(true)],
        }
    }

    let mut faces_equations = Vec::new();
    for assignments in faces {
        let mut equations = HashSet::new();
        for assignment in assignments {
            for boundary in &assignment.boundaries {
                if boundary.is_empty() {
                    continue;
                }
                for index in 0..boundary.len() {
                    let left = ports(boundary[index], true);
                    let right = ports(boundary[(index + 1) % boundary.len()], false);
                    for left in left.into_iter().flatten() {
                        for right in right.into_iter().flatten() {
                            equations.insert(if left <= right {
                                [left, right]
                            } else {
                                [right, left]
                            });
                        }
                    }
                }
            }
        }
        let mut face_equations = Vec::new();
        for equation in equations {
            face_equations.push(equation);
        }
        face_equations.sort_unstable_by(Ord::cmp);
        faces_equations.push(face_equations);
    }
    faces_equations
}

pub(super) fn possible_face_choices_within(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
    budget: &WorkBudget,
    faces_choices: &mut Vec<Vec<Vec<[usize; 2]>>>,
) -> bool {
    for (assignments, fallback) in faces.iter().zip(face_equations) {
        let mut choices = HashSet::new();
        for assignment in assignments {
            if !budget.charge() {
                return false;
            }
            let unknown = assignment
                .boundaries
                .iter()
                .flatten()
                .filter(|use_| use_.reversed.is_none())
                .count();
            let combinations = u32::try_from(unknown)
                .ok()
                .and_then(|unknown| 1usize.checked_shl(unknown));
            let Some(combinations) = combinations.filter(|combinations| *combinations <= 4_096)
            else {
                choices.clear();
                let copied = fallback.clone();
                choices.insert(copied);
                break;
            };
            'masks: for mask in 0..combinations {
                if !budget.charge() {
                    return false;
                }
                let mut variable = 0usize;
                let mut directions = Vec::new();
                for boundary in &assignment.boundaries {
                    let mut row = Vec::new();
                    for use_ in boundary {
                        let direction = use_.reversed.unwrap_or_else(|| {
                            let shift = unknown - variable - 1;
                            variable += 1;
                            mask & (1usize << shift) != 0
                        });
                        row.push(direction);
                    }
                    directions.push(row);
                }
                let mut equations = Vec::new();
                for (boundary, row) in assignment.boundaries.iter().zip(&directions) {
                    for index in 0..boundary.len() {
                        let next = (index + 1) % boundary.len();
                        let Some(left) = port(boundary[index], row[index], true) else {
                            continue 'masks;
                        };
                        let Some(right) = port(boundary[next], row[next], false) else {
                            continue 'masks;
                        };
                        equations.push(if left <= right {
                            [left, right]
                        } else {
                            [right, left]
                        });
                    }
                }
                equations.sort_unstable_by(Ord::cmp);
                equations.dedup();
                choices.insert(equations);
            }
        }
        let mut face_choices = Vec::new();
        for choice in choices {
            face_choices.push(choice);
        }
        face_choices.sort_unstable_by(Ord::cmp);
        faces_choices.push(face_choices);
    }
    !budget.exhausted()
}

#[cfg(test)]
pub(super) fn possible_face_choices(
    faces: &[Vec<MeshFaceBoundaryAssignment>],
    face_equations: &[Vec<[usize; 2]>],
) -> Vec<Vec<Vec<[usize; 2]>>> {
    let mut choices = Vec::new();
    assert!(
        possible_face_choices_within(
            faces,
            face_equations,
            &WorkBudget::new(usize::MAX),
            &mut choices
        ),
        "unbounded test face-choice materialization"
    );
    choices
}

pub(super) fn deduplicate_mesh_quotient_assignments(faces: &mut [Vec<MeshFaceBoundaryAssignment>]) {
    fn canonical_cycle(boundary: &[MeshBoundaryEdgeCandidate]) -> Vec<(usize, Option<bool>)> {
        let mut forward = Vec::new();
        let mut reversed = Vec::new();
        for use_ in boundary {
            forward.push((use_.edge, use_.reversed));
        }
        for use_ in boundary.iter().rev() {
            reversed.push((use_.edge, use_.reversed.map(|value| !value)));
        }
        let mut best = None;
        for values in [&forward, &reversed] {
            for start in 0..values.len() {
                let candidate = values[start..].iter().chain(&values[..start]);
                if best.is_none_or(|(best_values, best_start): (&Vec<_>, usize)| {
                    candidate
                        .clone()
                        .cmp(
                            best_values[best_start..]
                                .iter()
                                .chain(&best_values[..best_start]),
                        )
                        .is_lt()
                }) {
                    best = Some((values, start));
                }
            }
        }
        let mut canonical = Vec::new();
        if let Some((values, start)) = best {
            for &value in values[start..].iter().chain(&values[..start]) {
                canonical.push(value);
            }
        }
        canonical
    }

    for assignments in faces {
        let mut seen = HashSet::new();
        assignments.retain(|assignment| {
            let mut signature = Vec::new();
            for boundary in &assignment.boundaries {
                signature.push(canonical_cycle(boundary));
            }
            signature.sort_unstable_by(Ord::cmp);
            seen.insert(signature)
        });
    }
}

#[test]
fn same_class_edges_with_equal_choices_are_ordered_as_one_pair() {
    let choices = vec![vec![[0, 1], [1, 2]], vec![[0, 1], [1, 2]]];
    assert_eq!(
        edge_class_search_constraint(&[0, 0], &choices)
            .expect("matching edge classes")
            .ordered,
        vec![(0, 1)]
    );
}

#[test]
fn merging_an_edges_endpoints_reports_that_edge_as_changed() {
    let points = Arc::new(HashSet::from([0]));
    let left = MeshQuotient::new(vec![Arc::clone(&points), Arc::clone(&points)]);
    let mut right = left.clone();
    assert!(right.merge(0, 1).expect("merge the components").is_some());
    assert_eq!(
        changed_quotient_edges(&left, &right).expect("compute the changed quotient edges"),
        HashSet::from([0])
    );
}
