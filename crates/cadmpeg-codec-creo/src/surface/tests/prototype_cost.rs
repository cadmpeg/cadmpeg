// SPDX-License-Identifier: Apache-2.0

#[test]
fn surface_prototype_decode_cost_covers_nested_tokens_and_offsets() {
    use cadmpeg_core::decode::cost::DecodeCost;

    let mut scalars = crate::surface::arrays::CountedScalars::empty(2)
        .expect("two scalar slots");
    crate::decode::with_test_decode_ctx(|ctx| {
        scalars.fill_tokens(
            ctx,
            vec![(Some(1.5), vec![0xaa]), (None, vec![0xbb, 0xcc])],
        )
    })
    .expect("token-backed scalar slots are admitted")
    .expect("matching slot count");

    let body = vec![0x71, 0xaa, 0xbb, 0xcc];
    let parameter = crate::surface::SurfaceNamedParameter {
        name: "u_params".to_owned(),
        value: crate::surface::SurfaceNamedValue::CountedScalarArray(scalars),
        body: body.clone(),
        offset: 23,
        value_offset: 29,
    };
    let prototype = crate::surface::SurfacePrototypeRecord {
        family: crate::surface::SurfacePrototypeFamily::Other("other".to_owned()),
        parameters: vec![parameter],
        offset: 17,
    };
    let actual = crate::decode::with_test_decode_ctx(|ctx| {
        prototype.decode_cost(ctx, "creo prototype nested cost test")
    })
    .expect("nested prototype fields are admitted");

    let usize_bytes = u64::try_from(std::mem::size_of::<usize>()).expect("usize width fits");
    let u32_bytes = u64::try_from(std::mem::size_of::<u32>()).expect("u32 width fits");
    let f64_bytes = u64::try_from(std::mem::size_of::<f64>()).expect("f64 width fits");
    let body_bytes = u64::try_from(body.len()).expect("body length fits");
    let expected = 1 // Other-family tag
        + 5 // "other"
        + usize_bytes // prototype offset
        + 8 // "u_params"
        + 1 // CountedScalarArray tag
        + u32_bytes // scalar count
        + (1 + f64_bytes) + 1 // Some(1.5) and None slots
        + (1 + 1 + 2) // Some token-slice marker and its one-/two-byte tokens
        + body_bytes
        + (2 * usize_bytes); // header and value offsets
    assert_eq!(actual, expected);
}
