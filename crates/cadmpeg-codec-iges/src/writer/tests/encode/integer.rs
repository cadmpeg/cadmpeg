// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn encode_preserves_a_large_integer_attribute() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    let mut ir = CadIr::empty();
    let point = PointId::mint("test:model:point#large-integer").unwrap();
    ir.model.points.push(Point::new(
        point.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
        None,
    ));
    ir.model.attributes.push(SourceAttribute {
        native_ref: None,
        id: "test:model:attribute#large-integer".try_into().unwrap(),
        target: AttributeTarget::Point(point),
        name: "integer".into(),
        values: vec![Some(AttributeValue::Integer(4_294_967_040))],
    });
    let plan = plan_at(IgesVersion::V5_3, &ir, None).unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(
        decoded.report().losses.is_empty(),
        "{:?}",
        decoded.report().losses
    );
    assert!(decoded.ir().model.attributes.iter().any(|attribute| {
        attribute.name == "integer"
            && attribute.values == [Some(AttributeValue::Integer(4_294_967_040))]
    }));
}
