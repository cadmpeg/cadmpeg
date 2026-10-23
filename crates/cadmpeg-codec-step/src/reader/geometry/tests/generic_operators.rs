// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;

use crate::StepCodec;

#[test]
fn ap203_edition_one_generic_operator_restores_a_two_dimensional_replica() {
    let source = std::str::from_utf8(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"))
        .expect("fixture is UTF-8")
        .replace("AUTOMOTIVE_DESIGN", "CONFIG_CONTROL_DESIGN")
        .replace(
            "#55=DEFINITIONAL_REPRESENTATION('',(#54),#50);",
            "#55=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n\
             #71=DIRECTION('',(1.,0.));\n\
             #74=DIRECTION('',(0.,-1.));\n\
             #75=CARTESIAN_POINT('',(0.,0.));\n\
             #72=CARTESIAN_TRANSFORMATION_OPERATOR('','','',#71,#74,#75,1.);\n\
             #73=CURVE_REPLICA('',#54,#72);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode AP203 edition 1 generic pcurve operator");
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("replica pcurve");
    let PcurveGeometry::Transformed(placed) = &pcurve.geometry else {
        panic!("pcurve replica lost its transformation")
    };
    assert_eq!(
        placed.transform().rows(),
        [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]]
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message.contains("invalid axes, origin, or scale")
            || loss.message.contains("has no decoded surface or 2D curve")
    }));
}
