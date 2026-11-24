// SPDX-License-Identifier: Apache-2.0
//! The v11 design of a Rhino document's history records (§5.2: the records
//! are members of one unordered `Group`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::refs::{EntityRef, Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{Anchor, Members, PickTarget, Picked, Picks, Selection};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::CadIr;

use super::{feature_id_text, native_record_id, record_keys, HistoryRecord, Value};
use crate::wire::Uuid;

/// The bodies each object of the object table committed, keyed by the
/// object's UUID, for the objects whose UUID no other object holds.
pub(crate) type ObjectBodies = HashMap<Uuid, Vec<BodyId>>;

/// States the design scope of the root definition `root` (stated here where
/// the document holds none), its history group, one `Native` feature per
/// record with its antecedents as selections, and the current state each
/// record's descendants hold.
///
/// A document with no history record has no design records. An object that
/// no record lists among its descendants is in no current state, and the
/// scoped root lists no body (interim, errata A1).
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    records: &[HistoryRecord],
    objects: &ObjectBodies,
    root: Option<ProductDefinitionId>,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    if records.is_empty() {
        return Ok(());
    }
    let definition = match root {
        Some(root) => root,
        None => root_definition(ctx, ir)?,
    };
    if let Some(root) = ir
        .model
        .product_definitions
        .iter_mut()
        .find(|candidate| candidate.id == definition)
    {
        root.bodies.clear();
    }
    let keys = record_keys(records);
    let ids = keys
        .iter()
        .map(|key| FeatureId::mint(feature_id_text(key)).map_err(CodecError::malformed))
        .collect::<Result<Vec<_>, _>>()?;
    let history = History {
        objects,
        producers: producers(records),
        keys: &keys,
        ids: &ids,
    };
    let group = ContainerId::mint("rhino:history:group#records").map_err(CodecError::malformed)?;
    let mut members = BTreeSet::new();
    let mut current = Vec::new();
    for (index, record) in records.iter().enumerate() {
        ctx.charge_entities(1, "project Rhino v11 history feature")?;
        let id = ids[index].clone();
        let mut produced = distinct(&record.descendants)
            .flat_map(|descendant| history.bodies(descendant, Some(index)))
            .collect::<Vec<_>>();
        produced.sort();
        produced.dedup();
        current.push(CurrentState {
            at: Position::After(Member::Feature(id.clone())),
            bodies: produced,
            poses: BTreeMap::default(),
            status: EvaluationStatus::Current,
            native_ref: None,
        });
        members.insert(Member::Feature(id.clone()));
        let mut native_options = native_options(ctx, record)?;
        native_options.extend(history.antecedents(ctx, index, record)?);
        ir.design.features.push(Feature {
            id,
            name: None,
            suppressed: None,
            definition: FeatureDefinition {
                operation: FeatureOperation::Native {
                    kind: codec_name(&record.command_id.to_string())?,
                    native_options,
                    source_settings: NativeOptions::default(),
                },
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: Some(native_record_id(&keys[index])),
        });
    }
    ctx.charge_entities(2, "project Rhino v11 history scope")?;
    ir.design.containers.push(Container::Unordered {
        id: group.clone(),
        kind: UnorderedKind::Group,
        members,
    });
    ir.design.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.containers.sort_by(|a, b| a.id().cmp(b.id()));
    ir.design.scopes.push(DesignScope {
        definition,
        roots: vec![group],
        active_configuration: None,
        active_arrangements: BTreeSet::default(),
        occurrences: BTreeMap::default(),
        current,
        unproduced: BTreeSet::default(),
    });
    Ok(())
}

/// The document's root definition, where the document states none.
fn root_definition(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<ProductDefinitionId, CodecError> {
    ctx.charge_entities(1, "project Rhino v11 root definition")?;
    let id = ProductDefinitionId::mint("rhino:document:definition#root")
        .map_err(CodecError::malformed)?;
    ir.model.product_definitions.push(ProductDefinition::root(
        id.clone(),
        ExternalDocument::default(),
    ));
    Ok(id)
}

/// The record that lists each object among its descendants: `Some` where
/// exactly one record does, `None` where several do.
fn producers(records: &[HistoryRecord]) -> HashMap<&Uuid, Option<usize>> {
    let mut producers = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        for descendant in distinct(&record.descendants) {
            producers
                .entry(descendant)
                .and_modify(|producer| *producer = None)
                .or_insert(Some(index));
        }
    }
    producers
}

/// The non-nil ids of a list, first occurrence of each.
fn distinct(ids: &[Uuid]) -> impl Iterator<Item = &Uuid> {
    let mut seen = HashSet::new();
    ids.iter()
        .filter(move |id| !id.is_nil() && seen.insert(**id))
}

/// What the records of one document say about the objects they name.
struct History<'a> {
    objects: &'a ObjectBodies,
    producers: HashMap<&'a Uuid, Option<usize>>,
    keys: &'a [String],
    ids: &'a [FeatureId],
}

