// SPDX-License-Identifier: Apache-2.0
//! Reconcile ACT metadata with held neutral product labels and typed references.

use crate::{loss::F3dLossCode, native::F3dNative};
use cadmpeg_core::CodecError;
use cadmpeg_ir::products::ProductDefinitionKind;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::BTreeSet;

pub(super) fn reconcile(
    ir: &CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut stream_records = std::collections::BTreeMap::<String, Vec<String>>::new();
    for id in native
        .act_tables
        .iter()
        .map(|record| record.id.clone())
        .chain(native.act_types.iter().map(|record| record.id().to_owned()))
        .chain(native.act_entities.iter().map(|record| record.id().clone()))
        .chain(native.act_guids.iter().map(|record| record.id().to_owned()))
        .chain(
            native
                .act_registry_channels
                .iter()
                .map(|record| record.id().to_owned()),
        )
        .chain(
            native
                .act_root_components
                .iter()
                .map(|record| record.id().to_owned()),
        )
        .chain(
            native
                .act_table_references
                .iter()
                .map(|record| record.id().to_owned()),
        )
    {
        let Some(scope) = crate::ids::native_stream(&id) else {
            continue;
        };
        let scope = scope.strip_suffix("MetaStream.dat").map_or_else(
            || scope.to_owned(),
            |prefix| format!("{prefix}BulkStream.dat"),
        );
        stream_records.entry(scope).or_default().push(id);
    }
    let all_bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<BTreeSet<_>>();
    let mut roots = Vec::new();
    for mut root in std::mem::take(&mut native.act_root_components) {
        if ir.scope.model {
            let Some(product) = ir
                .model
                .product_definitions
                .iter()
                .find(|product| product.native_ref.as_deref() == Some(root.id().as_str()))
            else {
                continue;
            };
            if product.kind != ProductDefinitionKind::Part
                || product.bodies.iter().collect::<BTreeSet<_>>() != all_bodies
            {
                losses.push(F3dLossCode::WriterProductOmitted.note(format!(
                    "product {} has no standalone ACT root encoding",
                    product.id
                )));
                continue;
            }
            let name = root.layout().entity_id().to_owned();
            if product.source_name.is_some()
                || product.description.is_some()
                || product.part_number.is_some()
                || !product.bom_properties.is_empty()
            {
                losses.push(F3dLossCode::WriterProductOmitted.note(format!("product {} keeps its label and body membership; ACT has no field for its additional BOM properties", product.id)));
            }
            let label = product.label.as_deref().unwrap_or("").to_owned();
            root.try_set_strings(name, label)
                .map_err(CodecError::Malformed)?;
        }
        roots.push(root);
    }
    native.act_root_components = roots;
    // Appearance channel values have a canonical neutral binding home. If
    // preparation omitted that binding, its native carrier cannot provide the
    // omitted fact or resurrect a stale channel value.
    native.act_entities.retain(|record| {
        let appearance = record.channel_group().channels().contains_key("Appearance");
        let has_binding = ir
            .model
            .appearance_bindings
            .iter()
            .any(|binding| binding.source_entity_id.as_deref() == Some(record.entity_id()));
        if ir.scope.model && appearance && !has_binding {
            losses.push(F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                "ACT entity {} is omitted with its unsupported canonical appearance binding",
                record.id(),
            )));
            false
        } else {
            true
        }
    });
    let mut tables = Vec::new();
    for table in std::mem::take(&mut native.act_tables) {
        let Some(scope) = crate::ids::native_stream(&table.id) else {
            continue;
        };
        let Some(prefix) = scope.strip_suffix("BulkStream.dat") else {
            continue;
        };
        let meta_scope = format!("{prefix}MetaStream.dat");
        // A root's current index and dynamic class are typed content. An
        // explicit index edit does not require a caller to edit the redundant
        // MetaStream membership list too. Register that actual root in its
        // declared class, without reassigning an index held by another class.
        for root in native
            .act_root_components
            .iter()
            .filter(|record| record.stream() == scope)
        {
            if native.act_types.iter().any(|record| {
                crate::ids::native_stream(record.id()) == Some(meta_scope.as_str())
                    && record
                        .entities
                        .values()
                        .any(|index| *index == u64::from(root.record_index))
            }) {
                continue;
            }
            let mut owners = native.act_types.iter_mut().filter(|record| {
                crate::ids::native_stream(record.id()) == Some(meta_scope.as_str())
                    && &record.class_tag == root.class_tag()
            });
            let Some(owner) = owners.next() else {
                continue;
            };
            if owners.next().is_some() {
                continue;
            }
            let entities = owner
                .entities
                .values()
                .copied()
                .chain([u64::from(root.record_index)])
                .collect();
            owner.set_entities(crate::records::identity::ReferenceRun::unlocated(entities));
        }
        let types = native
            .act_types
            .iter()
            .filter(|record| crate::ids::native_stream(record.id()) == Some(meta_scope.as_str()))
            .collect::<Vec<_>>();
        let registered = |index| {
            types
                .iter()
                .filter(|record| {
                    record
                        .entities
                        .values()
                        .any(|entity| *entity == u64::from(index))
                })
                .count()
                == 1
        };
        if !registered(table.record_index) {
            losses.push(F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                "ACT table {} has no resolved typed class registration",
                table.id
            )));
            continue;
        }
        native
            .act_entities
            .retain(|record| record.stream() != scope || registered(record.record_index()));
        native.act_root_components.retain(|record| {
            record.stream() != scope
                || (registered(record.record_index)
                    && types.iter().any(|owner| {
                        &owner.class_tag == record.class_tag()
                            && owner
                                .entities
                                .values()
                                .any(|index| *index == u64::from(record.record_index))
                    }))
        });
        if native
            .act_root_components
            .iter()
            .filter(|record| record.stream() == scope)
            .count()
            != 1
        {
            let omitted = stream_records
                .get(scope)
                .map_or_else(String::new, |ids| ids.join(", "));
            losses.push(F3dLossCode::WriterActStreamOmitted.note(format!(
                "ACT stream {scope} has no unique encodable current document-root component after canonical reconciliation; omitted dependent typed records [{omitted}]"
            )));
            continue;
        }
        let known = native
            .act_entities
            .iter()
            .filter(|record| record.stream() == scope)
            .map(crate::records::act::ActEntity::record_index)
            .chain(
                native
                    .act_root_components
                    .iter()
                    .filter(|record| record.stream() == scope)
                    .map(|record| record.record_index),
            )
            .chain([table.record_index])
            .collect::<BTreeSet<_>>();
        native.act_table_references.retain(|record| {
            if record.stream() != scope || known.contains(&record.target_record) {
                return true;
            }
            losses.push(F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                "ACT table reference {} omits unresolved current record index {}",
                record.id(),
                record.target_record
            )));
            false
        });
        tables.push(table);
    }
    native.act_tables = tables;
    let scopes = native
        .act_tables
        .iter()
        .filter_map(|record| crate::ids::native_stream(&record.id))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    native
        .act_entities
        .retain(|record| scopes.contains(record.stream()));
    native
        .act_guids
        .retain(|record| scopes.contains(record.stream()));
    native
        .act_registry_channels
        .retain(|record| scopes.contains(record.stream()));
    native
        .act_root_components
        .retain(|record| scopes.contains(record.stream()));
    native
        .act_table_references
        .retain(|record| scopes.contains(record.stream()));
    let meta_scopes = scopes
        .iter()
        .filter_map(|scope| scope.strip_suffix("BulkStream.dat"))
        .map(|prefix| format!("{prefix}MetaStream.dat"))
        .collect::<BTreeSet<_>>();
    native.act_types.retain(|record| {
        crate::ids::native_stream(record.id()).is_some_and(|scope| meta_scopes.contains(scope))
    });
    Ok(())
}

