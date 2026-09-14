// SPDX-License-Identifier: Apache-2.0
//! Sketch native identity, table headers, and feature-definition record ids.

use cadmpeg_ir::features::FeatureId as IrFeatureId;
use cadmpeg_ir::ids::{CurveId, IdentityKey};
use cadmpeg_ir::sketches::{SketchConstraintId, SketchEntityId, SketchId};

use crate::container::ContainerScan;

use super::feature_history::owned_section_feature_id;
use super::native_records::{CreoSketchBucketHeader, CreoSketchTableHeader, CreoSketchTableKind};

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
    let mut push = |kind, row_count, offset| {
        headers.push(CreoSketchTableHeader {
            kind,
            row_count,
            offset,
        });
    };
    if let Some(table) = &definition.variables {
        push(
            CreoSketchTableKind::Variables {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = crate::feature::equation_table(&definition.body, 0, definition.body.len())
    {
        push(
            CreoSketchTableKind::Equations {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.segments {
        push(
            CreoSketchTableKind::Segments {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.trim_entities {
        push(
            CreoSketchTableKind::TrimEntities {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
                entry_ref: table.entry_ref,
                buckets: table
                    .buckets
                    .iter()
                    .map(|bucket| CreoSketchBucketHeader {
                        index: bucket.index,
                        declared_entry_count: bucket.declared_entry_count,
                        decoded_entry_count: bucket.decoded_entry_count,
                        offset: bucket.offset,
                    })
                    .collect(),
            },
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.trim_vertices {
        push(
            CreoSketchTableKind::TrimVertices {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
                entry_ref: table.entry_ref,
                buckets: table
                    .buckets
                    .iter()
                    .map(|bucket| CreoSketchBucketHeader {
                        index: bucket.index,
                        declared_entry_count: bucket.declared_entry_count,
                        decoded_entry_count: bucket.decoded_entry_count,
                        offset: bucket.offset,
                    })
                    .collect(),
            },
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.order_table {
        push(
            CreoSketchTableKind::Order {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.dimensions {
        push(
            CreoSketchTableKind::Dimensions {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        );
    }
    if let Some(table) = &definition.relations {
        push(
            CreoSketchTableKind::Relations {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        );
        if let Some(header) = table.skamps.as_ref().and_then(|table| table.header()) {
            push(
                CreoSketchTableKind::SolverIncidences {
                    declared_count: header.declared_count,
                    entity_ref: header.entity_ref,
                },
                table.skamps().len(),
                header.offset,
            );
        }
        if let Some(header) = table.triples.as_ref().and_then(|table| table.header()) {
            push(
                CreoSketchTableKind::RelationTriples {
                    declared_count: header.declared_count,
                    entity_ref: header.entity_ref,
                },
                table.triples().len(),
                header.offset,
            );
        }
    }
    if let Some(table) = &definition.saved_section {
        push(
            CreoSketchTableKind::SavedEntities,
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
        .filter(|candidate| candidate.identity.id() == definition.identity.id())
        .count()
        != 1
        || (definition.identity.schema_id().is_none()
            && definition.identity.owner_feature_id().is_none())
    {
        format!(
            "creo:featdefs:feature_definition#offset:{}",
            definition.offset
        )
    } else {
        format!(
            "creo:featdefs:feature_definition#{}",
            definition.identity.id()
        )
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
        .filter(|candidate| candidate.identity.id() == definition.identity.id())
        .count()
        != 1
        || (definition.identity.schema_id().is_none()
            && definition.identity.owner_feature_id().is_none())
    {
        format!("creo:featdefs:sketch#offset:{}", definition.offset)
    } else {
        format!("creo:featdefs:sketch#{}", definition.identity.id())
    }
}

pub(crate) fn model_sketch_id(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
) -> Option<SketchId> {
    let native_id = feature_sketch_record_id_in_scan(scan, definition);
    let scope = native_id.strip_prefix("creo:featdefs:sketch#")?;
    Some(SketchId::compose(
        &crate::identity::MODEL_SKETCH,
        IdentityKey::try_new(scope.to_owned()).ok()?,
    ))
}

pub(crate) fn sketch_identity_scope(sketch: &SketchId) -> &str {
    sketch
        .as_str()
        .strip_prefix("creo:model:sketch#")
        .unwrap_or(sketch.as_str())
}

pub(crate) fn sketch_identity_key(sketch: &SketchId) -> Option<IdentityKey> {
    IdentityKey::try_new(sketch_identity_scope(sketch).to_owned()).ok()
}

pub(crate) fn sketch_entity_id(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Option<SketchEntityId> {
    Some(SketchEntityId::compose(
        &crate::identity::FEATDEFS_SKETCH_ENTITY,
        sketch_identity_key(sketch)?.colon(IdentityKey::try_new(suffix.to_string()).ok()?),
    ))
}

pub(crate) fn sketch_constraint_id(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Option<SketchConstraintId> {
    Some(SketchConstraintId::compose(
        &crate::identity::FEATDEFS_SKETCH_CONSTRAINT,
        sketch_identity_key(sketch)?.colon(IdentityKey::try_new(suffix.to_string()).ok()?),
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

pub(crate) fn typed_sketch_section_curve_id(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Option<CurveId> {
    let suffix = IdentityKey::try_new(suffix.to_string()).ok()?;
    Some(CurveId::compose(
        &crate::identity::FEATDEFS_SECTION_CURVE,
        sketch_identity_key(sketch)?.colon(suffix),
    ))
}

pub(crate) fn sketch_point_ref(sketch: &SketchId, point: u32) -> String {
    format!("{}:point#{point}", sketch_native_ref(sketch))
}

pub(crate) fn sketch_feature_id(sketch: &SketchId) -> Option<IrFeatureId> {
    Some(IrFeatureId::compose(
        &crate::identity::MODEL_SKETCH_FEATURE,
        sketch_identity_key(sketch)?,
    ))
}

pub(crate) fn section_owner_feature_id(
    scan: &ContainerScan,
    definition_id: u32,
    sketch: &SketchId,
) -> Option<IrFeatureId> {
    owned_section_feature_id(scan, definition_id).map_or_else(
        || sketch_feature_id(sketch),
        |feature_id| {
            Some(IrFeatureId::compose(
                &crate::identity::MODEL_FEATURE,
                feature_id,
            ))
        },
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
        .filter(|definition| definition.identity.owner_feature_id() == Some(feature_id))
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    Some(feature_definition_record_id(scan, definition))
}
