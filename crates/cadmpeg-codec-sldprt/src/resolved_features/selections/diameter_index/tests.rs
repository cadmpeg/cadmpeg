use super::*;
use crate::records::{FeatureInputName, FeatureInputScalarRole, FeatureSource};
use std::collections::BTreeMap;

fn lane() -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 1024],
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "diameter-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 120,
                object_id: Some(ObjectId::Absent),
                value: "D2".into(),
            },
            FeatureInputName {
                id: "next-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 400,
                object_id: None,
                value: "Next".into(),
            },
        ],
        scalars: vec![FeatureInputScalar {
            id: "diameter".into(),
            parent: "lane".into(),
            feature_ref: None,
            ordinal: 0,
            offset: 150,
            object_id: 52,
            name: "diameter-name".into(),
            value: cadmpeg_ir::scalar::FiniteReal::new(0.008).unwrap(),
            role: FeatureInputScalarRole::Native,
            operands: Vec::new(),
        }],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn feature() -> Feature {
    Feature {
        id: "thread".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(53),
        ordinal: 0,
        name: "Thread".into(),
        kind: "Feature".into(),
        input_class: Some("moCosmeticThread_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

#[test]
fn diameter_interval_index_reuses_lane_work_and_keeps_absent_source_lazy() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let lane = lane();
    let mut feature = feature();
    let index = CosmeticDiameterIndex::new(&ctx, &lane).unwrap();
    feature.source_id = None;
    assert_eq!(index.tail(&ctx, &feature).unwrap(), None);
    assert!(index.records.get().is_none());
    feature.source_id = FeatureSource::from_value(53);
    assert_eq!(index.tail(&ctx, &feature).unwrap(), Some(158..400));
    let original = std::ptr::from_ref(index.records.get().unwrap());
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "index SLDPRT cosmetic diameter intervals",
        None,
    );
    for _ in 0..16 {
        assert_eq!(index.tail(&ctx, &feature).unwrap(), Some(158..400));
    }
    assert_eq!(std::ptr::from_ref(index.records.get().unwrap()), original);
}

#[test]
fn diameter_interval_index_uses_last_name_and_rejects_duplicate_scalars() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let feature = feature();
    let mut lane = lane();
    let mut last = lane.names[0].clone();
    last.value = "D1".into();
    lane.names.push(last);
    assert_eq!(
        CosmeticDiameterIndex::new(&ctx, &lane)
            .unwrap()
            .tail(&ctx, &feature)
            .unwrap(),
        None
    );
    lane.names.last_mut().unwrap().value = "D2".into();
    assert_eq!(
        CosmeticDiameterIndex::new(&ctx, &lane)
            .unwrap()
            .tail(&ctx, &feature)
            .unwrap(),
        Some(158..400)
    );
    let mut duplicate = lane.scalars[0].clone();
    duplicate.offset = 200;
    lane.scalars.push(duplicate);
    assert_eq!(
        CosmeticDiameterIndex::new(&ctx, &lane)
            .unwrap()
            .tail(&ctx, &feature)
            .unwrap(),
        None
    );
}
