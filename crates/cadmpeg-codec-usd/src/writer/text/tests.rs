// SPDX-License-Identifier: Apache-2.0

use super::{asset, identifier, quoted, real, real32, Emitter, Names};

#[test]
fn reals_read_back_to_their_value() {
    for value in [
        0.0,
        -0.0,
        1.0,
        0.1,
        1.0 / 3.0,
        1e-300,
        6.02e23,
        -2.5e-7,
        123_456.789,
    ] {
        let text = real(value);
        assert_eq!(
            text.parse::<f64>().unwrap().to_bits(),
            value.to_bits(),
            "{text}"
        );
    }
    assert_eq!(real(f64::INFINITY), "inf");
    assert_eq!(real(f64::NEG_INFINITY), "-inf");
    assert_eq!(real(f64::NAN), "nan");
    assert_eq!(real32(0.1), "0.1");
    assert_eq!(real32(1e-7), "1e-7");
}

#[test]
fn strings_escape_quotes_backslashes_and_controls() {
    assert_eq!(quoted("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\x01\"");
}

#[test]
fn asset_paths_choose_their_delimiter() {
    assert_eq!(asset("a.usd"), "@a.usd@");
    assert_eq!(asset("a@b.usd"), "@@@a@b.usd@@@");
}

#[test]
fn names_are_identifiers_and_unique() {
    assert_eq!(identifier("Part 1"), "Part_1");
    assert_eq!(identifier("1st"), "_1st");
    assert_eq!(identifier(""), "_");
    let mut names = Names::default();
    names.reserve("Body");
    assert_eq!(names.unique("Body"), "Body_1");
    assert_eq!(names.unique("Body"), "Body_2");
}

#[test]
fn long_arrays_wrap() {
    let mut emitter = Emitter::default();
    let items: Vec<String> = (0..10).map(|item| item.to_string()).collect();
    emitter.array("int[] a", &items, "");
    assert_eq!(
        emitter.finish(),
        "int[] a = [\n    0, 1, 2, 3, 4, 5, 6, 7,\n    8, 9\n]\n"
    );
}
