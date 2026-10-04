// SPDX-License-Identifier: Apache-2.0
//! Validation of borrowed annotations and native product links.

use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::index::ModelIndex;
use crate::native::view::{NativeArena, NativeEntity, NativeView};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde_value::Value;

enum AnnotatedEntity<'ctx, 'ir> {
    Projected(crate::schema::structural::Projection<'ctx>),
    Source(&'ir crate::unknown::UnknownRecord),
}

macro_rules! define_model_entity_projection {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        fn model_entity_projection<'ctx, 'ir>(
            ctx: &'ctx DecodeContext<'_>,
            ir: &'ir CadIr,
            wanted: &BorrowedIdentities<'_, '_>,
            entities: &mut Vec<(&'ir str, AnnotatedEntity<'ctx, 'ir>)>,
            storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        ) -> Result<(), CodecError> {
            $(for entity in ctx.admit_iter(
                ir.model.$field.as_slice(),
                "annotated model entity scan",
            )? {
                let id = crate::schema::EntitySchema::identity(entity);
                if wanted.contains(ctx, id)? {
                    let value = match crate::schema::structural::project(ctx, entity, "annotated entity projection") {
                        Ok(value) => value,
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => continue,
                    };
                    if let Some(position) = entity_position(ctx, entities, id)? {
                        entities[position].1 = AnnotatedEntity::Projected(value);
                    } else {
                        storage.with_storage(|| ctx.push_vec(entities, (id, AnnotatedEntity::Projected(value)), "annotated entity slots"))?;
                    }
                }
            })*
            Ok(())
        }
    };
}
crate::document::arena_registry!(define_model_entity_projection);

fn entity_position(
    ctx: &DecodeContext<'_>,
    entities: &[(&str, AnnotatedEntity<'_, '_>)],
    id: &str,
) -> Result<Option<usize>, CodecError> {
    for (position, (candidate, _)) in entities.iter().enumerate() {
        ctx.charge_work(1, "annotated entity lookup")?;
        ctx.charge_work(
            u64_from_index(id.len()),
            "annotated entity identity comparison",
        )?;
        if *candidate == id {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

pub(super) fn check_annotations<'ir>(
    ctx: &DecodeContext<'_>,
    view: NativeView<'ir>,
    annotations: &crate::Annotations,
    all_ids: &ModelIndex<'_>,
    source_fidelity: Option<&crate::source_fidelity::SourceFidelity>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let wanted = BorrowedIdentities::build(ctx, |add| {
        for (id, note) in annotations.exactness() {
            ctx.charge_work(1, "annotated entity selection")?;
            if !note.fields().is_empty() {
                add(id, ())?;
            }
        }
        Ok(())
    })?;
    let mut storage = ctx.reserve_scoped(0, "annotated entity storage")?;
    let mut entities = Vec::new();
    model_entity_projection(ctx, view.ir, &wanted, &mut entities, &mut storage)?;
    view.visit(
        |work| ctx.charge_work(u64_from_index(work), "annotated native arena scan"),
        |_, _, records| {
            let mut append_record = |record: NativeEntity<'ir>| -> Result<(), CodecError> {
                let id = record.id();
                if !wanted.contains(ctx, id)? || entity_position(ctx, &entities, id)?.is_some() {
                    return Ok(());
                }
                let value = match record {
                    NativeEntity::Product(product) => match crate::schema::structural::project(
                        ctx,
                        product,
                        "annotated native entity projection",
                    ) {
                        Ok(value) => AnnotatedEntity::Projected(value),
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => return Ok(()),
                    },
                    NativeEntity::Source(source) => AnnotatedEntity::Source(source),
                };
                storage.with_storage(|| {
                    ctx.push_vec(&mut entities, (id, value), "annotated entity slots")
                })?;
                Ok(())
            };
            match records {
                NativeArena::Product(products) => {
                    for product in ctx.admit_iter(products, "annotated native record scan")? {
                        append_record(NativeEntity::Product(product))?;
                    }
                }
                NativeArena::Source(sources, order) => {
                    for index in ctx.admit_iter(order, "annotated native record scan")? {
                        append_record(NativeEntity::Source(&sources[*index]))?;
                    }
                }
            }
            Ok(())
        },
    )?;
    let identity_exists = |id: &str| -> Result<bool, CodecError> {
        if all_ids.contains(id, ctx)? {
            return Ok(true);
        }
        match source_fidelity {
            Some(source) => ctx.contains_key_btree_map(
                source.retained_records(),
                id,
                "annotation source identity query",
            ),
            None => Ok(false),
        }
    };
    for (id, _) in ctx.admit_iter(&annotations.provenance, "annotation provenance scan")? {
        if !identity_exists(id)? {
            super::record_finding(
                ctx,
                findings,
                Check::Annotations,
                Severity::Error,
                Some(id),
                format_args!("provenance key does not resolve to an entity"),
            )?;
        }
    }
    for (id, note) in ctx.admit_iter(annotations.exactness(), "annotation exactness scan")? {
        if !identity_exists(id)? {
            super::record_finding(
                ctx,
                findings,
                Check::Annotations,
                Severity::Error,
                Some(id),
                format_args!("exactness key does not resolve to an entity"),
            )?;
            continue;
        }
        if note.fields().is_empty() {
            continue;
        }
        let Some(position) = entity_position(ctx, &entities, id)? else {
            super::record_finding(
                ctx,
                findings,
                Check::Annotations,
                Severity::Warning,
                Some(id),
                format_args!(
                    "entity could not be serialized to validate its exactness field paths"
                ),
            )?;
            continue;
        };
        for (path, _) in ctx.admit_iter(note.fields(), "annotation field path scan")? {
            let resolves = match &entities[position].1 {
                AnnotatedEntity::Projected(value) => {
                    field_path_resolves(ctx, value, path.as_str())?
                }
                AnnotatedEntity::Source(source) => {
                    source_field_path_resolves(ctx, source, path.as_str())?
                }
            };
            if !resolves {
                super::record_finding(
                    ctx,
                    findings,
                    Check::Annotations,
                    Severity::Warning,
                    Some(id),
                    format_args!("exactness field path `{path}` does not resolve"),
                )?;
            }
        }
    }
    Ok(())
}

fn source_field_path_resolves(
    ctx: &DecodeContext<'_>,
    record: &crate::unknown::UnknownRecord,
    path: &str,
) -> Result<bool, CodecError> {
    if path == "id" {
        return Ok(true);
    }
    if record.links().is_empty() {
        return Ok(false);
    }
    if path == "links" {
        return Ok(true);
    }
    let Some(index) = ctx.strip_prefix(path, "links.", "annotation source field path scan")? else {
        return Ok(false);
    };
    let index = match ctx.parse_text::<usize>(index, "annotation source field path scan")? {
        Ok(index) => index,
        Err(_) => return Ok(false),
    };
    Ok(index < record.links().len())
}

fn field_path_resolves(
    ctx: &DecodeContext<'_>,
    mut value: &Value,
    path: &str,
) -> Result<bool, CodecError> {
    let resolve_component = |current: &mut &Value, component: &str| -> Result<bool, CodecError> {
        loop {
            ctx.charge_work(1, "annotation field path node")?;
            match *current {
                Value::Option(Some(inner)) | Value::Newtype(inner) => *current = inner,
                _ => break,
            }
        }
        match *current {
            Value::Map(object) => {
                let mut next = None;
                for (key, child) in object {
                    ctx.charge_work(1, "annotation field map scan")?;
                    ctx.charge_work(
                        u64_from_index(component.len()),
                        "annotation field name comparison",
                    )?;
                    if matches!(key, Value::String(key) if key == component) {
                        next = Some(child);
                        break;
                    }
                }
                let Some(child) = next else {
                    return Ok(false);
                };
                *current = child;
            }
            Value::Seq(array) => {
                ctx.charge_work(
                    u64_from_index(component.len()),
                    "annotation field index scan",
                )?;
                let Ok(index) = component.parse::<usize>() else {
                    return Ok(false);
                };
                let Some(next) = array.get(index) else {
                    return Ok(false);
                };
                *current = next;
            }
            _ => return Ok(false),
        }
        Ok(true)
    };

    let mut component_start = 0;
    for (index, byte) in ctx
        .admit_iter(path.as_bytes(), "annotation field path scan")?
        .enumerate()
    {
        if *byte == b'.' {
            if !resolve_component(&mut value, &path[component_start..index])? {
                return Ok(false);
            }
            component_start = index + 1;
        }
    }
    resolve_component(&mut value, &path[component_start..])
}

pub(super) fn check_native_links(
    ctx: &DecodeContext<'_>,
    view: NativeView<'_>,
    all_ids: &crate::index::ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let ir = view.ir;
    let native_ids = BorrowedIdentities::build(ctx, |add| {
        view.visit(
            |work| ctx.charge_work(u64_from_index(work), "native identity arena scan"),
            |_, _, records| {
                match records {
                    NativeArena::Product(products) => {
                        for record in ctx.admit_iter(products, "native identity record scan")? {
                            add(record.id(), ())?;
                        }
                    }
                    NativeArena::Source(sources, order) => {
                        for index in ctx.admit_iter(order, "native identity record scan")? {
                            add(sources[*index].id().as_str(), ())?;
                        }
                    }
                }
                Ok(())
            },
        )
    })?;
    for feature in &ir.model.features {
        ctx.charge_work(1, "native reference owner scan")?;
        if let Some(target) = &feature.native_ref {
            if !native_ids.contains(ctx, target.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                )?;
            }
        }
        if let crate::features::FeatureDefinition::Operation(
            crate::features::FeatureOperation::HelixNativeAxis {
                axis_native_ref: target,
                ..
            },
        ) = feature.evaluation.definition()
        {
            if !native_ids.contains(ctx, target.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    format_args!("helix axis native_ref `{target}` does not resolve"),
                )?;
            }
        }
    }
    for parameter in &ir.model.parameters {
        ctx.charge_work(1, "native reference owner scan")?;
        if let Some(target) = &parameter.native_ref {
            if !native_ids.contains(ctx, target.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(parameter.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                )?;
            }
        }
        if let Some(semantic) = &parameter.pmi {
            if !native_ids.contains(ctx, semantic.native_ref.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(parameter.id.as_str()),
                    format_args!("PMI native_ref `{}` does not resolve", semantic.native_ref),
                )?;
            }
        }
    }
    for configuration in &ir.model.configurations {
        ctx.charge_work(1, "native reference owner scan")?;
        if let Some(target) = &configuration.native_ref {
            if !native_ids.contains(ctx, target.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(configuration.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                )?;
            }
        }
    }
    for sketch in &ir.model.sketches {
        ctx.charge_work(1, "native reference owner scan")?;
        if let Some(target) = &sketch.native_ref {
            if !native_ids.contains(ctx, target.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(sketch.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                )?;
            }
        }
    }
    for sketch in &ir.model.spatial_sketches {
        ctx.charge_work(1, "native reference owner scan")?;
        if let Some(target) = &sketch.native_ref {
            if !native_ids.contains(ctx, target.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(sketch.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                )?;
            }
        }
    }
    for constraint in &ir.model.sketch_constraints {
        ctx.charge_work(1, "native reference owner scan")?;
        if let Some(target) = &constraint.native_ref {
            if !native_ids.contains(ctx, target.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                )?;
            }
        }
        if let crate::sketches::SketchConstraintDefinitionInput::Native { operands, .. } =
            constraint.definition.kind()
        {
            for operand in ctx.admit_iter(operands, "native operand scan")? {
                if let Some(target) = &operand.native_ref {
                    if !native_ids.contains(ctx, target.as_str())? {
                        super::record_finding(
                            ctx,
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(constraint.id.as_str()),
                            format_args!("operand native_ref `{target}` does not resolve"),
                        )?;
                    }
                }
            }
        }
    }
    for constraint in &ir.model.spatial_sketch_constraints {
        ctx.charge_work(1, "native reference owner scan")?;
        if let Some(target) = &constraint.native_ref {
            if !native_ids.contains(ctx, target.as_str())? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                )?;
            }
        }
        if let crate::sketches::SpatialSketchConstraintDefinitionInput::Native {
            operands, ..
        } = constraint.definition.kind()
        {
            for operand in ctx.admit_iter(operands, "native operand scan")? {
                if let Some(target) = &operand.native_ref {
                    if !native_ids.contains(ctx, target.as_str())? {
                        super::record_finding(
                            ctx,
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(constraint.id.as_str()),
                            format_args!("operand native_ref `{target}` does not resolve"),
                        )?;
                    }
                }
            }
        }
    }

    // `unknowns` has a shared identity-link contract. Other arenas own their
    // field shapes; only an array made entirely of strings follows the generic
    // identity-link convention.
    view.visit(
        |work| ctx.charge_work(u64_from_index(work), "native link arena scan"),
        |_, arena, records| {
            for entity in records.records() {
                ctx.charge_work(1, "native link record scan")?;
                let record = match entity {
                    NativeEntity::Product(record) => record,
                    NativeEntity::Source(source) => {
                        for target in ctx.admit_iter(source.links(), "native outgoing link scan")? {
                            let target = target.as_str();
                            if !all_ids.contains(target, ctx)? {
                                super::record_finding(
                                    ctx,
                                    findings,
                                    Check::NativeLinks,
                                    Severity::Error,
                                    Some(source.id().as_str()),
                                    format_args!("native-record link `{target}` does not resolve"),
                                )?;
                            }
                        }
                        continue;
                    }
                };
                for _ in 0..=record.fields().len() {
                    ctx.charge_work(6, "native link field lookup")?;
                }
                let Some(value) = record.fields().get("links") else {
                    continue;
                };
                if arena == "unknowns" {
                    ctx.charge_work(
                        u64_from_index(value.as_array().map_or(0, Vec::len)),
                        "native link shape scan",
                    )?;
                } else {
                    let Some(links) = value.as_array() else {
                        continue;
                    };
                    if !ctx
                        .admit_iter(links, "native link shape scan")?
                        .all(serde_json::Value::is_string)
                    {
                        continue;
                    }
                }
                let serde_json::Value::Array(links) = value else {
                    super::record_finding(
                        ctx,
                        findings,
                        Check::NativeLinks,
                        Severity::Error,
                        Some(record.id()),
                        format_args!("{}", "native-record links must be an array of identities"),
                    )?;
                    continue;
                };
                for (index, link) in links.iter().enumerate() {
                    ctx.charge_work(1, "native link scan")?;
                    let Some(target) = link.as_str() else {
                        super::record_finding(
                            ctx,
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(record.id()),
                            format_args!("native-record link {index} must be an identity string"),
                        )?;
                        continue;
                    };
                    if !all_ids.contains(target, ctx)? {
                        super::record_finding(
                            ctx,
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(record.id()),
                            format_args!("native-record link `{target}` does not resolve"),
                        )?;
                    }
                }
            }
            Ok(())
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
