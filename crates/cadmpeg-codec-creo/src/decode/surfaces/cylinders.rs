// SPDX-License-Identifier: Apache-2.0
//! Hole, split, round, and positional cylinders and cones.

use crate::feature::rows::agreed_feature_affected_ids;
use crate::feature::schema::SchemaClass;
use crate::vecmath::normalize;
use crate::vecmath::unit_length;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::feature_history::dependencies::{
    agreed_feature_replay_geometry_ids, has_feature_affected_ids,
};
use super::super::feature_history::draft::section_sweep_allows_linear_extrusion;
use super::super::feature_history::round::{
    round_constant_radius, round_support_envelope_cylinder, slot_fillet_cylinder,
};
use super::super::holes::counterbore::{
    counterbore_dimension_tuple_matches_radius, counterbore_dimensions,
    counterbore_patch_geometries,
};
use super::super::holes::placement::cylinder_from_complementary_outline_bounds;
use super::super::holes::sweep::{circular_sweep_geometry, simple_hole_geometry};
use super::super::native::annotate;
use crate::decode::analytic::equations::{plane_intersection_line, PlaneEquation};
use crate::decode::analytic::planes::{is_axis_aligned, placed_planes, reconciled_model_plane};
use crate::decode::sketch_transfer::recipe::{
    feature_recipe, feature_schema_class, feature_section_sweep_semantics_conflict,
};
use crate::vecmath::{cross, dot};

/// Positional residual tolerance for reconstructed cylinders.
const EPS_CYLINDER_POSITION: f64 = 1.0e-8;
/// General reconstructed cylinder geometry tolerance.
const EPS_CYLINDER_GEOMETRY: f64 = 1.0e-9;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct PositionalCylinderTransferSummary {
    pub(in crate::decode) transferred: usize,
    pub(in crate::decode) round_edge_complete_envelopes: usize,
    pub(in crate::decode) round_edge_missing_support_planes: usize,
    pub(in crate::decode) round_edge_unsolved_carriers: usize,
    pub(in crate::decode) round_edge_solved_carriers: usize,
    pub(in crate::decode) round_edge_transferred_carriers: usize,
    pub(in crate::decode) round_edge_no_perpendicular_support_pair: usize,
    pub(in crate::decode) round_edge_endpoint_incidence_mismatch: usize,
    pub(in crate::decode) round_edge_radius_projection_mismatch: usize,
    pub(in crate::decode) round_edge_nonunique_radius: usize,
    pub(in crate::decode) round_edge_carrier_validation_failure: usize,
    pub(in crate::decode) round_edge_replay_conflict: usize,
    pub(in crate::decode) axial_interval_corner_envelopes: usize,
    pub(in crate::decode) axial_interval_corner_solved_carriers: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PerpendicularRoundEdgeFailure {
    NoPerpendicularSupportPair,
    EndpointIncidenceMismatch,
    RadiusProjectionMismatch,
    NonuniqueRadius,
    CarrierValidationFailure,
}

pub(in super::super) fn rowless_round_cylinder_pairs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &crate::surface::SurfaceRows,
) -> Result<Vec<(u32, u32, usize)>, cadmpeg_core::CodecError> {
    let mut pairs = Vec::new();
    for table in ctx.admit_iter(tables, "creo rowless round feature tables")? {
        let feature_id = table.feature_id;
        if !ctx.contains_btree_set(&round_feature_ids, &feature_id, "creo round feature ids lookup")? {
            continue;
        }
        let [first, second, rowless, cylinder] = table.entries.as_slice() else {
            continue;
        };
        if crate::surface::unique_surface_row(rows, first.entity_id).is_none()
            || crate::surface::unique_surface_row(rows, second.entity_id).is_none()
        {
            continue;
        }
        if rows.contains_id(rowless.entity_id) {
            continue;
        }
        if !crate::surface::unique_surface_row(rows, cylinder.entity_id).is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        }) {
            continue;
        }
        ctx.reserve_vec(&mut pairs, 1, "creo rowless round cylinder pairs")?;
        pairs.push((rowless.entity_id, cylinder.entity_id, table.offset));
    }
    Ok(pairs)
}

pub(in super::super) fn transfer_active_datum_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut transferred = 0;
    for datum in ctx.admit_iter(
        &scan.planes.datum_cylinders,
        "creo transfer active datum cylinders datum cylinders traversal",
    )? {
        let id = super::native_surface_id(ctx, scan, datum.id)?;
        let surface_exists = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if surface_exists {
            continue;
        }
        let frame = datum.frame;
        let radius = frame.radius();
        let cylinder_surface = cadmpeg_ir::geometry::analytic::CylinderSurface::new(
            frame.frame().finite_origin(),
            frame.frame().orthonormal_frame(),
            radius,
        );
        annotate(
            ctx,
            annotations,
            &id,
            "ActDatums",
            cadmpeg_core::decode::u64_from_index(datum.offset_in_payload),
            "active_datum_cylinder",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cylinder_surface,
                )),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("ActDatums:{}", datum.id),
                            "creo active datum cylinder source IDs",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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

pub(in super::super) fn transfer_constrained_slot_fillet_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut round_feature_ids = BTreeSet::new();
    for row in ctx
        .admit_iter(
            &scan.features.rows,
            "creo constrained slot round feature rows",
        )?
        .filter(|row| row.root_schema_class == Some(SchemaClass::Round))
    {
        local_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut round_feature_ids,
                row.feature_id,
                "creo constrained round feature ID nodes",
            )
        })?;
    }
    let mut transferred = 0;
    for feature_id in ctx.admit_iter(
        &round_feature_ids,
        "creo constrained slot round feature IDs",
    )? {
        let named = agreed_feature_affected_ids(
            &scan.features.affected_ids,
            *feature_id,
            crate::feature::rows::AffectedIdKind::Geometry,
        );
        let named_present = has_feature_affected_ids(
            ctx,
            &scan.features.affected_ids,
            *feature_id,
            crate::feature::rows::AffectedIdKind::Geometry,
        )?;
        let replay = agreed_feature_replay_geometry_ids(
            ctx,
            &scan.features.replay_affected_ids,
            *feature_id,
        )?;
        let affected = match (named, replay) {
            (Some(ids), _) => ids,
            (None, Some(ids)) if !named_present => ids,
            _ => continue,
        };
        let Some((cap_ids, support_ids)) = affected.split_at_checked(2) else {
            continue;
        };
        if support_ids.len() < 4 {
            continue;
        }
        let mut plane_storage = ctx.reserve_scoped(0, "creo constrained slot plane workspace")?;
        let local_planes = plane_storage.with_storage(|| placed_planes(ctx, scan))?;
        let mut planes = Vec::new();
        let mut visits = affected.iter();
        while let Some(id) = ctx.next_charged(&mut visits, "creo constrained slot affected IDs")? {
            let Some(plane) = reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *id)?
            else {
                break;
            };
            ctx.reserve_scoped_vec(&mut plane_storage, &mut planes, 1, "creo constrained slot plane rows")?;
            planes.push(plane);
        }
        if planes.len() != affected.len() {
            continue;
        }
        let cap_planes = [planes[0], planes[1]];
        let Some(cylinder) = slot_fillet_cylinder(ctx, cap_planes, &planes[cap_ids.len()..])?
        else {
            continue;
        };
        let mut selected_row = None;
        let mut ambiguous_row = false;
        let mut visits = scan.surfaces.rows.iter();
        while let Some(row) = ctx.next_charged(&mut visits, "creo constrained slot cylinder rows")? {
            if row.feature_id != *feature_id || row.kind != crate::surface::SurfaceKind::Cylinder {
                continue;
            }
            let (key, _key_storage) = crate::identity::compose_scoped::<SurfaceId>(ctx,
                &crate::identity::VISIBGEOM_SURFACE, row.id, "creo constrained slot query identity")?;
            let already_present = surfaces_index.lookup(ctx, &ir.model.surfaces,
                |surface| surface.id.as_str(), key.as_str())?.is_some();
            if already_present {
                continue;
            }
            if selected_row.is_some() {
                ambiguous_row = true;
                break;
            }
            selected_row = Some(row);
        }
        let Some(row) = selected_row.filter(|_| !ambiguous_row) else {
            continue;
        };
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            row.id,
            "creo constrained slot cylinder identity",
        )?;
        let Ok(cylinder_surface) = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::from(cylinder.origin),
            Vector3::from(cylinder.axis),
            Vector3::from(cylinder.ref_direction),
            cylinder.radius,
        ) else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            &id,
            "AllFeatur",
            cadmpeg_core::decode::u64_from_index(row.offset),
            "constrained_slot_fillet_cylinder",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cylinder_surface,
                )),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("AllFeatur:{}:{}", feature_id, row.id),
                            "creo constrained slot cylinder source IDs",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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

