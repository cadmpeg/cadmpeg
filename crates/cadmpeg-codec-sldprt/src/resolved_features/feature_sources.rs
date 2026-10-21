//! Borrowed history features with lane-local source identities.

use super::scalars::FeatureObjectNames;
#[cfg(test)]
use crate::records::FeatureInputLane;
use crate::records::{Feature, FeatureSource, ObjectId};
use std::collections::HashMap;

/// The source one lane adds to `feature`, given that lane's object names.
pub(super) fn lane_added_source(
    feature: &Feature,
    names: &FeatureObjectNames<'_>,
) -> Option<FeatureSource> {
    if feature.source_id.is_some() {
        return None;
    }
    names
        .of(feature)
        .and_then(|name| name.object_id)
        .and_then(ObjectId::value)
        .and_then(FeatureSource::from_value)
}

pub(crate) struct FeatureSources<'a> {
    features: Vec<&'a Feature>,
    added: HashMap<usize, FeatureSource>,
}

impl<'a> FeatureSources<'a> {
    pub(crate) fn new(features: &'a [Feature]) -> Self {
        Self::from_refs(features.iter())
    }

    fn from_refs(features: impl IntoIterator<Item = &'a Feature>) -> Self {
        Self {
            features: features.into_iter().collect(),
            added: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_lane(features: &'a [Feature], lane: &FeatureInputLane) -> Self {
        Self::for_lane_iter(features.iter(), lane)
    }

    #[cfg(test)]
    pub(crate) fn for_lane_iter(
        features: impl IntoIterator<Item = &'a Feature>,
        lane: &FeatureInputLane,
    ) -> Self {
        let mut view = Self::from_refs(features);
        if !lane.edge_selections.is_empty() || !lane.surface_selections.is_empty() {
            let names = FeatureObjectNames::new(&lane.names);
            view.added = view
                .features
                .iter()
                .enumerate()
                .filter_map(|(index, feature)| Some((index, lane_added_source(feature, &names)?)))
                .collect();
        }
        view
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = SourcedFeature<'a>> + '_ {
        self.features
            .iter()
            .copied()
            .enumerate()
            .map(|(index, feature)| SourcedFeature {
                feature,
                source: feature
                    .source_id
                    .or_else(|| self.added.get(&index).copied()),
            })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SourcedFeature<'a> {
    pub(crate) feature: &'a Feature,
    source: Option<FeatureSource>,
}

impl SourcedFeature<'_> {
    pub(crate) fn source_value(self) -> Option<u32> {
        self.source.and_then(FeatureSource::value)
    }
}

impl<'a> From<&'a Feature> for SourcedFeature<'a> {
    fn from(feature: &'a Feature) -> Self {
        Self {
            feature,
            source: feature.source_id,
        }
    }
}

#[cfg(test)]
mod tests;
