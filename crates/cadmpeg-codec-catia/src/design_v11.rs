// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of a CATIA part, projected from the decoded records:
//! its feature tree (§5.2), its features and sketches (§6.1, §6.2), and its
//! parameters. No source byte is read here.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::design::projection::fields::{FeatureField, ModelFeatureField};
use cadmpeg_ir::design::projection::Restatement;
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::decode::ModelingGraphScope;
use crate::loss::CatiaLossCode;
use crate::native::CatiaNative;

mod history;
mod parameters;
mod tree;

/// Project the decoded design records of `ir` into its v11 design layer, and
/// add to `losses` what the projection drops.
///
/// A part whose decode reads no feature tree and transfers no feature or
/// parameter states no design record. The scope's bodies are stated
/// unproduced once the root lists them ([`state_unproduced`]).
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    scope: &ModelingGraphScope,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let tree = tree::read(native, scope);
    if tree.is_none() && ir.model.features.is_empty() && ir.model.parameters.is_empty() {
        return Ok(());
    }
    let definition = root_definition(ctx, ir)?;
    parameters::project(ctx, ir, &definition, losses)?;
    let roots = history::project(ctx, ir, native, tree.as_ref(), losses)?;
    if roots.is_empty() && ir.design.parameters.is_empty() {
        return Ok(());
    }
    ctx.charge_entities(1, "project the CATIA v11 scope")?;
    ir.design.scopes.push(DesignScope {
        definition,
        roots,
        active_configuration: None,
        active_arrangements: BTreeSet::default(),
        occurrences: BTreeMap::default(),
        current: Vec::new(),
        unproduced: BTreeSet::default(),
    });
    Ok(())
}

/// Restate onto the projected parameters the exactness of the model
/// parameters they are built from. A kind the signature does not declare is
/// the default `REAL`, and is inferred.
///
/// # Errors
///
/// A resource refusal while the overrides are stated.
pub(crate) fn restate_exactness(
    ir: &CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    for id in parameters::defaulted_kinds(ir) {
        annotations.field_exactness(id, "kind", cadmpeg_ir::Exactness::Inferred)?;
    }
    cadmpeg_ir::design::projection::restate_exactness(
        ir,
        annotations,
        Restatement {
            feature: feature_inputs,
            parameter: parameters::inputs,
        },
    )
}

/// The model fields a restated feature's fields are built from.
#[deny(clippy::wildcard_enum_match_arm)]
fn feature_inputs(field: FeatureField) -> &'static [ModelFeatureField] {
    match field {
        FeatureField::Definition => &[ModelFeatureField::Definition, ModelFeatureField::SourceTag],
        FeatureField::NativeRef => &[ModelFeatureField::NativeRef],
        FeatureField::Name
        | FeatureField::Suppressed
        | FeatureField::Placement
        | FeatureField::Component
        | FeatureField::DrivenBy => &[],
    }
}

/// List as unproduced each body of a definition the scope spans that no
/// current state holds (§5.5): the decoder reads no feature result. Returns
/// the loss that names the unread results where it lists any.
pub(crate) fn state_unproduced(ir: &mut CadIr) -> Option<LossNote> {
    let scope = ir.design.scopes.first()?;
    let spanned = ir.spanned_definitions(&scope.definition);
    let current = scope
        .current
        .iter()
        .flat_map(|state| &state.bodies)
        .collect::<BTreeSet<_>>();
    let geometry = ir
        .geometry
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<BTreeSet<_>>();
    let unproduced = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| spanned.contains(definition.id.as_str()))
        .flat_map(|definition| {
            definition.bodies.iter().chain(
                definition
                    .instance_bodies
                    .iter()
                    .flat_map(|instance| &instance.bodies),
            )
        })
        .filter(|body| geometry.contains(body) && !current.contains(body))
        .cloned()
        .collect::<BTreeSet<_>>();
    if unproduced.is_empty() {
        return None;
    }
    let count = unproduced.len();
    ir.design.scopes[0].unproduced = unproduced;
    Some(CatiaLossCode::HistoryFeaturesUnread.note(format!(
        "no feature result of the part is read; {count} body(ies) of its scope are unproduced"
    )))
}

/// The part's root definition: the `File` definition with no source that no
/// occurrence places, stated here where the decode states none.
fn root_definition(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<ProductDefinitionId, CodecError> {
    let placed = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| &occurrence.prototype)
        .collect::<BTreeSet<_>>();
    if let Some(root) = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root() && !placed.contains(&definition.id))
    {
        return Ok(root.id.clone());
    }
    ctx.charge_entities(1, "state the CATIA root definition")?;
    let id = ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    );
    ir.model.product_definitions.push(ProductDefinition::root(
        id.clone(),
        ExternalDocument::default(),
    ));
    ir.model.product_definitions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(id)
}

#[cfg(test)]
mod tests;
