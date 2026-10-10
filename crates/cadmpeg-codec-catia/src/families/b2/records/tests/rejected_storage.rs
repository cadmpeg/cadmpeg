// SPDX-License-Identifier: Apache-2.0
//! Rejected and consumed record-lane storage.

#[test]
fn rejected_reference_lists_allocate_no_retained_entries() {
    let mut bytes = crate::test_support::test_b2::b2_reference_list_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let payload = records[0].payload().expect("reference payload").start;
    bytes[payload] = 0x81;
    bytes[payload + 1] = 0;
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..64 {
            assert!(
                super::super::b2_reference_lists_from_records(ctx, &bytes, &records)
                    .expect("rejected reference scratch")
                    .is_empty()
            );
        }
    });
}

#[test]
fn rational_nurbs_curves_retain_no_consumed_point_or_weight_backing() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::nurbs::WeightedPole3;

    let bytes = super::b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    // The source has four poles and eight knots. Each output vector grows
    // from zero to four slots; the knot lane requests eight slots at once.
    let retained_bytes = 8 * std::mem::size_of::<f64>()
        + 4 * std::mem::size_of::<WeightedPole3<FinitePoint3>>()
        + 4 * std::mem::size_of::<super::super::B2NurbsCurve>();
    let curves = crate::test_support::with_retained_limit(
        u64::try_from(retained_bytes).expect("fixed curve storage fits u64"),
        |ctx| super::super::b2_nurbs_curves(ctx, &bytes),
    )
    .expect("only surviving curve storage is retained");
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].geometry.degree(), 3);
    assert_eq!(
        curves[0].geometry.pole_rows().weights(),
        Some(vec![1.0, 0.72, 1.31, 0.93])
    );
}
