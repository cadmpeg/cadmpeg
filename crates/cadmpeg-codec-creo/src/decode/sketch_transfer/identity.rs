// SPDX-License-Identifier: Apache-2.0
//! External ids and saved-section entity identity.

use super::super::sketch::geometry::saved_section_entity_geometry;
use super::super::sketch_ids::{
    sketch_entity_id_admitted, sketch_identity_scope, sketch_native_ref_admitted,
};
use super::super::sweep::nurbs::saved_spline_sketch_geometry;
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

pub(in super::super) fn section_entity_external_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut identity_storage = ctx.reserve_scoped(0, "creo saved identity scratch storage")?;
    let mut ids = unique_section_segment_external_ids(ctx, definition)?;
    let Some(order) = &definition.order_table else {
        return Ok(ids);
    };
    let ambiguous_segment_ids = identity_storage
        .with_storage(|| ambiguous_section_segment_external_ids(ctx, definition))?;
    let unique_saved_ids =
        identity_storage.with_storage(|| unique_saved_section_internal_ids(ctx, definition))?;
    let ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
        std::convert::Infallible,
    >(ctx, definition, |entity| {
        let Some(internal_id) = saved_section_entity_identity(entity).0 else {
            return Ok(ControlFlow::Continue(()));
        };
        let Some(external_id) = saved_section_external_id(
            ctx,
            order,
            &unique_saved_ids,
            &ambiguous_segment_ids,
            internal_id,
        )?
        else {
            return Ok(ControlFlow::Continue(()));
        };
        ctx.insert_btree_set(
            &mut ids,
            external_id,
            "creo section entity external ID nodes",
        )?;
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(ids)
}

/// A saved-section entity may stand in for one opaque segment row, but it
/// must not override a decoded segment family with a different identity.
pub(in super::super) fn saved_section_entity_fallback_allowed(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> bool {
    let Some(segments) = definition.segments.as_ref() else {
        return true;
    };
    !segments.rows.contains_id(external_id)
        || matches!(segments.rows.get(external_id), Some(SegmentRow::Opaque(_)))
}

/// A saved line or arc may reconcile one ordinary row, but not a different
/// decoded segment family carrying the same external identifier.
pub(in super::super) fn saved_section_ordinary_geometry_allowed(
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> bool {
    let Some(segments) = definition.segments.as_ref() else {
        return true;
    };
    !segments.rows.contains_id(segment.external_id)
        || matches!(segments.rows.get(segment.external_id), Some(SegmentRow::Ordinary(candidate))
            if std::mem::discriminant(&candidate.kind) == std::mem::discriminant(&segment.kind))
}

/// A saved line may supply an axis witness only for an absent or uniquely
/// identified ordinary line row.  Special-family and conflicting identities
/// remain unresolved.
pub(in super::super) fn saved_section_line_witness_allowed(
    definition: &crate::feature::definitions::FeatureDefinition,
    external_id: u32,
) -> bool {
    let Some(segments) = definition.segments.as_ref() else {
        return true;
    };
    !segments.rows.contains_id(external_id)
        || matches!(segments.rows.get(external_id), Some(SegmentRow::Opaque(_)))
        || matches!(segments.rows.get(external_id), Some(SegmentRow::Ordinary(segment))
            if matches!(segment.kind, crate::feature::definitions::FeatureSegmentKind::Line(_)))
}

pub(in super::super) fn unique_section_segment_external_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    if let Some(table) = definition.segments.as_ref() {
        for (&id, ordinal) in ctx.admit_iter(
            table.rows.identity_entries(),
            "creo unique section segment identity rows",
        )? {
            if ordinal.is_some() {
                ctx.insert_btree_set(&mut ids, id, "creo unique section segment ID nodes")?;
            }
        }
    }
    Ok(ids)
}

pub(in super::super) fn ambiguous_section_segment_external_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    if let Some(table) = definition.segments.as_ref() {
        for (&id, ordinal) in ctx.admit_iter(
            table.rows.identity_entries(),
            "creo conflicting section segment identity rows",
        )? {
            if ordinal.is_none() {
                ctx.insert_btree_set(&mut ids, id, "creo ambiguous section segment ID nodes")?;
            }
        }
    }
    Ok(ids)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SavedSectionEntityKind {
    Line,
    Arc,
    Circle,
    Conic,
    Spline,
    Dummy,
}

impl SavedSectionEntityKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Arc => "arc",
            Self::Circle => "circle",
            Self::Conic => "conic",
            Self::Spline => "spline",
            Self::Dummy => "dummy",
        }
    }
}

