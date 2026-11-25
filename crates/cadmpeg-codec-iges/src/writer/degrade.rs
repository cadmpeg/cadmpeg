// SPDX-License-Identifier: Apache-2.0
//! Drops the faces and bodies that the IGES writer cannot encode, with the
//! records that only they use, and writes the rest (CADIR §3.3, §3.4).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::without_faces_and_bodies;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::ShellMember;
use cadmpeg_ir::CadIr;

use super::Synthesis;
use crate::loss::IgesLossCode;

/// A failed write and the records whose removal the failure asks for.
#[derive(Debug)]
pub(super) struct Unwritten {
    pub(super) error: CodecError,
    pub(super) records: BTreeSet<String>,
}

impl From<CodecError> for Unwritten {
    fn from(error: CodecError) -> Self {
        Self {
            error,
            records: BTreeSet::new(),
        }
    }
}

impl Unwritten {
    /// Names `id` when no inner step named a record.
    pub(super) fn or_naming(mut self, id: &str) -> Self {
        if self.records.is_empty() {
            self.records.insert(id.to_owned());
        }
        self
    }
}

/// Runs the step that writes the record `id`. A failure that names no record
/// names `id`.
pub(super) fn named<T>(
    id: &str,
    step: impl FnOnce() -> Result<T, Unwritten>,
) -> Result<T, Unwritten> {
    step().map_err(|unwritten| unwritten.or_naming(id))
}

/// The failure of the record `id`.
pub(super) fn naming(id: &str) -> impl FnOnce(CodecError) -> Unwritten + '_ {
    move |error| Unwritten::from(error).or_naming(id)
}

/// An error that no smaller output avoids.
pub(super) fn ends_the_write(error: &CodecError) -> bool {
    matches!(
        error,
        CodecError::ResourceLimit(_) | CodecError::Io(_) | CodecError::UnsupportedTarget(_)
    )
}

/// Synthesizes `ir`, with each open shell that `version` does not define
/// written as unjoined trimmed surfaces. Each time the write fails, drops the
/// faces, else the bodies, that use a record the failure names and synthesizes
/// again; when the failure names none, writes the empty file.
pub(super) fn synthesize_dropping(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: crate::IgesVersion,
) -> Result<Synthesis, CodecError> {
    let mut document = Cow::Borrowed(ir);
    let mut dropped = Vec::new();
    loop {
        if let Some((unjoined, mut notes)) =
            super::unjoined::unjoined_open_shells(&document, version)
        {
            dropped.append(&mut notes);
            document = Cow::Owned(unjoined);
        }
        let unwritten = match super::synthesize(ctx, &document, version) {
            Ok(mut synthesis) => {
                dropped.append(&mut synthesis.losses);
                synthesis.losses = dropped;
                return Ok(synthesis);
            }
            Err(unwritten) if ends_the_write(&unwritten.error) => return Err(unwritten.error),
            Err(unwritten) => unwritten,
        };
        let culprits = Culprits::named(&Uses::new(&document), &unwritten.records);
        if culprits.faces.is_empty() && culprits.bodies.is_empty() {
            return empty_file(ctx, &document, version, &unwritten.error, dropped);
        }
        let smaller = without_faces_and_bodies(
            &document,
            &culprits.faces.iter().map(String::as_str).collect(),
            &culprits.bodies.iter().map(String::as_str).collect(),
        )?;
        if smaller.geometry.faces.len() == document.geometry.faces.len()
            && smaller.geometry.bodies.len() == document.geometry.bodies.len()
        {
            return empty_file(ctx, &document, version, &unwritten.error, dropped);
        }
        dropped.extend(culprits.losses(&unwritten.error));
        document = Cow::Owned(smaller);
    }
}

/// The last step: every body goes, with each record that no body uses, and
/// the file holds what remains outside the geometry.
fn empty_file(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: crate::IgesVersion,
    error: &CodecError,
    mut dropped: Vec<LossNote>,
) -> Result<Synthesis, CodecError> {
    let mut empty = ir.clone();
    empty.geometry = cadmpeg_ir::document::Topology::default();
    let mut synthesis =
        super::synthesize(ctx, &empty, version).map_err(|unwritten| unwritten.error)?;
    dropped.extend(ir.geometry.bodies.iter().map(|body| {
        IgesLossCode::WriterBodyOmitted.note(format!(
            "body {} was not written, with its topology and the records only it uses: {error}",
            body.id
        ))
    }));
    dropped.extend(free_records(ir).into_iter().map(|id| {
        IgesLossCode::WriterFreeGeometryOmitted
            .note(format!("free record {id} was not written: {error}"))
    }));
    dropped.append(&mut synthesis.losses);
    synthesis.losses = dropped;
    Ok(synthesis)
}

/// The geometry records that no body uses, sorted.
fn free_records(ir: &CadIr) -> BTreeSet<&str> {
    let uses = Uses::new(ir);
    let geometry = &ir.geometry;
    geometry
        .points
        .iter()
        .map(|record| record.id.as_str())
        .chain(geometry.curves.iter().map(|record| record.id.as_str()))
        .chain(geometry.surfaces.iter().map(|record| record.id.as_str()))
        .chain(geometry.pcurves.iter().map(|record| record.id.as_str()))
        .chain(geometry.vertices.iter().map(|record| record.id.as_str()))
        .chain(geometry.edges.iter().map(|record| record.id.as_str()))
        .chain(geometry.coedges.iter().map(|record| record.id.as_str()))
        .chain(geometry.loops.iter().map(|record| record.id.as_str()))
        .chain(geometry.faces.iter().map(|record| record.id.as_str()))
        .chain(geometry.shells.iter().map(|record| record.id.as_str()))
        .chain(geometry.regions.iter().map(|record| record.id.as_str()))
        .filter(|id| !uses.bodies.contains_key(id))
        .collect()
}

/// The faces or bodies that one failed write needs.
struct Culprits {
    faces: BTreeSet<String>,
    bodies: BTreeSet<String>,
}

impl Culprits {
    /// The faces that use a named record, else the bodies that use one.
    fn named(uses: &Uses<'_>, records: &BTreeSet<String>) -> Self {
        let users = |map: &BTreeMap<&str, BTreeSet<&str>>| {
            records
                .iter()
                .filter_map(|id| map.get(id.as_str()))
                .flatten()
                .map(|user| (*user).to_owned())
                .collect::<BTreeSet<_>>()
        };
        let faces = users(&uses.faces);
        let bodies = if faces.is_empty() {
            users(&uses.bodies)
        } else {
            BTreeSet::new()
        };
        Self { faces, bodies }
    }

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

/// For each record id, the faces and the bodies that use it. A body uses the
/// records of the faces in its shells.
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
        let mut face_records = BTreeMap::<&str, Vec<&str>>::new();
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
            for record in &records {
                uses.faces
                    .entry(record)
                    .or_default()
                    .insert(face.id.as_str());
            }
            face_records.insert(face.id.as_str(), records);
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
                for face in geometry.faces.iter().filter(|face| face.shell == shell.id) {
                    records.extend(face_records.get(face.id.as_str()).into_iter().flatten());
                }
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
                        ShellMember::Subd { id } => records.push(id.as_str()),
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