#[cfg(test)]
mod tests;

pub(in super::super) fn transfer_rowless_round_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut candidate_storage = ctx.reserve_scoped(0, "creo cylinder candidate workspace")?;
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut round_feature_ids = BTreeSet::new();
    for row in ctx
        .admit_iter(&scan.features.rows, "creo rowless round feature rows")?
        .filter(|row| row.root_schema_class == Some(SchemaClass::Round))
    {
        candidate_storage.with_storage(|| ctx.insert_btree_set(
            &mut round_feature_ids,
            row.feature_id,
            "creo rowless round feature ID nodes",
        ))?;
    }
    let mut transferred = 0;
    let pairs = candidate_storage.with_storage(|| rowless_round_cylinder_pairs(
        ctx,
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    ))?;
    for (rowless_id, sibling_id, offset) in
        ctx.admit_iter(&pairs, "creo rowless round cylinder candidates")?
    {
        let (sibling, _sibling_storage) = crate::identity::compose_scoped::<SurfaceId>(
            ctx, &crate::identity::VISIBGEOM_SURFACE, *sibling_id,
            "creo rowless sibling query identity",
        )?;
        let selected_surface = surfaces_index.lookup(ctx, &ir.model.surfaces,
            |record| record.id.as_str(), sibling.as_str())?.flatten()
            .map(|index| &ir.model.surfaces[index]);
        let Some(cylinder_surface) = selected_surface.and_then(
            |surface| match source_carriers.surface_geometry(surface) {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) => {
                    Some(*cylinder)
                }
                _ => None,
            },
        ) else {
            continue;
        };
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            *rowless_id,
            "creo rowless round cylinder identity",
        )?;
        let surface_exists = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if surface_exists {
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            "AllFeatur",
            cadmpeg_core::decode::u64_from_index(*offset),
            "round_rowless_sibling_cylinder",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cylinder_surface,
                )),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("AllFeatur:{rowless_id}"),
                            "creo rowless round cylinder source IDs",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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

pub(in super::super) fn transfer_hole_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut hole_feature_ids = BTreeSet::new();
    for feature_id in ctx
        .admit_iter(&scan.features.rows, "creo hole feature rows")?
        .filter(|row| row.root_schema_class == Some(SchemaClass::Hole))
        .map(|row| row.feature_id)
    {
        local_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut hole_feature_ids,
                feature_id,
                "creo hole cylinder feature ID nodes",
            )
        })?;
    }
    let mut transferred = 0;
    for feature_id in ctx.admit_iter(&hole_feature_ids, "creo hole cylinder feature IDs")? {
        let simple = simple_hole_geometry(ctx, scan, *feature_id)?;
        let counterbore = if simple.is_some() {
            None
        } else {
            counterbore_patch_geometries(ctx, scan, ir, *feature_id)?
        };
        let mut transfer_hole_cylinder =
            |row: &crate::surface::SurfaceRow,
             geometry: cadmpeg_ir::geometry::analytic::CylinderSurface|
             -> Result<(), cadmpeg_core::CodecError> {
                let cylinder_id = row.id;
                let id = crate::identity::compose_checked::<SurfaceId>(
                    ctx,
                    &crate::identity::VISIBGEOM_SURFACE,
                    cylinder_id,
                    "creo hole cylinder identity",
                )?;
                let surface_exists = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
                if surface_exists {
                    return Ok(());
                }
                annotate(
                    ctx,
                    annotations,
                    &id,
                    "AllFeatur",
                    cadmpeg_core::decode::u64_from_index(row.offset),
                    "hole_cap_outline_cylinder",
                    Exactness::Derived,
                )?;
                ctx.charge_entities(1, "admit Creo model surfaces")?;
                source_carriers.admit_surface(
                    ctx,
                    ir,
                    Surface {
                        id,
                        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                            geometry,
                        )),
                        source_object: Some(SourceObjectAssociation {
                            format: cadmpeg_ir::CodecFormat::Creo,
                            object_id: cadmpeg_core::text::NonBlankString::for_decode(
                                ctx,
                                ctx.format_retained(
                                    format_args!("VisibGeom:{cylinder_id}"),
                                    "creo hole cylinder source IDs",
                                )?,
                                "validate nonblank text",
                            )?
                            .ok_or_else(|| {
                                cadmpeg_core::CodecError::malformed(
                                    "source object_id must not be empty",
                                )
                            })?,
                            name: None,
                            color: None,
                            visible: None,
                            layer: None,
                            instance_path: Vec::new(),
                        }),
                    },
                )?;
                transferred += 1;
                Ok(())
            };
        if let Some(hole) = &simple {
            for row in ctx.admit_iter(
                &hole.cylinder_rows,
                "creo simple hole cylinder rows traversal",
            )? {
                transfer_hole_cylinder(row, hole.geometry)?;
            }
        }
        if let Some(rows) = &counterbore {
            for (row, geometry) in
                ctx.admit_iter(rows, "creo counterbore patch cylinder rows traversal")?
            {
                transfer_hole_cylinder(row, *geometry)?;
            }
        }
    }
    Ok(transferred)
}

