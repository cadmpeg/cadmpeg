// SPDX-License-Identifier: Apache-2.0

use super::{UnitScalars, UnitScaleKind};
use cadmpeg_ir::scalar::PositiveReal;
use std::fmt::Write;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";

#[test]
fn shared_conversion_units_reuse_suffixes_without_a_logical_depth_cutoff() {
    let mut source = format!("{HEADER}#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));");
    for id in 2..=1024 {
        write!(source, "#{id}=(CONVERSION_BASED_UNIT('',#{}) LENGTH_UNIT() NAMED_UNIT(*));#{}=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#{});", id + 2000, id + 2000, id - 1).expect("authored chain");
    }
    source.push_str(TAIL);
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("unit graph");
    crate::test_support::with_service_context(&[], |_, ctx| {
        let mut units = UnitScalars::new(&exchange);
        for id in 1..=1024 {
            assert_eq!(
                units
                    .scale(UnitScaleKind::Length, id, ctx)
                    .expect("cached suffix"),
                Some(PositiveReal::ONE)
            );
        }
        assert_eq!(units.values.len(), 1024);
        for id in (1..=1024).rev() {
            assert_eq!(
                units
                    .scale(UnitScaleKind::Length, id, ctx)
                    .expect("memo hit"),
                Some(PositiveReal::ONE)
            );
        }
        assert_eq!(units.values.len(), 1024);
    });
}

#[test]
fn unit_memo_separates_dimensions_cycles_and_invalid_factors() {
    let source = format!(
        "{HEADER}
#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=(CONVERSION_BASED_UNIT('',#4) LENGTH_UNIT() NAMED_UNIT(*));
#4=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(2.),#1);
#5=(CONVERSION_BASED_UNIT('',#6) NAMED_UNIT(*) PLANE_ANGLE_UNIT());
#6=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.5),#2);
#7=(CONVERSION_BASED_UNIT('',#8) LENGTH_UNIT() NAMED_UNIT(*));
#8=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#9);
#9=(CONVERSION_BASED_UNIT('',#10) LENGTH_UNIT() NAMED_UNIT(*));
#10=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#7);
#11=(CONVERSION_BASED_UNIT('',#12) LENGTH_UNIT() NAMED_UNIT(*));
#12=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(-1.),#3);
#13=(CONVERSION_BASED_UNIT('',#14) LENGTH_UNIT() NAMED_UNIT(*));
#14=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(-1.),#11);
{TAIL}"
    );
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("unit graph");
    crate::test_support::with_service_context(&[], |_, ctx| {
        let mut units = UnitScalars::new(&exchange);
        for id in [7, 9, 11, 13, 999] {
            assert_eq!(
                units
                    .scale(UnitScaleKind::Length, id, ctx)
                    .expect("unresolved unit"),
                None
            );
        }
        for (kind, id, value) in [
            (UnitScaleKind::Length, 3, 2.0),
            (UnitScaleKind::Angle, 5, 0.5),
        ] {
            assert_eq!(
                units.scale(kind, id, ctx).expect("unit"),
                PositiveReal::new(value)
            );
        }
        assert_eq!(
            units
                .scale(UnitScaleKind::Angle, 3, ctx)
                .expect("wrong dimension"),
            None
        );
        assert_eq!(
            units
                .scale(UnitScaleKind::Length, 5, ctx)
                .expect("wrong dimension"),
            None
        );
        let mut active = std::collections::BTreeSet::from([3]);
        assert_eq!(
            units
                .resolve(UnitScaleKind::Length, 3, &mut active, ctx)
                .expect("active cycle"),
            None
        );
        assert_eq!(
            units
                .scale(UnitScaleKind::Length, 3, ctx)
                .expect("memo remains valid"),
            PositiveReal::new(2.0)
        );
    });
}
