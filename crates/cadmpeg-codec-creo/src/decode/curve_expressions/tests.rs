// SPDX-License-Identifier: Apache-2.0

#[test]
fn curve_expression_helix_rejects_nonfinite_local_origin() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=0-t*360\0z=-2+10*t\0";
    let mut record = crate::curve::expression_records(payload)
        .pop()
        .expect("complete curve expression");
    record
        .local_system
        .as_mut()
        .expect("local system")
        .explicit_slots
        .as_mut()
        .expect("explicit slots")[9] = f64::NAN;

    assert!(crate::curve::expression_helix(&record).is_some());
    assert!(super::curve_expression_helix_definition(&record).is_none());
}
