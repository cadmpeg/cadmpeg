// SPDX-License-Identifier: Apache-2.0
//! External ids and saved-section entity identity.

use super::super::sketch::geometry::saved_section_entity_geometry;
use super::super::sketch_ids::{sketch_entity_id, sketch_identity_scope, sketch_native_ref};
use super::super::sweep::nurbs::saved_spline_sketch_geometry;
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn section_entity_external_ids(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> BTreeSet<u32> {
    let mut ids = unique_section_segment_external_ids(definition);
    let Some(order) = &definition.order_table else {
        return ids;
    };
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
    let unique_saved_ids = unique_saved_section_internal_ids(definition);
    for external_id in semantic_saved_section_entities(definition)
        .filter_map(|entity| saved_section_entity_identity(entity).0)
        .filter_map(|internal_id| {
            saved_section_external_id(
                order,
                &unique_saved_ids,
                &ambiguous_segment_ids,
                internal_id,
            )
        })
    {
        ids.insert(external_id);
    }
    ids
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
    definition: &crate::feature::definitions::FeatureDefinition,
) -> BTreeSet<u32> {
    let mut ids = BTreeSet::new();
    for id in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.unique_ids())
    {
        ids.insert(id);
    }
    ids
}

pub(in super::super) fn ambiguous_section_segment_external_ids(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> BTreeSet<u32> {
    let mut ids = BTreeSet::new();
    for id in definition
        .segments
        .iter()
        .flat_map(|table| table.rows.conflicting_ids())
    {
        ids.insert(id);
    }
    ids
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
    definition: &crate::feature::definitions::FeatureDefinition,
    sketch: &SketchId,
    saved: &crate::feature::definitions::FeatureSavedEntity,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
) -> Result<Option<(SketchEntity, usize)>, cadmpeg_core::CodecError> {
    let (internal_id, offset, kind) = saved_section_entity_identity(saved);
    let unique_internal_id = internal_id.filter(|id| unique_saved_ids.contains(id));
    let external_id = if let Some(internal_id) = unique_internal_id {
        definition.order_table.as_ref().and_then(|order| {
            saved_section_external_id(order, unique_saved_ids, ambiguous_segment_ids, internal_id)
        })
    } else {
        None
    };
    let suffix = if let Some(internal_id) = unique_internal_id {
        match external_id {
            Some(external_id) => format!("{external_id}"),
            None => match kind {
                SavedSectionEntityKind::Spline | SavedSectionEntityKind::Dummy => {
                    format!("{internal_id}")
                }
                _ => format!("saved{internal_id}"),
            },
        }
    } else {
        format!("saved:offset:{offset}")
    };
    let id = if external_id.is_some() {
        sketch_entity_id(sketch, &suffix)
    } else {
        match kind {
            SavedSectionEntityKind::Spline | SavedSectionEntityKind::Dummy => {
                let namespace = if matches!(kind, SavedSectionEntityKind::Spline) {
                    &crate::identity::FEATDEFS_SAVED_SPLINE
                } else {
                    &crate::identity::FEATDEFS_SAVED_DUMMY
                };
                let text = format!(
                    "{}:{}:{}#{}:{suffix}",
                    namespace.format(),
                    namespace.scope(),
                    namespace.kind(),
                    sketch_identity_scope(sketch)
                );
                SketchEntityId::try_from(text).ok()
            }
            _ => sketch_entity_id(sketch, &suffix),
        }
    };
    let Some(id) = id else {
        return Ok(None);
    };
    let native_kind = format!("saved_{}", kind.name());
    Ok(Some((
        SketchEntity::new(
            id,
            sketch.clone(),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::new(native_kind).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("saved native kind must not be empty")
                })?,
            ),
        )
        .with_construction(true)
        .with_native_ref(Some(sketch_native_ref(sketch))),
        offset,
    )))
}