pub(in super::super) fn transfer_split_outline_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut workspace = ctx.reserve_scoped(0, "creo split cylinder workspace")?;
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let local_planes = workspace.with_storage(|| placed_planes(ctx, scan))?;
    let mut cylinders_by_plane = BTreeMap::<(u32, u32), BTreeSet<u32>>::new();
    let unique_topologies = workspace.with_storage(|| crate::identity::uniquely_identified_rows_checked(
        ctx,
        &scan.curves.topology_rows,
        |row| row.id,
    ))?;
    for edge in ctx.admit_iter(&unique_topologies, "creo split outline unique topologies")? {
        if edge.type_byte != 0 {
            continue;
        }
        let [Some(left), Some(right)] = edge.faces else {
            continue;
        };
        let (left, right) = (left.get(), right.get());
        let pair = match (scan.surfaces.rows.unique(left), scan.surfaces.rows.unique(right)) {
            (Some(plane), Some(cylinder))
                if plane.kind == crate::surface::SurfaceKind::Plane
                    && cylinder.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                Some(((left, cylinder.feature_id), right))
            }
            (Some(cylinder), Some(plane))
                if plane.kind == crate::surface::SurfaceKind::Plane
                    && cylinder.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                Some(((right, cylinder.feature_id), left))
            }
            _ => None,
        };
        if let Some((plane_and_feature, cylinder)) = pair {
            workspace.with_storage(|| {
            let cylinder_ids = ctx
                .entry_btree_map(
                    &mut cylinders_by_plane,
                    plane_and_feature,
                    "creo split cylinder plane nodes",
                )?
                .or_default();
            ctx.insert_btree_set(cylinder_ids, cylinder, "creo split cylinder ID nodes")?;
                Ok::<_, cadmpeg_core::CodecError>(())
            })?;
        }
    }

    let mut transferred = 0;
    for ((plane_id, _), cylinder_ids) in
        ctx.admit_iter(&cylinders_by_plane, "creo split cylinder plane groups")?
    {
        if cylinder_ids.len() != 2 { continue; }
        let mut cylinder_ids = cylinder_ids.iter().copied();
        let (Some(first_id), Some(second_id)) = (cylinder_ids.next(), cylinder_ids.next()) else { continue; };
        let Some(first) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, first_id)
        else {
            continue;
        };
        let Some(second) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, second_id)
        else {
            continue;
        };
        let Some(bounds) = first
            .split_cylinder_outline_bounds()
            .zip(second.split_cylinder_outline_bounds())
            .map(|(first, second)| [first, second])
        else {
            continue;
        };
        let Some(plane) =
            reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *plane_id)?
        else {
            continue;
        };
        let normal = Vector3::from(plane.normal);
        let plane_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            match cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::from(plane.origin),
                normal,
                cadmpeg_ir::geometry::derive_reference_direction(normal),
            ) {
                Ok(payload) => payload,
                Err(_) => continue,
            },
        ));
        let Some(geometry) = cylinder_from_complementary_outline_bounds(&plane_geometry, bounds)
        else {
            continue;
        };
        for cylinder_id in [first_id, second_id] {
            let id = crate::identity::compose_checked::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                cylinder_id,
                "creo split cylinder identities",
            )?;
            let surface_exists = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
            if surface_exists {
                continue;
            }
            let Some(row) = scan.surfaces.rows.unique(cylinder_id) else {
                continue;
            };
            annotate(
                ctx,
                annotations,
                &id,
                "VisibGeom",
                cadmpeg_core::decode::u64_from_index(row.offset),
                "split_outline_cylinder",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id,
                    geometry: geometry.try_clone_for_decode(ctx, "creo split cylinder geometry")?,
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.format_retained(
                                format_args!("VisibGeom:{cylinder_id}"),
                                "creo split cylinder source object IDs",
                            )?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "source object_id must not be empty",
                            )
                        })?,
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

fn round_edge_cylinder_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    envelope: crate::surface::Type24RoundEdgeEnvelope,
    radius: f64,
    support_planes: &[PlaneEquation],
) -> Result<Option<crate::surface::PositionalCylinderFrame>, cadmpeg_core::CodecError> {
    if !radius.is_finite() || radius <= 0.0 {
        return Ok(None);
    }
    let [first, second] = envelope.vertices;
    if !first.into_iter().chain(second).all(f64::is_finite) {
        return Ok(None);
    }
    let close_to_radius =
        |value: f64| (value - radius).abs() <= EPS_CYLINDER_GEOMETRY * radius.max(1.0);
    let plane_contains = |point: [f64; 3], plane: PlaneEquation| {
        let scale = point
            .into_iter()
            .chain(plane.origin)
            .map(f64::abs)
            .fold(1.0, f64::max);
        (dot(plane.normal, point) - dot(plane.normal, plane.origin)).abs()
            <= EPS_CYLINDER_POSITION * scale
    };
    let distance_from_axis = |point: [f64; 3], origin: [f64; 3], axis: [f64; 3]| {
        let relative = std::array::from_fn(|index| point[index] - origin[index]);
        let radial =
            std::array::from_fn(|index| relative[index] - axis[index] * dot(relative, axis));
        dot(radial, radial).sqrt()
    };
    let mut candidate: Option<crate::surface::PositionalCylinderFrame> = None;
    let mut first_planes = support_planes.iter().copied().enumerate();
    while let Some((first_index, first_support)) = ctx.next_charged(&mut first_planes, "creo round-edge first support planes")? {
        let Some(first_normal) = normalize(first_support.normal) else {
            continue;
        };
        let first_support = PlaneEquation {
            origin: first_support.origin,
            normal: first_normal,
        };
        let mut second_planes = support_planes[first_index + 1..].iter().copied();
        while let Some(second_support) = ctx.next_charged(&mut second_planes, "creo round-edge second support planes")? {
            let Some(second_normal) = normalize(second_support.normal) else {
                continue;
            };
            let second_support = PlaneEquation {
                origin: second_support.origin,
                normal: second_normal,
            };
            let cross_norm_squared = dot(
                cross(first_normal, second_normal),
                cross(first_normal, second_normal),
            );
            if cross_norm_squared <= EPS_CYLINDER_GEOMETRY {
                continue;
            }
            let first_on_first = plane_contains(first, first_support);
            let first_on_second = plane_contains(first, second_support);
            let second_on_first = plane_contains(second, first_support);
            let second_on_second = plane_contains(second, second_support);
            if !((first_on_first && second_on_second) || (first_on_second && second_on_first)) {
                continue;
            }
            for first_sign in [-1.0, 1.0] {
                for second_sign in [-1.0, 1.0] {
                    let first_offset = PlaneEquation {
                        origin: std::array::from_fn(|index| {
                            first_support.origin[index] + first_sign * radius * first_normal[index]
                        }),
                        normal: first_normal,
                    };
                    let second_offset = PlaneEquation {
                        origin: std::array::from_fn(|index| {
                            second_support.origin[index]
                                + second_sign * radius * second_normal[index]
                        }),
                        normal: second_normal,
                    };
                    let Some((origin, axis)) = plane_intersection_line(first_offset, second_offset)
                    else {
                        continue;
                    };
                    let first_distance = distance_from_axis(first, origin, axis);
                    let second_distance = distance_from_axis(second, origin, axis);
                    if !close_to_radius(first_distance) || !close_to_radius(second_distance) {
                        continue;
                    }
                    let relative = std::array::from_fn(|index| first[index] - origin[index]);
                    let radial = std::array::from_fn(|index| {
                        relative[index] - axis[index] * dot(relative, axis)
                    });
                    let Some(ref_direction) = normalize(radial) else {
                        continue;
                    };
                    let axial_span = dot(
                        std::array::from_fn(|index| second[index] - first[index]),
                        axis,
                    )
                    .abs();
                    let Some(frame) = crate::surface::PositionalCylinderFrame::new(
                        origin,
                        axis,
                        ref_direction,
                        radius,
                        (axial_span > EPS_CYLINDER_GEOMETRY * radius.max(1.0))
                            .then_some(axial_span),
                    ) else {
                        continue;
                    };
                    let same_line = candidate.is_some_and(|candidate| {
                        let parallel = dot(candidate.frame().axis(), frame.frame().axis()).abs();
                        let origin_distance = distance_from_axis(
                            candidate.frame().origin(),
                            frame.frame().origin(),
                            frame.frame().axis(),
                        );
                        parallel >= 1.0 - EPS_CYLINDER_GEOMETRY
                            && origin_distance <= EPS_CYLINDER_GEOMETRY * radius.max(1.0)
                    });
                    if !same_line {
                        if candidate.is_some() {
                            return Ok(None);
                        }
                        candidate = Some(frame);
                    }
                }
            }
        }
    }
    Ok(candidate)
}

