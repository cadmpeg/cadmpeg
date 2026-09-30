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
use super::super::uniqueness::exactly_one;
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
    rows: &[crate::surface::SurfaceRow],
) -> Result<Vec<(u32, u32, usize)>, cadmpeg_core::CodecError> {
    let mut pairs = Vec::new();
    for pair in tables.iter().filter_map(|table| {
        let feature_id = table.feature_id;
        round_feature_ids.contains(&feature_id).then_some(())?;
        let [first, second, rowless, cylinder] = table.entries.as_slice() else {
            return None;
        };
        crate::surface::unique_surface_row(rows, first.entity_id)
            .is_some()
            .then_some(())?;
        crate::surface::unique_surface_row(rows, second.entity_id)
            .is_some()
            .then_some(())?;
        (!rows.iter().any(|row| row.id == rowless.entity_id)).then_some(())?;
        crate::surface::unique_surface_row(rows, cylinder.entity_id)
            .is_some_and(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
            })
            .then_some(())?;
        Some((rowless.entity_id, cylinder.entity_id, table.offset))
    }) {
        ctx.reserve_vec(&mut pairs, 1, "creo rowless round cylinder pairs")?;
        pairs.push(pair);
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
    let mut transferred = 0;
    for datum in &scan.planes.datum_cylinders {
        let id = super::native_surface_id(ctx, scan, datum.id)?;
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
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
            datum.offset_in_payload as u64,
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
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("ActDatums:{}", datum.id),
                        "creo active datum cylinder source IDs",
                    )?)
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
    let mut round_feature_ids = BTreeSet::new();
    for row in scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(SchemaClass::Round))
    {
        if !round_feature_ids.contains(&row.feature_id) {
            ctx.charge_collection_items(1, "creo constrained round feature ID nodes")?;
            round_feature_ids.insert(row.feature_id);
        }
    }
    let mut transferred = 0;
    for feature_id in round_feature_ids {
        let named = agreed_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::rows::AffectedIdKind::Geometry,
        );
        let named_present = has_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::rows::AffectedIdKind::Geometry,
        );
        let replay =
            agreed_feature_replay_geometry_ids(&scan.features.replay_affected_ids, feature_id);
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
        let local_planes = placed_planes(ctx, scan)?;
        let mut planes = Vec::new();
        for id in affected {
            let Some(plane) = reconciled_model_plane(&local_planes, ir, source_carriers, *id)
            else {
                break;
            };
            ctx.reserve_vec(&mut planes, 1, "creo constrained slot plane rows")?;
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
        let Some(row) =
            crate::decode::uniqueness::exactly_one(scan.surfaces.rows.iter().filter(|row| {
                row.feature_id == feature_id
                    && row.kind == crate::surface::SurfaceKind::Cylinder
                    && !ir.model.surfaces.iter().any(|surface| {
                        crate::identity::matches_numbered_identity(
                            surface.id.as_str(),
                            "creo:visibgeom:surface#",
                            row.id,
                        )
                    })
            }))
        else {
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
            row.offset as u64,
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
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("AllFeatur:{}:{}", feature_id, row.id),
                        "creo constrained slot cylinder source IDs",
                    )?)
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
    let mut round_feature_ids = BTreeSet::new();
    for row in scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(SchemaClass::Round))
    {
        if !round_feature_ids.contains(&row.feature_id) {
            ctx.charge_collection_items(1, "creo rowless round feature ID nodes")?;
            round_feature_ids.insert(row.feature_id);
        }
    }
    let mut transferred = 0;
    for (rowless_id, sibling_id, offset) in rowless_round_cylinder_pairs(
        ctx,
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    )? {
        let Some(cylinder_surface) = exactly_one(ir.model.surfaces.iter().filter(|surface| {
            crate::identity::matches_numbered_identity(
                surface.id.as_str(),
                "creo:visibgeom:surface#",
                sibling_id,
            )
        }))
        .and_then(|surface| match source_carriers.surface_geometry(surface) {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) => Some(*cylinder),
            _ => None,
        }) else {
            continue;
        };
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            rowless_id,
            "creo rowless round cylinder identity",
        )?;
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            "AllFeatur",
            offset as u64,
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
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("AllFeatur:{rowless_id}"),
                        "creo rowless round cylinder source IDs",
                    )?)
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
    let mut hole_feature_ids = BTreeSet::new();
    for feature_id in scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(SchemaClass::Hole))
        .map(|row| row.feature_id)
    {
        if !hole_feature_ids.contains(&feature_id) {
            ctx.charge_collection_items(1, "creo hole cylinder feature ID nodes")?;
            hole_feature_ids.insert(feature_id);
        }
    }
    let mut transferred = 0;
    for feature_id in hole_feature_ids {
        let simple = simple_hole_geometry(ctx, scan, feature_id)?;
        let counterbore = if simple.is_some() {
            None
        } else {
            counterbore_patch_geometries(ctx, scan, ir, feature_id)?
        };
        let simple_rows = simple.into_iter().flat_map(|hole| {
            hole.cylinder_rows
                .into_iter()
                .map(move |row| (row, hole.geometry))
        });
        for (row, geometry) in simple_rows.chain(counterbore.into_iter().flatten()) {
            let cylinder_id = row.id;
            let id = crate::identity::compose_checked::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                cylinder_id,
                "creo hole cylinder identity",
            )?;
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "hole_cap_outline_cylinder",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(geometry)),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                            format_args!("VisibGeom:{cylinder_id}"),
                            "creo hole cylinder source IDs",
                        )?)
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