fn saved_section_entity_identity(
    entity: &crate::feature::definitions::FeatureSavedEntity,
) -> (Option<u32>, usize, SavedSectionEntityKind) {
    match entity {
        crate::feature::definitions::FeatureSavedEntity::Line(line) => (
            Some(line.entity_id),
            line.offset,
            SavedSectionEntityKind::Line,
        ),
        crate::feature::definitions::FeatureSavedEntity::Arc(arc) => {
            (Some(arc.entity_id), arc.offset, SavedSectionEntityKind::Arc)
        }
        crate::feature::definitions::FeatureSavedEntity::Circle(circle) => (
            Some(circle.entity_id),
            circle.offset,
            SavedSectionEntityKind::Circle,
        ),
        crate::feature::definitions::FeatureSavedEntity::Conic(conic) => (
            Some(conic.entity_id),
            conic.offset,
            SavedSectionEntityKind::Conic,
        ),
        crate::feature::definitions::FeatureSavedEntity::Spline(spline) => (
            spline.entity_id,
            spline.offset,
            SavedSectionEntityKind::Spline,
        ),
        crate::feature::definitions::FeatureSavedEntity::Dummy(dummy) => {
            (dummy.entity_id, dummy.offset, SavedSectionEntityKind::Dummy)
        }
    }
}

pub(in super::super) fn unresolved_saved_section_entity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    saved: &crate::feature::definitions::FeatureSavedEntity,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
) -> Result<Option<(SketchEntity, usize)>, cadmpeg_core::CodecError> {
    let (internal_id, offset, kind) = saved_section_entity_identity(saved);
    let unique_internal_id = match internal_id {
        Some(id)
            if ctx.contains_btree_set(
                unique_saved_ids,
                &id,
                "creo saved entity identity lookup",
            )? =>
        {
            Some(id)
        }
        _ => None,
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
    let mut suffix_storage = ctx.reserve_scoped(0, "creo unresolved saved suffix storage")?;
    let suffix = suffix_storage.with_storage(|| {
        if let Some(internal_id) = unique_internal_id {
            match external_id {
                Some(external_id) => ctx.format_retained(
                    format_args!("{external_id}"),
                    "creo unresolved saved entity suffix",
                ),
                None => match kind {
                    SavedSectionEntityKind::Spline | SavedSectionEntityKind::Dummy => ctx
                        .format_retained(
                            format_args!("{internal_id}"),
                            "creo unresolved saved entity suffix",
                        ),
                    _ => ctx.format_retained(
                        format_args!("saved{internal_id}"),
                        "creo unresolved saved entity suffix",
                    ),
                },
            }
        } else {
            ctx.format_retained(
                format_args!("saved:offset:{offset}"),
                "creo unresolved saved entity suffix",
            )
        }
    })?;
    let id = if external_id.is_some() {
        sketch_entity_id_admitted(ctx, sketch, &suffix)?
    } else {
        match kind {
            SavedSectionEntityKind::Spline | SavedSectionEntityKind::Dummy => {
                let namespace = if matches!(kind, SavedSectionEntityKind::Spline) {
                    &crate::identity::FEATDEFS_SAVED_SPLINE
                } else {
                    &crate::identity::FEATDEFS_SAVED_DUMMY
                };
                let text = ctx.format_retained(
                    format_args!(
                        "{}:{}:{}#{}:{suffix}",
                        namespace.format(),
                        namespace.scope(),
                        namespace.kind(),
                        sketch_identity_scope(sketch)
                    ),
                    "creo unresolved saved entity identity",
                )?;
                SketchEntityId::try_from(text).ok()
            }
            _ => sketch_entity_id_admitted(ctx, sketch, &suffix)?,
        }
    };
    let Some(id) = id else {
        return Ok(None);
    };
    let native_kind = ctx.format_retained(
        format_args!("saved_{}", kind.name()),
        "creo unresolved saved native kind",
    )?;
    Ok(Some((
        SketchEntity::new(
            id,
            sketch.try_clone_for_decode(ctx, "creo unresolved saved sketch identity")?,
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    native_kind,
                    "validate nonblank text",
                )?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("saved native kind must not be empty")
                })?,
            ),
        )
        .with_construction(true)
        .with_native_ref(Some(sketch_native_ref_admitted(ctx, sketch)?)),
        offset,
    )))
}

