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

    pub(crate) fn rollback_appended(self, ir: &mut CadIr) {
        let [bodies, regions, shells, faces, loops, coedges, edges, vertices, points, pcurves] =
            self.lengths;
        ir.model.bodies.truncate(bodies);
        ir.model.regions.truncate(regions);
        ir.model.shells.truncate(shells);
        ir.model.faces.truncate(faces);
        ir.model.loops.truncate(loops);
        ir.model.coedges.truncate(coedges);
        ir.model.edges.truncate(edges);
        ir.model.vertices.truncate(vertices);
        ir.model.points.truncate(points);
        ir.model.pcurves.truncate(pcurves);
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

#[cfg(test)]
mod tests {
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Point;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::CadIr;

    use super::TopologyAppendCheckpoint;

    #[test]
    fn rollback_preserves_preexisting_colliding_entity() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.push(Point {
            id: "iges:model:point#collision".into(),
            position: Point3::new(1.0, 2.0, 3.0),
            source_object: None,
        });
        let checkpoint = TopologyAppendCheckpoint::capture(&ir);
        ir.model.points.push(Point {
            id: "iges:model:point#collision".into(),
            position: Point3::new(4.0, 5.0, 6.0),
            source_object: None,
        });

        checkpoint.rollback_appended(&mut ir);

        assert_eq!(ir.model.points.len(), 1);
        assert_eq!(ir.model.points[0].position, Point3::new(1.0, 2.0, 3.0));
    }
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
