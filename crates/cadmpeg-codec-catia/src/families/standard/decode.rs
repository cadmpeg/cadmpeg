// SPDX-License-Identifier: Apache-2.0
//! Standard nested-stream decode route: B-rep topology attach and geometry.

use cadmpeg_core::convert::f32_from_f64;
use cadmpeg_core::decode::u64_from_index;

type StandardProcedureOutputs = Result<
    (
        HashMap<u32, SurfaceGeometry>,
        HashMap<u32, StandardSurfaceProcedure>,
    ),
    CodecError,
>;
type NativeEndpointEvidenceOutput =
    Result<Result<Option<Vec<Option<[usize; 2]>>>, &'static str>, CodecError>;

mod edge_geometry;
mod surface_membership;
use edge_geometry::{
    attach_standard_circles, attach_standard_lines, build_standard_edge_curve,
    canonical_unoriented_axis, circle_endpoint_range_choices,
    circular_range_choices_have_simple_selection, point_on_surface, point_on_surface_if_supported,
    standard_circle_axis_from_carrier, standard_pcurve_geometry,
};

use crate::families::standard::fbb::EdgeTableForm;
use crate::families::standard::records::AnalyticSurfaceKind;
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, EntityRewrite, Model};
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::{Pcurve, PcurveGeometry},
    Curve, CurveGeometry, DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SupportPcurve, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use cadmpeg_ir::schema::EntitySchema;
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region,
    Sense, Shell, Vertex,
};
use cadmpeg_ir::units::{FiniteVector, OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::Exactness;
use cadmpeg_ir::{AnnotationBuilder, Annotations};
use serde::{de::DeserializeOwned, Serialize};
use serde_value::ValueDeserializer;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::assemble::cgm_source;
use crate::assemble::{
    annotate, build_geometry_report, circle_parameter_range_from_surface_branch,
    link_payload_carriers, neutral_model_is_admissible, ordered_range, preserve_raw_payload,
    rational_pcurve_arc, unwrap_angle, TypedCounts,
};
use crate::container::{self, ContainerScan};
use crate::families::b5::transfer::parameter_record_bounds;
use crate::families::freeform::{
    append_consolidated_revolutions, append_freeform_surface_pools, ConsolidatedRevolutionBinding,
};
use crate::families::standard::{fbb, topology};
use crate::families::{FamilyEntityAdmission, FamilyOutput};
use crate::loss::CatiaLossCode;
use crate::math::unit_vector;
use crate::nurbs::reverse_nurbs_curve;
use crate::solve::matching::{
    distinct_domain_matching_with_budget, retain_distinct_matching_supports,
};
use crate::solve::{mesh_gauge::MeshEdgeGeometry, mesh_quotient, missing_edge};
use crate::variant::Variant;
use crate::wire::records::ConsolidatedRecord;

const EPS_STANDARD_DECODE_COARSE_GEOMETRY: f64 = 1.0e-6;
const EPS_STANDARD_DECODE_RELAXED_GEOMETRY: f64 = 1.0e-7;
const EPS_STANDARD_DECODE_GEOMETRY: f64 = 1.0e-9;

const EPS_PARAM_RESOLUTION_SPAN: f64 = EPS_STANDARD_DECODE_RELAXED_GEOMETRY;
const EPS_PARAM_TOLERANCE_SPAN: f64 = EPS_STANDARD_DECODE_GEOMETRY;
const EPS_SAME_CONE_GENERATOR: f64 = 2e-3;
const EPS_ANTIPODAL_CIRCLE: f64 = 2e-3;
const SPHERE_SECTION_ENDPOINT_TOLERANCE: f64 = 2e-3;
const SPHERE_CENTER_COINCIDENCE_TOLERANCE: f64 = 2e-3;
const CYLINDER_PLANE_CONIC_TOLERANCE: f64 = 2e-3;
const PERPENDICULAR_CYLINDER_CONIC_TOLERANCE: f64 = 2e-3;
const LINE_SEGMENT_GEOMETRY_TOLERANCE: f64 = 2e-3;
const ANALYTIC_CURVE_ENDPOINT_TOLERANCE: f64 = 2e-3;
const SUPPORT_AGREEMENT_TOLERANCE: f64 = EPS_STANDARD_DECODE_COARSE_GEOMETRY;
const STANDARD_FACE_BOUNDS_TOLERANCE: f64 = 2e-3;
const NURBS_SURFACE_MEMBERSHIP_TOLERANCE: f64 = 2e-3;
const NURBS_SHARED_BOUNDARY_TOLERANCE: f64 = EPS_STANDARD_DECODE_GEOMETRY;
const NURBS_SURFACE_SEEDS_PER_SPAN: usize = 3;
const NURBS_SURFACE_MAX_SEEDS: usize = 256;
const NURBS_SURFACE_REFINEMENT_ITERATIONS: usize = 24;
const NURBS_SURFACE_BACKTRACK_STEPS: usize = 8;
const NURBS_LINE_FACE_SAMPLES: [f64; 3] = [0.25, 0.5, 0.75];

fn bind_consolidated_revolution_faces_and_seams(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    revolutions: &[ConsolidatedRevolutionBinding],
) -> Result<(usize, usize), cadmpeg_core::CodecError> {
    const TOLERANCE: f64 = 2e-3;

    fn point_on_torus(point: Point3, geometry: &SurfaceGeometry, tolerance: f64) -> bool {
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) = geometry else {
            return false;
        };
        let center = torus_surface.center().get();
        let axis = torus_surface.frame().axis().as_raw();
        let major_radius = torus_surface.major_radius().get();
        let minor_radius = torus_surface.minor_radius().get();
        let offset = point.vector_from(center);
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
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) = geometry else {
            return None;
        };
        let center = torus_surface.center().get();
        let axis = torus_surface.frame().axis().as_raw();
        let major_radius = torus_surface.major_radius().get();
        let minor_radius = torus_surface.minor_radius().get();
        if !expected_sweep.is_finite()
            || expected_sweep <= 0.0
            || expected_sweep > std::f64::consts::PI
        {
            return None;
        }
        let start_offset = start.vector_from(center);
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
        if (sweep - expected_sweep).abs() > TOLERANCE / minor_radius {
            return None;
        }
        let normal = first.cross(second);
        let normal_norm = normal.norm();
        if !normal_norm.is_finite()
            || normal_norm == 0.0
            || (normal.dot(*axis) / normal_norm).abs() > EPS_STANDARD_DECODE_COARSE_GEOMETRY
        {
            return None;
        }
        Some((
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    circle_center,
                    Vector3::new(
                        normal.x / normal_norm,
                        normal.y / normal_norm,
                        normal.z / normal_norm,
                    ),
                    Vector3::new(
                        first.x / first_norm,
                        first.y / first_norm,
                        first.z / first_norm,
                    ),
                    minor_radius,
                )
                .ok()?,
            )),
            [0.0, expected_sweep],
        ))
    }

    let mut point_positions = HashMap::new();
    for point in &ir.model.points {
        let id = point
            .id
            .try_clone_for_decode(ctx, "catia_revolution_point_id_copy")?;
        ctx.insert_hash_map(
            &mut point_positions,
            id,
            point.position().get(),
            "catia_revolution_point_positions",
        )?;
    }
    let mut vertex_positions = HashMap::new();
    for vertex in &ir.model.vertices {
        if let Some(&position) = point_positions.get(&vertex.point) {
            let id = vertex
                .id
                .try_clone_for_decode(ctx, "catia_revolution_vertex_id_copy")?;
            ctx.insert_hash_map(
                &mut vertex_positions,
                id,
                position,
                "catia_revolution_vertex_positions",
            )?;
        }
    }
    let mut edge_indices = HashMap::new();
    for (index, edge) in ir.model.edges.iter().enumerate() {
        let id = edge
            .id
            .try_clone_for_decode(ctx, "catia_revolution_edge_id_copy")?;
        ctx.insert_hash_map(
            &mut edge_indices,
            id,
            index,
            "catia_revolution_edge_indices",
        )?;
    }
    let mut coedge_indices = HashMap::new();
    for (index, coedge) in ir.model.coedges.iter().enumerate() {
        let id = coedge
            .id
            .try_clone_for_decode(ctx, "catia_revolution_coedge_id_copy")?;
        ctx.insert_hash_map(
            &mut coedge_indices,
            id,
            index,
            "catia_revolution_coedge_indices",
        )?;
    }
    let mut loop_indices = HashMap::new();
    for (index, loop_) in ir.model.loops.iter().enumerate() {
        let id = loop_
            .id
            .try_clone_for_decode(ctx, "catia_revolution_loop_id_copy")?;
        ctx.insert_hash_map(
            &mut loop_indices,
            id,
            index,
            "catia_revolution_loop_indices",
        )?;
    }
    let mut unknown_surfaces = HashSet::new();
    for surface in &ir.model.surfaces {
        if matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        ) {
            let id = surface
                .id
                .try_clone_for_decode(ctx, "catia_revolution_unknown_surface_id_copy")?;
            ctx.insert_hash_set(
                &mut unknown_surfaces,
                id,
                "catia_revolution_unknown_surfaces",
            )?;
        }
    }
    let mut curve_indices = HashMap::new();
    for (index, curve) in ir.model.curves.iter().enumerate() {
        let id = curve
            .id
            .try_clone_for_decode(ctx, "catia_revolution_curve_id_copy")?;
        ctx.insert_hash_map(
            &mut curve_indices,
            id,
            index,
            "catia_revolution_curve_indices",
        )?;
    }
    let mut surface_bindings = HashMap::<SurfaceId, Option<usize>>::new();
    for face in &ir.model.faces {
        if !unknown_surfaces.contains(&face.surface) {
            continue;
        }
        let face_edges = face
            .loops
            .iter()
            .filter_map(|id| loop_indices.get(id))
            .flat_map(|index| ir.model.loops[*index].coedges())
            .filter_map(|id| coedge_indices.get(id))
            .filter_map(|index| edge_indices.get(&ir.model.coedges[*index].edge));
        let mut witnesses = Vec::new();
        for index in face_edges {
            let edge = &ir.model.edges[*index];
            for id in [&edge.start, &edge.end] {
                if let Some(&point) = vertex_positions.get(id) {
                    ctx.push_vec(&mut witnesses, point, "catia_revolution_face_witnesses")?;
                }
            }
            let Some(curve) = edge
                .curve()
                .and_then(|id| curve_indices.get(id))
                .map(|index| &ir.model.curves[*index].geometry)
            else {
                continue;
            };
            let Some([start, end]) = edge.param_range().map(cadmpeg_ir::units::FiniteVector::get)
            else {
                continue;
            };
            let parameter = start.midpoint(end);
            if let Some(point) = cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::decode::curve_point_for_decode(ctx, curve, parameter)?,
            )? {
                ctx.push_vec(
                    &mut witnesses,
                    point.get(),
                    "catia_revolution_face_witnesses",
                )?;
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
        if let Some(stored) = surface_bindings.get_mut(&face.surface) {
            if *stored != Some(binding) {
                *stored = None;
            }
        } else {
            let id = face
                .surface
                .try_clone_for_decode(ctx, "catia_revolution_binding_surface_id_copy")?;
            ctx.insert_hash_map(
                &mut surface_bindings,
                id,
                Some(binding),
                "catia_revolution_surface_bindings",
            )?;
        }
    }
    surface_bindings.retain(|_, binding| binding.is_some());
    for (surface_id, binding) in &surface_bindings {
        if let Some(surface) = ir
            .model
            .surfaces
            .iter_mut()
            .find(|surface| &surface.id == surface_id)
        {
            let Some(binding) = *binding else { continue };
            surface.geometry = revolutions[binding].geometry.try_clone_for_decode(ctx, "catia_revolution_surface_geometry_copy")?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &surface.id,
                "geometry",
                "catia_annotation_field",
            )?;
        }
    }

    let mut procedural_bindings = HashMap::<CurveId, Option<usize>>::new();
    for procedure in &ir.model.procedural_curves {
        let binding = (|| {
            let ProceduralCurveDefinition::Intersection { context, .. } = procedure.definition()
            else {
                return None;
            };
            let [Some(first), Some(second)] =
                std::array::from_fn(|side| context.sides()[side].surface.as_ref())
            else {
                return None;
            };
            let binding = (*surface_bindings.get(first)?)?;
            (surface_bindings.get(second) == Some(&Some(binding))).then_some(binding)
        })();
        let Some(binding) = binding else {
            continue;
        };
        let Some(owner) = ir.model.procedural_curve_owner(&procedure.id) else {
            continue;
        };
        if let Some(stored) = procedural_bindings.get_mut(owner) {
            *stored = None;
        } else {
            let id = owner.try_clone_for_decode(ctx, "catia_revolution_owner_id_copy")?;
            ctx.insert_hash_map(
                &mut procedural_bindings,
                id,
                Some(binding),
                "catia_revolution_procedural_bindings",
            )?;
        }
    }
    let mut curve_edge_counts = HashMap::<CurveId, usize>::new();
    for curve in ir.model.edges.iter().filter_map(|edge| edge.curve()) {
        if let Some(count) = curve_edge_counts.get_mut(curve) {
            *count += 1;
        } else {
            let id = curve.try_clone_for_decode(ctx, "catia_revolution_count_curve_id_copy")?;
            ctx.insert_hash_map(
                &mut curve_edge_counts,
                id,
                1,
                "catia_revolution_curve_edge_counts",
            )?;
        }
    }
    let mut seam_count = 0usize;
    for edge in &mut ir.model.edges {
        let Some(curve_id) = edge.curve() else {
            continue;
        };
        let Some(&curve_index) = curve_indices.get(curve_id) else {
            continue;
        };
        let unresolved = match &ir.model.curves[curve_index].geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }) => true,
            CurveGeometry::Procedural { cache, .. } => cache
                .as_ref()
                .is_none_or(|cache| matches!(cache, SolvedCurveGeometry::Unknown { .. })),
            CurveGeometry::Solved(_) => false,
        };
        if !unresolved || curve_edge_counts.get(curve_id) != Some(&1) {
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
        match &mut ir.model.curves[curve_index].geometry {
            CurveGeometry::Procedural { cache, .. } => {
                let CurveGeometry::Solved(solved) = geometry else {
                    continue;
                };
                *cache = Some(solved);
            }
            carrier @ CurveGeometry::Solved(_) => *carrier = geometry,
        }
        let curve = edge
            .curve()
            .map(|id| id.try_clone_for_decode(ctx, "catia_revolution_seam_curve_id_copy"))
            .transpose()?;
        edge.carrier = cadmpeg_ir::topology::EdgeCarrier::new(curve, Some(parameter_range))
            .map_err(cadmpeg_core::CodecError::malformed)?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &ir.model.curves[curve_index].id,
            "geometry",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &edge.id,
            "param_range",
            "catia_annotation_field",
        )?;
        seam_count += 1;
    }
    Ok((surface_bindings.len(), seam_count))
}

#[cfg(test)]
mod consolidated_revolution_binding_tests {
    use super::bind_consolidated_revolution_faces_and_seams;
    use crate::families::freeform::ConsolidatedRevolutionBinding;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralCurveDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
        SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ProceduralCurveId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};
    use cadmpeg_ir::AnnotationBuilder;

    #[test]
    fn revolution_binding_refuses_point_index_before_allocation() {
        let mut ir = CadIr::empty();
        ir.model.points.push(Point::new(
            PointId::mint("catia:test:point#revolution-limit").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                .expect("finite point"),
            None,
        ));
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            bind_consolidated_revolution_faces_and_seams(
                ctx,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &[],
            )
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                bind_consolidated_revolution_faces_and_seams(
                    ctx,
                    &mut ir,
                    &mut AnnotationBuilder::new(),
                    &[],
                )
            })
            .expect("service context admits one point"),
            (0, 0)
        );
    }

    #[test]
    fn torus_binding_uses_a_finite_midpoint_across_a_wide_curve_range() {
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;

        let mut ir = CadIr::empty();
        let surface_id =
            SurfaceId::mint("catia:test:surface#wide-torus").expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        let torus = |minor_radius| {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    if minor_radius == 2.0 { 5.0 } else { 6.0 },
                    minor_radius,
                )
                .expect("finite torus"),
            ))
        };
        let expected = torus(2.0);
        let curve_id =
            CurveId::mint("catia:test:curve#wide-torus-witness").expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    2,
                    vec![
                        -f64::MAX,
                        -f64::MAX,
                        -f64::MAX,
                        f64::MAX,
                        f64::MAX,
                        f64::MAX,
                    ],
                    vec![
                        Point3::new(7.0, 0.0, 0.0),
                        Point3::new(3.0, 0.0, 4.0),
                        Point3::new(7.0, 0.0, 0.0),
                    ],
                    None,
                    false,
                )
                .expect("finite wide curve"),
            )),
            source_object: None,
        });
        let vertices = [0, 1].map(|ordinal| {
            let point_id = PointId::mint(format!("catia:test:point#wide-torus-{ordinal}"))
                .expect("identity grammar");
            let vertex_id = VertexId::mint(format!("catia:test:vertex#wide-torus-{ordinal}"))
                .expect("identity grammar");
            ir.model.points.push(Point::new(
                point_id.clone(),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(7.0, 0.0, 0.0))
                    .expect("finite endpoint"),
                None,
            ));
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            vertex_id
        });
        let edge_id = EdgeId::mint("catia:test:edge#wide-torus-witness").expect("identity grammar");
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id),
                Some([-f64::MAX, f64::MAX]),
            )
            .expect("finite edge range"),
            start: vertices[0].clone(),
            end: vertices[1].clone(),
            tolerance: None,
        });
        let face_id = FaceId::mint("catia:test:face#wide-torus").expect("identity grammar");
        let loop_id = LoopId::mint("catia:test:loop#wide-torus").expect("identity grammar");
        let coedge_id = CoedgeId::mint("catia:test:coedge#wide-torus").expect("identity grammar");
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: ShellId::mint("catia:test:shell#wide-torus").expect("identity grammar"),
            surface: surface_id,
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id,
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(vec![coedge_id.clone()], Vec::new())
                    .expect("one-edge loop"),
            ),
        });
        ir.model.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id,
            edge: edge_id,
            radial_next: coedge_id,
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });

        let result = crate::test_support::with_service_context(|ctx| {
            bind_consolidated_revolution_faces_and_seams(
                ctx,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &[
                    ConsolidatedRevolutionBinding {
                        geometry: expected.clone(),
                        profile_sweep: 0.5,
                    },
                    ConsolidatedRevolutionBinding {
                        geometry: torus(1.0),
                        profile_sweep: 0.5,
                    },
                ],
            )
        })
        .expect("finite torus witnesses");
        assert_eq!(result, (1, 0));
        assert_eq!(ir.model.surfaces[0].geometry, expected);
    }

    #[test]
    fn one_revolution_torus_closes_face_aliases_and_meridian_seam() {
        let mut ir = CadIr::empty();
        let surface_ids = [
            SurfaceId::mint("catia:test:surface#face-surface%230".to_string())
                .expect("identity grammar"),
            SurfaceId::mint("catia:test:surface#face-surface%231".to_string())
                .expect("identity grammar"),
        ];
        for id in &surface_ids {
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
            });
        }
        let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
                3.0,
            )
            .expect("valid TorusSurface fixture"),
        ));
        let profile_end = std::f64::consts::PI - 0.5;
        let positions = [
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(2.0 + 3.0 * profile_end.cos(), 0.0, 3.0 * profile_end.sin()),
        ];
        for (index, position) in positions.into_iter().enumerate() {
            let point = PointId::mint(format!("catia:test:point#point%23{index}"))
                .expect("identity grammar");
            ir.model.points.push(Point::new(
                point.clone(),
                cadmpeg_ir::features::FinitePoint3::new(position)
                    .expect("a finite position is a point"),
                None,
            ));
            ir.model.vertices.push(Vertex {
                id: VertexId::mint(format!("catia:test:vertex#vertex%23{index}"))
                    .expect("identity grammar"),
                point,
                tolerance: None,
            });
        }
        let curve_id =
            CurveId::mint("catia:test:curve#seam-curve".to_string()).expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId::mint("catia:test:edge#seam-edge".to_string()).expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([0.0, 1.0]),
            )
            .expect("valid edge carrier"),
            start: VertexId::mint("catia:test:vertex#vertex%230".to_string())
                .expect("identity grammar"),
            end: VertexId::mint("catia:test:vertex#vertex%231".to_string())
                .expect("identity grammar"),
            tolerance: None,
        });
        for (side, surface) in surface_ids.iter().enumerate() {
            let face =
                FaceId::mint(format!("catia:test:face#face%23{side}")).expect("identity grammar");
            let loop_id =
                LoopId::mint(format!("catia:test:loop#loop%23{side}")).expect("identity grammar");
            let coedge = CoedgeId::mint(format!("catia:test:coedge#coedge%23{side}"))
                .expect("identity grammar");
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: ShellId::mint("catia:test:shell#shell".to_string())
                    .expect("identity grammar"),
                surface: surface.clone(),
                sense: Sense::Forward,
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
                name: None,
                color: None,
                tolerance: None,
            });
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(vec![coedge.clone()], Vec::new())
                        .expect("valid loop ring"),
                ),
            });
            ir.model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id,
                edge: EdgeId::mint("catia:test:edge#seam-edge".to_string())
                    .expect("identity grammar"),
                radial_next: CoedgeId::mint(format!("catia:test:coedge#coedge%23{}", 1 - side))
                    .expect("identity grammar"),
                sense: if side == 0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
            });
        }
        ir.model
            .add_procedural_curve(
                &curve_id,
                ProceduralCurve::new(
                    ProceduralCurveId::mint(
                        "catia:test:proceduralcurve#seam-construction".to_string(),
                    )
                    .expect("identity grammar"),
                    ProceduralCurveDefinition::Intersection {
                        context: IntcurveSupportContext::try_new(
                            std::array::from_fn(|side| IntcurveSupportSide {
                                surface: Some(surface_ids[side].clone()),
                                pcurve: None,
                            }),
                            [0.0, 1.0],
                            std::array::from_fn(|_| Vec::new()),
                        )
                        .expect("valid IntcurveSupportContext fixture"),
                        discontinuity_flag: false,
                        cache: None,
                    },
                ),
            )
            .expect("attach construction to its fixture carrier");

        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                bind_consolidated_revolution_faces_and_seams(
                    ctx,
                    &mut ir,
                    &mut AnnotationBuilder::new(),
                    &[ConsolidatedRevolutionBinding {
                        geometry: geometry.clone(),
                        profile_sweep: 0.5,
                    }],
                )
            })
            .expect("valid exactness fields"),
            (2, 1)
        );
        assert!(ir
            .model
            .surfaces
            .iter()
            .all(|surface| surface.geometry == geometry));
        assert!(
            matches!(ir.model.curves[0].geometry.solved_cache(), Some(SolvedCurveGeometry::Circle(circle_curve)) if { circle_curve.radius().get() == 3.0 })
        );
        assert_eq!(
            ir.model.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([0.0, 0.5])
        );
    }
}

fn refine_consolidated_analytic_surfaces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    surfaces: &mut [Option<SurfaceGeometry>],
) -> Result<HashMap<usize, usize>, cadmpeg_core::CodecError> {
    fn exactly_one<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
        let value = values.next()?;
        values.next().is_none().then_some(value)
    }

    let cylinders = ctx.collect_vec(
        crate::families::b2::records::b2_cylinders_from_records(bytes, records),
        "catia_standard_refined_cylinders",
    )?;
    let cones = ctx.collect_vec(
        crate::families::b2::records::b2_cones_from_records(bytes, records),
        "catia_standard_refined_cones",
    )?;
    let spheres = ctx.collect_vec(
        crate::families::b2::records::b2_spheres_from_records(bytes, records),
        "catia_standard_refined_spheres",
    )?;
    let tori = ctx.collect_vec(
        crate::families::b2::records::b2_tori_from_records(bytes, records),
        "catia_standard_refined_tori",
    )?;
    let quantized = |value: f64| f32_from_f64(value).map(f64::from);
    let same_point = |point: Point3, stored: [f64; 3]| {
        quantized(stored[0]).is_some_and(|value| point.x.to_bits() == value.to_bits())
            && quantized(stored[1]).is_some_and(|value| point.y.to_bits() == value.to_bits())
            && quantized(stored[2]).is_some_and(|value| point.z.to_bits() == value.to_bits())
    };
    let same_axis = |axis: Vector3, stored: [f64; 3]| {
        let Some(x) = f32_from_f64(stored[0]) else {
            return false;
        };
        let Some(y) = f32_from_f64(stored[1]) else {
            return false;
        };
        let z = (1.0 - f64::from(x * x + y * y))
            .max(0.0)
            .sqrt()
            .copysign(stored[2]);
        unit_vector(Vector3::new(f64::from(x), f64::from(y), z)).is_some_and(|reconstructed| {
            axis.x.to_bits() == reconstructed.as_raw().x.to_bits()
                && axis.y.to_bits() == reconstructed.as_raw().y.to_bits()
                && axis.z.to_bits() == reconstructed.as_raw().z.to_bits()
        })
    };
    let mut refined = HashMap::new();
    for (index, surface) in surfaces.iter_mut().enumerate() {
        let replacement = match surface.as_ref() {
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))) => {
                let origin = cylinder_surface.origin().get();
                let axis = cylinder_surface.frame().axis().as_raw();
                let radius = cylinder_surface.radius().get();
                exactly_one(cylinders.iter().filter(|cylinder| {
                    same_point(origin, cylinder.origin.get().into())
                        && same_axis(*axis, cylinder.frame.axis().get())
                        && quantized(cylinder.radius.get())
                            .is_some_and(|value| radius.to_bits() == value.to_bits())
                }))
                .map(|cylinder| (cylinder.surface_geometry(), cylinder.pos))
            }
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)))
                if {
                    let radius = cone_surface.radius().get();
                    let ratio = cone_surface.ratio().get();
                    radius == 0.0 && ratio == 1.0
                } =>
            {
                let origin = cone_surface.origin().get();
                let axis = cone_surface.frame().axis().as_raw();
                let half_angle = cone_surface.half_angle().get();
                exactly_one(cones.iter().filter(|cone| {
                    same_point(origin, cone.apex.get().into())
                        && same_axis(*axis, cone.frame.axis().get())
                        && quantized(cone.half_angle.get())
                            .is_some_and(|value| half_angle.to_bits() == value.to_bits())
                }))
                .map(|cone| {
                    (
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                            cadmpeg_ir::geometry::analytic::ConeSurface::new(
                                cone.apex,
                                cone.frame.into(),
                                cadmpeg_ir::scalar::NonNegativeLength::ZERO,
                                cadmpeg_ir::scalar::PositiveReal::ONE,
                                cone.half_angle,
                            ),
                        )),
                        cone.pos,
                    )
                })
            }
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface))) => {
                let center = sphere_surface.center().get();
                let radius = sphere_surface.radius().get();
                exactly_one(spheres.iter().filter(|sphere| {
                    same_point(center, sphere.center.get().into())
                        && quantized(sphere.radius.get())
                            .is_some_and(|value| radius.to_bits() == value.to_bits())
                }))
                .map(|sphere| {
                    (
                        crate::families::b2::records::b2_sphere_geometry(sphere),
                        sphere.pos,
                    )
                })
            }
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface))) => {
                let center = torus_surface.center().get();
                let axis = torus_surface.frame().axis().as_raw();
                let major_radius = torus_surface.major_radius().get();
                let minor_radius = torus_surface.minor_radius().get();
                exactly_one(tori.iter().filter(|torus| {
                    same_point(center, torus.center.get().into())
                        && same_axis(*axis, torus.frame.axis().get())
                        && quantized(torus.major_radius.get())
                            .is_some_and(|value| major_radius.to_bits() == value.to_bits())
                        && quantized(torus.minor_radius.get())
                            .is_some_and(|value| minor_radius.to_bits() == value.to_bits())
                }))
                .map(|torus| {
                    (
                        crate::families::b2::records::b2_torus_geometry(torus),
                        torus.pos,
                    )
                })
            }
            _ => None,
        };
        if let Some((geometry, source_pos)) = replacement {
            *surface = Some(geometry);
            ctx.insert_hash_map(
                &mut refined,
                index,
                source_pos,
                "catia_standard_refined_surface_sources",
            )?;
        }
    }
    Ok(refined)
}

#[cfg(test)]
mod consolidated_analytic_refinement_tests {
    use super::refine_consolidated_analytic_surfaces;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn refined_analytic_surfaces(
        bytes: &[u8],
        records: &[crate::wire::records::ConsolidatedRecord],
        surfaces: &mut [Option<SurfaceGeometry>],
    ) -> std::collections::HashMap<usize, usize> {
        crate::test_support::with_service_context(|ctx| {
            refine_consolidated_analytic_surfaces(ctx, bytes, records, surfaces)
                .expect("service decode")
        })
    }

    #[test]
    fn cylinder_refinement_refuses_cylinder_collection_before_copy() {
        let bytes = crate::test_support::test_b2::b2_cylinder_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let mut surfaces = [];
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            refine_consolidated_analytic_surfaces(ctx, &bytes, &records, &mut surfaces)
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_standard_refined_cylinders")
        );
        let refined = crate::test_support::with_service_context(|ctx| {
            refine_consolidated_analytic_surfaces(ctx, &bytes, &records, &mut surfaces)
        })
        .expect("service decode");
        assert!(refined.is_empty());
    }

    #[test]
    fn analytic_refinement_refuses_each_parsed_carrier_collection() {
        for (bytes, operation) in [
            (
                crate::test_support::test_b2::b2_cone_stream(),
                "catia_standard_refined_cones",
            ),
            (
                crate::test_support::test_b2::b2_sphere_stream(),
                "catia_standard_refined_spheres",
            ),
            (
                crate::test_support::test_b2::b2_torus_stream(),
                "catia_standard_refined_tori",
            ),
        ] {
            let records = crate::wire::records::consolidated_records(&bytes);
            let mut surfaces = [];
            let limited = crate::test_support::with_collection_limit(0, |ctx| {
                refine_consolidated_analytic_surfaces(ctx, &bytes, &records, &mut surfaces)
            });
            assert!(
                matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == operation)
            );
        }
    }

    #[test]
    fn unique_quantized_torus_refines_every_matching_face_to_binary64() {
        let mut bytes = crate::test_support::test_b2::b2_torus_stream();
        let exact_x = 1.000_000_01_f64;
        bytes[5..13].copy_from_slice(&exact_x.to_le_bytes());
        let coarse = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                Point3::new(
                    f64::from(
                        cadmpeg_core::convert::f32_from_f64(exact_x)
                            .expect("fixture value fits f32"),
                    ),
                    2.0,
                    3.0,
                ),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                7.0,
                2.0,
            )
            .expect("valid TorusSurface fixture"),
        ));
        let mut surfaces = vec![Some(coarse.clone()), Some(coarse)];
        let refined = refined_analytic_surfaces(
            &bytes,
            &crate::wire::records::consolidated_records(&bytes),
            &mut surfaces,
        );
        assert_eq!(refined, [(0, 0), (1, 0)].into());
        for surface in surfaces {
            assert!(
                matches!(surface, Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)))
                if {
                    let center = torus_surface.center().get();
                    center.x == exact_x
                })
            );
        }
    }

    fn refinement_refuses_out_of_range_center(center: f64, coarse_center: f64) {
        let mut bytes = crate::test_support::test_b2::b2_sphere_stream();
        bytes[5..13].copy_from_slice(&center.to_le_bytes());
        let coarse = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::new(coarse_center, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .expect("finite sphere fixture"),
        ));
        let records = crate::wire::records::consolidated_records(&bytes);
        let mut surfaces = [Some(coarse.clone())];
        assert!(refined_analytic_surfaces(&bytes, &records, &mut surfaces).is_empty());
        assert_eq!(surfaces, [Some(coarse)]);
    }

    #[test]
    fn analytic_refinement_refuses_positive_binary32_range_overflow() {
        let limit = f64::from(f32::MAX);
        refinement_refuses_out_of_range_center(limit.next_up(), limit);
    }

    #[test]
    fn analytic_refinement_refuses_negative_binary32_range_overflow() {
        let limit = -f64::from(f32::MAX);
        refinement_refuses_out_of_range_center(limit.next_down(), limit);
    }

    #[test]
    fn sphere_refinement_requires_one_matching_consolidated_carrier() {
        let mut bytes = crate::test_support::test_b2::b2_sphere_stream();
        let exact_x = 1.000_000_01_f64;
        bytes[5..13].copy_from_slice(&exact_x.to_le_bytes());
        let coarse = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::new(
                    f64::from(
                        cadmpeg_core::convert::f32_from_f64(exact_x)
                            .expect("fixture value fits f32"),
                    ),
                    2.0,
                    3.0,
                ),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .expect("valid SphereSurface fixture"),
        ));
        let mut unique = vec![Some(coarse.clone())];
        assert_eq!(
            refined_analytic_surfaces(
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                &mut unique,
            ),
            [(0, 0)].into()
        );
        assert!(
            matches!(unique[0], Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)))
            if {
                let center = sphere_surface.center().get();
                center.x == exact_x
            })
        );

        bytes.extend_from_slice(&bytes.clone());
        let mut ambiguous = vec![Some(coarse)];
        assert!(refined_analytic_surfaces(
            &bytes,
            &crate::wire::records::consolidated_records(&bytes),
            &mut ambiguous,
        )
        .is_empty());
    }

    #[test]
    fn cylinder_and_cone_refinement_use_their_complete_exact_frames() {
        let mut bytes = crate::test_support::test_b2::b2_cylinder_stream();
        bytes.extend_from_slice(&crate::test_support::test_b2::b2_cone_stream());
        let mut surfaces = vec![
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    2.0,
                )
                .expect("valid CylinderSurface fixture"),
            ))),
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    0.0,
                    1.0,
                    f64::from(0.25_f32),
                )
                .expect("valid ConeSurface fixture"),
            ))),
        ];
        assert_eq!(
            refined_analytic_surfaces(
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                &mut surfaces,
            )
            .len(),
            2
        );
        assert!(
            matches!(surfaces[0], Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)))
            if {
                let ref_direction = cylinder_surface.frame().reference().as_raw();
                *ref_direction == Vector3::new(0.0, 1.0, 0.0)
            })
        );
        assert!(
            matches!(surfaces[1], Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))) if { cone_surface.half_angle().get() == 0.25 })
        );
    }
}

/// Attach the standard route's unbound vertices to one wire body.
fn attach_free_vertices(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    if ir.model.vertices.is_empty() {
        return Ok(());
    }
    let body_id = BodyId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "standard", "body"),
        cadmpeg_ir::identity_key!("unbound-points"),
    );
    let region_id = RegionId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "standard", "region"),
        cadmpeg_ir::identity_key!("unbound-points"),
    );
    let shell_id = ShellId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "standard", "shell"),
        cadmpeg_ir::identity_key!("unbound-points"),
    );
    admission.reserve_entity(&mut ir.model.shells, "catia_standard_free_vertex_shells")?;
    let mut free_vertices = Vec::new();
    for vertex in &ir.model.vertices {
        let id = vertex
            .id
            .try_clone_for_decode(ctx, "catia_standard_free_vertex_id_copy")?;
        ctx.push_vec(&mut free_vertices, id, "catia_standard_free_vertex_members")?;
    }
    let shell = Shell::with_free_vertices(
        shell_id.try_clone_for_decode(ctx, "catia_standard_free_shell_id_copy")?,
        region_id.try_clone_for_decode(ctx, "catia_standard_free_shell_region_copy")?,
        free_vertices,
    )
    .map_err(cadmpeg_core::CodecError::malformed)?;
    for id in [body_id.as_str(), region_id.as_str(), shell_id.as_str()] {
        annotate(
            ctx,
            annotations,
            id,
            "MainDataStream+SurfacicReps",
            0,
            "unbound_point_owner",
            Exactness::Inferred,
        )?;
    }
    admission.reserve_entity(&mut ir.model.bodies, "catia_standard_free_vertex_bodies")?;
    let body_id_copy = body_id.try_clone_for_decode(ctx, "catia_standard_free_body_id_copy")?;
    let mut body_regions = Vec::new();
    ctx.push_vec(
        &mut body_regions,
        region_id.try_clone_for_decode(ctx, "catia_standard_free_body_region_copy")?,
        "catia_standard_free_body_regions",
    )?;
    ir.model.bodies.push(Body {
        id: body_id_copy,
        kind: BodyKind::Wire,
        regions: body_regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    admission.reserve_entity(&mut ir.model.regions, "catia_standard_free_vertex_regions")?;
    let mut region_shells = Vec::new();
    ctx.push_vec(
        &mut region_shells,
        shell_id,
        "catia_standard_free_region_shells",
    )?;
    ir.model.regions.push(Region {
        id: region_id,
        body: body_id,
        shells: region_shells,
    });
    ir.model.shells.push(shell);
    Ok(())
}

/// Materialize one exact object-stream support surface once.
fn standard_extrusion_support_id(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    surfaces: &mut Vec<Surface>,
    procedural_supports: &mut HashMap<u32, SurfaceId>,
    surface_object_id: u32,
    geometry: SurfaceGeometry,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<SurfaceId, cadmpeg_core::CodecError> {
    let source_object = cgm_source(ctx, "surface", surface_object_id)?;
    if let Some(id) = procedural_supports.get(&surface_object_id) {
        return id.try_clone_for_decode(ctx, "catia_extrusion_existing_support_id_copy");
    }
    let id = SurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "standard", "procedural-support"),
        surface_object_id,
    );
    annotate(
        ctx,
        annotations,
        &id,
        "object_stream_b5_03",
        0,
        format_args!("surface:{surface_object_id:08x}"),
        Exactness::ByteExact,
    )?;
    admission.reserve_entity(surfaces, "catia_standard_extrusion_support_surfaces")?;
    surfaces.push(Surface {
        id: id.try_clone_for_decode(ctx, "catia_extrusion_support_surface_id_copy")?,
        geometry,
        source_object: Some(source_object),
    });
    let map_id = id.try_clone_for_decode(ctx, "catia_extrusion_support_map_id_copy")?;
    ctx.insert_hash_map(
        procedural_supports,
        surface_object_id,
        map_id,
        "catia_extrusion_support_map",
    )?;
    Ok(id)
}

