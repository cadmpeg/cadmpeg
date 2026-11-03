use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::super::component_paths::component_feature_precedes;
use super::super::feature_sources::{FeatureSources, SourcedFeature};
use super::{
    compact_component_reference_list_at, compact_edge_component_path, ComponentPathReference,
};
use crate::layout::component_edge_type_signature as signature;
use crate::records::{FeatureHistory, FeatureInputComponentPathEntry, FeatureInputLane};

#[derive(Clone, Copy)]
enum Lookup<'a> {
    Unique(SourcedFeature<'a>),
    Ambiguous,
}

pub(crate) struct OwnerFeatures<'a> {
    by_source: HashMap<u32, Lookup<'a>>,
    by_id: HashMap<&'a str, Lookup<'a>>,
}

impl<'a> OwnerFeatures<'a> {
    pub(crate) fn for_lane(features: &FeatureSources<'a>, lane: &FeatureInputLane) -> Self {
        if lane.edge_selections.is_empty() {
            Self {
                by_source: HashMap::new(),
                by_id: HashMap::new(),
            }
        } else {
            Self::new(features)
        }
    }

    pub(crate) fn new(features: &FeatureSources<'a>) -> Self {
        let mut by_source = HashMap::new();
        let mut by_id = HashMap::new();
        for feature in features.iter() {
            by_id
                .entry(feature.feature.id.as_str())
                .and_modify(|entry| *entry = Lookup::Ambiguous)
                .or_insert(Lookup::Unique(feature));
            if let Some(source) = feature.source_value() {
                by_source
                    .entry(source)
                    .and_modify(|entry| *entry = Lookup::Ambiguous)
                    .or_insert(Lookup::Unique(feature));
            }
        }
        Self { by_source, by_id }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnerRefusal {
    ConsumerMissing,
    ConsumerAmbiguous,
    ExplicitOwnerMissing,
    OwnerAmbiguous,
    DifferentHistory,
    NonPreceding,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OwnerResolution {
    Absent,
    Resolved(String),
    Refused(OwnerRefusal),
}

impl OwnerResolution {
    pub(crate) fn into_feature(self) -> Option<String> {
        match self {
            Self::Resolved(feature) => Some(feature),
            Self::Absent | Self::Refused(_) => None,
        }
    }
}

pub(crate) fn resolve(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &OwnerFeatures<'_>,
    consumer_ref: &str,
) -> Result<OwnerResolution, CodecError> {
    let count = marker
        .checked_sub(12)
        .and_then(|offset| View::u32_le_at(payload, offset))
        .and_then(|count| usize::try_from(count).ok());
    let Some(count) = count else {
        return Ok(OwnerResolution::Absent);
    };
    let owner_source = if compact_component_reference_list_at(payload, marker).is_some() {
        None
    } else {
        let Some(ComponentPathReference(_, owner)) =
            compact_edge_component_path(ctx, payload, marker, count)?
        else {
            return Ok(OwnerResolution::Absent);
        };
        owner
    };
    let consumer = match features.by_id.get(consumer_ref) {
        Some(Lookup::Unique(feature)) => *feature,
        Some(Lookup::Ambiguous) => {
            return Ok(OwnerResolution::Refused(OwnerRefusal::ConsumerAmbiguous));
        }
        None => return Ok(OwnerResolution::Refused(OwnerRefusal::ConsumerMissing)),
    };
    let producer = if let Some(source) = owner_source {
        match features.by_source.get(&source) {
            Some(Lookup::Unique(feature)) => *feature,
            Some(Lookup::Ambiguous) => {
                return Ok(OwnerResolution::Refused(OwnerRefusal::OwnerAmbiguous));
            }
            None => return Ok(OwnerResolution::Refused(OwnerRefusal::ExplicitOwnerMissing)),
        }
    } else {
        let mut terminal = None;
        for component in components.iter().rev() {
            let Some(source) = View::u32_le_at(
                component.type_signature.as_ref(),
                signature::NATIVE_FEATURE_OBJECT_ID,
            ) else {
                continue;
            };
            match features.by_source.get(&source) {
                Some(Lookup::Unique(feature)) => {
                    terminal = Some(*feature);
                    break;
                }
                Some(Lookup::Ambiguous) => {
                    return Ok(OwnerResolution::Refused(OwnerRefusal::OwnerAmbiguous));
                }
                None => {}
            }
        }
        let Some(terminal) = terminal else {
            return Ok(OwnerResolution::Absent);
        };
        terminal
    };
    if producer.feature.parent != consumer.feature.parent {
        return Ok(OwnerResolution::Refused(OwnerRefusal::DifferentHistory));
    }
    if !component_feature_precedes(producer, consumer) {
        return Ok(OwnerResolution::Refused(OwnerRefusal::NonPreceding));
    }
    Ok(OwnerResolution::Resolved(producer.feature.id.clone()))
}

pub(crate) fn refused_selection_count(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<usize, CodecError> {
    let mut count = 0usize;
    for lane in lanes.iter().filter(|lane| !lane.edge_selections.is_empty()) {
        let sources = FeatureSources::for_lane_iter(
            histories.iter().flat_map(|history| &history.features),
            lane,
        );
        let features = OwnerFeatures::new(&sources);
        for selection in &lane.edge_selections {
            let Some(offset) = usize::try_from(selection.offset).ok() else {
                continue;
            };
            if matches!(
                resolve(
                    ctx,
                    &lane.native_payload,
                    offset,
                    &selection.components,
                    &features,
                    &selection.feature_ref,
                )?,
                OwnerResolution::Refused(_)
            ) {
                count = count.checked_add(1).ok_or_else(|| {
                    CodecError::malformed("SLDPRT refused edge selection count overflows usize")
                })?;
            }
        }
    }
    Ok(count)
}
