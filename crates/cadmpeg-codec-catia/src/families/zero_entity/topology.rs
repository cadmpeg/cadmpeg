//! Topology derived from resolved zero-entity support occurrences.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::math::Point3;

use super::records::ZeroEntitySupportRun;

const ENDPOINT_TOLERANCE: f64 = 2e-3;

/// One sense-oriented support occurrence owned by a face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ZeroEntityOrientedOccurrence {
    pub(crate) face_record_ordinal: u32,
    pub(crate) support_record_ordinal: u32,
    pub(crate) model_endpoints: [Point3; 2],
}

/// One physical-edge candidate established by two radial occurrences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ZeroEntityPhysicalEdgeCandidate {
    pub(crate) face_record_ordinals: [u32; 2],
    pub(crate) support_record_ordinals: [u32; 2],
    pub(crate) model_endpoints: [Point3; 2],
}

pub(crate) fn zero_entity_physical_edge_candidates(
    runs: &[ZeroEntitySupportRun],
) -> Vec<ZeroEntityPhysicalEdgeCandidate> {
    let occurrences = runs
        .iter()
        .filter_map(|run| run.face.as_ref())
        .flat_map(|face| {
            face.loops.iter().flat_map(|loop_record| {
                loop_record
                    .support_record_ordinals
                    .iter()
                    .copied()
                    .zip(loop_record.oriented_model_endpoints.iter().copied())
                    .map(
                        |(support_record_ordinal, model_endpoints)| ZeroEntityOrientedOccurrence {
                            face_record_ordinal: face.record_ordinal,
                            support_record_ordinal,
                            model_endpoints,
                        },
                    )
            })
        })
        .collect::<Vec<_>>();
    physical_edge_candidates(&occurrences)
}

pub(crate) fn physical_edge_candidates(
    occurrences: &[ZeroEntityOrientedOccurrence],
) -> Vec<ZeroEntityPhysicalEdgeCandidate> {
    let matches = coincident_occurrence_graph(occurrences);
    let face_indices = occurrences
        .iter()
        .map(|occurrence| occurrence.face_record_ordinal)
        .collect::<HashSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, ordinal)| (ordinal, index))
        .collect::<HashMap<_, _>>();
    let mut face_components = DisjointSet::new(face_indices.len());
    for (index, neighbors) in matches.iter().enumerate() {
        let [neighbor] = neighbors.as_slice() else {
            continue;
        };
        if matches[*neighbor].as_slice() != [index] {
            continue;
        }
        face_components.union(
            face_indices[&occurrences[index].face_record_ordinal],
            face_indices[&occurrences[*neighbor].face_record_ordinal],
        );
    }

    let mut visited = vec![false; occurrences.len()];
    let mut candidates = Vec::new();
    for start in 0..occurrences.len() {
        if visited[start] {
            continue;
        }
        let mut stack = vec![start];
        visited[start] = true;
        let mut group = Vec::new();
        while let Some(index) = stack.pop() {
            group.push(index);
            for neighbor in &matches[index] {
                if !visited[*neighbor] {
                    visited[*neighbor] = true;
                    stack.push(*neighbor);
                }
            }
        }
        let mut by_face_component = BTreeMap::<usize, Vec<usize>>::new();
        for index in group {
            let component =
                face_components.find(face_indices[&occurrences[index].face_record_ordinal]);
            by_face_component.entry(component).or_default().push(index);
        }
        for mut pair in by_face_component.into_values() {
            if pair.len() != 2 || !matches[pair[0]].contains(&pair[1]) {
                continue;
            }
            pair.sort_by_key(|index| occurrences[*index].support_record_ordinal);
            let [first, second] = [occurrences[pair[0]], occurrences[pair[1]]];
            candidates.push(ZeroEntityPhysicalEdgeCandidate {
                face_record_ordinals: [first.face_record_ordinal, second.face_record_ordinal],
                support_record_ordinals: [
                    first.support_record_ordinal,
                    second.support_record_ordinal,
                ],
                model_endpoints: first.model_endpoints,
            });
        }
    }
    candidates.sort_by_key(|candidate| candidate.support_record_ordinals);
    candidates
}

