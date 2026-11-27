// SPDX-License-Identifier: Apache-2.0
//! Historical topology rows, shell ranks, and body design links.

use super::body_closures::stable_ref;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalRow, AsmHistoricalTopology,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use std::collections::{BTreeMap, HashMap};

/// One topology family's rows found by key in logarithmic time. Among rows
/// that share a key the last one wins.
pub(super) struct KeyedRows<'a, T> {
    pub(super) rows: &'a [T],
    /// Row positions in key order, when the rows are not already in key order.
    pub(super) order: Option<Vec<usize>>,
}

impl<'a, T: AsmHistoricalRow> KeyedRows<'a, T> {
    pub(super) fn new(rows: &'a [T]) -> Self {
        let order = (!rows.is_sorted_by_key(T::key)).then(|| {
            let mut order = (0..rows.len()).collect::<Vec<_>>();
            order.sort_by_key(|&row| rows[row].key());
            order
        });
        Self { rows, order }
    }

    pub(super) fn len(&self) -> usize {
        self.rows.len()
    }

    /// The row at `rank` in key order.
    pub(super) fn at(&self, rank: usize) -> &'a T {
        &self.rows[self.order.as_ref().map_or(rank, |order| order[rank])]
    }

    /// The key-order rank of the row with `key`; distinct keys have distinct
    /// ranks below `len`.
    pub(super) fn rank(&self, key: i64) -> Option<usize> {
        let after = match &self.order {
            Some(order) => order.partition_point(|&row| self.rows[row].key() <= key),
            None => self.rows.partition_point(|row| row.key() <= key),
        };
        let rank = after.checked_sub(1)?;
        (self.at(rank).key() == key).then_some(rank)
    }

    pub(super) fn get(&self, key: i64) -> Option<&'a T> {
        self.rank(key).map(|rank| self.at(rank))
    }
}

impl<'a> KeyedRows<'a, AsmHistoricalRelation> {
    pub(super) fn members(&self, owner: i64) -> Option<&'a [i64]> {
        self.get(owner)
            .map(|relation| relation.member_refs.as_slice())
    }
}

/// The distinct row ranks one shell reaches, in first-visit order.
pub(super) struct ShellRanks {
    /// The shell visit that last reached each rank.
    pub(super) visits: Vec<usize>,
    pub(super) shell: usize,
    pub(super) ranks: Vec<usize>,
}

impl ShellRanks {
    pub(super) fn new<T: AsmHistoricalRow>(rows: &KeyedRows<'_, T>) -> Self {
        Self {
            visits: vec![0; rows.len()],
            shell: 0,
            ranks: Vec::new(),
        }
    }

    pub(super) fn start_shell(&mut self) {
        self.shell += 1;
        self.ranks.clear();
    }

    pub(super) fn visit(&mut self, rank: usize) {
        if self.visits[rank] != self.shell {
            self.visits[rank] = self.shell;
            self.ranks.push(rank);
        }
    }
}

