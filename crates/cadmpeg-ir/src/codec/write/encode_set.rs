// SPDX-License-Identifier: Apache-2.0
//! The part of a document that one output of an encode set holds (§3.4,
//! Encode sets).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

use super::OutputNames;
use crate::document::CadIr;
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::products::{ExternalDocument, ProductDefinition, Storage};

/// The document that the output of `definition` holds, where `root` is the
/// root definition of `ir` and `names` holds the caller's output names.
///
/// The output holds `definition`, each `Embedded` definition that an
/// occurrence it holds places, and their bodies. The root output also holds
/// each `Embedded` definition that no occurrence places, and the bodies that
/// no definition lists. Each other definition that an occurrence of the
/// output places stays as a link: a `File` definition becomes an `External`
/// one named by its output's name. The output of a definition other than the
/// root holds that definition as its root, with the definition's own source
/// and no native records.
///
/// # Errors
///
/// Returns a malformed error where the geometry does not serialize.
pub(super) fn output_document(
    ir: &CadIr,
    definition: Option<&ProductDefinitionId>,
    root: Option<&ProductDefinitionId>,
    names: &OutputNames,
) -> Result<CadIr, CodecError> {
    let model = &ir.model;
    let definitions: BTreeMap<&ProductDefinitionId, &ProductDefinition> = model
        .product_definitions
        .iter()
        .map(|definition| (&definition.id, definition))
        .collect();
    let prototypes: BTreeMap<&OccurrenceId, &ProductDefinitionId> = model
        .occurrences
        .iter()
        .map(|occurrence| (&occurrence.id, &occurrence.prototype))
        .collect();
    let is_root = definition == root;
    let mut held: BTreeSet<&ProductDefinitionId> = definition.into_iter().collect();
    if is_root {
        held.extend(
            crate::products::unplaced_definitions(model)
                .filter(|definition| matches!(definition.storage, Storage::Embedded))
                .map(|definition| &definition.id),
        );
    }
    let mut pending: Vec<&ProductDefinitionId> = held.iter().copied().collect();
    let mut linked = BTreeSet::new();
    let mut occurrences = BTreeSet::new();
    while let Some(id) = pending.pop() {
        let Some(holder) = definitions.get(id) else {
            continue;
        };
        for occurrence in &holder.occurrences {
            occurrences.insert(occurrence.clone());
            let Some(prototype) = prototypes
                .get(occurrence)
                .and_then(|prototype| definitions.get(prototype))
            else {
                continue;
            };
            if !matches!(prototype.storage, Storage::Embedded) {
                linked.insert(&prototype.id);
            } else if held.insert(&prototype.id) {
                pending.push(&prototype.id);
            }
        }
    }
    let listed = |definition: &'_ ProductDefinition| -> Vec<BodyId> {
        definition
            .bodies
            .iter()
            .chain(
                definition
                    .instance_bodies
                    .iter()
                    .flat_map(|instance| &instance.bodies),
            )
            .cloned()
            .collect()
    };
    let kept: BTreeSet<BodyId> = held
        .iter()
        .filter_map(|id| definitions.get(id))
        .flat_map(|definition| listed(definition))
        .collect();
    let owned: BTreeSet<BodyId> = model.product_definitions.iter().flat_map(listed).collect();
    let removed: BTreeSet<&BodyId> = ir
        .geometry
        .bodies
        .iter()
        .map(|body| &body.id)
        .filter(|body| !kept.contains(*body) && (!is_root || owned.contains(*body)))
        .collect();
    let mut out = if removed.is_empty() {
        ir.clone()
    } else {
        super::without_faces_and_bodies(
            ir,
            &BTreeSet::new(),
            &removed.iter().map(|body| body.as_str()).collect(),
        )?
    };
    super::cages::forget_bodies(&mut out, &removed);
    let held: BTreeSet<ProductDefinitionId> = held.into_iter().cloned().collect();
    let linked: BTreeSet<ProductDefinitionId> = linked
        .into_iter()
        .filter(|id| !held.contains(*id))
        .cloned()
        .collect();
    out.model
        .product_definitions
        .retain(|definition| held.contains(&definition.id) || linked.contains(&definition.id));
    for definition in &mut out.model.product_definitions {
        if !linked.contains(&definition.id) || !matches!(definition.storage, Storage::File { .. }) {
            continue;
        }
        let Some(name) = names.get(&definition.id) else {
            continue;
        };
        definition.storage = Storage::External(ExternalDocument::path(name.as_str()));
        definition.bodies.clear();
        definition.occurrences.clear();
        definition.instance_bodies.clear();
        definition.sketches.clear();
    }
    out.model
        .occurrences
        .retain(|occurrence| occurrences.contains(&occurrence.id));
    out.design.scopes.retain(|scope| {
        held.contains(&scope.definition) || !definitions.contains_key(&scope.definition)
    });
    if !is_root {
        let own = definition.and_then(|definition| {
            out.model
                .product_definitions
                .iter_mut()
                .find(|stated| &stated.id == definition)
        });
        if let Some(Storage::File { source, .. }) = own.map(|own| &mut own.storage) {
            out.source = source.take();
        }
        out.native = crate::native::Native::default();
    }
    Ok(out)
}
