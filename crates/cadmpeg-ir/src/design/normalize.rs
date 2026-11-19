// SPDX-License-Identifier: Apache-2.0
//! The decode wrapper's normalizer (§3.1).
//!
//! After the wrapper sorts every arena, it puts each one-spelling rule of
//! the specification into its canonical form, so no decoder implements a
//! spelling rule of its own: the positions of §5.3, the selection and
//! construction spellings of §5.8 and §5.9 (D43 among them), the instance-
//! target `state` of §8, the letter and grade of a combined `LimitsAndFits`,
//! and a `DimensionModifier::Other` or `ToleranceZoneForm::Other` whose text
//! is one of its type's own names. Each step rewrites only a spelling the
//! document already states, so normalizing twice equals normalizing once,
//! and no step states content the document does not hold. Validation still
//! checks every one-spelling rule (part 10), because a document can come
//! from elsewhere than a decode.
//!
//! D48, a dimension bound to a minted parameter whose expression negates one
//! reference, is not rewritten: no record states that a parameter was
//! minted rather than read, so a source parameter `-d3` and a minted one are
//! one value here (interim; inventory gap 20261119-N1-1).

mod constructions;
mod pmi;
mod selections;
mod states;
mod walk;

#[cfg(test)]
mod tests;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use self::constructions::Facts;
use self::states::States;
use self::walk::{Visit, Walk};
use crate::document::{CadIr, Design};

/// Puts every one-spelling rule the normalizer covers into its canonical
/// form.
///
/// # Errors
///
/// Refuses a document whose design records nest deeper than the default
/// session ceiling.
pub(crate) fn normalize(ir: &mut CadIr) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
    let normalized = normalize_in(&ctx, ir);
    ctx.finish_session()?;
    normalized
}

fn normalize_in(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let states = States::new(&ir.design);
    states.normalize_design(&mut ir.design);
    states.normalize_model(&mut ir.model);
    let facts = Facts::new(&ir.design);
    let mut visit = Visit::new(ctx, &facts);
    walk_design(&mut ir.design, &mut visit)?;
    pmi::normalize_design(&mut ir.design);
    pmi::normalize_model(&mut ir.model);
    Ok(())
}

/// Walks every design record that can hold a selection or a construction.
fn walk_design(design: &mut Design, visit: &mut Visit<'_, '_>) -> Result<(), CodecError> {
    design.scopes.walk(visit)?;
    design.containers.walk(visit)?;
    design.features.walk(visit)?;
    design.sketches.walk(visit)?;
    design.sketch_entities.walk(visit)?;
    design.sketch_constraints.walk(visit)?;
    design.spatial_sketches.walk(visit)?;
    design.spatial_sketch_entities.walk(visit)?;
    design.spatial_sketch_constraints.walk(visit)?;
    design.parameters.walk(visit)?;
    design.sheet_metal_rules.walk(visit)?;
    design.programs.walk(visit)?;
    design.configurations.walk(visit)?;
    design.arrangements.walk(visit)?;
    design.publications.walk(visit)
}
