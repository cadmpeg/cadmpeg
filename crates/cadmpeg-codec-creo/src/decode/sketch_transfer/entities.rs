// SPDX-License-Identifier: Apache-2.0
//! Section sketch entity emission and placed section curves.

use super::super::feature_history::link::{
    section_entity_is_generated_profile, section_generated_profile_surface_kinds,
};
use super::super::native::annotate;
use super::super::sketch::geometry::{saved_profile_chains, saved_section_entity_geometry};
use super::super::sketch_ids::{
    sketch_entity_id_admitted, sketch_identity_scope, sketch_native_ref_admitted,
    sketch_point_ref_admitted, typed_sketch_section_curve_id_admitted,
};
use super::super::sweep::nurbs::saved_spline_sketch_geometry;
use super::super::sweep::surfaces::{placed_section_geometry_curve, placed_sketch_curve_ref};
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::identity::{
    opaque_section_segment_identity_suffix_admitted, saved_section_external_id,
    section_segment_identity_suffix_admitted, unresolved_saved_section_entity,
    visit_semantic_saved_section_entities,
};
use crate::decode::sketch_transfer::loci::section_degenerate_axis_line;
use crate::decode::sketch_transfer::profiles::{
    unique_section_incidence_curve_family, SectionEntityIncidenceFamily,
};
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

// Callers pass one or two structural endpoint slots.
fn admitted_endpoint_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    points: &[u32],
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    let mut references = Vec::new();
    for &point in points {
        let reference = sketch_point_ref_admitted(ctx, sketch, point)?;
        ctx.reserve_vec(&mut references, 1, "creo section endpoint references")?;
        references.push(reference);
    }
    Ok(references)
}

fn admitted_optional_endpoint_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    points: &[Option<u32>; 2],
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    let mut references = Vec::new();
    for point in points {
        let Some(point) = *point else {
            continue;
        };
        let reference = sketch_point_ref_admitted(ctx, sketch, point)?;
        ctx.reserve_vec(&mut references, 1, "creo section endpoint references")?;
        references.push(reference);
    }
    Ok(references)
}

fn native_section_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    kind: &str,
) -> Result<SketchGeometry, cadmpeg_core::CodecError> {
    let kind = ctx.copy_retained_text(kind, "creo section native geometry kind")?;
    Ok(SketchGeometry::native(
        cadmpeg_core::text::NonBlankString::for_decode(ctx, kind, "validate nonblank text")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("native_kind must not be empty"))?,
    ))
}

fn copied_or_native_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometries: &BTreeMap<usize, SketchGeometry>,
    offset: usize,
    kind: &str,
) -> Result<SketchGeometry, cadmpeg_core::CodecError> {
    match ctx.get_btree_map(geometries, &offset, "creo section entity geometry lookup")? {
        Some(geometry) => geometry.try_clone_for_decode(ctx, "creo section geometry copy"),
        None => native_section_geometry(ctx, kind),
    }
}

fn section_row_suffix(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    unique: bool,
    external_id: u32,
    family: &str,
    offset: usize,
) -> Result<String, cadmpeg_core::CodecError> {
    if unique {
        ctx.format_retained(format_args!("{external_id}"), "creo section entity suffix")
    } else {
        ctx.format_retained(
            format_args!("{family}:offset:{offset}"),
            "creo section entity suffix",
        )
    }
}

fn push_section_entity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    entity: SketchEntity,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_entities(1, "admit Creo model sketch_entities")?;
    ctx.reserve_vec(entities, 1, "creo section entities")?;
    entities.push(entity);
    Ok(())
}

fn placed_source_object(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    object_id: impl std::fmt::Display,
) -> Result<SourceObjectAssociation, cadmpeg_core::CodecError> {
    let object_id = ctx.format_retained(
        format_args!("{object_id}"),
        "creo placed section source object",
    )?;
    Ok(SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Creo,
        object_id: cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            object_id,
            "validate nonblank text",
        )?
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("source object_id must not be empty"))?,
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    })
}

/// Source sketch, resolved geometry, profiles, and destinations for entity transfer.
pub(super) struct SectionEntityTransfer<'a, 'ctx, 'input> {
    pub scan: &'a ContainerScan<'a>,
    pub ir: &'a mut CadIr,
    pub annotations: &'a mut AnnotationBuilder,
    pub definition: &'a crate::feature::definitions::FeatureDefinition,
    pub transform: Option<&'a crate::placement::FeatureSectionTransform>,
    pub sketch_id: &'a SketchId,
    pub segments: &'a [&'a crate::feature::definitions::FeatureSegment],
    pub unique_segment_ids: &'a BTreeSet<u32>,
    pub unique_saved_ids: &'a BTreeSet<u32>,
    pub ambiguous_segment_ids: &'a BTreeSet<u32>,
    pub complete_segment_table: bool,
    pub solved: &'a BTreeSet<u32>,
    pub segment_geometries: &'a BTreeMap<usize, Option<SketchGeometry>>,
    pub resolved_segment_geometries: &'a BTreeMap<usize, Option<SketchGeometry>>,
    pub circle_geometries: &'a BTreeMap<usize, SketchGeometry>,
    pub point_geometries: &'a BTreeMap<usize, SketchGeometry>,
    pub centered_line_geometries: &'a BTreeMap<usize, SketchGeometry>,
    pub reference_line_geometries: &'a BTreeMap<usize, SketchGeometry>,
    pub materialized_saved_section_external_ids: &'a BTreeSet<u32>,
    pub profiles: Vec<Vec<SketchEntityUse>>,
    pub profile_entities: &'a BTreeSet<SketchEntityId>,
    pub losses: &'a mut Vec<cadmpeg_ir::report::loss::LossNote>,
    pub source_carriers: &'a mut crate::decode::source_carriers::SourceUnitCarriers<'ctx, 'input>,
}