/// Limit the emitted registry to records the typed ACT writer actually emits.
/// An excluded neutral scope does not erase native registration metadata;
/// this later encoding reduction names every membership without a payload.
pub(super) fn prepare_registrations(native: &mut F3dNative, losses: &mut Vec<LossNote>) {
    let mut written = std::collections::BTreeMap::<String, BTreeSet<u64>>::new();
    for table in &native.act_tables {
        let Some(scope) = crate::ids::native_stream(&table.id) else {
            continue;
        };
        let records = written.entry(scope.to_owned()).or_default();
        records.insert(u64::from(table.record_index));
        records.extend(
            native
                .act_entities
                .iter()
                .filter(|record| record.stream() == scope)
                .map(|record| u64::from(record.record_index())),
        );
        records.extend(
            native
                .act_root_components
                .iter()
                .filter(|record| record.stream() == scope)
                .map(|record| u64::from(record.record_index)),
        );
    }
    for record in &mut native.act_types {
        let Some(scope) = crate::ids::native_stream(record.id()) else {
            continue;
        };
        let Some(prefix) = scope.strip_suffix("MetaStream.dat") else {
            continue;
        };
        let Some(known) = written.get(&format!("{prefix}BulkStream.dat")) else {
            continue;
        };
        let (entities, omitted): (Vec<_>, Vec<_>) = record
            .entities
            .values()
            .copied()
            .partition(|index| known.contains(index));
        if !omitted.is_empty() {
            losses.push(F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                "ACT type {} omits unwritten instance record indices {omitted:?}",
                record.id()
            )));
            record.set_entities(crate::records::identity::ReferenceRun::unlocated(entities));
        }
    }
}

#[cfg(test)]
mod tests;
