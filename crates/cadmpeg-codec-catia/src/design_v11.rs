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
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition};
use cadmpeg_ir::CadIr;

mod parameters;

/// Project the decoded design records of `ir` into its v11 design layer.
///
/// A part whose decode transfers no parameter states no design record. A
/// body that no stated member produces is in no current state (interim,
/// errata A1).
pub(crate) fn project(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    if ir.model.parameters.is_empty() {
        return Ok(());
    }
    let definition = root_definition(ctx, ir)?;
    parameters::project(ctx, ir, &definition)?;
    ctx.charge_entities(1, "project the CATIA v11 scope")?;
    ir.design.scopes.push(DesignScope {
        definition,
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::default(),
        occurrences: BTreeMap::default(),
        current: Vec::new(),
    });
    Ok(())
}

/// The part's root definition: the `File` definition with no source that no
/// occurrence places, stated here where the decode states none. It lists no
/// body: no member the design states produces one (interim, errata A1).
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
