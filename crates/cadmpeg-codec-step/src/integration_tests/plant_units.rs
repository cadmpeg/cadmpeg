// SPDX-License-Identifier: Apache-2.0
//! A plant geometry context whose declared length unit its items'
//! end-to-end lengths contradict reports the unit they imply.

use std::fmt::Write as _;

use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;

/// A plant item in a context that declares the foot, with one end-to-end
/// length of `length` millimetres between two connectors for each of
/// `distances`, the connect points `distance` foot units apart.
fn plant_item(length: f64, distances: &[f64]) -> String {
    let mut records = String::from(
        "#1=APPLICATION_CONTEXT('plant');\n\
         #2=PRODUCT_CONTEXT('',#1,'plant item');\n\
         #3=PRODUCT('item','item','',(#2));\n\
         #4=PRODUCT_DEFINITION_FORMATION('','',#3);\n\
         #5=PRODUCT_DEFINITION_CONTEXT('physical occurrence',#1,'physical design');\n\
         #6=PIPING_COMPONENT_DEFINITION('item','',#4,#5);\n\
         #7=PRODUCT_DEFINITION_SHAPE('','',#6);\n\
         #8=SHAPE_DEFINITION_REPRESENTATION(#7,#20);\n\
         #9=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#10);\n\
         #10=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #11=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #12=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\n\
         #13=(CONVERSION_BASED_UNIT('inch',#9) LENGTH_UNIT() NAMED_UNIT(#12));\n\
         #14=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(12.),#13);\n\
         #15=(CONVERSION_BASED_UNIT('foot',#14) LENGTH_UNIT() NAMED_UNIT(#12));\n\
         #16=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#15,#11)) REPRESENTATION_CONTEXT('g0','plant'));\n\
         #20=SHAPE_REPRESENTATION('outline',(#21),#16);\n\
         #21=CARTESIAN_POINT('plant item location',(0.,0.,0.));\n",
    );
    for (index, distance) in distances.iter().enumerate() {
        let base = 100 + 20 * index;
        let ids: Vec<usize> = (base..base + 14).collect();
        writeln!(
            records,
            "#{a}=PLANT_ITEM_CONNECTOR($,'end 1',#7,$);\n\
             #{b}=PLANT_ITEM_CONNECTOR($,'end 2',#7,$);\n\
             #{c}=PROPERTY_DEFINITION($,$,#{a});\n\
             #{d}=REPRESENTATION($,(#{e}),$);\n\
             #{e}=CARTESIAN_POINT('connect point',(0.,0.,0.));\n\
             #{f}=PROPERTY_DEFINITION_REPRESENTATION(#{c},#{d});\n\
             #{g}=PROPERTY_DEFINITION($,$,#{b});\n\
             #{h}=REPRESENTATION($,(#{i}),$);\n\
             #{i}=CARTESIAN_POINT('connect point',({distance:?},0.,0.));\n\
             #{j}=PROPERTY_DEFINITION_REPRESENTATION(#{g},#{h});\n\
             #{k}=DIMENSIONAL_LOCATION($,$,#{a},#{b});\n\
             #{l}=SHAPE_DIMENSION_REPRESENTATION('pipe dimensional shape',(#{m}),$);\n\
             #{m}=(LENGTH_MEASURE_WITH_UNIT() MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(LENGTH_MEASURE({length:?}),#10) REPRESENTATION_ITEM('end to end length'));\n\
             #{n}=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#{k},#{l});",
            a = ids[0],
            b = ids[1],
            c = ids[2],
            d = ids[3],
            e = ids[4],
            f = ids[5],
            g = ids[6],
            h = ids[7],
            i = ids[8],
            j = ids[9],
            k = ids[10],
            l = ids[11],
            m = ids[12],
            n = ids[13],
        )
        .expect("write a dimension");
    }
    records
}

/// The messages of the `geometry.length-unit-contradicted` losses.
fn contradictions(records: &str) -> Vec<String> {
    decode_inline(records)
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::LengthUnitContradicted.kind())
        .map(|loss| loss.message.clone())
        .collect()
}

/// A length of 100 mm whose connect points lie 100 declared feet apart
/// implies a unit of 1 mm.
#[test]
fn a_foot_context_with_millimetre_dimensions_reports_its_implied_unit() {
    let messages = contradictions(&plant_item(100.0, &[100.0]));
    let [message] = messages.as_slice() else {
        panic!("one contradiction, got {messages:?}");
    };
    assert!(message.contains("#16"), "{message}");
    assert!(message.contains("1 of 1"), "{message}");
    assert!(
        message.contains("implies a unit of 1.000000e0 mm"),
        "{message}"
    );
}

/// Connect points `100 / 304.8` feet apart agree with the declared foot.
#[test]
fn dimensions_that_agree_with_the_declared_unit_report_nothing() {
    assert!(contradictions(&plant_item(100.0, &[100.0 / 304.8])).is_empty());
}

/// Two dimensions with different ratios share no clear majority.
#[test]
fn inconsistent_dimensions_report_nothing() {
    assert!(contradictions(&plant_item(100.0, &[100.0, 50.0])).is_empty());
}
