// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation,
};
use std::collections::BTreeMap;

fn document(history: bool) -> CadIr {
    let mut ir = crate::test_support::root_part_ir();
    let body: cadmpeg_ir::topology::Body = serde_json::from_value(
        serde_json::json!({"id":"inventor:brep:body#synthetic","kind":"solid"}),
    )
    .expect("body");
    ir.model.product_definitions[0].bodies.push(body.id.clone());
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("inventor:design:feature#base").expect("id"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            [body.id.clone()].into_iter().collect(),
        ),
        native_ref: None,
    });
    if history {
        ir.model.history_bodies.push(
            serde_json::from_value(serde_json::json!({
                "id": "inventor:history:body#synthetic",
                "produced_by": "inventor:design:feature#base",
                "state": {"ordinal": 0, "identifier": 1},
                "topology": {
                    "body": {"id":"inventor:history:body#synthetic.body","kind":"solid"},
                    "regions": [], "shells": [], "faces": [], "loops": [],
                    "coedges": [], "edges": [], "vertices": [], "surfaces": [],
                    "curves": [], "pcurves": [], "points": []
                }
            }))
            .expect("history body"),
        );
    }
    ir.geometry.bodies.push(body);
    ir
}

#[test]
fn history_bodies_keep_all_legacy_design_records() {
    let mut ir = document(true);
    let before = ir.model.clone();
    assert!(!contract(&mut ir));
    assert_eq!(ir.model, before);
}

#[test]
fn documents_without_history_bodies_allow_design_contraction() {
    let mut ir = document(false);
    let geometry = ir.geometry.clone();
    let definitions = ir.model.product_definitions.clone();
    assert!(contract(&mut ir));
    assert!(ir.model.features.is_empty());
    assert_eq!(ir.geometry, geometry);
    assert_eq!(ir.model.product_definitions, definitions);
}
