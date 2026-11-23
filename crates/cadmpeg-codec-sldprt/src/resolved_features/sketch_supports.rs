// SPDX-License-Identifier: Apache-2.0
//! The reference plane each sketch's profile names: the principal plane that
//! its own principal-plane component records name, else the compact
//! `moCompRefPlane_c` record of `docs/formats/sldprt.md`.

use std::collections::HashMap;

use crate::classification::{classify, FeatureClass};
use crate::records::{FeatureHistory, FeatureInputLane};

use super::compact_reference_planes::CompactReferencePlaneIndex;
use super::curves::{lane_sketch_plane_frames, SketchPlaneFrame};
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
            let plane = match principal_profile_frame(&plane_frames, &index, context, start, end) {
                Some((_, plane)) => principal_plane_feature(&plane_frames, &by_object, plane),
                None => index
                    .profile_source(context, start, end)
                    .and_then(|object| by_object.get(&object).copied().flatten()),
            };
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

/// The history feature of the lane's principal plane `plane`, where one
/// feature is that plane.
fn principal_plane_feature<'a>(
    plane_frames: &HashMap<u32, SketchPlaneFrame>,
    by_object: &HashMap<u32, Option<&'a str>>,
    plane: cadmpeg_ir::features::PrincipalPlane,
) -> Option<&'a str> {
    let mut features = plane_frames
        .iter()
        .filter(|(_, frame)| frame.principal == Some(plane))
        .filter_map(|(object, _)| by_object.get(object).copied().flatten());
    let feature = features.next()?;
    features.all(|other| other == feature).then_some(feature)
}

#[cfg(test)]
mod tests {
    use super::principal_plane_feature;
    use crate::resolved_features::curves::SketchPlaneFrame;
    use cadmpeg_ir::features::PrincipalPlane;
    use std::collections::HashMap;

    /// A profile that its own principal record places is supported by the
    /// lane's datum of that plane, whatever object number the datum has.
    #[test]
    fn a_principal_binding_names_the_lane_datum_of_its_plane() {
        let frames = HashMap::from([
            (2, SketchPlaneFrame::principal(PrincipalPlane::Front)),
            (4, SketchPlaneFrame::principal(PrincipalPlane::Top)),
            (5, SketchPlaneFrame::principal(PrincipalPlane::Right)),
        ]);
        let by_object = HashMap::from([(2, Some("front")), (4, Some("top")), (5, Some("right"))]);
        assert_eq!(
            principal_plane_feature(&frames, &by_object, PrincipalPlane::Top),
            Some("top")
        );
        assert_eq!(
            principal_plane_feature(&frames, &by_object, PrincipalPlane::Right),
            Some("right")
        );
        let mut twice = frames.clone();
        twice.insert(9, SketchPlaneFrame::principal(PrincipalPlane::Top));
        let mut two_datums = by_object.clone();
        two_datums.insert(9, Some("other top"));
        assert_eq!(
            principal_plane_feature(&twice, &two_datums, PrincipalPlane::Top),
            None
        );
        two_datums.insert(9, Some("top"));
        assert_eq!(
            principal_plane_feature(&twice, &two_datums, PrincipalPlane::Top),
            Some("top")
        );
    }
}
