// SPDX-License-Identifier: Apache-2.0
//! Every unit variant of an enum of `body.rs` or `shared.rs` tagged by `kind`
//! has one spelling: `{"kind": name}`. The sweep finds each such enum with
//! content under `value` in those files and reads every one of its unit
//! variants.

use std::collections::BTreeMap;
use std::path::Path;

use cadmpeg_core::decode::ResourceLimits;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use syn::{Fields, Item};

use crate::design::ops::body::{Heal, InstanceChange, PatternMethod, SplitMethod, TransformMode};
use crate::design::ops::shared::{
    Alignment, InstanceOrientation, JointKind, JointMotion, Locus, PathAxisControl, PathPosition,
    PathReference, PathRollControl, RunSides, SlotPosition, WidthMode,
};
use crate::design::read;
use crate::scalar::Length;

type Reader = fn(&Value) -> Result<Value, String>;

/// Reads `value` as a `T` and writes it back.
fn reads<T: Serialize + DeserializeOwned>(value: &Value) -> Result<Value, String> {
    let read: T = read::from_str(&value.to_string(), &ResourceLimits::desktop())
        .map_err(|error| error.to_string())?;
    serde_json::to_value(read).map_err(|error| error.to_string())
}

/// The reader of each adjacently tagged enum with a unit variant in the swept
/// files, by the enum's name.
fn readers() -> BTreeMap<&'static str, Reader> {
    let readers: [(&'static str, Reader); 10] = [
        ("Alignment", reads::<Alignment>),
        ("Heal", reads::<Heal>),
        ("InstanceOrientation", reads::<InstanceOrientation>),
        ("JointKind", reads::<JointKind>),
        ("JointMotion", reads::<JointMotion>),
        ("PatternMethod", reads::<PatternMethod>),
        ("RunSides", reads::<RunSides<Length>>),
        ("SlotPosition", reads::<SlotPosition>),
        ("SplitMethod", reads::<SplitMethod>),
        ("WidthMode", reads::<WidthMode>),
    ];
    BTreeMap::from(readers)
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops");
    let mut found = BTreeMap::new();
    for file in ["body.rs", "shared.rs"] {
        let path = root.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        let parsed = syn::parse_file(&text).unwrap_or_else(|error| panic!("{path:?}: {error}"));
        adjacent_unit_variants(&parsed.items, &mut found);
    }
    let readers = readers();
    assert_eq!(
        found.keys().map(String::as_str).collect::<Vec<_>>(),
        readers.keys().copied().collect::<Vec<_>>(),
        "every adjacently tagged enum with a unit variant has a reader here"
    );
    for (name, variants) in &found {
        let read = readers[name.as_str()];
        for variant in variants {
            let bare = json!({"kind": variant});
            assert_eq!(read(&bare).as_ref(), Ok(&bare), "{name}::{variant}");
            for stated in [Value::Null, json!({}), json!([]), json!(0), json!("")] {
                let error = read(&json!({"kind": variant, "value": stated}))
                    .expect_err(&format!("{name}::{variant} with value {stated} is refused"));
                assert!(
                    error.contains("does not state value"),
                    "{name}::{variant}: {error}"
                );
            }
        }
    }
}

/// A unit variant of an enum tagged by `kind` alone is `{"kind": name}` and
/// refuses every other key, `value` among them.
#[test]
fn every_internally_tagged_unit_variant_refuses_another_key() {
    let units: [(&str, Reader); 7] = [
        ("centroid", reads::<Locus>),
        ("plane_normal", reads::<PathReference>),
        ("skipped", reads::<InstanceChange>),
        ("move", reads::<TransformMode>),
        ("free", reads::<PathPosition>),
        ("free", reads::<PathAxisControl>),
        ("free", reads::<PathRollControl>),
    ];
    for (variant, read) in units {
        let bare = json!({"kind": variant});
        assert_eq!(read(&bare).as_ref(), Ok(&bare), "{variant}");
        for stated in [
            json!({"kind": variant, "value": null}),
            json!({"kind": variant, "value": {}}),
            json!({"kind": variant, "zz_bogus": 1}),
        ] {
            read(&stated).expect_err(&format!("{stated} is refused"));
        }
    }
}
