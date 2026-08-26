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

use super::super::analytic::{
    pcurve_edge_endpoint_evidence, placed_carriers, solved_topological_vertices, CarrierEquation,
    PlaneEquation,
};
use super::super::native::annotate;
use super::super::uniqueness::exactly_one;

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

fn resolve_carrier_intersection_curve(
    first: CarrierEquation,
    second: CarrierEquation,
    points: Option<[[f64; 3]; 2]>,
    allow_unresolved_endpoint_witness: bool,
) -> Option<(CurveGeometry, &'static str)> {
    let (geometry, tag) = carrier_intersection_curve(first, second)?;
    let candidates = analytic_curve_branches(&geometry, tag);
    resolve_curve_candidates(candidates.clone(), points).or_else(|| {
        // A one-sided pcurve on an unresolved adjacent face supplies only a
        // finite-edge witness. It does not veto the exact infinite plane line.
        (tag == "plane_intersection_line"
            && (points.is_none() || allow_unresolved_endpoint_witness))
            .then(|| resolve_curve_candidates(candidates, None))
            .flatten()
    })
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
    let endpoint_evidence = pcurve_edge_endpoint_evidence(scan, ir);
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
        let curve_id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        let allow_unresolved_endpoint_witness = endpoint_evidence
            .get(&row.id)
            .is_some_and(|evidence| !evidence.complete)
            && !nurbs_endpoint_witnesses.contains(&curve_id);
        let resolved = resolve_carrier_intersection_curve(
            first,
            second,
            points,
            allow_unresolved_endpoint_witness,
        )
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
        let id = curve_id;
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
            exactly_one(ir.model.surfaces.iter().filter(|surface| surface.id == id))
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{CurveGeometry, NurbsSurface, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::AnnotationBuilder;

    use super::transfer_nurbs_boundary_curves;
    use super::{resolve_carrier_intersection_curve, transfer_carrier_intersection_curves};
    use crate::decode::analytic::{CarrierEquation, PlaneEquation};
    use crate::topology::{HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence, TopologicalVertex};
    use crate::{container, curve, surface};

    #[test]
    fn carrier_intersection_rejects_solved_endpoints_off_candidate() {
        let first = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        });
        let second = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        });
        let off_candidate = [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0]];

        assert!(
            resolve_carrier_intersection_curve(first, second, Some(off_candidate), false).is_none()
        );
        assert!(
            resolve_carrier_intersection_curve(first, second, Some(off_candidate), true).is_some()
        );
        assert!(resolve_carrier_intersection_curve(first, second, None, false).is_some());
    }

    #[test]
    fn plane_intersection_survives_inconsistent_endpoint_witness() {
        let mut scan = container::scan_bytes(Vec::new());
        scan.surfaces.rows = [1_u32, 2, 3]
            .into_iter()
            .map(|id| surface::SurfaceRow {
                id,
                type_byte: surface::SurfaceKind::Plane.canonical_type_byte(),
                kind: surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 0,
            })
            .collect();
        scan.curves.topology_rows = vec![curve::CurveTopologyRow {
            id: 10,
            type_byte: 0,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [1, 2],
            next_edges: [10, 10],
            offset: 0,
        }];
        scan.curves.pcurves = vec![curve::PcurveEndpoints {
            curve_id: 10,
            faces: [1, 3],
            face_0_endpoints: [[0.0, 1.0], [1.0, 1.0]],
            face_1_endpoints: [[0.0, 0.0], [1.0, 0.0]],
            offset: 0,
        }];
        scan.topology.half_edges = vec![
            HalfEdge {
                id: HalfEdgeId {
                    curve_id: 10,
                    side: 0,
                },
                face_id: 1,
                next: None,
            },
            HalfEdge {
                id: HalfEdgeId {
                    curve_id: 10,
                    side: 1,
                },
                face_id: 2,
                next: None,
            },
        ];
        scan.topology.vertices = vec![
            TopologicalVertex {
                id: 1,
                half_edges: vec![HalfEdgeId {
                    curve_id: 10,
                    side: 0,
                }],
            },
            TopologicalVertex {
                id: 2,
                half_edges: vec![HalfEdgeId {
                    curve_id: 10,
                    side: 1,
                }],
            },
        ];
        scan.topology.half_edge_vertex_incidence = vec![
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 10,
                    side: 0,
                },
                start_vertex_id: 1,
                end_vertex_id: Some(2),
            },
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 10,
                    side: 1,
                },
                start_vertex_id: 2,
                end_vertex_id: Some(1),
            },
        ];

        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId("creo:visibgeom:surface#1".to_string()),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 2.0, 0.0),
                    normal: Vector3::new(0.0, 1.0, 0.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
            Surface {
                id: SurfaceId("creo:visibgeom:surface#2".to_string()),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
            Surface {
                id: SurfaceId("creo:visibgeom:surface#3".to_string()),
                geometry: SurfaceGeometry::Unknown { record: None },
                source_object: None,
            },
        ]);

        let transferred = transfer_carrier_intersection_curves(
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &BTreeSet::new(),
        );
        assert_eq!(
            transferred,
            BTreeSet::from([CurveId("creo:visibgeom:curve#10".to_string())])
        );
        assert!(matches!(
            ir.model
                .curves
                .iter()
                .find(|curve| curve.id == CurveId("creo:visibgeom:curve#10".to_string()))
                .map(|curve| &curve.geometry),
            Some(CurveGeometry::Line { origin, direction })
                if origin.x == 0.0
                    && origin.y == 2.0
                    && origin.z == 0.0
                    && direction.x == 1.0
                    && direction.y == 0.0
                    && direction.z == 0.0
        ));
    }

    #[test]
    fn nurbs_boundary_rejects_duplicate_model_surface_ids() {
        let mut scan = container::scan_bytes(Vec::new());
        scan.surfaces.rows = vec![
            surface::SurfaceRow {
                id: 1,
                type_byte: surface::SurfaceKind::Extrusion.canonical_type_byte(),
                kind: surface::SurfaceKind::Extrusion,
                feature_id: 0,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 0,
            },
            surface::SurfaceRow {
                id: 2,
                type_byte: surface::SurfaceKind::Plane.canonical_type_byte(),
                kind: surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 0,
            },
        ];
        scan.curves.topology_rows = vec![curve::CurveTopologyRow {
            id: 10,
            type_byte: 0,
            feature_id: 0,
            directions: [0; 2],
            faces: [1, 2],
            next_edges: [10, 10],
            offset: 0,
        }];

        let extrusion = Surface {
            id: SurfaceId("creo:visibgeom:surface#1".to_string()),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_knots: vec![0.0, 0.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count: 2,
                v_count: 2,
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(1.0, 0.0, 1.0),
                    Point3::new(1.0, 1.0, 1.0),
                ],
                weights: None,
                u_periodic: false,
                v_periodic: false,
            }),
            source_object: None,
        };
        let plane = Surface {
            id: SurfaceId("creo:visibgeom:surface#2".to_string()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .surfaces
            .extend([extrusion.clone(), extrusion, plane]);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default())
            .expect("test decode context");

        let result =
            transfer_nurbs_boundary_curves(&ctx, &scan, &mut ir, &mut AnnotationBuilder::new())
                .expect("transfer should not fail");

        assert!(result.ids.is_empty());
        assert!(ir.model.curves.is_empty());
    }
}
