// SPDX-License-Identifier: Apache-2.0
//! The topology rules of part 10 on a resolved state (§5.4, §5.6): each
//! stored child list is reciprocal with its children's parent pointers (D29),
//! and an edge with no vertex lies on a closed carrier with no parameter
//! range and is the only coedge of each loop that uses it.
//!
//! A breach is reported once along a chain, against the evaluation whose delta
//! makes it: its own delta holds one of the records the breach involves, and
//! its base state does not have the breach. A breach its base state has is the
//! base's: the base evaluation reports it, or the current-geometry topology
//! checks do. A reference to a record the state does not hold is the closure
//! rule's.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::{Arena, ResolvedState};
use crate::document::Topology;
use crate::geometry::{Curve, ProceduralCurve};
use crate::report::check::Finding;
use crate::scalar::PositiveLength;
use crate::topology::{Coedge, Edge, Face, Loop, Region, Shell};
use crate::validate::geometry_consistency::vertexless_edge_on_open_carrier;

/// A record of a state, and whether the evaluation's own delta holds a record
/// of its identity.
struct Held<'a, T> {
    record: &'a T,
    own: bool,
}

/// The records of the resolved state the rules read, by identity.
#[derive(Default)]
struct Records<'a> {
    regions: BTreeMap<&'a str, Held<'a, Region>>,
    shells: BTreeMap<&'a str, Held<'a, Shell>>,
    faces: BTreeMap<&'a str, Held<'a, Face>>,
    loops: BTreeMap<&'a str, Held<'a, Loop>>,
    coedges: BTreeMap<&'a str, Held<'a, Coedge>>,
    edges: BTreeMap<&'a str, Held<'a, Edge>>,
    curves: BTreeMap<&'a str, Held<'a, Curve>>,
    procedural_curves: BTreeMap<&'a str, Held<'a, ProceduralCurve>>,
}

fn hold<'a, T>(
    into: &mut BTreeMap<&'a str, Held<'a, T>>,
    id: &'a str,
    record: Option<&'a T>,
    own: bool,
) {
    if let Some(record) = record {
        into.insert(id, Held { record, own });
    }
}

impl<'a> Records<'a> {
    fn of(state: &ResolvedState<'a>, restated: &BTreeSet<&str>) -> Self {
        let mut records = Self::default();
        for (&id, located) in &state.records {
            let own = restated.contains(id);
            let (topology, at) = (located.topology, located.index);
            match located.arena {
                Arena::Region => hold(&mut records.regions, id, topology.regions.get(at), own),
                Arena::Shell => hold(&mut records.shells, id, topology.shells.get(at), own),
                Arena::Face => hold(&mut records.faces, id, topology.faces.get(at), own),
                Arena::Loop => hold(&mut records.loops, id, topology.loops.get(at), own),
                Arena::Coedge => hold(&mut records.coedges, id, topology.coedges.get(at), own),
                Arena::Edge => hold(&mut records.edges, id, topology.edges.get(at), own),
                Arena::Curve => hold(&mut records.curves, id, topology.curves.get(at), own),
                Arena::ProceduralCurve => hold(
                    &mut records.procedural_curves,
                    id,
                    topology.procedural_curves.get(at),
                    own,
                ),
                Arena::Body
                | Arena::Vertex
                | Arena::Point
                | Arena::Surface
                | Arena::Subd
                | Arena::Pcurve
                | Arena::ProceduralSurface => {}
            }
        }
        records
    }
}

/// The breaches of a state that involve a record the evaluation's own delta
/// holds, in the order found. A breach is identified by its message, which
/// names its records and its direction.
#[derive(Default)]
struct Breaches(Vec<String>);

impl Breaches {
    fn breach(&mut self, own: bool, message: fmt::Arguments<'_>) {
        if own {
            self.0.push(message.to_string());
        }
    }
}

pub(super) fn check_state_topology(
    ctx: &DecodeContext<'_>,
    linear: PositiveLength,
    base: &ResolvedState<'_>,
    state: &ResolvedState<'_>,
    delta: &Topology,
    evaluation: &str,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let restated: BTreeSet<&str> = state
        .records
        .iter()
        .filter(|(_, located)| std::ptr::eq(located.topology, delta))
        .map(|(&id, _)| id)
        .collect();
    if restated.is_empty() {
        return Ok(());
    }
    let mut inherited = Breaches::default();
    check_topology(ctx, linear, &Records::of(base, &restated), &mut inherited)?;
    let inherited: BTreeSet<String> = inherited.0.into_iter().collect();
    let mut breaches = Breaches::default();
    check_topology(ctx, linear, &Records::of(state, &restated), &mut breaches)?;
    for message in breaches.0 {
        if !inherited.contains(&message) {
            super::super::error(findings, evaluation, format_args!("{message}"));
        }
    }
    Ok(())
}

fn check_topology(
    ctx: &DecodeContext<'_>,
    linear: PositiveLength,
    records: &Records<'_>,
    breaches: &mut Breaches,
) -> Result<(), CodecError> {
    check_child_lists(records, breaches);
    check_vertexless_edges(ctx, linear, records, breaches)
}

