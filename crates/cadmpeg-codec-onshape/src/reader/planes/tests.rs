// SPDX-License-Identifier: Apache-2.0
use super::sketch_plane;
use serde_json::json;

#[test]
fn sketch_matrix_retains_translation_and_rotation() {
    let row = json!({"sketchMatrix":[0.0,-1.0,0.0,0.002, 1.0,0.0,0.0,0.003,
        0.0,0.0,1.0,-0.007, 0.0,0.0,0.0,1.0]});
    let plane = sketch_plane(&row).unwrap();
    assert_eq!(
        plane,
        json!({"origin":{"x":0.002,"y":0.003,"z":-0.007},
        "normal":{"x":0.0,"y":0.0,"z":1.0},"x":{"x":0.0,"y":1.0,"z":0.0}})
    );
}

#[test]
fn sketch_matrix_requires_a_rigid_homogeneous_transform() {
    for matrix in [
        json!([1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
        json!([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0]),
        json!([1.0, 0.0, 0.0]),
    ] {
        assert!(sketch_plane(&json!({"sketchMatrix":matrix})).is_err());
    }
}

#[test]
fn plane_losses_name_unsupported_forms_and_keep_unresolved_records_in_history() {
    use super::loss_code;
    use crate::loss::OnshapeLossCode::{HistoryNotSupported, PlaneNotSupported};
    let feature = json!({"featureType":"cPlane","parameters":[
        {"parameterId":"cplaneType","value":"OFFSET"}]});
    for reason in [
        "multiple sketch planes are not supported yet",
        "a sketch on more than one face is not supported yet",
        super::FACE_ORIENTATION_UNPROVEN,
    ] {
        assert!(PlaneNotSupported.matches(&loss_code(&feature, reason).note("plane")));
    }
    for reason in [
        "the selected datum plane is unresolved",
        "quantity expression must be a string",
    ] {
        assert!(HistoryNotSupported.matches(&loss_code(&feature, reason).note("history")));
    }
    let flipped = json!({"featureType":"cPlane","parameters":[
        {"parameterId":"cplaneType","value":"OFFSET"},
        {"parameterId":"oppositeDirection","value":true}]});
    assert!(PlaneNotSupported.matches(&loss_code(&flipped, "any").note("plane")));
}
