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
    attach_standard_circles, attach_standard_lines, bounds_overlap, build_standard_edge_curve,
    intersection_line_direction, nurbs_surface_control_bounds, point_bounds,
    point_on_standard_face, point_on_surface, point_on_surface_if_supported,
    resolve_standard_limit_curve_binding, same_cone_generator_pair, standard_limit_curve_bindings,
    standard_nurbs_line_pair_on_face, standard_pcurve_geometry,
    standard_shared_nurbs_boundary_pair_options, BoundsEntry, BoundsIndex, EdgeLineRole,
    StandardCirclePairConstraint, StandardLimitCurveBinding, StandardLinePairConstraint,
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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
use crate::solve::matching::{
    distinct_domain_matching_with_budget, retain_distinct_matching_supports,
};
use crate::solve::{mesh_gauge::MeshEdgeGeometry, mesh_quotient, missing_edge};
use crate::variant::Variant;
use crate::wire::records::ConsolidatedRecord;

const EPS_STANDARD_DECODE_COARSE_GEOMETRY: f64 = 1.0e-6;
const EPS_STANDARD_DECODE_GEOMETRY: f64 = 1.0e-9;

const EPS_ANTIPODAL_CIRCLE: f64 = 2e-3;
const SPHERE_SECTION_ENDPOINT_TOLERANCE: f64 = 2e-3;
const SPHERE_CENTER_COINCIDENCE_TOLERANCE: f64 = 2e-3;
const CYLINDER_PLANE_CONIC_TOLERANCE: f64 = 2e-3;
const PERPENDICULAR_CYLINDER_CONIC_TOLERANCE: f64 = 2e-3;
const ANALYTIC_CURVE_ENDPOINT_TOLERANCE: f64 = 2e-3;
const SUPPORT_AGREEMENT_TOLERANCE: f64 = EPS_STANDARD_DECODE_COARSE_GEOMETRY;
const STANDARD_FACE_BOUNDS_TOLERANCE: f64 = 2e-3;
const NURBS_SURFACE_MEMBERSHIP_TOLERANCE: f64 = 2e-3;
const NURBS_SURFACE_SEEDS_PER_SPAN: usize = 3;
const NURBS_SURFACE_MAX_SEEDS: usize = 256;
const NURBS_SURFACE_REFINEMENT_ITERATIONS: usize = 24;
const NURBS_SURFACE_BACKTRACK_STEPS: usize = 8;

fn bind_consolidated_revolution_faces_and_seams(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
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
    ) -> Option<(SolvedCurveGeometry, [f64; 2])> {
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
            SolvedCurveGeometry::Circle(
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
            ),
            [0.0, expected_sweep],
        ))
    }

    let mut storage = ctx.reserve_scoped(0, "catia_revolution_binding_storage")?;
    let (point_positions, edge_indices, coedge_indices, loop_indices, unknown_surfaces) =
        storage.with_storage(|| {
            let mut point_positions = HashMap::new();
            for point in ctx.admit_iter(&ir.model.points, "catia_standard_iteration")? {
                ctx.insert_hash_map(
                    &mut point_positions,
                    &point.id,
                    point.position().get(),
                    "catia_revolution_point_positions",
                )?;
            }
            let mut edge_indices = HashMap::new();
            for (index, edge) in ctx
                .admit_iter(&ir.model.edges, "catia_standard_iteration")?
                .enumerate()
            {
                ctx.insert_hash_map(
                    &mut edge_indices,
                    &edge.id,
                    index,
                    "catia_revolution_edge_indices",
                )?;
            }
            let mut coedge_indices = HashMap::new();
            for (index, coedge) in ctx
                .admit_iter(&ir.model.coedges, "catia_standard_iteration")?
                .enumerate()
            {
                ctx.insert_hash_map(
                    &mut coedge_indices,
                    &coedge.id,
                    index,
                    "catia_revolution_coedge_indices",
                )?;
            }
            let mut loop_indices = HashMap::new();
            for (index, loop_) in ctx
                .admit_iter(&ir.model.loops, "catia_standard_iteration")?
                .enumerate()
            {
                ctx.insert_hash_map(
                    &mut loop_indices,
                    &loop_.id,
                    index,
                    "catia_revolution_loop_indices",
                )?;
            }
            let mut unknown_surfaces = HashSet::new();
            for surface in ctx.admit_iter(&ir.model.surfaces, "catia_standard_iteration")? {
                if matches!(
                    surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                ) {
                    ctx.insert_hash_set(
                        &mut unknown_surfaces,
                        &surface.id,
                        "catia_revolution_unknown_surfaces",
                    )?;
                }
            }
            Ok::<_, CodecError>((
                point_positions,
                edge_indices,
                coedge_indices,
                loop_indices,
                unknown_surfaces,
            ))
        })?;
    let (vertex_positions, curve_indices) = storage.with_storage(|| {
        let mut vertex_positions = HashMap::new();
        for vertex in ctx.admit_iter(&ir.model.vertices, "catia_standard_iteration")? {
            if let Some(&position) = ctx.get_hash_map(
                &point_positions,
                &vertex.point,
                "catia_revolution_point_position_lookup",
            )? {
                ctx.insert_hash_map(
                    &mut vertex_positions,
                    &vertex.id,
                    position,
                    "catia_revolution_vertex_positions",
                )?;
            }
        }
        let mut curve_indices = HashMap::new();
        for (index, curve) in ctx
            .admit_iter(&ir.model.curves, "catia_standard_iteration")?
            .enumerate()
        {
            ctx.insert_hash_map(
                &mut curve_indices,
                &curve.id,
                index,
                "catia_revolution_curve_indices",
            )?;
        }
        Ok::<_, CodecError>((vertex_positions, curve_indices))
    })?;
    let mut surface_bindings = HashMap::<&SurfaceId, Option<usize>>::new();
    {
        let mut visits = ir.model.faces.iter();
        while let Some(face) = ctx.next_charged(&mut visits, "catia_standard_iteration")? {
            if !ctx.contains_hash_set(
                &unknown_surfaces,
                &&face.surface,
                "catia_revolution_unknown_surface_lookup",
            )? {
                continue;
            }
            let mut witness_storage = ctx.reserve_scoped(0, "catia_revolution_face_witnesses")?;
            let mut witnesses = Vec::new();
            {
                let mut visit_loop = |loop_id: &LoopId| -> Result<(), CodecError> {
                    let Some(&loop_index) =
                        ctx.get_hash_map(&loop_indices, loop_id, "catia_revolution_loop_lookup")?
                    else {
                        return Ok(());
                    };
                    let Some(loop_) = ir.model.loops.get(loop_index) else {
                        return Ok(());
                    };
                    for coedge_id in
                        ctx.admit_iter(loop_.coedges(), "catia_revolution_face_loop_coedges")?
                    {
                        let Some(&coedge_index) = ctx.get_hash_map(
                            &coedge_indices,
                            coedge_id,
                            "catia_revolution_coedge_lookup",
                        )?
                        else {
                            continue;
                        };
                        let Some(&edge_index) = ctx.get_hash_map(
                            &edge_indices,
                            &ir.model.coedges[coedge_index].edge,
                            "catia_revolution_edge_lookup",
                        )?
                        else {
                            continue;
                        };
                        let edge = &ir.model.edges[edge_index];
                        for id in [&edge.start, &edge.end] {
                            if let Some(&point) = ctx.get_hash_map(
                                &vertex_positions,
                                id,
                                "catia_revolution_vertex_position_lookup",
                            )? {
                                ctx.push_scoped_vec(
                                    &mut witness_storage,
                                    &mut witnesses,
                                    point,
                                    "catia_revolution_face_witnesses",
                                )?;
                            }
                        }
                        let Some(curve_id) = edge.curve() else {
                            continue;
                        };
                        let Some(&curve_index) = ctx.get_hash_map(
                            &curve_indices,
                            curve_id,
                            "catia_revolution_curve_lookup",
                        )?
                        else {
                            continue;
                        };
                        let curve = &ir.model.curves[curve_index].geometry;
                        let Some([start, end]) =
                            edge.param_range().map(cadmpeg_ir::units::FiniteVector::get)
                        else {
                            continue;
                        };
                        let parameter = start.midpoint(end);
                        if let Some(point) = cadmpeg_ir::eval::finite_or_refusal(
                            cadmpeg_ir::eval::decode::outer_refusal(
                                cadmpeg_ir::eval::decode::curve_point(ctx, curve, parameter),
                            )?,
                        )? {
                            ctx.push_scoped_vec(
                                &mut witness_storage,
                                &mut witnesses,
                                point.get(),
                                "catia_revolution_face_witnesses",
                            )?;
                        }
                    }
                    Ok(())
                };
                match &face.loops {
                    cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => {
                        for loop_id in ctx.admit_iter(loops, "catia_revolution_face_loops")? {
                            visit_loop(loop_id)?;
                        }
                    }
                    cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
                        visit_loop(outer)?;
                        for loop_id in ctx.admit_iter(inner, "catia_revolution_face_loops")? {
                            visit_loop(loop_id)?;
                        }
                    }
                }
            }
            if witnesses.len() < 2 {
                continue;
            }
            let mut binding = None;
            let ambiguous = ctx.any_by(
                revolutions.iter().enumerate(),
                |(index, revolution)| {
                    if !ctx.all_by(
                        &witnesses,
                        |point| Ok(point_on_torus(*point, &revolution.geometry, TOLERANCE)),
                        "catia_revolution_face_witness_test",
                    )? {
                        return Ok(false);
                    }
                    Ok(binding.replace(index).is_some())
                },
                "catia_revolution_face_matches",
            )?;
            let Some(binding) = binding.filter(|_| !ambiguous) else {
                continue;
            };
            if let Some(stored) = ctx.get_mut_hash_map(
                &mut surface_bindings,
                &face.surface,
                "catia_revolution_surface_binding_lookup",
            )? {
                if *stored != Some(binding) {
                    *stored = None;
                }
            } else {
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut surface_bindings,
                        &face.surface,
                        Some(binding),
                        "catia_revolution_surface_bindings",
                    )
                })?;
            }
        }
    }

    let (procedural_owners, _owner_storage) = ctx.unique_index(
        ir.model
            .curves
            .iter()
            .enumerate()
            .map(|(index, curve)| (curve.geometry.procedural_construction(), index)),
        "catia_revolution_procedural_owners",
    )?;
    let mut procedural_bindings = HashMap::<usize, Option<usize>>::new();
    for procedure in ctx.admit_iter(&ir.model.procedural_curves, "catia_standard_iteration")? {
        let ProceduralCurveDefinition::Intersection { context, .. } = procedure.definition() else {
            continue;
        };
        let [Some(first), Some(second)] =
            std::array::from_fn(|side| context.sides()[side].surface.as_ref())
        else {
            continue;
        };
        let Some(&Some(binding)) = ctx.get_hash_map(
            &surface_bindings,
            &first,
            "catia_revolution_surface_binding_lookup",
        )?
        else {
            continue;
        };
        if ctx.get_hash_map(
            &surface_bindings,
            &second,
            "catia_revolution_surface_binding_lookup",
        )? != Some(&Some(binding))
        {
            continue;
        }
        let Some(&Some(owner)) = ctx.get_hash_map(
            &procedural_owners,
            &Some(&procedure.id),
            "catia_revolution_procedural_owner_lookup",
        )?
        else {
            continue;
        };
        if let Some(stored) = ctx.get_mut_hash_map(
            &mut procedural_bindings,
            &owner,
            "catia_revolution_procedural_binding_lookup",
        )? {
            *stored = None;
        } else {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut procedural_bindings,
                    owner,
                    Some(binding),
                    "catia_revolution_procedural_bindings",
                )
            })?;
        }
    }
    let mut curve_edge_counts = HashMap::<usize, usize>::new();
    for curve in ctx
        .admit_iter(&ir.model.edges, "catia_standard_iteration")?
        .filter_map(|edge| edge.curve())
    {
        let Some(&curve_index) =
            ctx.get_hash_map(&curve_indices, curve, "catia_revolution_curve_lookup")?
        else {
            continue;
        };
        if let Some(count) = ctx.get_mut_hash_map(
            &mut curve_edge_counts,
            &curve_index,
            "catia_revolution_curve_edge_count_lookup",
        )? {
            *count += 1;
        } else {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut curve_edge_counts,
                    curve_index,
                    1,
                    "catia_revolution_curve_edge_counts",
                )
            })?;
        }
    }
    let mut seams = Vec::new();
    for (edge_index, edge) in ctx
        .admit_iter(&ir.model.edges, "catia_standard_iteration")?
        .enumerate()
    {
        let Some(curve_id) = edge.curve() else {
            continue;
        };
        let Some(&curve_index) =
            ctx.get_hash_map(&curve_indices, curve_id, "catia_revolution_curve_lookup")?
        else {
            continue;
        };
        let unresolved = match &ir.model.curves[curve_index].geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }) => true,
            CurveGeometry::Procedural { cache, .. } => cache
                .as_ref()
                .is_none_or(|cache| matches!(cache, SolvedCurveGeometry::Unknown { .. })),
            CurveGeometry::Solved(_) => false,
        };
        if !unresolved
            || ctx.get_hash_map(
                &curve_edge_counts,
                &curve_index,
                "catia_revolution_curve_edge_count_lookup",
            )? != Some(&1)
        {
            continue;
        }
        let Some(&Some(binding)) = ctx.get_hash_map(
            &procedural_bindings,
            &curve_index,
            "catia_revolution_procedural_binding_lookup",
        )?
        else {
            continue;
        };
        let Some(&start) = ctx.get_hash_map(
            &vertex_positions,
            &edge.start,
            "catia_revolution_vertex_position_lookup",
        )?
        else {
            continue;
        };
        let Some(&end) = ctx.get_hash_map(
            &vertex_positions,
            &edge.end,
            "catia_revolution_vertex_position_lookup",
        )?
        else {
            continue;
        };
        let Some((geometry, parameter_range)) = meridian_arc(
            start,
            end,
            &revolutions[binding].geometry,
            revolutions[binding].profile_sweep,
        ) else {
            continue;
        };
        storage.with_storage(|| {
            ctx.push_vec(
                &mut seams,
                (edge_index, curve_index, geometry, parameter_range),
                "catia_revolution_seams",
            )
        })?;
    }
    let bound_surfaces = surface_bindings.len();

    for surface in ctx.admit_iter(&mut ir.model.surfaces, "catia_standard_revolution_surfaces")? {
        let Some(&Some(binding)) = ctx.get_hash_map(
            &surface_bindings,
            &surface.id,
            "catia_revolution_surface_binding_lookup",
        )?
        else {
            continue;
        };
        surface.geometry = revolutions[binding]
            .geometry
            .try_clone_for_decode(ctx, "catia_revolution_surface_geometry_copy")?;
        crate::resource::derived_annotation(ctx, annotations, &surface.id, "geometry")?;
    }
    let seam_count = seams.len();
    for (edge_index, curve_index, geometry, parameter_range) in
        ctx.admit_iter(seams, "catia_revolution_seams")?
    {
        let curve = &mut ir.model.curves[curve_index];
        match &mut curve.geometry {
            CurveGeometry::Procedural { cache, .. } => *cache = Some(geometry),
            carrier @ CurveGeometry::Solved(_) => *carrier = CurveGeometry::Solved(geometry),
        }
        let edge = &mut ir.model.edges[edge_index];
        let curve_id = curve
            .id
            .try_clone_for_decode(ctx, "catia_revolution_seam_curve_id_copy")?;
        edge.carrier =
            cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), Some(parameter_range))
                .map_err(cadmpeg_core::CodecError::malformed)?;
        crate::resource::derived_annotation(ctx, annotations, &curve.id, "geometry")?;
        crate::resource::derived_annotation(ctx, annotations, &edge.id, "param_range")?;
    }
    Ok((bound_surfaces, seam_count))
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
                    &cadmpeg_test_support::service_decode_context(),
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
                .expect("fixture constructor admission")
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
                cadmpeg_ir::topology::LoopRing::new(
                    &cadmpeg_test_support::service_decode_context(),
                    vec![coedge_id.clone()],
                    Vec::new(),
                )
                .expect("fixture ring admission")
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
                    cadmpeg_ir::topology::LoopRing::new(
                        &cadmpeg_test_support::service_decode_context(),
                        vec![coedge.clone()],
                        Vec::new(),
                    )
                    .expect("fixture ring admission")
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
                &cadmpeg_ir::document::admission::StandardAdmission,
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
            .expect("procedural curve admission")
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
    mod mutable_lookup;
}

