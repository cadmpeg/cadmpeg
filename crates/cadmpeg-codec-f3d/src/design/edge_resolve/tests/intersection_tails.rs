// SPDX-License-Identifier: Apache-2.0

use super::super::{
    corroborated_triplet_intersection, resolved_edge_candidate_intersection, TripletProof,
};
use crate::records::topology::edge_recipe::{
    DesignEdgeRecipeSelectorClause, DesignEdgeRecipeSelectorContext, DesignTopologyIncident,
    DesignTopologyIncidentSide, DesignTopologyRecipeEntry, DesignTopologyRecipeTriplet,
};
use cadmpeg_core::CodecError;

#[test]
fn disjoint_references_leave_the_remaining_source_unvisited() {
    let tail_visited = std::cell::Cell::new(false);
    let references = [&[17, 18][..], &[19][..], &[17][..]];
    let source = references
        .into_iter()
        .enumerate()
        .map(|(index, reference)| {
            if index == 2 {
                tail_visited.set(true);
            }
            reference
        });
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            resolved_edge_candidate_intersection(ctx, &[], source).unwrap(),
            None
        );
    });
    assert!(!tail_visited.get());
}

fn common_selector(edges: &[i64]) -> DesignEdgeRecipeSelectorContext {
    let triplet = DesignTopologyRecipeTriplet {
        outer: std::num::NonZeroU32::new(3).unwrap(),
        middle: 2,
        incident: Some(DesignTopologyIncident {
            ordinal: 1,
            side: DesignTopologyIncidentSide::Preceding,
        }),
    };
    DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: vec![Some(DesignEdgeRecipeSelectorClause {
            entry: DesignTopologyRecipeEntry {
                selector: 0,
                boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
                topology_triplets: [triplet, triplet],
            },
            triplet_edge_slots: [edges.to_vec(), edges.to_vec()],
        })],
        incidence_matching_edge_slots: Vec::new(),
        boundary_count_matching_edge_slots: Vec::new(),
    }
}

fn contradiction_work(selectors: &[DesignEdgeRecipeSelectorContext]) -> u64 {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            corroborated_triplet_intersection(
                ctx,
                selectors,
                &std::iter::empty(),
                TripletProof::Common
            )
            .unwrap(),
            None
        );
        let Err(CodecError::ResourceLimit(usage)) =
            ctx.charge_work(u64::MAX, "completed triplet work")
        else {
            panic!("completed work observer");
        };
        usage.used
    })
}

#[test]
fn contradictory_triplet_pairs_skip_the_remaining_selector_tail() {
    let mut selectors = vec![common_selector(&[17]), common_selector(&[18])];
    let reached_work = contradiction_work(&selectors);
    selectors.extend(std::iter::repeat_n(common_selector(&[17, 18]), 4096));
    assert_eq!(contradiction_work(&selectors), reached_work);
}

#[test]
fn disjoint_pairs_do_not_constrain_a_later_common_triplet() {
    let mut disjoint = common_selector(&[17]);
    disjoint.clauses[0].as_mut().unwrap().triplet_edge_slots[1] = vec![18];
    let selectors = [disjoint, common_selector(&[19, 19])];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            corroborated_triplet_intersection(
                ctx,
                &selectors,
                &std::iter::empty(),
                TripletProof::Common
            )
            .unwrap(),
            Some(19)
        );
    });
}
