// SPDX-License-Identifier: Apache-2.0
//! The exactness of a model feature names the fields of the design feature
//! that restates it (§10).

use super::projection::stored_thin;
use crate::reader::id;

/// A thin extrusion's design feature restates the model feature with its id:
/// the derived `definition` and `dependencies` restate as the design
/// `definition`, and `outputs`, which no design field states, leaves.
#[test]
fn stored_thin_design_feature_takes_the_exactness_of_its_model_feature() {
    let result = crate::test_support::review::decode(&stored_thin(true, false, false));
    let plate = id("feature", "Plate");
    assert!(result
        .ir()
        .design
        .features
        .iter()
        .any(|feature| feature.id.as_str() == plate));
    let fields = result.source_fidelity().annotations.exactness()[plate.as_str()].fields();
    assert_eq!(
        fields.get("definition"),
        Some(&cadmpeg_ir::Exactness::Derived)
    );
    for field in fields.keys() {
        assert!(
            [
                "name",
                "suppressed",
                "definition",
                "placement",
                "component",
                "driven_by",
                "native_ref"
            ]
            .contains(&field.as_str()),
            "`{field}` names no design feature field"
        );
    }
}
