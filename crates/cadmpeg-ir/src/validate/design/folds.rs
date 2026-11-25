// SPDX-License-Identifier: Apache-2.0
//! A `Fold` input's four edges in its stored anchored state (§5.9): straight
//! edges that share one end vertex and lie in one plane.
//!
//! An edge is read where every pick resolves to one edge of the state: by its
//! `resolved` entities, else by a name that matches that one edge. A carrier
//! that is a conic is not straight; the plane is checked through the far end
//! of each edge, within the linear tolerance.

use std::collections::BTreeSet;

use crate::design::construction::{InputReference, Relation};
use crate::design::refs::EntityRef;
use crate::design::selection::{PickTarget, Picked, Picks};
use crate::ids::VertexId;
use crate::math::{Point3, Vector3};
use crate::report::check::Finding;
use crate::topology::{Edge, EdgeCarrier};

use super::fit::{curved, may_be_straight_edge};
use super::index::DesignIndex;
use super::matching::{Matcher, Matchers, StateTopology};
use super::selections::{anchored_states, Site};
use super::Held;

/// Checks the edges of every `Fold` input in each stored state it anchors in.
pub(super) fn check<'a>(
    index: &DesignIndex<'a>,
    matchers: &Matchers<'a>,
    held: &'a [Held<'a>],
    findings: &mut Vec<Finding>,
) {
    let linear = index.records.ir.tolerances.linear.get();
    for record in held {
        for construction in &record.constructions {
            for input in construction.inputs() {
                let folds = input
                    .role
                    .as_ref()
                    .is_some_and(|role| matches!(role.relation, Relation::Fold));
                let (true, Some(InputReference::Picks(selection))) = (folds, &input.reference)
                else {
                    continue;
                };
                let Picks::Named(picks) = &selection.picks else {
                    continue;
                };
                let picks = picks.as_slice();
                if picks.len() != 4 || picks.iter().any(|pick| !may_be_straight_edge(index, pick)) {
                    continue;
                }
                let site = Site::of(index, record, &selection.at);
                let mut messages = BTreeSet::new();
                for state in anchored_states(index, matchers.states(), site) {
                    let Some(matcher) = matchers.get(&state) else {
                        continue;
                    };
                    if let Some(message) = check_fold(&matcher, picks, linear) {
                        messages.insert(message);
                    }
                }
                for message in messages {
                    super::error(
                        findings,
                        record.holder.record(),
                        crate::rule!("design.fold-straight-edge-picks", "{message}"),
                    );
                }
            }
        }
    }
}

fn check_fold(matcher: &Matcher<'_>, picks: &[Picked], linear: f64) -> Option<&'static str> {
    let topology = matcher.topology()?;
    let edges = picks
        .iter()
        .map(|pick| picked_edge(matcher, topology, pick))
        .collect::<Option<Vec<&Edge>>>()?;
    let straight = edges.iter().all(|edge| match &edge.carrier {
        EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _) => topology
            .curves
            .get(curve.as_str())
            .and_then(|curve| curve.geometry.solved())
            .is_none_or(|carrier| !curved(carrier)),
        EdgeCarrier::Free | EdgeCarrier::Endpoints(_) => true,
    });
    if !straight {
        return Some("a Fold reference picks an edge that is not straight");
    }
    let mut shared: Option<BTreeSet<&str>> = None;
    for edge in &edges {
        let ends: BTreeSet<&str> = edge.vertices().map(VertexId::as_str).collect();
        shared = Some(match shared {
            None => ends,
            Some(shared) => shared.intersection(&ends).copied().collect(),
        });
    }
    let Some(&vertex) = shared.as_ref().and_then(|shared| shared.first()) else {
        return Some("a Fold reference's four edges do not share one end vertex");
    };
    let position = |vertex: &str| -> Option<Point3> {
        let point = topology.vertices.get(vertex)?.point.as_str();
        Some(topology.points.get(point)?.position().get())
    };
    let origin = position(vertex)?;
    let spokes = edges
        .iter()
        .map(|edge| {
            let far = edge.vertices().find(|end| end.as_str() != vertex)?;
            Some(position(far.as_str())?.vector_from(origin))
        })
        .collect::<Option<Vec<Vector3>>>()?;
    let mut normal = Vector3::new(0.0, 0.0, 0.0);
    for (at, first) in spokes.iter().enumerate() {
        for second in &spokes[at + 1..] {
            let cross = first.cross(*second);
            if cross.norm() > normal.norm() {
                normal = cross;
            }
        }
    }
    let length = normal.norm();
    if length <= f64::MIN_POSITIVE {
        return None;
    }
    let unit = Vector3::new(normal.x / length, normal.y / length, normal.z / length);
    if spokes.iter().any(|spoke| unit.dot(*spoke).abs() > linear) {
        return Some("a Fold reference's four edges do not lie in one plane");
    }
    None
}

/// The one edge of the state a pick resolves to, where it resolves to one.
fn picked_edge<'a>(
    matcher: &Matcher<'a>,
    topology: &StateTopology<'a>,
    pick: &Picked,
) -> Option<&'a Edge> {
    if pick.context.is_some() {
        return None;
    }
    let id = match (&pick.resolved, &pick.target) {
        (Some(resolved), _) => match resolved.as_slice() {
            [EntityRef::Edge(edge)] => edge.as_str(),
            _ => return None,
        },
        (None, PickTarget::Entity(name)) => {
            let matched = matcher.matched(name);
            if matched.len() != 1 {
                return None;
            }
            matched.first().copied()?
        }
        (None, _) => return None,
    };
    topology.edges.get(id).copied()
}

#[cfg(test)]
mod tests;