/// Emit one resolved object-stream extrusion construction in the standard family.
fn emit_standard_extrusion_definition(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    (surfaces, procedural_supports): (&mut Vec<Surface>, &mut HashMap<u32, SurfaceId>),
    extrusion_definitions: &mut HashMap<
        u32,
        cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction,
    >,
    extrusion: crate::families::b5::transfer::ResolvedExtrusionSurface,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    if let Some(definition) = extrusion_definitions.get(&extrusion.surface_object_id) {
        return copy_standard_extrusion_definition(ctx, definition);
    }
    let surface_object_id = extrusion.surface_object_id;
    let directrix_id = CurveId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "standard", "extrusion-directrix"),
        extrusion.directrix_object_id,
    );
    let parameter_range = extrusion.directrix_parameter_range;
    let direction = extrusion.direction;
    match extrusion.directrix {
        crate::families::b5::transfer::ResolvedExtrusionDirectrix::Intersection {
            supports,
            cache_fit_tolerance,
        } => {
            let [first, second] = *supports;
            let mut build_side = |side: crate::families::b5::transfer::ResolvedExtrusionSupport| {
                Ok::<_, cadmpeg_core::CodecError>(IntcurveSupportSide {
                    surface: Some(standard_extrusion_support_id(
                        ctx,
                        annotations,
                        surfaces,
                        procedural_supports,
                        side.surface_object_id,
                        side.surface,
                        admission,
                    )?),
                    pcurve: Some(SupportPcurve::new(
                        side.pcurve,
                        (side.pcurve_parameter_range
                            != extrusion.directrix_parameter_range.endpoints())
                        .then(|| DirectedParameterRange::new(side.pcurve_parameter_range).ok())
                        .flatten(),
                    )),
                })
            };
            let sides = [build_side(first)?, build_side(second)?];
            annotate(
                ctx,
                annotations,
                &directrix_id,
                "object_stream_a8_03_25",
                0,
                "two_support_directrix",
                Exactness::Unknown,
            )?;
            admission.reserve_entity(&mut ir.model.curves, "catia_extrusion_directrix_curves")?;
            ir.model.curves.push(Curve {
                id: directrix_id.try_clone_for_decode(ctx, "catia_extrusion_directrix_curve_id")?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: Some(cgm_source(ctx, "curve", extrusion.directrix_object_id)?),
            });
            let procedure_id = ProceduralCurveId::compose(
                &cadmpeg_ir::identity_namespace!(
                    "catia",
                    "standard",
                    "extrusion-directrix-procedure"
                ),
                extrusion.directrix_object_id,
            );
            annotate(
                ctx,
                annotations,
                &procedure_id,
                "object_stream_a8_03_25",
                0,
                "two_surface_pcurve_intersection",
                Exactness::ByteExact,
            )?;
            admission.reserve_entity(
                &mut ir.model.procedural_curves,
                "catia_extrusion_directrix_procedures",
            )?;
            let owner =
                directrix_id.try_clone_for_decode(ctx, "catia_extrusion_directrix_owner_id")?;
            ctx.charge_retained(
                u64_from_index(procedure_id.as_str().len()),
                "catia_extrusion_directrix_construction_id",
            )?;
            let procedure = ProceduralCurve::new(
                procedure_id,
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::over_interval(
                        sides,
                        extrusion.directrix_parameter_range,
                    ),
                    discontinuity_flag: false,
                    cache: Some(cadmpeg_ir::geometry::LegacyCache::new(
                        cadmpeg_ir::scalar::NonNegativeReal::from(cache_fit_tolerance).into(),
                    )),
                },
            );
            let _attached = ir
                .model
                .add_procedural_curve_charged(ctx, &owner, procedure)?;
        }
        crate::families::b5::transfer::ResolvedExtrusionDirectrix::SurfaceCurve {
            curve, ..
        } => {
            annotate(
                ctx,
                annotations,
                &directrix_id,
                "object_stream_b5_03_24",
                0,
                "support_pcurve_lift",
                Exactness::Derived,
            )?;
            admission.reserve_entity(
                &mut ir.model.curves,
                "catia_extrusion_surface_directrix_curves",
            )?;
            ir.model.curves.push(Curve {
                id: directrix_id.try_clone_for_decode(ctx, "catia_extrusion_surface_curve_id")?,
                geometry: curve,
                source_object: Some(cgm_source(ctx, "curve", extrusion.directrix_object_id)?),
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
            let source_id = CurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "standard", "extrusion-directrix-source"),
                source_object_id,
            );
            annotate(
                ctx,
                annotations,
                &source_id,
                "object_stream_b5_03_24",
                0,
                "support_pcurve_lift",
                Exactness::Derived,
            )?;
            admission
                .reserve_entity(&mut ir.model.curves, "catia_extrusion_offset_source_curves")?;
            ir.model.curves.push(Curve {
                id: source_id.try_clone_for_decode(ctx, "catia_extrusion_offset_source_id")?,
                geometry: source_curve,
                source_object: Some(cgm_source(ctx, "curve", source_object_id)?),
            });
            annotate(
                ctx,
                annotations,
                &directrix_id,
                "object_stream_b5_03_14",
                0,
                "fixed_direction_offset_curve",
                Exactness::Unknown,
            )?;
            admission.reserve_entity(
                &mut ir.model.curves,
                "catia_extrusion_offset_directrix_curves",
            )?;
            ir.model.curves.push(Curve {
                id: directrix_id
                    .try_clone_for_decode(ctx, "catia_extrusion_offset_directrix_id")?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: Some(cgm_source(ctx, "curve", extrusion.directrix_object_id)?),
            });
            let procedure_id = ProceduralCurveId::compose(
                &cadmpeg_ir::identity_namespace!(
                    "catia",
                    "standard",
                    "extrusion-directrix-procedure"
                ),
                extrusion.directrix_object_id,
            );
            annotate(
                ctx,
                annotations,
                &procedure_id,
                "object_stream_b5_03_14",
                0,
                "fixed_direction_offset_curve",
                Exactness::ByteExact,
            )?;
            let support = standard_extrusion_support_id(
                ctx,
                annotations,
                surfaces,
                procedural_supports,
                support.surface_object_id,
                support.surface,
                admission,
            )?;
            admission.reserve_entity(
                &mut ir.model.procedural_curves,
                "catia_extrusion_offset_procedures",
            )?;
            ctx.charge_retained(
                u64_from_index(procedure_id.as_str().len()),
                "catia_extrusion_offset_construction_id",
            )?;
            let owner =
                directrix_id.try_clone_for_decode(ctx, "catia_extrusion_offset_owner_id")?;
            let _attached = ir.model.add_procedural_curve_charged(ctx, &owner, ProceduralCurve::new(
                    procedure_id,
                    ProceduralCurveDefinition::Offset(
                        cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::along_direction(
                            source_id,
                            distance,
                            direction,
                            Some(support),
                            source_parameter_range,
                        ),
                    ),
                ))?;
        }
    }
    let definition = cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
        directrix_id,
        Some(parameter_range.into()),
        direction.into(),
        None,
        None,
    );
    let returned = copy_standard_extrusion_definition(ctx, &definition)?;
    ctx.insert_hash_map(
        extrusion_definitions,
        surface_object_id,
        definition,
        "catia_extrusion_definitions",
    )?;
    Ok(returned)
}

fn copy_standard_extrusion_definition(
    ctx: &DecodeContext<'_>,
    definition: &cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction,
) -> Result<ProceduralSurfaceDefinition, CodecError> {
    let directrix = definition
        .directrix()
        .try_clone_for_decode(ctx, "catia_extrusion_definition_directrix_id")?;
    Ok(ProceduralSurfaceDefinition::Extrusion(
        cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
            directrix,
            definition.parameter_interval(),
            *definition.direction(),
            definition.native_position(),
            None,
        ),
    ))
}

fn standard_freeform_e5_carrier_ids(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<HashMap<u32, u32>, CodecError> {
    let mut face_surfaces = HashMap::<u32, Option<u32>>::new();
    for (face, surface) in crate::families::e5::graph::face_surface_references(data) {
        if let Some(stored) = face_surfaces.get_mut(&face) {
            if *stored != Some(surface) {
                *stored = None;
            }
        } else {
            ctx.insert_hash_map(
                &mut face_surfaces,
                face,
                Some(surface),
                "catia_e5_face_surfaces",
            )?;
        }
    }

    let mut wrappers = HashMap::<u32, Option<u32>>::new();
    for wrapper in crate::families::e5::records::e5_surface_wrappers(ctx, data)? {
        let surface = wrapper.underlying_surface();
        if let Some(stored) = wrappers.get_mut(&wrapper.record_id) {
            if *stored != Some(surface) {
                *stored = None;
            }
        } else {
            ctx.insert_hash_map(
                &mut wrappers,
                wrapper.record_id,
                Some(surface),
                "catia_e5_wrapper_surfaces",
            )?;
        }
    }
    let mut carriers = HashMap::new();
    for (face, wrapper) in face_surfaces {
        let Some(surface) = wrapper.and_then(|wrapper| wrappers.get(&wrapper).copied().flatten())
        else {
            continue;
        };
        ctx.insert_hash_map(&mut carriers, face, surface, "catia_e5_face_carriers")?;
    }
    Ok(carriers)
}

/// Join a standard freeform face to a directly decoded E5 analytic carrier
/// through the serialized face and class-`0xf1` wrapper identities.
///
/// This path is exact: a geometric candidate is accepted only when the
/// standard tag names one E5 face, that face names one valid `0xf1` wrapper,
/// and the wrapper's first reference names one supported E5 surface carrier.
fn associate_standard_freeform_e5_surfaces(
    ctx: &DecodeContext<'_>,
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    data: &[u8],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<HashMap<u32, SurfaceGeometry>, CodecError> {
    let carrier_ids = standard_freeform_e5_carrier_ids(ctx, data)?;

    let mut surfaces = HashMap::<u32, Option<SurfaceGeometry>>::new();
    for surface in crate::families::e5::records::e5_surfaces(ctx, data, refusal)? {
        if let Some(stored) = surfaces.get_mut(&surface.record_id) {
            if stored
                .as_ref()
                .is_some_and(|geometry| geometry != &surface.geometry)
            {
                *stored = None;
            }
        } else {
            ctx.insert_hash_map(
                &mut surfaces,
                surface.record_id,
                Some(surface.geometry),
                "catia_e5_surface_carriers",
            )?;
        }
    }
    let mut associated = HashMap::new();
    for record in records {
        let crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } =
            record
        else {
            continue;
        };
        let Some(geometry) = carrier_ids
            .get(tag)
            .and_then(|carrier| surfaces.get(carrier))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let copied = (geometry).try_clone_for_decode(ctx, "catia_e5_surface_geometry_copy")?;
        ctx.insert_hash_map(
            &mut associated,
            *tag,
            copied,
            "catia_e5_associated_surfaces",
        )?;
    }
    Ok(associated)
}

fn copy_standard_procedure(
    ctx: &DecodeContext<'_>,
    procedure: &StandardSurfaceProcedure,
) -> Result<StandardSurfaceProcedure, CodecError> {
    Ok(match procedure {
        StandardSurfaceProcedure::RollingBall {
            carrier_object_id,
            definition,
            source,
        } => StandardSurfaceProcedure::RollingBall {
            carrier_object_id: *carrier_object_id,
            definition: Box::new(
                crate::families::b5::transfer::surfaces::copy_rolling_ball_definition(
                    ctx, definition,
                )?,
            ),
            source: *source,
        },
        StandardSurfaceProcedure::Offset {
            carrier_object_id,
            support_object_id,
            support,
            distance,
            parameter_bounds,
        } => {
            let support = match support {
                crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(geometry) => {
                    let geometry = match geometry {
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                                nurbs.try_clone_for_decode(
                                    ctx,
                                    "catia_standard_offset_support_copy",
                                )?,
                            ))
                        }
                        SurfaceGeometry::Solved(
                            SolvedSurfaceGeometry::Plane(_)
                            | SolvedSurfaceGeometry::Cylinder(_)
                            | SolvedSurfaceGeometry::Cone(_)
                            | SolvedSurfaceGeometry::Sphere(_)
                            | SolvedSurfaceGeometry::Torus(_),
                        ) => geometry.try_clone_for_decode(ctx, "catia_standard_offset_support_copy")?,
                        _ => {
                            return Err(CodecError::malformed(
                                "standard offset support has unexpected geometry",
                            ))
                        }
                    };
                    crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(geometry)
                }
                crate::families::b5::transfer::ResolvedOffsetSupport::Extrusion(extrusion) => {
                    ctx.charge_retained(
                        u64_from_index(std::mem::size_of::<
                            crate::families::b5::transfer::ResolvedExtrusionSurface,
                        >()),
                        "catia_standard_offset_extrusion_copy",
                    )?;
                    crate::families::b5::transfer::ResolvedOffsetSupport::Extrusion(Box::new(
                        crate::families::b5::transfer::copy_resolved_extrusion_surface(
                            ctx, extrusion,
                        )?,
                    ))
                }
            };
            StandardSurfaceProcedure::Offset {
                carrier_object_id: *carrier_object_id,
                support_object_id: *support_object_id,
                support,
                distance: *distance,
                parameter_bounds: *parameter_bounds,
            }
        }
        StandardSurfaceProcedure::Extrusion(extrusion) => {
            ctx.charge_retained(
                u64_from_index(std::mem::size_of::<
                    crate::families::b5::transfer::ResolvedExtrusionSurface,
                >()),
                "catia_standard_extrusion_plan_copy",
            )?;
            StandardSurfaceProcedure::Extrusion(Box::new(
                crate::families::b5::transfer::copy_resolved_extrusion_surface(ctx, extrusion)?,
            ))
        }
        StandardSurfaceProcedure::Revolution(revolution) => {
            ctx.charge_retained(
                u64_from_index(std::mem::size_of::<
                    crate::families::b5::transfer::ResolvedRevolutionSurface,
                >()),
                "catia_standard_revolution_plan_copy",
            )?;
            StandardSurfaceProcedure::Revolution(Box::new(
                crate::families::b5::transfer::ResolvedRevolutionSurface {
                    directrix: revolution
                        .directrix
                        .try_clone_for_decode(ctx, "catia_standard_revolution_directrix_copy")?,
                    axis_origin: revolution.axis_origin,
                    axis_direction: revolution.axis_direction,
                    angular_interval: revolution.angular_interval,
                    angular_parameter_interval: revolution.angular_parameter_interval,
                    parameter_interval: revolution.parameter_interval,
                },
            ))
        }
    })
}

/// Join standard freeform faces to exact E5 class-`0xd8` rolling-ball jets.
/// The face and wrapper identities are the same strict join used by analytic
/// E5 carriers; only the underlying carrier decoder differs. The carrier's
/// signed sense must agree with the owning face orientation before admission.
fn associate_standard_freeform_e5_rolling_ball_jets(
    ctx: &DecodeContext<'_>,
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    data: &[u8],
    decoded_jets: &[crate::families::e5::records::E5RollingBallJet],
) -> Result<HashMap<u32, StandardSurfaceProcedure>, CodecError> {
    let carrier_ids = standard_freeform_e5_carrier_ids(ctx, data)?;
    let mut jets = HashMap::<u32, Option<&crate::families::e5::records::E5RollingBallJet>>::new();
    for jet in decoded_jets {
        if let Some(stored) = jets.get_mut(&jet.record_id) {
            if stored.is_some_and(|existing| existing != jet) {
                *stored = None;
            }
        } else {
            ctx.insert_hash_map(
                &mut jets,
                jet.record_id,
                Some(jet),
                "catia_e5_rolling_ball_carriers",
            )?;
        }
    }
    let mut associated = HashMap::new();
    for record in records {
        let crate::families::standard::records::StandardSurfaceRecord::Freeform {
            tag,
            forward,
            ..
        } = record
        else {
            continue;
        };
        let Some(jet) = carrier_ids
            .get(tag)
            .and_then(|carrier| jets.get(carrier))
            .copied()
            .flatten()
        else {
            continue;
        };
        if *forward != (jet.sense == crate::families::e5::graph::Sign::Negative) {
            continue;
        }
        let Some(definition) = jet.definition(ctx)? else {
            continue;
        };
        ctx.insert_hash_map(
            &mut associated,
            *tag,
            StandardSurfaceProcedure::RollingBall {
                carrier_object_id: jet.record_id,
                definition: Box::new(definition),
                source: StandardRollingBallSource::E5D8,
            },
            "catia_e5_rolling_ball_associations",
        )?;
    }
    Ok(associated)
}

#[derive(Debug, Clone)]
struct StandardPopulationSelection {
    spine: Vec<u8>,
    records: Vec<crate::families::standard::records::StandardSurfaceRecord>,
    supports: Vec<crate::families::standard::records::StandardCurveSupport>,
    edge_table_form: EdgeTableForm,
    vertex_roster_compatible: bool,
}

fn standard_population_selections(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<
    Option<(
        StandardPopulationSelection,
        Vec<StandardPopulationSelection>,
    )>,
    CodecError,
> {
    let Some(brep) = scan.brep.as_ref() else {
        return Ok(None);
    };
    let standard_spine = scan.main_data_stream.as_deref().unwrap_or(brep);
    let layouts = fbb::fbb_population_layouts(ctx, standard_spine)?;
    let populations = crate::families::standard::records::standard_surface_populations(ctx, brep)?;
    let Some(pairs) =
        crate::families::standard::records::pair_standard_populations(ctx, &layouts, &populations)?
    else {
        return Ok(None);
    };
    let select = |(layout, population): (
        fbb::FbbPopulationLayout,
        crate::families::standard::records::StandardSurfacePopulation,
    )|
     -> Result<Option<StandardPopulationSelection>, CodecError> {
        let Some(spine) = fbb::population_spine(ctx, standard_spine, &layout)? else {
            return Ok(None);
        };
        Ok(Some(StandardPopulationSelection {
            spine: ctx.copy_retained(spine, "catia_standard_population_spine")?,
            records: population.records,
            supports: population.supports,
            edge_table_form: layout.edge_table_form,
            vertex_roster_compatible: crate::families::standard::records::standard_vertex_roster(
                ctx,
                &scan.data,
                layout.vertex_count,
            )?
            .is_some(),
        }))
    };
    let Some(first) = select(pairs.first)? else {
        return Ok(None);
    };
    let mut rest = Vec::new();
    for pair in pairs.rest {
        let Some(selection) = select(pair)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut rest, selection, "catia_standard_population_selections")?;
    }
    Ok(Some((first, rest)))
}

pub(in crate::families) fn try_decode_standard(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FamilyOutput>, cadmpeg_core::CodecError> {
    let surface_alias_tags = if matches!(scan.variant, Variant::StandardNested) {
        crate::object_graph::surface_alias_tag_map(ctx, &scan.data)?
    } else {
        HashMap::new()
    };
    let e5_jets = crate::families::e5::records::e5_rolling_ball_jets(ctx, &scan.data)?;
    match standard_population_selections(ctx, scan)? {
        None => {
            try_decode_standard_population(ctx, scan, None, refusal, &e5_jets, &surface_alias_tags)
        }
        Some((first, rest)) if rest.is_empty() => try_decode_standard_population(
            ctx,
            scan,
            Some(&first),
            refusal,
            &e5_jets,
            &surface_alias_tags,
        ),
        Some((first, rest)) => try_decode_standard_populations(
            ctx,
            scan,
            &first,
            &rest,
            refusal,
            &e5_jets,
            &surface_alias_tags,
        ),
    }
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

fn rescope_standard_id(
    ctx: &DecodeContext<'_>,
    text: &str,
    scope: &str,
) -> Result<String, CodecError> {
    match text.strip_prefix("catia:standard:") {
        Some(rest) => ctx.format_retained(
            format_args!("catia:standard:{scope}/{rest}"),
            "catia_standard_population_identity",
        ),
        None => ctx.copy_retained_text(text, "catia_standard_population_identity"),
    }
}

struct StandardPopulationScope<'a, 'b> {
    scope: &'a str,
    ctx: &'a DecodeContext<'b>,
}

impl EntityRewrite for StandardPopulationScope<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: Serialize + DeserializeOwned>(&mut self, entity: T) -> Result<T, Self::Error> {
        struct CountBytes(usize);
        impl std::io::Write for CountBytes {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .ok_or(std::io::ErrorKind::OutOfMemory)?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut size = CountBytes(0);
        serde_json::to_writer(&mut size, &entity).map_err(CodecError::malformed)?;
        let bytes = u64_from_index(size.0);
        self.ctx
            .charge_collection_items(bytes, "catia_standard_population_rewrite")?;
        let retained = bytes.checked_mul(4).ok_or_else(|| {
            self.ctx
                .refuse_codec_limit("catia_standard_population_rewrite", u64::MAX, u64::MAX)
        })?;
        self.ctx
            .charge_retained(retained, "catia_standard_population_rewrite")?;
        let refusal = std::cell::RefCell::new(None);
        let rewritten = cadmpeg_ir::schema::rewrite::identities(&entity, |id| {
            if refusal.borrow().is_some() {
                return String::new();
            }
            match rescope_standard_id(self.ctx, id, self.scope) {
                Ok(value) => value,
                Err(error) => {
                    *refusal.borrow_mut() = Some(error);
                    String::new()
                }
            }
        });
        let value = serde_value::to_value(rewritten);
        if let Some(error) = refusal.into_inner() {
            return Err(error);
        }
        let value = value.map_err(CodecError::malformed)?;
        T::deserialize(ValueDeserializer::<serde_value::DeserializerError>::new(
            value,
        ))
        .map_err(CodecError::malformed)
    }
}

fn merge_standard_population_annotations(
    ctx: &DecodeContext<'_>,
    target: &mut Annotations,
    mut source: Annotations,
    scope: &str,
) -> Result<Result<(), cadmpeg_ir::annotations::AnnotationIdentityCollision>, CodecError> {
    // Only standard-owned entities survive retain_standard_population_model.
    // The first population retains the shared payload and other carriers.
    source
        .provenance
        .retain(|id, _| id.starts_with("catia:standard:"));
    let mut annotations = AnnotationBuilder::resume(source);
    annotations.retain_exactness(|id| id.starts_with("catia:standard:"));
    source = annotations.build();
    if let Err(collision) = source.map_ids_charged(
        ctx,
        |id| match id.strip_prefix("catia:standard:") {
            Some(rest) => ctx.format_retained(
                format_args!("catia:standard:{scope}/{rest}"),
                "catia_standard_population_annotation_id",
            ),
            None => ctx.copy_retained_text(id, "catia_standard_population_annotation_id"),
        },
        "catia_standard_population_annotation_remap",
    )? {
        return Ok(Err(collision));
    }
    target.append_charged(ctx, source, "catia_standard_population_annotation_append")
}

fn try_decode_standard_populations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    first: &StandardPopulationSelection,
    rest: &[StandardPopulationSelection],
    refusal: &mut crate::nurbs::LaneRefusals,
    e5_jets: &[crate::families::e5::records::E5RollingBallJet],
    surface_alias_tags: &HashMap<u32, Option<u32>>,
) -> Result<Option<FamilyOutput>, cadmpeg_core::CodecError> {
    let Some(mut merged) = try_decode_standard_population(
        ctx,
        scan,
        Some(first),
        refusal,
        e5_jets,
        surface_alias_tags,
    )?
    else {
        return Ok(None);
    };
    let mut outputs = Vec::new();
    ctx.reserve_vec(
        &mut outputs,
        rest.len(),
        "catia_standard_population_outputs",
    )?;
    for selection in rest {
        let Some(output) = try_decode_standard_population(
            ctx,
            scan,
            Some(selection),
            refusal,
            e5_jets,
            surface_alias_tags,
        )?
        else {
            return Ok(None);
        };
        outputs.push(output);
    }
    let attached_topology_count = std::iter::once(&merged)
        .chain(outputs.iter())
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
        crate::coverage::ATTEMPTED_STANDARD_TOPOLOGY_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_CURVE_SUPPORT_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_NATIVE_ENDPOINT_PAIR_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_EMPTY_ENDPOINT_DOMAIN_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_SINGLETON_ENDPOINT_DOMAIN_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_MULTIPLE_ENDPOINT_DOMAIN_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_ENDPOINT_DOMAIN_CHOICE_COUNT,
    ]
    .map(|key| {
        (
            key,
            std::iter::once(&merged)
                .chain(outputs.iter())
                .map(|output| {
                    output
                        .report
                        .coverage
                        .get(key.as_str())
                        .copied()
                        .unwrap_or_default()
                })
                .sum::<usize>(),
        )
    });
    let population_count = 1 + rest.len();
    let admitted_face_rows = std::iter::once(first)
        .chain(rest.iter())
        .map(|selection| selection.records.len())
        .sum::<usize>();
    let all_fbb_rows_admitted =
        population_count == scan.census.fbb_runs && admitted_face_rows == scan.census.fbb_face_rows;
    let all_topologies_attached = attached_topology_count == population_count;

    for (index, output) in outputs.into_iter().enumerate() {
        merged.admitted_model_entities = merged
            .admitted_model_entities
            .checked_add(output.admitted_model_entities)
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "count CATIA standard population entities",
                    u64::MAX,
                    u64::MAX,
                )
            })?;
        let (scope, _scope_reservation) = ctx.format_scoped(
            format_args!("population-{}", index + 1),
            "catia_standard_population_scope",
        )?;
        let mut model = output.ir.model;
        retain_standard_population_model(&mut model);
        let mut rewriter = StandardPopulationScope { scope: &scope, ctx };
        match merged.ir.model.extend_rewritten_charged(
            ctx,
            model,
            &mut rewriter,
            "catia_standard_population_model_merge",
        ) {
            Ok(()) => {}
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => return Ok(None),
        }
        if let Err(error) = merge_standard_population_annotations(
            ctx,
            &mut merged.annotations,
            output.annotations,
            &scope,
        )? {
            refusal.push_annotation_collision(ctx, &error)?;
            return Ok(None);
        }
        if output.report.transfer.geometry_transferred() {
            merged.report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::full(true);
        }
    }

    for (key, value) in population_coverage {
        merged.report.coverage.record(key, value);
    }
    merged.report.coverage.record(
        crate::coverage::STANDARD_FBB_RUN_COUNT,
        scan.census.fbb_runs,
    );
    merged.report.coverage.record(
        crate::coverage::STANDARD_FBB_CANDIDATE_FACE_ROW_COUNT,
        scan.census.fbb_face_rows,
    );
    merged.report.coverage.record(
        crate::coverage::STANDARD_FBB_ADMITTED_FACE_ROW_COUNT,
        admitted_face_rows,
    );
    merged.report.coverage.record(
        crate::coverage::STANDARD_FBB_WITHHELD_FACE_ROW_COUNT,
        scan.census.fbb_face_rows - scan.census.fbb_face_rows.min(admitted_face_rows),
    );
    merged.report.coverage.record(
        crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT,
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
        crate::resource::push_loss(ctx, &mut merged.report.losses,
            CatiaLossCode::TopologyFbbRowsWithheld, format_args!(
                "{} candidate FBB face row(s) in {} marker group(s) were not admitted to the standard topology population; only {} row(s) have source-closed population bindings.",
                scan.census.fbb_face_rows - scan.census.fbb_face_rows.min(admitted_face_rows),
                scan.census.fbb_runs,
                admitted_face_rows,
            ), "catia_standard_merged_withheld_loss")?;
    }
    if !all_topologies_attached {
        crate::resource::push_loss(
            ctx,
            &mut merged.report.losses,
            CatiaLossCode::TopologyBoundaryGraphNotEmitted,
            format_args!(
                "The B-rep boundary graph was emitted for {attached_topology_count} of \
             {population_count} source-closed standard populations."
            ),
            "catia_standard_merged_boundary_loss",
        )?;
    }
    let mut typed = TypedCounts::default();
    for surface in &merged.ir.model.surfaces {
        typed.record(&surface.geometry);
    }
    crate::resource::push_loss(ctx, &mut merged.report.losses,
        CatiaLossCode::GeometryCarrierSummary, format_args!(
        "{} vertex point(s) were decoded verbatim from `05 08 01` records (3×f32 LE, millimetres, identity world placement) and {} analytic surface carrier(s) were decoded from `SurfacicReps` `00 33` records: {} plane, {} cylinder, {} cone, {} sphere, {} torus.",
        merged.ir.model.vertices.len(),
        typed.total(),
        typed.plane,
        typed.cylinder,
        typed.cone,
        typed.sphere,
        typed.torus,
    ), "catia_standard_merged_carrier_loss")?;
    crate::assemble::insert_unresolved_carrier_loss(ctx, &merged.ir, &mut merged.report.losses)?;
    Ok(Some(merged))
}

