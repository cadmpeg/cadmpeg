// SPDX-License-Identifier: Apache-2.0
//! Bind compact coils to registered solved placement constructions.

use crate::container::ContainerScan;
use crate::design::decode::operands::parse_entity_selection_frame;
use crate::ids::{native_scope_matches, native_stream};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::coil::{DesignCoilPlacement, DesignCoilSelection};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::placement_construction::DesignPlacementConstruction;
use cadmpeg_core::CodecError;

fn resolved_placement(
    bytes: &[u8],
    scope: &DesignParameterScope,
    headers: &[DesignRecordHeader],
    placements: &[DesignPlacementConstruction],
) -> Option<DesignCoilPlacement> {
    if scope.kind() != DesignFeatureKind::CoilPrimitive
        || scope.frame_length() != 431
        || scope.reference_members().len() != 8
        || scope.coil_placement().is_some()
        || scope.coil_transform().is_some()
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let selection_index = *scope.reference_members().values().next()?;
    let placement_index = *scope.reference_members().values().nth(1)?;
    let mut candidates = placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.record_index == u64::from(placement_index)
    });
    let placement = candidates.next()?;
    if candidates.next().is_some()
        || placement.type_version != 8
        || placement.fields.inferred_x_axis.value != 0
        || placement.fields.inferred_z_axis.value != 0
    {
        return None;
    }
    let transform = super::checked_solved_frame(placement)?;
    let m = transform.value.rows();
    let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if determinant <= 0.0 {
        return None;
    }
    let mut candidates = headers.iter().filter(|header| {
        native_stream(&header.id) == Some(stream) && header.record_index == selection_index
    });
    let header = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    let selection = parse_entity_selection_frame(
        bytes,
        selection_index,
        header.byte_offset,
        header.class_tag.as_str(),
    )?;
    Some(DesignCoilPlacement {
        selection_record_index: selection_index,
        selection_record_byte_offset: header.byte_offset,
        selection_class_tag: header.class_tag.clone(),
        selection: DesignCoilSelection::Persistent {
            asset_id: selection.asset_id.try_into().ok()?,
            context_id: selection.context_id.try_into().ok()?,
            serialization_form: selection.serialization_form,
            identity_record_index: selection.identity_record_index,
            primary_identity: selection.primary_identity,
            secondary: selection.secondary.map(|identity| {
                crate::records::identity::DesignSecondaryIdentity {
                    identity: identity.identity.value,
                    curve_identity: identity.curve_identity.map(|value| value.value),
                }
            }),
        },
        transform_record_index: placement_index,
        transform_record_byte_offset: placement.byte_offset,
        transform_class_tag: placement.class_tag.clone(),
        explicit_transform: placement.fields.matrix.map(|_| transform),
    })
}

pub(crate) fn bind(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    placements: &[DesignPlacementConstruction],
) -> Result<(), CodecError> {
    for scope in scopes {
        if scope.kind() != DesignFeatureKind::CoilPrimitive
            || scope.coil_placement().is_some()
            || scope.coil_transform().is_some()
        {
            continue;
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan
            .entries
            .iter()
            .find(|entry| native_scope_matches(stream, &entry.name))
        else {
            continue;
        };
        let placement =
            resolved_placement(scan.entry_bytes(&entry.name)?, scope, headers, placements);
        if let Some(placement) = placement {
            if let DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut() {
                slot.get_or_insert_with(Default::default).placement = Some(placement);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