fn refine_consolidated_analytic_surfaces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    surfaces: &mut [Option<SurfaceGeometry>],
    surface_records: &[crate::families::standard::records::StandardSurfaceRecord],
) -> Result<HashMap<usize, usize>, cadmpeg_core::CodecError> {
    // Stored carriers are binary32 values widened to binary64; a coarse
    // surface matches a carrier when every widened scalar has its exact bits.
    fn quantized(value: f64) -> Option<u64> {
        f32_from_f64(value).map(|value| f64::from(value).to_bits())
    }
    fn quantized_point(stored: [f64; 3]) -> Option<[u64; 3]> {
        Some([
            quantized(stored[0])?,
            quantized(stored[1])?,
            quantized(stored[2])?,
        ])
    }
    // The stored axis keeps binary32 x and y; z is reconstructed as the
    // unit-length complement with the stored sign.
    fn quantized_axis(stored: [f64; 3]) -> Option<[u64; 3]> {
        let x = f32_from_f64(stored[0])?;
        let y = f32_from_f64(stored[1])?;
        let z = (1.0 - f64::from(x * x + y * y))
            .max(0.0)
            .sqrt()
            .copysign(stored[2]);
        let axis = *unit_vector(Vector3::new(f64::from(x), f64::from(y), z))?.as_raw();
        Some([axis.x.to_bits(), axis.y.to_bits(), axis.z.to_bits()])
    }
    fn bits(point: Vector3) -> [u64; 3] {
        [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()]
    }
    fn point_bits(point: Point3) -> [u64; 3] {
        [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()]
    }
    fn is_unit_cone(surface: &SurfaceGeometry) -> bool {
        matches!(
            surface,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone))
                if cone.radius().get() == 0.0 && cone.ratio().get() == 1.0
        )
    }

    let considered = surfaces.len().min(surface_records.len());
    let surfaces = &mut surfaces[..considered];
    let wanted = ctx.fold(
        surfaces,
        [false; 4],
        |mut wanted, surface| {
            match surface {
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))) => {
                    wanted[0] = true;
                }
                Some(surface) if is_unit_cone(surface) => wanted[1] = true,
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))) => {
                    wanted[2] = true;
                }
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))) => {
                    wanted[3] = true;
                }
                _ => {}
            }
            Ok(wanted)
        },
        "catia_standard_refined_surface_kinds",
    )?;

    // Each index maps a quantized carrier key to its unique record; a key
    // shared by two records keeps a tombstone and refines nothing.
    let cylinders = if wanted[0] {
        let mut failure = None;
        let index = ctx.unique_index(
            crate::families::b2::records::b2_cylinders_from_records(ctx, bytes, records)?
                .map_while(|cylinder| cylinder.map_err(|error| failure = Some(error)).ok())
                .map(|cylinder| {
                    let key = quantized_point(cylinder.origin.get().into()).and_then(|origin| {
                        let axis = quantized_axis(cylinder.frame.axis().get())?;
                        let radius = quantized(cylinder.radius.get())?;
                        Some((origin, axis, radius))
                    });
                    (key, cylinder)
                }),
            "catia_standard_refined_cylinders",
        )?;
        if let Some(error) = failure {
            return Err(error);
        }
        Some(index)
    } else {
        None
    };
    let cones = if wanted[1] {
        Some(ctx.unique_index(
            crate::families::b2::records::b2_cones_from_records(ctx, bytes, records)?.map(|cone| {
                let key = quantized_point(cone.apex.get().into()).and_then(|apex| {
                    let axis = quantized_axis(cone.frame.axis().get())?;
                    let half_angle = quantized(cone.half_angle.get())?;
                    Some((apex, axis, half_angle))
                });
                (key, cone)
            }),
            "catia_standard_refined_cones",
        )?)
    } else {
        None
    };
    let spheres = if wanted[2] {
        Some(ctx.unique_index(
            crate::families::b2::records::b2_spheres_from_records(ctx, bytes, records)?.map(
                |sphere| {
                    let key = quantized_point(sphere.center.get().into()).and_then(|center| {
                        let radius = quantized(sphere.radius.get())?;
                        Some((center, radius))
                    });
                    (key, sphere)
                },
            ),
            "catia_standard_refined_spheres",
        )?)
    } else {
        None
    };
    let tori = if wanted[3] {
        Some(ctx.unique_index(
            crate::families::b2::records::b2_tori_from_records(ctx, bytes, records)?.map(|torus| {
                let key = quantized_point(torus.center.get().into()).and_then(|center| {
                    let axis = quantized_axis(torus.frame.axis().get())?;
                    let major_radius = quantized(torus.major_radius.get())?;
                    let minor_radius = quantized(torus.minor_radius.get())?;
                    Some((center, axis, major_radius, minor_radius))
                });
                (key, torus)
            }),
            "catia_standard_refined_tori",
        )?)
    } else {
        None
    };

    let mut refined = HashMap::new();
    for (index, surface) in ctx
        .admit_iter(surfaces, "catia_standard_refined_surface_slots")?
        .enumerate()
    {
        let replacement = match surface.as_ref() {
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))) => {
                let key = Some((
                    point_bits(cylinder_surface.origin().get()),
                    bits(*cylinder_surface.frame().axis().as_raw()),
                    cylinder_surface.radius().get().to_bits(),
                ));
                cylinders
                    .as_ref()
                    .map(|(index, _)| {
                        ctx.get_hash_map(index, &key, "catia_standard_refined_cylinder_lookup")
                    })
                    .transpose()?
                    .flatten()
                    .and_then(Option::as_ref)
                    .map(|cylinder| (cylinder.surface_geometry(), cylinder.pos))
            }
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)))
                if cone_surface.radius().get() == 0.0 && cone_surface.ratio().get() == 1.0 =>
            {
                let key = Some((
                    point_bits(cone_surface.origin().get()),
                    bits(*cone_surface.frame().axis().as_raw()),
                    cone_surface.half_angle().get().to_bits(),
                ));
                cones
                    .as_ref()
                    .map(|(index, _)| {
                        ctx.get_hash_map(index, &key, "catia_standard_refined_cone_lookup")
                    })
                    .transpose()?
                    .flatten()
                    .and_then(Option::as_ref)
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
                let key = Some((
                    point_bits(sphere_surface.center().get()),
                    sphere_surface.radius().get().to_bits(),
                ));
                spheres
                    .as_ref()
                    .map(|(index, _)| {
                        ctx.get_hash_map(index, &key, "catia_standard_refined_sphere_lookup")
                    })
                    .transpose()?
                    .flatten()
                    .and_then(Option::as_ref)
                    .map(|sphere| {
                        (
                            crate::families::b2::records::b2_sphere_geometry(sphere),
                            sphere.pos,
                        )
                    })
            }
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface))) => {
                let key = Some((
                    point_bits(torus_surface.center().get()),
                    bits(*torus_surface.frame().axis().as_raw()),
                    torus_surface.major_radius().get().to_bits(),
                    torus_surface.minor_radius().get().to_bits(),
                ));
                tori.as_ref()
                    .map(|(index, _)| {
                        ctx.get_hash_map(index, &key, "catia_standard_refined_torus_lookup")
                    })
                    .transpose()?
                    .flatten()
                    .and_then(Option::as_ref)
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
        let surface_record = crate::families::standard::records::StandardSurfaceRecord::Analytic(
            crate::families::standard::records::SurfacePrefix {
                pos: 0,
                target: 0,
                kind: crate::families::standard::records::AnalyticSurfaceKind::Plane,
            },
        );
        let surface_records = vec![surface_record; surfaces.len()];
        crate::test_support::with_service_context(|ctx| {
            refine_consolidated_analytic_surfaces(ctx, bytes, records, surfaces, &surface_records)
                .expect("service decode")
        })
    }

    fn plane_records(
        count: usize,
    ) -> Vec<crate::families::standard::records::StandardSurfaceRecord> {
        vec![
            crate::families::standard::records::StandardSurfaceRecord::Analytic(
                crate::families::standard::records::SurfacePrefix {
                    pos: 0,
                    target: 0,
                    kind: crate::families::standard::records::AnalyticSurfaceKind::Plane,
                },
            );
            count
        ]
    }

    fn coarse_surface(kind: &str) -> SurfaceGeometry {
        use cadmpeg_ir::geometry::analytic::{
            ConeSurface, CylinderSurface, SphereSurface, TorusSurface,
        };
        let origin = Point3::new(1.0, 2.0, 3.0);
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let reference = Vector3::new(1.0, 0.0, 0.0);
        SurfaceGeometry::Solved(match kind {
            "cylinder" => SolvedSurfaceGeometry::Cylinder(
                CylinderSurface::try_new(origin, axis, reference, 2.0).expect("cylinder"),
            ),
            "cone" => SolvedSurfaceGeometry::Cone(
                ConeSurface::try_new(origin, axis, reference, 0.0, 1.0, 0.25).expect("cone"),
            ),
            "sphere" => SolvedSurfaceGeometry::Sphere(
                SphereSurface::try_new(origin, axis, reference, 2.0).expect("sphere"),
            ),
            _ => SolvedSurfaceGeometry::Torus(
                TorusSurface::try_new(origin, axis, reference, 5.0, 2.0).expect("torus"),
            ),
        })
    }

    #[test]
    fn cylinder_refinement_refuses_cylinder_collection_before_copy() {
        let bytes = crate::test_support::test_b2::b2_cylinder_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let surface_records = plane_records(1);
        let mut surfaces = [Some(coarse_surface("cylinder"))];
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            refine_consolidated_analytic_surfaces(
                ctx,
                &bytes,
                &records,
                &mut surfaces,
                &surface_records,
            )
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_standard_refined_cylinders")
        );
        let refined = crate::test_support::with_service_context(|ctx| {
            refine_consolidated_analytic_surfaces(ctx, &bytes, &records, &mut [], &[])
        })
        .expect("service decode");
        assert!(refined.is_empty());
    }

    #[test]
    fn analytic_refinement_refuses_each_parsed_carrier_collection() {
        for (bytes, kind, operation) in [
            (
                crate::test_support::test_b2::b2_cone_stream(),
                "cone",
                "catia_standard_refined_cones",
            ),
            (
                crate::test_support::test_b2::b2_sphere_stream(),
                "sphere",
                "catia_standard_refined_spheres",
            ),
            (
                crate::test_support::test_b2::b2_torus_stream(),
                "torus",
                "catia_standard_refined_tori",
            ),
        ] {
            let records = crate::wire::records::consolidated_records(&bytes);
            let surface_records = plane_records(1);
            let mut surfaces = [Some(coarse_surface(kind))];
            let limited = crate::test_support::with_collection_limit(0, |ctx| {
                refine_consolidated_analytic_surfaces(
                    ctx,
                    &bytes,
                    &records,
                    &mut surfaces,
                    &surface_records,
                )
            });
            assert!(
                matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == operation)
            );
        }
    }

    #[test]
    fn analytic_refinement_parses_no_carrier_without_a_matching_surface_kind() {
        let bytes = crate::test_support::test_b2::b2_cylinder_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let surface_records = plane_records(1);
        let mut surfaces = [Some(coarse_surface("sphere"))];
        let refined = crate::test_support::with_collection_limit(0, |ctx| {
            refine_consolidated_analytic_surfaces(
                ctx,
                &bytes,
                &records,
                &mut surfaces,
                &surface_records,
            )
        })
        .expect("no cylinder surface needs the cylinder index");
        assert!(refined.is_empty());
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
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    if ir.model.vertices.is_empty() {
        return Ok(());
    }
    let body_id = standard_id(
        ctx,
        "body",
        format_args!("unbound-points"),
        BodyId::mint,
        "catia_standard_free_body_identity",
    )?;
    let region_id = standard_id(
        ctx,
        "region",
        format_args!("unbound-points"),
        RegionId::mint,
        "catia_standard_free_region_identity",
    )?;
    let shell_id = standard_id(
        ctx,
        "shell",
        format_args!("unbound-points"),
        ShellId::mint,
        "catia_standard_free_shell_identity",
    )?;
    admission.reserve_entity(&mut ir.model.shells, "catia_standard_free_vertex_shells")?;
    let mut free_vertices = Vec::new();
    for vertex in ctx.admit_iter(&ir.model.vertices, "catia_standard_iteration")? {
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
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    surfaces: &mut Vec<Surface>,
    procedural_supports: &mut HashMap<u32, usize>,
    map_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    surface_object_id: u32,
    geometry: SurfaceGeometry,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<SurfaceId, cadmpeg_core::CodecError> {
    let ctx = admission.context();
    if let Some(&index) = ctx.get_hash_map(
        procedural_supports,
        &surface_object_id,
        "catia_extrusion_support_lookup",
    )? {
        return surfaces[index]
            .id
            .try_clone_for_decode(ctx, "catia_extrusion_existing_support_id_copy");
    }
    let source_object = cgm_source(ctx, "surface", surface_object_id)?;
    let id = crate::resource::compose_u32_id(
        ctx,
        &cadmpeg_ir::identity_namespace!("catia", "standard", "procedural-support"),
        surface_object_id,
        SurfaceId::mint,
        "catia_extrusion_support_surface_id",
    )?;
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
    map_storage.with_storage(|| {
        ctx.insert_hash_map(
            procedural_supports,
            surface_object_id,
            surfaces.len() - 1,
            "catia_extrusion_support_map",
        )
    })?;
    Ok(id)
}

/// Emit one resolved object-stream extrusion construction in the standard family.
fn emit_standard_extrusion_definition(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    (surfaces, procedural_supports, map_storage): (
        &mut Vec<Surface>,
        &mut HashMap<u32, usize>,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ),
    extrusion_definitions: &mut HashMap<
        u32,
        cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction,
    >,
    extrusion: crate::families::b5::transfer::ResolvedExtrusionSurface,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<ProceduralSurfaceDefinition, cadmpeg_core::CodecError> {
    if let Some(definition) = ctx.get_hash_map(
        extrusion_definitions,
        &extrusion.surface_object_id,
        "catia_extrusion_definition_lookup",
    )? {
        return copy_standard_extrusion_definition(ctx, definition);
    }
    let surface_object_id = extrusion.surface_object_id;
    let directrix_id = map_storage.with_storage(|| {
        crate::resource::compose_u32_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "standard", "extrusion-directrix"),
            extrusion.directrix_object_id,
            CurveId::mint,
            "catia_extrusion_directrix_identity",
        )
    })?;
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
                        annotations,
                        surfaces,
                        procedural_supports,
                        map_storage,
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
            let procedure_id = crate::resource::compose_u32_id(
                ctx,
                &cadmpeg_ir::identity_namespace!(
                    "catia",
                    "standard",
                    "extrusion-directrix-procedure"
                ),
                extrusion.directrix_object_id,
                ProceduralCurveId::mint,
                "catia_extrusion_directrix_procedure_identity",
            )?;
            annotate(
                ctx,
                annotations,
                &procedure_id,
                "object_stream_a8_03_25",
                0,
                "two_surface_pcurve_intersection",
                Exactness::ByteExact,
            )?;
            admission.charge()?;
            let owner =
                directrix_id.try_clone_for_decode(ctx, "catia_extrusion_directrix_owner_id")?;

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
            let _attached = ir.model.add_procedural_curve(ctx, &owner, procedure)?;
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
            let source_id = crate::resource::compose_u32_id(
                ctx,
                &cadmpeg_ir::identity_namespace!("catia", "standard", "extrusion-directrix-source"),
                source_object_id,
                CurveId::mint,
                "catia_extrusion_source_identity",
            )?;
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
            let procedure_id = crate::resource::compose_u32_id(
                ctx,
                &cadmpeg_ir::identity_namespace!(
                    "catia",
                    "standard",
                    "extrusion-directrix-procedure"
                ),
                extrusion.directrix_object_id,
                ProceduralCurveId::mint,
                "catia_extrusion_directrix_procedure_identity",
            )?;
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
                annotations,
                surfaces,
                procedural_supports,
                map_storage,
                support.surface_object_id,
                support.surface,
                admission,
            )?;
            admission.charge()?;

            let owner =
                directrix_id.try_clone_for_decode(ctx, "catia_extrusion_offset_owner_id")?;
            let _attached = ir.model.add_procedural_curve(ctx, &owner, ProceduralCurve::new(
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
    map_storage.with_storage(|| {
        ctx.insert_hash_map(
            extrusion_definitions,
            surface_object_id,
            definition,
            "catia_extrusion_definitions",
        )
    })?;
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

fn standard_freeform_e5_carrier_ids<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    data: &[u8],
) -> Result<
    (
        HashMap<u32, u32>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "catia_e5_face_carriers")?;
    let carriers = storage.with_storage(|| {
        let mut face_surfaces = BTreeMap::<u32, Option<u32>>::new();
        // The record scan admits every byte it can visit before the first face.
        for (face, surface) in crate::families::e5::graph::face_surface_references(ctx, data)? {
            if let Some(stored) =
                ctx.get_mut_btree_map(&mut face_surfaces, &face, "catia_e5_face_surfaces")?
            {
                if *stored != Some(surface) {
                    *stored = None;
                }
            } else {
                ctx.insert_btree_map(
                    &mut face_surfaces,
                    face,
                    Some(surface),
                    "catia_e5_face_surfaces",
                )?;
            }
        }

        let mut wrappers = HashMap::<u32, Option<u32>>::new();
        let e5_wrappers = crate::families::e5::records::e5_surface_wrappers(ctx, data)?;
        for wrapper in ctx.admit_iter(&e5_wrappers, "catia_standard_e5_surface_wrappers")? {
            let surface = wrapper.underlying_surface();
            if let Some(stored) = ctx.get_mut_hash_map(
                &mut wrappers,
                &wrapper.record_id,
                "catia_e5_wrapper_surfaces",
            )? {
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
        for (&face, &wrapper) in
            ctx.admit_iter(&face_surfaces, "catia_standard_e5_face_surfaces")?
        {
            let Some(wrapper) = wrapper else {
                continue;
            };
            let Some(&Some(surface)) =
                ctx.get_hash_map(&wrappers, &wrapper, "catia_e5_wrapper_surfaces")?
            else {
                continue;
            };
            ctx.insert_hash_map(&mut carriers, face, surface, "catia_e5_face_carriers")?;
        }
        Ok::<_, CodecError>(carriers)
    })?;
    Ok((carriers, storage))
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
    carrier_ids: &HashMap<u32, u32>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<BTreeMap<u32, SurfaceGeometry>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "catia_e5_surface_carrier_scratch")?;
    let surfaces = storage.with_storage(|| {
        let mut surfaces = HashMap::<u32, Option<SurfaceGeometry>>::new();
        let e5_surfaces = crate::families::e5::records::e5_surfaces(ctx, data, refusal)?;
        for surface in ctx.admit_iter(e5_surfaces, "catia_e5_surface_carriers")? {
            if let Some(stored) = ctx.get_mut_hash_map(
                &mut surfaces,
                &surface.record_id,
                "catia_e5_surface_carriers",
            )? {
                // Surface geometry has no decode cost: this comparison is unpriced.
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
        Ok::<_, CodecError>(surfaces)
    })?;
    let mut associated = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_standard_e5_surface_records")? {
        let crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } =
            record
        else {
            continue;
        };
        let Some(carrier) = ctx.get_hash_map(carrier_ids, tag, "catia_e5_face_carrier_lookup")?
        else {
            continue;
        };
        let Some(Some(geometry)) =
            ctx.get_hash_map(&surfaces, carrier, "catia_e5_surface_carrier_lookup")?
        else {
            continue;
        };
        let copied = geometry.try_clone_for_decode(ctx, "catia_e5_surface_geometry_copy")?;
        ctx.insert_btree_map(
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
                        ) => geometry
                            .try_clone_for_decode(ctx, "catia_standard_offset_support_copy")?,
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
    carrier_ids: &HashMap<u32, u32>,
    decoded_jets: &[crate::families::e5::records::E5RollingBallJet],
) -> Result<BTreeMap<u32, StandardSurfaceProcedure>, CodecError> {
    // Each decoded jet is a distinct framed record, so a repeated record id
    // names two different carriers and keeps a tombstone.
    let (jets, _jet_storage) = ctx.unique_index(
        decoded_jets.iter().map(|jet| (jet.record_id, jet)),
        "catia_e5_rolling_ball_carriers",
    )?;
    let mut associated = BTreeMap::new();
    for record in ctx.admit_iter(records, "catia_standard_e5_rolling_ball_records")? {
        let crate::families::standard::records::StandardSurfaceRecord::Freeform {
            tag,
            forward,
            ..
        } = record
        else {
            continue;
        };
        let Some(carrier) = ctx.get_hash_map(carrier_ids, tag, "catia_e5_face_carrier_lookup")?
        else {
            continue;
        };
        let Some(&Some(jet)) =
            ctx.get_hash_map(&jets, carrier, "catia_e5_rolling_ball_carrier_lookup")?
        else {
            continue;
        };
        if *forward != (jet.sense == crate::families::e5::graph::Sign::Negative) {
            continue;
        }
        let Some(definition) = jet.definition(ctx)? else {
            continue;
        };
        ctx.insert_btree_map(
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
        crate::families::standard::records::pair_standard_populations(ctx, &layouts, populations)?
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
    let Some(rest) = ctx.collect_fallible_options(
        pairs.rest.into_iter().map(select),
        "catia_standard_population_selections",
    )?
    else {
        return Ok(None);
    };
    Ok(Some((first, rest)))
}

pub(in crate::families) fn try_decode_standard(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FamilyOutput>, cadmpeg_core::CodecError> {
    let mut metadata_storage = ctx.reserve_scoped(0, "catia_standard_route_metadata")?;
    let (surface_alias_tags, e5_jets, selections) = metadata_storage.with_storage(|| {
        let surface_alias_tags = if matches!(scan.variant, Variant::StandardNested) {
            crate::object_graph::surface_alias_tag_map(ctx, &scan.data)?
        } else {
            HashMap::new()
        };
        let e5_jets = crate::families::e5::records::e5_rolling_ball_jets(ctx, &scan.data)?;
        let selections = standard_population_selections(ctx, scan)?;
        Ok::<_, CodecError>((surface_alias_tags, e5_jets, selections))
    })?;
    match selections {
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

fn retain_standard_population_model(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
) -> Result<(), CodecError> {
    const OPERATION: &str = "catia_standard_population_retain";
    macro_rules! retain_standard {
        ($($field:ident),+ $(,)?) => {
            $(ctx.retain_vec(
                &mut model.$field,
                |entity| Ok(entity.identity().starts_with("catia:standard:")),
                OPERATION,
            )?;)+
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
    Ok(())
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

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, Self::Error> {
        cadmpeg_ir::schema::rewrite::identities(
            self.ctx,
            "catia_standard_population_rewrite",
            entity,
            |id: &str| rescope_standard_id(self.ctx, id, self.scope),
        )
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
    let mut keep = |id: &str| Ok(id.starts_with("catia:standard:"));
    source.retain_provenance(ctx, &mut keep)?;
    let mut annotations = AnnotationBuilder::resume(source);
    annotations.retain_exactness(ctx, keep)?;
    source = annotations.build();
    if let Err(collision) = source.map_ids(
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
    target.append(ctx, source, "catia_standard_population_annotation_append")
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
    {
        let mut visits = rest.iter();
        while let Some(selection) =
            ctx.next_charged(&mut visits, "catia_standard_population_selections")?
        {
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
    }
    let merged_output = [&merged];
    let coverage_total = |key: &str| -> Result<usize, CodecError> {
        let mut total = 0usize;
        for output in merged_output
            .iter()
            .copied()
            .chain(ctx.admit_iter(&outputs, "catia_standard_population_coverage")?)
        {
            total += ctx
                .get_btree_map(
                    &output.report.coverage,
                    key,
                    "catia_standard_population_coverage",
                )?
                .copied()
                .unwrap_or_default();
        }
        Ok(total)
    };
    let attached_topology_count = coverage_total("attached_standard_topology_count")?;
    let coverage_keys = [
        crate::coverage::ATTEMPTED_STANDARD_TOPOLOGY_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_CURVE_SUPPORT_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_NATIVE_ENDPOINT_PAIR_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_EMPTY_ENDPOINT_DOMAIN_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_SINGLETON_ENDPOINT_DOMAIN_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_MULTIPLE_ENDPOINT_DOMAIN_COUNT,
        crate::coverage::STANDARD_TOPOLOGY_ENDPOINT_DOMAIN_CHOICE_COUNT,
    ];
    let mut population_coverage = [(coverage_keys[0], 0); 7];
    for (index, key) in coverage_keys.into_iter().enumerate() {
        population_coverage[index] = (key, coverage_total(key.as_str())?);
    }
    let population_count = 1 + rest.len();
    let first_selection = [first];
    let admitted_face_rows = ctx
        .admit_iter(&first_selection, "catia_standard_population_face_rows")?
        .copied()
        .chain(ctx.admit_iter(rest, "catia_standard_population_face_rows")?)
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
        retain_standard_population_model(ctx, &mut model)?;
        let mut rewriter = StandardPopulationScope { scope: &scope, ctx };
        match merged
            .ir
            .model
            .extend_rewritten(
                ctx,
                model,
                &mut rewriter,
                "catia_standard_population_model_merge",
            )
            .map_err(CodecError::from)
        {
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
        merged.report.coverage.record(ctx, key, value)?;
    }
    merged.report.coverage.record(
        ctx,
        crate::coverage::STANDARD_FBB_RUN_COUNT,
        scan.census.fbb_runs,
    )?;
    merged.report.coverage.record(
        ctx,
        crate::coverage::STANDARD_FBB_CANDIDATE_FACE_ROW_COUNT,
        scan.census.fbb_face_rows,
    )?;
    merged.report.coverage.record(
        ctx,
        crate::coverage::STANDARD_FBB_ADMITTED_FACE_ROW_COUNT,
        admitted_face_rows,
    )?;
    merged.report.coverage.record(
        ctx,
        crate::coverage::STANDARD_FBB_WITHHELD_FACE_ROW_COUNT,
        scan.census.fbb_face_rows - scan.census.fbb_face_rows.min(admitted_face_rows),
    )?;
    merged.report.coverage.record(
        ctx,
        crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT,
        attached_topology_count,
    )?;

    ctx.retain_vec(
        &mut merged.report.losses,
        |loss| {
            Ok(!matches!(
                loss.code.local_code(),
                "topology.fbb-rows-withheld"
                    | "geometry.carrier-summary"
                    | "geometry.unresolved-carriers"
            ))
        },
        "catia_standard_merged_losses",
    )?;
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
    for surface in ctx.admit_iter(&merged.ir.model.surfaces, "catia_standard_iteration")? {
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
                Err(error) => return Some(Err(error.into())),
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
    let mut setup_storage = admitted!(ctx.reserve_scoped(0, "catia_standard_population_setup"));
    let (consolidated_records, points, vertex_roster, face_count, records, analytic_record_count, curve_supports, object_evidence, standard_limit_curve_count, revolution_record_count, curved_surfaces, refined_analytic_surfaces, planes, face_bounds, freeform_geometries, e5_freeform_tags, freeform_procedural_surfaces, unresolved_freeform_record_count) = admitted!(setup_storage.with_storage(|| {
        (|| -> Option<Result<_, CodecError>> {
    let sources = match container::consolidated_record_sources(ctx, scan) {
        Ok(sources) => sources,
        Err(error) => return Some(Err(error)),
    };
    let consolidated_records = match crate::wire::records::consolidated_records_in_sources(
        ctx,
        &scan.data,
        admitted!(ctx
            .admit_iter(&sources, "catia_standard_consolidated_record_source_visits")
            .map_err(CodecError::from)),
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
        match ctx.copy_slice(&selection.records, "catia_standard_selected_records") {
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
            for prefix in admitted!(ctx.admit_iter(&prefixes, "catia_standard_analytic_record_prefixes")) {
                if let Err(error) = ctx.push_vec(&mut records, crate::families::standard::records::StandardSurfaceRecord::Analytic(*prefix), "catia_standard_analytic_records") {
                    return Some(Err(error));
                }
            }
            records
        }
    };
    let analytic_record_count = admitted!(ctx.admit_iter(&records, "catia_standard_analytic_record_count"))
        .filter(|record| {
            matches!(
                record,
                crate::families::standard::records::StandardSurfaceRecord::Analytic(_)
            )
        })
        .count();
    let mut freeform_tags = BTreeSet::new();
    for record in admitted!(ctx.admit_iter(&records, "catia_standard_freeform_tags")) {
        if let crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } = record {
            if let Err(error) = ctx.insert_btree_set(&mut freeform_tags, *tag, "catia_standard_freeform_tags") {
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
        match ctx.copy_slice(&selection.supports, "catia_standard_selected_supports") {
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
    for support in admitted!(ctx.admit_iter(&curve_supports, "catia_standard_edge_tags")) {
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
    let revolution_records = admitted!(crate::families::b2::records::b2_revolutions_from_records(
        ctx,
        &scan.data,
        &consolidated_records,
    ));
    let revolution_record_count = revolution_records.count();
    let face_frame_vectors = match fbb::standard_face_frame_vectors(ctx, standard_spine, records.len()) {
        Ok(vectors) => vectors,
        Err(error) => return Some(Err(error)),
    };
    let mut curved_surfaces = Vec::new();
    for record in admitted!(ctx.admit_iter(&records, "catia_standard_curved_surfaces")) {
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
        &records,
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
    let planes = admitted!(ctx.collect_hash_map(
        plane_rows
            .into_iter()
            .map(|plane| (plane.target, plane)),
        "catia_plane_param_map"
    ));
    let mut face_bounds = Vec::new();
    for record in admitted!(ctx.admit_iter(&records, "catia_standard_face_bounds")) {
        let bounds = crate::families::standard::records::standard_face_bounds(brep, record);
        if let Err(error) = ctx.push_vec(&mut face_bounds, bounds, "catia_standard_face_bounds") {
            return Some(Err(error));
        }
    }
    let mut freeform_geometries = std::mem::take(&mut object_evidence.surface_geometries);
    let (e5_carrier_ids, _e5_carrier_storage) = match standard_freeform_e5_carrier_ids(ctx, &scan.data) {
        Ok(carriers) => carriers,
        Err(error) => return Some(Err(error)),
    };
    let e5_freeform_geometries = match associate_standard_freeform_e5_surfaces(ctx, &records, &scan.data, &e5_carrier_ids, refusal) {
        Ok(geometries) => geometries,
        Err(error) => return Some(Err(error)),
    };
    let mut e5_freeform_tags = HashSet::new();
    for (tag, geometry) in admitted!(ctx.admit_iter(e5_freeform_geometries, "catia_standard_e5_freeform_geometries")) {
        if let Err(error) = ctx.insert_hash_map(&mut freeform_geometries, tag, geometry, "catia_standard_e5_freeform_geometries") {
            return Some(Err(error));
        }
        if let Err(error) = ctx.insert_hash_set(&mut e5_freeform_tags, tag, "catia_standard_e5_freeform_tags") {
            return Some(Err(error));
        }
    }
    let mut freeform_procedural_surfaces = std::mem::take(&mut object_evidence.procedural_surfaces);
    let e5_freeform_procedural_surfaces = match associate_standard_freeform_e5_rolling_ball_jets(ctx, &records, &e5_carrier_ids, e5_jets) {
        Ok(procedures) => procedures,
        Err(error) => return Some(Err(error)),
    };
    for (tag, procedure) in admitted!(ctx.admit_iter(e5_freeform_procedural_surfaces, "catia_standard_e5_freeform_procedures")) {
        // Procedures hold surface geometry, which has no decode cost: this
        // comparison is unpriced.
        match admitted!(ctx.get_hash_map(&freeform_procedural_surfaces, &tag, "catia_standard_e5_freeform_procedure_lookup")) {
            Some(existing) if existing != &procedure => {
                admitted!(ctx.remove_hash_map(&mut freeform_procedural_surfaces, &tag, "catia_standard_e5_freeform_procedure_lookup"));
            }
            Some(_) => {}
            None => {
                if let Err(error) = ctx.insert_hash_map(&mut freeform_procedural_surfaces, tag, procedure, "catia_standard_e5_freeform_procedures") {
                    return Some(Err(error));
                }
            }
        }
    }
    let mut unresolved_freeform_record_count = 0usize;
    for record in admitted!(ctx.admit_iter(&records, "catia_standard_unresolved_freeform_records")) {
        let crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } = record else {
            continue;
        };
        if !admitted!(ctx.contains_key_hash_map(&freeform_geometries, tag, "catia_standard_unresolved_freeform_records"))
            && !admitted!(ctx.contains_key_hash_map(&freeform_procedural_surfaces, tag, "catia_standard_unresolved_freeform_records"))
        {
            unresolved_freeform_record_count += 1;
        }
    }
    if points.is_empty() && records.is_empty() {
        return None;
    }

            Some(Ok((consolidated_records, points, vertex_roster, face_count, records, analytic_record_count, curve_supports, object_evidence, standard_limit_curve_count, revolution_record_count, curved_surfaces, refined_analytic_surfaces, planes, face_bounds, freeform_geometries, e5_freeform_tags, freeform_procedural_surfaces, unresolved_freeform_record_count)))
        })().transpose()
    }))?;
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
    for (i, record) in admitted!(ctx.admit_iter(&records, "catia_standard_surface_rows")).enumerate() {
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
                let geometry = admitted!(admitted!(ctx.get_hash_map(&freeform_geometries, tag, "catia_standard_freeform_geometry_lookup"))
                    .map(|geometry| geometry.try_clone_for_decode(ctx, "catia_e5_surface_geometry_copy")).transpose())
                    .unwrap_or(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: None,
                    }));
                admitted!(setup_storage.with_storage(|| ctx.push_vec(&mut face_bindings, ((id.try_clone_for_decode(ctx, "catia_standard_face_binding_surface_id"))?, *forward, *pos), "catia_standard_face_bindings")));
                admitted!(setup_storage.with_storage(|| ctx.push_vec(&mut surface_annotations, (
                    (id.try_clone_for_decode(ctx, "catia_standard_annotation_surface_id"))?,
                    "MainDataStream+SurfacicReps",
                    *pos,
                    (ctx.copy_retained_text("surfacic_reps_freeform_alias", "catia_standard_surface_annotation_tag"))?,
                    if (ctx.contains_key_hash_map(&freeform_procedural_surfaces, tag, "catia_standard_freeform_procedure_lookup"))?
                        || (ctx.contains_hash_set(&e5_freeform_tags, tag, "catia_standard_e5_freeform_tags"))?
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
                ), "catia_standard_surface_annotations")));
                if let Err(error) = admission.reserve_entity(&mut surfaces, "catia_family_emit_surfaces") {
                    return Some(Err(error));
                }
                let surface_index = surfaces.len();
                surfaces.push(Surface {
                    id,
                    geometry,
                    source_object: Some(admitted!(cgm_source(ctx, "carrier", *tag))),
                });
                if let Some(procedure) = admitted!(admitted!(ctx.get_hash_map(&freeform_procedural_surfaces, tag, "catia_standard_freeform_procedure_lookup"))
                    .map(|procedure| copy_standard_procedure(ctx, procedure)).transpose()) {
                    admitted!(ctx.push_vec(&mut procedural_surface_plans, (i, surface_index, *tag, procedure), "catia_standard_procedural_surface_plans"));
                }
                continue;
            }
        };
        // A bridged plane parameter record contains the same `00 33 32`
        // marker as its SurfacicReps carrier.  One carrier exists per tag.
        if prefix.kind == AnalyticSurfaceKind::Plane
            && !admitted!(setup_storage.with_storage(|| ctx.insert_hash_set(&mut decoded_plane_targets, prefix.target, "catia_standard_decoded_plane_targets")))
        {
            continue;
        }
        let decoded = if prefix.kind == AnalyticSurfaceKind::Plane {
            admitted!(ctx.get_hash_map(&planes, &prefix.target, "catia_plane_param_lookup"))
                .and_then(crate::families::standard::records::decode_plane)
        } else {
            admitted!(curved_surfaces[i].as_ref().map(|surface| surface.try_clone_for_decode(ctx, "catia_standard_curved_surface_copy")).transpose())
        };
        match decoded {
            Some(geom) => {
                typed.record(&geom);
                let id = admitted!(crate::resource::compose_index_id(ctx,
                    &cadmpeg_ir::identity_namespace!("catia", "standard", "surf"),
                    i, SurfaceId::mint, "catia_standard_surface_id"));
                if let Some(forward) = crate::families::standard::records::face_sense(brep, prefix)
                {
                    admitted!(setup_storage.with_storage(|| ctx.push_vec(&mut face_bindings, ((id.try_clone_for_decode(ctx, "catia_standard_face_binding_surface_id"))?, forward, prefix.pos), "catia_standard_face_bindings")));
                }
                let (annotation_stream, annotation_offset, annotation_tag) =
                    if let Some(source_pos) = admitted!(ctx.get_hash_map(&refined_analytic_surfaces, &i, "catia_standard_refined_surface_lookup")) {
                        ("consolidated_b2_03", *source_pos,
                            admitted!(setup_storage.with_storage(|| ctx.copy_retained_text("consolidated_exact_analytic_surface", "catia_standard_surface_annotation_tag"))))
                    } else {
                        ("MainDataStream+SurfacicReps", prefix.pos,
                            admitted!(setup_storage.with_storage(|| ctx.format_retained(format_args!("surfacic_reps_{:02x}", prefix.kind.marker()), "catia_standard_surface_annotation_tag"))))
                    };
                admitted!(setup_storage.with_storage(|| ctx.push_vec(&mut surface_annotations, (
                    (id.try_clone_for_decode(ctx, "catia_standard_annotation_surface_id"))?,
                    annotation_stream,
                    annotation_offset,
                    annotation_tag,
                    Exactness::ByteExact,
                ), "catia_standard_surface_annotations")));
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
                    admitted!(setup_storage.with_storage(|| ctx.push_vec(&mut face_bindings, ((id.try_clone_for_decode(ctx, "catia_standard_face_binding_surface_id"))?, forward, prefix.pos), "catia_standard_face_bindings")));
                }
                admitted!(setup_storage.with_storage(|| ctx.push_vec(&mut surface_annotations, (
                    (id.try_clone_for_decode(ctx, "catia_standard_annotation_surface_id"))?,
                    "MainDataStream+SurfacicReps",
                    prefix.pos,
                    (ctx.format_retained(format_args!("surfacic_reps_{:02x}", prefix.kind.marker()), "catia_standard_surface_annotation_tag"))?,
                    Exactness::Unknown,
                ), "catia_standard_surface_annotations")));
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

    let mut procedural_storage = match ctx.reserve_scoped(0,"catia_standard_procedural_working_state") { Ok(storage) => storage, Err(error) => return Some(Err(error)) };
    let mut procedural_supports = HashMap::<u32, usize>::new();
    let mut extrusion_definitions = HashMap::new();
    for (index, surface_index, tag, procedure) in admitted!(ctx.admit_iter(procedural_surface_plans, "catia_standard_procedural_surface_plans")) {
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
                        if let Some(&index) = admitted!(ctx.get_hash_map(&procedural_supports, &support_object_id, "catia_standard_procedural_support_lookup")) {
                            admitted!(surfaces[index].id.try_clone_for_decode(ctx, "catia_standard_existing_support_id"))
                        } else {
                            let source_object = admitted!(cgm_source(ctx, "surface", support_object_id));
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
                            admitted!(procedural_storage.with_storage(|| ctx.insert_hash_map(&mut procedural_supports, support_object_id, surfaces.len() - 1, "catia_standard_procedural_supports")));
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
                        // Supports share one identity namespace, so a support
                        // object emitted earlier owns the first surface with
                        // this identity; otherwise the surface pushed here does.
                        let earlier_support = admitted!(ctx.get_hash_map(&procedural_supports, &support_object_id, "catia_standard_procedural_support_lookup")).copied();
                        let pushed_index = surfaces.len();
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
(&mut surfaces, &mut procedural_supports,&mut procedural_storage),
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
                        let support_index = earlier_support.unwrap_or(pushed_index);
                        let attached = if let Some(surface) =
                            surfaces.get_mut(support_index)
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
                        admitted!(procedural_storage.with_storage(|| ctx.insert_hash_map(&mut procedural_supports, support_object_id, support_index, "catia_standard_procedural_supports")));
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
(&mut surfaces, &mut procedural_supports,&mut procedural_storage),
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
                admitted!(crate::resource::derived_annotation(ctx, &mut annotations, &directrix_id, "geometry"));
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
        let attached = if let Some(surface_record) = surfaces.get_mut(surface_index)
        {
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

    for (i, p) in admitted!(ctx.admit_iter(&points, "catia_standard_point_rows")).enumerate() {
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
        admitted!(crate::resource::derived_annotation(ctx, &mut annotations, &vertex_id, "point"));
        if let Err(error) = admission.reserve_entity(&mut ir.model.vertices, "catia_family_emit_vertices") {
            return Some(Err(error));
        }
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (id, stream, offset, tag, exactness) in
        admitted!(ctx.admit_iter(&surface_annotations, "catia_standard_surface_annotations"))
    {
        admitted!(annotate(
            ctx,
            &mut annotations,
            id,
            stream,
            u64_from_index(*offset),
            tag,
            *exactness,
        ));
    }
    let mut topology_ir = std::mem::replace(&mut ir, CadIr::empty());
    let mut topology_annotations = admitted!(annotations.copy_transaction(ctx, "catia_standard_topology_annotations"));
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
    let topology_result = attach_standard_topology(ctx, crate::families::standard::decode::AttachStandardTopologyInputs { ir: &mut topology_ir, annotations: &mut topology_annotations, bindings: &face_bindings, records: &records, face_bounds: &face_bounds, spine: standard_spine, edge_table_form, brep, support_override: selection.map(|selection| selection.supports.as_slice()), source: &scan.data, e5_record_range: scan.e5_record_range.clone(), use_vertex_roster: selection.is_none_or(|selection| selection.vertex_roster_compatible), native_edge_faces: &object_evidence.edge_owner_faces, native_edge_supports: &object_evidence.edge_supports, limit_curves: &object_evidence.limit_curves, work_budget: &topology_budget, diagnostics: &mut topology_diagnostics, bound_limit_curve_count: &mut bound_standard_limit_curve_count, refusal, admission: &mut admission })
    .and_then(|()| {
        neutral_model_is_admissible(ctx, &mut topology_ir, &unknowns).map_err(StandardTopologyError::Resource)?
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
        annotations = admitted!(topology_annotations.into_retained());
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
        admitted!(ctx.retain_vec(
            &mut topology_ir.model.surfaces,
            |surface| {
                Ok(!surface.id.as_str().starts_with("catia:standard:edge-support-surface#"))
            },
            "catia_standard_fallback_surfaces",
        ));
        admitted!(ctx.retain_vec(
            &mut topology_ir.model.procedural_surfaces,
            |surface| {
                Ok(!surface.id.as_str().starts_with("catia:standard:edge-support-definition#"))
            },
            "catia_standard_fallback_procedural_surfaces",
        ));
        ir = topology_ir;
        let fallback_result = (|| -> Result<(), cadmpeg_core::CodecError> {
            attach_standard_circles(
                ctx,
                &mut ir,
                &mut annotations,
                &face_bindings,
                &curve_supports,
                &mut admission,
            )?;
            attach_standard_lines(
                ctx,
                &mut ir,
                &mut annotations,
                &face_bindings,
                &curve_supports,
                &mut admission,
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
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::ATTEMPTED_STANDARD_TOPOLOGY_COUNT,
        usize::from(true),
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_FBB_RUN_COUNT,
        scan.census.fbb_runs,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_FBB_CANDIDATE_FACE_ROW_COUNT,
        scan.census.fbb_face_rows,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_FBB_ADMITTED_FACE_ROW_COUNT,
        face_count,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_FBB_WITHHELD_FACE_ROW_COUNT,
        withheld_face_rows,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::ATTACHED_STANDARD_TOPOLOGY_COUNT,
        usize::from(topology_attached),
    ) { return Some(Err(error)); }
    for failure in StandardTopologyFailure::ALL {
        if let Err(error) = report.coverage.record(ctx,
            failure.coverage_key(),
            usize::from(topology_failure == Some(failure)),
        ) { return Some(Err(error)); }
    }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_CURVE_SUPPORT_COUNT,
        topology_diagnostics.curve_supports,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_NATIVE_ENDPOINT_PAIR_COUNT,
        topology_diagnostics.native_endpoint_pairs,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_EMPTY_ENDPOINT_DOMAIN_COUNT,
        topology_diagnostics.empty_endpoint_domains,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_SINGLETON_ENDPOINT_DOMAIN_COUNT,
        topology_diagnostics.singleton_endpoint_domains,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_MULTIPLE_ENDPOINT_DOMAIN_COUNT,
        topology_diagnostics.multiple_endpoint_domains,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_ENDPOINT_DOMAIN_CHOICE_COUNT,
        topology_diagnostics.endpoint_domain_choices,
    ) { return Some(Err(error)); }
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
        if let Err(error) = report.coverage.record(ctx,
            key,
            usize::from(
                topology_diagnostics.mesh_failure
                    == Some(mesh_quotient::MeshCandidateFailure::Rejected(rejection)),
            ),
        ) { return Some(Err(error)); }
    }
    let endpoint_incidence_rejection = match topology_diagnostics.mesh_failure {
        Some(mesh_quotient::MeshCandidateFailure::Rejected(
            mesh_quotient::MeshCandidateRejection::EndpointIncidence(rejection),
        )) => Some(rejection),
        _ => None,
    };
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_COUNT,
        usize::from(endpoint_incidence_rejection.is_some()),
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_NO_ASSIGNMENT_COUNT,
        usize::from(matches!(
            endpoint_incidence_rejection,
            Some(mesh_quotient::MeshEndpointIncidenceRejection::NoAssignment(
                _
            ))
        )),
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_MESH_REJECTION_ENDPOINT_INCIDENCE_BOUNDARY_RECONSTRUCTION_COUNT,
        usize::from(
            endpoint_incidence_rejection
                == Some(mesh_quotient::MeshEndpointIncidenceRejection::BoundaryReconstruction),
        ),
    ) { return Some(Err(error)); }
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
        if let Err(error) = report
            .coverage
            .record(ctx, key, usize::from(incidence_rejection == Some(rejection))) { return Some(Err(error)); }
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
        if let Err(error) = report.coverage.record(ctx,
            key,
            usize::from(
                topology_diagnostics.mesh_failure
                    == Some(mesh_quotient::MeshCandidateFailure::Ambiguous(ambiguity)),
            ),
        ) { return Some(Err(error)); }
    }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::STANDARD_TOPOLOGY_MESH_EXHAUSTION_QUOTIENT_PREPARATION_COUNT,
        0,
    ) { return Some(Err(error)); }
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
        if let Err(error) = report.coverage.record(ctx,
            key,
            usize::from(
                topology_diagnostics.mesh_failure
                    == Some(mesh_quotient::MeshCandidateFailure::Exhausted(exhaustion)),
            ),
        ) { return Some(Err(error)); }
    }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::REFINED_CONSOLIDATED_ANALYTIC_SURFACE_COUNT,
        refined_analytic_surfaces.len(),
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::DECODED_STANDARD_LIMIT_CURVE_COUNT,
        standard_limit_curve_count,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::BOUND_STANDARD_LIMIT_CURVE_COUNT,
        bound_standard_limit_curve_count,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::BOUND_CONSOLIDATED_REVOLUTION_FACE_SURFACE_COUNT,
        bound_revolution_face_surface_count,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::RESOLVED_CONSOLIDATED_REVOLUTION_SEAM_CURVE_COUNT,
        resolved_revolution_seam_curve_count,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::BOUND_CONSOLIDATED_STANDARD_EDGE_COUNT,
        consolidated_curve_bindings.standard_edges,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::BOUND_CONSOLIDATED_PARTNER_SUPPORT_COUNT,
        consolidated_curve_bindings.partner_supports,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::BOUND_CONSOLIDATED_PARTNER_FACE_PCURVE_PAIR_COUNT,
        consolidated_curve_bindings.partner_face_pcurve_pairs,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::BOUND_CONSOLIDATED_STANDARD_FACE_SURFACE_COUNT,
        consolidated_curve_bindings.standard_face_surfaces,
    ) { return Some(Err(error)); }
    if let Err(error) = report.coverage.record(ctx,
        crate::coverage::BOUND_CONSOLIDATED_STANDARD_FACE_PCURVE_COUNT,
        consolidated_curve_bindings.standard_face_pcurves,
    ) { return Some(Err(error)); }
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
    pub(super) edge_owner_faces: HashMap<u32, Vec<u32>>,
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
    surfaces: BTreeMap<u32, Vec<usize>>,
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
        let stored = ctx
            .get_hash_map(
                &self.supports,
                &object_id,
                "catia_standard_support_candidates",
            )?
            .copied();
        match stored {
            Some(None) => {}
            Some(Some(stored)) => {
                let same = self
                    .support_at(stored)
                    .zip(self.support_at(incoming))
                    .is_some_and(|(left, right)| left.equivalent(&right));
                if !same {
                    ctx.insert_hash_map(
                        &mut self.supports,
                        object_id,
                        None,
                        "catia_standard_support_candidates",
                    )?;
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
        ctx.push_btree_group(
            &mut self.surfaces,
            tag,
            index,
            "catia_standard_surface_candidates",
            "catia_standard_surface_candidate_evidence",
        )
    }

    /// Resolves the support recorded for an object id, treating an id with
    /// conflicting population bytes as unsupported.
    fn supported_ref(
        &self,
        ctx: &DecodeContext<'_>,
        conflicting_population_ids: &HashSet<u32>,
        object_id: u32,
    ) -> Result<Option<StandardSupportRef<'_>>, CodecError> {
        const OPERATION: &str = "catia_standard_procedure_support_lookup";
        if ctx.contains_hash_set(conflicting_population_ids, &object_id, OPERATION)? {
            return Ok(None);
        }
        Ok(ctx
            .get_hash_map(&self.supports, &object_id, OPERATION)?
            .copied()
            .flatten()
            .and_then(|location| self.support_at(location)))
    }

    fn procedure_is_supported(
        &self,
        ctx: &DecodeContext<'_>,
        conflicting_population_ids: &HashSet<u32>,
        procedure: &StandardSurfaceProcedure,
    ) -> Result<bool, CodecError> {
        match procedure {
            StandardSurfaceProcedure::Offset {
                support_object_id,
                support,
                ..
            } => Ok(self
                .supported_ref(ctx, conflicting_population_ids, *support_object_id)?
                .is_some_and(|candidate| {
                    candidate.equivalent(&StandardSupportRef::Offset(support))
                })),
            StandardSurfaceProcedure::Extrusion(extrusion) => ctx.all_by(
                extrusion.supports(),
                |side| {
                    Ok(self
                        .supported_ref(ctx, conflicting_population_ids, side.surface_object_id)?
                        .is_some_and(|candidate| {
                            candidate.equivalent(&StandardSupportRef::Geometry(&side.surface))
                        }))
                },
                "catia_standard_extrusion_support_check",
            ),
            StandardSurfaceProcedure::RollingBall { .. }
            | StandardSurfaceProcedure::Revolution(_) => Ok(true),
        }
    }

    fn into_outputs(
        mut self,
        ctx: &DecodeContext<'_>,
        conflicting_population_ids: &HashSet<u32>,
    ) -> StandardProcedureOutputs {
        let mut valid_procedure = ctx.alloc_filled(
            self.evidence.len(),
            false,
            "catia_standard_procedure_validity",
        )?;
        for (index, evidence) in ctx
            .admit_iter(&self.evidence, "catia_standard_iteration")?
            .enumerate()
        {
            if let Some(procedure) = evidence
                .as_ref()
                .and_then(StandardSurfaceEvidence::procedure_ref)
            {
                valid_procedure[index] =
                    self.procedure_is_supported(ctx, conflicting_population_ids, procedure)?;
            }
        }
        let mut surface_geometries = HashMap::new();
        let mut procedural_surfaces = HashMap::new();
        for (tag, indexes) in ctx.admit_iter(&self.surfaces, "catia_standard_surface_outputs")? {
            if ctx.contains_hash_set(
                conflicting_population_ids,
                tag,
                "catia_standard_conflicting_surface_filter",
            )? {
                continue;
            }
            let mut geometry = None;
            let mut procedure = None;
            let mut procedure_valid = false;
            let mut conflict = false;
            for &index in ctx.admit_iter(indexes, "catia_standard_surface_output_evidence")? {
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
                    *tag,
                    geometry,
                    "catia_standard_surface_geometries",
                )?;
            }
            if let Some(procedure) = procedure.filter(|_| procedure_valid) {
                ctx.insert_hash_map(
                    &mut procedural_surfaces,
                    *tag,
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
    tags: &BTreeSet<u32>,
    edge_tags: &HashSet<u32>,
    consolidated_records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<StandardObjectEvidence, cadmpeg_core::CodecError> {
    let streams = container::logical_record_streams(ctx, scan)?;
    let mut evidence =
        standard_object_evidence_from_streams(ctx, &streams.streams, tags, edge_tags, refusal)?;
    let mut limit_index = LimitCurveIndex::new(ctx, &evidence.limit_curves)?;
    merge_standard_limit_curves_from_records(
        ctx,
        &mut evidence.limit_curves,
        &mut limit_index,
        &scan.data,
        consolidated_records,
        refusal,
    )?;
    Ok(evidence)
}

struct LimitCurveIndex<'ctx> {
    by_fingerprint: HashMap<u64, Vec<usize>>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> LimitCurveIndex<'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>, curves: &[NurbsCurve]) -> Result<Self, CodecError> {
        let mut index = Self {
            by_fingerprint: HashMap::new(),
            storage: ctx.reserve_scoped(0, "catia_standard_limit_curve_index")?,
        };
        for (row, curve) in ctx
            .admit_iter(curves, "catia_standard_limit_curve_index")?
            .enumerate()
        {
            let key = edge_geometry::nurbs_curve_fingerprint(ctx, curve)?;
            index.insert(ctx, key, row)?;
        }
        Ok(index)
    }

    fn contains(
        &self,
        ctx: &DecodeContext<'_>,
        key: u64,
        curves: &[NurbsCurve],
        geometry: &NurbsCurve,
    ) -> Result<bool, CodecError> {
        const OP: &str = "catia standard limit curve dedup";
        let Some(bucket) = ctx.get_hash_map(&self.by_fingerprint, &key, OP)? else {
            return Ok(false);
        };
        // NurbsCurve has no DecodeCost; collision equality is unpriced.
        ctx.any_by(bucket, |&row| Ok(curves[row] == *geometry), OP)
    }

    fn insert(&mut self, ctx: &DecodeContext<'_>, key: u64, row: usize) -> Result<(), CodecError> {
        self.storage.with_storage(|| {
            ctx.push_hash_group(
                &mut self.by_fingerprint,
                key,
                row,
                "catia_standard_limit_curve_index",
                "catia_standard_limit_curve_index",
            )
        })
    }
}

fn merge_standard_limit_curves_from_records(
    ctx: &DecodeContext<'_>,
    curves: &mut Vec<NurbsCurve>,
    index: &mut LimitCurveIndex<'_>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut jets_storage = ctx.reserve_scoped(0, "catia_standard_a5_freeform_jets")?;
    let jets = jets_storage.with_storage(|| {
        crate::families::a5a8::records::a5_freeform_curves_from_records(ctx, data, records)
    })?;
    for jet in ctx.admit_iter(&jets, "catia_standard_a5_freeform_jets")? {
        for second_limit in [false, true] {
            let mut geometry_storage =
                ctx.reserve_scoped(0, "catia_standard_limit_curve_geometry")?;
            let Some(geometry) = geometry_storage.with_storage(|| {
                crate::families::a5a8::records::rolling_ball_limit_curve(
                    ctx,
                    jet,
                    second_limit,
                    refusal,
                )
            })?
            else {
                continue;
            };
            let key = edge_geometry::nurbs_curve_fingerprint(ctx, &geometry)?;
            if !index.contains(ctx, key, curves, &geometry)? {
                geometry_storage.commit()?;
                let row = curves.len();
                ctx.push_vec(curves, geometry, "catia standard limit curves")?;
                index.insert(ctx, key, row)?;
            }
        }
    }
    Ok(())
}

pub(super) fn standard_object_evidence_from_streams(
    ctx: &DecodeContext<'_>,
    streams: &[Vec<u8>],
    tags: &BTreeSet<u32>,
    edge_tags: &HashSet<u32>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<StandardObjectEvidence, cadmpeg_core::CodecError> {
    let mut evidence_store = StandardEvidenceStore::default();
    let mut edge_face_candidates = BTreeMap::<u32, Option<Vec<u32>>>::new();
    let mut edge_support_candidates = BTreeMap::<u32, Option<StandardEdgeSupport>>::new();
    let mut limit_curves = Vec::<NurbsCurve>::new();
    let mut limit_index = LimitCurveIndex::new(ctx, &limit_curves)?;
    let mut populations = Vec::new();
    for stream in ctx.admit_iter(streams, "catia_standard_object_streams")? {
        let records = crate::wire::records::consolidated_records_in_sources(
            ctx,
            stream,
            std::iter::once(std::iter::once(crate::wire::records::SourceExtent::whole(
                stream,
            ))),
        )?;
        merge_standard_limit_curves_from_records(
            ctx,
            &mut limit_curves,
            &mut limit_index,
            stream,
            &records,
            refusal,
        )?;
        for population in ctx.admit_iter(
            crate::families::b5::graph::object_stream_populations(ctx, stream)?,
            "catia_standard_object_populations",
        )? {
            ctx.push_vec(
                &mut populations,
                population,
                "catia_standard_object_populations",
            )?;
        }
    }
    // An object id conflicts when two of its frames, within one population or
    // across populations, carry different bytes.
    let mut population_objects = HashMap::<u32, Option<(usize, std::ops::Range<usize>)>>::new();
    let mut repeated_population_ids = HashSet::new();
    let mut conflicting_population_ids = HashSet::new();
    for (population_index, population) in ctx
        .admit_iter(&populations, "catia_standard_iteration")?
        .enumerate()
    {
        const OPERATION: &str = "catia_standard_population_objects";
        let mut objects = BTreeMap::<u32, Option<std::ops::Range<usize>>>::new();
        for frame in crate::families::b5::graph::object_stream_frames(ctx, population)? {
            let frame = frame?;
            let range = frame.start..frame.end;
            match ctx.get_mut_btree_map(&mut objects, &frame.object_id, OPERATION)? {
                Some(stored) => {
                    if let Some(previous) = stored.clone() {
                        if !ctx.equal_bytes(&population[previous], &population[range], OPERATION)? {
                            *stored = None;
                        }
                    }
                }
                None => {
                    ctx.insert_btree_map(&mut objects, frame.object_id, Some(range), OPERATION)?;
                }
            }
        }
        for (object_id, range) in ctx.admit_iter(objects, "catia_standard_population_object_ids")? {
            const MERGE: &str = "catia_standard_population_objects_by_id";
            let conflicting =
                match ctx.get_mut_hash_map(&mut population_objects, &object_id, MERGE)? {
                    Some(stored) => {
                        ctx.insert_hash_set(
                            &mut repeated_population_ids,
                            object_id,
                            "catia_standard_repeated_population_ids",
                        )?;
                        let same = match (stored.clone(), &range) {
                            (Some((previous_population, previous)), Some(range)) => ctx
                                .equal_bytes(
                                    &populations[previous_population][previous],
                                    &population[range.clone()],
                                    MERGE,
                                )?,
                            _ => false,
                        };
                        if !same {
                            *stored = None;
                        }
                        !same
                    }
                    None => {
                        let conflicting = range.is_none();
                        ctx.insert_hash_map(
                            &mut population_objects,
                            object_id,
                            range.map(|range| (population_index, range)),
                            MERGE,
                        )?;
                        conflicting
                    }
                };
            if conflicting {
                ctx.insert_hash_set(
                    &mut conflicting_population_ids,
                    object_id,
                    "catia_standard_conflicting_population_ids",
                )?;
            }
        }
    }
    {
        let mut visits = populations.iter();
        while let Some(stream) =
            ctx.next_charged(&mut visits, "catia_standard_population_streams")?
        {
            let frames = crate::families::b5::graph::collect_object_stream_frames(ctx, stream)?;
            let face_surfaces = crate::families::b5::graph::face_surface_references_from_frames(
                ctx, stream, &frames,
            )?;
            let mut surface_bindings = Vec::new();
            for &tag in ctx.admit_iter(tags, "catia_standard_object_surface_tags")? {
                ctx.push_vec(
                    &mut surface_bindings,
                    (tag, tag),
                    "catia_standard_surface_bindings",
                )?;
            }
            for &(face_id, surface_id) in
                ctx.admit_iter(&face_surfaces, "catia_standard_face_surface_bindings")?
            {
                if ctx.contains_btree_set(tags, &face_id, "catia_standard_face_surface_bindings")? {
                    ctx.push_vec(
                        &mut surface_bindings,
                        (face_id, surface_id),
                        "catia_standard_surface_bindings",
                    )?;
                }
            }
            let mut requested_surfaces = BTreeSet::new();
            for &(_, surface_id) in ctx.admit_iter(&surface_bindings, "catia_standard_iteration")? {
                ctx.insert_btree_set(
                    &mut requested_surfaces,
                    surface_id,
                    "catia_standard_requested_surfaces",
                )?;
            }
            let targeted_surfaces = crate::families::b5::graph::targeted_surfaces_from_frames(
                ctx,
                stream,
                &requested_surfaces,
                &frames,
                refusal,
            )?;
            let targeted_graph = crate::families::b5::graph::targeted_geometry_graph_from_frames(
                ctx, stream, &frames, refusal,
            )?;
            for &(object_id, surface_id) in
                ctx.admit_iter(&surface_bindings, "catia_standard_iteration")?
            {
                let Some(surface) = ctx.get_btree_map(
                    &targeted_surfaces,
                    &surface_id,
                    "catia_standard_targeted_surface_lookup",
                )?
                else {
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
                        None => {
                            crate::families::b5::transfer::resolved_surface_carrier(ctx, surface)?
                        }
                    })
                    .map(|carrier| match carrier {
                        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(
                            geometry,
                        ) => StandardSurfaceEvidence::Geometry(geometry),
                        crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall {
                            carrier_object_id,
                            definition,
                        } => StandardSurfaceEvidence::Procedure(
                            StandardSurfaceProcedure::RollingBall {
                                carrier_object_id,
                                definition: Box::new(*definition),
                                source: StandardRollingBallSource::ObjectStreamA8,
                            },
                        ),
                    })
                };
                let Some(evidence) = evidence else {
                    continue;
                };
                evidence_store.add(ctx, object_id, evidence)?;
            }
            if let Some(graph) = targeted_graph.as_ref() {
                for &(object_id, surface_id) in
                    ctx.admit_iter(&surface_bindings, "catia_standard_iteration")?
                {
                    if ctx.contains_key_btree_map(
                        &evidence_store.surfaces,
                        &object_id,
                        "catia_standard_surface_candidate_lookup",
                    )? {
                        continue;
                    }
                    let Some(evidence) =
                        standard_surface_evidence(ctx, graph, surface_id, refusal)?
                    else {
                        continue;
                    };
                    evidence_store.add(ctx, object_id, evidence)?;
                }
            }
            let edge_pcurves =
                crate::families::b5::graph::edge_support_pcurve_references_from_frames(
                    ctx, stream, edge_tags, &frames,
                )?;
            let mut requested_pcurves = HashSet::new();
            for (_, references) in
                ctx.admit_iter(&edge_pcurves, "catia_standard_edge_pcurve_references")?
            {
                for &pcurve_id in references {
                    ctx.insert_hash_set(
                        &mut requested_pcurves,
                        pcurve_id,
                        "catia_standard_requested_pcurves",
                    )?;
                }
            }
            let mut pcurves =
                BTreeMap::<u32, Option<crate::families::a5a8::records::A8Pcurve>>::new();
            for pcurve in ctx.admit_iter(
                crate::families::a5a8::records::object_stream_pcurves(ctx, stream)?,
                "catia_standard_pcurve_candidate_scan",
            )? {
                const CANDIDATES: &str = "catia_standard_pcurve_candidates";
                if !ctx.contains_hash_set(
                    &requested_pcurves,
                    &pcurve.object_id,
                    "catia_standard_requested_pcurve_filter",
                )? {
                    continue;
                }
                if let Some(stored) =
                    ctx.get_mut_btree_map(&mut pcurves, &pcurve.object_id, CANDIDATES)?
                {
                    if let Some(previous) = stored.as_ref() {
                        let same = previous.support_id == pcurve.support_id
                            && previous.range == pcurve.range
                            && previous.sites.len() == pcurve.sites.len()
                            && ctx.all_by(
                                previous.sites.iter().zip(&pcurve.sites),
                                |(left, right)| Ok(left == right),
                                CANDIDATES,
                            )?;
                        if !same {
                            *stored = None;
                        }
                    }
                } else {
                    ctx.insert_btree_map(&mut pcurves, pcurve.object_id, Some(pcurve), CANDIDATES)?;
                }
            }
            let mut surface_ids = BTreeSet::new();
            for pcurve in ctx
                .admit_iter(&pcurves, "catia_standard_iteration")?
                .map(|(_, value)| value)
                .filter_map(Option::as_ref)
            {
                ctx.insert_btree_set(
                    &mut surface_ids,
                    pcurve.support_id,
                    "catia_standard_pcurve_surface_ids",
                )?;
            }
            let targeted_surfaces = crate::families::b5::graph::targeted_surfaces_from_frames(
                ctx,
                stream,
                &surface_ids,
                &frames,
                refusal,
            )?;
            for (edge, references) in
                ctx.admit_iter(&edge_pcurves, "catia_standard_edge_pcurve_rows")?
            {
                let sides = references.map(|reference| -> Result<_, cadmpeg_core::CodecError> {
                    let Some(Some(pcurve)) =
                        ctx.get_btree_map(&pcurves, &reference, "catia_standard_pcurve_lookup")?
                    else {
                        return Ok(None);
                    };
                    let Some(surface) = ctx.get_btree_map(
                        &targeted_surfaces,
                        &pcurve.support_id,
                        "catia_standard_targeted_surface_lookup",
                    )?
                    else {
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
                ctx.entry_btree_map(
                    &mut edge_support_candidates,
                    *edge,
                    "catia_standard_edge_support_candidates",
                )?
                // Support carriers and pcurves have no decode cost: this
                // comparison is unpriced.
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| stored != &evidence) {
                        *stored = None;
                    }
                })
                .or_insert(Some(evidence));
            }
            let stream_edge_faces =
                crate::families::b5::graph::edge_face_references_from_frames(ctx, stream, &frames)?;
            for (edge, owners) in
                ctx.admit_iter(stream_edge_faces, "catia_standard_edge_face_rows")?
            {
                const OPERATION: &str = "catia_standard_edge_face_candidates";
                match ctx.get_mut_btree_map(&mut edge_face_candidates, &edge, OPERATION)? {
                    Some(stored) => {
                        if let Some(previous) = stored {
                            if !ctx.equal(previous, &owners, OPERATION)? {
                                *stored = None;
                            }
                        }
                    }
                    None => {
                        ctx.insert_btree_map(
                            &mut edge_face_candidates,
                            edge,
                            Some(owners),
                            OPERATION,
                        )?;
                    }
                }
            }
            let Some(graph) =
                crate::families::b5::graph::parse_from_frames(ctx, stream, &frames, refusal)?
            else {
                continue;
            };
            for &surface_id in ctx.admit_iter(tags, "catia_standard_object_surface_tags")? {
                let Some(evidence) = standard_surface_evidence(ctx, &graph, surface_id, refusal)?
                else {
                    continue;
                };
                evidence_store.add(ctx, surface_id, evidence)?;
            }
            for &(face_id, surface_id) in
                ctx.admit_iter(&face_surfaces, "catia_standard_face_surface_bindings")?
            {
                if !ctx.contains_btree_set(
                    tags,
                    &face_id,
                    "catia_standard_face_surface_bindings",
                )? {
                    continue;
                }
                let evidence = standard_surface_evidence(ctx, &graph, surface_id, refusal)?;
                let Some(evidence) = evidence else { continue };
                evidence_store.add(ctx, face_id, evidence)?;
            }
        }
    }
    let (surface_geometries, procedural_surfaces) =
        evidence_store.into_outputs(ctx, &conflicting_population_ids)?;
    // Edges and owners with an id repeated across populations are ambiguous.
    let repeated = |id: &u32| {
        ctx.contains_hash_set(
            &repeated_population_ids,
            id,
            "catia_standard_repeated_population_filter",
        )
    };
    let mut edge_owner_faces = HashMap::new();
    for (edge, owners) in
        ctx.admit_iter(edge_face_candidates, "catia_standard_edge_face_outputs")?
    {
        let Some(owners) = owners else {
            continue;
        };
        if repeated(&edge)?
            || ctx.any_by(&owners, repeated, "catia_standard_repeated_edge_owner_scan")?
        {
            continue;
        }
        ctx.insert_hash_map(
            &mut edge_owner_faces,
            edge,
            owners,
            "catia_standard_edge_owner_faces",
        )?;
    }
    let mut edge_supports = HashMap::new();
    for (edge, support) in ctx.admit_iter(
        edge_support_candidates,
        "catia_standard_edge_support_outputs",
    )? {
        let Some(support) = support else {
            continue;
        };
        if repeated(&edge)?
            || ctx.any_by(
                &support.surface_object_ids,
                repeated,
                "catia_standard_repeated_edge_support_scan",
            )?
        {
            continue;
        }
        ctx.insert_hash_map(
            &mut edge_supports,
            edge,
            support,
            "catia_standard_edge_supports",
        )?;
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
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
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
    for (face_index, (surface, forward, offset)) in ctx
        .admit_iter(bindings, "catia_standard_iteration")?
        .enumerate()
    {
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
            crate::resource::derived_annotation(ctx, annotations, &face_id, field)?;
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
    crate::resource::derived_annotation(ctx, annotations, &body_id, "kind")?;
    crate::resource::derived_annotation(ctx, annotations, &body_id, "regions")?;
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
    crate::resource::derived_annotation(ctx, annotations, &region_id, "body")?;
    crate::resource::derived_annotation(ctx, annotations, &region_id, "shells")?;
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
    crate::resource::derived_annotation(ctx, annotations, &shell_id, "region")?;
    crate::resource::derived_annotation(ctx, annotations, &shell_id, "faces")?;
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
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    components: &[Vec<usize>],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    if components.is_empty()
        || ctx.any_by(
            components,
            |faces| Ok(faces.is_empty()),
            "catia_standard_component_rows",
        )?
    {
        return Ok(false);
    }
    let component_face_count = ctx.fold(
        components,
        0usize,
        |total, faces| {
            total.checked_add(faces.len()).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_standard_component_face_count", u64::MAX, u64::MAX)
            })
        },
        "catia_standard_component_face_count",
    )?;
    if component_face_count != ir.model.faces.len() {
        return Ok(false);
    }
    let body_id = ctx
        .copy_retained_text("catia:standard:body#0", "catia_standard_partition_body_id")
        .and_then(|text| BodyId::mint(text).map_err(cadmpeg_core::CodecError::malformed))?;
    let Some(body_index) = ctx.position_by(
        &ir.model.bodies,
        |body| {
            ctx.equal(
                &body.id,
                &body_id,
                "catia_standard_partition_body_candidates",
            )
        },
        "catia_standard_partition_body_candidates",
    )?
    else {
        return Ok(false);
    };
    let Some(body) = ir.model.bodies.get_mut(body_index) else {
        return Ok(false);
    };
    let mut region_ids = Vec::new();
    for (component, _) in ctx
        .admit_iter(components, "catia_standard_partition_component_ids")?
        .enumerate()
    {
        let id = RegionId::mint(ctx.format_retained(
            format_args!("catia:standard:region#0-{component:01}"),
            "catia_standard_partition_region_id",
        )?)
        .map_err(CodecError::malformed)?;
        ctx.push_vec(&mut region_ids, id, "catia_standard_partition_region_ids")?;
    }
    let mut body_regions = Vec::new();
    for id in ctx.admit_iter(&region_ids, "catia_standard_iteration")? {
        ctx.push_vec(
            &mut body_regions,
            id.try_clone_for_decode(ctx, "catia_standard_partition_body_region_id")?,
            "catia_standard_partition_body_regions",
        )?;
    }
    body.regions = body_regions;
    crate::resource::derived_annotation(ctx, annotations, &body_id, "regions")?;

    {
        let mut visits = components.iter().enumerate();
        while let Some((component, faces)) =
            ctx.next_charged(&mut visits, "catia_standard_iteration")?
        {
            let region_id = region_ids[component]
                .try_clone_for_decode(ctx, "catia_standard_partition_region_copy")?;
            let shell_id = ShellId::mint(ctx.format_retained(
                format_args!("catia:standard:shell#0-{component:01}"),
                "catia_standard_partition_shell_id",
            )?)
            .map_err(CodecError::malformed)?;
            let mut face_ids = Vec::new();
            for &face in ctx.admit_iter(faces, "catia_standard_component_faces")? {
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
            {
                let mut visits = faces.iter();
                while let Some(&face) =
                    ctx.next_charged(&mut visits, "catia_standard_component_faces")?
                {
                    let Some(face) = ir.model.faces.get_mut(face) else {
                        return Ok(false);
                    };
                    face.shell = shell_id
                        .try_clone_for_decode(ctx, "catia_standard_partition_face_shell_id")?;
                    crate::resource::derived_annotation(ctx, annotations, &face.id, "shell")?;
                }
            }
            if component == 0 {
                let Some(region_index) = ctx.position_by(
                    &ir.model.regions,
                    |region| {
                        ctx.equal(
                            &region.id,
                            &region_id,
                            "catia_standard_partition_region_candidates",
                        )
                    },
                    "catia_standard_partition_region_candidates",
                )?
                else {
                    return Ok(false);
                };
                let Some(region) = ir.model.regions.get_mut(region_index) else {
                    return Ok(false);
                };
                region.shells = region_shells;
                let Some(shell_index) = ctx.position_by(
                    &ir.model.shells,
                    |shell| {
                        ctx.equal(
                            &shell.id,
                            &shell_id,
                            "catia_standard_partition_shell_candidates",
                        )
                    },
                    "catia_standard_partition_shell_candidates",
                )?
                else {
                    return Ok(false);
                };
                let Some(shell) = ir.model.shells.get_mut(shell_index) else {
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
                crate::resource::derived_annotation(ctx, annotations, &region_id, field)?;
            }
            admission.reserve_entity(&mut ir.model.regions, "catia_standard_partition_regions")?;
            ir.model.regions.push(Region {
                id: region_id
                    .try_clone_for_decode(ctx, "catia_standard_partition_model_region_id")?,
                body: body_id
                    .try_clone_for_decode(ctx, "catia_standard_partition_region_body_id")?,
                shells: region_shells,
            });
            for field in ["region", "faces"] {
                crate::resource::derived_annotation(ctx, annotations, &shell_id, field)?;
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
    }
    Ok(true)
}

pub(super) fn apply_standard_native_edge_faces(
    ctx: &DecodeContext<'_>,
    edge_faces: &mut [[usize; 2]],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    native_edge_faces: &HashMap<u32, Vec<u32>>,
) -> Result<(), CodecError> {
    if edge_faces.len() != supports.len() {
        return Ok(());
    }
    let mut face_by_carrier = HashMap::<u32, Option<usize>>::new();
    for (face, record) in ctx
        .admit_iter(records, "catia_standard_iteration")?
        .enumerate()
    {
        let carrier = match record {
            crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) => {
                prefix.target
            }
            crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } => *tag,
        };
        if let Some(stored) = ctx.get_mut_hash_map(
            &mut face_by_carrier,
            &carrier,
            "catia_standard_native_face_carriers",
        )? {
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
    {
        let mut visits = supports.iter().take(edge_faces.len()).enumerate();
        while let Some((edge, support)) =
            ctx.next_charged(&mut visits, "catia_standard_native_edge_face_supports")?
        {
            const OPERATION: &str = "catia_standard_native_owner_faces";
            let faces = &mut edge_faces[edge];
            if faces[0] != faces[1] {
                continue;
            }
            let Some(owner_ids) = ctx.get_hash_map(native_edge_faces, &support.tag, OPERATION)?
            else {
                continue;
            };
            // The edge takes the one other face its owners name; a second distinct
            // face leaves it unresolved.
            let mut candidate = None;
            let ambiguous = ctx.any_by(
                owner_ids,
                |owner| {
                    let Some(&Some(face)) = ctx.get_hash_map(&face_by_carrier, owner, OPERATION)?
                    else {
                        return Ok(false);
                    };
                    if face == faces[0] {
                        return Ok(false);
                    }
                    Ok(*candidate.get_or_insert(face) != face)
                },
                OPERATION,
            )?;
            if let (false, Some(face)) = (ambiguous, candidate) {
                faces[1] = face;
            }
        }
    }
    Ok(())
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
    AnnotationAccount,
> {
    ir: &'input0 mut CadIr,
    annotations: &'input1 mut AnnotationBuilder<AnnotationAccount>,
    bindings: &'input2 [(SurfaceId, bool, usize)],
    records: &'input3 [crate::families::standard::records::StandardSurfaceRecord],
    face_bounds: &'input4 [Option<crate::families::standard::records::StandardFaceBounds>],
    spine: &'input5 [u8],
    edge_table_form: EdgeTableForm,
    brep: &'input6 [u8],
    support_override: Option<&'input7 [crate::families::standard::records::StandardCurveSupport]>,
    source: &'input8 [u8],
    e5_record_range: Option<std::ops::Range<usize>>,
    use_vertex_roster: bool,
    native_edge_faces: &'input9 HashMap<u32, Vec<u32>>,
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
        impl cadmpeg_ir::annotations::AnnotationStorage,
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
        e5_record_range,
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

    let mut topology_storage = ctx
        .reserve_scoped(0, "catia_standard_topology_storage")
        .map_err(StandardTopologyError::Resource)?;
    let (
        surface_indices,
        supports,
        mut topology,
        point_assignment,
        native_supports_by_row,
        limit_curve_bindings,
        endpoint_candidates,
    ) =
        topology_storage
            .with_storage(|| {
                Ok::<_, CodecError>((|| -> Result<_, StandardTopologyError> {
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
                            |supports| ctx.copy_slice(supports, "catia_topology_support_override"),
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
                    let Some(mut edge_faces) = missing_edge::resolve_standard_edge_faces(
                        ctx,
                        spine,
                        &serialized_edge_faces,
                    )
                    .map_err(StandardTopologyError::Resource)?
                    else {
                        return Err(StandardTopologyFailure::EdgeFaceAssignment.into());
                    };
                    let mut deferred_port_edges = ctx
                        .alloc_filled(supports.len(), false, "catia_deferred_port_edges")
                        .map_err(StandardTopologyError::Resource)?;
                    let mut open_face_domains = None;
                    let mut endpoint_face_assignments = None;
                    apply_standard_native_edge_faces(
                        ctx,
                        &mut edge_faces,
                        &supports,
                        records,
                        native_edge_faces,
                    )
                    .map_err(StandardTopologyError::Resource)?;
                    for (edge, faces) in ctx
                        .admit_iter(&edge_faces, "catia_standard_native_edge_face_assignments")?
                        .take(supports.len())
                        .enumerate()
                    {
                        supports[edge].faces = *faces;
                    }
                    let surface_indices = (|| {
                        let mut surface_indices = HashMap::new();
                        for (index, surface) in ctx
                            .admit_iter(&ir.model.surfaces, "catia_standard_iteration")?
                            .enumerate()
                        {
                            let id = surface
                                .id
                                .try_clone_for_decode(ctx, "catia_standard_surface_id_copy")?;
                            ctx.insert_hash_map(
                                &mut surface_indices,
                                id,
                                index,
                                "catia_standard_surface_indices",
                            )?;
                        }
                        Ok::<_, CodecError>(surface_indices)
                    })()
                    .map_err(StandardTopologyError::Resource)?;
                    let face_bounds = (face_bounds.len() == face_count).then_some(face_bounds);
                    let face_point_membership = standard_face_point_membership(
                        ctx,
                        ir,
                        bindings,
                        &surface_indices,
                        face_bounds,
                    )
                    .map_err(StandardTopologyError::Resource)?;
                    let limit_curve_bindings = standard_limit_curve_bindings(
                        ctx,
                        ir,
                        bindings,
                        &surface_indices,
                        &supports,
                        limit_curves,
                    )
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
                    for point in ctx.admit_iter(&ir.model.points, "catia_standard_iteration")? {
                        point_coordinates.push([
                            f32_from_f64(point.position().get().x)
                                .ok_or(StandardTopologyFailure::ConflictingNativeEndpoints)?,
                            f32_from_f64(point.position().get().y)
                                .ok_or(StandardTopologyFailure::ConflictingNativeEndpoints)?,
                            f32_from_f64(point.position().get().z)
                                .ok_or(StandardTopologyFailure::ConflictingNativeEndpoints)?,
                        ]);
                    }
                    let visualization_endpoint_pairs = missing_edge::standard_edge_rows(ctx, spine)
                        .map_err(StandardTopologyError::Resource)?;
                    let visualization_endpoint_pairs = match visualization_endpoint_pairs {
                        Some(rows) => missing_edge::visualization_endpoint_pairs(
                            ctx,
                            source,
                            &rows,
                            &point_coordinates,
                        )
                        .map_err(StandardTopologyError::Resource)?,
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
                    {
                        let mut visits = supports.iter();
                        while let Some(support) = ctx
                            .next_charged(&mut visits, "catia_standard_iteration")
                            .map_err(StandardTopologyError::Resource)?
                        {
                            let Some(surface0) =
                                face_surface(ctx, ir, bindings, &surface_indices, support.faces[0])
                                    .map_err(StandardTopologyError::Resource)?
                            else {
                                return Err(StandardTopologyFailure::MissingFaceSurface.into());
                            };
                            let Some(surface1) =
                                face_surface(ctx, ir, bindings, &surface_indices, support.faces[1])
                                    .map_err(StandardTopologyError::Resource)?
                            else {
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
                            let [first, second] = support.faces;
                            let faces = [first.min(second), first.max(second)];
                            for (face, surface) in [
                                (support.faces[0], &surface0.geometry),
                                (support.faces[1], &surface1.geometry),
                            ] {
                                if !ctx
                                    .contains_key_hash_map(
                                        &face_incidence_candidates,
                                        &face,
                                        "catia_face_incidence_lookup",
                                    )
                                    .map_err(StandardTopologyError::Resource)?
                                {
                                    let mut points = Vec::new();
                                    for (index, point) in ctx
                                        .admit_iter(&ir.model.points, "catia_standard_iteration")?
                                        .enumerate()
                                    {
                                        if point_on_standard_face(
                                            ctx,
                                            point.position().get(),
                                            surface,
                                            face_bounds.as_ref().and_then(|bounds| bounds[face]),
                                        )
                                        .map_err(CodecError::from)
                                        .map_err(StandardTopologyError::Resource)?
                                        {
                                            ctx.push_vec(
                                                &mut points,
                                                index,
                                                "catia_face_incidence_points",
                                            )
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
                            if !ctx
                                .contains_key_hash_map(
                                    &incidence_candidates,
                                    &faces,
                                    "catia_incidence_candidate_lookup",
                                )
                                .map_err(StandardTopologyError::Resource)?
                            {
                                // Both rows list point indexes in increasing order, so
                                // their intersection is one merge walk.
                                let incidence_row = |face: usize| {
                                    ctx.get_hash_map(
                                        &face_incidence_candidates,
                                        &face,
                                        "catia_face_incidence_lookup",
                                    )
                                    .map(|row| row.map_or(&[][..], Vec::as_slice))
                                };
                                let left = incidence_row(faces[0])
                                    .map_err(StandardTopologyError::Resource)?;
                                let right = incidence_row(faces[1])
                                    .map_err(StandardTopologyError::Resource)?;
                                let mut shared = Vec::new();
                                let (mut left_index, mut right_index) = (0, 0);
                                while let (Some(&left_point), Some(&right_point)) =
                                    (left.get(left_index), right.get(right_index))
                                {
                                    ctx.charge_work(1, "catia_incidence_shared_points")
                                        .map_err(StandardTopologyError::Resource)?;
                                    match left_point.cmp(&right_point) {
                                        std::cmp::Ordering::Less => left_index += 1,
                                        std::cmp::Ordering::Greater => right_index += 1,
                                        std::cmp::Ordering::Equal => {
                                            ctx.push_vec(
                                                &mut shared,
                                                left_point,
                                                "catia_incidence_shared_points",
                                            )
                                            .map_err(StandardTopologyError::Resource)?;
                                            left_index += 1;
                                            right_index += 1;
                                        }
                                    }
                                }
                                ctx.insert_hash_map(
                                    &mut incidence_candidates,
                                    faces,
                                    shared,
                                    "catia_incidence_candidate_rows",
                                )
                                .map_err(StandardTopologyError::Resource)?;
                            }
                            let shared = ctx
                                .get_hash_map(
                                    &incidence_candidates,
                                    &faces,
                                    "catia_incidence_candidate_lookup",
                                )
                                .map_err(StandardTopologyError::Resource)?
                                .map_or(&[][..], Vec::as_slice);
                            ctx.copy_slice(shared, "catia_incidence_candidate_copy")
                                .map_err(StandardTopologyError::Resource)?
                        }
                    };
                            endpoint_candidates.push(candidates);
                        }
                    }
                    let edge_classes = standard_curve_edge_classes(ctx, &supports)
                        .map_err(StandardTopologyError::Resource)?;
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
                    let e5_topology = match e5_record_range {
                        Some(range) => {
                            crate::families::e5::graph::parse_topology(ctx, &source[range])
                                .map_err(StandardTopologyError::Resource)?
                        }
                        None => None,
                    };
                    if let Some(e5_topology) = e5_topology {
                        if !merge_standard_edge_vertex_references(
                            ctx,
                            &mut native_edges,
                            &e5_topology.edges,
                            |record| [record.start_vertex, record.end_vertex],
                        )
                        .map_err(StandardTopologyError::Resource)?
                        {
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
                    for support in ctx.admit_iter(&supports, "catia_native_port_options")? {
                        native_port_options.push(
                            ctx.get_btree_map(
                                &native_edges,
                                &support.tag,
                                "catia_native_port_options",
                            )
                            .map_err(StandardTopologyError::Resource)?
                            .copied(),
                        );
                    }
                    let mut native_ports = Vec::new();
                    let mut all_ports = true;
                    let mut port_options = native_port_options.iter().copied();
                    while let Some(pair) = ctx
                        .next_charged(&mut port_options, "catia_native_port_pair_visits")
                        .map_err(StandardTopologyError::Resource)?
                    {
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
                        .map(|roster| {
                            standard_serialized_endpoint_pairs(
                                ctx,
                                &supports,
                                &native_edges,
                                roster,
                            )
                        })
                        .transpose()
                        .map_err(StandardTopologyError::Resource)?
                        .flatten();
                    let native_support_edge_ids =
                        standard_native_support_edge_ids(ctx, &supports, native_edge_supports)
                            .map_err(StandardTopologyError::Resource)?;
                    let native_supports_by_row = ctx
                        .try_collect_vec(
                            native_support_edge_ids.iter().map(|edge| match edge {
                                Some(edge) => ctx.get_hash_map(
                                    native_edge_supports,
                                    edge,
                                    "catia_native_support_rows",
                                ),
                                None => Ok(None),
                            }),
                            "catia_native_support_rows",
                        )
                        .map_err(StandardTopologyError::Resource)?;
                    let Ok(native_endpoint_evidence) = merge_native_endpoint_evidence(
                        ctx,
                        graph_endpoint_pairs.as_deref(),
                        roster_endpoint_pairs.as_deref(),
                    )
                    .map_err(StandardTopologyError::Resource)?
                    else {
                        return Err(StandardTopologyFailure::ConflictingNativeEndpoints.into());
                    };
                    diagnostics.native_endpoint_pairs = match native_endpoint_evidence.as_ref() {
                        Some(pairs) => ctx
                            .admit_iter(pairs, "catia_standard_native_endpoint_pair_count")?
                            .filter(|pair| pair.is_some())
                            .count(),
                        None => 0,
                    };
                    if let Some(pairs) = &native_endpoint_evidence {
                        let mut pair_rows = pairs.iter().enumerate();
                        while let Some((edge, pair)) = ctx
                            .next_charged(&mut pair_rows, "catia_standard_native_endpoint_pairs")
                            .map_err(StandardTopologyError::Resource)?
                        {
                            let Some(pair) = *pair else { continue };
                            if !merge_ordered_endpoint_pair(&mut ordered_endpoint_pairs, edge, pair)
                            {
                                return Err(
                                    StandardTopologyFailure::ConflictingNativeEndpoints.into()
                                );
                            }
                        }
                    }
                    if let Some(pairs) = visualization_endpoint_pairs {
                        {
                            let mut visits = (pairs).into_iter().enumerate();
                            while let Some((edge, pair)) = ctx
                                .next_charged(&mut visits, "catia_visualization_endpoint_pairs")
                                .map_err(StandardTopologyError::Resource)?
                            {
                                if !merge_derived_endpoint_pair(
                                    &mut ordered_endpoint_pairs,
                                    edge,
                                    pair,
                                ) {
                                    return Err(
                                        StandardTopologyFailure::ConflictingNativeEndpoints.into(),
                                    );
                                }
                            }
                        }
                    }
                    {
                        let mut visits = limit_curve_bindings.iter().enumerate();
                        while let Some((edge, bindings)) = ctx
                            .next_charged(&mut visits, "catia_standard_limit_curve_bindings")
                            .map_err(StandardTopologyError::Resource)?
                        {
                            let Ok([binding]) =
                                <[StandardLimitCurveBinding; 1]>::try_from(bindings.as_slice())
                            else {
                                continue;
                            };
                            if !merge_derived_endpoint_pair(
                                &mut ordered_endpoint_pairs,
                                edge,
                                binding.points,
                            ) {
                                return Err(
                                    StandardTopologyFailure::ConflictingNativeEndpoints.into()
                                );
                            }
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
                        for (edge, bindings) in ctx
                            .admit_iter(
                                &limit_curve_bindings,
                                "catia_standard_limit_curve_bindings",
                            )?
                            .enumerate()
                        {
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
                            for binding in
                                ctx.admit_iter(bindings, "catia_standard_limit_curve_binding_rows")?
                            {
                                let [first, second] = binding.points;
                                limit_pairs.push([first.min(second), first.max(second)]);
                            }
                            ctx.sort_unstable_by(
                                &mut limit_pairs,
                                |value| value,
                                Ord::cmp,
                                "catia_limit_endpoint_pairs_sort",
                            )
                            .map_err(StandardTopologyError::Resource)?;
                            ctx.dedup_vec(&mut limit_pairs, "catia_limit_endpoint_pairs_dedup")
                                .map_err(StandardTopologyError::Resource)?;
                            if options[edge].is_empty() {
                                options[edge] = limit_pairs;
                            }
                        }
                    }
                    {
                        let mut visits = supports.iter().enumerate();
                        while let Some((edge, _)) = ctx
                            .next_charged(
                                &mut visits,
                                "catia_standard_native_endpoint_support_rows",
                            )
                            .map_err(StandardTopologyError::Resource)?
                        {
                            let native_pair =
                                match native_supports_by_row.get(edge).and_then(Option::as_ref) {
                                    Some(native) => standard_native_support_endpoint_pair(
                                        ctx,
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
                            if !merge_derived_endpoint_pair(&mut ordered_endpoint_pairs, edge, pair)
                            {
                                return Err(
                                    StandardTopologyFailure::ConflictingNativeEndpoints.into()
                                );
                            }
                            if let Some(options) = &mut endpoint_options {
                                if ctx
                                    .any_by(
                                        &options[edge],
                                        |candidate| {
                                            Ok(missing_edge::same_unordered_pair(*candidate, pair))
                                        },
                                        "catia_standard_endpoint_options",
                                    )
                                    .map_err(StandardTopologyError::Resource)?
                                {
                                    options[edge] = ctx
                                        .alloc_filled(
                                            1,
                                            pair,
                                            "catia_native_support_singleton_pair",
                                        )
                                        .map_err(StandardTopologyError::Resource)?;
                                }
                            }
                        }
                    }
                    if let (Some(options), Some(pairs)) =
                        (&mut endpoint_options, &native_endpoint_evidence)
                    {
                        for (edge, pair) in ctx
                            .admit_iter(pairs, "catia_standard_native_endpoint_evidence_rows")?
                            .take(options.len())
                            .enumerate()
                        {
                            if let Some(pair) = pair {
                                options[edge] = ctx
                                    .alloc_filled(1, *pair, "catia_native_evidence_singleton_pair")
                                    .map_err(StandardTopologyError::Resource)?;
                            }
                        }
                    }
                    if let (Some(options), Some(points)) =
                        (&mut endpoint_options, &allocation_endpoint_points)
                    {
                        corroborate_successor_endpoint_points(ctx, options, points)
                            .map_err(StandardTopologyError::Resource)?;
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
                    if let (Some(options), Some(pairs)) =
                        (&mut endpoint_options, &graph_propagated_endpoint_pairs)
                    {
                        for (edge, pair) in ctx
                            .admit_iter(pairs, "catia_standard_propagated_endpoint_rows")?
                            .take(options.len())
                            .enumerate()
                        {
                            if let Some(pair) = pair {
                                options[edge] = ctx
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
                        let handle_face_candidates =
                            missing_edge::standard_repeated_edge_face_handle_candidates(
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
                        {
                            let mut visits = supports.iter().enumerate();
                            while let Some((edge, support)) = ctx
                                .next_charged(&mut visits, "catia_standard_iteration")
                                .map_err(StandardTopologyError::Resource)?
                            {
                                let mut faces = Vec::new();
                                if support.faces[0] == support.faces[1] {
                                    for (face, _) in ctx
                                        .admit_iter(
                                            &ir.model.faces,
                                            "catia_standard_circle_face_rows",
                                        )?
                                        .enumerate()
                                        .filter(|(face, _)| *face != support.faces[0])
                                    {
                                        let Some(surface) =
                                            face_surface(ctx, ir, bindings, &surface_indices, face)
                                                .map_err(StandardTopologyError::Resource)?
                                        else {
                                            continue;
                                        };
                                        let bounds =
                                            face_bounds.as_ref().and_then(|bounds| bounds[face]);
                                        let res = ctx
                                            .any_by(
                                                &options[edge],
                                                |pair| {
                                                    for point in pair {
                                                        let Some(point) =
                                                            ir.model.points.get(*point)
                                                        else {
                                                            return Ok(false);
                                                        };
                                                        if !point_on_standard_face(
                                                            ctx,
                                                            point.position().get(),
                                                            &surface.geometry,
                                                            bounds,
                                                        )? {
                                                            return Ok(false);
                                                        }
                                                    }
                                                    Ok(standard_nurbs_line_pair_on_face(
                                                        ctx,
                                                        &surface.geometry,
                                                        support,
                                                        pair,
                                                        &ir.model.points,
                                                        bounds,
                                                    )?)
                                                },
                                                "catia_standard_repeated_edge_pairs",
                                            )
                                            .map_err(StandardTopologyError::Resource)?;
                                        if res {
                                            ctx.push_vec(
                                                &mut faces,
                                                face,
                                                "catia_repeated_allowed_faces",
                                            )
                                            .map_err(StandardTopologyError::Resource)?;
                                        }
                                    }
                                }
                                allowed_faces.push(faces);
                            }
                        }
                        let mut face_geometries = Vec::new();
                        ctx.reserve_vec(
                            &mut face_geometries,
                            face_count,
                            "catia_repeated_face_geometry_refs",
                        )
                        .map_err(StandardTopologyError::Resource)?;
                        let mut all_face_geometries = true;
                        let mut faces = 0..face_count;
                        while let Some(face) = ctx
                            .next_charged(&mut faces, "catia_repeated_face_geometry_rows")
                            .map_err(StandardTopologyError::Resource)?
                        {
                            let Some(surface) =
                                face_surface(ctx, ir, bindings, &surface_indices, face)
                                    .map_err(StandardTopologyError::Resource)?
                            else {
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
                        for support in
                            ctx.admit_iter(&supports, "catia_repeated_edge_geometry_refs")?
                        {
                            edge_geometries.push(&support.geometry);
                        }
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
                            ctx,
                            &edge_faces,
                            &mut allowed_faces,
                            face_bounds,
                            face_geometries.as_deref(),
                            &edge_geometries,
                        )
                        .map_err(StandardTopologyError::Resource)?;
                        let has_alternates = ctx
                            .any_by(
                                &allowed_faces,
                                |faces| Ok(!faces.is_empty()),
                                "catia_standard_alternate_face_domains",
                            )
                            .map_err(StandardTopologyError::Resource)?;
                        let endpoint_pairs = if has_alternates {
                            let mut pairs = Vec::new();
                            ctx.reserve_vec(
                                &mut pairs,
                                options.len(),
                                "catia_repeated_endpoint_pairs",
                            )
                            .map_err(StandardTopologyError::Resource)?;
                            let mut complete = true;
                            let mut rows = options.iter();
                            while let Some(choices) = ctx
                                .next_charged(&mut rows, "catia_repeated_endpoint_pairs")
                                .map_err(StandardTopologyError::Resource)?
                            {
                                let Ok([pair]) = <[[usize; 2]; 1]>::try_from(choices.as_slice())
                                else {
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
                                ctx.copy_slice(closure, "catia_repeated_endpoint_completed")
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
                            {
                                let mut visits = edge_faces.iter().take(supports.len()).enumerate();
                                while let Some((edge, faces)) = ctx
                                    .next_charged(
                                        &mut visits,
                                        "catia_standard_completed_edge_faces",
                                    )
                                    .map_err(StandardTopologyError::Resource)?
                                {
                                    let support = &mut supports[edge];
                                    if support.faces == *faces {
                                        continue;
                                    }
                                    support.faces = *faces;
                                    let Some(surface) =
                                        face_surface(ctx, ir, bindings, &surface_indices, faces[1])
                                            .map_err(StandardTopologyError::Resource)?
                                    else {
                                        return Err(
                                            StandardTopologyFailure::MissingFaceSurface.into()
                                        );
                                    };
                                    let bounds =
                                        face_bounds.as_ref().and_then(|bounds| bounds[faces[1]]);
                                    ctx.retain_vec(
                                        &mut options[edge],
                                        |pair| {
                                            for point in pair {
                                                let Some(point) = ir.model.points.get(*point)
                                                else {
                                                    return Ok(false);
                                                };
                                                if !point_on_standard_face(
                                                    ctx,
                                                    point.position().get(),
                                                    &surface.geometry,
                                                    bounds,
                                                )? {
                                                    return Ok(false);
                                                }
                                            }
                                            Ok(true)
                                        },
                                        "catia_standard_completed_endpoint_pairs",
                                    )
                                    .map_err(StandardTopologyError::Resource)?;
                                    if options[edge].is_empty() {
                                        return Err(
                                            StandardTopologyFailure::EmptyEndpointDomain.into()
                                        );
                                    }
                                }
                            }
                        } else {
                            for (edge, faces) in ctx
                                .admit_iter(&edge_faces, "catia_standard_deferred_edge_faces")?
                                .enumerate()
                            {
                                deferred_port_edges[edge] = faces[0] == faces[1]
                                    && allowed_faces
                                        .get(edge)
                                        .is_some_and(|faces| !faces.is_empty());
                            }
                            if ctx
                                .any_by(
                                    &allowed_faces,
                                    |faces| Ok(!faces.is_empty()),
                                    "catia_standard_open_face_domains",
                                )
                                .map_err(StandardTopologyError::Resource)?
                            {
                                open_face_domains = Some(allowed_faces);
                            }
                        }
                    }
                    let has_open_face_domains = match open_face_domains.as_ref() {
                        Some(domains) => ctx
                            .any_by(
                                domains,
                                |domain| Ok(!domain.is_empty()),
                                "catia_standard_open_face_domains",
                            )
                            .map_err(StandardTopologyError::Resource)?,
                        None => false,
                    };
                    let endpoint_pair_on_incident_faces =
                        |edge: usize, pair: [usize; 2]| -> Result<bool, CodecError> {
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
                                    let Some(surface) =
                                        face_surface(ctx, ir, bindings, &surface_indices, face)?
                                    else {
                                        return Ok(false);
                                    };
                                    let bounds =
                                        face_bounds.as_ref().and_then(|bounds| bounds[face]);
                                    if !point_on_standard_face(
                                        ctx,
                                        position,
                                        &surface.geometry,
                                        bounds,
                                    )? || !standard_nurbs_line_pair_on_face(
                                        ctx,
                                        &surface.geometry,
                                        &supports[edge],
                                        &pair,
                                        &ir.model.points,
                                        bounds,
                                    )? {
                                        return Ok(false);
                                    }
                                }
                            }
                            Ok(true)
                        };
                    if let Some(options) = &mut endpoint_options {
                        {
                            let mut visits = supports.iter().take(options.len()).enumerate();
                            while let Some((edge, _)) = ctx
                                .next_charged(
                                    &mut visits,
                                    "catia_standard_endpoint_option_supports",
                                )
                                .map_err(StandardTopologyError::Resource)?
                            {
                                let pairs = &mut options[edge];
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
                                ctx.retain_vec(
                                    pairs,
                                    |pair| {
                                        let Some(start) = ir
                                            .model
                                            .points
                                            .get(pair[0])
                                            .map(|point| point.position().get())
                                        else {
                                            return Ok(false);
                                        };
                                        let Some(end) = ir
                                            .model
                                            .points
                                            .get(pair[1])
                                            .map(|point| point.position().get())
                                        else {
                                            return Ok(false);
                                        };
                                        for &face in &support.faces {
                                            let Some(surface) = face_surface(
                                                ctx,
                                                ir,
                                                bindings,
                                                &surface_indices,
                                                face,
                                            )?
                                            else {
                                                return Ok(false);
                                            };
                                            if !standard_endpoint_pair_supports_topology(
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
                                    )? {
                                        return Ok(false);
                                    }
                                        }
                                        Ok(true)
                                    },
                                    "catia_standard_endpoint_topology_filter",
                                )
                                .map_err(StandardTopologyError::Resource)?;
                                if pairs.is_empty() {
                                    *pairs = unfiltered;
                                }
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
                            if let Some(placement_domains) =
                                missing_edge::standard_mesh_placement_endpoint_pairs(
                                    ctx,
                                    spine,
                                    &edge_faces,
                                    &seeds,
                                )
                                .map_err(StandardTopologyError::Resource)?
                            {
                                for (edge, mut domain) in ctx
                                    .admit_iter(
                                        placement_domains,
                                        "catia_standard_placement_domain_rows",
                                    )?
                                    .enumerate()
                                {
                                    if deferred_port_edges[edge] {
                                        continue;
                                    }
                                    ctx.retain_vec(
                                        &mut domain,
                                        |pair| endpoint_pair_on_incident_faces(edge, *pair),
                                        "catia_standard_placement_domain_faces",
                                    )
                                    .map_err(StandardTopologyError::Resource)?;
                                    if domain.is_empty() {
                                        continue;
                                    }
                                    // The domain only narrows a nonempty row, so a row changes
                                    // exactly when it was empty or loses a pair.
                                    if options[edge].is_empty() {
                                        options[edge] = domain;
                                        changed = true;
                                    } else {
                                        let before = options[edge].len();
                                        ctx.retain_vec(
                                            &mut options[edge],
                                            |pair| {
                                                ctx.any_by(
                                                    &domain,
                                                    |candidate| {
                                                        Ok(missing_edge::same_unordered_pair(
                                                            *pair, *candidate,
                                                        ))
                                                    },
                                                    "catia_standard_endpoint_domain_pairs",
                                                )
                                            },
                                            "catia_standard_endpoint_pair_domain_filter",
                                        )
                                        .map_err(StandardTopologyError::Resource)?;
                                        changed |= options[edge].len() != before;
                                    }
                                }
                            }
                            let boundary_domains = if ctx
                                .all_by(
                                    options.as_slice(),
                                    |domain| Ok(!domain.is_empty()),
                                    "catia_standard_complete_endpoint_domains",
                                )
                                .map_err(StandardTopologyError::Resource)?
                            {
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
                                for (edge, mut domain) in ctx
                                    .admit_iter(
                                        boundary_domains,
                                        "catia_standard_boundary_domain_rows",
                                    )?
                                    .enumerate()
                                {
                                    if deferred_port_edges[edge] {
                                        continue;
                                    }
                                    ctx.retain_vec(
                                        &mut domain,
                                        |pair| endpoint_pair_on_incident_faces(edge, *pair),
                                        "catia_standard_boundary_domain_faces",
                                    )
                                    .map_err(StandardTopologyError::Resource)?;
                                    // Filtering only removes pairs, so a row changes exactly
                                    // when it was empty and gains the domain or loses a pair.
                                    if options[edge].is_empty() {
                                        changed |= !domain.is_empty();
                                        options[edge] = domain;
                                    } else {
                                        let before = options[edge].len();
                                        ctx.retain_vec(
                                            &mut options[edge],
                                            |pair| {
                                                ctx.any_by(
                                                    &domain,
                                                    |candidate| {
                                                        Ok(missing_edge::same_unordered_pair(
                                                            *pair, *candidate,
                                                        ))
                                                    },
                                                    "catia_standard_boundary_domain_pairs",
                                                )
                                            },
                                            "catia_standard_boundary_pair_domain_filter",
                                        )
                                        .map_err(StandardTopologyError::Resource)?;
                                        changed |= options[edge].len() != before;
                                    }
                                }
                            }
                            if !changed {
                                break;
                            }
                        }
                        for (edge, _) in ctx
                            .admit_iter(
                                &supports,
                                "catia_standard_endpoint_option_filter_supports",
                            )?
                            .take(options.len())
                            .enumerate()
                        {
                            let pairs = &mut options[edge];
                            ctx.retain_vec(
                                pairs,
                                |pair| endpoint_pair_on_incident_faces(edge, *pair),
                                "catia_standard_endpoint_option_faces",
                            )
                            .map_err(StandardTopologyError::Resource)?;
                            ctx.sort_unstable_by(
                                pairs,
                                |value| value,
                                Ord::cmp,
                                "catia_standard_endpoint_pairs_sort",
                            )
                            .map_err(StandardTopologyError::Resource)?;
                            ctx.dedup_vec(pairs, "catia_standard_endpoint_pairs_dedup")
                                .map_err(StandardTopologyError::Resource)?;
                        }
                        for (edge, _) in ctx
                            .admit_iter(&supports, "catia_standard_endpoint_candidate_option_rows")?
                            .take(endpoint_candidates.len())
                            .take(options.len())
                            .enumerate()
                        {
                            let candidates = &mut endpoint_candidates[edge];
                            for point in ctx
                                .admit_iter(
                                    options[edge].as_slice(),
                                    "catia_standard_endpoint_option_points",
                                )?
                                .flat_map(|pair| [&pair[0], &pair[1]])
                            {
                                if !ctx
                                    .contains(
                                        candidates,
                                        point,
                                        "catia_standard_endpoint_candidate_point",
                                    )
                                    .map_err(StandardTopologyError::Resource)?
                                {
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
                            let Some(propagated) =
                                missing_edge::propagate_edge_port_points_with_ordered_seeds(
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
                            let choice_count = ctx.fold(
                                options,
                                0usize,
                                |total, choices| Ok(total + choices.len()),
                                "catia_standard_native_port_choice_count",
                            )?;
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
                    let propagated_endpoint_pairs = if let Some((options, ports)) =
                        endpoint_options.as_ref().zip(
                            missing_edge::edge_port_identities(ctx, spine)
                                .map_err(StandardTopologyError::Resource)?,
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
                            ctx.try_collect_vec(
                                propagated.iter().copied().zip(options.iter()).map(
                                    |(pair, candidates)| -> Result<_, CodecError> {
                                        let Some(pair) = pair else {
                                            return Ok(None);
                                        };
                                        let matches = ctx.any_by(
                                            candidates,
                                            |candidate| {
                                                Ok(*candidate == pair
                                                    || *candidate == [pair[1], pair[0]])
                                            },
                                            "catia_standard_propagated_candidates",
                                        )?;
                                        Ok(matches.then_some(pair))
                                    },
                                ),
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
                    let mut constrained_endpoint_options =
                        if let Some(options) = endpoint_options.as_ref() {
                            let mut copied = Vec::new();
                            for (edge, pairs) in ctx
                                .admit_iter(options, "catia_endpoint_option_copy")?
                                .enumerate()
                            {
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
                        let has_deferred_ports = ctx
                            .any_by(
                                &deferred_port_edges,
                                |deferred| Ok(*deferred),
                                "catia_standard_deferred_port_edges",
                            )
                            .map_err(StandardTopologyError::Resource)?;
                        let pruned = if has_deferred_ports {
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
                        let unique_pairs = if has_deferred_ports {
                            missing_edge::unique_mesh_edge_port_candidate_pairs_with_deferred(
                                ctx,
                                &ports,
                                options,
                                &deferred_port_edges,
                            )
                            .map_err(StandardTopologyError::Resource)?
                        } else {
                            missing_edge::unique_mesh_edge_port_candidate_pairs(
                                ctx, &ports, options,
                            )
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
                            for (edge, pair) in ctx
                                .admit_iter(&pairs, "catia_standard_unique_endpoint_option_rows")?
                                .take(options.len())
                                .enumerate()
                            {
                                if let Some(pair) = pair {
                                    ctx.retain_vec(
                                        &mut options[edge],
                                        |candidate| {
                                            Ok(missing_edge::same_unordered_pair(*candidate, *pair))
                                        },
                                        "catia_standard_unique_endpoint_options",
                                    )
                                    .map_err(StandardTopologyError::Resource)?;
                                }
                            }
                        }
                    }
                    if let Some(options) = &mut constrained_endpoint_options {
                        // A same-incidence row relation is not an endpoint identity. Keep its
                        // complete candidate domain for exact identity and mesh constraints.
                        for domain in ctx.admit_iter(
                            options.as_slice(),
                            "catia_standard_endpoint_domain_census",
                        )? {
                            match domain.len() {
                                0 => diagnostics.empty_endpoint_domains += 1,
                                1 => diagnostics.singleton_endpoint_domains += 1,
                                _ => diagnostics.multiple_endpoint_domains += 1,
                            }
                            diagnostics.endpoint_domain_choices += domain.len();
                        }
                    }
                    let resolved_endpoint_pairs = propagated_endpoint_pairs
                        .map(|pairs| {
                            ctx.collect_options(pairs, "catia_standard_resolved_endpoint_pairs")
                        })
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
                            topology::parse_fbb(ctx, spine)
                                .map_err(StandardTopologyError::Resource)?
                        }
                    } else {
                        let standard = fbb::parse_standard(ctx, spine)
                            .map_err(StandardTopologyError::Resource)?;
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
                        let Some(topology) =
                            (!has_open_face_domains).then_some(mesh_topology).flatten()
                        else {
                            return Ok(None);
                        };
                        let candidate_pairs = match resolved_endpoint_pairs.as_ref() {
                            Some(pairs) => Some(
                                ctx.copy_slice(pairs, "catia_standard_mesh_resolved_pair_copy")?,
                            ),
                            None => ctx.collect_options(
                                endpoint_candidates.iter().map(|candidates| {
                                    <[usize; 2]>::try_from(candidates.as_slice()).ok()
                                }),
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
                        let Some(point_assignment) =
                            topology.bind_vertex_points(ctx, &endpoint_pairs)?
                        else {
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
                            supports.iter().zip(endpoint_candidates.iter()).map(
                                |(support, candidates)| {
                                    match &support.geometry {
                    crate::families::standard::records::StandardCurveGeometry::Circle {
                        ..
                    } => <[usize; 2]>::try_from(candidates.as_slice()).ok(),
                    crate::families::standard::records::StandardCurveGeometry::Line
                    | crate::families::standard::records::StandardCurveGeometry::Bspline => None,
                }
                                },
                            ),
                            "catia_standard_circle_anchors",
                        )
                        .map_err(StandardTopologyError::Resource)?;
                    let native_fbb_topology =
                        if edge_table_form == EdgeTableForm::FbbOnly && !has_open_face_domains {
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
                    let (topology, point_assignment) = if let Some(bound) = mesh_bound {
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
        for point in ctx.admit_iter(&ir.model.points, "catia_solver_point_positions")? {
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
                for (edge, deferred) in ctx
                    .admit_iter(&solver_deferred_edges, "catia_solver_deferred_edge_rows")?
                    .copied()
                    .enumerate()
                {
                    if deferred && !edge_identity_evidence[edge] {
                        solver_options[edge].clear();
                    }
                }
                let endpoint_pairs_on_selected_faces =
                    |pairs: &[Option<[usize; 2]>]| -> Result<bool, CodecError> {
                        if pairs.len() != selected_supports.len() {
                            return Ok(false);
                        }
                        ctx.all_by(
                            selected_supports.iter().zip(pairs),
                            |(support, pair)| {
                                Ok(pair.is_none_or(|pair| {
                                    pair.iter().all(|&point| {
                                        support.faces.iter().all(|&face| point_on_face(face, point))
                                    })
                                }))
                            },
                            "catia_selected_face_pairs",
                        )
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
                        domains.iter()
                            .map(|domain| !domain.is_empty()),
                        "catia_standard_face_domain_edges",
                    )?,
                    None => ctx.alloc_filled(
                        solver_options.len(),
                        false,
                        "catia_standard_missing_face_domain_edges",
                    )?,
                };
                let selected_circle_constraint_edges = ctx.collect_vec(
                    selected_supports.iter()
                    .enumerate()
                    .map(|(edge, support)| {
                        matches!(
                            support.geometry,
                            crate::families::standard::records::StandardCurveGeometry::Circle { .. }
                        ) && solver_options[edge].len() > 1
                    }),
                    "catia_standard_circle_constraint_edges",
                )?;
                let partial_constraint_edges = ctx.collect_vec(
                    selected_circle_constraint_edges.iter()
                    .zip(
                        line_constraint.edge_roles.iter()
                        .map(|role| *role == EdgeLineRole::Flexible),
                    )
                    .zip(&face_domain_edges)
                    .map(|((circle, line), face)| *circle || line || *face),
                    "catia_standard_partial_constraint_edges",
                )?;
                let preferred_budget =
                    solve_budget.session_child_slice(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS);
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
                        partial_solution_valid: |pairs| -> Result<bool, CodecError> {
                            if !endpoint_pairs_on_selected_faces(pairs)? {
                                return Ok(false);
                            }
                            let Some(pairs) = line_constraint.edge_pairs(pairs) else {
                                return Ok(false);
                            };
                            line_constraint.is_valid(ctx, &pairs)
                        },
                        complete_solution_valid: |pairs| -> Result<bool, CodecError> {
                            if !endpoint_pairs_on_selected_faces(pairs)? {
                                return Ok(false);
                            }
                            let Some(pairs_for_lines) = line_constraint.edge_pairs(pairs) else {
                                return Ok(false);
                            };
                            if !line_constraint.is_simple(ctx, &pairs_for_lines)? {
                                return Ok(false);
                            }
                            circle_constraint.solution_is_simple(
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
                if solve_budget.consume_child(&preferred_budget).is_err() {
                    return Ok(mesh_quotient::MeshSolve::Failed(
                        mesh_quotient::MeshCandidateFailure::Exhausted(
                            mesh_quotient::MeshCandidateExhaustion::FaceDomainEnumeration,
                        ),
                    ));
                }
                let has_circle_preference = ctx.any_by(
                    &selected_circle_constraint_edges,
                    |constrained| Ok(*constrained),
                    "catia_standard_circle_preference_scan",
                )?;
                if has_circle_preference {
                    // Circular interval choice is a preference because both
                    // complementary arcs can be valid. The fallback relaxes
                    // only that choice; straight-carrier interval overlap is
                    // an invalid endpoint relation in both searches.
                    let fallback_budget = solve_budget
                        .session_child_slice(mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS);
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
                            partial_solution_valid: |pairs| -> Result<bool, CodecError> {
                                if !endpoint_pairs_on_selected_faces(pairs)? {
                                    return Ok(false);
                                }
                                let Some(pairs) = line_constraint.edge_pairs(pairs) else {
                                    return Ok(false);
                                };
                                line_constraint.is_simple(ctx, &pairs)
                            },
                            complete_solution_valid: |pairs| -> Result<bool, CodecError> {
                                if !endpoint_pairs_on_selected_faces(pairs)? {
                                    return Ok(false);
                                }
                                let Some(pairs) = line_constraint.edge_pairs(pairs) else {
                                    return Ok(false);
                                };
                                line_constraint.is_simple(ctx, &pairs)
                            },
                        },
                    )?;
                    if solve_budget.consume_child(&fallback_budget).is_err() {
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
                    let selected_supports = ctx.collect_vec(
                        supports
                            .iter()
                            .zip(selected_edge_faces)
                            .map(|(support, faces)| {
                                let mut selected = *support;
                                selected.faces = *faces;
                                selected
                            }),
                        "catia_selected_curve_supports",
                    )?;
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
        Ok(match outcome.require_work(ctx, work_budget)? {
            mesh_quotient::MeshSolve::Solved(candidate) => Some(candidate),
            mesh_quotient::MeshSolve::Failed(failure) => {
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
        if work_budget.exhausted() {
            ctx.charge_work(0, "catia_mesh_topology_work")
                .map_err(StandardTopologyError::Resource)?;
            return Err(StandardTopologyError::Resource(ctx.refuse_codec_limit(
                "catia_mesh_topology_work",
                0,
                1,
            )));
        }
        return Err((if matches!(
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
                        for (edge, faces) in ctx
                            .admit_iter(&edge_faces, "catia_standard_selected_face_supports")?
                            .take(supports.len())
                            .enumerate()
                        {
                            supports[edge].faces = *faces;
                        }
                    }
                    Ok((
                        surface_indices,
                        supports,
                        topology,
                        point_assignment,
                        native_supports_by_row,
                        limit_curve_bindings,
                        endpoint_candidates,
                    ))
                })())
            })
            .map_err(StandardTopologyError::Resource)??;
    let Some(edge_vertices) = validate_standard_topology(
        ir,
        annotations,
        &mut topology,
        &point_assignment,
        StandardTopologyValidation {
            supports: &supports,
            endpoint_candidates: &endpoint_candidates,
        },
        &mut topology_storage,
        admission,
    )
    .map_err(StandardTopologyError::Resource)?
    else {
        return Err(StandardTopologyFailure::InvalidTopologySolution.into());
    };
    let Some(topology) = topology_storage
        .with_storage(|| {
            crate::families::standard::topology::admitted::StandardTopology::new(ctx, topology)
        })
        .map_err(StandardTopologyError::Resource)?
    else {
        return Err(StandardTopologyFailure::InvalidTopologySolution.into());
    };
    let resolved_limit_curve_bindings = topology_storage
        .with_storage(|| -> Result<_, CodecError> {
            let mut resolved_limit_curve_bindings = Vec::new();
            ctx.reserve_vec(
                &mut resolved_limit_curve_bindings,
                edge_vertices.len(),
                "catia_resolved_limit_curve_bindings",
            )?;
            for (edge, logical_vertices) in ctx
                .admit_iter(&edge_vertices, "catia_resolved_limit_curve_bindings")?
                .enumerate()
            {
                let points = [
                    point_assignment[logical_vertices[0]],
                    point_assignment[logical_vertices[1]],
                ];
                resolved_limit_curve_bindings.push(resolve_standard_limit_curve_binding(
                    ctx,
                    &limit_curve_bindings[edge],
                    points,
                )?);
            }
            Ok(resolved_limit_curve_bindings)
        })
        .map_err(StandardTopologyError::Resource)?;
    *bound_limit_curve_count = ctx
        .admit_iter(
            &resolved_limit_curve_bindings,
            "catia_resolved_limit_curve_binding_count",
        )?
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
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    topology: &mut crate::families::standard::topology::StandardTopologyDraft,
    point_assignment: &[usize],
    validation: StandardTopologyValidation<'_>,
    output_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    let ctx = admission.context();
    let StandardTopologyValidation {
        supports,
        endpoint_candidates,
    } = validation;
    let face_count = ir.model.faces.len();
    if topology.face_count() != face_count
        || topology.edge_rows().len() != supports.len()
        || topology.vertex_points().len() != ir.model.points.len()
        || !ctx.all_by(
            topology.vertex_points().iter().zip(&ir.model.points),
            |(stored, point)| {
                let position = point.position().get();
                Ok(stored[0] == position.x && stored[1] == position.y && stored[2] == position.z)
            },
            "catia_standard_topology_vertex_points",
        )?
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
    let mut storage = ctx.reserve_scoped(0, "catia_standard_topology_validation_storage")?;
    let Some(body_kinds) = storage.with_storage(|| topology.body_kinds(ctx, &face_groups))? else {
        return Ok(None);
    };
    let Some(edge_vertices) = output_storage.with_storage(|| topology.edge_vertices(ctx))? else {
        return Ok(None);
    };
    if ctx.any_by(
        edge_vertices.iter().enumerate(),
        |(edge, vertices)| {
            const OPERATION: &str = "catia_standard_topology_endpoint_candidates";
            let candidates = &endpoint_candidates[edge];
            if candidates.is_empty() {
                return Ok(false);
            }
            let start = point_assignment[vertices[0]];
            let end = point_assignment[vertices[1]];
            Ok(!ctx.contains(candidates, &start, OPERATION)?
                || !ctx.contains(candidates, &end, OPERATION)?)
        },
        "catia_standard_topology_edge_vertices",
    )? {
        return Ok(None);
    }
    let mut bodies_by_id = HashMap::new();
    for (index, body) in ctx
        .admit_iter(&ir.model.bodies, "catia_standard_body_identity_rows")?
        .enumerate()
    {
        if ctx
            .get_hash_map(
                &bodies_by_id,
                body.id.as_str(),
                "catia_standard_body_identity_rows",
            )?
            .is_none()
        {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut bodies_by_id,
                    body.id.as_str(),
                    index,
                    "catia_standard_body_identity_rows",
                )
            })?;
        }
    }
    let mut body_arena_indices = Vec::new();
    {
        let mut visits = body_kinds.iter().enumerate();
        while let Some((body_index, _)) =
            ctx.next_charged(&mut visits, "catia_standard_body_kind_rows")?
        {
            let (id, _token) = ctx.format_scoped(
                format_args!("catia:standard:body#{body_index}"),
                "catia_standard_body_lookup_identity",
            )?;
            let Some(&index) = ctx.get_hash_map(
                &bodies_by_id,
                id.as_str(),
                "catia_standard_body_identity_rows",
            )?
            else {
                return Ok(None);
            };
            ctx.push_scoped_vec(
                &mut storage,
                &mut body_arena_indices,
                index,
                "catia_standard_body_arena_indices",
            )?;
        }
    }
    for (&arena_index, &kind) in ctx
        .admit_iter(&body_arena_indices, "catia_standard_body_arena_indices")?
        .zip(ctx.admit_iter(&body_kinds, "catia_standard_body_kinds")?)
    {
        ir.model.bodies[arena_index].kind = kind;
    }
    let components = storage.with_storage(|| topology.face_components(ctx))?;
    if !partition_standard_face_components(ctx, ir, annotations, &components, admission)? {
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
    topology: &crate::families::standard::topology::admitted::StandardTopology,
    face_index: usize,
    point_assignment: &[usize],
) -> Result<cadmpeg_ir::topology::FaceLoops, CodecError> {
    let Some(face_topology) = topology.faces().get(face_index) else {
        return Ok(cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()));
    };
    let mut storage = ctx.reserve_scoped(0, "catia_standard_face_loop_working_state")?;
    let ids = storage.with_storage(|| -> Result<_, CodecError> {
        let mut ids = Vec::new();
        ctx.reserve_vec(
            &mut ids,
            face_topology.boundaries().len(),
            "catia_standard_face_loop_ids",
        )?;
        for (loop_index, _) in ctx
            .admit_iter(
                face_topology.boundaries(),
                "catia_standard_face_loop_boundaries",
            )?
            .enumerate()
        {
            ids.push(standard_id(
                ctx,
                "loop",
                format_args!("{face_index}:{loop_index}"),
                LoopId::mint,
                "catia_standard_face_loop_identity",
            )?);
        }
        Ok(ids)
    })?;
    let unspecified = || -> Result<_, CodecError> {
        let mut copy = Vec::new();
        for id in ctx.admit_iter(&ids, "catia_standard_unspecified_loop_ids")? {
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
    let Some(surface) = face_surface(ctx, ir, bindings, surface_indices, face_index)? else {
        return unspecified();
    };
    let Some(rows) = storage.with_storage(|| -> Result<_, CodecError> {
        let mut rows = Vec::new();
        {
            let mut visits = (face_topology.boundaries()).iter().zip(ids.iter());
            while let Some((boundary, id)) =
                ctx.next_charged(&mut visits, "catia_standard_face_loop_boundaries")?
            {
                let mut points = Vec::new();
                {
                    let mut visits = boundary.coedges().iter();
                    while let Some(coedge) =
                        ctx.next_charged(&mut visits, "catia_standard_face_loop_coedges")?
                    {
                        let Some(point) = point_assignment
                            .get(coedge.start_vertex)
                            .and_then(|index| ir.model.points.get(*index))
                        else {
                            return Ok(None);
                        };
                        ctx.push_vec(
                            &mut points,
                            point.position().get(),
                            "catia_standard_planar_loop_points",
                        )?;
                    }
                }
                let id = id.try_clone_for_decode(ctx, "catia_standard_planar_loop_id_copy")?;
                ctx.push_vec(&mut rows, (id, points), "catia_standard_planar_loop_rows")?;
            }
        }
        Ok(Some(rows))
    })?
    else {
        return unspecified();
    };
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
    AnnotationAccount,
> {
    ir: &'input0 mut CadIr,
    annotations: &'input1 mut AnnotationBuilder<AnnotationAccount>,
    bindings: &'input2 [(SurfaceId, bool, usize)],
    brep: &'input3 [u8],
    surface_indices: &'input4 HashMap<SurfaceId, usize>,
    supports: &'input5 [crate::families::standard::records::StandardCurveSupport],
    edge_vertices: &'input6 [[usize; 2]],
    point_assignment: &'input7 [usize],
    topology: &'input8 crate::families::standard::topology::admitted::StandardTopology,
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
        impl cadmpeg_ir::annotations::AnnotationStorage,
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
    if topology.vertex_points().len() != ir.model.points.len()
        || topology.edge_rows().len() != edge_vertices.len()
        || topology.logical_vertex_count() != point_assignment.len()
    {
        return Err(CodecError::malformed(
            "admitted topology does not match its emission tables",
        ));
    }

    let mut native_surfaces = edge_geometry::NativeSurfaceIndex::new(admission.context())?;
    let mut edge_storage = ctx.reserve_scoped(0, "catia_standard_edge_reversed_storage")?;
    let mut edge_reversed = Vec::new();
    edge_storage.with_storage(|| {
        ctx.reserve_vec(
            &mut edge_reversed,
            supports.len(),
            "catia_standard_edge_reverse_flags",
        )
    })?;
    for (edge_index, (support, logical_vertices)) in ctx
        .admit_iter(supports, "catia_standard_emission_edge_supports")?
        .zip(ctx.admit_iter(edge_vertices, "catia_standard_emission_edge_vertices")?)
        .enumerate()
    {
        let start_point = point_assignment[logical_vertices[0]];
        let end_point = point_assignment[logical_vertices[1]];
        let native_support = match native_edge_supports
            .get(edge_index)
            .and_then(Option::as_ref)
        {
            Some(native)
                if standard_native_support_endpoint_pair(
                    ctx,
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
                native_surfaces: &mut native_surfaces,
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
            crate::resource::derived_annotation(ctx, annotations, &id, "curve")?;
        }
        crate::resource::derived_annotation(ctx, annotations, &id, "start")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "end")?;
        if param_range.is_some() {
            crate::resource::derived_annotation(ctx, annotations, &id, "param_range")?;
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

    let mut emission_storage = ctx.reserve_scoped(0, "catia_standard_emission_storage")?;
    let mut curve_indices = HashMap::new();
    for (index, curve) in ctx
        .admit_iter(&ir.model.curves, "catia_standard_curve_index_rows")?
        .enumerate()
    {
        emission_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut curve_indices,
                &curve.id,
                index,
                "catia_standard_curve_indices",
            )
        })?;
    }
    let mut edge_coedges = ctx.collect_indexed_vec(
        ir.model.edges.len(),
        "catia_standard_edge_coedge_rows",
        |_| Ok(Vec::new()),
    )?;
    for (face_index, face_topology) in ctx
        .admit_iter(topology.faces(), "catia_standard_topology_faces")?
        .enumerate()
    {
        let face_loops = standard_face_loops(
            admission.context(),
            ir,
            bindings,
            surface_indices,
            topology,
            face_index,
            point_assignment,
        )?;
        let face_surface_index = bindings
            .get(face_index)
            .map(|(id, _, _)| {
                ctx.get_hash_map(surface_indices, id, "catia_standard_face_surface_lookup")
            })
            .transpose()?
            .flatten()
            .copied()
            .ok_or_else(|| CodecError::malformed("standard face has no bound surface"))?;
        for (loop_index, boundary) in ctx
            .admit_iter(
                face_topology.boundaries(),
                "catia_standard_topology_boundaries",
            )?
            .enumerate()
        {
            let loop_id = standard_id(
                ctx,
                "loop",
                format_args!("{face_index}:{loop_index}"),
                LoopId::mint,
                "catia_standard_loop_identity",
            )?;
            let mut vertices = Vec::new();
            for edge_use in ctx.admit_iter(boundary.coedges(), "catia_standard_boundary_coedges")? {
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
            let ring = cadmpeg_ir::topology::LoopRing::new(ctx, coedges, vertex_uses)
                .map_err(cadmpeg_core::CodecError::from)?
                .map_err(CodecError::malformed)?;
            let coedge_ids = ring.coedges();
            for (coedge_index, edge_use) in ctx
                .admit_iter(boundary.coedges(), "catia_standard_boundary_pcurve_coedges")?
                .enumerate()
            {
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
                    .map(|id| {
                        ctx.get_hash_map(&curve_indices, &id, "catia_standard_curve_index_lookup")
                    })
                    .transpose()?
                    .flatten()
                    .map(|index| &ir.model.curves[*index].geometry);
                let pcurve_id = standard_pcurve_geometry(ctx,
&ir.model.surfaces[face_surface_index].geometry,
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
                    crate::resource::derived_annotation(ctx, annotations, &id, "geometry")?;
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
                    crate::resource::derived_annotation(ctx, annotations, &id, field)?;
                }
                if pcurve_id.is_some() {
                    crate::resource::derived_annotation(ctx, annotations, &id, "pcurves")?;
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
            crate::resource::derived_annotation(ctx, annotations, &loop_id, "face")?;
            crate::resource::derived_annotation(ctx, annotations, &loop_id, "coedges")?;
            crate::resource::derived_annotation(ctx, annotations, &loop_id, "vertex_uses")?;
            if face_loops.role(&loop_id) != LoopBoundaryRole::Unspecified {
                crate::resource::derived_annotation(ctx, annotations, &loop_id, "boundary_role")?;
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
    for uses in ctx.admit_iter(&edge_coedges, "catia_standard_edge_coedge_rows")? {
        for (position, current) in ctx
            .admit_iter(uses, "catia_standard_edge_coedge_entries")?
            .enumerate()
        {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
    parameters: [f64; N],
) -> Result<[Option<Point3>; N], cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "geometry helper boundary")?;
    let mut points = [None; N];
    for (index, parameter) in parameters.into_iter().enumerate() {
        let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
            pcurve,
            parameter,
        ))?
        else {
            continue;
        };
        points[index] = match cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
            surface,
            uv.u,
            uv.v,
        ) {
            Ok(point) => Some(point.get()),
            Err(failure) => failure.non_finite()?,
        };
    }
    Ok(points)
}

fn standard_native_support_endpoint_pair(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    support: &StandardEdgeSupport,
    points: &[Point],
    candidates: &[usize],
    required_pair: Option<[usize; 2]>,
) -> Result<Option<[usize; 2]>, cadmpeg_core::decode::ResourceLimit> {
    const VERTEX_MATCH_TOLERANCE: f64 = 2e-3;
    ctx.charge_work_limit(0, "geometry helper boundary")?;
    if points.is_empty() {
        return Ok(None);
    }

    let mut lifted = [[None; 2]; 2];
    for (index, (carrier, pcurve)) in support.carriers.iter().zip(&support.pcurves).enumerate() {
        let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) = carrier
        else {
            return Ok(None);
        };
        lifted[index] =
            lifted_standard_support_parameters(ctx, surface, pcurve, support.parameter_range)?;
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
    let point_for =
        |expected: Point3| -> Result<Option<usize>, cadmpeg_core::decode::ResourceLimit> {
            let mut selected = None;
            let unique = ctx.all_by_limit(
                candidates,
                |&candidate| {
                    let matches = points.get(candidate).is_some_and(|point| {
                        point.position().get().distance_squared(expected).sqrt()
                            <= VERTEX_MATCH_TOLERANCE
                    });
                    Ok(!matches || selected.replace(candidate).is_none())
                },
                "catia native support endpoint candidate",
            )?;
            Ok(selected.filter(|_| unique))
        };
    let pair = [point_for(first[0])?, point_for(first[1])?];
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
    for points in ctx.admit_iter(candidates, "catia_standard_initial_endpoint_candidates")? {
        let row = if let Ok(pair) = <[usize; 2]>::try_from(points.as_slice()) {
            ctx.alloc_filled(1, pair, "catia_standard_initial_endpoint_pair")?
        } else {
            Vec::new()
        };
        resolved.push(row);
    }
    for (edge, support) in ctx
        .admit_iter(supports, "catia_standard_edge_class_supports")?
        .enumerate()
    {
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
            for (left, &start) in ctx
                .admit_iter(
                    &candidates[edge],
                    "catia_standard_circle_endpoint_candidates",
                )?
                .enumerate()
            {
                let first_end = left + usize::from(!include_full_circle_seams);
                for &end in ctx.admit_iter(
                    &candidates[edge][first_end..],
                    "catia_standard_circle_endpoint_candidates",
                )? {
                    pairs.push([start, end]);
                }
            }
            resolved[edge] = pairs;
        }
    }
    let mut line_groups = BTreeMap::<[usize; 2], Vec<usize>>::new();
    for (edge, support) in ctx
        .admit_iter(supports, "catia_standard_line_supports")?
        .enumerate()
    {
        if !resolved[edge].is_empty() {
            continue;
        }
        let [first, second] = support.faces;
        let faces = [first.min(second), first.max(second)];
        let line_like = match support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Line => true,
            crate::families::standard::records::StandardCurveGeometry::Bspline => {
                let surfaces = [
                    face_surface(ctx, ir, bindings, surface_indices, faces[0])?,
                    face_surface(ctx, ir, bindings, surface_indices, faces[1])?,
                ]
                .map(|surface| surface.map(|surface| &surface.geometry));
                matches!(surfaces, [Some(left), Some(right)] if intersection_line_direction(left, right).is_some())
            }
            crate::families::standard::records::StandardCurveGeometry::Circle { .. } => false,
        };
        if line_like {
            ctx.push_btree_group(
                &mut line_groups,
                faces,
                edge,
                "catia_standard_line_groups",
                "catia_standard_line_group_edges",
            )?;
        }
    }
    {
        let mut visits = line_groups.iter();
        while let Some((faces, edges)) =
            ctx.next_charged(&mut visits, "catia_standard_line_groups")?
        {
            let Some(surface0) = face_surface(ctx, ir, bindings, surface_indices, faces[0])? else {
                return Ok(None);
            };
            let Some(surface1) = face_surface(ctx, ir, bindings, surface_indices, faces[1])? else {
                return Ok(None);
            };
            let direction = intersection_line_direction(&surface0.geometry, &surface1.geometry);
            let same_cone_surface = matches!(
                (&surface0.geometry, &surface1.geometry),
                (
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)),
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_))
                ) // Surface geometry has no decode cost: this comparison is unpriced.
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
            {
                let mut visits = points.iter().enumerate();
                while let Some((left, &start)) =
                    ctx.next_charged(&mut visits, "catia_standard_line_endpoint_candidates")?
                {
                    {
                        let mut visits = points[left + 1..].iter();
                        while let Some(&end_index) = ctx
                            .next_charged(&mut visits, "catia_standard_line_endpoint_candidates")?
                        {
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
                                let direction_norm =
                                    direction.x.hypot(direction.y).hypot(direction.z);
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
                                && point_on_surface(ctx, midpoint, &surface0.geometry)?
                                && point_on_surface(ctx, midpoint, &surface1.geometry)?
                            {
                                ctx.push_vec(
                                    &mut pairs,
                                    [points[left], end_index],
                                    "catia_standard_line_endpoint_pairs",
                                )?;
                            }
                        }
                    }
                }
            }
            ctx.sort_unstable_by(
                &mut pairs,
                |value| value,
                Ord::cmp,
                "catia standard line endpoint pairs sort",
            )?;
            ctx.dedup_vec(&mut pairs, "catia standard line endpoint pairs dedup")?;
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
                for edge in ctx.admit_iter(edges, "catia_standard_line_group_edges")? {
                    resolved[*edge] = ctx.copy_slice(&pairs, "catia_standard_line_pair_copy")?;
                }
            }
        }
    }
    let mut fallback_relation_budget = 65_536usize;
    for (edge, _) in ctx
        .admit_iter(
            candidates,
            "catia_standard_resolved_endpoint_candidate_rows",
        )?
        .take(resolved.len())
        .enumerate()
    {
        let pairs = &mut resolved[edge];
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
        for (left, &start) in ctx
            .admit_iter(points, "catia_standard_fallback_endpoint_candidates")?
            .enumerate()
        {
            fallback.extend(
                ctx.admit_iter(
                    &points[left + 1..],
                    "catia_standard_fallback_endpoint_candidates",
                )?
                .map(|&end| [start, end]),
            );
        }
        *pairs = fallback;
    }
    Ok(Some(resolved))
}

fn standard_curve_edge_classes(
    ctx: &DecodeContext<'_>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
) -> Result<Vec<usize>, CodecError> {
    // Two rows share a class when they join the same unordered face pair with
    // the same line, or the same circle bit for bit; the class is the first
    // such row.
    let mut first_rows = HashMap::<([usize; 2], Option<[u64; 4]>), usize>::new();
    let mut storage = ctx.reserve_scoped(0, "catia_standard_edge_class_index")?;
    let mut classes = Vec::new();
    ctx.reserve_vec(&mut classes, supports.len(), "catia_standard_edge_classes")?;
    for (edge, support) in ctx
        .admit_iter(supports, "catia_standard_edge_class_supports")?
        .enumerate()
    {
        let [first, second] = support.faces;
        let faces = [first.min(second), first.max(second)];
        let curve = match &support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Line => None,
            crate::families::standard::records::StandardCurveGeometry::Circle {
                center,
                radius,
            } => Some([
                center.x.to_bits(),
                center.y.to_bits(),
                center.z.to_bits(),
                radius.get().to_bits(),
            ]),
            crate::families::standard::records::StandardCurveGeometry::Bspline => {
                classes.push(edge);
                continue;
            }
        };
        let key = (faces, curve);
        let class = match ctx.get_hash_map(&first_rows, &key, "catia_standard_edge_class_index")? {
            Some(&class) => class,
            None => {
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut first_rows,
                        key,
                        edge,
                        "catia_standard_edge_class_index",
                    )
                })?;
                edge
            }
        };
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
    for support in ctx.admit_iter(supports, "catia_standard_geometry_gauge_keys")? {
        keys.push(match &support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Line => {
                MeshEdgeGeometry::Line
            }
            crate::families::standard::records::StandardCurveGeometry::Circle {
                center,
                radius,
            } => MeshEdgeGeometry::Circle {
                center: [center.x.to_bits(), center.y.to_bits(), center.z.to_bits()],
                radius: radius.get().to_bits(),
            },
            crate::families::standard::records::StandardCurveGeometry::Bspline => {
                MeshEdgeGeometry::Bspline
            }
        });
    }
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
    for (index, point) in ctx
        .admit_iter(points, "catia_circle_endpoint_points")?
        .enumerate()
    {
        let on_circle =
            (point.position().get().distance_squared(center).sqrt() - radius).abs() <= 1e-3;
        let mut incident = true;
        if let Some(faces) = faces {
            for (surface, bounds) in faces {
                if !point_on_standard_face(ctx, point.position().get(), surface, bounds)? {
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
    let mut storage = ctx.reserve_scoped(0, "catia_native_identity_points")?;
    let identity_points = storage.with_storage(|| {
        unique_native_identity_points(
            ctx,
            graph.vertices.logical_vertices(),
            graph.vertices.raw_points().len(),
            &graph.vertex_tolerances,
            points,
        )
    })?;
    let mut pairs = Vec::new();
    ctx.reserve_vec(&mut pairs, supports.len(), "catia_graph_endpoint_pairs")?;
    for support in ctx.admit_iter(supports, "catia_graph_endpoint_pairs")? {
        pairs.push(native_identity_pair(
            ctx,
            native_edges,
            &identity_points,
            support.tag,
            "catia_graph_endpoint_pairs",
        )?);
    }
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
    let mut storage = ctx.reserve_scoped(0, "catia_roster_point_identities")?;
    let mut point_by_identity = HashMap::new();
    {
        let mut visits = vertex_roster.iter().copied().enumerate();
        while let Some((point, identity)) =
            ctx.next_charged(&mut visits, "catia_roster_vertex_identities")?
        {
            if storage
                .with_storage(|| {
                    ctx.insert_hash_map(
                        &mut point_by_identity,
                        identity,
                        point,
                        "catia_roster_point_identities",
                    )
                })?
                .is_some()
            {
                return Ok(None);
            }
        }
    }
    let mut pairs = Vec::new();
    ctx.reserve_vec(&mut pairs, supports.len(), "catia_roster_endpoint_pairs")?;
    for support in ctx.admit_iter(supports, "catia_roster_endpoint_pairs")? {
        pairs.push(native_identity_pair(
            ctx,
            native_edges,
            &point_by_identity,
            support.tag,
            "catia_roster_endpoint_pairs",
        )?);
    }
    Ok(Some(pairs))
}

/// Maps a row tag through its native edge to the two points its vertex
/// identities name.
fn native_identity_pair(
    ctx: &DecodeContext<'_>,
    native_edges: &BTreeMap<u32, [u32; 2]>,
    points: &HashMap<u32, usize>,
    tag: u32,
    operation: &'static str,
) -> Result<Option<[usize; 2]>, CodecError> {
    let Some([start, end]) = ctx.get_btree_map(native_edges, &tag, operation)? else {
        return Ok(None);
    };
    let Some(&start) = ctx.get_hash_map(points, start, operation)? else {
        return Ok(None);
    };
    let Some(&end) = ctx.get_hash_map(points, end, operation)? else {
        return Ok(None);
    };
    Ok(Some([start, end]))
}

fn merge_standard_edge_vertex_references<T>(
    ctx: &DecodeContext<'_>,
    target: &mut BTreeMap<u32, [u32; 2]>,
    source: &BTreeMap<u32, T>,
    endpoints: impl Fn(&T) -> [u32; 2],
) -> Result<bool, CodecError> {
    {
        let mut visits = source.iter();
        while let Some((&edge, record)) =
            ctx.next_charged(&mut visits, "catia_standard_e5_topology_edges")?
        {
            let vertices = endpoints(record);
            match ctx.entry_btree_map(target, edge, "catia_standard_e5_topology_edges")? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(vertices);
                }
                std::collections::btree_map::Entry::Occupied(entry) if *entry.get() == vertices => {
                }
                std::collections::btree_map::Entry::Occupied(_) => return Ok(false),
            }
        }
    }
    Ok(true)
}

/// Resolve native two-sided edge carriers by equal standard and native identities.
/// Returns, for each support row, its tag when that tag has a native support
/// and names no other row.
pub(super) fn standard_native_support_edge_ids<V>(
    ctx: &DecodeContext<'_>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    native_supports: &HashMap<u32, V>,
) -> Result<Vec<Option<u32>>, CodecError> {
    const OPERATION: &str = "catia_native_support_row_counts";
    let mut count_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut exact_row_counts = HashMap::<u32, usize>::new();
    for support in ctx.admit_iter(supports, "catia_native_support_edge_rows")? {
        if ctx.contains_key_hash_map(native_supports, &support.tag, OPERATION)? {
            count_storage.with_storage(|| -> Result<(), CodecError> {
                *ctx.entry_hash_map(&mut exact_row_counts, support.tag, OPERATION)?
                    .or_default() += 1;
                Ok(())
            })?;
        }
    }
    ctx.try_collect_vec(
        supports.iter().map(|support| -> Result<_, CodecError> {
            Ok(
                (ctx.get_hash_map(&exact_row_counts, &support.tag, OPERATION)? == Some(&1))
                    .then_some(support.tag),
            )
        }),
        "catia_native_support_edge_ids",
    )
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
    for (edge, pair) in ctx
        .admit_iter(pairs, "catia_native_endpoint_candidate_rows")?
        .take(candidates.len())
        .enumerate()
    {
        let candidates = &mut candidates[edge];
        if let Some(pair) = pair {
            for point in pair {
                if !ctx.contains(candidates, point, "catia_native_endpoint_domain_points")? {
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
        (_, Some(mesh))
            if ctx.all_by(
                &mesh,
                |pair| Ok(pair.is_some()),
                "catia_mesh_propagated_endpoint_pairs",
            )? =>
        {
            mesh
        }
        (Some(raw), _)
            if ctx.all_by(
                &raw,
                |pair| Ok(pair.is_some()),
                "catia_raw_propagated_endpoint_pairs",
            )? =>
        {
            raw
        }
        (Some(raw), Some(mesh)) => {
            let mut merged = Vec::new();
            ctx.reserve_vec(&mut merged, raw.len(), "catia_propagated_pair_merge")?;
            for (raw, mesh) in ctx
                .admit_iter(raw, "catia_propagated_pair_merge")?
                .zip(mesh)
            {
                merged.push(match (raw, mesh) {
                    (Some(raw), Some(mesh)) if raw == mesh || raw == [mesh[1], mesh[0]] => {
                        Some(raw)
                    }
                    (Some(_), Some(_)) => None,
                    (Some(pair), None) | (None, Some(pair)) => Some(pair),
                    (None, None) => None,
                });
            }
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
            if ctx.all_by(
                roster,
                |pair| Ok(pair.is_some()),
                "catia_native_roster_endpoint_pairs",
            )? {
                return Ok(Ok(Some(
                    ctx.copy_slice(roster, "catia_native_roster_evidence_copy")?,
                )));
            }
            let mut merged = Vec::new();
            ctx.reserve_vec(
                &mut merged,
                graph.len(),
                "catia_native_endpoint_merged_evidence",
            )?;
            let mut pairs = graph.iter().zip(roster);
            while let Some((graph, roster)) =
                ctx.next_charged(&mut pairs, "catia_native_graph_endpoint_pairs")?
            {
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
            ctx.copy_slice(pairs, "catia_native_endpoint_evidence_copy")?,
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
    let mut storage = ctx.reserve_scoped(0, "catia_successor_point_identities")?;
    let mut point_by_identity = HashMap::new();
    for (point, identity) in ctx
        .admit_iter(vertex_roster, "catia_successor_vertex_identities")?
        .copied()
        .enumerate()
    {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut point_by_identity,
                identity,
                point,
                "catia_successor_point_identities",
            )
        })?;
    }
    let mut successors = Vec::new();
    ctx.reserve_vec(
        &mut successors,
        supports.len(),
        "catia_successor_endpoint_points",
    )?;
    let successor = |identity: Option<u32>| -> Result<Option<usize>, CodecError> {
        let Some(identity) = identity else {
            return Ok(None);
        };
        Ok(ctx
            .get_hash_map(
                &point_by_identity,
                &identity,
                "catia_successor_point_identities",
            )?
            .copied())
    };
    for support in ctx.admit_iter(supports, "catia_successor_endpoint_points")? {
        successors.push([
            successor(support.tag.checked_add(1))?,
            successor(support.tag.checked_add(2))?,
        ]);
    }
    Ok(successors)
}

fn corroborate_successor_endpoint_points(
    ctx: &DecodeContext<'_>,
    options: &mut [Vec<[usize; 2]>],
    points: &[[Option<usize>; 2]],
) -> Result<(), CodecError> {
    for (edge, points) in ctx
        .admit_iter(points, "catia_successor_endpoint_evidence_rows")?
        .take(options.len())
        .enumerate()
    {
        let options = &mut options[edge];
        for point in points.iter().flatten() {
            if ctx.any_by(
                options.as_slice(),
                |pair| Ok(pair.contains(point)),
                "catia_successor_endpoint_domain",
            )? {
                ctx.retain_vec(
                    options,
                    |pair| Ok(pair.contains(point)),
                    "catia_successor_endpoint_domain",
                )?;
            }
        }
    }
    Ok(())
}

fn unique_native_identity_points(
    ctx: &DecodeContext<'_>,
    vertices: &[crate::families::b5::graph::B5LogicalVertex],
    raw_point_count: usize,
    tolerances: &BTreeMap<usize, cadmpeg_ir::scalar::PositiveReal>,
    points: &[Point],
) -> Result<HashMap<u32, usize>, CodecError> {
    const MATCH_TOLERANCE: f64 = 2e-3;

    let mut storage = ctx.reserve_scoped(0, "catia_native_identity_point_order")?;
    let mut entries = storage.with_storage(|| {
        ctx.collect_vec(
            points.iter().enumerate().map(|(item, point)| BoundsEntry {
                item,
                bounds: point_bounds(point.position().get(), 0.0),
            }),
            "catia_native_identity_point_order",
        )
    })?;
    let index = BoundsIndex::new(ctx, &mut entries, "catia_native_identity_point_order")?;
    let mut matches = HashMap::new();
    {
        let mut visits = vertices.iter().enumerate();
        while let Some((rank, vertex)) =
            ctx.next_charged(&mut visits, "catia_native_logical_vertices")?
        {
            let row = raw_point_count.checked_add(rank).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_native_vertex_tolerance_row", u64::MAX, u64::MAX)
            })?;
            let tolerance = ctx
                .get_btree_map(tolerances, &row, "catia_native_vertex_tolerance_row")?
                .map_or(MATCH_TOLERANCE, |tolerance| tolerance.get())
                .max(MATCH_TOLERANCE);
            let target = vertex.point.get();
            let bounds = point_bounds(target, tolerance);
            let mut matched = None;
            let ambiguous = ctx.any_by(
                index.overlapping(bounds),
                |node| {
                    let Some(point) = node.item.filter(|_| bounds_overlap(node.bounds, bounds))
                    else {
                        return Ok(false);
                    };
                    Ok(points[point]
                        .position()
                        .get()
                        .distance_squared(target)
                        .sqrt()
                        <= tolerance
                        && matched.replace(point).is_some())
                },
                "catia_native_identity_point_match",
            )?;
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
    }
    Ok(matches)
}

fn standard_plane_normals_from_face_frames(
    ctx: &DecodeContext<'_>,
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    face_frame_vectors: &[Option<FiniteVector<3>>],
) -> Result<HashMap<u32, FiniteVector<3>>, CodecError> {
    let mut normals = HashMap::<u32, FiniteVector<3>>::new();
    let mut conflicting = HashSet::new();
    for (face, record) in ctx
        .admit_iter(records, "catia_standard_plane_surface_records")?
        .enumerate()
    {
        const OPERATION: &str = "catia_plane_normals";
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
        if ctx.contains_hash_set(&conflicting, &prefix.target, OPERATION)? {
            continue;
        }
        match ctx
            .get_hash_map(&normals, &prefix.target, OPERATION)?
            .copied()
        {
            Some(stored) if stored != normal => {
                ctx.remove_hash_map(&mut normals, &prefix.target, OPERATION)?;
                ctx.insert_hash_set(
                    &mut conflicting,
                    prefix.target,
                    "catia_plane_normal_conflicts",
                )?;
            }
            Some(_) => {}
            None => {
                ctx.insert_hash_map(&mut normals, prefix.target, normal, OPERATION)?;
            }
        }
    }
    Ok(normals)
}

fn face_surface<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    face: usize,
) -> Result<Option<&'a Surface>, CodecError> {
    let Some((id, _, _)) = bindings.get(face) else {
        return Ok(None);
    };
    Ok(ctx
        .get_hash_map(surface_indices, id, "catia_standard_face_surface_lookup")?
        .and_then(|&index| ir.model.surfaces.get(index)))
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
        ctx.collect_indexed_vec(bindings.len(), "catia_face_membership_rows", |_| {
            Ok(Vec::new())
        })?;
    for (face, _) in ctx
        .admit_iter(bindings, "catia_face_membership_binding_rows")?
        .take(memberships.len())
        .enumerate()
    {
        let membership = &mut memberships[face];
        let Some(surface) = face_surface(ctx, ir, bindings, surface_indices, face)? else {
            continue;
        };
        let bounds = face_bounds
            .and_then(|bounds| bounds.get(face).copied())
            .flatten();
        *membership =
            ctx.alloc_filled(ir.model.points.len(), false, "catia_face_point_membership")?;
        for (point, candidate) in ctx
            .admit_iter(&ir.model.points, "catia_face_membership_points")?
            .enumerate()
        {
            membership[point] =
                point_on_standard_face(ctx, candidate.position().get(), &surface.geometry, bounds)?;
        }
    }
    Ok(memberships)
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
    ctx: &DecodeContext<'_>,
    edge_faces: &[[usize; 2]],
    allowed_faces: &mut [Vec<usize>],
    face_bounds: Option<&[Option<crate::families::standard::records::StandardFaceBounds>]>,
    face_geometries: Option<&[&SurfaceGeometry]>,
    edge_geometries: &[&crate::families::standard::records::StandardCurveGeometry],
) -> Result<(), CodecError> {
    let Some(face_bounds) = face_bounds else {
        return Ok(());
    };
    // Domains without a serialized edge-face row have no candidate to refine.
    for (edge, _) in ctx
        .admit_iter(edge_faces, "catia_standard_repeated_face_geometry_rows")?
        .take(allowed_faces.len())
        .enumerate()
    {
        let alternatives = &mut allowed_faces[edge];
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
        if ctx.any_by(
            alternatives.as_slice(),
            |face| Ok(face_bounds.get(*face).copied().flatten().is_none()),
            "catia_standard_face_alternate_bounds",
        )? {
            continue;
        }
        let circular_support = matches!(
            edge_geometries.get(edge),
            Some(crate::families::standard::records::StandardCurveGeometry::Circle { .. })
        );
        let has_unknown_geometry = if circular_support {
            match face_geometries {
                None => true,
                Some(geometries) => {
                    let unknown = |face: &usize| {
                        Ok(matches!(
                            geometries.get(*face),
                            Some(SurfaceGeometry::Solved(
                                SolvedSurfaceGeometry::Unknown { .. }
                            ))
                        ))
                    };
                    unknown(&serialized_face)?
                        || ctx.any_by(
                            alternatives.as_slice(),
                            unknown,
                            "catia_standard_face_alternate_geometries",
                        )?
                }
            }
        } else {
            false
        };
        if has_unknown_geometry {
            continue;
        }
        let score = |face: usize| -> Result<(usize, usize), CodecError> {
            // Surface geometry has no decode cost: this comparison is unpriced.
            let distinct_circle_carrier = circular_support
                && face_geometries.is_some_and(|geometries| {
                    geometries
                        .get(serialized_face)
                        .zip(geometries.get(face))
                        .is_some_and(|(left, right)| left != right)
                });
            let overlap_dimension = match face_bounds.get(face).copied().flatten() {
                Some(candidate) => (0..3)
                    .filter(|axis| {
                        let axis = *axis;
                        let left = serialized_bounds.aabb_center[axis].get()
                            - serialized_bounds.aabb_half_extents[axis].get();
                        let right = serialized_bounds.aabb_center[axis].get()
                            + serialized_bounds.aabb_half_extents[axis].get();
                        let candidate_left = candidate.aabb_center[axis].get()
                            - candidate.aabb_half_extents[axis].get();
                        let candidate_right = candidate.aabb_center[axis].get()
                            + candidate.aabb_half_extents[axis].get();
                        right.min(candidate_right) - left.max(candidate_left)
                            > STANDARD_FACE_BOUNDS_TOLERANCE
                    })
                    .count(),
                None => 0,
            };
            Ok((usize::from(distinct_circle_carrier), overlap_dimension))
        };
        let mut best = (0, 0);
        let mut best_count = 0usize;
        for face in ctx
            .admit_iter(
                alternatives.as_slice(),
                "catia_standard_face_alternate_scores",
            )?
            .copied()
        {
            let candidate = score(face)?;
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
        ctx.retain_vec(
            alternatives,
            |face| Ok(score(*face)? == best),
            "catia_standard_face_alternate_score_filter",
        )?;
    }
    Ok(())
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
    let mut group_storage = ctx.reserve_scoped(0, "catia_shared_boundary_groups")?;
    let mut groups = BTreeMap::<[usize; 2], Vec<usize>>::new();
    for (edge, support) in ctx
        .admit_iter(supports, "catia_shared_boundary_supports")?
        .enumerate()
    {
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
        let [left, right] = support.faces;
        group_storage.with_storage(|| {
            ctx.push_btree_group(
                &mut groups,
                [left.min(right), left.max(right)],
                edge,
                "catia_shared_boundary_groups",
                "catia_shared_boundary_group_edges",
            )
        })?;
    }
    for (_, edges) in ctx.admit_iter(&groups, "catia_shared_boundary_groups")? {
        if edges.len() < 2 {
            continue;
        }
        if ctx.any_by(
            edges,
            |edge| Ok(!boundary_witnesses[*edge]),
            "catia_shared_boundary_group_edges",
        )? {
            for edge in ctx
                .admit_iter(edges, "catia_shared_boundary_restore_edges")?
                .copied()
            {
                filtered[edge] =
                    ctx.copy_slice(&original[edge], "catia_shared_boundary_original_domain")?;
            }
            continue;
        }
        let mut pair_storage = ctx.reserve_scoped(0, "catia_shared_boundary_filtered_pairs")?;
        let mut filtered_pairs = HashSet::new();
        for edge in ctx.admit_iter(edges, "catia_shared_boundary_pair_edges")? {
            for &[start, end] in
                ctx.admit_iter(&filtered[*edge], "catia_shared_boundary_edge_pairs")?
            {
                pair_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut filtered_pairs,
                        [start.min(end), start.max(end)],
                        "catia_shared_boundary_filtered_pairs",
                    )
                })?;
            }
        }
        if filtered_pairs.len() >= edges.len() {
            continue;
        }
        for edge in ctx
            .admit_iter(edges, "catia_shared_boundary_restore_edges")?
            .copied()
        {
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
    let mut witness_storage = ctx.reserve_scoped(0, "catia_selected_face_witnesses")?;
    let mut boundary_witnesses = Vec::new();
    for (edge, support) in ctx
        .admit_iter(supports, "catia_selected_face_supports")?
        .enumerate()
    {
        let Some(pairs) = options.get(edge) else {
            ctx.push_vec(
                &mut filtered_options,
                Vec::new(),
                "catia_selected_face_option_rows",
            )?;
            witness_storage.with_storage(|| {
                ctx.push_vec(
                    &mut boundary_witnesses,
                    false,
                    "catia_selected_face_witnesses",
                )
            })?;
            continue;
        };
        let (filtered, witnessed) =
            if edge_identity_evidence.get(edge).copied().unwrap_or(false)
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
                face_surface(ctx, ir, bindings, surface_indices, support.faces[0])?.zip(
                    face_surface(ctx, ir, bindings, surface_indices, support.faces[1])?,
                )
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
        witness_storage.with_storage(|| {
            ctx.push_vec(
                &mut boundary_witnesses,
                witnessed,
                "catia_selected_face_witnesses",
            )
        })?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    point: Point3,
    surface: &NurbsSurface,
) -> Result<Option<bool>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    // A positive-weight NURBS control net bounds the surface, so its AABB is a
    // sound negative test.  The bounded parameter search supplies positive
    // witnesses only.  A failed search inside that AABB is unknown, not proof
    // that the point is off the surface.
    if let Some(bounds) = nurbs_surface_control_bounds(ctx, surface)? {
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
    let Some(distance) = surface_membership::nurbs_surface_witness_distance(ctx, surface, point)?
    else {
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
    let mut storage = ctx.reserve_scoped(0, "catia_a5_invariant_working_state")?;
    let Some((normalized, domains)) = storage.with_storage(|| -> Result<_, CodecError> {
        let mut normalized = Vec::new();
        for edges in ctx.admit_iter(face_edges, "catia_a5_face_edge_rows")? {
            let mut by_owner = BTreeMap::<usize, BTreeSet<usize>>::new();
            for (owner, carriers) in ctx.admit_iter(edges, "catia_a5_face_owner_edges")? {
                if *owner >= owner_count || carriers.is_empty() {
                    continue;
                }
                let domain = ctx
                    .entry_btree_map(&mut by_owner, *owner, "catia_a5_owner_domain_rows")?
                    .or_default();
                for carrier in ctx.admit_iter(carriers, "catia_a5_owner_domain_carriers")? {
                    ctx.insert_btree_set(domain, *carrier, "catia_a5_owner_domain_carriers")?;
                }
            }
            ctx.push_vec(&mut normalized, by_owner, "catia_a5_normalized_faces")?;
        }
        let mut domains = Vec::new();
        for edges in ctx.admit_iter(&normalized, "catia_a5_normalized_faces")? {
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
        Ok(Some((normalized, domains)))
    })?
    else {
        return Ok(None);
    };
    let mut bindings = Vec::new();
    {
        let mut visits = domains.iter().zip(normalized.iter());
        while let Some((owners, labels)) =
            ctx.next_charged(&mut visits, "catia_a5_owner_domains")?
        {
            // A face binds only when its owners reach exactly one carrier.
            let mut carrier = None;
            let ambiguous = ctx.any_by(
                owners,
                |owner| {
                    let Some(owner_carriers) =
                        ctx.get_btree_map(labels, owner, "catia_a5_owner_domain_keys")?
                    else {
                        return Ok(false);
                    };
                    ctx.any_by(
                        owner_carriers,
                        |&reached| Ok(*carrier.get_or_insert(reached) != reached),
                        "catia_a5_reachable_carriers",
                    )
                },
                "catia_a5_owner_domain_keys",
            )?;
            ctx.push_vec(
                &mut bindings,
                carrier.filter(|_| !ambiguous),
                "catia_a5_invariant_bindings",
            )?;
        }
    }
    Ok(Some(bindings))
}

fn owner_matches_a5_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    tail: &crate::native::owner_numeric_tail::CatiaOwnerNumericTail,
    surface: &NurbsSurface,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "geometry helper boundary")?;
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
                cadmpeg_ir::eval::decode::nurbs_surface_point(
                    cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
                    surface,
                    u,
                    v,
                ),
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

fn distinct_face_witnesses(
    ctx: &DecodeContext<'_>,
    witnesses: &[Point3],
) -> Result<Vec<Point3>, CodecError> {
    const OP: &str = "catia_a5_distinct_face_witnesses";
    let mut storage = ctx.reserve_scoped(0, OP)?;
    let (mut entries, mut accepted) = storage.with_storage(|| {
        let entries =
            ctx.collect_indexed_vec(witnesses.len(), "catia_a5_witness_bounds_index", |item| {
                Ok(BoundsEntry {
                    bounds: point_bounds(witnesses[item], 0.0),
                    item,
                })
            })?;
        let accepted = ctx.alloc_filled(witnesses.len(), false, "catia_a5_witness_bounds_index")?;
        Ok::<_, CodecError>((entries, accepted))
    })?;
    let index = BoundsIndex::new(ctx, &mut entries, "catia_a5_witness_bounds_index")?;
    let mut distinct = Vec::new();
    for (item, &point) in ctx.admit_iter(witnesses, OP)?.enumerate() {
        let bounds = point_bounds(point, NURBS_SURFACE_MEMBERSHIP_TOLERANCE);
        if !ctx.any_by(
            index.overlapping(bounds),
            |node| {
                Ok(node.item.is_some_and(|other| {
                    accepted[other]
                        && bounds_overlap(node.bounds, bounds)
                        && witnesses[other].distance(point) <= NURBS_SURFACE_MEMBERSHIP_TOLERANCE
                }))
            },
            OP,
        )? {
            accepted[item] = true;
            ctx.push_vec(&mut distinct, point, OP)?;
        }
    }
    Ok(distinct)
}

struct A5BindingIndex<'a, 'ctx> {
    carriers: &'a [crate::families::a5a8::records::FreeformSurface],
    owners: &'a [crate::families::b2::records::B2OwnerPacket],
    carriers_by_bounds: BoundsIndex<'ctx>,
    owners_by_bounds: BoundsIndex<'ctx>,
}

impl<'a, 'ctx> A5BindingIndex<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        carriers: &'a [crate::families::a5a8::records::FreeformSurface],
        owners: &'a [crate::families::b2::records::B2OwnerPacket],
    ) -> Result<Self, CodecError> {
        const OP: &str = "catia_a5_binding_bounds_index";
        let mut storage = ctx.reserve_scoped(0, OP)?;
        let mut carrier_entries =
            storage.with_storage(|| {
                ctx.collect_indexed_vec(carriers.len(), OP, |item| {
                    let bounds = nurbs_surface_control_bounds(ctx, &carriers[item].geometry)?
                        .map_or([[f64::NEG_INFINITY, f64::INFINITY]; 3], |bounds| {
                            bounds.map(|[lower, upper]| {
                                [
                                    lower - NURBS_SURFACE_MEMBERSHIP_TOLERANCE,
                                    upper + NURBS_SURFACE_MEMBERSHIP_TOLERANCE,
                                ]
                            })
                        });
                    Ok(BoundsEntry { bounds, item })
                })
            })?;
        let carriers_by_bounds = BoundsIndex::new(ctx, &mut carrier_entries, OP)?;
        let mut owner_entries = storage.with_storage(|| {
            ctx.collect_indexed_vec(owners.len(), OP, |item| {
                Ok(BoundsEntry {
                    bounds: Self::owner_bounds(&owners[item].numeric_tail),
                    item,
                })
            })
        })?;
        let owners_by_bounds = BoundsIndex::new(ctx, &mut owner_entries, OP)?;
        Ok(Self {
            carriers,
            owners,
            carriers_by_bounds,
            owners_by_bounds,
        })
    }

    fn owner_bounds(
        tail: &crate::native::owner_numeric_tail::CatiaOwnerNumericTail,
    ) -> [[f64; 2]; 3] {
        tail.bounds().map(|[lower, upper]| {
            [
                f64::from(lower) - NURBS_SURFACE_MEMBERSHIP_TOLERANCE,
                f64::from(upper) + NURBS_SURFACE_MEMBERSHIP_TOLERANCE,
            ]
        })
    }

    fn matching_carriers(
        &self,
        ctx: &DecodeContext<'_>,
        tail: &crate::native::owner_numeric_tail::CatiaOwnerNumericTail,
    ) -> Result<Vec<usize>, CodecError> {
        const OP: &str = "catia_a5_owner_carriers";
        let bounds = Self::owner_bounds(tail);
        let mut matched = Vec::new();
        let mut nodes = self.carriers_by_bounds.overlapping(bounds);
        while let Some(node) = ctx.next_charged(&mut nodes, OP)? {
            let Some(carrier) = node.item.filter(|_| bounds_overlap(node.bounds, bounds)) else {
                continue;
            };
            if owner_matches_a5_carrier(ctx, tail, &self.carriers[carrier].geometry)? {
                ctx.push_vec(&mut matched, carrier, "catia_a5_owner_carrier_indices")?;
            }
        }
        if matched.len() > 2 {
            ctx.sort_unstable_by(&mut matched, |value| value, Ord::cmp, OP)?;
        } else if matched.len() == 2 && matched[0] > matched[1] {
            matched.swap(0, 1);
        }
        Ok(matched)
    }

    fn containing_owners(
        &self,
        ctx: &DecodeContext<'_>,
        face: crate::families::standard::records::StandardFaceBounds,
        owner_carriers: &[Vec<usize>],
    ) -> Result<Vec<usize>, CodecError> {
        const OP: &str = "catia_a5_containing_owner_indices";
        let bounds = std::array::from_fn(|axis| {
            [
                face.aabb_center[axis].get() - face.aabb_half_extents[axis].get(),
                face.aabb_center[axis].get() + face.aabb_half_extents[axis].get(),
            ]
        });
        let mut matched = Vec::new();
        let mut nodes = self.owners_by_bounds.overlapping(bounds);
        while let Some(node) = ctx.next_charged(&mut nodes, OP)? {
            let Some(owner) = node.item.filter(|_| bounds_overlap(node.bounds, bounds)) else {
                continue;
            };
            let value = &self.owners[owner];
            if !owner_carriers[owner].is_empty()
                && owner_contains_face_bounds(value.reference_encoding, &value.numeric_tail, face)
            {
                ctx.push_vec(&mut matched, owner, OP)?;
            }
        }
        if matched.len() > 2 {
            ctx.sort_unstable_by(&mut matched, |value| value, Ord::cmp, OP)?;
        } else if matched.len() == 2 && matched[0] > matched[1] {
            matched.swap(0, 1);
        }
        Ok(matched)
    }
}

fn standard_face_boundary_witnesses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<Vec<Point3>>, CodecError> {
    const LOOKUP: &str = "catia_a5_witness_lookup";
    let mut storage = ctx.reserve_scoped(0, "catia_a5_witness_indexes")?;
    let (vertex_positions, edges, coedges, loops, curves) = storage.with_storage(|| {
        let mut point_positions = HashMap::new();
        for point in ctx.admit_iter(&ir.model.points, "catia_a5_witness_point_positions")? {
            ctx.insert_hash_map(
                &mut point_positions,
                point.id.as_str(),
                point.position().get(),
                "catia_a5_witness_point_positions",
            )?;
        }
        let mut vertex_positions = HashMap::new();
        for vertex in ctx.admit_iter(&ir.model.vertices, "catia_a5_witness_vertices")? {
            if let Some(&position) =
                ctx.get_hash_map(&point_positions, vertex.point.as_str(), LOOKUP)?
            {
                ctx.insert_hash_map(
                    &mut vertex_positions,
                    vertex.id.as_str(),
                    position,
                    "catia_a5_witness_vertex_positions",
                )?;
            }
        }
        let mut edges = HashMap::new();
        for edge in ctx.admit_iter(&ir.model.edges, "catia_a5_witness_edges")? {
            ctx.insert_hash_map(&mut edges, edge.id.as_str(), edge, "catia_a5_witness_edges")?;
        }
        let mut coedges = HashMap::new();
        for coedge in ctx.admit_iter(&ir.model.coedges, "catia_a5_witness_coedges")? {
            ctx.insert_hash_map(
                &mut coedges,
                coedge.id.as_str(),
                coedge,
                "catia_a5_witness_coedges",
            )?;
        }
        let mut loops = HashMap::new();
        for loop_ in ctx.admit_iter(&ir.model.loops, "catia_a5_witness_loops")? {
            ctx.insert_hash_map(
                &mut loops,
                loop_.id.as_str(),
                loop_,
                "catia_a5_witness_loops",
            )?;
        }
        let mut curves = HashMap::new();
        for curve in ctx.admit_iter(&ir.model.curves, "catia_a5_witness_curves")? {
            ctx.insert_hash_map(
                &mut curves,
                curve.id.as_str(),
                curve,
                "catia_a5_witness_curves",
            )?;
        }
        Ok::<_, CodecError>((vertex_positions, edges, coedges, loops, curves))
    })?;
    let mut face_witnesses = Vec::new();
    for face in ctx.admit_iter(&ir.model.faces, "catia_a5_face_witness_rows")? {
        let mut witness_storage = ctx.reserve_scoped(0, "catia_a5_face_witness_points")?;
        let mut witnesses = Vec::new();
        {
            let mut visit_loop = |loop_id: &LoopId| -> Result<(), CodecError> {
                let Some(loop_) = ctx.get_hash_map(&loops, loop_id.as_str(), LOOKUP)? else {
                    return Ok(());
                };
                for coedge_id in ctx.admit_iter(loop_.coedges(), "catia_a5_witness_loop_coedges")? {
                    let Some(coedge) = ctx.get_hash_map(&coedges, coedge_id.as_str(), LOOKUP)?
                    else {
                        continue;
                    };
                    let Some(edge) = ctx.get_hash_map(&edges, coedge.edge.as_str(), LOOKUP)? else {
                        continue;
                    };
                    for id in [&edge.start, &edge.end] {
                        if let Some(&position) =
                            ctx.get_hash_map(&vertex_positions, id.as_str(), LOOKUP)?
                        {
                            ctx.push_scoped_vec(
                                &mut witness_storage,
                                &mut witnesses,
                                position,
                                "catia_a5_face_witness_points",
                            )?;
                        }
                    }
                    let Some(curve_id) = edge.curve() else {
                        continue;
                    };
                    let Some(curve) = ctx.get_hash_map(&curves, curve_id.as_str(), LOOKUP)? else {
                        continue;
                    };
                    let Some([start, end]) =
                        edge.param_range().map(cadmpeg_ir::units::FiniteVector::get)
                    else {
                        continue;
                    };
                    if let Some(point) = cadmpeg_ir::eval::finite_or_refusal(
                        cadmpeg_ir::eval::decode::outer_refusal(
                            cadmpeg_ir::eval::decode::curve_point(
                                ctx,
                                &curve.geometry,
                                0.5 * (start + end),
                            ),
                        )?,
                    )? {
                        ctx.push_scoped_vec(
                            &mut witness_storage,
                            &mut witnesses,
                            point.get(),
                            "catia_a5_face_witness_points",
                        )?;
                    }
                }
                Ok(())
            };
            match &face.loops {
                cadmpeg_ir::topology::FaceLoops::Unspecified { loops: loop_ids } => {
                    for loop_id in ctx.admit_iter(loop_ids, "catia_a5_face_boundary_loops")? {
                        visit_loop(loop_id)?;
                    }
                }
                cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
                    visit_loop(outer)?;
                    for loop_id in ctx.admit_iter(inner, "catia_a5_face_boundary_loops")? {
                        visit_loop(loop_id)?;
                    }
                }
            }
        }
        let distinct = distinct_face_witnesses(ctx, &witnesses)?;
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
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    source: StandardConsolidatedSource<'_>,
    face_bounds: &[Option<crate::families::standard::records::StandardFaceBounds>],
    budget: &WorkBudget<'_>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<usize, cadmpeg_core::CodecError> {
    let StandardConsolidatedSource { data, records } = source;
    // Every collection here is dropped before return; only the bound surface
    // copies are retained.
    let mut scratch = ctx.reserve_scoped(0, "catia_a5_owner_binding_scratch")?;
    let (carriers, owners) = scratch.with_storage(|| {
        let carriers =
            crate::families::a5a8::records::a5_surfaces_from_records(ctx, data, records, refusal)?;
        let owners = ctx.collect_vec(
            crate::families::b2::records::b2_owner_packets_from_records(ctx, data, records)?,
            "catia_a5_owner_packets",
        )?;
        Ok::<_, CodecError>((carriers, owners))
    })?;
    if carriers.is_empty() || owners.is_empty() || ir.model.faces.is_empty() {
        return Ok(0);
    }
    let index = A5BindingIndex::new(ctx, &carriers, &owners)?;
    let (owner_carriers, witnesses, unknown_faces) = scratch.with_storage(|| {
        let mut owner_carriers = Vec::new();
        for owner in ctx.admit_iter(&owners, "catia_a5_owner_packets")? {
            let matched = index.matching_carriers(ctx, &owner.numeric_tail)?;
            ctx.push_vec(&mut owner_carriers, matched, "catia_a5_owner_carrier_rows")?;
        }
        let witnesses = standard_face_boundary_witnesses(ctx, ir)?;
        let mut surface_indices = HashMap::new();
        for (index, surface) in ctx
            .admit_iter(&ir.model.surfaces, "catia_standard_iteration")?
            .enumerate()
        {
            ctx.insert_hash_map(
                &mut surface_indices,
                surface.id.as_str(),
                index,
                "catia_a5_surface_indices",
            )?;
        }
        let mut unknown_faces = Vec::new();
        for (face, value) in ctx
            .admit_iter(&ir.model.faces, "catia_a5_unknown_face_rows")?
            .enumerate()
        {
            const FACE_ID: &str = "catia_a5_unknown_face_rows";
            let Some(ordinal) = value.id.as_str().strip_prefix("catia:standard:face#") else {
                continue;
            };
            let Ok(ordinal) = ctx.parse_text::<usize>(ordinal, FACE_ID)? else {
                continue;
            };
            let Some(&surface) =
                ctx.get_hash_map(&surface_indices, value.surface.as_str(), FACE_ID)?
            else {
                continue;
            };
            if matches!(
                ir.model.surfaces[surface].geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
            ) {
                ctx.push_vec(&mut unknown_faces, (face, ordinal, surface), FACE_ID)?;
            }
        }
        Ok::<_, CodecError>((owner_carriers, witnesses, unknown_faces))
    })?;
    let mut face_edges = Vec::new();
    ctx.reserve_scoped_vec(
        &mut scratch,
        &mut face_edges,
        unknown_faces.len(),
        "catia_a5_face_edge_rows",
    )?;
    for &(face, ordinal, _) in ctx.admit_iter(&unknown_faces, "catia_a5_unknown_face_rows")? {
        let Some(Some(bounds)) = face_bounds.get(ordinal) else {
            face_edges.push(Vec::new());
            continue;
        };
        let mut face_storage = ctx.reserve_scoped(0, "catia_a5_face_carrier_scratch")?;
        let containing_owners =
            face_storage.with_storage(|| index.containing_owners(ctx, *bounds, &owner_carriers))?;
        let mut possible_carriers = BTreeSet::new();
        for owner in ctx.admit_iter(&containing_owners, "catia_a5_containing_owner_indices")? {
            for carrier in ctx.admit_iter(&owner_carriers[*owner], "catia_a5_possible_carriers")? {
                ctx.insert_scoped_btree_set(
                    &mut face_storage,
                    &mut possible_carriers,
                    *carrier,
                    "catia_a5_possible_carriers",
                    "catia_a5_possible_carriers",
                )?;
            }
        }
        let face_points = witnesses.get(face).filter(|points| points.len() >= 3);
        let mut face_carriers = BTreeSet::new();
        for carrier in ctx.admit_iter(&possible_carriers, "catia_a5_possible_carriers")? {
            let surface = &carriers[*carrier].geometry;
            let witnessed = match face_points {
                Some(points) => ctx.all_by(
                    points,
                    |point| Ok(point_on_nurbs_surface(ctx, *point, surface)? == Some(true)),
                    "catia_a5_face_witness_points",
                )?,
                None => false,
            };
            if witnessed {
                ctx.insert_scoped_btree_set(
                    &mut face_storage,
                    &mut face_carriers,
                    *carrier,
                    "catia_a5_face_carriers",
                    "catia_a5_face_carriers",
                )?;
            }
        }
        let mut edges = Vec::new();
        for owner in ctx.admit_iter(&containing_owners, "catia_a5_containing_owner_indices")? {
            let mut labels = Vec::new();
            for &carrier in
                ctx.admit_iter(&owner_carriers[*owner], "catia_a5_face_carrier_labels")?
            {
                if ctx.contains_btree_set(
                    &face_carriers,
                    &carrier,
                    "catia_a5_face_carrier_labels",
                )? {
                    ctx.push_scoped_vec(
                        &mut scratch,
                        &mut labels,
                        carrier,
                        "catia_a5_face_carrier_labels",
                    )?;
                }
            }
            if !labels.is_empty() {
                ctx.push_scoped_vec(
                    &mut scratch,
                    &mut edges,
                    (*owner, labels),
                    "catia_a5_face_owner_edges",
                )?;
            }
        }
        face_edges.push(edges);
    }
    let Some(bindings) = scratch.with_storage(|| {
        invariant_face_carrier_bindings(ctx, &face_edges, owners.len(), Some(budget))
    })?
    else {
        return Ok(0);
    };
    let mut bound = 0;
    for ((_, _, surface), carrier) in ctx
        .admit_iter(&unknown_faces, "catia_a5_unknown_face_bindings")?
        .zip(ctx.admit_iter(&bindings, "catia_a5_invariant_bindings")?)
    {
        let Some(carrier) = *carrier else {
            continue;
        };
        let surface = *surface;
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
                point_on_surface_if_supported(ctx, point, surface)? != Some(false)
            }
            _ => point_on_surface(ctx, point, surface)?,
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

        assert_eq!(
            super::edge_geometry::canonical_unoriented_axis(z()),
            Some(unit(z()))
        );
        assert_eq!(
            super::edge_geometry::canonical_unoriented_axis(Vector3::new(0.0, 0.0, -1.0)),
            Some(unit(z()))
        );
        let axis = super::edge_geometry::canonical_unoriented_axis(Vector3::new(-2.0, 1.0, 0.0))
            .expect("finite axis");
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
            &cadmpeg_test_support::service_decode_context(),
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
        .expect("fixture constructor admission")
        .expect("anisotropic nurbs surface");
        let residual = super::surface_membership::nurbs_surface_witness_distance(
            &cadmpeg_test_support::service_decode_context(),
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
