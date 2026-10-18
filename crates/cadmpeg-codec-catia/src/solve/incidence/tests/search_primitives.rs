use crate::solve::missing_edge::MeshFaceBoundaryAssignment;

use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;

use crate::solve::incidence::incidence_choice_components;

use crate::solve::mesh_quotient::MeshQuotient;

use crate::solve::tests::repeated_domain;

use crate::solve::missing_edge::MeshFaceBoundaryDomain;

use crate::solve::incidence::join_incidence_components_by_coupling;

#[test]
fn setting_a_mask_bit_allocates_the_mask_for_a_new_key() {
    use std::collections::HashMap;
    let mut masks = HashMap::<usize, Vec<u64>>::new();
    crate::test_support::with_service_context(|ctx| {
        super::super::set_mask_bit(
            ctx,
            &mut masks,
            3,
            0,
            1,
            1,
            "catia_face_configuration_mask_words",
        )
    })
    .expect("set mask bit");
    assert_eq!(masks.get(&3), Some(&vec![1]));
}

#[test]
fn face_configuration_support_accepts_agreeing_shared_edge_pairs() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
    let fixture = || {
        vec![
            vec![vec![(0, [0, 1])], vec![(0, [0, 1])]],
            vec![vec![(0, [0, 1])]],
        ]
    };
    let run = |ctx: &DecodeContext<'_>| {
        let mut domains = fixture();
        super::super::prune_face_configuration_support(ctx, &mut domains, &WorkBudget::new(1_000))
    };
    assert!(
        crate::test_support::with_service_context(run).expect("prune face configuration support")
    );
}

#[test]
fn face_singleton_support_accepts_agreeing_shared_edge_pairs() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
    let fixture = || {
        vec![
            vec![vec![(0, [0, 1])], vec![(0, [0, 1])]],
            vec![vec![(0, [0, 1])]],
        ]
    };
    let run = |ctx: &DecodeContext<'_>| {
        let mut domains = fixture();
        super::super::prune_face_configuration_singleton_support(
            ctx,
            &mut domains,
            &WorkBudget::new(1_000),
        )
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("prune face configuration singleton support"));
}

#[test]
fn assigning_an_edge_pair_raises_both_point_degrees_on_both_faces() {
    use std::collections::BTreeMap;
    let mut degrees = vec![BTreeMap::new(), BTreeMap::new()];
    let undo = super::super::adjust_incidence_degrees(&mut degrees, &[[0, 1]], 0, [2, 3]);
    assert_eq!(undo.entries.len(), 4);
    assert_eq!(degrees[0].len(), 2);
    assert_eq!(degrees[1].len(), 2);
}

#[test]
fn deferred_boundary_cycle_matches_its_exact_edge_uses() {
    use std::collections::HashSet;
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 2,
        reversed: None,
    };
    let mesh = crate::solve::missing_edge::MeshDeferredBoundaryCycle {
        length: 2,
        exact_uses: vec![(use_(0, 0), 1), (use_(1, 1), 1)],
    };
    let incidence = [(0, false), (1, false)];
    let missing = HashSet::new();
    let run = || super::super::deferred_boundary_cycle_matches(&mesh, &incidence, &missing);
    assert!(run());
}

#[test]
fn partial_assignment_on_a_face_triangle_is_orientable() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
    let edge_faces = [[0, 1], [1, 2], [2, 0]];
    let face_edges = vec![vec![0, 2], vec![0, 1], vec![1, 2]];
    let partial = [Some([0, 1]), Some([1, 2]), None];
    let run = |ctx: &DecodeContext<'_>| {
        super::super::partial_face_orientability_viable(
            ctx,
            &partial,
            &edge_faces,
            &face_edges,
            &WorkBudget::new(10_000),
        )
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("check partial face orientability viable"));
}

#[test]
fn incidence_components_group_edges_whose_choices_share_points() {
    use std::collections::HashSet;
    use std::sync::Arc;
    let choices = [
        vec![[0, 1], [0, 2]],
        vec![[1, 3], [2, 3]],
        vec![[4, 5], [4, 6]],
    ];
    let edge_faces = [[0, 0]; 3];
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let quotient = MeshQuotient::new(
        [
            HashSet::from([0, 1]),
            HashSet::from([0, 2]),
            HashSet::from([1, 3]),
            HashSet::from([3, 4]),
            HashSet::from([6, 7]),
            HashSet::from([6, 8]),
        ]
        .map(Arc::new)
        .to_vec(),
    );

    assert_eq!(
        incidence_choice_components(&choices, &edge_faces, Some(&domains), Some(&quotient))
            .expect("compute the incidence choice components"),
        vec![vec![0, 1], vec![2]],
    );
}

