// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::test_fixtures::{content_at, parse as with_view};

fn word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn real(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn empty_list(bytes: &mut Vec<u8>) {
    word(bytes, LIST_MARKER);
    word(bytes, 0);
}

fn point_list(bytes: &mut Vec<u8>, references: &[u32]) {
    let count = u32::try_from(references.len()).expect("fixture count");
    word(bytes, LIST_MARKER);
    word(bytes, count);
    word(bytes, count);
    word(bytes, 0);
    for reference in references {
        word(bytes, 0x8000_0000 | reference);
    }
}

/// How a fixture stores its auxiliary lists.
#[derive(Clone, Copy)]
pub(crate) enum Auxiliary {
    /// One empty type-2 list.
    List,
    /// Gate `0, 0`.
    None,
    /// Gate `1, 0` with an empty list and a two-member list.
    TwoLists,
}

pub(crate) fn prefix(first: u32, second: u32, auxiliary: Auxiliary) -> Vec<u8> {
    let mut bytes = content_at(8, 23, &[-1]);
    word(&mut bytes, 0x4_0000);
    word(&mut bytes, 3);
    point_list(&mut bytes, &[first, second]);
    match auxiliary {
        Auxiliary::List => empty_list(&mut bytes),
        Auxiliary::None => {
            word(&mut bytes, 0);
            word(&mut bytes, 0);
        }
        Auxiliary::TwoLists => {
            word(&mut bytes, 1);
            word(&mut bytes, 0);
            empty_list(&mut bytes);
            point_list(&mut bytes, &[first, second]);
        }
    }
    bytes
}

/// A kind-5 record.
pub(crate) fn reference_fixture(first: u32, second: u32, auxiliary: Auxiliary) -> Vec<u8> {
    let mut bytes = prefix(first, second, auxiliary);
    for value in [5, 0, 77] {
        word(&mut bytes, value);
    }
    bytes
}

/// A kind-4 record of degree 2 with three poles. A rational curve has
/// `weights` of the pole count; a polynomial curve has none.
/// Stored values that the record form does not fix.
struct Unfixed {
    state: u32,
    form: u32,
    pole_value: u32,
    curve_values: [u32; 2],
    knots: [f64; 6],
}

const USUAL: Unfixed = Unfixed {
    state: 0,
    form: 0,
    pole_value: 8,
    curve_values: [1, 1],
    knots: [0., 0., 0., 1., 1., 1.],
};

pub(crate) fn curve_fixture(first: u32, second: u32, weights: &[f64]) -> Vec<u8> {
    curve_fixture_with(first, second, weights, &USUAL)
}

fn curve_fixture_with(first: u32, second: u32, weights: &[f64], unfixed: &Unfixed) -> Vec<u8> {
    let mut bytes = prefix(first, second, Auxiliary::List);
    for value in [4, unfixed.state, unfixed.form, 2] {
        word(&mut bytes, value);
    }
    real(&mut bytes, 1e-9);
    for value in [6, 6, 8] {
        word(&mut bytes, value);
    }
    for value in unfixed.knots {
        real(&mut bytes, value);
    }
    let weight_count = u32::try_from(weights.len()).expect("fixture count");
    for value in [weight_count, weight_count, 8] {
        word(&mut bytes, value);
    }
    for value in weights {
        real(&mut bytes, *value);
    }
    for value in [3, unfixed.pole_value, 8] {
        word(&mut bytes, value);
    }
    for pole in [[0., 0.], [1., 1.], [2., 0.]] {
        for value in pole {
            real(&mut bytes, value);
        }
    }
    real(&mut bytes, 1e-12);
    for value in unfixed.curve_values {
        word(&mut bytes, value);
    }
    for value in [0., 1., 0.] {
        real(&mut bytes, value);
    }
    word(&mut bytes, 8);
    bytes
}

pub(crate) fn record(bytes: &[u8]) -> Result<PmDcSketchEntityPayload, CodecError> {
    with_view(bytes, |ctx, source| parse(ctx, source, 23))
}

fn data(bytes: &[u8]) -> MemberCurveRecord {
    let PmDcSketchEntityKind::MemberCurve { data } = record(bytes).expect("member curve").kind
    else {
        panic!("member curve");
    };
    *data
}

#[test]
fn a_reference_body_keeps_its_value_under_every_auxiliary_form() {
    for (auxiliary, lists) in [
        (Auxiliary::List, 1),
        (Auxiliary::None, 0),
        (Auxiliary::TwoLists, 2),
    ] {
        let data = data(&reference_fixture(4, 5, auxiliary));
        assert_eq!(data.auxiliary.len(), lists);
        assert_eq!(data.body, MemberCurveBody::Reference { value: 77 });
        assert!(data.native_reason().contains("reference value"));
    }
}

#[test]
fn polynomial_and_rational_curve_bodies_keep_every_lane() {
    let polynomial = data(&curve_fixture(4, 5, &[]));
    let rational = data(&curve_fixture(4, 5, &[1., 2., 1.]));
    for (data, weights) in [(polynomial, 0), (rational, 3)] {
        let MemberCurveBody::Curve(curve) = &data.body else {
            panic!("curve body");
        };
        assert_eq!(curve.degree, 2);
        assert_eq!(curve.knots.len(), 6);
        assert_eq!(curve.weights.len(), weights);
        assert_eq!(curve.poles[2][0].get(), 2.0);
        assert_eq!(curve.interval[1].get(), 1.0);
        assert!(data.native_reason().contains("exact curve rule"));
        let wire = serde_json::to_value(&data).expect("serialize member curve");
        assert_eq!(
            serde_json::from_value::<MemberCurveRecord>(wire).expect("deserialize"),
            data
        );
    }
}

#[test]
fn every_truncated_prefix_and_trailing_byte_is_refused() {
    for mut bytes in [
        curve_fixture(4, 5, &[1., 2., 1.]),
        reference_fixture(4, 5, Auxiliary::TwoLists),
    ] {
        for end in 0..bytes.len() {
            assert!(record(&bytes[..end]).is_err(), "prefix {end}");
        }
        bytes.push(0);
        assert!(record(&bytes).is_err());
    }
}

#[test]
fn an_unrecognised_shape_is_skipped_not_admitted() {
    let good = curve_fixture(4, 5, &[1., 2., 1.]);
    let find = |header: [u32; 3]| {
        let header: Vec<u8> = header.iter().flat_map(|v| v.to_le_bytes()).collect();
        good.windows(header.len())
            .position(|window| window == header.as_slice())
            .expect("lane header")
    };
    // A weight lane with another scalar width.
    let at = find([3, 3, 8]);
    let mut width = good.clone();
    width[at + 8..at + 12].copy_from_slice(&4u32.to_le_bytes());
    assert!(matches!(record(&width), Err(CodecError::NotImplemented(_))));
    // A weight count that is neither zero nor the pole count.
    let mut count = good.clone();
    count[at..at + 4].copy_from_slice(&2u32.to_le_bytes());
    assert!(record(&count).is_err());
    let mut count = good.clone();
    count[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
    count[at + 4..at + 8].copy_from_slice(&1u32.to_le_bytes());
    assert!(record(&count).is_err());
    // A body kind other than 4 and 5.
    let at = find([4, 0, 0]);
    let mut kind = good;
    kind[at..at + 4].copy_from_slice(&6u32.to_le_bytes());
    assert!(matches!(record(&kind), Err(CodecError::NotImplemented(_))));
    // A list gate other than 0, 0 and 1, 0.
    let mut gate = reference_fixture(4, 5, Auxiliary::None);
    let at = gate.len() - 20;
    gate[at..at + 4].copy_from_slice(&2u32.to_le_bytes());
    assert!(matches!(record(&gate), Err(CodecError::NotImplemented(_))));
}

#[test]
fn values_the_form_does_not_fix_are_kept_as_stored() {
    let unusual = Unfixed {
        state: 1,
        form: 1,
        pole_value: 9,
        curve_values: [2, 3],
        knots: [0., 1., 0.5, 1., 1., 1.],
    };
    let data = data(&curve_fixture_with(4, 5, &[], &unusual));
    let MemberCurveBody::Curve(curve) = &data.body else {
        panic!("curve body");
    };
    assert_eq!(curve.form, 1);
    assert_eq!(curve.knots[2].get(), 0.5);
    assert!(data.native_reason().contains("exact curve rule"));
}