pub(super) fn transfer_section_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    transfer: SectionEntityTransfer<'_, '_, '_>,
) -> Result<(Vec<SketchEntity>, Vec<Vec<SketchEntityUse>>), cadmpeg_core::CodecError> {
    let SectionEntityTransfer {
        scan,
        ir,
        annotations,
        definition,
        transform,
        sketch_id,
        segments,
        unique_segment_ids,
        unique_saved_ids,
        ambiguous_segment_ids,
        complete_segment_table,
        solved,
        segment_geometries,
        resolved_segment_geometries,
        circle_geometries,
        point_geometries,
        centered_line_geometries,
        reference_line_geometries,
        materialized_saved_section_external_ids,
        mut profiles,
        profile_entities,
        losses,
        source_carriers,
    } = transfer;
    let mut family_storage = ctx.reserve_scoped(0, "creo section family row storage")?;
    let mut circle_rows = Vec::new();
    let mut point_rows = Vec::new();
    let mut centered_line_rows = Vec::new();
    let mut reference_line_rows = Vec::new();
    let mut bounded_curve_rows = Vec::new();
    let mut conic_rows = Vec::new();
    let mut opaque_rows = Vec::new();
    if let Some(table) = definition.segments.as_ref() {
        for row in ctx.admit_iter(table.rows.as_slice(), "creo section family source rows")? {
            match row {
                SegmentRow::Ordinary(_) => {}
                SegmentRow::Circle(row) => ctx.push_scoped_vec(
                    &mut family_storage,
                    &mut circle_rows,
                    row,
                    "creo section family row storage",
                )?,
                SegmentRow::Point(row) => ctx.push_scoped_vec(
                    &mut family_storage,
                    &mut point_rows,
                    row,
                    "creo section family row storage",
                )?,
                SegmentRow::CenteredLine(row) => ctx.push_scoped_vec(
                    &mut family_storage,
                    &mut centered_line_rows,
                    row,
                    "creo section family row storage",
                )?,
                SegmentRow::ReferenceLine(row) => ctx.push_scoped_vec(
                    &mut family_storage,
                    &mut reference_line_rows,
                    row,
                    "creo section family row storage",
                )?,
                SegmentRow::BoundedCurve(row) => ctx.push_scoped_vec(
                    &mut family_storage,
                    &mut bounded_curve_rows,
                    row,
                    "creo section family row storage",
                )?,
                SegmentRow::Conic(row) => ctx.push_scoped_vec(
                    &mut family_storage,
                    &mut conic_rows,
                    row,
                    "creo section family row storage",
                )?,
                SegmentRow::Opaque(row) => ctx.push_scoped_vec(
                    &mut family_storage,
                    &mut opaque_rows,
                    row,
                    "creo section family row storage",
                )?,
            }
        }
    }
    let segment_geometry = |segment: &crate::feature::definitions::FeatureSegment| {
        if let Some(geometry) = ctx
            .get_btree_map(
                segment_geometries,
                &segment.offset,
                "creo section entity geometry lookup",
            )?
            .and_then(Option::as_ref)
        {
            return geometry
                .try_clone_for_decode(ctx, "creo section entity geometry copy")
                .map(Some);
        }
        if section_degenerate_axis_line(ctx, definition, segment)? {
            let kind = ctx.copy_retained_text("line", "creo section entity native kind")?;
            return Ok(Some(SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    kind,
                    "validate nonblank text",
                )?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("native_kind must not be empty")
                })?,
            )));
        }
        Ok::<_, cadmpeg_core::CodecError>(None)
    };
    let mut entities = Vec::new();
    for segment in ctx.admit_iter(segments, "creo emitted section segment rows")? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let Some(geometry) = segment_geometry(segment)? else {
            continue;
        };
        let suffix = suffix_storage.with_storage(|| {
            section_segment_identity_suffix_admitted(ctx, unique_segment_ids, segment)
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            match (geometry.definition(), segment.kind) {
                (SketchGeometryDefinition::Native { native_kind }, _)
                    if native_kind.as_str() == "line" =>
                {
                    "section_degenerate_axis_line"
                }
                (SketchGeometryDefinition::ReferenceLine { .. }, _) => {
                    "solved_section_axis_reference_line"
                }
                (_, crate::feature::definitions::FeatureSegmentKind::Line(_)) => {
                    "solved_section_line"
                }
                (_, crate::feature::definitions::FeatureSegmentKind::Arc(_)) => {
                    "solved_section_arc"
                }
                (_, crate::feature::definitions::FeatureSegmentKind::Point(_)) => {
                    "solved_section_point"
                }
            },
            if matches!(
                geometry.definition(),
                SketchGeometryDefinition::Native { .. }
            ) {
                Exactness::ByteExact
            } else {
                Exactness::Derived
            },
        )?;
        let construction = matches!(
            geometry.definition(),
            SketchGeometryDefinition::ReferenceLine { .. }
        ) || !ctx.contains_btree_set(
            unique_segment_ids,
            &segment.external_id,
            "creo section entity ID membership",
        )? || (!ctx.contains_btree_set(
            solved,
            &segment.external_id,
            "creo section entity ID membership",
        )? && !ctx.contains_btree_set(
            profile_entities,
            &id,
            "creo emitted profile entity membership",
        )?);
        let point_ids = segment.point_ids();
        let reverse = [point_ids[1], point_ids[0]];
        let endpoints = match (geometry.definition(), segment.kind) {
            (SketchGeometryDefinition::Native { native_kind }, _)
                if native_kind.as_str() == "line" =>
            {
                &point_ids[..1]
            }
            (SketchGeometryDefinition::ReferenceLine { .. }, _)
                if section_degenerate_axis_line(ctx, definition, segment)? =>
            {
                &point_ids[..1]
            }
            (_, crate::feature::definitions::FeatureSegmentKind::Arc(_)) => &reverse[..],
            (_, crate::feature::definitions::FeatureSegmentKind::Line(_)) => &point_ids[..],
            (_, crate::feature::definitions::FeatureSegmentKind::Point(_)) => &point_ids[..1],
        };
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, endpoints)?;
        let geometry_ref = placed_sketch_curve_ref(ctx, transform, sketch_id, &suffix, &geometry)?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(construction)
                .with_native_ref(Some(native_ref))
                .with_geometry_ref(geometry_ref)
                .with_endpoint_refs(endpoint_refs),
        )?;
    }
    for segment in ctx.admit_iter(segments, "creo unresolved section segment rows")? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        if section_degenerate_axis_line(ctx, definition, segment)?
            || ctx
                .get_btree_map(
                    segment_geometries,
                    &segment.offset,
                    "creo section entity geometry lookup",
                )?
                .and_then(Option::as_ref)
                .is_some()
        {
            continue;
        }
        let suffix = suffix_storage.with_storage(|| {
            section_segment_identity_suffix_admitted(ctx, unique_segment_ids, segment)
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            "unresolved_section_segment",
            Exactness::ByteExact,
        )?;
        let point_ids = segment.point_ids();
        let reverse = [point_ids[1], point_ids[0]];
        let endpoints = match segment.kind {
            crate::feature::definitions::FeatureSegmentKind::Arc(_) => &reverse[..],
            crate::feature::definitions::FeatureSegmentKind::Line(_) => &point_ids[..],
            crate::feature::definitions::FeatureSegmentKind::Point(_) => &point_ids[..1],
        };
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, endpoints)?;
        let geometry = native_section_geometry(
            ctx,
            match segment.kind {
                crate::feature::definitions::FeatureSegmentKind::Line(_) => "line",
                crate::feature::definitions::FeatureSegmentKind::Arc(_) => "arc",
                crate::feature::definitions::FeatureSegmentKind::Point(_) => "point",
            },
        )?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(true)
                .with_native_ref(Some(native_ref))
                .with_endpoint_refs(endpoint_refs),
        )?;
    }
    for segment in ctx.admit_iter(&circle_rows, "creo entities circles segment rows")? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let unique_external_id = ctx.contains_btree_set(
            unique_segment_ids,
            &segment.external_id,
            "creo section entity ID membership",
        )?;
        if unique_external_id
            && ctx.contains_btree_set(
                materialized_saved_section_external_ids,
                &segment.external_id,
                "creo section entity ID membership",
            )?
        {
            continue;
        }
        let suffix = suffix_storage.with_storage(|| {
            section_row_suffix(
                ctx,
                unique_external_id,
                segment.external_id,
                "circle",
                segment.offset,
            )
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        let geometry = copied_or_native_geometry(ctx, circle_geometries, segment.offset, "circle")?;
        let solved_geometry = matches!(
            geometry.definition(),
            SketchGeometryDefinition::Circle { .. }
        );
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            if solved_geometry {
                "solved_section_circle"
            } else {
                "unresolved_section_circle"
            },
            if solved_geometry {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        )?;
        let construction = !unique_external_id
            || !ctx.contains_btree_set(
                profile_entities,
                &id,
                "creo emitted profile entity membership",
            )?;
        let geometry_ref = placed_sketch_curve_ref(ctx, transform, sketch_id, &suffix, &geometry)?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(construction)
                .with_native_ref(Some(native_ref))
                .with_geometry_ref(geometry_ref),
        )?;
    }

    for segment in ctx.admit_iter(&point_rows, "creo entities points segment rows")? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let unique_external_id = ctx.contains_btree_set(
            unique_segment_ids,
            &segment.external_id,
            "creo section entity ID membership",
        )?;
        if unique_external_id
            && ctx.contains_btree_set(
                materialized_saved_section_external_ids,
                &segment.external_id,
                "creo section entity ID membership",
            )?
        {
            continue;
        }
        let suffix = suffix_storage.with_storage(|| {
            section_row_suffix(
                ctx,
                unique_external_id,
                segment.external_id,
                "point",
                segment.offset,
            )
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        let geometry = copied_or_native_geometry(ctx, point_geometries, segment.offset, "point")?;
        let solved_geometry = matches!(
            geometry.definition(),
            SketchGeometryDefinition::Point { .. }
        );
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            if solved_geometry {
                "solved_section_point"
            } else {
                "unresolved_section_point"
            },
            if solved_geometry {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        )?;
        let construction = !unique_external_id
            || !ctx.contains_btree_set(
                profile_entities,
                &id,
                "creo emitted profile entity membership",
            )?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, &[segment.point_id])?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(construction)
                .with_native_ref(Some(native_ref))
                .with_endpoint_refs(endpoint_refs),
        )?;
    }

    for segment in ctx.admit_iter(
        &centered_line_rows,
        "creo entities centered_lines segment rows",
    )? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let unique_external_id = ctx.contains_btree_set(
            unique_segment_ids,
            &segment.external_id,
            "creo section entity ID membership",
        )?;
        if unique_external_id
            && ctx.contains_btree_set(
                materialized_saved_section_external_ids,
                &segment.external_id,
                "creo section entity ID membership",
            )?
        {
            continue;
        }
        let suffix = suffix_storage.with_storage(|| {
            section_row_suffix(
                ctx,
                unique_external_id,
                segment.external_id,
                "centered_line",
                segment.offset,
            )
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        let geometry =
            copied_or_native_geometry(ctx, centered_line_geometries, segment.offset, "line")?;
        let solved_geometry =
            matches!(geometry.definition(), SketchGeometryDefinition::Line { .. });
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            if solved_geometry {
                "solved_section_centered_line"
            } else {
                "unresolved_section_centered_line"
            },
            if solved_geometry {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        )?;
        let geometry_ref = placed_sketch_curve_ref(ctx, transform, sketch_id, &suffix, &geometry)?;
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, &[0, 1])?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(true)
                .with_native_ref(Some(native_ref))
                .with_geometry_ref(geometry_ref)
                .with_endpoint_refs(endpoint_refs),
        )?;
    }

    for segment in ctx.admit_iter(
        &reference_line_rows,
        "creo entities reference_lines segment rows",
    )? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let unique_external_id = ctx.contains_btree_set(
            unique_segment_ids,
            &segment.external_id,
            "creo section entity ID membership",
        )?;
        if unique_external_id
            && ctx.contains_btree_set(
                materialized_saved_section_external_ids,
                &segment.external_id,
                "creo section entity ID membership",
            )?
        {
            continue;
        }
        let suffix = suffix_storage.with_storage(|| {
            section_row_suffix(
                ctx,
                unique_external_id,
                segment.external_id,
                "reference_line",
                segment.offset,
            )
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        let geometry = copied_or_native_geometry(
            ctx,
            reference_line_geometries,
            segment.offset,
            "reference_line",
        )?;
        let solved_geometry = matches!(
            geometry.definition(),
            SketchGeometryDefinition::ReferenceLine { .. }
        );
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            if solved_geometry {
                "solved_section_reference_line"
            } else {
                "unresolved_section_reference_line"
            },
            if solved_geometry {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        )?;
        let geometry_ref = placed_sketch_curve_ref(ctx, transform, sketch_id, &suffix, &geometry)?;
        let endpoint_refs = admitted_optional_endpoint_refs(ctx, sketch_id, &segment.point_ids)?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(true)
                .with_native_ref(Some(native_ref))
                .with_geometry_ref(geometry_ref)
                .with_endpoint_refs(endpoint_refs),
        )?;
    }

    for segment in ctx.admit_iter(
        &bounded_curve_rows,
        "creo entities bounded_curves segment rows",
    )? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let unique_external_id = ctx.contains_btree_set(
            unique_segment_ids,
            &segment.external_id,
            "creo section entity ID membership",
        )?;
        if unique_external_id
            && ctx.contains_btree_set(
                materialized_saved_section_external_ids,
                &segment.external_id,
                "creo section entity ID membership",
            )?
        {
            continue;
        }
        let suffix = suffix_storage.with_storage(|| {
            section_row_suffix(
                ctx,
                unique_external_id,
                segment.external_id,
                "bounded_curve",
                segment.offset,
            )
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        let construction = !unique_external_id
            || !ctx.contains_btree_set(
                profile_entities,
                &id,
                "creo emitted profile entity membership",
            )?;
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            "unresolved_section_bounded_curve",
            Exactness::ByteExact,
        )?;
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, &segment.point_ids)?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        let geometry = native_section_geometry(ctx, "bounded_curve")?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(construction)
                .with_native_ref(Some(native_ref))
                .with_endpoint_refs(endpoint_refs),
        )?;
    }

    for segment in ctx.admit_iter(&conic_rows, "creo entities conics segment rows")? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let unique_external_id = ctx.contains_btree_set(
            unique_segment_ids,
            &segment.external_id,
            "creo section entity ID membership",
        )?;
        if unique_external_id
            && ctx.contains_btree_set(
                materialized_saved_section_external_ids,
                &segment.external_id,
                "creo section entity ID membership",
            )?
        {
            continue;
        }
        let suffix = suffix_storage.with_storage(|| {
            section_row_suffix(
                ctx,
                unique_external_id,
                segment.external_id,
                "conic",
                segment.offset,
            )
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, suffix)? else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            "unresolved_section_conic",
            Exactness::ByteExact,
        )?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        let geometry = native_section_geometry(ctx, "conic")?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(true)
                .with_native_ref(Some(native_ref)),
        )?;
    }

    for segment in ctx.admit_iter(&opaque_rows, "creo entities opaque segment rows")? {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let unique_external_id = ctx.contains_btree_set(
            unique_segment_ids,
            &segment.external_id,
            "creo section entity ID membership",
        )?;
        if unique_external_id
            && ctx.contains_btree_set(
                materialized_saved_section_external_ids,
                &segment.external_id,
                "creo section entity ID membership",
            )?
        {
            continue;
        }
        let suffix = suffix_storage.with_storage(|| {
            opaque_section_segment_identity_suffix_admitted(ctx, unique_segment_ids, segment)
        })?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        let kind = if unique_external_id {
            unique_section_incidence_curve_family(ctx, definition, segment.external_id)?
        } else {
            None
        };
        let static_kind = match kind {
            Some(SectionEntityIncidenceFamily::Point) => Some("point"),
            Some(SectionEntityIncidenceFamily::BoundedCurve) => Some("bounded_curve"),
            Some(SectionEntityIncidenceFamily::LineOrArc) => Some("line_or_arc"),
            Some(SectionEntityIncidenceFamily::Line) => Some("line"),
            Some(SectionEntityIncidenceFamily::Arc) => Some("arc"),
            Some(SectionEntityIncidenceFamily::Circular) => Some("circle"),
            _ => None,
        };
        let geometry = if let Some(kind) = static_kind {
            native_section_geometry(ctx, kind)?
        } else {
            let kind = ctx.format_retained(
                format_args!("segment_type:{}", segment.kind),
                "creo section native geometry kind",
            )?;
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    kind,
                    "validate nonblank text",
                )?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("native_kind must not be empty")
                })?,
            )
        };
        let construction = !unique_external_id
            || !ctx.contains_btree_set(
                profile_entities,
                &id,
                "creo emitted profile entity membership",
            )?;
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(segment.offset),
            "opaque_section_segment",
            Exactness::ByteExact,
        )?;
        let geometry_ref = placed_sketch_curve_ref(ctx, transform, sketch_id, &suffix, &geometry)?;
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(construction)
                .with_native_ref(Some(native_ref))
                .with_geometry_ref(geometry_ref),
        )?;
    }

    let mut identity_storage = ctx.reserve_scoped(0, "creo saved entity identity index")?;
    let mut entity_ids = std::collections::HashSet::<String>::new();
    for entity in ctx.admit_iter(&entities, "creo saved entity identity sources")? {
        identity_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut entity_ids,
                ctx.copy_retained_text(entity.id().as_str(), "creo saved identity index keys")?,
                "creo saved identity index nodes",
            )
        })?;
    }
    let mut geometry_storage = ctx.reserve_scoped(0, "creo saved geometry scratch storage")?;
    let mut saved_section_geometries = Vec::new();
    let mut generated_saved_geometries = Vec::new();
    let ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
        std::convert::Infallible,
    >(ctx, definition, |saved| {
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let Some((internal_id, geometry, offset)) = saved_section_entity_geometry(saved) else {
            return Ok(ControlFlow::Continue(()));
        };
        let unique_internal_id = ctx.contains_btree_set(
            unique_saved_ids,
            &internal_id,
            "creo saved entity identity lookup",
        )?;
        let external_id = match definition.order_table.as_ref() {
            Some(order) if unique_internal_id => saved_section_external_id(
                ctx,
                order,
                unique_saved_ids,
                ambiguous_segment_ids,
                internal_id,
            )?,
            _ => None,
        };
        let suffix = if unique_internal_id {
            match external_id {
                Some(external_id) => suffix_storage.with_storage(|| {
                    ctx.format_retained(format_args!("{external_id}"), "creo saved entity suffix")
                })?,
                None => suffix_storage.with_storage(|| {
                    ctx.format_retained(
                        format_args!("saved{internal_id}"),
                        "creo saved entity suffix",
                    )
                })?,
            }
        } else {
            suffix_storage.with_storage(|| {
                ctx.format_retained(
                    format_args!("saved:offset:{offset}"),
                    "creo saved entity suffix",
                )
            })?
        };
        let mut entity_storage = ctx.reserve_scoped(0, "creo saved entity candidate identity")?;
        let Some(entity_id) = entity_storage.with_storage(|| sketch_entity_id_admitted(ctx, sketch_id, &suffix))? else {
            return Ok(ControlFlow::Continue(()));
        };
        let already_present = ctx.contains_hash_set(
            &entity_ids,
            entity_id.as_str(),
            "creo saved identity index membership",
        )?;
        if already_present {
            return Ok(ControlFlow::Continue(()));
        }
        let generated = if let Some(external_id) = external_id {
            if let Some(expected_kinds) = section_generated_profile_surface_kinds(&geometry) {
                section_entity_is_generated_profile(
                    ctx,
                    complete_segment_table,
                    definition.identity.owner_feature_id(),
                    external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )?
            } else {
                false
            }
        } else {
            false
        };
        annotate(
            ctx,
            annotations,
            entity_id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(offset),
            "saved_section_entity",
            Exactness::Derived,
        )?;
        if let Some(external_id) = external_id.filter(|_| generated) {
            let copied = geometry_storage.with_storage(|| {
                geometry.try_clone_for_decode(ctx, "creo generated saved geometry copy")
            })?;
            geometry_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut generated_saved_geometries,
                    1,
                    "creo generated saved geometry rows",
                )
            })?;
            generated_saved_geometries.push((external_id, copied));
        }
        let native_ref = ctx.format_retained(
            format_args!(
                "{}:saved_entity#{internal_id}",
                suffix_storage.with_storage(|| sketch_native_ref_admitted(ctx, sketch_id))?
            ),
            "creo saved entity native reference",
        )?;
        let entity = SketchEntity::new(
            entity_storage.commit_value(entity_id)?,
            sketch_id.try_clone_for_decode(ctx, "creo saved entity sketch identity")?,
            geometry.try_clone_for_decode(ctx, "creo saved entity geometry copy")?,
        )
        .with_construction(!generated)
        .with_native_ref(Some(native_ref))
        .with_geometry_ref(placed_sketch_curve_ref(
            ctx, transform, sketch_id, &suffix, &geometry,
        )?);
        identity_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut entity_ids,
                ctx.copy_retained_text(entity.id().as_str(), "creo saved identity index keys")?,
                "creo saved identity index nodes",
            )
        })?;
        push_section_entity(ctx, &mut entities, entity)?;
        geometry_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut saved_section_geometries,
                1,
                "creo saved section geometry rows",
            )
        })?;
        saved_section_geometries.push((internal_id, external_id, geometry, offset, suffix, suffix_storage));
        Ok(ControlFlow::Continue(()))
    })?;
    let ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
        std::convert::Infallible,
    >(ctx, definition, |saved| {
        let crate::feature::definitions::FeatureSavedEntity::Spline(spline) = saved else {
            return Ok(ControlFlow::Continue(()));
        };
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let mut spline_storage = ctx.reserve_scoped(0, "creo saved spline candidate geometry")?;
        let geometry = spline_storage
            .with_storage(|| saved_spline_sketch_geometry(ctx, spline, &mut refusal))?;
        let refused = refusal.take_records_checked()?;
        let Some(geometry) = geometry.filter(|_| refused.is_empty()) else {
            for record in ctx.admit_iter(&refused, "creo refused saved spline records")? {
                let message = ctx.format_retained(
                    format_args!(
                        "Feature {} states a saved section spline at offset {} that forms no \
                     sketch geometry: {record}",
                        definition.identity.id(),
                        spline.offset
                    ),
                    "creo unresolved saved spline loss text",
                )?;
                ctx.reserve_vec(losses, 1, "creo unresolved saved spline losses")?;
                losses.push(crate::loss::CreoLossCode::SectionSplineUnresolved.note(message));
            }
            return Ok(ControlFlow::Continue(()));
        };
        let mut suffix_storage = ctx.reserve_scoped(0, "creo section identity suffix storage")?;
        let unique_internal_id = match spline.entity_id {
            Some(id)
                if ctx.contains_btree_set(
                    unique_saved_ids,
                    &id,
                    "creo saved spline identity lookup",
                )? =>
            {
                Some(id)
            }
            _ => None,
        };
        let suffix = match unique_internal_id {
            Some(id) => suffix_storage.with_storage(|| {
                ctx.format_retained(format_args!("{id}"), "creo saved spline suffix")
            })?,
            None => suffix_storage.with_storage(|| {
                ctx.format_retained(
                    format_args!("offset{}", spline.offset),
                    "creo saved spline suffix",
                )
            })?,
        };
        let external_id = match (unique_internal_id, definition.order_table.as_ref()) {
            (Some(internal_id), Some(order)) => saved_section_external_id(
                ctx,
                order,
                unique_saved_ids,
                ambiguous_segment_ids,
                internal_id,
            )?,
            _ => None,
        };
        let generated = if let Some(external_id) = external_id {
            if let Some(expected_kinds) = section_generated_profile_surface_kinds(&geometry) {
                section_entity_is_generated_profile(
                    ctx,
                    complete_segment_table,
                    definition.identity.owner_feature_id(),
                    external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )?
            } else {
                false
            }
        } else {
            false
        };
        let mut entity_storage = ctx.reserve_scoped(0, "creo saved spline candidate identity")?;
        let entity_id = entity_storage.with_storage(|| Ok::<_, cadmpeg_core::CodecError>(if let Some(external_id) = external_id {
            sketch_entity_id_admitted(ctx, sketch_id, external_id)?
        } else {
            let namespace = &crate::identity::FEATDEFS_SAVED_SPLINE;
            let text = ctx.format_retained(
                format_args!(
                    "{}:{}:{}#{}:{suffix}",
                    namespace.format(),
                    namespace.scope(),
                    namespace.kind(),
                    sketch_identity_scope(sketch_id)
                ),
                "creo saved spline entity identity",
            )?;
            SketchEntityId::try_from(text).ok()
        }))?;
        let Some(entity_id) = entity_id else {
            return Ok(ControlFlow::Continue(()));
        };
        let already_present = ctx.contains_hash_set(
            &entity_ids,
            entity_id.as_str(),
            "creo saved identity index membership",
        )?;
        if already_present {
            return Ok(ControlFlow::Continue(()));
        }
        annotate(
            ctx,
            annotations,
            entity_id.as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(spline.offset),
            "saved_interpolation_spline",
            Exactness::Derived,
        )?;
        let native_ref = ctx.format_retained(
            format_args!(
                "{}:saved_spline#{suffix}",
                suffix_storage.with_storage(|| sketch_native_ref_admitted(ctx, sketch_id))?
            ),
            "creo saved spline native reference",
        )?;
        let geometry_ref = if transform.is_some() {
        let namespace = &crate::identity::FEATDEFS_SAVED_SPLINE_CURVE;
        let curve_text = ctx.format_retained(
            format_args!(
                "{}:{}:{}#{}:{suffix}",
                namespace.format(),
                namespace.scope(),
                namespace.kind(),
                sketch_identity_scope(sketch_id)
            ),
            "creo saved spline curve identity",
        )?;
        let curve_id = CurveId::try_from(curve_text).map_err(|_| {
            cadmpeg_core::CodecError::malformed("saved spline curve identity is invalid")
        })?;
            Some(cadmpeg_ir::ids::Identity::from(curve_id).into_string())
        } else {
            None
        };
        let entity = SketchEntity::new(
            entity_storage.commit_value(entity_id)?,
            sketch_id.try_clone_for_decode(ctx, "creo saved spline sketch identity")?,
            geometry.try_clone_for_decode(ctx, "creo saved spline geometry copy")?,
        )
        .with_construction(!generated)
        .with_native_ref(Some(native_ref))
        .with_geometry_ref(geometry_ref);
        identity_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut entity_ids,
                ctx.copy_retained_text(entity.id().as_str(), "creo saved identity index keys")?,
                "creo saved identity index nodes",
            )
        })?;
        push_section_entity(ctx, &mut entities, entity)?;
        if let Some(external_id) = external_id.filter(|_| generated) {
            geometry_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut generated_saved_geometries,
                    1,
                    "creo generated saved geometry rows",
                )
            })?;
            geometry_storage.absorb(&mut spline_storage)?;
            generated_saved_geometries.push((external_id, geometry));
        }
        Ok(ControlFlow::Continue(()))
    })?;
    let ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
        std::convert::Infallible,
    >(ctx, definition, |saved| {
        let mut candidate_storage = ctx.reserve_scoped(0, "creo unresolved saved candidate storage")?;
        let Some((entity, offset)) = candidate_storage.with_storage(|| unresolved_saved_section_entity(
            ctx,
            definition,
            sketch_id,
            saved,
            unique_saved_ids,
            ambiguous_segment_ids,
        ))?
        else {
            return Ok(ControlFlow::Continue(()));
        };
        let already_present = ctx.contains_hash_set(
            &entity_ids,
            entity.id().as_str(),
            "creo saved identity index membership",
        )?;
        if already_present {
            return Ok(ControlFlow::Continue(()));
        }
        annotate(
            ctx,
            annotations,
            entity.id().as_str(),
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(offset),
            "unresolved_saved_section_entity",
            Exactness::ByteExact,
        )?;
        identity_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut entity_ids,
                ctx.copy_retained_text(entity.id().as_str(), "creo saved identity index keys")?,
                "creo saved identity index nodes",
            )
        })?;
        push_section_entity(ctx, &mut entities, candidate_storage.commit_value(entity)?)?;
        Ok(ControlFlow::Continue(()))
    })?;
    let saved_profiles = saved_profile_chains(ctx, sketch_id, &generated_saved_geometries)?;
    ctx.extend_vec(
        &mut profiles,
        saved_profiles,
        "creo saved section profile rows",
    )?;
    if let Some(transform) = transform {
        let mut curve_identity_storage =
            ctx.reserve_scoped(0, "creo placed curve identity index")?;
        let mut curve_ids = std::collections::HashSet::<String>::new();
        for curve in ctx.admit_iter(&ir.model.curves, "creo placed curve identity sources")? {
            curve_identity_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut curve_ids,
                    ctx.copy_retained_text(curve.id.as_str(), "creo placed curve index keys")?,
                    "creo placed curve index nodes",
                )
            })?;
        }
        for segment in ctx.admit_iter(segments, "creo placed section segment rows")? {
            let mut suffix_storage =
                ctx.reserve_scoped(0, "creo section identity suffix storage")?;
            let section_geometry = match ctx
                .get_btree_map(
                    resolved_segment_geometries,
                    &segment.offset,
                    "creo placed resolved geometry lookup",
                )?
                .and_then(Option::as_ref)
            {
                Some(geometry) => Some(geometry),
                None => ctx
                    .get_btree_map(
                        segment_geometries,
                        &segment.offset,
                        "creo placed fallback geometry lookup",
                    )?
                    .and_then(Option::as_ref)
                    .filter(|geometry| {
                        matches!(
                            geometry.definition(),
                            SketchGeometryDefinition::ReferenceLine { .. }
                        )
                    }),
            };
            let Some(section_geometry) = section_geometry else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, section_geometry) else {
                continue;
            };
            let suffix = suffix_storage.with_storage(|| {
                section_segment_identity_suffix_admitted(ctx, unique_segment_ids, segment)
            })?;
            let mut curve_storage = ctx.reserve_scoped(0, "creo placed curve candidate identity")?;
            let Some(id) = curve_storage.with_storage(|| typed_sketch_section_curve_id_admitted(ctx, sketch_id, &suffix))? else {
                continue;
            };
            let already_present = ctx.contains_hash_set(
                &curve_ids,
                id.as_str(),
                "creo placed curve index membership",
            )?;
            if already_present {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(segment.offset),
                "placed_section_curve",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            curve_identity_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut curve_ids,
                    ctx.copy_retained_text(id.as_str(), "creo placed curve index keys")?,
                    "creo placed curve index nodes",
                )
            })?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: curve_storage.commit_value(id)?,
                    geometry,
                    source_object: Some(placed_source_object(
                        ctx,
                        format_args!(
                            "FeatDefs:section#{}:{suffix}",
                            sketch_identity_scope(sketch_id)
                        ),
                    )?),
                },
            )?;
        }
        for segment in ctx.admit_iter(&circle_rows, "creo placed section circles rows")? {
            let mut suffix_storage =
                ctx.reserve_scoped(0, "creo section identity suffix storage")?;
            let Some(section_geometry) = ctx.get_btree_map(
                circle_geometries,
                &segment.offset,
                "creo section entity geometry lookup",
            )?
            else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, section_geometry) else {
                continue;
            };
            let suffix = suffix_storage.with_storage(|| {
                section_row_suffix(
                    ctx,
                    ctx.contains_btree_set(
                        unique_segment_ids,
                        &segment.external_id,
                        "creo section entity ID membership",
                    )?,
                    segment.external_id,
                    "circle",
                    segment.offset,
                )
            })?;
            let mut curve_storage = ctx.reserve_scoped(0, "creo placed curve candidate identity")?;
            let Some(id) = curve_storage.with_storage(|| typed_sketch_section_curve_id_admitted(ctx, sketch_id, &suffix))? else {
                continue;
            };
            let already_present = ctx.contains_hash_set(
                &curve_ids,
                id.as_str(),
                "creo placed curve index membership",
            )?;
            if already_present {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(segment.offset),
                "placed_section_circle",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            curve_identity_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut curve_ids,
                    ctx.copy_retained_text(id.as_str(), "creo placed curve index keys")?,
                    "creo placed curve index nodes",
                )
            })?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: curve_storage.commit_value(id)?,
                    geometry,
                    source_object: Some(placed_source_object(
                        ctx,
                        format_args!(
                            "FeatDefs:section#{}:{suffix}",
                            sketch_identity_scope(sketch_id)
                        ),
                    )?),
                },
            )?;
        }

        for segment in ctx.admit_iter(
            &centered_line_rows,
            "creo placed section centered-lines rows",
        )? {
            let mut suffix_storage =
                ctx.reserve_scoped(0, "creo section identity suffix storage")?;
            let Some(section_geometry) = ctx.get_btree_map(
                centered_line_geometries,
                &segment.offset,
                "creo section entity geometry lookup",
            )?
            else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, section_geometry) else {
                continue;
            };
            let suffix = suffix_storage.with_storage(|| {
                section_row_suffix(
                    ctx,
                    ctx.contains_btree_set(
                        unique_segment_ids,
                        &segment.external_id,
                        "creo section entity ID membership",
                    )?,
                    segment.external_id,
                    "centered_line",
                    segment.offset,
                )
            })?;
            let mut curve_storage = ctx.reserve_scoped(0, "creo placed curve candidate identity")?;
            let Some(id) = curve_storage.with_storage(|| typed_sketch_section_curve_id_admitted(ctx, sketch_id, &suffix))? else {
                continue;
            };
            let already_present = ctx.contains_hash_set(
                &curve_ids,
                id.as_str(),
                "creo placed curve index membership",
            )?;
            if already_present {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(segment.offset),
                "placed_section_line",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            curve_identity_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut curve_ids,
                    ctx.copy_retained_text(id.as_str(), "creo placed curve index keys")?,
                    "creo placed curve index nodes",
                )
            })?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: curve_storage.commit_value(id)?,
                    geometry,
                    source_object: Some(placed_source_object(
                        ctx,
                        format_args!(
                            "FeatDefs:section#{}:{suffix}",
                            sketch_identity_scope(sketch_id)
                        ),
                    )?),
                },
            )?;
        }

        for (internal_id, external_id, section_geometry, offset, suffix, _suffix_storage) in
            ctx.admit_iter(saved_section_geometries, "creo placed saved geometry rows")?
        {
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            let mut curve_storage = ctx.reserve_scoped(0, "creo saved curve candidate identity")?;
            let Some(id) = curve_storage.with_storage(|| typed_sketch_section_curve_id_admitted(ctx, sketch_id, &suffix))? else {
                continue;
            };
            let already_present = ctx.contains_hash_set(
                &curve_ids,
                id.as_str(),
                "creo placed curve index membership",
            )?;
            if already_present {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(offset),
                "placed_saved_section_curve",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            curve_identity_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut curve_ids,
                    ctx.copy_retained_text(id.as_str(), "creo placed curve index keys")?,
                    "creo placed curve index nodes",
                )
            })?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: curve_storage.commit_value(id)?,
                    geometry,
                    source_object: Some(match external_id {
                        Some(external_id) => placed_source_object(
                            ctx,
                            format_args!(
                                "FeatDefs:section#{}:{external_id}",
                                sketch_identity_scope(sketch_id)
                            ),
                        )?,
                        None => placed_source_object(
                            ctx,
                            format_args!("FeatDefs:saved_entity#{internal_id}"),
                        )?,
                    }),
                },
            )?;
        }
    }
    Ok((entities, profiles))
}

