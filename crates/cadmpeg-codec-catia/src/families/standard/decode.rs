// SPDX-License-Identifier: Apache-2.0
//! Standard nested-stream decode route: B-rep topology attach and geometry.

use cadmpeg_core::decode::{alloc_filled, DecodeContext, WorkBudget};
use cadmpeg_ir::document::{CadIr, EntityRewrite, Model};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, NurbsSurface,
    Pcurve, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::schema::EntitySchema;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell,
    Vertex, VertexUse,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::{AnnotationBuilder, Annotations};
use serde::{de::DeserializeOwned, Serialize};
use serde_value::{Value, ValueDeserializer};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::assemble::cgm_source;
use crate::assemble::{
    annotate, attach_free_vertices, build_geometry_report,
    circle_parameter_range_from_surface_branch, link_payload_carriers, neutral_model_is_admissible,
    ordered_range, preserve_raw_payload, rational_pcurve_arc, source_meta, unit_vector,
    unwrap_angle, TypedCounts,
};
use crate::container::{self, ContainerScan};
use crate::families::freeform::{
    append_consolidated_revolutions, append_freeform_surface_pools, ConsolidatedRevolutionBinding,
};
use crate::families::standard::{fbb, topology};
use crate::families::FamilyOutput;
use crate::loss::CatiaLossCode;
use crate::solve::matching::{
    distinct_domain_matching_with_budget, retain_distinct_matching_supports,
};
use crate::solve::{mesh_gauge::MeshEdgeGeometry, mesh_quotient, missing_edge};
use crate::variant::Variant;
use crate::wire::records::ConsolidatedRecord;

const EPS_PARAM_RESOLUTION_SPAN: f64 = 1e-7;
const EPS_PARAM_TOLERANCE_SPAN: f64 = 1e-9;
const EPS_SAME_CONE_GENERATOR: f64 = 2e-3;
const EPS_ANTIPODAL_CIRCLE: f64 = 2e-3;
const SPHERE_SECTION_ENDPOINT_TOLERANCE: f64 = 2e-3;
const SPHERE_CENTER_COINCIDENCE_TOLERANCE: f64 = 2e-3;
const CYLINDER_PLANE_CONIC_TOLERANCE: f64 = 2e-3;
const PERPENDICULAR_CYLINDER_CONIC_TOLERANCE: f64 = 2e-3;
const LINE_SEGMENT_GEOMETRY_TOLERANCE: f64 = 2e-3;
const ANALYTIC_CURVE_ENDPOINT_TOLERANCE: f64 = 2e-3;
const SUPPORT_AGREEMENT_TOLERANCE: f64 = 1e-6;
const STANDARD_FACE_BOUNDS_TOLERANCE: f64 = 2e-3;
const NURBS_SURFACE_MEMBERSHIP_TOLERANCE: f64 = 2e-3;
const NURBS_SHARED_BOUNDARY_TOLERANCE: f64 = 1e-9;
const NURBS_SURFACE_SEEDS_PER_SPAN: usize = 3;
const NURBS_SURFACE_MAX_SEEDS: usize = 256;
const NURBS_SURFACE_REFINEMENT_ITERATIONS: usize = 24;
const NURBS_SURFACE_BACKTRACK_STEPS: usize = 8;
const NURBS_LINE_FACE_SAMPLES: [f64; 3] = [0.25, 0.5, 0.75];

fn bind_consolidated_revolution_faces_and_seams(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    revolutions: &[ConsolidatedRevolutionBinding],
) -> (usize, usize) {
    const TOLERANCE: f64 = 2e-3;

    fn point_on_torus(point: Point3, geometry: &SurfaceGeometry, tolerance: f64) -> bool {
        let SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } = geometry
        else {
            return false;
        };
        let offset = point.vector_from(*center);
        let axial = offset.dot(*axis);
        let radial = Vector3::new(
            offset.x - axial * axis.x,
            offset.y - axial * axis.y,
            offset.z - axial * axis.z,
        );
        let radial = radial.norm();
        [
            (radial - major_radius).hypot(axial),
            (radial + major_radius).hypot(axial),
        ]
        .into_iter()
        .any(|distance| (distance - minor_radius).abs() < tolerance)
    }

    fn meridian_arc(
        start: Point3,
        end: Point3,
        geometry: &SurfaceGeometry,
        expected_sweep: f64,
    ) -> Option<(CurveGeometry, [f64; 2])> {
        let SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } = geometry
        else {
            return None;
        };
        if !expected_sweep.is_finite()
            || expected_sweep <= 0.0
            || expected_sweep > std::f64::consts::PI
        {
            return None;
        }
        let start_offset = start.vector_from(*center);
        let start_axial = start_offset.dot(*axis);
        let start_radial = Vector3::new(
            start_offset.x - start_axial * axis.x,
            start_offset.y - start_axial * axis.y,
            start_offset.z - start_axial * axis.z,
        );
        let radial_norm = start_radial.norm();
        if !radial_norm.is_finite() || radial_norm == 0.0 {
            return None;
        }
        let radial_direction = Vector3::new(
            start_radial.x / radial_norm,
            start_radial.y / radial_norm,
            start_radial.z / radial_norm,
        );
        let mut centers = [-1.0, 1.0].into_iter().filter_map(|sign| {
            let circle_center = Point3::new(
                center.x + sign * major_radius * radial_direction.x,
                center.y + sign * major_radius * radial_direction.y,
                center.z + sign * major_radius * radial_direction.z,
            );
            let first = start.vector_from(circle_center);
            let second = end.vector_from(circle_center);
            (((first.norm() - minor_radius).abs() < TOLERANCE)
                && ((second.norm() - minor_radius).abs() < TOLERANCE))
                .then_some((circle_center, first, second))
        });
        let (circle_center, first, second) = centers.next()?;
        if centers.next().is_some() {
            return None;
        }
        let first_norm = first.norm();
        let second_norm = second.norm();
        let cosine = (first.dot(second) / (first_norm * second_norm)).clamp(-1.0, 1.0);
        let sweep = cosine.acos();
        if (sweep - expected_sweep).abs() > TOLERANCE / minor_radius.max(1.0) {
            return None;
        }
        let normal = first.cross(second);
        let normal_norm = normal.norm();
        if !normal_norm.is_finite()
            || normal_norm == 0.0
            || (normal.dot(*axis) / normal_norm).abs() > 1e-6
        {
            return None;
        }
        Some((
            CurveGeometry::Circle {
                center: circle_center,
                axis: Vector3::new(
                    normal.x / normal_norm,
                    normal.y / normal_norm,
                    normal.z / normal_norm,
                ),
                ref_direction: Vector3::new(
                    first.x / first_norm,
                    first.y / first_norm,
                    first.z / first_norm,
                ),
                radius: *minor_radius,
            },
            [0.0, expected_sweep],
        ))
    }

    let point_positions = ir
        .model
        .points
        .iter()
        .map(|point| (point.id.clone(), point.position))
        .collect::<HashMap<_, _>>();
    let vertex_positions = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| Some((vertex.id.clone(), *point_positions.get(&vertex.point)?)))
        .collect::<HashMap<_, _>>();
    let edge_indices = ir
        .model
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| (edge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let coedge_indices = ir
        .model
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let loop_indices = ir
        .model
        .loops
        .iter()
        .enumerate()
        .map(|(index, loop_)| (loop_.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let unknown_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. }))
        .map(|surface| surface.id.clone())
        .collect::<HashSet<_>>();
    let curve_indices = ir
        .model
        .curves
        .iter()
        .enumerate()
        .map(|(index, curve)| (curve.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut surface_bindings = HashMap::<SurfaceId, usize>::new();
    for face in &ir.model.faces {
        if !unknown_surfaces.contains(&face.surface) {
            continue;
        }
        let face_edges = face
            .loops
            .iter()
            .filter_map(|id| loop_indices.get(id))
            .flat_map(|index| &ir.model.loops[*index].coedges)
            .filter_map(|id| coedge_indices.get(id))
            .filter_map(|index| edge_indices.get(&ir.model.coedges[*index].edge))
            .collect::<Vec<_>>();
        let mut witnesses = Vec::with_capacity(3 * face_edges.len());
        for index in face_edges {
            let edge = &ir.model.edges[*index];
            witnesses.extend(
                [&edge.start, &edge.end]
                    .into_iter()
                    .filter_map(|id| vertex_positions.get(id).copied()),
            );
            let Some(curve) = edge
                .curve
                .as_ref()
                .and_then(|id| curve_indices.get(id))
                .map(|index| &ir.model.curves[*index].geometry)
            else {
                continue;
            };
            let Some([start, end]) = edge.param_range else {
                continue;
            };
            let parameter = start + (end - start) * 0.5;
            if let Some(point) = cadmpeg_ir::eval::curve_point(curve, parameter) {
                witnesses.push(point);
            }
        }
        if witnesses.len() < 2 {
            continue;
        }
        let mut matches = revolutions
            .iter()
            .enumerate()
            .filter(|(_, revolution)| {
                witnesses
                    .iter()
                    .all(|point| point_on_torus(*point, &revolution.geometry, TOLERANCE))
            })
            .map(|(index, _)| index);
        let Some(binding) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        surface_bindings
            .entry(face.surface.clone())
            .and_modify(|stored| {
                if *stored != binding {
                    *stored = usize::MAX;
                }
            })
            .or_insert(binding);
    }
    surface_bindings.retain(|_, binding| *binding != usize::MAX);
    for (surface_id, binding) in &surface_bindings {
        if let Some(surface) = ir
            .model
            .surfaces
            .iter_mut()
            .find(|surface| &surface.id == surface_id)
        {
            surface.geometry = revolutions[*binding].geometry.clone();
            annotations.derived(&surface.id, "geometry");
        }
    }

    let mut procedural_bindings = HashMap::<CurveId, Option<usize>>::new();
    for procedure in &ir.model.procedural_curves {
        let binding = (|| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedure.definition
            else {
                return None;
            };
            let [Some(first), Some(second)] =
                std::array::from_fn(|side| context.sides[side].surface.as_ref())
            else {
                return None;
            };
            let binding = *surface_bindings.get(first)?;
            (surface_bindings.get(second) == Some(&binding)).then_some(binding)
        })();
        let Some(binding) = binding else {
            continue;
        };
        procedural_bindings
            .entry(procedure.curve.clone())
            .and_modify(|stored| *stored = None)
            .or_insert(Some(binding));
    }
    let curve_edge_counts = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.as_ref())
        .fold(HashMap::<CurveId, usize>::new(), |mut counts, curve| {
            *counts.entry(curve.clone()).or_default() += 1;
            counts
        });
    let mut seam_count = 0usize;
    for edge in &mut ir.model.edges {
        let Some(curve_id) = edge.curve.as_ref() else {
            continue;
        };
        let Some(&curve_index) = curve_indices.get(curve_id) else {
            continue;
        };
        if !matches!(
            ir.model.curves[curve_index].geometry,
            CurveGeometry::Unknown { .. }
        ) || curve_edge_counts.get(curve_id) != Some(&1)
        {
            continue;
        }
        let Some(Some(binding)) = procedural_bindings.get(curve_id) else {
            continue;
        };
        let Some(start) = vertex_positions.get(&edge.start).copied() else {
            continue;
        };
        let Some(end) = vertex_positions.get(&edge.end).copied() else {
            continue;
        };
        let Some((geometry, parameter_range)) = meridian_arc(
            start,
            end,
            &revolutions[*binding].geometry,
            revolutions[*binding].profile_sweep,
        ) else {
            continue;
        };
        ir.model.curves[curve_index].geometry = geometry;
        edge.param_range = Some(parameter_range);
        annotations
            .derived(&ir.model.curves[curve_index].id, "geometry")
            .derived(&edge.id, "param_range");
        seam_count += 1;
    }
    (surface_bindings.len(), seam_count)
}

#[cfg(test)]
mod consolidated_revolution_binding_tests {
    use super::bind_consolidated_revolution_faces_and_seams;
    use crate::families::freeform::ConsolidatedRevolutionBinding;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralCurveDefinition, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ProceduralCurveId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Sense, Vertex};
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::AnnotationBuilder;

    #[test]
    fn one_revolution_torus_closes_face_aliases_and_meridian_seam() {
        let mut ir = CadIr::empty(Units::default());
        let surface_ids = [
            SurfaceId("face-surface#0".to_string()),
            SurfaceId("face-surface#1".to_string()),
        ];
        for id in &surface_ids {
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Unknown { record: None },
                source_object: None,
            });
        }
        let geometry = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 2.0,
            minor_radius: 3.0,
        };
        let profile_end = std::f64::consts::PI - 0.5;
        let positions = [
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(2.0 + 3.0 * profile_end.cos(), 0.0, 3.0 * profile_end.sin()),
        ];
        for (index, position) in positions.into_iter().enumerate() {
            let point = PointId(format!("point#{index}"));
            ir.model.points.push(Point {
                id: point.clone(),
                position,
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: VertexId(format!("vertex#{index}")),
                point,
                tolerance: None,
            });
        }
        let curve_id = CurveId("seam-curve".to_string());
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId("seam-edge".to_string()),
            curve: Some(curve_id.clone()),
            start: VertexId("vertex#0".to_string()),
            end: VertexId("vertex#1".to_string()),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        });
        for (side, surface) in surface_ids.iter().enumerate() {
            let face = FaceId(format!("face#{side}"));
            let loop_id = LoopId(format!("loop#{side}"));
            let coedge = CoedgeId(format!("coedge#{side}"));
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: ShellId("shell".to_string()),
                surface: surface.clone(),
                sense: Sense::Forward,
                loops: vec![loop_id.clone()],
                name: None,
                color: None,
                tolerance: None,
            });
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face,
                boundary_role: LoopBoundaryRole::Unspecified,
                coedges: vec![coedge.clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id,
                edge: EdgeId("seam-edge".to_string()),
                next: coedge.clone(),
                previous: coedge.clone(),
                radial_next: CoedgeId(format!("coedge#{}", 1 - side)),
                sense: if side == 0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
        }
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId("seam-construction".to_string()),
            curve: curve_id.clone(),
            definition: ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: std::array::from_fn(|side| IntcurveSupportSide {
                        surface: Some(surface_ids[side].clone()),
                        pcurve: None,
                        pcurve_parameter_range: None,
                    }),
                    parameter_range: [0.0, 1.0],
                    discontinuities: std::array::from_fn(|_| Vec::new()),
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });

        assert_eq!(
            bind_consolidated_revolution_faces_and_seams(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &[ConsolidatedRevolutionBinding {
                    geometry: geometry.clone(),
                    profile_sweep: 0.5,
                }],
            ),
            (2, 1)
        );
        assert!(ir
            .model
            .surfaces
            .iter()
            .all(|surface| surface.geometry == geometry));
        assert!(matches!(
            ir.model.curves[0].geometry,
            CurveGeometry::Circle { radius: 3.0, .. }
        ));
        assert_eq!(ir.model.edges[0].param_range, Some([0.0, 0.5]));
    }
}

fn refine_consolidated_analytic_surfaces(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    surfaces: &mut [Option<SurfaceGeometry>],
) -> HashMap<usize, usize> {
    fn exactly_one<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
        let value = values.next()?;
        values.next().is_none().then_some(value)
    }

    let cylinders = crate::families::b2::records::b2_cylinders_from_records(bytes, records);
    let cones = crate::families::b2::records::b2_cones_from_records(bytes, records);
    let spheres = crate::families::b2::records::b2_spheres_from_records(bytes, records);
    let tori = crate::families::b2::records::b2_tori_from_records(bytes, records);
    let quantized = |value: f64| f64::from(value as f32);
    let same_point = |point: Point3, stored: [f64; 3]| {
        point.x.to_bits() == quantized(stored[0]).to_bits()
            && point.y.to_bits() == quantized(stored[1]).to_bits()
            && point.z.to_bits() == quantized(stored[2]).to_bits()
    };
    let same_axis = |axis: Vector3, stored: [f64; 3]| {
        let x = stored[0] as f32;
        let y = stored[1] as f32;
        let z = (1.0 - f64::from(x * x + y * y))
            .max(0.0)
            .sqrt()
            .copysign(stored[2]);
        unit_vector(Vector3::new(f64::from(x), f64::from(y), z)).is_some_and(|reconstructed| {
            axis.x.to_bits() == reconstructed.x.to_bits()
                && axis.y.to_bits() == reconstructed.y.to_bits()
                && axis.z.to_bits() == reconstructed.z.to_bits()
        })
    };
    let mut refined = HashMap::new();
    for (index, surface) in surfaces.iter_mut().enumerate() {
        let replacement = match surface.as_ref() {
            Some(SurfaceGeometry::Cylinder {
                origin,
                axis,
                radius,
                ..
            }) => exactly_one(cylinders.iter().filter_map(|cylinder| {
                let geometry = &cylinder.geometry;
                let SurfaceGeometry::Cylinder {
                    axis: exact_axis, ..
                } = geometry
                else {
                    return None;
                };
                (same_point(*origin, cylinder.origin)
                    && same_axis(*axis, [exact_axis.x, exact_axis.y, exact_axis.z])
                    && radius.to_bits() == quantized(cylinder.radius).to_bits())
                .then_some((geometry, cylinder.pos))
            }))
            .map(|(geometry, pos)| (geometry.clone(), pos)),
            Some(SurfaceGeometry::Cone {
                origin,
                axis,
                radius,
                ratio,
                half_angle,
                ..
            }) if *radius == 0.0 && *ratio == 1.0 => exactly_one(cones.iter().filter(|cone| {
                same_point(*origin, cone.apex)
                    && same_axis(*axis, cone.axis)
                    && half_angle.to_bits() == quantized(cone.half_angle).to_bits()
            }))
            .map(|cone| {
                (
                    SurfaceGeometry::Cone {
                        origin: Point3::new(cone.apex[0], cone.apex[1], cone.apex[2]),
                        axis: Vector3::new(cone.axis[0], cone.axis[1], cone.axis[2]),
                        ref_direction: Vector3::new(cone.t1[0], cone.t1[1], cone.t1[2]),
                        radius: 0.0,
                        ratio: 1.0,
                        half_angle: cone.half_angle,
                    },
                    cone.pos,
                )
            }),
            Some(SurfaceGeometry::Sphere { center, radius, .. }) => {
                exactly_one(spheres.iter().filter(|sphere| {
                    same_point(*center, sphere.center)
                        && radius.to_bits() == quantized(sphere.radius).to_bits()
                }))
                .map(|sphere| {
                    (
                        crate::families::b2::records::b2_sphere_geometry(sphere),
                        sphere.pos,
                    )
                })
            }
            Some(SurfaceGeometry::Torus {
                center,
                axis,
                major_radius,
                minor_radius,
                ..
            }) => exactly_one(tori.iter().filter(|torus| {
                same_point(*center, torus.center)
                    && same_axis(*axis, torus.axis)
                    && major_radius.to_bits() == quantized(torus.major_radius).to_bits()
                    && minor_radius.to_bits() == quantized(torus.minor_radius).to_bits()
            }))
            .map(|torus| {
                (
                    crate::families::b2::records::b2_torus_geometry(torus),
                    torus.pos,
                )
            }),
            _ => None,
        };
        if let Some((geometry, source_pos)) = replacement {
            *surface = Some(geometry);
            refined.insert(index, source_pos);
        }
    }
    refined
}

#[cfg(test)]
mod consolidated_analytic_refinement_tests {
    use super::refine_consolidated_analytic_surfaces;
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    #[test]
    fn unique_quantized_torus_refines_every_matching_face_to_binary64() {
        let mut bytes = crate::test_support::b2_torus_stream();
        let exact_x = 1.000_000_01_f64;
        bytes[5..13].copy_from_slice(&exact_x.to_le_bytes());
        let coarse = SurfaceGeometry::Torus {
            center: Point3::new(f64::from(exact_x as f32), 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 7.0,
            minor_radius: 2.0,
        };
        let mut surfaces = vec![Some(coarse.clone()), Some(coarse)];
        let refined = refine_consolidated_analytic_surfaces(
            &bytes,
            &crate::wire::records::consolidated_records(&bytes),
            &mut surfaces,
        );
        assert_eq!(refined, [(0, 0), (1, 0)].into());
        for surface in surfaces {
            assert!(matches!(
                surface,
                Some(SurfaceGeometry::Torus { center, .. }) if center.x == exact_x
            ));
        }
    }

    #[test]
    fn sphere_refinement_requires_one_matching_consolidated_carrier() {
        let mut bytes = crate::test_support::b2_sphere_stream();
        let exact_x = 1.000_000_01_f64;
        bytes[5..13].copy_from_slice(&exact_x.to_le_bytes());
        let coarse = SurfaceGeometry::Sphere {
            center: Point3::new(f64::from(exact_x as f32), 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        };
        let mut unique = vec![Some(coarse.clone())];
        assert_eq!(
            refine_consolidated_analytic_surfaces(
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                &mut unique,
            ),
            [(0, 0)].into()
        );
        assert!(matches!(
            unique[0],
            Some(SurfaceGeometry::Sphere { center, .. }) if center.x == exact_x
        ));

        bytes.extend_from_slice(&bytes.clone());
        let mut ambiguous = vec![Some(coarse)];
        assert!(refine_consolidated_analytic_surfaces(
            &bytes,
            &crate::wire::records::consolidated_records(&bytes),
            &mut ambiguous,
        )
        .is_empty());
    }

    #[test]
    fn cylinder_and_cone_refinement_use_their_complete_exact_frames() {
        let mut bytes = crate::test_support::b2_cylinder_stream();
        bytes.extend_from_slice(&crate::test_support::b2_cone_stream());
        let mut surfaces = vec![
            Some(SurfaceGeometry::Cylinder {
                origin: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
            }),
            Some(SurfaceGeometry::Cone {
                origin: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 0.0,
                ratio: 1.0,
                half_angle: f64::from(0.25_f32),
            }),
        ];
        assert_eq!(
            refine_consolidated_analytic_surfaces(
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                &mut surfaces,
            )
            .len(),
            2
        );
        assert!(matches!(
            surfaces[0],
            Some(SurfaceGeometry::Cylinder { ref_direction, .. })
                if ref_direction == Vector3::new(0.0, 1.0, 0.0)
        ));
        assert!(matches!(
            surfaces[1],
            Some(SurfaceGeometry::Cone {
                half_angle: 0.25,
                ..
            })
        ));
    }
}

/// Materialize one exact object-stream support surface once.
fn standard_extrusion_support_id(
    annotations: &mut AnnotationBuilder,
    surfaces: &mut Vec<Surface>,
    procedural_supports: &mut HashMap<u32, SurfaceId>,
    side: &crate::families::b5::transfer::ResolvedExtrusionSupport,
) -> SurfaceId {
    procedural_supports
        .entry(side.surface_object_id)
        .or_insert_with(|| {
            let id = SurfaceId(format!(
                "catia:standard:procedural-support#{}",
                side.surface_object_id
            ));
            annotate(
                annotations,
                &id,
                "object_stream_b5_03",
                0,
                format!("surface:{:08x}", side.surface_object_id),
                Exactness::ByteExact,
            );
            surfaces.push(Surface {
                id: id.clone(),
                geometry: side.surface.clone(),
                source_object: Some(cgm_source("surface", side.surface_object_id)),
            });
            id
        })
        .clone()
}

/// Emit one resolved object-stream extrusion construction in the standard family.
pub(crate) fn emit_standard_extrusion_definition(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    surfaces: &mut Vec<Surface>,
    procedural_supports: &mut HashMap<u32, SurfaceId>,
    extrusion_definitions: &mut HashMap<u32, ProceduralSurfaceDefinition>,
    extrusion: crate::families::b5::transfer::ResolvedExtrusionSurface,
) -> ProceduralSurfaceDefinition {
    if let Some(definition) = extrusion_definitions.get(&extrusion.surface_object_id) {
        return definition.clone();
    }
    let surface_object_id = extrusion.surface_object_id;
    let directrix_id = CurveId(format!(
        "catia:standard:extrusion-directrix#{}",
        extrusion.directrix_object_id
    ));
    match extrusion.directrix {
        crate::families::b5::transfer::ResolvedExtrusionDirectrix::Intersection {
            supports,
            cache_fit_tolerance,
        } => {
            let sides = (*supports).map(|side| IntcurveSupportSide {
                surface: Some(standard_extrusion_support_id(
                    annotations,
                    surfaces,
                    procedural_supports,
                    &side,
                )),
                pcurve: Some(side.pcurve),
                pcurve_parameter_range: (side.pcurve_parameter_range
                    != extrusion.directrix_parameter_range)
                    .then_some(side.pcurve_parameter_range),
            });
            annotate(
                annotations,
                &directrix_id,
                "object_stream_a8_03_25",
                0,
                "two_support_directrix",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: directrix_id.clone(),
                geometry: CurveGeometry::Unknown { record: None },
                source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
            });
            let procedure_id = ProceduralCurveId(format!(
                "catia:standard:extrusion-directrix-procedure#{}",
                extrusion.directrix_object_id
            ));
            annotate(
                annotations,
                &procedure_id,
                "object_stream_a8_03_25",
                0,
                "two_surface_pcurve_intersection",
                Exactness::ByteExact,
            );
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedure_id,
                curve: directrix_id.clone(),
                definition: ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext {
                        sides,
                        parameter_range: extrusion.directrix_parameter_range,
                        discontinuities: std::array::from_fn(|_| Vec::new()),
                    },
                    discontinuity_flag: false,
                },
                cache_fit_tolerance: Some(cache_fit_tolerance),
            });
        }
        crate::families::b5::transfer::ResolvedExtrusionDirectrix::SurfaceCurve {
            curve, ..
        } => {
            annotate(
                annotations,
                &directrix_id,
                "object_stream_b5_03_24",
                0,
                "support_pcurve_lift",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: directrix_id.clone(),
                geometry: curve,
                source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
            });
        }
        crate::families::b5::transfer::ResolvedExtrusionDirectrix::Offset {
            source_object_id,
            support,
            source_curve,
            source_parameter_range,
            distance,
            direction,
        } => {
            let source_id = CurveId(format!(
                "catia:standard:extrusion-directrix-source#{source_object_id}"
            ));
            annotate(
                annotations,
                &source_id,
                "object_stream_b5_03_24",
                0,
                "support_pcurve_lift",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: source_id.clone(),
                geometry: source_curve,
                source_object: Some(cgm_source("curve", source_object_id)),
            });
            annotate(
                annotations,
                &directrix_id,
                "object_stream_b5_03_14",
                0,
                "fixed_direction_offset_curve",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: directrix_id.clone(),
                geometry: CurveGeometry::Unknown { record: None },
                source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
            });
            let procedure_id = ProceduralCurveId(format!(
                "catia:standard:extrusion-directrix-procedure#{}",
                extrusion.directrix_object_id
            ));
            annotate(
                annotations,
                &procedure_id,
                "object_stream_b5_03_14",
                0,
                "fixed_direction_offset_curve",
                Exactness::ByteExact,
            );
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedure_id,
                curve: directrix_id.clone(),
                definition: ProceduralCurveDefinition::Offset {
                    source: source_id,
                    distance,
                    direction: Some(direction),
                    support: Some(standard_extrusion_support_id(
                        annotations,
                        surfaces,
                        procedural_supports,
                        &support,
                    )),
                    normal: None,
                    parameter_range: Some(source_parameter_range),
                    distance_law: None,
                },
                cache_fit_tolerance: None,
            });
        }
    }
    let definition = ProceduralSurfaceDefinition::Extrusion {
        directrix: directrix_id,
        parameter_interval: Some(extrusion.directrix_parameter_range),
        direction: extrusion.direction,
        native_position: None,
        revision_form: None,
    };
    extrusion_definitions.insert(surface_object_id, definition.clone());
    definition
}

fn parameter_record_bounds(bounds: [[f64; 2]; 2]) -> [Option<f64>; 4] {
    [
        Some(bounds[0][0]),
        Some(bounds[0][1]),
        Some(bounds[1][0]),
        Some(bounds[1][1]),
    ]
}

fn standard_freeform_e5_carrier_ids(data: &[u8]) -> HashMap<u32, u32> {
    let mut face_surfaces = HashMap::<u32, Option<u32>>::new();
    for (face, surface) in crate::families::e5::graph::face_surface_references(data) {
        match face_surfaces.entry(face).or_insert(Some(surface)) {
            stored @ Some(_) if *stored != Some(surface) => *stored = None,
            _ => {}
        }
    }
    let face_surfaces = face_surfaces
        .into_iter()
        .filter_map(|(face, surface)| surface.map(|surface| (face, surface)))
        .collect::<HashMap<_, _>>();

    let mut wrappers = HashMap::<u32, Option<u32>>::new();
    for wrapper in crate::families::e5::records::e5_surface_wrappers(data) {
        match wrappers
            .entry(wrapper.record_id)
            .or_insert(Some(wrapper.underlying_surface()))
        {
            stored @ Some(_) if *stored != Some(wrapper.underlying_surface()) => *stored = None,
            _ => {}
        }
    }
    let wrappers = wrappers
        .into_iter()
        .filter_map(|(wrapper, surface)| surface.map(|surface| (wrapper, surface)))
        .collect::<HashMap<_, _>>();

    face_surfaces
        .into_iter()
        .filter_map(|(face, wrapper)| Some((face, *wrappers.get(&wrapper)?)))
        .collect()
}

/// Join a standard freeform face to a directly decoded E5 analytic carrier
/// through the serialized face and class-`0xf1` wrapper identities.
///
/// This path is exact: a geometric candidate is accepted only when the
/// standard tag names one E5 face, that face names one valid `0xf1` wrapper,
/// and the wrapper's first reference names one supported E5 surface carrier.
pub(crate) fn associate_standard_freeform_e5_surfaces(
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    data: &[u8],
) -> HashMap<u32, SurfaceGeometry> {
    let carrier_ids = standard_freeform_e5_carrier_ids(data);

    let mut surfaces = HashMap::<u32, Option<SurfaceGeometry>>::new();
    for surface in crate::families::e5::records::e5_surfaces(data) {
        match surfaces
            .entry(surface.record_id)
            .or_insert(Some(surface.geometry.clone()))
        {
            stored @ Some(_) if *stored != Some(surface.geometry.clone()) => *stored = None,
            _ => {}
        }
    }
    let surfaces = surfaces
        .into_iter()
        .filter_map(|(surface, geometry)| geometry.map(|geometry| (surface, geometry)))
        .collect::<HashMap<_, _>>();

    records
        .iter()
        .filter_map(|record| {
            let crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } =
                record
            else {
                return None;
            };
            let underlying_surface = *carrier_ids.get(tag)?;
            Some((*tag, surfaces.get(&underlying_surface)?.clone()))
        })
        .collect()
}

/// Join standard freeform faces to exact E5 class-`0xd8` rolling-ball jets.
/// The face and wrapper identities are the same strict join used by analytic
/// E5 carriers; only the underlying carrier decoder differs. The carrier's
/// signed sense must agree with the owning face orientation before admission.
pub(crate) fn associate_standard_freeform_e5_rolling_ball_jets(
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    data: &[u8],
) -> HashMap<u32, StandardSurfaceProcedure> {
    let carrier_ids = standard_freeform_e5_carrier_ids(data);
    let mut jets = HashMap::<u32, Option<crate::families::e5::records::E5RollingBallJet>>::new();
    for jet in crate::families::e5::records::e5_rolling_ball_jets(data) {
        match jets
            .entry(jet.record_id)
            .or_insert_with(|| Some(jet.clone()))
        {
            stored @ Some(_) if *stored != Some(jet) => *stored = None,
            _ => {}
        }
    }
    let jets = jets
        .into_iter()
        .filter_map(|(carrier, jet)| jet.map(|jet| (carrier, jet)))
        .collect::<HashMap<_, _>>();

    records
        .iter()
        .filter_map(|record| {
            let crate::families::standard::records::StandardSurfaceRecord::Freeform {
                tag,
                forward,
                ..
            } = record
            else {
                return None;
            };
            let carrier = *carrier_ids.get(tag)?;
            let jet = jets.get(&carrier)?;
            (*forward == (jet.sense == -1)).then_some((
                *tag,
                StandardSurfaceProcedure::RollingBall {
                    carrier_object_id: jet.record_id,
                    definition: jet.definition(),
                    source: StandardRollingBallSource::E5D8,
                },
            ))
        })
        .collect()
}

#[derive(Debug, Clone)]
struct StandardPopulationSelection {
    spine: Vec<u8>,
    records: Vec<crate::families::standard::records::StandardSurfaceRecord>,
    supports: Vec<crate::families::standard::records::StandardCurveSupport>,
    fbb_edge_table: bool,
    vertex_roster_compatible: bool,
}

fn standard_population_selections(
    scan: &ContainerScan<'_>,
) -> Option<Vec<StandardPopulationSelection>> {
    let brep = scan.brep.as_ref()?;
    let standard_spine = scan.main_data_stream.as_deref().unwrap_or(brep);
    let layouts = fbb::fbb_population_layouts(standard_spine);
    let populations = crate::families::standard::records::standard_surface_populations(brep);
    let pairs =
        crate::families::standard::records::pair_standard_populations(&layouts, &populations)?;
    pairs
        .into_iter()
        .map(|(layout, population)| {
            Some(StandardPopulationSelection {
                spine: fbb::population_spine(standard_spine, &layout)?.to_vec(),
                records: population.records,
                supports: population.supports,
                fbb_edge_table: layout.fbb_edge_table,
                vertex_roster_compatible:
                    crate::families::standard::records::standard_vertex_roster(
                        &scan.data,
                        layout.vertex_count,
                    )
                    .is_some(),
            })
        })
        .collect()
}

pub(crate) fn try_decode_standard(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Option<FamilyOutput> {
    let Some(selections) = standard_population_selections(scan) else {
        return try_decode_standard_population(ctx, scan, None);
    };
    if selections.len() == 1 {
        return try_decode_standard_population(ctx, scan, selections.first());
    }
    try_decode_standard_populations(ctx, scan, &selections)
}

fn retain_standard_population_model(model: &mut Model) {
    macro_rules! retain_standard {
        ($($field:ident),+ $(,)?) => {
            $(model.$field.retain(|entity| {
                entity.identity().starts_with("catia:standard:")
            });)+
        };
    }
    retain_standard!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        surfaces,
        curves,
        subds,
        pcurves,
        procedural_surfaces,
        procedural_curves,
        assets,
        features,
        feature_input_topologies,
        feature_result_topologies,
        configurations,
        parameters,
        sketches,
        sketch_entities,
        sketch_constraints,
        spatial_sketches,
        spatial_sketch_entities,
        spatial_sketch_constraints,
        spreadsheets,
        product_definitions,
        occurrences,
        assembly_joints,
        drawings,
        semantic_annotations,
        presentation_documents,
        view_presentations,
        tessellations,
        appearances,
        appearance_bindings,
        attributes,
        pmi,
        presentation_layers,
    );
}

fn rescope_standard_id(text: &str, scope: &str) -> String {
    text.strip_prefix("catia:standard:").map_or_else(
        || text.to_owned(),
        |rest| format!("catia:standard:{scope}/{rest}"),
    )
}

