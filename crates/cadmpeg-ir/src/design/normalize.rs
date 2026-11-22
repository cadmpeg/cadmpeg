// SPDX-License-Identifier: Apache-2.0
//! The decode wrapper's normalizer (§3.1).
//!
//! After the wrapper sorts every arena, it puts each one-spelling rule of
//! the specification into its canonical form, so no decoder implements a
//! spelling rule of its own, in design and process records alike: the
//! positions of §5.3, the selection and construction spellings of §5.8 and
//! §5.9 (D43 among them), the instance-target `state` of §8, the letter and
//! grade of a combined `LimitsAndFits`, and a `DimensionModifier::Other` or
//! `ToleranceZoneForm::Other` whose text is one of its type's own names.
//! Each step rewrites only a spelling the document already states, so
//! normalizing twice equals normalizing once, and no step states content
//! the document does not hold. Validation still checks every one-spelling
//! rule (part 10), because a document can come from elsewhere than a
//! decode.
//!
//! The walk is run one arena record at a time, each as a part of the
//! normalizer's session under the caller's `DecodePolicy`. A record whose
//! selections or constructions nest past the recursion ceiling is given up
//! where the walk reached the ceiling: what the walk had rewritten stays
//! rewritten, the rest keeps the decoder's spelling, and the walk goes on
//! to the next record. Each rewrite is local to the value it rewrites,
//! reads only facts gathered before the walk, and keeps the value's
//! meaning, so a record given up part way means what it meant. The wrapper
//! reports the records given up (§3.1).
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
use crate::document::CadIr;
use crate::schema::ArenaRecord;

/// Puts every one-spelling rule the normalizer covers into its canonical
/// form under the caller's ceilings, and returns the ids of the records it
/// gave up part way, in walk order.
///
/// # Errors
///
/// A refusal that stops the session (a `ResourceLimit` of `scope:
/// Session`); every other refusal gives up one record.
pub(crate) fn normalize(ir: &mut CadIr, policy: &DecodePolicy) -> Result<Vec<String>, CodecError> {
    if ir.design.is_empty() && ir.process.is_empty() {
        if !ir.model.pmi.is_empty() {
            pmi::normalize_model(&mut ir.model);
        }
        return Ok(Vec::new());
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let normalized = normalize_in(&ctx, ir);
    ctx.finish_session()?;
    normalized
}

fn normalize_in(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<Vec<String>, CodecError> {
    let states = States::new(&ir.design);
    states.normalize_design(&mut ir.design);
    states.normalize_model(&mut ir.model);
    states.normalize_process(&mut ir.process);
    let CadIr {
        geometry,
        design,
        process,
        model,
        ..
    } = ir;
    let facts = Facts::new(
        geometry,
        &design.features,
        &design.sketch_entities,
        &design.spatial_sketch_entities,
        &design.evaluations,
        &process.states,
    );
    let mut records = Records {
        ctx,
        visit: Visit::new(ctx, &facts),
        given_up: Vec::new(),
    };
    records.arena(&mut design.scopes)?;
    records.arena(&mut design.containers)?;
    records.arena(&mut design.features)?;
    records.arena(&mut design.sketches)?;
    records.arena(&mut design.sketch_entities)?;
    records.arena(&mut design.sketch_constraints)?;
    records.arena(&mut design.spatial_sketches)?;
    records.arena(&mut design.spatial_sketch_entities)?;
    records.arena(&mut design.spatial_sketch_constraints)?;
    records.arena(&mut design.parameters)?;
    records.arena(&mut design.sheet_metal_rules)?;
    records.arena(&mut design.programs)?;
    records.arena(&mut design.configurations)?;
    records.arena(&mut design.arrangements)?;
    records.arena(&mut design.publications)?;
    records.arena(&mut process.setups)?;
    records.arena(&mut process.operations)?;
    records.arena(&mut process.features)?;
    records.arena(&mut process.groups)?;
    records.arena(&mut process.presets)?;
    records.arena(&mut process.parameters)?;
    let given_up = records.given_up;
    pmi::normalize_design(design);
    if !model.pmi.is_empty() {
        pmi::normalize_model(model);
    }
    Ok(given_up)
}

/// The walk over arena records, each one a part.
struct Records<'f, 'c> {
    ctx: &'c DecodeContext<'c>,
    visit: Visit<'f, 'c>,
    /// The records given up part way.
    given_up: Vec<String>,
}

impl Records<'_, '_> {
    /// Walks each record of an arena as a part, and notes each record whose
    /// walk a part refusal stopped.
    fn arena<T: Walk + ArenaRecord>(&mut self, records: &mut [T]) -> Result<(), CodecError> {
        for record in records {
            let visit = &mut self.visit;
            if self.ctx.run_part(|| record.walk(visit))?.is_err() {
                self.given_up.push(record.record_identity().to_owned());
            }
        }
        Ok(())
    }
}