pub(in super::super) fn transfer_split_outline_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut rows = BTreeMap::new();
    for row in
        crate::identity::uniquely_identified_rows_checked(ctx, &scan.surfaces.rows, |row| row.id)?
    {
        ctx.charge_collection_items(1, "creo split cylinder row nodes")?;
        rows.insert(row.id, row);
    }
    let local_planes = placed_planes(ctx, scan)?;
    let mut cylinders_by_plane = BTreeMap::<(u32, u32), BTreeSet<u32>>::new();
    for edge in
        crate::identity::uniquely_identified_rows_checked(ctx, &scan.curves.topology_rows, |row| {
            row.id
        })?
    {
        if edge.type_byte != 0 {
            continue;
        }
        let [Some(left), Some(right)] = edge.faces else {
            continue;
        };
        let (left, right) = (left.get(), right.get());
        let pair = match (rows.get(&left), rows.get(&right)) {
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
            let cylinder_ids = match cylinders_by_plane.entry(plane_and_feature) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "creo split cylinder plane nodes")?;
                    entry.insert(BTreeSet::new())
                }
            };
            if !cylinder_ids.contains(&cylinder) {
                ctx.charge_collection_items(1, "creo split cylinder ID nodes")?;
                cylinder_ids.insert(cylinder);
            }
        }
    }

    let mut transferred = 0;
    for ((plane_id, _), cylinder_ids) in cylinders_by_plane {
        let mut cylinder_ids = cylinder_ids.into_iter();
        let (Some(first_id), Some(second_id), None) = (
            cylinder_ids.next(),
            cylinder_ids.next(),
            cylinder_ids.next(),
        ) else {
            continue;
        };
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
        let Some(plane) = reconciled_model_plane(&local_planes, ir, source_carriers, plane_id)
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
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            let row = rows[&cylinder_id];
            annotate(
                ctx,
                annotations,
                &id,
                "VisibGeom",
                row.offset as u64,
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
                        object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                            format_args!("VisibGeom:{cylinder_id}"),
                            "creo split cylinder source object IDs",
                        )?)
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
    envelope: crate::surface::Type24RoundEdgeEnvelope,
    radius: f64,
    support_planes: &[PlaneEquation],
) -> Option<crate::surface::PositionalCylinderFrame> {
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let [first, second] = envelope.vertices;
    if !first.into_iter().chain(second).all(f64::is_finite) {
        return None;
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
    for (first_index, first_support) in support_planes.iter().copied().enumerate() {
        let Some(first_normal) = normalize(first_support.normal) else {
            continue;
        };
        let first_support = PlaneEquation {
            origin: first_support.origin,
            normal: first_normal,
        };
        for second_support in support_planes.iter().copied().skip(first_index + 1) {
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
                            return None;
                        }
                        candidate = Some(frame);
                    }
                }
            }
        }
    }
    candidate
}

