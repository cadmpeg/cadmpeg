use super::{B5Pcurve, B5PcurveParameterization};
use crate::test_support::test_b5::{finite_lane, finite_vector};
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
use cadmpeg_core::CodecError;

fn pcurve(values: &[f64]) -> B5Pcurve {
    B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: u32::try_from(values.len() - 1).expect("fixture degree"),
        distinct_knots: finite_lane(&[0.0, 1.0]),
        multiplicities: vec![u32::try_from(values.len()).expect("fixture count"); 2],
        control_points: values.iter().map(|&value| finite_vector([4.0, value])).collect(),
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    }
}

#[test]
fn isocurve_order_visits_a_long_equal_prefix_once() {
    const COUNT: usize = 65;
    let mut values = [0.0; COUNT];
    values[COUNT - 1] = -1.0;
    let mut curve = pcurve(&values);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("caller context");
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "catia_b5_pcurve_knot_count", Some(1));
    let CodecError::ResourceLimit(original) =
        super::super::super::pcurves::isocurve_endpoint_parameters(&ctx, &curve, [0.0, 1.0])
            .expect_err("evaluation starts after one direction pass")
    else {
        panic!("typed refusal")
    };
    // The constant-U scan visits COUNT poles and its current core end probe.
    // The order scan visits exactly COUNT - 1 adjacent pairs.
    assert_eq!(original.used, (COUNT + 1 + COUNT - 1) as u64);
    assert_eq!(original.operation, "catia_b5_pcurve_knot_count");
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(probe);
    curve.control_points.clear();
    assert!(matches!(
        super::super::super::pcurves::isocurve_endpoint_parameters(&ctx, &curve, [0.0, 1.0]),
        Err(CodecError::ResourceLimit(next)) if next == original
    ));
    assert_eq!(super::isocurve_endpoint_parameters(&pcurve(&values), [0.0, 1.0]), Some([0.0, -1.0]));
}

#[test]
fn isocurve_order_stops_at_the_first_direction_conflict() {
    const COUNT: usize = 65;
    let mut values = [1.0; COUNT];
    values[0] = 0.0;
    values[1] = -1.0;
    let curve = pcurve(&values);
    crate::test_support::with_work_limit((COUNT + 1 + 2) as u64, |ctx| {
        assert!(super::super::super::pcurves::isocurve_endpoint_parameters(ctx, &curve, [0.0, 1.0])
            .expect("only two direction comparisons execute").is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn isocurve_order_preserves_equal_zero_and_invalid_endpoint_semantics() {
    for values in [[-0.0, 0.0, -0.0], [0.0, -0.0, 0.0]] {
        let curve = pcurve(&values);
        assert_eq!(super::isocurve_endpoint_parameters(&curve, [0.0, 1.0]), Some([0.0, 0.0]));
        assert!(super::isocurve_endpoint_parameters(&curve, [f64::NAN, 1.0]).is_none());
        assert!(super::isocurve_endpoint_parameters(&curve, [0.0, f64::NAN]).is_none());
    }
}
