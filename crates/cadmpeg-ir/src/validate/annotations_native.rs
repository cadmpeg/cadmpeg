// SPDX-License-Identifier: Apache-2.0
//! Validation of borrowed annotations and native product links.

use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::native::view::{NativeEntity, NativeView};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde_value::Value;

enum AnnotatedEntity<'ctx, 'ir> {
    Projected(crate::schema::structural::Projection<'ctx>),
    Product(&'ir crate::native::NativeRecord),
    Source(&'ir crate::unknown::UnknownRecord),
}

macro_rules! define_model_entity_projection {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        fn model_entity_projection<'ctx, 'ir>(
            ctx: &'ctx DecodeContext<'_>,
            ir: &'ir CadIr,
            wanted: &BorrowedIdentities<'_, bool>,
            add: &mut dyn FnMut(&'ir str, AnnotatedEntity<'ctx, 'ir>) -> Result<(), CodecError>,
        ) -> Result<(), CodecError> {
            $(for entity in &ir.model.$field {
                let id = crate::schema::EntitySchema::identity(entity);
                if wanted.contains(id) {
                    let value = match crate::schema::structural::project(ctx, entity, "annotated entity projection") {
                        Ok(value) => value,
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => continue,
                    };
                    add(id, AnnotatedEntity::Projected(value))?;
                }
            })*
            Ok(())
        }
    };
}
crate::document::arena_registry!(define_model_entity_projection);

pub(super) fn check_annotations(
    ctx: &DecodeContext<'_>,
    view: NativeView<'_>,
    annotations: &crate::Annotations,
    all_ids: &BorrowedIdentities<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut wanted = BorrowedIdentities::build(|add| {
        for (id, note) in annotations.exactness() {
            if !note.fields().is_empty() {
                add(id, false)?;
            }
        }
        Ok(())
    })?;
    let mut values = Vec::new();
    let entities = BorrowedIdentities::build(|add_identity| {
        // Sort small identity slots rather than moving projected payloads.
        let mut add = |id, value| {
            let position = values.len();
            values.push(value);
            add_identity(id, position)
        };
        view.visit::<cadmpeg_core::CodecError>(|_, _, records| {
            for record in records.records() {
                let Some(seen) = wanted.get_mut(record.id()) else {
                    continue;
                };
                if *seen {
                    continue;
                }
                *seen = true;
                let value = match record {
                    NativeEntity::Product(product) => AnnotatedEntity::Product(product),
                    NativeEntity::Source(source) => AnnotatedEntity::Source(source),
                };
                add(record.id(), value)?;
            }
            Ok(())
        })?;
        // Model entities take precedence over native records with the same id.
        // The identity index keeps the last inserted matching value.
        model_entity_projection(ctx, view.ir, &wanted, &mut add)
    })?;
    for id in annotations.provenance.keys() {
        if !all_ids.contains(id) {
            super::record_finding(
                findings,
                Check::Annotations,
                Severity::Error,
                Some(id),
                format_args!("provenance key does not resolve to an entity"),
            );
        }
    }
    for (id, note) in annotations.exactness() {
        if !all_ids.contains(id) {
            super::record_finding(
                findings,
                Check::Annotations,
                Severity::Error,
                Some(id),
                format_args!("exactness key does not resolve to an entity"),
            );
            continue;
        }
        if note.fields().is_empty() {
            continue;
        }
        let Some(position) = entities.get(id) else {
            super::record_finding(
                findings,
                Check::Annotations,
                Severity::Warning,
                Some(id),
                format_args!(
                    "entity could not be serialized to validate its exactness field paths"
                ),
            );
            continue;
        };
        let entity = &values[*position];
        for path in note.fields().keys() {
            let resolves = match entity {
                AnnotatedEntity::Projected(value) => field_path_resolves(value, path.as_str()),
                AnnotatedEntity::Product(product) => {
                    native_field_path_resolves(product, path.as_str())
                }
                AnnotatedEntity::Source(source) => {
                    source_field_path_resolves(source, path.as_str())
                }
            };
            if !resolves {
                super::record_finding(
                    findings,
                    Check::Annotations,
                    Severity::Warning,
                    Some(id),
                    format_args!("exactness field path `{path}` does not resolve"),
                );
            }
        }
    }
    Ok(())
}

fn native_field_path_resolves(record: &crate::native::NativeRecord, path: &str) -> bool {
    let mut components = path.split('.');
    let Some(first) = components.next() else {
        return false;
    };
    if first == "id" {
        return components.next().is_none();
    }

    let Some(field) = record.field(first) else {
        return false;
    };
    let mut value = &field;
    for component in components {
        value = match value {
            serde_json::Value::Object(object) => {
                let mut next = None;
                for (key, child) in object {
                    if key == component {
                        next = Some(child);
                        break;
                    }
                }
                let Some(child) = next else {
                    return false;
                };
                child
            }
            serde_json::Value::Array(array) => {
                let Some(child) = component
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| array.get(index))
                else {
                    return false;
                };
                child
            }
            _ => return false,
        };
    }
    true
}

fn source_field_path_resolves(record: &crate::unknown::UnknownRecord, path: &str) -> bool {
    if path == "id" {
        return true;
    }
    if record.links().is_empty() {
        return false;
    }
    if path == "links" {
        return true;
    }
    path.strip_prefix("links.")
        .and_then(|index| index.parse::<usize>().ok())
        .is_some_and(|index| index < record.links().len())
}

fn field_path_resolves(mut value: &Value, path: &str) -> bool {
    for component in path.split('.') {
        while let Value::Option(Some(inner)) | Value::Newtype(inner) = value {
            value = inner;
        }
        match value {
            Value::Map(object) => {
                let mut next = None;
                for (key, child) in object {
                    if matches!(key, Value::String(key) if key == component) {
                        next = Some(child);
                        break;
                    }
                }
                let Some(child) = next else {
                    return false;
                };
                value = child;
            }
            Value::Seq(array) => {
                let Ok(index) = component.parse::<usize>() else {
                    return false;
                };
                let Some(next) = array.get(index) else {
                    return false;
                };
                value = next;
            }
            _ => return false,
        }
    }
    true
}

pub(super) fn check_native_links(
    view: NativeView<'_>,
    all_ids: &crate::index::ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let ir = view.ir;
    let all_targets = BorrowedIdentities::build(|add| {
        for id in all_ids.identities(crate::index::StandardIndex) {
            add(id, ())?;
        }
        Ok(())
    })?;
    let native_ids = BorrowedIdentities::build(|add| {
        view.visit::<cadmpeg_core::CodecError>(|_, _, records| {
            for record in records.records() {
                add(record.id(), ())?;
            }
            Ok(())
        })
    })?;
    for feature in &ir.model.features {
        if let Some(target) = &feature.native_ref {
            if !native_ids.contains(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                );
            }
        }
        if let crate::features::FeatureDefinition::Operation(
            crate::features::FeatureOperation::HelixNativeAxis {
                axis_native_ref: target,
                ..
            },
        ) = feature.evaluation.definition()
        {
            if !native_ids.contains(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    format_args!("helix axis native_ref `{target}` does not resolve"),
                );
            }
        }
    }
    for parameter in &ir.model.parameters {
        if let Some(target) = &parameter.native_ref {
            if !native_ids.contains(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(parameter.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                );
            }
        }
        if let Some(semantic) = &parameter.pmi {
            if !native_ids.contains(semantic.native_ref.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(parameter.id.as_str()),
                    format_args!("PMI native_ref `{}` does not resolve", semantic.native_ref),
                );
            }
        }
    }
    for configuration in &ir.model.configurations {
        if let Some(target) = &configuration.native_ref {
            if !native_ids.contains(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(configuration.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                );
            }
        }
    }
    for sketch in &ir.model.sketches {
        if let Some(target) = &sketch.native_ref {
            if !native_ids.contains(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(sketch.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                );
            }
        }
    }
    for sketch in &ir.model.spatial_sketches {
        if let Some(target) = &sketch.native_ref {
            if !native_ids.contains(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(sketch.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                );
            }
        }
    }
    for constraint in &ir.model.sketch_constraints {
        if let Some(target) = &constraint.native_ref {
            if !native_ids.contains(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                );
            }
        }
        if let crate::sketches::SketchConstraintDefinitionInput::Native { operands, .. } =
            constraint.definition.kind()
        {
            for operand in operands {
                if let Some(target) = &operand.native_ref {
                    if !native_ids.contains(target.as_str()) {
                        super::record_finding(
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(constraint.id.as_str()),
                            format_args!("operand native_ref `{target}` does not resolve"),
                        );
                    }
                }
            }
        }
    }
    for constraint in &ir.model.spatial_sketch_constraints {
        if let Some(target) = &constraint.native_ref {
            if !native_ids.contains(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!("native_ref `{target}` does not resolve"),
                );
            }
        }
        if let crate::sketches::SpatialSketchConstraintDefinitionInput::Native {
            operands, ..
        } = constraint.definition.kind()
        {
            for operand in operands {
                if let Some(target) = &operand.native_ref {
                    if !native_ids.contains(target.as_str()) {
                        super::record_finding(
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(constraint.id.as_str()),
                            format_args!("operand native_ref `{target}` does not resolve"),
                        );
                    }
                }
            }
        }
    }

    // `unknowns` has a shared identity-link contract. Other arenas own their
    // field shapes; only an array made entirely of strings follows the generic
    // identity-link convention.
    view.visit::<cadmpeg_core::CodecError>(|_, arena, records| {
        for entity in records.records() {
            let NativeEntity::Product(record) = entity else {
                for target in entity.links() {
                    if !all_targets.contains(&target) {
                        super::record_finding(
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(entity.id()),
                            format_args!("native-record link `{target}` does not resolve"),
                        );
                    }
                }
                continue;
            };

            let Some(value) = record.field("links") else {
                continue;
            };

            if arena != "unknowns"
                && !value
                    .as_array()
                    .is_some_and(|links| links.iter().all(serde_json::Value::is_string))
            {
                continue;
            }
            let serde_json::Value::Array(links) = value else {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(record.id()),
                    format_args!("{}", "native-record links must be an array of identities"),
                );
                continue;
            };
            for (index, link) in links.iter().enumerate() {
                let Some(target) = link.as_str() else {
                    super::record_finding(
                        findings,
                        Check::NativeLinks,
                        Severity::Error,
                        Some(record.id()),
                        format_args!("native-record link {index} must be an identity string"),
                    );
                    continue;
                };
                if !all_targets.contains(target) {
                    super::record_finding(
                        findings,
                        Check::NativeLinks,
                        Severity::Error,
                        Some(record.id()),
                        format_args!("native-record link `{target}` does not resolve"),
                    );
                }
            }
        }
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