#[test]
fn coupled_edges_join_their_incidence_components() {
    let components = vec![vec![0, 2], vec![1], vec![3, 5], vec![4]];
    let active = [true, false, false, true, false, false];
    let expected = vec![vec![0, 2, 3, 5], vec![1], vec![4]];

    assert_eq!(
        join_incidence_components_by_coupling(components.clone(), &active)
            .expect("join incidence components by coupling"),
        expected,
    );
}

#[test]
fn common_boundary_propagation_succeeds_for_a_single_ordered_face() {
    use cadmpeg_core::decode::DecodeContext;
    use std::collections::HashSet;
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                },
            ]],
        },
    ])];
    let candidates = vec![Vec::new(), Vec::new()];
    let run = |ctx: &DecodeContext<'_>| {
        let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), 4));
        crate::solve::mesh_quotient::propagate_common_boundary_components(
            ctx,
            &domains,
            &candidates,
            &mut quotient,
        )
    };
    assert_eq!(
        crate::test_support::with_service_context(run)
            .expect("propagate common boundary components"),
        Some(())
    );
}

#[test]
fn unordered_cycle_search_closes_a_triangle() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
    use std::collections::HashSet;
    use std::sync::Arc;
    let quotient = MeshQuotient::new(
        [0, 1, 1, 2, 2, 0]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
    );
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(10_000);
        crate::solve::mesh_quotient::bounded_unordered_cycle_assignments(
            ctx,
            &[0, 1, 2],
            &quotient,
            16,
            &budget,
        )
    };
    assert!(!crate::test_support::with_service_context(run)
        .expect("compute the bounded unordered cycle assignments")
        .expect("closed cycle assignments")
        .is_empty());
}

/// Compact-domain viability over a dense table of the selected and assigned
/// endpoint pairs: the specification `compact_boundary_domain_viable` must
/// reproduce.
fn compact_boundary_domain_viable_by_dense_points(
    domain: &MeshFaceBoundaryDomain,
    assignment: &[Option<[usize; 2]>],
    selected: Option<(usize, [usize; 2])>,
) -> bool {
    use crate::solve::union_find::UnionFind;
    use std::collections::{HashMap, HashSet};

    let edges = match domain {
        MeshFaceBoundaryDomain::Ordered(_) => return true,
        MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => edges.clone(),
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            let mut edges = domain.missing_edges.clone();
            edges.extend(
                domain
                    .cycles
                    .iter()
                    .flat_map(|cycle| cycle.exact_uses.iter().map(|(use_, _)| use_.edge)),
            );
            edges
        }
    };
    let selected_pairs = edges
        .iter()
        .map(|&edge| {
            selected
                .filter(|(selected_edge, _)| *selected_edge == edge)
                .map(|(_, pair)| pair)
                .or(assignment[edge])
                .map(|pair| (edge, pair))
        })
        .collect::<Vec<_>>();
    let complete = selected_pairs.iter().all(Option::is_some);
    if matches!(domain, MeshFaceBoundaryDomain::UnorderedFullCycle(_)) && !complete {
        let mut point_nodes = HashMap::new();
        let mut degrees = Vec::<u8>::new();
        let mut components = UnionFind::new(0);
        for (_, pair) in selected_pairs.iter().flatten().copied() {
            let mut nodes = [0; 2];
            for (slot, point) in pair.into_iter().enumerate() {
                nodes[slot] = if let Some(&node) = point_nodes.get(&point) {
                    node
                } else {
                    let node = components.push();
                    degrees.push(0);
                    point_nodes.insert(point, node);
                    node
                };
            }
            for &node in &nodes {
                if degrees[node] >= 2 {
                    return false;
                }
                degrees[node] += 1;
            }
            if nodes[0] != nodes[1] {
                components.union(nodes[0], nodes[1]).expect("union nodes");
            }
        }
        let mut open_components = HashSet::new();
        for (node, degree) in degrees.into_iter().enumerate() {
            if degree < 2 {
                open_components.insert(components.find(node).expect("find node"));
            }
        }
        return (0..components.len())
            .all(|node| open_components.contains(&components.find(node).expect("find node")));
    }
    if !complete {
        return true;
    }
    let mut edge_points = vec![[0; 2]; assignment.len()];
    for (edge, pair) in selected_pairs.into_iter().flatten() {
        edge_points[edge] = pair;
    }
    match domain {
        MeshFaceBoundaryDomain::Ordered(_) => true,
        MeshFaceBoundaryDomain::UnorderedFullCycle(edges) => {
            crate::families::standard::topology::incidence_cycles(edges, &edge_points)
                .expect("compute the incidence cycles")
                .is_some_and(|cycles| cycles.len() == 1)
        }
        MeshFaceBoundaryDomain::DeferredValidation(domain) => {
            super::super::deferred_boundary_closes(domain, &edge_points)
                .expect("check that the deferred boundary closes")
        }
    }
}

