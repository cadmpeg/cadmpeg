// SPDX-License-Identifier: Apache-2.0
//! Transfer of carrier intersection curves and NURBS boundary curves.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::analytic::{placed_carriers, solved_topological_vertices, PlaneEquation};
use super::super::native::annotate;

use super::intersection_resolve::{
    fc14_held_coordinate, multi_component_intersection_candidates, resolve_curve_candidates,
    select_fc14_axis_coordinate_candidate,
};
use super::intersections::carrier_intersection_curve;
use super::nurbs_boundaries::{
    cubic_extrusion_plane_generator_curve, nurbs_plane_boundary_curve,
    shared_extrusion_generator_curve,
};

pub(in super::super) fn analytic_curve_branches(
    geometry: &CurveGeometry,
    tag: &'static str,
) -> Vec<(CurveGeometry, &'static str)> {
    let mut branches = vec![(geometry.clone(), tag)];
    if let CurveGeometry::Hyperbola {
        center,
        axis,
        major_direction,
        major_radius,
        minor_radius,
    } = geometry
    {
        branches.push((
            CurveGeometry::Hyperbola {
                center: *center,
                axis: *axis,
                major_direction: Vector3::new(
                    -major_direction.x,
                    -major_direction.y,
                    -major_direction.z,
                ),
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            },
            tag,
        ));
    }
    branches
}

pub(in super::super) fn transfer_carrier_intersection_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> BTreeSet<CurveId> {
    let mut transferred = BTreeSet::new();
    let carriers = placed_carriers(scan, ir);
    let solved_vertices =
        solved_topological_vertices(scan, ir, &carriers, nurbs_endpoint_witnesses);
    let edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let (Some(first), Some(second)) = (
            carriers.get(&row.faces[0]).copied(),
            carriers.get(&row.faces[1]).copied(),
        ) else {
            continue;
        };
        let points = (|| {
            let vertices = edge_vertices.get(&row.id)?;
            let points = [
                *solved_vertices.get(&vertices[0])?,
                *solved_vertices.get(&vertices[1])?,
            ];
            Some(points)
        })();
        let resolved = carrier_intersection_curve(first, second)
            .and_then(|(geometry, tag)| {
                resolve_curve_candidates(analytic_curve_branches(&geometry, tag), points)
            })
            .or_else(|| {
                let candidates = multi_component_intersection_candidates(first, second);
                if points.is_some() {
                    resolve_curve_candidates(candidates, points)
                } else {
                    let held = fc14_held_coordinate(&scan.curves.fc_coordinates, row.id)?;
                    select_fc14_axis_coordinate_candidate(candidates, held)
                }
            });
        let Some((geometry, tag)) = resolved else {
            continue;
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            tag,
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred.insert(id);
    }
    transferred
}

pub(in super::super) struct TransferredNurbsBoundaryCurves {
    pub(in super::super) ids: BTreeSet<CurveId>,
    pub(in super::super) endpoint_witnesses: BTreeSet<CurveId>,
    pub(in super::super) extrusion_plane_count: usize,
    pub(in super::super) extrusion_plane_section_generator_count: usize,
    pub(in super::super) shared_extrusion_generator_count: usize,
}

#[derive(Clone, Copy)]
pub(in super::super) enum NurbsBoundaryKind {
    ExtrusionPlane,
    ExtrusionPlaneSectionGenerator,
    SharedExtrusionGenerator,
}

pub(in super::super) fn transfer_nurbs_boundary_curves(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<TransferredNurbsBoundaryCurves, CodecError> {
    let mut result = TransferredNurbsBoundaryCurves {
        ids: BTreeSet::new(),
        endpoint_witnesses: BTreeSet::new(),
        extrusion_plane_count: 0,
        extrusion_plane_section_generator_count: 0,
        shared_extrusion_generator_count: 0,
    };
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let Some(first) = crate::surface::unique_surface_row(&scan.surfaces.rows, row.faces[0])
        else {
            continue;
        };
        let Some(second) = crate::surface::unique_surface_row(&scan.surfaces.rows, row.faces[1])
        else {
            continue;
        };
        let geometry = |surface_id| {
            let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            ir.model
                .surfaces
                .iter()
                .find(|surface| surface.id == id)
                .map(|surface| &surface.geometry)
        };
        let Some(first_geometry) = geometry(first.id) else {
            continue;
        };
        let Some(second_geometry) = geometry(second.id) else {
            continue;
        };
        let resolved = match (first.kind, second.kind, first_geometry, second_geometry) {
            (
                crate::surface::SurfaceKind::Extrusion,
                crate::surface::SurfaceKind::Plane,
                SurfaceGeometry::Nurbs(nurbs),
                SurfaceGeometry::Plane { origin, normal, .. },
            )
            | (
                crate::surface::SurfaceKind::Plane,
                crate::surface::SurfaceKind::Extrusion,
                SurfaceGeometry::Plane { origin, normal, .. },
                SurfaceGeometry::Nurbs(nurbs),
            ) => {
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                if let Some(geometry) = nurbs_plane_boundary_curve(nurbs, plane) {
                    Some((geometry, NurbsBoundaryKind::ExtrusionPlane))
                } else {
                    cubic_extrusion_plane_generator_curve(ctx, nurbs, plane)?.map(|geometry| {
                        (geometry, NurbsBoundaryKind::ExtrusionPlaneSectionGenerator)
                    })
                }
            }
            (
                crate::surface::SurfaceKind::Extrusion,
                crate::surface::SurfaceKind::Extrusion,
                SurfaceGeometry::Nurbs(first),
                SurfaceGeometry::Nurbs(second),
            ) => shared_extrusion_generator_curve(first, second)
                .map(|geometry| (geometry, NurbsBoundaryKind::SharedExtrusionGenerator)),
            _ => None,
        };
        let Some((geometry, kind)) = resolved else {
            continue;
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            match kind {
                NurbsBoundaryKind::ExtrusionPlane => "extrusion_plane_nurbs_boundary",
                NurbsBoundaryKind::ExtrusionPlaneSectionGenerator => {
                    "extrusion_plane_nurbs_section_generator"
                }
                NurbsBoundaryKind::SharedExtrusionGenerator => "shared_extrusion_nurbs_generator",
            },
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        result.ids.insert(id.clone());
        result.endpoint_witnesses.insert(id);
        match kind {
            NurbsBoundaryKind::ExtrusionPlane => result.extrusion_plane_count += 1,
            NurbsBoundaryKind::ExtrusionPlaneSectionGenerator => {
                result.extrusion_plane_section_generator_count += 1;
            }
            NurbsBoundaryKind::SharedExtrusionGenerator => {
                result.shared_extrusion_generator_count += 1;
            }
        }
    }
    Ok(result)
}
