// SPDX-License-Identifier: Apache-2.0

#[test]
fn neighboring_cylinder_does_not_establish_subject_carrier() {
    let (scan, ir) = super::rowless_round_identity_input();
    let before = ir.model.surfaces.clone();
    let mut losses = Vec::new();
    super::super::report_unresolved_round_carriers(&scan, &ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert_eq!(ir.model.surfaces, before);
}

#[test]
fn neighboring_cylinder_does_not_establish_subject_face_sense() {
    let (mut scan, mut ir) = super::rowless_round_identity_input();
    scan.surfaces.rows[2].reversed = true;
    ir.model.surfaces.push(super::model_cylinder(12, 7.0));
    let orientations = crate::decode::analytic::carriers::native_face_orientations(&scan);
    assert_eq!(orientations.get(&13), Some(&true));
    assert!(!orientations.contains_key(&12));
}

#[test]
fn independent_subject_carrier_survives_neighboring_cylinder() {
    let (scan, mut ir) = super::rowless_round_identity_input();
    ir.model.surfaces.push(super::model_cylinder(12, 7.0));
    let before = ir.model.surfaces.clone();
    let mut losses = Vec::new();
    super::super::report_unresolved_round_carriers(&scan, &ir, &mut losses);
    assert!(losses.is_empty());
    assert_eq!(ir.model.surfaces, before);
}
