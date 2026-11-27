// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of a Part Studio, restated from the decoded history.
//!
//! Every Part Studio whose history the reader admits is one scope (§5.1) at
//! its root definition, with one ordered `Timeline` container (§5.2) whose
//! members are the features in `features.json` order. Each model feature has
//! a design feature under its own id (§5.10). A feature takes its typed
//! operation (§6) where the model record states every operand the design
//! record needs, and otherwise its operation family, with a loss that names
//! what the model record carries beyond it.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::json;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use serde_json::Value;

use super::{id, push, RollbackBar};
use crate::loss::OnshapeLossCode;

mod operation;
mod sketches;

/// What the design projection reads of a source feature that its model
/// record does not keep.
#[derive(Debug, Default)]
pub(super) struct Source {
    /// The source `featureType`.
    pub(super) feature_type: Option<String>,
    /// Whether an extrude ends at the next face (`UP_TO_NEXT`).
    pub(super) next: bool,
    /// The design feature of a thin extrude, built from the source record.
    pub(super) thin: Option<Value>,
}

/// The source facts of each admitted feature, by its model feature id.
pub(super) type Sources = BTreeMap<String, Source>;

/// The outcome of a projection that keeps its design layer.
#[derive(Debug, Default)]
pub(super) struct Projected {
    /// The names of the thin extrudes whose design feature fell back to
    /// their family, with the reason.
    pub(super) thin_refused: Vec<(String, String)>,
}

fn note(notes: &mut Vec<LossNote>, code: OnshapeLossCode, subject: &str, detail: &str) {
    notes.push(code.note(format!("{detail}; subject: {subject}")));
}

/// Project the design layer of the Part Studio `ir`, whose model features
/// were decoded from the sources `sources`, with `bar` its rollback bar.
///
/// # Errors
///
/// A resource refusal, or a design record the IR refuses.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    notes: &mut Vec<LossNote>,
    bar: &RollbackBar,
    sources: &Sources,
) -> Result<Projected, CodecError> {
    let mut projected = Projected::default();
    if ir.model.features.is_empty() {
        return Ok(projected);
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.features.len()) + 2,
        "Onshape design timeline",
    )?;
    let malformed = |reason: String| CodecError::malformed(reason);
    super::reserve_history(&mut ir.design.features, ir.model.features.len())
        .map_err(CodecError::from)?;
    let sketch_failures = sketches::project(ctx, ir)?;
    let stated_sketches: BTreeSet<&str> = ir
        .design
        .sketches
        .iter()
        .map(|sketch| sketch.id.as_str())
        .collect();
    let stated_entities: BTreeMap<&str, &str> = ir
        .design
        .sketch_entities
        .iter()
        .map(|entity| (entity.id.as_str(), entity.sketch.as_str()))
        .collect();
    let mut designed = Vec::with_capacity(ir.model.features.len());
    let mut preceding = BTreeSet::new();
    let mut timeline: Vec<_> = ir.model.features.iter().collect();
    timeline.sort_by_key(|feature| feature.ordinal);
    for model in timeline {
        let source = sources.get(model.id.as_str());
        let name = model.name.as_deref().unwrap_or(model.id.as_str());
        let context = operation::Context {
            sketches: &stated_sketches,
            entities: &stated_entities,
            preceding: &preceding,
        };
        let thin = source.and_then(|source| source.thin.as_ref());
        let typed = match (thin, sketch_failures.get(&model.id)) {
            (Some(thin), _) => match operation::thin(thin, &context) {
                Ok(feature) => Ok(feature.definition.operation),
                Err(reason) => {
                    projected.thin_refused.push((name.to_owned(), reason));
                    Err(None)
                }
            },
            (None, Some(reason)) => Err(Some((
                FeatureOperation::Unresolved {
                    family: OperationFamily::Sketch,
                },
                OnshapeLossCode::DesignOperationUnresolved,
                format!("{reason}; the model record carries the decoded sketch"),
            ))),
            (None, None) => operation::project(model, source, &context)
                .map_err(|fallback| Some((fallback.operation, fallback.code, fallback.reason))),
        };
        let operation = match typed {
            Ok(operation) => operation,
            Err(None) => FeatureOperation::Unresolved {
                family: OperationFamily::Extrude,
            },
            Err(Some((operation, code, reason))) => {
                if !reason.is_empty() {
                    note(notes, code, name, &reason);
                }
                operation
            }
        };
        if let FeatureOperation::Sketch(sketch) = &operation {
            preceding.insert(sketch.as_str().to_owned());
        }
        designed.push(Feature {
            id: model.id.clone(),
            name: model.name.clone(),
            suppressed: model.suppressed.map(Dimension::Literal),
            definition: cadmpeg_ir::design::feature::FeatureDefinition {
                operation,
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        });
    }
    ir.design.features.extend(designed);
    install_scope(ctx, ir, bar).map_err(malformed)?;
    Ok(projected)
}

/// State the Part Studio's timeline and scope over its design features.
fn install_scope(ctx: &DecodeContext<'_>, ir: &mut CadIr, bar: &RollbackBar) -> Result<(), String> {
    let definition = owner(ctx, ir).map_err(|error| error.to_string())?;
    let container = id("container", "Part Studio");
    let bodies: Vec<_> = ir.geometry.bodies.iter().map(|body| &body.id).collect();
    let members: Vec<_> = ir
        .design
        .features
        .iter()
        .map(|feature| json!({"kind":"feature","value":feature.id}))
        .collect();
    // The bar stands before the first kept member at or after the stored
    // rollback feature. The current state is the state before the bar.
    let barred = bar.0.iter().find_map(|native| {
        let member = id("feature", native);
        ir.design
            .features
            .iter()
            .position(|feature| feature.id.as_str() == member)
    });
    let (rollback, at) = match barred {
        Some(0) => (
            json!({"kind":"before","value":members[0]}),
            json!({"kind":"start","value":container}),
        ),
        Some(index) => (
            json!({"kind":"before","value":members[index]}),
            json!({"kind":"after","value":members[index - 1]}),
        ),
        None => (
            json!({"kind":"end"}),
            json!({"kind":"after","value":members[members.len() - 1]}),
        ),
    };
    push(
        &mut ir.design.containers,
        json!({"kind":"ordered","value":{"id":container,"kind":"timeline","members":members,"folders":[],"rollback":rollback}}),
    )?;
    push(
        &mut ir.design.scopes,
        json!({"definition":definition,"roots":[container],"active_arrangements":[],"occurrences":{},"unproduced":[],
        "current":[{"at":at,"bodies":bodies,"poses":[],"status":"current"}]}),
    )?;
    Ok(())
}

/// The Part Studio's root definition, stated where the STEP export states
/// none.
fn owner(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<cadmpeg_ir::ids::ProductDefinitionId, CodecError> {
    let placed: BTreeSet<_> = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| occurrence.prototype.clone())
        .collect();
    if let Some(root) = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root() && !placed.contains(&definition.id))
    {
        return Ok(root.id.clone());
    }
    let stated = cadmpeg_ir::ids::ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("onshape", "model", "product-definition"),
        cadmpeg_ir::identity_key!("part-studio"),
    );
    ir.state_root_definition(
        ctx,
        stated.clone(),
        cadmpeg_ir::products::ExternalDocument::default(),
    )?;
    Ok(stated)
}
