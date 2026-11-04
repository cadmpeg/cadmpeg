// SPDX-License-Identifier: Apache-2.0
//! Unique-owner lookups for feature definitions, transforms, profiles, and datum planes.

use std::collections::{BTreeMap, BTreeSet};

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

/// Positions of items keyed by a canonical numbered identity under `prefix`.
///
/// [`NumberedIdentityPositions::unique`] returns what `exactly_one` returns
/// over items whose identity matches `prefix` and the number.
/// [`NumberedIdentityPositions::contains`] is what `any` returns. A pass that
/// appends items keeps one and records each item it appends.
pub(crate) struct NumberedIdentityPositions {
    prefix: &'static str,
    /// `None` marks a number that two or more items carry.
    positions: BTreeMap<u32, Option<usize>>,
}

impl NumberedIdentityPositions {
    pub(crate) fn new(
        ids: impl IntoIterator<Item = impl AsRef<str>>,
        prefix: &'static str,
    ) -> Self {
        let mut indexed = Self {
            prefix,
            positions: BTreeMap::new(),
        };
        for (position, id) in ids.into_iter().enumerate() {
            indexed.admit(id.as_ref(), position);
        }
        indexed
    }

    /// Index the item at `position`, which must not be indexed yet.
    pub(crate) fn admit(&mut self, id: &str, position: usize) {
        let Some(number) = crate::identity::numbered_identity(id, self.prefix) else {
            return;
        };
        match self.positions.entry(number) {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(position));
            }
        }
    }

    /// Return whether any item carries the numbered identity.
    pub(crate) fn contains(&self, number: u32) -> bool {
        self.positions.contains_key(&number)
    }

    /// Return the only item that carries the numbered identity.
    pub(crate) fn unique(&self, number: u32) -> Option<usize> {
        self.positions.get(&number).copied().flatten()
    }
}

/// Positions of items keyed by their exact identity string.
///
/// [`ExactIdPositions::first`] returns what `find` returns over items whose
/// identity equals the query. [`ExactIdPositions::unique`] returns what
/// `exactly_one` returns. [`ExactIdPositions::contains`] is what `any`
/// returns. A pass that appends items keeps one and records each item it
/// appends.
pub(crate) struct ExactIdPositions {
    /// First position carrying the identity.
    first: BTreeMap<String, usize>,
    /// Identities carried by two or more items.
    duplicates: BTreeSet<String>,
}

impl ExactIdPositions {
    pub(crate) fn new(ids: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let mut indexed = Self {
            first: BTreeMap::new(),
            duplicates: BTreeSet::new(),
        };
        for (position, id) in ids.into_iter().enumerate() {
            indexed.admit(id.as_ref(), position);
        }
        indexed
    }

    /// Index the item at `position`, which must not be indexed yet.
    pub(crate) fn admit(&mut self, id: &str, position: usize) {
        match self.first.entry(id.to_owned()) {
            std::collections::btree_map::Entry::Occupied(_) => {
                self.duplicates.insert(id.to_owned());
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(position);
            }
        }
    }

