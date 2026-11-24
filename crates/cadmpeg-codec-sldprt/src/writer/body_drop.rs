// SPDX-License-Identifier: Apache-2.0
//! Bodies the B-rep encoder cannot write (§3.4, "Content the target cannot
//! hold"). The generated partitions are encoded again without each body that
//! owns the record an encode failure names (§3.2), until they encode.

use std::collections::{BTreeSet, HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;

/// The encoded partitions, the reduced document when bodies were left out,
/// and the losses that name them.
pub(super) struct Encoded<T> {
    pub(super) partitions: T,
    pub(super) reduced: Option<CadIr>,
    pub(super) losses: Vec<LossNote>,
}

/// Run `encode` on `ir`, and on each failure leave out the bodies the failure
/// names. A failure that names no body's record first leaves out the geometry
/// no body owns, then every body. A resource or I/O failure, and a failure
/// with nothing left to leave out, is returned. `state` names the
/// configuration whose stored state `ir` is, which the note names; `None` is
/// the current geometry, whose reduction to no face is also noted.
pub(super) fn encode_without_unwritable_bodies<T>(
    ir: &CadIr,
    state: Option<&ConfigurationId>,
    encode: impl Fn(&CadIr) -> Result<T, CodecError>,
) -> Result<Encoded<T>, CodecError> {
    let mut reduced: Option<CadIr> = None;
    let mut entries = Vec::new();
    loop {
        let current = reduced.as_ref().unwrap_or(ir);
        let error = match encode(current) {
            Ok(partitions) => {
                let mut losses = Vec::new();
                if !entries.is_empty() {
                    let of = state.map_or_else(String::new, |configuration| {
                        format!(" of the state configuration {configuration} stores")
                    });
                    losses.push(SldprtLossCode::WriterBodiesOmitted.note(format!(
                        "{} body or bodies{of}, or geometry no body owns, cannot be encoded as \
                         SLDPRT B-rep records and are not written: {}",
                        entries.len(),
                        entries.join("; ")
                    )));
                }
                if reduced.is_some() && state.is_none() {
                    losses.extend(super::reduce::empty_output(current));
                }
                return Ok(Encoded {
                    partitions,
                    reduced,
                    losses,
                });
            }
            Err(error @ (CodecError::ResourceLimit(_) | CodecError::Io(_))) => return Err(error),
            Err(error) => error,
        };
        let reason = error.to_string();
        let owned = owned_records(current);
        let mut named = named_bodies(&owned, &reason);
        named.retain(|body| current.geometry.bodies.iter().any(|row| &row.id == body));
        let kept = if named.is_empty() {
            let all = current
                .geometry
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>();
            let stripped = super::body_subset(current, &all)?;
            if record_count(&stripped) < record_count(current) {
                entries.push(format!("geometry no body owns ({reason})"));
                reduced = Some(stripped);
                continue;
            }
            if all.is_empty() {
                return Err(error);
            }
            entries.extend(
                all.iter()
                    .map(|body| format!("{} ({reason})", body.as_str())),
            );
            Vec::new()
        } else {
            entries.extend(
                named
                    .iter()
                    .map(|body| format!("{} ({reason})", body.as_str())),
            );
            current
                .geometry
                .bodies
                .iter()
                .filter(|body| !named.contains(&body.id))
                .map(|body| body.id.clone())
                .collect()
        };
        let mut subset = super::body_subset(current, &kept)?;
        for configuration in &mut subset.model.configurations {
            if let Some(bodies) = &mut configuration.bodies {
                bodies.retain(|body| kept.contains(body));
            }
        }
        reduced = Some(subset);
    }
}

/// Every geometry record id each body owns, keyed by body.
fn owned_records(ir: &CadIr) -> HashMap<&BodyId, HashSet<&str>> {
    let geometry = &ir.geometry;
    let shells = geometry
        .shells
        .iter()
        .map(|shell| (&shell.id, shell))
        .collect::<HashMap<_, _>>();
    let faces_by_shell =
        geometry
            .faces
            .iter()
            .fold(HashMap::<_, Vec<_>>::new(), |mut faces, face| {
                faces.entry(&face.shell).or_default().push(face);
                faces
            });
    let loops = geometry
        .loops
        .iter()
        .map(|loop_| (&loop_.id, loop_))
        .collect::<HashMap<_, _>>();
    let coedges = geometry
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let edges = geometry
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let vertices = geometry
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, vertex))
        .collect::<HashMap<_, _>>();
    let mut owned = HashMap::<&BodyId, HashSet<&str>>::new();
    for region in &geometry.regions {
        let records = owned.entry(&region.body).or_default();
        records.insert(region.body.as_str());
        records.insert(region.id.as_str());
        let mut edge_ids = Vec::new();
        let mut vertex_ids = Vec::new();
        for shell_id in region.stated_shells() {
            records.insert(shell_id.as_str());
            let Some(shell) = shells.get(shell_id) else {
                continue;
            };
            edge_ids.extend(shell.wire_edges());
            vertex_ids.extend(shell.free_vertices());
            for face in faces_by_shell.get(shell_id).into_iter().flatten() {
                records.insert(face.id.as_str());
                records.insert(face.surface.as_str());
                for loop_id in face.loops.stated() {
                    records.insert(loop_id.as_str());
                    let Some(loop_) = loops.get(loop_id) else {
                        continue;
                    };
                    vertex_ids.extend(loop_.vertices());
                    for coedge_id in loop_.coedges() {
                        records.insert(coedge_id.as_str());
                        if let Some(coedge) = coedges.get(coedge_id) {
                            edge_ids.push(&coedge.edge);
                        }
                    }
                }
            }
        }
        for edge_id in edge_ids {
            records.insert(edge_id.as_str());
            let Some(edge) = edges.get(edge_id) else {
                continue;
            };
            if let Some(curve) = edge.curve() {
                records.insert(curve.as_str());
            }
            vertex_ids.extend(edge.vertices());
        }
        for vertex_id in vertex_ids {
            records.insert(vertex_id.as_str());
            if let Some(vertex) = vertices.get(vertex_id) {
                records.insert(vertex.point.as_str());
            }
        }
    }
    for body in &geometry.bodies {
        owned.entry(&body.id).or_default().insert(body.id.as_str());
    }
    owned
}

/// Bodies that own a record `message` names as a whole token.
fn named_bodies(owned: &HashMap<&BodyId, HashSet<&str>>, message: &str) -> BTreeSet<BodyId> {
    let tokens = message
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|c: char| {
                matches!(c, ',' | ';' | ':' | '.' | '(' | ')' | '\'' | '"' | '`')
            })
        })
        .collect::<HashSet<_>>();
    owned
        .iter()
        .filter(|(_, records)| records.iter().any(|record| tokens.contains(record)))
        .map(|(body, _)| (*body).clone())
        .collect()
}

fn record_count(ir: &CadIr) -> usize {
    let geometry = &ir.geometry;
    geometry.surfaces.len()
        + geometry.curves.len()
        + geometry.points.len()
        + geometry.vertices.len()
        + geometry.edges.len()
        + geometry.coedges.len()
        + geometry.loops.len()
        + geometry.faces.len()
        + geometry.shells.len()
        + geometry.regions.len()
        + geometry.pcurves.len()
}