fn rescope_standard_value(value: &mut Value, scope: &str) {
    match value {
        Value::String(text) => *text = rescope_standard_id(text, scope),
        Value::Seq(items) => items
            .iter_mut()
            .for_each(|item| rescope_standard_value(item, scope)),
        Value::Map(fields) => {
            let entries = std::mem::take(fields);
            for (mut key, mut item) in entries {
                rescope_standard_value(&mut key, scope);
                rescope_standard_value(&mut item, scope);
                fields.insert(key, item);
            }
        }
        Value::Option(Some(item)) | Value::Newtype(item) => rescope_standard_value(item, scope),
        _ => {}
    }
}

struct StandardPopulationScope<'a> {
    scope: &'a str,
}

impl EntityRewrite for StandardPopulationScope<'_> {
    type Error = String;

    fn rewrite<T: Serialize + DeserializeOwned>(&mut self, entity: T) -> Result<T, Self::Error> {
        let mut value = serde_value::to_value(entity)
            .map_err(|error| format!("standard population entity serialization failed: {error}"))?;
        rescope_standard_value(&mut value, self.scope);
        T::deserialize(ValueDeserializer::<serde_value::DeserializerError>::new(
            value,
        ))
        .map_err(|error| format!("standard population entity rewrite failed: {error}"))
    }
}

fn merge_standard_population_annotations(
    target: &mut Annotations,
    source: Annotations,
    scope: &str,
) -> Option<()> {
    let mut stream_map = Vec::with_capacity(source.streams.len());
    for stream in source.streams {
        let index = target
            .streams
            .iter()
            .position(|candidate| candidate == &stream)
            .unwrap_or_else(|| {
                target.streams.push(stream);
                target.streams.len() - 1
            });
        stream_map.push(u32::try_from(index).ok()?);
    }
    for (id, mut provenance) in source.provenance {
        let stream = *stream_map.get(usize::try_from(provenance.stream).ok()?)?;
        provenance.stream = stream;
        target
            .provenance
            .insert(rescope_standard_id(&id, scope), provenance);
    }
    target.exactness.extend(
        source
            .exactness
            .into_iter()
            .map(|(id, note)| (rescope_standard_id(&id, scope), note)),
    );
    Some(())
}

fn try_decode_standard_populations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    selections: &[StandardPopulationSelection],
) -> Option<FamilyOutput> {
    let mut outputs = selections
        .iter()
        .map(|selection| try_decode_standard_population(ctx, scan, Some(selection)))
        .collect::<Option<Vec<_>>>()?;
    let attached_topology_count = outputs
        .iter()
        .map(|output| {
            output
                .report
                .coverage
                .get("attached_standard_topology_count")
                .copied()
                .unwrap_or_default()
        })
        .sum::<usize>();
    let population_coverage = [
        "attempted_standard_topology_count",
        "standard_topology_curve_support_count",
        "standard_topology_native_endpoint_pair_count",
        "standard_topology_empty_endpoint_domain_count",
        "standard_topology_singleton_endpoint_domain_count",
        "standard_topology_multiple_endpoint_domain_count",
        "standard_topology_endpoint_domain_choice_count",
    ]
    .into_iter()
    .map(|key| {
        (
            key,
            outputs
                .iter()
                .map(|output| output.report.coverage.get(key).copied().unwrap_or_default())
                .sum::<usize>(),
        )
    })
    .collect::<Vec<_>>();
    let admitted_face_rows = selections
        .iter()
        .map(|selection| selection.records.len())
        .sum::<usize>();
    let all_fbb_rows_admitted =
        selections.len() == scan.census.fbb_runs && admitted_face_rows == scan.census.fbb_face_rows;
    let all_topologies_attached = attached_topology_count == selections.len();

    let mut merged = outputs.remove(0);
    for (index, (_population, output)) in selections.iter().skip(1).zip(outputs).enumerate() {
        let scope = format!("population-{}", index + 1);
        let mut model = output.ir.model;
        retain_standard_population_model(&mut model);
        let mut rewriter = StandardPopulationScope { scope: &scope };
        merged
            .ir
            .model
            .extend_rewritten(model, &mut rewriter)
            .ok()?;
        merge_standard_population_annotations(&mut merged.annotations, output.annotations, &scope)?;
        merged.report.geometry_transferred |= output.report.geometry_transferred;
    }

    for (key, value) in population_coverage {
        merged.report.coverage.insert(key.to_string(), value);
    }
    merged
        .report
        .coverage
        .insert("standard_fbb_run_count".to_string(), scan.census.fbb_runs);
    merged.report.coverage.insert(
        "standard_fbb_candidate_face_row_count".to_string(),
        scan.census.fbb_face_rows,
    );
    merged.report.coverage.insert(
        "standard_fbb_admitted_face_row_count".to_string(),
        admitted_face_rows,
    );
    merged.report.coverage.insert(
        "standard_fbb_withheld_face_row_count".to_string(),
        scan.census.fbb_face_rows.saturating_sub(admitted_face_rows),
    );
    merged.report.coverage.insert(
        "attached_standard_topology_count".to_string(),
        attached_topology_count,
    );

    merged.report.losses.retain(|loss| {
        !matches!(
            loss.code.local_code(),
            "topology.fbb-rows-withheld"
                | "geometry.carrier-summary"
                | "geometry.unresolved-carriers"
        )
    });
    if !all_fbb_rows_admitted {
        merged.report.losses.push(CatiaLossCode::TopologyFbbRowsWithheld.note(
            format!(
                "{} candidate FBB face row(s) in {} marker group(s) were not admitted to the standard topology population; only {} row(s) have source-closed population bindings.",
                scan.census.fbb_face_rows.saturating_sub(admitted_face_rows),
                scan.census.fbb_runs,
                admitted_face_rows,
            ),
        ));
    }
    if !all_topologies_attached {
        merged
            .report
            .losses
            .push(CatiaLossCode::TopologyBoundaryGraphNotEmitted.note(format!(
            "The B-rep boundary graph was emitted for {} of {} source-closed standard populations.",
            attached_topology_count,
            selections.len(),
        )));
    }
    let mut typed = TypedCounts::default();
    for surface in &merged.ir.model.surfaces {
        typed.record(&surface.geometry);
    }
    merged.report.losses.push(CatiaLossCode::GeometryCarrierSummary.note(format!(
        "{} vertex point(s) were decoded verbatim from `05 08 01` records (3×f32 LE, millimetres, identity world placement) and {} analytic surface carrier(s) were decoded from `SurfacicReps` `00 33` records: {} plane, {} cylinder, {} cone, {} sphere, {} torus.",
        merged.ir.model.vertices.len(),
        typed.total(),
        typed.plane,
        typed.cylinder,
        typed.cone,
        typed.sphere,
        typed.torus,
    )));
    crate::assemble::insert_unresolved_carrier_loss(&merged.ir, &mut merged.report.losses);
    Some(merged)
}

fn try_decode_standard_population(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    selection: Option<&StandardPopulationSelection>,
) -> Option<FamilyOutput> {
    let work_budget = ctx.work_budget(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS as u64);
    let brep = scan.brep.as_ref()?;
    let default_spine = scan.main_data_stream.as_deref().unwrap_or(brep);
    let standard_spine = selection.map_or(default_spine, |selection| selection.spine.as_slice());
    let fbb_only = selection.map_or(scan.variant == Variant::FbbOnly, |selection| {
        selection.fbb_edge_table
    });
    if !work_budget.charge() {
        return None;
    }
    let consolidated_records = crate::wire::records::consolidated_records_in_sources(
        &scan.data,
        container::consolidated_record_sources(scan),
    );
    let points = (if fbb_only {
        fbb::fbb_only_vertex_points(standard_spine)
    } else {
        fbb::standard_vertex_points(standard_spine)
    })
    .unwrap_or_default()
    .into_iter()
    .map(|[x, y, z]| Point3::new(x, y, z))
    .collect::<Vec<_>>();
    let vertex_roster = selection
        .is_none_or(|selection| selection.vertex_roster_compatible)
        .then(|| {
            crate::families::standard::records::standard_vertex_roster(&scan.data, points.len())
        })
        .flatten();
    let face_count = selection.map_or_else(
        || fbb::standard_face_count(standard_spine).unwrap_or_default(),
        |selection| selection.records.len(),
    );
    let records = selection.map_or_else(
        || {
            crate::families::standard::records::standard_surface_records(brep, face_count)
                .unwrap_or_else(|| {
                    crate::families::standard::records::surface_prefixes(brep)
                        .into_iter()
                        .map(crate::families::standard::records::StandardSurfaceRecord::Analytic)
                        .collect()
                })
        },
        |selection| selection.records.clone(),
    );
    let analytic_record_count = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                crate::families::standard::records::StandardSurfaceRecord::Analytic(_)
            )
        })
        .count();
    let freeform_tags = records
        .iter()
        .filter_map(|record| match record {
            crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } => {
                Some(*tag)
            }
            crate::families::standard::records::StandardSurfaceRecord::Analytic(_) => None,
        })
        .collect::<HashSet<_>>();
    let standard_edge_count = selection.map_or_else(
        || {
            (if fbb_only {
                fbb::fbb_only_edge_count(standard_spine)
            } else {
                fbb::standard_edge_count(standard_spine)
            })
            .filter(|count| *count > 0)
        },
        |selection| (!selection.supports.is_empty()).then_some(selection.supports.len()),
    );
    let curve_supports = selection.map_or_else(
        || {
            crate::families::standard::records::standard_curve_supports(
                brep,
                face_count,
                standard_edge_count,
            )
        },
        |selection| selection.supports.clone(),
    );
    let edge_tags = curve_supports
        .iter()
        .map(|support| support.tag)
        .collect::<HashSet<_>>();
    let object_evidence =
        standard_object_evidence(scan, &freeform_tags, &edge_tags, &consolidated_records);
    let standard_limit_curve_count = object_evidence.limit_curves.len();
    let revolution_record_count = crate::families::b2::records::b2_revolutions_from_records(
        &scan.data,
        &consolidated_records,
    )
    .len();
    let face_frame_vectors = fbb::standard_face_frame_vectors(standard_spine, records.len());
    let mut curved_surfaces = records
        .iter()
        .map(|record| match record {
            crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix)
                if prefix.kind != 0x32 =>
            {
                crate::families::standard::records::decode_curved(brep, prefix)
            }
            crate::families::standard::records::StandardSurfaceRecord::Analytic(_)
            | crate::families::standard::records::StandardSurfaceRecord::Freeform { .. } => None,
        })
        .collect::<Vec<_>>();
    let refined_analytic_surfaces = refine_consolidated_analytic_surfaces(
        &scan.data,
        &consolidated_records,
        &mut curved_surfaces,
    );
    let plane_normals = standard_plane_normals_from_face_frames(&records, &face_frame_vectors);
    let planes: HashMap<u32, crate::families::standard::records::PlaneParams> =
        crate::families::standard::records::plane_params(brep, &plane_normals)
            .into_iter()
            .map(|plane| (plane.target, plane))
            .collect();
    let face_bounds = records
        .iter()
        .map(|record| crate::families::standard::records::standard_face_bounds(brep, record))
        .collect::<Vec<_>>();
    let mut freeform_geometries = object_evidence.surface_geometries.clone();
    let e5_freeform_geometries = associate_standard_freeform_e5_surfaces(&records, &scan.data);
    let mut e5_freeform_tags = HashSet::new();
    for (tag, geometry) in e5_freeform_geometries {
        freeform_geometries.insert(tag, geometry);
        e5_freeform_tags.insert(tag);
    }
    let mut freeform_procedural_surfaces = object_evidence.procedural_surfaces.clone();
    let e5_freeform_procedural_surfaces =
        associate_standard_freeform_e5_rolling_ball_jets(&records, &scan.data);
    for (tag, procedure) in e5_freeform_procedural_surfaces {
        match freeform_procedural_surfaces.get(&tag) {
            Some(existing) if existing != &procedure => {
                freeform_procedural_surfaces.remove(&tag);
            }
            Some(_) => {}
            None => {
                freeform_procedural_surfaces.insert(tag, procedure);
            }
        }
    }
    let unresolved_freeform_record_count = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. }
                    if !freeform_geometries.contains_key(tag)
                        && !freeform_procedural_surfaces.contains_key(tag)
            )
        })
        .count();
    let mut surfaces = Vec::new();
    let mut surface_annotations = Vec::new();
    let mut face_bindings = Vec::new();
    let mut procedural_surface_plans = Vec::new();
    let mut decoded_plane_targets = HashSet::new();
    let mut plane_faces = 0usize;
    let mut typed = TypedCounts::default();
    for (i, record) in records.iter().enumerate() {
        let crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) = record
        else {
            let crate::families::standard::records::StandardSurfaceRecord::Freeform {
                pos,
                tag,
                forward,
                ..
            } = record
            else {
                unreachable!()
            };
            let id = SurfaceId(format!("catia:standard:surf#{i}"));
            let geometry = freeform_geometries
                .get(tag)
                .cloned()
                .unwrap_or(SurfaceGeometry::Unknown { record: None });
            face_bindings.push((id.clone(), *forward, *pos));
            surface_annotations.push((
                id.clone(),
                "MainDataStream+SurfacicReps",
                *pos,
                "surfacic_reps_freeform_alias".to_string(),
                if freeform_procedural_surfaces.contains_key(tag) || e5_freeform_tags.contains(tag)
                {
                    Exactness::ByteExact
                } else if matches!(geometry, SurfaceGeometry::Unknown { .. }) {
                    Exactness::Unknown
                } else {
                    Exactness::ByteExact
                },
            ));
            surfaces.push(Surface {
                id: id.clone(),
                geometry,
                source_object: Some(cgm_source("carrier", *tag)),
            });
            if let Some(procedure) = freeform_procedural_surfaces.get(tag).cloned() {
                procedural_surface_plans.push((i, id, *tag, procedure));
            }
            continue;
        };
        // A bridged plane parameter record contains the same `00 33 32`
        // marker as its SurfacicReps carrier.  One carrier exists per tag.
        if prefix.kind == 0x32 && !decoded_plane_targets.insert(prefix.target) {
            continue;
        }
        let decoded = if prefix.kind == 0x32 {
            planes
                .get(&prefix.target)
                .and_then(crate::families::standard::records::decode_plane)
        } else {
            curved_surfaces[i].clone()
        };
        match decoded {
            Some(geom) => {
                typed.record(&geom);
                let id = SurfaceId(format!("catia:standard:surf#{i}"));
                if let Some(forward) = crate::families::standard::records::face_sense(brep, prefix)
                {
                    face_bindings.push((id.clone(), forward, prefix.pos));
                }
                let (annotation_stream, annotation_offset, annotation_tag) =
                    refined_analytic_surfaces.get(&i).map_or(
                        (
                            "MainDataStream+SurfacicReps",
                            prefix.pos,
                            format!("surfacic_reps_{:02x}", prefix.kind),
                        ),
                        |source_pos| {
                            (
                                "consolidated_b2_03",
                                *source_pos,
                                "consolidated_exact_analytic_surface".to_string(),
                            )
                        },
                    );
                surface_annotations.push((
                    id.clone(),
                    annotation_stream,
                    annotation_offset,
                    annotation_tag,
                    Exactness::ByteExact,
                ));
                surfaces.push(Surface {
                    id,
                    geometry: geom,
                    source_object: Some(cgm_source("carrier", prefix.target)),
                });
            }
            None => {
                if prefix.kind == 0x32 {
                    plane_faces += 1;
                }
                let id = SurfaceId(format!("catia:standard:surf#{i}"));
                if let Some(forward) = crate::families::standard::records::face_sense(brep, prefix)
                {
                    face_bindings.push((id.clone(), forward, prefix.pos));
                }
                surface_annotations.push((
                    id.clone(),
                    "MainDataStream+SurfacicReps",
                    prefix.pos,
                    format!("surfacic_reps_{:02x}", prefix.kind),
                    Exactness::Unknown,
                ));
                surfaces.push(Surface {
                    id,
                    geometry: SurfaceGeometry::Unknown {
                        record: Some(UnknownId("catia:payload:unknown#brep-stream".to_string())),
                    },
                    source_object: Some(cgm_source("carrier", prefix.target)),
                });
            }
        }
    }

    if points.is_empty() && surfaces.is_empty() {
        return None;
    }

    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut unknowns,
        &mut annotations,
        scan,
        "catia:payload:unknown#brep-stream",
    );
    let mut procedural_supports = HashMap::<u32, SurfaceId>::new();
    let mut extrusion_definitions = HashMap::<u32, ProceduralSurfaceDefinition>::new();
    for (index, surface, tag, procedure) in procedural_surface_plans {
        let procedural_id = ProceduralSurfaceId(format!("catia:standard:procedural-surf#{index}"));
        let record_bounds = match &procedure {
            StandardSurfaceProcedure::Extrusion(extrusion) => {
                Some(parameter_record_bounds(extrusion.parameter_bounds))
            }
            StandardSurfaceProcedure::Offset {
                parameter_bounds, ..
            } => Some(parameter_record_bounds(*parameter_bounds)),
            StandardSurfaceProcedure::RollingBall { .. }
            | StandardSurfaceProcedure::Revolution(_) => None,
        };
        let (source, carrier, definition, exactness) = match procedure {
            StandardSurfaceProcedure::RollingBall {
                carrier_object_id,
                definition,
                source: procedure_source,
            } => (
                match procedure_source {
                    StandardRollingBallSource::ObjectStreamA8 => "object_stream_a8_03_32",
                    StandardRollingBallSource::E5D8 => "e5_0d_03_d8",
                },
                carrier_object_id,
                definition,
                Exactness::ByteExact,
            ),
            StandardSurfaceProcedure::Offset {
                carrier_object_id,
                support_object_id,
                support,
                distance,
                parameter_bounds: _,
            } => {
                let support_id = match support {
                    crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(support) => {
                        procedural_supports
                            .entry(support_object_id)
                            .or_insert_with(|| {
                                let id = SurfaceId(format!(
                                    "catia:standard:procedural-support#{support_object_id}"
                                ));
                                annotate(
                                    &mut annotations,
                                    &id,
                                    "object_stream_b5_03",
                                    0,
                                    format!("surface:{support_object_id:08x}"),
                                    Exactness::ByteExact,
                                );
                                surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry: support,
                                    source_object: Some(cgm_source("surface", support_object_id)),
                                });
                                id
                            })
                            .clone()
                    }
                    crate::families::b5::transfer::ResolvedOffsetSupport::Extrusion(extrusion) => {
                        let record_bounds = parameter_record_bounds(extrusion.parameter_bounds);
                        let support_id = SurfaceId(format!(
                            "catia:standard:procedural-support#{support_object_id}"
                        ));
                        annotate(
                            &mut annotations,
                            &support_id,
                            "object_stream_b5_03_2c",
                            0,
                            format!("surface:{support_object_id:08x}"),
                            Exactness::ByteExact,
                        );
                        surfaces.push(Surface {
                            id: support_id.clone(),
                            geometry: SurfaceGeometry::Unknown { record: None },
                            source_object: Some(cgm_source("surface", support_object_id)),
                        });
                        let definition = emit_standard_extrusion_definition(
                            &mut ir,
                            &mut annotations,
                            &mut surfaces,
                            &mut procedural_supports,
                            &mut extrusion_definitions,
                            *extrusion,
                        );
                        ir.model.procedural_surfaces.push(ProceduralSurface {
                            id: ProceduralSurfaceId(format!(
                                "catia:standard:procedural-support-definition#{support_object_id}"
                            )),
                            surface: support_id.clone(),
                            definition,
                            cache_fit_tolerance: None,
                            record_bounds: Some(record_bounds),
                        });
                        procedural_supports.insert(support_object_id, support_id.clone());
                        support_id
                    }
                };
                (
                    "object_stream_b5_03_30",
                    carrier_object_id,
                    ProceduralSurfaceDefinition::Offset {
                        support: support_id,
                        distance,
                        u_sense: None,
                        v_sense: None,
                        extension_flags: Vec::new(),
                        revision_form: None,
                    },
                    Exactness::Derived,
                )
            }
            StandardSurfaceProcedure::Extrusion(extrusion) => {
                let carrier = extrusion.surface_object_id;
                let definition = emit_standard_extrusion_definition(
                    &mut ir,
                    &mut annotations,
                    &mut surfaces,
                    &mut procedural_supports,
                    &mut extrusion_definitions,
                    *extrusion,
                );
                (
                    "object_stream_b5_03_2c",
                    carrier,
                    definition,
                    Exactness::ByteExact,
                )
            }
            StandardSurfaceProcedure::Revolution(revolution) => {
                let directrix_id = CurveId(format!("catia:standard:revolution-profile#{tag}"));
                annotate(
                    &mut annotations,
                    &directrix_id,
                    "object_stream_b5_03_2d",
                    0,
                    "profile_curve",
                    Exactness::Derived,
                );
                annotations.derived(&directrix_id, "geometry");
                ir.model.curves.push(Curve {
                    id: directrix_id.clone(),
                    geometry: CurveGeometry::Nurbs(revolution.directrix.clone()),
                    source_object: None,
                });
                (
                    "object_stream_b5_03_2d",
                    tag,
                    ProceduralSurfaceDefinition::Revolution {
                        directrix: directrix_id,
                        axis_origin: revolution.axis_origin,
                        axis_direction: revolution.axis_direction,
                        angular_interval: revolution.angular_interval,
                        angular_parameter_interval: Some(revolution.angular_parameter_interval),
                        parameter_interval: Some(revolution.parameter_interval),
                        transposed: false,
                        revision_form: None,
                    },
                    Exactness::Derived,
                )
            }
        };
        if matches!(
            &definition,
            ProceduralSurfaceDefinition::RollingBallJet { .. }
        ) {
            if let Some(surface_record) = surfaces
                .iter_mut()
                .find(|candidate| candidate.id == surface)
            {
                surface_record.geometry = SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                };
            }
        }
        annotate(
            &mut annotations,
            &procedural_id,
            source,
            0,
            format!("face_object_id:{tag:08x}:result_carrier:{carrier:08x}"),
            exactness,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface,
            definition,
            cache_fit_tolerance: None,
            record_bounds,
        });
    }
    ir.model.surfaces = surfaces;
    let resolved_consolidated_revolutions =
        crate::families::b2::records::b2_resolved_revolutions_from_records(
            &scan.data,
            &consolidated_records,
        );
    let resolved_revolution_count = resolved_consolidated_revolutions.len();
    let consolidated_revolutions = append_consolidated_revolutions(
        &mut ir,
        &mut annotations,
        &resolved_consolidated_revolutions,
    );

    for (i, p) in points.iter().enumerate() {
        let point_id = PointId(format!("catia:standard:pt#{i}"));
        annotate(
            &mut annotations,
            &point_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: *p,
            source_object: vertex_roster
                .as_ref()
                .map(|roster| cgm_source("vertex", roster[i])),
        });
        let vertex_id = VertexId(format!("catia:standard:v#{i}"));
        annotate(
            &mut annotations,
            &vertex_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (id, stream, offset, tag, exactness) in surface_annotations {
        annotate(&mut annotations, &id, stream, offset as u64, tag, exactness);
    }
    let mut topology_ir = ir.clone();
    let mut topology_annotations = annotations.clone();
    attach_standard_faces(
        &mut topology_ir,
        &mut topology_annotations,
        &face_bindings,
        standard_spine,
    );
    let mut bound_standard_limit_curve_count = 0;
    let mut topology_diagnostics = StandardTopologyDiagnostics::default();
    let topology_budget = ctx.work_budget(mesh_quotient::MAX_MESH_TOPOLOGY_OPERATIONS as u64);
    let topology_result = attach_standard_topology(
        &mut topology_ir,
        &mut topology_annotations,
        &face_bindings,
        &records,
        &face_bounds,
        standard_spine,
        fbb_only,
        brep,
        selection.map(|selection| selection.supports.as_slice()),
        &scan.data,
        selection.is_none_or(|selection| selection.vertex_roster_compatible),
        &object_evidence.edge_owner_faces,
        &object_evidence.edge_supports,
        &object_evidence.limit_curves,
        &topology_budget,
        &mut topology_diagnostics,
        &mut bound_standard_limit_curve_count,
    )
    .and_then(|()| {
        neutral_model_is_admissible(&mut topology_ir, &unknowns)
            .then_some(())
            .ok_or(StandardTopologyFailure::InadmissibleNeutralModel)
    });
    let topology_failure = topology_result.as_ref().err().copied();
    let topology_attached = topology_failure.is_none();
    if topology_attached {
        ir = topology_ir;
        annotations = topology_annotations;
    } else {
        attach_standard_circles(
            &mut ir,
            &mut annotations,
            &face_bindings,
            brep,
            standard_edge_count,
        );
        attach_standard_lines(
            &mut ir,
            &mut annotations,
            &face_bindings,
            brep,
            standard_edge_count,
        );
        if !ir.model.vertices.is_empty() {
            attach_free_vertices(
                &mut ir,
                &mut annotations,
                "standard",
                "MainDataStream+SurfacicReps",
            );
        }
    }
    let (bound_revolution_face_surface_count, resolved_revolution_seam_curve_count) =
        bind_consolidated_revolution_faces_and_seams(
            &mut ir,
            &mut annotations,
            &consolidated_revolutions,
        );
    let mut consolidated_curve_bindings = append_freeform_surface_pools(
        &mut ir,
        &mut annotations,
        &scan.data,
        &consolidated_records,
        &scan.surface_alias_tags,
    );
    let owner_binding_budget =
        ctx.work_budget(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS as u64);
    consolidated_curve_bindings.standard_face_surfaces += bind_standard_a5_owner_surfaces(
        &mut ir,
        &mut annotations,
        &scan.data,
        &consolidated_records,
        &face_bounds,
        &owner_binding_budget,
    );
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();

    let mut report = build_geometry_report(
        &ir,
        scan,
        &typed,
        plane_faces,
        analytic_record_count,
        &crate::assemble::GeometryReportCounts {
            face_local_freeform: unresolved_freeform_record_count
                .saturating_sub(bound_revolution_face_surface_count)
                .saturating_sub(consolidated_curve_bindings.standard_face_surfaces),
            unbound_revolution: revolution_record_count.saturating_sub(resolved_revolution_count),
            admitted_standard_face_rows: face_count,
        },
        topology_failure.map(StandardTopologyFailure::message),
    );
    report.coverage.insert(
        "attempted_standard_topology_count".to_string(),
        usize::from(true),
    );
    report
        .coverage
        .insert("standard_fbb_run_count".to_string(), scan.census.fbb_runs);
    report.coverage.insert(
        "standard_fbb_candidate_face_row_count".to_string(),
        scan.census.fbb_face_rows,
    );
    report.coverage.insert(
        "standard_fbb_admitted_face_row_count".to_string(),
        face_count,
    );
    report.coverage.insert(
        "standard_fbb_withheld_face_row_count".to_string(),
        scan.census.fbb_face_rows.saturating_sub(face_count),
    );
    report.coverage.insert(
        "attached_standard_topology_count".to_string(),
        usize::from(topology_attached),
    );
    for failure in StandardTopologyFailure::ALL {
        report.coverage.insert(
            failure.coverage_key().to_string(),
            usize::from(topology_failure == Some(failure)),
        );
    }
    report.coverage.insert(
        "standard_topology_curve_support_count".to_string(),
        topology_diagnostics.curve_supports,
    );
    report.coverage.insert(
        "standard_topology_native_endpoint_pair_count".to_string(),
        topology_diagnostics.native_endpoint_pairs,
    );
    report.coverage.insert(
        "standard_topology_empty_endpoint_domain_count".to_string(),
        topology_diagnostics.empty_endpoint_domains,
    );
    report.coverage.insert(
        "standard_topology_singleton_endpoint_domain_count".to_string(),
        topology_diagnostics.singleton_endpoint_domains,
    );
    report.coverage.insert(
        "standard_topology_multiple_endpoint_domain_count".to_string(),
        topology_diagnostics.multiple_endpoint_domains,
    );
    report.coverage.insert(
        "standard_topology_endpoint_domain_choice_count".to_string(),
        topology_diagnostics.endpoint_domain_choices,
    );
    for (key, rejection) in [
        (
            "standard_topology_mesh_rejection_input_structure_count",
            mesh_quotient::MeshCandidateRejection::InputStructure,
        ),
        (
            "standard_topology_mesh_rejection_input_cardinality_count",
            mesh_quotient::MeshCandidateRejection::InputCardinality,
        ),
        (
            "standard_topology_mesh_rejection_face_boundary_cardinality_count",
            mesh_quotient::MeshCandidateRejection::FaceBoundaryCardinality,
        ),
        (
            "standard_topology_mesh_rejection_port_cardinality_count",
            mesh_quotient::MeshCandidateRejection::PortCardinality,
        ),
        (
            "standard_topology_mesh_rejection_quotient_preparation_count",
            mesh_quotient::MeshCandidateRejection::QuotientPreparation,
        ),
        (
            "standard_topology_mesh_rejection_edge_class_constraint_count",
            mesh_quotient::MeshCandidateRejection::EdgeClassConstraint,
        ),
    ] {
        report.coverage.insert(
            key.to_string(),
            usize::from(topology_diagnostics.mesh_rejection == Some(rejection)),
        );
    }
    let endpoint_incidence_rejection =
        topology_diagnostics
            .mesh_rejection
            .and_then(|rejection| match rejection {
                mesh_quotient::MeshCandidateRejection::EndpointIncidence(rejection) => {
                    Some(rejection)
                }
                _ => None,
            });
    report.coverage.insert(
        "standard_topology_mesh_rejection_endpoint_incidence_count".to_string(),
        usize::from(endpoint_incidence_rejection.is_some()),
    );
    report.coverage.insert(
        "standard_topology_mesh_rejection_endpoint_incidence_no_assignment_count".to_string(),
        usize::from(matches!(
            endpoint_incidence_rejection,
            Some(mesh_quotient::MeshEndpointIncidenceRejection::NoAssignment(
                _
            ))
        )),
    );
    report.coverage.insert(
        "standard_topology_mesh_rejection_endpoint_incidence_boundary_reconstruction_count"
            .to_string(),
        usize::from(
            endpoint_incidence_rejection
                == Some(mesh_quotient::MeshEndpointIncidenceRejection::BoundaryReconstruction),
        ),
    );
    let incidence_rejection = endpoint_incidence_rejection.and_then(|rejection| match rejection {
        mesh_quotient::MeshEndpointIncidenceRejection::NoAssignment(rejection) => Some(rejection),
        mesh_quotient::MeshEndpointIncidenceRejection::BoundaryReconstruction => None,
    });
    for (key, rejection) in [
        (
            "standard_topology_mesh_rejection_incidence_input_shape_count",
            crate::solve::incidence::IncidenceRejection::InputShape,
        ),
        (
            "standard_topology_mesh_rejection_incidence_choice_pruning_count",
            crate::solve::incidence::IncidenceRejection::ChoicePruning,
        ),
        (
            "standard_topology_mesh_rejection_incidence_fixed_assignment_count",
            crate::solve::incidence::IncidenceRejection::FixedAssignment,
        ),
        (
            "standard_topology_mesh_rejection_incidence_component_domain_count",
            crate::solve::incidence::IncidenceRejection::ComponentDomain,
        ),
        (
            "standard_topology_mesh_rejection_incidence_component_composition_count",
            crate::solve::incidence::IncidenceRejection::ComponentComposition,
        ),
    ] {
        report.coverage.insert(
            key.to_string(),
            usize::from(incidence_rejection == Some(rejection)),
        );
    }
    for (key, ambiguity) in [
        (
            "standard_topology_mesh_ambiguity_coordinate_root_closure_count",
            mesh_quotient::MeshCandidateAmbiguity::CoordinateRootClosure,
        ),
        (
            "standard_topology_mesh_ambiguity_endpoint_resolution_count",
            mesh_quotient::MeshCandidateAmbiguity::EndpointResolution,
        ),
        (
            "standard_topology_mesh_ambiguity_distinct_topology_solutions_count",
            mesh_quotient::MeshCandidateAmbiguity::DistinctTopologySolutions,
        ),
    ] {
        report.coverage.insert(
            key.to_string(),
            usize::from(topology_diagnostics.mesh_ambiguity == Some(ambiguity)),
        );
    }
    for (key, exhaustion) in [
        (
            "standard_topology_mesh_exhaustion_quotient_preparation_count",
            mesh_quotient::MeshCandidateExhaustion::QuotientPreparation,
        ),
        (
            "standard_topology_mesh_exhaustion_incidence_enumeration_count",
            mesh_quotient::MeshCandidateExhaustion::IncidenceEnumeration,
        ),
        (
            "standard_topology_mesh_exhaustion_endpoint_resolution_count",
            mesh_quotient::MeshCandidateExhaustion::EndpointResolution,
        ),
    ] {
        report.coverage.insert(
            key.to_string(),
            usize::from(topology_diagnostics.mesh_exhaustion == Some(exhaustion)),
        );
    }
    report.coverage.insert(
        "refined_consolidated_analytic_surface_count".to_string(),
        refined_analytic_surfaces.len(),
    );
    report.coverage.insert(
        "decoded_standard_limit_curve_count".to_string(),
        standard_limit_curve_count,
    );
    report.coverage.insert(
        "bound_standard_limit_curve_count".to_string(),
        bound_standard_limit_curve_count,
    );
    report.coverage.insert(
        "bound_consolidated_revolution_face_surface_count".to_string(),
        bound_revolution_face_surface_count,
    );
    report.coverage.insert(
        "resolved_consolidated_revolution_seam_curve_count".to_string(),
        resolved_revolution_seam_curve_count,
    );
    report.coverage.insert(
        "bound_consolidated_standard_edge_count".to_string(),
        consolidated_curve_bindings.standard_edges,
    );
    report.coverage.insert(
        "bound_consolidated_partner_support_count".to_string(),
        consolidated_curve_bindings.partner_supports,
    );
    report.coverage.insert(
        "bound_consolidated_partner_face_pcurve_pair_count".to_string(),
        consolidated_curve_bindings.partner_face_pcurve_pairs,
    );
    report.coverage.insert(
        "bound_consolidated_standard_face_surface_count".to_string(),
        consolidated_curve_bindings.standard_face_surfaces,
    );
    report.coverage.insert(
        "bound_consolidated_standard_face_pcurve_count".to_string(),
        consolidated_curve_bindings.standard_face_pcurves,
    );
    Some(FamilyOutput {
        ir,
        report,
        annotations,
        unknowns,
        standard_face_population: true,
    })
}

#[derive(Default)]
pub(crate) struct StandardObjectEvidence {
    pub(crate) surface_geometries: HashMap<u32, SurfaceGeometry>,
    pub(crate) procedural_surfaces: HashMap<u32, StandardSurfaceProcedure>,
    pub(crate) edge_owner_faces: HashMap<u32, HashSet<u32>>,
    pub(crate) edge_supports: HashMap<u32, StandardEdgeSupport>,
    pub(crate) limit_curves: Vec<NurbsCurve>,
}

