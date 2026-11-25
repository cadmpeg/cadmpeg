// SPDX-License-Identifier: Apache-2.0
//! Face connectivity: the shell partition of faces that share an edge or
//! vertex.

use std::collections::HashSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::topology_transfer::connected_components;

#[test]
fn face_connectivity_partitions_transitively_without_reordering() {
    let sets = [
        HashSet::from(["edge-a".to_owned()]),
        HashSet::from(["edge-b".to_owned()]),
        HashSet::from(["edge-a".to_owned(), "edge-c".to_owned()]),
        HashSet::from(["edge-c".to_owned()]),
        HashSet::new(),
    ];

    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
    assert_eq!(
        connected_components(&ctx, &sets).expect("connected-component allocation"),
        vec![vec![0, 2, 3], vec![1], vec![4]]
    );
    assert!(connected_components(&ctx, &[])
        .expect("connected-component allocation")
        .is_empty());
}

/// The pairwise search `connected_components` replaced, kept as an oracle.
fn pairwise_components(connectivity: &[HashSet<String>]) -> Vec<Vec<usize>> {
    let mut assigned = vec![false; connectivity.len()];
    let mut components = Vec::new();
    for seed in 0..connectivity.len() {
        if assigned[seed] {
            continue;
        }
        assigned[seed] = true;
        let mut component = Vec::new();
        let mut stack = vec![seed];
        while let Some(current) = stack.pop() {
            component.push(current);
            for candidate in 0..connectivity.len() {
                if !assigned[candidate]
                    && !connectivity[current].is_disjoint(&connectivity[candidate])
                {
                    assigned[candidate] = true;
                    stack.push(candidate);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components
}

#[test]
fn face_connectivity_matches_the_pairwise_partition() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = |bound: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state % bound).expect("bounded test value")
    };
    for (faces, key_pool) in [(1, 1), (8, 4), (40, 30), (200, 150), (200, 400), (300, 60)] {
        let sets = (0..faces)
            .map(|_| {
                (0..next(4))
                    .map(|_| format!("edge:{}", next(key_pool)))
                    .collect::<HashSet<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            connected_components(&ctx, &sets).expect("connected-component allocation"),
            pairwise_components(&sets),
            "{faces} faces over {key_pool} keys"
        );
    }
}

#[test]
fn mesh_scale_face_connectivity_is_linear() {
    // Two interleaved strips: face `i` shares an edge with face `i + 2`, and
    // every fourth face also touches one shared hub vertex of its strip. The
    // pairwise search needs about 2^33 set probes for this shell.
    const FACES: usize = 1 << 17;
    let sets = (0..FACES)
        .map(|face| {
            let mut keys = HashSet::from([format!("edge:{face}"), format!("edge:{}", face + 2)]);
            if face % 4 < 2 {
                keys.insert(format!("vertex:hub-{}", face % 2));
            }
            keys
        })
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
    assert_eq!(
        connected_components(&ctx, &sets).expect("connected-component allocation"),
        vec![
            (0..FACES).step_by(2).collect::<Vec<_>>(),
            (1..FACES).step_by(2).collect::<Vec<_>>(),
        ]
    );
}
