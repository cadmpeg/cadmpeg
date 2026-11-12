// SPDX-License-Identifier: Apache-2.0
//! Bind solved placement geometry to construction-plane scopes.

use std::collections::HashMap;

use crate::ids::native_stream;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut, DesignWorkPlaneTransform,
};
use crate::records::placement_construction::DesignPlacementConstruction;

pub(crate) fn has_operand(
    scope: &DesignParameterScope,
    placements: &[DesignPlacementConstruction],
) -> bool {
    placements
        .iter()
        .any(|placement| references(scope, placement))
}

fn references(scope: &DesignParameterScope, placement: &DesignPlacementConstruction) -> bool {
    native_stream(&scope.id).is_some_and(|stream| native_stream(&placement.id) == Some(stream))
        && scope
            .reference_members()
            .values()
            .any(|index| u64::from(*index) == placement.record_index)
}

fn unique_placement<'a>(
    scope: &DesignParameterScope,
    placements: &'a [DesignPlacementConstruction],
) -> Option<&'a DesignPlacementConstruction> {
    if scope.kind() != DesignFeatureKind::WorkPlane {
        return None;
    }
    let mut candidates = placements
        .iter()
        .filter(|placement| references(scope, placement));
    let placement = candidates.next()?;
    candidates.next().is_none().then_some(placement)
}

pub(crate) fn solved_frame(
    scope: &DesignParameterScope,
    placements: &[DesignPlacementConstruction],
) -> Option<DesignWorkPlaneTransform> {
    let frame = super::checked_solved_frame(unique_placement(scope, placements)?)?;
    let (matrix, offset) = (frame.value, frame.offset);
    Some(DesignWorkPlaneTransform {
        work_plane_transform: matrix,
        work_plane_transform_offset: offset,
        reference: None,
        work_plane_construction: None,
    })
}

pub(crate) fn bind(
    scopes: &mut [DesignParameterScope],
    placements: &[DesignPlacementConstruction],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        if scope.kind() != DesignFeatureKind::WorkPlane {
            continue;
        }
        if has_operand(scope, placements) {
            let frame = solved_frame(scope, placements);
            if let Some(placement) = unique_placement(scope, placements).filter(|placement| {
                frame.is_some()
                    && (placement.fields.inferred_x_axis.value != 0
                        || placement.fields.inferred_z_axis.value != 0)
            }) {
                losses.push(crate::loss::F3dLossCode::WorkPlaneInferredAxisAssumed.note(format!(
                    "WorkPlane {} takes its frame from the stored matrix or identity marker of placement {}, whose isInferredXAxis byte is {} and isInferredZAxis byte is {}.",
                    scope.id,
                    placement.id,
                    placement.fields.inferred_x_axis.value,
                    placement.fields.inferred_z_axis.value
                )));
            }
            if let DesignScopePayloadMut::WorkPlane(slot) = scope.payload_mut() {
                *slot = frame;
            }
        }
        if scope.work_plane_frame().is_none() {
            losses.push(crate::loss::F3dLossCode::WorkPlaneFrameUnbound.note(format!(
                "WorkPlane {} retains its native definition because no unique checked solved frame binds its operands.",
                scope.id
            )));
        }
    }
    losses
}

pub(crate) fn class_pair_inferred_axis_losses(
    scopes: &[DesignParameterScope],
    placements: &[DesignPlacementConstruction],
    inferred_axes: &HashMap<String, (u64, [u8; 2])>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    scopes
        .iter()
        .filter(|scope| !has_operand(scope, placements))
        .filter_map(|scope| {
            let frame = scope.work_plane_frame()?;
            let &(offset, [inferred_x_axis, inferred_z_axis]) = inferred_axes.get(&scope.id)?;
            (frame.work_plane_transform_offset == offset).then(|| {
                crate::loss::F3dLossCode::WorkPlaneInferredAxisAssumed.note(format!(
                    "WorkPlane {} takes its frame from the class-pair placement matrix at offset {offset}, whose isInferredXAxis byte is {inferred_x_axis} and isInferredZAxis byte is {inferred_z_axis}.",
                    scope.id
                ))
            })
        })
        .collect()
}

const WORK_PLANE_FEATURE_NAME_TYPE_GUID: &str = "8D424E29-DE23-4B1F-A3AD-84C52A497D1A";
const WORK_PLANE_FEATURE_NAME_BASE_TYPE_GUID: &str = "1E5F6CE7-7544-4DE1-8C7B-6BACF6DC8E40";
const WORK_PLANE_FEATURE_NAME_VERSION: u32 = 1;

