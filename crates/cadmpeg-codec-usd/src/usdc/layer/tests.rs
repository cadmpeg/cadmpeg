// SPDX-License-Identifier: Apache-2.0

use super::{join, number, variant_name};

#[test]
fn path_text_joins_elements() {
    assert_eq!(join("/", "World", false), "/World");
    assert_eq!(join("/World", "Mesh", false), "/World/Mesh");
    assert_eq!(join("/World", "points", true), "/World.points");
    assert_eq!(
        join("/World", "{shading=red}", false),
        "/World{shading=red}"
    );
    assert_eq!(
        join("/World{shading=red}", "Cube", false),
        "/World{shading=red}Cube"
    );
}

#[test]
fn variant_elements_name_the_set_or_the_variant() {
    assert_eq!(variant_name("{shading=}"), "shading");
    assert_eq!(variant_name("{shading=red}"), "red");
    assert_eq!(variant_name("Cube"), "Cube");
}

#[test]
fn integral_numbers_carry_their_integer() {
    assert_eq!(number(24.0).integer, Some(24));
    assert_eq!(number(0.5).integer, None);
    assert_eq!(number(f64::INFINITY).integer, None);
}