#[cfg(test)]
mod tests {
    mod saved_storage;
    mod placed_storage;
    mod set_owner_tests;
    use super::{
        admitted_endpoint_refs, copied_or_native_geometry, native_section_geometry,
        placed_source_object, push_section_entity, section_row_suffix,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};

    fn with_policy<T>(policy: &DecodePolicy, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).expect("empty root");
        run(&ctx)
    }

    #[test]
    fn native_section_geometry_kind_refuses_before_retained_copy() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo section native geometry kind"),
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                native_section_geometry(&ctx, "line")
            },
        );
        assert!(
            matches!(with_policy(&policy, |ctx| native_section_geometry(ctx, "line")),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo section native geometry kind")
        );
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                native_section_geometry(&ctx, "line")
            },
        );
        assert_eq!(
            with_policy(&policy, |ctx| native_section_geometry(ctx, "line"))
                .expect("exact cap admits kind"),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("line").expect("valid kind")
            )
        );
    }

    #[test]
    fn copied_section_geometry_refuses_before_nested_text_copy() {
        let geometries = std::collections::BTreeMap::from([(
            7,
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("circle").expect("valid kind"),
            ),
        )]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo section geometry copy"),
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                copied_or_native_geometry(&ctx, &geometries, 7, "line")
            },
        );
        assert!(
            matches!(with_policy(&policy, |ctx| copied_or_native_geometry(ctx, &geometries, 7, "line")),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo section geometry copy")
        );
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                copied_or_native_geometry(&ctx, &geometries, 7, "line")
            },
        );
        assert_eq!(
            with_policy(&policy, |ctx| copied_or_native_geometry(
                ctx,
                &geometries,
                7,
                "line"
            ))
            .expect("exact cap admits copy"),
            geometries[&7]
        );
    }

    #[test]
    fn section_row_suffix_refuses_each_retained_choice() {
        for (unique, expected) in [(true, "42"), (false, "circle:offset:9")] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some("creo section entity suffix"),
                |cap| {
                    let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                    let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                    trial_policy.limits.max_retained_bytes = cap;
                    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &trial_arena,
                        &trial_policy,
                    )
                    .expect("root");
                    section_row_suffix(&ctx, unique, 42, "circle", 9)
                },
            );
            assert!(
                matches!(with_policy(&policy, |ctx| section_row_suffix(ctx, unique, 42, "circle", 9)),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == "creo section entity suffix")
            );
            policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| {
                    let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                    let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                    trial_policy.limits.max_retained_bytes = cap;
                    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &trial_arena,
                        &trial_policy,
                    )
                    .expect("root");
                    section_row_suffix(&ctx, unique, 42, "circle", 9)
                },
            );
            assert_eq!(
                with_policy(&policy, |ctx| section_row_suffix(
                    ctx, unique, 42, "circle", 9
                ))
                .expect("exact cap admits suffix"),
                expected
            );
        }
    }

    #[test]
    fn placed_section_source_refuses_before_object_id_formatting() {
        let expected = "FeatDefs:section#5:42";
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo placed section source object"),
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                placed_source_object(&ctx, expected)
            },
        );
        assert!(
            matches!(with_policy(&policy, |ctx| placed_source_object(ctx, expected)),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo placed section source object")
        );
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                placed_source_object(&ctx, expected)
            },
        );
        let admitted = with_policy(&policy, |ctx| placed_source_object(ctx, expected))
            .expect("exact cap admits placed source");
        assert_eq!(admitted.object_id.as_str(), expected);
    }

    #[test]
    fn section_entity_row_refuses_before_vector_growth() {
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let id = SketchEntityId::mint("creo:featdefs:sketch_entity#5:7").expect("valid entity ID");
        let geometry = SketchGeometry::native(
            cadmpeg_core::text::NonBlankString::try_from("line").expect("valid kind"),
        );
        let entity = || SketchEntity::new(id.clone(), sketch.clone(), geometry.clone());
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(
            matches!(with_policy(&policy, |ctx| push_section_entity(ctx, &mut Vec::new(), entity())),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo section entities")
        );
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            None,
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_collection_items = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                push_section_entity(&ctx, &mut Vec::new(), entity())
            },
        );
        let mut entities = Vec::new();
        with_policy(&policy, |ctx| {
            push_section_entity(ctx, &mut entities, entity())
        })
        .expect("exact cap admits row");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id(), &id);
    }

    #[test]
    fn endpoint_references_refuse_nested_text_and_vector_slot() {
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let expected = "creo:featdefs:sketch#5:point#7";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo sketch point reference"),
            |cap| {
                let trial_arena = cadmpeg_core::decode::DecodeArena::new();
                let mut trial_policy = cadmpeg_core::decode::DecodePolicy::service();
                trial_policy.limits.max_retained_bytes = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &trial_arena,
                    &trial_policy,
                )
                .expect("root");
                admitted_endpoint_refs(&ctx, &sketch, &[7])
            },
        );
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(admitted_endpoint_refs(&ctx, &sketch, &[7]),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo sketch point reference"));
        policy.limits.max_retained_bytes = DecodePolicy::service().limits.max_retained_bytes;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(admitted_endpoint_refs(&ctx, &sketch, &[7]),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo section endpoint references"));
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        assert_eq!(
            admitted_endpoint_refs(&ctx, &sketch, &[7]).expect("service endpoint"),
            [expected]
        );
    }
}
