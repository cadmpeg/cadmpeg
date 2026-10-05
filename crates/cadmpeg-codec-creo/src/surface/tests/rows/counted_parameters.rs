// SPDX-License-Identifier: Apache-2.0
use crate::scalar;

#[test]
fn counted_parameter_state_resize_refuses_work() {
    let cache = scalar::ScalarCache::default();
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo_counted_parameter_slots",
        |ctx| crate::surface::counted_parameter_scalar_slots(ctx, &[0xe4], 1, &cache),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo_counted_parameter_slots")
    );
}

#[test]
fn counted_parameter_state_cost_counts_map_entries_and_active_parse() {
    use crate::surface::{CountedParameterParse, CountedParameterState};
    use cadmpeg_core::decode::cost::DecodeCost;
    crate::decode::with_test_decode_ctx(|ctx| {
        let empty = CountedParameterState(std::collections::BTreeMap::new());
        assert_eq!(empty.decode_cost(ctx, "counted parameter state cost")?, 0);
        let state = CountedParameterState(std::collections::BTreeMap::from([
            (0, CountedParameterParse::Ambiguous),
            (
                1,
                CountedParameterParse::Unique(vec![(Some(1.0), vec![0xe4])]),
            ),
        ]));
        // Two keys, two parse tags, one option tag, one scalar and one source byte.
        let expected =
            2 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>()) + 2 + 1 + 8 + 1;
        assert_eq!(
            state.decode_cost(ctx, "counted parameter state cost")?,
            expected
        );
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .expect("state costs fit service work");
}
