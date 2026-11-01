// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};
use serde_json::Value;

use super::*;

#[test]
fn coil_decode_retains_both_boolean_polarities_and_four_selector_candidates() {
    for selector in 0..4 {
        for first in [false, true] {
            for second in [false, true] {
                let bytes = crate::test_support::coil::document(first, second, selector);
                let (ir, report, _) = InventorCodec
                    .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                    .expect("synthetic Coil decode")
                    .into_parts();
                assert_eq!(ir.model.features.len(), 1);
                assert!(ir.model.feature_result_topologies.is_empty());
                assert!(matches!(ir.model.features[0].evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Native { kind: NativeFeatureKind::Other(kind), parameters })
                    if kind == "Coil" && parameters.is_empty()));
                let losses = report
                    .losses
                    .iter()
                    .filter(|loss| loss.code.local_code() == "feature.coil-operands-native")
                    .collect::<Vec<_>>();
                assert_eq!(losses.len(), 1);
                assert_eq!(losses[0].message, "Retained Inventor Coil natively: slot 3 Boolean construction role and polarity are unresolved.");
                let properties = ir
                    .native
                    .namespace("inventor")
                    .expect("native namespace")
                    .arena_as::<Value>("pm_dc_feature_properties")
                    .expect("native properties");
                assert_eq!(properties.len(), 6);
                assert_eq!(properties[0]["kind"]["value"], first);
                assert_eq!(properties[1]["kind"]["value"], second);
            }
        }
    }
}
