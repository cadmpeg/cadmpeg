// SPDX-License-Identifier: Apache-2.0
//! Positional spheres, tori, extrusion planes, and tabulated cylinders.

use crate::feature::schema::SchemaClass;
use crate::vecmath::normalize;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::feature_history::round::{
    paired_five_coordinate_sphere_center, round_constant_radius, unique_surface_parameter_record,
};
use super::super::native::annotate;
use super::super::sweep::nurbs::{extruded_nurbs_surface, placed_tabulated_cylinder_directrix};
use crate::decode::sketch_transfer::recipe::feature_schema_class;
use crate::decode::source_carriers::SourceUnitCarriers;
use crate::vecmath::cross;

use super::prototypes::{
    prototype_scalar, surface_prototype_frame_bounds, unique_surface_prototype_associations,
};

pub(in super::super) fn transfer_paired_envelope_spheres(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    if !matches!(scan.framing.layout, crate::container::Layout::Nd) {
        return Ok(0);
    }
    let mut frames = super::prototypes::PrototypeFrames::new(ctx)?;
    let mut transferred = 0;
    let mut association_storage = ctx.reserve_scoped(0, "creo paired sphere association workspace")?;
    let mut associations = Vec::new();
    let prototypes = association_storage.with_storage(|| unique_surface_prototype_associations(ctx, scan))?;
    for (prototype, associated_row, section) in ctx
        .admit_iter(&prototypes, "creo paired sphere prototype traversal")?
        .copied()
    {
        let prototype = prototype.record();
        let Some(frame) = surface_prototype_frame_bounds(ctx, scan, section, prototype.offset, &mut frames)?
        else {
            continue;
        };
        ctx.reserve_scoped_vec(&mut association_storage, &mut associations, 1, "creo paired sphere associations")?;
        associations.push((prototype, associated_row, section, frame));
    }
    let mut counts = std::collections::HashMap::new();
    association_storage.with_storage(|| {
        for (prototype, row, _, frame) in ctx.admit_iter(&associations, "creo paired sphere association count")? {
            if matches!(prototype.family, crate::surface::SurfacePrototypeFamily::Torus(_)) {
                *ctx.entry_hash_map(&mut counts, (row.feature_id, *frame), "creo paired sphere count nodes")?.or_insert(0usize) += 1;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    })?;
    for (prototype, associated_row, section, (frame_start, frame_end)) in ctx.admit_iter(
        &associations,
        "creo transfer paired envelope spheres associations traversal",
    )? {
        if !matches!(
            prototype.family,
            crate::surface::SurfacePrototypeFamily::Torus(_)
        ) || prototype_scalar(ctx, prototype, "radius1")? != Some(0.0)
        {
            continue;
        }
        let Some(radius) = prototype_scalar(ctx, prototype, "radius2")?
            .filter(|radius| radius.is_finite() && *radius > 0.0)
        else {
            continue;
        };
        let associated_prototype_count = counts.get(&(associated_row.feature_id, (*frame_start, *frame_end))).copied().unwrap_or(0);
        if associated_prototype_count != 1 {
            continue;
        }
        let mut rows = scan.surfaces.rows.iter();
        let mut selected = [None; 3];
        for selected in &mut selected {
            *selected = ctx.find_by(&mut rows, |row| Ok(row.offset >= *frame_start && row.offset < *frame_end
                && row.feature_id == associated_row.feature_id && row.kind == crate::surface::SurfaceKind::TorusOrSphere),
                "creo paired sphere row search")?;
        }
        let [Some(first_row), Some(second_row), None] = selected else { continue; };
        let envelopes = [first_row, second_row].map(|row| {
            Ok(unique_surface_parameter_record(ctx, scan, row)?
                .and_then(crate::surface::SurfaceParameterRecord::type26_five_coordinate_envelope))
        });
        let [first_envelope, second_envelope]: [Result<_, cadmpeg_core::CodecError>; 2] = envelopes;
        let (Some(first_envelope), Some(second_envelope)) = (first_envelope?, second_envelope?)
        else {
            continue;
        };
        let Some(center) =
            paired_five_coordinate_sphere_center(ctx, [first_envelope, second_envelope], radius)?
        else {
            continue;
        };
        for row in [first_row, second_row] {
            let (id, id_storage) = crate::identity::compose_scoped::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                row.id,
                "creo decoded model identity",
            )?;
            let identity_present = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
            if identity_present {
                continue;
            }
            let Ok(sphere_surface) = cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::from(center),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            ) else {
                continue;
            };
            id_storage.commit()?;
        annotate(
                ctx,
                annotations,
                &id,
                section.name(),
                cadmpeg_core::decode::u64_from_index(row.offset),
                "paired_type26_sphere_envelope",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                        sphere_surface,
                    )),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(
                            ctx,
                            format_args!("{}:{}", section.name(), row.id),
                            "creo source object identity",
                        )?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
            transferred += 1;
        }
    }
    Ok(transferred)
}