pub(in super::super) fn unique_saved_section_internal_ids(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> BTreeSet<u32> {
    let mut counts = BTreeMap::new();
    for internal_id in semantic_saved_section_entities(definition)
        .filter_map(|entity| saved_section_entity_identity(entity).0)
    {
        *counts.entry(internal_id).or_insert(0usize) += 1;
    }
    let mut ids = BTreeSet::new();
    for (internal_id, count) in counts {
        if count == 1 {
            ids.insert(internal_id);
        }
    }
    ids
}

pub(in super::super) fn saved_section_internal_id_is_unique(
    definition: &crate::feature::definitions::FeatureDefinition,
    internal_id: u32,
) -> bool {
    semantic_saved_section_entities(definition)
        .filter(|entity| saved_section_entity_identity(entity).0 == Some(internal_id))
        .take(2)
        .count()
        == 1
}

fn saved_section_entity_is_elided_prototype(
    definition: &crate::feature::definitions::FeatureDefinition,
    entity: &crate::feature::definitions::FeatureSavedEntity,
) -> bool {
    let Some(internal_id) = saved_section_entity_identity(entity).0 else {
        return false;
    };
    definition
        .segments
        .as_ref()
        .is_some_and(|segments| segments.has_elided_prototype)
        && definition
            .order_table
            .as_ref()
            .is_some_and(|order| order.has_prototype)
        && definition.saved_section.as_ref().is_some_and(|saved| {
            crate::feature::definitions::saved_entity_offset(entity) == saved.offset
                && saved.entities.iter().any(|candidate| {
                    crate::feature::definitions::saved_entity_offset(candidate) > saved.offset
                        && saved_section_entity_identity(candidate).0 == Some(internal_id)
                })
        })
}

pub(in super::super) fn semantic_saved_section_entities(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> impl Iterator<Item = &crate::feature::definitions::FeatureSavedEntity> {
    definition
        .saved_section
        .iter()
        .flat_map(|saved| &saved.entities)
        .filter(|entity| !saved_section_entity_is_elided_prototype(definition, entity))
}

pub(in super::super) fn materialized_saved_section_external_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let unique_saved_ids = unique_saved_section_internal_ids(definition);
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
    let mut external_ids = BTreeSet::new();
    for entity in semantic_saved_section_entities(definition) {
        let materializes = match entity {
            crate::feature::definitions::FeatureSavedEntity::Spline(spline) => {
                saved_spline_sketch_geometry(ctx, spline, refusal)?.is_some()
            }
            _ => saved_section_entity_geometry(entity).is_some(),
        };
        if !materializes {
            continue;
        }
        let Some(internal_id) = saved_section_entity_identity(entity).0 else {
            continue;
        };
        if !unique_saved_ids.contains(&internal_id) {
            continue;
        }
        if let Some(external_id) = definition.order_table.as_ref().and_then(|order| {
            saved_section_external_id(
                order,
                &unique_saved_ids,
                &ambiguous_segment_ids,
                internal_id,
            )
        }) {
            external_ids.insert(external_id);
        }
    }
    Ok(external_ids)
}

pub(in super::super) fn saved_section_external_id(
    order: &crate::feature::definitions::FeatureOrderTable,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
    internal_id: u32,
) -> Option<u32> {
    unique_saved_ids.contains(&internal_id).then_some(())?;
    let external_id = order.external_id(internal_id)?;
    (!ambiguous_segment_ids.contains(&external_id)).then_some(external_id)
}

pub(in super::super) fn section_segment_identity_suffix(
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> String {
    if unique_external_ids.contains(&segment.external_id) {
        format!("{}", segment.external_id)
    } else {
        format!("offset:{}", segment.offset)
    }
}

pub(super) fn opaque_section_segment_identity_suffix(
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::definitions::FeatureOpaqueSegment,
) -> String {
    if unique_external_ids.contains(&segment.external_id) {
        format!("{}", segment.external_id)
    } else {
        format!("opaque:offset:{}", segment.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        saved_section_entity_fallback_allowed, saved_section_line_witness_allowed,
        saved_section_ordinary_geometry_allowed,
    };
    use crate::decode::tests::opaque;

    #[test]
    fn unresolved_saved_dummy_takes_an_offset_identity() {
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
        let (entity, offset) = ({
            super::unresolved_saved_section_entity(
                &definition,
                &sketch,
                &saved,
                &std::collections::BTreeSet::new(),
                &std::collections::BTreeSet::new(),
            )
        })
        .expect("service admission")
        .expect("saved dummy entity");
        assert_eq!(offset, 9);
        assert_eq!(
            entity.id().as_str(),
            "creo:featdefs:saved_dummy#5:saved:offset:9"
        );
    }

    #[test]
    fn section_entity_suffix_without_a_unique_id_uses_the_offset() {
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
        assert_eq!(
            super::section_segment_identity_suffix(&std::collections::BTreeSet::new(), &segment),
            "offset:9"
        );
    }

    #[test]
    fn opaque_entity_suffix_without_a_unique_id_uses_the_offset() {
        let segment = opaque(42);
        let expected = format!("opaque:offset:{}", segment.offset);
        assert_eq!(
            super::opaque_section_segment_identity_suffix(
                &std::collections::BTreeSet::new(),
                &segment
            ),
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
            }],
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
    fn section_segment_id_sets_split_unique_and_ambiguous_ids() {
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
        assert_eq!(
            super::unique_section_segment_external_ids(&unique),
            std::collections::BTreeSet::from([7])
        );
        assert_eq!(
            super::ambiguous_section_segment_external_ids(&ambiguous),
            std::collections::BTreeSet::from([7])
        );
    }

    #[test]
    fn saved_section_ids_map_the_unique_internal_id_to_its_external_id() {
        let definition = saved_line_definition();
        assert_eq!(
            super::unique_saved_section_internal_ids(&definition),
            std::collections::BTreeSet::from([3])
        );
        assert_eq!(
            super::section_entity_external_ids(&definition),
            std::collections::BTreeSet::from([42])
        );
    }

    #[test]
    fn materialized_saved_section_external_ids_follow_the_order_table() {
        let definition = saved_line_definition();
        let ids = crate::decode::with_service_context(|ctx| {
            super::materialized_saved_section_external_ids(
                ctx,
                &definition,
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect("one ID fits service policy");
        assert_eq!(ids, std::collections::BTreeSet::from([42]));
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
