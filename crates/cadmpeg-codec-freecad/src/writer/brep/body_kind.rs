// SPDX-License-Identifier: Apache-2.0
//! The body kind the `FreeCAD` reader derives from a written solid.

use std::collections::HashMap;

use cadmpeg_ir::topology::{Body, BodyKind, Sense};
use cadmpeg_ir::CadIr;

/// Neutral topology looked up by identity.
pub(super) struct Topology<'a> {
    shells: HashMap<&'a str, &'a cadmpeg_ir::topology::Shell>,
    faces: HashMap<&'a str, &'a cadmpeg_ir::topology::Face>,
    loops: HashMap<&'a str, &'a cadmpeg_ir::topology::Loop>,
    coedges: HashMap<&'a str, &'a cadmpeg_ir::topology::Coedge>,
    edges: HashMap<&'a str, &'a cadmpeg_ir::topology::Edge>,
    regions: HashMap<&'a str, &'a cadmpeg_ir::topology::Region>,
}

impl<'a> Topology<'a> {
    pub(super) fn new(ir: &'a CadIr) -> Self {
        let model = &ir.model;
        Self {
            shells: model.shells.iter().map(|row| (row.id.as_str(), row)).collect(),
            faces: model.faces.iter().map(|row| (row.id.as_str(), row)).collect(),
            loops: model.loops.iter().map(|row| (row.id.as_str(), row)).collect(),
            coedges: model.coedges.iter().map(|row| (row.id.as_str(), row)).collect(),
            edges: model.edges.iter().map(|row| (row.id.as_str(), row)).collect(),
            regions: model.regions.iter().map(|row| (row.id.as_str(), row)).collect(),
        }
    }

    /// Whether every curved edge of `shell` is used equally often in each
    /// direction, the reader's test for a closed shell.
    fn shell_closed(&self, shell: &str) -> bool {
        let Some(shell) = self.shells.get(shell) else {
            return false;
        };
        let mut uses = HashMap::<&str, [usize; 2]>::new();
        for face in shell.faces() {
            let Some(face) = self.faces.get(face.as_str()) else {
                return false;
            };
            for wire in &face.loops {
                let Some(wire) = self.loops.get(wire.as_str()) else {
                    return false;
                };
                for coedge in wire.coedges() {
                    let Some(coedge) = self.coedges.get(coedge.as_str()) else {
                        return false;
                    };
                    let curved = self
                        .edges
                        .get(coedge.edge.as_str())
                        .is_some_and(|edge| edge.curve().is_some());
                    if curved {
                        let slot = &mut uses.entry(coedge.edge.as_str()).or_default()
                            [usize::from(coedge.sense == Sense::Reversed)];
                        *slot += 1;
                    }
                }
            }
        }
        !uses.is_empty()
            && uses
                .values()
                .all(|&[forward, reversed]| forward > 0 && forward == reversed)
    }

    /// The kind the reader derives for `body` written as `root` of
    /// `shapes`: a solid reads by its shells' closure, a wire or edge as a
    /// wire, a shell or face as a sheet, a vertex as general, and a compound
    /// as the kind its members share or general.
    pub(super) fn reading(
        &self,
        body: &Body,
        root: usize,
        shapes: &[super::Shape],
    ) -> Option<BodyKind> {
        let mut shared = None;
        let mut pending = vec![root];
        let mut visited = std::collections::HashSet::new();
        while let Some(index) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            let shape = shapes.get(index)?;
            let kind = match shape.kind {
                "So" => self.solid_reading(body),
                "Wi" | "Ed" => BodyKind::Wire,
                "Sh" | "Fa" => BodyKind::Sheet,
                "Ve" => BodyKind::General,
                _ => {
                    pending.extend(shape.children.iter().map(|(child, _)| *child));
                    continue;
                }
            };
            match shared {
                None => shared = Some(kind),
                Some(existing) if existing == kind => {}
                Some(_) => return Some(BodyKind::General),
            }
        }
        shared
    }

    /// The kind the reader derives for the solids of `body`: a solid when
    /// every shell closes, otherwise a sheet.
    fn solid_reading(&self, body: &Body) -> BodyKind {
        let closed = body.regions.iter().all(|region| {
            self.regions.get(region.as_str()).is_some_and(|region| {
                region
                    .shells
                    .iter()
                    .all(|shell| self.shell_closed(shell.as_str()))
            })
        });
        if closed {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        }
    }
}
