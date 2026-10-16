// SPDX-License-Identifier: Apache-2.0
//! Use pairs of a non-manifold edge in a closed shell.

use cadmpeg_ir::topology::{Coedge, Sense};
use std::collections::{HashMap, VecDeque};

/// Forward and reversed use pairs of the coedges of one edge.
///
/// The coedges are taken in `radial_next` ring order when that walk visits
/// exactly them, else in the given order. From the first reversed coedge on,
/// each coedge pairs with the oldest unpaired coedge of the opposite sense, so
/// in an alternating ring each reversed coedge pairs with its successor.
/// Returns `None` when the forward and reversed counts differ.
pub(super) fn use_pairs<'a>(coedges: &[&'a Coedge]) -> Option<Vec<[&'a Coedge; 2]>> {
    let forward = coedges
        .iter()
        .filter(|coedge| coedge.sense == Sense::Forward)
        .count();
    if forward * 2 != coedges.len() {
        return None;
    }
    let ring = radial_order(coedges).unwrap_or_else(|| coedges.to_vec());
    let start = ring
        .iter()
        .position(|coedge| coedge.sense == Sense::Reversed)?;
    let mut pending_forward = VecDeque::new();
    let mut pending_reversed = VecDeque::new();
    let mut pairs = Vec::with_capacity(forward);
    for coedge in ring[start..].iter().chain(&ring[..start]).copied() {
        let (same, opposite) = match coedge.sense {
            Sense::Forward => (&mut pending_forward, &mut pending_reversed),
            Sense::Reversed => (&mut pending_reversed, &mut pending_forward),
        };
        match opposite.pop_front() {
            Some(partner) => pairs.push([partner, coedge]),
            None => same.push_back(coedge),
        }
    }
    Some(pairs)
}

fn radial_order<'a>(coedges: &[&'a Coedge]) -> Option<Vec<&'a Coedge>> {
    let by_id = coedges
        .iter()
        .map(|coedge| (&coedge.id, *coedge))
        .collect::<HashMap<_, _>>();
    let first = *coedges.first()?;
    let mut ring = vec![first];
    let mut current = first;
    while ring.len() < coedges.len() {
        current = by_id.get(&current.radial_next).copied()?;
        if current.id == first.id {
            return None;
        }
        ring.push(current);
    }
    (current.radial_next == first.id).then_some(ring)
}
