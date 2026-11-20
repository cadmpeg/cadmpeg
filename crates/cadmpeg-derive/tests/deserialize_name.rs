// SPDX-License-Identifier: Apache-2.0
//! A data-free enum that derives `DeserializeName` reads its variant's name
//! and refuses every other spelling, read directly or from buffered content.

use cadmpeg_derive::DeserializeName;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Base {
    /// The parent's frame.
    Parent,
    #[serde(rename = "world_origin")]
    World,
    SemiFinish = 7,
}

#[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Holder {
    Placed { base: Base },
}

#[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Direct {
    base: Base,
}

macro_rules! labels {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
        #[serde(deny_unknown_fields)]
        enum $name {
            $(
                #[serde(rename = $label)]
                $variant,
            )*
        }
    };
}

labels! {
    /// Labels stated through a template.
    Label {
        Stored => "stored-verbatim",
        Other => "other",
    }
}

const SECOND_SPELLINGS: [&str; 9] = [
    r#"{"parent": null}"#,
    r#"{"parent": {}}"#,
    r#"{"parent": []}"#,
    r#"{"parent": 1}"#,
    r#"{"parent": null, "world_origin": null}"#,
    "null",
    "0",
    r#"["parent"]"#,
    "true",
];

#[test]
fn a_data_free_enum_reads_its_variant_name() {
    for (base, name) in [
        (Base::Parent, "parent"),
        (Base::World, "world_origin"),
        (Base::SemiFinish, "semi_finish"),
    ] {
        assert_eq!(serde_json::to_value(base).expect("writes"), json!(name));
        assert_eq!(
            serde_json::from_str::<Base>(&format!("{name:?}")).expect("the name reads"),
            base
        );
        assert_eq!(
            serde_json::from_value::<Base>(json!(name)).expect("the name value reads"),
            base
        );
    }
    for unknown in ["\"world\"", "\"Parent\"", "\"\""] {
        serde_json::from_str::<Base>(unknown).expect_err("a name no variant has is refused");
    }
}

#[test]
fn a_data_free_enum_refuses_every_other_spelling() {
    for text in SECOND_SPELLINGS {
        serde_json::from_str::<Base>(text).expect_err(text);
        let value: Value = serde_json::from_str(text).expect("probe JSON");
        serde_json::from_value::<Base>(value).expect_err(text);
        serde_json::from_str::<Direct>(&format!(r#"{{"base": {text}}}"#)).expect_err(text);
    }
}

#[test]
fn a_macro_template_reads_the_names_its_fragments_state() {
    assert_eq!(
        serde_json::from_str::<Label>(r#""stored-verbatim""#).expect("the renamed name reads"),
        Label::Stored
    );
    assert_eq!(
        serde_json::from_str::<Label>(r#""other""#).expect("the name reads"),
        Label::Other
    );
    for text in [r#""stored""#, r#"{"other": null}"#] {
        serde_json::from_str::<Label>(text).expect_err(text);
    }
}

#[test]
fn a_data_free_enum_in_buffered_content_refuses_every_other_spelling() {
    assert_eq!(
        serde_json::from_str::<Holder>(r#"{"kind": "placed", "base": "parent"}"#)
            .expect("the name reads inside a tagged variant"),
        Holder::Placed { base: Base::Parent }
    );
    for text in SECOND_SPELLINGS {
        serde_json::from_str::<Holder>(&format!(r#"{{"kind": "placed", "base": {text}}}"#))
            .expect_err(text);
    }
}
