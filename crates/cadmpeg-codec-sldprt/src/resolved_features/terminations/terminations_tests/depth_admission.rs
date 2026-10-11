use crate::records::{
    Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureInputScalar,
    FeatureInputScalarRole, FeatureSource, ObjectId,
};
use std::collections::BTreeMap;

#[test]
fn blind_extrusion_depth_name_has_a_comparison_boundary() {
    for value in ["D1", "Depth", "D2"] {
        let mut payload = vec![0; 126];
        payload[100..102].copy_from_slice(&[0x0c, 0x8e]);
        payload[104] = 1;
        payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
        let block = payload.len();
        payload.resize(block + 16, 0);
        payload[block + 9] = 0x20;
        payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
        payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
        payload.resize(350, 0);
        let histories = vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![Feature {
                id: "extrusion".into(),
                parent: "history".into(),
                xml_tag: "Feature".into(),
                tree_parent: None,
                source_id: FeatureSource::from_value(7),
                ordinal: 0,
                name: "extrusion".into(),
                kind: "Boss-Extrude".into(),
                input_class: Some("moICE_c".into()),
                suppressed: false,
                parameters: BTreeMap::new(),
                dimension_properties: BTreeMap::new(),
                properties: BTreeMap::new(),
                text: None,
                content: Vec::new(),
            }],
        }];
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: vec![
                FeatureInputName {
                    id: "owner-name".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 0,
                    value: "extrusion".into(),
                    object_id: ObjectId::from_value(7),
                },
                FeatureInputName {
                    id: "depth-name".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: 250,
                    value: value.into(),
                    object_id: None,
                },
            ],
            scalars: vec![FeatureInputScalar {
                id: "depth".into(),
                parent: "lane".into(),
                feature_ref: None,
                ordinal: 0,
                offset: 300,
                object_id: 0,
                name: "depth-name".into(),
                value: cadmpeg_ir::scalar::FiniteReal::new(0.005).unwrap(),
                role: FeatureInputScalarRole::Driving,
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
        };
        let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let mut histories = histories.clone();
            super::super::enrich_history_extrusion_terminations(
                ctx,
                &mut histories,
                std::slice::from_ref(&lane),
            )?;
            Ok(histories)
        };
        let admitted = solve(&cadmpeg_test_support::service_decode_context()).unwrap();
        assert_eq!(
            admitted[0].features[0]
                .properties
                .get("EndCondition")
                .map(String::as_str),
            Some("Blind")
        );
        crate::test_support::work_refusal_at("compare SLDPRT extrusion D1 depth name", solve);
        if value != "D1" {
            crate::test_support::work_refusal_at("compare SLDPRT extrusion named depth", solve);
        }
    }
}
