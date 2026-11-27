// SPDX-License-Identifier: Apache-2.0
//! The part of a document that one output of an encode set holds (§3.4,
//! Encode sets).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::OutputNames;
use crate::document::CadIr;
use crate::features::FeatureId;
use crate::give_up::{Arena, Released, Seed};
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::products::{ExternalDocument, OperandContainer, ProductDefinition, Storage};
use crate::report::loss::LossNote;
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
/// root holds that definition as its root, with the definition's own source.
///
/// The records of each definition the output does not hold belong to that
/// definition's own output, so this one drops them: the definition's
/// features, parameters, sketches, history bodies, meshes, point clouds,
/// and design scope with the records of the scope, and each record that
/// needs a record left out, such as the tessellation of a body or an
/// annotation on a face (§3.3). A native record belongs to the output of
/// the definition whose key qualifies its scope, and to the root output
/// where its scope is unqualified (§2.2). A record that no definition owns belongs to
/// the root output; the output of another definition holds it only where a
/// record that output holds names it. Returns the document with what the
/// removal of the records it leaves out released.
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
) -> Result<(CadIr, Released), CodecError> {
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
    let (others, mut unowned) = unheld_records(ir, &definitions, root, &held);
    if is_root {
        unowned.clear();
    } else {
        let own = definition.and_then(|definition| {
            out.model
                .product_definitions
                .iter_mut()
                .find(|stated| &stated.id == definition)
        });
        if let Some(Storage::File { source, .. }) = own.map(|own| &mut own.storage) {
            out.source = source.take();
        }
    }
    if gone.is_empty() && others.is_empty() && unowned.is_empty() {
        return Ok((out, Released::default()));
    }
    let released = crate::give_up::drop_records_needing(ctx, &mut out, gone, others, unowned)?;
    Ok((out, released))
}

/// What the outputs of an encode set hold of its document: the first output
/// that holds each record, by its position, and, from the removals that
/// planned each output, the records each record named that the output left
/// out, by output, and each record that an output holds in a partial form.
#[derive(Default)]
pub(super) struct Holdings {
    held: BTreeMap<String, usize>,
    needs: BTreeMap<usize, BTreeMap<String, BTreeSet<String>>>,
    partial: BTreeSet<(String, usize)>,
}

/// How an entry of `export:record.unheld` stands to the output it names.
#[derive(Clone, Copy)]
enum Unheld {
    /// The output left the record out.
    Dropped,
    /// The output holds the record in a partial form.
    Partial,
}

impl Holdings {
    /// Adds `document`, the document of the written output at `output`, and
    /// what planning it `released`.
    pub(super) fn add(&mut self, output: usize, document: &CadIr, released: Released) {
        let holds = record_ids(document);
        for record in released
            .marked
            .into_iter()
            .chain(released.states.into_keys().map(|(_, state)| state))
        {
            if holds.contains(&record) {
                self.partial.insert((record, output));
            }
        }
        for id in holds {
            self.held.entry(id).or_insert(output);
        }
        self.needs.insert(output, released.needs);
    }

    /// The `export:record.unheld` losses of the outputs added so far, the
    /// output at each position named by `names`: each record of `ir` that
    /// no output holds, and each record that an output holds in a partial
    /// form, each with the records of other outputs that it needs in each
    /// output that left it out or holds it in part (§3.4, Encode sets).
    pub(super) fn losses(&self, format: &str, ir: &CadIr, names: &[String]) -> Vec<LossNote> {
        let lost = record_ids(ir)
            .into_iter()
            .filter(|id| !self.held.contains_key(id))
            .flat_map(|id| {
                let entries: Vec<String> = self
                    .needs
                    .iter()
                    .filter(|(_, needs)| needs.contains_key(&id))
                    .map(|(output, _)| self.entry(&id, *output, Unheld::Dropped, names))
                    .collect();
                if entries.is_empty() {
                    vec![id]
                } else {
                    entries
                }
            });
        let partial = self
            .partial
            .iter()
            .map(|(id, output)| self.entry(id, *output, Unheld::Partial, names));
        super::loss::record_unheld(
            format,
            "holds these records in no output of the encode set",
            lost,
        )
        .into_iter()
        .chain(super::loss::record_unheld(
            format,
            "holds these records without the records of other outputs they name",
            partial,
        ))
        .collect()
    }

