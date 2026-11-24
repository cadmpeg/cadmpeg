// SPDX-License-Identifier: Apache-2.0
use super::frame;
use serde_json::json;

#[test]
fn deterministic_face_precedes_top_feature_id() {
    let stored = json!({"origin":{"x":0.0,"y":0.0,"z":0.01},
        "normal":{"x":0.0,"y":0.0,"z":1.0},"x":{"x":0.0,"y":1.0,"z":0.0}});
    let feature = json!({"plane":stored,"parameters":[{"parameterId":"sketchPlane",
        "queries":[{"featureId":"Top","deterministicIds":["support-face"]}]}]});
    let support = json!({"origin":{"x":0.0,"y":0.0,"z":10.0},
        "normal":{"x":0.0,"y":0.0,"z":1.0}});
    let result = frame(
        &feature,
        &|native| {
            assert_eq!(native, "support-face");
            Ok(support.clone())
        },
        &|_| Err("unresolved datum".into()),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"origin":{"x":0.0,"y":0.0,"z":10.0},
        "normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":0.0,"y":1.0,"z":0.0}})
    );
}
