// SPDX-License-Identifier: Apache-2.0

use super::{round_to, typed, Component, Components, Shape, TypeError, ValueType};
use crate::text::syntax::{Number, Value};

fn number(float: f64, integer: Option<i128>) -> Value {
    Value::Number(Number { float, integer })
}

fn tuple(values: &[f64]) -> Value {
    Value::Tuple(values.iter().map(|value| number(*value, None)).collect())
}

#[test]
fn type_names_name_their_component_and_shape() {
    let point = ValueType::parse("point3f").unwrap();
    assert_eq!(point.component, Component::Float);
    assert_eq!(point.shape, Shape::Vector(3));
    assert_eq!(ValueType::parse("quatd").unwrap().shape, Shape::Quaternion);
    assert_eq!(
        ValueType::parse("matrix4d").unwrap().shape,
        Shape::Matrix(4)
    );
    assert_eq!(ValueType::parse("frame4d").unwrap().shape, Shape::Matrix(4));
    assert_eq!(ValueType::parse("int2").unwrap().shape, Shape::Vector(2));
    assert_eq!(ValueType::parse("opaque"), None);
    assert_eq!(ValueType::parse("flaot"), None);
}

#[test]
fn integers_truncate_and_keep_their_range() {
    let read = |value: Value, ty: &str| typed(&value, ty, false).map(|typed| typed.components);
    assert_eq!(
        read(number(1.9, None), "int"),
        Ok(Components::Integers(vec![1]))
    );
    assert_eq!(
        read(number(-1.9, None), "int"),
        Ok(Components::Integers(vec![-1]))
    );
    assert_eq!(
        read(number(256.0, Some(256)), "uchar"),
        Err(TypeError::Range)
    );
    assert_eq!(read(number(-1.0, Some(-1)), "uint"), Err(TypeError::Range));
    assert_eq!(read(number(3e9, None), "int"), Err(TypeError::Range));
    assert_eq!(read(number(f64::NAN, None), "int"), Err(TypeError::Range));
    assert_eq!(
        read(
            number(
                18_446_744_073_709_551_615.0,
                Some(18_446_744_073_709_551_615)
            ),
            "uint64"
        ),
        Ok(Components::Integers(vec![18_446_744_073_709_551_615]))
    );
}

#[test]
fn booleans_admit_numbers_and_the_four_words() {
    let read = |value: Value| typed(&value, "bool", false).map(|typed| typed.components);
    for (value, expected) in [
        (number(2.0, Some(2)), true),
        (number(0.0, Some(0)), false),
        (number(-0.5, None), true),
        (Value::Identifier("YES".into()), true),
        (Value::Identifier("False".into()), false),
        (Value::Identifier("no".into()), false),
    ] {
        assert_eq!(
            read(value.clone()),
            Ok(Components::Bools(vec![expected])),
            "{value:?}"
        );
    }
    assert!(read(Value::Identifier("on".into())).is_err());
}

#[test]
fn reals_round_to_their_declared_precision() {
    assert_eq!(round_to(0.1, Component::Double), 0.1);
    assert_eq!(round_to(0.1, Component::Float), f64::from(0.1_f32));
    assert_eq!(round_to(1e40, Component::Float), f64::INFINITY);
    assert_eq!(round_to(-1e40, Component::Float), f64::NEG_INFINITY);
    assert_eq!(round_to(1.0 / 3.0, Component::Half), 0.333_251_953_125);
    assert_eq!(round_to(65_504.0, Component::Half), 65_504.0);
    assert_eq!(round_to(65_520.0, Component::Half), f64::INFINITY);
    assert_eq!(round_to(70_000.0, Component::Half), f64::INFINITY);
    assert_eq!(
        round_to(5.960_464_477_539_063e-8, Component::Half),
        5.960_464_477_539_063e-8
    );
    assert_eq!(round_to(2.0e-8, Component::Half), 0.0);
}

#[test]
fn shapes_must_match_the_declaration() {
    assert_eq!(
        typed(&tuple(&[1.0, 2.0]), "float3", false),
        Err(TypeError::Shape("a tuple with one item per component"))
    );
    assert!(typed(&number(1.0, Some(1)), "float", true).is_err());
    assert!(typed(&Value::List(vec![number(1.0, Some(1))]), "float", false).is_err());
    let points = typed(
        &Value::List(vec![tuple(&[1.0, 2.0, 3.0]), tuple(&[4.0, 5.0, 6.0])]),
        "point3d",
        true,
    )
    .unwrap();
    assert_eq!(points.width(), 3);
    assert_eq!(points.reals(), Some(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0][..]));
    let matrix = typed(
        &Value::Tuple(vec![
            tuple(&[1.0, 0.0, 0.0, 0.0]),
            tuple(&[0.0, 1.0, 0.0, 0.0]),
            tuple(&[0.0, 0.0, 1.0, 0.0]),
            tuple(&[5.0, 6.0, 7.0, 1.0]),
        ]),
        "matrix4d",
        false,
    )
    .unwrap();
    assert_eq!(matrix.reals().map(<[f64]>::len), Some(16));
}

#[test]
fn text_types_admit_their_spellings() {
    assert_eq!(
        typed(&Value::String("p.usd".into()), "asset", false).map(|typed| typed.components),
        Ok(Components::Texts(vec!["p.usd".into()]))
    );
    assert_eq!(
        typed(&Value::Asset("p.usd".into()), "asset", false).map(|typed| typed.components),
        Ok(Components::Texts(vec!["p.usd".into()]))
    );
    assert!(typed(&Value::Asset("p.usd".into()), "string", false).is_err());
    assert!(typed(&Value::Identifier("x".into()), "token", false).is_err());
}