fn try_decode_standard_population(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    selection: Option<&StandardPopulationSelection>,
    refusal: &mut crate::nurbs::LaneRefusals,
    e5_jets: &[crate::families::e5::records::E5RollingBallJet],
    surface_alias_tags: &HashMap<u32, Option<u32>>,
) -> Result<Option<FamilyOutput>, cadmpeg_core::CodecError> {
    (|| -> Option<Result<FamilyOutput, cadmpeg_core::CodecError>> {
    macro_rules! admitted {
        ($value:expr) => {
            match $value {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            }
        };
    }
    let mut admission = FamilyEntityAdmission::new(ctx);
    let work_budget = ctx.work_budget(u64_from_index(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS));
    let brep = scan.brep.as_ref()?;
    let default_spine = scan.main_data_stream.as_deref().unwrap_or(brep);
    let standard_spine = selection.map_or(default_spine, |selection| selection.spine.as_slice());
    let edge_table_form = selection.map_or_else(
        || match scan.variant {
            Variant::FbbOnly => EdgeTableForm::FbbOnly,
            _ => EdgeTableForm::Standard,
        },
        |selection| selection.edge_table_form,
    );
    if !work_budget.charge() {
        return None;
    }
    let sources = match container::consolidated_record_sources(ctx, scan) {
        Ok(sources) => sources,
        Err(error) => return Some(Err(error)),
    };
    let consolidated_records = match crate::wire::records::consolidated_records_in_sources(
        ctx, &scan.data, sources,
    ) {
        Ok(records) => records,
        Err(error) => return Some(Err(error)),
    };
    let vertex_points = match edge_table_form {
        EdgeTableForm::FbbOnly => match fbb::fbb_only_vertex_points(ctx, standard_spine) {
            Ok(points) => points,
            Err(error) => return Some(Err(error)),
        },
        EdgeTableForm::Standard => match fbb::standard_vertex_points(ctx, standard_spine) {
            Ok(points) => points,
            Err(error) => return Some(Err(error)),
        },
    };
    let points = vertex_points.unwrap_or_default();
    let vertex_roster = if selection.is_none_or(|selection| selection.vertex_roster_compatible) {
        match crate::families::standard::records::standard_vertex_roster(ctx, &scan.data, points.len()) {
            Ok(roster) => roster,
            Err(error) => return Some(Err(error)),
        }
    } else {
        None
    };
    let face_count = if let Some(selection) = selection {
        selection.records.len()
    } else {
        match fbb::standard_face_count(ctx, standard_spine) {
            Ok(count) => count.unwrap_or_default(),
            Err(error) => return Some(Err(error)),
        }
    };
    let records = if let Some(selection) = selection {
        match ctx.copy_retained_slice(&selection.records, "catia_standard_selected_records") {
            Ok(records) => records,
            Err(error) => return Some(Err(error)),
        }
    } else {
        let parsed = match crate::families::standard::records::standard_surface_records(ctx, brep, face_count) {
            Ok(records) => records,
            Err(error) => return Some(Err(error)),
        };
        if let Some(records) = parsed {
            records
        } else {
            let prefixes = match crate::families::standard::records::surface_prefixes(ctx, brep) {
                Ok(prefixes) => prefixes,
                Err(error) => return Some(Err(error)),
            };
            let mut records = Vec::new();
            for prefix in prefixes {
                if let Err(error) = ctx.push_vec(&mut records, crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix), "catia_standard_analytic_records") {
                    return Some(Err(error));
                }
            }
            records
        }
    };
    let analytic_record_count = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                crate::families::standard::records::StandardSurfaceRecord::Analytic(_)
            )
        })
        .count();
    let mut freeform_tags = HashSet::new();
    for record in &records {
        if let crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } = record {
            if let Err(error) = ctx.insert_hash_set(&mut freeform_tags, *tag, "catia_standard_freeform_tags") {
                return Some(Err(error));
            }
        }
    }
    let standard_edge_count = if let Some(selection) = selection {
        (!selection.supports.is_empty()).then_some(selection.supports.len())
    } else {
        let count = if edge_table_form == EdgeTableForm::FbbOnly {
            match fbb::fbb_only_edge_count(ctx, standard_spine) {
                Ok(count) => count,
                Err(error) => return Some(Err(error)),
            }
        } else {
            match fbb::standard_edge_count(ctx, standard_spine) {
                Ok(count) => count,
                Err(error) => return Some(Err(error)),
            }
        };
        count.filter(|count| *count > 0)
    };
    let curve_supports = if let Some(selection) = selection {
        match ctx.copy_retained_slice(&selection.supports, "catia_standard_selected_supports") {
            Ok(supports) => supports,
            Err(error) => return Some(Err(error)),
        }
    } else {
        match crate::families::standard::records::standard_curve_supports(ctx, brep, face_count, standard_edge_count) {
            Ok(supports) => supports,
            Err(error) => return Some(Err(error)),
        }
    };
    let mut edge_tags = HashSet::new();
    for support in &curve_supports {
        if let Err(error) = ctx.insert_hash_set(&mut edge_tags, support.tag, "catia_standard_edge_tags") {
            return Some(Err(error));
        }
    }
    let mut object_evidence = match standard_object_evidence(
        ctx,
        scan,
        &freeform_tags,
        &edge_tags,
        &consolidated_records,
        refusal,
    ) {
        Ok(evidence) => evidence,
        Err(error) => return Some(Err(error)),
    };
    let standard_limit_curve_count = object_evidence.limit_curves.len();
    let revolution_record_count = crate::families::b2::records::b2_revolutions_from_records(
        &scan.data,
        &consolidated_records,
    )
    .count();
    let face_frame_vectors = match fbb::standard_face_frame_vectors(ctx, standard_spine, records.len()) {
        Ok(vectors) => vectors,
        Err(error) => return Some(Err(error)),
    };
    let mut curved_surfaces = Vec::new();
    for record in &records {
        let surface = match record {
            crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix)
                if prefix.kind != AnalyticSurfaceKind::Plane =>
            {
                crate::families::standard::records::decode_curved(brep, prefix)
            }
            crate::families::standard::records::StandardSurfaceRecord::Analytic(_)
            | crate::families::standard::records::StandardSurfaceRecord::Freeform { .. } => None,
        };
        if let Err(error) = ctx.push_vec(&mut curved_surfaces, surface, "catia_standard_curved_surfaces") {
            return Some(Err(error));
        }
    }
    let refined_analytic_surfaces = match refine_consolidated_analytic_surfaces(
        ctx,
        &scan.data,
        &consolidated_records,
        &mut curved_surfaces,
    ) {
        Ok(surfaces) => surfaces,
        Err(error) => return Some(Err(error)),
    };
    let plane_normals = match standard_plane_normals_from_face_frames(ctx, &records, &face_frame_vectors) {
        Ok(normals) => normals,
        Err(error) => return Some(Err(error)),
    };
    let plane_rows = match crate::families::standard::records::plane_params(ctx, brep, &plane_normals) {
        Ok(planes) => planes,
        Err(error) => return Some(Err(error)),
    };
    let mut planes = HashMap::new();
    for plane in plane_rows {
        if let Err(error) = ctx.insert_hash_map(&mut planes, plane.target, plane, "catia_plane_param_map") {
            return Some(Err(error));
        }
    }
    let mut face_bounds = Vec::new();
    for record in &records {
        let bounds = crate::families::standard::records::standard_face_bounds(brep, record);
        if let Err(error) = ctx.push_vec(&mut face_bounds, bounds, "catia_standard_face_bounds") {
            return Some(Err(error));
        }
    }
    let mut freeform_geometries = std::mem::take(&mut object_evidence.surface_geometries);
    let e5_freeform_geometries = match associate_standard_freeform_e5_surfaces(ctx, &records, &scan.data, refusal) {
        Ok(geometries) => geometries,
        Err(error) => return Some(Err(error)),
    };
    let mut e5_freeform_tags = HashSet::new();
    for (tag, geometry) in e5_freeform_geometries {
        if let Err(error) = ctx.insert_hash_map(&mut freeform_geometries, tag, geometry, "catia_standard_e5_freeform_geometries") {
            return Some(Err(error));
        }
        if let Err(error) = ctx.insert_hash_set(&mut e5_freeform_tags, tag, "catia_standard_e5_freeform_tags") {
            return Some(Err(error));
        }
    }
    let mut freeform_procedural_surfaces = std::mem::take(&mut object_evidence.procedural_surfaces);
    let e5_freeform_procedural_surfaces = match associate_standard_freeform_e5_rolling_ball_jets(ctx, &records, &scan.data, e5_jets) {
        Ok(procedures) => procedures,
        Err(error) => return Some(Err(error)),
    };
    for (tag, procedure) in e5_freeform_procedural_surfaces {
        match freeform_procedural_surfaces.get(&tag) {
            Some(existing) if existing != &procedure => {
                freeform_procedural_surfaces.remove(&tag);
            }
            Some(_) => {}
            None => {
                if let Err(error) = ctx.insert_hash_map(&mut freeform_procedural_surfaces, tag, procedure, "catia_standard_e5_freeform_procedures") {
                    return Some(Err(error));
                }
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
    if points.is_empty() && records.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    let payload_index = match preserve_raw_payload(
        ctx,
        &mut unknowns,
        &mut annotations,
        scan,
        admitted!(ctx.copy_retained_text("catia:payload:unknown#brep-stream", "catia_standard_payload_id").and_then(|text| UnknownId::mint(text).map_err(cadmpeg_core::CodecError::malformed))),
    ) {
        Ok(index) => index,
        Err(error) => return Some(Err(error)),
    };
    let mut surfaces = Vec::new();
    let mut surface_annotations = Vec::new();
    let mut face_bindings = Vec::new();
    let mut procedural_surface_plans = Vec::new();
    let mut decoded_plane_targets = HashSet::new();
    let mut plane_faces = 0usize;
    let mut typed = TypedCounts::default();
    for (i, record) in records.iter().enumerate() {
        let prefix = match record {
            crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) => prefix,
            crate::families::standard::records::StandardSurfaceRecord::Freeform {
                pos,
                tag,
                forward,
                ..
            } => {
                let id = admitted!(crate::resource::compose_index_id(ctx,
                    &cadmpeg_ir::identity_namespace!("catia", "standard", "surf"),
                    i, SurfaceId::mint, "catia_standard_surface_id"));
                let geometry = admitted!(freeform_geometries.get(tag)
                    .map(|geometry| (geometry).try_clone_for_decode(ctx, "catia_e5_surface_geometry_copy")).transpose())
                    .unwrap_or(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: None,
                    }));
                admitted!(ctx.push_vec(&mut face_bindings, (admitted!(id.try_clone_for_decode(ctx, "catia_standard_face_binding_surface_id")), *forward, *pos), "catia_standard_face_bindings"));
                admitted!(ctx.push_vec(&mut surface_annotations, (
                    admitted!(id.try_clone_for_decode(ctx, "catia_standard_annotation_surface_id")),
                    "MainDataStream+SurfacicReps",
                    *pos,
                    admitted!(ctx.copy_retained_text("surfacic_reps_freeform_alias", "catia_standard_surface_annotation_tag")),
                    if freeform_procedural_surfaces.contains_key(tag)
                        || e5_freeform_tags.contains(tag)
                    {
                        Exactness::ByteExact
                    } else if matches!(
                        geometry,
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                    ) {
                        Exactness::Unknown
                    } else {
                        Exactness::ByteExact
                    },
                ), "catia_standard_surface_annotations"));
                if let Err(error) = admission.reserve_entity(&mut surfaces, "catia_family_emit_surfaces") {
                    return Some(Err(error));
                }
                surfaces.push(Surface {
                    id: admitted!(id.try_clone_for_decode(ctx, "catia_standard_surface_record_id")),
                    geometry,
                    source_object: Some(admitted!(cgm_source(ctx, "carrier", *tag))),
                });
                if let Some(procedure) = admitted!(freeform_procedural_surfaces.get(tag)
                    .map(|procedure| copy_standard_procedure(ctx, procedure)).transpose()) {
                    admitted!(ctx.push_vec(&mut procedural_surface_plans, (i, id, *tag, procedure), "catia_standard_procedural_surface_plans"));
                }
                continue;
            }
        };
        // A bridged plane parameter record contains the same `00 33 32`
        // marker as its SurfacicReps carrier.  One carrier exists per tag.
        if prefix.kind == AnalyticSurfaceKind::Plane
            && !admitted!(ctx.insert_hash_set(&mut decoded_plane_targets, prefix.target, "catia_standard_decoded_plane_targets"))
        {
            continue;
        }
        let decoded = if prefix.kind == AnalyticSurfaceKind::Plane {
            planes
                .get(&prefix.target)
                .and_then(crate::families::standard::records::decode_plane)
        } else {
            curved_surfaces[i].clone()
        };
        match decoded {
            Some(geom) => {
                typed.record(&geom);
                let id = admitted!(crate::resource::compose_index_id(ctx,
                    &cadmpeg_ir::identity_namespace!("catia", "standard", "surf"),
                    i, SurfaceId::mint, "catia_standard_surface_id"));
                if let Some(forward) = crate::families::standard::records::face_sense(brep, prefix)
                {
                    admitted!(ctx.push_vec(&mut face_bindings, (admitted!(id.try_clone_for_decode(ctx, "catia_standard_face_binding_surface_id")), forward, prefix.pos), "catia_standard_face_bindings"));
                }
                let (annotation_stream, annotation_offset, annotation_tag) =
                    if let Some(source_pos) = refined_analytic_surfaces.get(&i) {
                        ("consolidated_b2_03", *source_pos,
                            admitted!(ctx.copy_retained_text("consolidated_exact_analytic_surface", "catia_standard_surface_annotation_tag")))
                    } else {
                        ("MainDataStream+SurfacicReps", prefix.pos,
                            admitted!(ctx.format_retained(format_args!("surfacic_reps_{:02x}", prefix.kind.marker()), "catia_standard_surface_annotation_tag")))
                    };
                admitted!(ctx.push_vec(&mut surface_annotations, (
                    admitted!(id.try_clone_for_decode(ctx, "catia_standard_annotation_surface_id")),
                    annotation_stream,
                    annotation_offset,
                    annotation_tag,
                    Exactness::ByteExact,
                ), "catia_standard_surface_annotations"));
                if let Err(error) = admission.reserve_entity(&mut surfaces, "catia_family_emit_surfaces") {
                    return Some(Err(error));
                }
                surfaces.push(Surface {
                    id,
                    geometry: geom,
                    source_object: Some(admitted!(cgm_source(ctx, "carrier", prefix.target))),
                });
            }
            None => {
                if prefix.kind == AnalyticSurfaceKind::Plane {
                    plane_faces += 1;
                }
                let id = admitted!(crate::resource::compose_index_id(ctx,
                    &cadmpeg_ir::identity_namespace!("catia", "standard", "surf"),
                    i, SurfaceId::mint, "catia_standard_surface_id"));
                if let Some(forward) = crate::families::standard::records::face_sense(brep, prefix)
                {
                    admitted!(ctx.push_vec(&mut face_bindings, (admitted!(id.try_clone_for_decode(ctx, "catia_standard_face_binding_surface_id")), forward, prefix.pos), "catia_standard_face_bindings"));
                }
                admitted!(ctx.push_vec(&mut surface_annotations, (
                    admitted!(id.try_clone_for_decode(ctx, "catia_standard_annotation_surface_id")),
                    "MainDataStream+SurfacicReps",
                    prefix.pos,
                    admitted!(ctx.format_retained(format_args!("surfacic_reps_{:02x}", prefix.kind.marker()), "catia_standard_surface_annotation_tag")),
                    Exactness::Unknown,
                ), "catia_standard_surface_annotations"));
                if let Err(error) = admission.reserve_entity(&mut surfaces, "catia_family_emit_surfaces") {
                    return Some(Err(error));
                }
                surfaces.push(Surface {
                    id,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: Some(admitted!(ctx.copy_retained_text("catia:payload:unknown#brep-stream", "catia_standard_unknown_record_id").and_then(|text| UnknownId::mint(text).map_err(cadmpeg_core::CodecError::malformed)))),
                    }),
                    source_object: Some(admitted!(cgm_source(ctx, "carrier", prefix.target))),
                });
            }
        }
    }

    let mut procedural_supports = HashMap::<u32, SurfaceId>::new();
    let mut extrusion_definitions = HashMap::new();
    for (index, surface, tag, procedure) in procedural_surface_plans {
        let procedural_id = admitted!(crate::resource::compose_index_id(ctx,
            &cadmpeg_ir::identity_namespace!("catia", "standard", "procedural-surf"),
            index, ProceduralSurfaceId::mint, "catia_standard_procedural_surface_id"));
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
                *definition,
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
                        let source_object = admitted!(cgm_source(ctx, "surface", support_object_id));
                        if let Some(id) = procedural_supports.get(&support_object_id) {
                            admitted!(id.try_clone_for_decode(ctx, "catia_standard_existing_support_id"))
                        } else {
                            let id = admitted!(crate::resource::compose_u32_id(ctx,
                                &cadmpeg_ir::identity_namespace!(
                                    "catia",
                                    "standard",
                                    "procedural-support"
                                ),
                                support_object_id, SurfaceId::mint,
                                "catia_standard_procedural_support_id"));
                            admitted!(annotate(
                                ctx,
                                &mut annotations,
                                &id,
                                "object_stream_b5_03",
                                0,
                                format_args!("surface:{support_object_id:08x}"),
                                Exactness::ByteExact));
                            if let Err(error) = admission.reserve_entity(&mut surfaces, "catia_family_emit_surfaces") {
                                return Some(Err(error));
                            }
                            surfaces.push(Surface {
                                id: admitted!(id.try_clone_for_decode(ctx, "catia_standard_support_record_id")),
                                geometry: support,
                                source_object: Some(source_object),
                            });
                            admitted!(ctx.insert_hash_map(&mut procedural_supports, support_object_id, admitted!(id.try_clone_for_decode(ctx, "catia_standard_support_map_id")), "catia_standard_procedural_supports"));
                            id
                        }
                    }
                    crate::families::b5::transfer::ResolvedOffsetSupport::Extrusion(extrusion) => {
                        let record_bounds = parameter_record_bounds(extrusion.parameter_bounds);
                        let support_id = admitted!(crate::resource::compose_u32_id(ctx,
                            &cadmpeg_ir::identity_namespace!(
                                "catia",
                                "standard",
                                "procedural-support"
                            ),
                            support_object_id, SurfaceId::mint,
                            "catia_standard_procedural_support_id"));
                        admitted!(annotate(
                            ctx,
                            &mut annotations,
                            &support_id,
                            "object_stream_b5_03_2c",
                            0,
                            format_args!("surface:{support_object_id:08x}"),
                            Exactness::ByteExact));
                        if let Err(error) = admission.reserve_entity(&mut surfaces, "catia_family_emit_surfaces") {
                            return Some(Err(error));
                        }
                        surfaces.push(Surface {
                            id: admitted!(support_id.try_clone_for_decode(ctx, "catia_standard_support_record_id")),
                            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                                record: None,
                            }),
                            source_object: Some(admitted!(cgm_source(ctx, "surface", support_object_id))),
                        });
                        let definition = match emit_standard_extrusion_definition(ctx,
&mut ir,
&mut annotations,
(&mut surfaces, &mut procedural_supports),
&mut extrusion_definitions,
*extrusion,
&mut admission) {
                            Ok(definition) => definition,
                            Err(error) => return Some(Err(error)),
                        };
                        let construction = admitted!(crate::resource::compose_u32_id(ctx,
                            &cadmpeg_ir::identity_namespace!(
                                "catia",
                                "standard",
                                "procedural-support-definition"
                            ),
                            support_object_id, ProceduralSurfaceId::mint,
                            "catia_standard_support_construction_id"));
                        let attached = if let Some(surface) =
                            surfaces.iter_mut().find(|surface| surface.id == support_id)
                        {
                            let cache = std::mem::replace(
                                &mut surface.geometry,
                                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                                    record: None,
                                }),
                            );
                            match cache {
                                SurfaceGeometry::Solved(cache) => {
                                    surface.geometry = SurfaceGeometry::Procedural {
                                        construction: admitted!(construction.try_clone_for_decode(ctx, "catia_standard_support_construction_ref")),
                                        cache: Some(cache),
                                    };
                                    true
                                }
                                cache @ SurfaceGeometry::Procedural { .. } => {
                                    surface.geometry = cache;
                                    false
                                }
                            }
                        } else {
                            false
                        };
                        if attached {
                            if let Err(error) = admission.reserve_entity(&mut ir.model.procedural_surfaces, "catia_family_emit_procedural_surfaces") {
                                return Some(Err(error));
                            }
                            ir.model.procedural_surfaces.push(ProceduralSurface::new(
                                construction,
                                definition,
                                Some(record_bounds),
                            ));
                        }
                        admitted!(ctx.insert_hash_map(&mut procedural_supports, support_object_id, admitted!(support_id.try_clone_for_decode(ctx, "catia_standard_support_map_id")), "catia_standard_procedural_supports"));
                        support_id
                    }
                };
                (
                    "object_stream_b5_03_30",
                    carrier_object_id,
                    ProceduralSurfaceDefinition::Offset(
                        cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
                            support_id,
                            distance,
                            None,
                            None,
                            false,
                            cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                            None,
                        ),
                    ),
                    Exactness::Derived,
                )
            }
            StandardSurfaceProcedure::Extrusion(extrusion) => {
                let carrier = extrusion.surface_object_id;
                let definition = match emit_standard_extrusion_definition(ctx,
&mut ir,
&mut annotations,
(&mut surfaces, &mut procedural_supports),
&mut extrusion_definitions,
*extrusion,
&mut admission) {
                    Ok(definition) => definition,
                    Err(error) => return Some(Err(error)),
                };
                (
                    "object_stream_b5_03_2c",
                    carrier,
                    definition,
                    Exactness::ByteExact,
                )
            }
            StandardSurfaceProcedure::Revolution(revolution) => {
                let directrix_id = admitted!(crate::resource::compose_u32_id(ctx,
                    &cadmpeg_ir::identity_namespace!("catia", "standard", "revolution-profile"),
                    tag, CurveId::mint, "catia_standard_revolution_profile_id"));
                admitted!(annotate(
                    ctx,
                    &mut annotations,
                    &directrix_id,
                    "object_stream_b5_03_2d",
                    0,
                    "profile_curve",
                    Exactness::Derived));
                admitted!(crate::resource::derived_annotation(ctx, &mut annotations, &directrix_id, "geometry", "catia_annotation_field"));
                if let Err(error) = admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves") {
                    return Some(Err(error));
                }
                ir.model.curves.push(Curve {
                    id: admitted!(directrix_id.try_clone_for_decode(ctx, "catia_standard_revolution_curve_record_id")),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                        revolution.directrix,
                    )),
                    source_object: None,
                });
                (
                    "object_stream_b5_03_2d",
                    tag,
                    ProceduralSurfaceDefinition::Revolution(cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(directrix_id, (revolution.axis_origin, revolution.axis_direction), revolution.angular_interval, Some(revolution.angular_parameter_interval), Some(revolution.parameter_interval), false, cadmpeg_ir::geometry::CacheContract::from_form(None)).ok()?),
                    Exactness::Derived,
                )
            }
        };
        let cacheless = matches!(&definition, ProceduralSurfaceDefinition::RollingBallJet(_));
        let attached = if let Some(surface_record) = surfaces.iter_mut()
            .find(|candidate| candidate.id == surface) {
            if cacheless {
                surface_record.geometry = SurfaceGeometry::Procedural {
                    construction: admitted!(procedural_id.try_clone_for_decode(ctx, "catia_standard_procedural_surface_ref")),
                    cache: None,
                };
                true
            } else if matches!(surface_record.geometry, SurfaceGeometry::Solved(_)) {
                let construction = admitted!(procedural_id.try_clone_for_decode(ctx, "catia_standard_procedural_surface_ref"));
                let cache = std::mem::replace(
                    &mut surface_record.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                );
                if let SurfaceGeometry::Solved(cache) = cache {
                    surface_record.geometry = SurfaceGeometry::Procedural {
                        construction,
                        cache: Some(cache),
                    };
                    true
                } else {
                    surface_record.geometry = cache;
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        admitted!(annotate(
            ctx,
            &mut annotations,
            &procedural_id,
            source,
            0,
            format_args!("face_object_id:{tag:08x}:result_carrier:{carrier:08x}"),
            exactness));
        if attached {
            if let Err(error) = admission.reserve_entity(&mut ir.model.procedural_surfaces, "catia_family_emit_procedural_surfaces") {
                return Some(Err(error));
            }
            ir.model.procedural_surfaces.push(ProceduralSurface::new(
                procedural_id,
                definition,
                record_bounds,
            ));
        }
    }
    ir.model.surfaces = surfaces;
    let resolved_consolidated_revolutions =
        match crate::families::b2::records::b2_resolved_revolutions_from_records(
            ctx, &scan.data, &consolidated_records,
        ) {
            Ok(revolutions) => revolutions,
            Err(error) => return Some(Err(error)),
        };
    let resolved_revolution_count = resolved_consolidated_revolutions.len();
    // The bindings this call returns are read below, through
    // `bind_consolidated_revolution_faces_and_seams`.
    let consolidated_revolutions = match append_consolidated_revolutions(
        &mut ir,
        &mut annotations,
        &resolved_consolidated_revolutions,
        &mut admission,
    ) {
        Ok(bindings) => bindings,
        Err(error) => return Some(Err(error)),
    };

    for (i, p) in points.iter().enumerate() {
        let point_id = admitted!(crate::resource::compose_index_id(ctx,
            &cadmpeg_ir::identity_namespace!("catia", "standard", "pt"),
            i, PointId::mint, "catia_standard_point_id"));
        admitted!(annotate(
            ctx,
            &mut annotations,
            &point_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact));
        if let Err(error) = admission.reserve_entity(&mut ir.model.points, "catia_family_emit_points") {
            return Some(Err(error));
        }
        let source_object = match vertex_roster.as_ref() {
            Some(roster) => Some(admitted!(cgm_source(ctx, "vertex", roster[i]))),
            None => None,
        };
        ir.model.points.push(Point::new(
            admitted!(point_id.try_clone_for_decode(ctx, "catia_standard_point_record_id")),
            *p,
            source_object,
        ));
        let vertex_id = admitted!(crate::resource::compose_index_id(ctx,
            &cadmpeg_ir::identity_namespace!("catia", "standard", "v"),
            i, VertexId::mint, "catia_standard_vertex_id"));
        admitted!(annotate(
            ctx,
            &mut annotations,
            &vertex_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact));
        admitted!(crate::resource::derived_annotation(ctx, &mut annotations, &vertex_id, "point", "catia_annotation_field"));
        if let Err(error) = admission.reserve_entity(&mut ir.model.vertices, "catia_family_emit_vertices") {
            return Some(Err(error));
        }
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (id, stream, offset, tag, exactness) in surface_annotations {
        admitted!(annotate(ctx, &mut annotations, &id, stream, u64_from_index(offset), tag, exactness));
    }
    let mut topology_ir = std::mem::replace(&mut ir, CadIr::empty());
    let mut topology_annotations = admitted!(annotations.copy_charged(ctx, "catia_standard_topology_annotations"));
    match attach_standard_faces(
        ctx,
        &mut topology_ir,
        &mut topology_annotations,
        &face_bindings,
        standard_spine,
        &mut admission,
    ) {
        Ok(()) => {}
        Err(error) => return Some(Err(error)),
    }
    let mut bound_standard_limit_curve_count = 0;
    let mut topology_diagnostics = StandardTopologyDiagnostics::default();
    let topology_budget = ctx.work_budget(u64_from_index(mesh_quotient::MAX_MESH_TOPOLOGY_OPERATIONS));
    let topology_result = attach_standard_topology(ctx, crate::families::standard::decode::AttachStandardTopologyInputs { ir: &mut topology_ir, annotations: &mut topology_annotations, bindings: &face_bindings, records: &records, face_bounds: &face_bounds, spine: standard_spine, edge_table_form, brep, support_override: selection.map(|selection| selection.supports.as_slice()), source: &scan.data, use_vertex_roster: selection.is_none_or(|selection| selection.vertex_roster_compatible), native_edge_faces: &object_evidence.edge_owner_faces, native_edge_supports: &object_evidence.edge_supports, limit_curves: &object_evidence.limit_curves, work_budget: &topology_budget, diagnostics: &mut topology_diagnostics, bound_limit_curve_count: &mut bound_standard_limit_curve_count, refusal, admission: &mut admission })
    .and_then(|()| {
        neutral_model_is_admissible(&mut topology_ir, &unknowns)?
            .then_some(())
            .ok_or(StandardTopologyError::Semantic(
                StandardTopologyFailure::InadmissibleNeutralModel,
            ))
    });
    let topology_failure = match topology_result {
        Ok(()) => None,
        Err(StandardTopologyError::Semantic(failure)) => Some(failure),
        Err(StandardTopologyError::Resource(error)) => return Some(Err(error)),
    };
    let topology_attached = topology_failure.is_none();
    if topology_attached {
        ir = topology_ir;
        annotations = topology_annotations;
    } else {
        // The candidate adds only topology and native edge-support carriers.
        // Restore the source carriers for the analytic fallback by discarding
        // those candidate-owned rows before moving the model back.
        topology_ir.model.bodies.clear();
        topology_ir.model.regions.clear();
        topology_ir.model.shells.clear();
        topology_ir.model.faces.clear();
        topology_ir.model.loops.clear();
        topology_ir.model.coedges.clear();
        topology_ir.model.edges.clear();
        topology_ir.model.pcurves.clear();
        topology_ir.model.surfaces.retain(|surface| {
            !surface.id.as_str().starts_with("catia:standard:edge-support-surface#")
        });
        topology_ir.model.procedural_surfaces.retain(|surface| {
            !surface.id.as_str().starts_with("catia:standard:edge-support-definition#")
        });
        ir = topology_ir;
        let fallback_result = (|| -> Result<(), cadmpeg_core::CodecError> {
            attach_standard_circles(
                &mut ir, &mut annotations, &face_bindings, &curve_supports, &mut admission,
            )?;
            attach_standard_lines(
                &mut ir, &mut annotations, &face_bindings, &curve_supports, &mut admission,
            )?;
            attach_free_vertices(ctx, &mut ir, &mut annotations, &mut admission)
        })();
        match fallback_result {
            Ok(()) => {}
            Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Some(Err(error)),
            Err(_) => return None,
        }
    }
    let (bound_revolution_face_surface_count, resolved_revolution_seam_curve_count) =
        match bind_consolidated_revolution_faces_and_seams(
            ctx,
            &mut ir,
            &mut annotations,
            &consolidated_revolutions,
        ) {
            Ok(bound) => bound,
            Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Some(Err(error)),
            Err(_) => return None,
        };
    let mut consolidated_curve_bindings = match append_freeform_surface_pools(
        &mut ir,
        &mut annotations,
        &scan.data,
        &consolidated_records,
        surface_alias_tags,
        refusal,
        &mut admission,
    ) {
        Ok(bindings) => bindings,
        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Some(Err(error)),
        Err(_) => return None,
    };
    let owner_binding_budget =
        ctx.work_budget(u64_from_index(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS));
    consolidated_curve_bindings.standard_face_surfaces += match bind_standard_a5_owner_surfaces(
        ctx,
        &mut ir,
        &mut annotations,
        StandardConsolidatedSource {
            data: &scan.data,
            records: &consolidated_records,
        },
        &face_bounds,
        &owner_binding_budget,
        refusal,
    ) {
        Ok(bound) => bound,
        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Some(Err(error)),
        Err(_) => return None,
    };
    if let Err(error) = link_payload_carriers(ctx, &ir, &mut unknowns[payload_index], &mut annotations) {
        return Some(Err(error));
    }
    let annotations = annotations.build();

    let (Some(face_local_freeform), Some(unbound_revolution), Some(withheld_face_rows)) = (
        unresolved_freeform_record_count
            .checked_sub(bound_revolution_face_surface_count)
            .and_then(|count| count.checked_sub(consolidated_curve_bindings.standard_face_surfaces)),
        revolution_record_count.checked_sub(resolved_revolution_count),
        scan.census.fbb_face_rows.checked_sub(face_count),
    ) else {
        return Some(Err(cadmpeg_core::CodecError::malformed(
            "CATIA geometry report counts are inconsistent",
        )));
    };

    let mut report = match build_geometry_report(ctx,
&ir,
scan,
&typed,
(plane_faces, analytic_record_count),
&crate::assemble::GeometryReportCounts {
            face_local_freeform,
            unbound_revolution,
            admitted_standard_face_rows: face_count,
        },
topology_failure.map(StandardTopologyFailure::message)) {
        Ok(report) => report,
        Err(error) => return Some(Err(error)),
    };
    if consolidated_curve_bindings.rechart_numeric_failures != 0 {
        if let Err(error) = crate::resource::push_loss(ctx, &mut report.losses,
            CatiaLossCode::GeometryPcurveRechartNonFinite, format_args!(
            "{} pcurve rechart attempts produced non-finite coordinates; native records remain retained",
            consolidated_curve_bindings.rechart_numeric_failures,
        ), "catia_standard_rechart_loss") {
            return Some(Err(error));
        }
    }
    report.coverage.record(
        crate::coverage::ATTEMPTED_STANDARD_TOPOLOGY_COUNT,
        usize::from(true),
    );
    report.coverage.record(
        crate::coverage::STANDARD_FBB_RUN_COUNT,
        scan.census.fbb_runs,
    );
    report.coverage.record(
        crate::coverage::STANDARD_FBB_CANDIDATE_FACE_ROW_COUNT,
        scan.census.fbb_face_rows,
    );
    report.coverage.record(
        crate::coverage::STANDARD_FBB_ADMITTED_FACE_ROW_COUNT,
        face_count,
    );
    report.coverage.record(
        crate::coverage::STANDARD_FBB_WITHHELD_FACE_ROW_COUNT,
        withheld_face_rows,
    );
    report.coverage.record(
        crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT,
        usize::from(topology_attached),
    );
    for failure in StandardTopologyFailure::ALL {
        report.coverage.record(
            failure.coverage_key(),
            usize::from(topology_failure == Some(failure)),
        );
    }
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_CURVE_SUPPORT_COUNT,
        topology_diagnostics.curve_supports,
    );
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_NATIVE_ENDPOINT_PAIR_COUNT,
        topology_diagnostics.native_endpoint_pairs,
    );
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_EMPTY_ENDPOINT_DOMAIN_COUNT,
        topology_diagnostics.empty_endpoint_domains,
    );
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_SINGLETON_ENDPOINT_DOMAIN_COUNT,
        topology_diagnostics.singleton_endpoint_domains,
    );
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_MULTIPLE_ENDPOINT_DOMAIN_COUNT,
        topology_diagnostics.multiple_endpoint_domains,
    );
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_ENDPOINT_DOMAIN_CHOICE_COUNT,
        topology_diagnostics.endpoint_domain_choices,
    );
    for (key, rejection) in [
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INPUT_STRUCTURE_COUNT,
            mesh_quotient::MeshCandidateRejection::InputStructure,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INPUT_CARDINALITY_COUNT,
            mesh_quotient::MeshCandidateRejection::InputCardinality,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_FACE_BOUNDARY_CARDINALITY_COUNT,
            mesh_quotient::MeshCandidateRejection::FaceBoundaryCardinality,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_PORT_CARDINALITY_COUNT,
            mesh_quotient::MeshCandidateRejection::PortCardinality,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_QUOTIENT_PREPARATION_COUNT,
            mesh_quotient::MeshCandidateRejection::QuotientPreparation,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_EDGE_CLASS_CONSTRAINT_COUNT,
            mesh_quotient::MeshCandidateRejection::EdgeClassConstraint,
        ),
    ] {
        report.coverage.record(
            key,
            usize::from(
                topology_diagnostics.mesh_failure
                    == Some(mesh_quotient::MeshCandidateFailure::Rejected(rejection)),
            ),
        );
    }
    let endpoint_incidence_rejection = match topology_diagnostics.mesh_failure {
        Some(mesh_quotient::MeshCandidateFailure::Rejected(
            mesh_quotient::MeshCandidateRejection::EndpointIncidence(rejection),
        )) => Some(rejection),
        _ => None,
    };
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_COUNT,
        usize::from(endpoint_incidence_rejection.is_some()),
    );
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_NO_ASSIGNMENT_COUNT,
        usize::from(matches!(
            endpoint_incidence_rejection,
            Some(mesh_quotient::MeshEndpointIncidenceRejection::NoAssignment(
                _
            ))
        )),
    );
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_BOUNDARY_RECONSTRUCTION_COUNT,
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
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_INPUT_SHAPE_COUNT,
            crate::solve::incidence::IncidenceRejection::InputShape,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_CHOICE_PRUNING_COUNT,
            crate::solve::incidence::IncidenceRejection::ChoicePruning,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_FIXED_ASSIGNMENT_COUNT,
            crate::solve::incidence::IncidenceRejection::FixedAssignment,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_DOMAIN_COUNT,
            crate::solve::incidence::IncidenceRejection::ComponentDomain,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_INCIDENCE_COMPONENT_COMPOSITION_COUNT,
            crate::solve::incidence::IncidenceRejection::ComponentComposition,
        ),
    ] {
        report
            .coverage
            .record(key, usize::from(incidence_rejection == Some(rejection)));
    }
    for (key, ambiguity) in [
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_AMBIGUITY_COORDINATE_ROOT_CLOSURE_COUNT,
            mesh_quotient::MeshCandidateAmbiguity::CoordinateRootClosure,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_AMBIGUITY_ENDPOINT_RESOLUTION_COUNT,
            mesh_quotient::MeshCandidateAmbiguity::EndpointResolution,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_AMBIGUITY_DISTINCT_TOPOLOGY_SOLUTIONS_COUNT,
            mesh_quotient::MeshCandidateAmbiguity::DistinctTopologySolutions,
        ),
    ] {
        report.coverage.record(
            key,
            usize::from(
                topology_diagnostics.mesh_failure
                    == Some(mesh_quotient::MeshCandidateFailure::Ambiguous(ambiguity)),
            ),
        );
    }
    report.coverage.record(
        crate::coverage::STANDARD_TOPOLOGY_MESH_EXHAUSTION_QUOTIENT_PREPARATION_COUNT,
        0,
    );
    for (key, exhaustion) in [
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_EXHAUSTION_INCIDENCE_ENUMERATION_COUNT,
            mesh_quotient::MeshCandidateExhaustion::IncidenceEnumeration,
        ),
        (
            crate::coverage::STANDARD_TOPOLOGY_MESH_EXHAUSTION_ENDPOINT_RESOLUTION_COUNT,
            mesh_quotient::MeshCandidateExhaustion::EndpointResolution,
        ),
    ] {
        report.coverage.record(
            key,
            usize::from(
                topology_diagnostics.mesh_failure
                    == Some(mesh_quotient::MeshCandidateFailure::Exhausted(exhaustion)),
            ),
        );
    }
    report.coverage.record(
        crate::coverage::REFINED_CONSOLIDATED_ANALYTIC_SURFACE_COUNT,
        refined_analytic_surfaces.len(),
    );
    report.coverage.record(
        crate::coverage::DECODED_STANDARD_LIMIT_CURVE_COUNT,
        standard_limit_curve_count,
    );
    report.coverage.record(
        crate::coverage::BOUND_STANDARD_LIMIT_CURVE_COUNT,
        bound_standard_limit_curve_count,
    );
    report.coverage.record(
        crate::coverage::BOUND_CONSOLIDATED_REVOLUTION_FACE_SURFACE_COUNT,
        bound_revolution_face_surface_count,
    );
    report.coverage.record(
        crate::coverage::RESOLVED_CONSOLIDATED_REVOLUTION_SEAM_CURVE_COUNT,
        resolved_revolution_seam_curve_count,
    );
    report.coverage.record(
        crate::coverage::BOUND_CONSOLIDATED_STANDARD_EDGE_COUNT,
        consolidated_curve_bindings.standard_edges,
    );
    report.coverage.record(
        crate::coverage::BOUND_CONSOLIDATED_PARTNER_SUPPORT_COUNT,
        consolidated_curve_bindings.partner_supports,
    );
    report.coverage.record(
        crate::coverage::BOUND_CONSOLIDATED_PARTNER_FACE_PCURVE_PAIR_COUNT,
        consolidated_curve_bindings.partner_face_pcurve_pairs,
    );
    report.coverage.record(
        crate::coverage::BOUND_CONSOLIDATED_STANDARD_FACE_SURFACE_COUNT,
        consolidated_curve_bindings.standard_face_surfaces,
    );
    report.coverage.record(
        crate::coverage::BOUND_CONSOLIDATED_STANDARD_FACE_PCURVE_COUNT,
        consolidated_curve_bindings.standard_face_pcurves,
    );
    Some(Ok(FamilyOutput {
        ir,
        report,
        annotations,
        unknowns,
        admitted_model_entities: admission.admitted(),
    }))
    })()
    .transpose()
}

#[derive(Default)]
pub(in crate::families::standard) struct StandardObjectEvidence {
    pub(super) surface_geometries: HashMap<u32, SurfaceGeometry>,
    pub(super) procedural_surfaces: HashMap<u32, StandardSurfaceProcedure>,
    pub(super) edge_owner_faces: HashMap<u32, HashSet<u32>>,
    edge_supports: HashMap<u32, StandardEdgeSupport>,
    limit_curves: Vec<NurbsCurve>,
}

#[derive(Default)]
struct StandardTopologyDiagnostics {
    curve_supports: usize,
    native_endpoint_pairs: usize,
    empty_endpoint_domains: usize,
    singleton_endpoint_domains: usize,
    multiple_endpoint_domains: usize,
    endpoint_domain_choices: usize,
    mesh_failure: Option<mesh_quotient::MeshCandidateFailure>,
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

enum StandardTopologyError {
    Semantic(StandardTopologyFailure),
    Resource(cadmpeg_core::CodecError),
}

impl From<StandardTopologyFailure> for StandardTopologyError {
    fn from(failure: StandardTopologyFailure) -> Self {
        Self::Semantic(failure)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for StandardTopologyError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(limit.into())
    }
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

