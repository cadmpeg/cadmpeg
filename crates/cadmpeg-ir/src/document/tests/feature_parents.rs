// SPDX-License-Identifier: Apache-2.0

use crate::validate::validate_neutral;
use crate::CadIr;

use crate::features::{
    Feature, FeatureDefinition, FeatureOperation, FeatureTreeNodeRole, TreeChildren,
};

fn predecessor_document() -> CadIr {
    let mut ir = CadIr::empty();
    for (key, ordinal) in [("0-parent", 0), ("1-child", 1)] {
        ir.model.features.push(Feature {
            id: format!("test:parent-wire:feature#{key}")
                .try_into()
                .unwrap(),
            ordinal,
            name: None,
            suppressed: None,
            dependencies: crate::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),
            evaluation: crate::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
            native_ref: None,
        });
    }
    ir.model
        .set_feature_regeneration_parent(
            "test:parent-wire:feature#1-child".try_into().unwrap(),
            "test:parent-wire:feature#0-parent".try_into().unwrap(),
        )
        .unwrap();
    ir
}

#[test]
fn mutated_parent_graphs_refuse_writing_and_produce_located_validation_findings() {
    for mutation in [
        "missing parent",
        "missing child",
        "ordinal",
        "tree ownership",
    ] {
        let mut ir = predecessor_document();
        let before = ir.to_canonical_json().unwrap();
        assert_eq!(CadIr::from_json(&before).unwrap(), ir);
        assert!(validate_neutral(&ir, Vec::new()).is_ok());
        match mutation {
            "missing parent" => {
                ir.model.features.remove(0);
            }
            "missing child" => {
                ir.model.features.remove(1);
            }
            "ordinal" => {
                ir.model.features[1].ordinal = 0;
            }
            "tree ownership" => {
                ir.model.features[0]
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(FeatureOperation::TreeNode {
                        role: FeatureTreeNodeRole::History,
                        children: TreeChildren::new(
                            vec!["test:parent-wire:feature#1-child".try_into().unwrap()],
                            None,
                        )
                        .unwrap(),
                    }));
            }
            _ => unreachable!(),
        }
        assert!(serde_json::to_value(&ir.model).is_err(), "{mutation}");
        assert!(serde_json::to_value(&ir).is_err(), "{mutation}");
        assert!(ir.to_canonical_json().is_err(), "{mutation}");
        let report = validate_neutral(&ir, Vec::new());
        assert!(
            report.findings.iter().any(|finding| {
                finding.check == crate::report::check::Check::ReferentialIntegrity
                    && finding.severity == crate::report::Severity::Error
                    && finding.entity.as_deref() == Some("test:parent-wire:feature#1-child")
            }),
            "{mutation}: {:?}",
            report.findings
        );
    }
}

#[test]
fn complete_cadir_admission_checks_the_predecessor_against_current_feature_rows() {
    let ir = predecessor_document();
    let wire = serde_json::to_value(&ir).unwrap();
    assert_eq!(serde_json::from_value::<CadIr>(wire.clone()).unwrap(), ir);
    for mutation in ["missing parent", "ordinal", "tree ownership"] {
        let mut invalid = wire.clone();
        match mutation {
            "missing parent" => {
                invalid["model"]["features"][1]["regeneration_parent"] =
                    serde_json::json!("test:parent-wire:feature#absent");
            }
            "ordinal" => {
                invalid["model"]["features"][1]["ordinal"] = serde_json::json!(0);
            }
            "tree ownership" => {
                invalid["model"]["features"][0]["definition"] = serde_json::json!({
                    "definition": "tree_node", "role": "history", "children": {"children": ["test:parent-wire:feature#1-child"]}
                });
            }
            _ => unreachable!(),
        }
        let error = serde_json::from_value::<CadIr>(invalid)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("test:parent-wire:feature#1-child"),
            "{mutation}: {error}"
        );
    }
}