/// A region's `shells`, a face's `loops`, and a loop ring's `coedges` name
/// children whose parent pointer names their lister, and each child is listed
/// by the record its parent pointer names. A face lists a loop once, and a
/// shell lists a wire edge or free vertex once.
fn check_child_lists(records: &Records<'_>, breaches: &mut Breaches) {
    for (&id, region) in &records.regions {
        for shell in &region.record.shells {
            let shell = shell.as_str();
            if let Some(child) = records.shells.get(shell) {
                if child.record.region.as_str() != id {
                    breaches.breach(
                        region.own || child.own,
                        format_args!(
                            "region {id} lists shell {shell}, whose parent is another record"
                        ),
                    );
                }
            }
        }
    }
    for (&id, face) in &records.faces {
        let mut seen = BTreeSet::new();
        for loop_ in &face.record.loops {
            let loop_ = loop_.as_str();
            if !seen.insert(loop_) {
                breaches.breach(
                    face.own,
                    format_args!("face {id} lists loop {loop_} more than once"),
                );
            }
            if let Some(child) = records.loops.get(loop_) {
                if child.record.face.as_str() != id {
                    breaches.breach(
                        face.own || child.own,
                        format_args!(
                            "face {id} lists loop {loop_}, whose parent is another record"
                        ),
                    );
                }
            }
        }
    }
    for (&id, loop_) in &records.loops {
        for coedge in loop_.record.coedges() {
            let coedge = coedge.as_str();
            if let Some(child) = records.coedges.get(coedge) {
                if child.record.owner_loop.as_str() != id {
                    breaches.breach(
                        loop_.own || child.own,
                        format_args!(
                            "loop {id} lists coedge {coedge}, whose parent is another record"
                        ),
                    );
                }
            }
        }
    }
    for (&id, shell) in &records.shells {
        let region = shell.record.region.as_str();
        if let Some(parent) = records.regions.get(region) {
            if !parent.record.shells.contains(&shell.record.id) {
                breaches.breach(
                    shell.own || parent.own,
                    format_args!("shell {id} is not listed by its region {region}"),
                );
            }
        }
        let mut seen = BTreeSet::new();
        for member in shell
            .record
            .wire_edges()
            .map(crate::ids::EdgeId::as_str)
            .chain(
                shell
                    .record
                    .free_vertices()
                    .map(crate::ids::VertexId::as_str),
            )
        {
            if !seen.insert(member) {
                breaches.breach(
                    shell.own,
                    format_args!("shell {id} lists member {member} more than once"),
                );
            }
        }
    }
    for (&id, loop_) in &records.loops {
        let face = loop_.record.face.as_str();
        if let Some(parent) = records.faces.get(face) {
            if !parent.record.loops.contains(&loop_.record.id) {
                breaches.breach(
                    loop_.own || parent.own,
                    format_args!("loop {id} is not listed by its face {face}"),
                );
            }
        }
    }
    for (&id, coedge) in &records.coedges {
        let owner = coedge.record.owner_loop.as_str();
        if let Some(parent) = records.loops.get(owner) {
            if !parent.record.coedges().contains(&coedge.record.id) {
                breaches.breach(
                    coedge.own || parent.own,
                    format_args!("coedge {id} is not listed by its loop {owner}"),
                );
            }
        }
    }
}

/// An edge with no vertex has a carrier curve, states no parameter range,
/// and lies on a closed carrier; each of its coedges is the only coedge of
/// its loop.
fn check_vertexless_edges(
    ctx: &DecodeContext<'_>,
    linear: PositiveLength,
    records: &Records<'_>,
    breaches: &mut Breaches,
) -> Result<(), CodecError> {
    for (&id, edge) in &records.edges {
        if edge.record.ends.is_some() {
            continue;
        }
        let Some(curve_id) = edge
            .record
            .curve()
            .filter(|_| edge.record.param_range().is_none())
        else {
            breaches.breach(
                edge.own,
                format_args!(
                    "edge {id} has no vertex, so it needs a carrier curve and no parameter range"
                ),
            );
            continue;
        };
        let Some(curve) = records.curves.get(curve_id.as_str()) else {
            continue;
        };
        let procedural = curve
            .record
            .geometry
            .procedural_construction()
            .and_then(|construction| records.procedural_curves.get(construction.as_str()));
        let own = edge.own || curve.own || procedural.is_some_and(|procedural| procedural.own);
        let cache_fit_tolerance =
            procedural.and_then(|procedural| procedural.record.cache_fit_tolerance());
        if own
            && vertexless_edge_on_open_carrier(
                ctx,
                linear,
                edge.record,
                curve.record,
                cache_fit_tolerance,
            )?
        {
            breaches.breach(
                own,
                format_args!(
                    "edge {id} has no vertex and lies on an open carrier curve {}",
                    curve_id.as_str()
                ),
            );
        }
    }
    for (&id, coedge) in &records.coedges {
        let edge_id = coedge.record.edge.as_str();
        let Some(edge) = records
            .edges
            .get(edge_id)
            .filter(|edge| edge.record.ends.is_none())
        else {
            continue;
        };
        let owner = coedge.record.owner_loop.as_str();
        let Some(parent) = records.loops.get(owner) else {
            continue;
        };
        if parent.record.coedges().len() != 1 {
            breaches.breach(
                coedge.own || edge.own || parent.own,
                format_args!(
                    "coedge {id} uses edge {edge_id}, which has no vertex, and is not the only coedge of its loop {owner}"
                ),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
