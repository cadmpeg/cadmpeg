// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{FeatureTrimEntity, FeatureTrimEntityTable, TrimEntityKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::mem::size_of;

#[test]
fn trim_last_unmatched_row_keeps_its_unique_segment_without_an_empty_visit() {
    let mut definition = super::arc_radius_definition([3.0, 3.0]);
    definition.segments.as_mut().expect("segments").declared_count = 1;
    definition.trim_entities = Some(FeatureTrimEntityTable {
        declared_count: None, entity_ref: None, entry_ref: None, buckets: Vec::new(),
        rows: vec![FeatureTrimEntity {
            external_id: 99, mode: Some(0), vertices: [2, 3], kind: TrimEntityKind::Arc { center_vertex: 1 }, offset: 0,
        }], solved_external_ids: vec![99], offset: 0,
    });
    // Three singleton sorts each admit two roster visits and
    // (one value + two keys) * two levels * eight bytes of sort work.
    // Six other visits include the current core bucket/windows end probes;
    // these upstream generic semantics remain a separate shared request.
    // Two unique-candidate visits and three u32 comparisons complete the route.
    let scalar_bytes = u64::try_from(size_of::<u32>()).expect("u32 size");
    let sort_work = 2 + 3 * scalar_bytes * 2 * 8;
    let work = 3 * sort_work + 6 + 2 + 3 + 6 * scalar_bytes;
    for cap in [0, work - 1, work, work + 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || super::super::trim_segment_ids(&ctx, &definition);
        let original = if cap >= work {
            assert_eq!(run().expect("unique missing partner admitted"), vec![Some(10)]);
            ctx.charge_work_limit(cap - work + 1, "after trim partner boundary").expect_err("exact work used")
        } else {
            let Err(CodecError::ResourceLimit(original)) = run() else { panic!("present work must refuse"); };
            if cap == work - 1 {
                assert_eq!((original.operation, original.used, original.additional),
                    ("creo trim segment IDs", work - scalar_bytes, scalar_bytes));
            }
            original
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        if cap >= work { assert_eq!(original.used, work); }
        for _ in 0..2 {
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}
