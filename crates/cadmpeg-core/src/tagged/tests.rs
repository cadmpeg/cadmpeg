// SPDX-License-Identifier: Apache-2.0
//! Each probe enum is declared twice from one source: once with serde's
//! derived `Deserialize` and once with `DeserializeTagged`. Both readers read
//! each input, and the tests require the same value or the same refusal.

use std::fmt::Debug;

use serde::de::{DeserializeOwned, Deserializer};
use serde::Deserialize;

/// A payload struct that refuses unknown keys.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Point {
    x: f64,
    y: f64,
}

/// A payload struct that a struct variant flattens.
#[derive(Debug, PartialEq, Deserialize)]
struct Offset {
    dx: f64,
}

/// A space whose ids every probe can read.
trait Space {
    type Id: DeserializeOwned + Debug + PartialEq;
}

impl Space for u8 {
    type Id = u32;
}

/// A field reader that adds one, so the probes show that it ran.
fn successor<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    u32::deserialize(deserializer).map(|value| value + 1)
}

/// The enum holds the probe in a field, so the parser that reads the field
/// states the position of a refusal.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Holder<T> {
    field: T,
}

macro_rules! probes {
    ($($item:item)*) => {
        mod serde_form {
            use super::{successor, Offset, Point, Space};
            use serde::Deserialize as Reader;
            $($item)*
        }
        mod tagged_form {
            use super::{successor, Offset, Point, Space};
            use cadmpeg_derive::DeserializeTagged as Reader;
            $($item)*
        }
    };
}

probes! {
    #[derive(Debug, PartialEq, Reader)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub(super) enum Internal {
        #[serde(deserialize_with = "crate::tag_only::read")]
        Empty,
        Plain,
        Count {
            count: u32,
            #[serde(default)]
            label: Option<String>,
        },
        Wrapped(Point),
        #[serde(rename = "renamed")]
        Moved { to: Point },
        #[serde(alias = "nested_alias")]
        Nested { inner: Box<Adjacent> },
        Steps { steps: Vec<Internal> },
    }

    #[derive(Debug, PartialEq, Reader)]
    #[serde(tag = "type")]
    pub(super) enum Loose {
        A { a: u8 },
        B(Point),
    }

    #[derive(Debug, PartialEq, Reader)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub(super) enum Flat {
        Spread {
            #[serde(flatten)]
            offset: Offset,
            extra: u8,
        },
    }

    #[derive(Debug, PartialEq, Reader)]
    #[serde(tag = "kind", content = "value", rename_all = "snake_case", deny_unknown_fields)]
    pub(super) enum Adjacent {
        #[serde(deserialize_with = "crate::tag_only::refuse_value")]
        Empty,
        Bare,
        Number(u32),
        Maybe(Option<String>),
        Pair { left: u8, right: u8 },
        Inner(Internal),
        #[serde(rename = "next")]
        Successor(#[serde(deserialize_with = "successor")] u32),
        Folder { steps: Vec<Adjacent> },
    }

    #[derive(Debug, PartialEq, Reader)]
    #[serde(tag = "t", content = "c")]
    pub(super) enum LooseAdjacent {
        A(u8),
        B { b: u8 },
    }

    #[derive(Debug, PartialEq, Reader)]
    #[serde(
        tag = "kind",
        content = "value",
        rename_all = "snake_case",
        deny_unknown_fields,
        bound = ""
    )]
    pub(super) enum Generic<S: Space> {
        One(S::Id),
        Two { a: S::Id, b: S::Id },
        Other { n: u8 },
    }

    #[derive(Debug, PartialEq, Reader)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    pub(super) enum Inferred<T> {
        One(T),
        Two { a: T, b: Vec<T> },
    }

    #[derive(Debug, PartialEq, Reader)]
    #[serde(tag = "kind", content = "value", rename_all = "snake_case")]
    #[serde(bound(deserialize = "T: serde::Deserialize<'de> + Clone"))]
    pub(super) enum Stated<T> {
        One(T),
        Two { a: T },
    }
}

/// What a reader makes of `text`: the value, or the refusal's message.
fn read<T: DeserializeOwned + Debug>(text: &str) -> String {
    match serde_json::from_str::<T>(text) {
        Ok(value) => format!("ok {value:?}"),
        Err(error) => format!("refused {error}"),
    }
}