#[derive(Default)]
struct StandardTopologyDiagnostics {
    curve_supports: usize,
    native_endpoint_pairs: usize,
    empty_endpoint_domains: usize,
    singleton_endpoint_domains: usize,
    multiple_endpoint_domains: usize,
    endpoint_domain_choices: usize,
    mesh_rejection: Option<mesh_quotient::MeshCandidateRejection>,
    mesh_ambiguity: Option<mesh_quotient::MeshCandidateAmbiguity>,
    mesh_exhaustion: Option<mesh_quotient::MeshCandidateExhaustion>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StandardTopologyFailure {
    NoCurveSupports,
    EdgeFaceAssignment,
    MissingFaceSurface,
    ConflictingNativeEndpoints,
    NativeEndpointPropagation,
    EmptyEndpointDomain,
    NoTopologySolution,
    AmbiguousTopologySolution,
    TopologySearchExhausted,
    InvalidTopologySolution,
    InadmissibleNeutralModel,
}

impl StandardTopologyFailure {
    const ALL: [Self; 11] = [
        Self::NoCurveSupports,
        Self::EdgeFaceAssignment,
        Self::MissingFaceSurface,
        Self::ConflictingNativeEndpoints,
        Self::NativeEndpointPropagation,
        Self::EmptyEndpointDomain,
        Self::NoTopologySolution,
        Self::AmbiguousTopologySolution,
        Self::TopologySearchExhausted,
        Self::InvalidTopologySolution,
        Self::InadmissibleNeutralModel,
    ];

    const fn coverage_key(self) -> &'static str {
        match self {
            Self::NoCurveSupports => "standard_topology_failure_no_curve_supports_count",
            Self::EdgeFaceAssignment => "standard_topology_failure_edge_face_assignment_count",
            Self::MissingFaceSurface => "standard_topology_failure_missing_face_surface_count",
            Self::ConflictingNativeEndpoints => {
                "standard_topology_failure_conflicting_native_endpoints_count"
            }
            Self::NativeEndpointPropagation => {
                "standard_topology_failure_native_endpoint_propagation_count"
            }
            Self::EmptyEndpointDomain => "standard_topology_failure_empty_endpoint_domain_count",
            Self::NoTopologySolution => "standard_topology_failure_no_solution_count",
            Self::AmbiguousTopologySolution => "standard_topology_failure_ambiguous_solution_count",
            Self::TopologySearchExhausted => "standard_topology_failure_search_exhausted_count",
            Self::InvalidTopologySolution => "standard_topology_failure_invalid_solution_count",
            Self::InadmissibleNeutralModel => "standard_topology_failure_inadmissible_model_count",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::NoCurveSupports => "the curve support table is absent or empty",
            Self::EdgeFaceAssignment => "serialized edge owners do not resolve to face pairs",
            Self::MissingFaceSurface => "an edge owner has no decoded face surface",
            Self::ConflictingNativeEndpoints => {
                "native edge endpoint sources assign conflicting vertices"
            }
            Self::NativeEndpointPropagation => {
                "native edge endpoint identities cannot be propagated consistently"
            }
            Self::EmptyEndpointDomain => {
                "surface constraints eliminate every endpoint pair for an edge"
            }
            Self::NoTopologySolution => {
                "trim, port, and endpoint constraints have no complete topology solution"
            }
            Self::AmbiguousTopologySolution => {
                "trim, port, and endpoint constraints admit distinct complete topology solutions"
            }
            Self::TopologySearchExhausted => {
                "the bounded topology search exhausted its operation or solution budget"
            }
            Self::InvalidTopologySolution => {
                "the solved topology violates model cardinality or incidence invariants"
            }
            Self::InadmissibleNeutralModel => {
                "the emitted neutral model does not satisfy codec admissibility invariants"
            }
        }
    }
}

fn retry_rejected_mesh_solution(
    preferred: mesh_quotient::MeshCandidateSolve,
    fallback: impl FnOnce() -> mesh_quotient::MeshCandidateSolve,
) -> mesh_quotient::MeshCandidateSolve {
    match preferred {
        mesh_quotient::MeshCandidateSolve::Rejected(_)
        | mesh_quotient::MeshCandidateSolve::Exhausted(
            mesh_quotient::MeshCandidateExhaustion::PreferredSolutionSearch,
        ) => fallback(),
        outcome => outcome,
    }
}

#[derive(Clone, PartialEq)]
/// Exact two-sided construction evidence keyed by a standard edge identity.
pub(crate) struct StandardEdgeSupport {
    /// Persistent support-surface identities in wrapper order.
    pub(crate) surface_object_ids: [u32; 2],
    /// Exact neutral support carriers.
    pub(crate) carriers: [crate::families::b5::transfer::ResolvedPcurveSurface; 2],
    /// Exact support pcurves in wrapper order.
    pub(crate) pcurves: [PcurveGeometry; 2],
    /// Shared native parameter interval.
    pub(crate) parameter_range: [f64; 2],
}

#[derive(Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum StandardSurfaceProcedure {
    RollingBall {
        carrier_object_id: u32,
        definition: ProceduralSurfaceDefinition,
        source: StandardRollingBallSource,
    },
    Offset {
        carrier_object_id: u32,
        support_object_id: u32,
        support: crate::families::b5::transfer::ResolvedOffsetSupport,
        distance: f64,
        parameter_bounds: [[f64; 2]; 2],
    },
    Extrusion(Box<crate::families::b5::transfer::ResolvedExtrusionSurface>),
    Revolution(Box<crate::families::b5::transfer::ResolvedRevolutionSurface>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StandardRollingBallSource {
    ObjectStreamA8,
    E5D8,
}

#[derive(Clone, PartialEq)]
pub(crate) struct StandardSurfaceEvidence {
    geometry: Option<SurfaceGeometry>,
    procedure: Option<StandardSurfaceProcedure>,
}

impl StandardSurfaceEvidence {
    fn from_parts(
        geometry: Option<SurfaceGeometry>,
        procedure: Option<StandardSurfaceProcedure>,
    ) -> Option<Self> {
        if geometry.is_none() && procedure.is_none() {
            return None;
        }
        Some(Self {
            geometry,
            procedure,
        })
    }

    fn geometry(geometry: SurfaceGeometry) -> Self {
        Self {
            geometry: Some(geometry),
            procedure: None,
        }
    }

    fn procedure(procedure: StandardSurfaceProcedure) -> Self {
        Self {
            geometry: None,
            procedure: Some(procedure),
        }
    }
}

pub(crate) fn standard_object_evidence(
    scan: &ContainerScan,
    tags: &HashSet<u32>,
    edge_tags: &HashSet<u32>,
    consolidated_records: &[ConsolidatedRecord],
) -> StandardObjectEvidence {
    let mut evidence = standard_object_evidence_from_streams(
        container::logical_record_streams(scan),
        tags,
        edge_tags,
    );
    merge_standard_limit_curves_from_records(
        &mut evidence.limit_curves,
        &scan.data,
        consolidated_records,
    );
    evidence
}

fn merge_standard_limit_curves_from_records(
    curves: &mut Vec<NurbsCurve>,
    data: &[u8],
    records: &[ConsolidatedRecord],
) {
    for jet in crate::families::a5a8::records::a5_freeform_curves_from_records(data, records) {
        for second_limit in [false, true] {
            let Some(geometry) =
                crate::families::a5a8::records::rolling_ball_limit_curve(&jet, second_limit)
            else {
                continue;
            };
            if !curves.contains(&geometry) {
                curves.push(geometry);
            }
        }
    }
}

pub(crate) fn standard_object_evidence_from_streams(
    streams: impl IntoIterator<Item = Vec<u8>>,
    tags: &HashSet<u32>,
    edge_tags: &HashSet<u32>,
) -> StandardObjectEvidence {
    let mut surface_candidates = HashMap::<u32, Option<StandardSurfaceEvidence>>::new();
    let mut support_candidates =
        HashMap::<u32, Option<crate::families::b5::transfer::ResolvedOffsetSupport>>::new();
    let mut edge_face_candidates = HashMap::<u32, Option<HashSet<u32>>>::new();
    let mut edge_support_candidates = HashMap::<u32, Option<StandardEdgeSupport>>::new();
    let mut limit_curves = Vec::<NurbsCurve>::new();
    let streams = streams.into_iter().collect::<Vec<_>>();
    for stream in &streams {
        let records = crate::wire::records::consolidated_records(stream);
        merge_standard_limit_curves_from_records(&mut limit_curves, stream, &records);
    }
    let populations = streams
        .iter()
        .flat_map(|stream| crate::families::b5::graph::object_stream_populations(stream))
        .collect::<Vec<_>>();
    let mut population_objects = HashMap::<u32, Option<Vec<u8>>>::new();
    let mut seen_population_ids = HashSet::new();
    let mut repeated_population_ids = HashSet::new();
    for population in &populations {
        let mut objects = HashMap::<u32, Option<Vec<u8>>>::new();
        for frame in crate::families::b5::graph::object_stream_frames(population) {
            let bytes = population[frame.start..frame.end].to_vec();
            objects
                .entry(frame.object_id)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| *stored != bytes) {
                        *stored = None;
                    }
                })
                .or_insert(Some(bytes));
        }
        for (object_id, bytes) in objects {
            if !seen_population_ids.insert(object_id) {
                repeated_population_ids.insert(object_id);
            }
            population_objects
                .entry(object_id)
                .and_modify(|stored| {
                    if stored
                        .as_ref()
                        .zip(bytes.as_ref())
                        .is_none_or(|(stored, incoming)| stored != incoming)
                    {
                        *stored = None;
                    }
                })
                .or_insert(bytes);
        }
    }
    let conflicting_population_ids = population_objects
        .into_iter()
        .filter_map(|(object_id, bytes)| bytes.is_none().then_some(object_id))
        .collect::<HashSet<_>>();
    for stream in populations {
        let frames = crate::families::b5::graph::object_stream_frames(&stream);
        let face_surfaces =
            crate::families::b5::graph::face_surface_references_from_frames(&stream, &frames);
        let surface_bindings = tags
            .iter()
            .map(|&tag| (tag, tag))
            .chain(
                face_surfaces
                    .iter()
                    .filter(|(face_id, _)| tags.contains(face_id))
                    .copied(),
            )
            .collect::<Vec<_>>();
        let requested_surfaces = surface_bindings
            .iter()
            .map(|(_, surface_id)| *surface_id)
            .collect::<HashSet<_>>();
        let targeted_surfaces = crate::families::b5::graph::targeted_surfaces_from_frames(
            &stream,
            &requested_surfaces,
            &frames,
        );
        let targeted_graph =
            crate::families::b5::graph::targeted_geometry_graph_from_frames(&stream, &frames);
        for &(object_id, surface_id) in &surface_bindings {
            let Some(surface) = targeted_surfaces.get(&surface_id) else {
                continue;
            };
            let evidence = targeted_graph
                .as_ref()
                .and_then(|graph| standard_surface_evidence(graph, surface_id))
                .or_else(|| {
                    targeted_graph
                        .as_ref()
                        .and_then(|graph| {
                            crate::families::b5::transfer::resolved_surface_carrier_in_graph(
                                graph, surface_id,
                            )
                        })
                        .or_else(|| {
                            crate::families::b5::transfer::resolved_surface_carrier(surface)
                        })
                        .map(|carrier| match carrier {
                            crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                                geometry,
                            ) => StandardSurfaceEvidence::geometry(geometry),
                            crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall {
                                carrier_object_id,
                                definition,
                            } => StandardSurfaceEvidence::procedure(
                                StandardSurfaceProcedure::RollingBall {
                                    carrier_object_id,
                                    definition: *definition,
                                    source: StandardRollingBallSource::ObjectStreamA8,
                                },
                            ),
                        })
                });
            let Some(evidence) = evidence else {
                continue;
            };
            merge_standard_procedure_supports(&mut support_candidates, &evidence);
            merge_standard_surface_evidence(&mut surface_candidates, object_id, evidence);
        }
        if let Some(graph) = targeted_graph.as_ref() {
            for &(object_id, surface_id) in &surface_bindings {
                if surface_candidates.contains_key(&object_id) {
                    continue;
                }
                let Some(evidence) = standard_surface_evidence(graph, surface_id) else {
                    continue;
                };
                merge_standard_procedure_supports(&mut support_candidates, &evidence);
                merge_standard_surface_evidence(&mut surface_candidates, object_id, evidence);
            }
        }
        let edge_pcurves = crate::families::b5::graph::edge_support_pcurve_references_from_frames(
            &stream, edge_tags, &frames,
        );
        let requested_pcurves = edge_pcurves
            .values()
            .flatten()
            .copied()
            .collect::<HashSet<_>>();
        let mut pcurves = HashMap::<u32, Option<crate::families::a5a8::records::A8Pcurve>>::new();
        for pcurve in crate::families::a5a8::records::object_stream_pcurves(&stream)
            .into_iter()
            .filter(|pcurve| requested_pcurves.contains(&pcurve.object_id))
        {
            pcurves
                .entry(pcurve.object_id)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| {
                        stored.support_id != pcurve.support_id
                            || stored.degree != pcurve.degree
                            || stored.knots != pcurve.knots
                            || stored.points != pcurve.points
                            || stored.first_derivatives != pcurve.first_derivatives
                            || stored.second_derivatives != pcurve.second_derivatives
                            || stored.range != pcurve.range
                    }) {
                        *stored = None;
                    }
                })
                .or_insert(Some(pcurve));
        }
        let surface_ids = pcurves
            .values()
            .filter_map(Option::as_ref)
            .map(|pcurve| pcurve.support_id)
            .collect::<HashSet<_>>();
        let targeted_surfaces = crate::families::b5::graph::targeted_surfaces_from_frames(
            &stream,
            &surface_ids,
            &frames,
        );
        for (edge, references) in edge_pcurves {
            let sides = references.map(|reference| {
                let pcurve = pcurves.get(&reference)?.as_ref()?;
                crate::families::b5::transfer::resolved_object_stream_pcurve(
                    pcurve,
                    targeted_surfaces.get(&pcurve.support_id)?,
                    targeted_graph.as_ref(),
                )
            });
            let [Some(first), Some(second)] = sides else {
                continue;
            };
            if first.parameter_range != second.parameter_range {
                continue;
            }
            let evidence = StandardEdgeSupport {
                surface_object_ids: [first.surface_object_id, second.surface_object_id],
                carriers: [first.carrier, second.carrier],
                pcurves: [first.geometry, second.geometry],
                parameter_range: first.parameter_range,
            };
            edge_support_candidates
                .entry(edge)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| stored != &evidence) {
                        *stored = None;
                    }
                })
                .or_insert(Some(evidence));
        }
        let stream_edge_faces =
            crate::families::b5::graph::edge_face_references_from_frames(&stream, &frames);
        for (edge, owners) in stream_edge_faces {
            edge_face_candidates
                .entry(edge)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| *stored != owners) {
                        *stored = None;
                    }
                })
                .or_insert(Some(owners));
        }
        let Some(graph) = crate::families::b5::graph::parse_from_frames(&stream, &frames) else {
            continue;
        };
        for &surface_id in tags {
            let Some(evidence) = standard_surface_evidence(&graph, surface_id) else {
                continue;
            };
            merge_standard_procedure_supports(&mut support_candidates, &evidence);
            merge_standard_surface_evidence(&mut surface_candidates, surface_id, evidence);
        }
        for &(face_id, surface_id) in face_surfaces
            .iter()
            .filter(|(face_id, _)| tags.contains(face_id))
        {
            let evidence = standard_surface_evidence(&graph, surface_id);
            let Some(evidence) = evidence else { continue };
            merge_standard_procedure_supports(&mut support_candidates, &evidence);
            merge_standard_surface_evidence(&mut surface_candidates, face_id, evidence);
        }
    }
    surface_candidates.retain(|object_id, _| !conflicting_population_ids.contains(object_id));
    support_candidates.retain(|object_id, _| !conflicting_population_ids.contains(object_id));
    edge_face_candidates.retain(|edge, owners| {
        !repeated_population_ids.contains(edge)
            && owners
                .as_ref()
                .is_none_or(|owners| owners.is_disjoint(&repeated_population_ids))
    });
    edge_support_candidates.retain(|edge, support| {
        !repeated_population_ids.contains(edge)
            && support.as_ref().is_none_or(|support| {
                support
                    .surface_object_ids
                    .iter()
                    .all(|surface| !repeated_population_ids.contains(surface))
            })
    });
    StandardObjectEvidence {
        surface_geometries: surface_candidates
            .iter()
            .filter_map(|(&tag, evidence)| Some((tag, evidence.as_ref()?.geometry.clone()?)))
            .collect(),
        procedural_surfaces: surface_candidates
            .into_iter()
            .filter_map(|(tag, evidence)| {
                let procedure = evidence?.procedure?;
                let valid = match &procedure {
                    StandardSurfaceProcedure::Offset {
                        support_object_id,
                        support,
                        ..
                    } => {
                        support_candidates
                            .get(support_object_id)
                            .and_then(Option::as_ref)
                            == Some(support)
                    }
                    StandardSurfaceProcedure::RollingBall { .. } => true,
                    StandardSurfaceProcedure::Extrusion(extrusion) => {
                        extrusion.supports().into_iter().all(|side| {
                            support_candidates
                                .get(&side.surface_object_id)
                                .and_then(Option::as_ref)
                                == Some(
                                    &crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(
                                        side.surface.clone(),
                                    ),
                                )
                        })
                    }
                    StandardSurfaceProcedure::Revolution(_) => true,
                };
                valid.then_some((tag, procedure))
            })
            .collect(),
        edge_owner_faces: edge_face_candidates
            .into_iter()
            .filter_map(|(edge, owners)| Some((edge, owners?)))
            .collect(),
        edge_supports: edge_support_candidates
            .into_iter()
            .filter_map(|(edge, support)| Some((edge, support?)))
            .collect(),
        limit_curves,
    }
}

fn standard_surface_evidence(
    graph: &crate::families::b5::graph::B5Graph,
    surface_id: u32,
) -> Option<StandardSurfaceEvidence> {
    let geometry = crate::families::b5::transfer::resolved_surface_geometry(graph, surface_id);
    let procedure = crate::families::b5::transfer::resolved_offset_surface(graph, surface_id)
        .map(|offset| StandardSurfaceProcedure::Offset {
            carrier_object_id: offset.carrier_object_id,
            support_object_id: offset.support_object_id,
            support: offset.support,
            distance: offset.distance,
            parameter_bounds: offset.parameter_bounds,
        })
        .or_else(|| {
            crate::families::b5::transfer::resolved_extrusion_surface(graph, surface_id)
                .map(Box::new)
                .map(StandardSurfaceProcedure::Extrusion)
        })
        .or_else(|| {
            crate::families::b5::transfer::resolved_surface_procedural_definition(graph, surface_id)
                .map(
                    |(carrier_object_id, definition)| StandardSurfaceProcedure::RollingBall {
                        carrier_object_id,
                        definition,
                        source: StandardRollingBallSource::ObjectStreamA8,
                    },
                )
        })
        .or_else(|| {
            crate::families::b5::transfer::resolved_revolution_surface(graph, surface_id)
                .map(Box::new)
                .map(StandardSurfaceProcedure::Revolution)
        });
    StandardSurfaceEvidence::from_parts(geometry, procedure)
}

fn merge_standard_surface_evidence(
    candidates: &mut HashMap<u32, Option<StandardSurfaceEvidence>>,
    tag: u32,
    evidence: StandardSurfaceEvidence,
) {
    let incoming = evidence.clone();
    candidates
        .entry(tag)
        .and_modify(|stored| {
            let Some(stored_evidence) = stored.take() else {
                return;
            };
            let (stored_geometry, stored_procedure) =
                (stored_evidence.geometry, stored_evidence.procedure);
            let (incoming_geometry, incoming_procedure) =
                (incoming.geometry.clone(), incoming.procedure.clone());
            let EvidencePart::Merged(geometry) =
                merge_standard_evidence_part(stored_geometry, incoming_geometry)
            else {
                return;
            };
            let EvidencePart::Merged(procedure) =
                merge_standard_evidence_part(stored_procedure, incoming_procedure)
            else {
                return;
            };
            *stored = Some(StandardSurfaceEvidence {
                geometry,
                procedure,
            });
        })
        .or_insert(Some(evidence));
}

enum EvidencePart<T> {
    Conflict,
    Merged(Option<T>),
}

fn merge_standard_evidence_part<T: PartialEq>(
    stored: Option<T>,
    incoming: Option<T>,
) -> EvidencePart<T> {
    match (stored, incoming) {
        (Some(stored), Some(incoming)) if stored != incoming => EvidencePart::Conflict,
        (Some(stored), _) => EvidencePart::Merged(Some(stored)),
        (None, incoming) => EvidencePart::Merged(incoming),
    }
}

fn merge_standard_procedure_supports(
    candidates: &mut HashMap<u32, Option<crate::families::b5::transfer::ResolvedOffsetSupport>>,
    evidence: &StandardSurfaceEvidence,
) {
    let Some(procedure) = evidence.procedure.as_ref() else {
        return;
    };
    match procedure {
        StandardSurfaceProcedure::Offset {
            support_object_id,
            support,
            ..
        } => {
            candidates
                .entry(*support_object_id)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| stored != support) {
                        *stored = None;
                    }
                })
                .or_insert_with(|| Some(support.clone()));
        }
        StandardSurfaceProcedure::Extrusion(extrusion) => {
            for side in extrusion.supports() {
                let support = crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(
                    side.surface.clone(),
                );
                candidates
                    .entry(side.surface_object_id)
                    .and_modify(|stored| {
                        if stored.as_ref().is_some_and(|stored| stored != &support) {
                            *stored = None;
                        }
                    })
                    .or_insert(Some(support));
            }
        }
        StandardSurfaceProcedure::RollingBall { .. } | StandardSurfaceProcedure::Revolution(_) => {}
    }
}

/// Attach standard analytic carriers to faces only when every FBB face has a
/// decoded carrier and its stored sense byte.
pub(crate) fn attach_standard_faces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) {
    let face_count = fbb::standard_face_count(brep).unwrap_or_default();
    if face_count == 0 || face_count != bindings.len() {
        return;
    }
    let body_id = BodyId("catia:standard:body#0".to_string());
    let region_id = RegionId("catia:standard:region#0-0".to_string());
    let shell_id = ShellId("catia:standard:shell#0-0".to_string());
    let mut face_ids = Vec::with_capacity(face_count);
    for (face_index, (surface, forward, offset)) in bindings.iter().enumerate() {
        let face_id = FaceId(format!("catia:standard:face#{face_index}"));
        annotate(
            annotations,
            &face_id,
            "MainDataStream+SurfacicReps",
            *offset as u64,
            "surfacic_reps_face_sense",
            Exactness::ByteExact,
        );
        for field in ["shell", "surface", "sense"] {
            annotations.derived(&face_id, field);
        }
        face_ids.push(face_id.clone());
        ir.model.faces.push(Face {
            id: face_id,
            shell: shell_id.clone(),
            surface: surface.clone(),
            sense: if *forward {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        });
    }
    annotate(
        annotations,
        &body_id,
        "MainDataStream+SurfacicReps",
        0,
        "standard_body",
        Exactness::Inferred,
    );
    annotations
        .derived(&body_id, "kind")
        .derived(&body_id, "regions");
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    annotate(
        annotations,
        &region_id,
        "MainDataStream+SurfacicReps",
        0,
        "derived_region",
        Exactness::Inferred,
    );
    annotations
        .derived(&region_id, "body")
        .derived(&region_id, "shells");
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
    });
    annotate(
        annotations,
        &shell_id,
        "MainDataStream+SurfacicReps",
        0,
        "derived_shell",
        Exactness::Inferred,
    );
    annotations
        .derived(&shell_id, "region")
        .derived(&shell_id, "faces");
    ir.model.shells.push(Shell {
        id: shell_id,
        region: region_id,
        faces: face_ids,
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
}

pub(crate) fn partition_standard_face_components(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    components: &[Vec<usize>],
) -> bool {
    if components.is_empty()
        || components.iter().any(Vec::is_empty)
        || components.iter().flatten().count() != ir.model.faces.len()
    {
        return false;
    }
    let body_id = BodyId("catia:standard:body#0".to_string());
    let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == body_id) else {
        return false;
    };
    let region_ids: Vec<RegionId> = (0..components.len())
        .map(|component| RegionId(format!("catia:standard:region#0-{component}")))
        .collect();
    body.regions.clone_from(&region_ids);
    annotations.derived(&body_id, "regions");

    for (component, faces) in components.iter().enumerate() {
        let region_id = region_ids[component].clone();
        let shell_id = ShellId(format!("catia:standard:shell#0-{component}"));
        let face_ids: Vec<FaceId> = faces
            .iter()
            .map(|face| FaceId(format!("catia:standard:face#{face}")))
            .collect();
        for &face in faces {
            let Some(face) = ir.model.faces.get_mut(face) else {
                return false;
            };
            face.shell = shell_id.clone();
            annotations.derived(&face.id, "shell");
        }
        if component == 0 {
            let Some(region) = ir
                .model
                .regions
                .iter_mut()
                .find(|region| region.id == region_id)
            else {
                return false;
            };
            region.shells = vec![shell_id.clone()];
            let Some(shell) = ir
                .model
                .shells
                .iter_mut()
                .find(|shell| shell.id == shell_id)
            else {
                return false;
            };
            shell.faces = face_ids;
            continue;
        }
        for (id, tag) in [
            (&region_id.0, "derived_region"),
            (&shell_id.0, "derived_shell"),
        ] {
            annotate(
                annotations,
                id,
                "MainDataStream+SurfacicReps",
                0,
                tag,
                Exactness::Inferred,
            );
        }
        annotations
            .derived(&region_id, "body")
            .derived(&region_id, "shells");
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id.clone()],
        });
        annotations
            .derived(&shell_id, "region")
            .derived(&shell_id, "faces");
        ir.model.shells.push(Shell {
            id: shell_id,
            region: region_id,
            faces: face_ids,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
    }
    true
}

pub(crate) fn apply_standard_native_edge_faces(
    edge_faces: &mut [[usize; 2]],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    native_edge_faces: &HashMap<u32, HashSet<u32>>,
) {
    if edge_faces.len() != supports.len() {
        return;
    }
    let mut face_by_carrier = HashMap::<u32, Option<usize>>::new();
    for (face, record) in records.iter().enumerate() {
        let carrier = match record {
            crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) => {
                prefix.target
            }
            crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } => *tag,
        };
        face_by_carrier
            .entry(carrier)
            .and_modify(|stored| *stored = None)
            .or_insert(Some(face));
    }
    for (faces, support) in edge_faces.iter_mut().zip(supports) {
        if faces[0] != faces[1] {
            continue;
        }
        let Some(owner_ids) = native_edge_faces.get(&support.tag) else {
            continue;
        };
        let candidates = owner_ids
            .iter()
            .filter_map(|owner| face_by_carrier.get(owner).copied().flatten())
            .filter(|face| *face != faces[0])
            .collect::<HashSet<_>>();
        if let Some(&face) = candidates.iter().next().filter(|_| candidates.len() == 1) {
            faces[1] = face;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StandardLimitCurveBinding {
    curve: usize,
    points: [usize; 2],
    parameter_range: [f64; 2],
}

fn split_bezier_half(control: &[Point3]) -> Option<(Vec<Point3>, Vec<Point3>)> {
    let mut levels = vec![control.to_vec()];
    while levels.last()?.len() > 1 {
        levels.push(
            levels
                .last()?
                .windows(2)
                .map(|pair| {
                    Point3::new(
                        0.5 * (pair[0].x + pair[1].x),
                        0.5 * (pair[0].y + pair[1].y),
                        0.5 * (pair[0].z + pair[1].z),
                    )
                })
                .collect(),
        );
    }
    let left = levels.iter().map(|level| level[0]).collect::<Vec<_>>();
    let right = levels
        .iter()
        .rev()
        .map(|level| *level.last().expect("nonempty Bézier level"))
        .collect::<Vec<_>>();
    Some((left, right))
}

fn collect_bezier_point_parameters(
    control: &[Point3],
    range: [f64; 2],
    point: Point3,
    tolerance: f64,
    parameter_resolution: f64,
    parameters: &mut Vec<(f64, f64)>,
) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    struct Node {
        control: Vec<Point3>,
        range: [f64; 2],
        depth: usize,
    }

    let lower_bound = |control: &[Point3]| {
        let bounds = |coordinate: fn(Point3) -> f64| {
            control
                .iter()
                .copied()
                .map(coordinate)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
                    (low.min(value), high.max(value))
                })
        };
        let axis_distance = |value: f64, low: f64, high: f64| {
            if value < low {
                low - value
            } else if value > high {
                value - high
            } else {
                0.0
            }
        };
        let [(x0, x1), (y0, y1), (z0, z1)] = [bounds(|p| p.x), bounds(|p| p.y), bounds(|p| p.z)];
        axis_distance(point.x, x0, x1)
            .hypot(axis_distance(point.y, y0, y1))
            .hypot(axis_distance(point.z, z0, z1))
    };
    let midpoint = |control: &[Point3]| {
        let mut level = control.to_vec();
        while level.len() > 1 {
            level = level
                .windows(2)
                .map(|pair| {
                    Point3::new(
                        0.5 * (pair[0].x + pair[1].x),
                        0.5 * (pair[0].y + pair[1].y),
                        0.5 * (pair[0].z + pair[1].z),
                    )
                })
                .collect();
        }
        level.first().copied()
    };

    let root_lower_bound = lower_bound(control);
    if root_lower_bound > tolerance {
        return;
    }
    let Some(root_midpoint) = midpoint(control) else {
        return;
    };
    let mut best = (
        0.5 * (range[0] + range[1]),
        root_midpoint.distance_squared(point).sqrt(),
    );
    let Some((&first, &last)) = control.first().zip(control.last()) else {
        return;
    };
    for (parameter, position) in [(range[0], first), (range[1], last)] {
        let distance = position.distance_squared(point).sqrt();
        if distance < best.1 {
            best = (parameter, distance);
        }
    }

    let mut nodes = vec![Node {
        control: control.to_vec(),
        range,
        depth: 0,
    }];
    let mut queue = BinaryHeap::from([(Reverse(root_lower_bound.to_bits()), 0usize)]);
    while let Some((Reverse(lower_bits), node_index)) = queue.pop() {
        let lower = f64::from_bits(lower_bits);
        if lower > tolerance || lower > best.1 {
            continue;
        }
        let node = &nodes[node_index];
        if node.depth >= 48 || node.range[1] - node.range[0] <= parameter_resolution {
            let Some(position) = midpoint(&node.control) else {
                continue;
            };
            let candidate = (
                0.5 * (node.range[0] + node.range[1]),
                position.distance_squared(point).sqrt(),
            );
            if candidate.1 < best.1 {
                best = candidate;
            }
            if candidate.1 <= tolerance {
                parameters.push(candidate);
            }
            continue;
        }
        let Some((left, right)) = split_bezier_half(&node.control) else {
            continue;
        };
        let middle = 0.5 * (node.range[0] + node.range[1]);
        let depth = node.depth + 1;
        for (control, range) in [
            (left, [node.range[0], middle]),
            (right, [middle, node.range[1]]),
        ] {
            let lower = lower_bound(&control);
            if lower > tolerance || lower > best.1 {
                continue;
            }
            if let Some(position) = midpoint(&control) {
                let candidate = (
                    0.5 * (range[0] + range[1]),
                    position.distance_squared(point).sqrt(),
                );
                if candidate.1 < best.1 {
                    best = candidate;
                }
            }
            let index = nodes.len();
            nodes.push(Node {
                control,
                range,
                depth,
            });
            queue.push((Reverse(lower.to_bits()), index));
        }
    }
    if best.1 <= tolerance {
        parameters.push(best);
    }
}

fn standard_limit_curve_point_parameter(
    curve: &NurbsCurve,
    point: Point3,
    tolerance: f64,
) -> Option<f64> {
    let span_count = curve.control_points.len().checked_div(6)?;
    if span_count == 0
        || span_count * 6 != curve.control_points.len()
        || curve.knots.len() != (span_count + 1) * 6
        || curve.weights.is_some()
        || curve.degree != 5
    {
        return None;
    }
    let [parameter_start, parameter_end] = cadmpeg_ir::eval::nurbs_curve_parameter_domain(curve)?;
    let parameter_span = parameter_end - parameter_start;
    let control_polygon_length = curve
        .control_points
        .chunks_exact(6)
        .map(|control| {
            control
                .windows(2)
                .map(|pair| pair[0].distance_squared(pair[1]).sqrt())
                .sum::<f64>()
        })
        .sum::<f64>();
    let parameter_tolerance = (4.0 * tolerance * parameter_span
        / control_polygon_length.max(tolerance))
    .max(EPS_PARAM_TOLERANCE_SPAN * parameter_span.max(1.0));
    let parameter_resolution =
        0.05 * parameter_tolerance.min(EPS_PARAM_RESOLUTION_SPAN * parameter_span.max(1.0));
    let mut parameters = Vec::new();
    for span in 0..span_count {
        collect_bezier_point_parameters(
            &curve.control_points[span * 6..(span + 1) * 6],
            [curve.knots[span * 6], curve.knots[(span + 1) * 6]],
            point,
            tolerance,
            parameter_resolution,
            &mut parameters,
        );
    }
    parameters.sort_by(|left, right| left.1.total_cmp(&right.1));
    let &(parameter, _) = parameters.first()?;
    let ambiguous = parameters
        .iter()
        .skip(1)
        .any(|&(other, _)| (other - parameter).abs() > parameter_tolerance);
    (!ambiguous).then_some(parameter)
}

