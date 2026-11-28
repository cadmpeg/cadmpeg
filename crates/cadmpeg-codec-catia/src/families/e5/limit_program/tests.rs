// SPDX-License-Identifier: Apache-2.0
use super::{CurveLimits, SuffixControl, SurfaceLimits};
use cadmpeg_core::decode::View;

fn curve_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for arc in [2_u16, 9] {
        bytes.extend_from_slice(&arc.to_le_bytes());
    }
    for bits in [
        0x8000_0000_0000_0000_u64,
        0x7ff8_0000_0000_0042,
        2.0_f64.to_bits(),
        f64::INFINITY.to_bits(),
    ] {
        bytes.extend_from_slice(&bits.to_le_bytes());
    }
    bytes.push(7);
    bytes
}

fn surface_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for patch in [3_u16, 5, 7, 11] {
        bytes.extend_from_slice(&patch.to_le_bytes());
    }
    for bits in 1_u64..=8 {
        bytes.extend_from_slice(&bits.to_le_bytes());
    }
    for flag in [13_u16, 17, 19] {
        bytes.extend_from_slice(&flag.to_le_bytes());
    }
    for bits in 21_u64..=28 {
        bytes.extend_from_slice(&bits.to_le_bytes());
    }
    bytes.extend_from_slice(&[31, 37, 41, 43, 47, 53]);
    bytes
}

#[test]
fn curve_program_keeps_limit_words_and_unknown_controls() {
    let bytes = curve_bytes();
    let mut view = View::over_retained(&bytes);
    let value = CurveLimits::read(&mut view).unwrap();
    assert!(view.is_empty());
    assert_eq!(value.arcs(), [2, 9]);
    assert_eq!(value.lower_local_bits, 0x8000_0000_0000_0000);
    assert_eq!(value.upper_local_bits, 0x7ff8_0000_0000_0042);
    assert_eq!(value.coefficient_bits, 2.0_f64.to_bits());
    assert_eq!(value.shift_bits, f64::INFINITY.to_bits());
    assert_eq!(value.extrapolation_control, 7);
    assert!(!value.is_disabled_identity());
}

#[test]
fn surface_program_keeps_every_independent_field() {
    let bytes = surface_bytes();
    let mut view = View::over_retained(&bytes);
    let value = SurfaceLimits::read(&mut view).unwrap();
    assert!(view.is_empty());
    assert_eq!(value.patches, [3, 5, 7, 11]);
    assert_eq!(value.local_bits, [1, 2, 3, 4]);
    assert_eq!(value.affine_bits, [5, 6, 7, 8]);
    assert_eq!(value.extrapolation_flags, [13, 17, 19]);
    assert_eq!(value.continuation_bits, [21, 22, 23, 24, 25, 26, 27, 28]);
    assert_eq!(
        value.suffix_controls.map(|control| control.0),
        [31, 37, 41, 43, 47, 53]
    );
    assert!(!value.is_disabled_identity());
}

#[test]
fn disabled_geometry_guards_admit_signed_zero_without_normalizing() {
    let minus_zero = (-0.0_f64).to_bits();
    let mut curve = CurveLimits {
        lower_arc: 1,
        upper_arc: 1,
        lower_local_bits: minus_zero,
        upper_local_bits: 1.0_f64.to_bits(),
        coefficient_bits: 1.0_f64.to_bits(),
        shift_bits: minus_zero,
        extrapolation_control: 0,
    };
    assert!(curve.is_disabled_identity());
    assert_eq!(curve.locals()[0].to_bits(), minus_zero);
    assert_eq!(curve.shift_bits, minus_zero);
    curve.extrapolation_control = 1;
    assert!(!curve.is_disabled_identity());
    curve.extrapolation_control = 0;
    curve.coefficient_bits = 2.0_f64.to_bits();
    assert!(!curve.is_disabled_identity());
    let mut surface = SurfaceLimits {
        patches: [1; 4],
        local_bits: [minus_zero; 4],
        affine_bits: [1.0_f64.to_bits(), minus_zero, 1.0_f64.to_bits(), minus_zero],
        extrapolation_flags: [0; 3],
        continuation_bits: [minus_zero; 8],
        suffix_controls: [SuffixControl(0); 6],
    };
    assert!(surface.is_disabled_identity());
    assert_eq!(surface.locals().map(f64::to_bits), [minus_zero; 4]);
    assert_eq!(surface.continuation_bits, [minus_zero; 8]);
    for index in 0..3 {
        surface.extrapolation_flags[index] = 1;
        assert!(!surface.is_disabled_identity());
        surface.extrapolation_flags[index] = 0;
    }
    for index in 0..8 {
        surface.continuation_bits[index] = f64::NAN.to_bits();
        assert!(!surface.is_disabled_identity());
        surface.continuation_bits[index] = minus_zero;
    }
    for index in 0..6 {
        surface.suffix_controls[index] = SuffixControl(1);
        assert!(!surface.is_disabled_identity());
        surface.suffix_controls[index] = SuffixControl(0);
    }
    surface.affine_bits[2] = 2.0_f64.to_bits();
    assert!(!surface.is_disabled_identity());
}

#[test]
fn program_readers_require_exact_trailers() {
    let mut curve = curve_bytes();
    let mut surface = surface_bytes();
    assert_eq!(curve.len(), CurveLimits::LEN);
    assert_eq!(surface.len(), SurfaceLimits::LEN);
    for length in 0..curve.len() {
        assert!(CurveLimits::read(&mut View::over_retained(&curve[..length])).is_none());
    }
    for length in 0..surface.len() {
        assert!(SurfaceLimits::read(&mut View::over_retained(&surface[..length])).is_none());
    }
    curve.push(0);
    surface.push(0);
    assert!(CurveLimits::read(&mut View::over_retained(&curve)).is_none());
    assert!(SurfaceLimits::read(&mut View::over_retained(&surface)).is_none());
}
