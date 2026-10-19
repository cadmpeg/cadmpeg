use super::{markers_by_owner, only_enclosed, FeaturePositions};
use crate::records::{SketchInputEntity, SketchInputKind};
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, SketchFeatureBinding,
};

fn feature(position: usize, native_ref: Option<String>) -> Feature {
    Feature {
        id: FeatureId::mint(format!("synthetic:test:id#feature-{position}"))
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        needs_recompute: false,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(None),
            },
        )),
        native_ref,
    }
}

#[test]
fn profile_indexes_match_the_feature_and_sketch_scans() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from((state >> 33) % bound).unwrap()
    };
    for _ in 0..200 {
        let features = (0..next(12))
            .map(|position| {
                let native = next(6);
                feature(position, (native > 0).then(|| format!("native-{native}")))
            })
            .collect::<Vec<_>>();
        let positions = FeaturePositions::new(&features);
        for native in (0..7).map(|native| format!("native-{native}")) {
            let scanned = features
                .iter()
                .enumerate()
                .filter(|(_, feature)| feature.native_ref.as_deref() == Some(native.as_str()))
                .map(|(position, _)| position)
                .collect::<Vec<_>>();
            assert_eq!(positions.of(&native), scanned.as_slice());
            assert_eq!(
                positions.first(&native),
                features
                    .iter()
                    .position(|feature| feature.native_ref.as_deref() == Some(native.as_str()))
            );
        }

        let sketches = (0..next(8))
            .map(|_| (next(4) > 0).then(|| u64::try_from(next(16)).unwrap()))
            .collect::<Vec<_>>();
        let mut offsets = sketches
            .iter()
            .enumerate()
            .filter_map(|(position, offset)| Some(((*offset)?, position)))
            .collect::<Vec<_>>();
        offsets.sort_unstable();
        for start in 0..17 {
            for end in (start..18).map(|end| (end < 17).then_some(end)) {
                let mut enclosed = sketches.iter().enumerate().filter(|(_, offset)| {
                    offset
                        .is_some_and(|offset| offset > start && end.is_none_or(|end| offset < end))
                });
                let scanned = match (enclosed.next(), enclosed.next()) {
                    (Some((position, _)), None) => Some(position),
                    _ => None,
                };
                assert_eq!(only_enclosed(&offsets, start, end), scanned);
            }
        }
    }
}

#[test]
fn owned_markers_match_the_lane_scans() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let base = SketchInputEntity::new("entity", "lane", 0, 0, SketchInputKind::Point);
    let owners = ["sketch-0", "sketch-1", "sketch-2", "absent"];
    for _ in 0..32 {
        let markers = (0..next(120))
            .map(|_| {
                let mut marker =
                    base.with_test_position(u32::try_from(next(40)).unwrap(), next(80));
                marker.feature_ref =
                    (next(5) != 0).then(|| owners[usize::try_from(next(3)).unwrap()].to_owned());
                marker
            })
            .collect::<Vec<_>>();
        let owned = markers_by_owner(&markers);
        for owner in owners {
            let scanned = markers
                .iter()
                .filter(|marker| marker.feature_ref.as_deref() == Some(owner))
                .collect::<Vec<_>>();
            let indexed = owned.get(owner).cloned().unwrap_or_default();
            assert_eq!(scanned.len(), indexed.len());
            assert!(scanned
                .iter()
                .zip(&indexed)
                .all(|(scanned, indexed)| std::ptr::eq(*scanned, *indexed)));
            assert_eq!(
                owned
                    .get(owner)
                    .and_then(|markers| markers.iter().map(|marker| marker.offset()).min()),
                scanned.iter().map(|marker| marker.offset()).min()
            );
            let end = next(90);
            assert_eq!(
                indexed
                    .iter()
                    .filter(|marker| marker.offset() < end)
                    .count(),
                markers
                    .iter()
                    .filter(|marker| {
                        marker.feature_ref.as_deref() == Some(owner) && marker.offset() < end
                    })
                    .count()
            );
        }
    }
}
