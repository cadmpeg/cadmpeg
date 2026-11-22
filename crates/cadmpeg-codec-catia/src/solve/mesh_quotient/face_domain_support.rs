// SPDX-License-Identifier: Apache-2.0
//! Endpoint-pair and incident-edge support checks for face domains.

use crate::families::standard::topology::{incidence_cycles, StandardTopologyDraft};
use crate::solve::incidence::deferred_boundary_assignment;
use crate::solve::missing_edge::{
    same_unordered_pair, MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment,
    MeshFaceBoundaryDomain,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) fn mesh_candidate_point_pairs(
    ctx: &DecodeContext<'_>,
    topology: &StandardTopologyDraft,
    point_assignment: &[usize],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    let mut pairs = Vec::new();
    for [start, end] in edge_vertices {
        let (Some(start), Some(end)) = (point_assignment.get(start), point_assignment.get(end))
        else {
            return Ok(None);
        };
        pairs.push(Some([*start, *end]));
    }
    Ok(Some(pairs))
}

pub(super) fn endpoint_pairs_respect_candidate_domains(
    pairs: &[Option<[usize; 2]>],
    edge_candidates: &[Vec<[usize; 2]>],
) -> bool {
    if pairs.len() != edge_candidates.len() {
        return false;
    }
    pairs.iter().zip(edge_candidates).all(|(pair, candidates)| {
        let Some(pair) = pair else {
            return true;
        };
        let valid = candidates.is_empty()
            || candidates
                .iter()
                .any(|candidate| same_unordered_pair(*candidate, *pair));
        valid
    })
}

pub(super) fn restore_unique_endpoint_pair_orientations(
    pairs: &[[usize; 2]],
    candidates: &[Vec<[usize; 2]>],
) -> Option<Vec<[usize; 2]>> {
    if pairs.len() != candidates.len() {
        return None;
    }
    let mut oriented = Vec::new();
    for (&pair, candidates) in pairs.iter().zip(candidates) {
        let mut unique = None;
        let mut multiple = false;
        for &candidate in candidates {
            if !same_unordered_pair(candidate, pair) {
                continue;
            }
            if unique.is_some_and(|previous| previous != candidate) {
                multiple = true;
                break;
            }
            unique = Some(candidate);
        }
        let direction = if multiple {
            pair
        } else if let Some(unique) = unique {
            unique
        } else if candidates.is_empty() {
            pair
        } else {
            return None;
        };
        oriented.push(direction);
    }
    Some(oriented)
}

pub(super) fn materialize_boundary_domains(
    domains: &[MeshFaceBoundaryDomain],
    edge_pairs: &[[usize; 2]],
) -> Result<Option<Vec<Vec<MeshFaceBoundaryAssignment>>>, CodecError> {
    let mut materialized = Vec::new();

    for domain in domains {
        let assignments = match domain {
            MeshFaceBoundaryDomain::Ordered(assignments) => {
                let mut copies = Vec::new();

                for assignment in assignments {
                    let boundaries = assignment.boundaries.clone();
                    copies.push(MeshFaceBoundaryAssignment { boundaries });
                }
                copies
            }
            MeshFaceBoundaryDomain::DeferredValidation(domain) => {
                let Some(assignment) = deferred_boundary_assignment(domain, edge_pairs)? else {
                    return Ok(None);
                };
                vec![assignment]
            }
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
                let Some(cycles) = incidence_cycles(edges, edge_pairs)? else {
                    return Ok(None);
                };
                let [cycle] = cycles.as_slice() else {
                    return Ok(None);
                };
                let length = cycle.len();
                let mut boundary = Vec::new();

                for (index, &(edge, reversed)) in cycle.iter().enumerate() {
                    boundary.push(MeshBoundaryEdgeCandidate {
                        edge,
                        start: index,
                        end: (index + 1) % length,
                        reversed: Some(reversed),
                    });
                }
                let boundaries = vec![boundary];
                vec![MeshFaceBoundaryAssignment { boundaries }]
            }
        };
        materialized.push(assignments);
    }
    Ok(Some(materialized))
}

// A concrete face assignment is viable only when each selected incident face
// can host the edge in its retained trim-domain representation. This is a
// necessary incidence check; it does not select an alternate face.
pub(super) fn mesh_domains_have_incident_edge_support(
    edge_faces: &[[usize; 2]],
    domains: &[MeshFaceBoundaryDomain],
) -> bool {
    edge_faces.iter().enumerate().all(|(edge, faces)| {
        faces.iter().all(|face| {
            let Some(domain) = domains.get(*face) else {
                return false;
            };
            match domain {
                MeshFaceBoundaryDomain::Ordered(assignments) => {
                    assignments.iter().any(|assignment| {
                        assignment
                            .boundaries
                            .iter()
                            .flatten()
                            .any(|candidate| candidate.edge == edge)
                    })
                }
                MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => edges.contains(&edge),
                MeshFaceBoundaryDomain::DeferredValidation(coverage) => {
                    coverage.missing_edges.contains(&edge)
                        || coverage
                            .cycles
                            .iter()
                            .flat_map(|cycle| &cycle.exact_uses)
                            .any(|(candidate, _)| candidate.edge == edge)
                }
            }
        })
    })
}

