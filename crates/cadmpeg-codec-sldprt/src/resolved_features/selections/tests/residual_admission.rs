use crate::records::{
    Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId,
};
use std::collections::BTreeMap;

fn selected_object(class: Option<&str>, xml_tag: &str) -> (Vec<FeatureHistory>, FeatureInputLane) {
    let feature = Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: xml_tag.into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(7),
        ordinal: 0,
        name: "object".into(),
        kind: String::new(),
        input_class: class.map(str::to_owned),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    }];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 128],
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            value: "object".into(),
            object_id: ObjectId::from_value(7),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (histories, lane)
}

#[test]
fn short_thicken_intervals_do_not_query_surface_tokens() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    const OPERATION: &str = "find SLDPRT Thicken surface token";
    for (length, start) in [(104, 0), (105, 0), (128, 78)] {
        let (histories, mut lane) = selected_object(Some("moThicken_c"), "Feature");
        lane.native_payload.resize(length, 0);
        lane.names[0].offset = start;
        lane.classes = (0..100)
            .map(|ordinal| crate::records::FeatureInputClass {
                id: format!("class-{ordinal}"),
                parent: "lane".into(),
                ordinal,
                offset: 0,
                name: "moCompSurfaceBody_c".into(),
            })
            .collect();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            OPERATION,
            None,
        );
        assert!(super::super::compact_surface_selections(
            &ctx,
            &histories,
            &histories[0].features,
            &lane,
            &[]
        )
        .unwrap()
        .is_empty());
        let cadmpeg_core::CodecError::ResourceLimit(limit) =
            ctx.charge_work(1, OPERATION).unwrap_err()
        else {
            panic!("the unread token lookup must leave the probe armed");
        };
        assert_eq!(limit.operation, OPERATION);
    }
    let (histories, mut lane) = selected_object(Some("moThicken_c"), "Feature");
    lane.classes.push(crate::records::FeatureInputClass {
        id: "surface-class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        name: "moCompSurfaceBody_c".into(),
    });
    crate::test_support::work_refusal_at(OPERATION, |ctx| {
        super::super::compact_surface_selections(
            ctx,
            &histories,
            &histories[0].features,
            &lane,
            &[],
        )
    });
}
