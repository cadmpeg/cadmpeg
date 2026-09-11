// SPDX-License-Identifier: Apache-2.0
//! The loft path carries its endpoints inside the path curve object, so
//! endpoints without a curve have no wire form.

use crate::geometry::{LoftPath, LoftPathCurve};
use crate::ids::CurveId;

fn curve_id() -> CurveId {
    CurveId::mint("test:model:curve#path").expect("valid identity")
}

#[test]
fn loft_path_carries_endpoints_inside_its_curve_object() {
    let path = LoftPath {
        path: Some(LoftPathCurve {
            id: curve_id(),
            endpoints: Some([Some(0.0), Some(1.0)]),
        }),
        auxiliaries: Vec::new(),
        flag: 4,
    };
    let wire = serde_json::to_value(&path).expect("serializes");
    assert_eq!(wire["path"]["curve"], "test:model:curve#path");
    assert_eq!(wire["path"]["endpoints"], serde_json::json!([0.0, 1.0]));
    assert_eq!(
        serde_json::from_value::<LoftPath>(wire).expect("round trip"),
        path
    );

    let flat = serde_json::json!({
        "endpoints": [0.0, 1.0],
        "auxiliaries": [],
        "flag": 4,
    });
    let error = serde_json::from_value::<LoftPath>(flat)
        .err()
        .expect("a flat endpoints key has no loft path field")
        .to_string();
    assert!(error.contains("endpoints"), "{error}");

    let orphan = serde_json::json!({
        "path": {"endpoints": [0.0, 1.0]},
        "auxiliaries": [],
        "flag": 4,
    });
    let error = serde_json::from_value::<LoftPath>(orphan)
        .err()
        .expect("endpoints without a curve are unrepresentable")
        .to_string();
    assert!(error.contains("curve"), "{error}");
}
