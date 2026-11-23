// SPDX-License-Identifier: Apache-2.0
//! A `TOLERANCE_VALUE` that stores its upper deviation as `lower_bound`
//! states its deviations in order and keeps the stored order.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::parameter::Tolerance;
use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition};
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::test_support::exchange::decode_inline;
use crate::{StepCodec, StepSchema, StepWriteOptions};

const EPS_DEVIATION: f64 = 1.0e-12;

fn plus_minus(lower_bound: &str, upper_bound: &str) -> String {
    format!(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=SHAPE_ASPECT('feature','',#5,.T.);
#10=DIMENSIONAL_SIZE(#6,'diameter');
#11=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE({lower_bound}),#1);
#12=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE({upper_bound}),#1);
#13=TOLERANCE_VALUE(#11,#12);
#14=PLUS_MINUS_TOLERANCE(#13,#10);
#99=UNRESOLVED_PRODUCT();"
    )
}

fn dimension(ir: &CadIr) -> (&PmiAnnotation, [f64; 2]) {
    ir.model
        .pmi
        .iter()
        .find_map(|annotation| match &annotation.definition {
            PmiDefinition::Dimension(dimension) => match &dimension.tolerance {
                Some(Tolerance::Deviations(deviations)) => {
                    Some((annotation, [deviations.lower.get(), deviations.upper.get()]))
                }
                other => panic!("deviations: {other:?}"),
            },
            _ => None,
        })
        .expect("dimension annotation")
}

fn upper_first(annotation: &PmiAnnotation) -> bool {
    match annotation.source_settings.get("step:tol_order") {
        Some(NativeOption::Text(Dimension::Literal(order))) => {
            assert_eq!(order.as_str(), "upper_lower");
            true
        }
        None => false,
        other => panic!("step:tol_order: {other:?}"),
    }
}

#[test]
fn deviations_stored_upper_first_are_stated_in_order_and_written_back_upper_first() {
    for (lower_bound, upper_bound, stored_upper_first) in
        [("0.1", "-0.1", true), ("-0.1", "0.1", false)]
    {
        let result = decode_inline(&plus_minus(lower_bound, upper_bound));
        let (annotation, [lower, upper]) = dimension(result.ir());
        assert!((lower + 0.1).abs() < EPS_DEVIATION, "{lower}");
        assert!((upper - 0.1).abs() < EPS_DEVIATION, "{upper}");
        assert_eq!(upper_first(annotation), stored_upper_first);

        let mut output = Vec::new();
        write_step(
            result.ir(),
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .expect("write the tolerance");
        let roundtrip = StepCodec::default()
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("decode the written tolerance");
        let (annotation, [lower, upper]) = dimension(roundtrip.ir());
        assert!((lower + 0.1).abs() < EPS_DEVIATION, "{lower}");
        assert!((upper - 0.1).abs() < EPS_DEVIATION, "{upper}");
        assert_eq!(upper_first(annotation), stored_upper_first);
    }
}
