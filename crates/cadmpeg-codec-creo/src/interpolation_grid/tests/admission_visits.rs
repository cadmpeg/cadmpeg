// SPDX-License-Identifier: Apache-2.0

use super::super::InterpolationGrid;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn fixed_mixed_validation_keeps_only_variable_grid_work() {
    for (u_count, v_count) in [(2_u32, 2_u32), (2, 3), (3, 2)] {
        let u = usize::try_from(u_count).expect("fixture axis fits usize");
        let v = usize::try_from(v_count).expect("fixture axis fits usize");
        for nonfinite in [false, true] {
            // Existing parameter validation admits three scalar operations per
            // parameter; variable vector validation admits three coordinates
            // per vector. Fixed corner validation has no input-sized work.
            let events = [
                (u64::from(3 * u_count), "creo interpolation grid parameter validation"),
                (u64::from(3 * v_count), "creo interpolation grid parameter validation"),
                (u64::from(3 * u_count * v_count), "creo interpolation grid vector validation"),
                (u64::from(6 * v_count), "creo interpolation grid vector validation"),
                (u64::from(6 * u_count), "creo interpolation grid vector validation"),
            ];
            let work: u64 = events.iter().map(|(units, _)| units).sum();
            for cap in 0..=work + 1 {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_collection_items = 0;
                policy.limits.max_entities = 0;
                policy.limits.max_recursion_depth = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let run = || {
                    let mut mixed = [[1.0; 3]; 4];
                    if nonfinite { mixed[3][2] = f64::INFINITY; }
                    // Test inputs exist before the constructor. This control
                    // measures its validation, not ownership of caller storage.
                    InterpolationGrid::try_new(
                        &ctx,
                        vec![[0.0; 3]; u * v],
                        (0..u_count).map(f64::from).collect(),
                        (0..v_count).map(f64::from).collect(),
                        vec![[0.0; 3]; 2 * v],
                        vec![[0.0; 3]; 2 * u],
                        mixed,
                    )
                };
                let original = if cap >= work {
                    let actual = run().expect("variable validation admitted");
                    if nonfinite {
                        assert!(actual.is_none());
                    } else {
                        let grid = actual.expect("finite complete grid");
                        assert_eq!(grid.points().len(), u * v);
                        assert_eq!(grid.mixed_derivatives(), &[[1.0; 3]; 4]);
                    }
                    let original = ctx.charge_work_limit(cap - work + 1, "after fixed mixed validation")
                        .expect_err("exact variable work consumed");
                    assert_eq!(original.used, work);
                    original
                } else {
                    let mut used = 0;
                    let (additional, operation) = events.iter().copied().find(|(units, _)| {
                        if used + units > cap { true } else { used += units; false }
                    }).expect("first refused variable field");
                    let Err(CodecError::ResourceLimit(original)) = run() else {
                        panic!("variable grid validation must refuse");
                    };
                    assert_eq!((original.dimension, original.limit, original.used, original.additional, original.operation),
                        (ResourceDimension::WorkUnits, cap, used, additional, operation));
                    original
                };
                for _ in 0..2 {
                    assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}