    /// Return whether any item carries `id`.
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.first.contains_key(id)
    }

    /// Return the first item that carries `id`.
    pub(crate) fn first(&self, id: &str) -> Option<usize> {
        self.first.get(id).copied()
    }

    /// Return the only item that carries `id`.
    pub(crate) fn unique(&self, id: &str) -> Option<usize> {
        if self.duplicates.contains(id) {
            None
        } else {
            self.first.get(id).copied()
        }
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
    use super::{exactly_one, ExactIdPositions, NumberedIdentityPositions, UniqueById};

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

    fn parameter_record(surface_id: u32, offset: usize) -> crate::surface::SurfaceParameterRecord {
        crate::surface::SurfaceParameterRecord {
            surface_id,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
                crate::surface::SurfaceKind::Plane,
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset,
            type26_minor_radius: crate::surface::InlineType26MinorRadius::Absent,
            body_offset: 0,
        }
    }

    #[test]
    fn unique_by_id_matches_unique_surface_parameter() {
        let records = [
            parameter_record(3, 10),
            parameter_record(1, 20),
            parameter_record(3, 30),
            parameter_record(0, 40),
            parameter_record(7, 50),
            parameter_record(5, 60),
            parameter_record(7, 70),
        ];
        let index = UniqueById::new(&records, |record| record.surface_id);
        for id in 0..=8 {
            assert_eq!(
                index.get(id).map(|record| record.offset),
                crate::surface::unique_surface_parameter(&records, id).map(|record| record.offset),
                "{id}"
            );
        }
    }

    #[test]
    fn numbered_identity_positions_match_the_prefix_scan() {
        let prefix = "creo:visibgeom:point#";
        let ids = [
            "creo:visibgeom:point#1",
            "creo:visibgeom:point#2",
            "creo:visibgeom:point#02",
            "creo:visibgeom:vertex#3",
            "creo:visibgeom:point#3",
            "creo:visibgeom:point#2",
            "creo:visibgeom:point#4",
            "creo:novisgeom:point#5",
            "creo:visibgeom:point#4294967295",
        ];
        let scanned = |ids: &[&str], number: u32| {
            (
                ids.iter()
                    .any(|id| crate::identity::matches_numbered_identity(id, prefix, number)),
                exactly_one(ids.iter().enumerate().filter_map(|(position, id)| {
                    crate::identity::matches_numbered_identity(id, prefix, number)
                        .then_some(position)
                })),
            )
        };
        let index = NumberedIdentityPositions::new(ids, prefix);
        for number in [0, 1, 2, 3, 4, 5, 6, u32::MAX] {
            assert_eq!(
                (index.contains(number), index.unique(number)),
                scanned(&ids, number),
                "{number}"
            );
        }

        let mut positions = NumberedIdentityPositions::new(ids.iter().take(3).copied(), prefix);
        for appended in 3..=ids.len() {
            let slice = &ids[..appended];
            for number in [0, 1, 2, 3, 4, 5, 6, u32::MAX] {
                assert_eq!(
                    (positions.contains(number), positions.unique(number)),
                    scanned(slice, number),
                    "{number} after {appended} ids"
                );
            }
            if appended < ids.len() {
                positions.admit(ids[appended], appended);
            }
        }
    }

    #[test]
    fn exact_id_positions_match_the_equality_scan() {
        let ids = [
            "creo:visibgeom:surface#1",
            "creo:visibgeom:surface#2",
            "creo:visibgeom:surface#02",
            "creo:novisgeom:surface#2",
            "creo:visibgeom:surface#2",
            "creo:actdatums:surface#4",
            "creo:visibgeom:pcurve#1:5",
            "creo:visibgeom:pcurve#1:5",
            "creo:visibgeom:curve#7",
        ];
        let scanned =
            |ids: &[&str], id: &str| {
                (
                    ids.contains(&id),
                    ids.iter().position(|candidate| *candidate == id),
                    exactly_one(ids.iter().enumerate().filter_map(|(position, candidate)| {
                        (*candidate == id).then_some(position)
                    })),
                )
            };
        let queries = [
            "creo:visibgeom:surface#1",
            "creo:visibgeom:surface#2",
            "creo:visibgeom:surface#02",
            "creo:novisgeom:surface#2",
            "creo:actdatums:surface#4",
            "creo:visibgeom:pcurve#1:5",
            "creo:visibgeom:curve#7",
            "creo:visibgeom:surface#3",
        ];
        let index = ExactIdPositions::new(ids);
        for id in queries {
            assert_eq!(
                (index.contains(id), index.first(id), index.unique(id)),
                scanned(&ids, id),
                "{id}"
            );
        }

        let mut positions = ExactIdPositions::new(ids.iter().take(3).copied());
        for appended in 3..=ids.len() {
            let slice = &ids[..appended];
            for id in queries {
                assert_eq!(
                    (
                        positions.contains(id),
                        positions.first(id),
                        positions.unique(id)
                    ),
                    scanned(slice, id),
                    "{id} after {appended} ids"
                );
            }
            if appended < ids.len() {
                positions.admit(ids[appended], appended);
            }
        }
    }
}
