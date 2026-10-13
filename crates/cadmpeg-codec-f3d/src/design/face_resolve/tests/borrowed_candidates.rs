// SPDX-License-Identifier: Apache-2.0
use crate::design::face_resolve::{self, FaceCandidate};
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::FaceId;

fn ids(candidates: Option<Vec<FaceCandidate<'_>>>) -> Option<Vec<FaceId>> {
    candidates.map(|candidates| {
        candidates
            .into_iter()
            .map(|candidate| match candidate {
                FaceCandidate::Existing { face, .. } => face.clone(),
                FaceCandidate::Generated(slot) => crate::ids::brep_face_id(slot),
            })
            .collect()
    })
}

pub(super) fn resolved_face_operand(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<FaceId>>, CodecError> {
    let (candidates, _storage) = ctx
        .with_scoped_storage("test resolved candidate proof", || {
            face_resolve::resolved_face_operand(ctx, operand)
        })?;
    Ok(ids(candidates))
}

pub(super) fn explicit_bounded_face_candidates(
    ctx: &DecodeContext<'_>,
    operand: &DesignFaceOperand,
) -> Result<Option<Vec<FaceId>>, CodecError> {
    let (candidates, _storage) = ctx
        .with_scoped_storage("test explicit candidate proof", || {
            face_resolve::explicit_bounded_face_candidates(ctx, operand)
        })?;
    Ok(ids(candidates))
}