fn standard_limit_curve_bindings(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    curves: &[NurbsCurve],
) -> Vec<Vec<StandardLimitCurveBinding>> {
    const VERTEX_MATCH_TOLERANCE: f64 = 2e-3;

    let curve_points = curves
        .iter()
        .map(|curve| {
            ir.model
                .points
                .iter()
                .enumerate()
                .filter_map(|(point, value)| {
                    standard_limit_curve_point_parameter(
                        curve,
                        value.position,
                        VERTEX_MATCH_TOLERANCE,
                    )
                    .map(|parameter| (point, parameter))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut edge_curves = vec![Vec::<StandardLimitCurveBinding>::new(); supports.len()];
    for (curve, points) in curve_points.iter().enumerate() {
        for (edge, support) in supports.iter().enumerate() {
            if !matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Bspline
            ) {
                continue;
            }
            let candidates = points
                .iter()
                .copied()
                .filter(|(point, _)| {
                    let position = ir.model.points[*point].position;
                    support.faces.iter().all(|face| {
                        face_surface(ir, bindings, surface_indices, *face).is_some_and(|surface| {
                            matches!(surface.geometry, SurfaceGeometry::Unknown { .. })
                                || point_on_surface(position, &surface.geometry)
                        })
                    })
                })
                .collect::<Vec<_>>();
            let Ok([(start, start_parameter), (end, end_parameter)]) =
                <[(usize, f64); 2]>::try_from(candidates)
            else {
                continue;
            };
            let geometry = CurveGeometry::Nurbs(curves[curve].clone());
            let Some(midpoint) =
                cadmpeg_ir::eval::curve_point(&geometry, 0.5 * (start_parameter + end_parameter))
            else {
                continue;
            };
            let mut checked_surface = false;
            let agrees = support.faces.iter().all(|face| {
                let Some(surface) = face_surface(ir, bindings, surface_indices, *face) else {
                    return false;
                };
                if matches!(surface.geometry, SurfaceGeometry::Unknown { .. }) {
                    return true;
                }
                checked_surface = true;
                point_on_surface(midpoint, &surface.geometry)
            });
            if checked_surface && agrees {
                edge_curves[edge].push(StandardLimitCurveBinding {
                    curve,
                    points: [start, end],
                    parameter_range: [start_parameter, end_parameter],
                });
            }
        }
    }
    edge_curves
}

fn resolve_standard_limit_curve_binding(
    bindings: &[StandardLimitCurveBinding],
    points: [usize; 2],
) -> Option<StandardLimitCurveBinding> {
    let matches = bindings
        .iter()
        .filter(|binding| missing_edge::same_unordered_pair(binding.points, points))
        .copied()
        .collect::<Vec<_>>();
    let [mut binding] = <[StandardLimitCurveBinding; 1]>::try_from(matches).ok()?;
    if binding.points != points {
        binding.points.reverse();
        binding.parameter_range.reverse();
    }
    Some(binding)
}

#[allow(clippy::too_many_arguments)]
fn attach_standard_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    face_bounds: &[Option<crate::families::standard::records::StandardFaceBounds>],
    spine: &[u8],
    fbb_only: bool,
    brep: &[u8],
    support_override: Option<&[crate::families::standard::records::StandardCurveSupport]>,
    source: &[u8],
    use_vertex_roster: bool,
    native_edge_faces: &HashMap<u32, HashSet<u32>>,
    native_edge_supports: &HashMap<u32, StandardEdgeSupport>,
    limit_curves: &[NurbsCurve],
    work_budget: &WorkBudget<'_>,
    diagnostics: &mut StandardTopologyDiagnostics,
    bound_limit_curve_count: &mut usize,
) -> Result<(), StandardTopologyFailure> {
    let face_count = ir.model.faces.len();
    let Some(edge_count) = (if fbb_only {
        crate::families::standard::fbb::fbb_only_edge_count(spine)
    } else {
        crate::families::standard::fbb::standard_edge_count(spine)
    })
    .filter(|count| *count > 0) else {
        return Err(StandardTopologyFailure::NoCurveSupports);
    };
    let mut supports = support_override.map_or_else(
        || {
            crate::families::standard::records::standard_curve_supports(
                brep,
                face_count,
                Some(edge_count),
            )
        },
        ToOwned::to_owned,
    );
    if supports.is_empty() {
        return Err(StandardTopologyFailure::NoCurveSupports);
    }
    diagnostics.curve_supports = supports.len();
    let serialized_edge_faces = supports
        .iter()
        .map(|support| support.faces)
        .collect::<Vec<_>>();
    let Some(mut edge_faces) =
        missing_edge::resolve_standard_edge_faces(spine, &serialized_edge_faces)
    else {
        return Err(StandardTopologyFailure::EdgeFaceAssignment);
    };
    let mut deferred_port_edges = alloc_filled(supports.len(), false, "catia_deferred_port_edges")
        .map_err(|_| StandardTopologyFailure::TopologySearchExhausted)?;
    let mut open_face_domains = None;
    let mut endpoint_face_assignments = None;
    apply_standard_native_edge_faces(&mut edge_faces, &supports, records, native_edge_faces);
    for (support, faces) in supports.iter_mut().zip(&edge_faces) {
        support.faces = *faces;
    }
    let surface_indices = ir
        .model
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| (surface.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let face_bounds = (face_bounds.len() == face_count).then_some(face_bounds);
    let face_point_membership =
        standard_face_point_membership(ir, bindings, &surface_indices, face_bounds);
    let limit_curve_bindings =
        standard_limit_curve_bindings(ir, bindings, &surface_indices, &supports, limit_curves);
    let mut ordered_endpoint_pairs =
        alloc_filled(supports.len(), None, "catia_ordered_endpoint_pairs")
            .map_err(|_| StandardTopologyFailure::TopologySearchExhausted)?;
    let point_coordinates = ir
        .model
        .points
        .iter()
        .map(|point| {
            [
                point.position.x as f32,
                point.position.y as f32,
                point.position.z as f32,
            ]
        })
        .collect::<Vec<_>>();
    let visualization_endpoint_pairs = missing_edge::standard_edge_rows(spine).and_then(|rows| {
        missing_edge::visualization_endpoint_pairs(source, &rows, &point_coordinates)
    });
    if let Some(pairs) = &visualization_endpoint_pairs {
        if pairs.len() != ordered_endpoint_pairs.len() {
            return Err(StandardTopologyFailure::ConflictingNativeEndpoints);
        }
    }
    let mut endpoint_candidates = Vec::with_capacity(supports.len());
    let mut incidence_candidates = HashMap::<[usize; 2], Vec<usize>>::new();
    let mut face_incidence_candidates = HashMap::<usize, Vec<usize>>::new();
    for support in &supports {
        let Some(surface0) = face_surface(ir, bindings, &surface_indices, support.faces[0]) else {
            return Err(StandardTopologyFailure::MissingFaceSurface);
        };
        let Some(surface1) = face_surface(ir, bindings, &surface_indices, support.faces[1]) else {
            return Err(StandardTopologyFailure::MissingFaceSurface);
        };
        let candidates = match &support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Circle {
                center,
                radius,
            } => standard_circle_endpoint_candidates(
                &ir.model.points,
                *center,
                *radius,
                Some([
                    (
                        &surface0.geometry,
                        face_bounds
                            .as_ref()
                            .and_then(|bounds| bounds[support.faces[0]]),
                    ),
                    (
                        &surface1.geometry,
                        face_bounds
                            .as_ref()
                            .and_then(|bounds| bounds[support.faces[1]]),
                    ),
                ]),
            ),
            crate::families::standard::records::StandardCurveGeometry::Line
            | crate::families::standard::records::StandardCurveGeometry::Bspline => {
                let mut faces = support.faces;
                faces.sort_unstable();
                for (face, surface) in [
                    (support.faces[0], &surface0.geometry),
                    (support.faces[1], &surface1.geometry),
                ] {
                    face_incidence_candidates.entry(face).or_insert_with(|| {
                        ir.model
                            .points
                            .iter()
                            .enumerate()
                            .filter_map(|(index, point)| {
                                point_on_standard_face(
                                    point.position,
                                    surface,
                                    face_bounds.as_ref().and_then(|bounds| bounds[face]),
                                )
                                .then_some(index)
                            })
                            .collect()
                    });
                }
                incidence_candidates
                    .entry(faces)
                    .or_insert_with(|| {
                        let right = face_incidence_candidates[&faces[1]]
                            .iter()
                            .copied()
                            .collect::<HashSet<_>>();
                        face_incidence_candidates[&faces[0]]
                            .iter()
                            .copied()
                            .filter(|point| right.contains(point))
                            .collect()
                    })
                    .clone()
            }
        };
        endpoint_candidates.push(candidates);
    }
    let edge_classes = standard_curve_edge_classes(&supports);
    let edge_geometry = standard_curve_geometry_gauge_keys(&supports);
    let topology_graph = crate::families::b5::graph::parse(source);
    let mut native_edges = topology_graph
        .as_ref()
        .and_then(crate::families::b5::graph::B5Graph::referenced_edge_vertex_references)
        .unwrap_or_else(|| crate::families::b5::graph::edge_vertex_references(source));
    if let Some(e5_topology) = crate::container::e5_record_stream(source)
        .and_then(|range| crate::families::e5::graph::parse_topology(&source[range]))
    {
        let e5_edges = e5_topology
            .edges
            .into_values()
            .map(|edge| (edge.record_id, [edge.start_vertex, edge.end_vertex]));
        if !merge_standard_edge_vertex_references(&mut native_edges, e5_edges) {
            return Err(StandardTopologyFailure::ConflictingNativeEndpoints);
        }
    }
    let graph_endpoint_pairs = standard_native_graph_endpoint_pairs(
        topology_graph.as_ref(),
        &supports,
        &native_edges,
        &ir.model.points,
    );
    let native_port_options = supports
        .iter()
        .map(|support| native_edges.get(&support.tag).copied())
        .collect::<Vec<_>>();
    let native_ports = native_port_options
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>();
    let vertex_roster = use_vertex_roster
        .then(|| {
            crate::families::standard::records::standard_vertex_roster(
                source,
                ir.model.points.len(),
            )
        })
        .flatten();
    let allocation_endpoint_points = vertex_roster
        .as_ref()
        .map(|roster| standard_successor_endpoint_points(&supports, roster));
    let roster_endpoint_pairs = vertex_roster
        .as_ref()
        .and_then(|roster| standard_serialized_endpoint_pairs(&supports, &native_edges, roster));
    let native_support_ids = native_edge_supports.keys().copied().collect::<HashSet<_>>();
    let native_support_edge_ids = standard_native_support_edge_ids(&supports, &native_support_ids);
    let native_supports_by_row = native_support_edge_ids
        .iter()
        .map(|edge| edge.and_then(|edge| native_edge_supports.get(&edge).cloned()))
        .collect::<Vec<_>>();
    let Ok(native_endpoint_evidence) = merge_native_endpoint_evidence(
        graph_endpoint_pairs.as_deref(),
        roster_endpoint_pairs.as_deref(),
    ) else {
        return Err(StandardTopologyFailure::ConflictingNativeEndpoints);
    };
    diagnostics.native_endpoint_pairs = native_endpoint_evidence
        .as_ref()
        .map_or(0, |pairs| pairs.iter().flatten().count());
    if let Some(pairs) = &native_endpoint_evidence {
        for (edge, pair) in pairs
            .iter()
            .enumerate()
            .filter_map(|(edge, pair)| pair.as_ref().copied().map(|pair| (edge, pair)))
        {
            if !merge_ordered_endpoint_pair(&mut ordered_endpoint_pairs, edge, pair) {
                return Err(StandardTopologyFailure::ConflictingNativeEndpoints);
            }
        }
    }
    if let Some(pairs) = visualization_endpoint_pairs {
        for (edge, pair) in pairs.into_iter().enumerate() {
            if !merge_derived_endpoint_pair(&mut ordered_endpoint_pairs, edge, pair) {
                return Err(StandardTopologyFailure::ConflictingNativeEndpoints);
            }
        }
    }
    for (edge, bindings) in limit_curve_bindings.iter().enumerate() {
        let Ok([binding]) = <[StandardLimitCurveBinding; 1]>::try_from(bindings.as_slice()) else {
            continue;
        };
        if !merge_derived_endpoint_pair(&mut ordered_endpoint_pairs, edge, binding.points) {
            return Err(StandardTopologyFailure::ConflictingNativeEndpoints);
        }
    }
    if let Some(pairs) = &native_endpoint_evidence {
        include_native_endpoint_pairs(&mut endpoint_candidates, pairs);
    }
    let mut endpoint_options = resolve_standard_endpoint_pairs(
        ir,
        bindings,
        &surface_indices,
        &supports,
        &endpoint_candidates,
    );
    if let Some(options) = &mut endpoint_options {
        for (edge, bindings) in limit_curve_bindings.iter().enumerate() {
            if bindings.is_empty() {
                continue;
            }
            let mut limit_pairs = bindings
                .iter()
                .map(|binding| {
                    let mut points = binding.points;
                    points.sort_unstable();
                    points
                })
                .collect::<Vec<_>>();
            limit_pairs.sort_unstable();
            limit_pairs.dedup();
            if options[edge].is_empty() {
                options[edge] = limit_pairs;
            }
        }
    }
    for edge in 0..supports.len() {
        let native_pair = native_supports_by_row
            .get(edge)
            .and_then(Option::as_ref)
            .and_then(|native| {
                standard_native_support_endpoint_pair(
                    native,
                    &ir.model.points,
                    &endpoint_candidates[edge],
                    native_endpoint_evidence
                        .as_ref()
                        .and_then(|pairs| pairs[edge]),
                )
            });
        let Some(pair) = native_pair else { continue };
        if !merge_derived_endpoint_pair(&mut ordered_endpoint_pairs, edge, pair) {
            return Err(StandardTopologyFailure::ConflictingNativeEndpoints);
        }
        if let Some(options) = &mut endpoint_options {
            if options[edge]
                .iter()
                .any(|candidate| missing_edge::same_unordered_pair(*candidate, pair))
            {
                options[edge] = vec![pair];
            }
        }
    }
    if let (Some(options), Some(pairs)) = (&mut endpoint_options, &native_endpoint_evidence) {
        for (options, pair) in options.iter_mut().zip(pairs) {
            if let Some(pair) = pair {
                *options = vec![*pair];
            }
        }
    }
    if let (Some(options), Some(points)) = (&mut endpoint_options, &allocation_endpoint_points) {
        corroborate_successor_endpoint_points(options, points);
    }
    let graph_propagated_endpoint_pairs = match native_endpoint_evidence.as_ref() {
        Some(pairs) => {
            let Some(propagated) =
                missing_edge::propagate_partial_edge_port_points_with_ordered_seeds(
                    &native_port_options,
                    pairs,
                    &ordered_endpoint_pairs,
                )
            else {
                return Err(StandardTopologyFailure::NativeEndpointPropagation);
            };
            Some(propagated)
        }
        None => None,
    };
    if let (Some(options), Some(pairs)) = (&mut endpoint_options, &graph_propagated_endpoint_pairs)
    {
        for (options, pair) in options.iter_mut().zip(pairs) {
            if let Some(pair) = pair {
                *options = vec![*pair];
            }
        }
    }
    if let Some(pairs) = &graph_propagated_endpoint_pairs {
        include_native_endpoint_pairs(&mut endpoint_candidates, pairs);
    }
    if let Some(options) = &mut endpoint_options {
        let handle_face_candidates = missing_edge::standard_repeated_edge_face_handle_candidates(
            spine,
            &serialized_edge_faces,
        );
        let mut allowed_faces = supports
            .iter()
            .enumerate()
            .map(|(edge, support)| {
                if support.faces[0] != support.faces[1] {
                    return Vec::new();
                }
                (0..face_count)
                    .filter(|face| *face != support.faces[0])
                    .filter(|face| {
                        let Some(surface) = face_surface(ir, bindings, &surface_indices, *face)
                        else {
                            return false;
                        };
                        options[edge].iter().any(|pair| {
                            pair.iter().all(|point| {
                                ir.model.points.get(*point).is_some_and(|point| {
                                    point_on_standard_face(
                                        point.position,
                                        &surface.geometry,
                                        face_bounds.as_ref().and_then(|bounds| bounds[*face]),
                                    )
                                })
                            }) && standard_nurbs_line_pair_on_face(
                                &surface.geometry,
                                support,
                                pair,
                                &ir.model.points,
                                face_bounds.as_ref().and_then(|bounds| bounds[*face]),
                            )
                        })
                    })
                    .collect()
            })
            .collect::<Vec<_>>();
        let face_geometries = (0..face_count)
            .map(|face| {
                face_surface(ir, bindings, &surface_indices, face)
                    .map(|surface| surface.geometry.clone())
            })
            .collect::<Option<Vec<_>>>();
        let edge_geometries = supports
            .iter()
            .map(|support| support.geometry.clone())
            .collect::<Vec<_>>();
        if let Some(handle_face_candidates) = handle_face_candidates {
            missing_edge::refine_repeated_edge_face_candidates(
                &edge_faces,
                &mut allowed_faces,
                &handle_face_candidates,
            )
            .ok_or(StandardTopologyFailure::EdgeFaceAssignment)?;
        }
        refine_repeated_face_domains_by_geometry_and_bounds(
            &edge_faces,
            &mut allowed_faces,
            face_bounds,
            face_geometries.as_deref(),
            &edge_geometries,
        );
        let has_alternates = allowed_faces.iter().any(|faces| !faces.is_empty());
        let endpoint_closures = has_alternates
            .then(|| {
                options
                    .iter()
                    .map(|pairs| {
                        <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                            .ok()
                            .map(|[pair]| pair)
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .flatten()
            .and_then(|pairs| {
                missing_edge::repeated_face_endpoint_closures(
                    &edge_faces,
                    &allowed_faces,
                    &pairs,
                    face_count,
                )
            });
        let endpoint_completed = endpoint_closures
            .as_deref()
            .and_then(|closures| match closures {
                [closure] => Some(closure.clone()),
                _ => None,
            });
        if endpoint_closures
            .as_ref()
            .is_some_and(|closures| closures.len() > 1)
        {
            endpoint_face_assignments = endpoint_closures;
        }
        // A non-empty domain remains open when endpoint degree closure does
        // not select one complete incidence assignment. Face-local endpoint
        // evidence cannot choose among multiple globally closed assignments.
        let completed = endpoint_completed.or_else(|| {
            (!has_alternates)
                .then(|| {
                    missing_edge::resolve_standard_duplicate_edge_faces(
                        spine,
                        &edge_faces,
                        &allowed_faces,
                    )
                })
                .flatten()
        });
        if let Some(completed) = completed {
            edge_faces = completed;
            for (edge, (support, faces)) in supports.iter_mut().zip(&edge_faces).enumerate() {
                if support.faces == *faces {
                    continue;
                }
                support.faces = *faces;
                let Some(surface) = face_surface(ir, bindings, &surface_indices, faces[1]) else {
                    return Err(StandardTopologyFailure::MissingFaceSurface);
                };
                options[edge].retain(|pair| {
                    pair.iter().all(|point| {
                        ir.model.points.get(*point).is_some_and(|point| {
                            point_on_standard_face(
                                point.position,
                                &surface.geometry,
                                face_bounds.as_ref().and_then(|bounds| bounds[faces[1]]),
                            )
                        })
                    })
                });
                if options[edge].is_empty() {
                    return Err(StandardTopologyFailure::EmptyEndpointDomain);
                }
            }
        } else {
            for (edge, faces) in edge_faces.iter().enumerate() {
                deferred_port_edges[edge] = faces[0] == faces[1]
                    && allowed_faces
                        .get(edge)
                        .is_some_and(|faces| !faces.is_empty());
            }
            if allowed_faces.iter().any(|faces| !faces.is_empty()) {
                open_face_domains = Some(allowed_faces);
            }
        }
    }
    let has_open_face_domains = open_face_domains
        .as_ref()
        .is_some_and(|domains| domains.iter().any(|domain| !domain.is_empty()));
    let endpoint_pair_on_incident_faces = |edge: usize, pair: [usize; 2]| {
        pair.iter().all(|point| {
            let Some(position) = ir.model.points.get(*point).map(|point| point.position) else {
                return false;
            };
            supports[edge].faces.iter().all(|face| {
                face_surface(ir, bindings, &surface_indices, *face).is_some_and(|surface| {
                    let bounds = face_bounds.as_ref().and_then(|bounds| bounds[*face]);
                    point_on_standard_face(position, &surface.geometry, bounds)
                        && standard_nurbs_line_pair_on_face(
                            &surface.geometry,
                            &supports[edge],
                            &pair,
                            &ir.model.points,
                            bounds,
                        )
                })
            })
        })
    };
    if let Some(options) = &mut endpoint_options {
        for (edge, pairs) in options.iter_mut().enumerate() {
            let support = &supports[edge];
            if matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Bspline
            ) {
                continue;
            }
            let unfiltered = pairs.clone();
            pairs.retain(|pair| {
                let Some(start) = ir.model.points.get(pair[0]).map(|point| point.position) else {
                    return false;
                };
                let Some(end) = ir.model.points.get(pair[1]).map(|point| point.position) else {
                    return false;
                };
                support.faces.iter().all(|&face| {
                    let Some(surface) = face_surface(ir, bindings, &surface_indices, face) else {
                        return false;
                    };
                    standard_endpoint_pair_supports_topology(
                        &surface.geometry,
                        support,
                        start,
                        end,
                        crate::families::standard::records::standard_face_witness(
                            brep,
                            bindings[face].2,
                        ),
                    )
                })
            });
            if pairs.is_empty() {
                *pairs = unfiltered;
            }
        }
    }
    if let Some(options) = &mut endpoint_options {
        loop {
            let seeds = options
                .iter()
                .map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|[pair]| pair)
                })
                .collect::<Vec<_>>();
            let mut changed = false;
            if let Some(placement_domains) =
                missing_edge::standard_mesh_placement_endpoint_pairs(spine, &edge_faces, &seeds)
            {
                for (edge, mut domain) in placement_domains.into_iter().enumerate() {
                    if deferred_port_edges[edge] {
                        continue;
                    }
                    domain.retain(|pair| endpoint_pair_on_incident_faces(edge, *pair));
                    if domain.is_empty() {
                        continue;
                    }
                    let previous = options[edge].clone();
                    if options[edge].is_empty() {
                        options[edge] = domain;
                    } else {
                        options[edge].retain(|pair| {
                            domain.iter().any(|candidate| {
                                missing_edge::same_unordered_pair(*pair, *candidate)
                            })
                        });
                    }
                    changed |= options[edge] != previous;
                }
            }
            if let Some(boundary_domains) = options
                .iter()
                .all(|domain| !domain.is_empty())
                .then(|| {
                    missing_edge::standard_mesh_prune_endpoint_candidates(
                        spine,
                        &edge_faces,
                        options,
                    )
                })
                .flatten()
            {
                for (edge, mut domain) in boundary_domains.into_iter().enumerate() {
                    if deferred_port_edges[edge] {
                        continue;
                    }
                    domain.retain(|pair| endpoint_pair_on_incident_faces(edge, *pair));
                    let previous = options[edge].clone();
                    if options[edge].is_empty() {
                        options[edge] = domain;
                    } else {
                        options[edge].retain(|pair| {
                            domain.iter().any(|candidate| {
                                missing_edge::same_unordered_pair(*pair, *candidate)
                            })
                        });
                    }
                    changed |= options[edge] != previous;
                }
            }
            if !changed {
                break;
            }
        }
        for (edge, pairs) in options.iter_mut().enumerate() {
            pairs.retain(|pair| endpoint_pair_on_incident_faces(edge, *pair));
            pairs.sort_unstable();
            pairs.dedup();
        }
        for (candidates, options) in endpoint_candidates.iter_mut().zip(&mut *options) {
            for point in options.iter().flatten() {
                if !candidates.contains(point) {
                    candidates.push(*point);
                }
            }
        }
    }
    let graph_propagated_pairs = graph_propagated_endpoint_pairs
        .as_ref()
        .and_then(|pairs| pairs.iter().copied().collect::<Option<Vec<_>>>());
    let native_endpoint_pairs = graph_propagated_pairs.or_else(|| {
        endpoint_options.as_ref().and_then(|options| {
            const MAX_NATIVE_PORT_CHOICES: usize = 65_536;
            const MAX_NATIVE_PORT_WORK: usize = 20_000_000;

            let ports = native_ports.as_ref()?;
            let seeds = options
                .iter()
                .map(|choices| {
                    <[[usize; 2]; 1]>::try_from(choices.as_slice())
                        .ok()
                        .map(|[pair]| pair)
                })
                .collect::<Vec<_>>();
            let propagated = missing_edge::propagate_edge_port_points_with_ordered_seeds(
                ports,
                &seeds,
                &ordered_endpoint_pairs,
            )?;
            if let Some(complete) = propagated.iter().copied().collect::<Option<Vec<_>>>() {
                return Some(complete);
            }
            // Exhaustive binding is a fallback after exact identity propagation.
            // Large symmetric choice sets remain unresolved and continue through
            // trim-mesh and incidence paths instead of making decode unbounded.
            let choice_count = options.iter().map(Vec::len).sum::<usize>();
            (choice_count <= MAX_NATIVE_PORT_CHOICES
                && options
                    .len()
                    .checked_mul(choice_count)
                    .is_some_and(|work| work <= MAX_NATIVE_PORT_WORK))
            .then(|| missing_edge::bind_edge_port_candidates(ports, options))?
        })
    });
    let propagated_endpoint_pairs = endpoint_options
        .as_ref()
        .zip(missing_edge::edge_port_identities(spine))
        .and_then(|(options, ports)| {
            let pairs = options
                .iter()
                .map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|pair| pair[0])
                })
                .collect::<Vec<_>>();
            missing_edge::propagate_edge_port_points_with_ordered_seeds_and_deferred(
                &ports,
                &pairs,
                &ordered_endpoint_pairs,
                &deferred_port_edges,
            )
        })
        .zip(endpoint_options.as_ref())
        .map(|(propagated, options)| {
            propagated
                .into_iter()
                .zip(options)
                .map(|(pair, candidates)| {
                    pair.filter(|pair| {
                        candidates.iter().any(|candidate| {
                            *candidate == *pair || *candidate == [pair[1], pair[0]]
                        })
                    })
                })
                .collect::<Vec<_>>()
        });
    let mesh_propagated_endpoint_pairs = endpoint_options
        .as_ref()
        .zip(missing_edge::standard_mesh_edge_ports(spine))
        .and_then(|(options, ports)| {
            let pairs = options
                .iter()
                .map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|pair| pair[0])
                })
                .collect::<Vec<_>>();
            missing_edge::propagate_edge_port_points_with_ordered_seeds_and_deferred(
                &ports,
                &pairs,
                &ordered_endpoint_pairs,
                &deferred_port_edges,
            )
        });
    let propagated_endpoint_pairs = combine_propagated_endpoint_pairs(
        propagated_endpoint_pairs,
        mesh_propagated_endpoint_pairs,
    );
    let mut constrained_endpoint_options = endpoint_options.as_ref().map(|options| {
        options
            .iter()
            .enumerate()
            .map(|(edge, pairs)| {
                propagated_endpoint_pairs
                    .as_ref()
                    .and_then(|propagated| propagated[edge])
                    .map_or_else(|| pairs.clone(), |pair| vec![pair])
            })
            .collect::<Vec<_>>()
    });
    if let (Some(options), Some(ports)) = (
        constrained_endpoint_options.as_mut(),
        missing_edge::standard_mesh_edge_ports(spine),
    ) {
        let pruned = if deferred_port_edges.iter().any(|deferred| *deferred) {
            fbb::prune_edge_candidates_by_port_domains_with_deferred(
                &ports,
                options,
                &deferred_port_edges,
            )
        } else {
            fbb::prune_edge_candidates_by_port_domains(&ports, options)
        };
        if let Some(pruned) = pruned {
            *options = pruned;
        }
        let unique_pairs = if deferred_port_edges.iter().any(|deferred| *deferred) {
            missing_edge::unique_mesh_edge_port_candidate_pairs_with_deferred(
                &ports,
                options,
                &deferred_port_edges,
            )
        } else {
            missing_edge::unique_mesh_edge_port_candidate_pairs(&ports, options)
                .map(|pairs| pairs.into_iter().map(Some).collect())
        };
        if let Some(pairs) = unique_pairs {
            for (domain, pair) in options
                .iter_mut()
                .zip(pairs)
                .filter_map(|(domain, pair)| pair.map(|pair| (domain, pair)))
            {
                domain.retain(|candidate| missing_edge::same_unordered_pair(*candidate, pair));
            }
        }
    }
    if let Some(options) = &mut constrained_endpoint_options {
        // A same-incidence row relation is not an endpoint identity. Keep its
        // complete candidate domain for exact identity and mesh constraints.
        diagnostics.empty_endpoint_domains =
            options.iter().filter(|domain| domain.is_empty()).count();
        diagnostics.singleton_endpoint_domains =
            options.iter().filter(|domain| domain.len() == 1).count();
        diagnostics.multiple_endpoint_domains =
            options.iter().filter(|domain| domain.len() > 1).count();
        diagnostics.endpoint_domain_choices = options.iter().map(Vec::len).sum();
    }
    let resolved_endpoint_pairs = propagated_endpoint_pairs
        .and_then(|pairs| pairs.into_iter().collect::<Option<Vec<[usize; 2]>>>());
    if let Some(pairs) = &resolved_endpoint_pairs {
        let pairs = pairs.iter().copied().map(Some).collect::<Vec<_>>();
        include_native_endpoint_pairs(&mut endpoint_candidates, &pairs);
    }
    let fbb_mesh_ports = fbb_only
        .then(|| missing_edge::standard_mesh_edge_ports(spine))
        .flatten();
    let mesh_topology = if fbb_only {
        fbb_mesh_ports
            .as_deref()
            .and_then(|ports| topology::parse_fbb_with_native_vertices(spine, ports))
            .or_else(|| topology::parse_fbb(spine))
    } else {
        fbb::parse_standard(spine)
            .or_else(|| topology::parse_fbb_with_native_vertices(spine, native_ports.as_ref()?))
    };
    let mesh_bound = (!has_open_face_domains)
        .then_some(mesh_topology)
        .flatten()
        .and_then(|topology| {
            let endpoint_pairs = resolved_endpoint_pairs
                .clone()
                .or_else(|| {
                    endpoint_candidates
                        .iter()
                        .map(|candidates| <[usize; 2]>::try_from(candidates.as_slice()).ok())
                        .collect::<Option<Vec<[usize; 2]>>>()
                })
                .or_else(|| {
                    let ports = topology
                        .edge_vertices()?
                        .into_iter()
                        .map(|[left, right]| {
                            Some([u32::try_from(left).ok()?, u32::try_from(right).ok()?])
                        })
                        .collect::<Option<Vec<_>>>()?;
                    missing_edge::bind_edge_port_candidates(
                        &ports,
                        constrained_endpoint_options.as_ref()?,
                    )
                })?;
            let point_assignment = topology.bind_vertex_points(&endpoint_pairs)?;
            Some((topology, point_assignment))
        });
    let circle_anchors: Vec<Option<[usize; 2]>> = supports
        .iter()
        .zip(&endpoint_candidates)
        .map(|(support, candidates)| match &support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Circle { .. } => {
                <[usize; 2]>::try_from(candidates.as_slice()).ok()
            }
            crate::families::standard::records::StandardCurveGeometry::Line
            | crate::families::standard::records::StandardCurveGeometry::Bspline => None,
        })
        .collect();
    let mut mesh_search_exhausted = false;
    let native_fbb_topology = if fbb_only && !has_open_face_domains {
        native_endpoint_pairs.as_ref().and_then(|pairs| {
            fbb::parse_fbb_endpoints_with_edge_classes(
                spine,
                &edge_faces,
                pairs,
                Some(&edge_classes),
            )
        })
    } else {
        None
    };
    let mut selected_face_assignment = None;
    let (mut topology, point_assignment) = if let Some(bound) = mesh_bound {
        bound
    } else if let Some(topology) = native_fbb_topology {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else if let Some(topology) = (!has_open_face_domains)
        .then_some(native_endpoint_pairs.as_ref())
        .flatten()
        .and_then(|pairs| {
            fbb::parse_standard_endpoints_with_edge_classes(
                spine,
                &edge_faces,
                pairs,
                Some(&edge_classes),
            )
        })
    {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else if let Some(bound) = constrained_endpoint_options.as_ref().and_then(|options| {
        let edge_identity_evidence = supports
            .iter()
            .enumerate()
            .map(|(edge, _)| {
                standard_edge_identity_is_admitted(
                    ordered_endpoint_pairs[edge],
                    native_endpoint_evidence
                        .as_ref()
                        .and_then(|pairs| pairs.get(edge).copied().flatten()),
                    native_supports_by_row[edge].is_some(),
                    !limit_curve_bindings[edge].is_empty(),
                )
            })
            .collect::<Vec<_>>();
        let edge_direction_evidence = native_endpoint_evidence.as_ref().map_or_else(
            || supports.iter().map(|_| false).collect::<Vec<_>>(),
            |pairs| pairs.iter().map(Option::is_some).collect(),
        );
        let point_on_face = |face: usize, point: usize| {
            if let Some(membership) = face_point_membership.as_ref() {
                return membership
                    .get(face)
                    .and_then(|points| points.get(point))
                    .copied()
                    .unwrap_or(false);
            }
            let Some(position) = ir.model.points.get(point).map(|point| point.position) else {
                return false;
            };
            face_surface(ir, bindings, &surface_indices, face).is_some_and(|surface| {
                point_on_standard_face(
                    position,
                    &surface.geometry,
                    face_bounds
                        .and_then(|bounds| bounds.get(face).copied())
                        .flatten(),
                )
            })
        };
        let point_positions = ir
            .model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>();
        let mut solver_deferred_edges = deferred_port_edges.clone();
        if let Some(ports) = missing_edge::edge_port_identities(spine) {
            if !missing_edge::expand_deferred_edge_port_components(
                &ports,
                &mut solver_deferred_edges,
            ) {
                return None;
            }
        }
        let solve_mesh_candidate =
            |selected_edge_faces: &[[usize; 2]],
             selected_supports: &[crate::families::standard::records::StandardCurveSupport],
             selected_edge_classes: &[usize],
             solve_budget: &WorkBudget<'_>| {
                // FBB-only rows are complete boundary runs. Their global
                // handle quotient is the incidence source.
                let mut solver_options = standard_endpoint_options_for_selected_faces(
                    ir,
                    bindings,
                    &surface_indices,
                    selected_supports,
                    &point_positions,
                    options,
                    &edge_identity_evidence,
                );
                for (edge, deferred) in solver_deferred_edges.iter().copied().enumerate() {
                    if deferred && !edge_identity_evidence[edge] {
                        solver_options[edge].clear();
                    }
                }
                let endpoint_pairs_on_selected_faces = |pairs: &[Option<[usize; 2]>]| {
                    if pairs.len() != selected_supports.len() {
                        return false;
                    }
                    pairs.iter().enumerate().all(|(edge, pair)| {
                        let Some(pair) = pair else {
                            return true;
                        };
                        pair.iter().all(|point| {
                            selected_supports[edge]
                                .faces
                                .iter()
                                .all(|face| point_on_face(*face, *point))
                        })
                    })
                };
                let line_constraint = StandardLinePairConstraint::new(
                    &ir.model.points,
                    selected_supports,
                    &solver_options,
                );
                let face_domain_edges = open_face_domains.as_ref().map_or_else(
                    || solver_options.iter().map(|_| false).collect::<Vec<_>>(),
                    |domains| domains.iter().map(|domain| !domain.is_empty()).collect(),
                );
                let selected_circle_constraint_edges = selected_supports
                    .iter()
                    .enumerate()
                    .map(|(edge, support)| {
                        matches!(
                            support.geometry,
                            crate::families::standard::records::StandardCurveGeometry::Circle { .. }
                        ) && solver_options[edge].len() > 1
                    })
                    .collect::<Vec<_>>();
                let partial_constraint_edges = selected_circle_constraint_edges
                    .iter()
                    .zip(line_constraint.flexible_edge_mask())
                    .zip(&face_domain_edges)
                    .map(|((circle, line), face)| *circle || *line || *face)
                    .collect::<Vec<_>>();
                let preferred_budget =
                    solve_budget.child_slice(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS);
                let preferred = mesh_quotient::parse_standard_mesh_candidate_outcome(
                    spine,
                    selected_edge_faces,
                    &solver_options,
                    selected_edge_classes,
                    &edge_geometry,
                    &edge_identity_evidence,
                    &edge_direction_evidence,
                    has_open_face_domains,
                    &partial_constraint_edges,
                    &partial_constraint_edges,
                    Some(&partial_constraint_edges),
                    None,
                    &preferred_budget,
                    |pairs| {
                        endpoint_pairs_on_selected_faces(pairs) && line_constraint.is_valid(pairs)
                    },
                    |pairs| {
                        endpoint_pairs_on_selected_faces(pairs)
                            && line_constraint.is_simple(pairs)
                            && standard_circle_pair_solution_is_simple(
                                ir,
                                bindings,
                                &surface_indices,
                                selected_supports,
                                &solver_options,
                                pairs,
                            )
                    },
                );
                if !solve_budget.charge_by(preferred_budget.consumed()) {
                    return mesh_quotient::MeshCandidateSolve::Exhausted(
                        mesh_quotient::MeshCandidateExhaustion::FaceDomainEnumeration,
                    );
                }
                let has_circle_preference = selected_circle_constraint_edges
                    .iter()
                    .any(|constrained| *constrained);
                if has_circle_preference {
                    // Circular interval choice is a preference because both
                    // complementary arcs can be valid. The fallback relaxes
                    // only that choice; straight-carrier interval overlap is
                    // an invalid endpoint relation in both searches.
                    let fallback_budget =
                        solve_budget.child_slice(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS);
                    let fallback = mesh_quotient::parse_standard_mesh_candidate_outcome(
                        spine,
                        selected_edge_faces,
                        &solver_options,
                        selected_edge_classes,
                        &edge_geometry,
                        &edge_identity_evidence,
                        &edge_direction_evidence,
                        has_open_face_domains,
                        &partial_constraint_edges,
                        &partial_constraint_edges,
                        Some(&partial_constraint_edges),
                        None,
                        &fallback_budget,
                        |pairs| {
                            endpoint_pairs_on_selected_faces(pairs)
                                && line_constraint.is_simple(pairs)
                        },
                        |pairs| {
                            endpoint_pairs_on_selected_faces(pairs)
                                && line_constraint.is_simple(pairs)
                        },
                    );
                    if !solve_budget.charge_by(fallback_budget.consumed()) {
                        return mesh_quotient::MeshCandidateSolve::Exhausted(
                            mesh_quotient::MeshCandidateExhaustion::FaceDomainEnumeration,
                        );
                    }
                    retry_rejected_mesh_solution(preferred, || fallback)
                } else {
                    preferred
                }
            };
        let outcome = if has_open_face_domains {
            let domains = open_face_domains.as_deref().unwrap_or_default();
            let face_assignments = endpoint_face_assignments.as_deref().map_or(
                mesh_quotient::MeshFaceAssignmentCandidates::Domains {
                    edge_faces: &edge_faces,
                    allowed_faces: domains,
                    face_count,
                },
                |assignments| mesh_quotient::MeshFaceAssignmentCandidates::Concrete {
                    assignments,
                    face_count,
                },
            );
            match mesh_quotient::parse_standard_mesh_candidate_outcome_with_face_assignments(
                face_assignments,
                work_budget,
                |selected_edge_faces, branch_budget| {
                    let selected_supports = supports
                        .iter()
                        .zip(selected_edge_faces)
                        .map(|(support, faces)| {
                            let mut selected = support.clone();
                            selected.faces = *faces;
                            selected
                        })
                        .collect::<Vec<_>>();
                    let selected_edge_classes = standard_curve_edge_classes(&selected_supports);
                    solve_mesh_candidate(
                        selected_edge_faces,
                        &selected_supports,
                        &selected_edge_classes,
                        branch_budget,
                    )
                },
            ) {
                mesh_quotient::MeshFaceDomainCandidateSolve::Solved(
                    faces,
                    topology,
                    assignment,
                ) => {
                    selected_face_assignment = Some(faces);
                    mesh_quotient::MeshCandidateSolve::Solved(topology, assignment)
                }
                mesh_quotient::MeshFaceDomainCandidateSolve::Rejected(rejection) => {
                    mesh_quotient::MeshCandidateSolve::Rejected(rejection)
                }
                mesh_quotient::MeshFaceDomainCandidateSolve::Ambiguous(ambiguity) => {
                    mesh_quotient::MeshCandidateSolve::Ambiguous(ambiguity)
                }
                mesh_quotient::MeshFaceDomainCandidateSolve::Exhausted(exhaustion) => {
                    mesh_quotient::MeshCandidateSolve::Exhausted(exhaustion)
                }
            }
        } else {
            solve_mesh_candidate(&edge_faces, &supports, &edge_classes, work_budget)
        };
        match outcome {
            mesh_quotient::MeshCandidateSolve::Solved(topology, assignment) => {
                Some((topology, assignment))
            }
            mesh_quotient::MeshCandidateSolve::Rejected(rejection) => {
                diagnostics.mesh_rejection = Some(rejection);
                None
            }
            mesh_quotient::MeshCandidateSolve::Ambiguous(ambiguity) => {
                diagnostics.mesh_ambiguity = Some(ambiguity);
                None
            }
            mesh_quotient::MeshCandidateSolve::Exhausted(exhaustion) => {
                diagnostics.mesh_exhaustion = Some(exhaustion);
                mesh_search_exhausted = true;
                None
            }
        }
    }) {
        bound
    } else if let Some(topology) = (!has_open_face_domains)
        .then_some(constrained_endpoint_options.as_ref())
        .flatten()
        .and_then(|options| {
            missing_edge::standard_mesh_edge_ports(spine)
                .and_then(|ports| {
                    fbb::parse_standard_port_endpoint_candidates(
                        spine,
                        &edge_faces,
                        options,
                        &ports,
                        work_budget,
                    )
                })
                .or_else(|| {
                    fbb::parse_standard_endpoint_candidates(
                        spine,
                        &edge_faces,
                        options,
                        work_budget,
                    )
                })
        })
    {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else if let Some(topology) = (!has_open_face_domains)
        .then(|| fbb::parse_standard_motif(spine, &edge_faces, &circle_anchors))
        .flatten()
    {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else {
        return Err(if mesh_search_exhausted || work_budget.exhausted() {
            StandardTopologyFailure::TopologySearchExhausted
        } else if diagnostics.mesh_ambiguity.is_some() {
            StandardTopologyFailure::AmbiguousTopologySolution
        } else {
            StandardTopologyFailure::NoTopologySolution
        });
    };
    if let Some(faces) = selected_face_assignment {
        edge_faces = faces;
        for (support, faces) in supports.iter_mut().zip(&edge_faces) {
            support.faces = *faces;
        }
    }
    let Some(edge_vertices) = validate_standard_topology(
        ir,
        annotations,
        &mut topology,
        &point_assignment,
        &supports,
        &endpoint_candidates,
        face_count,
    ) else {
        return Err(StandardTopologyFailure::InvalidTopologySolution);
    };
    let resolved_limit_curve_bindings = edge_vertices
        .iter()
        .enumerate()
        .map(|(edge, logical_vertices)| {
            let points = [
                point_assignment[logical_vertices[0]],
                point_assignment[logical_vertices[1]],
            ];
            resolve_standard_limit_curve_binding(&limit_curve_bindings[edge], points)
        })
        .collect::<Vec<_>>();
    *bound_limit_curve_count = resolved_limit_curve_bindings
        .iter()
        .filter(|binding| binding.is_some())
        .count();
    emit_standard_topology(
        ir,
        annotations,
        bindings,
        brep,
        &surface_indices,
        &supports,
        &edge_vertices,
        &point_assignment,
        &topology,
        &native_supports_by_row,
        &resolved_limit_curve_bindings,
        limit_curves,
    );
    Ok(())
}

/// Validates the solved topology against the decoded model, applies body kinds
/// and face partitioning, and returns the per-edge logical vertex pairs.
#[allow(clippy::question_mark)]
fn validate_standard_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &mut crate::families::standard::topology::StandardTopology,
    point_assignment: &[usize],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_candidates: &[Vec<usize>],
    face_count: usize,
) -> Option<Vec<[usize; 2]>> {
    if topology.face_count() != face_count
        || topology.edge_rows().len() != supports.len()
        || topology.vertex_points().len() != ir.model.points.len()
        || !topology
            .vertex_points()
            .iter()
            .zip(&ir.model.points)
            .all(|(stored, point)| {
                stored[0] == point.position.x
                    && stored[1] == point.position.y
                    && stored[2] == point.position.z
            })
    {
        return None;
    }
    let face_groups = vec![topology.face_count()];
    if topology.orient_solid_body_cycles(&face_groups).is_none() {
        return None;
    }
    let Some(body_kinds) = topology.body_kinds(&face_groups) else {
        return None;
    };
    let Some(edge_vertices) = topology.edge_vertices() else {
        return None;
    };
    if edge_vertices.iter().enumerate().any(|(edge, vertices)| {
        let start = point_assignment[vertices[0]];
        let end = point_assignment[vertices[1]];
        !endpoint_candidates[edge].is_empty()
            && (!endpoint_candidates[edge].contains(&start)
                || !endpoint_candidates[edge].contains(&end))
    }) {
        return None;
    }
    let Some(body_arena_indices) = (0..body_kinds.len())
        .map(|body_index| {
            let id = BodyId(format!("catia:standard:body#{body_index}"));
            ir.model.bodies.iter().position(|body| body.id == id)
        })
        .collect::<Option<Vec<_>>>()
    else {
        return None;
    };
    for (&arena_index, &kind) in body_arena_indices.iter().zip(&body_kinds) {
        ir.model.bodies[arena_index].kind = kind;
    }
    if !partition_standard_face_components(ir, annotations, &topology.face_components()) {
        return None;
    }
    Some(edge_vertices)
}

fn standard_boundary_roles(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    topology: &crate::families::standard::topology::StandardTopology,
    face_index: usize,
    point_assignment: &[usize],
) -> Vec<LoopBoundaryRole> {
    let Some(face_topology) = topology.faces().get(face_index) else {
        return Vec::new();
    };
    if face_topology.boundaries.len() <= 1 {
        return if face_topology.boundaries.is_empty() {
            Vec::new()
        } else {
            vec![LoopBoundaryRole::Outer]
        };
    }
    let Some(surface_id) = bindings.get(face_index).map(|binding| &binding.0) else {
        return vec![LoopBoundaryRole::Unspecified; face_topology.boundaries.len()];
    };
    let Some(&surface_index) = surface_indices.get(surface_id) else {
        return vec![LoopBoundaryRole::Unspecified; face_topology.boundaries.len()];
    };
    let Some(surface) = ir.model.surfaces.get(surface_index) else {
        return vec![LoopBoundaryRole::Unspecified; face_topology.boundaries.len()];
    };
    let Some(boundaries) = face_topology
        .boundaries
        .iter()
        .map(|boundary| {
            boundary
                .coedges
                .iter()
                .map(|coedge| {
                    let point_index = *point_assignment.get(coedge.start_vertex)?;
                    Some(ir.model.points.get(point_index)?.position)
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()
    else {
        return vec![LoopBoundaryRole::Unspecified; face_topology.boundaries.len()];
    };
    crate::boundary_roles::classify_planar_boundary_roles(&surface.geometry, &boundaries)
}

/// Emits the edge, loop, coedge, and pcurve IR layers for the solved topology.
#[allow(clippy::too_many_arguments)]
fn emit_standard_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    edge_vertices: &[[usize; 2]],
    point_assignment: &[usize],
    topology: &crate::families::standard::topology::StandardTopology,
    native_edge_supports: &[Option<StandardEdgeSupport>],
    limit_curve_bindings: &[Option<StandardLimitCurveBinding>],
    limit_curves: &[NurbsCurve],
) {
    let mut edge_reversed = Vec::with_capacity(supports.len());
    for (edge_index, (support, logical_vertices)) in supports.iter().zip(edge_vertices).enumerate()
    {
        let start_point = point_assignment[logical_vertices[0]];
        let end_point = point_assignment[logical_vertices[1]];
        let native_support = native_edge_supports
            .get(edge_index)
            .and_then(Option::as_ref)
            .filter(|native| {
                standard_native_support_endpoint_pair(
                    native,
                    &ir.model.points,
                    &[start_point, end_point],
                    Some([start_point, end_point]),
                )
                .is_some()
            });
        let (curve, param_range) = build_standard_edge_curve(
            ir,
            annotations,
            bindings,
            surface_indices,
            brep,
            support,
            [start_point, end_point],
            native_support,
            limit_curve_bindings[edge_index]
                .map(|binding| (&limit_curves[binding.curve], binding.parameter_range)),
        );
        let reversed = param_range.is_some_and(|range| range[0] > range[1]);
        edge_reversed.push(reversed);
        let param_range = param_range.map(ordered_range);
        let [start_point, end_point] = if reversed {
            [end_point, start_point]
        } else {
            [start_point, end_point]
        };
        let id = EdgeId(format!("catia:standard:edge#{edge_index}"));
        annotate(
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            support.pos as u64,
            "standard_spine_edge_row",
            Exactness::ByteExact,
        );
        if curve.is_some() {
            annotations.derived(&id, "curve");
        }
        annotations.derived(&id, "start").derived(&id, "end");
        if param_range.is_some() {
            annotations.derived(&id, "param_range");
        }
        ir.model.edges.push(Edge {
            id,
            curve,
            start: VertexId(format!("catia:standard:v#{start_point}")),
            end: VertexId(format!("catia:standard:v#{end_point}")),
            param_range,
            tolerance: None,
        });
    }

    let curve_indices = ir
        .model
        .curves
        .iter()
        .enumerate()
        .map(|(index, curve)| (curve.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut edge_coedges = vec![Vec::new(); ir.model.edges.len()];
    for (face_index, face_topology) in topology.faces().iter().enumerate() {
        let boundary_roles = standard_boundary_roles(
            ir,
            bindings,
            surface_indices,
            topology,
            face_index,
            point_assignment,
        );
        for (loop_index, boundary) in face_topology.boundaries.iter().enumerate() {
            let loop_id = LoopId(format!("catia:standard:loop#{face_index}:{loop_index}"));
            let boundary_role = boundary_roles.get(loop_index).copied().unwrap_or_default();
            let coedge_ids: Vec<CoedgeId> = (0..boundary.coedges.len())
                .map(|coedge_index| {
                    CoedgeId(format!(
                        "catia:standard:coedge#{face_index}:{loop_index}:{coedge_index}"
                    ))
                })
                .collect();
            let vertex_uses: Vec<VertexUse> = boundary
                .coedges
                .iter()
                .enumerate()
                .map(|(coedge_index, edge_use)| VertexUse {
                    vertex: VertexId(format!(
                        "catia:standard:v#{}",
                        point_assignment[edge_use.end_vertex]
                    )),
                    after: Some(coedge_ids[coedge_index].clone()),
                    pcurves: Vec::new(),
                })
                .collect();
            for (coedge_index, edge_use) in boundary.coedges.iter().enumerate() {
                let support = &supports[edge_use.edge_row];
                let logical_vertices = edge_vertices[edge_use.edge_row];
                let start = ir.model.points[point_assignment[logical_vertices[0]]].position;
                let end = ir.model.points[point_assignment[logical_vertices[1]]].position;
                let edge_curve = ir.model.edges[edge_use.edge_row]
                    .curve
                    .as_ref()
                    .and_then(|id| curve_indices.get(id))
                    .map(|index| &ir.model.curves[*index].geometry);
                let pcurve_id = standard_pcurve_geometry(
                    &ir.model.surfaces[surface_indices[&bindings[face_index].0]].geometry,
                    support,
                    start,
                    end,
                    crate::families::standard::records::standard_face_witness(
                        brep,
                        bindings[face_index].2,
                    ),
                    edge_curve,
                )
                .map(|(geometry, range)| {
                    let id = PcurveId(format!(
                        "catia:standard:pcurve#{face_index}:{loop_index}:{coedge_index}"
                    ));
                    annotate(
                        annotations,
                        &id,
                        "MainDataStream+SurfacicReps",
                        support.pos as u64,
                        "derived_surface_parameter_curve",
                        Exactness::Derived,
                    );
                    annotations.derived(&id, "geometry");
                    ir.model.pcurves.push(Pcurve {
                        id: id.clone(),
                        geometry,
                        wrapper_reversed: None,
                        parameter_range: Some(range),
                        fit_tolerance: None,
                        native_tail_flags: None,
                    });
                    (id, range)
                });
                let arena_index = ir.model.coedges.len();
                edge_coedges[edge_use.edge_row].push(arena_index);
                let id = coedge_ids[coedge_index].clone();
                annotate(
                    annotations,
                    &id,
                    "MainDataStream+SurfacicReps",
                    0,
                    "trim_mesh_boundary_run",
                    Exactness::ByteExact,
                );
                for field in [
                    "owner_loop",
                    "edge",
                    "next",
                    "previous",
                    "radial_next",
                    "sense",
                ] {
                    annotations.derived(&id, field);
                }
                if pcurve_id.is_some() {
                    annotations.derived(&id, "pcurves");
                }
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: loop_id.clone(),
                    edge: EdgeId(format!("catia:standard:edge#{}", edge_use.edge_row)),
                    next: coedge_ids[(coedge_index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(coedge_index + coedge_ids.len() - 1) % coedge_ids.len()]
                        .clone(),
                    radial_next: coedge_ids[coedge_index].clone(),
                    sense: if edge_use.reversed ^ edge_reversed[edge_use.edge_row] {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves: pcurve_id
                        .map(|(pcurve, range)| cadmpeg_ir::topology::PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: edge_use.reversed.then_some([range[1], range[0]]),
                        })
                        .into_iter()
                        .collect(),
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
            annotate(
                annotations,
                &loop_id,
                "MainDataStream+SurfacicReps",
                0,
                "trim_mesh_boundary_cycle",
                Exactness::ByteExact,
            );
            annotations
                .derived(&loop_id, "face")
                .derived(&loop_id, "coedges")
                .derived(&loop_id, "vertex_uses");
            if boundary_role != LoopBoundaryRole::Unspecified {
                annotations.derived(&loop_id, "boundary_role");
            }
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: FaceId(format!("catia:standard:face#{face_index}")),
                boundary_role,
                coedges: coedge_ids,
                vertex_uses,
            });
            ir.model.faces[face_index].loops.push(loop_id);
        }
    }
    for uses in edge_coedges {
        for (position, current) in uses.iter().enumerate() {
            let next = uses[(position + 1) % uses.len()];
            ir.model.coedges[*current].radial_next = ir.model.coedges[next].id.clone();
        }
    }
}

pub(crate) fn standard_native_support_endpoint_pair(
    support: &StandardEdgeSupport,
    points: &[Point],
    candidates: &[usize],
    required_pair: Option<[usize; 2]>,
) -> Option<[usize; 2]> {
    const VERTEX_MATCH_TOLERANCE: f64 = 2e-3;

    let lifted = support
        .carriers
        .iter()
        .zip(&support.pcurves)
        .map(|(carrier, pcurve)| {
            let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) = carrier
            else {
                return None;
            };
            Some(support.parameter_range.map(|parameter| {
                let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter)?;
                cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v)
            }))
        })
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = <[[Option<Point3>; 2]; 2]>::try_from(lifted).ok()?;
    let first = first.into_iter().collect::<Option<Vec<_>>>()?;
    let second = second.into_iter().collect::<Option<Vec<_>>>()?;
    let direct = first
        .iter()
        .zip(&second)
        .map(|(left, right)| left.distance_squared(*right).sqrt())
        .fold(0.0, f64::max);
    let reversed = first
        .iter()
        .zip(second.iter().rev())
        .map(|(left, right)| left.distance_squared(*right).sqrt())
        .fold(0.0, f64::max);
    if direct.min(reversed) > SUPPORT_AGREEMENT_TOLERANCE {
        return None;
    }
    let pair = first
        .into_iter()
        .map(|expected| {
            let matches = candidates
                .iter()
                .copied()
                .filter(|point| {
                    points.get(*point).is_some_and(|point| {
                        point.position.distance_squared(expected).sqrt() <= VERTEX_MATCH_TOLERANCE
                    })
                })
                .collect::<Vec<_>>();
            <[usize; 1]>::try_from(matches).ok().map(|[point]| point)
        })
        .collect::<Option<Vec<_>>>()?;
    <[usize; 2]>::try_from(pair)
        .ok()
        .filter(|pair| pair[0] != pair[1])
        .filter(|pair| {
            required_pair.is_none_or(|required| missing_edge::same_unordered_pair(*pair, required))
        })
}

pub(crate) fn resolve_standard_endpoint_pairs(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    candidates: &[Vec<usize>],
) -> Option<Vec<Vec<[usize; 2]>>> {
    const MAX_PAIR_RELATIONS_PER_EDGE: usize = 65_536;

    let mut resolved: Vec<Vec<[usize; 2]>> = candidates
        .iter()
        .map(|points| {
            <[usize; 2]>::try_from(points.as_slice())
                .map(|pair| vec![pair])
                .unwrap_or_default()
        })
        .collect();
    for (edge, support) in supports.iter().enumerate() {
        let crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } =
            support.geometry
        else {
            continue;
        };
        let count = candidates[edge].len();
        if count < 2 {
            continue;
        }
        let include_full_circle_seams = count == 2
            && if let [start, end] = candidates[edge].as_slice() {
                if let (Some(start), Some(end)) = (
                    ir.model.points.get(*start).map(|point| point.position),
                    ir.model.points.get(*end).map(|point| point.position),
                ) {
                    let midpoint = Point3::new(
                        (start.x + end.x) * 0.5,
                        (start.y + end.y) * 0.5,
                        (start.z + end.z) * 0.5,
                    );
                    midpoint.distance(center) <= EPS_ANTIPODAL_CIRCLE
                        && (start.distance(end) - 2.0 * radius).abs() <= EPS_ANTIPODAL_CIRCLE
                } else {
                    false
                }
            } else {
                false
            };
        let relation_count = count
            .checked_mul(count.saturating_sub(1))
            .and_then(|value| value.checked_div(2))
            .and_then(|value| value.checked_add(if include_full_circle_seams { count } else { 0 }));
        if relation_count.is_some_and(|relations| relations <= MAX_PAIR_RELATIONS_PER_EDGE) {
            let mut pairs = Vec::with_capacity(relation_count.unwrap_or_default());
            for (left, &start) in candidates[edge].iter().enumerate() {
                let first_end = left + usize::from(!include_full_circle_seams);
                for &end in &candidates[edge][first_end..] {
                    pairs.push([start, end]);
                }
            }
            resolved[edge] = pairs;
        }
    }
    let mut line_groups = HashMap::<[usize; 2], Vec<usize>>::new();
    for (edge, support) in supports.iter().enumerate() {
        if !resolved[edge].is_empty() {
            continue;
        }
        let mut faces = support.faces;
        faces.sort_unstable();
        let line_like = match support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Line => true,
            crate::families::standard::records::StandardCurveGeometry::Bspline => {
                let surfaces = faces.map(|face| {
                    face_surface(ir, bindings, surface_indices, face)
                        .map(|surface| &surface.geometry)
                });
                matches!(surfaces, [Some(left), Some(right)] if intersection_line_direction(left, right).is_some())
            }
            crate::families::standard::records::StandardCurveGeometry::Circle { .. } => false,
        };
        if line_like {
            line_groups.entry(faces).or_default().push(edge);
        }
    }
    for (faces, edges) in line_groups {
        let surface0 = face_surface(ir, bindings, surface_indices, faces[0])?;
        let surface1 = face_surface(ir, bindings, surface_indices, faces[1])?;
        let direction = intersection_line_direction(&surface0.geometry, &surface1.geometry);
        let same_cone_surface = matches!(
            (&surface0.geometry, &surface1.geometry),
            (SurfaceGeometry::Cone { .. }, SurfaceGeometry::Cone { .. })
        ) && surface0.geometry == surface1.geometry;
        let points = candidates.get(*edges.first()?)?;
        let relation_count = points
            .len()
            .checked_mul(points.len().saturating_sub(1))
            .and_then(|value| value.checked_div(2));
        if relation_count.is_none_or(|count| count > MAX_PAIR_RELATIONS_PER_EDGE) {
            continue;
        }
        let mut pairs = Vec::new();
        for (left, &start) in points.iter().enumerate() {
            for &end_index in &points[left + 1..] {
                let start_point = ir.model.points.get(start)?.position;
                let end_point = ir.model.points.get(end_index)?.position;
                let segment = Vector3::new(
                    end_point.x - start_point.x,
                    end_point.y - start_point.y,
                    end_point.z - start_point.z,
                );
                let segment_norm = segment.x.hypot(segment.y).hypot(segment.z);
                let midpoint = Point3::new(
                    (start_point.x + end_point.x) * 0.5,
                    (start_point.y + end_point.y) * 0.5,
                    (start_point.z + end_point.z) * 0.5,
                );
                let follows_direction = direction.is_none_or(|direction| {
                    let direction_norm = direction.x.hypot(direction.y).hypot(direction.z);
                    direction_norm != 0.0
                        && segment
                            .cross(direction)
                            .dot(segment.cross(direction))
                            .sqrt()
                            <= 1e-2 * segment_norm * direction_norm
                });
                let follows_same_cone_generator = !same_cone_surface
                    || same_cone_generator_pair(
                        &surface0.geometry,
                        &surface1.geometry,
                        start_point,
                        end_point,
                    );
                if segment_norm != 0.0
                    && follows_direction
                    && follows_same_cone_generator
                    && point_on_surface(midpoint, &surface0.geometry)
                    && point_on_surface(midpoint, &surface1.geometry)
                {
                    pairs.push([points[left], end_index]);
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        if pairs.len() < edges.len() {
            continue;
        }
        // A multi-row relation is not ordered by the support-table ordinal.
        // Keep every valid pair on every physical row until the trim quotient
        // binds the row; lexicographic row assignment can break a serialized
        // boundary even when the resulting analytic edge set is equivalent.
        if pairs.len() == edges.len() && edges.len() == 1 {
            resolved[edges[0]] = vec![pairs[0]];
        } else {
            for edge in edges {
                resolved[edge].clone_from(&pairs);
            }
        }
    }
    let mut fallback_relation_budget = 65_536usize;
    for (edge, pairs) in resolved.iter_mut().enumerate() {
        if !pairs.is_empty() {
            continue;
        }
        let points = &candidates[edge];
        let relation_count = points
            .len()
            .checked_mul(points.len().saturating_sub(1))
            .and_then(|value| value.checked_div(2));
        let Some(relation_count) = relation_count.filter(|count| {
            *count <= MAX_PAIR_RELATIONS_PER_EDGE && *count <= fallback_relation_budget
        }) else {
            continue;
        };
        fallback_relation_budget -= relation_count;
        *pairs = points
            .iter()
            .enumerate()
            .flat_map(|(left, &start)| points[left + 1..].iter().map(move |&end| [start, end]))
            .collect();
    }
    Some(resolved)
}

fn standard_curve_edge_classes(
    supports: &[crate::families::standard::records::StandardCurveSupport],
) -> Vec<usize> {
    let mut classes = Vec::with_capacity(supports.len());
    for (edge, support) in supports.iter().enumerate() {
        let class = supports[..edge]
            .iter()
            .position(|candidate| {
                let mut candidate_faces = candidate.faces;
                candidate_faces.sort_unstable();
                let mut support_faces = support.faces;
                support_faces.sort_unstable();
                candidate_faces == support_faces
                    && match (&candidate.geometry, &support.geometry) {
                        (
                            crate::families::standard::records::StandardCurveGeometry::Circle {
                                center: left_center,
                                radius: left_radius,
                            },
                            crate::families::standard::records::StandardCurveGeometry::Circle {
                                center: right_center,
                                radius: right_radius,
                            },
                        ) => {
                            left_center.x.to_bits() == right_center.x.to_bits()
                                && left_center.y.to_bits() == right_center.y.to_bits()
                                && left_center.z.to_bits() == right_center.z.to_bits()
                                && left_radius.to_bits() == right_radius.to_bits()
                        }
                        (
                            crate::families::standard::records::StandardCurveGeometry::Line,
                            crate::families::standard::records::StandardCurveGeometry::Line,
                        ) => true,
                        _ => false,
                    }
            })
            .map_or(edge, |candidate| classes[candidate]);
        classes.push(class);
    }
    classes
}

fn standard_curve_geometry_gauge_keys(
    supports: &[crate::families::standard::records::StandardCurveSupport],
) -> Vec<MeshEdgeGeometry> {
    supports
        .iter()
        .map(|support| match &support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Line => {
                MeshEdgeGeometry::Line
            }
            crate::families::standard::records::StandardCurveGeometry::Circle {
                center,
                radius,
            } => MeshEdgeGeometry::Circle {
                center: [center.x.to_bits(), center.y.to_bits(), center.z.to_bits()],
                radius: radius.to_bits(),
            },
            crate::families::standard::records::StandardCurveGeometry::Bspline => {
                MeshEdgeGeometry::Bspline
            }
        })
        .collect()
}

pub(crate) fn standard_circle_endpoint_candidates(
    points: &[Point],
    center: Point3,
    radius: f64,
    faces: Option<
        [(
            &SurfaceGeometry,
            Option<crate::families::standard::records::StandardFaceBounds>,
        ); 2],
    >,
) -> Vec<usize> {
    points
        .iter()
        .enumerate()
        .filter_map(|(index, point)| {
            let on_circle = (point.position.distance_squared(center).sqrt() - radius).abs() <= 1e-3;
            let incident = faces.is_none_or(|faces| {
                faces.into_iter().all(|(surface, bounds)| {
                    point_on_standard_face(point.position, surface, bounds)
                })
            });
            (on_circle && incident).then_some(index)
        })
        .collect()
}

/// Resolve standard-row endpoints from equal standard and native edge identities.
pub(crate) fn standard_native_graph_endpoint_pairs(
    graph: Option<&crate::families::b5::graph::B5Graph>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    native_edges: &BTreeMap<u32, [u32; 2]>,
    points: &[Point],
) -> Option<Vec<Option<[usize; 2]>>> {
    let graph = graph?;
    let identity_points = unique_native_identity_points(
        &graph.logical_vertex_refs,
        &graph.logical_vertex_points,
        graph.vertex_points.len(),
        &graph.vertex_tolerances,
        points,
    );
    Some(
        supports
            .iter()
            .map(|support| {
                let [start_identity, end_identity] = native_edges.get(&support.tag)?;
                Some([
                    *identity_points.get(start_identity)?,
                    *identity_points.get(end_identity)?,
                ])
            })
            .collect(),
    )
}

/// Bind standard rows to ordered coordinate rows through the file-global
/// object journal: `0x60.tag` selects the `b5 03 5e` object id, whose ordered
/// vertex identities select positions in the standard vertex roster.
pub(crate) fn standard_serialized_endpoint_pairs(
    supports: &[crate::families::standard::records::StandardCurveSupport],
    native_edges: &BTreeMap<u32, [u32; 2]>,
    vertex_roster: &[u32],
) -> Option<Vec<Option<[usize; 2]>>> {
    let mut point_by_identity = HashMap::with_capacity(vertex_roster.len());
    for (point, identity) in vertex_roster.iter().copied().enumerate() {
        if point_by_identity.insert(identity, point).is_some() {
            return None;
        }
    }
    Some(
        supports
            .iter()
            .map(|support| {
                let [start, end] = native_edges.get(&support.tag)?;
                Some([*point_by_identity.get(start)?, *point_by_identity.get(end)?])
            })
            .collect(),
    )
}

pub(crate) fn merge_standard_edge_vertex_references(
    target: &mut BTreeMap<u32, [u32; 2]>,
    source: impl IntoIterator<Item = (u32, [u32; 2])>,
) -> bool {
    for (edge, vertices) in source {
        match target.entry(edge) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(vertices);
            }
            std::collections::btree_map::Entry::Occupied(entry) if *entry.get() == vertices => {}
            std::collections::btree_map::Entry::Occupied(_) => return false,
        }
    }
    true
}

/// Resolve native two-sided edge carriers by equal standard and native identities.
pub(crate) fn standard_native_support_edge_ids(
    supports: &[crate::families::standard::records::StandardCurveSupport],
    native_support_ids: &HashSet<u32>,
) -> Vec<Option<u32>> {
    let mut exact_row_counts = HashMap::<u32, usize>::new();
    for support in supports {
        if native_support_ids.contains(&support.tag) {
            *exact_row_counts.entry(support.tag).or_default() += 1;
        }
    }

    supports
        .iter()
        .map(|support| {
            (native_support_ids.contains(&support.tag)
                && exact_row_counts.get(&support.tag) == Some(&1))
            .then_some(support.tag)
        })
        .collect()
}

/// Return whether a standard edge has an admitted identity binding.
///
/// Native port pairs remain useful for endpoint propagation and candidate
/// pruning, but an allocation-only port pair does not bind its row to decoded
/// coordinates and therefore must not freeze an evidence-preserving gauge.
pub(crate) fn standard_edge_identity_is_admitted(
    ordered_endpoint_pair: Option<[usize; 2]>,
    native_endpoint_pair: Option<[usize; 2]>,
    has_native_support: bool,
    has_limit_curve_binding: bool,
) -> bool {
    ordered_endpoint_pair.is_some()
        || native_endpoint_pair.is_some()
        || has_native_support
        || has_limit_curve_binding
}

pub(crate) fn include_native_endpoint_pairs(
    candidates: &mut [Vec<usize>],
    pairs: &[Option<[usize; 2]>],
) {
    for (candidates, pair) in candidates.iter_mut().zip(pairs) {
        if let Some(pair) = pair {
            for point in pair {
                if !candidates.contains(point) {
                    candidates.push(*point);
                }
            }
        }
    }
}

pub(crate) fn combine_propagated_endpoint_pairs(
    raw: Option<Vec<Option<[usize; 2]>>>,
    mesh: Option<Vec<Option<[usize; 2]>>>,
) -> Option<Vec<Option<[usize; 2]>>> {
    let pairs = match (raw, mesh) {
        (_, Some(mesh)) if mesh.iter().all(Option::is_some) => mesh,
        (Some(raw), _) if raw.iter().all(Option::is_some) => raw,
        (Some(raw), Some(mesh)) => raw
            .into_iter()
            .zip(mesh)
            .map(|(raw, mesh)| match (raw, mesh) {
                (Some(raw), Some(mesh)) if raw == mesh || raw == [mesh[1], mesh[0]] => Some(raw),
                (Some(_), Some(_)) => None,
                (Some(pair), None) | (None, Some(pair)) => Some(pair),
                (None, None) => None,
            })
            .collect(),
        (Some(pairs), None) | (None, Some(pairs)) => pairs,
        (None, None) => return None,
    };
    (!pairs.is_empty()).then_some(pairs)
}

pub(crate) fn merge_native_endpoint_evidence(
    graph: Option<&[Option<[usize; 2]>]>,
    roster: Option<&[Option<[usize; 2]>]>,
) -> Result<Option<Vec<Option<[usize; 2]>>>, &'static str> {
    match (graph, roster) {
        (Some(graph), Some(roster)) => {
            if graph.len() != roster.len() {
                return Err("native endpoint evidence length mismatch");
            }
            // The roster is the standard BREP's serialized identity-to-point
            // relation. Graph coordinates are reconstructed from independent
            // object records and only supply identities absent from the roster.
            if roster.iter().all(Option::is_some) {
                return Ok(Some(roster.to_vec()));
            }
            graph
                .iter()
                .zip(roster)
                .map(|(graph, roster)| match (graph, roster) {
                    (Some(graph), Some(roster)) if graph != roster => {
                        Err("conflicting native endpoint evidence")
                    }
                    (Some(pair), _) | (_, Some(pair)) => Ok(Some(*pair)),
                    (None, None) => Ok(None),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
        (Some(pairs), None) | (None, Some(pairs)) => Ok(Some(pairs.to_vec())),
        (None, None) => Ok(None),
    }
}

fn merge_ordered_endpoint_pair(
    ordered_pairs: &mut [Option<[usize; 2]>],
    edge: usize,
    pair: [usize; 2],
) -> bool {
    let Some(slot) = ordered_pairs.get_mut(edge) else {
        return false;
    };
    match slot {
        Some(previous) => *previous == pair,
        None => {
            *slot = Some(pair);
            true
        }
    }
}

/// Merge endpoint coordinates derived from support geometry without replacing
/// the direction selected by a native identity source. Support pcurves
/// corroborate the endpoint identity, but their wrapper order is not a second
/// directed edge-identity source.
pub(crate) fn merge_derived_endpoint_pair(
    ordered_pairs: &mut [Option<[usize; 2]>],
    edge: usize,
    pair: [usize; 2],
) -> bool {
    let Some(slot) = ordered_pairs.get_mut(edge) else {
        return false;
    };
    match slot {
        Some(previous) => missing_edge::same_unordered_pair(*previous, pair),
        None => {
            *slot = Some(pair);
            true
        }
    }
}

/// Return checked successor identities as endpoint-domain corroboration.
///
/// The creation-order pattern is not a row identity. It may narrow an
/// existing geometric domain independently for either successor identity, but
/// it never supplies native endpoint evidence by itself.
pub(crate) fn standard_successor_endpoint_points(
    supports: &[crate::families::standard::records::StandardCurveSupport],
    vertex_roster: &[u32],
) -> Vec<[Option<usize>; 2]> {
    let point_by_identity = vertex_roster
        .iter()
        .copied()
        .enumerate()
        .map(|(point, identity)| (identity, point))
        .collect::<HashMap<_, _>>();
    supports
        .iter()
        .map(|support| {
            [
                support
                    .tag
                    .checked_add(1)
                    .and_then(|identity| point_by_identity.get(&identity).copied()),
                support
                    .tag
                    .checked_add(2)
                    .and_then(|identity| point_by_identity.get(&identity).copied()),
            ]
        })
        .collect()
}

pub(crate) fn corroborate_successor_endpoint_points(
    options: &mut [Vec<[usize; 2]>],
    points: &[[Option<usize>; 2]],
) {
    for (options, points) in options.iter_mut().zip(points) {
        for point in points.iter().flatten() {
            if options.iter().any(|pair| pair.contains(point)) {
                options.retain(|pair| pair.contains(point));
            }
        }
    }
}

pub(crate) fn unique_native_identity_points(
    identities: &[u32],
    coordinates: &[[f64; 3]],
    raw_point_count: usize,
    tolerances: &BTreeMap<usize, f64>,
    points: &[Point],
) -> HashMap<u32, usize> {
    const MATCH_TOLERANCE: f64 = 2e-3;

    identities
        .iter()
        .copied()
        .zip(coordinates)
        .enumerate()
        .filter_map(|(rank, (identity, coordinate))| {
            let tolerance = tolerances
                .get(&(raw_point_count + rank))
                .copied()
                .unwrap_or(MATCH_TOLERANCE)
                .max(MATCH_TOLERANCE);
            let matches = points
                .iter()
                .enumerate()
                .filter_map(|(index, point)| {
                    (point
                        .position
                        .distance_squared(Point3::new(coordinate[0], coordinate[1], coordinate[2]))
                        .sqrt()
                        <= tolerance)
                        .then_some(index)
                })
                .collect::<Vec<_>>();
            <[usize; 1]>::try_from(matches)
                .ok()
                .map(|[point]| (identity, point))
        })
        .collect()
}

pub(crate) fn intersection_line_direction(
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
) -> Option<Vector3> {
    const ANGULAR_TOLERANCE: f64 = 1e-9;

    match (left, right) {
        (
            SurfaceGeometry::Plane { normal: left, .. },
            SurfaceGeometry::Plane { normal: right, .. },
        ) => {
            let direction = (*left).cross(*right);
            let norm = direction.x.hypot(direction.y).hypot(direction.z);
            (norm.is_finite() && norm != 0.0).then_some(direction)
        }
        (SurfaceGeometry::Plane { normal, .. }, SurfaceGeometry::Cylinder { axis, .. })
        | (SurfaceGeometry::Cylinder { axis, .. }, SurfaceGeometry::Plane { normal, .. }) => {
            ((*normal).dot(*axis).abs() <= ANGULAR_TOLERANCE).then_some(*axis)
        }
        (
            SurfaceGeometry::Cylinder {
                axis: left_axis, ..
            },
            SurfaceGeometry::Cylinder {
                axis: right_axis, ..
            },
        ) => ((*left_axis).cross(*right_axis).norm() <= ANGULAR_TOLERANCE).then_some(*left_axis),
        _ => None,
    }
}

/// A line on one right circular or elliptical cone is a generator through its
/// apex. Same-carrier line rows have no surface-intersection direction, so
/// their endpoint relation needs this independent straight-branch predicate.
pub(crate) fn same_cone_generator_pair(
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
    start: Point3,
    end: Point3,
) -> bool {
    if left != right {
        return false;
    }
    let SurfaceGeometry::Cone {
        origin,
        axis,
        radius,
        half_angle,
        ..
    } = left
    else {
        return false;
    };
    let tangent = half_angle.tan();
    if !tangent.is_finite() || tangent == 0.0 {
        return false;
    }
    let apex_offset = -*radius / tangent;
    if !apex_offset.is_finite() {
        return false;
    }
    let apex = Point3::new(
        origin.x + apex_offset * axis.x,
        origin.y + apex_offset * axis.y,
        origin.z + apex_offset * axis.z,
    );
    if ![apex.x, apex.y, apex.z].into_iter().all(f64::is_finite) {
        return false;
    }
    let segment = end.vector_from(start);
    let segment_length = segment.norm();
    if !segment_length.is_finite() || segment_length == 0.0 {
        return false;
    }
    if start.distance(apex) <= EPS_SAME_CONE_GENERATOR
        || end.distance(apex) <= EPS_SAME_CONE_GENERATOR
    {
        return true;
    }
    let line_distance = start.vector_from(apex).cross(segment).norm() / segment_length;
    line_distance.is_finite() && line_distance <= EPS_SAME_CONE_GENERATOR
}

/// Collect plane normals only from trim-packet frame vectors, which carry the
/// stored normal's signed sense. A target with conflicting frame vectors stays
/// unresolved.
pub(crate) fn standard_plane_normals_from_face_frames(
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    face_frame_vectors: &[Option<[f64; 3]>],
) -> HashMap<u32, [f64; 3]> {
    let mut candidates = HashMap::<u32, Option<[f64; 3]>>::new();
    for (face, record) in records.iter().enumerate() {
        let crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) = record
        else {
            continue;
        };
        if prefix.kind != 0x32 {
            continue;
        }
        let Some(normal) = face_frame_vectors.get(face).copied().flatten() else {
            continue;
        };
        candidates
            .entry(prefix.target)
            .and_modify(|stored| {
                if stored.is_some_and(|stored| stored != normal) {
                    *stored = None;
                }
            })
            .or_insert(Some(normal));
    }
    candidates
        .into_iter()
        .filter_map(|(target, normal)| normal.map(|normal| (target, normal)))
        .collect()
}

pub(crate) fn face_surface<'a>(
    ir: &'a CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    face: usize,
) -> Option<&'a Surface> {
    let id = &bindings.get(face)?.0;
    ir.model.surfaces.get(*surface_indices.get(id)?)
}