fn unique_tangent_axial_interval_corner_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[crate::surface::PositionalCylinderFrame],
    support_planes: &[PlaneEquation],
) -> Result<Option<crate::surface::PositionalCylinderFrame>, cadmpeg_core::CodecError> {
    let mut best = None;
    let mut maximum = 0;
    let mut tied = false;
    for candidate in ctx
        .admit_iter(candidates, "creo axial interval corner cylinder candidates")?
        .copied()
    {
        let axis = unit_length(*candidate.frame().orthonormal_frame().axis());
        let score = ctx
            .admit_iter(support_planes, "creo axial interval support planes")?
            .filter(|plane| {
                let Some(normal) = normalize(plane.normal) else {
                    return false;
                };
                if dot(axis, normal).abs() > EPS_CYLINDER_GEOMETRY {
                    return false;
                }
                let distance =
                    (dot(normal, candidate.frame().origin()) - dot(normal, plane.origin)).abs();
                (distance - candidate.radius().get()).abs()
                    <= EPS_CYLINDER_POSITION * candidate.radius().get().max(1.0)
            })
            .count();
        if score > maximum {
            maximum = score;
            best = Some(candidate);
            tied = false;
        } else if score != 0 && score == maximum {
            tied = true;
        }
    }
    if tied {
        return Ok(None);
    }
    Ok(best)
}

fn unique_support_tangent_cylinder_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    stored: crate::surface::PositionalCylinderFrame,
    support_planes: &[PlaneEquation],
) -> Result<Option<crate::surface::PositionalCylinderFrame>, cadmpeg_core::CodecError> {
    let axis = unit_length(*stored.frame().orthonormal_frame().axis());
    let mut origins_storage = ctx.reserve_scoped(0, "creo support tangent origin workspace")?;
    let mut witness_storage = ctx.reserve_scoped(0, "creo support tangent witness workspace")?;
    let mut origins = Vec::new();
    ctx.reserve_scoped_vec(&mut origins_storage, &mut origins, 1, "creo support tangent initial origins")?;
    origins.push(stored.frame().origin());
    let mut witnessed_axis = [false; 3];
    let mut witnessed_planes = Vec::new();
    let mut visits = support_planes.iter();
        while let Some(plane) = ctx.next_charged(&mut visits, "creo support tangent support planes")? {
        let Some(normal) = normalize(plane.normal) else {
            return Ok(None);
        };
        if dot(axis, normal).abs() > EPS_CYLINDER_GEOMETRY {
            return Ok(None);
        }
        let mut axis_indices = (0..3).filter(|index| {
            normal[*index].abs() > 1.0 - EPS_CYLINDER_GEOMETRY
                && (0..3)
                    .filter(|other| *other != *index)
                    .all(|other| normal[other].abs() <= EPS_CYLINDER_GEOMETRY)
        });
        let Some(axis_index) = axis_indices.next() else {
            return Ok(None);
        };
        if axis_indices.next().is_some() {
            return Ok(None);
        }
        let plane_offset = dot(normal, plane.origin);
        let candidates = || {
            [
                (plane_offset - stored.radius().get()) / normal[axis_index],
                (plane_offset + stored.radius().get()) / normal[axis_index],
            ]
            .into_iter()
            .filter(|coordinate| coordinate.is_finite())
            .filter(|coordinate| {
                let scale = coordinate
                    .abs()
                    .max(stored.frame().origin()[axis_index].abs())
                    .max(stored.radius().get())
                    .max(1.0);
                (coordinate.abs() - stored.frame().origin()[axis_index].abs()).abs()
                    <= EPS_CYLINDER_POSITION * scale
            })
        };
        if candidates().next().is_none() {
            continue;
        }
        witnessed_axis[axis_index] = true;
        ctx.reserve_scoped_vec(
            &mut witness_storage,
            &mut witnessed_planes,
            1,
            "creo support tangent witness planes",
        )?;
        witnessed_planes.push(PlaneEquation {
            origin: plane.origin,
            normal,
        });
        let mut next_storage = ctx.reserve_scoped(0, "creo support tangent origin workspace")?;
        let mut next = Vec::new();
        for origin in ctx.admit_iter(
            &origins,
            "creo unique support tangent cylinder frame origins traversal",
        )? {
            for coordinate in candidates() {
                let mut candidate = *origin;
                candidate[axis_index] = coordinate;
                if !ctx.any_by(
                    &next,
                    |known: &[f64; 3]| {
                        Ok(known.iter().zip(candidate).all(|(left, right)| {
                            (left - right).abs()
                                <= EPS_CYLINDER_POSITION * left.abs().max(right.abs()).max(1.0)
                        }))
                    },
                    "creo support tangent next origin search",
                )? {
                    ctx.reserve_scoped_vec(&mut next_storage, &mut next, 1, "creo support tangent next origins")?;
                    next.push(candidate);
                }
            }
        }
        origins = next;
        origins_storage = next_storage;
    }
    if !witnessed_axis.into_iter().any(|witnessed| witnessed) {
        return Ok(None);
    }
    let mut frame = None;
    let mut visits = origins.iter();
        while let Some(origin) = ctx.next_charged(&mut visits, "creo support tangent resolved origins")? {
        let tangent_to_all = ctx.all_by(
            &witnessed_planes,
            |plane| {
                Ok({
                    let normal = plane.normal;
                    let distance = (dot(normal, *origin) - dot(normal, plane.origin)).abs();
                    let scale = distance.max(stored.radius().get()).max(1.0);
                    (distance - stored.radius().get()).abs() <= EPS_CYLINDER_POSITION * scale
                })
            },
            "creo support tangent witnessed planes",
        )?;
        if !tangent_to_all {
            continue;
        }
        let Some(candidate) = crate::surface::PositionalCylinderFrame::with_admitted_dimensions(
            *origin,
            stored.frame().axis(),
            stored.frame().ref_direction(),
            stored.radius(),
            stored.length(),
        ) else {
            continue;
        };
        if let Some(known) = frame {
            if !crate::surface::cylinder_frame_readers::positional_cylinder_frames_agree(
                known, candidate,
            ) {
                return Ok(None);
            }
        } else {
            frame = Some(candidate);
        }
    }
    drop(origins_storage);
    Ok(frame)
}

fn perpendicular_round_edge_cylinder_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    envelope: crate::surface::Type24RoundEdgeEnvelope,
    support_planes: &[PlaneEquation],
) -> Result<
    Result<crate::surface::PositionalCylinderFrame, PerpendicularRoundEdgeFailure>,
    cadmpeg_core::CodecError,
