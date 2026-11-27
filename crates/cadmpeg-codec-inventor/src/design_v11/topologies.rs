// SPDX-License-Identifier: Apache-2.0
//! The design features that restate the input and result topology records
//! of the model features ([CADIR §5.7](https://github.com/cadmpeg/cadmpeg/blob/main/docs/cadir-v11.md#57-current-names),
//! [§5.8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/cadir-v11.md#58-selections)).
//!
//! The typed operation of a feature restates its input state through the
//! source-selector picks of its selections. A feature restates its result
//! through the native output records and the generated-edge picks of the
//! later features that select it.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::dressup::FilletSet;
use cadmpeg_ir::design::ops::sheet_metal::SheetMetalKind;
use cadmpeg_ir::design::projection::restated::Restater;
use cadmpeg_ir::design::selection::{Anchor, MemberOrder, PickTarget, Picks, Selection, Witness};
use cadmpeg_ir::features::evaluation::{FeatureInputTopology, FeatureResultTopology};
use cadmpeg_ir::features::selections::EdgeSelection;
use cadmpeg_ir::features::{Feature as LegacyFeature, FeatureOperation as LegacyOperation};
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_ir::CadIr;

/// State the restatement of each input and result topology record that a
/// design feature of `ir` restates. `properties` holds the identities of the
/// native feature property records.
///
/// # Errors
///
/// Returns a codec error where the decode's resource policy refuses the
/// visit of a feature's references.
pub(super) fn restate(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    properties: &BTreeSet<String>,
) -> Result<(), CodecError> {
    let typed = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let index = ir
                .design
                .features
                .binary_search_by(|item| item.id.cmp(&feature.id))
                .ok()?;
            let operation = &ir.design.features[index].definition.operation;
            (!matches!(
                operation,
                FeatureOperation::Native { .. } | FeatureOperation::Unresolved { .. }
            ))
            .then_some((&feature.id, (feature, operation)))
        })
        .collect::<BTreeMap<_, _>>();
    let mut inputs = BTreeMap::new();
    for input in &ir.model.feature_input_topologies {
        inputs
            .entry(&input.input_of)
            .and_modify(|unique| *unique = false)
            .or_insert(true);
    }
    let mut restated = Vec::new();
    for input in &ir.model.feature_input_topologies {
        let Some(&(feature, operation)) = typed.get(&input.input_of) else {
            continue;
        };
        let mut referenced = false;
        feature.visit_identity_references(ctx, &mut |id| {
            referenced |= id == input.id.as_str();
            Ok(())
        })?;
        if !referenced {
            continue;
        }
        let operands = if inputs.get(&input.input_of) == Some(&true) {
            InputOperands::of(operation)
        } else {
            InputOperands::default()
        };
        restated.push((
            input.id.as_str().to_owned(),
            input.input_of.as_str().to_owned(),
            input_cut(input, &operands),
        ));
    }
    let picked: BTreeSet<(&str, &str)> = typed
        .values()
        .flat_map(|(feature, _)| generated_edges(feature))
        .collect();
    for result in &ir.model.feature_result_topologies {
        if ir
            .design
            .features
            .binary_search_by(|item| item.id.cmp(&result.output_of))
            .is_err()
        {
            continue;
        }
        restated.push((
            result.id.as_str().to_owned(),
            result.output_of.as_str().to_owned(),
            result_cut(result, properties, &picked),
        ));
    }
    for (legacy, feature, cut) in restated {
        ir.model
            .restated_mut()
            .record(&legacy, Restater::Record(feature), cut);
    }
    Ok(())
}

/// Selectors stated by an operation's geometry operands in its input state.
#[derive(Default)]
struct InputOperands<'a> {
    edges: BTreeMap<u32, Vec<&'a Witness>>,
    faces: BTreeMap<u32, Vec<&'a Witness>>,
}

impl<'a> InputOperands<'a> {
    fn of(operation: &'a FeatureOperation) -> Self {
        let mut operands = Self::default();
        match operation {
            FeatureOperation::Chamfer(chamfer) => {
                for set in &chamfer.sets {
                    Self::names(&mut operands.edges, &set.edges, false);
                    if let Some(reference) = &set.reference {
                        Self::names(&mut operands.faces, reference, false);
                    }
                }
            }
            FeatureOperation::Fillet(fillet) => {
                for set in &fillet.sets {
                    if let FilletSet::Edges { edges, .. } = set {
                        Self::names(&mut operands.edges, edges, false);
                    }
                }
            }
            FeatureOperation::SheetMetal(operation) => {
                if let SheetMetalKind::ContourFlange {
                    edges: Some(edges), ..
                } = &operation.kind
                {
                    Self::names(&mut operands.edges, edges, true);
                }
            }
            FeatureOperation::MoveFaces(operation) => {
                Self::names(&mut operands.faces, &operation.faces, false);
            }
            FeatureOperation::Offset(operation) => {
                for set in &operation.sets {
                    Self::names(&mut operands.faces, &set.faces, false);
                }
            }
            FeatureOperation::Shell(operation) => {
                Self::names(&mut operands.faces, &operation.removed, false);
            }
            FeatureOperation::Thread(operation) => {
                Self::names(&mut operands.faces, &operation.face, false);
            }
            FeatureOperation::Thicken(operation) => {
                Self::names(&mut operands.faces, &operation.faces, false);
            }
            _ => {}
        }
        operands
    }