pub(in super::super) fn unique_saved_section_internal_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut count_storage = ctx.reserve_scoped(0, "creo saved identity count storage")?;
    let mut counts = BTreeMap::new();
    let ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
        std::convert::Infallible,
    >(ctx, definition, |entity| {
        let Some(internal_id) = saved_section_entity_identity(entity).0 else {
            return Ok(ControlFlow::Continue(()));
        };
        *count_storage
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut counts,
                    internal_id,
                    "creo saved section ID count nodes",
                )
            })?
            .or_insert(0usize) += 1;
        Ok(ControlFlow::Continue(()))
    })?;
    let mut ids = BTreeSet::new();
    for (&internal_id, &count) in ctx.admit_iter(&counts, "creo saved section identity counts")? {
        if count == 1 {
            ctx.insert_btree_set(&mut ids, internal_id, "creo unique saved section ID nodes")?;
        }
    }
    Ok(ids)
}

pub(in super::super) fn saved_section_entity_by_internal_id<'definition>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'definition crate::feature::definitions::FeatureDefinition,
    internal_id: u32,
) -> Result<
    Option<&'definition crate::feature::definitions::FeatureSavedEntity>,
    cadmpeg_core::CodecError,
> {
    let mut selected = None;
    let outcome = visit_semantic_saved_section_entities(ctx, definition, |entity| {
        if saved_section_entity_identity(entity).0 == Some(internal_id) {
            if selected.is_some() {
                return Ok(ControlFlow::Break(()));
            }
            selected = Some(entity);
        }
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(match outcome {
        ControlFlow::Break(()) => None,
        ControlFlow::Continue(()) => selected,
    })
}

fn saved_section_entity_is_elided_prototype(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    entity: &crate::feature::definitions::FeatureSavedEntity,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(internal_id) = saved_section_entity_identity(entity).0 else {
        return Ok(false);
    };
    let has_prototype = definition
        .segments
        .as_ref()
        .is_some_and(|segments| segments.has_elided_prototype)
        && definition
            .order_table
            .as_ref()
            .is_some_and(|order| order.has_prototype);
    if !has_prototype {
        return Ok(false);
    }
    let Some(saved) = definition
        .saved_section
        .as_ref()
        .filter(|saved| crate::feature::definitions::saved_entity_offset(entity) == saved.offset)
    else {
        return Ok(false);
    };
    ctx.any_by(
        &saved.entities,
        |candidate| {
            Ok(
                crate::feature::definitions::saved_entity_offset(candidate) > saved.offset
                    && saved_section_entity_identity(candidate).0 == Some(internal_id),
            )
        },
        "creo saved section prototype rows",
    )
}

pub(in super::super) fn visit_semantic_saved_section_entities<'definition, B>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'definition crate::feature::definitions::FeatureDefinition,
    mut visit: impl FnMut(
        &'definition crate::feature::definitions::FeatureSavedEntity,
    ) -> Result<ControlFlow<B>, cadmpeg_core::CodecError>,
) -> Result<ControlFlow<B>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(saved) = definition
        .saved_section
        .as_ref()
        .filter(|saved| !saved.entities.is_empty())
    else {
        return Ok(ControlFlow::Continue(()));
    };
    let found = ctx.find_map(
        &saved.entities,
        |entity| {
            if saved_section_entity_is_elided_prototype(ctx, definition, entity)? {
                return Ok(None);
            }
            Ok(match visit(entity)? {
                ControlFlow::Break(value) => Some(value),
                ControlFlow::Continue(()) => None,
            })
        },
        "creo saved section entities",
    )?;
    Ok(found.map_or(ControlFlow::Continue(()), ControlFlow::Break))
}

/// The saved-section entities that are not elided prototypes, in stored order,
/// for callers that visit every one. The list lives in scoped storage.
pub(in super::super) fn semantic_saved_section_entities<'ctx, 'definition>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'definition crate::feature::definitions::FeatureDefinition,
) -> Result<
    (
        Vec<&'definition crate::feature::definitions::FeatureSavedEntity>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    const OPERATION: &str = "creo saved section entities";
    let mut reservation = ctx.reserve_scoped(0, OPERATION)?;
    let mut entities = Vec::new();
    let Some(saved) = definition.saved_section.as_ref() else {
        return Ok((entities, reservation));
    };
    for entity in ctx.admit_iter(&saved.entities, OPERATION)? {
        if !saved_section_entity_is_elided_prototype(ctx, definition, entity)? {
            ctx.push_scoped_vec(&mut reservation, &mut entities, entity, OPERATION)?;
        }
    }
    Ok((entities, reservation))
}

