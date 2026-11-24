// SPDX-License-Identifier: Apache-2.0
//! Per-entity tolerances from the uncertainty of each representation context.

use std::collections::HashMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, LoopId, RegionId, ShellId, VertexId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::ShellMember;

use super::TopologyData;
use crate::loss::StepLossCode;
use crate::parse::Exchange;
use crate::reader::geometry::{
    is_representation_record, representation_context, ContextUncertainties,
};

/// Give each face, edge, and vertex the linear uncertainty of the
/// representation contexts whose representations hold its body, and report
/// the disagreeing context values that reach no topology.
///
/// An entity under several contexts takes the largest of their values. An
/// entity keeps a larger tolerance it already carries. A value equal to
/// `tolerances.linear` leaves the entity on the document tolerance.
pub(in crate::reader) fn carry_context_uncertainties(
    exchange: &Exchange,
    ir: &mut CadIr,
    uncertainties: &ContextUncertainties,
    topology: &TopologyData,
    losses: &mut Vec<LossNote>,
) {
    let mut body_bounds = HashMap::<BodyId, PositiveLength>::new();
    for (&id, record) in exchange.records() {
        if !is_representation_record(record) {
            continue;
        }
        let Some(candidates) = representation_context(record)
            .and_then(|context| uncertainties.by_context.get(&context))
        else {
            continue;
        };
        let Some(largest) = candidates.last() else {
            continue;
        };
        let held = crate::reader::representation::items(record)
            .into_iter()
            .flatten()
            .chain(std::iter::once(id))
            .filter_map(|root| topology.body_by_root.get(&root))
            .flatten();
        for body in held {
            raise(&mut body_bounds, body.clone(), *largest);
        }
    }
    // A value is carried only when some body keeps it: a body under several
    // contexts keeps the largest, and the smaller values reach none of it.
    let carried = body_bounds.values().copied().collect::<Vec<_>>();
    if !body_bounds.is_empty() {
        apply_body_bounds(ir, &body_bounds);
    }
    if let Some((values, unresolved)) = &uncertainties.disagreement {
        report_disagreement(ir, values, *unresolved, &carried, losses);
    }
}

fn raise<K: std::hash::Hash + Eq>(
    bounds: &mut HashMap<K, PositiveLength>,
    key: K,
    value: PositiveLength,
) {
    let bound = bounds.entry(key).or_insert(value);
    if value.get() > bound.get() {
        *bound = value;
    }
}

/// Carry each body's bound down its regions, shells, faces, loops, edges,
/// and vertices, then write the bounds that differ from the document
/// tolerance.
fn apply_body_bounds(ir: &mut CadIr, body_bounds: &HashMap<BodyId, PositiveLength>) {
    let geometry = &ir.geometry;
    let mut regions = HashMap::<&RegionId, PositiveLength>::new();
    for region in &geometry.regions {
        if let Some(&bound) = body_bounds.get(&region.body) {
            raise(&mut regions, &region.id, bound);
        }
    }
    let mut shells = HashMap::<&ShellId, PositiveLength>::new();
    let mut edges = HashMap::<EdgeId, PositiveLength>::new();
    let mut vertices = HashMap::<VertexId, PositiveLength>::new();
    for shell in &geometry.shells {
        let Some(&bound) = regions.get(&shell.region) else {
            continue;
        };
        raise(&mut shells, &shell.id, bound);
        for member in &shell.members {
            match member {
                ShellMember::WireEdge { id } => raise(&mut edges, id.clone(), bound),
                ShellMember::FreeVertex { id } => raise(&mut vertices, id.clone(), bound),
            }
        }
    }
    let mut faces = HashMap::<FaceId, PositiveLength>::new();
    for face in &geometry.faces {
        if let Some(&bound) = shells.get(&face.shell) {
            raise(&mut faces, face.id.clone(), bound);
        }
    }
    let mut loops = HashMap::<&LoopId, PositiveLength>::new();
    for loop_ in &geometry.loops {
        let Some(&bound) = faces.get(&loop_.face) else {
            continue;
        };
        raise(&mut loops, &loop_.id, bound);
        for vertex in loop_.vertices() {
            raise(&mut vertices, vertex.clone(), bound);
        }
    }
    for coedge in &geometry.coedges {
        if let Some(&bound) = loops.get(&coedge.owner_loop) {
            raise(&mut edges, coedge.edge.clone(), bound);
        }
    }
    for edge in &geometry.edges {
        let Some(&bound) = edges.get(&edge.id) else {
            continue;
        };
        for vertex in edge.ends.iter().flat_map(|ends| [&ends.start, &ends.end]) {
            raise(&mut vertices, vertex.clone(), bound);
        }
    }

    let document = ir.tolerances.linear;
    let geometry = &mut ir.geometry;
    for face in &mut geometry.faces {
        widen(&mut face.tolerance, faces.get(&face.id), document);
    }
    for edge in &mut geometry.edges {
        widen(&mut edge.tolerance, edges.get(&edge.id), document);
    }
    for vertex in &mut geometry.vertices {
        widen(&mut vertex.tolerance, vertices.get(&vertex.id), document);
    }
}

fn widen(
    tolerance: &mut Option<PositiveLength>,
    bound: Option<&PositiveLength>,
    document: PositiveLength,
) {
    let Some(&bound) = bound else {
        return;
    };
    if bound == document || tolerance.is_some_and(|stated| stated.get() >= bound.get()) {
        return;
    }
    *tolerance = Some(bound);
}

/// Report the disagreeing context values that no face, edge, or vertex
/// carries, and the measures that did not resolve.
fn report_disagreement(
    ir: &CadIr,
    values: &[PositiveLength],
    unresolved: usize,
    carried: &[PositiveLength],
    losses: &mut Vec<LossNote>,
) {
    let uncarried = values
        .iter()
        .filter(|value| !carried.contains(value))
        .collect::<Vec<_>>();
    if uncarried.is_empty() && unresolved == 0 {
        return;
    }
    let list = |values: &mut dyn Iterator<Item = &PositiveLength>| {
        values
            .map(|value| format!("{:?}", value.get()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let default_linear = ir.tolerances.linear.get();
    let listed = list(&mut values.iter());
    let missing = list(&mut uncarried.iter().copied());
    let message = format!(
        "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT records give {} different linear uncertainty values in millimetres ({listed}) and {unresolved} unresolved measure(s); the linear tolerance keeps the default {default_linear:?}; {} value(s) ({missing}) reach no decoded face, edge, or vertex",
        values.len(),
        uncarried.len(),
    );
    losses.push(StepLossCode::UncertaintyLengthAmbiguous.note(message));
}