> {
    let [first, second] = envelope.vertices;
    let delta = std::array::from_fn::<_, 3, _>(|index| second[index] - first[index]);
    let plane_contains = |point: [f64; 3], plane: PlaneEquation| {
        let scale = point
            .into_iter()
            .chain(plane.origin)
            .map(f64::abs)
            .fold(1.0, f64::max);
        (dot(plane.normal, point) - dot(plane.normal, plane.origin)).abs()
            <= EPS_CYLINDER_POSITION * scale
    };
    let mut radius: Option<f64> = None;
    let mut has_perpendicular_support_pair = false;
    let mut has_endpoint_incidence = false;
    let mut has_equal_radius_projections = false;
    let mut first_planes = support_planes.iter().copied().enumerate();
    while let Some((first_index, first_support)) = ctx.next_charged(&mut first_planes, "creo perpendicular round-edge first support planes")? {
        let Some(first_normal) = normalize(first_support.normal) else {
            continue;
        };
        let first_support = PlaneEquation {
            origin: first_support.origin,
            normal: first_normal,
        };
        let mut second_planes = support_planes[first_index + 1..].iter().copied();
        while let Some(second_support) = ctx.next_charged(&mut second_planes, "creo perpendicular round-edge second support planes")? {
            let Some(second_normal) = normalize(second_support.normal) else {
                continue;
            };
            if dot(first_normal, second_normal).abs() > EPS_CYLINDER_GEOMETRY {
                continue;
            }
            has_perpendicular_support_pair = true;
            let second_support = PlaneEquation {
                origin: second_support.origin,
                normal: second_normal,
            };
            for (first_plane, first_plane_normal, second_plane, second_plane_normal) in [
                (first_support, first_normal, second_support, second_normal),
                (second_support, second_normal, first_support, first_normal),
            ] {
                if !plane_contains(first, first_plane) || !plane_contains(second, second_plane) {
                    continue;
                }
                has_endpoint_incidence = true;
                let first_radius = dot(delta, first_plane_normal).abs();
                let second_radius = dot(delta, second_plane_normal).abs();
                let scale = first_radius.max(second_radius).max(1.0);
                if first_radius <= EPS_CYLINDER_GEOMETRY * scale
                    || (first_radius - second_radius).abs() > EPS_CYLINDER_GEOMETRY * scale
                {
                    continue;
                }
                has_equal_radius_projections = true;
                if let Some(known) = radius {
                    if (known - first_radius).abs() > EPS_CYLINDER_GEOMETRY * scale {
                        return Ok(Err(PerpendicularRoundEdgeFailure::NonuniqueRadius));
                    }
                } else {
                    radius = Some(first_radius);
                }
            }
        }
    }
    let Some(radius) = radius else {
        return Ok(Err(if !has_perpendicular_support_pair {
            PerpendicularRoundEdgeFailure::NoPerpendicularSupportPair
        } else if !has_endpoint_incidence {
            PerpendicularRoundEdgeFailure::EndpointIncidenceMismatch
        } else if !has_equal_radius_projections {
            PerpendicularRoundEdgeFailure::RadiusProjectionMismatch
        } else {
            PerpendicularRoundEdgeFailure::NonuniqueRadius
        }));
    };
    Ok(
        round_edge_cylinder_frame(ctx, envelope, radius, support_planes)?
            .ok_or(PerpendicularRoundEdgeFailure::CarrierValidationFailure),
    )
}

