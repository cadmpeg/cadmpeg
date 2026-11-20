// SPDX-License-Identifier: Apache-2.0
//! Every unit variant of a process enum tagged by `kind` with content under
//! `value` has one spelling: `{"kind": name}`. The sweep finds each such enum
//! in the process module's source and reads every one of its unit variants.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use syn::{Fields, Item};

use super::read_desktop;
use crate::process::cycle::CycleKind;
use crate::process::groups::{GroupKind, ProcessDefinition};
use crate::process::limits::{AxialFrom, HeightFrom, RadiusFrom};
use crate::process::operation::{GeometryRole, OperationClass};
use crate::process::settings::{PlungeKind, RetractPolicy};
use crate::process::setup::{JobKind, NcFunction, WorkOffset};
use crate::process::stock::{WcsOrientation, WcsOrigin};
use crate::process::technology::{Compensation, Coolant, CutMode, FeedRole, JetCut, RestSource};
use crate::process::toolpath::Motion;

type Reader = fn(&str) -> Result<Value, String>;

/// Reads `text` as a `T` and writes it back.
fn reads<T: Serialize + DeserializeOwned>(text: &str) -> Result<Value, String> {
    let read: T = read_desktop(text).map_err(|error| error.to_string())?;
    serde_json::to_value(read).map_err(|error| error.to_string())
}

/// The reader of each adjacently tagged process enum with a unit variant, by
/// the enum's name.
fn readers() -> BTreeMap<&'static str, Reader> {
    let readers: [(&'static str, Reader); 22] = [
        ("AxialFrom", reads::<AxialFrom>),
        ("Compensation", reads::<Compensation>),
        ("Coolant", reads::<Coolant>),
        ("CutMode", reads::<CutMode>),
        ("CycleKind", reads::<CycleKind>),
        ("FeedRole", reads::<FeedRole>),
        ("GeometryRole", reads::<GeometryRole>),
        ("GroupKind", reads::<GroupKind>),
        ("HeightFrom", reads::<HeightFrom>),
        ("JetCut", reads::<JetCut>),
        ("JobKind", reads::<JobKind>),
        ("Motion", reads::<Motion>),
        ("NcFunction", reads::<NcFunction>),
        ("OperationClass", reads::<OperationClass>),
        ("PlungeKind", reads::<PlungeKind>),
        ("ProcessDefinition", reads::<ProcessDefinition>),
        ("RadiusFrom", reads::<RadiusFrom>),
        ("RestSource", reads::<RestSource>),
        ("RetractPolicy", reads::<RetractPolicy>),
        ("WcsOrientation", reads::<WcsOrientation>),
        ("WcsOrigin", reads::<WcsOrigin>),
        ("WorkOffset", reads::<WorkOffset>),
    ];
    BTreeMap::from(readers)
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "tests") {
                sources(&path, out);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn serde_args(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("serde"))
        .filter_map(|attr| attr.meta.require_list().ok())
        .map(|list| list.tokens.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The wire name of a variant under `rename_all = "snake_case"`.
fn snake_case(ident: &str) -> String {
    let mut out = String::new();
    for (i, c) in ident.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

fn adjacent_unit_variants(items: &[Item], out: &mut BTreeMap<String, Vec<String>>) {
    for item in items {
        match item {
            Item::Enum(item) => {
                let args = serde_args(&item.attrs);
                if !args.contains("content = \"value\"") {
                    continue;
                }
                assert!(
                    args.contains("rename_all = \"snake_case\""),
                    "{} names its variants in snake case",
                    item.ident
                );
                let units: Vec<String> = item
                    .variants
                    .iter()
                    .filter(|variant| matches!(variant.fields, Fields::Unit))
                    .map(|variant| snake_case(&variant.ident.to_string()))
                    .collect();
                if !units.is_empty() {
                    out.insert(item.ident.to_string(), units);
                }
            }
            Item::Mod(module) => {
                let test_only = module.attrs.iter().any(|attr| attr.path().is_ident("cfg"));
                if let (Some((_, items)), false) = (&module.content, test_only) {
                    adjacent_unit_variants(items, out);
                }
            }
            _ => {}
        }
    }
}

#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/process");
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut found = BTreeMap::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let parsed = syn::parse_file(&text).unwrap_or_else(|error| panic!("{file:?}: {error}"));
        adjacent_unit_variants(&parsed.items, &mut found);
    }
    let readers = readers();
    assert_eq!(
        found.keys().map(String::as_str).collect::<Vec<_>>(),
        readers.keys().copied().collect::<Vec<_>>(),
        "every adjacently tagged process enum with a unit variant has a reader here"
    );
    for (name, variants) in &found {
        let read = readers[name.as_str()];
        for variant in variants {
            let bare = json!({"kind": variant});
            assert_eq!(
                read(&bare.to_string()).as_ref(),
                Ok(&bare),
                "{name}::{variant}"
            );
            for stated in [
                Value::Null,
                json!({}),
                json!([]),
                json!(0),
                json!(false),
                json!(""),
            ] {
                for text in [
                    format!(r#"{{"kind": "{variant}", "value": {stated}}}"#),
                    format!(r#"{{"value": {stated}, "kind": "{variant}"}}"#),
                ] {
                    let error =
                        read(&text).expect_err(&format!("{name}::{variant} as {text} is refused"));
                    assert!(
                        error.contains("does not state value"),
                        "{name}::{variant}: {error}"
                    );
                }
            }
        }
    }
}
