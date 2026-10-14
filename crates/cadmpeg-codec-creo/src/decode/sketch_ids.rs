// SPDX-License-Identifier: Apache-2.0
//! Sketch native identity, table headers, and feature-definition record ids.

use cadmpeg_ir::features::FeatureId as IrFeatureId;
use cadmpeg_ir::ids::CurveId;
#[cfg(test)]
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::sketches::{SketchConstraintId, SketchEntityId, SketchId};

use crate::container::ContainerScan;

use super::feature_history::outputs::owned_section_feature_id;
use super::native_records::{CreoSketchBucketHeader, CreoSketchTableHeader, CreoSketchTableKind};
use super::uniqueness::exactly_one;

pub(super) fn feature_definition_has_sketch_design(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> bool {
    if definition.variables.is_some()
        || definition.segments.is_some()
        || definition.trim_entities.is_some()
        || definition.trim_vertices.is_some()
        || definition.order_table.is_some()
        || definition.section_3d.is_some()
        || definition.saved_section.is_some()
        || definition.dimensions.is_some()
        || definition.relations.is_some()
    {
        return true;
    }
    crate::feature::definitions::equation_table(&definition.body, 0, definition.body.len())
        .is_some()
}

pub(super) fn sketch_table_headers(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<CreoSketchTableHeader>, cadmpeg_core::CodecError> {
    let mut headers = Vec::new();
    let mut push = |kind, row_count, offset| -> Result<(), cadmpeg_core::CodecError> {
        headers.push(CreoSketchTableHeader {
            kind,
            row_count,
            offset,
        });
        Ok(())
    };
    if let Some(table) = &definition.variables {
        push(
            CreoSketchTableKind::Variables {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        )?;
    }
    if let Some(table) =
        crate::feature::definitions::equation_table(&definition.body, 0, definition.body.len())
    {
        push(
            CreoSketchTableKind::Equations {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            definition.body_position(table.offset)?.source()?.get(),
        )?;
    }
    if let Some(table) = &definition.segments {
        push(
            CreoSketchTableKind::Segments {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        )?;
    }
    if let Some(table) = &definition.trim_entities {
        let mut buckets = Vec::new();
        buckets.extend(table.buckets.iter().map(|bucket| CreoSketchBucketHeader {
            index: bucket.index,
            declared_entry_count: bucket.declared_entry_count,
            decoded_entry_count: bucket.decoded_entry_count,
            offset: bucket.offset,
        }));
        push(
            CreoSketchTableKind::TrimEntities {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
                entry_ref: table.entry_ref,
                buckets,
            },
            table.rows.len(),
            table.offset,
        )?;
    }
    if let Some(table) = &definition.trim_vertices {
        let mut buckets = Vec::new();
        buckets.extend(table.buckets.iter().map(|bucket| CreoSketchBucketHeader {
            index: bucket.index,
            declared_entry_count: bucket.declared_entry_count,
            decoded_entry_count: bucket.decoded_entry_count,
            offset: bucket.offset,
        }));
        push(
            CreoSketchTableKind::TrimVertices {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
                entry_ref: table.entry_ref,
                buckets,
            },
            table.rows.len(),
            table.offset,
        )?;
    }
    if let Some(table) = &definition.order_table {
        push(
            CreoSketchTableKind::Order {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        )?;
    }
    if let Some(table) = &definition.dimensions {
        push(
            CreoSketchTableKind::Dimensions {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        )?;
    }
    if let Some(table) = &definition.relations {
        push(
            CreoSketchTableKind::Relations {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            table.offset,
        )?;
        if let Some(header) = table.skamps.as_ref().and_then(|table| table.header()) {
            push(
                CreoSketchTableKind::SolverIncidences {
                    declared_count: header.declared_count,
                    entity_ref: header.entity_ref,
                },
                table.skamps().len(),
                header.offset,
            )?;
        }
        if let Some(header) = table.triples.as_ref().and_then(|table| table.header()) {
            push(
                CreoSketchTableKind::RelationTriples {
                    declared_count: header.declared_count,
                    entity_ref: header.entity_ref,
                },
                table.triples().len(),
                header.offset,
            )?;
        }
    }
    if let Some(table) = &definition.saved_section {
        push(
            CreoSketchTableKind::SavedEntities,
            table.entities.len(),
            table.offset,
        )?;
    }
    headers.as_mut_slice().sort_by_key(|left| left.offset);
    Ok(headers)
}

pub(super) fn binary_flag_value(flag: crate::feature::definitions::BinaryFlag) -> bool {
    match flag {
        crate::feature::definitions::BinaryFlag::Clear => false,
        crate::feature::definitions::BinaryFlag::Set => true,
    }
}

pub(super) fn feature_definition_record_id(
    scan: &ContainerScan,
    definition: &crate::feature::definitions::FeatureDefinition,
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

pub(super) fn feature_sketch_record_id_in_scan(
    scan: &ContainerScan,
    definition: &crate::feature::definitions::FeatureDefinition,
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

pub(super) fn model_sketch_id(
    scan: &ContainerScan,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Option<SketchId> {
    let ambiguous = scan
        .features
        .definitions
        .iter()
        .filter(|candidate| candidate.identity.id() == definition.identity.id())
        .count()
        != 1
        || (definition.identity.schema_id().is_none()
            && definition.identity.owner_feature_id().is_none());
    let text = if ambiguous {
        format!("creo:model:sketch#offset:{}", definition.offset)
    } else {
        format!("creo:model:sketch#{}", definition.identity.id())
    };
    SketchId::try_from(text).ok()
}

pub(super) fn sketch_identity_scope(sketch: &SketchId) -> &str {
    sketch
        .as_str()
        .strip_prefix("creo:model:sketch#")
        .unwrap_or(sketch.as_str())
}

#[cfg(test)]
pub(super) fn sketch_identity_key(sketch: &SketchId) -> Option<IdentityKey> {
    IdentityKey::try_new(sketch_identity_scope(sketch).to_owned()).ok()
}

#[cfg(test)]
pub(super) fn sketch_entity_id(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Option<SketchEntityId> {
    Some(SketchEntityId::compose(
        &crate::identity::FEATDEFS_SKETCH_ENTITY,
        sketch_identity_key(sketch)?.colon(IdentityKey::try_new(suffix.to_string()).ok()?),
    ))
}

pub(super) fn sketch_entity_id_admitted(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Option<SketchEntityId> {
    let text = format!(
        "creo:featdefs:sketch_entity#{}:{suffix}",
        sketch_identity_scope(sketch),
    );
    SketchEntityId::try_from(text).ok()
}

pub(super) fn sketch_constraint_id_admitted(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Option<SketchConstraintId> {
    let text = format!(
        "creo:featdefs:sketch_constraint#{}:{suffix}",
        sketch_identity_scope(sketch),
    );
    SketchConstraintId::try_from(text).ok()
}

pub(super) fn sketch_native_ref_admitted(sketch: &SketchId) -> String {
    format!("creo:featdefs:sketch#{}", sketch_identity_scope(sketch))
}

pub(super) fn sketch_section_curve_id_admitted(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> String {
    let namespace = &crate::identity::FEATDEFS_SECTION_CURVE;
    format!(
        "{}:{}:{}#{}:{suffix}",
        namespace.format(),
        namespace.scope(),
        namespace.kind(),
        sketch_identity_scope(sketch)
    )
}

pub(super) fn typed_sketch_section_curve_id_admitted(
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Option<CurveId> {
    let namespace = &crate::identity::FEATDEFS_SECTION_CURVE;
    let text = format!(
        "{}:{}:{}#{}:{suffix}",
        namespace.format(),
        namespace.scope(),
        namespace.kind(),
        sketch_identity_scope(sketch)
    );
    CurveId::try_from(text).ok()
}

pub(super) fn sketch_point_ref_admitted(sketch: &SketchId, point: u32) -> String {
    format!(
        "creo:featdefs:sketch#{}:point#{point}",
        sketch_identity_scope(sketch)
    )
}

pub(super) fn sketch_feature_id_admitted(sketch: &SketchId) -> Option<IrFeatureId> {
    let namespace = &crate::identity::MODEL_SKETCH_FEATURE;
    let text = format!(
        "{}:{}:{}#{}",
        namespace.format(),
        namespace.scope(),
        namespace.kind(),
        sketch_identity_scope(sketch)
    );
    IrFeatureId::try_from(text).ok()
}

pub(super) fn section_owner_feature_id(
    scan: &ContainerScan,
    definition_id: u32,
    sketch: &SketchId,
) -> Option<IrFeatureId> {
    let text = if let Some(feature_id) = owned_section_feature_id(scan, definition_id) {
        format!("creo:model:feature#{feature_id}")
    } else {
        format!(
            "creo:model:sketch_feature#{}",
            sketch_identity_scope(sketch)
        )
    };
    IrFeatureId::try_from(text).ok()
}

pub(super) fn owning_feature_definition_ref(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<String> {
    let definition = exactly_one(
        scan.features
            .definitions
            .iter()
            .filter(|definition| definition.identity.owner_feature_id() == Some(feature_id)),
    )?;
    Some(feature_definition_record_id(scan, definition))
}

#[cfg(test)]
mod tests;