/// Cache the exact face-membership predicate used by endpoint search.
///
/// Face geometry and standard face bounds are immutable while a topology
/// candidate is searched. The cache changes only lookup cost; allocation
/// failure returns `None`, and callers retain the original predicate.
pub(crate) fn standard_face_point_membership(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    face_bounds: Option<&[Option<crate::families::standard::records::StandardFaceBounds>]>,
) -> Option<Vec<Vec<bool>>> {
    bindings
        .iter()
        .enumerate()
        .map(|(face, _)| {
            let surface = face_surface(ir, bindings, surface_indices, face)?;
            let bounds = face_bounds
                .and_then(|bounds| bounds.get(face).copied())
                .flatten();
            let mut membership =
                alloc_filled(ir.model.points.len(), false, "catia_face_point_membership").ok()?;
            for (point, candidate) in ir.model.points.iter().enumerate() {
                membership[point] =
                    point_on_standard_face(candidate.position, &surface.geometry, bounds);
            }
            Some(membership)
        })
        .collect()
}

pub(crate) fn point_on_standard_face(
    point: Point3,
    surface: &SurfaceGeometry,
    bounds: Option<crate::families::standard::records::StandardFaceBounds>,
) -> bool {
    if bounds.is_some_and(|bounds| !point_inside_standard_face_bounds(point, bounds)) {
        return false;
    }
    point_on_surface_if_supported(point, surface) != Some(false)
}

