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
        if appearance && !has_binding {
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
        native
            .act_root_components
            .retain(|record| record.stream() != scope || registered(record.record_index));
        let known = native
            .act_entities
            .iter()
            .filter(|record| record.stream() == scope)
            .map(|record| record.record_index())
            .chain(
                native
                    .act_root_components
                    .iter()
                    .filter(|record| record.stream() == scope)
                    .map(|record| record.record_index),
            )
            .chain([table.record_index])
            .collect::<BTreeSet<_>>();
        native
            .act_table_references
            .retain(|record| record.stream() != scope || known.contains(&record.target_record));
        for record in native
            .act_types
            .iter_mut()
            .filter(|record| crate::ids::native_stream(record.id()) == Some(meta_scope.as_str()))
        {
            let entities = record
                .entities
                .values()
                .copied()
                .filter(|index| {
                    u32::try_from(*index)
                        .ok()
                        .is_some_and(|index| known.contains(&index))
                })
                .collect::<Vec<_>>();
            if entities.len() != record.entities.len() {
                losses.push(F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                    "ACT type {} omits unresolved instance references",
                    record.id()
                )));
                record.set_entities(crate::records::identity::ReferenceRun::unlocated(entities));
            }
        }
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

#[cfg(test)]
mod tests;
