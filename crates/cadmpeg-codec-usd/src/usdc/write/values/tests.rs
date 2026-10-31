// SPDX-License-Identifier: Apache-2.0

use super::half_bits;

#[test]
fn half_bits_round_to_nearest_even() {
    assert_eq!(half_bits(0.0), 0x0000);
    assert_eq!(half_bits(-0.0), 0x8000);
    assert_eq!(half_bits(1.0), 0x3C00);
    assert_eq!(half_bits(-2.0), 0xC000);
    assert_eq!(half_bits(65504.0), 0x7BFF);
    assert_eq!(half_bits(1e6), 0x7C00);
    assert_eq!(half_bits(f64::from(2.0_f32.powi(-24))), 0x0001);
    assert_eq!(half_bits(1.0 + f64::from(2.0_f32.powi(-11))), 0x3C00);
    assert_eq!(half_bits(1.0 + 3.0 * f64::from(2.0_f32.powi(-11))), 0x3C02);
}

#[test]
fn a_real_array_takes_the_narrowest_compressed_form() {
    use super::{compressed, Lane};
    use crate::usdc::value::Component;
    let code = |values: Vec<f64>, component| {
        compressed(&Lane::Reals(values), component)
            .expect("compress")
            .map(|bytes| bytes[8])
    };
    let integral: Vec<f64> = (0..20).map(f64::from).collect();
    assert_eq!(code(integral.clone(), Component::F32), Some(b'i'));
    let mut signed_zero = integral;
    signed_zero[3] = -0.0;
    assert_eq!(code(signed_zero, Component::F64), None);
    let repeated: Vec<f64> = (0..40).map(|at| [0.5, 0.25][at % 2]).collect();
    assert_eq!(code(repeated, Component::F16), Some(b't'));
    let distinct: Vec<f64> = (0..40).map(|at| f64::from(at) + 0.5).collect();
    assert_eq!(code(distinct, Component::F64), None);
    assert_eq!(code(vec![16_777_217.0; 20], Component::F32), Some(b'i'));
    assert_eq!(code(vec![3e9; 20], Component::F64), Some(b't'));
}

/// Scalar numeric arrays of each compressed form read back as written.
#[test]
fn compressed_arrays_read_back() {
    use crate::test_support::{with_context, with_root};
    use crate::text::grammar::parse_layer;
    let list = |values: Vec<String>| values.join(", ");
    let text = format!(
        "#usda 1.0\ndef Mesh \"M\"\n{{\n    int[] faceVertexIndices = [{}]\n    uint[] u = [{}]\n    int64[] wide = [{}]\n    uint64[] uwide = [{}]\n    float[] whole = [{}]\n    half[] table = [{}]\n    double[] plain = [{}]\n    float[] short = [1.5, 2]\n}}\n",
        list((0..300).map(|at| ((at * 7919) % 1000).to_string()).collect()),
        list((0..40).map(|at| (u64::from(u32::MAX) - at).to_string()).collect()),
        list((0..40).map(|at| (i64::MIN + at * 3).to_string()).collect()),
        list((0..40).map(|at| (u64::MAX - at).to_string()).collect()),
        list((0..64).map(|at| (at % 9).to_string()).collect()),
        list((0..64).map(|at| ["0.5", "0.25", "-1.75"][at % 3].to_owned()).collect()),
        list((0..32).map(|at| format!("{at}.125")).collect()),
    );
    let (parsed, bytes) = with_context(text.as_bytes(), |ctx| {
        let layer = parse_layer(ctx, text.as_bytes()).expect("parse");
        let bytes = crate::usdc::write::write(&layer).expect("write");
        (layer, bytes)
    });
    let (read, unread) = with_root(&bytes, |ctx, root| {
        crate::usdc::read(ctx, root).expect("read")
    });
    assert!(unread.is_empty(), "{unread:?}");
    let typed_values = |layer: &crate::text::syntax::Layer| -> Vec<crate::text::value::Typed> {
        layer.prims[0]
            .body
            .properties
            .iter()
            .map(|property| match property {
                crate::text::syntax::PropertySpec::Attribute(attribute) => match &attribute.field {
                    crate::text::syntax::AttributeField::Default(
                        crate::text::syntax::AttributeValue::Value(value),
                    ) => crate::text::value::typed(value, &attribute.type_name, attribute.array)
                        .expect("typed"),
                    other => panic!("{other:?}"),
                },
                other @ crate::text::syntax::PropertySpec::Relationship(_) => {
                    panic!("{other:?}")
                }
            })
            .collect()
    };
    assert_eq!(typed_values(&parsed), typed_values(&read));
}
