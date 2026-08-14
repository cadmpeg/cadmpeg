// SPDX-License-Identifier: Apache-2.0
//! Unique-owner lookups for feature definitions, transforms, profiles, and datum planes.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) fn exactly_one<T>(mut iter: impl Iterator<Item = T>) -> Option<T> {
    let first = iter.next()?;
    iter.next().is_none().then_some(first)
}

pub(crate) fn unique_owned_feature_definition(
    definitions: &[crate::feature::FeatureDefinition],
    feature_id: u32,
) -> Option<&crate::feature::FeatureDefinition> {
    exactly_one(
        definitions
            .iter()
            .filter(|definition| definition.owner_feature_id == Some(feature_id)),
    )
}

pub(crate) fn unique_feature_section_transform(
    transforms: &[crate::placement::FeatureSectionTransform],
    definition_id: u32,
    section_offset: usize,
) -> Option<&crate::placement::FeatureSectionTransform> {
    let transform = exactly_one(transforms.iter().filter(|transform| {
        transform.definition_id == definition_id && transform.offset == section_offset
    }))?;
    if let Some(feature_id) = transform.feature_id {
        let feature_matches = transforms
            .iter()
            .filter(|candidate| candidate.feature_id == Some(feature_id))
            .count();
        (feature_matches == 1).then_some(())?;
    }
    Some(transform)
}

pub(crate) fn unique_feature_definition_for_transform<'a>(
    definitions: &'a [crate::feature::FeatureDefinition],
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<&'a crate::feature::FeatureDefinition> {
    exactly_one(definitions.iter().filter(|definition| {
        definition.id == transform.definition_id
            && definition
                .section_3d
                .as_ref()
                .is_some_and(|section| section.offset == transform.offset)
    }))
}

pub(crate) fn unique_feature_profile_definition<'a>(
    definitions: &'a [crate::feature::FeatureDefinition],
    transforms: &[crate::placement::FeatureSectionTransform],
    feature_id: u32,
) -> Option<&'a crate::feature::FeatureDefinition> {
    let feature_transforms = transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    match feature_transforms.as_slice() {
        [transform] => unique_feature_definition_for_transform(definitions, transform),
        [] => unique_owned_feature_definition(definitions, feature_id),
        _ => None,
    }
}

pub(crate) fn unique_feature_profile_ref(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<ProfileRef> {
    unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )
    .map(|definition| section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition)))
}

pub(crate) fn unique_feature_datum_plane(
    datums: &[crate::datum::DatumPlane],
    feature_id: u32,
) -> Option<&crate::datum::DatumPlane> {
    exactly_one(datums.iter().filter(|datum| datum.feature_id == feature_id))
}
