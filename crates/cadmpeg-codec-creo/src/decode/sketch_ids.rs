// SPDX-License-Identifier: Apache-2.0
//! Sketch native identity, table headers, and feature-definition record ids.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FeatureId as IrFeatureId;
use cadmpeg_ir::ids::CurveId;
#[cfg(test)]
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::sketches::{SketchConstraintId, SketchEntityId, SketchId};

use crate::container::ContainerScan;

use super::feature_history::outputs::owned_section_feature_id;
use super::native_records::{CreoSketchBucketHeader, CreoSketchTableHeader, CreoSketchTableKind};
use super::uniqueness::exactly_one_by;

pub(super) fn feature_definition_has_sketch_design(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<bool, cadmpeg_core::CodecError> {
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
        return Ok(true);
    }
    let mut storage = ctx.reserve_scoped(0, "creo sketch design equation scratch")?;
    Ok(storage.with_storage(|| crate::feature::definitions::equation_table(
        ctx, &definition.body, 0, definition.body.len(),
    ))?.is_some())
}

pub(super) fn sketch_table_headers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Vec<CreoSketchTableHeader>, cadmpeg_core::CodecError> {
    let mut headers = Vec::new();
    let mut push = |kind, row_count, offset| -> Result<(), cadmpeg_core::CodecError> {
        ctx.reserve_vec(&mut headers, 1, "creo sketch table headers")?;
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
    let mut equation_storage = ctx.reserve_scoped(0, "creo sketch header equation scratch")?;
    if let Some(table) = equation_storage.with_storage(|| crate::feature::definitions::equation_table(
        ctx, &definition.body, 0, definition.body.len(),
    ))? {
        push(
            CreoSketchTableKind::Equations {
                declared_count: table.declared_count,
                entity_ref: table.entity_ref,
            },
            table.rows.len(),
            definition.body_position(table.offset)?.source()?.get(),
        )?;
    }
    drop(equation_storage);
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
        ctx.reserve_vec(
            &mut buckets,
            table.buckets.len(),
            "creo sketch trim entity headers",
        )?;
        buckets.extend(ctx.admit_iter(&table.buckets, "creo sketch bucket header rows")?.map(|bucket| CreoSketchBucketHeader {
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
        ctx.reserve_vec(
            &mut buckets,
            table.buckets.len(),
            "creo sketch trim vertex headers",
        )?;
        buckets.extend(ctx.admit_iter(&table.buckets, "creo sketch bucket header rows")?.map(|bucket| CreoSketchBucketHeader {
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
    // Each optional table contributes at most one of eleven header kinds.
    headers.sort_by_key(|header| header.offset);
    Ok(headers)
}

pub(super) fn binary_flag_value(flag: crate::feature::definitions::BinaryFlag) -> bool {
    match flag {
        crate::feature::definitions::BinaryFlag::Clear => false,
        crate::feature::definitions::BinaryFlag::Set => true,
    }
}

pub(super) fn feature_definition_record_id(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<String, CodecError> {
    if exactly_one_by(ctx, &scan.features.definitions,
        |candidate| Ok(candidate.identity.id() == definition.identity.id()),
        "creo feature definition identity count")?.is_none()
        || (definition.identity.schema_id().is_none()
            && definition.identity.owner_feature_id().is_none())
    {
        ctx.format_retained(
            format_args!(
                "creo:featdefs:feature_definition#offset:{}",
                definition.offset
            ),
            "creo feature definition record id",
        )
    } else {
        ctx.format_retained(
            format_args!(
                "creo:featdefs:feature_definition#{}",
                definition.identity.id()
            ),
            "creo feature definition record id",
        )
    }
}

pub(super) fn feature_sketch_record_id_in_scan(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<String, CodecError> {
    if exactly_one_by(ctx, &scan.features.definitions,
        |candidate| Ok(candidate.identity.id() == definition.identity.id()),
        "creo native sketch identity uniqueness")?.is_none()
        || (definition.identity.schema_id().is_none()
            && definition.identity.owner_feature_id().is_none())
    {
        ctx.format_retained(
            format_args!("creo:featdefs:sketch#offset:{}", definition.offset),
            "creo native sketch identity",
        )
    } else {
        ctx.format_retained(
            format_args!("creo:featdefs:sketch#{}", definition.identity.id()),
            "creo native sketch identity",
        )
    }
}

pub(super) fn model_sketch_id(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Option<SketchId>, CodecError> {
    let ambiguous = exactly_one_by(ctx, &scan.features.definitions,
        |candidate| Ok(candidate.identity.id() == definition.identity.id()),
        "creo model sketch identity uniqueness")?.is_none()
        || (definition.identity.schema_id().is_none()
            && definition.identity.owner_feature_id().is_none());
    let text = if ambiguous {
        ctx.format_retained(
            format_args!("creo:model:sketch#offset:{}", definition.offset),
            "creo model sketch identity",
        )?
    } else {
        ctx.format_retained(
            format_args!("creo:model:sketch#{}", definition.identity.id()),
            "creo model sketch identity",
        )?
    };
    Ok(SketchId::try_from(text).ok())
}

/// The sketch identity without its constant namespace prefix; the comparison
/// reads at most the prefix's bytes.
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
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Result<Option<SketchEntityId>, CodecError> {
    let text = ctx.format_retained(
        format_args!(
            "creo:featdefs:sketch_entity#{}:{suffix}",
            sketch_identity_scope(sketch),
        ),
        "creo sketch entity identity",
    )?;
    Ok(SketchEntityId::try_from(text).ok())
}

pub(super) fn sketch_constraint_id_admitted(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Result<Option<SketchConstraintId>, CodecError> {
    let text = ctx.format_retained(
        format_args!(
            "creo:featdefs:sketch_constraint#{}:{suffix}",
            sketch_identity_scope(sketch),
        ),
        "creo sketch constraint identity",
    )?;
    Ok(SketchConstraintId::try_from(text).ok())
}

pub(super) fn sketch_native_ref_admitted(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!("creo:featdefs:sketch#{}", sketch_identity_scope(sketch)),
        "creo sketch native reference",
    )
}

pub(super) fn sketch_section_curve_id_admitted(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Result<String, CodecError> {
    let namespace = &crate::identity::FEATDEFS_SECTION_CURVE;
    ctx.format_retained(
        format_args!(
            "{}:{}:{}#{}:{suffix}",
            namespace.format(),
            namespace.scope(),
            namespace.kind(),
            sketch_identity_scope(sketch)
        ),
        "creo section curve reference",
    )
}

pub(super) fn typed_sketch_section_curve_id_admitted(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
) -> Result<Option<CurveId>, CodecError> {
    let namespace = &crate::identity::FEATDEFS_SECTION_CURVE;
    let text = ctx.format_retained(
        format_args!(
            "{}:{}:{}#{}:{suffix}",
            namespace.format(),
            namespace.scope(),
            namespace.kind(),
            sketch_identity_scope(sketch)
        ),
        "creo section curve identity",
    )?;
    Ok(CurveId::try_from(text).ok())
}

pub(super) fn sketch_point_ref_admitted(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
    point: u32,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!(
            "creo:featdefs:sketch#{}:point#{point}",
            sketch_identity_scope(sketch)
        ),
        "creo sketch point reference",
    )
}

pub(super) fn sketch_feature_id_admitted(
    ctx: &DecodeContext<'_>,
    sketch: &SketchId,
) -> Result<Option<IrFeatureId>, CodecError> {
    let namespace = &crate::identity::MODEL_SKETCH_FEATURE;
    let text = ctx.format_retained(
        format_args!(
            "{}:{}:{}#{}",
            namespace.format(),
            namespace.scope(),
            namespace.kind(),
            sketch_identity_scope(sketch)
        ),
        "creo sketch feature identity",
    )?;
    Ok(IrFeatureId::try_from(text).ok())
}

pub(super) fn section_owner_feature_id(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    definition_id: u32,
    sketch: &SketchId,
) -> Result<Option<IrFeatureId>, CodecError> {
    let text = if let Some(feature_id) = owned_section_feature_id(scan, definition_id) {
        ctx.format_retained(
            format_args!("creo:model:feature#{feature_id}"),
            "creo section owner feature identity",
        )?
    } else {
        ctx.format_retained(
            format_args!(
                "creo:model:sketch_feature#{}",
                sketch_identity_scope(sketch)
            ),
            "creo section owner feature identity",
        )?
    };
    Ok(IrFeatureId::try_from(text).ok())
}

pub(super) fn owning_feature_definition_ref(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<String>, CodecError> {
    let Some(definition) = exactly_one_by(ctx, &scan.features.definitions,
        |definition| Ok(definition.identity.owner_feature_id() == Some(feature_id)),
        "creo owning feature definition lookup")? else {
        return Ok(None);
    };
    feature_definition_record_id(ctx, scan, definition).map(Some)
}

#[cfg(test)]
mod tests;