fn point_inside_standard_face_bounds(
    point: Point3,
    bounds: crate::families::standard::records::StandardFaceBounds,
) -> bool {
    let coordinates = [point.x, point.y, point.z];
    let inside_aabb = coordinates.iter().enumerate().all(|(axis, coordinate)| {
        (*coordinate - bounds.aabb_center[axis]).abs()
            <= bounds.aabb_half_extents[axis] + STANDARD_FACE_BOUNDS_TOLERANCE
    });
    let distance_squared = coordinates
        .iter()
        .enumerate()
        .map(|(axis, coordinate)| (*coordinate - bounds.sphere_center[axis]).powi(2))
        .sum::<f64>();
    inside_aabb && distance_squared.sqrt() <= bounds.sphere_radius + STANDARD_FACE_BOUNDS_TOLERANCE
}

/// Narrow a repeated-face domain only when one alternate has a strictly larger
/// circular-carrier or positive-dimensional AABB relation with the serialized
/// face.
///
/// A real shared surface boundary must have a positive overlap along at least
/// one world axis. The overlap dimension is deliberately used as a partial
/// order after the distinct-carrier rank for circular supports: ties remain
/// domains, so this helper cannot choose between symmetric or insufficiently
/// bounded incidences.
pub(crate) fn refine_repeated_face_domains_by_geometry_and_bounds(
    edge_faces: &[[usize; 2]],
    allowed_faces: &mut [Vec<usize>],
    face_bounds: Option<&[Option<crate::families::standard::records::StandardFaceBounds>]>,
    face_geometries: Option<&[SurfaceGeometry]>,
    edge_geometries: &[crate::families::standard::records::StandardCurveGeometry],
) {
    let Some(face_bounds) = face_bounds else {
        return;
    };
    for (edge, alternatives) in allowed_faces.iter_mut().enumerate() {
        if alternatives.is_empty() {
            continue;
        }
        let Some([serialized_face, repeated_face]) = edge_faces.get(edge).copied() else {
            continue;
        };
        if serialized_face != repeated_face {
            continue;
        }
        let Some(serialized_bounds) = face_bounds.get(serialized_face).copied().flatten() else {
            continue;
        };
        if alternatives
            .iter()
            .any(|face| face_bounds.get(*face).copied().flatten().is_none())
        {
            continue;
        }
        let circular_support = matches!(
            edge_geometries.get(edge),
            Some(crate::families::standard::records::StandardCurveGeometry::Circle { .. })
        );
        if circular_support
            && face_geometries.is_none_or(|geometries| {
                std::iter::once(serialized_face)
                    .chain(alternatives.iter().copied())
                    .any(|face| {
                        matches!(geometries.get(face), Some(SurfaceGeometry::Unknown { .. }))
                    })
            })
        {
            continue;
        }
        let scores = alternatives
            .iter()
            .copied()
            .map(|face| {
                let distinct_circle_carrier = circular_support
                    && face_geometries.is_some_and(|geometries| {
                        geometries
                            .get(serialized_face)
                            .zip(geometries.get(face))
                            .is_some_and(|(left, right)| left != right)
                    });
                let overlap_dimension =
                    face_bounds
                        .get(face)
                        .copied()
                        .flatten()
                        .map_or(0, |candidate| {
                            (0..3)
                                .filter(|axis| {
                                    let left = serialized_bounds.aabb_center[*axis]
                                        - serialized_bounds.aabb_half_extents[*axis];
                                    let right = serialized_bounds.aabb_center[*axis]
                                        + serialized_bounds.aabb_half_extents[*axis];
                                    let candidate_left = candidate.aabb_center[*axis]
                                        - candidate.aabb_half_extents[*axis];
                                    let candidate_right = candidate.aabb_center[*axis]
                                        + candidate.aabb_half_extents[*axis];
                                    right.min(candidate_right) - left.max(candidate_left)
                                        > STANDARD_FACE_BOUNDS_TOLERANCE
                                })
                                .count() as u8
                        });
                (face, (distinct_circle_carrier as u8, overlap_dimension))
            })
            .collect::<Vec<_>>();
        let best = scores
            .iter()
            .map(|(_, score)| *score)
            .max()
            .unwrap_or((0, 0));
        if best == (0, 0) || scores.iter().filter(|(_, score)| *score == best).count() != 1 {
            continue;
        }
        alternatives.retain(|face| {
            scores
                .iter()
                .any(|(candidate, score)| candidate == face && *score == best)
        });
    }
}

fn standard_nurbs_line_pair_on_face(
    surface: &SurfaceGeometry,
    support: &crate::families::standard::records::StandardCurveSupport,
    pair: &[usize; 2],
    points: &[Point],
    bounds: Option<crate::families::standard::records::StandardFaceBounds>,
) -> bool {
    if !matches!(surface, SurfaceGeometry::Nurbs(_))
        || !matches!(
            support.geometry,
            crate::families::standard::records::StandardCurveGeometry::Line
        )
    {
        return true;
    }
    let Some(start) = points.get(pair[0]).map(|point| point.position) else {
        return false;
    };
    let Some(end) = points.get(pair[1]).map(|point| point.position) else {
        return false;
    };
    NURBS_LINE_FACE_SAMPLES.iter().all(|fraction| {
        let point = Point3::new(
            start.x + fraction * (end.x - start.x),
            start.y + fraction * (end.y - start.y),
            start.z + fraction * (end.z - start.z),
        );
        point_on_standard_face(point, surface, bounds)
    })
}

fn nurbs_surface_control_bounds(surface: &NurbsSurface) -> Option<[[f64; 2]; 3]> {
    if surface.weights.as_ref().is_some_and(|weights| {
        weights.len() != surface.control_points.len()
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
    }) {
        return None;
    }
    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for point in &surface.control_points {
        for (axis, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
            if !coordinate.is_finite() {
                return None;
            }
            bounds[axis][0] = bounds[axis][0].min(coordinate);
            bounds[axis][1] = bounds[axis][1].max(coordinate);
        }
    }
    bounds
        .iter()
        .all(|[lower, upper]| lower.is_finite() && upper.is_finite() && lower <= upper)
        .then_some(bounds)
}

fn nurbs_surface_parameter_domain(surface: &NurbsSurface) -> Option<[[f64; 2]; 2]> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if u_count <= u_degree
        || v_count <= v_degree
        || surface.control_points.len() != u_count.checked_mul(v_count)?
    {
        return None;
    }
    let domains = [
        [
            *surface.u_knots.get(u_degree)?,
            *surface.u_knots.get(u_count)?,
        ],
        [
            *surface.v_knots.get(v_degree)?,
            *surface.v_knots.get(v_count)?,
        ],
    ];
    domains
        .into_iter()
        .all(|[lower, upper]| lower.is_finite() && upper.is_finite() && lower < upper)
        .then_some(domains)
}

fn reverse_nurbs_curve(curve: &NurbsCurve) -> Option<NurbsCurve> {
    let [lower, upper] = cadmpeg_ir::eval::nurbs_curve_parameter_domain(curve)?;
    let sum = lower + upper;
    if !sum.is_finite() {
        return None;
    }
    let knots = curve
        .knots
        .iter()
        .rev()
        .map(|knot| sum - knot)
        .collect::<Vec<_>>();
    knots
        .iter()
        .copied()
        .all(f64::is_finite)
        .then_some(NurbsCurve {
            degree: curve.degree,
            knots,
            control_points: curve.control_points.iter().rev().copied().collect(),
            weights: curve
                .weights
                .as_ref()
                .map(|weights| weights.iter().rev().copied().collect()),
            periodic: curve.periodic,
        })
}

fn nurbs_shared_boundary_scalar_matches(left: f64, right: f64) -> bool {
    left.is_finite()
        && right.is_finite()
        && (left - right).abs()
            <= NURBS_SHARED_BOUNDARY_TOLERANCE * left.abs().max(right.abs()).max(1.0)
}

fn nurbs_shared_boundary_curves_match(left: &NurbsCurve, right: &NurbsCurve) -> bool {
    let same_payload = |left: &NurbsCurve, right: &NurbsCurve| {
        left.degree == right.degree
            && left.periodic == right.periodic
            && left.knots.len() == right.knots.len()
            && left
                .knots
                .iter()
                .zip(&right.knots)
                .all(|(left, right)| nurbs_shared_boundary_scalar_matches(*left, *right))
            && left.control_points.len() == right.control_points.len()
            && left
                .control_points
                .iter()
                .zip(&right.control_points)
                .all(|(left, right)| {
                    [left.x, left.y, left.z]
                        .into_iter()
                        .zip([right.x, right.y, right.z])
                        .all(|(left, right)| nurbs_shared_boundary_scalar_matches(left, right))
                })
            && match (&left.weights, &right.weights) {
                (None, None) => true,
                (Some(left), Some(right)) => {
                    left.len() == right.len()
                        && left.iter().zip(right).all(|(left, right)| {
                            nurbs_shared_boundary_scalar_matches(*left, *right)
                        })
                }
                _ => false,
            }
    };
    same_payload(left, right)
        || reverse_nurbs_curve(right).is_some_and(|reversed| same_payload(left, &reversed))
}

fn nurbs_surface_boundary_curves(surface: &NurbsSurface) -> Option<[NurbsCurve; 4]> {
    let [[u_lower, u_upper], [v_lower, v_upper]] = nurbs_surface_parameter_domain(surface)?;
    Some([
        cadmpeg_ir::eval::nurbs_surface_isocurve(
            surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::U,
            u_lower,
        )?,
        cadmpeg_ir::eval::nurbs_surface_isocurve(
            surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::U,
            u_upper,
        )?,
        cadmpeg_ir::eval::nurbs_surface_isocurve(
            surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::V,
            v_lower,
        )?,
        cadmpeg_ir::eval::nurbs_surface_isocurve(
            surface,
            cadmpeg_ir::geometry::SurfaceParameterAxis::V,
            v_upper,
        )?,
    ])
}

fn nurbs_boundary_contains_point(curve: &NurbsCurve, point: Point3) -> bool {
    let Some([lower, upper]) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(curve) else {
        return false;
    };
    [lower, 0.5 * (lower + upper), upper]
        .into_iter()
        .any(|seed| {
            cadmpeg_ir::eval::nurbs_curve_parameter_near_point(
                curve,
                point,
                NURBS_SURFACE_MEMBERSHIP_TOLERANCE,
                seed,
            )
            .is_some()
        })
}

/// Return endpoint pairs that lie on an exact shared NURBS carrier boundary.
///
/// A shared boundary is a positive relation between two tensor-product
/// carriers. It is not inferred from carrier AABBs or from a sampled surface
/// intersection. `None` means that the relation is unavailable; `Some` may be
/// empty when the relation is present but no supplied pair lies on it.
pub(crate) fn standard_shared_nurbs_boundary_pair_options(
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
    points: &[Point3],
    options: &[[usize; 2]],
) -> Option<Vec<[usize; 2]>> {
    let (SurfaceGeometry::Nurbs(left), SurfaceGeometry::Nurbs(right)) = (left, right) else {
        return None;
    };
    let left_boundaries = nurbs_surface_boundary_curves(left)?;
    let right_boundaries = nurbs_surface_boundary_curves(right)?;
    let shared_boundaries = left_boundaries
        .iter()
        .filter(|left| {
            right_boundaries
                .iter()
                .any(|right| nurbs_shared_boundary_curves_match(left, right))
        })
        .collect::<Vec<_>>();
    if shared_boundaries.is_empty() {
        return None;
    }
    Some(
        options
            .iter()
            .copied()
            .filter(|pair| {
                shared_boundaries.iter().any(|boundary| {
                    pair.iter().all(|point| {
                        points
                            .get(*point)
                            .is_some_and(|point| nurbs_boundary_contains_point(boundary, *point))
                    })
                })
            })
            .collect(),
    )
}

fn standard_shared_boundary_group_domains(
    supports: &[crate::families::standard::records::StandardCurveSupport],
    original: &[Vec<[usize; 2]>],
    filtered: &mut [Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
    boundary_witnesses: &[bool],
) {
    if supports.len() != original.len()
        || supports.len() != filtered.len()
        || supports.len() != edge_identity_evidence.len()
        || supports.len() != boundary_witnesses.len()
    {
        return;
    }
    // A shared carrier boundary is positive endpoint evidence, not a row
    // identity. Repeated rows can include trimmed boundaries in the carrier
    // interior. If any row lacks a non-empty witness relation, or if the
    // retained witness pairs cannot cover the complete repeated-row group,
    // preserve every original domain and defer the row assignment to the
    // admitted port and trim relations.
    let mut groups = HashMap::<[usize; 2], Vec<usize>>::new();
    for (edge, support) in supports.iter().enumerate() {
        if edge_identity_evidence[edge]
            || !matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Bspline
            )
            || support.faces[0] == support.faces[1]
            || original[edge].is_empty()
        {
            continue;
        }
        let mut faces = support.faces;
        faces.sort_unstable();
        groups.entry(faces).or_default().push(edge);
    }
    for edges in groups.into_values() {
        if edges.len() < 2 {
            continue;
        }
        if edges.iter().any(|edge| !boundary_witnesses[*edge]) {
            for edge in edges {
                filtered[edge].clone_from(&original[edge]);
            }
            continue;
        }
        let filtered_pairs = edges
            .iter()
            .flat_map(|edge| filtered[*edge].iter().copied())
            .map(|mut pair| {
                pair.sort_unstable();
                pair
            })
            .collect::<HashSet<_>>();
        if filtered_pairs.len() >= edges.len() {
            continue;
        }
        for edge in edges {
            filtered[edge].clone_from(&original[edge]);
        }
    }
}

fn standard_endpoint_options_for_selected_faces(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    points: &[Point3],
    options: &[Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
) -> Vec<Vec<[usize; 2]>> {
    let (mut filtered_options, boundary_witnesses): (Vec<Vec<[usize; 2]>>, Vec<bool>) = supports
        .iter()
        .enumerate()
        .map(|(edge, support)| {
            let Some(pairs) = options.get(edge) else {
                return (Vec::new(), false);
            };
            if edge_identity_evidence.get(edge).copied().unwrap_or(false)
                || !matches!(
                    support.geometry,
                    crate::families::standard::records::StandardCurveGeometry::Bspline
                )
                || support.faces[0] == support.faces[1]
            {
                return (pairs.clone(), false);
            }
            let Some(left) = face_surface(ir, bindings, surface_indices, support.faces[0]) else {
                return (pairs.clone(), false);
            };
            let Some(right) = face_surface(ir, bindings, surface_indices, support.faces[1]) else {
                return (pairs.clone(), false);
            };
            let filtered = standard_shared_nurbs_boundary_pair_options(
                &left.geometry,
                &right.geometry,
                points,
                pairs,
            );
            match filtered {
                Some(filtered) if !filtered.is_empty() => (filtered, true),
                _ => (pairs.clone(), false),
            }
        })
        .unzip();
    standard_shared_boundary_group_domains(
        supports,
        options,
        &mut filtered_options,
        edge_identity_evidence,
        &boundary_witnesses,
    );
    filtered_options
}

fn nurbs_surface_axis_samples(knots: &[f64], degree: usize, count: usize) -> Option<Vec<f64>> {
    let mut boundaries = Vec::new();
    for &knot in knots.get(degree..=count)? {
        if boundaries.last().is_none_or(|previous| *previous != knot) {
            boundaries.push(knot);
        }
    }
    let mut samples = Vec::new();
    for pair in boundaries.windows(2) {
        let [lower, upper] = *pair else {
            continue;
        };
        if !lower.is_finite() || !upper.is_finite() || lower >= upper {
            continue;
        }
        for step in 0..NURBS_SURFACE_SEEDS_PER_SPAN {
            let fraction = step as f64 / (NURBS_SURFACE_SEEDS_PER_SPAN - 1) as f64;
            samples.push(lower + fraction * (upper - lower));
        }
    }
    (!samples.is_empty()).then_some(samples)
}

fn nurbs_surface_start_grid(surface: &NurbsSurface, domains: [[f64; 2]; 2]) -> Option<Vec<Point2>> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let u_samples = nurbs_surface_axis_samples(&surface.u_knots, u_degree, u_count)?;
    let v_samples = nurbs_surface_axis_samples(&surface.v_knots, v_degree, v_count)?;
    if u_samples.len().checked_mul(v_samples.len())? > NURBS_SURFACE_MAX_SEEDS {
        let side = (NURBS_SURFACE_MAX_SEEDS as f64).sqrt() as usize;
        let mut grid = Vec::with_capacity(side * side);
        for u in 0..side {
            for v in 0..side {
                let u_fraction = u as f64 / (side - 1) as f64;
                let v_fraction = v as f64 / (side - 1) as f64;
                grid.push(Point2::new(
                    domains[0][0] + u_fraction * (domains[0][1] - domains[0][0]),
                    domains[1][0] + v_fraction * (domains[1][1] - domains[1][0]),
                ));
            }
        }
        return Some(grid);
    }
    Some(
        u_samples
            .into_iter()
            .flat_map(|u| v_samples.iter().copied().map(move |v| Point2::new(u, v)))
            .collect(),
    )
}

fn nurbs_surface_point_distance_squared(
    surface: &NurbsSurface,
    point: Point3,
    uv: Point2,
) -> Option<f64> {
    let position = cadmpeg_ir::eval::nurbs_surface_point(surface, uv.u, uv.v)?;
    let distance = position.vector_from(point);
    let squared = distance.dot(distance);
    squared.is_finite().then_some(squared)
}

fn refine_nurbs_surface_point(
    surface: &NurbsSurface,
    point: Point3,
    seed: Point2,
    domains: [[f64; 2]; 2],
) -> Option<f64> {
    let mut parameters = seed;
    for _ in 0..NURBS_SURFACE_REFINEMENT_ITERATIONS {
        let partials =
            cadmpeg_ir::eval::nurbs_surface_partials(surface, parameters.u, parameters.v)?;
        let residual = partials.point.vector_from(point);
        let du_squared = partials.du.dot(partials.du);
        let mixed = partials.du.dot(partials.dv);
        let dv_squared = partials.dv.dot(partials.dv);
        let determinant = du_squared * dv_squared - mixed * mixed;
        if !determinant.is_finite()
            || determinant.abs() <= f64::EPSILON * du_squared.max(dv_squared).powi(2)
        {
            break;
        }
        let du_residual = partials.du.dot(residual);
        let dv_residual = partials.dv.dot(residual);
        let step = Point2::new(
            (dv_squared * du_residual - mixed * dv_residual) / determinant,
            (du_squared * dv_residual - mixed * du_residual) / determinant,
        );
        let current = nurbs_surface_point_distance_squared(surface, point, parameters)?;
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..NURBS_SURFACE_BACKTRACK_STEPS {
            let candidate = Point2::new(
                (parameters.u - scale * step.u).clamp(domains[0][0], domains[0][1]),
                (parameters.v - scale * step.v).clamp(domains[1][0], domains[1][1]),
            );
            let distance = nurbs_surface_point_distance_squared(surface, point, candidate)?;
            if distance <= current {
                accepted = Some((candidate, distance));
                break;
            }
            scale *= 0.5;
        }
        let Some((candidate, distance)) = accepted else {
            break;
        };
        parameters = candidate;
        if distance <= NURBS_SURFACE_MEMBERSHIP_TOLERANCE.powi(2) {
            return Some(distance);
        }
    }
    nurbs_surface_point_distance_squared(surface, point, parameters)
}

fn nurbs_surface_witness_distance(surface: &NurbsSurface, point: Point3) -> Option<f64> {
    let domains = nurbs_surface_parameter_domain(surface)?;
    let starts = nurbs_surface_start_grid(surface, domains)?;
    starts
        .into_iter()
        .filter_map(|seed| refine_nurbs_surface_point(surface, point, seed, domains))
        .min_by(f64::total_cmp)
}

fn point_on_nurbs_surface(point: Point3, surface: &NurbsSurface) -> Option<bool> {
    // A positive-weight NURBS control net bounds the surface, so its AABB is a
    // sound negative test.  The bounded parameter search supplies positive
    // witnesses only.  A failed search inside that AABB is unknown, not proof
    // that the point is off the surface.
    if let Some(bounds) = nurbs_surface_control_bounds(surface) {
        let outside =
            [point.x, point.y, point.z]
                .into_iter()
                .enumerate()
                .any(|(axis, coordinate)| {
                    coordinate < bounds[axis][0] - NURBS_SURFACE_MEMBERSHIP_TOLERANCE
                        || coordinate > bounds[axis][1] + NURBS_SURFACE_MEMBERSHIP_TOLERANCE
                });
        if outside {
            return Some(false);
        }
    }
    let distance = nurbs_surface_witness_distance(surface, point)?;
    (distance <= NURBS_SURFACE_MEMBERSHIP_TOLERANCE.powi(2)).then_some(true)
}

fn invariant_face_carrier_bindings(
    face_edges: &[Vec<(usize, Vec<usize>)>],
    owner_count: usize,
    budget: Option<&WorkBudget<'_>>,
) -> Option<Vec<Option<usize>>> {
    let normalized = face_edges
        .iter()
        .map(|edges| {
            let mut by_owner = BTreeMap::<usize, HashSet<usize>>::new();
            for (owner, carriers) in edges {
                if *owner >= owner_count || carriers.is_empty() {
                    continue;
                }
                by_owner
                    .entry(*owner)
                    .or_default()
                    .extend(carriers.iter().copied());
            }
            by_owner
        })
        .collect::<Vec<_>>();
    let mut domains = normalized
        .iter()
        .map(|edges| edges.keys().copied().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let matching = distinct_domain_matching_with_budget(
        domains.iter().map(Vec::as_slice),
        owner_count,
        budget,
        None,
    )?;
    retain_distinct_matching_supports(&mut domains, owner_count, &matching, budget)?;
    Some(
        domains
            .iter()
            .zip(&normalized)
            .map(|(owners, labels)| {
                let carriers = owners
                    .iter()
                    .filter_map(|owner| labels.get(owner))
                    .flatten()
                    .copied()
                    .collect::<HashSet<_>>();
                if carriers.len() != 1 {
                    return None;
                }
                carriers.into_iter().next()
            })
            .collect(),
    )
}

fn owner_matches_a5_carrier(
    tail: &crate::families::b2::records::B2OwnerNumericTail,
    surface: &NurbsSurface,
) -> bool {
    let Some(domain) = nurbs_surface_parameter_domain(surface) else {
        return false;
    };
    if (0..2).any(|axis| {
        tail.lower[axis] < domain[axis][0] - NURBS_SURFACE_MEMBERSHIP_TOLERANCE
            || tail.upper[axis] > domain[axis][1] + NURBS_SURFACE_MEMBERSHIP_TOLERANCE
    }) {
        return false;
    }
    [tail.lower[0], tail.upper[0]].into_iter().all(|u| {
        [tail.lower[1], tail.upper[1]].into_iter().all(|v| {
            cadmpeg_ir::eval::nurbs_surface_point(surface, u, v).is_some_and(|point| {
                [point.x, point.y, point.z]
                    .into_iter()
                    .enumerate()
                    .all(|(axis, value)| {
                        value
                            >= f64::from(tail.bounds[axis][0]) - NURBS_SURFACE_MEMBERSHIP_TOLERANCE
                            && value
                                <= f64::from(tail.bounds[axis][1])
                                    + NURBS_SURFACE_MEMBERSHIP_TOLERANCE
                    })
            })
        })
    })
}

fn owner_contains_face_bounds(
    tail: &crate::families::b2::records::B2OwnerNumericTail,
    bounds: crate::families::standard::records::StandardFaceBounds,
) -> bool {
    (0..3).all(|axis| {
        let lower = bounds.aabb_center[axis] - bounds.aabb_half_extents[axis];
        let upper = bounds.aabb_center[axis] + bounds.aabb_half_extents[axis];
        lower >= f64::from(tail.bounds[axis][0]) - NURBS_SURFACE_MEMBERSHIP_TOLERANCE
            && upper <= f64::from(tail.bounds[axis][1]) + NURBS_SURFACE_MEMBERSHIP_TOLERANCE
    })
}

fn standard_face_boundary_witnesses(ir: &CadIr) -> Vec<Vec<Point3>> {
    let point_positions = ir
        .model
        .points
        .iter()
        .map(|point| (point.id.clone(), point.position))
        .collect::<HashMap<_, _>>();
    let vertex_positions = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| Some((vertex.id.clone(), *point_positions.get(&vertex.point)?)))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.clone(), edge))
        .collect::<HashMap<_, _>>();
    let coedges = ir
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.clone(), coedge))
        .collect::<HashMap<_, _>>();
    let loops = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.clone(), loop_))
        .collect::<HashMap<_, _>>();
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.clone(), curve))
        .collect::<HashMap<_, _>>();
    ir.model
        .faces
        .iter()
        .map(|face| {
            let mut witnesses = Vec::new();
            for edge in face
                .loops
                .iter()
                .filter_map(|id| loops.get(id))
                .flat_map(|loop_| &loop_.coedges)
                .filter_map(|id| coedges.get(id))
                .filter_map(|coedge| edges.get(&coedge.edge))
            {
                witnesses.extend(
                    [&edge.start, &edge.end]
                        .into_iter()
                        .filter_map(|id| vertex_positions.get(id).copied()),
                );
                let Some((curve, [start, end])) = edge
                    .curve
                    .as_ref()
                    .and_then(|id| curves.get(id))
                    .zip(edge.param_range)
                else {
                    continue;
                };
                if let Some(point) =
                    cadmpeg_ir::eval::curve_point(&curve.geometry, 0.5 * (start + end))
                {
                    witnesses.push(point);
                }
            }
            let mut distinct = Vec::<Point3>::new();
            for point in witnesses {
                if distinct
                    .iter()
                    .all(|stored| stored.distance(point) > NURBS_SURFACE_MEMBERSHIP_TOLERANCE)
                {
                    distinct.push(point);
                }
            }
            distinct
        })
        .collect()
}

