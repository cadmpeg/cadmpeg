// SPDX-License-Identifier: Apache-2.0
//! Surface-tool body-trim operands.

use cadmpeg_ir::features::{
    BodySelection, BodyTrimSide, FeatureDefinition, FeatureOperation, TrimBodyOperands,
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

pub(super) fn definition<'a>(
    source: &PmDcFeature,
    resolve: impl Fn(u32) -> Option<&'a PmDcFeatureProperty>,
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
        .any(|(slot, reference)| reference.qualified() != matches!(slot, 0 | 1 | 3 | 4 | 6 | 7 | 8))
        || [faces, work_plane, results]
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
    let boolean = |reference, record_type| {
        matches!(
            typed(reference, record_type).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::Boolean { name, name_value: 0, value: false })
                if name.is_empty()
        )
    };
    if !enumeration(*mode, super::SPLIT_ENUM_TYPE, 3, 1)
        || !enumeration(*tool_kind, super::TOOL_ENUM_TYPE, 4, 3)
        || !boolean(*control, BOOLEAN_TYPE)
        || !boolean(*side_control, super::SPLIT_BOOLEAN_TYPE)
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
    let body = |reference| {
        let property = typed(reference, SURFACE_BODY_TYPE)?;
        match property.kind {
            PmDcFeaturePropertyKind::SurfaceBody { body }
                if body.index() != 0 && !body.qualified() =>
            {
                Some(property.id())
            }
            _ => None,
        }
    };
    let target = body(*target)?;
    let surface = body(*surface)?;
    if target == surface {
        return None;
    }
    Some(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
        operands: TrimBodyOperands::new(
            BodySelection::Native(target),
            BodySelection::Native(surface),
        )
        .ok()?,
        keep: BodyTrimSide::Unresolved,
    }))
}

#[cfg(test)]
pub(crate) mod tests;