fn parse_constructed_plane_display_name(
    framed_bytes: &[u8],
    entity_id: u64,
) -> Option<(u32, Option<String>)> {
    use crate::layout::constructed_plane_entity_prefix as plane_prefix;
    use cadmpeg_core::decode::View;

    if framed_bytes.len() < plane_prefix::LEN {
        return None;
    }
    let (entity_id_str, mut cursor) = crate::bytes::lp_utf16_bounded(
        framed_bytes,
        plane_prefix::ENTITY_ID_CODE_UNIT_COUNT,
        1..=64,
    )?;
    let parsed_id = crate::records::identity::DesignEntityId::try_from(entity_id_str).ok()?;
    if parsed_id.suffix() != entity_id {
        return None;
    }
    match crate::bytes::take_reference(framed_bytes, &mut cursor)? {
        crate::bytes::Reference::Local { target, .. } if target != 0 => {}
        _ => return None,
    }
    View::u32_le_at(framed_bytes, cursor)?;
    cursor += 4;
    if framed_bytes.get(cursor..cursor + 5) != Some(&[0; 5]) {
        return None;
    }
    cursor += 5;
    let feature_record_index = match crate::bytes::take_reference(framed_bytes, &mut cursor)? {
        crate::bytes::Reference::Local { target, .. } if target != 0 => {
            u32::try_from(target).ok()?
        }
        _ => return None,
    };
    if framed_bytes.get(cursor..cursor + 2) != Some(&[0x00, 0x01]) {
        return None;
    }
    cursor += 2;
    let (raw_name, _) = crate::bytes::lp_utf16_bounded(framed_bytes, cursor, 0..=256)?;
    let display_name = (!raw_name.trim().is_empty() && !raw_name.chars().any(char::is_control))
        .then_some(raw_name);
    Some((feature_record_index, display_name))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkPlaneFeatureName {
    Unique(Option<String>),
    Conflict,
}

pub(crate) fn decode_work_plane_feature_names(
    scan: &crate::container::ContainerScan,
) -> Result<std::collections::BTreeMap<(String, u32), WorkPlaneFeatureName>, cadmpeg_core::CodecError>
{
    use crate::design::decode::meta::{metadata_for_bulk_stream, typed_primary_frames};
    use crate::records::entity_header::DESIGN_MODULE_GEOMETRY;
    use std::collections::BTreeMap;

    let mut out: BTreeMap<(String, u32), WorkPlaneFeatureName> = BTreeMap::new();

    for entry in scan.entries.iter().filter(|entry| {
        scan.is_design_stream(entry, cadmpeg_core::container::ContainerRole::Bulkstream)
    }) {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let frames = typed_primary_frames(
            bytes,
            &meta,
            WORK_PLANE_FEATURE_NAME_TYPE_GUID,
            "work-plane",
        )?;
        for frame in frames {
            if frame.design_type.version != WORK_PLANE_FEATURE_NAME_VERSION {
                continue;
            }
            if frame.design_type.module != DESIGN_MODULE_GEOMETRY
                || !frame
                    .design_type
                    .base_type_guid
                    .value()
                    .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                    .is_some_and(|base| {
                        base.eq_ignore_ascii_case(WORK_PLANE_FEATURE_NAME_BASE_TYPE_GUID)
                    })
            {
                continue;
            }

            let Some(framed_bytes) = bytes.get(frame.start..frame.end) else {
                continue;
            };
            let Some((feature_rec_u32, name_str)) =
                parse_constructed_plane_display_name(framed_bytes, frame.entity_id)
            else {
                continue;
            };

            let key = (entry.name.clone(), feature_rec_u32);
            out.entry(key)
                .and_modify(|existing| match existing {
                    WorkPlaneFeatureName::Unique(prev) if *prev != name_str => {
                        *existing = WorkPlaneFeatureName::Conflict;
                    }
                    WorkPlaneFeatureName::Unique(_) | WorkPlaneFeatureName::Conflict => {}
                })
                .or_insert(WorkPlaneFeatureName::Unique(name_str));
        }
    }

    Ok(out)
}

pub(crate) fn bind_feature_names(
    scan: &crate::container::ContainerScan,
    scopes: &mut [DesignParameterScope],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let decoded = match decode_work_plane_feature_names(scan) {
        Ok(decoded) => decoded,
        Err(error) => {
            return vec![
                crate::loss::F3dLossCode::WorkPlaneDisplayNameUndecoded.note(format!(
                    "constructed plane display names were not decoded: {error}"
                )),
            ];
        }
    };
    let mut losses = Vec::new();
    for scope in scopes {
        if scope.kind() != DesignFeatureKind::WorkPlane {
            continue;
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let mut matched: Option<&WorkPlaneFeatureName> = None;
        for ((entry_name, feature_rec), candidate) in &decoded {
            if *feature_rec == scope.record_index
                && crate::ids::native_scope_matches(stream, entry_name)
            {
                matched = Some(candidate);
                break;
            }
        }
        match matched {
            Some(WorkPlaneFeatureName::Conflict) => {
                scope.display_name = None;
                losses.push(
                    crate::loss::F3dLossCode::WorkPlaneDisplayNameAmbiguous.note(format!(
                        "WorkPlane {} receives conflicting constructed-plane display names.",
                        scope.id
                    )),
                );
            }
            Some(WorkPlaneFeatureName::Unique(Some(name))) => {
                if let Some(label) = scope.label.as_ref() {
                    if label.value != *name {
                        scope.display_name = None;
                        losses.push(
                            crate::loss::F3dLossCode::WorkPlaneDisplayNameAmbiguous.note(format!(
                                "WorkPlane {} label {:?} disagrees with constructed-plane display name {:?}.",
                                scope.id, label.value, name
                            )),
                        );
                        continue;
                    }
                }
                scope.display_name = Some(name.clone());
            }
            Some(WorkPlaneFeatureName::Unique(None)) | None => {}
        }
    }
    losses
}

#[cfg(test)]
mod tests;
