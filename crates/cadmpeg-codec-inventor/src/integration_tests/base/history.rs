// SPDX-License-Identifier: Apache-2.0
//! Base results that the final model no longer holds, over a real chain
//! (spec section 13.4).
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::Severity;

use crate::test_support::base_envelope::{fixtures, RESULT_KEY, SOURCE_INDEX};
use crate::test_support::history_carrier::introducing_carrier_bytes;
use crate::test_support::test_fixtures::base_envelope_fixture;
use crate::InventorCodec;
/// Decodes the Base fixture over a closed synthetic chain whose current
/// model holds no body: the state that first lists the source index holds
/// the sphere solid with `key`, and the state before it holds none.
fn decode_over_chain(key: i64) -> CadIr {
    let (design, base) = fixtures();
    let carrier = introducing_carrier_bytes(RefWidth::Eight, 4, i64::from(SOURCE_INDEX), key);
    let bytes = base_envelope_fixture(&carrier, &design, &base);
    InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthetic Base decode")
        .into_parts()
        .0
}

#[test]
fn a_closed_chain_types_a_base_whose_final_model_holds_no_body() {
    let ir = decode_over_chain(RESULT_KEY);
    assert!(ir.geometry.bodies.is_empty());
    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(ir.model.feature_result_topologies.len(), 2);
    for feature in &ir.model.features {
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        ));
        assert!(feature.source_properties.contains_key("result_body_link"));
    }
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let findings = InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("native validation");
    assert!(
        !findings
            .iter()
            .any(|finding| finding.severity >= Severity::Error),
        "{findings:?}"
    );
}

#[test]
fn a_closed_chain_that_introduces_another_key_leaves_the_base_native() {
    let ir = decode_over_chain(RESULT_KEY + 1);
    assert!(ir.model.feature_result_topologies.is_empty());
    assert!(ir.model.features.iter().all(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    )));
}
