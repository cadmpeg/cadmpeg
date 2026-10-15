// SPDX-License-Identifier: Apache-2.0
//! Resource admission for zero-entity native records.

#[test]
fn native_zero_entity_records_result() {
    let bytes = [0xa9, 0x03, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
    let run = || super::super::zero_entity_records(&bytes, 0..bytes.len());

    assert_eq!(run().len(), 1);
}

#[test]
fn native_zero_entity_pair_ids_result() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite point");
    let pair = crate::families::zero_entity::topology::ZeroEntityEndpointPairCandidate {
        face_record_ordinals: [1, 2],
        support_record_ordinals: [3, 4],
        model_endpoints: [point, point],
        model_midpoint: point,
    };
    let admitted = super::super::zero_entity_endpoint_pair_candidates(vec![pair]);
    assert_eq!(
        admitted[0].id,
        "catia:zero-entity:endpoint-pair-candidate#0"
    );
    assert_eq!(admitted[0].face_records[0], "catia:zero-entity:record#1");
}
