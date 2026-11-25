// SPDX-License-Identifier: Apache-2.0
//! Validation of borrowed annotations and native product links.

use crate::document::CadIr;
use crate::index::identities::{BorrowedIdentities, IdentitySet};
use crate::native::view::{NativeEntity, NativeView};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

enum AnnotatedEntity<'ir> {
    Projected(serde_json::Value),
    Product(&'ir crate::native::NativeRecord),
    Source(&'ir crate::unknown::UnknownRecord),
}

macro_rules! define_model_entity_projection {
    ($( $field:ident: $element:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        fn model_entity_projection<'ir>(
            ctx: &DecodeContext<'_>,
            ir: &'ir CadIr,
            wanted: &BorrowedIdentities<'_, bool>,
            add: &mut dyn FnMut(&'ir str, AnnotatedEntity<'ir>),
        ) -> Result<(), CodecError> {
            $(for entity in &ir.$home.$field {
                let id = crate::schema::EntitySchema::identity(entity);
                if wanted.contains(id) {
                    let value = match crate::schema::structural::project(ctx, entity, "annotated entity projection") {
                        Ok(value) => value,
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => continue,
                    };
                    add(id, AnnotatedEntity::Projected(value));
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
    all_ids: &dyn IdentitySet,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut wanted = BorrowedIdentities::build(|add| {
        for (id, note) in annotations.exactness() {
            if !note.fields().is_empty() {
                add(id, false);
            }
        }
    });
    let mut values = Vec::new();
    let mut entities = BorrowedIdentities::new();
    // Sort small identity slots rather than moving projected payloads.
    let mut add = |id, value| {
        entities.insert(id, values.len());
        values.push(value);
    };
    view.for_each(|_, _, records| {
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
            add(record.id(), value);
        }
    });
    // Model entities take precedence over native records with the same id.
    // The identity index keeps the last inserted matching value.
    model_entity_projection(ctx, view.ir, &wanted, &mut add)?;
    for id in annotations.provenance.keys() {
        if !all_ids.contains(id) {
            super::record_finding(
                findings,
                Check::Annotations,
                Severity::Error,
                Some(id),
                crate::rule!(
                    "annotations_native.provenance-key-does-not-resolve-entity",
                    "provenance key does not resolve to an entity"
                ),
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
                crate::rule!(
                    "annotations_native.exactness-key-does-not-resolve-entity",
                    "exactness key does not resolve to an entity"
                ),
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
                crate::rule!(
                    "annotations_native.entity-could-not-serialized-validate-exactness",
                    "entity could not be serialized to validate its exactness field paths"
                ),
            );
            continue;
        };
        let entity = &values[*position];
        let product_fields = match entity {
            AnnotatedEntity::Product(product) => Some(product.fields()?),
            AnnotatedEntity::Projected(_) | AnnotatedEntity::Source(_) => None,
        };
        for path in note.fields().keys() {
            let resolves = match entity {
                AnnotatedEntity::Projected(value) => field_path_resolves(value, path.as_str()),
                AnnotatedEntity::Product(_) => product_fields
                    .as_ref()
                    .is_some_and(|fields| native_field_path_resolves(fields, path.as_str())),
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
                    crate::rule!(
                        "annotations_native.exactness-field-path-does-not-resolve",
                        "exactness field path `{path}` does not resolve"
                    ),
                );
            }
        }
    }
    Ok(())
}

/// Whether `path` names a member of a native record, given the record's
/// codec-owned fields.
fn native_field_path_resolves(
    fields: &serde_json::Map<String, serde_json::Value>,
    path: &str,
) -> bool {
    let mut components = path.split('.');
    let Some(first) = components.next() else {
        return false;
    };
    if first == "id" {
        return components.next().is_none();
    }

    let Some(mut value) = fields.get(first) else {
        return false;
    };
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

fn field_path_resolves(mut value: &serde_json::Value, path: &str) -> bool {
    for component in path.split('.') {
        let next = match value {
            serde_json::Value::Object(object) => object.get(component),
            serde_json::Value::Array(array) => component
                .parse::<usize>()
                .ok()
                .and_then(|index| array.get(index)),
            _ => None,
        };
        let Some(next) = next else {
            return false;
        };
        value = next;
    }
    true
}

/// Whether native link `target` resolves: `owns` holds it, or the document
/// holds no native namespace of its format, so the link is provenance of a
/// source whose native records the document does not carry.
pub(super) fn native_link_resolves(ir: &CadIr, owns: impl Fn(&str) -> bool, target: &str) -> bool {
    owns(target)
        || target
            .split_once(':')
            .is_none_or(|(format, _)| ir.native.namespace(format).is_none())
}

pub(super) fn check_native_links(
    view: NativeView<'_>,
    all_ids: &crate::index::ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let ir = view.ir;
    let resolves = |target: &str| {
        native_link_resolves(ir, |target| all_ids.native_record_owns(target), target)
    };
    for feature in &ir.model.features {
        if let Some(target) = &feature.native_ref {
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    crate::rule!(
                        "annotations_native.native-ref-does-not-resolve",
                        "native_ref `{target}` does not resolve"
                    ),
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
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    crate::rule!(
                        "annotations_native.helix-axis-native-ref-does-not",
                        "helix axis native_ref `{target}` does not resolve"
                    ),
                );
            }
        }
    }
    for parameter in &ir.model.parameters {
        if let Some(target) = &parameter.native_ref {
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(parameter.id.as_str()),
                    crate::rule!(
                        "annotations_native.native-ref-does-not-resolve",
                        "native_ref `{target}` does not resolve"
                    ),
                );
            }
        }
        if let Some(semantic) = &parameter.pmi {
            if !resolves(semantic.native_ref.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(parameter.id.as_str()),
                    crate::rule!(
                        "annotations_native.pmi-native-ref-does-not-resolve",
                        "PMI native_ref `{}` does not resolve",
                        semantic.native_ref
                    ),
                );
            }
        }
    }
    for configuration in &ir.model.configurations {
        if let Some(target) = &configuration.native_ref {
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(configuration.id.as_str()),
                    crate::rule!(
                        "annotations_native.native-ref-does-not-resolve",
                        "native_ref `{target}` does not resolve"
                    ),
                );
            }
        }
    }
    for sketch in &ir.model.sketches {
        if let Some(target) = &sketch.native_ref {
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(sketch.id.as_str()),
                    crate::rule!(
                        "annotations_native.native-ref-does-not-resolve",
                        "native_ref `{target}` does not resolve"
                    ),
                );
            }
        }
    }
    for sketch in &ir.model.spatial_sketches {
        if let Some(target) = &sketch.native_ref {
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(sketch.id.as_str()),
                    crate::rule!(
                        "annotations_native.native-ref-does-not-resolve",
                        "native_ref `{target}` does not resolve"
                    ),
                );
            }
        }
    }
    for constraint in &ir.model.sketch_constraints {
        if let Some(target) = &constraint.native_ref {
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(constraint.id.as_str()),
                    crate::rule!(
                        "annotations_native.native-ref-does-not-resolve",
                        "native_ref `{target}` does not resolve"
                    ),
                );
            }
        }
        if let crate::sketches::SketchConstraintDefinitionInput::Native { operands, .. } =
            constraint.definition.kind()
        {
            for operand in operands {
                if let Some(target) = &operand.native_ref {
                    if !resolves(target.as_str()) {
                        super::record_finding(
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(constraint.id.as_str()),
                            crate::rule!(
                                "annotations_native.operand-native-ref-does-not-resolve",
                                "operand native_ref `{target}` does not resolve"
                            ),
                        );
                    }
                }
            }
        }
    }
    for constraint in &ir.model.spatial_sketch_constraints {
        if let Some(target) = &constraint.native_ref {
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(constraint.id.as_str()),
                    crate::rule!(
                        "annotations_native.native-ref-does-not-resolve",
                        "native_ref `{target}` does not resolve"
                    ),
                );
            }
        }
        if let crate::sketches::SpatialSketchConstraintDefinitionInput::Native {
            operands, ..
        } = constraint.definition.kind()
        {
            for operand in operands {
                if let Some(target) = &operand.native_ref {
                    if !resolves(target.as_str()) {
                        super::record_finding(
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(constraint.id.as_str()),
                            crate::rule!(
                                "annotations_native.operand-native-ref-does-not-resolve",
                                "operand native_ref `{target}` does not resolve"
                            ),
                        );
                    }
                }
            }
        }
    }
    for mesh in &ir.model.tessellations {
        if let Some(target) = &mesh.native_ref {
            if !resolves(target.as_str()) {
                super::record_finding(
                    findings,
                    Check::NativeLinks,
                    Severity::Error,
                    Some(mesh.id.as_str()),
                    crate::rule!(
                        "annotations_native.native-ref-does-not-resolve",
                        "native_ref `{target}` does not resolve"
                    ),
                );
            }
        }
    }

    // `unknowns` has a shared identity-link contract. Other arenas own their
    // field shapes; only an array made entirely of strings follows the generic
    // identity-link convention.
    view.for_each(|_, arena, records| {
        for entity in records.records() {
            let NativeEntity::Product(record) = entity else {
                for target in entity.links() {
                    if !all_ids.contains(&target, crate::index::StandardIndex) {
                        super::record_finding(
                            findings,
                            Check::NativeLinks,
                            Severity::Error,
                            Some(entity.id()),
                            crate::rule!(
                                "annotations_native.native-record-link-does-not-resolve",
                                "native-record link `{target}` does not resolve"
                            ),
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
                    crate::rule!(
                        "annotations_native.native-record-links-must-array-identities",
                        "{}",
                        "native-record links must be an array of identities"
                    ),
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
                        crate::rule!(
                            "annotations_native.native-record-link-must-identity-string",
                            "native-record link {index} must be an identity string"
                        ),
                    );
                    continue;
                };
                check_native_record_link(all_ids, arena, record.id(), target, findings);
            }
        }
    });
}

/// A native record's link resolves, and to a native record: native records
/// carry no reference into the neutral model (§2.1 Linkage, part 10). A
/// given-up part's `unknowns` stub links its innermost containing record,
/// which may be neutral (§3.3).
fn check_native_record_link(
    all_ids: &crate::index::ModelIndex<'_>,
    arena: &str,
    record: &str,
    target: &str,
    findings: &mut Vec<Finding>,
) {
    if !all_ids.contains(target, crate::index::StandardIndex) {
        super::record_finding(
            findings,
            Check::NativeLinks,
            Severity::Error,
            Some(record),
            crate::rule!(
                "annotations_native.native-record-link-does-not-resolve",
                "native-record link `{target}` does not resolve"
            ),
        );
    } else if arena != "unknowns" && !all_ids.native_record_owns(target) {
        super::record_finding(
            findings,
            Check::NativeLinks,
            Severity::Error,
            Some(record),
            crate::rule!(
                "annotations_native.native-record-link-names-neutral-record",
                "native-record link `{target}` names a neutral record; a native record references no record of the neutral model"
            ),
        );
    }
}

#[cfg(test)]
mod tests;
