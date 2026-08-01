// SPDX-License-Identifier: Apache-2.0
//! Typed IGES entity accessors and neutral projection.

use std::collections::BTreeSet;

use cadmpeg_ir::CadIr;

#[derive(Clone, Copy)]
pub(crate) struct TopologyAppendCheckpoint {
    lengths: [usize; 10],
}

impl TopologyAppendCheckpoint {
    pub(crate) fn capture(ir: &CadIr) -> Self {
        Self {
            lengths: [
                ir.model.bodies.len(),
                ir.model.regions.len(),
                ir.model.shells.len(),
                ir.model.faces.len(),
                ir.model.loops.len(),
                ir.model.coedges.len(),
                ir.model.edges.len(),
                ir.model.vertices.len(),
                ir.model.points.len(),
                ir.model.pcurves.len(),
            ],
        }
    }

    pub(crate) fn appended_ids(self, ir: &CadIr) -> BTreeSet<String> {
        let [bodies, regions, shells, faces, loops, coedges, edges, vertices, points, pcurves] =
            self.lengths;
        let mut ids = BTreeSet::new();
        ids.extend(
            ir.model.bodies[bodies..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.regions[regions..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.shells[shells..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.faces[faces..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.loops[loops..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.coedges[coedges..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.edges[edges..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.vertices[vertices..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.points[points..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids.extend(
            ir.model.pcurves[pcurves..]
                .iter()
                .map(|item| item.id.to_string()),
        );
        ids
    }

    pub(crate) fn rollback(ir: &mut CadIr, ids: &BTreeSet<String>) {
        ir.model
            .bodies
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .regions
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .shells
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .faces
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .loops
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .coedges
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .edges
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .vertices
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .points
            .retain(|item| !ids.contains(&item.id.to_string()));
        ir.model
            .pcurves
            .retain(|item| !ids.contains(&item.id.to_string()));
    }

    pub(crate) fn rollback_appended(self, ir: &mut CadIr) {
        let ids = self.appended_ids(ir);
        Self::rollback(ir, &ids);
    }
}

pub(crate) fn directed_cycle(
    sequence: u32,
    visited: &mut BTreeSet<u32>,
    successors: impl Fn(u32) -> Vec<u32>,
) -> bool {
    if visited.contains(&sequence) {
        return false;
    }
    let mut active = BTreeSet::new();
    let mut stack = vec![(sequence, false)];
    while let Some((current, expanded)) = stack.pop() {
        if expanded {
            active.remove(&current);
            visited.insert(current);
            continue;
        }
        if visited.contains(&current) {
            continue;
        }
        if !active.insert(current) {
            return true;
        }
        stack.push((current, true));
        for target in successors(current).into_iter().rev() {
            if active.contains(&target) {
                return true;
            }
            if !visited.contains(&target) {
                stack.push((target, false));
            }
        }
    }
    false
}

pub(crate) mod analytic_surfaces;
pub(crate) mod annotation;
pub(crate) mod brep;
pub(crate) mod composite;
pub(crate) mod conics;
pub(crate) mod copious;
pub(crate) mod csg;
pub(crate) mod curve_conversion;
pub(crate) mod drawing;
pub(crate) mod evaluation;
pub(crate) mod geometry;
pub(crate) mod offsets;
pub(crate) mod presentation;
pub(crate) mod splines;
pub(crate) mod structure;
pub(crate) mod surfaces;
pub(crate) mod trimming;