impl History<'_> {
    /// The bodies of `object` where `producer` is the one record that lists
    /// it among its descendants, or, with `None`, where any one record does.
    fn bodies(&self, object: &Uuid, producer: Option<usize>) -> Vec<BodyId> {
        match self.producers.get(object) {
            Some(Some(index)) if producer.is_none_or(|producer| producer == *index) => {
                self.objects.get(object).cloned().unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// The record's antecedents, each a pick by its object UUID. A pick of an
    /// object another record produces is anchored after that record, under
    /// `rhino:antecedents.<its key>`; every other pick is anchored `Current`,
    /// under `rhino:antecedents` (§5.8). A pick resolves to the object's bodies where its
    /// anchored state holds them.
    fn antecedents(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        record: &HistoryRecord,
    ) -> Result<NativeOptions, CodecError> {
        let mut groups = BTreeMap::<Option<usize>, Vec<Picked>>::new();
        for object in distinct(&record.antecedents) {
            ctx.charge_entities(1, "project Rhino v11 history antecedent")?;
            let producer = match self.producers.get(object) {
                Some(Some(producer)) if *producer != index => Some(*producer),
                _ => None,
            };
            let bodies = self.bodies(object, None);
            groups.entry(producer).or_default().push(Picked {
                target: PickTarget::Native { within: None },
                context: None,
                native: Some(NativeName {
                    codec: cadmpeg_ir::codec_id!("rhino"),
                    value: NativeNameValue::Text(text(object.to_string())?),
                }),
                domain: None,
                witness: None,
                resolved: (!bodies.is_empty())
                    .then(|| bodies.into_iter().map(EntityRef::Body).collect()),
            });
        }
        let mut options = NativeOptions::default();
        for (producer, picks) in groups {
            let (name, at) = match producer {
                Some(producer) => (
                    format!("antecedents.{}", self.keys[producer]),
                    Anchor::Pinned(Position::After(Member::Feature(self.ids[producer].clone()))),
                ),
                None => ("antecedents".to_owned(), Anchor::Current),
            };
            let picks = Members::new(picks).map_err(CodecError::malformed)?;
            options.insert(
                codec_name(&name)?,
                NativeOption::Selection(Selection {
                    at,
                    picks: Picks::Named(picks),
                    native: None,
                }),
            );
        }
        Ok(options)
    }
}

/// Each boolean, integer, real, text, and UUID a record stores, under the
/// record's value id (`rhino:value_<id>`, `rhino:value_<id>_<n>` for its n-th
/// repeat) and, where the value holds several, the element's index.
fn native_options(
    ctx: &DecodeContext<'_>,
    record: &HistoryRecord,
) -> Result<NativeOptions, CodecError> {
    let mut options = NativeOptions::default();
    let mut repeats = HashMap::<i32, usize>::new();
    for value in &record.values {
        let repeat = repeats.entry(value.id).or_default();
        let key = if *repeat == 0 {
            format!("value_{}", value.id)
        } else {
            format!("value_{}_{}", value.id, repeat)
        };
        *repeat += 1;
        let elements = match &value.value {
            Value::Booleans(values) => values
                .iter()
                .map(|value| Some(NativeOption::Bool(Dimension::Literal(*value))))
                .collect(),
            Value::Integers(values) => values
                .iter()
                .map(|value| Some(NativeOption::Integer(Dimension::Literal(i64::from(*value)))))
                .collect(),
            Value::Doubles(values) => values
                .iter()
                .map(|value| {
                    FiniteReal::new(*value)
                        .map(|value| NativeOption::Real(Dimension::Literal(value)))
                })
                .collect(),
            Value::Strings(values) => values
                .iter()
                .map(|value| {
                    NonBlankString::new(value.clone())
                        .map(|value| NativeOption::Text(Dimension::Literal(value)))
                })
                .collect(),
            Value::Uuids(values) => values
                .iter()
                .map(|value| {
                    NonBlankString::new(value.to_string())
                        .map(|value| NativeOption::Text(Dimension::Literal(value)))
                })
                .collect(),
            _ => Vec::new(),
        };
        let single = elements.len() == 1;
        for (index, element) in elements.into_iter().enumerate() {
            let Some(element) = element else { continue };
            ctx.charge_entities(1, "project Rhino v11 history option")?;
            let name = if single {
                key.clone()
            } else {
                format!("{key}.{index}")
            };
            options.insert(codec_name(&name)?, element);
        }
    }
    Ok(options)
}

/// A native option key or operation kind, `rhino:<name>` (§2.7).
fn codec_name(name: &str) -> Result<CodecName, CodecError> {
    CodecName::new(&cadmpeg_ir::codec_id!("rhino"), name)
        .ok_or_else(|| CodecError::malformed("Rhino v11 history name is blank"))
}

fn text(value: String) -> Result<NonBlankString, CodecError> {
    NonBlankString::new(value)
        .ok_or_else(|| CodecError::malformed("Rhino v11 history text is blank"))
}

#[cfg(test)]
mod tests;