fn unique_tangent_axial_interval_corner_frame(
    candidates: &[crate::surface::PositionalCylinderFrame],
    support_planes: &[PlaneEquation],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let mut best = None;
    let mut maximum = 0;
    let mut tied = false;
    for candidate in candidates.iter().copied() {
        let axis = unit_length(*candidate.frame().orthonormal_frame().axis());
        let score = support_planes
            .iter()
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
    (!tied).then_some(best?)
}

fn unique_support_tangent_cylinder_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    stored: crate::surface::PositionalCylinderFrame,
    support_planes: &[PlaneEquation],
) -> Result<Option<crate::surface::PositionalCylinderFrame>, cadmpeg_core::CodecError> {
    let axis = unit_length(*stored.frame().orthonormal_frame().axis());
    let mut origins = Vec::new();
    ctx.reserve_vec(&mut origins, 1, "creo support tangent initial origins")?;
    origins.push(stored.frame().origin());
    let mut witnessed_axis = [false; 3];
    let mut witnessed_planes = Vec::new();
    for plane in support_planes {
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
        ctx.reserve_vec(
            &mut witnessed_planes,
            1,
            "creo support tangent witness planes",
        )?;
        witnessed_planes.push(PlaneEquation {
            origin: plane.origin,
            normal,
        });
        let mut next = Vec::new();
        for origin in &origins {
            for coordinate in candidates() {
                let mut candidate = *origin;
                candidate[axis_index] = coordinate;
                if !next.iter().any(|known: &[f64; 3]| {
                    known.iter().zip(candidate).all(|(left, right)| {
                        (left - right).abs()
                            <= EPS_CYLINDER_POSITION * left.abs().max(right.abs()).max(1.0)
                    })
                }) {
                    ctx.reserve_vec(&mut next, 1, "creo support tangent next origins")?;
                    next.push(candidate);
                }
            }
        }
        origins = next;
    }
    if !witnessed_axis.into_iter().any(|witnessed| witnessed) {
        return Ok(None);
    }
    let mut frame = None;
    for origin in origins {
        let tangent_to_all = witnessed_planes.iter().all(|plane| {
            let normal = plane.normal;
            let distance = (dot(normal, origin) - dot(normal, plane.origin)).abs();
            let scale = distance.max(stored.radius().get()).max(1.0);
            (distance - stored.radius().get()).abs() <= EPS_CYLINDER_POSITION * scale
        });
        if !tangent_to_all {
            continue;
        }
        let Some(candidate) = crate::surface::PositionalCylinderFrame::with_admitted_dimensions(
            origin,
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
    Ok(frame)
}

fn perpendicular_round_edge_cylinder_frame(
    envelope: crate::surface::Type24RoundEdgeEnvelope,
    support_planes: &[PlaneEquation],
) -> Result<crate::surface::PositionalCylinderFrame, PerpendicularRoundEdgeFailure> {
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
    for (first_index, first_support) in support_planes.iter().copied().enumerate() {
        let Some(first_normal) = normalize(first_support.normal) else {
            continue;
        };
        let first_support = PlaneEquation {
            origin: first_support.origin,
            normal: first_normal,
        };
        for second_support in support_planes.iter().copied().skip(first_index + 1) {
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
                        return Err(PerpendicularRoundEdgeFailure::NonuniqueRadius);
                    }
                } else {
                    radius = Some(first_radius);
                }
            }
        }
    }
    let Some(radius) = radius else {
        return Err(if !has_perpendicular_support_pair {
            PerpendicularRoundEdgeFailure::NoPerpendicularSupportPair
        } else if !has_endpoint_incidence {
            PerpendicularRoundEdgeFailure::EndpointIncidenceMismatch
        } else if !has_equal_radius_projections {
            PerpendicularRoundEdgeFailure::RadiusProjectionMismatch
        } else {
            PerpendicularRoundEdgeFailure::NonuniqueRadius
        });
    };
    round_edge_cylinder_frame(envelope, radius, support_planes)
        .ok_or(PerpendicularRoundEdgeFailure::CarrierValidationFailure)
}

