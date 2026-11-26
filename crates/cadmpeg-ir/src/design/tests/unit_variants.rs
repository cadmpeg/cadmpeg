// SPDX-License-Identifier: Apache-2.0
//! Every unit variant of an enum tagged by `kind` with content under `value`
//! has one spelling: `{"kind": name}`. The sweep finds each such enum in the
//! design module's source and reads every one of its unit variants. Each
//! operation family under `ops` sweeps its own files with
//! [`assert_units_refuse_value`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use syn::{Fields, Item};

use super::read_desktop;
use crate::design::configuration::{ArrangementKind, PartNumber};
use crate::design::construction::Relation;
use crate::design::container::Rollback;
use crate::design::dimension::ParameterRef;
use crate::design::evaluation::StateBase;
use crate::design::expr::{ExpressionDialect, MeasureKind, Node};
use crate::design::feature::{FeatureOperation, FuzzyTolerance};
use crate::design::names::GenerationRole;
use crate::design::options::{
    BooleanOp, CornerRound, ProjectionDirection, SectionCondition, ThinEnds, WallSide,
};
use crate::design::parameter::{ParameterDefinition, Tolerance};
use crate::design::program::ProgramStage;
use crate::design::selection::{
    Anchor, Continuity, Ordered, PickTarget, Picks, Propagation, SelectionRule,
};
use crate::design::units::ValueKind;

pub(in crate::design) type Reader = fn(&Value) -> Result<Value, String>;

/// Reads `value` as a `T` and writes it back.
pub(in crate::design) fn reads<T: Serialize + DeserializeOwned>(
    value: &Value,
) -> Result<Value, String> {
    let read: T = read_desktop(&value.to_string()).map_err(|error| error.to_string())?;
    cadmpeg_core::serde_erase::json::to_value(read).map_err(|error| error.to_string())
}

/// The reader of each adjacently tagged design enum with a unit variant, by
/// the enum's name.
fn readers() -> BTreeMap<&'static str, Reader> {
    let readers: [(&'static str, Reader); 27] = [
        ("Anchor", reads::<Anchor>),
        ("ArrangementKind", reads::<ArrangementKind>),
        ("BooleanOp", reads::<BooleanOp>),
        ("Continuity", reads::<Continuity>),
        ("CornerRound", reads::<CornerRound>),
        ("ExpressionDialect", reads::<ExpressionDialect>),
        ("FeatureOperation", reads::<FeatureOperation>),
        ("FuzzyTolerance", reads::<FuzzyTolerance>),
        ("GenerationRole", reads::<GenerationRole>),
        ("MeasureKind", reads::<MeasureKind>),
        ("Node", reads::<Node<ParameterRef>>),
        ("ParameterDefinition", reads::<ParameterDefinition>),
        ("PartNumber", reads::<PartNumber>),
        ("PickTarget", reads::<PickTarget>),
        ("Picks", reads::<Picks<Ordered>>),
        ("ProgramStage", reads::<ProgramStage>),
        ("ProjectionDirection", reads::<ProjectionDirection>),
        ("Propagation", reads::<Propagation>),
        ("Relation", reads::<Relation>),
        ("Rollback", reads::<Rollback>),
        ("SectionCondition", reads::<SectionCondition>),
        ("SelectionRule", reads::<SelectionRule>),
        ("StateBase", reads::<StateBase>),
        ("ThinEnds", reads::<ThinEnds>),
        ("Tolerance", reads::<Tolerance>),
        ("ValueKind", reads::<ValueKind>),
        ("WallSide", reads::<WallSide>),
    ];
    BTreeMap::from(readers)
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name != "tests" && name != "ops")
            {
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

/// Finds every adjacently tagged enum with a unit variant in `files`,
/// requires a reader in `readers` for each and for nothing else, and requires
/// each unit variant to read as its kind alone and refuse a stated `value`.
pub(in crate::design) fn assert_units_refuse_value(
    files: &[PathBuf],
    readers: &BTreeMap<&'static str, Reader>,
) {
    let mut found = BTreeMap::new();
    for file in files {
        let text = std::fs::read_to_string(file).unwrap();
        let parsed = syn::parse_file(&text).unwrap_or_else(|error| panic!("{file:?}: {error}"));
        adjacent_unit_variants(&parsed.items, &mut found);
    }
    assert_eq!(
        found.keys().map(String::as_str).collect::<Vec<_>>(),
        readers.keys().copied().collect::<Vec<_>>(),
        "every adjacently tagged enum with a unit variant in {files:?} has a reader here"
    );
    for (name, variants) in &found {
        let read = readers[name.as_str()];
        for variant in variants {
            let bare = json!({"kind": variant});
            assert_eq!(read(&bare).as_ref(), Ok(&bare), "{name}::{variant}");
            for stated in [Value::Null, json!({}), json!([])] {
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

#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design");
    let mut files = Vec::new();
    sources(&root, &mut files);
    assert_units_refuse_value(&files, &readers());
}