pub(in super::super) fn transfer_positional_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<PositionalCylinderTransferSummary, cadmpeg_core::CodecError> {
    let mut workspace = ctx.reserve_scoped(0, "creo positional cylinder workspace")?;
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut round_feature_ids = BTreeSet::new();
    for row in ctx.admit_iter(
        &*scan.surfaces.rows,
        "creo positional cylinder surface rows",
    )? {
        if row.kind == crate::surface::SurfaceKind::Cylinder
            && feature_schema_class(ctx, scan, row.feature_id)? == Some(SchemaClass::Round)
        {
            workspace.with_storage(|| ctx.insert_btree_set(
                &mut round_feature_ids,
                row.feature_id,
                "creo positional round feature ID nodes",
            ))?;
        }
    }
    let mut constant_round_radii = BTreeMap::new();
    for feature_id in ctx.admit_iter(&round_feature_ids, "creo positional round feature IDs")? {
        if let Some(radius) = round_constant_radius(ctx, scan, ir, source_carriers, *feature_id)? {
            workspace.with_storage(|| ctx.insert_btree_map(
                &mut constant_round_radii,
                *feature_id,
                radius,
                "creo constant round radius nodes",
            ))?;
        }
    }
    let local_planes = workspace.with_storage(|| placed_planes(ctx, scan))?;
    let mut adjacent_plane_ids = BTreeMap::<u32, BTreeSet<u32>>::new();
    let unique_topologies = workspace.with_storage(|| crate::identity::uniquely_identified_rows_checked(
        ctx,
        &scan.curves.topology_rows,
        |row| row.id,
    ))?;
    for edge in ctx.admit_iter(&unique_topologies, "creo positional unique topologies")? {
        let [Some(left), Some(right)] = edge.faces else {
            continue;
        };
        let (left, right) = (left.get(), right.get());
        for (surface_id, other_id) in [(left, right), (right, left)] {
            if scan.surfaces.rows
                .unique(surface_id)
                .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
                && scan.surfaces.rows
                    .unique(other_id)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            {
                workspace.with_storage(|| {
                let plane_ids = match ctx.entry_btree_map(
                    &mut adjacent_plane_ids,
                    surface_id,
                    "creo positional adjacent cylinder nodes",
                )? {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(BTreeSet::new())
                    }
                };
                ctx.insert_btree_set(
                    plane_ids,
                    other_id,
                    "creo positional adjacent plane ID nodes",
                )?;
                    Ok::<_, cadmpeg_core::CodecError>(())
                })?;
            }
        }
    }
    let mut round_edge_support_planes = BTreeMap::new();
    for (surface_id, plane_ids) in
        ctx.admit_iter(&adjacent_plane_ids, "creo adjacent plane groups")?
    {
        let mut planes = Vec::new();
        for plane_id in ctx.admit_iter(plane_ids, "creo adjacent plane IDs")? {
            if let Some(plane) =
                reconciled_model_plane(ctx, &local_planes, ir, source_carriers, *plane_id)?
            {
                workspace.with_storage(|| ctx.reserve_vec(&mut planes, 1, "creo positional support planes"))?;
                planes.push(plane);
            }
        }
        workspace.with_storage(|| ctx.insert_btree_map(
            &mut round_edge_support_planes,
            *surface_id,
            planes,
            "creo positional support plane nodes",
        ))?;
    }
    let mut summary = PositionalCylinderTransferSummary::default();
    for record in ctx.admit_iter(
        &*scan.surfaces.parameters,
        "creo transfer positional cylinders parameters traversal",
    )? {
        let unique_parameter =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id);
        if unique_parameter.is_none_or(|unique| !std::ptr::eq(unique, record)) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        else {
            continue;
        };
        let feature_class = feature_schema_class(ctx, scan, row.feature_id)?;
        let inline_non_plane = record.has_inline_non_plane_envelope()
            || record.has_inline_non_plane_local_system_suffix(ctx)?;
        let selector_corner_interval = record.selector_corner_interval_cylinder_frame().is_some();
        let axial_interval_corner_candidates = if feature_class == Some(SchemaClass::Round)
            && !inline_non_plane
            && !selector_corner_interval
        {
            record.type24_axial_interval_corner_candidates()
        } else {
            None
        };
        let round_edge_envelope = (feature_class == Some(SchemaClass::Round)
            && !selector_corner_interval)
            .then(|| record.type24_round_edge_envelope())
            .flatten();
        if round_edge_envelope.is_some() {
            summary.round_edge_complete_envelopes += 1;
        }
        let round_support_frame = if feature_class == Some(SchemaClass::Round) {
            match record.type24_scalar_frame_round_envelope() {
                Some(envelope) => round_support_envelope_cylinder(
                    ctx,
                    scan,
                    ir,
                    source_carriers,
                    row.feature_id,
                    envelope,
                )?,
                None => None,
            }
        } else {
            None
        };
        let support_planes = ctx.get_btree_map(&round_edge_support_planes, &row.id, "creo round edge support planes lookup")?;
        let support_tangent_frame = if selector_corner_interval {
            None
        } else {
            match (record.positional_cylinder_frame(), support_planes) {
                (Some(stored), Some(planes)) => {
                    unique_support_tangent_cylinder_frame(ctx, stored, planes)?
                }
                _ => None,
            }
        };
        if axial_interval_corner_candidates.is_some() {
            summary.axial_interval_corner_envelopes += 1;
        }
        let axial_interval_corner_frame =
            match (support_planes, axial_interval_corner_candidates.as_ref()) {
                (Some(planes), Some(candidates)) => {
                    unique_tangent_axial_interval_corner_frame(ctx, candidates, planes)?
                }
                _ => None,
            };
        if axial_interval_corner_frame.is_some() {
            summary.axial_interval_corner_solved_carriers += 1;
        }
        let perpendicular_result = match support_planes.zip(round_edge_envelope) {
            Some((support_planes, envelope)) => Some(perpendicular_round_edge_cylinder_frame(
                ctx,
                envelope,
                support_planes,
            )?),
            None => None,
        };
        let round_edge_frame = match support_planes.zip(round_edge_envelope) {
            Some((support_planes, envelope)) => {
                let replay = match ctx.get_btree_map(&constant_round_radii, &row.feature_id, "creo constant round radii lookup")?.copied() {
                    Some(radius) => {
                        round_edge_cylinder_frame(ctx, envelope, radius, support_planes)?
                    }
                    None => None,
                };
                let perpendicular = perpendicular_result
                    .as_ref()
                    .and_then(|result| result.as_ref().ok())
                    .copied();
                match (replay, perpendicular) {
                    (Some(replay), Some(perpendicular))
                        if crate::surface::cylinder_frame_readers::positional_cylinder_frames_agree(
                            replay,
                            perpendicular,
                        ) => Some(replay),
                    (Some(frame), None) | (None, Some(frame)) => Some(frame),
                    _ => None,
                }
            }
            None => None,
        };
        if round_edge_envelope.is_some() {
            if support_planes.is_none_or(|planes| planes.len() < 2) {
                summary.round_edge_missing_support_planes += 1;
            } else if round_edge_frame.is_some() {
                summary.round_edge_solved_carriers += 1;
            } else {
                summary.round_edge_unsolved_carriers += 1;
                if let Some(Err(failure)) = perpendicular_result {
                    match failure {
                        PerpendicularRoundEdgeFailure::NoPerpendicularSupportPair => {
                            summary.round_edge_no_perpendicular_support_pair += 1;
                        }
                        PerpendicularRoundEdgeFailure::EndpointIncidenceMismatch => {
                            summary.round_edge_endpoint_incidence_mismatch += 1;
                        }
                        PerpendicularRoundEdgeFailure::RadiusProjectionMismatch => {
                            summary.round_edge_radius_projection_mismatch += 1;
                        }
                        PerpendicularRoundEdgeFailure::NonuniqueRadius => {
                            summary.round_edge_nonunique_radius += 1;
                        }
                        PerpendicularRoundEdgeFailure::CarrierValidationFailure => {
                            summary.round_edge_carrier_validation_failure += 1;
                        }
                    }
                } else {
                    summary.round_edge_replay_conflict += 1;
                }
            }
        }
        // Section-cut rows can use the same type-24 selector and scalar-frame
        // shape as a repeated-diameter round. The row body alone does not
        // establish a cylinder carrier in that feature context; section
        // geometry transfer owns the interpretation.
        // The same type-24 shape is only a neutral cylinder for class 913
        // when its complete generated set proves one constant radius or this
        // row has an independent cap/support-envelope cylinder proof.
        if row.kind == crate::surface::SurfaceKind::Cylinder
            && !inline_non_plane
            && !selector_corner_interval
            && (matches!(feature_class, Some(SchemaClass::Cut))
                || matches!(feature_class, Some(SchemaClass::Round))
                    && !ctx.contains_key_btree_map(&constant_round_radii, &row.feature_id, "creo constant round radii lookup")?
                    && round_support_frame.is_none()
                    && round_edge_frame.is_none()
                    && support_tangent_frame.is_none()
                    && axial_interval_corner_frame.is_none())
        {
            continue;
        }
        let reference_bound_frame = || -> Result<
            Option<(crate::surface::PositionalCylinderFrame, CylinderFrameMechanism)>,
            cadmpeg_core::CodecError,
        > {
            let mut reference_storage = ctx.reserve_scoped(0, "creo reference cylinder workspace")?;
            let mut entity_ids = BTreeSet::new();
            for table in ctx
                .admit_iter(&scan.features.entity_tables, "creo reference cylinder entity tables")?
                .filter(|table| table.feature_id == row.feature_id)
            {
                for entry in ctx.admit_iter(&table.entries, "creo reference cylinder entity entries")? {
                    reference_storage.with_storage(|| ctx.insert_btree_set(
                        &mut entity_ids,
                        entry.entity_id,
                        "creo reference cylinder entity ID nodes",
                    ))?;
                }
            }
            let mut circles = Vec::new();
            for circle in ctx
                .admit_iter(&scan.references.circles, "creo reference cylinder circles")?
            {
                if !ctx.contains_btree_set(&entity_ids, &circle.entity_id, "creo entity ids lookup")? { continue; }
                reference_storage.with_storage(|| ctx.reserve_vec(&mut circles, 1, "creo reference cylinder circles"))?;
                circles.push(circle);
            }
            let generated_cylinder_count = ctx
                .admit_iter(&*scan.surfaces.rows, "creo generated cylinder rows")?
                .filter(|candidate| {
                    candidate.feature_id == row.feature_id
                        && candidate.kind == crate::surface::SurfaceKind::Cylinder
                })
                .count();
            if generated_cylinder_count == 1 {
                if let Some(frame) = reference_circle_pair_cylinder_frame(&circles) {
                    return Ok(Some((frame, CylinderFrameMechanism::ReferenceCirclePair)));
                }
            }
            let Some(envelope) = record.type24_scalar_frame_round_envelope() else {
                return Ok(None);
            };
            Ok(reference_cap_bound_round_frame(ctx, envelope, &circles)?
                .map(|frame| (frame, CylinderFrameMechanism::RoundReferenceCap)))
        };
        let (frame, mechanism) = if selector_corner_interval {
            let Some(frame) = record.positional_cylinder_frame() else {
                continue;
            };
            (frame, CylinderFrameMechanism::SelectorCornerInterval)
        } else if let Some(frame) = round_edge_frame {
            (frame, CylinderFrameMechanism::RoundEdgeEndpoint)
        } else if let Some(frame) = support_tangent_frame {
            (frame, CylinderFrameMechanism::SupportTangent)
        } else if inline_non_plane {
            let Some(frame) = record.positional_cylinder_frame() else {
                continue;
            };
            (frame, CylinderFrameMechanism::InlinePositionalSurfaceRow)
        } else if let Some(frame) = round_support_frame {
            (frame, CylinderFrameMechanism::RoundSupportEnvelope)
        } else if let Some(frame) = axial_interval_corner_frame {
            (frame, CylinderFrameMechanism::AxialIntervalCorner)
        } else if let Some(frame) = record.positional_cylinder_frame() {
            (frame, CylinderFrameMechanism::PositionalCylinderFrame)
        } else {
            let Some(frame) = reference_bound_frame()? else {
                continue;
            };
            frame
        };
        let stored_frame_agrees = record.positional_cylinder_frame().is_some_and(|stored| {
            crate::surface::cylinder_frame_readers::positional_cylinder_frames_agree(stored, frame)
        });
        let witnessed_frame_replaces_stored = mechanism.replaces_stored_frame();
        let row_local_frame_selected = (stored_frame_agrees || witnessed_frame_replaces_stored)
            && (feature_class != Some(SchemaClass::Round) || mechanism.row_local_under_round());
        if feature_class == Some(SchemaClass::Hole)
            && counterbore_dimensions(ctx, scan, ir, row.feature_id)?.is_some_and(|dimensions| {
                !counterbore_dimension_tuple_matches_radius(dimensions, frame.radius().get())
            })
        {
            continue;
        }
        // The repair arm below and the transfer arm after it state the same
        // carrier from the same frame.
        let cylinder_surface = cadmpeg_ir::geometry::analytic::CylinderSurface::new(
            frame.frame().finite_origin(),
            frame.frame().orthonormal_frame(),
            frame.radius(),
        );
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            record.surface_id,
            "creo positional cylinder identity",
        )?;
        let existing = surfaces_index.lookup(ctx, &ir.model.surfaces,
            |record| record.id.as_str(), id.as_str())?;
        if let Some(position) = existing {
            if let Some(surface) = position.filter(|_| row_local_frame_selected)
                .and_then(|index| ir.model.surfaces.get_mut(index)) {
                source_carriers.replace_surface_geometry(ctx, surface,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)))?;
                annotate(ctx, annotations, &id, "VisibGeom",
                    cadmpeg_core::decode::u64_from_index(row.offset),
                    "positional_cylinder_frame_reconciled", Exactness::Derived)?;
            }
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(row.offset),
            mechanism.label(),
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cylinder_surface,
                )),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("VisibGeom:{}", record.surface_id),
                            "creo positional cylinder source IDs",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        summary.transferred += 1;
        summary.round_edge_transferred_carriers +=
            usize::from(mechanism == CylinderFrameMechanism::RoundEdgeEndpoint);
    }
    Ok(summary)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CylinderFrameMechanism {
    ReferenceCirclePair,
    RoundReferenceCap,
    SelectorCornerInterval,
    RoundEdgeEndpoint,
    SupportTangent,
    InlinePositionalSurfaceRow,
    RoundSupportEnvelope,
    AxialIntervalCorner,
    PositionalCylinderFrame,
}