pub(in super::super) fn transfer_positional_tori(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut workspace = ctx.reserve_scoped(0, "creo positional selection workspace")?;
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut round_feature_ids = BTreeSet::new();
    for row in ctx.admit_iter(
        &*scan.surfaces.rows,
        "creo transfer positional tori rows traversal",
    )? {
        if row.kind == crate::surface::SurfaceKind::TorusOrSphere
            && feature_schema_class(ctx, scan, row.feature_id)? == Some(SchemaClass::Round)
        {
            workspace.with_storage(|| ctx.insert_btree_set(
                &mut round_feature_ids,
                row.feature_id,
                "creo positional torus round feature ids",
            ))?;
        }
    }
    let mut constant_round_feature_ids = BTreeSet::new();
    for feature_id in ctx
        .admit_iter(
            &round_feature_ids,
            "creo positional round feature traversal",
        )?
        .copied()
    {
        if round_constant_radius(ctx, scan, ir, source_carriers, feature_id)?.is_some() {
            workspace.with_storage(|| ctx.insert_btree_set(
                &mut constant_round_feature_ids,
                feature_id,
                "creo positional torus constant round ids",
            ))?;
        }
    }
    let mut transferred = 0;
    for record in ctx.admit_iter(
        &*scan.surfaces.parameters,
        "creo transfer positional tori parameters traversal",
    )? {
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
        else {
            continue;
        };
        if row.kind != crate::surface::SurfaceKind::TorusOrSphere
            || crate::surface::unique_surface_parameter(
                &scan.surfaces.parameters,
                record.surface_id,
            )
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        // Class-913 type-26 rows can be rolling-radius samples from the same
        // generated round family. A positional torus frame is a neutral
        // carrier only after the complete family proves one constant radius.
        let inline_non_plane = record.has_inline_non_plane_envelope()
            || record.has_inline_non_plane_local_system_suffix(ctx)?;
        if row.kind == crate::surface::SurfaceKind::TorusOrSphere
            && feature_schema_class(ctx, scan, row.feature_id)? == Some(SchemaClass::Round)
            && !ctx.contains_btree_set(&constant_round_feature_ids, &row.feature_id, "creo constant round feature ids lookup")?
            && !inline_non_plane
        {
            continue;
        }
        let Some(frame) = record.positional_torus_frame() else {
            continue;
        };
        let (id, id_storage) = crate::identity::compose_scoped::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            row.id,
            "creo decoded model identity",
        )?;
        let identity_present = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if identity_present {
            continue;
        }
        let Some(section) = ctx.find_by(
            &scan.framing.sections,
            |section| Ok(section.contains(row.offset)),
            "creo positional section search",
        )?
        else {
            continue;
        };
        let center = frame.frame().finite_origin();
        let placement = frame.frame().orthonormal_frame();
        let minor_radius = frame.minor_radius().into();
        let geometry = if let Ok(major_radius) =
            cadmpeg_ir::scalar::PositiveLength::try_from(frame.major_radius())
        {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                cadmpeg_ir::geometry::analytic::TorusSurface::new(
                    center,
                    placement,
                    major_radius,
                    minor_radius,
                ),
            ))
        } else {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::analytic::SphereSurface::new(center, placement, minor_radius),
            ))
        };
        id_storage.commit()?;
        annotate(
            ctx,
            annotations,
            &id,
            section.name(),
            cadmpeg_core::decode::u64_from_index(row.offset),
            "positional_torus_frame",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("{}:{}", section.name(), row.id),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

pub(in super::super) fn transfer_positional_line_extrusion_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut workspace = ctx.reserve_scoped(0, "creo positional selection workspace")?;
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut replay_bound_surfaces = BTreeSet::new();
    for replay in ctx.admit_iter(
        &scan.curves.tabulated_cylinder_replays,
        "creo transfer tabulated cylinder spline extrusions tabulated cylinder replays traversal",
    )? {
        workspace.with_storage(|| ctx.insert_btree_set(
            &mut replay_bound_surfaces,
            replay.surface_id,
            "creo line-extrusion replay surface ids",
        ))?;
    }
    let mut transferred = 0;
    for record in ctx.admit_iter(
        &*scan.surfaces.parameters,
        "creo transfer positional line extrusion planes parameters traversal",
    )? {
        if ctx.contains_btree_set(&replay_bound_surfaces, &record.surface_id, "creo replay bound surfaces lookup")? {
            continue;
        }
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        if crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id).is_none() {
            continue;
        }
        let Some(frame) = record.line_extrusion_frame() else {
            continue;
        };
        let directrix =
            std::array::from_fn(|axis| frame.directrix[1][axis] - frame.directrix[0][axis]);
        let (Some(_direction), Some(u_axis), Some(normal)) = (
            normalize(frame.direction),
            normalize(directrix),
            normalize(cross(directrix, frame.direction)),
        ) else {
            continue;
        };
        let (surface_id, surface_id_storage) = crate::identity::compose_scoped::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            record.surface_id,
            "creo decoded model identity",
        )?;
        let identity_present = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), surface_id.as_str())?.is_some();
        if identity_present {
            continue;
        }
        let (curve_id, curve_id_storage) = crate::identity::compose_scoped::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE_DIRECTRIX,
            record.surface_id,
            "creo positional directrix identity",
        )?;
        let (procedural_id, procedural_id_storage) = crate::identity::compose_scoped::<ProceduralSurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE_EXTRUSION,
            record.surface_id,
            "creo positional extrusion identity",
        )?;
        let Ok(line_curve) = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::from(frame.directrix[0]),
            Vector3::from(u_axis),
        ) else {
            continue;
        };
        let Ok(plane_surface) = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::from(frame.directrix[0]),
            Vector3::from(normal),
            Vector3::from(u_axis),
        ) else {
            continue;
        };
        curve_id_storage.commit()?;
        procedural_id_storage.commit()?;
        surface_id_storage.commit()?;
        annotate(
            ctx,
            annotations,
            &curve_id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(record.body_offset),
            "positional_line_extrusion_directrix",
            Exactness::Derived,
        )?;
        annotate(
            ctx,
            annotations,
            &surface_id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(record.body_offset),
            "positional_line_extrusion_plane",
            Exactness::Derived,
        )?;
        annotate(
            ctx,
            annotations,
            &procedural_id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(record.body_offset),
            "positional_line_extrusion_construction",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                id: curve_id.try_clone_for_decode(ctx, "creo construction curve identity copy")?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("VisibGeom:surface_directrix#{}", record.surface_id),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id: surface_id
                    .try_clone_for_decode(ctx, "creo construction surface identity copy")?,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("VisibGeom:{}", record.surface_id),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        source_carriers.admit_procedural_surface(
            ctx,
            ir,
            &surface_id,
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                curve_id,
                None,
                Vector3::from(frame.direction),
                None,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .map(|admitted_payload| {
                ProceduralSurface::new(
                    procedural_id,
                    ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                    None,
                )
            })
            .map_err(cadmpeg_core::CodecError::malformed)?,
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

fn section_contains_offset(section: &crate::container::Section, offset: usize) -> bool {
    section.contains(offset)
}

/// Report every refused tabulated-cylinder lane against the row that stated it.
fn note_tabulated_cylinder_refusals(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface_id: u32,
    replay_offset: usize,
    lane: &str,
    refused: &[String],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    for record in ctx.admit_iter(refused, "creo tabulated cylinder refusal traversal")? {
        let message = ctx.format_retained(
            format_args!(
                "VisibGeom surface row {surface_id} states a tabulated-cylinder replay at offset \
                 {replay_offset} whose {lane} lane forms no carrier: {record}"
            ),
            "creo tabulated cylinder refusal text",
        )?;
        ctx.reserve_vec(losses, 1, "creo tabulated cylinder losses")?;
        losses.push(crate::loss::CreoLossCode::VisibGeomSurfaceUntransferred.note(message));
    }
    Ok(())
}

fn unique_tabulated_cylinder_prototype<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>, scan: &'a ContainerScan<'_>,
    replay: &crate::surface::TabulatedCylinderCurveReplay,
) -> Result<Option<&'a crate::surface::SurfacePrototypeRecord>, cadmpeg_core::CodecError> {
    let Some(section) = crate::decode::uniqueness::exactly_one_by(ctx, &scan.framing.sections,
        |section| Ok(section_contains_offset(section, replay.surface_row_offset)),
        "creo tabulated replay section search")? else { return Ok(None); };
    crate::decode::uniqueness::exactly_one_by(ctx, &scan.surfaces.prototype_records, |record| {
        if !section_contains_offset(section, record.offset)
            || record.family != crate::surface::SurfacePrototypeFamily::Extrusion(crate::surface::ExtrusionLabel::TabulatedCylinder) { return Ok(false); }
        let Some(field) = super::prototypes::prototype_field(ctx, record, "c_pnts")? else { return Ok(false); };
        let crate::surface::SurfaceNamedValue::ContiguousEntityReferences(ids) = &field.value else { return Ok(false); };
        Ok(<&[u32; 4]>::try_from(ids.as_slice()).ok() == Some(&replay.control_point_ids))
    }, "creo tabulated replay prototype search")
}