struct SplitMix(u64);

impl SplitMix {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        usize::try_from(value % u64::try_from(bound).expect("bound fits u64"))
            .expect("value below bound fits usize")
    }
}

#[test]
fn compact_boundary_domain_viability_matches_a_dense_point_table() {
    use crate::solve::missing_edge::{MeshDeferredBoundaryCycle, MeshDeferredFaceBoundary};

    let mut random = SplitMix(0xc0de);
    let mut outcomes = [0usize; 2];
    for case in 0..20_000 {
        let edge_count = 1 + random.below(7);
        let point_count = 1 + random.below(5);
        let edges = (0..random.below(edge_count + 2))
            .map(|_| random.below(edge_count))
            .collect::<Vec<_>>();
        let domain = if case % 2 == 0 {
            MeshFaceBoundaryDomain::UnorderedFullCycle(edges)
        } else {
            let missing_edges = edges;
            let mut cycles = Vec::new();
            for _ in 0..=random.below(2) {
                let length = 1 + random.below(6);
                let mut exact_uses = Vec::new();
                for _ in 0..random.below(3) {
                    let start = random.below(length);
                    let span = 1 + random.below(length);
                    exact_uses.push((
                        MeshBoundaryEdgeCandidate {
                            edge: random.below(edge_count),
                            start,
                            end: (start + span) % length,
                            reversed: None,
                        },
                        span,
                    ));
                }
                cycles.push(MeshDeferredBoundaryCycle { length, exact_uses });
            }
            MeshFaceBoundaryDomain::DeferredValidation(MeshDeferredFaceBoundary {
                cycles,
                missing_edges,
            })
        };
        let mut assignment = Vec::new();
        for _ in 0..edge_count {
            let pair = [random.below(point_count), random.below(point_count)];
            assignment.push((random.below(4) != 0).then_some(pair));
        }
        let selected_edge = random.below(edge_count);
        let selected_pair = [random.below(point_count), random.below(point_count)];
        let selected = (random.below(2) == 0).then_some((selected_edge, selected_pair));
        let expected =
            compact_boundary_domain_viable_by_dense_points(&domain, &assignment, selected);
        let actual = super::super::compact_boundary_domain_viable(&domain, &assignment, selected)
            .expect("check that the compact boundary domain is viable");
        outcomes[usize::from(expected)] += 1;
        assert_eq!(
            actual, expected,
            "{domain:?} with {assignment:?} and {selected:?}"
        );
    }
    assert!(
        outcomes.iter().all(|count| *count > 2_000),
        "outcomes {outcomes:?}"
    );
}

#[test]
fn dead_incidence_states_match_a_set_of_endpoint_pair_vectors() {
    use super::super::{DeadIncidenceKey, DeadIncidenceStates};
    use std::collections::HashSet;

    let narrow_limit = usize::try_from(u32::MAX).expect("u32 fits in usize");
    let points = [0, 1, 2, narrow_limit - 1, narrow_limit, narrow_limit + 1];
    let pair = |random: &mut SplitMix| {
        (random.below(4) != 0).then(|| {
            [
                points[random.below(points.len())],
                points[random.below(points.len())],
            ]
        })
    };
    let mut random = SplitMix(0xdead);
    let mut outcomes = [0usize; 2];
    let mut wide_keys = 0usize;
    for _ in 0..2_000 {
        let assignment_len = 1 + random.below(3);
        let edges = (0..random.below(assignment_len + 1))
            .map(|_| random.below(assignment_len))
            .collect::<Vec<_>>();
        let mut assignment = (0..assignment_len)
            .map(|_| pair(&mut random))
            .collect::<Vec<_>>();
        let mut states = DeadIncidenceStates::default();
        let mut expected_states = HashSet::new();
        for _ in 0..20 {
            let edge = random.below(assignment_len);
            assignment[edge] = pair(&mut random);
            let expected_state = edges
                .iter()
                .map(|&edge| assignment[edge])
                .collect::<Vec<_>>();
            let expected_live = !expected_states.contains(&expected_state);
            let key = states.live_key(&edges, &assignment);
            outcomes[usize::from(expected_live)] += 1;
            assert_eq!(
                key.is_some(),
                expected_live,
                "{expected_state:?} against {expected_states:?}"
            );
            if let Some(key) = key {
                wide_keys += usize::from(matches!(key, DeadIncidenceKey::Wide(_)));
                if random.below(2) == 0 {
                    states.insert(key);
                    expected_states.insert(expected_state);
                }
            }
        }
    }
    assert!(
        outcomes.iter().all(|count| *count > 2_000),
        "outcomes {outcomes:?}"
    );
    assert!(wide_keys > 2_000, "only {wide_keys} wide keys");
}
