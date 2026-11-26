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

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::dialect::Grammar;
use crate::loss::EdrawingsLossCode;

/// Decodes the inflated model stream `body` into `ir`. Returns whether any
/// geometry was transferred. Entities are charged through `ctx`.
pub(crate) fn decode_body(
    ctx: &DecodeContext<'_>,
    body: View<'_>,
    grammar: Grammar,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<bool, CodecError> {
    let scene = match scene::Scene::collect(body, grammar) {
        Ok(scene) => scene,
        Err(error) => {
            losses.extend(cadmpeg_ir::unknown::part::record_given_up(
                crate::package::MODEL_MEMBER,
                EdrawingsLossCode::ModelMalformed.note(format!(
                    "the model stream broke a record rule ({error}); the package carries no model data"
                )),
            ));
            return Ok(false);
        }
    };
    if !scene.orphan_slots.is_empty() || scene.duplicate_objects > 0 {
        losses.push(EdrawingsLossCode::ModelObjectInvalid.note(format!(
            "{} shell slots lie outside every library body and {} objects repeat an earlier id; \
             they were not transferred",
            scene.orphan_slots.len(),
            scene.duplicate_objects
        )));
        let code = EdrawingsLossCode::ModelObjectInvalid.kind();
        losses.extend(scene.orphan_slots.iter().map(|slot| {
            cadmpeg_ir::unknown::part::record_unreadable(format!("object#{slot}"), &code)
        }));
    }
    if scene.annotation_objects > 0 {
        losses.push(EdrawingsLossCode::AnnotationNotTransferred.note(format!(
            "{} geometry and image side objects, the 2D annotation of the model, were read and not \
             transferred (EDR-18)",
            scene.annotation_objects
        )));
    }
    let transferred = emit::emit(ctx, &scene, ir, losses)?;
    ir.finalize();
    Ok(transferred)
}
