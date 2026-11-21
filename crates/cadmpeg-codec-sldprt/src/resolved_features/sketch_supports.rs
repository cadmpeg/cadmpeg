// SPDX-License-Identifier: Apache-2.0
//! The reference plane each sketch's profile names: the principal plane of
//! its local component records, else the compact `moCompRefPlane_c` record
//! of `docs/formats/sldprt.md`.

use std::collections::HashMap;

use crate::classification::{classify, FeatureClass};
use crate::records::{FeatureHistory, FeatureInputLane};

use super::compact_reference_planes::{principal_source, CompactReferencePlaneIndex};
use super::holes::feature_object_byte_ranges;
use super::scalars::FeatureObjectNames;

/// The native record of the reference plane each sketch feature's profile
/// names, by the sketch's native record. A sketch whose lanes name two
/// planes, or a plane no history record is, has none.
pub(crate) fn sketch_supports<'a>(
    histories: &'a [FeatureHistory],
    lanes: &[FeatureInputLane],
) -> HashMap<&'a str, &'a str> {
    let mut candidates = HashMap::<&str, Option<&str>>::new();
    for lane in lanes {
        let ranges = feature_object_byte_ranges(histories, lane);
        let index = CompactReferencePlaneIndex::new(&lane.native_payload);
        let names = FeatureObjectNames::new(&lane.names);
        let mut by_object = HashMap::<u32, Option<&str>>::new();
        for feature in histories.iter().flat_map(|history| &history.features) {
            let Some(object) = names
                .of(feature)
                .and_then(|name| name.object_id)
                .and_then(crate::records::ObjectId::value)
            else {
                continue;
            };
            by_object
                .entry(object)
                .and_modify(|found| *found = None)
                .or_insert(Some(feature.id.as_str()));
        }
        for sketch in histories
            .iter()
            .flat_map(|history| &history.features)
            .filter(|feature| classify(feature) == Some(FeatureClass::Sketch))
        {
            let Some(&(context, start, end)) = ranges.get(sketch.id.as_str()) else {
                continue;
            };
            let plane = index
                .local_principal_component(context, start, end)
                .map(principal_source)
                .or_else(|| index.profile_source(context, start, end))
                .and_then(|object| by_object.get(&object).copied().flatten());
            let Some(plane) = plane else {
                continue;
            };
            candidates
                .entry(sketch.id.as_str())
                .and_modify(|found| {
                    if *found != Some(plane) {
                        *found = None;
                    }
                })
                .or_insert(Some(plane));
        }
    }
    candidates
        .into_iter()
        .filter_map(|(sketch, plane)| Some((sketch, plane?)))
        .collect()
}
