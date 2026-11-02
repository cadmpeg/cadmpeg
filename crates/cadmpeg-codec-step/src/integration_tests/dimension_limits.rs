// SPDX-License-Identifier: Apache-2.0
//! Limit dimensions retain absolute limits through STEP generation.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::pmi::{DimensionTolerance, PmiDefinition};

use crate::export::write_step;
use crate::test_support::exchange::decode_inline;
use crate::test_support::pmi_limits::LIMIT_DIMENSION;
use crate::{StepCodec, StepSchema, StepWriteOptions};

const EPS_LIMIT: f64 = 1.0e-12;

#[test]
fn limit_dimensions_round_trip_with_and_without_a_nominal() {
    for with_nominal in [true, false] {
        let input = if with_nominal {
            LIMIT_DIMENSION.to_owned()
        } else {
            LIMIT_DIMENSION.replace("(#12,#11,#13)", "(#12,#13)")
        };
        let result = decode_inline(&input);
        let mut output = Vec::new();
        write_step(
            result.ir(),
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .expect("write limits");
        let text = std::str::from_utf8(&output).expect("STEP text");
        for name in ["'lower limit'", "'upper limit'"] {
            assert!(text.contains(name), "{name} in {text}");
        }
        assert_eq!(text.contains("'nominal value'"), with_nominal);
        assert!(!text.contains("PLUS_MINUS_TOLERANCE"));
        let roundtrip = StepCodec::default()
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("decode generated limits");
        let dimension = roundtrip
            .ir()
            .model
            .pmi
            .iter()
            .find_map(|annotation| match &annotation.definition {
                PmiDefinition::Dimension(dimension) => Some(dimension),
                _ => None,
            })
            .expect("dimension");
        assert_eq!(dimension.nominal().is_some(), with_nominal);
        if let Some(nominal) = dimension.nominal() {
            assert!((nominal.value.get() - 35.0).abs() < EPS_LIMIT);
        }
        let Some(DimensionTolerance::Limits { lower, upper }) = dimension.tolerance() else {
            panic!("limits: {dimension:?}");
        };
        assert!((lower.value.get() - 34.8).abs() < EPS_LIMIT);
        assert!((upper.value.get() - 35.2).abs() < EPS_LIMIT);
    }
}
