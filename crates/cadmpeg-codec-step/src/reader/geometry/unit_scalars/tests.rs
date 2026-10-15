// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::scalar::PositiveReal;

use super::{UnitScalarIndex, UnitState};
use crate::parse::Exchange;
use crate::reader::geometry::UnitScaleKind;
use crate::test_support::with_service_context;

fn exchange(records: &str) -> Exchange {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("valid synthetic units")
        .0
}

#[test]
fn unit_scalar_memo_matches_standalone_units_and_rejections() {
    let exchange = exchange("#1=(LENGTH_UNIT() SI_UNIT(.MILLI.,.METRE.));#2=(PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#3=CONVERSION_BASED_UNIT('',#10);#4=CONVERSION_BASED_UNIT('',#11);#5=CONVERSION_BASED_UNIT('',#12);#6=CONVERSION_BASED_UNIT('',#13);#7=CONVERSION_BASED_UNIT('',#14);#8=CONVERSION_BASED_UNIT('',#15);#10=MEASURE_WITH_UNIT(2.,#1);#11=MEASURE_WITH_UNIT(-2.,#1);#12=MEASURE_WITH_UNIT(1.E308,#1);#13=MEASURE_WITH_UNIT(2.,#5);#14=MEASURE_WITH_UNIT(1.E-310,#1);#15=MEASURE_WITH_UNIT(1.E-310,#7);");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    let mut index = UnitScalarIndex::new(&exchange, &ctx).expect("memo");
    for id in 1..=15 {
        for kind in [UnitScaleKind::Length, UnitScaleKind::Angle] {
            let expected = kind
                .resolve(id, &exchange, &mut BTreeSet::new(), &ctx)
                .expect("standalone walk");
            assert_eq!(
                index.resolve(kind, id).expect("memoized walk"),
                expected,
                "unit {id}"
            );
        }
    }
    drop(index);
    ctx.finish_session().expect("unrefused units");
}

#[test]
fn unit_scalar_memo_cycles_do_not_disable_other_roots() {
    let exchange = exchange("#1=CONVERSION_BASED_UNIT('',#10);#2=CONVERSION_BASED_UNIT('',#11);#3=(LENGTH_UNIT() SI_UNIT(.MILLI.,.METRE.));#4=CONVERSION_BASED_UNIT('',#12);#10=MEASURE_WITH_UNIT(1.,#2);#11=MEASURE_WITH_UNIT(1.,#1);#12=MEASURE_WITH_UNIT(2.,#3);");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    let mut index = UnitScalarIndex::new(&exchange, &ctx).expect("memo");
    for _ in 0..16 {
        assert!(index
            .resolve(UnitScaleKind::Length, 1)
            .expect("cycle")
            .is_none());
        assert!(index
            .resolve(UnitScaleKind::Angle, 2)
            .expect("cycle")
            .is_none());
        assert_eq!(
            index
                .resolve(UnitScaleKind::Length, 4)
                .expect("independent chain")
                .map(PositiveReal::get),
            Some(2.0)
        );
    }
    assert_eq!(index.nodes.len(), 4);
    assert_eq!(index.factors.len(), 3);
    assert!(index
        .nodes
        .values()
        .all(|state| matches!(state, UnitState::Ready(_))));
    drop(index);
    ctx.finish_session().expect("unrefused roots");
}

#[test]
fn unit_scalar_memo_depth_cut_preserves_resolvable_suffixes() {
    let mut records = String::new();
    for id in 1..=256 {
        write!(
            records,
            "#{id}=CONVERSION_BASED_UNIT('',#{});#{}=MEASURE_WITH_UNIT(1.,#{});",
            1000 + id,
            1000 + id,
            id + 1
        )
        .expect("chain");
    }
    records.push_str("#257=(LENGTH_UNIT() SI_UNIT(.MILLI.,.METRE.));");
    let exchange = exchange(&records);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    let mut index = UnitScalarIndex::new(&exchange, &ctx).expect("memo");
    assert!(index
        .resolve(UnitScaleKind::Length, 1)
        .expect("clipped root")
        .is_none());
    assert!(
        !index.nodes.contains_key(&257),
        "the clipped suffix was not parsed"
    );
    assert_eq!(
        index
            .resolve(UnitScaleKind::Length, 2)
            .expect("suffix root")
            .map(PositiveReal::get),
        Some(1.0)
    );
    assert!(index
        .resolve(UnitScaleKind::Length, 1)
        .expect("cached clipped root")
        .is_none());
    drop(index);
    ctx.finish_session().expect("unrefused cut");
}

#[test]
fn unit_scalar_memo_reads_shared_factor_once_with_linear_work() {
    const ROOTS: usize = 400;
    const FACTOR_ITEMS: usize = 4096;
    let mut records =
        "#1=(LENGTH_UNIT() SI_UNIT(.MILLI.,.METRE.));#2=MEASURE_WITH_UNIT(".to_owned();
    for _ in 0..FACTOR_ITEMS {
        records.push_str("$, ");
    }
    records.push_str("2.,#1);");
    for id in 3..3 + ROOTS {
        write!(records, "#{id}=CONVERSION_BASED_UNIT('',#2);").expect("root");
    }
    let exchange = exchange(&records);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two factor-field scans and charged keyed operations fit this bound.
    // Re-reading the wide factor for each root exceeds it.
    policy.limits.max_work_units = 64 * u64::try_from(ROOTS + FACTOR_ITEMS).expect("small fixture");
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    let mut index = UnitScalarIndex::new(&exchange, &ctx).expect("memo");
    for id in 3..3 + ROOTS {
        assert_eq!(
            index
                .resolve(
                    UnitScaleKind::Length,
                    u64::try_from(id).expect("small unit id")
                )
                .expect("linear factor work")
                .map(PositiveReal::get),
            Some(2.0)
        );
        assert!(index
            .resolve(
                UnitScaleKind::Angle,
                u64::try_from(id).expect("small unit id")
            )
            .expect("shared dimension")
            .is_none());
    }
    assert_eq!(index.factors.len(), 1);
    assert_eq!(index.nodes.len(), ROOTS + 1);
    drop(index);
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test unit memo scratch reuse",
        )
        .expect("all unit memo storage released");
    drop(reuse);
    ctx.finish_session().expect("unrefused memo");
}
