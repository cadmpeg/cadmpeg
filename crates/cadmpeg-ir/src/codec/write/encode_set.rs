// SPDX-License-Identifier: Apache-2.0
//! The part of a document that one output of an encode set holds (§3.4,
//! Encode sets).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::OutputNames;
use crate::document::CadIr;
use crate::give_up::{Arena, Seed};
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::products::{ExternalDocument, ProductDefinition, Storage};
use crate::tessellation::Meshed;

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
/// The records of each definition the output does not hold belong to that
/// definition's own output, so this one drops them: the definition's
/// features, parameters, sketches, history bodies, meshes, and design scope
/// with the records of the scope, and each record that needs a record left
/// out, such as the tessellation of a body or an annotation on a face
/// (§3.3).
///
/// # Errors
///
/// Returns a malformed error where the geometry does not serialize, and a
/// resource refusal of `ctx` for the removal of the records.
pub(super) fn output_document(
    ctx: &DecodeContext<'_>,
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
    let gone: BTreeSet<String> = {
        let kept = identities(&out);
        identities(ir)
            .difference(&kept)
            .map(|id| (*id).to_owned())
            .collect()
    };
    let others: BTreeSet<&ProductDefinitionId> = definitions
        .keys()
        .copied()
        .filter(|id| !held.contains(*id))
        .collect();
    let seeds = others_records(ir, &others);
    if !gone.is_empty() || !seeds.is_empty() {
        crate::give_up::drop_records_needing(ctx, &mut out, gone, seeds)?;
    }
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

/// The ids of the geometry, product definitions, and occurrences of `ir`.
fn identities(ir: &CadIr) -> BTreeSet<&str> {
    ir.geometry
        .identities()
        .into_iter()
        .chain(
            ir.model
                .product_definitions
                .iter()
                .map(|stated| stated.id.as_str()),
        )
        .chain(
            ir.model
                .occurrences
                .iter()
                .map(|occurrence| occurrence.id.as_str()),
        )
        .collect()
}

/// The records of `ir` that belong to a definition of `others`: its
/// features with their parameters and history bodies, its own parameters,
/// the sketches it lists, its meshes, and its design scope.
fn others_records(ir: &CadIr, others: &BTreeSet<&ProductDefinitionId>) -> Vec<Seed> {
    let of_other = |definition: Option<&ProductDefinitionId>| {
        definition.is_some_and(|definition| others.contains(definition))
    };
    let model = &ir.model;
    let features: BTreeSet<&str> = model
        .features
        .iter()
        .filter(|feature| of_other(feature.product_definition.as_ref()))
        .map(|feature| feature.id.as_str())
        .collect();
    let seeds = |home: &'static str, field: &'static str, ids: Vec<&str>| {
        let arena = Arena::home(home, field);
        ids.into_iter()
            .filter_map(move |id| {
                Some(Seed {
                    arena: arena.clone()?,
                    id: id.to_owned(),
                })
            })
            .collect::<Vec<_>>()
    };
    let parameters = model
        .parameters
        .iter()
        .filter(|parameter| {
            of_other(parameter.product_definition.as_ref())
                || parameter
                    .owner
                    .as_ref()
                    .is_some_and(|owner| features.contains(owner.as_str()))
        })
        .map(|parameter| parameter.id.as_str())
        .collect();
    let history_bodies = model
        .history_bodies
        .iter()
        .filter(|body| {
            body.produced_by
                .as_ref()
                .is_some_and(|feature| features.contains(feature.as_str()))
        })
        .map(|body| body.id.as_str())
        .collect();
    let sketches = model
        .product_definitions
        .iter()
        .filter(|definition| others.contains(&definition.id))
        .flat_map(|definition| &definition.sketches)
        .map(crate::sketches::SketchId::as_str)
        .collect();
    let tessellations = model
        .tessellations
        .iter()
        .filter(|tessellation| match &tessellation.of {
            Meshed::Display { definition } | Meshed::Definition { definition, .. } => {
                others.contains(definition)
            }
            Meshed::Body { .. } | Meshed::Pmi { .. } | Meshed::Unresolved => false,
        })
        .map(|tessellation| tessellation.id.as_str())
        .collect();
    let scopes = ir
        .design
        .scopes
        .iter()
        .filter(|scope| others.contains(&scope.definition))
        .map(|scope| scope.definition.as_str())
        .collect();
    let mut all = seeds("model", "features", features.into_iter().collect());
    all.extend(seeds("model", "parameters", parameters));
    all.extend(seeds("model", "history_bodies", history_bodies));
    all.extend(seeds("model", "sketches", sketches));
    all.extend(seeds("model", "tessellations", tessellations));
    all.extend(seeds("design", "scopes", scopes));
    all
}
