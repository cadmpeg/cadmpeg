// SPDX-License-Identifier: Apache-2.0
//! Correspondence framing, arithmetic and resource admission.

use super::{parse, E5CurveSupport, E5CurveSupportKind, E5ParameterProgram};
use cadmpeg_ir::scalar::FiniteReal;

fn payload(mode: u8, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x82, 0x81, 0x82, 0x81, mode, 0];
    for value in [2.0_f64, 8.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(tail);
    bytes
}

fn affine() -> Vec<u8> {
    let mut tail = Vec::new();
    for value in [0.125_f64, 0.0, 3.0, -2.0, 8.0, 2.0, 8.0, 0.125] {
        tail.extend_from_slice(&value.to_le_bytes());
    }
    tail.extend_from_slice(&0_u32.to_le_bytes());
    payload(3, &tail)
}

fn sampled() -> Vec<u8> {
    let mut tail = Vec::new();
    for value in [0.125_f64, 0.0, 3.0] {
        tail.extend_from_slice(&value.to_le_bytes());
    }
    tail.push(0x84);
    for value in [8.0_f64, 6.0, 3.0, 2.0, 0.0, 1.0, 2.0, 3.0, 2.0, 8.0, 0.125] {
        tail.extend_from_slice(&value.to_le_bytes());
    }
    tail.extend_from_slice(&0_u32.to_le_bytes());
    payload(5, &tail)
}

fn admit(bytes: &[u8]) -> Option<E5CurveSupport> {
    crate::test_support::with_service_context(|ctx| parse(ctx, 0xc1, bytes))
        .expect("service budget")
}

#[test]
fn affine_maps_retain_negative_scale_and_independent_fields() {
    let support = admit(&affine()).expect("negative affine map");
    let E5CurveSupportKind::Intersection {
        curves,
        correspondence,
    } = &support.kind
    else {
        panic!("intersection production");
    };
    assert_eq!(*curves, [1, 2]);
    assert_eq!(correspondence.map_accuracy.get(), 0.125);
    assert_eq!(correspondence.fit_tolerance.get(), 0.125);
    assert_eq!(correspondence.co_domain.map(FiniteReal::get), [0.0, 3.0]);
    assert!(
        matches!(correspondence.program, E5ParameterProgram::Affine { scale, offset }
        if scale.get() == -2.0 && offset.get() == 8.0)
    );
    let wire = serde_json::to_value(&support).expect("store fields");
    assert_eq!(
        serde_json::from_value::<E5CurveSupport>(wire.clone()).expect("load"),
        support
    );
    let mut conflicting = wire;
    conflicting["kind"]["correspondence"]["fit_tolerance"] = serde_json::json!(0.25);
    assert!(serde_json::from_value::<E5CurveSupport>(conflicting).is_err());
}

#[test]
fn sampled_lanes_are_separate_and_need_no_affine_interpolation() {
    let support = admit(&sampled()).expect("non-affine sites");
    let E5CurveSupportKind::Intersection { correspondence, .. } = support.kind else {
        panic!("intersection production");
    };
    let E5ParameterProgram::Sampled {
        reference_parameters,
        co_parameters,
    } = correspondence.program
    else {
        panic!("sampled production");
    };
    assert_eq!(
        reference_parameters
            .iter()
            .map(|v| v.get())
            .collect::<Vec<_>>(),
        [8.0, 6.0, 3.0, 2.0]
    );
    assert_eq!(
        co_parameters.iter().map(|v| v.get()).collect::<Vec<_>>(),
        [0.0, 1.0, 2.0, 3.0]
    );
}

#[test]
fn incomplete_or_inconsistent_programs_are_rejected() {
    for bytes in [affine(), sampled()] {
        for length in 0..bytes.len() {
            assert!(admit(&bytes[..length]).is_none(), "length {length}");
        }
        let mut excess = bytes.clone();
        excess.push(0);
        assert!(admit(&excess).is_none());
        let mut terminal = bytes.clone();
        *terminal.last_mut().expect("terminal") = 1;
        assert!(admit(&terminal).is_none());
        let mut unknown = bytes;
        unknown[4] = 7;
        assert!(admit(&unknown).is_none());
    }
    let mut bytes = sampled();
    // Repeated reference site violates strict monotonicity.
    bytes[22 + 25 + 8..22 + 25 + 16].copy_from_slice(&8.0_f64.to_le_bytes());
    assert!(admit(&bytes).is_none());
    let mut bytes = affine();
    bytes[22 + 24..22 + 32].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(admit(&bytes).is_none());
}

#[test]
fn sampled_arrays_refuse_before_collection_growth() {
    let result = crate::test_support::with_collection_limit(0, |ctx| parse(ctx, 0xc1, &sampled()));
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_map_samples")
    );
}
