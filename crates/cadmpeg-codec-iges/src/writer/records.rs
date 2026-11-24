// SPDX-License-Identifier: Apache-2.0
//! Model records by id for one semantic write.

use cadmpeg_ir::geometry::{pcurve::Pcurve, Curve, Surface};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::schema::EntitySchema;
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

/// Model records looked up by id. A lookup returns the first record with the
/// id in arena order.
pub(super) trait ModelRecords {
    /// The model that holds the records.
    fn ir(&self) -> &CadIr;
    /// The vertex with `id`.
    fn vertex(&self, id: &str) -> Option<&Vertex>;
    /// The point with `id`.
    fn point(&self, id: &str) -> Option<&Point>;
}

impl ModelRecords for CadIr {
    fn ir(&self) -> &CadIr {
        self
    }

    fn vertex(&self, id: &str) -> Option<&Vertex> {
        self.geometry
            .vertices
            .iter()
            .find(|vertex| vertex.id.as_str() == id)
    }

    fn point(&self, id: &str) -> Option<&Point> {
        self.geometry
            .points
            .iter()
            .find(|point| point.id.as_str() == id)
    }
}

/// The first record of each id in the arenas that the semantic writer reads
/// per topology use, and the face supports of each pcurve.
pub(super) struct TopologyIndex<'ir> {
    pub(super) ir: &'ir CadIr,
    pub(super) bodies: BTreeMap<&'ir str, &'ir Body>,
    pub(super) regions: BTreeMap<&'ir str, &'ir Region>,
    pub(super) shells: BTreeMap<&'ir str, &'ir Shell>,
    pub(super) faces: BTreeMap<&'ir str, &'ir Face>,
    pub(super) loops: BTreeMap<&'ir str, &'ir Loop>,
    pub(super) coedges: BTreeMap<&'ir str, &'ir Coedge>,
    pub(super) edges: BTreeMap<&'ir str, &'ir Edge>,
    pub(super) vertices: BTreeMap<&'ir str, &'ir Vertex>,
    pub(super) points: BTreeMap<&'ir str, &'ir Point>,
    pub(super) curves: BTreeMap<&'ir str, &'ir Curve>,
    pub(super) surfaces: BTreeMap<&'ir str, &'ir Surface>,
    pub(super) pcurves: BTreeMap<&'ir str, &'ir Pcurve>,
    pcurve_supports: BTreeMap<&'ir str, BTreeSet<&'ir SurfaceId>>,
}

impl<'ir> TopologyIndex<'ir> {
    pub(super) fn new(ir: &'ir CadIr) -> Self {
        let geometry = &ir.geometry;
        let loops = first_by_id(&geometry.loops);
        let coedges = first_by_id(&geometry.coedges);
        let mut pcurve_supports = BTreeMap::<_, BTreeSet<_>>::new();
        for face in &geometry.faces {
            for loop_ in face
                .loops
                .stated()
                .filter_map(|loop_id| loops.get(loop_id.as_str()).copied())
            {
                for coedge in loop_
                    .coedges()
                    .iter()
                    .filter_map(|coedge_id| coedges.get(coedge_id.as_str()).copied())
                {
                    for pcurve_use in &coedge.pcurves {
                        pcurve_supports
                            .entry(pcurve_use.pcurve.as_str())
                            .or_default()
                            .insert(&face.surface);
                    }
                }
            }
        }
        Self {
            ir,
            bodies: first_by_id(&geometry.bodies),
            regions: first_by_id(&geometry.regions),
            shells: first_by_id(&geometry.shells),
            faces: first_by_id(&geometry.faces),
            loops,
            coedges,
            edges: first_by_id(&geometry.edges),
            vertices: first_by_id(&geometry.vertices),
            points: first_by_id(&geometry.points),
            curves: first_by_id(&geometry.curves),
            surfaces: first_by_id(&geometry.surfaces),
            pcurves: first_by_id(&geometry.pcurves),
            pcurve_supports,
        }
    }

    /// The surfaces of the faces with a loop coedge that uses `pcurve`, in id
    /// order.
    pub(super) fn pcurve_support_surfaces(
        &self,
        pcurve: &str,
    ) -> impl Iterator<Item = &'ir SurfaceId> + '_ {
        self.pcurve_supports
            .get(pcurve)
            .into_iter()
            .flatten()
            .copied()
    }
}

impl ModelRecords for TopologyIndex<'_> {
    fn ir(&self) -> &CadIr {
        self.ir
    }

    fn vertex(&self, id: &str) -> Option<&Vertex> {
        self.vertices.get(id).copied()
    }

    fn point(&self, id: &str) -> Option<&Point> {
        self.points.get(id).copied()
    }
}

fn first_by_id<T: EntitySchema>(records: &[T]) -> BTreeMap<&str, &T> {
    let mut first = BTreeMap::new();
    for record in records {
        first.entry(record.identity()).or_insert(record);
    }
    first
}
