// SPDX-License-Identifier: Apache-2.0
//! Selected-face split operands.

use cadmpeg_ir::features::{
    FaceSelection, FeatureDefinition, FeatureId, FeatureOperation, SplitFaceTool,
};

use crate::{
    feature::{
        motion::BODY_SELECTION_TYPE, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty,
        PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, BOOLEAN_TYPE, BOUNDARY_PATCH_TYPE,
        FACE_COLLECTION_TYPE, FACE_ITEM_TYPE, FEATURE_TYPE, SURFACE_BODY_TYPE,
    },
    pmdc::PmDcReference,
    record_identity::RecordTypeId,
};

pub(super) fn definition<'a>(
    source: &PmDcFeature,
    resolve: impl Fn(u32) -> Option<&'a PmDcFeatureProperty>,
    plane: impl Fn(u32) -> Option<FeatureId>,
) -> Option<FeatureDefinition> {
    if source.save_version_major != 19
        || source.properties.marker() != 2
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
        .any(|(slot, reference)| reference.qualified() != (slot < 8))
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
    if !enumeration(*mode, super::SPLIT_ENUM_TYPE, 3, 2)
        || !enumeration(*tool_kind, super::TOOL_ENUM_TYPE, 4, 2)
        || !matches!(
            typed(*control, BOOLEAN_TYPE).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::Boolean { name, name_value: 0, value: false })
                if name.is_empty()
        )
        || !matches!(
            typed(*side_control, super::SPLIT_BOOLEAN_TYPE).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::Boolean { name, name_value: 0, .. })
                if name.is_empty()
        )
        || surface.index() != 0
        || results.index() != 0
    {
        return None;
    }
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
    if !matches!(
        typed(*target, SURFACE_BODY_TYPE)?.kind,
        PmDcFeaturePropertyKind::SurfaceBody { .. }
    ) {
        return None;
    }
    let [face] = references(
        *faces,
        FACE_COLLECTION_TYPE,
        PmDcFeatureReferenceFamily::FaceCollection,
    )?
    else {
        return None;
    };
    let PmDcFeaturePropertyKind::FaceItem { index_references } =
        &typed(*face, FACE_ITEM_TYPE)?.kind
    else {
        return None;
    };
    if index_references.marker() != 2
        || !matches!(index_references.values(), [value] if *value != 0)
    {
        return None;
    }
    Some(FeatureDefinition::Operation(FeatureOperation::SplitFace {
        targets: FaceSelection::Native(resolve(faces.index())?.id()),
        tool: SplitFaceTool::Plane {
            plane: plane(work_plane.record_ordinal()?)?,
        },
    }))
}

#[cfg(test)]
pub(crate) mod tests;
