// SPDX-License-Identifier: Apache-2.0
//! Sketch native identity, table headers, and feature-definition record ids.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) fn feature_definition_has_sketch_design(
    definition: &crate::feature::FeatureDefinition,
) -> bool {
    definition.variables.is_some()
        || crate::feature::equation_table(&definition.body, 0, definition.body.len()).is_some()
        || definition.segments.is_some()
        || definition.trim_entities.is_some()
        || definition.trim_vertices.is_some()
        || definition.order_table.is_some()
        || definition.section_3d.is_some()
        || definition.saved_section.is_some()
        || definition.dimensions.is_some()
        || definition.relations.is_some()
}

pub(crate) fn sketch_table_headers(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<CreoSketchTableHeader> {
    let mut headers = Vec::new();
    let mut push = |kind, declared_count, entity_ref, entry_ref, buckets, row_count, offset| {
        headers.push(CreoSketchTableHeader {
            kind,
            declared_count,
            entity_ref,
            entry_ref,
            buckets,
            row_count,
            offset,
        });
    };
    if let Some(table) = &definition.variables {
        push(
            "variables",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = crate::feature::equation_table(&definition.body, 0, definition.body.len())
    {
        push(
            "equations",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.segments {
        push(
            "segments",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.retained_row_count(),
            table.offset,
        );
    }
    if let Some(table) = &definition.trim_entities {
        push(
            "trim_entities",
            table.declared_count,
            table.entity_ref,
            table.entry_ref,
            table
                .buckets
                .iter()
                .map(|bucket| CreoSketchBucketHeader {
                    index: bucket.index,
                    declared_entry_count: bucket.declared_entry_count,
                    decoded_entry_count: bucket.decoded_entry_count,
                    offset: bucket.offset,
                })
                .collect(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.trim_vertices {
        push(
            "trim_vertices",
            table.declared_count,
            table.entity_ref,
            table.entry_ref,
            table
                .buckets
                .iter()
                .map(|bucket| CreoSketchBucketHeader {
                    index: bucket.index,
                    declared_entry_count: bucket.declared_entry_count,
                    decoded_entry_count: bucket.decoded_entry_count,
                    offset: bucket.offset,
                })
                .collect(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.order_table {
        push(
            "order",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.dimensions {
        push(
            "dimensions",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.relations {
        push(
            "relations",
            Some(table.declared_count),
            table.entity_ref,
            None,
            Vec::new(),
            table.rows.len(),
            table.offset,
        );
        if let Some(header) = &table.skamp_header {
            push(
                "solver_incidences",
                Some(header.declared_count),
                Some(header.entity_ref),
                None,
                Vec::new(),
                table.skamps.len(),
                header.offset,
            );
        }
        if let Some(header) = &table.triples_header {
            push(
                "relation_triples",
                Some(header.declared_count),
                Some(header.entity_ref),
                None,
                Vec::new(),
                table.triples.len(),
                header.offset,
            );
        }
    }
    if let Some(table) = &definition.saved_section {
        push(
            "saved_entities",
            None,
            None,
            None,
            Vec::new(),
            table.entities.len(),
            table.offset,
        );
    }
    headers.sort_by_key(|header| header.offset);
    headers
}

pub(crate) fn binary_flag_value(flag: crate::feature::BinaryFlag) -> bool {
    match flag {
        crate::feature::BinaryFlag::Clear => false,
        crate::feature::BinaryFlag::Set => true,
    }
}

pub(crate) fn feature_definition_record_id(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
) -> String {
    if scan
        .features
        .definitions
        .iter()
        .filter(|candidate| candidate.id == definition.id)
        .count()
        != 1
        || (definition.id == 0 && definition.owner_feature_id.is_none())
    {
        format!(
            "creo:featdefs:feature_definition#offset:{}",
            definition.offset
        )
    } else {
        format!("creo:featdefs:feature_definition#{}", definition.id)
    }
}

pub(crate) fn feature_sketch_record_id_in_scan(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
) -> String {
    if scan
        .features
        .definitions
        .iter()
        .filter(|candidate| candidate.id == definition.id)
        .count()
        != 1
        || (definition.id == 0 && definition.owner_feature_id.is_none())
    {
        format!("creo:featdefs:sketch#offset:{}", definition.offset)
    } else {
        format!("creo:featdefs:sketch#{}", definition.id)
    }
}

pub(crate) fn model_sketch_id(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
) -> SketchId {
    let native_id = feature_sketch_record_id_in_scan(scan, definition);
    SketchId(native_id.replacen("creo:featdefs:sketch#", "creo:model:sketch#", 1))
}

pub(crate) fn sketch_identity_scope(sketch: &SketchId) -> &str {
    sketch
        .0
        .strip_prefix("creo:model:sketch#")
        .unwrap_or(&sketch.0)
}

pub(crate) fn sketch_entity_id(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> SketchEntityId {
    SketchEntityId(format!(
        "creo:featdefs:sketch_entity#{}:{suffix}",
        sketch_identity_scope(sketch)
    ))
}

pub(crate) fn sketch_constraint_id(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> SketchConstraintId {
    SketchConstraintId(format!(
        "creo:featdefs:sketch_constraint#{}:{suffix}",
        sketch_identity_scope(sketch)
    ))
}

pub(crate) fn sketch_native_ref(sketch: &SketchId) -> String {
    format!("creo:featdefs:sketch#{}", sketch_identity_scope(sketch))
}

pub(crate) fn sketch_section_curve_id(sketch: &SketchId, suffix: impl std::fmt::Display) -> String {
    format!(
        "creo:featdefs:section_curve#{}:{suffix}",
        sketch_identity_scope(sketch)
    )
}

pub(crate) fn sketch_point_ref(sketch: &SketchId, point: u32) -> String {
    format!("{}:point#{point}", sketch_native_ref(sketch))
}

pub(crate) fn sketch_feature_id(sketch: &SketchId) -> IrFeatureId {
    IrFeatureId(format!(
        "creo:model:sketch_feature#{}",
        sketch_identity_scope(sketch)
    ))
}

pub(crate) fn section_owner_feature_id(
    scan: &ContainerScan,
    definition_id: u32,
    sketch: &SketchId,
) -> IrFeatureId {
    owned_section_feature_id(scan, definition_id).map_or_else(
        || sketch_feature_id(sketch),
        |feature_id| IrFeatureId(format!("creo:model:feature#{feature_id}")),
    )
}

pub(crate) fn owning_feature_definition_ref(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<String> {
    let definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.owner_feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    Some(feature_definition_record_id(scan, definition))
}
