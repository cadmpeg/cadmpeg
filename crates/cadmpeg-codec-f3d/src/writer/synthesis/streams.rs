// SPDX-License-Identifier: Apache-2.0
//! Native Design identities are local to their containing stream pair.

use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::native::F3dNative;

fn scope(id: &str) -> &str {
    let stream = crate::ids::native_stream(id).unwrap_or(crate::ids::DEFAULT_STREAM);
    stream
        .strip_suffix("/MetaStream.dat")
        .or_else(|| stream.strip_suffix("/BulkStream.dat"))
        .unwrap_or(stream)
}

fn scopes(native: &F3dNative) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    macro_rules! add {
        ($field:ident, $record:ident => $id:expr) => {
            for $record in &native.$field {
                result.insert(scope($id).to_owned());
            }
        };
    }
    add!(design_types, r => r.id().as_str());
    add!(design_entity_headers, r => r.id.as_str());
    add!(design_record_headers, r => r.id.as_str());
    add!(construction_recipes, r => r.id.as_str());
    add!(sketch_points, r => r.id.as_str());
    add!(sketch_curve_identities, r => r.id.as_str());
    add!(sketch_relations, r => r.id.as_str());
    add!(sketch_texts, r => r.id.as_str());
    result
}

pub(super) fn validate(native: &F3dNative) -> Result<(), CodecError> {
    use crate::writer::generate::preconditions::{
        validate_source_less_design_ownership, validate_source_less_recipes,
        validate_source_less_sketch_graph,
    };
    for stream in scopes(native) {
        let mut local = F3dNative::default();
        macro_rules! copy {
            ($field:ident, $record:ident => $id:expr) => {
                local.$field = native
                    .$field
                    .iter()
                    .filter(|$record| scope($id) == stream)
                    .cloned()
                    .collect();
            };
        }
        copy!(design_types, r => r.id().as_str());
        copy!(design_entity_headers, r => r.id.as_str());
        copy!(design_record_headers, r => r.id.as_str());
        copy!(sketch_points, r => r.id.as_str());
        copy!(sketch_curve_identities, r => r.id.as_str());
        copy!(sketch_relations, r => r.id.as_str());
        copy!(sketch_texts, r => r.id.as_str());
        copy!(construction_recipes, r => r.id.as_str());
        local
            .design_types
            .sort_by(|left, right| left.class_tag.as_str().cmp(right.class_tag.as_str()));
        for record in &local.design_types {
            if record.module.len() == 36 && crate::bytes::is_guid_prefix(&record.module) {
                return Err(CodecError::malformed(format_args!(
                    "Design type module name is GUID-shaped: {}",
                    record.module,
                )));
            }
        }
        for result in [
            validate_source_less_design_ownership(&local),
            validate_source_less_sketch_graph(&local),
            validate_source_less_recipes(&local),
        ] {
            match result {
                Err(error) if super::declines(&error) => {}
                other => other?,
            }
        }
    }
    Ok(())
}

/// A single generated registry cannot distinguish colliding source-local
/// identities. Keep independently compatible contexts and report the others.
pub(super) fn reconcile(native: &mut F3dNative, losses: &mut Vec<LossNote>) {
    let mut guids = BTreeSet::new();
    let mut entities = BTreeSet::new();
    let mut recipes = Vec::new();
    let mut omitted = BTreeSet::new();
    for stream in scopes(native) {
        let types = native
            .design_types
            .iter()
            .filter(|r| scope(r.id()) == stream)
            .collect::<Vec<_>>();
        let own_recipes = native
            .construction_recipes
            .iter()
            .filter(|r| scope(&r.id) == stream)
            .collect::<Vec<_>>();
        let conflict = types.iter().any(|record| {
            guids.contains(&record.type_guid.as_str().to_ascii_lowercase())
                || record
                    .entities
                    .values()
                    .any(|entity| entities.contains(entity))
        }) || own_recipes.iter().any(|record| {
            recipes.iter().any(|(kind, id)| {
                *kind == record.kind
                    && *id == record.design.as_ref().map(|design| design.id.value.clone())
            })
        });
        if conflict {
            omitted.insert(stream);
            continue;
        }
        for record in types {
            guids.insert(record.type_guid.as_str().to_ascii_lowercase());
            entities.extend(record.entities.values().copied());
        }
        for recipe in own_recipes {
            recipes.push((
                recipe.kind,
                recipe.design.as_ref().map(|design| design.id.value.clone()),
            ));
        }
    }
    if omitted.is_empty() {
        return;
    }
    macro_rules! retain {
        ($field:ident, $record:ident => $id:expr) => {
            native
                .$field
                .retain(|$record| !omitted.contains(scope($id)));
        };
    }
    retain!(design_types, r => r.id().as_str());
    retain!(design_entity_headers, r => r.id.as_str());
    retain!(design_record_headers, r => r.id.as_str());
    retain!(construction_recipes, r => r.id.as_str());
    native
        .persistent_references
        .retain(|r| !omitted.contains(scope(&r.id)));
    native
        .lost_edge_references
        .retain(|r| !omitted.contains(scope(&r.id)));
    native
        .design_body_members
        .retain(|r| !omitted.contains(scope(r.id())));
    losses.push(crate::loss::F3dLossCode::WriterNativeRecordsOmitted.note(
        "native Design metadata with colliding stream-local identities is omitted from the single generated registry",
    ));
}
