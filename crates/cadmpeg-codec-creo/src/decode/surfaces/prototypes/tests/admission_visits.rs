// SPDX-License-Identifier: Apache-2.0

use super::super::{
      prototype_parameter_array,
     prototype_vector_array, 
      
};
use crate::surface::{
    SurfaceNamedParameter, SurfaceNamedValue, SurfacePrototypeFamily, SurfacePrototypeRecord,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn record(name: &str, value: SurfaceNamedValue) -> SurfacePrototypeRecord {
    SurfacePrototypeRecord::new_for_test(
        SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Splsrf),
        vec![SurfaceNamedParameter {
            name: name.to_owned(),
            value,
            body: Vec::new(),
            offset: 0,
            value_offset: 0,
        }],
        0,
    )
}

fn assert_work_events(
    events: &[(u64, &'static str)],
    query: impl Fn(&DecodeContext<'_>) -> Result<bool, CodecError>,
) {
    let total: u64 = events.iter().map(|(work, _)| work).sum();
    let observed = std::cell::RefCell::new(Vec::new());
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &events
            .iter()
            .map(|(_, operation)| *operation)
            .collect::<Vec<_>>(),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = query(&ctx);
            if let Err(CodecError::ResourceLimit(original)) = &result {
                assert_eq!((original.limit, original.used), (cap, cap));
                observed
                    .borrow_mut()
                    .push((original.additional, original.operation));
                assert_eq!(ctx.resource_refusal().as_ref(), Some(original));
                assert!(
                    matches!(query(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == *original)
                );
            }
            result.map(|matched| assert!(matched))
        },
    );
    assert_eq!(observed.into_inner(), events);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = total;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(query(&ctx).expect("exact work admits expected values"));
    let original = ctx
        .charge_work_limit(1, "after prototype array visits")
        .expect_err("exact cap");
    assert_eq!(
        (original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, total, 1)
    );
    assert!(matches!(query(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

fn array_events(name: &str, visits: usize, operation: &'static str) -> Vec<(u64, &'static str)> {
    // One present field visit. Core equality admits each eight-byte name once.
    // The concrete scalar/chunk traversal then admits each present visit once.
    let bytes = u64::try_from(name.len()).expect("fixture name fits u64");
    let mut events = vec![
        (1, "creo prototype field search"),
        (bytes, "creo prototype field name comparison"),
        (bytes, "creo prototype field name comparison"),
    ];
    events.extend(std::iter::repeat_n((1, operation), visits));
    events
}

#[test]
fn prototype_vector_arrays_admit_present_triples_before_scalar_inspection() {
    for count in [0, 1, 4] {
        let mut array =
            crate::surface::arrays::DimensionedScalars::empty(count, 3).expect("triple extent");
        array
            .fill_values(vec![
                Some(2.0);
                3 * usize::try_from(count)
                    .expect("fixture count fits usize")
            ])
            .expect("finite triples");
        let present = record("i_points", SurfaceNamedValue::ScalarArray(array));
        let expected = vec![[2.0; 3]; usize::try_from(count).expect("fixture count fits usize")];
        assert_work_events(
            &array_events(
                "i_points",
                usize::try_from(count).expect("fixture count fits usize"),
                "creo prototype vector array traversal",
            ),
            |ctx| {
                let mut storage = ctx.reserve_scoped(0, "test vector projection storage")?;
                let actual =
                    storage.with_storage(|| prototype_vector_array(ctx, &present, "i_points"))?;
                Ok(actual.as_ref() == Some(&expected))
            },
        );
    }
    let missing = record(
        "i_points",
        SurfaceNamedValue::ScalarArray(
            crate::surface::arrays::DimensionedScalars::empty(129, 3)
                .expect("missing triple extent"),
        ),
    );
    assert_work_events(
        &array_events("i_points", 1, "creo prototype vector array traversal"),
        |ctx| prototype_vector_array(ctx, &missing, "i_points").map(|actual| actual.is_none()),
    );
}

#[test]
fn prototype_parameter_arrays_admit_present_values_and_stop_at_absence() {
    for count in [0, 1, 4] {
        let mut array =
            crate::surface::arrays::CountedScalars::empty(count).expect("parameter extent");
        array
            .fill_values(vec![
                Some(2.0);
                usize::try_from(count)
                    .expect("fixture count fits usize")
            ])
            .expect("finite parameters");
        let present = record("u_params", SurfaceNamedValue::CountedScalarArray(array));
        let expected = vec![2.0; usize::try_from(count).expect("fixture count fits usize")];
        assert_work_events(
            &array_events(
                "u_params",
                usize::try_from(count).expect("fixture count fits usize"),
                "creo prototype parameter array traversal",
            ),
            |ctx| {
                let mut storage = ctx.reserve_scoped(0, "test parameter projection storage")?;
                let actual = storage
                    .with_storage(|| prototype_parameter_array(ctx, &present, "u_params"))?;
                Ok(actual.as_ref() == Some(&expected))
            },
        );
    }
    let missing = record(
        "u_params",
        SurfaceNamedValue::CountedScalarArray(
            crate::surface::arrays::CountedScalars::empty(129).expect("missing parameter extent"),
        ),
    );
    assert_work_events(
        &array_events("u_params", 1, "creo prototype parameter array traversal"),
        |ctx| prototype_parameter_array(ctx, &missing, "u_params").map(|actual| actual.is_none()),
    );
}

