// SPDX-License-Identifier: Apache-2.0
//! The records each give-up drops in arenas the CADIR wire omits.

use std::collections::BTreeMap;

use crate::codec::tests::{decode_charging, ChargesLabelCodec};
use crate::CadIr;

/// A legacy feature of the arena the CADIR wire omits, with `dependencies`
/// and `native_ref`.
fn legacy_feature(
    id: &str,
    dependencies: &[&str],
    native_ref: Option<&str>,
) -> crate::features::Feature {
    let mint = |text: &str| crate::features::FeatureId::mint(text).unwrap();
    crate::features::Feature {
        product_definition: None,
        id: mint(id),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::try_from(
            dependencies
                .iter()
                .map(|dependency| mint(dependency))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            crate::features::FeatureDefinition::Operation(
                crate::features::FeatureOperation::StoredGeometry {},
            ),
        ),
        native_ref: native_ref.map(str::to_owned),
    }
}

/// A record of an arena the CADIR wire omits that needs a record a decoder
/// gave up is dropped with it, transitively, and the give-up names it;
/// one that only projects it keeps its values and is marked (§2.5, §3.3).
#[test]
fn a_legacy_feature_that_needs_a_given_up_feature_is_dropped_with_it() {
    fn features(ir: &mut CadIr) {
        ir.model.features.extend([
            legacy_feature("test:model:feature#b", &["test:model:feature#a"], None),
            legacy_feature("test:model:feature#c", &["test:model:feature#b"], None),
            legacy_feature("test:model:feature#d", &[], None),
            legacy_feature("test:model:feature#e", &[], Some("test:model:feature#a")),
        ]);
    }
    let result = decode_charging(&ChargesLabelCodec(features, "test:model:feature#a"));
    let kept = result
        .ir()
        .model
        .features
        .iter()
        .map(|feature| (feature.id.as_str(), feature.native_ref.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        kept,
        [
            ("test:model:feature#d", None),
            ("test:model:feature#e", None)
        ]
    );
    let charges = result
        .report()
        .losses
        .iter()
        .filter(|loss| crate::unknown::part::gives_up(&loss.code))
        .collect::<Vec<_>>();
    let [charge] = charges.as_slice() else {
        panic!("one give-up for the one given-up feature: {charges:?}");
    };
    assert!(
        charge.message.ends_with(
            "; dropped=[test:model:feature#b, test:model:feature#c]; \
             marked=[test:model:feature#e]"
        ),
        "{}",
        charge.message
    );
}
