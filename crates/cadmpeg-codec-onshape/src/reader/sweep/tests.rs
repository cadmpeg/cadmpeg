// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn connectivity_budget_refuses_work_before_curve_lookup() {
    let count = 1_001;
    let entities = (0..count)
        .map(|index| json!(format!("curve-{index}")))
        .collect::<Vec<_>>();
    assert_eq!(
        order_path(&entities, &Model::default()).unwrap_err(),
        PATH_BUDGET_EXHAUSTED
    );
}

#[test]
fn clamped_rational_spline_endpoints_use_the_end_poles() {
    let entity = cadmpeg_core::serde_erase::json::from_value(json!({
        "id":"synthetic:model:entity#spline", "sketch":"synthetic:model:sketch#path",
        "construction":false, "endpoint_refs":[],
        "geometry":{"kind":"nurbs", "curve":{
            "degree":1, "knots":[0.0,0.0,1.0,1.0], "periodic":false,
            "poles":{"kind":"rational", "points":[
                {"point":{"u":2.0,"v":3.0},"weight":1.0},
                {"point":{"u":4.0,"v":5.0},"weight":0.5}
            ]}
        }}
    }))
    .unwrap();
    assert_eq!(endpoints(&entity).unwrap(), [[2.0, 3.0], [4.0, 5.0]]);
}
