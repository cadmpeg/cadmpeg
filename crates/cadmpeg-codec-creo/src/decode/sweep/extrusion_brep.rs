// SPDX-License-Identifier: Apache-2.0
//! Resolved extrusion B-rep transfer.

use super::super::feature_history::draft::feature_allows_additive_linear_extrusion;
use super::super::feature_history::link::generated_profile_entry_is_admissible;
use super::super::native::annotate;
use super::super::sketch::intersect::section_point_in_model;
use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::{
    exactly_one_by, unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::extent::resolved_feature_extrusion_span;
use super::nurbs::{
    extrusion_brep_side_surface, oriented_sketch_nurbs_curve, placed_section_nurbs,
    translated_nurbs_curve,
};
use super::pcurves::{add_extrusion_pcurve, PcurveAdmission};
use super::profiles::{
    extrusion_cap_pcurve, extrusion_side_uvs, line_pcurve, ordered_extrusion_profiles,
    oriented_arc_parameterization, resolved_sketch_profiles, ProfileGeometry,
};
use crate::container::ContainerScan;
use crate::decode::analytic::edges::nurbs_intrinsic_parameter_range;
use crate::decode::sketch_transfer::recipe::feature_is_first_material_operation;
use crate::lane_refusal::JoinedLaneRecords;
use crate::vecmath::normalize;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::BTreeSet;

const GENERATED_EXTRUSION_SIDE_KINDS: &[crate::surface::SurfaceKind] = &[
    crate::surface::SurfaceKind::Plane,
    crate::surface::SurfaceKind::Cylinder,
    crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
];

fn cap_record<'a>(
    ctx: &'a cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: u32,
    profile_index: usize,
    cap: &str,
    entity_index: usize,
) -> Result<(String, cadmpeg_core::decode::ScopedReservation<'a>), cadmpeg_core::CodecError> {
    ctx.format_scoped(
        format_args!("extrusion feature {feature_id} profile {profile_index} {cap} cap at entity {entity_index}"),
        "creo extrusion cap record text",
    )
}

fn refused_lane_message(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &str,
    records: &[String],
) -> Result<String, cadmpeg_core::CodecError> {
    ctx.format_retained(
        format_args!("Refused lanes on {record}: {}", JoinedLaneRecords(records)),
        "creo extrusion refused lane error",
    )
}

fn missing_cap_message(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &str,
) -> Result<String, cadmpeg_core::CodecError> {
    ctx.format_retained(
        format_args!("{record} states no pcurve geometry"),
        "creo extrusion missing cap error",
    )
}

fn generated_extrusion_identity<I>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    key: impl std::fmt::Display,
) -> Result<I, cadmpeg_core::CodecError>
where
    I: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>,
{
    crate::identity::compose_checked(
        ctx,
        &crate::identity::FEATURE_EXTRUSION,
        key,
        "creo extrusion generated identities",
    )
}

fn copy_extrusion_identity<I>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &str,
) -> Result<I, cadmpeg_core::CodecError>
where
    I: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>,
{
    crate::identity::copy_checked_id(ctx, source, "creo extrusion entity ID copies")
}

fn push_rejected_extrusion(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    diagnostics: &mut crate::decode::surfaces::brep::BrepTransferDiagnostics,
    body_id: BodyId,
    reason: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let reason =
        ctx.format_retained(format_args!("{reason}"), "creo extrusion rejection reason")?;
    ctx.reserve_vec(
        &mut diagnostics.rejected_extrusion_bodies,
        1,
        "creo extrusion rejection diagnostics",
    )?;
    diagnostics
        .rejected_extrusion_bodies
        .push((body_id, reason));
    Ok(())
}

fn cap_coedge_ids_admitted(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: u32,
    profile_index: usize,
    count: usize,
    side: &'static str,
    reversed: bool,
    operation: &'static str,
) -> Result<Vec<CoedgeId>, cadmpeg_core::CodecError> {
    let mut ids = Vec::new();
    for position in ctx.admit_iter(0..count, operation)? {
        let index = if reversed {
            count - 1 - position
        } else {
            position
        };
        ctx.reserve_vec(&mut ids, 1, operation)?;
        ids.push(generated_extrusion_identity(
            ctx,
            format_args!("{feature_id}:coedge:{profile_index}:{index}:{side}"),
        )?);
    }
    Ok(ids)
}

fn copy_ring_coedges(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ids: &[CoedgeId],
    collection_operation: &'static str,
    identity_operation: &'static str,
) -> Result<Vec<CoedgeId>, cadmpeg_core::CodecError> {
    let mut copied = Vec::new();
    ctx.reserve_vec(&mut copied, ids.len(), collection_operation)?;
    for id in ctx.admit_iter(ids, collection_operation)? {
        copied.push(crate::identity::copy_checked_id(
            ctx,
            id.as_str(),
            identity_operation,
        )?);
    }
    Ok(copied)
}

