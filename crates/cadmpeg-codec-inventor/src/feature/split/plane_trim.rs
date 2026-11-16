// SPDX-License-Identifier: Apache-2.0
//! Work-plane body-trim operands.

use cadmpeg_ir::features::{
    BodySelection, BodyTrimSide, FeatureDefinition, FeatureId, FeatureOperation,
};

use crate::{
    feature::{
        motion::BODY_SELECTION_TYPE, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty,
        PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, BOOLEAN_TYPE, BOUNDARY_PATCH_TYPE,
        FEATURE_TYPE, SURFACE_BODY_TYPE,
    },
    pmdc::PmDcReference,
    record_identity::RecordTypeId,
};

/// Segment major versions of the work-plane trim form.
const SIDE_TRUE_VERSION: u8 = 19;
const SIDE_FALSE_VERSIONS: [u8; 2] = [19, 23];

pub(super) fn definition<'a>(
    source: &PmDcFeature,
    resolve: impl Fn(u32) -> Option<&'a PmDcFeatureProperty>,
    plane: impl Fn(u32) -> Option<FeatureId>,
) -> Option<FeatureDefinition> {
    if source.properties.marker() != 2
        || source.identity.type_id != RecordTypeId::from_bytes(FEATURE_TYPE)
    {
        return None;
    }
    let [mode, control, faces, tool_kind, patches, work_plane, side_control, inputs, surface, results] =
        source.properties.references()
    else {
        return None;
    };
    if source
        .properties
        .references()
        .iter()
        .enumerate()
        .any(|(slot, reference)| reference.qualified() != matches!(slot, 0 | 1 | 3 | 4 | 5 | 6 | 7))
        || [faces, surface, results]
            .iter()
            .any(|reference| reference.index() != 0)
    {
        return None;
    }
    let typed = |reference: PmDcReference, type_id| {
        if !reference.qualified() {
            return None;
        }
        let property = resolve(reference.index())?;
        (property.identity.type_id == RecordTypeId::from_bytes(type_id)).then_some(property)
    };
    let enumeration = |reference: PmDcReference, record_type, expected_type, expected_value| {
        matches!(
            typed(reference, record_type).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value,
                value,
            }) if *type_value == expected_type && *value == expected_value
        )
    };
    if !enumeration(*mode, super::SPLIT_ENUM_TYPE, 3, 1)
        || !enumeration(*tool_kind, super::TOOL_ENUM_TYPE, 4, super::TOOL_WORK_PLANE)
        || !matches!(
            typed(*control, BOOLEAN_TYPE).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::Boolean { name, name_value: 0, value: false })
                if name.is_empty()
        )
    {
        return None;
    }
    let keep = match typed(*side_control, super::SPLIT_BOOLEAN_TYPE).map(|p| &p.kind) {
        Some(PmDcFeaturePropertyKind::Boolean {
            name,
            name_value: 0,
            value,
        }) if name.is_empty() => match value {
            true if source.save_version_major == SIDE_TRUE_VERSION => BodyTrimSide::Forward,
            false if SIDE_FALSE_VERSIONS.contains(&source.save_version_major) => {
                BodyTrimSide::Reverse
            }
            _ => return None,
        },
        _ => return None,
    };
    let references =
        |reference: PmDcReference, type_id, expected| match &typed(reference, type_id)?.kind {
            PmDcFeaturePropertyKind::References { family, items }
                if *family == expected && items.marker() == 2 =>
            {
                Some(items.references())
            }
            _ => None,
        };
    if !references(
        *patches,
        BOUNDARY_PATCH_TYPE,
        PmDcFeatureReferenceFamily::BoundaryPatch,
    )?
    .is_empty()
    {
        return None;
    }
    let [target] = references(
        *inputs,
        BODY_SELECTION_TYPE,
        PmDcFeatureReferenceFamily::BodySelection,
    )?
    else {
        return None;
    };
    let target = typed(*target, SURFACE_BODY_TYPE)?;
    match target.kind {
        PmDcFeaturePropertyKind::SurfaceBody { body } if body.index() != 0 && !body.qualified() => {
        }
        _ => return None,
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::TrimBodiesAtPlane {
            targets: BodySelection::Native(target.id()),
            plane: plane(work_plane.record_ordinal()?)?,
            keep,
        },
    ))
}

#[cfg(test)]
pub(crate) mod tests;
