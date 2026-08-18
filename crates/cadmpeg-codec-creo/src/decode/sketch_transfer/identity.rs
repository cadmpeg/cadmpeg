// SPDX-License-Identifier: Apache-2.0
//! External ids and saved-section entity identity.

use super::super::sketch::saved_section_entity_geometry;
use super::super::sketch_ids::{sketch_entity_id, sketch_identity_scope, sketch_native_ref};
use super::super::sweep::saved_spline_sketch_geometry;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn section_entity_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    let mut ids = unique_section_segment_external_ids(definition);
    let Some(order) = &definition.order_table else {
        return ids;
    };
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
    let unique_saved_ids = unique_saved_section_internal_ids(definition);
    ids.extend(
        semantic_saved_section_entities(definition)
            .filter_map(|entity| saved_section_entity_identity(entity).0)
            .filter_map(|internal_id| {
                saved_section_external_id(
                    order,
                    &unique_saved_ids,
                    &ambiguous_segment_ids,
                    internal_id,
                )
            }),
    );
    ids
}

pub(in super::super) fn section_segment_external_id_counts(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, usize> {
    definition
        .segments
        .as_ref()
        .map_or_else(BTreeMap::new, |table| {
            table
                .rows
                .iter()
                .map(|row| row.external_id)
                .chain(table.circle_rows.iter().map(|row| row.external_id))
                .chain(table.point_rows.iter().map(|row| row.external_id))
                .chain(table.centered_line_rows.iter().map(|row| row.external_id))
                .chain(table.reference_line_rows.iter().map(|row| row.external_id))
                .chain(table.bounded_curve_rows.iter().map(|row| row.external_id))
                .chain(table.conic_rows.iter().map(|row| row.external_id))
                .chain(table.opaque_rows.iter().map(|row| row.external_id))
                .fold(BTreeMap::new(), |mut counts, external_id| {
                    *counts.entry(external_id).or_insert(0) += 1;
                    counts
                })
        })
}

/// A saved-section entity may stand in for one opaque segment row, but it
/// must not override a decoded segment family with a different identity.
pub(in super::super) fn saved_section_entity_fallback_allowed(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> bool {
    let Some(segments) = definition.segments.as_ref() else {
        return true;
    };
    let count = segments.external_id_count(external_id);
    count == 0
        || (count == 1
            && segments
                .opaque_rows
                .iter()
                .any(|segment| segment.external_id == external_id))
}

pub(in super::super) fn unique_section_segment_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    section_segment_external_id_counts(definition)
        .into_iter()
        .filter_map(|(external_id, count)| (count == 1).then_some(external_id))
        .collect()
}

pub(in super::super) fn ambiguous_section_segment_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    section_segment_external_id_counts(definition)
        .into_iter()
        .filter_map(|(external_id, count)| (count > 1).then_some(external_id))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum SavedSectionEntityKind {
    Line,
    Arc,
    Circle,
    Conic,
    Spline,
    Dummy,
}

impl SavedSectionEntityKind {
    pub(in super::super) const fn name(self) -> &'static str {
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

pub(in super::super) fn saved_section_entity_identity(
    entity: &crate::feature::FeatureSavedEntity,
) -> (Option<u32>, usize, SavedSectionEntityKind) {
    match entity {
        crate::feature::FeatureSavedEntity::Line(line) => (
            Some(line.entity_id),
            line.offset,
            SavedSectionEntityKind::Line,
        ),
        crate::feature::FeatureSavedEntity::Arc(arc) => {
            (Some(arc.entity_id), arc.offset, SavedSectionEntityKind::Arc)
        }
        crate::feature::FeatureSavedEntity::Circle(circle) => (
            Some(circle.entity_id),
            circle.offset,
            SavedSectionEntityKind::Circle,
        ),
        crate::feature::FeatureSavedEntity::Conic(conic) => (
            Some(conic.entity_id),
            conic.offset,
            SavedSectionEntityKind::Conic,
        ),
        crate::feature::FeatureSavedEntity::Spline(spline) => (
            spline.entity_id,
            spline.offset,
            SavedSectionEntityKind::Spline,
        ),
        crate::feature::FeatureSavedEntity::Dummy(dummy) => {
            (dummy.entity_id, dummy.offset, SavedSectionEntityKind::Dummy)
        }
    }
}

pub(in super::super) fn unresolved_saved_section_entity(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
    saved: &crate::feature::FeatureSavedEntity,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
) -> (SketchEntity, usize) {
    let (internal_id, offset, kind) = saved_section_entity_identity(saved);
    let unique_internal_id = internal_id.is_some_and(|id| unique_saved_ids.contains(&id));
    let external_id = if unique_internal_id {
        definition.order_table.as_ref().and_then(|order| {
            saved_section_external_id(order, unique_saved_ids, ambiguous_segment_ids, internal_id?)
        })
    } else {
        None
    };
    let suffix = if unique_internal_id {
        external_id.map_or_else(
            || {
                let internal_id = internal_id.expect("unique saved entity has an id");
                match kind {
                    SavedSectionEntityKind::Spline | SavedSectionEntityKind::Dummy => {
                        internal_id.to_string()
                    }
                    _ => format!("saved{internal_id}"),
                }
            },
            |external_id| external_id.to_string(),
        )
    } else {
        format!("saved:offset:{offset}")
    };
    let id = external_id.map_or_else(
        || match kind {
            SavedSectionEntityKind::Spline => SketchEntityId(format!(
                "creo:featdefs:saved_spline#{}:{suffix}",
                sketch_identity_scope(sketch)
            )),
            SavedSectionEntityKind::Dummy => SketchEntityId(format!(
                "creo:featdefs:saved_dummy#{}:{suffix}",
                sketch_identity_scope(sketch)
            )),
            _ => sketch_entity_id(sketch, &suffix),
        },
        |external_id| sketch_entity_id(sketch, external_id),
    );
    (
        SketchEntity {
            id,
            sketch: sketch.clone(),
            construction: true,
            native_ref: Some(sketch_native_ref(sketch)),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: format!("saved_{}", kind.name()),
            },
        },
        offset,
    )
}

pub(in super::super) fn unique_saved_section_internal_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    semantic_saved_section_entities(definition)
        .filter_map(|entity| saved_section_entity_identity(entity).0)
        .fold(BTreeMap::new(), |mut counts, internal_id| {
            *counts.entry(internal_id).or_insert(0usize) += 1;
            counts
        })
        .into_iter()
        .filter_map(|(internal_id, count)| (count == 1).then_some(internal_id))
        .collect()
}