#[cfg(test)]
mod face_domain_support_tests {
    use super::{
        endpoint_pairs_respect_candidate_domains, materialize_boundary_domains,
        mesh_domains_have_incident_edge_support, restore_unique_endpoint_pair_orientations,
    };
    use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
    use crate::solve::missing_edge::MeshDeferredFaceBoundary;
    use crate::solve::missing_edge::MeshFaceBoundaryAssignment;
    use crate::solve::missing_edge::MeshFaceBoundaryDomain;

    fn assignment(edge: usize) -> MeshFaceBoundaryAssignment {
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 1,
                reversed: None,
            }]],
        }
    }

    #[test]
    fn every_concrete_incident_face_must_host_the_edge() {
        let edge_faces = [[0, 1]];
        let valid = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
        ];
        assert!(mesh_domains_have_incident_edge_support(&edge_faces, &valid));

        let wrong_face = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::Ordered(vec![assignment(1)]),
        ];
        assert!(!mesh_domains_have_incident_edge_support(
            &edge_faces,
            &wrong_face
        ));

        let unordered = vec![
            MeshFaceBoundaryDomain::Ordered(vec![assignment(0)]),
            MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0]),
        ];
        assert!(mesh_domains_have_incident_edge_support(
            &edge_faces,
            &unordered
        ));

        let deferred = vec![
            MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
                boundaries: vec![vec![
                    MeshBoundaryEdgeCandidate {
                        edge: 0,
                        start: 0,
                        end: 1,
                        reversed: None,
                    },
                    MeshBoundaryEdgeCandidate {
                        edge: 1,
                        start: 1,
                        end: 2,
                        reversed: None,
                    },
                ]],
            }]),
            MeshFaceBoundaryDomain::DeferredValidation(MeshDeferredFaceBoundary {
                cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                    length: 2,
                    exact_uses: vec![(
                        MeshBoundaryEdgeCandidate {
                            edge: 0,
                            start: 0,
                            end: 1,
                            reversed: Some(false),
                        },
                        1,
                    )],
                }],
                missing_edges: vec![1],
            }),
        ];
        assert!(mesh_domains_have_incident_edge_support(
            &[[0, 1], [0, 1]],
            &deferred
        ));
        assert!(!mesh_domains_have_incident_edge_support(
            &[[0, 1], [0, 1], [0, 1]],
            &deferred
        ));
    }

    #[test]
    fn endpoint_pairs_must_remain_inside_candidate_domains() {
        let candidates = vec![vec![[1, 2], [2, 3]], Vec::new()];
        assert!(endpoint_pairs_respect_candidate_domains(
            &[Some([2, 1]), None],
            &candidates,
        ));
        assert!(!endpoint_pairs_respect_candidate_domains(
            &[Some([0, 2]), None],
            &candidates,
        ));
        assert!(!endpoint_pairs_respect_candidate_domains(
            &[Some([1, 2])],
            &candidates,
        ));
    }

    #[test]
    fn unique_candidate_restores_endpoint_pair_orientation() {
        assert_eq!(
            restore_unique_endpoint_pair_orientations(
                &[[0, 1], [2, 3], [4, 5]],
                &[vec![[1, 0]], vec![[2, 3], [3, 2]], Vec::new()],
            ),
            Some(vec![[1, 0], [2, 3], [4, 5]])
        );
        assert!(restore_unique_endpoint_pair_orientations(&[[0, 1]], &[vec![[2, 3]]]).is_none());
    }

    #[test]
    fn complete_endpoint_pairs_materialize_deferred_boundaries() {
        let domains = [MeshFaceBoundaryDomain::DeferredValidation(
            MeshDeferredFaceBoundary {
                cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                    length: 2,
                    exact_uses: vec![(
                        MeshBoundaryEdgeCandidate {
                            edge: 0,
                            start: 0,
                            end: 1,
                            reversed: Some(false),
                        },
                        1,
                    )],
                }],
                missing_edges: vec![1],
            },
        )];

        let assignments = materialize_boundary_domains(&domains, &[[0, 1], [0, 1]])
            .expect("materialize boundary domains")
            .expect("closed deferred boundary");
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].len(), 1);
        assert_eq!(
            assignments[0][0].boundaries[0]
                .iter()
                .map(|use_| use_.edge)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn complete_endpoint_pairs_materialize_unordered_cycle() {
        let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];
        let assignments = materialize_boundary_domains(&domains, &[[0, 1], [1, 2], [2, 0]])
            .expect("materialize boundary domains")
            .expect("closed unordered boundary");

        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].len(), 1);
        assert_eq!(assignments[0][0].boundaries[0].len(), 3);
        assert!(assignments[0][0].boundaries[0]
            .iter()
            .all(|use_| use_.reversed.is_some()));
    }

    #[test]
    fn ordered_boundary_domain_materializes_without_endpoint_pairs() {
        let domains = [MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 1,
                    reversed: Some(false),
                }]],
            },
        ])];
        assert!(materialize_boundary_domains(&domains, &[])
            .expect("materialize boundary domains")
            .is_some());
    }

    #[test]
    fn unordered_cycle_domain_materializes_with_closing_pairs() {
        let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];
        let pairs = [[0, 1], [1, 2], [2, 0]];
        assert!(materialize_boundary_domains(&domains, &pairs)
            .expect("materialize boundary domains")
            .is_some());
    }
}
