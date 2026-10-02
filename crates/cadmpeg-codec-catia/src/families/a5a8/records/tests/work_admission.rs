use super::super::{a5_knots, a5_nurbs_curves};
use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

fn work_refusals<T>(run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>)
    -> std::collections::HashSet<&'static str> {
    let mut operations = std::collections::HashSet::new();
    let mut cap = 0;
    for _ in 0..1024 {
        match crate::test_support::with_work_limit(cap, &run) {
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits => {
                operations.insert(limit.operation);
                cap = limit.used.checked_add(limit.additional).expect("finite fixture work");
            }
            Ok(_) => return operations,
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    panic!("fixture did not finish its admitted work");
}

#[test]
fn a5_knots_refuse_multiplicity_scan_and_expansion_work() {
    let operations = work_refusals(|ctx| a5_knots(ctx, &[0.0, 1.0], 1));
    for operation in ["catia_a5_multiplicity_emit", "catia_a5_knot_expansion_scan", "catia_a5_knot_expansion_emit"] {
        assert!(operations.contains(operation), "missing work refusal for {operation}");
    }
    assert_eq!(crate::test_support::with_service_context(|ctx| a5_knots(ctx, &[0.0, 1.0], 1))
        .expect("service work"), Some((vec![0.0, 0.0, 1.0, 1.0], 2)));
}

#[test]
fn a5_nurbs_knots_refuse_scan_and_expansion_work() {
    let bytes = super::curve_and_guide_records::a5_nurbs_curve_stream();
    let operations = work_refusals(|ctx| a5_nurbs_curves(ctx, &bytes));
    for operation in ["catia_a5_nurbs_knot_expansion_scan", "catia_a5_nurbs_knot_expansion_emit"] {
        assert!(operations.contains(operation), "missing work refusal for {operation}");
    }
    assert_eq!(crate::test_support::with_service_context(|ctx| a5_nurbs_curves(ctx, &bytes))
        .expect("service work").len(), 1);
}