    const fn coverage_key(self) -> cadmpeg_ir::report::decode::CoverageKey {
        match self {
            Self::NoCurveSupports => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_NO_CURVE_SUPPORTS_COUNT
            }
            Self::EdgeFaceAssignment => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_EDGE_FACE_ASSIGNMENT_COUNT
            }
            Self::MissingFaceSurface => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_MISSING_FACE_SURFACE_COUNT
            }
            Self::ConflictingNativeEndpoints => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_CONFLICTING_NATIVE_ENDPOINTS_COUNT
            }
            Self::NativeEndpointPropagation => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_NATIVE_ENDPOINT_PROPAGATION_COUNT
            }
            Self::EmptyEndpointDomain => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_EMPTY_ENDPOINT_DOMAIN_COUNT
            }
            Self::NoTopologySolution => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_NO_SOLUTION_COUNT
            }
            Self::AmbiguousTopologySolution => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_AMBIGUOUS_SOLUTION_COUNT
            }
            Self::TopologySearchExhausted => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_SEARCH_EXHAUSTED_COUNT
            }
            Self::InvalidTopologySolution => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_INVALID_SOLUTION_COUNT
            }
            Self::InadmissibleNeutralModel => {
                crate::coverage::STANDARD_TOPOLOGY_FAILURE_INADMISSIBLE_MODEL_COUNT
            }
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
        mesh_quotient::MeshSolve::Failed(
            mesh_quotient::MeshCandidateFailure::Rejected(_)
            | mesh_quotient::MeshCandidateFailure::Exhausted(
                mesh_quotient::MeshCandidateExhaustion::PreferredSolutionSearch,
            ),
        ) => fallback(),
        outcome => outcome,
    }
}

#[derive(Clone, PartialEq)]
/// Exact two-sided construction evidence keyed by a standard edge identity.
struct StandardEdgeSupport {
    /// Persistent support-surface identities in wrapper order.
    surface_object_ids: [u32; 2],
    /// Exact neutral support carriers.
    carriers: [crate::families::b5::transfer::ResolvedPcurveSurface; 2],
    /// Exact support pcurves in wrapper order.
    pcurves: [PcurveGeometry; 2],
    /// Shared native parameter interval.
    parameter_range: [f64; 2],
}

#[derive(Clone, PartialEq)]
pub(super) enum StandardSurfaceProcedure {
    RollingBall {
        carrier_object_id: u32,
        definition: Box<ProceduralSurfaceDefinition>,
        source: StandardRollingBallSource,
    },
    Offset {
        carrier_object_id: u32,
        support_object_id: u32,
        support: crate::families::b5::transfer::ResolvedOffsetSupport,
        distance: FiniteReal,
        parameter_bounds: [cadmpeg_ir::topology::IncreasingParameterInterval; 2],
    },
    Extrusion(Box<crate::families::b5::transfer::ResolvedExtrusionSurface>),
    Revolution(Box<crate::families::b5::transfer::ResolvedRevolutionSurface>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StandardRollingBallSource {
    ObjectStreamA8,
    E5D8,
}

enum StandardSurfaceEvidence {
    Geometry(SurfaceGeometry),
    Procedure(StandardSurfaceProcedure),
    Both(SurfaceGeometry, StandardSurfaceProcedure),
}

#[derive(Clone, Copy)]
enum StandardSupportLocation {
    Offset(usize),
    ExtrusionSide(usize, usize),
}

enum StandardSupportRef<'a> {
    Offset(&'a crate::families::b5::transfer::ResolvedOffsetSupport),
    Geometry(&'a SurfaceGeometry),
}

impl StandardSupportRef<'_> {
    fn equivalent(&self, other: &Self) -> bool {
        use crate::families::b5::transfer::ResolvedOffsetSupport;
        match (self, other) {
            (Self::Offset(left), Self::Offset(right)) => *left == *right,
            (Self::Geometry(left), Self::Geometry(right)) => *left == *right,
            (Self::Offset(ResolvedOffsetSupport::Geometry(left)), Self::Geometry(right))
            | (Self::Geometry(right), Self::Offset(ResolvedOffsetSupport::Geometry(left))) => {
                left == *right
            }
            _ => false,
        }
    }
}

#[derive(Default)]
struct StandardEvidenceStore {
    evidence: Vec<Option<StandardSurfaceEvidence>>,
    surfaces: HashMap<u32, Vec<usize>>,
    supports: HashMap<u32, Option<StandardSupportLocation>>,
}

impl StandardEvidenceStore {
    fn support_at(&self, location: StandardSupportLocation) -> Option<StandardSupportRef<'_>> {
        let index = match location {
            StandardSupportLocation::Offset(index)
            | StandardSupportLocation::ExtrusionSide(index, _) => index,
        };
        let procedure = self.evidence.get(index)?.as_ref()?.procedure_ref()?;
        match (location, procedure) {
            (
                StandardSupportLocation::Offset(_),
                StandardSurfaceProcedure::Offset { support, .. },
            ) => Some(StandardSupportRef::Offset(support)),
            (
                StandardSupportLocation::ExtrusionSide(_, side),
                StandardSurfaceProcedure::Extrusion(extrusion),
            ) => extrusion
                .supports()
                .nth(side)
                .map(|support| StandardSupportRef::Geometry(&support.surface)),
            _ => None,
        }
    }

    fn merge_support(
        &mut self,
        ctx: &DecodeContext<'_>,
        object_id: u32,
        incoming: StandardSupportLocation,
    ) -> Result<(), CodecError> {
        match self.supports.get(&object_id).copied() {
            Some(None) => {}
            Some(Some(stored)) => {
                let same = self
                    .support_at(stored)
                    .zip(self.support_at(incoming))
                    .is_some_and(|(left, right)| left.equivalent(&right));
                if !same {
                    self.supports.insert(object_id, None);
                }
            }
            None => {
                ctx.insert_hash_map(
                    &mut self.supports,
                    object_id,
                    Some(incoming),
                    "catia_standard_support_candidates",
                )?;
            }
        }
        Ok(())
    }

    fn add(
        &mut self,
        ctx: &DecodeContext<'_>,
        tag: u32,
        evidence: StandardSurfaceEvidence,
    ) -> Result<(), CodecError> {
        let index = self.evidence.len();
        ctx.push_vec(
            &mut self.evidence,
            Some(evidence),
            "catia_standard_evidence_records",
        )?;
        let mut support_locations = [None, None];
        match self.evidence[index]
            .as_ref()
            .and_then(StandardSurfaceEvidence::procedure_ref)
        {
            Some(StandardSurfaceProcedure::Offset {
                support_object_id, ..
            }) => {
                support_locations[0] =
                    Some((*support_object_id, StandardSupportLocation::Offset(index)));
            }
            Some(StandardSurfaceProcedure::Extrusion(extrusion)) => {
                for (side, support) in extrusion.supports().enumerate() {
                    support_locations[side] = Some((
                        support.surface_object_id,
                        StandardSupportLocation::ExtrusionSide(index, side),
                    ));
                }
            }
            _ => {}
        }
        for (object_id, location) in support_locations.into_iter().flatten() {
            self.merge_support(ctx, object_id, location)?;
        }
        ctx.admit_hash_map_entry(
            &mut self.surfaces,
            &tag,
            "catia_standard_surface_candidates",
        )?;
        ctx.push_vec(
            self.surfaces.entry(tag).or_default(),
            index,
            "catia_standard_surface_candidate_evidence",
        )
    }

    fn procedure_is_supported(&self, procedure: &StandardSurfaceProcedure) -> bool {
        match procedure {
            StandardSurfaceProcedure::Offset {
                support_object_id,
                support,
                ..
            } => self
                .supports
                .get(support_object_id)
                .copied()
                .flatten()
                .and_then(|location| self.support_at(location))
                .is_some_and(|candidate| {
                    candidate.equivalent(&StandardSupportRef::Offset(support))
                }),
            StandardSurfaceProcedure::Extrusion(extrusion) => extrusion.supports().all(|side| {
                self.supports
                    .get(&side.surface_object_id)
                    .copied()
                    .flatten()
                    .and_then(|location| self.support_at(location))
                    .is_some_and(|candidate| {
                        candidate.equivalent(&StandardSupportRef::Geometry(&side.surface))
                    })
            }),
            StandardSurfaceProcedure::RollingBall { .. }
            | StandardSurfaceProcedure::Revolution(_) => true,
        }
    }

    fn into_outputs(
        mut self,
        ctx: &DecodeContext<'_>,
        conflicting_population_ids: &HashSet<u32>,
    ) -> StandardProcedureOutputs {
        self.supports
            .retain(|object_id, _| !conflicting_population_ids.contains(object_id));
        let mut valid_procedure = ctx.alloc_filled(
            self.evidence.len(),
            false,
            "catia_standard_procedure_validity",
        )?;
        for (index, evidence) in self.evidence.iter().enumerate() {
            if let Some(procedure) = evidence
                .as_ref()
                .and_then(StandardSurfaceEvidence::procedure_ref)
            {
                valid_procedure[index] = self.procedure_is_supported(procedure);
            }
        }
        let mut surface_geometries = HashMap::new();
        let mut procedural_surfaces = HashMap::new();
        for (tag, indexes) in self.surfaces {
            if conflicting_population_ids.contains(&tag) {
                continue;
            }
            let mut geometry = None;
            let mut procedure = None;
            let mut procedure_valid = false;
            let mut conflict = false;
            for index in indexes {
                let Some(incoming) = self.evidence.get_mut(index).and_then(Option::take) else {
                    continue;
                };
                let (incoming_geometry, incoming_procedure) = incoming.into_parts();
                let had_procedure = procedure.is_some();
                let has_incoming_procedure = incoming_procedure.is_some();
                let EvidencePart::Merged(merged_geometry) =
                    merge_standard_evidence_part(geometry.take(), incoming_geometry)
                else {
                    conflict = true;
                    break;
                };
                geometry = merged_geometry;
                let EvidencePart::Merged(merged_procedure) =
                    merge_standard_evidence_part(procedure.take(), incoming_procedure)
                else {
                    conflict = true;
                    break;
                };
                procedure = merged_procedure;
                if !had_procedure && has_incoming_procedure {
                    procedure_valid = valid_procedure[index];
                }
            }
            if conflict {
                continue;
            }
            if let Some(geometry) = geometry {
                ctx.insert_hash_map(
                    &mut surface_geometries,
                    tag,
                    geometry,
                    "catia_standard_surface_geometries",
                )?;
            }
            if let Some(procedure) = procedure.filter(|_| procedure_valid) {
                ctx.insert_hash_map(
                    &mut procedural_surfaces,
                    tag,
                    procedure,
                    "catia_standard_procedural_surfaces",
                )?;
            }
        }
        Ok((surface_geometries, procedural_surfaces))
    }
}

impl StandardSurfaceEvidence {
    fn into_parts(self) -> (Option<SurfaceGeometry>, Option<StandardSurfaceProcedure>) {
        match self {
            Self::Geometry(geometry) => (Some(geometry), None),
            Self::Procedure(procedure) => (None, Some(procedure)),
            Self::Both(geometry, procedure) => (Some(geometry), Some(procedure)),
        }
    }

    fn from_parts(
        geometry: Option<SurfaceGeometry>,
        procedure: Option<StandardSurfaceProcedure>,
    ) -> Option<Self> {
        match (geometry, procedure) {
            (Some(geometry), None) => Some(Self::Geometry(geometry)),
            (None, Some(procedure)) => Some(Self::Procedure(procedure)),
            (Some(geometry), Some(procedure)) => Some(Self::Both(geometry, procedure)),
            (None, None) => None,
        }
    }

    #[cfg(test)]
    fn geometry_ref(&self) -> Option<&SurfaceGeometry> {
        match self {
            Self::Geometry(geometry) | Self::Both(geometry, _) => Some(geometry),
            Self::Procedure(_) => None,
        }
    }

    fn procedure_ref(&self) -> Option<&StandardSurfaceProcedure> {
        match self {
            Self::Procedure(procedure) | Self::Both(_, procedure) => Some(procedure),
            Self::Geometry(_) => None,
        }
    }
}

fn standard_object_evidence(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    tags: &HashSet<u32>,
    edge_tags: &HashSet<u32>,
    consolidated_records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<StandardObjectEvidence, cadmpeg_core::CodecError> {
    let mut evidence = standard_object_evidence_from_streams(
        ctx,
        container::logical_record_streams(ctx, scan)?,
        tags,
        edge_tags,
        refusal,
    )?;
    merge_standard_limit_curves_from_records(
        ctx,
        &mut evidence.limit_curves,
        &scan.data,
        consolidated_records,
        refusal,
    )?;
    Ok(evidence)
}

fn merge_standard_limit_curves_from_records(
    ctx: &DecodeContext<'_>,
    curves: &mut Vec<NurbsCurve>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<(), cadmpeg_core::CodecError> {
    for jet in crate::families::a5a8::records::a5_freeform_curves_from_records(ctx, data, records)?
    {
        for second_limit in [false, true] {
            let Some(geometry) = crate::families::a5a8::records::rolling_ball_limit_curve(
                ctx,
                &jet,
                second_limit,
                refusal,
            )?
            else {
                continue;
            };
            if !curves.contains(&geometry) {
                ctx.push_vec(curves, geometry, "catia standard limit curves")?;
            }
        }
    }
    Ok(())
}

pub(super) fn standard_object_evidence_from_streams(
    ctx: &DecodeContext<'_>,
    streams: impl IntoIterator<Item = Vec<u8>>,
    tags: &HashSet<u32>,
    edge_tags: &HashSet<u32>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<StandardObjectEvidence, cadmpeg_core::CodecError> {
    let mut evidence_store = StandardEvidenceStore::default();
    let mut edge_face_candidates = HashMap::<u32, Option<HashSet<u32>>>::new();
    let mut edge_support_candidates = HashMap::<u32, Option<StandardEdgeSupport>>::new();
    let mut limit_curves = Vec::<NurbsCurve>::new();
    let mut populations = Vec::new();
    for stream in streams {
        let records = crate::wire::records::consolidated_records_in_sources(
            ctx,
            &stream,
            std::iter::once(std::iter::once(crate::wire::records::SourceExtent::whole(
                &stream,
            ))),
        )?;
        merge_standard_limit_curves_from_records(
            ctx,
            &mut limit_curves,
            &stream,
            &records,
            refusal,
        )?;
        for population in crate::families::b5::graph::object_stream_populations(ctx, &stream)? {
            ctx.push_vec(
                &mut populations,
                population,
                "catia_standard_object_populations",
            )?;
        }
    }
    let mut population_objects = HashMap::<u32, Option<Vec<u8>>>::new();
    let mut seen_population_ids = HashSet::new();
    let mut repeated_population_ids = HashSet::new();
    for population in &populations {
        let mut objects = HashMap::<u32, Option<Vec<u8>>>::new();
        for frame in crate::families::b5::graph::object_stream_frames(population) {
            let bytes = ctx.copy_retained_slice(
                &population[frame.start..frame.end],
                "catia_standard_population_object_bytes",
            )?;
            ctx.admit_hash_map_entry(
                &mut objects,
                &frame.object_id,
                "catia_standard_population_objects",
            )?;
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
            if !ctx.insert_hash_set(
                &mut seen_population_ids,
                object_id,
                "catia_standard_seen_population_ids",
            )? {
                ctx.insert_hash_set(
                    &mut repeated_population_ids,
                    object_id,
                    "catia_standard_repeated_population_ids",
                )?;
            }
            ctx.admit_hash_map_entry(
                &mut population_objects,
                &object_id,
                "catia_standard_population_objects_by_id",
            )?;
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
    let mut conflicting_population_ids = HashSet::new();
    for (object_id, bytes) in population_objects {
        if bytes.is_none() {
            ctx.insert_hash_set(
                &mut conflicting_population_ids,
                object_id,
                "catia_standard_conflicting_population_ids",
            )?;
        }
    }
    for stream in populations {
        let frames = crate::families::b5::graph::collect_object_stream_frames(ctx, &stream)?;
        let face_surfaces =
            crate::families::b5::graph::face_surface_references_from_frames(ctx, &stream, &frames)?;
        let mut surface_bindings = Vec::new();
        for binding in tags.iter().map(|&tag| (tag, tag)).chain(
            face_surfaces
                .iter()
                .filter(|(face_id, _)| tags.contains(face_id))
                .copied(),
        ) {
            ctx.push_vec(
                &mut surface_bindings,
                binding,
                "catia_standard_surface_bindings",
            )?;
        }
        let mut requested_surfaces = HashSet::new();
        for &(_, surface_id) in &surface_bindings {
            ctx.insert_hash_set(
                &mut requested_surfaces,
                surface_id,
                "catia_standard_requested_surfaces",
            )?;
        }
        let targeted_surfaces = crate::families::b5::graph::targeted_surfaces_from_frames(
            ctx,
            &stream,
            &requested_surfaces,
            &frames,
            refusal,
        )?;
        let targeted_graph = crate::families::b5::graph::targeted_geometry_graph_from_frames(
            ctx, &stream, &frames, refusal,
        )?;
        for &(object_id, surface_id) in &surface_bindings {
            let Some(surface) = targeted_surfaces.get(&surface_id) else {
                continue;
            };
            let graph_evidence = match targeted_graph.as_ref() {
                Some(graph) => standard_surface_evidence(ctx, graph, surface_id, refusal)?,
                None => None,
            };
            let evidence = if graph_evidence.is_some() {
                graph_evidence
            } else {
                let graph_carrier = match targeted_graph.as_ref() {
                    Some(graph) => {
                        crate::families::b5::transfer::resolved_surface_carrier_in_graph(
                            ctx, graph, surface_id, refusal,
                        )?
                    }
                    None => None,
                };
                (match graph_carrier {
                    Some(carrier) => Some(carrier),
                    None => crate::families::b5::transfer::resolved_surface_carrier(ctx, surface)?,
                })
                .map(|carrier| match carrier {
                    crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(geometry) => {
                        StandardSurfaceEvidence::Geometry(geometry)
                    }
                    crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall {
                        carrier_object_id,
                        definition,
                    } => {
                        StandardSurfaceEvidence::Procedure(StandardSurfaceProcedure::RollingBall {
                            carrier_object_id,
                            definition: Box::new(*definition),
                            source: StandardRollingBallSource::ObjectStreamA8,
                        })
                    }
                })
            };
            let Some(evidence) = evidence else {
                continue;
            };
            evidence_store.add(ctx, object_id, evidence)?;
        }
        if let Some(graph) = targeted_graph.as_ref() {
            for &(object_id, surface_id) in &surface_bindings {
                if evidence_store.surfaces.contains_key(&object_id) {
                    continue;
                }
                let Some(evidence) = standard_surface_evidence(ctx, graph, surface_id, refusal)?
                else {
                    continue;
                };
                evidence_store.add(ctx, object_id, evidence)?;
            }
        }
        let edge_pcurves = crate::families::b5::graph::edge_support_pcurve_references_from_frames(
            ctx, &stream, edge_tags, &frames,
        )?;
        let mut requested_pcurves = HashSet::new();
        for &pcurve_id in edge_pcurves.values().flatten() {
            ctx.insert_hash_set(
                &mut requested_pcurves,
                pcurve_id,
                "catia_standard_requested_pcurves",
            )?;
        }
        let mut pcurves = HashMap::<u32, Option<crate::families::a5a8::records::A8Pcurve>>::new();
        for pcurve in crate::families::a5a8::records::object_stream_pcurves(ctx, &stream)?
            .into_iter()
            .filter(|pcurve| requested_pcurves.contains(&pcurve.object_id))
        {
            ctx.admit_hash_map_entry(
                &mut pcurves,
                &pcurve.object_id,
                "catia_standard_pcurve_candidates",
            )?;
            pcurves
                .entry(pcurve.object_id)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| {
                        stored.support_id != pcurve.support_id
                            || stored.sites != pcurve.sites
                            || stored.range != pcurve.range
                    }) {
                        *stored = None;
                    }
                })
                .or_insert(Some(pcurve));
        }
        let mut surface_ids = HashSet::new();
        for pcurve in pcurves.values().filter_map(Option::as_ref) {
            ctx.insert_hash_set(
                &mut surface_ids,
                pcurve.support_id,
                "catia_standard_pcurve_surface_ids",
            )?;
        }
        let targeted_surfaces = crate::families::b5::graph::targeted_surfaces_from_frames(
            ctx,
            &stream,
            &surface_ids,
            &frames,
            refusal,
        )?;
        for (edge, references) in edge_pcurves {
            let sides = references.map(|reference| -> Result<_, cadmpeg_core::CodecError> {
                let Some(pcurve) = pcurves.get(&reference).and_then(Option::as_ref) else {
                    return Ok(None);
                };
                let Some(surface) = targeted_surfaces.get(&pcurve.support_id) else {
                    return Ok(None);
                };
                crate::families::b5::transfer::resolved_object_stream_pcurve(
                    ctx,
                    pcurve,
                    surface,
                    targeted_graph.as_ref(),
                    refusal,
                )
            });
            let [left, right] = sides;
            let [Some(first), Some(second)] = [left?, right?] else {
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
            ctx.admit_hash_map_entry(
                &mut edge_support_candidates,
                &edge,
                "catia_standard_edge_support_candidates",
            )?;
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
            crate::families::b5::graph::edge_face_references_from_frames(ctx, &stream, &frames)?;
        for (edge, owners) in stream_edge_faces {
            ctx.admit_hash_map_entry(
                &mut edge_face_candidates,
                &edge,
                "catia_standard_edge_face_candidates",
            )?;
            edge_face_candidates
                .entry(edge)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| *stored != owners) {
                        *stored = None;
                    }
                })
                .or_insert(Some(owners));
        }
        let Some(graph) =
            crate::families::b5::graph::parse_from_frames(ctx, &stream, &frames, refusal)?
        else {
            continue;
        };
        for &surface_id in tags {
            let Some(evidence) = standard_surface_evidence(ctx, &graph, surface_id, refusal)?
            else {
                continue;
            };
            evidence_store.add(ctx, surface_id, evidence)?;
        }
        for &(face_id, surface_id) in face_surfaces
            .iter()
            .filter(|(face_id, _)| tags.contains(face_id))
        {
            let evidence = standard_surface_evidence(ctx, &graph, surface_id, refusal)?;
            let Some(evidence) = evidence else { continue };
            evidence_store.add(ctx, face_id, evidence)?;
        }
    }
    let (surface_geometries, procedural_surfaces) =
        evidence_store.into_outputs(ctx, &conflicting_population_ids)?;
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
    let mut edge_owner_faces = HashMap::new();
    for (edge, owners) in edge_face_candidates {
        if let Some(owners) = owners {
            ctx.insert_hash_map(
                &mut edge_owner_faces,
                edge,
                owners,
                "catia_standard_edge_owner_faces",
            )?;
        }
    }
    let mut edge_supports = HashMap::new();
    for (edge, support) in edge_support_candidates {
        if let Some(support) = support {
            ctx.insert_hash_map(
                &mut edge_supports,
                edge,
                support,
                "catia_standard_edge_supports",
            )?;
        }
    }
    Ok(StandardObjectEvidence {
        surface_geometries,
        procedural_surfaces,
        edge_owner_faces,
        edge_supports,
        limit_curves,
    })
}

fn standard_surface_evidence(
    ctx: &DecodeContext<'_>,
    graph: &crate::families::b5::graph::B5Graph,
    surface_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<StandardSurfaceEvidence>, cadmpeg_core::CodecError> {
    let geometry =
        crate::families::b5::transfer::resolved_surface_geometry(ctx, graph, surface_id, refusal)?;
    let procedure = if let Some(offset) =
        crate::families::b5::transfer::resolved_offset_surface(ctx, graph, surface_id, refusal)?
    {
        Some(StandardSurfaceProcedure::Offset {
            carrier_object_id: offset.carrier_object_id,
            support_object_id: offset.support_object_id,
            support: offset.support,
            distance: offset.distance,
            parameter_bounds: offset.parameter_bounds,
        })
    } else if let Some(extrusion) =
        crate::families::b5::transfer::resolved_extrusion_surface(ctx, graph, surface_id, refusal)?
    {
        Some(StandardSurfaceProcedure::Extrusion(Box::new(extrusion)))
    } else if let Some((carrier_object_id, definition)) =
        crate::families::b5::transfer::resolved_surface_procedural_definition(
            ctx, graph, surface_id, refusal,
        )?
    {
        Some(StandardSurfaceProcedure::RollingBall {
            carrier_object_id,
            definition: Box::new(definition),
            source: StandardRollingBallSource::ObjectStreamA8,
        })
    } else {
        crate::families::b5::transfer::resolved_revolution_surface(ctx, graph, surface_id, refusal)?
            .map(Box::new)
            .map(StandardSurfaceProcedure::Revolution)
    };
    Ok(StandardSurfaceEvidence::from_parts(geometry, procedure))
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

/// Attach standard analytic carriers to faces only when every FBB face has a
/// decoded carrier and its stored sense byte.
fn attach_standard_faces(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let face_count = fbb::standard_face_count(ctx, brep)?.unwrap_or_default();
    if face_count == 0 || face_count != bindings.len() {
        return Ok(());
    }
    let body_id = ctx
        .copy_retained_text("catia:standard:body#0", "catia_standard_body_id")
        .and_then(|text| BodyId::mint(text).map_err(cadmpeg_core::CodecError::malformed))?;
    let region_id = ctx
        .copy_retained_text("catia:standard:region#0-0", "catia_standard_region_id")
        .and_then(|text| RegionId::mint(text).map_err(cadmpeg_core::CodecError::malformed))?;
    let shell_id = ctx
        .copy_retained_text("catia:standard:shell#0-0", "catia_standard_shell_id")
        .and_then(|text| ShellId::mint(text).map_err(cadmpeg_core::CodecError::malformed))?;
    let mut face_ids = Vec::new();
    for (face_index, (surface, forward, offset)) in bindings.iter().enumerate() {
        let face_id = FaceId::mint(ctx.format_retained(
            format_args!("catia:standard:face#{face_index:01}"),
            "catia_standard_face_id",
        )?)
        .map_err(CodecError::malformed)?;
        annotate(
            ctx,
            annotations,
            &face_id,
            "MainDataStream+SurfacicReps",
            u64_from_index(*offset),
            "surfacic_reps_face_sense",
            Exactness::ByteExact,
        )?;
        for field in ["shell", "surface", "sense"] {
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &face_id,
                field,
                "catia_annotation_field",
            )?;
        }
        ctx.push_vec(
            &mut face_ids,
            face_id.try_clone_for_decode(ctx, "catia_standard_shell_face_id")?,
            "catia_standard_shell_face_ids",
        )?;
        admission.reserve_entity(&mut ir.model.faces, "catia_standard_model_faces")?;
        ir.model.faces.push(Face {
            id: face_id,
            shell: shell_id.try_clone_for_decode(ctx, "catia_standard_face_shell_id")?,
            surface: surface.try_clone_for_decode(ctx, "catia_standard_face_surface_id")?,
            sense: if *forward {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        });
    }
    annotate(
        ctx,
        annotations,
        &body_id,
        "MainDataStream+SurfacicReps",
        0,
        "standard_body",
        Exactness::Inferred,
    )?;
    crate::resource::derived_annotation(
        ctx,
        annotations,
        &body_id,
        "kind",
        "catia_annotation_field",
    )?;
    crate::resource::derived_annotation(
        ctx,
        annotations,
        &body_id,
        "regions",
        "catia_annotation_field",
    )?;
    let mut body_regions = Vec::new();
    ctx.push_vec(
        &mut body_regions,
        region_id.try_clone_for_decode(ctx, "catia_standard_body_region_id")?,
        "catia_standard_body_regions",
    )?;
    admission.reserve_entity(&mut ir.model.bodies, "catia_standard_model_bodies")?;
    ir.model.bodies.push(Body {
        id: body_id.try_clone_for_decode(ctx, "catia_standard_model_body_id")?,
        kind: BodyKind::Sheet,
        regions: body_regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    annotate(
        ctx,
        annotations,
        &region_id,
        "MainDataStream+SurfacicReps",
        0,
        "derived_region",
        Exactness::Inferred,
    )?;
    crate::resource::derived_annotation(
        ctx,
        annotations,
        &region_id,
        "body",
        "catia_annotation_field",
    )?;
    crate::resource::derived_annotation(
        ctx,
        annotations,
        &region_id,
        "shells",
        "catia_annotation_field",
    )?;
    let mut region_shells = Vec::new();
    ctx.push_vec(
        &mut region_shells,
        shell_id.try_clone_for_decode(ctx, "catia_standard_region_shell_id")?,
        "catia_standard_region_shells",
    )?;
    admission.reserve_entity(&mut ir.model.regions, "catia_standard_model_regions")?;
    ir.model.regions.push(Region {
        id: region_id.try_clone_for_decode(ctx, "catia_standard_model_region_id")?,
        body: body_id,
        shells: region_shells,
    });
    annotate(
        ctx,
        annotations,
        &shell_id,
        "MainDataStream+SurfacicReps",
        0,
        "derived_shell",
        Exactness::Inferred,
    )?;
    crate::resource::derived_annotation(
        ctx,
        annotations,
        &shell_id,
        "region",
        "catia_annotation_field",
    )?;
    crate::resource::derived_annotation(
        ctx,
        annotations,
        &shell_id,
        "faces",
        "catia_annotation_field",
    )?;
    admission.reserve_entity(&mut ir.model.shells, "catia_standard_model_shells")?;
    ir.model.shells.push(
        Shell::with_faces(shell_id, region_id, face_ids)
            .map_err(cadmpeg_core::CodecError::malformed)?,
    );
    Ok(())
}

fn partition_standard_face_components(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    components: &[Vec<usize>],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    if components.is_empty()
        || components.iter().any(Vec::is_empty)
        || components.iter().flatten().count() != ir.model.faces.len()
    {
        return Ok(false);
    }
    let body_id = ctx
        .copy_retained_text("catia:standard:body#0", "catia_standard_partition_body_id")
        .and_then(|text| BodyId::mint(text).map_err(cadmpeg_core::CodecError::malformed))?;
    let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == body_id) else {
        return Ok(false);
    };
    let mut region_ids = Vec::new();
    for component in 0..components.len() {
        let id = RegionId::mint(ctx.format_retained(
            format_args!("catia:standard:region#0-{component:01}"),
            "catia_standard_partition_region_id",
        )?)
        .map_err(CodecError::malformed)?;
        ctx.push_vec(&mut region_ids, id, "catia_standard_partition_region_ids")?;
    }
    let mut body_regions = Vec::new();
    for id in &region_ids {
        ctx.push_vec(
            &mut body_regions,
            id.try_clone_for_decode(ctx, "catia_standard_partition_body_region_id")?,
            "catia_standard_partition_body_regions",
        )?;
    }
    body.regions = body_regions;
    crate::resource::derived_annotation(
        ctx,
        annotations,
        &body_id,
        "regions",
        "catia_annotation_field",
    )?;

    for (component, faces) in components.iter().enumerate() {
        let region_id = region_ids[component]
            .try_clone_for_decode(ctx, "catia_standard_partition_region_copy")?;
        let shell_id = ShellId::mint(ctx.format_retained(
            format_args!("catia:standard:shell#0-{component:01}"),
            "catia_standard_partition_shell_id",
        )?)
        .map_err(CodecError::malformed)?;
        let mut face_ids = Vec::new();
        for &face in faces {
            let id = FaceId::mint(ctx.format_retained(
                format_args!("catia:standard:face#{face:01}"),
                "catia_standard_partition_face_id",
            )?)
            .map_err(CodecError::malformed)?;
            ctx.push_vec(&mut face_ids, id, "catia_standard_partition_face_ids")?;
        }
        let mut region_shells = Vec::new();
        ctx.push_vec(
            &mut region_shells,
            shell_id.try_clone_for_decode(ctx, "catia_standard_partition_region_shell_id")?,
            "catia_standard_partition_region_shells",
        )?;
        for &face in faces {
            let Some(face) = ir.model.faces.get_mut(face) else {
                return Ok(false);
            };
            face.shell =
                shell_id.try_clone_for_decode(ctx, "catia_standard_partition_face_shell_id")?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &face.id,
                "shell",
                "catia_annotation_field",
            )?;
        }
        if component == 0 {
            let Some(region) = ir
                .model
                .regions
                .iter_mut()
                .find(|region| region.id == region_id)
            else {
                return Ok(false);
            };
            region.shells = region_shells;
            let Some(shell) = ir
                .model
                .shells
                .iter_mut()
                .find(|shell| shell.id == shell_id)
            else {
                return Ok(false);
            };
            if shell.replace_faces(face_ids).is_err() {
                return Ok(false);
            }
            continue;
        }
        for (id, tag) in [
            (region_id.as_str(), "derived_region"),
            (shell_id.as_str(), "derived_shell"),
        ] {
            annotate(
                ctx,
                annotations,
                id,
                "MainDataStream+SurfacicReps",
                0,
                tag,
                Exactness::Inferred,
            )?;
        }
        for field in ["body", "shells"] {
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &region_id,
                field,
                "catia_annotation_field",
            )?;
        }
        admission.reserve_entity(&mut ir.model.regions, "catia_standard_partition_regions")?;
        ir.model.regions.push(Region {
            id: region_id.try_clone_for_decode(ctx, "catia_standard_partition_model_region_id")?,
            body: body_id.try_clone_for_decode(ctx, "catia_standard_partition_region_body_id")?,
            shells: region_shells,
        });
        for field in ["region", "faces"] {
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &shell_id,
                field,
                "catia_annotation_field",
            )?;
        }
        admission.reserve_entity(&mut ir.model.shells, "catia_standard_partition_shells")?;
        ir.model
            .shells
            .push(match Shell::with_faces(shell_id, region_id, face_ids) {
                Ok(shell) => shell,
                Err(_) => {
                    return Ok(false);
                }
            });
    }
    Ok(true)
}

pub(super) fn apply_standard_native_edge_faces(
    ctx: &DecodeContext<'_>,
    edge_faces: &mut [[usize; 2]],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    native_edge_faces: &HashMap<u32, HashSet<u32>>,
) -> Result<(), CodecError> {
    if edge_faces.len() != supports.len() {
        return Ok(());
    }
    let mut face_by_carrier = HashMap::<u32, Option<usize>>::new();
    for (face, record) in records.iter().enumerate() {
        let carrier = match record {
            crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) => {
                prefix.target
            }
            crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } => *tag,
        };
        if let Some(stored) = face_by_carrier.get_mut(&carrier) {
            *stored = None;
        } else {
            ctx.insert_hash_map(
                &mut face_by_carrier,
                carrier,
                Some(face),
                "catia_standard_native_face_carriers",
            )?;
        }
    }
    for (faces, support) in edge_faces.iter_mut().zip(supports) {
        if faces[0] != faces[1] {
            continue;
        }
        let Some(owner_ids) = native_edge_faces.get(&support.tag) else {
            continue;
        };
        let mut candidates = HashSet::new();
        for face in owner_ids
            .iter()
            .filter_map(|owner| face_by_carrier.get(owner).copied().flatten())
            .filter(|face| *face != faces[0])
        {
            ctx.insert_hash_set(
                &mut candidates,
                face,
                "catia_standard_native_face_candidates",
            )?;
        }
        if let Some(&face) = candidates.iter().next().filter(|_| candidates.len() == 1) {
            faces[1] = face;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StandardLimitCurveBinding {
    curve: usize,
    points: [usize; 2],
    parameter_range: [f64; 2],
}

type BezierSpan = [Point3; 6];

fn bezier_levels(control: BezierSpan) -> [[Point3; 6]; 6] {
    let mut levels = [control; 6];
    for degree in 0..5 {
        for index in 0..(5 - degree) {
            levels[degree + 1][index] = Point3::new(
                levels[degree][index]
                    .x
                    .midpoint(levels[degree][index + 1].x),
                levels[degree][index]
                    .y
                    .midpoint(levels[degree][index + 1].y),
                levels[degree][index]
                    .z
                    .midpoint(levels[degree][index + 1].z),
            );
        }
    }
    levels
}

fn split_bezier_half(control: BezierSpan) -> (BezierSpan, BezierSpan) {
    let levels = bezier_levels(control);
    let left = std::array::from_fn(|index| levels[index][0]);
    let right = std::array::from_fn(|index| levels[5 - index][index]);
    (left, right)
}

fn collect_bezier_point_parameters(
    ctx: &DecodeContext<'_>,
    control: BezierSpan,
    range: [f64; 2],
    point: Point3,
    tolerance: f64,
    parameter_resolution: f64,
    parameters: &mut Vec<(f64, f64)>,
) -> Result<(), CodecError> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    struct Node {
        control: BezierSpan,
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
    let midpoint = |control: &BezierSpan| bezier_levels(*control)[5][0];

    let root_lower_bound = lower_bound(&control);
    if root_lower_bound > tolerance {
        return Ok(());
    }
    let root_midpoint = midpoint(&control);
    let mut best = (range[0].midpoint(range[1]), root_midpoint.distance(point));
    let first = control[0];
    let last = control[5];
    for (parameter, position) in [(range[0], first), (range[1], last)] {
        let distance = position.distance(point);
        if distance < best.1 {
            best = (parameter, distance);
        }
    }

    let mut nodes = Vec::new();
    ctx.push_vec(
        &mut nodes,
        Node {
            control,
            range,
            depth: 0,
        },
        "catia_bezier_search_nodes",
    )?;
    let mut queue = BinaryHeap::new();
    ctx.reserve_heap(&mut queue, 1, "catia_bezier_search_queue")?;
    queue.push((Reverse(root_lower_bound.to_bits()), 0usize));
    while let Some((Reverse(lower_bits), node_index)) = queue.pop() {
        ctx.charge_work(1, "catia_bezier_search_work")?;
        let lower = f64::from_bits(lower_bits);
        if lower > tolerance || lower > best.1 {
            continue;
        }
        let node = &nodes[node_index];
        if node.depth >= 48 || node.range[1] - node.range[0] <= parameter_resolution {
            let position = midpoint(&node.control);
            let candidate = (
                node.range[0].midpoint(node.range[1]),
                position.distance(point),
            );
            if candidate.1 < best.1 {
                best = candidate;
            }
            if candidate.1 <= tolerance {
                ctx.push_vec(parameters, candidate, "catia_bezier_parameters")?;
            }
            continue;
        }
        let (left, right) = split_bezier_half(node.control);
        let middle = node.range[0].midpoint(node.range[1]);
        let depth = node.depth + 1;
        for (control, range) in [
            (left, [node.range[0], middle]),
            (right, [middle, node.range[1]]),
        ] {
            let lower = lower_bound(&control);
            if lower > tolerance || lower > best.1 {
                continue;
            }
            let position = midpoint(&control);
            let candidate = (range[0].midpoint(range[1]), position.distance(point));
            if candidate.1 < best.1 {
                best = candidate;
            }
            let index = nodes.len();
            ctx.push_vec(
                &mut nodes,
                Node {
                    control,
                    range,
                    depth,
                },
                "catia_bezier_search_nodes",
            )?;
            ctx.reserve_heap(&mut queue, 1, "catia_bezier_search_queue")?;
            queue.push((Reverse(lower.to_bits()), index));
        }
    }
    if best.1 <= tolerance {
        ctx.push_vec(parameters, best, "catia_bezier_parameters")?;
    }
    Ok(())
}

fn standard_limit_curve_point_parameter(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    point: Point3,
    tolerance: f64,
) -> Result<Option<f64>, CodecError> {
    let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } = curve.pole_rows() else {
        return Ok(None);
    };
    let span_count = points.len() / 6;
    if span_count == 0
        || span_count * 6 != points.len()
        || curve.knots().len() != (span_count + 1) * 6
        || curve.degree() != 5
    {
        return Ok(None);
    }
    let Some(domain) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(curve) else {
        return Ok(None);
    };
    let [parameter_start, parameter_end] = domain.endpoints();
    let parameter_span = parameter_end - parameter_start;
    let control_polygon_length = points
        .chunks_exact(6)
        .map(|control| {
            control
                .windows(2)
                .map(|pair| pair[0].distance(pair[1].get()))
                .sum::<f64>()
        })
        .sum::<f64>();
    let (parameter_tolerance, parameter_resolution) = if parameter_span.is_finite() {
        let parameter_tolerance = (4.0 * tolerance * parameter_span
            / control_polygon_length.max(tolerance))
        .max(EPS_PARAM_TOLERANCE_SPAN * parameter_span);
        (
            parameter_tolerance,
            0.05 * parameter_tolerance.min(EPS_PARAM_RESOLUTION_SPAN * parameter_span),
        )
    } else {
        let half_span = parameter_end * 0.5 - parameter_start * 0.5;
        let tolerance_fraction =
            (4.0 * tolerance / control_polygon_length.max(tolerance)).max(EPS_PARAM_TOLERANCE_SPAN);
        (
            2.0 * (half_span * tolerance_fraction),
            2.0 * (half_span * (0.05 * tolerance_fraction.min(EPS_PARAM_RESOLUTION_SPAN))),
        )
    };
    let mut parameters = Vec::new();
    for (span, control_points) in points.chunks_exact(6).enumerate() {
        let control: BezierSpan = std::array::from_fn(|index| control_points[index].get());
        collect_bezier_point_parameters(
            ctx,
            control,
            [curve.knots()[span * 6], curve.knots()[(span + 1) * 6]],
            point,
            tolerance,
            parameter_resolution,
            &mut parameters,
        )?;
    }
    parameters.sort_by(|left, right| left.1.total_cmp(&right.1));
    let Some(&(parameter, _)) = parameters.first() else {
        return Ok(None);
    };
    let ambiguous = parameters
        .iter()
        .skip(1)
        .any(|&(other, _)| (other - parameter).abs() > parameter_tolerance);
    Ok((!ambiguous).then_some(parameter))
}

fn standard_limit_curve_bindings(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    curves: &[NurbsCurve],
) -> Result<Vec<Vec<StandardLimitCurveBinding>>, CodecError> {
    const VERTEX_MATCH_TOLERANCE: f64 = 2e-3;

    let mut curve_points = Vec::new();
    ctx.reserve_vec(
        &mut curve_points,
        curves.len(),
        "catia_limit_curve_point_rows",
    )?;
    for curve in curves {
        let mut row = Vec::new();
        for (point, value) in ir.model.points.iter().enumerate() {
            if let Some(parameter) = standard_limit_curve_point_parameter(
                ctx,
                curve,
                value.position().get(),
                VERTEX_MATCH_TOLERANCE,
            )? {
                ctx.push_vec(
                    &mut row,
                    (point, parameter),
                    "catia_limit_curve_point_parameters",
                )?;
            }
        }
        curve_points.push(row);
    }
    let mut edge_curves = ctx.alloc_filled(
        supports.len(),
        Vec::<StandardLimitCurveBinding>::new(),
        "catia_limit_curve_edge_rows",
    )?;
    for (curve, points) in curve_points.iter().enumerate() {
        for (edge, support) in supports.iter().enumerate() {
            if !matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Bspline
            ) {
                continue;
            }
            let mut candidates = Vec::new();
            for (point, parameter) in points.iter().copied() {
                let res = {
                    let position = ir.model.points[point].position().get();
                    let mut all_faces = true;
                    for face in support.faces {
                        let Some(surface) = face_surface(ir, bindings, surface_indices, face)
                        else {
                            all_faces = false;
                            break;
                        };
                        if !matches!(
                            surface.geometry,
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                        ) && !point_on_surface(position, &surface.geometry)?
                        {
                            all_faces = false;
                            break;
                        }
                    }
                    all_faces
                };
                if res {
                    ctx.push_vec(
                        &mut candidates,
                        (point, parameter),
                        "catia_limit_curve_candidates",
                    )?;
                }
            }
            let Ok([(start, start_parameter), (end, end_parameter)]) =
                <[(usize, f64); 2]>::try_from(candidates)
            else {
                continue;
            };
            let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                curves[curve].try_clone_for_decode(ctx, "catia_limit_curve_geometry_copy")?,
            ));
            let midpoint = match cadmpeg_ir::eval::decode::curve_point_for_decode(ctx, &geometry, 0.5 * (start_parameter + end_parameter))? {
                Ok(point) => point,
                Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                    return Err(limit.into());
                }
                Err(
                    cadmpeg_ir::eval::EvaluationFailure::NoValue
                    | cadmpeg_ir::eval::EvaluationFailure::NonFinite(_),
                ) => continue,
            };
            let mut checked_surface = false;
            let mut agrees = true;
            for face in support.faces {
                let Some(surface) = face_surface(ir, bindings, surface_indices, face) else {
                    agrees = false;
                    break;
                };
                if matches!(
                    surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                ) {
                    continue;
                }
                checked_surface = true;
                if !point_on_surface(midpoint.get(), &surface.geometry)? {
                    agrees = false;
                    break;
                }
            }
            if checked_surface && agrees {
                ctx.push_vec(
                    &mut edge_curves[edge],
                    StandardLimitCurveBinding {
                        curve,
                        points: [start, end],
                        parameter_range: [start_parameter, end_parameter],
                    },
                    "catia_limit_curve_edge_bindings",
                )?;
            }
        }
    }
    Ok(edge_curves)
}

