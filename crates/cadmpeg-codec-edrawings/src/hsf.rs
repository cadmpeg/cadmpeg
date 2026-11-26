// SPDX-License-Identifier: Apache-2.0
//! The HOOPS Stream Format model body.
//!
//! [`decode_body`] walks the inflated model stream with exact record length
//! rules ([`walk`]), collects library bodies, tree nodes, and the object store
//! ([`scene`]), decodes shells into meshes ([`shape`]), and emits bodies,
//! product definitions, and occurrences ([`emit`]). The stream stores display
//! meshes only, so every body is a faceted B-rep.

mod emit;
mod scene;
mod shape;
mod walk;

use std::collections::BTreeSet;

use cadmpeg_core::decode::{u64_from_index, ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{Cause, DecodeParts, GivenUpPart, Part};

use crate::dialect::Grammar;
use crate::loss::EdrawingsLossCode;
use scene::{Object, Scene, Span};

/// Part kind of an object record of the store.
const OBJECT_PART: &str = "model-object";
/// Part kind of a tree node segment.
const NODE_PART: &str = "tree-node";

/// Gives up the record or segment at `span` of the model stream.
fn give_up_span(
    ctx: &DecodeContext<'_>,
    scene: &Scene<'_>,
    kind: &'static str,
    span: Span,
    cause: Cause,
    error: CodecError,
) -> Result<GivenUpPart, CodecError> {
    let bytes = scene
        .stream
        .get(span.start..span.end)
        .ok_or_else(|| CodecError::malformed("a record lies outside the model stream"))?;
    let part = Part::at(
        "edrawings",
        kind,
        u64_from_index(span.start),
        bytes,
        [ArenaClass::Model],
    )
    .in_stream(cadmpeg_ir::stream_name!("eModel"));
    ctx.give_up_part(part, cause, error)
}

/// Decodes the inflated model stream `body` into `ir`. Returns whether any
/// geometry was transferred, or the cause and the record rule that stopped
/// the walk, for the caller to give the model member up. Entities are
/// charged through `ctx`, and the records that are not transferred go to
/// `given_up`.
pub(crate) fn decode_body(
    ctx: &DecodeContext<'_>,
    body: View<'_>,
    grammar: Grammar,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<Result<bool, (Cause, CodecError)>, CodecError> {
    let scene = match Scene::collect(body, grammar) {
        Ok(scene) => scene,
        Err(error) => {
            return Ok(Err((
                error.cause,
                CodecError::malformed(format_args!("the model stream walk stopped: {error}")),
            )));
        }
    };
    let code = EdrawingsLossCode::ModelObjectInvalid.kind();
    let sources: BTreeSet<u64> = scene
        .objects
        .values()
        .filter_map(|stored| match stored.object {
            Object::Instance(instance) => Some(instance.source),
            Object::Shell(_) => None,
        })
        .collect();
    let mut unplaced_slots = 0usize;
    for slot in &scene.orphan_slots {
        let Some(stored) = scene
            .objects
            .get(slot)
            .filter(|_| !sources.contains(&u64::from(*slot)))
        else {
            unplaced_slots += 1;
            continue;
        };
        given_up.push(give_up_span(
            ctx,
            &scene,
            OBJECT_PART,
            stored.span,
            Cause::Unreadable,
            CodecError::malformed(format_args!(
                "{code}: the shell slot that places the object lies outside every library body"
            )),
        )?);
    }
    if unplaced_slots > 0 {
        losses.push(EdrawingsLossCode::ModelObjectInvalid.note(format!(
            "{unplaced_slots} shell slots lie outside every library body and were not transferred"
        )));
    }
    for (id, span) in &scene.duplicate_objects {
        given_up.push(give_up_span(
            ctx,
            &scene,
            OBJECT_PART,
            *span,
            Cause::Unreadable,
            CodecError::malformed(format_args!(
                "{code}: an earlier object has the id {id}, and the decode keeps the earlier object"
            )),
        )?);
    }
    if scene.annotation_objects > 0 {
        losses.push(EdrawingsLossCode::AnnotationNotTransferred.note(format!(
            "{} geometry and image side objects, the 2D annotation of the model, were read and not \
             transferred (EDR-18)",
            scene.annotation_objects
        )));
    }
    let transferred = emit::emit(ctx, &scene, ir, losses, given_up)?;
    ir.finalize();
    Ok(Ok(transferred))
}
