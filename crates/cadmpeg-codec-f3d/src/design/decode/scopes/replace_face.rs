// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::layout::replace_face_fixed_prefix as layout;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::surface_ops::DesignReplaceFaceOperation;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

fn reference_run<'a>(view: &mut View<'a>) -> Option<&'a [u8]> {
    let count = view.u32_le()?;
    let count = view.counted(u64::from(count), 11)?.get();
    let bytes = view.take(count.checked_mul(11)?)?;
    bytes
        .chunks_exact(11)
        .all(|v| marked_record_reference(v, 0).is_some())
        .then_some(bytes)
}

pub(super) fn exact_replace_face_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignReplaceFaceOperation>, CodecError> {
    if scope.kind() != DesignFeatureKind::ReplaceFace {
        return Ok(None);
    }
    let Some(frame) = frame(bytes, scope) else {
        return Ok(None);
    };
    let Some((identity, _)) = fixed_relaxed_guid_text(bytes, frame.identity_at)? else {
        return Ok(None);
    };
    let copy = |run: &[u8]| -> Result<Vec<u32>, CodecError> {
        let count = View::over_retained(run)
            .counted(u64_from_index(run.len() / 11), 11)
            .ok_or_else(|| CodecError::malformed("replacement group bounds"))?;
        let mut values = ctx.alloc_filled(count.get(), 0u32, "F3D replacement groups")?;
        for (value, reference) in values.iter_mut().zip(run.chunks_exact(11)) {
            *value = View::u32_le_at(reference, 1)
                .ok_or_else(|| CodecError::malformed("replacement reference"))?;
        }
        Ok(values)
    };
    Ok(Some(DesignReplaceFaceOperation {
        tangent_chain: frame.tangent_chain,
        replacement_group_record_indices: copy(frame.replacements)?,
        target_group_record_indices: copy(frame.targets)?,
        trailing_identity: identity,
    }))
}

struct ReplaceFaceFrame<'a> {
    tangent_chain: bool,
    replacements: &'a [u8],
    targets: &'a [u8],
    identity_at: usize,
}

fn frame<'a>(bytes: &'a [u8], scope: &DesignParameterScope) -> Option<ReplaceFaceFrame<'a>> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.reference_count_offset()).ok()?;
    let bytes = bytes.get(start..end)?;
    if bytes
        .get(layout::ZERO_RUN..layout::TANGENT_CHAIN)?
        .iter()
        .any(|v| *v != 0)
    {
        return None;
    }
    let tangent_chain = match *bytes.get(layout::TANGENT_CHAIN)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let mut view = View::over_retained(bytes.get(layout::REPLACEMENT_GROUP_COUNT..)?);
    let replacements = reference_run(&mut view)?;
    let targets = reference_run(&mut view)?;
    let identity_at = bytes.len().checked_sub(view.remaining())?;
    let (_, after_identity) = crate::design::decode::text::fixed_guid_ascii(bytes, identity_at)?;
    (bytes.get(after_identity..)? == [0, 0, 0]).then_some(ReplaceFaceFrame {
        tangent_chain,
        replacements,
        targets,
        identity_at: start.checked_add(identity_at)?,
    })
}