fn resolve_standard_limit_curve_binding(
    bindings: &[StandardLimitCurveBinding],
    points: [usize; 2],
) -> Option<StandardLimitCurveBinding> {
    let mut matches = bindings
        .iter()
        .filter(|binding| missing_edge::same_unordered_pair(binding.points, points))
        .copied();
    let mut binding = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    if binding.points != points {
        binding.points.reverse();
        binding.parameter_range.reverse();
    }
    Some(binding)
}

struct AttachStandardTopologyInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input6,
    'input7,
    'input8,
    'input9,
    'input10,
    'input11,
    'input12,
    'input13,
    'input14,
    'input15,
    'input16,
    'input17,
    'input18,
    'input19,
> {
    ir: &'input0 mut CadIr,
    annotations: &'input1 mut AnnotationBuilder,
    bindings: &'input2 [(SurfaceId, bool, usize)],
    records: &'input3 [crate::families::standard::records::StandardSurfaceRecord],
    face_bounds: &'input4 [Option<crate::families::standard::records::StandardFaceBounds>],
    spine: &'input5 [u8],
    edge_table_form: EdgeTableForm,
    brep: &'input6 [u8],
    support_override: Option<&'input7 [crate::families::standard::records::StandardCurveSupport]>,
    source: &'input8 [u8],
    use_vertex_roster: bool,
    native_edge_faces: &'input9 HashMap<u32, HashSet<u32>>,
    native_edge_supports: &'input10 HashMap<u32, StandardEdgeSupport>,
    limit_curves: &'input11 [NurbsCurve],
    work_budget: &'input13 WorkBudget<'input12>,
    diagnostics: &'input14 mut StandardTopologyDiagnostics,
    bound_limit_curve_count: &'input15 mut usize,
    refusal: &'input16 mut crate::nurbs::LaneRefusals,
    admission: &'input19 mut FamilyEntityAdmission<'input17, 'input18>,
}