/// What a reader makes of the `serde_json::Value` of `text`.
fn value<T: DeserializeOwned + Debug>(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return "no JSON value".to_owned();
    };
    match serde_json::from_value::<T>(value) {
        Ok(value) => format!("ok {value:?}"),
        Err(error) => format!("refused {error}"),
    }
}

/// `outcome` without the position that a refusal states.
fn without_position(outcome: &str) -> &str {
    let Some(at) = outcome.rfind(" at line ") else {
        return outcome;
    };
    let tail = &outcome[at + " at line ".len()..];
    let positioned = tail.split_once(" column ").is_some_and(|(line, column)| {
        [line, column]
            .iter()
            .all(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
    });
    if positioned {
        &outcome[..at]
    } else {
        outcome
    }
}

/// Requires the two readers to agree on each input: read directly, read from
/// a `Value`, and read as a field. With `positions` the refusals agree to the
/// position; without it they agree on the message.
fn agree<S, T>(inputs: &[&str], positions: bool)
where
    S: DeserializeOwned + Debug,
    T: DeserializeOwned + Debug,
{
    for input in inputs {
        let held = format!("{{\"field\": {input}}}");
        for (serde, tagged) in [
            (read::<S>(input), read::<T>(input)),
            (read::<Holder<S>>(&held), read::<Holder<T>>(&held)),
        ] {
            if positions {
                assert_eq!(tagged, serde, "{input}");
            } else {
                assert_eq!(
                    without_position(&tagged),
                    without_position(&serde),
                    "{input}"
                );
            }
        }
        assert_eq!(value::<T>(input), value::<S>(input), "{input}");
    }
}

const INTERNAL: &[&str] = &[
    r#"{"kind":"empty"}"#,
    r#"{"kind":"empty","x":1}"#,
    r#"["empty"]"#,
    r#"{"kind":"plain","x":[1,{"y":2}],"z":null}"#,
    r#"{"kind":"count","count":3}"#,
    r#"{"count":3,"kind":"count"}"#,
    r#"{ "kind" : "count" , "count" : 3 , "label" : "a\"b\\c\u0001é😀" }"#,
    r#"{"kind":"count","count":3,"label":null}"#,
    r#"{"kind":"count","count":3,"extra":true}"#,
    r#"{"kind":"count"}"#,
    r#"{"kind":"count","count":3,"count":4}"#,
    r#"{"kind":"count","kind":"count","count":3}"#,
    r#"{"count":3}"#,
    r"{}",
    r#"{"kind":"bogus"}"#,
    r#"{"kind":3}"#,
    r#"{"kind":{"count":null},"count":3}"#,
    r#"{"kind":null}"#,
    r#"["count",3]"#,
    r#"["count",3,null]"#,
    r"[]",
    r#""count""#,
    "null",
    "3",
    "true",
    r#"{"kind":"count","count":3.0}"#,
    r#"{"kind":"count","count":-1}"#,
    r#"{"kind":"count","count":18446744073709551616}"#,
    r#"{"kind":"count","count":1e2}"#,
    r#"{"kind":"count","count":"3"}"#,
    r#"{"kind":"wrapped","x":1.5,"y":-0.0}"#,
    r#"{"kind":"wrapped","x":1e300,"y":5e-324}"#,
    r#"{"kind":"wrapped","x":0.1,"y":123456789012345680000}"#,
    r#"{"kind":"wrapped","x":1,"y":-2}"#,
    r#"{"kind":"wrapped","x":1}"#,
    r#"{"kind":"wrapped","x":1,"y":2,"z":3}"#,
    r#"["wrapped",1,2]"#,
    r#"{"kind":"renamed","to":{"x":1,"y":2}}"#,
    r#"{"kind":"moved","to":{"x":1,"y":2}}"#,
    r#"{"kind":"nested","inner":{"kind":"number","value":5}}"#,
    r#"{"kind":"nested_alias","inner":{"kind":"bare"}}"#,
    r#"{"kind":"nested","inner":{"kind":"number","value":"x"}}"#,
    r#"{"kind":"nested","inner":{"value":7,"kind":"number"}}"#,
    r#"{"kind":"steps","steps":[{"kind":"empty"},{"kind":"count","count":1}]}"#,
    r#"{"kind":"steps","steps":[{"kind":"count","count":"x"}]}"#,
    r#"{"kind":"steps","steps":[{"kind":"steps","steps":[{"kind":"bogus"}]}]}"#,
    r#"{"kind":"count","count":}"#,
    r#"{"kind":"count","count":3"#,
    r#"{"kind":"count","count":3,}"#,
    r#"{"kind":"count","count":3} x"#,
];

const ADJACENT: &[&str] = &[
    r#"{"kind":"empty"}"#,
    r#"{"kind":"empty","value":null}"#,
    r#"{"value":null,"kind":"empty"}"#,
    r#"{"kind":"bare"}"#,
    r#"{"kind":"bare","value":null}"#,
    r#"{"kind":"bare","value":1}"#,
    r#"{"kind":"number","value":5}"#,
    r#"{"value":5,"kind":"number"}"#,
    r#"{"kind":"number"}"#,
    r#"{"kind":"number","value":"x"}"#,
    r#"{"value":"x","kind":"number"}"#,
    r#"{"kind":"maybe"}"#,
    r#"{"kind":"maybe","value":null}"#,
    r#"{"kind":"maybe","value":"s"}"#,
    r#"{"kind":"pair","value":{"left":1,"right":2}}"#,
    r#"{"kind":"pair","value":[1,2]}"#,
    r#"{"value":[1,2],"kind":"pair"}"#,
    r#"{"kind":"pair","value":7}"#,
    r#"{"kind":"pair","value":null}"#,
    r#"{"kind":"pair"}"#,
    r#"{"kind":"pair","value":{"left":1}}"#,
    r#"{"kind":"pair","value":{"left":1,"right":2,"up":3}}"#,
    r#"{"kind":"inner","value":{"kind":"count","count":2}}"#,
    r#"{"kind":"inner","value":{"kind":"count"}}"#,
    r#"{"kind":"next","value":4}"#,
    r#"{"kind":"successor","value":4}"#,
    r#"{"kind":"number","value":5,"extra":1}"#,
    r#"{"extra":1,"kind":"number","value":5}"#,
    r#"{"kind":"number","value":5,"kind":"number"}"#,
    r#"{"kind":"number","kind":"number","value":5}"#,
    r#"{"kind":"number","value":5,"value":6}"#,
    r#"{"value":5,"value":6}"#,
    r#"{"value":5}"#,
    r"{}",
    r#"["number",5]"#,
    r#"["number"]"#,
    r"[]",
    r#"["bogus",5]"#,
    r#"[{"number":null},5]"#,
    r#"{"kind":{"number":null},"value":5}"#,
    r#"{"kind":{"number":1},"value":5}"#,
    r#"{"kind":{"bogus":null},"value":5}"#,
    r#"{"kind":7,"value":5}"#,
    r#"{"kind":"folder","value":{"steps":[{"kind":"number","value":1}]}}"#,
    r#"{"kind":"folder","value":{"steps":[{"kind":"number","value":-1}]}}"#,
    r#""number""#,
    r#"{"kind":"bogus","value":1}"#,
    r#"{"value":1,"kind":"bogus"}"#,
    r#"{"kind":"number","value":5"#,
    r#"{"kind":"number","value":}"#,
];

const LOOSE: &[&str] = &[
    r#"{"type":"a","a":1,"zz":2}"#,
    r#"{"zz":[1],"type":"a","a":1}"#,
    r#"{"type":"b","x":1,"y":2}"#,
    r#"{"type":"b","x":1,"y":2,"zz":3}"#,
    r#"{"type":"c"}"#,
];

const FLAT: &[&str] = &[
    r#"{"kind":"spread","dx":1.5,"extra":2}"#,
    r#"{"extra":2,"dx":1.5,"kind":"spread","other":true}"#,
    r#"{"kind":"spread","extra":2}"#,
    r#"["spread",1.5,2]"#,
];

const LOOSE_ADJACENT: &[&str] = &[
    r#"{"t":"a","c":1,"z":[{"deep":true}]}"#,
    r#"{"z":1,"c":1,"t":"a"}"#,
    r#"{"z":1,"t":"a"}"#,
    r#"{"t":"b","c":{"b":1}}"#,
    r#"{"t":"b","c":[1]}"#,
    r#"{"t":"a","c":1,"t":"a"}"#,
];

const GENERIC: &[&str] = &[
    r#"{"kind":"one","value":7}"#,
    r#"{"kind":"one","value":-7}"#,
    r#"{"kind":"two","value":{"a":1,"b":2}}"#,
    r#"{"kind":"two","value":{"a":1}}"#,
    r#"{"kind":"other","value":{"n":1}}"#,
];

const INFERRED: &[&str] = &[
    r#"{"kind":"one","value":7}"#,
    r#"{"kind":"one"}"#,
    r#"{"kind":"two","a":1,"b":[2,3]}"#,
    r#"{"kind":"two","a":1,"b":[-2]}"#,
    r#"{"kind":"two","a":1}"#,
];

#[test]
fn internally_tagged_enums_read_as_serde_reads_them() {
    agree::<serde_form::Internal, tagged_form::Internal>(INTERNAL, true);
    agree::<serde_form::Loose, tagged_form::Loose>(LOOSE, true);
    agree::<serde_form::Flat, tagged_form::Flat>(FLAT, true);
}

/// A generic enum without `bound = ""` reads each parameter as an owned
/// value: with serde's inferred bound, or with the bound it states.
#[test]
fn generic_enums_with_serde_bounds_read_as_serde_reads_them() {
    agree::<serde_form::Inferred<u32>, tagged_form::Inferred<u32>>(INFERRED, true);
    agree::<serde_form::Stated<u32>, tagged_form::Stated<u32>>(GENERIC, false);
}

#[test]
fn adjacently_tagged_enums_read_as_serde_reads_them() {
    agree::<serde_form::Adjacent, tagged_form::Adjacent>(ADJACENT, false);
    agree::<serde_form::LooseAdjacent, tagged_form::LooseAdjacent>(LOOSE_ADJACENT, false);
    agree::<serde_form::Generic<u8>, tagged_form::Generic<u8>>(GENERIC, false);
}

#[test]
fn a_refusal_inside_adjacent_content_states_the_position_after_the_object() {
    assert_eq!(
        read::<tagged_form::Adjacent>(r#"{"kind":"number","value":"x"}"#),
        "refused invalid type: string \"x\", expected u32 at line 1 column 29"
    );
}

#[test]
fn a_short_sequence_form_states_the_struct_variant_alone() {
    let input = r#"["count"]"#;
    assert!(read::<serde_form::Internal>(input).starts_with("refused invalid length 0"));
    assert_eq!(
        read::<tagged_form::Internal>(input),
        "refused invalid length 0, expected struct variant Internal::Count"
    );
}

#[test]
fn an_extra_sequence_element_refuses_in_serde_json_words() {
    let input = r#"["count",3,null,5]"#;
    assert!(read::<serde_form::Internal>(input).starts_with("refused "));
    assert_eq!(
        read::<tagged_form::Internal>(input),
        "refused trailing characters"
    );
}

/// `levels` steps nested in each other around one empty step.
fn nested_steps(levels: usize) -> String {
    let mut text = String::new();
    for _ in 0..levels {
        text.push_str(r#"{"kind":"steps","steps":["#);
    }
    text.push_str(r#"{"kind":"empty"}"#);
    for _ in 0..levels {
        text.push_str("]}");
    }
    text
}

/// `levels` folders nested in each other around one number.
fn nested_folders(levels: usize) -> String {
    let mut text = String::new();
    for _ in 0..levels {
        text.push_str(r#"{"kind":"folder","value":{"steps":["#);
    }
    text.push_str(r#"{"kind":"number","value":1}"#);
    for _ in 0..levels {
        text.push_str("]}}");
    }
    text
}

#[test]
fn the_depth_limit_refuses_at_the_container_serde_refuses() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let steps: Vec<String> = (58..=66).map(nested_steps).collect();
            let steps: Vec<&str> = steps.iter().map(String::as_str).collect();
            agree::<serde_form::Internal, tagged_form::Internal>(&steps, true);
            assert!(read::<tagged_form::Internal>(&nested_steps(62)).starts_with("ok"));
            assert!(read::<tagged_form::Internal>(&nested_steps(64))
                .contains("recursion limit exceeded"));
            let folders: Vec<String> = (38..=46).map(nested_folders).collect();
            let folders: Vec<&str> = folders.iter().map(String::as_str).collect();
            agree::<serde_form::Adjacent, tagged_form::Adjacent>(&folders, true);
        })
        .expect("the probe thread starts")
        .join()
        .expect("the probe thread completes");
}