pub(in super::super) fn transfer_positional_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<PositionalCylinderTransferSummary, cadmpeg_core::CodecError> {
    let mut round_feature_ids = BTreeSet::new();
    for row in scan.surfaces.rows.iter().filter(|row| {
        row.kind == crate::surface::SurfaceKind::Cylinder
            && feature_schema_class(scan, row.feature_id) == Some(SchemaClass::Round)
    }) {
        if !round_feature_ids.contains(&row.feature_id) {
            ctx.charge_collection_items(1, "creo positional round feature ID nodes")?;
            round_feature_ids.insert(row.feature_id);
        }
    }
    let mut constant_round_radii = BTreeMap::new();
    for feature_id in round_feature_ids {
        if let Some(radius) = round_constant_radius(ctx, scan, ir, source_carriers, feature_id)? {
            ctx.charge_collection_items(1, "creo constant round radius nodes")?;
            constant_round_radii.insert(feature_id, radius);
        }
    }
    let local_planes = placed_planes(ctx, scan)?;
    let mut unique_rows = BTreeMap::new();
    for row in
        crate::identity::uniquely_identified_rows_checked(ctx, &scan.surfaces.rows, |row| row.id)?
    {
        ctx.charge_collection_items(1, "creo positional cylinder row nodes")?;
        unique_rows.insert(row.id, row);
    }
    let mut adjacent_plane_ids = BTreeMap::<u32, BTreeSet<u32>>::new();
    for edge in
        crate::identity::uniquely_identified_rows_checked(ctx, &scan.curves.topology_rows, |row| {
            row.id
        })?
    {
        let [Some(left), Some(right)] = edge.faces else {
            continue;
        };
        let (left, right) = (left.get(), right.get());
        for (surface_id, other_id) in [(left, right), (right, left)] {
            if unique_rows
                .get(&surface_id)
                .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
                && unique_rows
                    .get(&other_id)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            {
                let plane_ids = match adjacent_plane_ids.entry(surface_id) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        ctx.charge_collection_items(1, "creo positional adjacent cylinder nodes")?;
                        entry.insert(BTreeSet::new())
                    }
                };
                if !plane_ids.contains(&other_id) {
                    ctx.charge_collection_items(1, "creo positional adjacent plane ID nodes")?;
                    plane_ids.insert(other_id);
                }
            }
        }
    }
    let mut round_edge_support_planes = BTreeMap::new();
    for (surface_id, plane_ids) in adjacent_plane_ids {
        let mut planes = Vec::new();
        for plane_id in plane_ids {
            if let Some(plane) =
                reconciled_model_plane(&local_planes, ir, source_carriers, plane_id)
            {
                ctx.reserve_vec(&mut planes, 1, "creo positional support planes")?;
                planes.push(plane);
            }
        }
        ctx.charge_collection_items(1, "creo positional support plane nodes")?;
        round_edge_support_planes.insert(surface_id, planes);
    }
    let mut summary = PositionalCylinderTransferSummary::default();
    for record in &scan.surfaces.parameters {
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            != Some(record)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        else {
            continue;
        };
        let feature_class = feature_schema_class(scan, row.feature_id);
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
        let support_planes = round_edge_support_planes.get(&row.id);
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
        let axial_interval_corner_frame = support_planes.and_then(|planes| {
            axial_interval_corner_candidates
                .as_ref()
                .and_then(|candidates| {
                    unique_tangent_axial_interval_corner_frame(candidates, planes)
                })
        });
        if axial_interval_corner_frame.is_some() {
            summary.axial_interval_corner_solved_carriers += 1;
        }
        let perpendicular_result =
            support_planes
                .zip(round_edge_envelope)
                .map(|(support_planes, envelope)| {
                    perpendicular_round_edge_cylinder_frame(envelope, support_planes)
                });
        let round_edge_frame = support_planes.and_then(|support_planes| {
            round_edge_envelope.and_then(|envelope| {
                let replay = constant_round_radii
                    .get(&row.feature_id)
                    .copied()
                    .and_then(|radius| round_edge_cylinder_frame(envelope, radius, support_planes));
                let perpendicular = perpendicular_result.as_ref()?.as_ref().ok().copied();
                match (replay, perpendicular) {
                    (Some(replay), Some(perpendicular))
    if crate::surface::cylinder_frame_readers::positional_cylinder_frames_agree(
                            replay,
                            perpendicular,
                        ) =>
                    {
                        Some(replay)
                    }
                    (Some(frame), None) | (None, Some(frame)) => Some(frame),
                    _ => None,
                }
            })
        });
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
                    && !constant_round_radii.contains_key(&row.feature_id)
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
            let mut entity_ids = BTreeSet::new();
            for entity_id in scan
                .features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == row.feature_id)
                .flat_map(|table| table.entries.iter().map(|entry| entry.entity_id))
            {
                if !entity_ids.contains(&entity_id) {
                    ctx.charge_collection_items(1, "creo reference cylinder entity ID nodes")?;
                    entity_ids.insert(entity_id);
                }
            }
            let mut circles = Vec::new();
            for circle in scan
                .references
                .circles
                .iter()
                .filter(|circle| entity_ids.contains(&circle.entity_id))
            {
                ctx.reserve_vec(&mut circles, 1, "creo reference cylinder circles")?;
                circles.push(circle);
            }
            let generated_cylinder_count = scan
                .surfaces
                .rows
                .iter()
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
            Ok(reference_cap_bound_round_frame(envelope, &circles)
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
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            if row_local_frame_selected
                && ir
                    .model
                    .surfaces
                    .iter()
                    .filter(|surface| surface.id == id)
                    .count()
                    == 1
            {
                if let Some(surface) = ir
                    .model
                    .surfaces
                    .iter_mut()
                    .find(|surface| surface.id == id)
                {
                    source_carriers.replace_surface_geometry(
                        ctx,
                        surface,
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
                    )?;
                    annotate(
                        ctx,
                        annotations,
                        &id,
                        "VisibGeom",
                        row.offset as u64,
                        "positional_cylinder_frame_reconciled",
                        Exactness::Derived,
                    )?;
                }
            }
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
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
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("VisibGeom:{}", record.surface_id),
                        "creo positional cylinder source IDs",
                    )?)
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
    (first.center_stored && second.center_stored).then_some(())?;
    let radius = first.radius.get();
    let second_radius = second.radius.get();
    let radius_scale = radius.max(second_radius).max(1.0);
    ((second_radius - radius).abs() <= EPS_CYLINDER_GEOMETRY * radius_scale).then_some(())?;
    let first_center: [f64; 3] = first.center.get().into();
    let second_center: [f64; 3] = second.center.get().into();
    let scale = first_center
        .iter()
        .chain(&second_center)
        .map(|value| value.abs())
        .fold(radius_scale, f64::max);
    let first_axis = crate::vecmath::unit_length(first.axis);
    let second_axis = crate::vecmath::unit_length(second.axis);
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
        let start: [f64; 3] = circle.start.get().into();
        let center: [f64; 3] = circle.center.get().into();
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
    envelope: crate::surface::Type24RoundEnvelope,
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let [_, _] = circles else {
        return None;
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
        let radial_indices = [radial.next()?, radial.next()?];
        if radial_indices.iter().any(|index| {
            ((second[*index] - first[*index]).abs() - envelope.diameter).abs() > tolerance
        }) || (second[axis_index] - first[axis_index]).abs() <= tolerance
        {
            continue;
        }
        let cap_pair = |coordinate: f64, crossed: bool| {
            let mut first_corner = first;
            let mut second_corner = second;
            first_corner[axis_index] = coordinate;
            second_corner[axis_index] = coordinate;
            if crossed {
                first_corner[radial_indices[1]] = second[radial_indices[1]];
                second_corner[radial_indices[1]] = first[radial_indices[1]];
            }
            circles.iter().any(|circle| {
                <[f64; 3]>::from(*circle.axis.as_raw())
                    .iter()
                    .enumerate()
                    .all(|(index, component)| {
                        if index == axis_index {
                            (component.abs() - 1.0).abs() <= EPS_CYLINDER_GEOMETRY
                        } else {
                            component.abs() <= EPS_CYLINDER_GEOMETRY
                        }
                    })
                    && ((point_matches(circle.start.get().into(), first_corner)
                        && point_matches(circle.end.get().into(), second_corner))
                        || (point_matches(circle.end.get().into(), first_corner)
                            && point_matches(circle.start.get().into(), second_corner)))
            })
        };
        if ![false, true].into_iter().any(|crossed| {
            cap_pair(first[axis_index], crossed) && cap_pair(second[axis_index], crossed)
        }) {
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
        let frame = crate::surface::PositionalCylinderFrame::new(
            origin,
            axis,
            ref_direction,
            envelope.diameter / 2.0,
            Some((second[axis_index] - first[axis_index]).abs()),
        )?;
        if candidate.is_some() {
            return None;
        }
        candidate = Some(frame);
    }
    candidate
}

