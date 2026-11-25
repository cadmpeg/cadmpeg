// SPDX-License-Identifier: Apache-2.0
//! Drops the faces and bodies that the IGES writer cannot encode, with the
//! records that only they use, and writes the rest (CADIR §3.3, §3.4).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::without_faces_and_bodies;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::ShellMember;
use cadmpeg_ir::CadIr;

use super::Synthesis;
use crate::loss::IgesLossCode;

/// An error that no smaller output avoids.
pub(super) fn ends_the_write(error: &CodecError) -> bool {
    matches!(
        error,
        CodecError::ResourceLimit(_) | CodecError::Io(_) | CodecError::UnsupportedTarget(_)
    )
}

/// Synthesizes `ir`. Each time the write fails, drops the faces or bodies
/// that the failure needs and synthesizes again; when the failure names none,
/// writes the empty file.
pub(super) fn synthesize_dropping(
    ir: &CadIr,
    version: crate::IgesVersion,
) -> Result<Synthesis, CodecError> {
    let mut document = Cow::Borrowed(ir);
    let mut dropped = Vec::new();
    loop {
        let error = match super::synthesize(&document, version) {
            Ok(mut synthesis) => {
                dropped.append(&mut synthesis.losses);
                synthesis.losses = dropped;
                return Ok(synthesis);
            }
            Err(error) if ends_the_write(&error) => return Err(error),
            Err(error) => error,
        };
        let Some(culprits) = culprits(&document, version, &error)? else {
            return empty_file(&document, version, &error, dropped);
        };
        dropped.extend(culprits.losses(&error));
        document = Cow::Owned(without_faces_and_bodies(
            &document,
            &culprits.faces.iter().map(String::as_str).collect(),
            &culprits.bodies.iter().map(String::as_str).collect(),
        )?);
    }
}

/// The last step: every body goes, and the file holds what remains outside
/// the geometry.
fn empty_file(
    ir: &CadIr,
    version: crate::IgesVersion,
    error: &CodecError,
    mut dropped: Vec<LossNote>,
) -> Result<Synthesis, CodecError> {
    let mut empty = ir.clone();
    empty.geometry = cadmpeg_ir::document::Topology::default();
    let mut synthesis = super::synthesize(&empty, version)?;
    if ir.geometry.bodies.is_empty() {
        dropped.push(
            IgesLossCode::WriterFreeGeometryOmitted
                .note(format!("the document's geometry was not written: {error}")),
        );
    }
    dropped.extend(ir.geometry.bodies.iter().map(|body| {
        IgesLossCode::WriterBodyOmitted.note(format!(
            "body {} was not written, with its topology and the records only it uses: {error}",
            body.id
        ))
    }));
    dropped.append(&mut synthesis.losses);
    synthesis.losses = dropped;
    Ok(synthesis)
}

/// The faces or bodies that one failed write needs.
struct Culprits {
    faces: BTreeSet<String>,
    bodies: BTreeSet<String>,
}

impl Culprits {
    fn losses(&self, error: &CodecError) -> impl Iterator<Item = LossNote> + '_ {
        let error = error.to_string();
        let faces = self.faces.iter().map({
            let error = error.clone();
            move |face| {
                IgesLossCode::WriterFaceOmitted.note(format!(
                    "face {face} was not written, with the records only it uses: {error}"
                ))
            }
        });
        let bodies = self.bodies.iter().map(move |body| {
            IgesLossCode::WriterBodyOmitted.note(format!(
                "body {body} was not written, with its topology and the records only it uses: {error}"
            ))
        });
        faces.chain(bodies)
    }
}

/// Finds the faces, else the bodies, that the failure needs: those that use a
/// record the message names, else the smallest set found by halving whose
/// removal changes the outcome of the write.
fn culprits(
    ir: &CadIr,
    version: crate::IgesVersion,
    error: &CodecError,
) -> Result<Option<Culprits>, CodecError> {
    let uses = Uses::new(ir);
    let message = error.to_string();
    let named = message
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|c: char| {
                matches!(
                    c,
                    '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '.' | '\'' | '"' | '`'
                )
            })
        })
        .collect::<BTreeSet<_>>();
    let faces = named
        .iter()
        .filter_map(|id| uses.faces.get(id))
        .flatten()
        .map(|face| (*face).to_owned())
        .collect::<BTreeSet<_>>();
    if !faces.is_empty() {
        return Ok(Some(Culprits {
            faces,
            bodies: BTreeSet::new(),
        }));
    }
    let bodies = named
        .iter()
        .filter_map(|id| uses.bodies.get(id))
        .flatten()
        .map(|body| (*body).to_owned())
        .collect::<BTreeSet<_>>();
    if !bodies.is_empty() {
        return Ok(Some(Culprits {
            faces: BTreeSet::new(),
            bodies,
        }));
    }
    let changes = |faces: &[&str], bodies: &[&str]| -> Result<bool, CodecError> {
        let smaller = without_faces_and_bodies(
            ir,
            &faces.iter().copied().collect(),
            &bodies.iter().copied().collect(),
        )?;
        Ok(match super::synthesize(&smaller, version) {
            Ok(_) => true,
            Err(other) => other.to_string() != message,
        })
    };
    let faces = ir
        .geometry
        .faces
        .iter()
        .map(|face| face.id.as_str())
        .collect::<Vec<_>>();
    if let Some(faces) = isolate(&faces, |set| changes(set, &[]))? {
        return Ok(Some(Culprits {
            faces: faces.iter().map(|face| (*face).to_owned()).collect(),
            bodies: BTreeSet::new(),
        }));
    }
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect::<Vec<_>>();
    Ok(
        isolate(&bodies, |set| changes(&[], set))?.map(|bodies| Culprits {
            faces: BTreeSet::new(),
            bodies: bodies.iter().map(|body| (*body).to_owned()).collect(),
        }),
    )
}

