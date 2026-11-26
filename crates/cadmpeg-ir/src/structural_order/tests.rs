// SPDX-License-Identifier: Apache-2.0
//! §2.5's derived structural order on the leaf kinds and on derived types.

use std::cmp::Ordering;

use super::StructuralOrd;

#[derive(cadmpeg_derive::StructuralOrd)]
struct Declared {
    second_by_name: u32,
    first_by_name: u32,
    #[serde(skip)]
    #[allow(dead_code)]
    unwritten: u32,
}

#[derive(cadmpeg_derive::StructuralOrd)]
enum Shape<R = f64> {
    Zeta,
    Alpha(R),
    Mid { value: R },
}

#[test]
fn numbers_compare_by_value_with_one_zero() {
    assert_eq!((-0.0_f64).structural_cmp(&0.0), Ordering::Equal);
    assert_eq!((-1.0_f64).structural_cmp(&0.0), Ordering::Less);
    assert_eq!(2_i64.structural_cmp(&-3), Ordering::Greater);
    assert_eq!(false.structural_cmp(&true), Ordering::Less);
}

#[test]
fn options_lists_and_strings_follow_section_2_5() {
    assert_eq!(None::<u32>.structural_cmp(&Some(0)), Ordering::Less);
    assert_eq!(
        vec![1_u32, 2].structural_cmp(&vec![1, 2, 0]),
        Ordering::Less
    );
    assert_eq!(vec![2_u32].structural_cmp(&vec![1, 9]), Ordering::Greater);
    assert_eq!("Z".structural_cmp("a"), Ordering::Less);
    assert_eq!("\u{e9}".structural_cmp("\u{20ac}"), Ordering::Less);
}

#[test]
fn a_struct_compares_its_written_fields_in_declared_order() {
    let value = |second_by_name, first_by_name, unwritten| Declared {
        second_by_name,
        first_by_name,
        unwritten,
    };
    assert_eq!(
        value(1, 9, 0).structural_cmp(&value(2, 0, 0)),
        Ordering::Less
    );
    assert_eq!(
        value(1, 1, 5).structural_cmp(&value(1, 1, 7)),
        Ordering::Equal
    );
}

#[test]
fn an_enum_compares_variants_in_declared_order_then_payload() {
    assert_eq!(
        Shape::Zeta.structural_cmp(&Shape::Alpha(0.0)),
        Ordering::Less
    );
    assert_eq!(
        Shape::Alpha(5.0).structural_cmp(&Shape::Mid { value: 0.0 }),
        Ordering::Less
    );
    assert_eq!(
        Shape::Mid { value: -0.0 }.structural_cmp(&Shape::Mid { value: 0.0 }),
        Ordering::Equal
    );
    assert_eq!(
        Shape::Alpha(1.0).structural_cmp(&Shape::Alpha(2.0)),
        Ordering::Less
    );
}
