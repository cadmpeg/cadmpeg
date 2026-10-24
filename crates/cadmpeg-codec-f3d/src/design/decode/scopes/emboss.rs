// SPDX-License-Identifier: Apache-2.0
//! Parse the fixed Emboss operation prefix without assigning its control enums.

use crate::bytes::lp_utf16_bounded;
use crate::records::feature::emboss::DesignEmbossConstruction;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, View};

pub(super) fn exact_emboss_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignEmbossConstruction> {
    if scope.kind() != DesignFeatureKind::Emboss || scope.reference_members().len() != 9 {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.paired_byte_offset()).ok()?;
    let frame = bytes.get(start..end)?;
    let mut view = View::over_retained(frame);
    if view.u32_le()? != 3
        || view.take(3)? != scope.class_tag.as_str().as_bytes()
        || view.u32_le()? != scope.record_index
        || view.take(9)? != [0; 9]
    {
        return None;
    }
    let control_words = [view.u32_le()?, view.u32_le()?];
    let depth_owner = reference(&mut view, start)?;
    let auxiliary_group = reference(&mut view, start)?;
    let horizontal_owner = reference(&mut view, start)?;
    if view.u32_le()? != 1 {
        return None;
    }
    let native_pair = [view.u64_le()?, view.u64_le()?];
    let rotation_owner = reference(&mut view, start)?;
    let face_group = reference(&mut view, start)?;
    let vertical_owner = reference(&mut view, start)?;
    if view.u32_le()? != 1 {
        return None;
    }
    let profile_group = reference(&mut view, start)?;
    if view.take(12)? != [0; 12] || view.u32_le()? != 1 {
        return None;
    }
    let repeated_face = reference(&mut view, start)?;
    if repeated_face.value != face_group.value {
        return None;
    }
    let (guid, after_guid) = lp_utf16_bounded(frame, view.position(), 36..=36)?;
    if guid != "00000000-0000-0000-0000-000000000000" {
        return None;
    }
    view.seek(after_guid)?;
    if view.take(3)? != [0; 3]
        || scope.reference_count_offset()
            != scope
                .byte_offset()
                .checked_add(u64_from_index(view.position()))?
    {
        return None;
    }
    let construction = DesignEmbossConstruction {
        control_words,
        native_pair,
        depth_owner,
        auxiliary_group,
        horizontal_owner,
        rotation_owner,
        face_group,
        vertical_owner,
        profile_group,
        sketch_profile: None,
    };
    construction.links_match(scope).then_some(construction)
}

fn reference(view: &mut View<'_>, start: usize) -> Option<Located<u32>> {
    if view.u8()? != 1 {
        return None;
    }
    let offset = u64_from_index(start.checked_add(view.position())?);
    let value = u32::try_from(view.u64_le()?).ok()?;
    if value == 0 || view.take(2)? != [0; 2] {
        return None;
    }
    Some(Located { value, offset })
}

#[cfg(test)]
mod tests;
