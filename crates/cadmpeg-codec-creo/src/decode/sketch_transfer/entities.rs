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
    section_segment_identity_suffix_admitted, semantic_saved_section_entities,
    unresolved_saved_section_entity,
};
use crate::decode::sketch_transfer::loci::section_degenerate_axis_line;
use crate::decode::sketch_transfer::profiles::{
    unique_section_incidence_curve_family, SectionEntityIncidenceFamily,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

fn admitted_endpoint_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    points: impl IntoIterator<Item = u32>,
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    let mut references = Vec::new();
    for point in points {
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
        cadmpeg_core::text::NonBlankString::new(kind)
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("native_kind must not be empty"))?,
    ))
}

fn copied_or_native_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometries: &BTreeMap<usize, SketchGeometry>,
    offset: usize,
    kind: &str,
) -> Result<SketchGeometry, cadmpeg_core::CodecError> {
    match geometries.get(&offset) {
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
        object_id: cadmpeg_core::text::NonBlankString::new(object_id).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("source object_id must not be empty")
        })?,
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    })
}

#[allow(clippy::too_many_arguments)] // mechanical extract from transfer_sketches
pub(super) fn transfer_section_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    definition: &crate::feature::definitions::FeatureDefinition,
    transform: Option<&crate::placement::FeatureSectionTransform>,
    sketch_id: &SketchId,
    segments: &[&crate::feature::definitions::FeatureSegment],
    unique_segment_ids: &BTreeSet<u32>,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
    complete_segment_table: bool,
    solved: &BTreeSet<u32>,
    segment_geometries: &BTreeMap<usize, Option<SketchGeometry>>,
    resolved_segment_geometries: &BTreeMap<usize, Option<SketchGeometry>>,
    circle_geometries: &BTreeMap<usize, SketchGeometry>,
    point_geometries: &BTreeMap<usize, SketchGeometry>,
    centered_line_geometries: &BTreeMap<usize, SketchGeometry>,
    reference_line_geometries: &BTreeMap<usize, SketchGeometry>,
    materialized_saved_section_external_ids: &BTreeSet<u32>,
    mut profiles: Vec<Vec<SketchEntityUse>>,
    profile_entities: &BTreeSet<SketchEntityId>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<(Vec<SketchEntity>, Vec<Vec<SketchEntityUse>>), cadmpeg_core::CodecError> {
    let segment_geometry = |segment: &crate::feature::definitions::FeatureSegment| -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
        if let Some(geometry) = segment_geometries.get(&segment.offset).and_then(Option::as_ref) {
            return geometry.try_clone_for_decode(ctx, "creo section entity geometry copy").map(Some);
        }
        if section_degenerate_axis_line(definition, segment) {
            let kind = ctx.copy_retained_text("line", "creo section entity native kind")?;
            return Ok(Some(SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::new(kind).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("native_kind must not be empty")
                })?,
            )));
        }
        Ok(None)
    };
    let mut entities = Vec::new();
    for segment in segments {
        let Some(geometry) = segment_geometry(segment)? else {
            continue;
        };
        let suffix = section_segment_identity_suffix_admitted(ctx, unique_segment_ids, segment)?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            segment.offset as u64,
            match (geometry.definition(), segment.kind) {
                (SketchGeometryDefinition::Native { native_kind }, _) if native_kind == "line" => {
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
        ) || !unique_segment_ids.contains(&segment.external_id)
            || (!solved.contains(&segment.external_id) && !profile_entities.contains(&id));
        let point_ids = segment.point_ids();
        let reverse = [point_ids[1], point_ids[0]];
        let endpoints = match (geometry.definition(), segment.kind) {
            (SketchGeometryDefinition::Native { native_kind }, _) if native_kind == "line" => {
                &point_ids[..1]
            }
            (SketchGeometryDefinition::ReferenceLine { .. }, _)
                if section_degenerate_axis_line(definition, segment) =>
            {
                &point_ids[..1]
            }
            (_, crate::feature::definitions::FeatureSegmentKind::Arc(_)) => &reverse[..],
            (_, crate::feature::definitions::FeatureSegmentKind::Line(_)) => &point_ids[..],
            (_, crate::feature::definitions::FeatureSegmentKind::Point(_)) => &point_ids[..1],
        };
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, endpoints.iter().copied())?;
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
    for segment in segments.iter().filter(|segment| {
        !section_degenerate_axis_line(definition, segment)
            && segment_geometries
                .get(&segment.offset)
                .and_then(Option::as_ref)
                .is_none()
    }) {
        let suffix = section_segment_identity_suffix_admitted(ctx, unique_segment_ids, segment)?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            segment.offset as u64,
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
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, endpoints.iter().copied())?;
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
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.circles())
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = section_row_suffix(
            ctx,
            unique_external_id,
            segment.external_id,
            "circle",
            segment.offset,
        )?;
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
            segment.offset as u64,
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
        let construction = !unique_external_id || !profile_entities.contains(&id);
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
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.points())
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = section_row_suffix(
            ctx,
            unique_external_id,
            segment.external_id,
            "point",
            segment.offset,
        )?;
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
            segment.offset as u64,
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
        let construction = !unique_external_id || !profile_entities.contains(&id);
        let sketch_copy =
            sketch_id.try_clone_for_decode(ctx, "creo section entity sketch identity")?;
        let native_ref = sketch_native_ref_admitted(ctx, sketch_id)?;
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, [segment.point_id])?;
        push_section_entity(
            ctx,
            &mut entities,
            SketchEntity::new(id, sketch_copy, geometry)
                .with_construction(construction)
                .with_native_ref(Some(native_ref))
                .with_endpoint_refs(endpoint_refs),
        )?;
    }
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.centered_lines())
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = section_row_suffix(
            ctx,
            unique_external_id,
            segment.external_id,
            "centered_line",
            segment.offset,
        )?;
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
            segment.offset as u64,
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
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, [0, 1])?;
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
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.reference_lines())
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = section_row_suffix(
            ctx,
            unique_external_id,
            segment.external_id,
            "reference_line",
            segment.offset,
        )?;
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
            segment.offset as u64,
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
        let endpoint_refs =
            admitted_endpoint_refs(ctx, sketch_id, segment.point_ids.into_iter().flatten())?;
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
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.bounded_curves())
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = section_row_suffix(
            ctx,
            unique_external_id,
            segment.external_id,
            "bounded_curve",
            segment.offset,
        )?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        let construction = !unique_external_id || !profile_entities.contains(&id);
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            segment.offset as u64,
            "unresolved_section_bounded_curve",
            Exactness::ByteExact,
        )?;
        let endpoint_refs = admitted_endpoint_refs(ctx, sketch_id, segment.point_ids)?;
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
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.conics())
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = section_row_suffix(
            ctx,
            unique_external_id,
            segment.external_id,
            "conic",
            segment.offset,
        )?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, suffix)? else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            segment.offset as u64,
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
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.opaque())
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix =
            opaque_section_segment_identity_suffix_admitted(ctx, unique_segment_ids, segment)?;
        let Some(id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        let kind = unique_external_id
            .then(|| unique_section_incidence_curve_family(definition, segment.external_id))
            .flatten();
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
            SketchGeometry::native(cadmpeg_core::text::NonBlankString::new(kind).ok_or_else(
                || cadmpeg_core::CodecError::malformed("native_kind must not be empty"),
            )?)
        };
        let construction = !unique_external_id || !profile_entities.contains(&id);
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "FeatDefs",
            segment.offset as u64,
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
    let mut saved_section_geometries = Vec::new();
    let mut generated_saved_geometries = Vec::new();
    for (internal_id, geometry, offset) in
        semantic_saved_section_entities(definition).filter_map(saved_section_entity_geometry)
    {
        let unique_internal_id = unique_saved_ids.contains(&internal_id);
        let external_id = if unique_internal_id {
            definition.order_table.as_ref().and_then(|order| {
                saved_section_external_id(
                    order,
                    unique_saved_ids,
                    ambiguous_segment_ids,
                    internal_id,
                )
            })
        } else {
            None
        };
        let suffix = if unique_internal_id {
            match external_id {
                Some(external_id) => {
                    ctx.format_retained(format_args!("{external_id}"), "creo saved entity suffix")?
                }
                None => ctx.format_retained(
                    format_args!("saved{internal_id}"),
                    "creo saved entity suffix",
                )?,
            }
        } else {
            ctx.format_retained(
                format_args!("saved:offset:{offset}"),
                "creo saved entity suffix",
            )?
        };
        let Some(entity_id) = sketch_entity_id_admitted(ctx, sketch_id, &suffix)? else {
            continue;
        };
        if entities.iter().any(|entity| entity.id() == &entity_id) {
            continue;
        }
        let generated = external_id.is_some_and(|external_id| {
            section_generated_profile_surface_kinds(&geometry).is_some_and(|expected_kinds| {
                section_entity_is_generated_profile(
                    complete_segment_table,
                    definition.identity.owner_feature_id(),
                    external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )
            })
        });
        let Some(curve_id) = typed_sketch_section_curve_id_admitted(ctx, sketch_id, &suffix)?
        else {
            continue;
        };
        annotate(
            ctx,
            annotations,
            entity_id.as_str(),
            "FeatDefs",
            offset as u64,
            "saved_section_entity",
            Exactness::Derived,
        )?;
        if let Some(external_id) = external_id.filter(|_| generated) {
            let copied =
                geometry.try_clone_for_decode(ctx, "creo generated saved geometry copy")?;
            ctx.reserve_vec(
                &mut generated_saved_geometries,
                1,
                "creo generated saved geometry rows",
            )?;
            generated_saved_geometries.push((external_id, copied));
        }
        let native_ref = ctx.format_retained(
            format_args!(
                "{}:saved_entity#{internal_id}",
                sketch_native_ref_admitted(ctx, sketch_id)?
            ),
            "creo saved entity native reference",
        )?;
        let entity = SketchEntity::new(
            entity_id,
            sketch_id.try_clone_for_decode(ctx, "creo saved entity sketch identity")?,
            geometry.try_clone_for_decode(ctx, "creo saved entity geometry copy")?,
        )
        .with_construction(!generated)
        .with_native_ref(Some(native_ref))
        .with_geometry_ref(placed_sketch_curve_ref(
            ctx, transform, sketch_id, &suffix, &geometry,
        )?);
        push_section_entity(ctx, &mut entities, entity)?;
        ctx.reserve_vec(
            &mut saved_section_geometries,
            1,
            "creo saved section geometry rows",
        )?;
        saved_section_geometries.push((internal_id, external_id, geometry, offset, curve_id));
    }
    for spline in semantic_saved_section_entities(definition).filter_map(|entity| match entity {
        crate::feature::definitions::FeatureSavedEntity::Spline(spline) => Some(spline),
        _ => None,
    }) {
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let geometry = saved_spline_sketch_geometry(ctx, spline, &mut refusal)?;
        let refused = refusal.take_records_checked()?;
        let Some(geometry) = geometry.filter(|_| refused.is_empty()) else {
            for record in &refused {
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
            continue;
        };
        let unique_internal_id = spline.entity_id.filter(|id| unique_saved_ids.contains(id));
        let suffix = match unique_internal_id {
            Some(id) => ctx.format_retained(format_args!("{id}"), "creo saved spline suffix")?,
            None => ctx.format_retained(
                format_args!("offset{}", spline.offset),
                "creo saved spline suffix",
            )?,
        };
        let external_id = unique_internal_id.and_then(|internal_id| {
            definition.order_table.as_ref().and_then(|order| {
                saved_section_external_id(
                    order,
                    unique_saved_ids,
                    ambiguous_segment_ids,
                    internal_id,
                )
            })
        });
        let generated = external_id.is_some_and(|external_id| {
            let Some(expected_kinds) = section_generated_profile_surface_kinds(&geometry) else {
                return false;
            };
            section_entity_is_generated_profile(
                complete_segment_table,
                definition.identity.owner_feature_id(),
                external_id,
                expected_kinds,
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            )
        });
        let entity_id = if let Some(external_id) = external_id {
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
        };
        let Some(entity_id) = entity_id else {
            continue;
        };
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
        if entities.iter().any(|entity| entity.id() == &entity_id) {
            continue;
        }
        annotate(
            ctx,
            annotations,
            entity_id.as_str(),
            "FeatDefs",
            spline.offset as u64,
            "saved_interpolation_spline",
            Exactness::Derived,
        )?;
        let native_ref = ctx.format_retained(
            format_args!(
                "{}:saved_spline#{suffix}",
                sketch_native_ref_admitted(ctx, sketch_id)?
            ),
            "creo saved spline native reference",
        )?;
        let geometry_ref = transform
            .map(|_| {
                ctx.copy_retained_text(curve_id.as_str(), "creo saved spline geometry reference")
            })
            .transpose()?;
        let entity = SketchEntity::new(
            entity_id,
            sketch_id.try_clone_for_decode(ctx, "creo saved spline sketch identity")?,
            geometry.try_clone_for_decode(ctx, "creo saved spline geometry copy")?,
        )
        .with_construction(!generated)
        .with_native_ref(Some(native_ref))
        .with_geometry_ref(geometry_ref);
        push_section_entity(ctx, &mut entities, entity)?;
        if let Some(external_id) = external_id.filter(|_| generated) {
            ctx.reserve_vec(
                &mut generated_saved_geometries,
                1,
                "creo generated saved geometry rows",
            )?;
            generated_saved_geometries.push((external_id, geometry));
        }
    }
    for saved in semantic_saved_section_entities(definition) {
        let Some((entity, offset)) = unresolved_saved_section_entity(
            ctx,
            definition,
            sketch_id,
            saved,
            unique_saved_ids,
            ambiguous_segment_ids,
        )?
        else {
            continue;
        };
        if entities.iter().any(|existing| existing.id() == entity.id()) {
            continue;
        }
        annotate(
            ctx,
            annotations,
            entity.id().as_str(),
            "FeatDefs",
            offset as u64,
            "unresolved_saved_section_entity",
            Exactness::ByteExact,
        )?;
        push_section_entity(ctx, &mut entities, entity)?;
    }
    let saved_profiles = saved_profile_chains(ctx, sketch_id, &generated_saved_geometries)?;
    ctx.reserve_vec(
        &mut profiles,
        saved_profiles.len(),
        "creo saved section profile rows",
    )?;
    profiles.extend(saved_profiles);
    if let Some(transform) = transform {
        for segment in segments {
            let Some(section_geometry) = resolved_segment_geometries
                .get(&segment.offset)
                .and_then(Option::as_ref)
                .or_else(|| {
                    segment_geometries
                        .get(&segment.offset)
                        .and_then(Option::as_ref)
                        .filter(|geometry| {
                            matches!(
                                geometry.definition(),
                                SketchGeometryDefinition::ReferenceLine { .. }
                            )
                        })
                })
            else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, section_geometry) else {
                continue;
            };
            let suffix =
                section_segment_identity_suffix_admitted(ctx, unique_segment_ids, segment)?;
            let Some(id) = typed_sketch_section_curve_id_admitted(ctx, sketch_id, &suffix)? else {
                continue;
            };
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "placed_section_curve",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id,
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
        for segment in definition
            .segments
            .iter()
            .flat_map(|segments| segments.rows.circles())
        {
            let Some(section_geometry) = circle_geometries.get(&segment.offset) else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, section_geometry) else {
                continue;
            };
            let suffix = section_row_suffix(
                ctx,
                unique_segment_ids.contains(&segment.external_id),
                segment.external_id,
                "circle",
                segment.offset,
            )?;
            let Some(id) = typed_sketch_section_curve_id_admitted(ctx, sketch_id, &suffix)? else {
                continue;
            };
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "placed_section_circle",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id,
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
        for segment in definition
            .segments
            .iter()
            .flat_map(|segments| segments.rows.centered_lines())
        {
            let Some(section_geometry) = centered_line_geometries.get(&segment.offset) else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, section_geometry) else {
                continue;
            };
            let suffix = section_row_suffix(
                ctx,
                unique_segment_ids.contains(&segment.external_id),
                segment.external_id,
                "centered_line",
                segment.offset,
            )?;
            let Some(id) = typed_sketch_section_curve_id_admitted(ctx, sketch_id, &suffix)? else {
                continue;
            };
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "placed_section_line",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id,
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
        for (internal_id, external_id, section_geometry, offset, id) in saved_section_geometries {
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                offset as u64,
                "placed_saved_section_curve",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id,
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
        policy.limits.max_retained_bytes = 3;
        assert!(
            matches!(with_policy(&policy, |ctx| native_section_geometry(ctx, "line")),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo section native geometry kind")
        );
        policy.limits.max_retained_bytes = 4;
        assert_eq!(
            with_policy(&policy, |ctx| native_section_geometry(ctx, "line"))
                .expect("exact cap admits kind"),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::new("line").expect("valid kind")
            )
        );
    }

    #[test]
    fn copied_section_geometry_refuses_before_nested_text_copy() {
        let geometries = std::collections::BTreeMap::from([(
            7,
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::new("circle").expect("valid kind"),
            ),
        )]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 5;
        assert!(
            matches!(with_policy(&policy, |ctx| copied_or_native_geometry(ctx, &geometries, 7, "line")),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo section geometry copy")
        );
        policy.limits.max_retained_bytes = 6;
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
            policy.limits.max_retained_bytes = expected.len() as u64 - 1;
            assert!(
                matches!(with_policy(&policy, |ctx| section_row_suffix(ctx, unique, 42, "circle", 9)),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == "creo section entity suffix")
            );
            policy.limits.max_retained_bytes = expected.len() as u64;
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
        policy.limits.max_retained_bytes = expected.len() as u64 - 1;
        assert!(
            matches!(with_policy(&policy, |ctx| placed_source_object(ctx, expected)),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo placed section source object")
        );
        policy.limits.max_retained_bytes = expected.len() as u64;
        let admitted = with_policy(&policy, |ctx| placed_source_object(ctx, expected))
            .expect("exact cap admits placed source");
        assert_eq!(admitted.object_id.as_str(), expected);
    }

    #[test]
    fn section_entity_row_refuses_before_vector_growth() {
        let sketch = SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        let id = SketchEntityId::mint("creo:featdefs:sketch_entity#5:7").expect("valid entity ID");
        let geometry = SketchGeometry::native(
            cadmpeg_core::text::NonBlankString::new("line").expect("valid kind"),
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
        policy.limits.max_collection_items = 1;
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
        policy.limits.max_retained_bytes = expected.len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(admitted_endpoint_refs(&ctx, &sketch, [7]),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "creo sketch point reference"));
        policy.limits.max_retained_bytes = DecodePolicy::service().limits.max_retained_bytes;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(admitted_endpoint_refs(&ctx, &sketch, [7]),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo section endpoint references"));
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        assert_eq!(
            admitted_endpoint_refs(&ctx, &sketch, [7]).expect("service endpoint"),
            [expected]
        );
    }
}