fn sketch_profiles_cover_generated_extrusion_sides(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    definition: &crate::feature::definitions::FeatureDefinition,
    feature_id: u32,
    sketch: &Sketch,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut node_storage = ctx.reserve_scoped(0, "creo extrusion profile roster storage")?;
    let mut profile_entity_set = BTreeSet::<&str>::new();
    let mut profile_count = 0;
    for profile in ctx.admit_iter(sketch.profiles.as_slice(), "creo extrusion profile roster rows")? {
    for entity_use in ctx.admit_iter(profile, "creo extrusion profile roster uses")? {
        profile_count += 1;
        let id = entity_use.entity.as_str();
        node_storage.with_storage(|| ctx.insert_btree_set(
            &mut profile_entity_set,
            id,
            "creo extrusion profile entity ID nodes",
        ))?;
    }
    }
    let mut expected_entity_set = BTreeSet::<&str>::new();
    let mut expected_count = 0;
    for table in ctx.admit_iter(&scan.features.entity_tables, "creo extrusion profile source tables")?
        .filter(|table| table.feature_id == feature_id)
    {
        for entry in ctx.admit_iter(&table.entries, "creo extrusion profile source entries")? {
            let Some(external_id) = entry.source_entity_id() else {
                continue;
            };
            let (entity, _reservation) = ctx.format_scoped(
                format_args!(
                    "creo:featdefs:sketch_entity#{}:{external_id}",
                    definition.identity.id()
                ),
                "creo extrusion expected sketch entity ID",
            )?;
            let Some(matched) = ctx.get_btree_set(&profile_entity_set, entity.as_str(), "creo extrusion profile entity membership")? else {
                continue;
            };
            if !generated_profile_entry_is_admissible(
                ctx,
                feature_id,
                table,
                entry,
                GENERATED_EXTRUSION_SIDE_KINDS,
                &scan.surfaces.rows,
            )? {
                continue;
            }
            expected_count += 1;
            node_storage.with_storage(|| ctx.insert_btree_set(
                &mut expected_entity_set,
                *matched,
                "creo extrusion expected entity ID nodes",
            ))?;
        }
    }
    // Expected IDs are a subset of profile IDs. Equal cardinalities prove agreement.
    Ok(expected_count > 0
        && expected_count == expected_entity_set.len()
        && profile_count == expected_entity_set.len())
}