impl CylinderFrameMechanism {
    const fn label(self) -> &'static str {
        match self {
            Self::ReferenceCirclePair => "reference_circle_pair_cylinder_frame",
            Self::RoundReferenceCap => "round_reference_cap_cylinder_frame",
            Self::SelectorCornerInterval => "selector_corner_interval_cylinder",
            Self::RoundEdgeEndpoint => "round_edge_endpoint_cylinder",
            Self::SupportTangent => "support_tangent_cylinder",
            Self::InlinePositionalSurfaceRow => "inline_positional_surface_row",
            Self::RoundSupportEnvelope => "round_support_envelope_cylinder",
            Self::AxialIntervalCorner => "axial_interval_corner_cylinder",
            Self::PositionalCylinderFrame => "positional_cylinder_frame",
        }
    }
    const fn replaces_stored_frame(self) -> bool {
        matches!(self, Self::RoundEdgeEndpoint | Self::SupportTangent)
    }
    const fn row_local_under_round(self) -> bool {
        matches!(
            self,
            Self::InlinePositionalSurfaceRow
                | Self::SelectorCornerInterval
                | Self::RoundEdgeEndpoint
                | Self::SupportTangent
        )
    }
}

pub(in super::super) fn reference_circle_pair_cylinder_frame(
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let [first, second] = circles else {
        return None;
    };
    (first.center_stored() && second.center_stored()).then_some(())?;
    let radius = first.radius().get();
    let second_radius = second.radius().get();
    let radius_scale = radius.max(second_radius).max(1.0);
    ((second_radius - radius).abs() <= EPS_CYLINDER_GEOMETRY * radius_scale).then_some(())?;
    let first_center: [f64; 3] = first.center().get().into();
    let second_center: [f64; 3] = second.center().get().into();
    let scale = first_center
        .iter()
        .chain(&second_center)
        .map(|value| value.abs())
        .fold(radius_scale, f64::max);
    let first_axis = crate::vecmath::unit_length(first.axis());
    let second_axis = crate::vecmath::unit_length(second.axis());
    ((dot(first_axis, second_axis).abs() - 1.0).abs() <= EPS_CYLINDER_GEOMETRY).then_some(())?;
    let displacement: [f64; 3] =
        std::array::from_fn(|index| second_center[index] - first_center[index]);
    let length = dot(displacement, displacement).sqrt();
    (length.is_finite() && length > EPS_CYLINDER_GEOMETRY * scale).then_some(())?;
    let center_direction = displacement.map(|value| value / length);
    ((dot(center_direction, first_axis).abs() - 1.0).abs() <= EPS_CYLINDER_GEOMETRY
        && (dot(center_direction, second_axis).abs() - 1.0).abs() <= EPS_CYLINDER_GEOMETRY)
        .then_some(())?;
    let validated_radial = |circle: &crate::reference::ReferenceCircle, axis| {
        let start: [f64; 3] = circle.start().get().into();
        let center: [f64; 3] = circle.center().get().into();
        let vector: [f64; 3] = std::array::from_fn(|index| start[index] - center[index]);
        let length = dot(vector, vector).sqrt();
        ((length - radius).abs() <= EPS_CYLINDER_GEOMETRY * radius_scale
            && dot(axis, vector).abs() <= EPS_CYLINDER_GEOMETRY * radius_scale)
            .then_some((vector, length))
    };
    let (radial, radial_length) = validated_radial(first, first_axis)?;
    validated_radial(second, second_axis)?;
    crate::surface::PositionalCylinderFrame::new(
        first_center,
        first_axis,
        radial.map(|value| value / radial_length),
        radius,
        Some(length),
    )
}

