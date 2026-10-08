use super::{a8_class21_test_payload, parse_a8_class21_pcurve};

#[test]
fn a8_class21_rejected_tail_does_not_retain_output_lanes() {
    let mut payload = a8_class21_test_payload();
    let offset = payload.len() - 36 + 10;
    payload[offset..offset + 8].copy_from_slice(&(-1.0_f64).to_le_bytes());
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..32 {
            assert!(parse_a8_class21_pcurve(ctx, 7, &payload)?.is_none());
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("late rejected outputs retain no bytes");
}

#[test]
fn a8_class21_jet_projections_refuse_before_traversal() {
    let payload = a8_class21_test_payload();
    for operation in [
        "catia_b5_pcurve_point_jet_pair_scan",
        "catia_b5_pcurve_first_jet_pair_scan",
        "catia_b5_pcurve_second_jet_pair_scan",
    ] {
        let refusal = crate::test_support::with_work_refusal(operation, |ctx| {
            parse_a8_class21_pcurve(ctx, 7, &payload)
        });
        assert!(
            matches!(refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation)
        );
    }
}