    /// The entry of `record` at the output at `output`, which left it out or
    /// holds it in part: its id, that output's name, and each record of
    /// another output that it needs there, with that output's name. A
    /// record it needs that no output holds stands for the records that one
    /// needs in the same output.
    fn entry(&self, record: &str, output: usize, form: Unheld, names: &[String]) -> String {
        let name = |output: usize| format!("{:?}", names.get(output).map_or("", String::as_str));
        let mut entry = match form {
            Unheld::Dropped => format!("{record} from {}", name(output)),
            Unheld::Partial => format!("{record} in {}", name(output)),
        };
        let Some(needs) = self.needs.get(&output) else {
            return entry;
        };
        let mut causes = BTreeSet::new();
        let mut seen = BTreeSet::from([record]);
        let mut pending: Vec<&str> = vec![record];
        while let Some(id) = pending.pop() {
            for named in needs.get(id).into_iter().flatten() {
                if !seen.insert(named.as_str()) {
                    continue;
                }
                match self.held.get(named) {
                    Some(holder) => {
                        causes.insert(format!("{named} of {}", name(*holder)));
                    }
                    None if needs.contains_key(named) => pending.push(named),
                    None => {
                        causes.insert(named.clone());
                    }
                }
            }
        }
        if !causes.is_empty() {
            entry.push_str(" needs ");
            entry.push_str(&causes.into_iter().collect::<Vec<_>>().join(" and "));
        }
        entry
    }
}

/// The id of each record of `ir`, native records included.
fn record_ids(ir: &CadIr) -> BTreeSet<String> {
    ir.arenas()
        .record_ids()
        .into_values()
        .flat_map(|(_, ids)| ids)
        .chain(
            ir.native
                .0
                .values()
                .flat_map(|namespace| namespace.arenas().values().flatten())
                .map(crate::native::NativeRecord::id),
        )
        .map(str::to_owned)
        .collect()
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

/// Which output holds a record.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The output whose definitions a stated owner names.
    Held,
    /// The output of another definition.
    Other,
    /// No definition owns the record.
    Unowned,
}

