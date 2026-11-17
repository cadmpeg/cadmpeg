// SPDX-License-Identifier: Apache-2.0
//! The placed instance tree of a design's occurrences.
//!
//! The file's root definition places the top-level occurrences, and every
//! other occurrence sits in an occurrence that places the definition owning
//! it. A definition that more than one occurrence places lists the child
//! occurrences of each, whose identities extend their parent's
//! ([`crate::ids::nested_component_occurrence_id`]), so the parent of each
//! child is recovered exactly.

use cadmpeg_ir::document::Model;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition};

/// The root definition of a decoded file: the file, which places the
/// top-level occurrences.
pub(crate) fn file_root_id() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "product-definition"),
        cadmpeg_ir::identity_key!("file"),
    )
}

/// The file's root definition in `model`, added when it is absent.
pub(crate) fn file_root(model: &mut Model) -> &mut ProductDefinition {
    let id = file_root_id();
    let position = match model
        .product_definitions
        .iter()
        .position(|definition| definition.id == id)
    {
        Some(position) => position,
        None => {
            model
                .product_definitions
                .push(ProductDefinition::root(id, ExternalDocument::default()));
            model.product_definitions.len() - 1
        }
    };
    &mut model.product_definitions[position]
}

/// Places `occurrence` at the top level of the file, after the occurrences
/// already there.
pub(crate) fn place_top_level(model: &mut Model, occurrence: OccurrenceId) {
    let root = file_root(model);
    if !root.occurrences.contains(&occurrence) {
        root.occurrences.push(occurrence);
    }
}

/// Lists `occurrence` among the occurrences of the definition `owner`.
pub(crate) fn place_in(model: &mut Model, owner: &ProductDefinitionId, occurrence: OccurrenceId) {
    if let Some(definition) = model
        .product_definitions
        .iter_mut()
        .find(|definition| &definition.id == owner)
    {
        if !definition.occurrences.contains(&occurrence) {
            definition.occurrences.push(occurrence);
        }
    }
}

/// Lists `child` inside `parent`: among the occurrences of the definition
/// `parent` places, or at the top level when `parent` is `None`.
pub(crate) fn place_under(model: &mut Model, parent: Option<&OccurrenceId>, child: OccurrenceId) {
    let owner = parent.and_then(|parent| {
        model
            .occurrences
            .iter()
            .find(|occurrence| &occurrence.id == parent)
            .map(|occurrence| occurrence.prototype.clone())
    });
    match owner {
        Some(owner) => place_in(model, &owner, child),
        None => place_top_level(model, child),
    }
}

/// Whether `occurrence` sits at the top level: the file's root definition,
/// or no definition, lists it.
pub(crate) fn is_top_level(definitions: &[ProductDefinition], occurrence: &OccurrenceId) -> bool {
    cadmpeg_ir::products::owner(definitions, occurrence).is_none_or(ProductDefinition::is_root)
}

/// The occurrence `occurrence` sits in, or `None` at the top level.
pub(crate) fn parent<'m>(
    definitions: &[ProductDefinition],
    occurrences: &'m [Occurrence],
    occurrence: &OccurrenceId,
) -> Option<&'m Occurrence> {
    let owner = cadmpeg_ir::products::owner(definitions, occurrence)?;
    if owner.is_root() {
        return None;
    }
    let mut placements = occurrences
        .iter()
        .filter(|candidate| candidate.prototype == owner.id);
    let first = placements.next()?;
    let Some(second) = placements.next() else {
        return Some(first);
    };
    std::iter::once(first)
        .chain(std::iter::once(second))
        .chain(placements)
        .find(|candidate| extends(occurrence, &candidate.id))
}

/// The occurrence each nested occurrence sits in.
pub(crate) fn parent_ids(
    definitions: &[ProductDefinition],
    occurrences: &[Occurrence],
) -> std::collections::BTreeMap<OccurrenceId, OccurrenceId> {
    occurrences
        .iter()
        .filter_map(|occurrence| {
            parent(definitions, occurrences, &occurrence.id)
                .map(|parent| (occurrence.id.clone(), parent.id.clone()))
        })
        .collect()
}

/// Whether the identity of `child` extends that of `parent`.
fn extends(child: &OccurrenceId, parent: &OccurrenceId) -> bool {
    child
        .as_str()
        .strip_prefix(parent.as_str())
        .is_some_and(|rest| rest.starts_with(':'))
}
