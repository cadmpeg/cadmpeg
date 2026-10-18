// SPDX-License-Identifier: Apache-2.0
//! Unique-owner lookups for feature definitions, transforms, profiles, and datum planes.

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ProfileRef;

use crate::container::ContainerScan;

use super::feature_history::axes::section_profile_ref;
use super::sketch_ids::feature_sketch_record_id_in_scan;

pub(crate) fn exactly_one<T>(mut iter: impl Iterator<Item = T>) -> Option<T> {
    let first = iter.next()?;
    iter.next().is_none().then_some(first)
}

/// Rows keyed by a numeric identifier, indexed once.
///
/// [`UniqueById::get`] returns what `exactly_one` returns over the rows that
/// carry the identifier, without scanning the rows per lookup.
pub(crate) struct UniqueById<'a, T> {
    /// `None` marks an identifier that two or more rows carry.
    rows: BTreeMap<u32, Option<&'a T>>,
}

impl<'a, T> UniqueById<'a, T> {
    pub(crate) fn new(rows: impl IntoIterator<Item = &'a T>, id: impl Fn(&T) -> u32) -> Self {
        let mut indexed = BTreeMap::new();
        for row in rows {
            match indexed.entry(id(row)) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(row));
                }
            }
        }
        Self { rows: indexed }
    }

    /// Return the only row that carries `id`.
    pub(crate) fn get(&self, id: u32) -> Option<&'a T> {
        self.rows.get(&id).copied().flatten()
    }

    /// Return whether any row carries `id`.
    pub(crate) fn contains(&self, id: u32) -> bool {
        self.rows.contains_key(&id)
    }
}

pub(super) fn unique_owned_feature_definition(
    definitions: &[crate::feature::definitions::FeatureDefinition],
    feature_id: u32,
) -> Option<&crate::feature::definitions::FeatureDefinition> {
    exactly_one(
        definitions
            .iter()
            .filter(|definition| definition.identity.owner_feature_id() == Some(feature_id)),
    )
}

pub(super) fn unique_feature_section_transform(
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
        if feature_matches != 1 {
            return None;
        }
    }
    Some(transform)
}

pub(super) fn unique_feature_definition_for_transform<'a>(
    definitions: &'a [crate::feature::definitions::FeatureDefinition],
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<&'a crate::feature::definitions::FeatureDefinition> {
    exactly_one(definitions.iter().filter(|definition| {
        definition.identity.id() == transform.definition_id
            && definition
                .section_3d
                .as_ref()
                .is_some_and(|section| section.offset == transform.offset)
    }))
}

pub(super) fn unique_feature_profile_definition<'a>(
    definitions: &'a [crate::feature::definitions::FeatureDefinition],
    transforms: &'a [crate::placement::FeatureSectionTransform],
    feature_id: u32,
) -> Option<&'a crate::feature::definitions::FeatureDefinition> {
    let mut feature_transforms = transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id));
    match (feature_transforms.next(), feature_transforms.next()) {
        (Some(transform), None) => unique_feature_definition_for_transform(definitions, transform),
        (None, None) => unique_owned_feature_definition(definitions, feature_id),
        _ => None,
    }
}

pub(super) fn unique_feature_profile_ref(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<ProfileRef> {
    let definition = unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )?;
    Some(section_profile_ref(
        ir,
        feature_sketch_record_id_in_scan(scan, definition),
    ))
}

pub(super) fn unique_feature_datum_plane(
    datums: &[crate::datum::DatumPlaneRecord],
    feature_id: u32,
) -> Option<&crate::datum::DatumPlaneRecord> {
    exactly_one(datums.iter().filter(|datum| datum.feature_id == feature_id))
}

#[cfg(test)]
mod tests {
    use super::{exactly_one, UniqueById};

    #[test]
    fn unique_by_id_matches_the_per_identifier_scan() {
        let rows = [
            (3, 'a'),
            (1, 'b'),
            (3, 'c'),
            (0, 'd'),
            (7, 'e'),
            (3, 'f'),
            (7, 'g'),
        ];
        let index = UniqueById::new(&rows, |row| row.0);
        for id in 0..=8 {
            assert_eq!(
                index.get(id),
                exactly_one(rows.iter().filter(|row| row.0 == id)),
                "{id}"
            );
            assert_eq!(
                index.contains(id),
                rows.iter().any(|row| row.0 == id),
                "{id}"
            );
        }
    }
}