/// Transfer one exact extrusion carrier per tabulated-cylinder spline replay.
///
/// A refused directrix or extrusion lane leaves the surface row without a
/// carrier, which the model carries, so it is a loss note naming the
/// `VisibGeom` surface row and the replay offset.
pub(in super::super) fn transfer_tabulated_cylinder_spline_extrusions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut workspace = ctx.reserve_scoped(0, "creo positional selection workspace")?;
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut replay_counts = BTreeMap::<u32, usize>::new();
    for replay in ctx.admit_iter(
        &scan.curves.tabulated_cylinder_replays,
        "creo transfer positional line extrusion planes tabulated cylinder replays traversal",
    )? {
        workspace.with_storage(|| {
        *ctx.entry_btree_map(
            &mut replay_counts,
            replay.surface_id,
            "creo tabulated-cylinder replay counts",
        )?
        .or_default() += 1;
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
    }
    let mut transferred = 0;
    for replay in ctx.admit_iter(
        &scan.curves.tabulated_cylinder_replays,
        "creo transfer tabulated cylinder spline extrusions tabulated cylinder replays traversal",
    )? {
        if ctx.get_btree_map(&replay_counts, &replay.surface_id, "creo replay counts lookup")? != Some(&1) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, replay.surface_id)
        else {
            continue;
        };
        if row.kind
            != crate::surface::SurfaceKind::Extrusion(
                crate::surface::ExtrusionVariant::TabulatedCylinder,
            )
            || row.offset != replay.surface_row_offset
        {
            continue;
        }
        let Some(parameters) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, replay.surface_id)
        else {
            continue;
        };
        let chart_origin = match unique_tabulated_cylinder_prototype(ctx, scan, replay)? {
            Some(prototype) => super::prototypes::prototype_tabulated_chart_origin(ctx, prototype)?,
            None => None,
        };
        let mut geometry_storage = ctx.reserve_scoped(0, "creo tabulated extrusion geometry")?;
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let directrix = geometry_storage.with_storage(|| placed_tabulated_cylinder_directrix(
            ctx,
            replay,
            parameters,
            chart_origin,
            &mut refusal,
        ))?;
        let refused = refusal.take_records_checked()?;
        let Some((directrix, sweep)) = directrix.filter(|_| refused.is_empty()) else {
            note_tabulated_cylinder_refusals(
                ctx,
                replay.surface_id,
                replay.offset,
                "directrix",
                &refused,
                losses,
            )?;
            continue;
        };
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let surface = geometry_storage.with_storage(|| extruded_nurbs_surface(
            ctx,
            &directrix,
            sweep,
            &format_args!(
                "VisibGeom surface row {} tabulated-cylinder replay at offset {}",
                replay.surface_id, replay.offset
            ),
            &mut refusal,
        ))?;
        let refused = refusal.take_records_checked()?;
        let Some(surface) = surface.filter(|_| refused.is_empty()) else {
            note_tabulated_cylinder_refusals(
                ctx,
                replay.surface_id,
                replay.offset,
                "extrusion",
                &refused,
                losses,
            )?;
            continue;
        };
        let (curve_id, curve_id_storage) = crate::identity::compose_scoped::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_TABULATED_DIRECTRIX,
            replay.surface_id,
            "creo tabulated directrix identity",
        )?;
        let (surface_id, surface_id_storage) = crate::identity::compose_scoped::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            replay.surface_id,
            "creo decoded model identity",
        )?;
        let identity_present = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), surface_id.as_str())?.is_some();
        if identity_present {
            continue;
        }
        let (procedural_id, procedural_id_storage) = crate::identity::compose_scoped::<ProceduralSurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_TABULATED_EXTRUSION,
            replay.surface_id,
            "creo tabulated extrusion identity",
        )?;
        geometry_storage.commit()?;
        curve_id_storage.commit()?;
        procedural_id_storage.commit()?;
        surface_id_storage.commit()?;
        annotate(
            ctx,
            annotations,
            &curve_id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(replay.offset),
            "tabulated_cylinder_directrix",
            Exactness::Derived,
        )?;
        annotate(
            ctx,
            annotations,
            &surface_id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(replay.surface_row_offset),
            "tabulated_cylinder_surface",
            Exactness::Derived,
        )?;
        annotate(
            ctx,
            annotations,
            &procedural_id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(replay.surface_row_offset),
            "tabulated_cylinder_extrusion",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                id: curve_id.try_clone_for_decode(ctx, "creo construction curve identity copy")?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("VisibGeom:curve#{}", replay.curve_id),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id: surface_id
                    .try_clone_for_decode(ctx, "creo construction surface identity copy")?,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("VisibGeom:{}", replay.surface_id),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        source_carriers.admit_procedural_surface(
            ctx,
            ir,
            &surface_id,
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                curve_id,
                Some([0.0, 1.0]),
                Vector3::from(sweep),
                None,
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .map(|admitted_payload| {
                ProceduralSurface::new(
                    procedural_id,
                    ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                    None,
                )
            })
            .map_err(cadmpeg_core::CodecError::malformed)?,
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

#[cfg(test)]
mod tests;
