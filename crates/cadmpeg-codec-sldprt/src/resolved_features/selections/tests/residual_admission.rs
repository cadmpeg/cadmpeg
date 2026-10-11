use crate::records::{Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId};
use std::collections::BTreeMap;

fn selected_object(class: Option<&str>, xml_tag: &str) -> (Vec<FeatureHistory>, FeatureInputLane) {
    let feature = Feature {
        id: "feature".into(), parent: "history".into(), xml_tag: xml_tag.into(),
        tree_parent: None, source_id: FeatureSource::from_value(7), ordinal: 0,
        name: "object".into(), kind: String::new(), input_class: class.map(str::to_owned),
        suppressed: false, parameters: BTreeMap::new(), dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(), text: None, content: Vec::new(),
    };
    let histories = vec![FeatureHistory { id: "history".into(), part_name: None,
        properties: BTreeMap::new(), content: Vec::new(), configurations: Vec::new(),
        features: vec![feature], }];
    let lane = FeatureInputLane {
        id: "lane".into(), configuration: None, native_payload: vec![0; 128], classes: Vec::new(),
        names: vec![FeatureInputName { id: "name".into(), parent: "lane".into(), ordinal: 0,
            offset: 0, value: "object".into(), object_id: ObjectId::from_value(7), }],
        scalars: Vec::new(), relation_bindings: Vec::new(), relation_instances: Vec::new(),
        body_selections: Vec::new(), edge_selections: Vec::new(), surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(), references: Vec::new(), sketch_entities: Vec::new(),
    };
    (histories, lane)
}

#[test]
fn surface_xml_tag_predicate_has_a_comparison_boundary() {
    for tag in ["Extrusion", "Cut", "Feature"] {
        let (histories, lane) = selected_object(None, tag);
        let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            super::super::compact_surface_selections(ctx, &histories, &histories[0].features, &lane, &[])
        };
        assert!(solve(&cadmpeg_test_support::service_decode_context()).unwrap().is_empty());
        crate::test_support::work_refusal_at("compare SLDPRT surface extrusion XML tag", solve);
        if tag != "Extrusion" {
            crate::test_support::work_refusal_at("compare SLDPRT surface cut XML tag", solve);
        }
    }
}

#[test]
fn surface_fillet_class_predicate_has_a_comparison_boundary() {
    for class in ["Fillet_c", "VarFillet_c"] {
        let (histories, lane) = selected_object(Some(class), "Feature");
        let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            super::super::compact_surface_selections(ctx, &histories, &histories[0].features, &lane, &[])
        };
        assert!(solve(&cadmpeg_test_support::service_decode_context()).unwrap().is_empty());
        crate::test_support::work_refusal_at("compare SLDPRT surface fillet class", solve);
    }
}
