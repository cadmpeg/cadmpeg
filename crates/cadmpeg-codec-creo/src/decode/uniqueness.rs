// SPDX-License-Identifier: Apache-2.0
//! Unique-owner lookups for feature definitions, transforms, profiles, and datum planes.

use cadmpeg_core::decode::{DecodeContext, u64_from_index};
use cadmpeg_core::CodecError;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ProfileRef;

use crate::container::ContainerScan;

use super::feature_history::axes::section_profile_ref;
use super::sketch_ids::feature_sketch_record_id_in_scan;

pub(crate) fn exactly_one<T>(mut iter: impl Iterator<Item = T>) -> Option<T> {
    let first = iter.next()?;
    iter.next().is_none().then_some(first)
}

pub(super) fn unique_owned_feature_definition<'a>(
    ctx: &DecodeContext<'_>,
    definitions: &'a [crate::feature::definitions::FeatureDefinition],
    feature_id: u32,
) -> Result<Option<&'a crate::feature::definitions::FeatureDefinition>, CodecError> {
    ctx.charge_work(u64_from_index(definitions.len()), "creo unique owner scan")?;
    Ok(exactly_one(
        definitions
            .iter()
            .filter(|definition| definition.identity.owner_feature_id() == Some(feature_id)),
    ))
}

pub(super) fn unique_feature_section_transform<'a>(
    ctx: &DecodeContext<'_>,
    transforms: &'a [crate::placement::FeatureSectionTransform],
    definition_id: u32,
    section_offset: usize,
) -> Result<Option<&'a crate::placement::FeatureSectionTransform>, CodecError> {
    ctx.charge_work(u64_from_index(transforms.len()), "creo unique owner scan")?;
    let Some(transform) = exactly_one(transforms.iter().filter(|transform| {
        transform.definition_id == definition_id && transform.offset == section_offset
    })) else { return Ok(None); };
    if let Some(feature_id) = transform.feature_id {
        ctx.charge_work(u64_from_index(transforms.len()), "creo unique transform owner scan")?;
        let feature_matches = transforms
            .iter()
            .filter(|candidate| candidate.feature_id == Some(feature_id))
            .count();
        if feature_matches != 1 { return Ok(None); }
    }
    Ok(Some(transform))
}

pub(super) fn unique_feature_definition_for_transform<'a>(
    ctx: &DecodeContext<'_>,
    definitions: &'a [crate::feature::definitions::FeatureDefinition],
    transform: &crate::placement::FeatureSectionTransform,
) -> Result<Option<&'a crate::feature::definitions::FeatureDefinition>, CodecError> {
    ctx.charge_work(u64_from_index(definitions.len()), "creo unique owner scan")?;
    Ok(exactly_one(definitions.iter().filter(|definition| {
        definition.identity.id() == transform.definition_id
            && definition
                .section_3d
                .as_ref()
                .is_some_and(|section| section.offset == transform.offset)
    })))
}

pub(super) fn unique_feature_profile_definition<'a>(
    ctx: &DecodeContext<'_>,
    definitions: &'a [crate::feature::definitions::FeatureDefinition],
    transforms: &'a [crate::placement::FeatureSectionTransform],
    feature_id: u32,
) -> Result<Option<&'a crate::feature::definitions::FeatureDefinition>, CodecError> {
    ctx.charge_work(u64_from_index(transforms.len()), "creo unique profile transform scan")?;
    let mut feature_transforms = transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id));
    match (feature_transforms.next(), feature_transforms.next()) {
        (Some(transform), None) => unique_feature_definition_for_transform(ctx, definitions, transform),
        (None, None) => unique_owned_feature_definition(ctx, definitions, feature_id),
        _ => Ok(None),
    }
}

pub(super) fn unique_feature_profile_ref(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Option<ProfileRef>, cadmpeg_core::CodecError> {
    let Some(definition) = unique_feature_profile_definition(
        ctx,
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )? else {
        return Ok(None);
    };
    Ok(Some(section_profile_ref(
        ctx,
        ir,
        feature_sketch_record_id_in_scan(ctx, scan, definition)?,
    )?))
}

pub(super) fn unique_feature_datum_plane<'a>(
    ctx: &DecodeContext<'_>,
    datums: &'a [crate::datum::DatumPlaneRecord],
    feature_id: u32,
) -> Result<Option<&'a crate::datum::DatumPlaneRecord>, CodecError> {
    ctx.charge_work(u64_from_index(datums.len()), "creo unique owner scan")?;
    Ok(exactly_one(datums.iter().filter(|datum| datum.feature_id == feature_id)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn datum_unique_owner_scan_refuses_work_before_query() {
        let datum = crate::datum::DatumPlaneRecord::new(1, 7, crate::datum::DatumPlane::new(crate::axis::Axis::Z, 0.0).expect("finite plane"), 0.0, [[None; 2]; 2], 0).expect("finite datum");
        let datums = [datum];
        crate::test_support::assert_work_boundaries(&["creo unique owner scan"], |ctx| super::unique_feature_datum_plane(ctx, &datums, 7).map(|value| value.map(|value| value.id)));
    }
}
