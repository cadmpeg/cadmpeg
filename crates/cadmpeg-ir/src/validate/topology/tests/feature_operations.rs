// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::validate::validate_neutral;

#[test]
fn feature_operation_geometry_is_validated() {
    use crate::features::{Feature, FeatureDefinition, FeatureId};

    let definitions = vec![
        FeatureDefinition::Form { cages: Vec::new() },
        FeatureDefinition::Form {
            cages: vec![
                crate::ids::SubdId::mint("synthetic:test:subd#missing").expect("valid identity")
            ],
        },
    ];
    let expected = ["references missing Form control cage `synthetic:test:subd#missing`"];
    let mut ir = unit_cube();
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:feature#invalid-{ordinal}"))
                .expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: crate::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),

            evaluation: crate::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(!findings
        .iter()
        .any(|finding| { finding.entity.as_deref() == Some("synthetic:test:feature#invalid-0") }));
    for message in expected {
        assert!(findings.iter().any(|finding| finding.message == message));
    }
}