pub(in super::super) fn transfer_resolved_extrusion_breps(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    diagnostics: &mut crate::decode::surfaces::brep::BrepTransferDiagnostics,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut transferred = 0;
    for transform in ctx.admit_iter(&scan.features.section_transforms, "creo sweep transform scan")? {
        if unique_feature_section_transform(
            ctx,
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )?
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_additive_linear_extrusion(ctx, scan, feature_id)?
            || !feature_is_first_material_operation(ctx, scan, feature_id)?
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let Some(sketch_id) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        let Some(span) =
            resolved_feature_extrusion_span(ctx, scan, ir, source_carriers, definition, transform)?
        else {
            continue;
        };
        macro_rules! extrusion_id {
            ($id:ident, $suffix:literal $(, $part:expr)*) => {
                generated_extrusion_identity::<$id>(
                    ctx,
                    format_args!(concat!("{}:", $suffix), feature_id $(, $part)*),
                )?
            };
        }
        macro_rules! copy_id {
            ($source:expr) => {
                copy_extrusion_identity(ctx, ($source).as_str())?
            };
        }
        let length = span.upper() - span.lower();
        let Some(sketch) = exactly_one_by(ctx, &ir.model.sketches, |sketch| ctx.equal(sketch.id.as_str(), sketch_id.as_str(), "creo extrusion sketch identity"), "creo extrusion sketch scan")? else { continue; };
        if !sketch_profiles_cover_generated_extrusion_sides(
            ctx, scan, definition, feature_id, sketch,
        )? {
            continue;
        }
        let (profiles, mut profile_storage) = ctx.with_scoped_storage("creo extrusion profile scratch", || resolved_sketch_profiles(ctx, ir, source_carriers, &sketch_id, 1))?;
        let Some(profiles) = profiles
        else {
            continue;
        };
        let Some(profiles) = profile_storage.with_storage(|| ordered_extrusion_profiles(ctx, profiles))? else {
            continue;
        };
        let body_id = extrusion_id!(BodyId, "body");
        if ctx.any_by(&ir.model.bodies, |body| ctx.equal(body.id.as_str(), body_id.as_str(), "creo model identity comparison"), "creo model identity scan")? {
            continue;
        }
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let mut probe_storage = ctx.reserve_scoped(0, "creo extrusion side probe surface")?;
        let mut unprojectable = false;
        let mut entity_index = 0;
        let mut profile_rows = profiles.iter();
        'probe: while let Some(profile) = ctx.next_charged(&mut profile_rows, "creo extrusion probe profile rows")? {
            let mut entities = profile.entities().iter();
            while let Some(entity) = ctx.next_charged(&mut entities, "creo extrusion probe profile entities")? {
            let (sketch_geometry, _geometry_storage) = ctx.with_scoped_storage("creo extrusion side probe sketch", || entity.geometry().to_sketch(ctx))?;
            let Some(sketch_geometry) = sketch_geometry else {
                unprojectable = true;
                break 'probe;
            };
            let surface = probe_storage.with_storage(|| extrusion_brep_side_surface(
                ctx,
                transform,
                &sketch_geometry,
                entity.reversed(),
                [entity.start(), entity.end()],
                span,
                &mut crate::lane_refusal::LaneRefusalContext::new(
                    &format_args!("extrusion feature {feature_id} profile entity {entity_index}"),
                    &mut refusal,
                ),
            ))?;
            if surface.is_none()
            {
                unprojectable = true;
                break 'probe;
            }
                entity_index += 1;
            }
        }
        let records = refusal.take_records_checked()?;
        if !records.is_empty() {
            // The probe states every side before the first record of this body
            // reaches the model, so a refused lane leaves no partial body and
            // the model carries the absence of this one extrusion.
            push_rejected_extrusion(
                ctx,
                diagnostics,
                copy_id!(body_id),
                format_args!(
                    "refused extrusion side lanes: {}",
                    JoinedLaneRecords(&records)
                ),
            )?;
            continue;
        }
        if unprojectable {
            continue;
        }
        let forward_caps = profiles[0].area() > 0.0;

        let region_id = extrusion_id!(RegionId, "region");
        let shell_id = extrusion_id!(ShellId, "shell");
        let bottom_face = extrusion_id!(FaceId, "face:bottom");
        let top_face = extrusion_id!(FaceId, "face:top");
        let mut shell_faces = Vec::new();
        for face in [&bottom_face, &top_face] {
            ctx.reserve_vec(&mut shell_faces, 1, "creo extrusion shell face IDs")?;
            shell_faces.push(copy_id!(face));
        }
        for (profile_index, profile) in ctx.admit_iter(&profiles, "creo extrusion profile rows")?.enumerate() {
            for index in ctx.admit_iter(0..profile.entities().len(), "creo extrusion shell face source scan")? {
                ctx.reserve_vec(&mut shell_faces, 1, "creo extrusion shell face IDs")?;
                shell_faces.push(extrusion_id!(
                    FaceId,
                    "face:{}:side:{}",
                    profile_index,
                    index
                ));
            }
        }
        let shell = match Shell::new(
            copy_id!(shell_id),
            copy_id!(region_id),
            shell_faces,
            Vec::new(),
            Vec::new(),
        ) {
            Ok(shell) => shell,
            Err(error) => {
                push_rejected_extrusion(ctx, diagnostics, body_id, error)?;
                continue;
            }
        };
        let bottom_surface = extrusion_id!(SurfaceId, "surface:bottom");
        let top_surface = extrusion_id!(SurfaceId, "surface:top");
        for (id, offset) in [
            (&bottom_surface, span.lower()),
            (&top_surface, span.upper()),
        ] {
            annotate(
                ctx,
                annotations,
                id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(transform.offset),
                "extrusion_cap_plane",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: copy_id!(id),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                            Point3::new(
                                transform.origin()[0] + offset * transform.normal()[0],
                                transform.origin()[1] + offset * transform.normal()[1],
                                transform.origin()[2] + offset * transform.normal()[2],
                            ),
                            transform.normal_vector(),
                            transform.u_axis_vector(),
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                    )),
                    source_object: None,
                },
            )?;
        }

        let mut bottom_loops = Vec::new();
        let mut top_loops = Vec::new();
        for (profile_index, validated) in ctx.admit_iter(&profiles, "creo extrusion profile rows")?.enumerate() {
            let profile = validated.entities();
            let count = profile.len();
            let (mut bottom_vertices, mut bottom_vertices_storage) = ctx.temporary_vec(0, "creo extrusion profile vertex IDs")?;
            let (mut top_vertices, mut top_vertices_storage) = ctx.temporary_vec(0, "creo extrusion profile vertex IDs")?;
            for (index, entity) in ctx.admit_iter(profile, "creo extrusion profile entity traversal")?.enumerate() {
                let start = entity.start();

                for (side, offset, arena, storage) in [
                    ("bottom", span.lower(), &mut bottom_vertices, &mut bottom_vertices_storage),
                    ("top", span.upper(), &mut top_vertices, &mut top_vertices_storage),
                ] {
                    let position = section_point_in_model(transform, start);
                    let side_key = match side {
                        "bottom" => cadmpeg_ir::identity_key!("bottom"),
                        "top" => cadmpeg_ir::identity_key!("top"),
                        _ => continue,
                    };
                    let point_id =
                        extrusion_id!(PointId, "point:{}:{}:{}", profile_index, index, &side_key);
                    let vertex_id =
                        extrusion_id!(VertexId, "vertex:{}:{}:{}", profile_index, index, &side_key);
                    let finite_position = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                        position[0] + offset * transform.normal()[0],
                        position[1] + offset * transform.normal()[1],
                        position[2] + offset * transform.normal()[2],
                    ))
                    .ok_or(Point::NON_FINITE_POSITION)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                    ctx.charge_entities(1, "admit Creo model points")?;
                    source_carriers.admit_point(
                        ctx,
                        ir,
                        Point::new(copy_id!(point_id), finite_position, None),
                    )?;
                    ctx.charge_entities(1, "admit Creo model vertices")?;
                    source_carriers.admit_vertex(
                        ctx,
                        ir,
                        Vertex {
                            id: copy_id!(vertex_id),
                            point: point_id,
                            tolerance: None,
                        },
                    )?;
                    ctx.reserve_scoped_vec(storage, arena, 1, "creo extrusion profile vertex IDs")?;
                    arena.push(vertex_id);
                }
            }

            let (mut bottom_edges, mut bottom_edges_storage) = ctx.temporary_vec(0, "creo extrusion profile edge IDs")?;
            let (mut top_edges, mut top_edges_storage) = ctx.temporary_vec(0, "creo extrusion profile edge IDs")?;
            let (mut vertical_edges, mut vertical_edges_storage) = ctx.temporary_vec(0, "creo extrusion vertical edge IDs")?;
            for (index, entity) in ctx.admit_iter(profile, "creo extrusion profile entity traversal")?.enumerate() {
                let geometry = entity.geometry();
                let Some(sketch_geometry) = geometry.to_sketch(ctx)? else {
                    continue;
                };
                let reversed = entity.reversed();
                let start = entity.start();
                let end = entity.end();

                let next = (index + 1) % count;
                for (side, offset, vertices, arena, storage) in [
                    ("bottom", span.lower(), &bottom_vertices, &mut bottom_edges, &mut bottom_edges_storage),
                    ("top", span.upper(), &top_vertices, &mut top_edges, &mut top_edges_storage),
                ] {
                    let side_key = match side {
                        "bottom" => cadmpeg_ir::identity_key!("bottom"),
                        "top" => cadmpeg_ir::identity_key!("top"),
                        _ => continue,
                    };
                    let curve_id =
                        extrusion_id!(CurveId, "curve:{}:{}:{}", profile_index, index, &side_key);
                    let edge_id =
                        extrusion_id!(EdgeId, "edge:{}:{}:{}", profile_index, index, &side_key);
                    let curve = match geometry {
                        ProfileGeometry::Line { .. } => {
                            let placed_start = section_point_in_model(transform, start);
                            let placed_end = section_point_in_model(transform, end);
                            let Some(direction) = normalize(std::array::from_fn(|axis| {
                                placed_end[axis] - placed_start[axis]
                            })) else {
                                continue;
                            };
                            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                                    Point3::new(
                                        placed_start[0] + offset * transform.normal()[0],
                                        placed_start[1] + offset * transform.normal()[1],
                                        placed_start[2] + offset * transform.normal()[2],
                                    ),
                                    Vector3::from(direction),
                                )
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                            ))
                        }
                        ProfileGeometry::Arc { center, radius, .. }
                        | ProfileGeometry::Circle { center, radius } => {
                            let center = section_point_in_model(transform, [center.u, center.v]);
                            let (axis_sign, _) = oriented_arc_parameterization(reversed, 0.0, 0.0);
                            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                                    Point3::new(
                                        center[0] + offset * transform.normal()[0],
                                        center[1] + offset * transform.normal()[1],
                                        center[2] + offset * transform.normal()[2],
                                    ),
                                    Vector3::new(
                                        axis_sign * transform.normal()[0],
                                        axis_sign * transform.normal()[1],
                                        axis_sign * transform.normal()[2],
                                    ),
                                    transform.u_axis_vector(),
                                    radius.get(),
                                )
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                            ))
                        }
                        ProfileGeometry::Nurbs { .. } => {
                            let Some(nurbs) =
                                oriented_sketch_nurbs_curve(ctx, &sketch_geometry, reversed)?
                            else {
                                continue;
                            };
                            let Some(placed) = placed_section_nurbs(ctx, transform, &nurbs)? else {
                                continue;
                            };
                            let Some(translated) = translated_nurbs_curve(
                                ctx,
                                &placed,
                                [
                                    offset * transform.normal()[0],
                                    offset * transform.normal()[1],
                                    offset * transform.normal()[2],
                                ],
                            )?
                            else {
                                continue;
                            };
                            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(translated))
                        }
                    };
                    ctx.charge_entities(1, "admit Creo model curves")?;
                    source_carriers.admit_curve(
                        ctx,
                        ir,
                        Curve {
                            id: copy_id!(curve_id),
                            geometry: curve,
                            source_object: None,
                        },
                    )?;
                    let param_range = match geometry {
                        ProfileGeometry::Line { .. } => {
                            Some([0.0, (end[0] - start[0]).hypot(end[1] - start[1])])
                        }
                        ProfileGeometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        } => Some(
                            oriented_arc_parameterization(
                                reversed,
                                start_angle.get(),
                                end_angle.get(),
                            )
                            .1,
                        ),
                        ProfileGeometry::Circle { .. } => Some(
                            oriented_arc_parameterization(reversed, 0.0, std::f64::consts::TAU).1,
                        ),
                        ProfileGeometry::Nurbs { .. } => {
                            oriented_sketch_nurbs_curve(ctx, &sketch_geometry, reversed)?
                                .and_then(|nurbs| nurbs_intrinsic_parameter_range(&nurbs))
                                .map(cadmpeg_ir::scalar::FiniteReal::raw_array)
                        }
                    };
                    ctx.charge_entities(1, "admit Creo model edges")?;
                    source_carriers.admit_edge(
                        ctx,
                        ir,
                        Edge {
                            id: copy_id!(edge_id),
                            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                                Some(curve_id),
                                param_range,
                            )
                            .map_err(cadmpeg_core::CodecError::malformed)?,
                            start: copy_id!(vertices[index]),
                            end: copy_id!(vertices[next]),
                            tolerance: None,
                        },
                    )?;
                    ctx.reserve_scoped_vec(storage, arena, 1, "creo extrusion profile edge IDs")?;
                    arena.push(edge_id);
                }
                let curve_id = extrusion_id!(CurveId, "curve:{}:{}:vertical", profile_index, index);
                let edge_id = extrusion_id!(EdgeId, "edge:{}:{}:vertical", profile_index, index);
                let origin = section_point_in_model(transform, start);
                ctx.charge_entities(1, "admit Creo model curves")?;
                source_carriers.admit_curve(
                    ctx,
                    ir,
                    Curve {
                        id: copy_id!(curve_id),
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                                Point3::new(
                                    origin[0] + span.lower() * transform.normal()[0],
                                    origin[1] + span.lower() * transform.normal()[1],
                                    origin[2] + span.lower() * transform.normal()[2],
                                ),
                                transform.normal_vector(),
                            )
                            .map_err(cadmpeg_core::CodecError::malformed)?,
                        )),
                        source_object: None,
                    },
                )?;
                ctx.charge_entities(1, "admit Creo model edges")?;
                source_carriers.admit_edge(
                    ctx,
                    ir,
                    Edge {
                        id: copy_id!(edge_id),
                        carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                            Some(curve_id),
                            Some([0.0, length]),
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                        start: copy_id!(bottom_vertices[index]),
                        end: copy_id!(top_vertices[index]),
                        tolerance: None,
                    },
                )?;
                ctx.reserve_scoped_vec(&mut vertical_edges_storage, &mut vertical_edges, 1, "creo extrusion vertical edge IDs")?;
                vertical_edges.push(edge_id);
            }

            let bottom_loop = extrusion_id!(LoopId, "loop:{}:bottom", profile_index);
            let top_loop = extrusion_id!(LoopId, "loop:{}:top", profile_index);
            ctx.reserve_vec(&mut bottom_loops, 1, "creo extrusion bottom loop IDs")?;
            bottom_loops.push(copy_id!(bottom_loop));
            ctx.reserve_vec(&mut top_loops, 1, "creo extrusion top loop IDs")?;
            top_loops.push(copy_id!(top_loop));
            let bottom_coedges = cap_coedge_ids_admitted(
                ctx,
                feature_id,
                profile_index,
                count,
                "bottom-cap",
                true,
                "creo extrusion bottom cap coedge IDs",
            )?;
            let top_coedges = cap_coedge_ids_admitted(
                ctx,
                feature_id,
                profile_index,
                count,
                "top-cap",
                false,
                "creo extrusion top cap coedge IDs",
            )?;
            ctx.charge_entities(1, "admit Creo model loops")?;
            ctx.reserve_vec(&mut ir.model.loops, 1, "creo extrusion model loops")?;
            ir.model.loops.push(IrLoop {
                id: copy_id!(bottom_loop),
                face: copy_id!(bottom_face),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(
                        ctx,
                        copy_ring_coedges(
                            ctx,
                            &bottom_coedges,
                            "creo extrusion bottom ring coedge copies",
                            "creo extrusion bottom ring coedge identities",
                        )?,
                        Vec::new(),
                    )
                    .map_err(cadmpeg_core::CodecError::from)?
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            });
            ctx.charge_entities(1, "admit Creo model loops")?;
            ctx.reserve_vec(&mut ir.model.loops, 1, "creo extrusion model loops")?;
            ir.model.loops.push(IrLoop {
                id: copy_id!(top_loop),
                face: copy_id!(top_face),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(
                        ctx,
                        copy_ring_coedges(
                            ctx,
                            &top_coedges,
                            "creo extrusion top ring coedge copies",
                            "creo extrusion top ring coedge identities",
                        )?,
                        Vec::new(),
                    )
                    .map_err(cadmpeg_core::CodecError::from)?
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            });
            for ring_index in ctx.admit_iter(0..count, "creo extrusion cap ring traversal")? {
                let edge_index = count - 1 - ring_index;
                let id = copy_id!(bottom_coedges[ring_index]);
                let entity = &profile[edge_index];
                let geometry = entity.geometry();
                let Some(sketch_geometry) = geometry.to_sketch(ctx)? else {
                    continue;
                };
                let reversed = entity.reversed();
                let start = entity.start();
                let end = entity.end();

                let bottom_pcurve = add_extrusion_pcurve(
                    ctx,
                    ir,
                    annotations,
                    PcurveAdmission::Existing(source_carriers, &bottom_surface),
                    extrusion_id!(
                        PcurveId,
                        "pcurve:{}:{}:bottom-cap",
                        profile_index,
                        edge_index
                    ),
                    transform.offset,
                    {
                        let mut refusal = crate::lane_refusal::LaneRefusals::new();
                        let (record, _record_reservation) =
                            cap_record(ctx, feature_id, profile_index, "bottom", edge_index)?;
                        let cap = extrusion_cap_pcurve(
                            ctx,
                            &sketch_geometry,
                            reversed,
                            start,
                            end,
                            &record,
                            &mut refusal,
                        )?;
                        let records = refusal.take_records_checked()?;
                        if !records.is_empty() {
                            // The shell of this body already declares this cap
                            // face, so the model cannot omit the pcurve.
                            return Err(cadmpeg_core::CodecError::Malformed(refused_lane_message(
                                ctx, &record, &records,
                            )?));
                        }
                        let Some(cap) = cap else {
                            return Err(cadmpeg_core::CodecError::Malformed(missing_cap_message(
                                ctx, &record,
                            )?));
                        };
                        cap
                    },
                )?;
                ctx.charge_entities(1, "admit Creo model coedges")?;
                source_carriers.admit_coedge(
                    ctx,
                    ir,
                    Coedge {
                        id,
                        owner_loop: copy_id!(bottom_loop),
                        edge: copy_id!(bottom_edges[edge_index]),
                        radial_next: extrusion_id!(
                            CoedgeId,
                            "coedge:{}:{}:side-bottom",
                            profile_index,
                            edge_index
                        ),
                        sense: Sense::Reversed,
                        pcurves: ctx.collect_vec(
                            [PcurveUse {
                                pcurve: bottom_pcurve,
                                isoparametric: None,
                                parameter_range: None,
                            }],
                            "creo extrusion bottom coedge pcurve uses",
                        )?,
                        use_curve: None,
                    },
                )?;
                let id = copy_id!(top_coedges[ring_index]);
                let entity = &profile[ring_index];
                let geometry = entity.geometry();
                let Some(sketch_geometry) = geometry.to_sketch(ctx)? else {
                    continue;
                };
                let reversed = entity.reversed();
                let start = entity.start();
                let end = entity.end();

                let top_pcurve = add_extrusion_pcurve(
                    ctx,
                    ir,
                    annotations,
                    PcurveAdmission::Existing(source_carriers, &top_surface),
                    extrusion_id!(PcurveId, "pcurve:{}:{}:top-cap", profile_index, ring_index),
                    transform.offset,
                    {
                        let mut refusal = crate::lane_refusal::LaneRefusals::new();
                        let (record, _record_reservation) =
                            cap_record(ctx, feature_id, profile_index, "top", ring_index)?;
                        let cap = extrusion_cap_pcurve(
                            ctx,
                            &sketch_geometry,
                            reversed,
                            start,
                            end,
                            &record,
                            &mut refusal,
                        )?;
                        let records = refusal.take_records_checked()?;
                        if !records.is_empty() {
                            // The shell of this body already declares this cap
                            // face, so the model cannot omit the pcurve.
                            return Err(cadmpeg_core::CodecError::Malformed(refused_lane_message(
                                ctx, &record, &records,
                            )?));
                        }
                        let Some(cap) = cap else {
                            return Err(cadmpeg_core::CodecError::Malformed(missing_cap_message(
                                ctx, &record,
                            )?));
                        };
                        cap
                    },
                )?;
                ctx.charge_entities(1, "admit Creo model coedges")?;
                source_carriers.admit_coedge(
                    ctx,
                    ir,
                    Coedge {
                        id,
                        owner_loop: copy_id!(top_loop),
                        edge: copy_id!(top_edges[ring_index]),
                        radial_next: extrusion_id!(
                            CoedgeId,
                            "coedge:{}:{}:side-top",
                            profile_index,
                            ring_index
                        ),
                        sense: Sense::Forward,
                        pcurves: ctx.collect_vec(
                            [PcurveUse {
                                pcurve: top_pcurve,
                                isoparametric: None,
                                parameter_range: None,
                            }],
                            "creo extrusion top coedge pcurve uses",
                        )?,
                        use_curve: None,
                    },
                )?;
            }

            let forward_sides = validated.area() > 0.0;
            let mut entities = profile.iter().enumerate();
            while let Some((index, entity)) = ctx.next_charged(&mut entities, "creo extrusion profile entity traversal")? {
                let geometry = entity.geometry();
                let Some(sketch_geometry) = geometry.to_sketch(ctx)? else {
                    continue;
                };
                let start = entity.start();

                let next = (index + 1) % count;
                let surface_id =
                    extrusion_id!(SurfaceId, "surface:{}:side:{}", profile_index, index);
                let mut refusal = crate::lane_refusal::LaneRefusals::new();
                let (record, _record_reservation) = ctx.format_scoped(
                    format_args!(
                        "extrusion feature {feature_id} profile {profile_index} side {index}"
                    ),
                    "creo extrusion side record text",
                )?;
                let mut diagnostics =
                    crate::lane_refusal::LaneRefusalContext::new(&record, &mut refusal);
                let surface_geometry = extrusion_brep_side_surface(
                    ctx,
                    transform,
                    &sketch_geometry,
                    profile[index].reversed(),
                    [start, profile[index].end()],
                    span,
                    &mut diagnostics,
                )?;
                let records = refusal.take_records_checked()?;
                if !records.is_empty() {
                    // The shell of this body already declares this side face,
                    // so the model cannot omit the surface.
                    return Err(cadmpeg_core::CodecError::Malformed(refused_lane_message(
                        ctx, &record, &records,
                    )?));
                }
                let Some(surface_geometry) = surface_geometry else {
                    break;
                };
                ctx.charge_entities(1, "admit Creo model surfaces")?;
                source_carriers.admit_surface(
                    ctx,
                    ir,
                    Surface {
                        id: copy_id!(surface_id),
                        geometry: surface_geometry,
                        source_object: None,
                    },
                )?;
                let face_id = extrusion_id!(FaceId, "face:{}:side:{}", profile_index, index);
                let loop_id = extrusion_id!(LoopId, "loop:{}:side:{}", profile_index, index);
                let coedges = [
                    extrusion_id!(CoedgeId, "coedge:{}:{}:side-bottom", profile_index, index),
                    extrusion_id!(
                        CoedgeId,
                        "coedge:{}:{}:side-vertical-out",
                        profile_index,
                        next
                    ),
                    extrusion_id!(CoedgeId, "coedge:{}:{}:side-top", profile_index, index),
                    extrusion_id!(
                        CoedgeId,
                        "coedge:{}:{}:side-vertical-in",
                        profile_index,
                        index
                    ),
                ];
                ctx.charge_entities(1, "admit Creo model loops")?;
                ctx.reserve_vec(&mut ir.model.loops, 1, "creo extrusion model loops")?;
                ir.model.loops.push(IrLoop {
                    id: copy_id!(loop_id),
                    face: copy_id!(face_id),
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                        cadmpeg_ir::topology::LoopRing::new(
                            ctx,
                            copy_ring_coedges(
                                ctx,
                                &coedges,
                                "creo extrusion side ring coedge copies",
                                "creo extrusion side ring coedge identities",
                            )?,
                            Vec::new(),
                        )
                        .map_err(cadmpeg_core::CodecError::from)?
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                    ),
                });
                let edge_uses: [(EdgeId, Sense); 4] = [
                    (copy_id!(bottom_edges[index]), Sense::Forward),
                    (copy_id!(vertical_edges[next]), Sense::Forward),
                    (copy_id!(top_edges[index]), Sense::Reversed),
                    (copy_id!(vertical_edges[index]), Sense::Reversed),
                ];
                let side_uvs = extrusion_side_uvs(
                    &sketch_geometry,
                    profile[index].reversed(),
                    start,
                    profile[index].end(),
                    span,
                );
                for use_index in 0..4 {
                    let radial_next = match use_index {
                        0 => copy_id!(bottom_coedges[count - 1 - index]),
                        1 => extrusion_id!(
                            CoedgeId,
                            "coedge:{}:{}:side-vertical-in",
                            profile_index,
                            next
                        ),
                        2 => copy_id!(top_coedges[index]),
                        3 => extrusion_id!(
                            CoedgeId,
                            "coedge:{}:{}:side-vertical-out",
                            profile_index,
                            index
                        ),
                        _ => continue,
                    };
                    let pcurve = add_extrusion_pcurve(
                        ctx,
                        ir,
                        annotations,
                        PcurveAdmission::Existing(source_carriers, &surface_id),
                        extrusion_id!(
                            PcurveId,
                            "pcurve:{}:{}:side:{}",
                            profile_index,
                            index,
                            use_index
                        ),
                        transform.offset,
                        line_pcurve(side_uvs[use_index][0], side_uvs[use_index][1]).ok_or_else(
                            || {
                                cadmpeg_core::CodecError::malformed(
                                    "extrusion pcurve geometry is invalid",
                                )
                            },
                        )?,
                    )?;
                    ctx.charge_entities(1, "admit Creo model coedges")?;
                    source_carriers.admit_coedge(
                        ctx,
                        ir,
                        Coedge {
                            id: copy_id!(coedges[use_index]),
                            owner_loop: copy_id!(loop_id),
                            edge: copy_id!(edge_uses[use_index].0),
                            radial_next,
                            sense: edge_uses[use_index].1,
                            pcurves: ctx.collect_vec(
                                [PcurveUse {
                                    pcurve,
                                    isoparametric: None,
                                    parameter_range: None,
                                }],
                                "creo extrusion side coedge pcurve uses",
                            )?,
                            use_curve: None,
                        },
                    )?;
                }
                ctx.charge_entities(1, "admit Creo model faces")?;
                source_carriers.admit_face(
                    ctx,
                    ir,
                    Face {
                        id: copy_id!(face_id),
                        shell: copy_id!(shell_id),
                        surface: surface_id,
                        sense: if forward_sides {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        loops: cadmpeg_ir::topology::FaceLoops::unspecified(
                            ctx.collect_vec([loop_id], "creo extrusion side face loop IDs")?,
                        ),
                        name: None,
                        color: None,
                        tolerance: None,
                    },
                )?;
            }
        }
        ctx.charge_entities(1, "admit Creo model faces")?;
        source_carriers.admit_face(
            ctx,
            ir,
            Face {
                id: bottom_face,
                shell: copy_id!(shell_id),
                surface: bottom_surface,
                sense: if forward_caps {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(bottom_loops),
                name: None,
                color: None,
                tolerance: None,
            },
        )?;
        ctx.charge_entities(1, "admit Creo model faces")?;
        source_carriers.admit_face(
            ctx,
            ir,
            Face {
                id: top_face,
                shell: copy_id!(shell_id),
                surface: top_surface,
                sense: if forward_caps {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(top_loops),
                name: None,
                color: None,
                tolerance: None,
            },
        )?;
        ctx.charge_entities(1, "admit Creo model shells")?;
        ctx.reserve_vec(&mut ir.model.shells, 1, "creo extrusion model shells")?;
        ir.model.shells.push(shell);
        ctx.charge_entities(1, "admit Creo model regions")?;
        ctx.reserve_vec(&mut ir.model.regions, 1, "creo extrusion model regions")?;
        ir.model.regions.push(Region {
            id: copy_id!(region_id),
            body: copy_id!(body_id),
            shells: ctx.collect_vec([shell_id], "creo extrusion region shell IDs")?,
        });
        ctx.charge_entities(1, "admit Creo model bodies")?;
        source_carriers.admit_body(
            ctx,
            ir,
            Body {
                id: body_id,
                kind: BodyKind::Solid,
                regions: ctx.collect_vec([region_id], "creo extrusion body region IDs")?,
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

#[cfg(test)]
mod tests;
