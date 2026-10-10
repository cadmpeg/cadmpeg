// SPDX-License-Identifier: Apache-2.0

use super::super::{
    first_instance_surface_row, frame_bound, prototype_parameter_array,
    prototype_tabulated_chart_origin, prototype_vector_array,
    surface_prototype_frame_bounds, unique_surface_prototype_associations,
    PrototypeFrames, PrototypeRows,
};
use crate::surface::{SurfaceNamedParameter, SurfaceNamedValue, SurfacePrototypeFamily, SurfacePrototypeRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn record(name: &str, value: SurfaceNamedValue) -> SurfacePrototypeRecord {
    SurfacePrototypeRecord::new_for_test(
SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Splsrf),
vec![SurfaceNamedParameter {
            name: name.to_owned(), value, body: Vec::new(), offset: 0, value_offset: 0,
        }],
0,
)
}

fn assert_work_events(
    events: &[(u64, &'static str)],
    query: impl Fn(&DecodeContext<'_>) -> Result<bool, CodecError>,
) {
    let total: u64 = events.iter().map(|(work, _)| work).sum();
    for cap in 0..=total {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = query(&ctx);
        let original = if cap == total {
            assert!(result.expect("exact work admits expected values"));
            let original = ctx.charge_work_limit(1, "after prototype array visits").expect_err("exact cap");
            assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, total, 1));
            original
        } else {
            let mut used = 0;
            let (additional, operation) = events.iter().copied().find(|(work, _)| {
                if used + work > cap { true } else { used += work; false }
            }).expect("first refused event");
            let original = ctx.resource_refusal().expect("work refusal");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.limit, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, cap, used, additional, operation));
            original
        };
        assert!(matches!(query(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

fn array_events(name: &str, visits: usize, operation: &'static str) -> Vec<(u64, &'static str)> {
    // One present field visit. Core equality admits each eight-byte name once.
    // The concrete scalar/chunk traversal then admits each present visit once.
    let bytes = name.len() as u64;
    let mut events = vec![(1, "creo prototype field search"),
        (bytes, "creo prototype field name comparison"), (bytes, "creo prototype field name comparison")];
    events.extend(std::iter::repeat_n((1, operation), visits));
    events
}

#[test]
fn prototype_vector_arrays_admit_present_triples_before_scalar_inspection() {
    for count in [0, 1, 4] {
        let mut array = crate::surface::arrays::DimensionedScalars::empty(count, 3).expect("triple extent");
        array.fill_values(vec![Some(2.0); 3 * count as usize]).expect("finite triples");
        let present = record("i_points", SurfaceNamedValue::ScalarArray(array));
        let expected = vec![[2.0; 3]; count as usize];
        assert_work_events(&array_events("i_points", count as usize, "creo prototype vector array traversal"), |ctx| {
            let mut storage = ctx.reserve_scoped(0, "test vector projection storage")?;
            let actual = storage.with_storage(|| prototype_vector_array(ctx, &present, "i_points"))?;
            Ok(actual.as_ref() == Some(&expected))
        });
    }
    let missing = record("i_points", SurfaceNamedValue::ScalarArray(
        crate::surface::arrays::DimensionedScalars::empty(129, 3).expect("missing triple extent")));
    assert_work_events(&array_events("i_points", 1, "creo prototype vector array traversal"), |ctx| {
        prototype_vector_array(ctx, &missing, "i_points").map(|actual| actual.is_none())
    });
}

#[test]
fn prototype_parameter_arrays_admit_present_values_and_stop_at_absence() {
    for count in [0, 1, 4] {
        let mut array = crate::surface::arrays::CountedScalars::empty(count).expect("parameter extent");
        array.fill_values(vec![Some(2.0); count as usize]).expect("finite parameters");
        let present = record("u_params", SurfaceNamedValue::CountedScalarArray(array));
        let expected = vec![2.0; count as usize];
        assert_work_events(&array_events("u_params", count as usize, "creo prototype parameter array traversal"), |ctx| {
            let mut storage = ctx.reserve_scoped(0, "test parameter projection storage")?;
            let actual = storage.with_storage(|| prototype_parameter_array(ctx, &present, "u_params"))?;
            Ok(actual.as_ref() == Some(&expected))
        });
    }
    let missing = record("u_params", SurfaceNamedValue::CountedScalarArray(
        crate::surface::arrays::CountedScalars::empty(129).expect("missing parameter extent")));
    assert_work_events(&array_events("u_params", 1, "creo prototype parameter array traversal"), |ctx| {
        prototype_parameter_array(ctx, &missing, "u_params").map(|actual| actual.is_none())
    });
}

#[test]
fn prototype_fixed_returns_are_free_and_preserve_original_refusal() {
    let scan = crate::test_support::empty_container_scan();
    let section = crate::container::Section::scan_for_test("VisibGeom".to_owned(), 32, 48, None, &[0; 48])
        .expect("section extent").section;
    let wrong_family = SurfacePrototypeRecord::new_for_test(
SurfacePrototypeFamily::Plane,
Vec::new(),
0,
);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let rows = PrototypeRows::new(&ctx, &[]).expect("empty index");
    let mut frames = PrototypeFrames::new(&ctx).expect("empty workspace");
    let mut check = |refused| {
        let results = [
            prototype_tabulated_chart_origin(&ctx, &wrong_family).map(|origin| origin.is_none()),
            first_instance_surface_row(&ctx, &rows, 32, 48, 40, crate::surface::SurfaceKind::Plane).map(|row| row.is_none()),
            surface_prototype_frame_bounds(&ctx, &scan, &section, 40, &mut frames).map(|bounds| bounds == Some((32, 48))),
            frame_bound(&ctx, &section, 8).map(|bound| bound == 40),
            unique_surface_prototype_associations(&ctx, &scan).map(|associations| associations.is_empty()),
        ];
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert!(result.expect("fixed return"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after prototype fixed returns").expect_err("zero cap");
    assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, 0, 1));
    check(true);
    assert_eq!(ctx.resource_refusal(), Some(original));
}