    fn names<O: MemberOrder>(
        named: &mut BTreeMap<u32, Vec<&'a Witness>>,
        selection: &'a Selection<O>,
        attachment: bool,
    ) {
        if selection.at != Anchor::Input {
            return;
        }
        let Picks::Named(picks) = &selection.picks else {
            return;
        };
        for pick in picks.as_slice() {
            if !matches!(pick.target, PickTarget::Native { within: None })
                || pick.context.is_some()
                || pick.domain.is_some()
            {
                continue;
            }
            let Some(NativeName {
                codec,
                value: NativeNameValue::Integers(values),
            }) = &pick.native
            else {
                continue;
            };
            if *codec != cadmpeg_ir::codec_id!("inventor") {
                continue;
            }
            let selector = match (attachment, values.as_slice()) {
                (false, [selector]) | (true, [_, _, _, selector, _, _]) => *selector,
                _ => continue,
            };
            if let Ok(selector) = u32::try_from(selector) {
                named.entry(selector).or_default().extend(&pick.witness);
            }
        }
    }
}

fn selector(id: &str) -> Option<u32> {
    id.rsplit_once('-')?.1.parse().ok()
}

/// The wire keys of the input fields that the typed operands leave out.
fn input_cut(input: &FeatureInputTopology, operands: &InputOperands<'_>) -> Vec<&'static str> {
    let edge_witnesses = |id| selector(id).and_then(|selector| operands.edges.get(&selector));
    let mut cut = Vec::new();
    if input
        .faces
        .iter()
        .any(|face| !selector(face.as_str()).is_some_and(|key| operands.faces.contains_key(&key)))
    {
        cut.push("faces");
    }
    if input
        .edges
        .iter()
        .any(|edge| edge_witnesses(edge.as_str()).is_none())
    {
        cut.push("edges");
    }
    if input.edge_geometry.iter().any(|sample| {
        !sample.faces.is_empty()
            || !sample.supports.is_empty()
            || sample.carrier.is_some()
            || sample.start.is_some() != sample.end.is_some()
            || !edge_witnesses(sample.edge.as_str()).is_some_and(|witnesses| {
                let witness = super::operations::selections::edge_witness(sample);
                witnesses.iter().any(|held| **held == witness)
            })
    }) {
        cut.push("edge_geometry");
    }
    for (key, held) in [
        ("bodies", !input.bodies.is_empty()),
        ("vertices", !input.vertices.is_empty()),
        ("face_owners", !input.face_owners.is_empty()),
        ("face_geometry", !input.face_geometry.is_empty()),
        ("body_writers", !input.body_writers.is_empty()),
    ] {
        if held {
            cut.push(key);
        }
    }
    cut
}

/// The wire keys of the fields of `result` that no written record holds:
/// the native property records hold its bodies and output slot, and the
/// generated-edge picks of typed features hold its edges.
fn result_cut(
    result: &FeatureResultTopology,
    properties: &BTreeSet<String>,
    picked: &BTreeSet<(&str, &str)>,
) -> Vec<&'static str> {
    let feature = result.output_of.as_str();
    let mut cut = Vec::new();
    if result
        .bodies()
        .iter()
        .any(|body| !properties.contains(body.as_str()))
        || result
            .edges()
            .iter()
            .any(|edge| !picked.contains(&(feature, edge.as_str())))
        || !result.faces().is_empty()
        || !result.vertices().is_empty()
    {
        cut.push("members");
    }
    if result
        .native_ref
        .as_ref()
        .is_some_and(|native| !properties.contains(native))
    {
        cut.push("native_ref");
    }
    cut
}

/// The (feature, local edge) pairs that the generated-edge selections of
/// `feature` pick.
fn generated_edges(feature: &LegacyFeature) -> Vec<(&str, &str)> {
    let selections: Vec<&EdgeSelection> = match feature.evaluation.definition().operation() {
        LegacyOperation::Fillet { groups } => groups.iter().map(|group| &group.edges).collect(),
        LegacyOperation::Chamfer { groups, .. } => {
            groups.iter().map(|group| &group.edges).collect()
        }
        LegacyOperation::SheetMetalAttachedPlate { attachment, .. }
        | LegacyOperation::SheetMetalBend { attachment, .. } => vec![attachment],
        _ => Vec::new(),
    };
    selections
        .into_iter()
        .filter_map(|selection| match selection {
            EdgeSelection::Generated { edges, .. } => Some(edges),
            _ => None,
        })
        .flat_map(|edges| {
            edges
                .iter()
                .map(|edge| (edge.feature.as_str(), edge.local_id.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests;