fn bind_standard_a5_owner_surfaces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
    records: &[ConsolidatedRecord],
    face_bounds: &[Option<crate::families::standard::records::StandardFaceBounds>],
    budget: &WorkBudget<'_>,
) -> usize {
    let carriers = crate::families::a5a8::records::a5_surfaces_from_records(data, records);
    let owners = crate::families::b2::records::b2_owner_packets_from_records(data, records);
    if carriers.is_empty() || owners.is_empty() || ir.model.faces.is_empty() {
        return 0;
    }
    let owner_carriers = owners
        .iter()
        .map(|owner| {
            carriers
                .iter()
                .enumerate()
                .filter_map(|(carrier, value)| {
                    let SurfaceGeometry::Nurbs(surface) = &value.geometry else {
                        return None;
                    };
                    owner_matches_a5_carrier(&owner.numeric_tail, surface).then_some(carrier)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let witnesses = standard_face_boundary_witnesses(ir);
    let surface_indices = ir
        .model
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| (surface.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let unknown_faces = ir
        .model
        .faces
        .iter()
        .enumerate()
        .filter_map(|(face, value)| {
            let ordinal = value
                .id
                .0
                .strip_prefix("catia:standard:face#")?
                .parse::<usize>()
                .ok()?;
            let surface = *surface_indices.get(&value.surface)?;
            matches!(
                ir.model.surfaces[surface].geometry,
                SurfaceGeometry::Unknown { .. }
            )
            .then_some((face, ordinal, surface))
        })
        .collect::<Vec<_>>();
    let mut face_edges = Vec::with_capacity(unknown_faces.len());
    for &(face, ordinal, _) in &unknown_faces {
        let Some(Some(bounds)) = face_bounds.get(ordinal) else {
            face_edges.push(Vec::new());
            continue;
        };
        let containing_owners = owners
            .iter()
            .enumerate()
            .filter_map(|(owner, value)| {
                (!owner_carriers[owner].is_empty()
                    && owner_contains_face_bounds(&value.numeric_tail, *bounds))
                .then_some(owner)
            })
            .collect::<Vec<_>>();
        let possible_carriers = containing_owners
            .iter()
            .flat_map(|owner| owner_carriers[*owner].iter().copied())
            .collect::<HashSet<_>>();
        let face_carriers = possible_carriers
            .into_iter()
            .filter(|carrier| {
                let SurfaceGeometry::Nurbs(surface) = &carriers[*carrier].geometry else {
                    return false;
                };
                witnesses.get(face).is_some_and(|points| {
                    points.len() >= 3
                        && points
                            .iter()
                            .all(|point| point_on_nurbs_surface(*point, surface) == Some(true))
                })
            })
            .collect::<HashSet<_>>();
        face_edges.push(
            containing_owners
                .into_iter()
                .filter_map(|owner| {
                    let labels = owner_carriers[owner]
                        .iter()
                        .filter(|carrier| face_carriers.contains(carrier))
                        .copied()
                        .collect::<Vec<_>>();
                    (!labels.is_empty()).then_some((owner, labels))
                })
                .collect(),
        );
    }
    let Some(bindings) = invariant_face_carrier_bindings(&face_edges, owners.len(), Some(budget))
    else {
        return 0;
    };
    let mut bound = 0;
    for ((_, _, surface), carrier) in unknown_faces.into_iter().zip(bindings) {
        let Some(carrier) = carrier else {
            continue;
        };
        ir.model.surfaces[surface].geometry = carriers[carrier].geometry.clone();
        annotations.derived(&ir.model.surfaces[surface].id, "geometry");
        bound += 1;
    }
    bound
}

/// Keep a topological endpoint pair when p-curve derivation cannot prove it.
///
/// The endpoint domain is an input to exact trim-cycle and port-identity
/// solving. P-curve construction is a later, optional emission step. A
/// spherical face can carry a non-isoparametric circular section; the generic
/// UV-midpoint p-curve test cannot derive that section from its endpoints, but
/// the serialized circle carrier and face membership still make the pair
/// admissible for topology solving.
pub(super) fn standard_endpoint_pair_supports_topology(
    surface: &SurfaceGeometry,
    support: &crate::families::standard::records::StandardCurveSupport,
    start: Point3,
    end: Point3,
    witness: Option<Point3>,
) -> bool {
    let endpoint_is_supported = |point| match surface {
        SurfaceGeometry::Nurbs(_) => point_on_surface_if_supported(point, surface) != Some(false),
        _ => point_on_surface(point, surface),
    };
    if !endpoint_is_supported(start) || !endpoint_is_supported(end) {
        return false;
    }
    if standard_pcurve_geometry(surface, support, start, end, witness, None).is_some() {
        return true;
    }
    if matches!(surface, SurfaceGeometry::Nurbs(_)) {
        // A bounded model-space NURBS search may remain unknown inside the
        // control-net bound.  Topology retains that pair; a UV p-curve is
        // optional and is derived only from an admitted parameterization.
        return true;
    }
    matches!(
        (surface, &support.geometry),
        (
            SurfaceGeometry::Sphere { .. },
            crate::families::standard::records::StandardCurveGeometry::Circle { center, radius },
        ) if (start.distance(*center) - *radius).abs() <= SPHERE_SECTION_ENDPOINT_TOLERANCE
            && (end.distance(*center) - *radius).abs() <= SPHERE_SECTION_ENDPOINT_TOLERANCE
    )
}

pub(crate) fn standard_pcurve_geometry(
    surface: &SurfaceGeometry,
    support: &crate::families::standard::records::StandardCurveSupport,
    start: Point3,
    end: Point3,
    witness: Option<Point3>,
    edge_curve: Option<&CurveGeometry>,
) -> Option<(PcurveGeometry, [f64; 2])> {
    if matches!(edge_curve, Some(CurveGeometry::Unknown { .. })) {
        return None;
    }
    if !point_on_surface(start, surface) || !point_on_surface(end, surface) {
        return None;
    }
    let mut uv = [
        analytic_surface_uv(surface, start)?,
        analytic_surface_uv(surface, end)?,
    ];
    if let SurfaceGeometry::Cone {
        origin,
        axis,
        radius,
        half_angle,
        ..
    } = surface
    {
        let tangent = half_angle.tan();
        if tangent.is_finite() && tangent != 0.0 {
            let apex_offset = -*radius / tangent;
            if apex_offset.is_finite() {
                let apex = Point3::new(
                    origin.x + apex_offset * axis.x,
                    origin.y + apex_offset * axis.y,
                    origin.z + apex_offset * axis.z,
                );
                if start.distance_squared(apex) <= 1e-6 {
                    uv[0].u = uv[1].u;
                }
                if end.distance_squared(apex) <= 1e-6 {
                    uv[1].u = uv[0].u;
                }
            }
        }
    }
    let reference_uv = uv[0];
    unwrap_standard_uv(surface, &mut uv[1], reference_uv);

    if let (
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius },
        Some(witness),
    ) = (&support.geometry, witness)
    {
        if let Some(end) = witnessed_surface_circle_end(surface, *center, *radius, uv, witness) {
            uv[1] = end;
        }
    }

    if let (
        SurfaceGeometry::Plane { normal, .. },
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius },
    ) = (surface, &support.geometry)
    {
        const CIRCLE_TOLERANCE: f64 = 2e-3;
        let contained_carrier = point_on_surface(*center, surface)
            && (start.distance(*center) - *radius).abs() <= CIRCLE_TOLERANCE
            && (end.distance(*center) - *radius).abs() <= CIRCLE_TOLERANCE
            && edge_curve.is_none_or(|curve| {
                matches!(
                    curve,
                    CurveGeometry::Circle {
                        axis,
                        radius: curve_radius,
                        ..
                    } if axis.cross(*normal).norm() <= CIRCLE_TOLERANCE
                        && (*curve_radius - *radius).abs() <= CIRCLE_TOLERANCE
                )
            });
        if !contained_carrier {
            return None;
        }
        let center_uv = analytic_surface_uv(surface, *center)?;
        let range = if start == end {
            let angle = (uv[0].v - center_uv.v).atan2(uv[0].u - center_uv.u);
            [angle, angle + std::f64::consts::TAU]
        } else {
            let range = uv.map(|point| (point.v - center_uv.v).atan2(point.u - center_uv.u));
            ordered_range([range[0], unwrap_angle(range[1], range[0])])
        };
        let geometry = rational_pcurve_arc([center_uv.u, center_uv.v], *radius, range)?;
        return Some((geometry, range));
    }

    let direction = Point2::new(uv[1].u - uv[0].u, uv[1].v - uv[0].v);
    let midpoint_uv = Point2::new(uv[0].u + 0.5 * direction.u, uv[0].v + 0.5 * direction.v);
    let midpoint = cadmpeg_ir::eval::surface_point(surface, midpoint_uv.u, midpoint_uv.v)?;
    let on_curve = match &support.geometry {
        crate::families::standard::records::StandardCurveGeometry::Line => {
            let chord = end.vector_from(start);
            let offset = midpoint.vector_from(start);
            chord.cross(offset).norm() <= 2e-3 * chord.norm().max(1.0)
        }
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } => {
            (midpoint.distance_squared(*center).sqrt() - radius).abs() <= 2e-3
        }
        crate::families::standard::records::StandardCurveGeometry::Bspline => match edge_curve {
            Some(CurveGeometry::Line { origin, direction }) => {
                let offset = midpoint.vector_from(*origin);
                (*direction).cross(offset).norm() <= 2e-3 * (*direction).norm().max(1.0)
            }
            _ => false,
        },
    };
    on_curve.then_some((
        PcurveGeometry::Line {
            origin: uv[0],
            direction,
        },
        [0.0, 1.0],
    ))
}

pub(crate) fn witness_arc_end(start: f64, short_end: f64, witness: f64) -> Option<f64> {
    let delta = short_end - start;
    if delta == 0.0 {
        return None;
    }
    let long_end = short_end - delta.signum() * std::f64::consts::TAU;
    let contains = |end: f64| {
        (-2..=2).any(|turn| {
            let witness = witness + f64::from(turn) * std::f64::consts::TAU;
            witness > start.min(end) && witness < start.max(end)
        })
    };
    match (contains(short_end), contains(long_end)) {
        (true, false) => Some(short_end),
        (false, true) => Some(long_end),
        _ => None,
    }
}

pub(crate) fn witnessed_surface_circle_end(
    surface: &SurfaceGeometry,
    center: Point3,
    radius: f64,
    uv: [Point2; 2],
    witness: Point3,
) -> Option<Point2> {
    let witness_uv = analytic_surface_uv(surface, witness)?;
    let lanes: &[usize] = match surface {
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. } => &[0],
        SurfaceGeometry::Torus { .. } => &[0, 1],
        _ => return None,
    };
    let candidates = lanes
        .iter()
        .filter_map(|lane| {
            let mut candidate = uv[1];
            let (start, short_end, witness) = if *lane == 0 {
                (uv[0].u, uv[1].u, witness_uv.u)
            } else {
                (uv[0].v, uv[1].v, witness_uv.v)
            };
            let selected = witness_arc_end(start, short_end, witness)?;
            if *lane == 0 {
                candidate.u = selected;
            } else {
                candidate.v = selected;
            }
            let midpoint = cadmpeg_ir::eval::surface_point(
                surface,
                0.5 * (uv[0].u + candidate.u),
                0.5 * (uv[0].v + candidate.v),
            )?;
            ((midpoint.distance_squared(center).sqrt() - radius).abs() <= 2e-3).then_some(candidate)
        })
        .collect::<Vec<_>>();
    <[Point2; 1]>::try_from(candidates).ok().map(|[end]| end)
}

pub(crate) fn analytic_surface_uv(surface: &SurfaceGeometry, point: Point3) -> Option<Point2> {
    match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let offset = point.vector_from(*origin);
            let v_axis = (*normal).cross(*u_axis);
            Some(Point2::new(offset.dot(*u_axis), offset.dot(v_axis)))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            ..
        } => {
            let offset = point.vector_from(*origin);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                offset.dot(tangent).atan2(offset.dot(*ref_direction)),
                offset.dot(*axis),
            ))
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ratio,
            ..
        } => {
            if !ratio.is_finite() || *ratio == 0.0 {
                return None;
            }
            let offset = point.vector_from(*origin);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                (offset.dot(tangent) / ratio).atan2(offset.dot(*ref_direction)),
                offset.dot(*axis),
            ))
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if !radius.is_finite() || *radius == 0.0 {
                return None;
            }
            let offset = point.vector_from(*center);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                offset.dot(tangent).atan2(offset.dot(*ref_direction)),
                (offset.dot(*axis) / radius).clamp(-1.0, 1.0).asin(),
            ))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            ..
        } => {
            let offset = point.vector_from(*center);
            let tangent = (*axis).cross(*ref_direction);
            let u = offset.dot(tangent).atan2(offset.dot(*ref_direction));
            let radial = Vector3::new(
                u.cos() * ref_direction.x + u.sin() * tangent.x,
                u.cos() * ref_direction.y + u.sin() * tangent.y,
                u.cos() * ref_direction.z + u.sin() * tangent.z,
            );
            Some(Point2::new(
                u,
                offset.dot(*axis).atan2(offset.dot(radial) - major_radius),
            ))
        }
        _ => None,
    }
}

pub(crate) fn unwrap_standard_uv(surface: &SurfaceGeometry, value: &mut Point2, reference: Point2) {
    match surface {
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. } => value.u = unwrap_angle(value.u, reference.u),
        SurfaceGeometry::Torus { .. } => {
            value.u = unwrap_angle(value.u, reference.u);
            value.v = unwrap_angle(value.v, reference.v);
        }
        _ => {}
    }
}

pub(crate) fn point_on_surface(point: Point3, surface: &SurfaceGeometry) -> bool {
    point_on_surface_if_supported(point, surface).unwrap_or(false)
}

fn point_on_surface_if_supported(point: Point3, surface: &SurfaceGeometry) -> Option<bool> {
    const TOLERANCE: f64 = 1e-3;
    let residual = match surface {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            point.vector_from(*origin).dot(*normal).abs()
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let axial = point.vector_from(*origin).dot(*axis);
            let radial = point.distance_squared(*origin) - axial * axial;
            (radial.max(0.0).sqrt() - *radius).abs()
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            half_angle,
            ..
        } => {
            let axial = point.vector_from(*origin).dot(*axis);
            let radial = (point.distance_squared(*origin) - axial * axial)
                .max(0.0)
                .sqrt();
            (radial - (radius + axial * half_angle.tan()).abs()).abs()
        }
        SurfaceGeometry::Sphere { center, radius, .. } => {
            (point.distance_squared(*center).sqrt() - radius.abs()).abs()
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let axial = point.vector_from(*center).dot(*axis);
            let radial = (point.distance_squared(*center) - axial * axial)
                .max(0.0)
                .sqrt();
            (((radial - major_radius).powi(2) + axial * axial).sqrt() - minor_radius.abs()).abs()
        }
        SurfaceGeometry::Nurbs(surface) => {
            return point_on_nurbs_surface(point, surface);
        }
        SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => return None,
    };
    Some(residual <= TOLERANCE)
}

pub(crate) fn standard_spline_line(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    const TOLERANCE: f64 = 2e-3;

    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
    let [Some(left), Some(right)] = surfaces else {
        return None;
    };
    let start = ir.model.points.get(points[0])?.position;
    let end = ir.model.points.get(points[1])?.position;
    if !point_on_surface(start, &left.geometry)
        || !point_on_surface(start, &right.geometry)
        || !point_on_surface(end, &left.geometry)
        || !point_on_surface(end, &right.geometry)
    {
        return None;
    }
    let direction = end.vector_from(start);
    let length = direction.x.hypot(direction.y).hypot(direction.z);
    if !length.is_finite() || length == 0.0 {
        return None;
    }
    let follows_carrier_line = match (&left.geometry, &right.geometry) {
        (
            SurfaceGeometry::Plane {
                normal: left_normal,
                ..
            },
            SurfaceGeometry::Plane {
                normal: right_normal,
                ..
            },
        ) => {
            let intersection = (*left_normal).cross(*right_normal);
            let norm = intersection.x.hypot(intersection.y).hypot(intersection.z);
            norm.is_finite()
                && norm > 0.0
                && direction.cross(intersection.scale(1.0 / norm)).norm() <= TOLERANCE
        }
        (SurfaceGeometry::Cylinder { axis, .. }, SurfaceGeometry::Cylinder { .. })
            if support.faces[0] == support.faces[1] =>
        {
            direction.cross(*axis).norm() <= TOLERANCE
        }
        (
            SurfaceGeometry::Cone {
                origin,
                axis,
                radius,
                half_angle,
                ..
            },
            SurfaceGeometry::Cone { .. },
        ) if support.faces[0] == support.faces[1] => {
            let tangent = half_angle.tan();
            if !tangent.is_finite() || tangent == 0.0 {
                false
            } else {
                let apex = (*origin).translated(*axis, -radius / tangent);
                apex.vector_from(start)
                    .cross(direction.scale(1.0 / length))
                    .norm()
                    <= TOLERANCE
            }
        }
        _ => false,
    };
    if !follows_carrier_line {
        return None;
    }
    Some((
        CurveGeometry::Line {
            origin: start,
            direction: direction.scale(1.0 / length),
        },
        [0.0, length],
    ))
}

fn standard_spline_circle(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Option<CurveGeometry> {
    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
    let [Some(left), Some(right)] = surfaces else {
        return None;
    };
    let (sphere_center, sphere_radius, plane_origin, plane_normal) =
        match (&left.geometry, &right.geometry) {
            (
                SurfaceGeometry::Sphere { center, radius, .. },
                SurfaceGeometry::Plane { origin, normal, .. },
            )
            | (
                SurfaceGeometry::Plane { origin, normal, .. },
                SurfaceGeometry::Sphere { center, radius, .. },
            ) => (*center, *radius, *origin, *normal),
            _ => return None,
        };
    let axis = unit_vector(plane_normal)?;
    let sphere_radius = sphere_radius.abs();
    let signed_distance = sphere_center.vector_from(plane_origin).dot(axis);
    if !sphere_radius.is_finite() || sphere_radius <= 0.0 || !signed_distance.is_finite() {
        return None;
    }
    let section_radius_squared = sphere_radius * sphere_radius - signed_distance * signed_distance;
    if !section_radius_squared.is_finite()
        || section_radius_squared <= SPHERE_SECTION_ENDPOINT_TOLERANCE.powi(2)
    {
        return None;
    }
    let section_center = sphere_center.translated(axis, -signed_distance);
    let section_radius = section_radius_squared.sqrt();
    let start = ir.model.points.get(points[0])?.position;
    let end = ir.model.points.get(points[1])?.position;
    if !point_on_surface(start, &left.geometry)
        || !point_on_surface(start, &right.geometry)
        || !point_on_surface(end, &left.geometry)
        || !point_on_surface(end, &right.geometry)
        || (start.distance(section_center) - section_radius).abs()
            > SPHERE_SECTION_ENDPOINT_TOLERANCE
        || (end.distance(section_center) - section_radius).abs() > SPHERE_SECTION_ENDPOINT_TOLERANCE
    {
        return None;
    }
    Some(CurveGeometry::Circle {
        center: section_center,
        axis,
        ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
        radius: section_radius,
    })
}

fn standard_spline_cylinder_plane(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Option<CurveGeometry> {
    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
    let [Some(left), Some(right)] = surfaces else {
        return None;
    };
    let (cylinder_axis, cylinder_origin, cylinder_radius, plane_origin, plane_normal) =
        match (&left.geometry, &right.geometry) {
            (
                SurfaceGeometry::Cylinder {
                    axis,
                    origin: cylinder_origin,
                    radius,
                    ..
                },
                SurfaceGeometry::Plane {
                    origin: plane_origin,
                    normal: plane_normal,
                    ..
                },
            )
            | (
                SurfaceGeometry::Plane {
                    origin: plane_origin,
                    normal: plane_normal,
                    ..
                },
                SurfaceGeometry::Cylinder {
                    axis,
                    origin: cylinder_origin,
                    radius,
                    ..
                },
            ) => (
                *axis,
                *cylinder_origin,
                *radius,
                *plane_origin,
                *plane_normal,
            ),
            _ => return None,
        };
    let cylinder_axis = unit_vector(cylinder_axis)?;
    let plane_normal = unit_vector(plane_normal)?;
    let cylinder_radius = cylinder_radius.abs();
    if !cylinder_radius.is_finite() || cylinder_radius <= 0.0 {
        return None;
    }
    let axis_dot_normal = cylinder_axis.dot(plane_normal);
    if !axis_dot_normal.is_finite() || axis_dot_normal.abs() <= CYLINDER_PLANE_CONIC_TOLERANCE {
        return None;
    }
    let axis_parameter =
        -cylinder_origin.vector_from(plane_origin).dot(plane_normal) / axis_dot_normal;
    if !axis_parameter.is_finite() {
        return None;
    }
    let center = cylinder_origin.translated(cylinder_axis, axis_parameter);
    let start = ir.model.points.get(points[0])?.position;
    let end = ir.model.points.get(points[1])?.position;
    if !point_on_surface(start, &left.geometry)
        || !point_on_surface(start, &right.geometry)
        || !point_on_surface(end, &left.geometry)
        || !point_on_surface(end, &right.geometry)
    {
        return None;
    }
    let minor_vector = cylinder_axis.cross(plane_normal);
    let minor_norm = minor_vector.norm();
    if !minor_norm.is_finite() {
        return None;
    }
    if minor_norm <= CYLINDER_PLANE_CONIC_TOLERANCE {
        return Some(CurveGeometry::Circle {
            center,
            axis: plane_normal,
            ref_direction: cadmpeg_ir::geometry::derive_reference_direction(plane_normal),
            radius: cylinder_radius,
        });
    }
    let minor_direction = minor_vector.scale(1.0 / minor_norm);
    let radial_normal =
        (plane_normal - cylinder_axis.scale(axis_dot_normal)).scale(1.0 / minor_norm);
    let major_unscaled = radial_normal - cylinder_axis.scale(minor_norm / axis_dot_normal);
    let major_norm = major_unscaled.norm();
    if !major_norm.is_finite() || major_norm <= 0.0 {
        return None;
    }
    let major_direction = major_unscaled.scale(1.0 / major_norm);
    let major_radius = cylinder_radius * major_norm;
    if !major_radius.is_finite() || major_radius <= 0.0 {
        return None;
    }
    let endpoint_is_on_ellipse = |point: Point3| {
        let offset = point.vector_from(center);
        let major = offset.dot(major_direction) / major_radius;
        let minor = offset.dot(minor_direction) / cylinder_radius;
        let equation = major * major + minor * minor;
        equation.is_finite() && (equation - 1.0).abs() <= CYLINDER_PLANE_CONIC_TOLERANCE
    };
    if !endpoint_is_on_ellipse(start) || !endpoint_is_on_ellipse(end) {
        return None;
    }
    Some(CurveGeometry::Ellipse {
        center,
        axis: plane_normal,
        major_direction,
        major_radius,
        minor_radius: cylinder_radius,
    })
}

fn standard_spline_perpendicular_cylinders(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Option<CurveGeometry> {
    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
    let [Some(left), Some(right)] = surfaces else {
        return None;
    };
    let (first_axis, first_origin, first_radius, second_axis, second_origin, second_radius) =
        match (&left.geometry, &right.geometry) {
            (
                SurfaceGeometry::Cylinder {
                    axis,
                    origin,
                    radius,
                    ..
                },
                SurfaceGeometry::Cylinder {
                    axis: second_axis,
                    origin: second_origin,
                    radius: second_radius,
                    ..
                },
            ) => (
                *axis,
                *origin,
                *radius,
                *second_axis,
                *second_origin,
                *second_radius,
            ),
            _ => return None,
        };
    let first_axis = unit_vector(first_axis)?;
    let second_axis = unit_vector(second_axis)?;
    let first_radius = first_radius.abs();
    let second_radius = second_radius.abs();
    if !first_radius.is_finite()
        || !second_radius.is_finite()
        || first_radius <= 0.0
        || second_radius <= 0.0
        || (first_radius - second_radius).abs() > PERPENDICULAR_CYLINDER_CONIC_TOLERANCE
    {
        return None;
    }
    let axis_dot = first_axis.dot(second_axis);
    if !axis_dot.is_finite() || axis_dot.abs() > PERPENDICULAR_CYLINDER_CONIC_TOLERANCE {
        return None;
    }
    let denominator = 1.0 - axis_dot * axis_dot;
    if !denominator.is_finite() || denominator <= 0.0 {
        return None;
    }
    let axis_offset = second_origin.vector_from(first_origin);
    let first_parameter =
        (axis_offset.dot(first_axis) - axis_dot * axis_offset.dot(second_axis)) / denominator;
    let second_parameter = axis_dot * first_parameter - axis_offset.dot(second_axis);
    if !first_parameter.is_finite() || !second_parameter.is_finite() {
        return None;
    }
    let first_center = first_origin.translated(first_axis, first_parameter);
    let second_center = second_origin.translated(second_axis, second_parameter);
    if first_center.distance(second_center) > PERPENDICULAR_CYLINDER_CONIC_TOLERANCE {
        return None;
    }
    let center = Point3::new(
        (first_center.x + second_center.x) * 0.5,
        (first_center.y + second_center.y) * 0.5,
        (first_center.z + second_center.z) * 0.5,
    );
    let start = ir.model.points.get(points[0])?.position;
    let end = ir.model.points.get(points[1])?.position;
    if !point_on_surface(start, &left.geometry)
        || !point_on_surface(start, &right.geometry)
        || !point_on_surface(end, &left.geometry)
        || !point_on_surface(end, &right.geometry)
    {
        return None;
    }
    let minor_direction = unit_vector(first_axis.cross(second_axis))?;
    let radius = (first_radius + second_radius) * 0.5;
    let major_radius = radius * 2.0_f64.sqrt();
    if !radius.is_finite() || !major_radius.is_finite() || major_radius <= 0.0 {
        return None;
    }
    let branches = [
        (first_axis - second_axis, first_axis + second_axis),
        (first_axis + second_axis, first_axis - second_axis),
    ]
    .into_iter()
    .filter_map(|(axis, major_direction)| {
        let axis = unit_vector(axis)?;
        let major_direction = unit_vector(major_direction)?;
        let endpoint_is_on_branch = |point: Point3| {
            let offset = point.vector_from(center);
            let major = offset.dot(major_direction) / major_radius;
            let minor = offset.dot(minor_direction) / radius;
            let equation = major * major + minor * minor;
            offset.dot(axis).abs() <= PERPENDICULAR_CYLINDER_CONIC_TOLERANCE
                && equation.is_finite()
                && (equation - 1.0).abs() <= PERPENDICULAR_CYLINDER_CONIC_TOLERANCE
        };
        (endpoint_is_on_branch(start) && endpoint_is_on_branch(end)).then_some(
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius: radius,
            },
        )
    })
    .collect::<Vec<_>>();
    let [geometry] = branches.as_slice() else {
        return None;
    };
    Some(geometry.clone())
}

fn standard_native_support_witness(native: &StandardEdgeSupport) -> Option<Point3> {
    let parameter = 0.5 * (native.parameter_range[0] + native.parameter_range[1]);
    let lifted = native
        .carriers
        .iter()
        .zip(&native.pcurves)
        .map(|(carrier, pcurve)| {
            let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) = carrier
            else {
                return None;
            };
            let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter)?;
            cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v)
        })
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = <[Point3; 2]>::try_from(lifted).ok()?;
    (first.distance_squared(second).sqrt() <= SUPPORT_AGREEMENT_TOLERANCE).then_some(first)
}

fn standard_analytic_curve_angle(geometry: &CurveGeometry, point: Point3) -> Option<f64> {
    let (center, first, second) = match geometry {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => (
            *center,
            ref_direction.scale(*radius),
            axis.cross(*ref_direction).scale(*radius),
        ),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            *center,
            major_direction.scale(*major_radius),
            axis.cross(*major_direction).scale(*minor_radius),
        ),
        _ => return None,
    };
    let offset = point.vector_from(center);
    let first_length = first.norm();
    let second_length = second.norm();
    if !first_length.is_finite()
        || !second_length.is_finite()
        || first_length <= 0.0
        || second_length <= 0.0
    {
        return None;
    }
    let first_component = offset.dot(first) / first_length.powi(2);
    let second_component = offset.dot(second) / second_length.powi(2);
    let residual = first_component * first_component + second_component * second_component - 1.0;
    (residual.is_finite() && residual.abs() <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE)
        .then(|| second_component.atan2(first_component))
}

fn standard_analytic_curve_parameter_range(
    geometry: &CurveGeometry,
    start: Point3,
    end: Point3,
    witness: Option<Point3>,
) -> Option<[f64; 2]> {
    if start.distance_squared(end).sqrt() <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE {
        return Some([0.0, std::f64::consts::TAU]);
    }
    let start = standard_analytic_curve_angle(geometry, start)?;
    let short_end = unwrap_angle(standard_analytic_curve_angle(geometry, end)?, start);
    let end = witness.map_or(Some(short_end), |witness| {
        witness_arc_end(
            start,
            short_end,
            standard_analytic_curve_angle(geometry, witness)?,
        )
    })?;
    crate::nurbs::canonical_periodic_range([start, end])
}

fn standard_oriented_analytic_curve_parameter_range(
    geometry: &mut CurveGeometry,
    start: Point3,
    end: Point3,
    witness: Point3,
) -> Option<[f64; 2]> {
    if let Some(range) =
        standard_analytic_curve_parameter_range(geometry, start, end, Some(witness))
    {
        return Some(range);
    }
    let original_axis = match geometry {
        CurveGeometry::Circle { axis, .. } | CurveGeometry::Ellipse { axis, .. } => *axis,
        _ => return None,
    };
    match geometry {
        CurveGeometry::Circle { axis, .. } | CurveGeometry::Ellipse { axis, .. } => {
            *axis = axis.scale(-1.0);
        }
        _ => unreachable!("analytic orientation was checked above"),
    }
    let range = standard_analytic_curve_parameter_range(geometry, start, end, Some(witness));
    if range.is_none() {
        match geometry {
            CurveGeometry::Circle { axis, .. } | CurveGeometry::Ellipse { axis, .. } => {
                *axis = original_axis;
            }
            _ => unreachable!("analytic orientation was checked above"),
        }
    }
    range
}

fn standard_oriented_native_support_pcurves(
    native: &StandardEdgeSupport,
    points: &[Point],
    endpoint_pair: [usize; 2],
) -> Option<[PcurveGeometry; 2]> {
    let Some(native_pair) =
        standard_native_support_endpoint_pair(native, points, &endpoint_pair, Some(endpoint_pair))
    else {
        return Some(native.pcurves.clone());
    };
    if native_pair == endpoint_pair {
        return Some(native.pcurves.clone());
    }
    Some([
        crate::nurbs::reverse_pcurve_geometry(&native.pcurves[0], native.parameter_range)?,
        crate::nurbs::reverse_pcurve_geometry(&native.pcurves[1], native.parameter_range)?,
    ])
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_standard_edge_curve(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    brep: &[u8],
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
    native_support: Option<&StandardEdgeSupport>,
    limit_curve: Option<(&NurbsCurve, [f64; 2])>,
) -> (Option<CurveId>, Option<[f64; 2]>) {
    let (mut geometry, mut param_range) = match &support.geometry {
        crate::families::standard::records::StandardCurveGeometry::Line => {
            let start = ir.model.points[points[0]].position;
            let end = ir.model.points[points[1]].position;
            let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
            let length = delta.x.hypot(delta.y).hypot(delta.z);
            if !length.is_finite() || length == 0.0 {
                return (None, None);
            }
            (
                CurveGeometry::Line {
                    origin: start,
                    direction: Vector3::new(delta.x / length, delta.y / length, delta.z / length),
                },
                Some([0.0, length]),
            )
        }
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } => {
            let start = ir.model.points[points[0]].position;
            let end = ir.model.points[points[1]].position;
            let mut axes: Vec<Vector3> = support
                .faces
                .iter()
                .filter_map(|face| face_surface(ir, bindings, surface_indices, *face))
                .filter_map(|surface| {
                    standard_circle_axis_from_carrier(*center, *radius, &surface.geometry)
                })
                .collect();
            axes.extend(native_support.into_iter().flat_map(|native| {
                native.carriers.iter().filter_map(|carrier| {
                    let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) =
                        carrier
                    else {
                        return None;
                    };
                    standard_circle_axis_from_carrier(*center, *radius, surface)
                })
            }));
            if axes.is_empty() {
                axes.extend(circle_axis_from_endpoints(*center, *radius, start, end));
            }
            let axis = axes.first().copied();
            let conflicting_axes = axis.is_some_and(|axis| {
                axes.iter()
                    .skip(1)
                    .any(|other| axis.dot(*other).abs() < 0.9999)
            });
            match axis.filter(|_| !conflicting_axes) {
                Some(axis) if points[0] == points[1] => {
                    match full_circle_frame(*center, *radius, axis, start) {
                        Some((axis, ref_direction)) => (
                            CurveGeometry::Circle {
                                center: *center,
                                axis,
                                ref_direction,
                                radius: *radius,
                            },
                            Some([0.0, std::f64::consts::TAU]),
                        ),
                        None => (
                            CurveGeometry::Unknown {
                                record: Some(UnknownId(
                                    "catia:payload:unknown#brep-stream".to_string(),
                                )),
                            },
                            None,
                        ),
                    }
                }
                Some(axis) => {
                    let candidates = [axis, axis.scale(-1.0)]
                        .into_iter()
                        .filter_map(|axis| {
                            let ref_direction =
                                cadmpeg_ir::geometry::derive_reference_direction(axis);
                            let range = standard_circle_param_range(
                                ir,
                                bindings,
                                surface_indices,
                                brep,
                                support,
                                *center,
                                *radius,
                                axis,
                                ref_direction,
                                start,
                                end,
                            )
                            .or_else(|| {
                                native_support.and_then(|native| {
                                    native_support_circle_param_range(
                                        native,
                                        *center,
                                        *radius,
                                        axis,
                                        ref_direction,
                                        start,
                                        end,
                                    )
                                })
                            })?;
                            Some((
                                axis,
                                ref_direction,
                                crate::nurbs::canonical_periodic_range(range)?,
                            ))
                        })
                        .collect::<Vec<_>>();
                    let (axis, ref_direction, param_range) = match candidates.as_slice() {
                        [(axis, reference, range)] => (*axis, *reference, Some(*range)),
                        _ => (
                            axis,
                            cadmpeg_ir::geometry::derive_reference_direction(axis),
                            None,
                        ),
                    };
                    (
                        CurveGeometry::Circle {
                            center: *center,
                            axis,
                            ref_direction,
                            radius: *radius,
                        },
                        param_range,
                    )
                }
                None => (
                    CurveGeometry::Unknown {
                        record: Some(UnknownId("catia:payload:unknown#brep-stream".to_string())),
                    },
                    None,
                ),
            }
        }
        crate::families::standard::records::StandardCurveGeometry::Bspline => {
            if let Some((limit_curve, parameter_range)) = limit_curve {
                (
                    CurveGeometry::Nurbs(limit_curve.clone()),
                    Some(parameter_range),
                )
            } else {
                match standard_spline_line(ir, bindings, surface_indices, support, points) {
                    Some((geometry, range)) => (geometry, Some(range)),
                    None => {
                        match standard_spline_circle(ir, bindings, surface_indices, support, points)
                        {
                            Some(geometry) => (geometry, None),
                            None => match standard_spline_cylinder_plane(
                                ir,
                                bindings,
                                surface_indices,
                                support,
                                points,
                            ) {
                                Some(geometry) => (geometry, None),
                                None => match standard_spline_perpendicular_cylinders(
                                    ir,
                                    bindings,
                                    surface_indices,
                                    support,
                                    points,
                                ) {
                                    Some(geometry) => (geometry, None),
                                    None => (
                                        CurveGeometry::Unknown {
                                            record: Some(UnknownId(
                                                "catia:payload:unknown#brep-stream".to_string(),
                                            )),
                                        },
                                        None,
                                    ),
                                },
                            },
                        }
                    }
                }
            }
        }
    };
    if param_range.is_none()
        && matches!(
            geometry,
            CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. }
        )
    {
        let endpoints = [
            ir.model.points[points[0]].position,
            ir.model.points[points[1]].position,
        ];
        if let Some(witness) = native_support.and_then(standard_native_support_witness) {
            param_range = standard_oriented_analytic_curve_parameter_range(
                &mut geometry,
                endpoints[0],
                endpoints[1],
                witness,
            );
        }
    }
    let oriented_native_support_pcurves = if matches!(
        &support.geometry,
        crate::families::standard::records::StandardCurveGeometry::Bspline
    ) {
        match native_support {
            Some(native) => {
                match standard_oriented_native_support_pcurves(native, &ir.model.points, points) {
                    Some(pcurves) => Some(pcurves),
                    None => return (None, None),
                }
            }
            None => None,
        }
    } else {
        None
    };
    let id = CurveId(format!("catia:standard:curve#{}", support.pos));
    annotate(
        annotations,
        &id,
        "MainDataStream+SurfacicReps",
        support.pos as u64,
        "curve_support_60",
        match (&support.geometry, &geometry) {
            (_, CurveGeometry::Unknown { .. }) => Exactness::Unknown,
            (crate::families::standard::records::StandardCurveGeometry::Bspline, _) => {
                Exactness::Derived
            }
            _ => Exactness::ByteExact,
        },
    );
    if matches!(&geometry, CurveGeometry::Line { .. }) {
        annotations
            .derived(&id, "geometry.origin")
            .derived(&id, "geometry.direction");
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Bspline,
            CurveGeometry::Circle { .. }
        )
    ) {
        annotations
            .derived(&id, "geometry.center")
            .derived(&id, "geometry.axis")
            .derived(&id, "geometry.ref_direction")
            .derived(&id, "geometry.radius");
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Bspline,
            CurveGeometry::Ellipse { .. }
        )
    ) {
        annotations
            .derived(&id, "geometry.center")
            .derived(&id, "geometry.axis")
            .derived(&id, "geometry.major_direction")
            .derived(&id, "geometry.major_radius")
            .derived(&id, "geometry.minor_radius");
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Circle { .. },
            CurveGeometry::Circle { .. }
        )
    ) {
        annotations.derived(&id, "geometry.axis");
    }
    let geometry_is_unknown = matches!(&geometry, CurveGeometry::Unknown { .. });
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry,
        source_object: Some(cgm_source("edge-support", support.tag)),
    });
    if matches!(
        &support.geometry,
        crate::families::standard::records::StandardCurveGeometry::Bspline
    ) {
        let sides = if let Some(native) = native_support {
            let Some(pcurves) = oriented_native_support_pcurves.as_ref() else {
                return (None, None);
            };
            let mut surfaces = Vec::with_capacity(2);
            for side in 0..2 {
                surfaces.push(ensure_native_edge_support_surface(
                    ir,
                    annotations,
                    native.surface_object_ids[side],
                    &native.carriers[side],
                ));
            }
            std::array::from_fn(|side| IntcurveSupportSide {
                surface: Some(surfaces[side].clone()),
                pcurve: Some(pcurves[side].clone()),
                pcurve_parameter_range: Some(native.parameter_range),
            })
        } else {
            support.faces.map(|face| {
                let surface = bindings
                    .get(face)
                    .and_then(|(id, _, _)| surface_indices.get(id).map(|_| id.clone()));
                IntcurveSupportSide {
                    surface,
                    pcurve: None,
                    pcurve_parameter_range: None,
                }
            })
        };
        if sides.iter().all(|side| side.surface.is_some())
            && (native_support.is_some() || sides[0].surface != sides[1].surface)
        {
            let curve_parameter_range = param_range.or_else(|| {
                geometry_is_unknown
                    .then(|| native_support.map_or([0.0, 1.0], |native| native.parameter_range))
            });
            if let Some(curve_parameter_range) = curve_parameter_range {
                let procedural_id =
                    ProceduralCurveId(format!("catia:standard:intersection#{}", support.pos));
                annotate(
                    annotations,
                    &procedural_id,
                    "MainDataStream+SurfacicReps",
                    support.pos as u64,
                    "standard_surface_intersection",
                    Exactness::Derived,
                );
                annotations
                    .derived(&procedural_id, "curve")
                    .derived(&procedural_id, "definition");
                ir.model.procedural_curves.push(ProceduralCurve {
                    id: procedural_id,
                    curve: id.clone(),
                    definition: ProceduralCurveDefinition::Intersection {
                        context: IntcurveSupportContext {
                            sides,
                            parameter_range: ordered_range(curve_parameter_range),
                            discontinuities: std::array::from_fn(|_| Vec::new()),
                        },
                        discontinuity_flag: false,
                    },
                    cache_fit_tolerance: None,
                });
                param_range = Some(curve_parameter_range);
            }
        }
    }
    (Some(id), param_range)
}

