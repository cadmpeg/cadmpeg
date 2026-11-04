// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use std::io::Cursor;

#[test]
fn sweep_history_keeps_branches_and_body_links_without_inventing_policies() {
    let file = crate::test_support::sweep_history::prt();
    let decoded = crate::NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("two body-writing sweep branches");
    let ir = decoded.ir();
    assert_eq!(ir.model.bodies.len(), 1);
    let sweeps = ir
        .model
        .features
        .iter()
        .filter(|f| f.name.as_deref() == Some("SWP104"))
        .collect::<Vec<_>>();
    assert_eq!(sweeps.len(), 2);
    for sweep in sweeps {
        assert!(sweep.evaluation.outputs().is_empty());
        assert!(matches!(sweep.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) if kind.as_str() == "SWP104"));
    }
    let arenas = ir.native.namespace("nx").unwrap().arenas();
    assert_eq!(arenas["feature_operation_body_writes"].len(), 2);
    assert!(arenas
        .get("feature_body_write_group_partition_uses")
        .is_none_or(Vec::is_empty));
    let branches = serde_json::to_value(&arenas["feature_swp104_leading_branches"]).unwrap();
    assert_eq!(branches.as_array().unwrap().len(), 2);
    assert_eq!(branches[0]["parameter_class_index"], 17);
    assert_eq!(branches[0]["profile_class_index"], 18);
    assert_eq!(branches[0]["declared_count"], 4);
    assert_eq!(branches[0]["witnessed_count"], 3);
    assert_eq!(branches[0]["state_lane"], "000100010000");
    assert_eq!(branches[0]["members"].as_array().unwrap().len(), 3);
    assert_eq!(branches[0]["successor"]["object_index"], 4);
    assert_eq!(branches[1]["leading_zero"], true);
    assert!(branches[1]["witnessed_count"].is_null());
    assert_eq!(branches[1]["state_lane"], "0000000000");
    assert_eq!(branches[1]["successor"]["object_index"], 2);
    for branch in branches.as_array().unwrap() {
        assert_eq!(
            branch["raw_scalars"],
            serde_json::json!([
                "2f03ec460ed80a18",
                "2f60624dd2f1a9fc",
                "2f14f8b588e368f1",
                "2f04f8b588e368f1"
            ])
        );
        assert_eq!(
            branch["scalars"],
            serde_json::json!([0.000_038, 0.002, 0.00008, 0.00004])
        );
    }
}
