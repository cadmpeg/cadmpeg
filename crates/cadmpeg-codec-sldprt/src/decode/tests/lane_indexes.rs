//! Lane name and unclassed feature indexes match their scans.

use crate::brep::feature_source::FeatureSourceId;
use crate::records::{Feature, FeatureInputName, FeatureSource, ObjectId};
use std::collections::BTreeMap;

fn feature(ordinal: u32, source: Option<u32>, input_class: Option<String>) -> Feature {
    Feature {
        id: format!("synthetic:feature#{ordinal}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: source
            .map(|source| FeatureSource::Id(FeatureSourceId::try_from(source).unwrap())),
        ordinal,
        name: format!("Feature{ordinal}"),
        kind: "Extrude".into(),
        input_class,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

#[test]
fn lane_indexes_match_the_name_and_feature_scans() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from((state >> 33) % bound).unwrap()
    };
    for _ in 0..64 {
        let names = (0..next(40))
            .map(|ordinal| FeatureInputName {
                id: format!("name-{ordinal}"),
                parent: "lane".into(),
                ordinal,
                offset: u64::from(next(30)),
                object_id: ObjectId::from_value(next(5)),
                value: format!("Feature{ordinal}"),
            })
            .collect::<Vec<_>>();
        let by_offset = super::super::names_by_offset(&names);
        for offset in 0..32 {
            let scanned = names.iter().find(|name| name.offset == offset);
            assert_eq!(
                by_offset.get(&offset).map(|name| std::ptr::from_ref(*name)),
                scanned.map(std::ptr::from_ref)
            );
        }

        let features = (0..next(40))
            .map(|ordinal| {
                let source = next(6);
                feature(
                    ordinal,
                    (source > 0).then_some(source),
                    (next(3) == 0).then(|| "class".to_owned()),
                )
            })
            .collect::<Vec<_>>();
        let by_source = super::super::unclassed_features_by_source(&features);
        for source in 0..7 {
            let scanned = features
                .iter()
                .filter(|feature| {
                    feature.source_value() == Some(source) && feature.input_class.is_none()
                })
                .map(std::ptr::from_ref)
                .collect::<Vec<_>>();
            let indexed = by_source
                .get(&source)
                .into_iter()
                .flatten()
                .map(|feature| std::ptr::from_ref(*feature))
                .collect::<Vec<_>>();
            assert_eq!(indexed, scanned);
        }
    }
}
