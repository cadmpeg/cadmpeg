// SPDX-License-Identifier: Apache-2.0

use crate::validate::validate_neutral;
use crate::CadIr;

use crate::features::tree::{FeatureTreeNodeRole, TreeChildren};
use crate::features::{Feature, FeatureDefinition, FeatureOperation};

fn predecessor_document() -> CadIr {
    let mut ir = CadIr::empty();
    for (key, ordinal) in [("0-parent", 0), ("1-child", 1)] {
        ir.model.features.push(Feature {
            product_definition: None,
            id: format!("test:parent-wire:feature#{key}")
                .try_into()
                .unwrap(),
            ordinal,
            name: None,
            suppressed: None,
            dependencies: crate::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),
            evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
            native_ref: None,
        });
    }
    ir.model
        .set_feature_regeneration_parent(
            &("test:parent-wire:feature#1-child".try_into().unwrap()),
            &("test:parent-wire:feature#0-parent".try_into().unwrap()),
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
        assert!(validate_neutral(&ir, Vec::new())
            .expect("validate the neutral model")
            .is_ok());
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
        assert!(
            cadmpeg_core::serde_erase::json::to_value(&ir.model).is_err(),
            "{mutation}"
        );
        assert!(
            cadmpeg_core::serde_erase::json::to_value(&ir).is_err(),
            "{mutation}"
        );
        assert!(ir.to_canonical_json().is_err(), "{mutation}");
        let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
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