pub(in super::super) fn materialized_saved_section_external_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut identity_storage = ctx.reserve_scoped(0, "creo saved identity scratch storage")?;
    let unique_saved_ids =
        identity_storage.with_storage(|| unique_saved_section_internal_ids(ctx, definition))?;
    let ambiguous_segment_ids = identity_storage
        .with_storage(|| ambiguous_section_segment_external_ids(ctx, definition))?;
    let mut external_ids = BTreeSet::new();
    let ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
        std::convert::Infallible,
    >(ctx, definition, |entity| {
        let materializes = match entity {
            crate::feature::definitions::FeatureSavedEntity::Spline(spline) => {
                let mut spline_storage =
                    ctx.reserve_scoped(0, "creo saved spline identity geometry")?;
                spline_storage
                    .with_storage(|| saved_spline_sketch_geometry(ctx, spline, refusal))?
                    .is_some()
            }
            _ => saved_section_entity_geometry(entity).is_some(),
        };
        if !materializes {
            return Ok(ControlFlow::Continue(()));
        }
        let Some(internal_id) = saved_section_entity_identity(entity).0 else {
            return Ok(ControlFlow::Continue(()));
        };
        let Some(order) = definition.order_table.as_ref() else {
            return Ok(ControlFlow::Continue(()));
        };
        if let Some(external_id) = saved_section_external_id(
            ctx,
            order,
            &unique_saved_ids,
            &ambiguous_segment_ids,
            internal_id,
        )? {
            ctx.insert_btree_set(
                &mut external_ids,
                external_id,
                "creo materialized saved-section external ID nodes",
            )?;
        }
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(external_ids)
}

pub(in super::super) fn saved_section_external_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    order: &crate::feature::definitions::FeatureOrderTable,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
    internal_id: u32,
) -> Result<Option<u32>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "creo saved section external ID lookup";
    if !ctx.contains_btree_set(unique_saved_ids, &internal_id, OPERATION)? {
        return Ok(None);
    }
    let Some(external_id) = order.external_id(internal_id) else {
        return Ok(None);
    };
    Ok(
        (!ctx.contains_btree_set(ambiguous_segment_ids, &external_id, OPERATION)?)
            .then_some(external_id),
    )
}

