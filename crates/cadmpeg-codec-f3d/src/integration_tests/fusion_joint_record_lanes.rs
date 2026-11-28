// SPDX-License-Identifier: Apache-2.0
//! A CC0 Fusion save whose joint records store their freedoms in lanes.

use super::decode;

#[test]
fn joint_record_lanes_select_the_joint_kind_and_limits() {
    let result =
        decode(include_bytes!("../../../../corpus/f3d/joint_record_freedom_lanes.f3d").to_vec());
    let wire = cadmpeg_test_support::wire::in_memory_json(result.ir());
    let joints = wire["model"]["assembly_joints"]
        .as_array()
        .expect("assembly joints");
    let kinds = joints
        .iter()
        .map(|joint| joint["operands"]["value"]["kind"].clone())
        .collect::<Vec<_>>();
    let names = kinds
        .iter()
        .map(|kind| kind["kind"].as_str().expect("kind").to_owned())
        .collect::<Vec<_>>();
    // Timeline order: rigid, revolute, slider, cylindrical, pin-slot, planar,
    // ball, limited revolute, limited slider. Pin-slot and planar motions
    // have no paired joint kind and stay fixed.
    assert_eq!(
        names,
        [
            "fixed",
            "revolute",
            "slider",
            "cylindrical",
            "fixed",
            "fixed",
            "ball",
            "revolute",
            "slider"
        ],
        "{kinds:#?}"
    );
    let angular = &kinds[7]["angular_limits"];
    assert!((angular["minimum"].as_f64().unwrap() + 30f64.to_radians()).abs() < 1e-12);
    assert!((angular["maximum"].as_f64().unwrap() - 60f64.to_radians()).abs() < 1e-12);
    let linear = &kinds[8]["linear_limits"];
    assert!((linear["minimum"].as_f64().unwrap() + 5.0).abs() < 1e-12);
    assert!((linear["maximum"].as_f64().unwrap() - 20.0).abs() < 1e-12);
    assert!(kinds[1].get("angular_limits").is_none(), "{:#}", kinds[1]);
}