fn coincident_occurrence_graph(occurrences: &[ZeroEntityOrientedOccurrence]) -> Vec<Vec<usize>> {
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    for (index, occurrence) in occurrences.iter().enumerate() {
        for endpoint in occurrence.model_endpoints {
            cells
                .entry(endpoint_cell(endpoint))
                .or_default()
                .push(index);
        }
    }
    let mut matches = vec![Vec::new(); occurrences.len()];
    for (index, occurrence) in occurrences.iter().enumerate() {
        let mut possible = HashSet::new();
        for endpoint in occurrence.model_endpoints {
            let cell = endpoint_cell(endpoint);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let neighbor = [
                            cell[0].saturating_add(dx),
                            cell[1].saturating_add(dy),
                            cell[2].saturating_add(dz),
                        ];
                        if let Some(indices) = cells.get(&neighbor) {
                            possible.extend(indices.iter().copied().filter(|other| *other > index));
                        }
                    }
                }
            }
        }
        for other in possible {
            if unordered_endpoints_match(
                occurrence.model_endpoints,
                occurrences[other].model_endpoints,
            ) {
                matches[index].push(other);
                matches[other].push(index);
            }
        }
    }
    for neighbors in &mut matches {
        neighbors.sort_unstable();
    }
    matches
}

fn endpoint_cell(point: Point3) -> [i64; 3] {
    [
        (point.x / ENDPOINT_TOLERANCE).floor() as i64,
        (point.y / ENDPOINT_TOLERANCE).floor() as i64,
        (point.z / ENDPOINT_TOLERANCE).floor() as i64,
    ]
}

fn unordered_endpoints_match(left: [Point3; 2], right: [Point3; 2]) -> bool {
    let direct = left[0].distance(right[0]).max(left[1].distance(right[1]));
    let reversed = left[0].distance(right[1]).max(left[1].distance(right[0]));
    direct.min(reversed) <= ENDPOINT_TOLERANCE
}

struct DisjointSet {
    parents: Vec<usize>,
}

impl DisjointSet {
    fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
        }
    }

    fn find(&mut self, index: usize) -> usize {
        if self.parents[index] != index {
            self.parents[index] = self.find(self.parents[index]);
        }
        self.parents[index]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parents[right] = left;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occurrence(
        face_record_ordinal: u32,
        support_record_ordinal: u32,
        model_endpoints: [Point3; 2],
    ) -> ZeroEntityOrientedOccurrence {
        ZeroEntityOrientedOccurrence {
            face_record_ordinal,
            support_record_ordinal,
            model_endpoints,
        }
    }

    #[test]
    fn coincident_edges_partition_by_face_incidence_components() {
        let coincident = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let first_link = [Point3::new(0.0, 1.0, 0.0), Point3::new(1.0, 1.0, 0.0)];
        let second_link = [Point3::new(0.0, 2.0, 0.0), Point3::new(1.0, 2.0, 0.0)];
        let occurrences = [
            occurrence(10, 1, coincident),
            occurrence(11, 2, coincident),
            occurrence(12, 3, coincident),
            occurrence(13, 4, coincident),
            occurrence(10, 5, first_link),
            occurrence(11, 6, first_link),
            occurrence(12, 7, second_link),
            occurrence(13, 8, second_link),
        ];

        let candidates = physical_edge_candidates(&occurrences);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.support_record_ordinals)
                .collect::<Vec<_>>(),
            [[1, 2], [3, 4], [5, 6], [7, 8]]
        );
        assert!(physical_edge_candidates(&occurrences[..4]).is_empty());
    }

    #[test]
    fn radial_match_crosses_spatial_cells_and_accepts_reversed_endpoints() {
        let occurrences = [
            occurrence(
                10,
                1,
                [Point3::new(-0.000_1, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            ),
            occurrence(
                11,
                2,
                [
                    Point3::new(1.001_9, 0.0, 0.0),
                    Point3::new(0.001_8, 0.0, 0.0),
                ],
            ),
        ];

        assert_eq!(
            physical_edge_candidates(&occurrences)[0].support_record_ordinals,
            [1, 2]
        );
    }
}
