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
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
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

/// States the design scope of the root definition, its history group, one
/// `Native` feature per record, and the current state each record's
/// descendants hold, into `ir`, a candidate appended to a document whose
/// root definition is `root`.
///
/// A document with no history record has no design records. An object that
/// no record lists among its descendants is in no current state (interim,
/// errata A1).
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
    let producers = unique_producers(records);
    let group = ContainerId::mint("rhino:history:group#records").map_err(CodecError::malformed)?;
    let mut members = BTreeSet::new();
    let mut current = Vec::new();
    for (record, key) in records.iter().zip(record_keys(records)) {
        ctx.charge_entities(1, "project Rhino v11 history feature")?;
        let id = FeatureId::mint(feature_id_text(&key)).map_err(CodecError::malformed)?;
        let mut produced = Vec::new();
        for descendant in distinct(&record.descendants) {
            if producers.get(descendant) != Some(&1) {
                continue;
            }
            produced.extend(objects.get(descendant).into_iter().flatten().cloned());
        }
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
        ir.design.features.push(Feature {
            id,
            name: None,
            suppressed: None,
            definition: FeatureDefinition {
                operation: operation(ctx, record)?,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: Some(native_record_id(&key)),
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

/// How many records list each object among their descendants.
fn unique_producers(records: &[HistoryRecord]) -> HashMap<&Uuid, usize> {
    let mut producers = HashMap::new();
    for record in records {
        for descendant in distinct(&record.descendants) {
            *producers.entry(descendant).or_default() += 1;
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

/// A record's operation: the command the record replays, by its UUID, with
/// the record's scalar values as options whose meaning is not established.
fn operation(
    ctx: &DecodeContext<'_>,
    record: &HistoryRecord,
) -> Result<FeatureOperation, CodecError> {
    Ok(FeatureOperation::Native {
        kind: text(format!("rhino:{}", record.command_id))?,
        native_options: native_options(ctx, record)?,
        source_settings: NativeOptions::default(),
    })
}

/// Each boolean, integer, real, text, and UUID a record stores, under the
/// record's value id (`value_<id>`, `value_<id>_<n>` for its n-th repeat) and,
/// where the value holds several, the element's index.
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
            options.insert(text(name)?, element);
        }
    }
    Ok(options)
}

fn text(value: String) -> Result<NonBlankString, CodecError> {
    NonBlankString::new(value)
        .ok_or_else(|| CodecError::malformed("Rhino v11 history text is blank"))
}

#[cfg(test)]
mod tests;