pub(in super::super) fn saved_section_entity_is_elided_prototype(
    definition: &crate::feature::FeatureDefinition,
    entity: &crate::feature::FeatureSavedEntity,
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
            crate::feature::saved_entity_offset(entity) == saved.offset
                && saved.entities.iter().any(|candidate| {
                    crate::feature::saved_entity_offset(candidate) > saved.offset
                        && saved_section_entity_identity(candidate).0 == Some(internal_id)
                })
        })
}

pub(in super::super) fn semantic_saved_section_entities(
    definition: &crate::feature::FeatureDefinition,
) -> impl Iterator<Item = &crate::feature::FeatureSavedEntity> {
    definition
        .saved_section
        .iter()
        .flat_map(|saved| &saved.entities)
        .filter(|entity| !saved_section_entity_is_elided_prototype(definition, entity))
}

pub(in super::super) fn materialized_saved_section_external_ids(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeSet<u32> {
    let unique_saved_ids = unique_saved_section_internal_ids(definition);
    let ambiguous_segment_ids = ambiguous_section_segment_external_ids(definition);
    semantic_saved_section_entities(definition)
        .filter_map(|entity| {
            match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => {
                    saved_spline_sketch_geometry(spline)?;
                }
                _ => {
                    saved_section_entity_geometry(entity)?;
                }
            }
            let internal_id = saved_section_entity_identity(entity).0?;
            unique_saved_ids.contains(&internal_id).then_some(())?;
            definition.order_table.as_ref().and_then(|order| {
                saved_section_external_id(
                    order,
                    &unique_saved_ids,
                    &ambiguous_segment_ids,
                    internal_id,
                )
            })
        })
        .collect()
}

pub(in super::super) fn saved_section_external_id(
    order: &crate::feature::FeatureOrderTable,
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
    segment: &crate::feature::FeatureSegment,
) -> String {
    if unique_external_ids.contains(&segment.external_id) {
        segment.external_id.to_string()
    } else {
        format!("offset:{}", segment.offset)
    }
}

pub(in super::super) fn opaque_section_segment_identity_suffix(
    unique_external_ids: &BTreeSet<u32>,
    segment: &crate::feature::FeatureOpaqueSegment,
) -> String {
    if unique_external_ids.contains(&segment.external_id) {
        segment.external_id.to_string()
    } else {
        format!("opaque:offset:{}", segment.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::saved_section_entity_fallback_allowed;

    fn definition(
        segments: Option<crate::feature::FeatureSegmentTable>,
    ) -> crate::feature::FeatureDefinition {
        crate::feature::FeatureDefinition {
            id: 917,
            owner_feature_id: None,
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

    fn segment_table() -> crate::feature::FeatureSegmentTable {
        crate::feature::FeatureSegmentTable {
            declared_count: 0,
            has_elided_prototype: false,
            entity_ref: None,
            rows: Vec::new(),
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }
    }

    fn ordinary_line(external_id: u32) -> crate::feature::FeatureSegment {
        crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: external_id as usize,
        }
    }

    fn circle(external_id: u32) -> crate::feature::FeatureCircleSegment {
        crate::feature::FeatureCircleSegment {
            center_id: 1,
            radius_ref: 2,
            external_id,
            offset: external_id as usize,
        }
    }

    fn opaque(external_id: u32) -> crate::feature::FeatureOpaqueSegment {
        crate::feature::FeatureOpaqueSegment {
            kind: 25,
            directions: [None; 3],
            point_ids: [None; 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: external_id as usize,
        }
    }

    #[test]
    fn saved_fallback_requires_absent_or_unique_opaque_identity() {
        assert!(saved_section_entity_fallback_allowed(&definition(None), 7));
        assert!(saved_section_entity_fallback_allowed(
            &definition(Some(segment_table())),
            7
        ));

        let mut unique_opaque = segment_table();
        unique_opaque.opaque_rows.push(opaque(7));
        assert!(saved_section_entity_fallback_allowed(
            &definition(Some(unique_opaque)),
            7
        ));

        let mut ordinary = segment_table();
        ordinary.rows.push(ordinary_line(7));
        assert!(!saved_section_entity_fallback_allowed(
            &definition(Some(ordinary)),
            7
        ));

        let mut special = segment_table();
        special.circle_rows.push(circle(7));
        assert!(!saved_section_entity_fallback_allowed(
            &definition(Some(special)),
            7
        ));

        let mut cross_family = segment_table();
        cross_family.opaque_rows.push(opaque(7));
        cross_family.rows.push(ordinary_line(7));
        assert!(!saved_section_entity_fallback_allowed(
            &definition(Some(cross_family)),
            7
        ));

        let mut duplicate_opaque = segment_table();
        duplicate_opaque.opaque_rows.extend([opaque(7), opaque(7)]);
        assert!(!saved_section_entity_fallback_allowed(
            &definition(Some(duplicate_opaque)),
            7
        ));
    }
}