fn attach_standard_topology(
    ctx: &DecodeContext<'_>,
    inputs: AttachStandardTopologyInputs<
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
    >,
) -> Result<(), StandardTopologyError> {
    let AttachStandardTopologyInputs {
        ir,
        annotations,
        bindings,
        records,
        face_bounds,
        spine,
        edge_table_form,
        brep,
        support_override,
        source,
        use_vertex_roster,
        native_edge_faces,
        native_edge_supports,
        limit_curves,
        work_budget,
        diagnostics,
        bound_limit_curve_count,
        refusal,
        admission,
    } = inputs;

    let face_count = ir.model.faces.len();
    let edge_count = if edge_table_form == EdgeTableForm::FbbOnly {
        crate::families::standard::fbb::fbb_only_edge_count(ctx, spine)
            .map_err(StandardTopologyError::Resource)?
    } else {
        crate::families::standard::fbb::standard_edge_count(ctx, spine)
            .map_err(StandardTopologyError::Resource)?
    };
    let Some(edge_count) = edge_count.filter(|count| *count > 0) else {
        return Err(StandardTopologyFailure::NoCurveSupports.into());
    };
    let mut supports = support_override
        .map_or_else(
            || {
                crate::families::standard::records::standard_curve_supports(
                    ctx,
                    brep,
                    face_count,
                    Some(edge_count),
                )
            },
            |supports| ctx.copy_retained_slice(supports, "catia_topology_support_override"),
        )
        .map_err(StandardTopologyError::Resource)?;
    if supports.is_empty() {
        return Err(StandardTopologyFailure::NoCurveSupports.into());
    }
    diagnostics.curve_supports = supports.len();
    let serialized_edge_faces = ctx
        .collect_vec(
            supports.iter().map(|support| support.faces),
            "catia_standard_serialized_edge_faces",
        )
        .map_err(StandardTopologyError::Resource)?;
    let Some(mut edge_faces) =
        missing_edge::resolve_standard_edge_faces(ctx, spine, &serialized_edge_faces)
            .map_err(StandardTopologyError::Resource)?
    else {
        return Err(StandardTopologyFailure::EdgeFaceAssignment.into());
    };
    let mut deferred_port_edges = ctx
        .alloc_filled(supports.len(), false, "catia_deferred_port_edges")
        .map_err(StandardTopologyError::Resource)?;
    let mut open_face_domains = None;
    let mut endpoint_face_assignments = None;
    apply_standard_native_edge_faces(ctx, &mut edge_faces, &supports, records, native_edge_faces)
        .map_err(StandardTopologyError::Resource)?;
    for (support, faces) in supports.iter_mut().zip(&edge_faces) {
        support.faces = *faces;
    }
    let mut surface_indices = HashMap::new();
    for (index, surface) in ir.model.surfaces.iter().enumerate() {
        let id = surface
            .id
            .try_clone_for_decode(ctx, "catia_standard_surface_id_copy")
            .map_err(StandardTopologyError::Resource)?;
        ctx.insert_hash_map(
            &mut surface_indices,
            id,
            index,
            "catia_standard_surface_indices",
        )
        .map_err(StandardTopologyError::Resource)?;
    }
    let face_bounds = (face_bounds.len() == face_count).then_some(face_bounds);
    let face_point_membership =
        standard_face_point_membership(ctx, ir, bindings, &surface_indices, face_bounds)
            .map_err(StandardTopologyError::Resource)?;
    let limit_curve_bindings =
        standard_limit_curve_bindings(ctx, ir, bindings, &surface_indices, &supports, limit_curves)
            .map_err(StandardTopologyError::Resource)?;
    let mut ordered_endpoint_pairs = ctx
        .alloc_filled(supports.len(), None, "catia_ordered_endpoint_pairs")
        .map_err(StandardTopologyError::Resource)?;
    let mut point_coordinates = Vec::new();
    ctx.reserve_vec(
        &mut point_coordinates,
        ir.model.points.len(),
        "catia_visualization_point_coordinates",
    )
    .map_err(StandardTopologyError::Resource)?;
    for point in &ir.model.points {
        point_coordinates.push([
            f32_from_f64(point.position().get().x)
                .ok_or(StandardTopologyFailure::ConflictingNativeEndpoints)?,
            f32_from_f64(point.position().get().y)
                .ok_or(StandardTopologyFailure::ConflictingNativeEndpoints)?,
            f32_from_f64(point.position().get().z)
                .ok_or(StandardTopologyFailure::ConflictingNativeEndpoints)?,
        ]);
    }
    let visualization_endpoint_pairs =
        missing_edge::standard_edge_rows(ctx, spine).map_err(StandardTopologyError::Resource)?;
    let visualization_endpoint_pairs = match visualization_endpoint_pairs {
        Some(rows) => {
            missing_edge::visualization_endpoint_pairs(ctx, source, &rows, &point_coordinates)
                .map_err(StandardTopologyError::Resource)?
        }
        None => None,
    };
    if let Some(pairs) = &visualization_endpoint_pairs {
        if pairs.len() != ordered_endpoint_pairs.len() {
            return Err(StandardTopologyFailure::ConflictingNativeEndpoints.into());
        }
    }
    let mut endpoint_candidates = Vec::new();
    ctx.reserve_vec(
        &mut endpoint_candidates,
        supports.len(),
        "catia_standard_endpoint_candidate_rows",
    )
    .map_err(StandardTopologyError::Resource)?;
    let mut incidence_candidates = HashMap::<[usize; 2], Vec<usize>>::new();
    let mut face_incidence_candidates = HashMap::<usize, Vec<usize>>::new();
    for support in &supports {
        let Some(surface0) = face_surface(ir, bindings, &surface_indices, support.faces[0]) else {
            return Err(StandardTopologyFailure::MissingFaceSurface.into());
        };
        let Some(surface1) = face_surface(ir, bindings, &surface_indices, support.faces[1]) else {
            return Err(StandardTopologyFailure::MissingFaceSurface.into());
        };
        let candidates = match &support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Circle {
                center,
                radius,
            } => standard_circle_endpoint_candidates(
                ctx,
                &ir.model.points,
                center.get(),
                radius.get(),
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
            )
            .map_err(StandardTopologyError::Resource)?,
            crate::families::standard::records::StandardCurveGeometry::Line
            | crate::families::standard::records::StandardCurveGeometry::Bspline => {
                let mut faces = support.faces;
                faces.sort_unstable();
                for (face, surface) in [
                    (support.faces[0], &surface0.geometry),
                    (support.faces[1], &surface1.geometry),
                ] {
                    if !face_incidence_candidates.contains_key(&face) {
                        let mut points = Vec::new();
                        for (index, point) in ir.model.points.iter().enumerate() {
                            if point_on_standard_face(
                                point.position().get(),
                                surface,
                                face_bounds.as_ref().and_then(|bounds| bounds[face]),
                            )
                            .map_err(CodecError::from)
                            .map_err(StandardTopologyError::Resource)?
                            {
                                ctx.push_vec(&mut points, index, "catia_face_incidence_points")
                                    .map_err(StandardTopologyError::Resource)?;
                            }
                        }
                        ctx.insert_hash_map(
                            &mut face_incidence_candidates,
                            face,
                            points,
                            "catia_face_incidence_rows",
                        )
                        .map_err(StandardTopologyError::Resource)?;
                    }
                }
                if !incidence_candidates.contains_key(&faces) {
                    let mut right = HashSet::new();
                    for point in face_incidence_candidates[&faces[1]].iter().copied() {
                        ctx.insert_hash_set(&mut right, point, "catia_incidence_right_points")
                            .map_err(StandardTopologyError::Resource)?;
                    }
                    let mut shared = Vec::new();
                    for point in face_incidence_candidates[&faces[0]]
                        .iter()
                        .copied()
                        .filter(|point| right.contains(point))
                    {
                        ctx.push_vec(&mut shared, point, "catia_incidence_shared_points")
                            .map_err(StandardTopologyError::Resource)?;
                    }
                    ctx.insert_hash_map(
                        &mut incidence_candidates,
                        faces,
                        shared,
                        "catia_incidence_candidate_rows",
                    )
                    .map_err(StandardTopologyError::Resource)?;
                }
                ctx.copy_retained_slice(
                    &incidence_candidates[&faces],
                    "catia_incidence_candidate_copy",
                )
                .map_err(StandardTopologyError::Resource)?
            }
        };
        endpoint_candidates.push(candidates);
    }
    let edge_classes =
        standard_curve_edge_classes(ctx, &supports).map_err(StandardTopologyError::Resource)?;
    let edge_geometry = standard_curve_geometry_gauge_keys(ctx, &supports)
        .map_err(StandardTopologyError::Resource)?;
    let topology_graph = crate::families::b5::graph::parse(ctx, source, refusal)
        .map_err(StandardTopologyError::Resource)?;
    let native_edges = match topology_graph.as_ref() {
        Some(graph) => graph
            .referenced_edge_vertex_references(ctx)
            .map_err(StandardTopologyError::Resource)?,
        None => None,
    };
    let mut native_edges = match native_edges {
        Some(edges) => edges,
        None => crate::families::b5::graph::edge_vertex_references(ctx, source)
            .map_err(StandardTopologyError::Resource)?,
    };
    let e5_topology = match crate::container::e5_record_stream(source) {
        Some(range) => crate::families::e5::graph::parse_topology(ctx, &source[range])
            .map_err(StandardTopologyError::Resource)?,
        None => None,
    };
    if let Some(e5_topology) = e5_topology {
        let e5_edges = e5_topology
            .edges
            .into_iter()
            .map(|(record_id, edge)| (record_id, [edge.start_vertex, edge.end_vertex]));
        if !merge_standard_edge_vertex_references(&mut native_edges, e5_edges) {
            return Err(StandardTopologyFailure::ConflictingNativeEndpoints.into());
        }
    }
    let graph_endpoint_pairs = standard_native_graph_endpoint_pairs(
        ctx,
        topology_graph.as_ref(),
        &supports,
        &native_edges,
        &ir.model.points,
    )
    .map_err(StandardTopologyError::Resource)?;
    let mut native_port_options = Vec::new();
    ctx.reserve_vec(
        &mut native_port_options,
        supports.len(),
        "catia_native_port_options",
    )
    .map_err(StandardTopologyError::Resource)?;
    native_port_options.extend(
        supports
            .iter()
            .map(|support| native_edges.get(&support.tag).copied()),
    );
    let mut native_ports = Vec::new();
    let mut all_ports = true;
    for pair in native_port_options.iter().copied() {
        let Some(pair) = pair else {
            all_ports = false;
            break;
        };
        ctx.push_vec(&mut native_ports, pair, "catia_native_port_pairs")
            .map_err(StandardTopologyError::Resource)?;
    }
    let native_ports = all_ports.then_some(native_ports);
    let vertex_roster = if use_vertex_roster {
        crate::families::standard::records::standard_vertex_roster(
            ctx,
            source,
            ir.model.points.len(),
        )
        .map_err(StandardTopologyError::Resource)?
    } else {
        None
    };
    let allocation_endpoint_points = vertex_roster
        .as_ref()
        .map(|roster| standard_successor_endpoint_points(ctx, &supports, roster))
        .transpose()
        .map_err(StandardTopologyError::Resource)?;
    let roster_endpoint_pairs = vertex_roster
        .as_ref()
        .map(|roster| standard_serialized_endpoint_pairs(ctx, &supports, &native_edges, roster))
        .transpose()
        .map_err(StandardTopologyError::Resource)?
        .flatten();
    let mut native_support_ids = HashSet::new();
    for id in native_edge_supports.keys().copied() {
        ctx.insert_hash_set(&mut native_support_ids, id, "catia_native_support_ids")
            .map_err(StandardTopologyError::Resource)?;
    }
    let native_support_edge_ids =
        standard_native_support_edge_ids(ctx, &supports, &native_support_ids)
            .map_err(StandardTopologyError::Resource)?;
    let mut native_supports_by_row = Vec::new();
    ctx.reserve_vec(
        &mut native_supports_by_row,
        native_support_edge_ids.len(),
        "catia_native_support_rows",
    )
    .map_err(StandardTopologyError::Resource)?;
    native_supports_by_row.extend(
        native_support_edge_ids
            .iter()
            .map(|edge| edge.and_then(|edge| native_edge_supports.get(&edge))),
    );
    let Ok(native_endpoint_evidence) = merge_native_endpoint_evidence(
        ctx,
        graph_endpoint_pairs.as_deref(),
        roster_endpoint_pairs.as_deref(),
    )
    .map_err(StandardTopologyError::Resource)?
    else {
        return Err(StandardTopologyFailure::ConflictingNativeEndpoints.into());
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
                return Err(StandardTopologyFailure::ConflictingNativeEndpoints.into());
            }
        }
    }
    if let Some(pairs) = visualization_endpoint_pairs {
        for (edge, pair) in pairs.into_iter().enumerate() {
            if !merge_derived_endpoint_pair(&mut ordered_endpoint_pairs, edge, pair) {
                return Err(StandardTopologyFailure::ConflictingNativeEndpoints.into());
            }
        }
    }
    for (edge, bindings) in limit_curve_bindings.iter().enumerate() {
        let Ok([binding]) = <[StandardLimitCurveBinding; 1]>::try_from(bindings.as_slice()) else {
            continue;
        };
        if !merge_derived_endpoint_pair(&mut ordered_endpoint_pairs, edge, binding.points) {
            return Err(StandardTopologyFailure::ConflictingNativeEndpoints.into());
        }
    }
    if let Some(pairs) = &native_endpoint_evidence {
        include_native_endpoint_pairs(ctx, &mut endpoint_candidates, pairs)
            .map_err(StandardTopologyError::Resource)?;
    }
    let mut endpoint_options = resolve_standard_endpoint_pairs(
        ctx,
        ir,
        bindings,
        &surface_indices,
        &supports,
        &endpoint_candidates,
    )
    .map_err(StandardTopologyError::Resource)?;
    if let Some(options) = &mut endpoint_options {
        for (edge, bindings) in limit_curve_bindings.iter().enumerate() {
            if bindings.is_empty() {
                continue;
            }
            let mut limit_pairs = Vec::new();
            ctx.reserve_vec(
                &mut limit_pairs,
                bindings.len(),
                "catia_limit_endpoint_pairs",
            )
            .map_err(StandardTopologyError::Resource)?;
            limit_pairs.extend(bindings.iter().map(|binding| {
                let mut points = binding.points;
                points.sort_unstable();
                points
            }));
            limit_pairs.sort_unstable();
            limit_pairs.dedup();
            if options[edge].is_empty() {
                options[edge] = limit_pairs;
            }
        }
    }
    for edge in 0..supports.len() {
        let native_pair = match native_supports_by_row.get(edge).and_then(Option::as_ref) {
            Some(native) => standard_native_support_endpoint_pair(
                native,
                &ir.model.points,
                &endpoint_candidates[edge],
                native_endpoint_evidence
                    .as_ref()
                    .and_then(|pairs| pairs[edge]),
            )?,
            None => None,
        };
        let Some(pair) = native_pair else { continue };
        if !merge_derived_endpoint_pair(&mut ordered_endpoint_pairs, edge, pair) {
            return Err(StandardTopologyFailure::ConflictingNativeEndpoints.into());
        }
        if let Some(options) = &mut endpoint_options {
            if options[edge]
                .iter()
                .any(|candidate| missing_edge::same_unordered_pair(*candidate, pair))
            {
                options[edge] = ctx
                    .alloc_filled(1, pair, "catia_native_support_singleton_pair")
                    .map_err(StandardTopologyError::Resource)?;
            }
        }
    }
    if let (Some(options), Some(pairs)) = (&mut endpoint_options, &native_endpoint_evidence) {
        for (options, pair) in options.iter_mut().zip(pairs) {
            if let Some(pair) = pair {
                *options = ctx
                    .alloc_filled(1, *pair, "catia_native_evidence_singleton_pair")
                    .map_err(StandardTopologyError::Resource)?;
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
                    ctx,
                    &native_port_options,
                    pairs,
                    &ordered_endpoint_pairs,
                )
                .map_err(StandardTopologyError::Resource)?
            else {
                return Err(StandardTopologyFailure::NativeEndpointPropagation.into());
            };
            Some(propagated)
        }
        None => None,
    };
    if let (Some(options), Some(pairs)) = (&mut endpoint_options, &graph_propagated_endpoint_pairs)
    {
        for (options, pair) in options.iter_mut().zip(pairs) {
            if let Some(pair) = pair {
                *options = ctx
                    .alloc_filled(1, *pair, "catia_propagated_singleton_pair")
                    .map_err(StandardTopologyError::Resource)?;
            }
        }
    }
    if let Some(pairs) = &graph_propagated_endpoint_pairs {
        include_native_endpoint_pairs(ctx, &mut endpoint_candidates, pairs)
            .map_err(StandardTopologyError::Resource)?;
    }
    if let Some(options) = &mut endpoint_options {
        let handle_face_candidates = missing_edge::standard_repeated_edge_face_handle_candidates(
            ctx,
            spine,
            &serialized_edge_faces,
        )
        .map_err(StandardTopologyError::Resource)?;
        let mut allowed_faces = Vec::new();
        ctx.reserve_vec(
            &mut allowed_faces,
            supports.len(),
            "catia_repeated_allowed_face_rows",
        )
        .map_err(StandardTopologyError::Resource)?;
        for (edge, support) in supports.iter().enumerate() {
            let mut faces = Vec::new();
            if support.faces[0] == support.faces[1] {
                for face in (0..face_count).filter(|face| *face != support.faces[0]) {
                    let res = {
                        let Some(surface) = face_surface(ir, bindings, &surface_indices, face)
                        else {
                            continue;
                        };
                        let mut any_pair = false;
                        for pair in &options[edge] {
                            let mut all_points = true;
                            for point in pair {
                                let Some(point) = ir.model.points.get(*point) else {
                                    all_points = false;
                                    break;
                                };
                                if !point_on_standard_face(
                                    point.position().get(),
                                    &surface.geometry,
                                    face_bounds.as_ref().and_then(|bounds| bounds[face]),
                                )
                                .map_err(CodecError::from)
                                .map_err(StandardTopologyError::Resource)?
                                {
                                    all_points = false;
                                    break;
                                }
                            }
                            if all_points
                                && standard_nurbs_line_pair_on_face(
                                    &surface.geometry,
                                    support,
                                    pair,
                                    &ir.model.points,
                                    face_bounds.as_ref().and_then(|bounds| bounds[face]),
                                )
                                .map_err(CodecError::from)
                                .map_err(StandardTopologyError::Resource)?
                            {
                                any_pair = true;
                                break;
                            }
                        }
                        any_pair
                    };
                    if res {
                        ctx.push_vec(&mut faces, face, "catia_repeated_allowed_faces")
                            .map_err(StandardTopologyError::Resource)?;
                    }
                }
            }
            allowed_faces.push(faces);
        }
        let mut face_geometries = Vec::new();
        ctx.reserve_vec(
            &mut face_geometries,
            face_count,
            "catia_repeated_face_geometry_refs",
        )
        .map_err(StandardTopologyError::Resource)?;
        let mut all_face_geometries = true;
        for face in 0..face_count {
            let Some(surface) = face_surface(ir, bindings, &surface_indices, face) else {
                all_face_geometries = false;
                break;
            };
            face_geometries.push(&surface.geometry);
        }
        let face_geometries = all_face_geometries.then_some(face_geometries);
        let mut edge_geometries = Vec::new();
        ctx.reserve_vec(
            &mut edge_geometries,
            supports.len(),
            "catia_repeated_edge_geometry_refs",
        )
        .map_err(StandardTopologyError::Resource)?;
        edge_geometries.extend(supports.iter().map(|support| &support.geometry));
        if let Some(handle_face_candidates) = handle_face_candidates {
            missing_edge::refine_repeated_edge_face_candidates(
                ctx,
                &edge_faces,
                &mut allowed_faces,
                &handle_face_candidates,
            )
            .map_err(StandardTopologyError::Resource)?
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
        let endpoint_pairs = if has_alternates {
            let mut pairs = Vec::new();
            ctx.reserve_vec(&mut pairs, options.len(), "catia_repeated_endpoint_pairs")
                .map_err(StandardTopologyError::Resource)?;
            let mut complete = true;
            for choices in options.iter() {
                let Ok([pair]) = <[[usize; 2]; 1]>::try_from(choices.as_slice()) else {
                    complete = false;
                    break;
                };
                pairs.push(pair);
            }
            complete.then_some(pairs)
        } else {
            None
        };
        let endpoint_closures = match endpoint_pairs {
            Some(pairs) => missing_edge::repeated_face_endpoint_closures(
                ctx,
                &edge_faces,
                &allowed_faces,
                &pairs,
                face_count,
            )
            .map_err(StandardTopologyError::Resource)?,
            None => None,
        };
        let endpoint_completed = match endpoint_closures.as_deref() {
            Some([closure]) => Some(
                ctx.copy_retained_slice(closure, "catia_repeated_endpoint_completed")
                    .map_err(StandardTopologyError::Resource)?,
            ),
            _ => None,
        };
        if endpoint_closures
            .as_ref()
            .is_some_and(|closures| closures.len() > 1)
        {
            endpoint_face_assignments = endpoint_closures;
        }
        // A non-empty domain remains open when endpoint degree closure does
        // not select one complete incidence assignment. Face-local endpoint
        // evidence cannot choose among multiple globally closed assignments.
        let completed = if endpoint_completed.is_some() || has_alternates {
            endpoint_completed
        } else {
            missing_edge::resolve_standard_duplicate_edge_faces(
                ctx,
                spine,
                &edge_faces,
                &allowed_faces,
            )
            .map_err(StandardTopologyError::Resource)?
        };
        if let Some(completed) = completed {
            edge_faces = completed;
            for (edge, (support, faces)) in supports.iter_mut().zip(&edge_faces).enumerate() {
                if support.faces == *faces {
                    continue;
                }
                support.faces = *faces;
                let Some(surface) = face_surface(ir, bindings, &surface_indices, faces[1]) else {
                    return Err(StandardTopologyFailure::MissingFaceSurface.into());
                };
                let mut pair_index = 0;
                while pair_index < options[edge].len() {
                    let mut all_points = true;
                    for point in options[edge][pair_index] {
                        let Some(point) = ir.model.points.get(point) else {
                            all_points = false;
                            break;
                        };
                        if !point_on_standard_face(
                            point.position().get(),
                            &surface.geometry,
                            face_bounds.as_ref().and_then(|bounds| bounds[faces[1]]),
                        )
                        .map_err(CodecError::from)
                        .map_err(StandardTopologyError::Resource)?
                        {
                            all_points = false;
                            break;
                        }
                    }
                    if all_points {
                        pair_index += 1;
                    } else {
                        options[edge].remove(pair_index);
                    }
                }
                if options[edge].is_empty() {
                    return Err(StandardTopologyFailure::EmptyEndpointDomain.into());
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
    let endpoint_pair_on_incident_faces =
        |edge: usize, pair: [usize; 2]| -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
            for point in pair {
                let Some(position) = ir
                    .model
                    .points
                    .get(point)
                    .map(|point| point.position().get())
                else {
                    return Ok(false);
                };
                for face in supports[edge].faces {
                    let Some(surface) = face_surface(ir, bindings, &surface_indices, face) else {
                        return Ok(false);
                    };
                    let bounds = face_bounds.as_ref().and_then(|bounds| bounds[face]);
                    if !point_on_standard_face(position, &surface.geometry, bounds)?
                        || !standard_nurbs_line_pair_on_face(
                            &surface.geometry,
                            &supports[edge],
                            &pair,
                            &ir.model.points,
                            bounds,
                        )?
                    {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
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
            let unfiltered = ctx
                .copy_slice(pairs, "catia_standard_unfiltered_endpoint_pairs")
                .map_err(StandardTopologyError::Resource)?;
            let mut limit_error = None;
            pairs.retain(|pair| {
                if limit_error.is_some() {
                    return true;
                }
                let Some(start) = ir
                    .model
                    .points
                    .get(pair[0])
                    .map(|point| point.position().get())
                else {
                    return false;
                };
                let Some(end) = ir
                    .model
                    .points
                    .get(pair[1])
                    .map(|point| point.position().get())
                else {
                    return false;
                };
                for &face in &support.faces {
                    let Some(surface) = face_surface(ir, bindings, &surface_indices, face) else {
                        return false;
                    };
                    match standard_endpoint_pair_supports_topology(
                        ctx,
                        &surface.geometry,
                        support,
                        start,
                        end,
                        crate::families::standard::records::standard_face_witness(
                            brep,
                            bindings[face].2,
                        ),
                        refusal,
                    ) {
                        Ok(true) => {}
                        Ok(false) => return false,
                        Err(error) => {
                            limit_error = Some(error);
                            return true;
                        }
                    }
                }
                true
            });
            if let Some(error) = limit_error {
                return Err(StandardTopologyError::Resource(error));
            }
            if pairs.is_empty() {
                *pairs = unfiltered;
            }
        }
    }
    if let Some(options) = &mut endpoint_options {
        loop {
            let seeds = ctx
                .collect_vec(
                    options.iter().map(|pairs| {
                        <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                            .ok()
                            .map(|[pair]| pair)
                    }),
                    "catia_standard_placement_seeds",
                )
                .map_err(StandardTopologyError::Resource)?;
            let mut changed = false;
            if let Some(placement_domains) = missing_edge::standard_mesh_placement_endpoint_pairs(
                ctx,
                spine,
                &edge_faces,
                &seeds,
            )
            .map_err(StandardTopologyError::Resource)?
            {
                for (edge, mut domain) in placement_domains.into_iter().enumerate() {
                    if deferred_port_edges[edge] {
                        continue;
                    }
                    let mut refusal = None;
                    domain.retain(|pair| {
                        if refusal.is_some() {
                            return true;
                        }
                        match endpoint_pair_on_incident_faces(edge, *pair) {
                            Ok(on_faces) => on_faces,
                            Err(limit) => {
                                refusal = Some(limit);
                                false
                            }
                        }
                    });
                    if let Some(limit) = refusal {
                        return Err(StandardTopologyError::Resource(CodecError::from(limit)));
                    }
                    if domain.is_empty() {
                        continue;
                    }
                    let previous = ctx
                        .copy_slice(&options[edge], "catia_standard_placement_previous")
                        .map_err(StandardTopologyError::Resource)?;
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
            let boundary_domains = if options.iter().all(|domain| !domain.is_empty()) {
                missing_edge::standard_mesh_prune_endpoint_candidates(
                    ctx,
                    spine,
                    &edge_faces,
                    options,
                )
                .map_err(StandardTopologyError::Resource)?
            } else {
                None
            };
            if let Some(boundary_domains) = boundary_domains {
                for (edge, mut domain) in boundary_domains.into_iter().enumerate() {
                    if deferred_port_edges[edge] {
                        continue;
                    }
                    let mut refusal = None;
                    domain.retain(|pair| {
                        if refusal.is_some() {
                            return true;
                        }
                        match endpoint_pair_on_incident_faces(edge, *pair) {
                            Ok(on_faces) => on_faces,
                            Err(limit) => {
                                refusal = Some(limit);
                                false
                            }
                        }
                    });
                    if let Some(limit) = refusal {
                        return Err(StandardTopologyError::Resource(CodecError::from(limit)));
                    }
                    let previous = ctx
                        .copy_slice(&options[edge], "catia_standard_boundary_previous")
                        .map_err(StandardTopologyError::Resource)?;
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
            let mut refusal = None;
            pairs.retain(|pair| {
                if refusal.is_some() {
                    return true;
                }
                match endpoint_pair_on_incident_faces(edge, *pair) {
                    Ok(on_faces) => on_faces,
                    Err(limit) => {
                        refusal = Some(limit);
                        false
                    }
                }
            });
            if let Some(limit) = refusal {
                return Err(StandardTopologyError::Resource(CodecError::from(limit)));
            }
            pairs.sort_unstable();
            pairs.dedup();
        }
        for (candidates, options) in endpoint_candidates.iter_mut().zip(&mut *options) {
            for point in options.iter().flatten() {
                if !candidates.contains(point) {
                    ctx.push_vec(
                        candidates,
                        *point,
                        "catia_standard_endpoint_candidate_point",
                    )
                    .map_err(StandardTopologyError::Resource)?;
                }
            }
        }
    }
    let graph_propagated_pairs = graph_propagated_endpoint_pairs
        .as_ref()
        .map(|pairs| {
            ctx.collect_options(
                pairs.iter().copied(),
                "catia_standard_graph_propagated_pairs",
            )
        })
        .transpose()
        .map_err(StandardTopologyError::Resource)?
        .flatten();
    let native_endpoint_pairs = if let Some(pairs) = graph_propagated_pairs {
        Some(pairs)
    } else {
        (|| -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
            const MAX_NATIVE_PORT_CHOICES: usize = 65_536;
            const MAX_NATIVE_PORT_WORK: usize = 20_000_000;

            let Some(options) = endpoint_options.as_ref() else {
                return Ok(None);
            };
            let Some(ports) = native_ports.as_ref() else {
                return Ok(None);
            };
            let seeds = ctx.collect_vec(
                options.iter().map(|choices| {
                    <[[usize; 2]; 1]>::try_from(choices.as_slice())
                        .ok()
                        .map(|[pair]| pair)
                }),
                "catia_standard_native_port_seeds",
            )?;
            let Some(propagated) = missing_edge::propagate_edge_port_points_with_ordered_seeds(
                ctx,
                ports,
                &seeds,
                &ordered_endpoint_pairs,
            )?
            else {
                return Ok(None);
            };
            if let Some(complete) = ctx.collect_options(
                propagated.iter().copied(),
                "catia_standard_native_complete_pairs",
            )? {
                return Ok(Some(complete));
            }
            // Exhaustive binding is a fallback after exact identity propagation.
            // Large symmetric choice sets remain unresolved and continue through
            // trim-mesh and incidence paths instead of making decode unbounded.
            let choice_count = options.iter().map(Vec::len).sum::<usize>();
            if choice_count <= MAX_NATIVE_PORT_CHOICES
                && options
                    .len()
                    .checked_mul(choice_count)
                    .is_some_and(|work| work <= MAX_NATIVE_PORT_WORK)
            {
                missing_edge::bind_edge_port_candidates(ctx, ports, options)
            } else {
                Ok(None)
            }
        })()
        .map_err(StandardTopologyError::Resource)?
    };
    let propagated_endpoint_pairs = if let Some((options, ports)) = endpoint_options.as_ref().zip(
        missing_edge::edge_port_identities(ctx, spine).map_err(StandardTopologyError::Resource)?,
    ) {
        let pairs = ctx
            .collect_vec(
                options.iter().map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|pair| pair[0])
                }),
                "catia_standard_propagated_seeds",
            )
            .map_err(StandardTopologyError::Resource)?;
        missing_edge::propagate_edge_port_points_with_ordered_seeds_and_deferred(
            ctx,
            &ports,
            &pairs,
            &ordered_endpoint_pairs,
            &deferred_port_edges,
        )
        .map_err(StandardTopologyError::Resource)?
        .map(|propagated| {
            ctx.collect_vec(
                propagated
                    .into_iter()
                    .zip(options)
                    .map(|(pair, candidates)| {
                        pair.filter(|pair| {
                            candidates.iter().any(|candidate| {
                                *candidate == *pair || *candidate == [pair[1], pair[0]]
                            })
                        })
                    }),
                "catia_standard_validated_propagated_pairs",
            )
        })
        .transpose()
        .map_err(StandardTopologyError::Resource)?
    } else {
        None
    };
    let mesh_propagated_endpoint_pairs = if let Some((options, ports)) =
        endpoint_options.as_ref().zip(
            missing_edge::standard_mesh_edge_ports(ctx, spine)
                .map_err(StandardTopologyError::Resource)?,
        ) {
        let pairs = ctx
            .collect_vec(
                options.iter().map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|pair| pair[0])
                }),
                "catia_standard_mesh_propagated_seeds",
            )
            .map_err(StandardTopologyError::Resource)?;
        missing_edge::propagate_edge_port_points_with_ordered_seeds_and_deferred(
            ctx,
            &ports,
            &pairs,
            &ordered_endpoint_pairs,
            &deferred_port_edges,
        )
        .map_err(StandardTopologyError::Resource)?
    } else {
        None
    };
    let propagated_endpoint_pairs = combine_propagated_endpoint_pairs(
        ctx,
        propagated_endpoint_pairs,
        mesh_propagated_endpoint_pairs,
    )
    .map_err(StandardTopologyError::Resource)?;
    let mut constrained_endpoint_options = if let Some(options) = endpoint_options.as_ref() {
        let mut copied = Vec::new();
        for (edge, pairs) in options.iter().enumerate() {
            let row = if let Some(pair) = propagated_endpoint_pairs
                .as_ref()
                .and_then(|propagated| propagated.get(edge).copied().flatten())
            {
                ctx.alloc_filled(1, pair, "catia_endpoint_propagated_pair")
                    .map_err(StandardTopologyError::Resource)?
            } else {
                ctx.copy_slice(pairs, "catia_endpoint_pair_copy")
                    .map_err(StandardTopologyError::Resource)?
            };
            ctx.push_vec(&mut copied, row, "catia_endpoint_option_copy")
                .map_err(StandardTopologyError::Resource)?;
        }
        Some(copied)
    } else {
        None
    };
    if let (Some(options), Some(ports)) = (
        constrained_endpoint_options.as_mut(),
        missing_edge::standard_mesh_edge_ports(ctx, spine)
            .map_err(StandardTopologyError::Resource)?,
    ) {
        let pruned = if deferred_port_edges.iter().any(|deferred| *deferred) {
            fbb::prune_edge_candidates_by_port_domains_with_deferred(
                ctx,
                &ports,
                options,
                &deferred_port_edges,
            )
        } else {
            fbb::prune_edge_candidates_by_port_domains(ctx, &ports, options)
        }
        .map_err(StandardTopologyError::Resource)?;
        if let Some(pruned) = pruned {
            *options = pruned;
        }
        let unique_pairs = if deferred_port_edges.iter().any(|deferred| *deferred) {
            missing_edge::unique_mesh_edge_port_candidate_pairs_with_deferred(
                ctx,
                &ports,
                options,
                &deferred_port_edges,
            )
            .map_err(StandardTopologyError::Resource)?
        } else {
            missing_edge::unique_mesh_edge_port_candidate_pairs(ctx, &ports, options)
                .map_err(StandardTopologyError::Resource)?
                .map(|pairs| {
                    ctx.collect_vec(
                        pairs.into_iter().map(Some),
                        "catia_standard_unique_port_pair_options",
                    )
                })
                .transpose()
                .map_err(StandardTopologyError::Resource)?
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
        .map(|pairs| ctx.collect_options(pairs, "catia_standard_resolved_endpoint_pairs"))
        .transpose()
        .map_err(StandardTopologyError::Resource)?
        .flatten();
    if let Some(pairs) = &resolved_endpoint_pairs {
        let pairs = ctx
            .collect_vec(
                pairs.iter().copied().map(Some),
                "catia_standard_included_native_pairs",
            )
            .map_err(StandardTopologyError::Resource)?;
        include_native_endpoint_pairs(ctx, &mut endpoint_candidates, &pairs)
            .map_err(StandardTopologyError::Resource)?;
    }
    let fbb_mesh_ports = if edge_table_form == EdgeTableForm::FbbOnly {
        missing_edge::standard_mesh_edge_ports(ctx, spine)
            .map_err(StandardTopologyError::Resource)?
    } else {
        None
    };
    let mesh_topology = if edge_table_form == EdgeTableForm::FbbOnly {
        let native = if let Some(ports) = fbb_mesh_ports.as_deref() {
            topology::parse_fbb_with_native_vertices(ctx, spine, ports)
                .map_err(StandardTopologyError::Resource)?
        } else {
            None
        };
        if native.is_some() {
            native
        } else {
            topology::parse_fbb(ctx, spine).map_err(StandardTopologyError::Resource)?
        }
    } else {
        let standard = fbb::parse_standard(ctx, spine).map_err(StandardTopologyError::Resource)?;
        if standard.is_some() {
            standard
        } else if let Some(ports) = native_ports.as_ref() {
            topology::parse_fbb_with_native_vertices(ctx, spine, ports)
                .map_err(StandardTopologyError::Resource)?
        } else {
            None
        }
    };
    let mesh_bound = (|| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(topology) = (!has_open_face_domains).then_some(mesh_topology).flatten() else {
            return Ok(None);
        };
        let candidate_pairs = match resolved_endpoint_pairs.as_ref() {
            Some(pairs) => Some(ctx.copy_slice(pairs, "catia_standard_mesh_resolved_pair_copy")?),
            None => ctx.collect_options(
                endpoint_candidates
                    .iter()
                    .map(|candidates| <[usize; 2]>::try_from(candidates.as_slice()).ok()),
                "catia_standard_mesh_candidate_pairs",
            )?,
        };
        let endpoint_pairs = if let Some(pairs) = candidate_pairs {
            Some(pairs)
        } else {
            let Some(vertices) = topology.edge_vertices(ctx)? else {
                return Ok(None);
            };
            let ports = ctx.collect_options(
                vertices.into_iter().map(|[left, right]| {
                    Some([u32::try_from(left).ok()?, u32::try_from(right).ok()?])
                }),
                "catia_standard_mesh_vertex_ports",
            )?;
            let Some(ports) = ports else {
                return Ok(None);
            };
            let Some(options) = constrained_endpoint_options.as_ref() else {
                return Ok(None);
            };
            missing_edge::bind_edge_port_candidates(ctx, &ports, options)?
        };
        let Some(endpoint_pairs) = endpoint_pairs else {
            return Ok(None);
        };
        let Some(point_assignment) = topology.bind_vertex_points(ctx, &endpoint_pairs)? else {
            return Ok(None);
        };
        Ok(Some((topology, point_assignment)))
    })();
    let mesh_bound = match mesh_bound {
        Ok(bound) => bound,
        Err(error) => return Err(StandardTopologyError::Resource(error)),
    };
    let circle_anchors = ctx
        .collect_vec(
            supports
                .iter()
                .zip(&endpoint_candidates)
                .map(|(support, candidates)| match &support.geometry {
                    crate::families::standard::records::StandardCurveGeometry::Circle {
                        ..
                    } => <[usize; 2]>::try_from(candidates.as_slice()).ok(),
                    crate::families::standard::records::StandardCurveGeometry::Line
                    | crate::families::standard::records::StandardCurveGeometry::Bspline => None,
                }),
            "catia_standard_circle_anchors",
        )
        .map_err(StandardTopologyError::Resource)?;
    let mut mesh_search_exhausted = false;
    let native_fbb_topology = if edge_table_form == EdgeTableForm::FbbOnly && !has_open_face_domains
    {
        if let Some(pairs) = native_endpoint_pairs.as_ref() {
            fbb::parse_fbb_endpoints_with_edge_classes(
                ctx,
                spine,
                &edge_faces,
                pairs,
                Some(&edge_classes),
            )
            .map_err(StandardTopologyError::Resource)?
        } else {
            None
        }
    } else {
        None
    };
    let mut selected_face_assignment = None;
    let (mut topology, point_assignment) = if let Some(bound) = mesh_bound {
        bound
    } else if let Some(topology) = native_fbb_topology {
        let point_assignment = ctx
            .collect_vec(
                0..ir.model.points.len(),
                "catia_standard_native_point_assignment",
            )
            .map_err(StandardTopologyError::Resource)?;
        (topology, point_assignment)
    } else if let Some(topology) = (|| -> Result<Option<_>, cadmpeg_core::CodecError> {
        if has_open_face_domains {
            return Ok(None);
        }
        if let Some(pairs) = native_endpoint_pairs.as_ref() {
            fbb::parse_standard_endpoints_with_edge_classes(
                ctx,
                spine,
                &edge_faces,
                pairs,
                Some(&edge_classes),
            )
        } else {
            Ok(None)
        }
    })()
    .map_err(StandardTopologyError::Resource)?
    {
        let point_assignment = ctx
            .collect_vec(
                0..ir.model.points.len(),
                "catia_standard_fbb_point_assignment",
            )
            .map_err(StandardTopologyError::Resource)?;
        (topology, point_assignment)
    } else if let Some(bound) = (|| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(options) = constrained_endpoint_options.as_ref() else {
            return Ok(None);
        };
        let edge_identity_evidence = ctx.collect_vec(
            supports.iter().enumerate().map(|(edge, _)| {
                standard_edge_identity_is_admitted(
                    ordered_endpoint_pairs[edge],
                    native_endpoint_evidence
                        .as_ref()
                        .and_then(|pairs| pairs.get(edge).copied().flatten()),
                    native_supports_by_row[edge].is_some(),
                    !limit_curve_bindings[edge].is_empty(),
                )
            }),
            "catia_standard_edge_identity_evidence",
        )?;
        let edge_direction_evidence = match native_endpoint_evidence.as_ref() {
            Some(pairs) => ctx.collect_vec(
                pairs.iter().map(Option::is_some),
                "catia_standard_edge_direction_evidence",
            )?,
            None => ctx.alloc_filled(
                supports.len(),
                false,
                "catia_standard_missing_direction_evidence",
            )?,
        };
        let point_on_face = |face: usize, point: usize| {
            face_point_membership
                .get(face)
                .and_then(|points| points.get(point))
                .copied()
                .unwrap_or(false)
        };
        let mut point_positions = Vec::new();
        for point in &ir.model.points {
            ctx.push_vec(
                &mut point_positions,
                point.position().get(),
                "catia_solver_point_positions",
            )?;
        }
        let mut solver_deferred_edges =
            ctx.copy_slice(&deferred_port_edges, "catia_solver_deferred_edges")?;
        if let Some(ports) = missing_edge::edge_port_identities(ctx, spine)? {
            if !missing_edge::expand_deferred_edge_port_components(
                ctx,
                &ports,
                &mut solver_deferred_edges,
            )? {
                return Ok(None);
            }
        }
        let solve_mesh_candidate =
            |selected_edge_faces: &[[usize; 2]],
             selected_supports: &[crate::families::standard::records::StandardCurveSupport],
             selected_edge_classes: &[usize],
             solve_budget: &WorkBudget<'_>|
             -> Result<mesh_quotient::MeshCandidateSolve, cadmpeg_core::CodecError> {
                // FBB-only rows are complete boundary runs. Their global
                // handle quotient is the incidence source.
                let mut solver_options = standard_endpoint_options_for_selected_faces(
                    ctx,
                    ir,
                    bindings,
                    &surface_indices,
                    selected_supports,
                    &point_positions,
                    (options, &edge_identity_evidence),
                )?;
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
                    ctx,
                    &ir.model.points,
                    selected_supports,
                    &solver_options,
                )?;
                let circle_constraint =
                    StandardCirclePairConstraint::new(ctx, selected_supports, &solver_options)?;
                let face_domain_edges = match open_face_domains.as_ref() {
                    Some(domains) => ctx.collect_vec(
                        domains.iter().map(|domain| !domain.is_empty()),
                        "catia_standard_face_domain_edges",
                    )?,
                    None => ctx.alloc_filled(
                        solver_options.len(),
                        false,
                        "catia_standard_missing_face_domain_edges",
                    )?,
                };
                let selected_circle_constraint_edges = ctx.collect_vec(
                    selected_supports.iter().enumerate().map(|(edge, support)| {
                        matches!(
                            support.geometry,
                            crate::families::standard::records::StandardCurveGeometry::Circle { .. }
                        ) && solver_options[edge].len() > 1
                    }),
                    "catia_standard_circle_constraint_edges",
                )?;
                let partial_constraint_edges = ctx.collect_vec(
                    selected_circle_constraint_edges
                        .iter()
                        .zip(line_constraint.flexible_edge_mask())
                        .zip(&face_domain_edges)
                        .map(|((circle, line), face)| *circle || line || *face),
                    "catia_standard_partial_constraint_edges",
                )?;
                let preferred_budget =
                    solve_budget.child_slice(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS);
                let preferred = mesh_quotient::parse_standard_mesh_candidate_outcome(
                    ctx,
                    crate::solve::mesh_quotient::ParseStandardMeshCandidateOutcomeInputs {
                        bytes: spine,
                        edge_faces: selected_edge_faces,
                        edge_candidates: &solver_options,
                        edge_classes: selected_edge_classes,
                        edge_geometry: &edge_geometry,
                        edge_identity_evidence: &edge_identity_evidence,
                        edge_direction_evidence: &edge_direction_evidence,
                        global_handle_ports: has_open_face_domains,
                        partial_constraint_edges: &partial_constraint_edges,
                        preferred_assignment_edges: &partial_constraint_edges,
                        priority_edges: Some(&partial_constraint_edges),
                        assignment_dependencies: None,
                        budget: &preferred_budget,
                        partial_solution_valid: |pairs| {
                            endpoint_pairs_on_selected_faces(pairs)
                                && line_constraint
                                    .edge_pairs(pairs)
                                    .is_some_and(|pairs| line_constraint.is_valid(&pairs))
                        },
                        complete_solution_valid: |pairs| {
                            endpoint_pairs_on_selected_faces(pairs)
                                && line_constraint
                                    .edge_pairs(pairs)
                                    .is_some_and(|pairs| line_constraint.is_simple(&pairs))
                                && standard_circle_pair_solution_is_simple(
                                    &circle_constraint,
                                    ir,
                                    bindings,
                                    &surface_indices,
                                    selected_supports,
                                    &solver_options,
                                    pairs,
                                )
                        },
                    },
                )?;
                if !solve_budget.charge_by(preferred_budget.consumed()) {
                    return Ok(mesh_quotient::MeshSolve::Failed(
                        mesh_quotient::MeshCandidateFailure::Exhausted(
                            mesh_quotient::MeshCandidateExhaustion::FaceDomainEnumeration,
                        ),
                    ));
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
                        ctx,
                        crate::solve::mesh_quotient::ParseStandardMeshCandidateOutcomeInputs {
                            bytes: spine,
                            edge_faces: selected_edge_faces,
                            edge_candidates: &solver_options,
                            edge_classes: selected_edge_classes,
                            edge_geometry: &edge_geometry,
                            edge_identity_evidence: &edge_identity_evidence,
                            edge_direction_evidence: &edge_direction_evidence,
                            global_handle_ports: has_open_face_domains,
                            partial_constraint_edges: &partial_constraint_edges,
                            preferred_assignment_edges: &partial_constraint_edges,
                            priority_edges: Some(&partial_constraint_edges),
                            assignment_dependencies: None,
                            budget: &fallback_budget,
                            partial_solution_valid: |pairs| {
                                endpoint_pairs_on_selected_faces(pairs)
                                    && line_constraint
                                        .edge_pairs(pairs)
                                        .is_some_and(|pairs| line_constraint.is_simple(&pairs))
                            },
                            complete_solution_valid: |pairs| {
                                endpoint_pairs_on_selected_faces(pairs)
                                    && line_constraint
                                        .edge_pairs(pairs)
                                        .is_some_and(|pairs| line_constraint.is_simple(&pairs))
                            },
                        },
                    )?;
                    if !solve_budget.charge_by(fallback_budget.consumed()) {
                        return Ok(mesh_quotient::MeshSolve::Failed(
                            mesh_quotient::MeshCandidateFailure::Exhausted(
                                mesh_quotient::MeshCandidateExhaustion::FaceDomainEnumeration,
                            ),
                        ));
                    }
                    Ok(retry_rejected_mesh_solution(preferred, || fallback))
                } else {
                    Ok(preferred)
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
                ctx,
                face_assignments,
                work_budget,
                |selected_edge_faces, branch_budget| {
                    let mut selected_supports = Vec::new();
                    ctx.reserve_vec(
                        &mut selected_supports,
                        supports.len(),
                        "catia_selected_curve_supports",
                    )?;
                    selected_supports.extend(supports.iter().zip(selected_edge_faces).map(
                        |(support, faces)| {
                            let mut selected = support.clone();
                            selected.faces = *faces;
                            selected
                        },
                    ));
                    let selected_edge_classes =
                        standard_curve_edge_classes(ctx, &selected_supports)?;
                    solve_mesh_candidate(
                        selected_edge_faces,
                        &selected_supports,
                        &selected_edge_classes,
                        branch_budget,
                    )
                },
            )? {
                mesh_quotient::MeshSolve::Solved((faces, topology, assignment)) => {
                    selected_face_assignment = Some(faces);
                    mesh_quotient::MeshSolve::Solved((topology, assignment))
                }
                mesh_quotient::MeshSolve::Failed(failure) => {
                    mesh_quotient::MeshSolve::Failed(failure)
                }
            }
        } else {
            solve_mesh_candidate(&edge_faces, &supports, &edge_classes, work_budget)?
        };
        Ok(match outcome {
            mesh_quotient::MeshSolve::Solved(candidate) => Some(candidate),
            mesh_quotient::MeshSolve::Failed(failure) => {
                mesh_search_exhausted |=
                    matches!(failure, mesh_quotient::MeshCandidateFailure::Exhausted(_));
                diagnostics.mesh_failure = Some(failure);
                None
            }
        })
    })()
    .map_err(StandardTopologyError::Resource)?
    {
        bound
    } else if let Some(topology) = (|| -> Result<Option<_>, cadmpeg_core::CodecError> {
        if has_open_face_domains {
            return Ok(None);
        }
        let Some(options) = constrained_endpoint_options.as_ref() else {
            return Ok(None);
        };
        if let Some(ports) = missing_edge::standard_mesh_edge_ports(ctx, spine)? {
            let candidate = fbb::parse_standard_port_endpoint_candidates(
                ctx,
                spine,
                &edge_faces,
                options,
                &ports,
                work_budget,
            )?;
            if candidate.is_some() {
                return Ok(candidate);
            }
        }
        fbb::parse_standard_endpoint_candidates(ctx, spine, &edge_faces, options, work_budget)
    })()
    .map_err(StandardTopologyError::Resource)?
    {
        let point_assignment = ctx
            .collect_vec(
                0..ir.model.points.len(),
                "catia_standard_endpoint_point_assignment",
            )
            .map_err(StandardTopologyError::Resource)?;
        (topology, point_assignment)
    } else if let Some(topology) = (if has_open_face_domains {
        Ok(None)
    } else {
        fbb::parse_standard_motif(ctx, spine, &edge_faces, &circle_anchors)
    })
    .map_err(StandardTopologyError::Resource)?
    {
        let point_assignment = ctx
            .collect_vec(
                0..ir.model.points.len(),
                "catia_standard_motif_point_assignment",
            )
            .map_err(StandardTopologyError::Resource)?;
        (topology, point_assignment)
    } else {
        return Err((if mesh_search_exhausted || work_budget.exhausted() {
            StandardTopologyFailure::TopologySearchExhausted
        } else if matches!(
            diagnostics.mesh_failure,
            Some(mesh_quotient::MeshCandidateFailure::Ambiguous(_))
        ) {
            StandardTopologyFailure::AmbiguousTopologySolution
        } else {
            StandardTopologyFailure::NoTopologySolution
        })
        .into());
    };
    if let Some(faces) = selected_face_assignment {
        edge_faces = faces;
        for (support, faces) in supports.iter_mut().zip(&edge_faces) {
            support.faces = *faces;
        }
    }
    let Some(edge_vertices) = validate_standard_topology(
        ctx,
        ir,
        annotations,
        &mut topology,
        &point_assignment,
        StandardTopologyValidation {
            supports: &supports,
            endpoint_candidates: &endpoint_candidates,
        },
        admission,
    )
    .map_err(StandardTopologyError::Resource)?
    else {
        return Err(StandardTopologyFailure::InvalidTopologySolution.into());
    };
    let mut resolved_limit_curve_bindings = Vec::new();
    ctx.reserve_vec(
        &mut resolved_limit_curve_bindings,
        edge_vertices.len(),
        "catia_resolved_limit_curve_bindings",
    )
    .map_err(StandardTopologyError::Resource)?;
    for (edge, logical_vertices) in edge_vertices.iter().enumerate() {
        let points = [
            point_assignment[logical_vertices[0]],
            point_assignment[logical_vertices[1]],
        ];
        resolved_limit_curve_bindings.push(resolve_standard_limit_curve_binding(
            &limit_curve_bindings[edge],
            points,
        ));
    }
    *bound_limit_curve_count = resolved_limit_curve_bindings
        .iter()
        .filter(|binding| binding.is_some())
        .count();
    emit_standard_topology(
        ctx,
        crate::families::standard::decode::EmitStandardTopologyInputs {
            ir,
            annotations,
            bindings,
            brep,
            surface_indices: &surface_indices,
            supports: &supports,
            edge_vertices: &edge_vertices,
            point_assignment: &point_assignment,
            topology: &topology,
            native_edge_supports: &native_supports_by_row,
            limit_curve_bindings: &resolved_limit_curve_bindings,
            limit_curves,
            refusal,
            admission,
        },
    )
    .map_err(|error| match error {
        cadmpeg_core::CodecError::ResourceLimit(_) => StandardTopologyError::Resource(error),
        _ => StandardTopologyError::Semantic(StandardTopologyFailure::InadmissibleNeutralModel),
    })?;
    Ok(())
}

#[derive(Clone, Copy)]
struct StandardTopologyValidation<'a> {
    supports: &'a [crate::families::standard::records::StandardCurveSupport],
    endpoint_candidates: &'a [Vec<usize>],
}

/// Validates the solved topology against the decoded model, applies body kinds
/// and face partitioning, and returns the per-edge logical vertex pairs.
fn validate_standard_topology(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &mut crate::families::standard::topology::StandardTopology,
    point_assignment: &[usize],
    validation: StandardTopologyValidation<'_>,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    let StandardTopologyValidation {
        supports,
        endpoint_candidates,
    } = validation;
    let face_count = ir.model.faces.len();
    if topology.face_count() != face_count
        || topology.edge_rows().len() != supports.len()
        || topology.vertex_points().len() != ir.model.points.len()
        || !topology
            .vertex_points()
            .iter()
            .zip(&ir.model.points)
            .all(|(stored, point)| {
                stored[0] == point.position().get().x
                    && stored[1] == point.position().get().y
                    && stored[2] == point.position().get().z
            })
    {
        return Ok(None);
    }
    let face_groups = [topology.face_count()];
    if topology
        .orient_solid_body_cycles(ctx, &face_groups)?
        .is_none()
    {
        return Ok(None);
    }
    let Some(body_kinds) = topology.body_kinds(ctx, &face_groups)? else {
        return Ok(None);
    };
    let Some(edge_vertices) = topology.edge_vertices(ctx)? else {
        return Ok(None);
    };
    if edge_vertices.iter().enumerate().any(|(edge, vertices)| {
        let start = point_assignment[vertices[0]];
        let end = point_assignment[vertices[1]];
        !endpoint_candidates[edge].is_empty()
            && (!endpoint_candidates[edge].contains(&start)
                || !endpoint_candidates[edge].contains(&end))
    }) {
        return Ok(None);
    }
    let mut body_arena_indices = Vec::new();
    for body_index in 0..body_kinds.len() {
        let id = standard_id(
            ctx,
            "body",
            format_args!("{body_index}"),
            BodyId::mint,
            "catia_standard_body_lookup_identity",
        )?;
        let Some(arena_index) = ir.model.bodies.iter().position(|body| body.id == id) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut body_arena_indices,
            arena_index,
            "catia_standard_body_arena_indices",
        )?;
    }
    for (&arena_index, &kind) in body_arena_indices.iter().zip(&body_kinds) {
        ir.model.bodies[arena_index].kind = kind;
    }
    if !partition_standard_face_components(
        ctx,
        ir,
        annotations,
        &topology.face_components(ctx)?,
        admission,
    )? {
        return Ok(None);
    }
    Ok(Some(edge_vertices))
}

/// The face's loop ids and their classification, built once from the boundary
/// rows the solved topology states for this face.
fn standard_id<T>(
    ctx: &DecodeContext<'_>,
    kind: &'static str,
    key: std::fmt::Arguments<'_>,
    mint: impl FnOnce(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    operation: &'static str,
) -> Result<T, CodecError> {
    let (key, _reservation) = ctx.format_scoped(key, operation)?;
    let id = ctx.format_retained(format_args!("catia:standard:{kind}#{key}"), operation)?;
    mint(id).map_err(CodecError::malformed)
}

fn standard_face_loops(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    topology: &crate::families::standard::topology::StandardTopology,
    face_index: usize,
    point_assignment: &[usize],
) -> Result<cadmpeg_ir::topology::FaceLoops, CodecError> {
    let Some(face_topology) = topology.faces().get(face_index) else {
        return Ok(cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()));
    };
    let mut ids = Vec::new();
    ctx.reserve_vec(
        &mut ids,
        face_topology.boundaries.len(),
        "catia_standard_face_loop_ids",
    )?;
    for loop_index in 0..face_topology.boundaries.len() {
        ids.push(standard_id(
            ctx,
            "loop",
            format_args!("{face_index}:{loop_index}"),
            LoopId::mint,
            "catia_standard_face_loop_identity",
        )?);
    }
    let unspecified = || -> Result<_, CodecError> {
        let mut copy = Vec::new();
        for id in &ids {
            let id = id.try_clone_for_decode(ctx, "catia_standard_unspecified_loop_id_copy")?;
            ctx.push_vec(&mut copy, id, "catia_standard_unspecified_loop_ids")?;
        }
        Ok(cadmpeg_ir::topology::FaceLoops::unspecified(copy))
    };
    if let [single] = ids.as_slice() {
        return Ok(cadmpeg_ir::topology::FaceLoops::classified(
            single.try_clone_for_decode(ctx, "catia_standard_single_loop_id_copy")?,
            Vec::new(),
        ));
    }
    let Some(surface_id) = bindings.get(face_index).map(|binding| &binding.0) else {
        return unspecified();
    };
    let Some(&surface_index) = surface_indices.get(surface_id) else {
        return unspecified();
    };
    let Some(surface) = ir.model.surfaces.get(surface_index) else {
        return unspecified();
    };
    let mut rows = Vec::new();
    for (boundary, id) in face_topology.boundaries.iter().zip(&ids) {
        let mut points = Vec::new();
        for coedge in &boundary.coedges {
            let Some(point) = point_assignment
                .get(coedge.start_vertex)
                .and_then(|index| ir.model.points.get(*index))
            else {
                return unspecified();
            };
            ctx.push_vec(
                &mut points,
                point.position().get(),
                "catia_standard_planar_loop_points",
            )?;
        }
        let id = id.try_clone_for_decode(ctx, "catia_standard_planar_loop_id_copy")?;
        ctx.push_vec(&mut rows, (id, points), "catia_standard_planar_loop_rows")?;
    }
    crate::boundary_roles::classify_planar_boundaries(ctx, &surface.geometry, &rows)
}

/// Emits the edge, loop, coedge, and pcurve IR layers for the solved topology.
struct EmitStandardTopologyInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input6,
    'input7,
    'input8,
    'input9,
    'input10,
    'input11,
    'input12,
    'input13,
    'input14,
    'input15,
    'input16,
> {
    ir: &'input0 mut CadIr,
    annotations: &'input1 mut AnnotationBuilder,
    bindings: &'input2 [(SurfaceId, bool, usize)],
    brep: &'input3 [u8],
    surface_indices: &'input4 HashMap<SurfaceId, usize>,
    supports: &'input5 [crate::families::standard::records::StandardCurveSupport],
    edge_vertices: &'input6 [[usize; 2]],
    point_assignment: &'input7 [usize],
    topology: &'input8 crate::families::standard::topology::StandardTopology,
    native_edge_supports: &'input9 [Option<&'input10 StandardEdgeSupport>],
    limit_curve_bindings: &'input11 [Option<StandardLimitCurveBinding>],
    limit_curves: &'input12 [NurbsCurve],
    refusal: &'input13 mut crate::nurbs::LaneRefusals,
    admission: &'input16 mut FamilyEntityAdmission<'input14, 'input15>,
}

