// SPDX-License-Identifier: Apache-2.0
//! Nested solid snapshots and sheet profiles use the same kernel header.

use super::{assert_valid, decode};
use crate::loss::SldprtLossCode;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::{
    owned_triangle_with_kind, parasolid_with_body, triangle_body,
};

#[test]
fn nested_solid_transmit_has_feature_input_solid_body_retained_loss() {
    let description = ": TRANSMIT FILE created by modeller version 2400000";
    let schema = "SCH_2400000_20000_13006";
    let solid = owned_triangle_with_kind(0, 700, 0.0, 1);
    let sheet = owned_triangle_with_kind(0, 700, 0.0, 3);
    let mut payload = parasolid_with_body("profile", "SCH_SW_33103_11000", &triangle_body());
    payload.extend(parasolid_with_body(description, schema, &solid));
    payload.extend(parasolid_with_body(description, schema, &sheet));
    for active_body in [solid.as_slice(), &[]] {
        let baseline = decode(sldprt_with_body(active_body));
        let mut bytes = sldprt_with_body(active_body);
        bytes.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &payload,
        ));
        let result = decode(bytes);
        let model = &result.ir().model;
        assert_eq!(model.bodies, baseline.ir().model.bodies);
        assert_eq!(model.faces, baseline.ir().model.faces);
        assert_eq!(model.sketches.len(), 2);
        assert_eq!(model.sketch_entities.len(), 6);
        assert!(model
            .sketches
            .iter()
            .any(|sketch| sketch.name.as_deref() == Some("profile")));
        assert!(model
            .sketches
            .iter()
            .any(|sketch| sketch.name.as_deref() == Some(description)));
        let retained = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == SldprtLossCode::FeatureInputSolidBodyRetained.kind())
            .count();
        assert_eq!(retained, 1);
        assert_eq!(
            SldprtLossCode::FeatureInputSolidBodyRetained
                .kind()
                .local_code(),
            "feature.input-solid-body-retained"
        );
        let native = sldprt_native(result.ir());
        assert_eq!(native.feature_input_lanes.len(), 1);
        assert_eq!(
            native.feature_input_lanes[0].native_payload.as_slice(),
            payload
        );
        assert_valid(&result);
    }
}

#[test]
fn stock_body_partition_retains_inputs_and_current_partition_membership() {
    let description = ": TRANSMIT FILE (partition) created by modeller version 2400000";
    let schema = "SCH_2400000_20000_13006";
    let current = owned_triangle_with_kind(0, 700, 0.0, 1);
    let stock = owned_triangle_with_kind(0, 900, 10.0, 1);
    let payload = parasolid_with_body(description, schema, &stock);
    for current_body in [current.as_slice(), &[]] {
        let baseline = decode(sldprt_with_body(current_body));
        let mut bytes = sldprt_with_body(current_body);
        bytes.extend(make_block(0x20, "Contents/Config-0-StockBodies", &payload));
        let result = decode(bytes.clone());
        assert_eq!(result.ir().model.bodies, baseline.ir().model.bodies);
        assert_eq!(result.ir().model.faces, baseline.ir().model.faces);
        assert_eq!(
            result.ir().model.configurations,
            baseline.ir().model.configurations
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == SldprtLossCode::FeatureInputSolidBodyRetained.kind()));
        let retained = result
            .source_fidelity()
            .retained_record(crate::SOURCE_IMAGE_ID)
            .unwrap();
        assert_eq!(retained.data().unwrap(), bytes);
        assert_valid(&result);
    }
    let mut bytes = sldprt_with_body(&current);
    bytes.extend(make_block(0x20, "Contents/Config-1-Partition", &payload));
    let result = decode(bytes);
    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 2);
    assert_valid(&result);
}