#[cfg(test)]
pub(in super::super) fn section_segment_identity_suffix(
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> String {
    if unique_external_ids.contains(&segment.external_id) {
        segment.external_id.to_string()
    } else {
        format!("offset:{}", segment.offset)
    }
}

pub(in super::super) fn section_segment_identity_suffix_admitted(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<String, cadmpeg_core::CodecError> {
    if ctx.contains_btree_set(
        unique_external_ids,
        &segment.external_id,
        "creo section suffix identity membership",
    )? {
        ctx.format_retained(
            format_args!("{}", segment.external_id),
            "creo section entity suffix",
        )
    } else {
        ctx.format_retained(
            format_args!("offset:{}", segment.offset),
            "creo section entity suffix",
        )
    }
}

pub(super) fn opaque_section_segment_identity_suffix_admitted(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::definitions::FeatureOpaqueSegment,
) -> Result<String, cadmpeg_core::CodecError> {
    if ctx.contains_btree_set(
        unique_external_ids,
        &segment.external_id,
        "creo section suffix identity membership",
    )? {
        ctx.format_retained(
            format_args!("{}", segment.external_id),
            "creo opaque entity suffix",
        )
    } else {
        ctx.format_retained(
            format_args!("opaque:offset:{}", segment.offset),
            "creo opaque entity suffix",
        )
    }
}

#[cfg(test)]
mod tests {
    mod admission_recovery;
    mod record_lookup;
    mod spline_storage;
    use super::{
        saved_section_entity_fallback_allowed, saved_section_line_witness_allowed,
        saved_section_ordinary_geometry_allowed,
    };
    use crate::decode::tests::opaque;

    #[test]
    fn semantic_saved_entity_scan_refuses_before_visitor_callback() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let definition = saved_line_definition();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut visited = false;
        let error = super::visit_semantic_saved_section_entities::<()>(&ctx, &definition, |_| {
            visited = true;
            Ok(std::ops::ControlFlow::Continue(()))
        })
        .expect_err("saved entity scan needs work");
        assert!(!visited);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo saved section entities")
        );
    }

    #[test]
    fn saved_identity_uniqueness_stops_after_second_match() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut definition = saved_line_definition();
        definition.segments = Some(segment_table());
        definition
            .segments
            .as_mut()
            .expect("segments")
            .has_elided_prototype = true;
        definition
            .order_table
            .as_mut()
            .expect("order")
            .has_prototype = true;
        let saved = definition.saved_section.as_mut().expect("saved section");
        saved.offset = 9;
        let dummy = |entity_id, offset| {
            crate::feature::definitions::FeatureSavedEntity::Dummy(
                crate::feature::definitions::FeatureSavedDummy {
                    entity_id: Some(entity_id),
                    body: Vec::new(),
                    offset,
                },
            )
        };
        saved.entities = vec![dummy(3, 0), dummy(3, 1), dummy(4, 9), dummy(4, 10)];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let mut short = definition.clone();
        short
            .saved_section
            .as_mut()
            .expect("saved section")
            .entities
            .truncate(2);
        policy.limits.max_work_units = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            None,
            |cap| {
                let trial_arena = DecodeArena::new();
                let mut trial_policy = policy;
                trial_policy.limits.max_work_units = cap;
                let (trial_ctx, _) =
                    DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
                super::saved_section_entity_by_internal_id(&trial_ctx, &short, 3)
            },
        );
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(
            super::saved_section_entity_by_internal_id(&ctx, &definition, 3)
                .expect("uniqueness stops before the later prototype search")
                .is_none()
        );
    }

    #[test]
    fn unresolved_saved_dummy_refuses_each_field() {
        use cadmpeg_core::decode::ResourceDimension;

        let definition = definition(None);
        let saved = crate::feature::definitions::FeatureSavedEntity::Dummy(
            crate::feature::definitions::FeatureSavedDummy {
                entity_id: None,
                body: Vec::new(),
                offset: 9,
            },
        );
        let sketch =
            cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#5").expect("valid sketch ID");
        for (dimension, operation) in [
            (
                ResourceDimension::MaterializedBytes,
                "creo unresolved saved entity suffix",
            ),
            (
                ResourceDimension::RetainedBytes,
                "creo unresolved saved entity identity",
            ),
            (
                ResourceDimension::RetainedBytes,
                "creo unresolved saved native kind",
            ),
            (
                ResourceDimension::RetainedBytes,
                "creo unresolved saved sketch identity",
            ),
            (
                ResourceDimension::RetainedBytes,
                "creo sketch native reference",
            ),
        ] {
            let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
                super::unresolved_saved_section_entity(
                    ctx,
                    &definition,
                    &sketch,
                    &saved,
                    &std::collections::BTreeSet::new(),
                    &std::collections::BTreeSet::new(),
                )
            });
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.dimension == dimension && refusal.operation == operation)
            );
        }
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let service = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &service)
            .expect("empty root");
        let (entity, offset) = super::unresolved_saved_section_entity(
            &ctx,
            &definition,
            &sketch,
            &saved,
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
        )
        .expect("service admission")
        .expect("saved dummy entity");
        assert_eq!(offset, 9);
        assert_eq!(
            entity.id().as_str(),
            "creo:featdefs:saved_dummy#5:saved:offset:9"
        );
    }

    #[test]
    fn section_entity_suffix_refuses_before_retained_formatting() {
        let segment = crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 9,
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
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
                super::section_segment_identity_suffix_admitted(
                    &ctx,
                    &std::collections::BTreeSet::new(),
                    &segment,
                )
            },
        );
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root");
        assert!(
            matches!(super::section_segment_identity_suffix_admitted(&ctx, &std::collections::BTreeSet::new(), &segment),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && refusal.operation == "creo section entity suffix")
        );
        let service = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &service)
            .expect("empty root");
        assert_eq!(
            super::section_segment_identity_suffix_admitted(
                &ctx,
                &std::collections::BTreeSet::new(),
                &segment
            )
            .expect("service suffix"),
            "offset:9"
        );
    }

    #[test]
    fn opaque_entity_suffix_refuses_before_retained_formatting() {
        let segment = opaque(42);
        let expected = format!("opaque:offset:{}", segment.offset);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo opaque entity suffix"),
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
                super::opaque_section_segment_identity_suffix_admitted(
                    &ctx,
                    &std::collections::BTreeSet::new(),
                    &segment,
                )
            },
        );
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root");
        assert!(
            matches!(super::opaque_section_segment_identity_suffix_admitted(&ctx, &std::collections::BTreeSet::new(), &segment),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && refusal.operation == "creo opaque entity suffix")
        );
        let service = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &service)
            .expect("empty root");
        assert_eq!(
            super::opaque_section_segment_identity_suffix_admitted(
                &ctx,
                &std::collections::BTreeSet::new(),
                &segment
            )
            .expect("service suffix"),
            expected
        );
    }

    fn definition(
        segments: Option<crate::feature::definitions::FeatureSegmentTable>,
    ) -> crate::feature::definitions::FeatureDefinition {
        crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        }
    }

    fn segment_table() -> crate::feature::definitions::FeatureSegmentTable {
        crate::feature::definitions::FeatureSegmentTable {
            declared_count: 0,
            has_elided_prototype: false,
            entity_ref: None,
            rows: crate::feature::segment_rows::SegmentRows::default(),
            offset: 0,
        }
    }

    fn ordinary_line(external_id: u32) -> crate::feature::definitions::FeatureSegment {
        crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line([1, 2]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: usize::try_from(external_id).expect("fixture index fits usize"),
        }
    }

    fn circle(external_id: u32) -> crate::feature::definitions::FeatureCircleSegment {
        crate::feature::definitions::FeatureCircleSegment {
            center_id: 1,
            radius_ref: 2,
            external_id,
            offset: usize::try_from(external_id).expect("fixture index fits usize"),
        }
    }

    fn saved_line_definition() -> crate::feature::definitions::FeatureDefinition {
        let mut definition = definition(None);
        definition.order_table = Some(crate::feature::definitions::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::definitions::FeatureOrderRow {
                external_id: 42,
                internal_id: 3,
                bitmask: 0,
                offset: 0,
            }]
            .into(),
            offset: 0,
        });
        definition.saved_section = Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Line(
                crate::feature::definitions::FeatureSavedLine {
                    entity_id: 3,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [[Some(0.0), Some(0.0), None], [Some(1.0), Some(0.0), None]],
                    body: Vec::new(),
                    offset: 0,
                },
            )],
            offset: 0,
        });
        definition
    }

    #[test]
    fn section_segment_id_sets_refuse_before_each_node() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut unique = definition(Some(segment_table()));
        unique.segments.as_mut().expect("segments").rows.insert(
            crate::feature::segment_rows::SegmentRow::Ordinary(ordinary_line(7)),
        );
        let mut ambiguous = unique.clone();
        ambiguous
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Circle(circle(7)));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let error = super::unique_section_segment_external_ids(&ctx, &unique)
            .expect_err("one unique ID exceeds zero nodes");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "creo unique section segment ID nodes"),
            "{error:?}"
        );
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let error = super::ambiguous_section_segment_external_ids(&ctx, &ambiguous)
            .expect_err("one ambiguous ID exceeds zero nodes");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "creo ambiguous section segment ID nodes"),
            "{error:?}"
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            assert_eq!(
                super::unique_section_segment_external_ids(ctx, &unique)
                    .expect("service unique IDs"),
                std::collections::BTreeSet::from([7])
            );
            assert_eq!(
                super::ambiguous_section_segment_external_ids(ctx, &ambiguous)
                    .expect("service ambiguous IDs"),
                std::collections::BTreeSet::from([7])
            );
        });
    }

    #[test]
    fn section_segment_identity_scan_refuses_before_tree_update() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let mut table = segment_table();
        table
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
                ordinary_line(7),
            ));
        let definition = definition(Some(table));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");

        let error = super::unique_section_segment_external_ids(&ctx, &definition)
            .expect_err("one identity row exceeds zero work units");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo unique section segment identity rows"),
            "{error:?}"
        );
    }

    #[test]
    fn saved_section_identity_nodes_refuse_at_count_result_and_external_id() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let definition = saved_line_definition();
        crate::test_support::assert_refusal_order(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            &[
                "creo saved section ID count nodes",
                "creo unique saved section ID nodes",
            ],
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                super::unique_saved_section_internal_ids(&ctx, &definition)
            },
        );
        let error = crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "creo section entity external ID nodes",
            |ctx| super::section_entity_external_ids(ctx, &definition),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "creo section entity external ID nodes"),
            "{error:?}"
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            assert_eq!(
                super::unique_saved_section_internal_ids(ctx, &definition)
                    .expect("service saved IDs"),
                std::collections::BTreeSet::from([3])
            );
            assert_eq!(
                super::section_entity_external_ids(ctx, &definition).expect("service external IDs"),
                std::collections::BTreeSet::from([42])
            );
        });
    }

    #[test]
    fn materialized_saved_section_external_id_refuses_before_set_node() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let definition = saved_line_definition();
        let arena = DecodeArena::new();
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service)
            .expect("empty root fits service policy");
        let ids = super::materialized_saved_section_external_ids(
            &ctx,
            &definition,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("one ID fits service policy");
        assert_eq!(ids, std::collections::BTreeSet::from([42]));

        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::CollectionItems,
            "creo materialized saved-section external ID nodes",
            |ctx| {
                super::materialized_saved_section_external_ids(
                    ctx,
                    &definition,
                    &mut crate::lane_refusal::LaneRefusals::new(),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo materialized saved-section external ID nodes")
        );
    }

    #[test]
    fn saved_fallback_requires_absent_or_unique_opaque_identity() {
        assert!(saved_section_entity_fallback_allowed(&definition(None), 7));
        assert!(saved_section_line_witness_allowed(&definition(None), 7));
        assert!(saved_section_entity_fallback_allowed(
            &definition(Some(segment_table())),
            7
        ));
        assert!(saved_section_line_witness_allowed(
            &definition(Some(segment_table())),
            7
        ));

        let mut unique_opaque = segment_table();
        unique_opaque
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Opaque(opaque(7)));
        assert!(saved_section_entity_fallback_allowed(
            &definition(Some(unique_opaque)),
            7
        ));

        let mut ordinary = segment_table();
        let line = ordinary_line(7);
        ordinary
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
                line.clone(),
            ));
        assert!(!saved_section_entity_fallback_allowed(
            &definition(Some(ordinary)),
            7
        ));

        let ordinary_definition = definition(Some({
            let mut segments = segment_table();
            segments
                .rows
                .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
                    line.clone(),
                ));
            segments
        }));
        assert!(saved_section_ordinary_geometry_allowed(
            &ordinary_definition,
            &line
        ));
        assert!(saved_section_line_witness_allowed(&ordinary_definition, 7));

        let mut ordinary_arc = segment_table();
        ordinary_arc
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
                crate::feature::definitions::FeatureSegment {
                    kind: crate::feature::definitions::FeatureSegmentKind::Arc(line.point_ids()),
                    ..line.clone()
                },
            ));
        assert!(!saved_section_line_witness_allowed(
            &definition(Some(ordinary_arc)),
            7
        ));

        let mut special = segment_table();
        special
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Circle(circle(7)));
        let special_definition = definition(Some(special));
        assert!(!saved_section_entity_fallback_allowed(
            &special_definition,
            7
        ));
        assert!(!saved_section_line_witness_allowed(&special_definition, 7));

        let mut cross_family_geometry = segment_table();
        cross_family_geometry
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
                line.clone(),
            ));
        cross_family_geometry
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Circle(circle(7)));
        assert!(!saved_section_ordinary_geometry_allowed(
            &definition(Some(cross_family_geometry.clone())),
            &line
        ));
        assert!(!saved_section_line_witness_allowed(
            &definition(Some(cross_family_geometry)),
            7
        ));

        let mut cross_family = segment_table();
        cross_family
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Opaque(opaque(7)));
        cross_family
            .rows
            .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
                ordinary_line(7),
            ));
        assert!(!saved_section_entity_fallback_allowed(
            &definition(Some(cross_family)),
            7
        ));

        let mut duplicate_opaque = segment_table();
        duplicate_opaque
            .rows
            .edit_opaque(|rows| rows.extend([opaque(7), opaque(7)]));
        assert!(!saved_section_entity_fallback_allowed(
            &definition(Some(duplicate_opaque)),
            7
        ));

        let mut duplicate_line = segment_table();
        duplicate_line
            .rows
            .edit_ordinary(|rows| rows.extend([line.clone(), line]));
        assert!(!saved_section_line_witness_allowed(
            &definition(Some(duplicate_line)),
            7
        ));
    }
}