fn emit_standard_topology(
    ctx: &DecodeContext<'_>,
    inputs: EmitStandardTopologyInputs<
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
    >,
) -> Result<(), cadmpeg_core::CodecError> {
    let EmitStandardTopologyInputs {
        ir,
        annotations,
        bindings,
        brep,
        surface_indices,
        supports,
        edge_vertices,
        point_assignment,
        topology,
        native_edge_supports,
        limit_curve_bindings,
        limit_curves,
        refusal,
        admission,
    } = inputs;

    let mut edge_reversed = Vec::new();
    ctx.reserve_vec(
        &mut edge_reversed,
        supports.len(),
        "catia_standard_edge_reverse_flags",
    )?;
    for (edge_index, (support, logical_vertices)) in supports.iter().zip(edge_vertices).enumerate()
    {
        let start_point = point_assignment[logical_vertices[0]];
        let end_point = point_assignment[logical_vertices[1]];
        let native_support = match native_edge_supports
            .get(edge_index)
            .and_then(Option::as_ref)
        {
            Some(native)
                if standard_native_support_endpoint_pair(
                    native,
                    &ir.model.points,
                    &[start_point, end_point],
                    Some([start_point, end_point]),
                )?
                .is_some() =>
            {
                Some(*native)
            }
            _ => None,
        };
        let (curve, param_range) = build_standard_edge_curve(
            ctx,
            crate::families::standard::decode::edge_geometry::BuildStandardEdgeCurveInputs {
                ir,
                annotations,
                bindings,
                surface_indices,
                brep,
                support,
                points: [start_point, end_point],
                native_support,
                limit_curve: limit_curve_bindings[edge_index]
                    .map(|binding| (&limit_curves[binding.curve], binding.parameter_range)),
                refusal,
                admission,
            },
        )?;
        let reversed = param_range.is_some_and(|range| range[0] > range[1]);
        edge_reversed.push(reversed);
        let param_range = param_range.map(ordered_range);
        let [start_point, end_point] = if reversed {
            [end_point, start_point]
        } else {
            [start_point, end_point]
        };
        let id = standard_id(
            ctx,
            "edge",
            format_args!("{edge_index}"),
            EdgeId::mint,
            "catia_standard_edge_identity",
        )?;
        annotate(
            ctx,
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            u64_from_index(support.pos),
            "standard_spine_edge_row",
            Exactness::ByteExact,
        )?;
        if curve.is_some() {
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &id,
                "curve",
                "catia_annotation_field",
            )?;
        }
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "start",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "end",
            "catia_annotation_field",
        )?;
        if param_range.is_some() {
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &id,
                "param_range",
                "catia_annotation_field",
            )?;
        }
        admission.reserve_entity(&mut ir.model.edges, "catia_standard_model_edges")?;
        ir.model.edges.push(Edge {
            id,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(curve, param_range)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            start: standard_id(
                ctx,
                "v",
                format_args!("{start_point}"),
                VertexId::mint,
                "catia_standard_edge_start_identity",
            )?,
            end: standard_id(
                ctx,
                "v",
                format_args!("{end_point}"),
                VertexId::mint,
                "catia_standard_edge_end_identity",
            )?,
            tolerance: None,
        });
    }

    let mut curve_indices = HashMap::new();
    for (index, curve) in ir.model.curves.iter().enumerate() {
        let id = curve
            .id
            .try_clone_for_decode(ctx, "catia_standard_curve_index_id_copy")?;
        ctx.insert_hash_map(
            &mut curve_indices,
            id,
            index,
            "catia_standard_curve_indices",
        )?;
    }
    let mut edge_coedges = ctx.alloc_filled(
        ir.model.edges.len(),
        Vec::new(),
        "catia_standard_edge_coedge_rows",
    )?;
    for (face_index, face_topology) in topology.faces().iter().enumerate() {
        let face_loops = standard_face_loops(
            admission.context(),
            ir,
            bindings,
            surface_indices,
            topology,
            face_index,
            point_assignment,
        )?;
        for (loop_index, boundary) in face_topology.boundaries.iter().enumerate() {
            let loop_id = standard_id(
                ctx,
                "loop",
                format_args!("{face_index}:{loop_index}"),
                LoopId::mint,
                "catia_standard_loop_identity",
            )?;
            let mut vertices = Vec::new();
            for edge_use in &boundary.coedges {
                let point = point_assignment[edge_use.end_vertex];
                let vertex = standard_id(
                    ctx,
                    "v",
                    format_args!("{point}"),
                    VertexId::mint,
                    "catia_standard_ring_vertex_identity",
                )?;
                ctx.push_vec(&mut vertices, vertex, "catia_standard_ring_vertices")?;
            }
            let vertices: cadmpeg_ir::features::NonEmptyMembers<VertexId> =
                vertices.try_into().map_err(CodecError::malformed)?;
            let mut coedges = Vec::new();
            let mut vertex_uses = Vec::new();
            for (ordinal, vertex) in vertices.into_iter().enumerate() {
                let coedge = standard_id(
                    ctx,
                    "coedge",
                    format_args!("{face_index}:{loop_index}:{ordinal}"),
                    CoedgeId::mint,
                    "catia_standard_ring_members",
                )?;
                let after = coedge.try_clone_for_decode(ctx, "catia_standard_ring_members")?;
                ctx.push_vec(
                    &mut vertex_uses,
                    AnchoredVertexUse {
                        vertex,
                        after,
                        pcurves: Vec::new(),
                    },
                    "catia_standard_ring_members",
                )?;
                ctx.push_vec(&mut coedges, coedge, "catia_standard_ring_members")?;
            }
            let ring =
                cadmpeg_ir::topology::LoopRing::try_new_for_decode(ctx, coedges, vertex_uses)?
                    .map_err(CodecError::malformed)?;
            let coedge_ids = ring.coedges();
            for (coedge_index, edge_use) in boundary.coedges.iter().enumerate() {
                let support = &supports[edge_use.edge_row];
                let logical_vertices = edge_vertices[edge_use.edge_row];
                let start = ir.model.points[point_assignment[logical_vertices[0]]]
                    .position()
                    .get();
                let end = ir.model.points[point_assignment[logical_vertices[1]]]
                    .position()
                    .get();
                let edge_curve = ir.model.edges[edge_use.edge_row]
                    .curve()
                    .and_then(|id| curve_indices.get(id))
                    .map(|index| &ir.model.curves[*index].geometry);
                let pcurve_id = standard_pcurve_geometry(ctx,
&ir.model.surfaces[surface_indices[&bindings[face_index].0]].geometry,
support,
(start, end),
crate::families::standard::records::standard_face_witness(
                        brep,
                        bindings[face_index].2,
                    ),
edge_curve,
refusal)?
                .map(|(geometry, range)| -> Result<_, cadmpeg_core::CodecError> {
                    let id = standard_id(ctx, "pcurve",
                        format_args!("{face_index}:{loop_index}:{coedge_index}"),
                        PcurveId::mint, "catia_standard_pcurve_identity")?;
                    annotate(
                        ctx,
                        annotations,
                        &id,
                        "MainDataStream+SurfacicReps",
                        u64_from_index(support.pos),
                        "derived_surface_parameter_curve",
                        Exactness::Derived)?;
                    crate::resource::derived_annotation(ctx, annotations, &id, "geometry", "catia_annotation_field")?;
                    admission.reserve_entity(&mut ir.model.pcurves, "catia_standard_model_pcurves")?;
                    ir.model.pcurves.push(Pcurve {
                        id: id.try_clone_for_decode(ctx, "catia_standard_pcurve_id_copy")?,
                        geometry,
                        metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                            None,
                            Some(
                                cadmpeg_ir::units::FiniteVector::new(range)
                                    .ok_or(cadmpeg_ir::geometry::pcurve::PcurveMetadata::NON_FINITE_PARAMETER_RANGE)
                                    .map_err(cadmpeg_core::CodecError::malformed)?,
                            ),
                            None,
                        ),
                    });
                    Ok((id, range))
                })
                .transpose()?;
                let arena_index = ir.model.coedges.len();
                ctx.push_vec(
                    &mut edge_coedges[edge_use.edge_row],
                    arena_index,
                    "catia_standard_edge_coedge_entries",
                )?;
                let id = coedge_ids[coedge_index]
                    .try_clone_for_decode(ctx, "catia_standard_coedge_id_copy")?;
                annotate(
                    ctx,
                    annotations,
                    &id,
                    "MainDataStream+SurfacicReps",
                    0,
                    "trim_mesh_boundary_run",
                    Exactness::ByteExact,
                )?;
                for field in ["owner_loop", "edge", "radial_next", "sense"] {
                    crate::resource::derived_annotation(
                        ctx,
                        annotations,
                        &id,
                        field,
                        "catia_annotation_field",
                    )?;
                }
                if pcurve_id.is_some() {
                    crate::resource::derived_annotation(
                        ctx,
                        annotations,
                        &id,
                        "pcurves",
                        "catia_annotation_field",
                    )?;
                }
                let pcurve_use = pcurve_id
                    .map(|(pcurve, range)| {
                        edge_use
                            .reversed
                            .then_some([range[1], range[0]])
                            .map(cadmpeg_ir::geometry::DirectedParameterRange::new)
                            .transpose()
                            .map(|parameter_range| cadmpeg_ir::topology::PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range,
                            })
                    })
                    .transpose()
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                let pcurves = match pcurve_use {
                    Some(pcurve) => {
                        let mut pcurves = Vec::new();
                        ctx.push_vec(&mut pcurves, pcurve, "catia_standard_coedge_pcurve_use")?;
                        pcurves
                    }
                    None => Vec::new(),
                };
                admission.reserve_entity(&mut ir.model.coedges, "catia_standard_model_coedges")?;
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: loop_id
                        .try_clone_for_decode(ctx, "catia_standard_coedge_owner_loop_copy")?,
                    edge: standard_id(
                        ctx,
                        "edge",
                        format_args!("{}", edge_use.edge_row),
                        EdgeId::mint,
                        "catia_standard_coedge_edge_identity",
                    )?,
                    radial_next: coedge_ids[coedge_index]
                        .try_clone_for_decode(ctx, "catia_standard_radial_id_copy")?,
                    sense: if edge_use.reversed ^ edge_reversed[edge_use.edge_row] {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves,
                    use_curve: None,
                });
            }
            annotate(
                ctx,
                annotations,
                &loop_id,
                "MainDataStream+SurfacicReps",
                0,
                "trim_mesh_boundary_cycle",
                Exactness::ByteExact,
            )?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &loop_id,
                "face",
                "catia_annotation_field",
            )?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &loop_id,
                "coedges",
                "catia_annotation_field",
            )?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &loop_id,
                "vertex_uses",
                "catia_annotation_field",
            )?;
            if face_loops.role(&loop_id) != LoopBoundaryRole::Unspecified {
                crate::resource::derived_annotation(
                    ctx,
                    annotations,
                    &loop_id,
                    "boundary_role",
                    "catia_annotation_field",
                )?;
            }
            admission.reserve_entity(&mut ir.model.loops, "catia_standard_model_loops")?;
            ir.model.loops.push(Loop {
                id: loop_id,
                face: standard_id(
                    ctx,
                    "face",
                    format_args!("{face_index}"),
                    FaceId::mint,
                    "catia_standard_loop_face_identity",
                )?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
        }
        ir.model.faces[face_index].loops = face_loops;
    }
    for uses in edge_coedges {
        for (position, current) in uses.iter().enumerate() {
            let next = uses[(position + 1) % uses.len()];
            let next_id = ir.model.coedges[next]
                .id
                .try_clone_for_decode(ctx, "catia_standard_radial_next_id_copy")?;
            ir.model.coedges[*current].radial_next = next_id;
        }
    }
    Ok(())
}

fn lifted_standard_support_parameters<const N: usize>(
    surface: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
    parameters: [f64; N],
) -> Result<[Option<Point3>; N], cadmpeg_core::decode::ResourceLimit> {
    let mut points = [None; N];
    for (index, parameter) in parameters.into_iter().enumerate() {
        let Some(uv) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::pcurve_uv(pcurve, parameter))?
        else {
            continue;
        };
        points[index] = match cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v) {
            Ok(point) => Some(point.get()),
            Err(failure) => failure.non_finite()?,
        };
    }
    Ok(points)
}

fn standard_native_support_endpoint_pair(
    support: &StandardEdgeSupport,
    points: &[Point],
    candidates: &[usize],
    required_pair: Option<[usize; 2]>,
) -> Result<Option<[usize; 2]>, cadmpeg_core::decode::ResourceLimit> {
    const VERTEX_MATCH_TOLERANCE: f64 = 2e-3;

    let mut lifted = [[None; 2]; 2];
    for (index, (carrier, pcurve)) in support.carriers.iter().zip(&support.pcurves).enumerate() {
        let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) = carrier
        else {
            return Ok(None);
        };
        lifted[index] =
            lifted_standard_support_parameters(surface, pcurve, support.parameter_range)?;
    }
    let [[Some(first_start), Some(first_end)], [Some(second_start), Some(second_end)]] = lifted
    else {
        return Ok(None);
    };
    let first = [first_start, first_end];
    let second = [second_start, second_end];
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
        return Ok(None);
    }
    let point_for = |expected: Point3| {
        let mut matches = candidates.iter().copied().filter(|point| {
            points.get(*point).is_some_and(|point| {
                point.position().get().distance_squared(expected).sqrt() <= VERTEX_MATCH_TOLERANCE
            })
        });
        let point = matches.next()?;
        matches.next().is_none().then_some(point)
    };
    let pair = [point_for(first[0]), point_for(first[1])];
    Ok(match pair {
        [Some(start), Some(end)] if start != end => {
            let pair = [start, end];
            required_pair
                .is_none_or(|required| missing_edge::same_unordered_pair(pair, required))
                .then_some(pair)
        }
        _ => None,
    })
}

fn resolve_standard_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    candidates: &[Vec<usize>],
) -> Result<Option<Vec<Vec<[usize; 2]>>>, CodecError> {
    const MAX_PAIR_RELATIONS_PER_EDGE: usize = 65_536;

    let mut resolved = Vec::new();
    ctx.reserve_vec(
        &mut resolved,
        candidates.len(),
        "catia_standard_resolved_endpoint_rows",
    )?;
    for points in candidates {
        let row = if let Ok(pair) = <[usize; 2]>::try_from(points.as_slice()) {
            ctx.alloc_filled(1, pair, "catia_standard_initial_endpoint_pair")?
        } else {
            Vec::new()
        };
        resolved.push(row);
    }
    for (edge, support) in supports.iter().enumerate() {
        let crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } =
            support.geometry
        else {
            continue;
        };
        let center = center.get();
        let radius = radius.get();
        let count = candidates[edge].len();
        if count < 2 {
            continue;
        }
        let include_full_circle_seams = count == 2
            && if let [start, end] = candidates[edge].as_slice() {
                if let (Some(start), Some(end)) = (
                    ir.model
                        .points
                        .get(*start)
                        .map(|point| point.position().get()),
                    ir.model
                        .points
                        .get(*end)
                        .map(|point| point.position().get()),
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
            .checked_mul(count - 1)
            .and_then(|value| value.checked_div(2))
            .and_then(|value| value.checked_add(if include_full_circle_seams { count } else { 0 }));
        if let Some(relation_count) =
            relation_count.filter(|relations| *relations <= MAX_PAIR_RELATIONS_PER_EDGE)
        {
            let mut pairs = Vec::new();
            ctx.reserve_vec(
                &mut pairs,
                relation_count,
                "catia_standard_circle_endpoint_pairs",
            )?;
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
            ctx.admit_hash_map_entry(&mut line_groups, &faces, "catia_standard_line_groups")?;
            ctx.push_vec(
                line_groups.entry(faces).or_default(),
                edge,
                "catia_standard_line_group_edges",
            )?;
        }
    }
    for (faces, edges) in line_groups {
        let Some(surface0) = face_surface(ir, bindings, surface_indices, faces[0]) else {
            return Ok(None);
        };
        let Some(surface1) = face_surface(ir, bindings, surface_indices, faces[1]) else {
            return Ok(None);
        };
        let direction = intersection_line_direction(&surface0.geometry, &surface1.geometry);
        let same_cone_surface = matches!(
            (&surface0.geometry, &surface1.geometry),
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_))
            )
        ) && surface0.geometry == surface1.geometry;
        let Some(points) = edges.first().and_then(|edge| candidates.get(*edge)) else {
            return Ok(None);
        };
        let relation_count = points
            .len()
            .checked_sub(1)
            .and_then(|other| points.len().checked_mul(other))
            .and_then(|value| value.checked_div(2));
        if relation_count.is_none_or(|count| count > MAX_PAIR_RELATIONS_PER_EDGE) {
            continue;
        }
        let mut pairs = Vec::new();
        for (left, &start) in points.iter().enumerate() {
            for &end_index in &points[left + 1..] {
                let Some(start_point) = ir
                    .model
                    .points
                    .get(start)
                    .map(|point| point.position().get())
                else {
                    return Ok(None);
                };
                let Some(end_point) = ir
                    .model
                    .points
                    .get(end_index)
                    .map(|point| point.position().get())
                else {
                    return Ok(None);
                };
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
                    && point_on_surface(midpoint, &surface0.geometry)?
                    && point_on_surface(midpoint, &surface1.geometry)?
                {
                    ctx.push_vec(
                        &mut pairs,
                        [points[left], end_index],
                        "catia_standard_line_endpoint_pairs",
                    )?;
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
            resolved[edges[0]] =
                ctx.alloc_filled(1, pairs[0], "catia_standard_line_singleton_pair")?;
        } else {
            for edge in edges {
                resolved[edge] =
                    ctx.copy_retained_slice(&pairs, "catia_standard_line_pair_copy")?;
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
            .checked_sub(1)
            .and_then(|other| points.len().checked_mul(other))
            .and_then(|value| value.checked_div(2));
        let Some(relation_count) = relation_count.filter(|count| {
            *count <= MAX_PAIR_RELATIONS_PER_EDGE && *count <= fallback_relation_budget
        }) else {
            continue;
        };
        fallback_relation_budget -= relation_count;
        let mut fallback = Vec::new();
        ctx.reserve_vec(
            &mut fallback,
            relation_count,
            "catia_standard_fallback_endpoint_pairs",
        )?;
        for (left, &start) in points.iter().enumerate() {
            fallback.extend(points[left + 1..].iter().map(|&end| [start, end]));
        }
        *pairs = fallback;
    }
    Ok(Some(resolved))
}

fn standard_curve_edge_classes(
    ctx: &DecodeContext<'_>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
) -> Result<Vec<usize>, CodecError> {
    let mut classes = Vec::new();
    ctx.reserve_vec(&mut classes, supports.len(), "catia_standard_edge_classes")?;
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
                                && left_radius.get().to_bits() == right_radius.get().to_bits()
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
    Ok(classes)
}

fn standard_curve_geometry_gauge_keys(
    ctx: &DecodeContext<'_>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
) -> Result<Vec<MeshEdgeGeometry>, CodecError> {
    let mut keys = Vec::new();
    ctx.reserve_vec(
        &mut keys,
        supports.len(),
        "catia_standard_geometry_gauge_keys",
    )?;
    keys.extend(supports.iter().map(|support| match &support.geometry {
        crate::families::standard::records::StandardCurveGeometry::Line => MeshEdgeGeometry::Line,
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } => {
            MeshEdgeGeometry::Circle {
                center: [center.x.to_bits(), center.y.to_bits(), center.z.to_bits()],
                radius: radius.get().to_bits(),
            }
        }
        crate::families::standard::records::StandardCurveGeometry::Bspline => {
            MeshEdgeGeometry::Bspline
        }
    }));
    Ok(keys)
}

fn standard_circle_endpoint_candidates(
    ctx: &DecodeContext<'_>,
    points: &[Point],
    center: Point3,
    radius: f64,
    faces: Option<
        [(
            &SurfaceGeometry,
            Option<crate::families::standard::records::StandardFaceBounds>,
        ); 2],
    >,
) -> Result<Vec<usize>, CodecError> {
    let mut candidates = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let on_circle =
            (point.position().get().distance_squared(center).sqrt() - radius).abs() <= 1e-3;
        let mut incident = true;
        if let Some(faces) = faces {
            for (surface, bounds) in faces {
                if !point_on_standard_face(point.position().get(), surface, bounds)? {
                    incident = false;
                    break;
                }
            }
        }
        if on_circle && incident {
            ctx.push_vec(&mut candidates, index, "catia_circle_endpoint_candidates")?;
        }
    }
    Ok(candidates)
}

/// Resolve standard-row endpoints from equal standard and native edge identities.
fn standard_native_graph_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    graph: Option<&crate::families::b5::graph::B5Graph>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    native_edges: &BTreeMap<u32, [u32; 2]>,
    points: &[Point],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let Some(graph) = graph else { return Ok(None) };
    let identity_points = unique_native_identity_points(
        ctx,
        graph.vertices.logical_vertices(),
        graph.vertices.raw_points().len(),
        &graph.vertex_tolerances,
        points,
    )?;
    let mut pairs = Vec::new();
    ctx.reserve_vec(&mut pairs, supports.len(), "catia_graph_endpoint_pairs")?;
    pairs.extend(supports.iter().map(|support| {
        let [start_identity, end_identity] = native_edges.get(&support.tag)?;
        Some([
            *identity_points.get(start_identity)?,
            *identity_points.get(end_identity)?,
        ])
    }));
    Ok(Some(pairs))
}

/// Bind standard rows to ordered coordinate rows through the file-global
/// object journal: `0x60.tag` selects the `b5 03 5e` object id, whose ordered
/// vertex identities select positions in the standard vertex roster.
fn standard_serialized_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    native_edges: &BTreeMap<u32, [u32; 2]>,
    vertex_roster: &[u32],
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let mut point_by_identity = HashMap::new();
    for (point, identity) in vertex_roster.iter().copied().enumerate() {
        if ctx
            .insert_hash_map(
                &mut point_by_identity,
                identity,
                point,
                "catia_roster_point_identities",
            )?
            .is_some()
        {
            return Ok(None);
        }
    }
    let mut pairs = Vec::new();
    ctx.reserve_vec(&mut pairs, supports.len(), "catia_roster_endpoint_pairs")?;
    pairs.extend(supports.iter().map(|support| {
        let [start, end] = native_edges.get(&support.tag)?;
        Some([*point_by_identity.get(start)?, *point_by_identity.get(end)?])
    }));
    Ok(Some(pairs))
}

fn merge_standard_edge_vertex_references(
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
pub(super) fn standard_native_support_edge_ids(
    ctx: &DecodeContext<'_>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    native_support_ids: &HashSet<u32>,
) -> Result<Vec<Option<u32>>, CodecError> {
    let mut exact_row_counts = HashMap::<u32, usize>::new();
    for support in supports {
        if native_support_ids.contains(&support.tag) {
            ctx.admit_hash_map_entry(
                &mut exact_row_counts,
                &support.tag,
                "catia_native_support_row_counts",
            )?;
            *exact_row_counts.entry(support.tag).or_default() += 1;
        }
    }

    let mut ids = Vec::new();
    ctx.reserve_vec(&mut ids, supports.len(), "catia_native_support_edge_ids")?;
    ids.extend(supports.iter().map(|support| {
        (native_support_ids.contains(&support.tag)
            && exact_row_counts.get(&support.tag) == Some(&1))
        .then_some(support.tag)
    }));
    Ok(ids)
}

/// Return whether a standard edge has an admitted identity binding.
///
/// Native port pairs remain useful for endpoint propagation and candidate
/// pruning, but an allocation-only port pair does not bind its row to decoded
/// coordinates and therefore must not freeze an evidence-preserving gauge.
fn standard_edge_identity_is_admitted(
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

fn include_native_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    candidates: &mut [Vec<usize>],
    pairs: &[Option<[usize; 2]>],
) -> Result<(), CodecError> {
    for (candidates, pair) in candidates.iter_mut().zip(pairs) {
        if let Some(pair) = pair {
            for point in pair {
                if !candidates.contains(point) {
                    ctx.push_vec(candidates, *point, "catia_native_endpoint_domain_points")?;
                }
            }
        }
    }
    Ok(())
}

fn combine_propagated_endpoint_pairs(
    ctx: &DecodeContext<'_>,
    raw: Option<Vec<Option<[usize; 2]>>>,
    mesh: Option<Vec<Option<[usize; 2]>>>,
) -> Result<Option<Vec<Option<[usize; 2]>>>, CodecError> {
    let pairs = match (raw, mesh) {
        (Some(raw), Some(mesh)) if raw.len() != mesh.len() => return Ok(None),
        (_, Some(mesh)) if mesh.iter().all(Option::is_some) => mesh,
        (Some(raw), _) if raw.iter().all(Option::is_some) => raw,
        (Some(raw), Some(mesh)) => {
            let mut merged = Vec::new();
            ctx.reserve_vec(&mut merged, raw.len(), "catia_propagated_pair_merge")?;
            merged.extend(
                raw.into_iter()
                    .zip(mesh)
                    .map(|(raw, mesh)| match (raw, mesh) {
                        (Some(raw), Some(mesh)) if raw == mesh || raw == [mesh[1], mesh[0]] => {
                            Some(raw)
                        }
                        (Some(_), Some(_)) => None,
                        (Some(pair), None) | (None, Some(pair)) => Some(pair),
                        (None, None) => None,
                    }),
            );
            merged
        }
        (Some(pairs), None) | (None, Some(pairs)) => pairs,
        (None, None) => return Ok(None),
    };
    Ok((!pairs.is_empty()).then_some(pairs))
}

fn merge_native_endpoint_evidence(
    ctx: &DecodeContext<'_>,
    graph: Option<&[Option<[usize; 2]>]>,
    roster: Option<&[Option<[usize; 2]>]>,
) -> NativeEndpointEvidenceOutput {
    match (graph, roster) {
        (Some(graph), Some(roster)) => {
            if graph.len() != roster.len() {
                return Ok(Err("native endpoint evidence length mismatch"));
            }
            // The roster is the standard BREP's serialized identity-to-point
            // relation. Graph coordinates are reconstructed from independent
            // object records and only supply identities absent from the roster.
            if roster.iter().all(Option::is_some) {
                return Ok(Ok(Some(ctx.copy_retained_slice(
                    roster,
                    "catia_native_roster_evidence_copy",
                )?)));
            }
            let mut merged = Vec::new();
            ctx.reserve_vec(
                &mut merged,
                graph.len(),
                "catia_native_endpoint_merged_evidence",
            )?;
            for (graph, roster) in graph.iter().zip(roster) {
                let pair = match (graph, roster) {
                    (Some(graph), Some(roster)) if graph != roster => {
                        return Ok(Err("conflicting native endpoint evidence"));
                    }
                    (Some(pair), _) | (_, Some(pair)) => Some(*pair),
                    (None, None) => None,
                };
                merged.push(pair);
            }
            Ok(Ok(Some(merged)))
        }
        (Some(pairs), None) | (None, Some(pairs)) => Ok(Ok(Some(
            ctx.copy_retained_slice(pairs, "catia_native_endpoint_evidence_copy")?,
        ))),
        (None, None) => Ok(Ok(None)),
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
fn merge_derived_endpoint_pair(
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
fn standard_successor_endpoint_points(
    ctx: &DecodeContext<'_>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    vertex_roster: &[u32],
) -> Result<Vec<[Option<usize>; 2]>, CodecError> {
    let mut point_by_identity = HashMap::new();
    for (point, identity) in vertex_roster.iter().copied().enumerate() {
        ctx.insert_hash_map(
            &mut point_by_identity,
            identity,
            point,
            "catia_successor_point_identities",
        )?;
    }
    let mut successors = Vec::new();
    ctx.reserve_vec(
        &mut successors,
        supports.len(),
        "catia_successor_endpoint_points",
    )?;
    successors.extend(supports.iter().map(|support| {
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
    }));
    Ok(successors)
}

fn corroborate_successor_endpoint_points(
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

fn unique_native_identity_points(
    ctx: &DecodeContext<'_>,
    vertices: &[crate::families::b5::graph::B5LogicalVertex],
    raw_point_count: usize,
    tolerances: &BTreeMap<usize, cadmpeg_ir::scalar::PositiveReal>,
    points: &[Point],
) -> Result<HashMap<u32, usize>, CodecError> {
    const MATCH_TOLERANCE: f64 = 2e-3;

    let mut matches = HashMap::new();
    for (rank, vertex) in vertices.iter().enumerate() {
        let row = raw_point_count.checked_add(rank).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_native_vertex_tolerance_row", u64::MAX, u64::MAX)
        })?;
        let tolerance = tolerances
            .get(&row)
            .map_or(MATCH_TOLERANCE, |tolerance| tolerance.get())
            .max(MATCH_TOLERANCE);
        let mut matched = None;
        let mut ambiguous = false;
        for (index, point) in points.iter().enumerate() {
            ctx.charge_work(1, "catia_native_identity_point_match")?;
            if point
                .position()
                .get()
                .distance_squared(vertex.point.get())
                .sqrt()
                <= tolerance
            {
                if matched.is_some() {
                    ambiguous = true;
                    break;
                }
                matched = Some(index);
            }
        }
        if !ambiguous {
            if let Some(point) = matched {
                ctx.insert_hash_map(
                    &mut matches,
                    vertex.object_id,
                    point,
                    "catia_native_identity_points",
                )?;
            }
        }
    }
    Ok(matches)
}

fn intersection_line_direction(left: &SurfaceGeometry, right: &SurfaceGeometry) -> Option<Vector3> {
    const ANGULAR_TOLERANCE: f64 = EPS_STANDARD_DECODE_GEOMETRY;

    match (left, right) {
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
        ) => {
            let left = plane_surface.frame().axis().as_raw();
            let right = plane_surface_2.frame().axis().as_raw();
            let direction = (*left).cross(*right);
            let norm = direction.x.hypot(direction.y).hypot(direction.z);
            (norm.is_finite() && norm != 0.0).then_some(direction)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
        ) => {
            let normal = plane_surface.frame().axis().as_raw();
            let axis = cylinder_surface.frame().axis().as_raw();
            ((*normal).dot(*axis).abs() <= ANGULAR_TOLERANCE).then_some(*axis)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface_2)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
        ) => {
            let axis = cylinder_surface_2.frame().axis().as_raw();
            let normal = plane_surface_2.frame().axis().as_raw();
            ((*normal).dot(*axis).abs() <= ANGULAR_TOLERANCE).then_some(*axis)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface_2)),
        ) => {
            let left_axis = cylinder_surface.frame().axis().as_raw();
            let right_axis = cylinder_surface_2.frame().axis().as_raw();
            ((*left_axis).cross(*right_axis).norm() <= ANGULAR_TOLERANCE).then_some(*left_axis)
        }
        _ => None,
    }
}

/// A line on one right circular or elliptical cone is a generator through its
/// apex. Same-carrier line rows have no surface-intersection direction, so
/// their endpoint relation needs this independent straight-branch predicate.
fn same_cone_generator_pair(
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
    start: Point3,
    end: Point3,
) -> bool {
    if left != right {
        return false;
    }
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) = left else {
        return false;
    };
    let origin = cone_surface.origin().get();
    let axis = cone_surface.frame().axis().as_raw();
    let radius = cone_surface.radius().get();
    let half_angle = cone_surface.half_angle().get();
    let tangent = half_angle.tan();
    if !tangent.is_finite() || tangent == 0.0 {
        return false;
    }
    let apex_offset = -radius / tangent;
    if !apex_offset.is_finite() {
        return false;
    }
    let apex = Point3::new(
        origin.x + apex_offset * axis.x,
        origin.y + apex_offset * axis.y,
        origin.z + apex_offset * axis.z,
    );
    if !apex.is_finite() {
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
fn standard_plane_normals_from_face_frames(
    ctx: &DecodeContext<'_>,
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    face_frame_vectors: &[Option<FiniteVector<3>>],
) -> Result<HashMap<u32, FiniteVector<3>>, CodecError> {
    let mut candidates = HashMap::<u32, Option<FiniteVector<3>>>::new();
    for (face, record) in records.iter().enumerate() {
        let crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) = record
        else {
            continue;
        };
        if prefix.kind != AnalyticSurfaceKind::Plane {
            continue;
        }
        let Some(normal) = face_frame_vectors.get(face).copied().flatten() else {
            continue;
        };
        if let Some(stored) = candidates.get_mut(&prefix.target) {
            if stored.is_some_and(|stored| stored != normal) {
                *stored = None;
            }
        } else {
            ctx.insert_hash_map(
                &mut candidates,
                prefix.target,
                Some(normal),
                "catia_plane_normal_candidates",
            )?;
        }
    }
    let mut normals = HashMap::new();
    for (target, normal) in candidates {
        if let Some(normal) = normal {
            ctx.insert_hash_map(&mut normals, target, normal, "catia_plane_normals")?;
        }
    }
    Ok(normals)
}

fn face_surface<'a>(
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
/// candidate is searched. The cache changes only lookup cost.
fn standard_face_point_membership(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    face_bounds: Option<&[Option<crate::families::standard::records::StandardFaceBounds>]>,
) -> Result<Vec<Vec<bool>>, cadmpeg_core::CodecError> {
    let mut memberships =
        ctx.alloc_filled(bindings.len(), Vec::new(), "catia_face_membership_rows")?;
    for (face, membership) in memberships.iter_mut().enumerate() {
        let Some(surface) = face_surface(ir, bindings, surface_indices, face) else {
            continue;
        };
        let bounds = face_bounds
            .and_then(|bounds| bounds.get(face).copied())
            .flatten();
        *membership =
            ctx.alloc_filled(ir.model.points.len(), false, "catia_face_point_membership")?;
        for (point, candidate) in ir.model.points.iter().enumerate() {
            membership[point] =
                point_on_standard_face(candidate.position().get(), &surface.geometry, bounds)?;
        }
    }
    Ok(memberships)
}

fn point_on_standard_face(
    point: Point3,
    surface: &SurfaceGeometry,
    bounds: Option<crate::families::standard::records::StandardFaceBounds>,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    if bounds.is_some_and(|bounds| !point_inside_standard_face_bounds(point, bounds)) {
        return Ok(false);
    }
    Ok(point_on_surface_if_supported(point, surface)? != Some(false))
}

fn point_inside_standard_face_bounds(
    point: Point3,
    bounds: crate::families::standard::records::StandardFaceBounds,
) -> bool {
    let coordinates = [point.x, point.y, point.z];
    let inside_aabb = coordinates.iter().enumerate().all(|(axis, coordinate)| {
        (*coordinate - bounds.aabb_center[axis].get()).abs()
            <= bounds.aabb_half_extents[axis].get() + STANDARD_FACE_BOUNDS_TOLERANCE
    });
    let distance_squared = coordinates
        .iter()
        .enumerate()
        .map(|(axis, coordinate)| (*coordinate - bounds.sphere_center[axis].get()).powi(2))
        .sum::<f64>();
    inside_aabb
        && distance_squared.sqrt() <= bounds.sphere_radius.get() + STANDARD_FACE_BOUNDS_TOLERANCE
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
fn refine_repeated_face_domains_by_geometry_and_bounds(
    edge_faces: &[[usize; 2]],
    allowed_faces: &mut [Vec<usize>],
    face_bounds: Option<&[Option<crate::families::standard::records::StandardFaceBounds>]>,
    face_geometries: Option<&[&SurfaceGeometry]>,
    edge_geometries: &[&crate::families::standard::records::StandardCurveGeometry],
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
                        matches!(
                            geometries.get(face),
                            Some(SurfaceGeometry::Solved(
                                SolvedSurfaceGeometry::Unknown { .. }
                            ))
                        )
                    })
            })
        {
            continue;
        }
        let score = |face: usize| {
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
                                let left = serialized_bounds.aabb_center[*axis].get()
                                    - serialized_bounds.aabb_half_extents[*axis].get();
                                let right = serialized_bounds.aabb_center[*axis].get()
                                    + serialized_bounds.aabb_half_extents[*axis].get();
                                let candidate_left = candidate.aabb_center[*axis].get()
                                    - candidate.aabb_half_extents[*axis].get();
                                let candidate_right = candidate.aabb_center[*axis].get()
                                    + candidate.aabb_half_extents[*axis].get();
                                right.min(candidate_right) - left.max(candidate_left)
                                    > STANDARD_FACE_BOUNDS_TOLERANCE
                            })
                            .count()
                    });
            (usize::from(distinct_circle_carrier), overlap_dimension)
        };
        let mut best = (0, 0);
        let mut best_count = 0usize;
        for face in alternatives.iter().copied() {
            let candidate = score(face);
            if candidate > best {
                best = candidate;
                best_count = 1;
            } else if candidate == best {
                best_count += 1;
            }
        }
        if best == (0, 0) || best_count != 1 {
            continue;
        }
        alternatives.retain(|face| score(*face) == best);
    }
}