/// Halves `candidates` while one half alone changes the outcome. `None` when
/// removing every candidate leaves the outcome as it was.
fn isolate<'a>(
    candidates: &[&'a str],
    mut changes: impl FnMut(&[&'a str]) -> Result<bool, CodecError>,
) -> Result<Option<Vec<&'a str>>, CodecError> {
    if candidates.is_empty() || !changes(candidates)? {
        return Ok(None);
    }
    let mut set = candidates.to_vec();
    while set.len() > 1 {
        let (left, right) = set.split_at(set.len() / 2);
        if changes(left)? {
            set = left.to_vec();
        } else if changes(right)? {
            set = right.to_vec();
        } else {
            break;
        }
    }
    Ok(Some(set))
}

/// For each record id, the faces and the bodies that use it.
struct Uses<'ir> {
    faces: BTreeMap<&'ir str, BTreeSet<&'ir str>>,
    bodies: BTreeMap<&'ir str, BTreeSet<&'ir str>>,
}

impl<'ir> Uses<'ir> {
    fn new(ir: &'ir CadIr) -> Self {
        let geometry = &ir.geometry;
        let loops = geometry
            .loops
            .iter()
            .map(|loop_| (loop_.id.as_str(), loop_))
            .collect::<BTreeMap<_, _>>();
        let coedges = geometry
            .coedges
            .iter()
            .map(|coedge| (coedge.id.as_str(), coedge))
            .collect::<BTreeMap<_, _>>();
        let edges = geometry
            .edges
            .iter()
            .map(|edge| (edge.id.as_str(), edge))
            .collect::<BTreeMap<_, _>>();
        let vertices = geometry
            .vertices
            .iter()
            .map(|vertex| (vertex.id.as_str(), vertex))
            .collect::<BTreeMap<_, _>>();
        let edge_records = |id: &'ir str, records: &mut Vec<&'ir str>| {
            records.push(id);
            let Some(edge) = edges.get(id).copied() else {
                return;
            };
            records.extend(edge.curve().map(cadmpeg_ir::ids::CurveId::as_str));
            for vertex in edge.vertices() {
                records.push(vertex.as_str());
                records.extend(
                    vertices
                        .get(vertex.as_str())
                        .copied()
                        .map(|vertex| vertex.point.as_str()),
                );
            }
        };
        let mut uses = Self {
            faces: BTreeMap::new(),
            bodies: BTreeMap::new(),
        };
        for face in &geometry.faces {
            let mut records = vec![face.id.as_str(), face.surface.as_str()];
            for loop_id in face.loops.stated() {
                records.push(loop_id.as_str());
                let Some(loop_) = loops.get(loop_id.as_str()).copied() else {
                    continue;
                };
                for vertex in loop_.vertices() {
                    records.push(vertex.as_str());
                    records.extend(
                        vertices
                            .get(vertex.as_str())
                            .copied()
                            .map(|vertex| vertex.point.as_str()),
                    );
                }
                if let Some((_, pcurves)) = loop_.singular_vertex() {
                    records.extend(pcurves.iter().map(|use_| use_.pcurve.as_str()));
                }
                for coedge_id in loop_.coedges() {
                    records.push(coedge_id.as_str());
                    let Some(coedge) = coedges.get(coedge_id.as_str()).copied() else {
                        continue;
                    };
                    records.extend(coedge.pcurves.iter().map(|use_| use_.pcurve.as_str()));
                    records.extend(coedge.use_curve.iter().map(|use_| use_.curve.as_str()));
                    edge_records(coedge.edge.as_str(), &mut records);
                }
            }
            for record in records {
                uses.faces
                    .entry(record)
                    .or_default()
                    .insert(face.id.as_str());
            }
        }
        for body in &geometry.bodies {
            let mut records = vec![body.id.as_str()];
            let regions = geometry
                .regions
                .iter()
                .filter(|region| region.body == body.id)
                .collect::<Vec<_>>();
            records.extend(regions.iter().map(|region| region.id.as_str()));
            for shell in geometry
                .shells
                .iter()
                .filter(|shell| regions.iter().any(|region| region.id == shell.region))
            {
                records.push(shell.id.as_str());
                for member in &shell.members {
                    match member {
                        ShellMember::WireEdge { id } => edge_records(id.as_str(), &mut records),
                        ShellMember::FreeVertex { id } => {
                            records.push(id.as_str());
                            records.extend(
                                vertices
                                    .get(id.as_str())
                                    .copied()
                                    .map(|vertex| vertex.point.as_str()),
                            );
                        }
                    }
                }
            }
            for record in records {
                uses.bodies
                    .entry(record)
                    .or_default()
                    .insert(body.id.as_str());
            }
        }
        uses
    }
}

#[cfg(test)]
mod tests;
