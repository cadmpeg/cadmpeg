// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use crate::loss::InventorLossCode;
use crate::InventorCodec;

#[test]
fn surface_extrusion_keeps_its_native_definition_and_pinned_loss() {
    let bytes = crate::test_support::surface_extrusion::fixture();
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("synthetic surface extrusion");
    let (ir, report, _) = decoded.into_parts();
    assert_eq!(ir.model.features.len(), 1);
    let feature = &ir.model.features[0];
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
            if kind.as_str() == "ExtrudeSurface"
    ));
    assert!(feature.native_ref.is_some());
    assert!(ir.model.feature_result_topologies.is_empty());
    let code = InventorLossCode::FeatureOperationGraphOpen
        .note("expected")
        .code;
    assert_eq!(code.local_code(), "feature.operation-graph-open");
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| loss.code == code)
            .count(),
        1
    );
}