pub(in super::super) fn transfer_positional_cones(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        let Some(frame) = record.positional_cone_frame() else {
            continue;
        };
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            != Some(record)
        {
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
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let cone_surface = super::apex_cone(frame.frame(), frame.half_angle());
        annotate(
            ctx,
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
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
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("VisibGeom:{}", record.surface_id),
                        "creo positional cone source IDs",
                    )?)
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
    let mut sweep_feature_ids = BTreeSet::new();
    for feature_id in scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.root_schema_class == Some(SchemaClass::Protrusion)
                && !feature_section_sweep_semantics_conflict(scan, row.feature_id)
                && section_sweep_allows_linear_extrusion(
                    Some(SchemaClass::Protrusion),
                    feature_recipe(scan, row.feature_id),
                )
        })
        .map(|row| row.feature_id)
    {
        if !sweep_feature_ids.contains(&feature_id) {
            ctx.charge_collection_items(1, "creo circular sweep feature ID nodes")?;
            sweep_feature_ids.insert(feature_id);
        }
    }
    let mut transferred = 0;
    for feature_id in sweep_feature_ids {
        let Some(sweep) = circular_sweep_geometry(ctx, scan, feature_id)? else {
            continue;
        };
        for row in &sweep.cylinder_rows {
            let cylinder_id = row.id;
            let id = crate::identity::compose_checked::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                cylinder_id,
                "creo circular sweep cylinder identity",
            )?;
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
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
                        object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                            format_args!("VisibGeom:{cylinder_id}"),
                            "creo circular sweep cylinder source IDs",
                        )?)
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
    let mut transferred = 0;
    for frame in &scan.planes.cross_section_local_systems {
        let decoded_frame = frame.frame();
        let (Some(origin), Some(normal), Some(u_axis)) = (
            decoded_frame.origin,
            decoded_frame.normal(),
            decoded_frame.u_axis(),
        ) else {
            continue;
        };
        if is_axis_aligned(normal) {
            continue;
        }
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::CROSS_SECTION_GEOMETRY_SURFACE,
            frame.surface_id,
            "creo cross-section local-system plane identity",
        )?;
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
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
            frame.offset as u64,
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
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("Xsections:{}", frame.surface_id),
                        "creo cross-section local-system plane source IDs",
                    )?)
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
    for plane in &scan.planes.cross_section_outlines {
        let id = crate::identity::compose_checked::<SurfaceId>(
            ctx,
            &crate::identity::CROSS_SECTION_GEOMETRY_SURFACE,
            plane.surface_id,
            "creo cross-section outline plane identity",
        )?;
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
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
            plane.offset as u64,
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
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                        format_args!("Xsections:{}", plane.surface_id),
                        "creo cross-section outline plane source IDs",
                    )?)
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