pub(super) fn historical_topology(
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    macro_rules! topology_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }

    fn refs<'a>(ids: impl Iterator<Item = &'a str>) -> Option<Vec<i64>> {
        ids.map(stable_ref).collect::<Option<Vec<_>>>()
    }

    fn relations<'a, M>(
        items: impl Iterator<Item = (&'a str, M)>,
    ) -> Option<Vec<AsmHistoricalRelation>>
    where
        M: Iterator<Item = &'a str>,
    {
        let mut collected = Vec::new();
        for (owner, members) in items {
            let owner_ref = stable_ref(owner)?;
            let member_refs = refs(members)?;

            collected.push(AsmHistoricalRelation {
                owner_ref,
                member_refs,
            });
        }
        Some(collected)
    }

    super::chamfer_supports::note(brep);
    let mut surface_radii = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
            let radius = match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                    cylinder_surface.radius().get()
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                    sphere_surface.radius().get()
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                    torus_surface.minor_radius().get()
                }
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceRadius {
                surface: stable_ref(surface.id.as_str())?,
                radius: radius.abs(),
            })
        })
        .collect::<Vec<_>>();
    let mut blend_radii = HashMap::new();
    for (owner, procedural) in &brep.procedural_surfaces {
        let cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition::Blend(
            definition_payload,
        ) = procedural.definition()
        else {
            continue;
        };
        let radius = definition_payload.radius();

        let cadmpeg_ir::geometry::blends::BlendRadiusLaw::Constant { signed_radius } = radius
        else {
            continue;
        };
        let Some(surface) = stable_ref(owner.as_str()) else {
            continue;
        };
        blend_radii.insert(surface, signed_radius.get().abs());
    }

    surface_radii.retain(|candidate| !blend_radii.contains_key(&candidate.surface));
    for (surface, radius) in blend_radii {
        surface_radii.push(crate::history_records::AsmHistoricalSurfaceRadius { surface, radius });
    }
    surface_radii.sort_by_key(|a| a.surface);
    let mut surface_cylinders = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
            else {
                return None;
            };
            let origin = cylinder_surface.origin().get();
            let axis = *cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            Some(crate::history_records::AsmHistoricalCylinder {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                axis,
                radius: radius.abs(),
            })
        })
        .collect::<Vec<_>>();
    surface_cylinders.sort_by_key(|a| a.surface);
    let mut surface_planes = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved()
            else {
                return None;
            };
            let origin = plane_surface.origin().get();
            let normal = *plane_surface.frame().axis().as_raw();
            Some(crate::history_records::AsmHistoricalPlane {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                normal,
            })
        })
        .collect::<Vec<_>>();
    surface_planes.sort_by_key(|a| a.surface);
    let mut surface_axes = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
            let (origin, direction) = match surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                    let origin = cylinder_surface.origin().get();
                    let axis = *cylinder_surface.frame().axis().as_raw();
                    (origin, axis)
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                    let origin = cone_surface.origin().get();
                    let axis = *cone_surface.frame().axis().as_raw();
                    (origin, axis)
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                    let center = torus_surface.center().get();
                    let axis = *torus_surface.frame().axis().as_raw();
                    (center, axis)
                }
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceAxis {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                direction,
            })
        })
        .collect::<Vec<_>>();
    surface_axes.sort_by_key(|a| a.surface);

    let mut regions_by_body = HashMap::<&str, Vec<&str>>::new();
    for region in &brep.regions {
        regions_by_body
            .entry(region.body.as_str())
            .or_default()
            .push(region.id.as_str());
    }
    for body in &brep.bodies {
        if let Some(chain) = stable_ref(body.id.as_str())
            .and_then(|record| brep.chain_order.body_regions.get(&record))
        {
            regions_by_body.insert(
                body.id.as_str(),
                chain
                    .iter()
                    .map(cadmpeg_ir::ids::RegionId::as_str)
                    .collect(),
            );
        }
    }
    let mut faces_by_shell = HashMap::<&str, Vec<&str>>::new();
    for face in &brep.faces {
        faces_by_shell
            .entry(face.shell.as_str())
            .or_default()
            .push(face.id.as_str());
    }
    for shell in &brep.shells {
        if let Some(chain) = stable_ref(shell.id.as_str())
            .and_then(|record| brep.chain_order.shell_faces.get(&record))
        {
            faces_by_shell.insert(
                shell.id.as_str(),
                chain.iter().map(cadmpeg_ir::ids::FaceId::as_str).collect(),
            );
        }
    }
    let mut ring_neighbors = HashMap::new();
    for loop_ in &brep.loops {
        let ring = loop_.coedges();
        for (ordinal, coedge) in ring.iter().enumerate() {
            let key = (&loop_.id, coedge);

            ring_neighbors.entry(key).or_insert((
                &ring[(ordinal + 1) % ring.len()],
                &ring[(ordinal + ring.len() - 1) % ring.len()],
            ));
        }
    }

    let (face_envelopes, edge_envelopes) = super::face_envelopes::derive(brep);
    let mut topology = AsmHistoricalTopology {
        face_envelopes: face_envelopes.into(),
        edge_envelopes: edge_envelopes.into(),
        bodies: topology_some!(refs(brep.bodies.iter().map(|entity| entity.id.as_str()))),
        regions: topology_some!(refs(brep.regions.iter().map(|entity| entity.id.as_str()))),
        shells: topology_some!(refs(brep.shells.iter().map(|entity| entity.id.as_str()))),
        faces: topology_some!(refs(brep.faces.iter().map(|entity| entity.id.as_str()))),
        loops: topology_some!(refs(brep.loops.iter().map(|entity| entity.id.as_str()))),
        coedges: topology_some!(refs(brep.coedges.iter().map(|entity| entity.id.as_str()))),
        edges: topology_some!(refs(brep.edges.iter().map(|entity| entity.id.as_str()))),
        vertices: topology_some!(refs(brep.vertices.iter().map(|entity| entity.id.as_str()))),
        points: topology_some!(refs(brep.points.iter().map(|entity| entity.id.as_str()))),
        surfaces: topology_some!(refs(brep.surfaces.iter().map(|entity| entity.id.as_str()))),
        surface_radii,
        surface_cylinders,
        surface_planes,
        surface_axes,
        curves: topology_some!(refs(brep.curves.iter().map(|entity| entity.id.as_str()))),
        curve_axes: brep
            .curves
            .iter()
            .filter_map(|curve| {
                use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
                let (origin, direction) = match curve.geometry {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                        let origin = line_curve.origin().get();
                        let direction = *line_curve.direction().as_raw();
                        (origin, direction)
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                        let center = circle_curve.center().get();
                        let axis = *circle_curve.frame().axis().as_raw();
                        (center, axis)
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                        let center = ellipse_curve.center().get();
                        let axis = *ellipse_curve.frame().axis().as_raw();
                        (center, axis)
                    }
                    _ => return None,
                };
                Some(crate::history_records::AsmHistoricalCurveAxis {
                    curve: stable_ref(curve.id.as_str())?,
                    origin,
                    direction,
                })
            })
            .collect::<Vec<_>>(),
        pcurves: topology_some!(refs(brep.pcurves.iter().map(|entity| entity.id.as_str()))),
        persistent_subentity_tags: Vec::new(),
        body_regions: topology_some!(relations(brep.bodies.iter().map(|body| {
            (
                body.id.as_str(),
                regions_by_body
                    .get(body.id.as_str())
                    .into_iter()
                    .flatten()
                    .copied(),
            )
        }))),
        region_shells: topology_some!(relations(brep.regions.iter().map(|region| {
            (
                region.id.as_str(),
                region.stated_shells().map(cadmpeg_ir::ids::ShellId::as_str),
            )
        }))),
        shell_faces: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                faces_by_shell
                    .get(shell.id.as_str())
                    .into_iter()
                    .flatten()
                    .copied(),
            )
        }))),
        shell_wire_edges: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                shell.wire_edges().map(cadmpeg_ir::ids::EdgeId::as_str),
            )
        }))),
        shell_free_vertices: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                shell.free_vertices().map(cadmpeg_ir::ids::VertexId::as_str),
            )
        }))),
        face_loops: topology_some!(relations(brep.faces.iter().map(|face| {
            (
                face.id.as_str(),
                face.loops.stated().map(cadmpeg_ir::ids::LoopId::as_str),
            )
        }))),
        loop_coedges: topology_some!(relations(brep.loops.iter().map(|loop_| {
            (
                loop_.id.as_str(),
                loop_
                    .coedges()
                    .iter()
                    .map(cadmpeg_ir::ids::CoedgeId::as_str),
            )
        }))),
        coedge_topology: topology_some!(brep
            .coedges
            .iter()
            .map(|coedge| {
                let (next, previous) = ring_neighbors
                    .get(&(&coedge.owner_loop, &coedge.id))
                    .copied()?;
                Some(AsmHistoricalCoedge {
                    coedge: stable_ref(coedge.id.as_str())?,
                    owner_loop: stable_ref(coedge.owner_loop.as_str())?,
                    edge: stable_ref(coedge.edge.as_str())?,
                    next: stable_ref(next.as_str())?,
                    previous: stable_ref(previous.as_str())?,
                    radial_next: stable_ref(coedge.radial_next.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        edge_vertices: topology_some!(brep
            .edges
            .iter()
            .map(|edge| {
                Some(AsmHistoricalEdge {
                    edge: stable_ref(edge.id.as_str())?,
                    start_vertex: stable_ref(edge.start()?.as_str())?,
                    end_vertex: stable_ref(edge.end()?.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        face_surfaces: topology_some!(brep
            .faces
            .iter()
            .map(|face| {
                Some(AsmHistoricalCarrierBinding {
                    entity: stable_ref(face.id.as_str())?,
                    carrier: stable_ref(face.surface.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        edge_curves: topology_some!(brep
            .edges
            .iter()
            .map(|edge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: stable_ref(edge.id.as_str())?,
                    carrier: match edge.curve() {
                        Some(curve) => Some(stable_ref(curve.as_str())?),
                        None => None,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()),
        coedge_pcurves: topology_some!(brep
            .coedges
            .iter()
            .map(|coedge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: stable_ref(coedge.id.as_str())?,
                    carrier: match coedge.pcurves.first() {
                        Some(use_) => Some(stable_ref(use_.pcurve.as_str())?),
                        None => None,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()),
        vertex_points: topology_some!(brep
            .vertices
            .iter()
            .map(|vertex| {
                Some(AsmHistoricalCarrierBinding {
                    entity: stable_ref(vertex.id.as_str())?,
                    carrier: stable_ref(vertex.point.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        point_positions: topology_some!(brep
            .points
            .iter()
            .map(|point| {
                Some(AsmHistoricalPoint {
                    point: stable_ref(point.id.as_str())?,
                    position: point.position().get(),
                })
            })
            .collect::<Option<Vec<_>>>()),
        edge_samples: super::edge_samples::historical_edge_samples(brep)?,
        face_samples: super::face_samples::historical_face_samples(brep),
        body_design_links: last_body_design_links(&body_design_links(brep)?),
    };
    // Transitions diff each family in key order. The decoder appends the
    // containers it builds for saved free edges after the containers it walks,
    // and the curves it builds for tolerant coedges after the curves it walks.
    topology.bodies.sort_unstable();
    topology.regions.sort_unstable();
    topology.shells.sort_unstable();
    topology.curves.sort_unstable();
    topology.curve_axes.sort_by_key(|axis| axis.curve);
    for family in [
        &mut topology.body_regions,
        &mut topology.region_shells,
        &mut topology.shell_faces,
        &mut topology.shell_wire_edges,
        &mut topology.shell_free_vertices,
    ] {
        family.sort_by_key(|relation| relation.owner_ref);
    }
    Ok(Some(topology))
}

pub(crate) fn historical_topology_with_tags(
    brep: &crate::brep::Brep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    let Some(mut topology) = historical_topology(&brep.asm)? else {
        return Ok(None);
    };
    for tag in &brep.persistent_subentity_tags {
        let (entity_kind, entity_ref) = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(face) => {
                (AsmHistoricalEntityKind::Face, stable_ref(face.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(edge) => {
                (AsmHistoricalEntityKind::Edge, stable_ref(edge.as_str()))
            }
            _ => continue,
        };
        let Some(entity_ref) = entity_ref else {
            continue;
        };
        let design_references = tag.design_references.clone();
        let token = tag.token.as_str().to_owned();

        topology.persistent_subentity_tags.push(
            crate::history_records::AsmHistoricalPersistentSubentityTag {
                entity_kind,
                entity_ref,
                selector: tag.selector,
                token,
                token_integer: tag.token_integer,
                design_references,
                ordinal: tag.ordinal,
            },
        );
    }
    topology
        .persistent_subentity_tags
        .sort_by_key(|tag| tag.entity_ref);
    Ok(Some(topology))
}

/// The persistent design links attached to the bodies of `brep`.
pub(super) fn body_design_links(
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Result<Vec<crate::records::sketch_links::PersistentDesignLink>, cadmpeg_core::CodecError> {
    let mut links = Vec::new();
    for attribute in &brep.attributes {
        links.extend(crate::brep::persistent_design_links(attribute)?);
    }
    Ok(links)
}

/// The highest-ordinal persistent design link of each body, in body order.
pub(super) fn last_body_design_links(
    links: &[crate::records::sketch_links::PersistentDesignLink],
) -> Vec<crate::history_records::AsmHistoricalBodyDesignLink> {
    let mut last = BTreeMap::<i64, &crate::records::sketch_links::PersistentDesignLink>::new();
    for link in links {
        let cadmpeg_ir::attributes::AttributeTarget::Body(body) = &link.target else {
            continue;
        };
        let Some(body) = stable_ref(body.as_str()) else {
            continue;
        };
        last.entry(body)
            .and_modify(|previous| {
                if link.ordinal > previous.ordinal {
                    *previous = link;
                }
            })
            .or_insert(link);
    }
    last.into_iter()
        .map(
            |(body, link)| crate::history_records::AsmHistoricalBodyDesignLink {
                body,
                design_id: link.design_id.as_str().to_owned(),
                design_reference: link.design_reference,
            },
        )
        .collect()
}