fn standard_nurbs_line_pair_on_face(
    surface: &SurfaceGeometry,
    support: &crate::families::standard::records::StandardCurveSupport,
    pair: &[usize; 2],
    points: &[Point],
    bounds: Option<crate::families::standard::records::StandardFaceBounds>,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    if !matches!(
        surface,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
    ) || !matches!(
        support.geometry,
        crate::families::standard::records::StandardCurveGeometry::Line
    ) {
        return Ok(true);
    }
    let Some(start) = points.get(pair[0]).map(|point| point.position().get()) else {
        return Ok(false);
    };
    let Some(end) = points.get(pair[1]).map(|point| point.position().get()) else {
        return Ok(false);
    };
    for fraction in NURBS_LINE_FACE_SAMPLES {
        let point = Point3::new(
            start.x + fraction * (end.x - start.x),
            start.y + fraction * (end.y - start.y),
            start.z + fraction * (end.z - start.z),
        );
        if !point_on_standard_face(point, surface, bounds)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn nurbs_surface_control_bounds(surface: &NurbsSurface) -> Option<[[f64; 2]; 3]> {
    if surface
        .pole_weights()
        .is_some_and(|weights| weights.into_iter().any(|weight| weight.get() <= 0.0))
    {
        return None;
    }
    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for point in surface.poles() {
        for (axis, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
            bounds[axis][0] = bounds[axis][0].min(coordinate);
            bounds[axis][1] = bounds[axis][1].max(coordinate);
        }
    }
    Some(bounds)
}

fn nurbs_surface_parameter_domain(surface: &NurbsSurface) -> Option<[[f64; 2]; 2]> {
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let domains = [
        [
            *surface.u_knots().get(u_degree)?,
            *surface.u_knots().get(u_count)?,
        ],
        [
            *surface.v_knots().get(v_degree)?,
            *surface.v_knots().get(v_count)?,
        ],
    ];
    domains
        .into_iter()
        .all(|[lower, upper]| lower < upper)
        .then_some(domains)
}

fn nurbs_shared_boundary_scalar_matches(left: f64, right: f64) -> bool {
    (left - right).abs() <= NURBS_SHARED_BOUNDARY_TOLERANCE * left.abs().max(right.abs()).max(1.0)
}

fn nurbs_shared_boundary_curves_match(
    ctx: &DecodeContext<'_>,
    left: &NurbsCurve,
    right: &NurbsCurve,
) -> Result<bool, CodecError> {
    let same_payload = |left: &NurbsCurve, right: &NurbsCurve| {
        left.degree() == right.degree()
            && left.periodic() == right.periodic()
            && left.knots().len() == right.knots().len()
            && left
                .knots()
                .iter()
                .zip(right.knots())
                .all(|(left, right)| nurbs_shared_boundary_scalar_matches(*left, *right))
            && left.pole_count() == right.pole_count()
            && (0..left.pole_count()).all(|index| {
                let Some((left_point, right_point)) = left
                    .pole_rows()
                    .point_at(index)
                    .zip(right.pole_rows().point_at(index))
                else {
                    return false;
                };
                let left_point = left_point.get();
                let right_point = right_point.get();
                [left_point.x, left_point.y, left_point.z]
                    .into_iter()
                    .zip([right_point.x, right_point.y, right_point.z])
                    .all(|(left, right)| nurbs_shared_boundary_scalar_matches(left, right))
                    && match (
                        left.pole_rows().weight_at(index),
                        right.pole_rows().weight_at(index),
                    ) {
                        (None, None) => true,
                        (Some(left), Some(right)) => {
                            nurbs_shared_boundary_scalar_matches(left, right)
                        }
                        _ => false,
                    }
            })
    };
    if same_payload(left, right) {
        return Ok(true);
    }
    let Some(range) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(right)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(false);
    };
    Ok(reverse_nurbs_curve(ctx, right, range)?
        .ok()
        .is_some_and(|reversed| same_payload(left, &reversed)))
}

fn nurbs_surface_boundary_curves(
    surface: &NurbsSurface,
) -> Result<Option<[NurbsCurve; 4]>, cadmpeg_core::decode::ResourceLimit> {
    let Some([[u_lower, u_upper], [v_lower, v_upper]]) = nurbs_surface_parameter_domain(surface)
    else {
        return Ok(None);
    };
    let curve =
        |axis, parameter| cadmpeg_ir::eval::nurbs_surface_isocurve(surface, axis, parameter);
    let Some(u_lower) = curve(
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        u_lower,
    )?
    else {
        return Ok(None);
    };
    let Some(u_upper) = curve(
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        u_upper,
    )?
    else {
        return Ok(None);
    };
    let Some(v_lower) = curve(
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
        v_lower,
    )?
    else {
        return Ok(None);
    };
    let Some(v_upper) = curve(
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
        v_upper,
    )?
    else {
        return Ok(None);
    };
    Ok(Some([u_lower, u_upper, v_lower, v_upper]))
}

fn nurbs_boundary_contains_point(
    curve: &NurbsCurve,
    point: Point3,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    let Some([lower, upper]) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(curve)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(false);
    };
    for seed in [lower, 0.5 * (lower + upper), upper] {
        if cadmpeg_ir::eval::nurbs_curve_parameter_near_point(
            curve,
            point,
            NURBS_SURFACE_MEMBERSHIP_TOLERANCE,
            seed,
        )?
        .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Return endpoint pairs that lie on an exact shared NURBS carrier boundary.
///
/// A shared boundary is a positive relation between two tensor-product
/// carriers. It is not inferred from carrier AABBs or from a sampled surface
/// intersection. `None` means that the relation is unavailable; `Some` may be
/// empty when the relation is present but no supplied pair lies on it.
fn standard_shared_nurbs_boundary_pair_options(
    ctx: &DecodeContext<'_>,
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
    points: &[Point3],
    options: &[[usize; 2]],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(left)),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(right)),
    ) = (left, right)
    else {
        return Ok(None);
    };
    let Some(left_boundaries) = nurbs_surface_boundary_curves(left)? else {
        return Ok(None);
    };
    let Some(right_boundaries) = nurbs_surface_boundary_curves(right)? else {
        return Ok(None);
    };
    let mut shared = [false; 4];
    for (index, left) in left_boundaries.iter().enumerate() {
        for right in &right_boundaries {
            if nurbs_shared_boundary_curves_match(ctx, left, right)? {
                shared[index] = true;
                break;
            }
        }
    }
    if !shared.contains(&true) {
        return Ok(None);
    }
    let mut matched = Vec::new();
    for &pair in options {
        let mut pair_matches = false;
        for (boundary, is_shared) in left_boundaries.iter().zip(shared) {
            if !is_shared {
                continue;
            }
            let mut both = true;
            for point in pair {
                let Some(point) = points.get(point) else {
                    both = false;
                    break;
                };
                if !nurbs_boundary_contains_point(boundary, *point)? {
                    both = false;
                    break;
                }
            }
            if both {
                pair_matches = true;
                break;
            }
        }
        if pair_matches {
            ctx.push_vec(&mut matched, pair, "catia_shared_nurbs_boundary_pairs")?;
        }
    }
    Ok(Some(matched))
}

fn standard_shared_boundary_group_domains(
    ctx: &DecodeContext<'_>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    original: &[Vec<[usize; 2]>],
    filtered: &mut [Vec<[usize; 2]>],
    edge_identity_evidence: &[bool],
    boundary_witnesses: &[bool],
) -> Result<(), CodecError> {
    if supports.len() != original.len()
        || supports.len() != filtered.len()
        || supports.len() != edge_identity_evidence.len()
        || supports.len() != boundary_witnesses.len()
    {
        return Ok(());
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
        if let Some(edges) = groups.get_mut(&faces) {
            ctx.push_vec(edges, edge, "catia_shared_boundary_group_edges")?;
        } else {
            let mut edges = Vec::new();
            ctx.push_vec(&mut edges, edge, "catia_shared_boundary_group_edges")?;
            ctx.insert_hash_map(&mut groups, faces, edges, "catia_shared_boundary_groups")?;
        }
    }
    for edges in groups.into_values() {
        if edges.len() < 2 {
            continue;
        }
        if edges.iter().any(|edge| !boundary_witnesses[*edge]) {
            for edge in edges {
                filtered[edge] =
                    ctx.copy_slice(&original[edge], "catia_shared_boundary_original_domain")?;
            }
            continue;
        }
        let mut filtered_pairs = HashSet::new();
        for mut pair in edges
            .iter()
            .flat_map(|edge| filtered[*edge].iter().copied())
        {
            pair.sort_unstable();
            ctx.insert_hash_set(
                &mut filtered_pairs,
                pair,
                "catia_shared_boundary_filtered_pairs",
            )?;
        }
        if filtered_pairs.len() >= edges.len() {
            continue;
        }
        for edge in edges {
            filtered[edge] =
                ctx.copy_slice(&original[edge], "catia_shared_boundary_original_domain")?;
        }
    }
    Ok(())
}

fn standard_endpoint_options_for_selected_faces(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    points: &[Point3],
    (options, edge_identity_evidence): (&[Vec<[usize; 2]>], &[bool]),
) -> Result<Vec<Vec<[usize; 2]>>, CodecError> {
    let mut filtered_options = Vec::new();
    let mut boundary_witnesses = Vec::new();
    for (edge, support) in supports.iter().enumerate() {
        let Some(pairs) = options.get(edge) else {
            ctx.push_vec(
                &mut filtered_options,
                Vec::new(),
                "catia_selected_face_option_rows",
            )?;
            ctx.push_vec(
                &mut boundary_witnesses,
                false,
                "catia_selected_face_witnesses",
            )?;
            continue;
        };
        let (filtered, witnessed) = if edge_identity_evidence.get(edge).copied().unwrap_or(false)
            || !matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Bspline
            )
            || support.faces[0] == support.faces[1]
        {
            (
                ctx.copy_slice(pairs, "catia_selected_face_original_pairs")?,
                false,
            )
        } else if let Some((left, right)) =
            face_surface(ir, bindings, surface_indices, support.faces[0]).zip(face_surface(
                ir,
                bindings,
                surface_indices,
                support.faces[1],
            ))
        {
            match standard_shared_nurbs_boundary_pair_options(
                ctx,
                &left.geometry,
                &right.geometry,
                points,
                pairs,
            )? {
                Some(filtered) if !filtered.is_empty() => (filtered, true),
                _ => (
                    ctx.copy_slice(pairs, "catia_selected_face_original_pairs")?,
                    false,
                ),
            }
        } else {
            (
                ctx.copy_slice(pairs, "catia_selected_face_original_pairs")?,
                false,
            )
        };
        ctx.push_vec(
            &mut filtered_options,
            filtered,
            "catia_selected_face_option_rows",
        )?;
        ctx.push_vec(
            &mut boundary_witnesses,
            witnessed,
            "catia_selected_face_witnesses",
        )?;
    }
    standard_shared_boundary_group_domains(
        ctx,
        supports,
        options,
        &mut filtered_options,
        edge_identity_evidence,
        &boundary_witnesses,
    )?;
    Ok(filtered_options)
}

fn point_on_nurbs_surface(
    point: Point3,
    surface: &NurbsSurface,
) -> Result<Option<bool>, cadmpeg_core::decode::ResourceLimit> {
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
            return Ok(Some(false));
        }
    }
    let Some(distance) = surface_membership::nurbs_surface_witness_distance(surface, point)? else {
        return Ok(None);
    };
    Ok((distance <= NURBS_SURFACE_MEMBERSHIP_TOLERANCE).then_some(true))
}

fn invariant_face_carrier_bindings(
    ctx: &DecodeContext<'_>,
    face_edges: &[Vec<(usize, Vec<usize>)>],
    owner_count: usize,
    budget: Option<&WorkBudget<'_>>,
) -> Result<Option<Vec<Option<usize>>>, cadmpeg_core::CodecError> {
    let mut normalized = Vec::new();
    for edges in face_edges {
        let mut by_owner = BTreeMap::<usize, HashSet<usize>>::new();
        for (owner, carriers) in edges {
            if *owner >= owner_count || carriers.is_empty() {
                continue;
            }
            if !by_owner.contains_key(owner) {
                ctx.insert_btree_map(
                    &mut by_owner,
                    *owner,
                    HashSet::new(),
                    "catia_a5_owner_domain_rows",
                )?;
            }
            let Some(domain) = by_owner.get_mut(owner) else {
                continue;
            };
            for carrier in carriers.iter().copied() {
                ctx.insert_hash_set(domain, carrier, "catia_a5_owner_domain_carriers")?;
            }
        }
        ctx.push_vec(&mut normalized, by_owner, "catia_a5_normalized_faces")?;
    }
    let mut domains = Vec::new();
    for edges in &normalized {
        let domain = ctx.collect_vec(edges.keys().copied(), "catia_a5_owner_domain_keys")?;
        ctx.push_vec(&mut domains, domain, "catia_a5_owner_domains")?;
    }
    let matching = distinct_domain_matching_with_budget(
        ctx,
        domains.iter().map(Vec::as_slice),
        owner_count,
        budget,
        None,
    )?;
    let Some(matching) = matching else {
        return Ok(None);
    };
    let Some(_) =
        retain_distinct_matching_supports(ctx, &mut domains, owner_count, &matching, budget)?
    else {
        return Ok(None);
    };
    let mut bindings = Vec::new();
    for (owners, labels) in domains.iter().zip(&normalized) {
        let mut carriers = HashSet::new();
        for carrier in owners
            .iter()
            .filter_map(|owner| labels.get(owner))
            .flatten()
            .copied()
        {
            ctx.insert_hash_set(&mut carriers, carrier, "catia_a5_reachable_carriers")?;
        }
        if carriers.len() == 1 {
            ctx.push_vec(
                &mut bindings,
                carriers.into_iter().next(),
                "catia_a5_invariant_bindings",
            )?;
        } else {
            ctx.push_vec(&mut bindings, None, "catia_a5_invariant_bindings")?;
        }
    }
    Ok(Some(bindings))
}

fn owner_matches_a5_carrier(
    tail: &crate::native::owner_numeric_tail::CatiaOwnerNumericTail,
    surface: &NurbsSurface,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    let Some(domain) = nurbs_surface_parameter_domain(surface) else {
        return Ok(false);
    };
    if (0..2).any(|axis| {
        tail.lower()[axis] < domain[axis][0] - NURBS_SURFACE_MEMBERSHIP_TOLERANCE
            || tail.upper()[axis] > domain[axis][1] + NURBS_SURFACE_MEMBERSHIP_TOLERANCE
    }) {
        return Ok(false);
    }
    for u in [tail.lower()[0], tail.upper()[0]] {
        for v in [tail.lower()[1], tail.upper()[1]] {
            let Some(point) = cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::nurbs_surface_point(surface, u, v),
            )?
            else {
                return Ok(false);
            };
            if ![point.x, point.y, point.z]
                .into_iter()
                .enumerate()
                .all(|(axis, value)| {
                    value >= f64::from(tail.bounds()[axis][0]) - NURBS_SURFACE_MEMBERSHIP_TOLERANCE
                        && value
                            <= f64::from(tail.bounds()[axis][1])
                                + NURBS_SURFACE_MEMBERSHIP_TOLERANCE
                })
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn owner_contains_face_bounds(
    reference_encoding: crate::families::b2::records::B2OwnerReferenceEncoding,
    tail: &crate::native::owner_numeric_tail::CatiaOwnerNumericTail,
    bounds: crate::families::standard::records::StandardFaceBounds,
) -> bool {
    if reference_encoding != crate::families::b2::records::B2OwnerReferenceEncoding::AllCompact {
        return false;
    }
    (0..3).all(|axis| {
        let lower = bounds.aabb_center[axis].get() - bounds.aabb_half_extents[axis].get();
        let upper = bounds.aabb_center[axis].get() + bounds.aabb_half_extents[axis].get();
        lower >= f64::from(tail.bounds()[axis][0]) - NURBS_SURFACE_MEMBERSHIP_TOLERANCE
            && upper <= f64::from(tail.bounds()[axis][1]) + NURBS_SURFACE_MEMBERSHIP_TOLERANCE
    })
}

fn standard_face_boundary_witnesses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<Vec<Point3>>, CodecError> {
    let mut point_positions = HashMap::new();
    for point in &ir.model.points {
        ctx.insert_hash_map(
            &mut point_positions,
            point.id.as_str(),
            point.position().get(),
            "catia_a5_witness_point_positions",
        )?;
    }
    let mut vertex_positions = HashMap::new();
    for vertex in &ir.model.vertices {
        if let Some(&position) = point_positions.get(vertex.point.as_str()) {
            ctx.insert_hash_map(
                &mut vertex_positions,
                vertex.id.as_str(),
                position,
                "catia_a5_witness_vertex_positions",
            )?;
        }
    }
    let mut edges = HashMap::new();
    for edge in &ir.model.edges {
        ctx.insert_hash_map(&mut edges, edge.id.as_str(), edge, "catia_a5_witness_edges")?;
    }
    let mut coedges = HashMap::new();
    for coedge in &ir.model.coedges {
        ctx.insert_hash_map(
            &mut coedges,
            coedge.id.as_str(),
            coedge,
            "catia_a5_witness_coedges",
        )?;
    }
    let mut loops = HashMap::new();
    for loop_ in &ir.model.loops {
        ctx.insert_hash_map(
            &mut loops,
            loop_.id.as_str(),
            loop_,
            "catia_a5_witness_loops",
        )?;
    }
    let mut curves = HashMap::new();
    for curve in &ir.model.curves {
        ctx.insert_hash_map(
            &mut curves,
            curve.id.as_str(),
            curve,
            "catia_a5_witness_curves",
        )?;
    }
    let mut face_witnesses = Vec::new();
    for face in &ir.model.faces {
        let mut witnesses = Vec::new();
        for edge in face
            .loops
            .iter()
            .filter_map(|id| loops.get(id.as_str()))
            .flat_map(|loop_| loop_.coedges())
            .filter_map(|id| coedges.get(id.as_str()))
            .filter_map(|coedge| edges.get(coedge.edge.as_str()))
        {
            for id in [&edge.start, &edge.end] {
                if let Some(&position) = vertex_positions.get(id.as_str()) {
                    ctx.push_vec(&mut witnesses, position, "catia_a5_face_witness_points")?;
                }
            }
            let Some((curve, [start, end])) = edge
                .curve()
                .and_then(|id| curves.get(id.as_str()))
                .zip(edge.param_range().map(cadmpeg_ir::units::FiniteVector::get))
            else {
                continue;
            };
            if let Some(point) = cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::decode::curve_point_for_decode(ctx, &curve.geometry, 0.5 * (start + end))?,
            )? {
                ctx.push_vec(&mut witnesses, point.get(), "catia_a5_face_witness_points")?;
            }
        }
        let mut distinct = Vec::new();
        for point in witnesses {
            if distinct
                .iter()
                .all(|stored: &Point3| stored.distance(point) > NURBS_SURFACE_MEMBERSHIP_TOLERANCE)
            {
                ctx.push_vec(&mut distinct, point, "catia_a5_distinct_face_witnesses")?;
            }
        }
        ctx.push_vec(&mut face_witnesses, distinct, "catia_a5_face_witness_rows")?;
    }
    Ok(face_witnesses)
}

#[derive(Clone, Copy)]
struct StandardConsolidatedSource<'a> {
    data: &'a [u8],
    records: &'a [ConsolidatedRecord],
}

fn bind_standard_a5_owner_surfaces(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source: StandardConsolidatedSource<'_>,
    face_bounds: &[Option<crate::families::standard::records::StandardFaceBounds>],
    budget: &WorkBudget<'_>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<usize, cadmpeg_core::CodecError> {
    let StandardConsolidatedSource { data, records } = source;
    let carriers =
        crate::families::a5a8::records::a5_surfaces_from_records(ctx, data, records, refusal)?;
    let owners = ctx.collect_vec(
        crate::families::b2::records::b2_owner_packets_from_records(data, records),
        "catia_a5_owner_packets",
    )?;
    if carriers.is_empty() || owners.is_empty() || ir.model.faces.is_empty() {
        return Ok(0);
    }
    let mut owner_carriers = Vec::new();
    for owner in &owners {
        let mut matched = Vec::new();
        for (carrier, value) in carriers.iter().enumerate() {
            if owner_matches_a5_carrier(&owner.numeric_tail, &value.geometry)? {
                ctx.push_vec(&mut matched, carrier, "catia_a5_owner_carrier_indices")?;
            }
        }
        ctx.push_vec(&mut owner_carriers, matched, "catia_a5_owner_carrier_rows")?;
    }
    let witnesses = standard_face_boundary_witnesses(ctx, ir)?;
    let mut surface_indices = HashMap::new();
    for (index, surface) in ir.model.surfaces.iter().enumerate() {
        ctx.insert_hash_map(
            &mut surface_indices,
            surface.id.as_str(),
            index,
            "catia_a5_surface_indices",
        )?;
    }
    let unknown_faces = ctx.collect_vec(
        ir.model
            .faces
            .iter()
            .enumerate()
            .filter_map(|(face, value)| {
                let ordinal = value
                    .id
                    .as_str()
                    .strip_prefix("catia:standard:face#")?
                    .parse::<usize>()
                    .ok()?;
                let surface = *surface_indices.get(value.surface.as_str())?;
                matches!(
                    ir.model.surfaces[surface].geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                )
                .then_some((face, ordinal, surface))
            }),
        "catia_a5_unknown_face_rows",
    )?;
    let mut face_edges = Vec::new();
    ctx.reserve_vec(
        &mut face_edges,
        unknown_faces.len(),
        "catia_a5_face_edge_rows",
    )?;
    for &(face, ordinal, _) in &unknown_faces {
        let Some(Some(bounds)) = face_bounds.get(ordinal) else {
            face_edges.push(Vec::new());
            continue;
        };
        let containing_owners = ctx.collect_vec(
            owners.iter().enumerate().filter_map(|(owner, value)| {
                (!owner_carriers[owner].is_empty()
                    && owner_contains_face_bounds(
                        value.reference_encoding,
                        &value.numeric_tail,
                        *bounds,
                    ))
                .then_some(owner)
            }),
            "catia_a5_containing_owner_indices",
        )?;
        let mut possible_carriers = HashSet::new();
        for carrier in containing_owners
            .iter()
            .flat_map(|owner| owner_carriers[*owner].iter().copied())
        {
            ctx.insert_hash_set(
                &mut possible_carriers,
                carrier,
                "catia_a5_possible_carriers",
            )?;
        }
        let mut face_carriers = HashSet::new();
        for carrier in possible_carriers {
            let surface = &carriers[carrier].geometry;
            let mut witnessed = false;
            if let Some(points) = witnesses.get(face) {
                if points.len() >= 3 {
                    witnessed = true;
                    for point in points {
                        if point_on_nurbs_surface(*point, surface)? != Some(true) {
                            witnessed = false;
                            break;
                        }
                    }
                }
            }
            if witnessed {
                ctx.insert_hash_set(&mut face_carriers, carrier, "catia_a5_face_carriers")?;
            }
        }
        let mut edges = Vec::new();
        for owner in containing_owners {
            let labels = ctx.collect_vec(
                owner_carriers[owner]
                    .iter()
                    .filter(|carrier| face_carriers.contains(carrier))
                    .copied(),
                "catia_a5_face_carrier_labels",
            )?;
            if !labels.is_empty() {
                ctx.push_vec(&mut edges, (owner, labels), "catia_a5_face_owner_edges")?;
            }
        }
        face_edges.push(edges);
    }
    let Some(bindings) =
        invariant_face_carrier_bindings(ctx, &face_edges, owners.len(), Some(budget))?
    else {
        return Ok(0);
    };
    let mut bound = 0;
    for ((_, _, surface), carrier) in unknown_faces.into_iter().zip(bindings) {
        let Some(carrier) = carrier else {
            continue;
        };
        ir.model.surfaces[surface].geometry =
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                carriers[carrier]
                    .geometry
                    .try_clone_for_decode(ctx, "catia_a5_bound_surface_copy")?,
            ));
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &ir.model.surfaces[surface].id,
            "geometry",
            "catia_annotation_field",
        )?;
        bound += 1;
    }
    Ok(bound)
}

/// Keep a topological endpoint pair when p-curve derivation cannot prove it.
///
/// The endpoint domain is an input to exact trim-cycle and port-identity
/// solving. P-curve construction is a later, optional emission step. A
/// spherical face can carry a non-isoparametric circular section; the generic
/// UV-midpoint p-curve test cannot derive that section from its endpoints, but
/// the serialized circle carrier and face membership still make the pair
/// admissible for topology solving.
fn standard_endpoint_pair_supports_topology(
    ctx: &DecodeContext<'_>,
    surface: &SurfaceGeometry,
    support: &crate::families::standard::records::StandardCurveSupport,
    start: Point3,
    end: Point3,
    witness: Option<FinitePoint3>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<bool, cadmpeg_core::CodecError> {
    let endpoint_is_supported = |point| -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
        Ok(match surface {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => {
                point_on_surface_if_supported(point, surface)? != Some(false)
            }
            _ => point_on_surface(point, surface)?,
        })
    };
    if !endpoint_is_supported(start)? || !endpoint_is_supported(end)? {
        return Ok(false);
    }
    if standard_pcurve_geometry(ctx, surface, support, (start, end), witness, None, refusal)?
        .is_some()
    {
        return Ok(true);
    }
    if matches!(
        surface,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
    ) {
        // A bounded model-space NURBS search may remain unknown inside the
        // control-net bound.  Topology retains that pair; a UV p-curve is
        // optional and is derived only from an admitted parameterization.
        return Ok(true);
    }
    Ok(matches!((surface, &support.geometry), (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)),
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius },
    ) if {
        (start.distance(center.get()) - radius.get()).abs() <= SPHERE_SECTION_ENDPOINT_TOLERANCE
            && (end.distance(center.get()) - radius.get()).abs() <= SPHERE_SECTION_ENDPOINT_TOLERANCE
    }))
}

type CircleFaceKey = (u64, u64, u64, u64, usize);

#[derive(Clone, Copy)]
struct CircleRangeChoices {
    ranges: [[f64; 2]; 2],
    len: usize,
}

impl AsRef<[[f64; 2]]> for CircleRangeChoices {
    fn as_ref(&self) -> &[[f64; 2]] {
        &self.ranges[..self.len]
    }
}

struct StandardCirclePairConstraint {
    ranges_by_face: RefCell<HashMap<CircleFaceKey, Vec<CircleRangeChoices>>>,
}

impl StandardCirclePairConstraint {
    fn new(
        ctx: &DecodeContext<'_>,
        supports: &[crate::families::standard::records::StandardCurveSupport],
        endpoint_options: &[Vec<[usize; 2]>],
    ) -> Result<Self, CodecError> {
        let mut ranges_by_face = HashMap::<CircleFaceKey, Vec<CircleRangeChoices>>::new();
        for (support, options) in supports.iter().zip(endpoint_options) {
            if options.len() <= 1 {
                continue;
            }
            let crate::families::standard::records::StandardCurveGeometry::Circle {
                center,
                radius,
            } = &support.geometry
            else {
                continue;
            };
            let center = center.get();
            let radius = radius.get();
            for &face in &support.faces {
                let key = (
                    center.x.to_bits(),
                    center.y.to_bits(),
                    center.z.to_bits(),
                    radius.to_bits(),
                    face,
                );
                if !ranges_by_face.contains_key(&key) {
                    ctx.insert_hash_map(
                        &mut ranges_by_face,
                        key,
                        Vec::new(),
                        "catia_standard_circle_constraint_faces",
                    )?;
                }
                if let Some(ranges) = ranges_by_face.get_mut(&key) {
                    ctx.push_vec(
                        ranges,
                        CircleRangeChoices {
                            ranges: [[0.0; 2]; 2],
                            len: 0,
                        },
                        "catia_standard_circle_constraint_ranges",
                    )?;
                }
            }
        }
        for ranges in ranges_by_face.values_mut() {
            ranges.clear();
        }
        Ok(Self {
            ranges_by_face: RefCell::new(ranges_by_face),
        })
    }
}

fn standard_circle_pair_solution_is_simple(
    constraint: &StandardCirclePairConstraint,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    let mut range_choices = constraint.ranges_by_face.borrow_mut();
    for choices in range_choices.values_mut() {
        choices.clear();
    }
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
        let center = center.get();
        let radius = radius.get();
        let Some(start) = ir
            .model
            .points
            .get(pair[0])
            .map(|point| point.position().get())
        else {
            return false;
        };
        let Some(end) = ir
            .model
            .points
            .get(pair[1])
            .map(|point| point.position().get())
        else {
            return false;
        };
        let axes = support
            .faces
            .iter()
            .filter_map(|face| face_surface(ir, bindings, surface_indices, *face))
            .filter_map(|surface| {
                standard_circle_axis_from_carrier(center, radius, &surface.geometry)
            });
        let mut axes = axes;
        let Some(axis) = axes
            .next()
            .and_then(|axis| canonical_unoriented_axis(*axis.as_raw()))
        else {
            continue;
        };
        if axes.any(|other| {
            canonical_unoriented_axis(*other.as_raw())
                .is_none_or(|other| axis.as_raw().dot(*other.as_raw()).abs() < 0.9999)
        }) {
            return false;
        }
        let Some(choices) = circle_endpoint_range_choices(center, radius, axis, start, end) else {
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
            let Some(ranges) = range_choices.get_mut(&key) else {
                return false;
            };
            ranges.push(choices);
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeLineRole {
    NotLine,
    Fixed,
    Flexible,
}

struct StandardLinePairConstraint {
    points: Vec<Point3>,
    edge_roles: Vec<EdgeLineRole>,
    edges_by_face: HashMap<usize, Vec<usize>>,
}

impl StandardLinePairConstraint {
    fn new(
        ctx: &DecodeContext<'_>,
        points: &[Point],
        supports: &[crate::families::standard::records::StandardCurveSupport],
        endpoint_options: &[Vec<[usize; 2]>],
    ) -> Result<Self, CodecError> {
        let points = ctx.collect_vec(
            points.iter().map(|point| point.position().get()),
            "catia_standard_line_constraint_points",
        )?;
        let edge_roles = ctx.collect_vec(
            supports.iter().enumerate().map(|(edge, support)| {
                if !matches!(
                    support.geometry,
                    crate::families::standard::records::StandardCurveGeometry::Line
                ) {
                    EdgeLineRole::NotLine
                } else if endpoint_options
                    .get(edge)
                    .is_some_and(|options| options.len() > 1)
                {
                    EdgeLineRole::Flexible
                } else {
                    EdgeLineRole::Fixed
                }
            }),
            "catia_standard_line_constraint_roles",
        )?;
        let mut edges_by_face = HashMap::<usize, Vec<usize>>::new();

        for (edge, support) in supports.iter().enumerate() {
            if edge_roles[edge] != EdgeLineRole::Flexible {
                continue;
            }
            for &face in &support.faces {
                if !edges_by_face.contains_key(&face) {
                    ctx.insert_hash_map(
                        &mut edges_by_face,
                        face,
                        Vec::new(),
                        "catia_standard_line_constraint_faces",
                    )?;
                }
                if let Some(edges) = edges_by_face.get_mut(&face) {
                    if !edges.contains(&edge) {
                        ctx.push_vec(edges, edge, "catia_standard_line_constraint_face_edges")?;
                    }
                }
            }
        }

        Ok(Self {
            points,
            edge_roles,
            edges_by_face,
        })
    }

    fn flexible_edge_mask(&self) -> impl Iterator<Item = bool> + '_ {
        self.edge_roles
            .iter()
            .map(|role| *role == EdgeLineRole::Flexible)
    }

    fn edge_pairs<'a>(&self, pairs: &'a [Option<[usize; 2]>]) -> Option<StandardLineEdgePairs<'a>> {
        StandardLineEdgePairs::new(pairs, self.edge_roles.len())
    }

    fn is_valid(&self, pairs: &StandardLineEdgePairs<'_>) -> bool {
        self.edge_roles
            .iter()
            .zip(pairs.pairs())
            .all(|(role, pair)| {
                if *role == EdgeLineRole::NotLine {
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

    fn is_simple(&self, pairs: &StandardLineEdgePairs<'_>) -> bool {
        if !self.is_valid(pairs) {
            return false;
        }
        for edges in self.edges_by_face.values() {
            for (left_position, &left_edge) in edges.iter().enumerate() {
                let Some(left_pair) = pairs.pairs()[left_edge] else {
                    continue;
                };
                let Some(left) = standard_line_segment(&self.points, left_pair) else {
                    continue;
                };
                for &right_edge in &edges[left_position + 1..] {
                    let Some(right_pair) = pairs.pairs()[right_edge] else {
                        continue;
                    };
                    let Some(right) = standard_line_segment(&self.points, right_pair) else {
                        continue;
                    };
                    if !standard_line_segments_are_simple(left, right) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Owns the length agreement between a candidate solution and the edge roles
/// it is validated against; the field is unreachable outside this module.
mod line_edge_pairs {
    /// Endpoint pairs whose length matches the constraint's edge roles.
    pub(super) struct StandardLineEdgePairs<'a> {
        pairs: &'a [Option<[usize; 2]>],
    }

    impl<'a> StandardLineEdgePairs<'a> {
        /// Admits a candidate solution that has one entry per edge role.
        pub(super) fn new(pairs: &'a [Option<[usize; 2]>], edge_count: usize) -> Option<Self> {
            (pairs.len() == edge_count).then_some(Self { pairs })
        }

        /// Returns the candidate entries, one per edge role.
        pub(super) fn pairs(&self) -> &'a [Option<[usize; 2]>] {
            self.pairs
        }
    }
}

use line_edge_pairs::StandardLineEdgePairs;

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
fn standard_line_pair_solution_is_simple(
    points: &[Point],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    let point_positions = points
        .iter()
        .map(|point| point.position().get())
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
fn standard_line_pair_solution_is_simple_cached(
    points: &[Point],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    crate::test_support::with_service_context(|ctx| {
        let constraint = StandardLinePairConstraint::new(ctx, points, supports, endpoint_options)
            .expect("service budget admits line constraint");
        constraint
            .edge_pairs(pairs)
            .is_some_and(|pairs| constraint.is_simple(&pairs))
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod circle_axis_tests {
    use super::edge_geometry::{circle_axis_from_carrier, standard_circle_axis_from_carrier};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::units::UnitVector3;

    fn unit(value: Vector3) -> UnitVector3 {
        UnitVector3::new(value).expect("unit fixture")
    }

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
        let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin(), z(), x())
                .expect("valid PlaneSurface fixture"),
        ));
        assert_eq!(
            circle_axis_from_carrier(origin(), 2.0, &plane),
            Some(unit(z()))
        );

        let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(origin(), z(), x(), 2.0)
                .expect("valid CylinderSurface fixture"),
        ));
        assert_eq!(
            circle_axis_from_carrier(origin(), 2.0, &cylinder),
            Some(unit(z()))
        );
        assert_eq!(circle_axis_from_carrier(origin(), 3.0, &cylinder), None);

        let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(origin(), z(), x(), 5.0)
                .expect("valid SphereSurface fixture"),
        ));
        assert_eq!(
            circle_axis_from_carrier(Point3::new(0.0, 0.0, 3.0), 4.0, &sphere),
            Some(unit(z()))
        );
        assert_eq!(circle_axis_from_carrier(origin(), 5.0, &sphere), None);

        let tiny = 1e-200;
        let unit_sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(origin(), z(), x(), 1.0)
                .expect("valid SphereSurface fixture"),
        ));
        assert_eq!(
            circle_axis_from_carrier(Point3::new(tiny, 0.0, 0.0), 1.0, &unit_sphere),
            Some(unit(x()))
        );

        let torus = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::analytic::TorusSurface::try_new(origin(), z(), x(), 10.0, 2.0)
                .expect("valid TorusSurface fixture"),
        ));
        assert_eq!(
            circle_axis_from_carrier(Point3::new(10.0, 0.0, 0.0), 2.0, &torus),
            Some(unit(y()))
        );
        assert_eq!(
            circle_axis_from_carrier(Point3::new(0.0, 0.0, 2.0), 10.0, &torus),
            Some(unit(z()))
        );
    }

    #[test]
    fn centered_sphere_does_not_override_a_cylinder_axis() {
        let center = Point3::new(1e-10, 0.0, -1e-10);
        let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(origin(), z(), x(), 3.175)
                .expect("valid SphereSurface fixture"),
        ));
        let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(center.x, 0.0, center.z),
                y(),
                x(),
                3.175,
            )
            .expect("valid CylinderSurface fixture"),
        ));

        assert_eq!(
            standard_circle_axis_from_carrier(center, 3.175, &sphere),
            None
        );
        assert_eq!(
            standard_circle_axis_from_carrier(center, 3.175, &cylinder),
            Some(unit(y()))
        );
    }

    #[test]
    fn unoriented_circle_axes_use_one_parameter_frame() {
        const AXIS_COMPONENT_TOLERANCE: f64 = 1e-12;

        assert_eq!(super::canonical_unoriented_axis(z()), Some(unit(z())));
        assert_eq!(
            super::canonical_unoriented_axis(Vector3::new(0.0, 0.0, -1.0)),
            Some(unit(z()))
        );
        let axis =
            super::canonical_unoriented_axis(Vector3::new(-2.0, 1.0, 0.0)).expect("finite axis");
        let axis = axis.as_raw();
        let length = 5.0_f64.sqrt();
        assert!((axis.x - 2.0 / length).abs() < AXIS_COMPONENT_TOLERANCE);
        assert!((axis.y + 1.0 / length).abs() < AXIS_COMPONENT_TOLERANCE);
    }
    #[test]
    fn anisotropic_surface_chart_retains_its_point_witness() {
        use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
        let u = NurbsSurfaceAxis::new(1, vec![0., 0., 1e-10, 1e-10], false);
        let v = NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false);
        let surface = NurbsSurface::from_lanes(
            u,
            v,
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0., 0., 0.), Point3::new(0., 1., 0.)],
                    vec![Point3::new(1., 0., 0.), Point3::new(1., 1., 0.)],
                ],
                None,
            ),
            false,
        )
        .expect("anisotropic nurbs surface");
        let residual = super::surface_membership::nurbs_surface_witness_distance(
            &surface,
            Point3::new(0.3, 0.4, 0.),
        )
        .expect("witness evaluation accepts the fixture")
        .expect("witness distance for a point on the surface");
        assert!(residual <= super::NURBS_SURFACE_MEMBERSHIP_TOLERANCE);
    }
}

#[cfg(test)]
mod numerical_range_tests;
