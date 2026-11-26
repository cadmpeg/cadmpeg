// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of a CATIA part, projected from the decoded records.
//!
//! The part's design scope and its parameters are restated from the records
//! the decoder already transferred; no source byte is read here. The decoder
//! reads no body or geometrical-set membership, so it states no feature,
//! container or sketch: §5.2 makes every feature a member of exactly one
//! container, and a container the source holds cannot be named before its
//! membership is read (interim, spec gap G2-05).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::design::projection::fields::{FeatureField, ModelFeatureField};
use cadmpeg_ir::design::projection::{lower_exactness, Restatement};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::loss::CatiaLossCode;

mod parameters;

/// Project the decoded design records of `ir` into its v11 design layer, and
/// add to `losses` what the projection drops.
///
/// A part whose decode transfers no parameter states no design record. The
/// scope's bodies are stated unproduced once the root lists them
/// ([`state_unproduced`]).
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if ir.model.parameters.is_empty() {
        return Ok(());
    }
    let definition = root_definition(ctx, ir)?;
    parameters::project(ctx, ir, &definition, losses)?;
    ctx.charge_entities(1, "project the CATIA v11 scope")?;
    ir.design.scopes.push(DesignScope {
        definition,
        roots: Vec::new(),
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
    cadmpeg_ir::design::projection::restate_exactness(
        ir,
        annotations,
        Restatement {
            feature: feature_inputs,
            parameter: parameters::inputs,
        },
    )?;
    for id in parameters::defaulted_kinds(ir) {
        lower_exactness(annotations, id, "kind", cadmpeg_ir::Exactness::Inferred)?;
    }
    Ok(())
}

/// The projection states no design feature.
fn feature_inputs(field: FeatureField) -> &'static [ModelFeatureField] {
    match field {
        FeatureField::Name
        | FeatureField::Suppressed
        | FeatureField::Definition
        | FeatureField::Placement
        | FeatureField::Component
        | FeatureField::DrivenBy
        | FeatureField::NativeRef => &[],
    }
}

/// List as unproduced each body of a definition the scope spans that no
/// current state holds (§5.5): the decoder reads no member that produces
/// one. Returns the loss that names the unread history where it lists any.
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
        "the part's feature history is not read; {count} body(ies) of its scope are unproduced"
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
