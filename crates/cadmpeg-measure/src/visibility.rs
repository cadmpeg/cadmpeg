// SPDX-License-Identifier: Apache-2.0
//! Body display state in the selected assembly instances.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::design::selection::{OccurrencePath, PathStep, StepRef};
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::references::InstanceTarget;
use cadmpeg_ir::CadIr;

use crate::Selection;

pub(crate) fn bodies<'ir>(
    ir: &'ir CadIr,
    selection: &Selection,
    ctx: &DecodeContext<'_>,
) -> Result<HashMap<&'ir str, Option<bool>>, CodecError> {
    let definitions: HashMap<_, _> = ir
        .model
        .product_definitions
        .iter()
        .map(|definition| (&definition.id, definition))
        .collect();
    let occurrences: HashMap<_, _> = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (&occurrence.id, occurrence))
        .collect();
    let mut pending = Vec::new();
    if let Some(id) = &selection.occurrence {
        let above = selection
            .occurrence_above
            .iter()
            .map(|id| occurrences.get(id).copied())
            .collect::<Option<Vec<_>>>();
        if let Some(path) = above
            .and_then(|above| crate::occurrence::chain(ir, &above, occurrences.get(id).copied()?))
        {
            if let Some(definition) = path
                .last()
                .and_then(|step| definitions.get(&step.prototype))
            {
                pending.push((
                    *definition,
                    path.into_iter()
                        .map(|step| step.id.clone())
                        .collect::<Vec<_>>(),
                ));
            }
        }
    } else {
        let placed: HashSet<_> = occurrences.values().map(|entry| &entry.prototype).collect();
        pending.extend(
            definitions
                .values()
                .filter(|definition| definition.is_root() || !placed.contains(&definition.id))
                .map(|definition| (*definition, Vec::new())),
        );
    }
    let mut out = HashMap::new();
    while let Some((definition, path)) = pending.pop() {
        ctx.charge_entities(1, "measure visibility instance")?;
        let path = OccurrencePath(
            path.into_iter()
                .map(|id| PathStep {
                    occurrence: StepRef::Occurrence(id),
                    configuration: None,
                })
                .collect(),
        );
        for body in &definition.bodies {
            if out.get(body.as_str()) == Some(&Some(true)) {
                continue;
            }
            ctx.charge_entities(1, "measure visibility body")?;
            let target = InstanceTarget {
                entity: AppearanceTarget::Body(body.clone()),
                path: (!path.0.is_empty()).then(|| path.clone()),
                state: None,
            };
            let visible = ir.effective_visibility(&target).ok();
            out.entry(body.as_str())
                .and_modify(|held| *held = either(*held, visible))
                .or_insert(visible);
        }
        for id in &definition.occurrences {
            let Some(occurrence) = occurrences.get(id) else {
                continue;
            };
            let Some(child) = definitions.get(&occurrence.prototype) else {
                continue;
            };
            let mut next: Vec<OccurrenceId> = path
                .0
                .iter()
                .filter_map(|step| match &step.occurrence {
                    StepRef::Occurrence(id) => Some(id.clone()),
                    StepRef::Native(_) => None,
                })
                .collect();
            if next.contains(id) {
                continue;
            }
            next.push(id.clone());
            pending.push((*child, next));
        }
    }
    for body in &ir.geometry.bodies {
        out.entry(body.id.as_str()).or_insert_with(|| {
            ir.effective_visibility(&InstanceTarget::new(AppearanceTarget::Body(
                body.id.clone(),
            )))
            .ok()
        });
    }
    Ok(out)
}

fn either(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