pub(in super::super) fn reference_cap_bound_round_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    envelope: crate::surface::Type24RoundEnvelope,
    circles: &[&crate::reference::ReferenceCircle],
) -> Result<Option<crate::surface::PositionalCylinderFrame>, cadmpeg_core::CodecError> {
    let [_, _] = circles else {
        return Ok(None);
    };
    let [first, second] = envelope.extent_endpoints;
    let scale = first
        .iter()
        .chain(&second)
        .copied()
        .map(f64::abs)
        .fold(envelope.diameter.max(1.0), f64::max);
    let tolerance = EPS_CYLINDER_GEOMETRY * scale;
    let point_matches = |actual: [f64; 3], expected: [f64; 3]| {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= tolerance)
    };
    let mut candidate = None;
    for axis_index in 0..3 {
        let mut radial = (0..3).filter(|index| *index != axis_index);
        let (Some(first_radial), Some(second_radial)) = (radial.next(), radial.next()) else {
            return Ok(None);
        };
        let radial_indices = [first_radial, second_radial];
        if radial_indices.iter().any(|index| {
            ((second[*index] - first[*index]).abs() - envelope.diameter).abs() > tolerance
        }) || (second[axis_index] - first[axis_index]).abs() <= tolerance
        {
            continue;
        }
        let cap_pair = |coordinate: f64, crossed: bool| -> Result<bool, cadmpeg_core::CodecError> {
            let mut first_corner = first;
            let mut second_corner = second;
            first_corner[axis_index] = coordinate;
            second_corner[axis_index] = coordinate;
            if crossed {
                first_corner[radial_indices[1]] = second[radial_indices[1]];
                second_corner[radial_indices[1]] = first[radial_indices[1]];
            }
            for circle in ctx.admit_iter(circles, "creo round reference cap circles")? {
                if <[f64; 3]>::from(*circle.axis().as_raw())
                    .iter()
                    .enumerate()
                    .all(|(index, component)| {
                        if index == axis_index {
                            (component.abs() - 1.0).abs() <= EPS_CYLINDER_GEOMETRY
                        } else {
                            component.abs() <= EPS_CYLINDER_GEOMETRY
                        }
                    })
                    && ((point_matches(circle.start().get().into(), first_corner)
                        && point_matches(circle.end().get().into(), second_corner))
                        || (point_matches(circle.end().get().into(), first_corner)
                            && point_matches(circle.start().get().into(), second_corner)))
                {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        let mut matching_cap_pair = false;
        for crossed in [false, true] {
            if cap_pair(first[axis_index], crossed)? && cap_pair(second[axis_index], crossed)? {
                matching_cap_pair = true;
                break;
            }
        }
        if !matching_cap_pair {
            continue;
        }
        let mut origin = first;
        for index in &radial_indices {
            origin[*index] = first[*index].midpoint(second[*index]);
        }
        let mut axis = [0.0; 3];
        axis[axis_index] = (second[axis_index] - first[axis_index]).signum();
        let mut ref_direction = [0.0; 3];
        let reference_index = radial_indices[0];
        ref_direction[reference_index] =
            (second[reference_index] - first[reference_index]).signum();
        let Some(frame) = crate::surface::PositionalCylinderFrame::new(
            origin,
            axis,
            ref_direction,
            envelope.diameter / 2.0,
            Some((second[axis_index] - first[axis_index]).abs()),
        ) else {
            return Ok(None);
        };
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some(frame);
    }
    Ok(candidate)
}

pub(in super::super) fn transfer_positional_cones(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut transferred = 0;
    for record in ctx.admit_iter(
        &*scan.surfaces.parameters,
        "creo transfer positional cones parameters traversal",
    )? {
        let Some(frame) = record.positional_cone_frame() else {
            continue;
        };
        let unique_parameter =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id);
        if unique_parameter.is_none_or(|unique| !std::ptr::eq(unique, record)) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cone)
        else {
            continue;
        };
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            record.surface_id,
            "creo positional cone identity",
        )?;
        let surface_exists = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if surface_exists {
            continue;
        }
        let cone_surface = super::apex_cone(frame.frame(), frame.half_angle());
        annotate(
            ctx,
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(row.offset),
            "positional_cone_frame",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("VisibGeom:{}", record.surface_id),
                            "creo positional cone source IDs",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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

pub(in super::super) fn transfer_circular_sweep_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut sweep_feature_ids = BTreeSet::new();
    for row in ctx.admit_iter(&scan.features.rows, "creo circular sweep feature rows")? {
        if row.root_schema_class == Some(SchemaClass::Protrusion)
            && !feature_section_sweep_semantics_conflict(ctx, scan, row.feature_id)?
            && section_sweep_allows_linear_extrusion(
                Some(SchemaClass::Protrusion),
                feature_recipe(scan, row.feature_id),
            )
        {
            local_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut sweep_feature_ids,
                    row.feature_id,
                    "creo circular sweep feature ID nodes",
                )
            })?;
        }
    }
    let mut transferred = 0;
    for feature_id in ctx.admit_iter(&sweep_feature_ids, "creo circular sweep feature IDs")? {
        let mut sweep_storage = ctx.reserve_scoped(0, "creo circular sweep workspace")?;
        let Some(sweep) = sweep_storage.with_storage(|| circular_sweep_geometry(ctx, scan, *feature_id))? else {
            continue;
        };
        for row in ctx.admit_iter(
            &sweep.cylinder_rows,
            "creo transfer circular sweep cylinders cylinder rows traversal",
        )? {
            let cylinder_id = row.id;
            let id = crate::identity::compose_checked::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                cylinder_id,
                "creo circular sweep cylinder identity",
            )?;
            let surface_exists = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
            if surface_exists {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "AllFeatur",
                cadmpeg_core::decode::u64_from_index(row.offset),
                "circular_sweep_cap_outline_cylinder",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                        sweep.geometry,
                    )),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.format_retained(
                                format_args!("VisibGeom:{cylinder_id}"),
                                "creo circular sweep cylinder source IDs",
                            )?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "source object_id must not be empty",
                            )
                        })?,
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

pub(in super::super) fn transfer_cross_section_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut transferred = 0;
    for frame in ctx.admit_iter(
        &scan.planes.cross_section_local_systems,
        "creo transfer cross section planes cross section local systems traversal",
    )? {
        let decoded_frame = frame.frame();
        let (Some(origin), Some(normal), Some(u_axis)) = (
            decoded_frame.origin,
            decoded_frame.normal(),
            decoded_frame.u_axis(),
        ) else {
            continue;
        };
        if is_axis_aligned(ctx, normal)? {
            continue;
        }
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::CROSS_SECTION_GEOMETRY_SURFACE,
            frame.surface_id,
            "creo cross-section local-system plane identity",
        )?;
        let surface_exists = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if surface_exists {
            continue;
        }
        let Ok(plane_surface) = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::from(origin),
            Vector3::from(normal),
            Vector3::from(u_axis),
        ) else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            &id,
            "Xsections",
            cadmpeg_core::decode::u64_from_index(frame.offset),
            "cross_section_plane_local_system",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("Xsections:{}", frame.surface_id),
                            "creo cross-section local-system plane source IDs",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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
    for plane in ctx.admit_iter(
        &scan.planes.cross_section_outlines,
        "creo transfer cross section planes cross section outlines traversal",
    )? {
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::CROSS_SECTION_GEOMETRY_SURFACE,
            plane.surface_id,
            "creo cross-section outline plane identity",
        )?;
        let surface_exists = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), id.as_str())?.is_some();
        if surface_exists {
            continue;
        }
        let Ok(plane_surface) = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::from(plane.origin),
            Vector3::from(plane.normal),
            Vector3::from(plane.u_axis),
        ) else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            &id,
            "Xsections",
            cadmpeg_core::decode::u64_from_index(plane.offset),
            "cross_section_plane_outline_held_coordinate",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("Xsections:{}", plane.surface_id),
                            "creo cross-section outline plane source IDs",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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