/// The records of `ir` that the output of the definitions `held` leaves
/// out: those another definition of `definitions` owns, then those no
/// definition owns. A feature is owned by its product definition; a
/// parameter by its product definition and by its owner feature; a history
/// body by the feature that produced it; a sketch by each definition that
/// lists it; a mesh of a definition, a point cloud, and a design scope by
/// their definition; a record of the `assembly_joints` arena by the
/// definition that lists the occurrences its connectors name, and by the
/// root definition where they name the occurrences of no definition or of
/// several. A connector on `Root {}` names the root of the document that
/// holds the joint, whichever output that is.
fn unheld_records(
    ir: &CadIr,
    definitions: &BTreeMap<&ProductDefinitionId, &ProductDefinition>,
    root: Option<&ProductDefinitionId>,
    held: &BTreeSet<ProductDefinitionId>,
) -> (Vec<Seed>, Vec<Seed>) {
    let of = |definition: Option<&ProductDefinitionId>| match definition {
        Some(definition) if held.contains(definition) => Reach::Held,
        Some(definition) if definitions.contains_key(definition) => Reach::Other,
        Some(_) | None => Reach::Unowned,
    };
    let model = &ir.model;
    let features: BTreeMap<&str, Reach> = model
        .features
        .iter()
        .map(|feature| (feature.id.as_str(), of(feature.product_definition.as_ref())))
        .collect();
    let feature = |id: Option<&str>| {
        id.and_then(|id| features.get(id).copied())
            .unwrap_or(Reach::Unowned)
    };
    let parameters = model.parameters.iter().map(|parameter| {
        let owner = feature(parameter.owner.as_ref().map(FeatureId::as_str));
        let reach = match of(parameter.product_definition.as_ref()) {
            Reach::Other => Reach::Other,
            Reach::Held if owner == Reach::Other => Reach::Other,
            Reach::Held => Reach::Held,
            Reach::Unowned => owner,
        };
        (parameter.id.as_str(), reach)
    });
    let history_bodies = model.history_bodies.iter().map(|body| {
        (
            body.id.as_str(),
            feature(body.produced_by.as_ref().map(FeatureId::as_str)),
        )
    });
    let listing: BTreeMap<&str, Reach> = model
        .product_definitions
        .iter()
        .flat_map(|definition| {
            let reach = of(Some(&definition.id));
            definition
                .sketches
                .iter()
                .map(move |sketch| (sketch.as_str(), reach))
        })
        .fold(BTreeMap::new(), |mut listing, (sketch, reach)| {
            let entry = listing.entry(sketch).or_insert(reach);
            if reach == Reach::Held {
                *entry = Reach::Held;
            }
            listing
        });
    let sketches = model.sketches.iter().map(|sketch| {
        let id = sketch.id.as_str();
        (id, listing.get(id).copied().unwrap_or(Reach::Unowned))
    });
    let tessellations = model.tessellations.iter().filter_map(|tessellation| {
        let reach = match &tessellation.of {
            Meshed::Display { definition } | Meshed::Definition { definition, .. } => {
                of(Some(definition))
            }
            Meshed::Unresolved => Reach::Unowned,
            Meshed::Body { .. } | Meshed::Pmi { .. } => return None,
        };
        Some((tessellation.id.as_str(), reach))
    });
    let point_clouds = model
        .point_clouds
        .iter()
        .map(|cloud| (cloud.id.as_str(), of(Some(&cloud.definition))));
    let scopes = ir
        .design
        .scopes
        .iter()
        .map(|scope| (scope.definition.as_str(), of(Some(&scope.definition))));
    let placing: BTreeMap<&OccurrenceId, &ProductDefinitionId> = model
        .product_definitions
        .iter()
        .flat_map(|definition| {
            definition
                .occurrences
                .iter()
                .map(move |occurrence| (occurrence, &definition.id))
        })
        .collect();
    let joints = model.assembly_joints.iter().map(|joint| {
        let named: BTreeSet<Option<&ProductDefinitionId>> = joint
            .connectors()
            .filter_map(|connector| match &connector.operand.container {
                OperandContainer::Occurrence { occurrence, .. } => {
                    Some(placing.get(occurrence).copied())
                }
                OperandContainer::Root {} | OperandContainer::External { .. } => None,
            })
            .collect();
        let owner = match named.into_iter().collect::<Vec<_>>().as_slice() {
            [owner @ Some(_)] => *owner,
            _ => root,
        };
        (joint.id.as_str(), of(owner))
    });
    let mut others = Vec::new();
    let mut unowned = Vec::new();
    let mut sort = |home: &'static str,
                    field: &'static str,
                    records: &mut dyn Iterator<Item = (&str, Reach)>| {
        let Some(arena) = Arena::home(home, field) else {
            return;
        };
        for (id, reach) in records {
            let seed = Seed {
                arena: arena.clone(),
                id: id.to_owned(),
            };
            match reach {
                Reach::Held => {}
                Reach::Other => others.push(seed),
                Reach::Unowned => unowned.push(seed),
            }
        }
    };
    sort(
        "model",
        "features",
        &mut features.iter().map(|(id, reach)| (*id, *reach)),
    );
    sort("model", "parameters", &mut { parameters });
    sort("model", "history_bodies", &mut { history_bodies });
    sort("model", "sketches", &mut { sketches });
    sort("model", "tessellations", &mut { tessellations });
    sort("model", "point_clouds", &mut { point_clouds });
    sort("design", "scopes", &mut { scopes });
    sort("model", "assembly_joints", &mut { joints });
    let files: BTreeMap<&str, &ProductDefinitionId> = definitions
        .iter()
        .filter(|(_, definition)| matches!(definition.storage, Storage::File { .. }))
        .filter_map(|(id, _)| Some((id.as_str().split_once('#')?.1, *id)))
        .collect();
    for (codec, namespace) in &ir.native.0 {
        for (arena, records) in namespace.arenas() {
            for record in records {
                let id = record.id();
                let scope = id.split(':').nth(1).unwrap_or_default();
                let reach = match scope.split_once('@') {
                    Some((key, _)) => of(files.get(key).copied()),
                    None => of(root),
                };
                let seed = Seed {
                    arena: Arena::Native {
                        codec: codec.as_str().to_owned(),
                        arena: arena.as_str().to_owned(),
                    },
                    id: id.to_owned(),
                };
                match reach {
                    Reach::Held => {}
                    Reach::Other => others.push(seed),
                    Reach::Unowned => unowned.push(seed),
                }
            }
        }
    }
    (others, unowned)
}
