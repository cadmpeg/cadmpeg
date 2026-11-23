// SPDX-License-Identifier: Apache-2.0
//! The reference plane each sketch's profile names: the principal plane that
//! its own principal-plane component records name, else the compact
//! `moCompRefPlane_c` record of `docs/formats/sldprt.md`.

use std::collections::HashMap;

use crate::classification::{classify, FeatureClass};
use crate::records::{FeatureHistory, FeatureInputLane};

use super::compact_reference_planes::CompactReferencePlaneIndex;
use super::curves::lane_sketch_plane_frames;
use super::holes::{feature_object_byte_ranges, principal_profile_frame};
use super::scalars::FeatureObjectNames;

/// The native record of the reference plane each sketch feature's profile
/// names, by the sketch's native record. A sketch whose lanes name two
/// planes, or a plane no history record is, has none.
pub(crate) fn sketch_supports<'a>(
    features: &[cadmpeg_ir::features::Feature],
    histories: &'a [FeatureHistory],
    lanes: &[FeatureInputLane],
) -> HashMap<&'a str, &'a str> {
    let mut candidates = HashMap::<&str, Option<&str>>::new();
    for lane in lanes {
        let ranges = feature_object_byte_ranges(histories, lane);
        let index = CompactReferencePlaneIndex::new(&lane.native_payload);
        let plane_frames = lane_sketch_plane_frames(features, histories, lane);
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
            let object =
                if principal_profile_frame(&plane_frames, &index, context, start, end).is_some() {
                    index.principal_profile_source(start, end)
                } else {
                    index.profile_source(context, start, end)
                };
            let plane = object.and_then(|object| by_object.get(&object).copied().flatten());
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