fn ensure_native_edge_support_surface(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    surface_object_id: u32,
    carrier: &crate::families::b5::transfer::ResolvedPcurveSurface,
) -> SurfaceId {
    let source = cgm_source("surface", surface_object_id);
    let source_matches = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| surface.source_object.as_ref() == Some(&source))
        .map(|surface| surface.id.clone())
        .collect::<HashSet<_>>();
    if source_matches.len() == 1 {
        return source_matches
            .into_iter()
            .next()
            .expect("one identity-matched support surface");
    }
    if let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(geometry) = carrier {
        let geometry_matches = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.geometry == *geometry)
            .map(|surface| surface.id.clone())
            .collect::<HashSet<_>>();
        if source_matches.is_empty() && geometry_matches.len() == 1 {
            return geometry_matches
                .into_iter()
                .next()
                .expect("one geometry-matched support surface");
        }
    }
    let id = SurfaceId(format!(
        "catia:standard:edge-support-surface#{surface_object_id}"
    ));
    let procedural_id = match carrier {
        crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall { .. } => {
            Some(ProceduralSurfaceId(format!(
                "catia:standard:edge-support-definition#{surface_object_id}"
            )))
        }
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(_) => None,
    };
    annotate(
        annotations,
        &id,
        "CATPart",
        0,
        "native_edge_support_surface",
        Exactness::ByteExact,
    );
    let geometry = match carrier {
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(geometry) => {
            geometry.clone()
        }
        crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall { .. } => {
            SurfaceGeometry::Procedural {
                construction: procedural_id
                    .clone()
                    .expect("rolling-ball support procedure id"),
            }
        }
    };
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        geometry,
        source_object: Some(source),
    });
    if let (
        Some(procedural_id),
        crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall {
            carrier_object_id,
            definition,
        },
    ) = (procedural_id, carrier)
    {
        annotate(
            annotations,
            &procedural_id,
            "object_stream_a8_03_32",
            0,
            format!(
                "support_surface:{surface_object_id:08x}:result_carrier:{carrier_object_id:08x}"
            ),
            Exactness::ByteExact,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: id.clone(),
            definition: definition.as_ref().clone(),
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }
    id
}

pub(crate) fn standard_circle_pair_solution_is_simple(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    type CircleFaceKey = (u64, u64, u64, u64, usize);

    let mut range_choices = HashMap::<CircleFaceKey, Vec<Vec<[f64; 2]>>>::new();
    for ((support, options), pair) in supports.iter().zip(endpoint_options).zip(pairs) {
        let Some(pair) = pair else {
            continue;
        };
        if options.len() <= 1 {
            continue;
        }
        let crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } =
            &support.geometry
        else {
            continue;
        };
        let Some(start) = ir.model.points.get(pair[0]).map(|point| point.position) else {
            return false;
        };
        let Some(end) = ir.model.points.get(pair[1]).map(|point| point.position) else {
            return false;
        };
        let axes = support
            .faces
            .iter()
            .filter_map(|face| face_surface(ir, bindings, surface_indices, *face))
            .filter_map(|surface| {
                standard_circle_axis_from_carrier(*center, *radius, &surface.geometry)
            })
            .collect::<Vec<_>>();
        let Some(axis) = axes.first().copied().and_then(canonical_unoriented_axis) else {
            continue;
        };
        if axes.iter().skip(1).any(|other| {
            canonical_unoriented_axis(*other).is_none_or(|other| axis.dot(other).abs() < 0.9999)
        }) {
            return false;
        }
        let Some(choices) = circle_endpoint_range_choices(*center, *radius, axis, start, end)
        else {
            continue;
        };
        for &face in &support.faces {
            let key = (
                center.x.to_bits(),
                center.y.to_bits(),
                center.z.to_bits(),
                radius.to_bits(),
                face,
            );
            range_choices.entry(key).or_default().push(choices.clone());
        }
    }
    for choices in range_choices.values() {
        if !circular_range_choices_have_simple_selection(choices) {
            return false;
        }
    }
    true
}

/// Require line endpoint assignments to partition each shared straight
/// carrier into disjoint edge intervals. Exact coincident intervals remain
/// admissible because seam and duplicate-edge representations can share one
/// carrier; a partial collinear overlap is the non-simple alternative.
#[derive(Clone, Copy)]
struct StandardLineSegment {
    start: Point3,
    end: Point3,
}

#[derive(Clone, Copy)]
struct StandardLineSelection {
    pair: [usize; 2],
    segment: StandardLineSegment,
}

type StandardLinePairKey = ((usize, [usize; 2]), (usize, [usize; 2]));

struct StandardLinePairConstraint {
    points: Vec<Point3>,
    line_edges: Vec<bool>,
    flexible_edges: Vec<bool>,
    edges_by_face: HashMap<usize, Vec<usize>>,
    simplicity_cache: RefCell<HashMap<StandardLinePairKey, bool>>,
}

impl StandardLinePairConstraint {
    fn new(
        points: &[Point],
        supports: &[crate::families::standard::records::StandardCurveSupport],
        endpoint_options: &[Vec<[usize; 2]>],
    ) -> Self {
        let points = points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>();
        let line_edges = supports
            .iter()
            .map(|support| {
                matches!(
                    support.geometry,
                    crate::families::standard::records::StandardCurveGeometry::Line
                )
            })
            .collect::<Vec<_>>();
        let mut flexible_edges = vec![false; supports.len()];
        let mut edges_by_face = HashMap::<usize, Vec<usize>>::new();

        for (edge, (support, options)) in supports.iter().zip(endpoint_options).enumerate() {
            if !matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Line
            ) || options.len() <= 1
            {
                continue;
            }
            flexible_edges[edge] = true;
            for &face in &support.faces {
                let edges = edges_by_face.entry(face).or_default();
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
        }

        Self {
            points,
            line_edges,
            flexible_edges,
            edges_by_face,
            simplicity_cache: RefCell::new(HashMap::new()),
        }
    }

    fn flexible_edge_mask(&self) -> &[bool] {
        &self.flexible_edges
    }

    fn is_valid(&self, pairs: &[Option<[usize; 2]>]) -> bool {
        pairs.iter().enumerate().all(|(edge, pair)| {
            if !self.line_edges.get(edge).copied().unwrap_or(false) {
                return true;
            }
            let Some(pair) = pair else {
                return true;
            };
            let Some(segment) = standard_line_segment(&self.points, *pair) else {
                return false;
            };
            standard_line_segment_is_materializable(segment)
        })
    }

    fn is_simple(&self, pairs: &[Option<[usize; 2]>]) -> bool {
        if !self.is_valid(pairs) {
            return false;
        }
        let mut selected = vec![None; self.flexible_edges.len()];
        for (edge, pair) in pairs.iter().enumerate() {
            if !self.flexible_edges.get(edge).copied().unwrap_or(false) {
                continue;
            }
            let Some(pair) = pair else {
                continue;
            };
            let segment = standard_line_segment(&self.points, *pair);
            let Some(segment) = segment else {
                continue;
            };
            selected[edge] = Some(StandardLineSelection {
                pair: *pair,
                segment,
            });
        }

        for edges in self.edges_by_face.values() {
            for (left_position, &left_edge) in edges.iter().enumerate() {
                let Some(left) = selected[left_edge] else {
                    continue;
                };
                for &right_edge in &edges[left_position + 1..] {
                    let Some(right) = selected[right_edge] else {
                        continue;
                    };
                    let key = ordered_line_pair(left_edge, left.pair, right_edge, right.pair);
                    let incompatible = {
                        let cached = {
                            let cache = self.simplicity_cache.borrow();
                            cache.get(&key).copied()
                        };
                        if let Some(simple) = cached {
                            !simple
                        } else {
                            let simple =
                                standard_line_segments_are_simple(left.segment, right.segment);
                            self.simplicity_cache.borrow_mut().insert(key, simple);
                            !simple
                        }
                    };
                    if incompatible {
                        return false;
                    }
                }
            }
        }
        true
    }
}

fn ordered_line_pair(
    left_edge: usize,
    left_pair: [usize; 2],
    right_edge: usize,
    right_pair: [usize; 2],
) -> StandardLinePairKey {
    if left_edge < right_edge {
        ((left_edge, left_pair), (right_edge, right_pair))
    } else {
        ((right_edge, right_pair), (left_edge, left_pair))
    }
}

fn standard_line_segment(points: &[Point3], pair: [usize; 2]) -> Option<StandardLineSegment> {
    Some(StandardLineSegment {
        start: *points.get(pair[0])?,
        end: *points.get(pair[1])?,
    })
}

fn standard_line_segment_is_materializable(segment: StandardLineSegment) -> bool {
    let delta = segment.end.vector_from(segment.start);
    let length = delta.x.hypot(delta.y).hypot(delta.z);
    length.is_finite() && length != 0.0
}

fn standard_line_segments_are_simple(
    left: StandardLineSegment,
    right: StandardLineSegment,
) -> bool {
    let left_axis = left.end.vector_from(left.start);
    let left_length = left_axis.norm();
    let right_axis = right.end.vector_from(right.start);
    let right_length = right_axis.norm();
    if left_length <= LINE_SEGMENT_GEOMETRY_TOLERANCE
        || right_length <= LINE_SEGMENT_GEOMETRY_TOLERANCE
    {
        return true;
    }
    let left_unit = left_axis.scale(1.0 / left_length);
    let parallel_error = left_unit.cross(right_axis.scale(1.0 / right_length)).norm();
    let line_error = left_unit
        .cross(right.start.vector_from(left.start))
        .norm()
        .max(left_unit.cross(right.end.vector_from(left.start)).norm());
    if parallel_error > LINE_SEGMENT_GEOMETRY_TOLERANCE
        || line_error > LINE_SEGMENT_GEOMETRY_TOLERANCE
    {
        return true;
    }
    let left_interval = [0.0, left_length];
    let right_interval = [
        left_unit.dot(right.start.vector_from(left.start)),
        left_unit.dot(right.end.vector_from(left.start)),
    ];
    let right_interval = [
        right_interval[0].min(right_interval[1]),
        right_interval[0].max(right_interval[1]),
    ];
    let overlap = left_interval[1].min(right_interval[1]) - left_interval[0].max(right_interval[0]);
    if overlap <= LINE_SEGMENT_GEOMETRY_TOLERANCE {
        return true;
    }
    (left_interval[0] - right_interval[0]).abs() <= LINE_SEGMENT_GEOMETRY_TOLERANCE
        && (left_interval[1] - right_interval[1]).abs() <= LINE_SEGMENT_GEOMETRY_TOLERANCE
}

#[cfg(test)]
pub(crate) fn standard_line_pair_solution_is_simple(
    points: &[Point],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    let point_positions = points
        .iter()
        .map(|point| point.position)
        .collect::<Vec<_>>();
    let segments = supports
        .iter()
        .zip(endpoint_options)
        .zip(pairs)
        .filter_map(|((support, options), pair)| {
            if !matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Line
            ) {
                return None;
            }
            if options.len() <= 1 {
                return None;
            }
            let pair = pair.as_ref()?;
            Some((
                support.faces,
                standard_line_segment(&point_positions, *pair)?,
            ))
        })
        .collect::<Vec<_>>();
    if supports.iter().zip(pairs).any(|(support, pair)| {
        matches!(
            support.geometry,
            crate::families::standard::records::StandardCurveGeometry::Line
        ) && pair.is_some_and(|pair| {
            standard_line_segment(&point_positions, pair)
                .is_none_or(|segment| !standard_line_segment_is_materializable(segment))
        })
    }) {
        return false;
    }
    if segments
        .iter()
        .any(|(_, segment)| !standard_line_segment_is_materializable(*segment))
    {
        return false;
    }
    let mut segments_by_face = HashMap::<usize, Vec<StandardLineSegment>>::new();
    for (faces, segment) in segments {
        for face in faces {
            segments_by_face.entry(face).or_default().push(segment);
        }
    }
    segments_by_face.into_values().all(|segments| {
        segments.iter().enumerate().all(|(left_index, left)| {
            segments[left_index + 1..]
                .iter()
                .all(|right| standard_line_segments_are_simple(*left, *right))
        })
    })
}

#[cfg(test)]
pub(crate) fn standard_line_pair_solution_is_simple_cached(
    points: &[Point],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    StandardLinePairConstraint::new(points, supports, endpoint_options).is_simple(pairs)
}

fn circle_endpoint_range_choices(
    center: Point3,
    radius: f64,
    axis: Vector3,
    start: Point3,
    end: Point3,
) -> Option<Vec<[f64; 2]>> {
    const ENDPOINT_TOLERANCE: f64 = 2e-3;

    if !radius.is_finite()
        || radius <= 0.0
        || (start.distance(center) - radius).abs() > ENDPOINT_TOLERANCE
        || (end.distance(center) - radius).abs() > ENDPOINT_TOLERANCE
    {
        return None;
    }
    if start.distance(end) <= ENDPOINT_TOLERANCE {
        return Some(vec![[0.0, std::f64::consts::TAU]]);
    }
    let axis = unit_vector(axis)?;
    let reference = cadmpeg_ir::geometry::derive_reference_direction(axis);
    let tangent = axis.cross(reference);
    let angle = |point: Point3| {
        let offset = point.vector_from(center);
        offset
            .dot(tangent)
            .atan2(offset.dot(reference))
            .rem_euclid(std::f64::consts::TAU)
    };
    let mut endpoints = [angle(start), angle(end)];
    if endpoints.iter().any(|angle| !angle.is_finite()) {
        return None;
    }
    endpoints.sort_by(f64::total_cmp);
    let short = crate::nurbs::canonical_periodic_range(endpoints)?;
    let long = crate::nurbs::canonical_periodic_range([
        endpoints[1],
        endpoints[0] + std::f64::consts::TAU,
    ])?;
    Some(vec![short, long])
}

pub(crate) fn circular_range_choices_have_simple_selection(choices: &[Vec<[f64; 2]>]) -> bool {
    const MAX_SELECTION_STATES: usize = 4_096;

    fn visit(
        choices: &[Vec<[f64; 2]>],
        index: usize,
        selected: &mut Vec<[f64; 2]>,
        states: &mut usize,
    ) -> Option<bool> {
        if *states >= MAX_SELECTION_STATES {
            return None;
        }
        *states += 1;
        if index == choices.len() {
            return Some(true);
        }
        for &range in &choices[index] {
            selected.push(range);
            let compatible = circular_ranges_are_nonoverlapping_or_coincident(selected);
            if compatible {
                match visit(choices, index + 1, selected, states) {
                    Some(true) => {
                        selected.pop();
                        return Some(true);
                    }
                    None => {
                        selected.pop();
                        return None;
                    }
                    Some(false) => {}
                }
            }
            selected.pop();
        }
        Some(false)
    }

    if choices.iter().any(Vec::is_empty) {
        return false;
    }
    visit(choices, 0, &mut Vec::new(), &mut 0).unwrap_or(true)
}

pub(crate) fn circular_ranges_are_nonoverlapping_or_coincident(ranges: &[[f64; 2]]) -> bool {
    fn segments(range: [f64; 2]) -> Vec<[f64; 2]> {
        let span = range[1] - range[0];
        let start = range[0].rem_euclid(std::f64::consts::TAU);
        let end = start + span;
        if end <= std::f64::consts::TAU {
            vec![[start, end]]
        } else {
            vec![
                [start, std::f64::consts::TAU],
                [0.0, end - std::f64::consts::TAU],
            ]
        }
    }

    ranges.iter().enumerate().all(|(left_index, left)| {
        ranges[left_index + 1..].iter().all(|right| {
            let coincident =
                (right[0] - left[0]).abs() <= 1e-9 && (right[1] - left[1]).abs() <= 1e-9;
            coincident
                || !segments(*left).iter().any(|left| {
                    segments(*right)
                        .iter()
                        .any(|right| left[1].min(right[1]) - left[0].max(right[0]) > 1e-6)
                })
        })
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn standard_circle_param_range(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    brep: &[u8],
    support: &crate::families::standard::records::StandardCurveSupport,
    center: Point3,
    radius: f64,
    axis: Vector3,
    ref_direction: Vector3,
    start: Point3,
    end: Point3,
) -> Option<[f64; 2]> {
    let mut ranges = support.faces.iter().filter_map(|face| {
        let surface = face_surface(ir, bindings, surface_indices, *face)?;
        let witness = crate::families::standard::records::standard_face_witness(
            brep,
            bindings.get(*face)?.2,
        )?;
        let (PcurveGeometry::Line { origin, direction }, _) =
            standard_pcurve_geometry(&surface.geometry, support, start, end, Some(witness), None)?
        else {
            return None;
        };
        circle_parameter_range_from_surface_branch(
            &surface.geometry,
            center,
            radius,
            axis,
            ref_direction,
            start,
            end,
            origin,
            direction,
        )
    });
    let range = ranges.next()?;
    if ranges.any(|other| (other[0] - range[0]).abs() > 1e-9 || (other[1] - range[1]).abs() > 1e-9)
    {
        return None;
    }
    Some(range)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn native_support_circle_param_range(
    support: &StandardEdgeSupport,
    center: Point3,
    radius: f64,
    axis: Vector3,
    ref_direction: Vector3,
    start: Point3,
    end: Point3,
) -> Option<[f64; 2]> {
    const GEOMETRY_TOLERANCE: f64 = 2e-3;

    let parameters = [
        support.parameter_range[0],
        0.5 * (support.parameter_range[0] + support.parameter_range[1]),
        support.parameter_range[1],
    ];
    let lifted = support
        .carriers
        .iter()
        .zip(&support.pcurves)
        .map(|(carrier, pcurve)| {
            let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) = carrier
            else {
                return None;
            };
            let carrier_axis = standard_circle_axis_from_carrier(center, radius, surface)?;
            (carrier_axis.dot(axis) >= 0.9999).then_some(())?;
            Some(parameters.map(|parameter| {
                let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter)?;
                cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v)
            }))
        })
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = <[[Option<Point3>; 3]; 2]>::try_from(lifted).ok()?;
    let first = first.into_iter().collect::<Option<Vec<_>>>()?;
    let second = second.into_iter().collect::<Option<Vec<_>>>()?;
    if first
        .iter()
        .zip(&second)
        .any(|(left, right)| left.distance_squared(*right).sqrt() > SUPPORT_AGREEMENT_TOLERANCE)
    {
        return None;
    }
    let endpoint_error = |left: Point3, right: Point3| left.distance_squared(right).sqrt();
    let source_forward = endpoint_error(first[0], start) <= GEOMETRY_TOLERANCE
        && endpoint_error(first[2], end) <= GEOMETRY_TOLERANCE;
    let source_reversed = endpoint_error(first[0], end) <= GEOMETRY_TOLERANCE
        && endpoint_error(first[2], start) <= GEOMETRY_TOLERANCE;
    if source_forward == source_reversed {
        return None;
    }
    let witness = first[1];
    let transverse = axis.cross(ref_direction);
    let angle = |point: Point3| {
        let radial = point.vector_from(center);
        let axial = radial.dot(axis);
        let radial_length = radial.norm();
        (axial.abs() <= GEOMETRY_TOLERANCE && (radial_length - radius).abs() <= GEOMETRY_TOLERANCE)
            .then(|| radial.dot(transverse).atan2(radial.dot(ref_direction)))
    };
    let start_angle = angle(start)?;
    let end_angle = unwrap_angle(angle(end)?, start_angle);
    let witness_angle = angle(witness)?;
    let selected_end = witness_arc_end(start_angle, end_angle, witness_angle)?;
    Some([start_angle, selected_end])
}

pub(crate) fn attach_standard_circles(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
    edge_count: Option<usize>,
) {
    for circle in
        crate::families::standard::records::standard_circles(brep, bindings.len(), edge_count)
    {
        let axes: Vec<Vector3> = circle
            .faces
            .iter()
            .filter_map(|face| bindings.get(*face))
            .filter_map(|(surface_id, _, _)| {
                ir.model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
            })
            .filter_map(|surface| {
                standard_circle_axis_from_carrier(circle.center, circle.radius, &surface.geometry)
            })
            .collect();
        let Some(axis) = axes.first().copied() else {
            continue;
        };
        if axes
            .iter()
            .skip(1)
            .any(|other| axis.dot(*other).abs() < 0.9999)
        {
            continue;
        }
        let index = ir.model.curves.len();
        let id = CurveId(format!("catia:standard:circle#{index}"));
        annotate(
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            circle.pos as u64,
            "curve_support_60_circle",
            Exactness::ByteExact,
        );
        annotations.derived(&id, "geometry.axis");
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Circle {
                center: circle.center,
                axis,
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
                radius: circle.radius,
            },
            source_object: Some(cgm_source("edge-support", circle.tag)),
        });
    }
}

fn circle_axis_from_endpoints(
    center: Point3,
    radius: f64,
    start: Point3,
    end: Point3,
) -> Option<Vector3> {
    let start_radius = start.vector_from(center);
    let end_radius = end.vector_from(center);
    let start_length = start_radius.norm();
    let end_length = end_radius.norm();
    if (start_length - radius).abs() > 1e-3 || (end_length - radius).abs() > 1e-3 {
        return None;
    }
    let normal = start_radius.cross(end_radius);
    (normal.norm() > 1e-6 * start_length * end_length)
        .then(|| unit_vector(normal))
        .flatten()
}

fn full_circle_frame(
    center: Point3,
    radius: f64,
    axis: Vector3,
    start: Point3,
) -> Option<(Vector3, Vector3)> {
    const TOLERANCE: f64 = 2e-3;

    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let axis = unit_vector(axis)?;
    let radial = start.vector_from(center);
    let radial_length = radial.norm();
    if !radial_length.is_finite() || (radial_length - radius).abs() > TOLERANCE {
        return None;
    }
    let ref_direction = unit_vector(radial)?;
    (axis.dot(ref_direction).abs() <= TOLERANCE).then_some((axis, ref_direction))
}

fn canonical_unoriented_axis(axis: Vector3) -> Option<Vector3> {
    let axis = unit_vector(axis)?;
    let value = [axis.x, axis.y, axis.z]
        .into_iter()
        .max_by(|left, right| left.abs().total_cmp(&right.abs()))?;
    Some(if value.is_sign_negative() {
        axis.scale(-1.0)
    } else {
        axis
    })
}

fn standard_circle_axis_from_carrier(
    center: Point3,
    circle_radius: f64,
    surface: &SurfaceGeometry,
) -> Option<Vector3> {
    if let SurfaceGeometry::Sphere {
        center: sphere_center,
        radius: sphere_radius,
        ..
    } = surface
    {
        let center_distance = center.distance(*sphere_center);
        if center_distance <= SPHERE_CENTER_COINCIDENCE_TOLERANCE
            && close_length(circle_radius, *sphere_radius)
        {
            return None;
        }
    }
    circle_axis_from_carrier(center, circle_radius, surface)
}

pub(crate) fn circle_axis_from_carrier(
    center: Point3,
    circle_radius: f64,
    surface: &SurfaceGeometry,
) -> Option<Vector3> {
    match surface {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            close_length(center.vector_from(*origin).dot(*normal), 0.0).then_some(*normal)
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let offset = center.vector_from(*origin);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            (close_length(radial.norm(), 0.0) && close_length(circle_radius, *radius))
                .then_some(*axis)
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            half_angle,
            ..
        } => {
            let offset = center.vector_from(*origin);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            let section_radius = (radius + axial * half_angle.tan()).abs();
            (close_length(radial.norm(), 0.0) && close_length(circle_radius, section_radius))
                .then_some(*axis)
        }
        SurfaceGeometry::Sphere {
            center: sphere_center,
            radius: sphere_radius,
            ..
        } => {
            let offset = center.vector_from(*sphere_center);
            let distance = offset.x.hypot(offset.y).hypot(offset.z);
            (distance.is_finite()
                && distance != 0.0
                && close_squared(
                    distance * distance + circle_radius * circle_radius,
                    sphere_radius * sphere_radius,
                ))
            .then(|| offset.scale(1.0 / distance))
        }
        SurfaceGeometry::Torus {
            center: torus_center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let offset = center.vector_from(*torus_center);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            let radial_distance = radial.norm();
            if close_length(axial, 0.0)
                && close_length(radial_distance, *major_radius)
                && close_length(circle_radius, *minor_radius)
            {
                unit_vector((*axis).cross(radial))
            } else if close_length(radial_distance, 0.0)
                && close_squared(
                    (circle_radius - major_radius).powi(2) + axial * axial,
                    minor_radius * minor_radius,
                )
            {
                Some(*axis)
            } else {
                None
            }
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

pub(crate) fn close_length(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-5 * (1.0 + left.abs().max(right.abs()))
}

pub(crate) fn close_squared(left: f64, right: f64) -> bool {
    (left - right).abs() <= 2e-5 * (1.0 + left.abs().max(right.abs()))
}

pub(crate) fn attach_standard_lines(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
    edge_count: Option<usize>,
) {
    for line in crate::families::standard::records::standard_lines(brep, bindings.len(), edge_count)
    {
        let Some((origin_a, normal_a)) = plane_for_face(ir, bindings, line.faces[0]) else {
            continue;
        };
        let Some((origin_b, normal_b)) = plane_for_face(ir, bindings, line.faces[1]) else {
            continue;
        };
        let Some((origin, direction)) =
            plane_intersection_line(origin_a, normal_a, origin_b, normal_b)
        else {
            continue;
        };
        let index = ir.model.curves.len();
        let id = CurveId(format!("catia:standard:line#{index}"));
        annotate(
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            line.pos as u64,
            "curve_support_60_line",
            Exactness::ByteExact,
        );
        annotations
            .derived(&id, "geometry.origin")
            .derived(&id, "geometry.direction");
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Line { origin, direction },
            source_object: Some(cgm_source("edge-support", line.tag)),
        });
    }
}

fn plane_intersection_line(
    origin_a: Point3,
    normal_a: Vector3,
    origin_b: Point3,
    normal_b: Vector3,
) -> Option<(Point3, Vector3)> {
    let direction = normal_a.cross(normal_b);
    let direction_length = direction.x.hypot(direction.y).hypot(direction.z);
    if !direction_length.is_finite() || direction_length == 0.0 {
        return None;
    }
    let direction = direction.scale(1.0 / direction_length);
    let d_a = normal_a.dot(Vector3::new(origin_a.x, origin_a.y, origin_a.z));
    let d_b = normal_b.dot(Vector3::new(origin_b.x, origin_b.y, origin_b.z));
    let numerator = Vector3::new(
        d_a * normal_b.x - d_b * normal_a.x,
        d_a * normal_b.y - d_b * normal_a.y,
        d_a * normal_b.z - d_b * normal_a.z,
    );
    let scaled_origin = numerator.cross(direction);
    let origin = Point3::new(
        scaled_origin.x / direction_length,
        scaled_origin.y / direction_length,
        scaled_origin.z / direction_length,
    );
    [origin.x, origin.y, origin.z]
        .into_iter()
        .all(f64::is_finite)
        .then_some((origin, direction))
}

pub(crate) fn plane_for_face(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    face: usize,
) -> Option<(cadmpeg_ir::math::Point3, Vector3)> {
    let (surface_id, _, _) = bindings.get(face)?;
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)?;
    match &surface.geometry {
        SurfaceGeometry::Plane { origin, normal, .. } => Some((*origin, *normal)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod circle_axis_tests {
    use super::{circle_axis_from_carrier, standard_circle_axis_from_carrier};
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    fn x() -> Vector3 {
        Vector3::new(1.0, 0.0, 0.0)
    }

    fn y() -> Vector3 {
        Vector3::new(0.0, 1.0, 0.0)
    }

    fn z() -> Vector3 {
        Vector3::new(0.0, 0.0, 1.0)
    }

    fn origin() -> Point3 {
        Point3::new(0.0, 0.0, 0.0)
    }

    #[test]
    fn circle_axes_follow_exact_carrier_sections() {
        let plane = SurfaceGeometry::Plane {
            origin: origin(),
            normal: z(),
            u_axis: x(),
        };
        assert_eq!(circle_axis_from_carrier(origin(), 2.0, &plane), Some(z()));

        let cylinder = SurfaceGeometry::Cylinder {
            origin: origin(),
            axis: z(),
            ref_direction: x(),
            radius: 2.0,
        };
        assert_eq!(
            circle_axis_from_carrier(origin(), 2.0, &cylinder),
            Some(z())
        );
        assert_eq!(circle_axis_from_carrier(origin(), 3.0, &cylinder), None);

        let sphere = SurfaceGeometry::Sphere {
            center: origin(),
            axis: z(),
            ref_direction: x(),
            radius: 5.0,
        };
        assert_eq!(
            circle_axis_from_carrier(Point3::new(0.0, 0.0, 3.0), 4.0, &sphere),
            Some(z())
        );
        assert_eq!(circle_axis_from_carrier(origin(), 5.0, &sphere), None);

        let tiny = 1e-200;
        let unit_sphere = SurfaceGeometry::Sphere {
            center: origin(),
            axis: z(),
            ref_direction: x(),
            radius: 1.0,
        };
        assert_eq!(
            circle_axis_from_carrier(Point3::new(tiny, 0.0, 0.0), 1.0, &unit_sphere),
            Some(x())
        );

        let torus = SurfaceGeometry::Torus {
            center: origin(),
            axis: z(),
            ref_direction: x(),
            major_radius: 10.0,
            minor_radius: 2.0,
        };
        assert_eq!(
            circle_axis_from_carrier(Point3::new(10.0, 0.0, 0.0), 2.0, &torus),
            Some(y())
        );
        assert_eq!(
            circle_axis_from_carrier(Point3::new(0.0, 0.0, 2.0), 10.0, &torus),
            Some(z())
        );
    }

    #[test]
    fn centered_sphere_does_not_override_a_cylinder_axis() {
        let center = Point3::new(1e-10, 0.0, -1e-10);
        let sphere = SurfaceGeometry::Sphere {
            center: origin(),
            axis: z(),
            ref_direction: x(),
            radius: 3.175,
        };
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(center.x, 0.0, center.z),
            axis: y(),
            ref_direction: x(),
            radius: 3.175,
        };

        assert_eq!(
            standard_circle_axis_from_carrier(center, 3.175, &sphere),
            None
        );
        assert_eq!(
            standard_circle_axis_from_carrier(center, 3.175, &cylinder),
            Some(y())
        );
    }

    #[test]
    fn unoriented_circle_axes_use_one_parameter_frame() {
        const AXIS_COMPONENT_TOLERANCE: f64 = 1e-12;

        assert_eq!(super::canonical_unoriented_axis(z()), Some(z()));
        assert_eq!(
            super::canonical_unoriented_axis(Vector3::new(0.0, 0.0, -1.0)),
            Some(z())
        );
        let axis =
            super::canonical_unoriented_axis(Vector3::new(-2.0, 1.0, 0.0)).expect("finite axis");
        let length = 5.0_f64.sqrt();
        assert!((axis.x - 2.0 / length).abs() < AXIS_COMPONENT_TOLERANCE);
        assert!((axis.y + 1.0 / length).abs() < AXIS_COMPONENT_TOLERANCE);
    }
}
