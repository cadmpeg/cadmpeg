// SPDX-License-Identifier: Apache-2.0
//! Fixture trees for the declaration index and the optional-field census.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::optional_absence_census;

/// A source tree holding the three wire crates, removed when dropped.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cadmpeg-source-census-{name}-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("clear a stale fixture");
        }
        let mut files: Vec<(&str, String)> = files
            .iter()
            .map(|(path, text)| (*path, (*text).to_owned()))
            .collect();
        let mut defaults = vec![
            ("crates/cadmpeg-core/src/lib.rs", String::new()),
            ("crates/cadmpeg-asm/src/lib.rs", String::new()),
            ("Cargo.toml", WORKSPACE_MANIFEST.to_owned()),
        ];
        for (manifest, package) in [
            ("crates/cadmpeg-ir/Cargo.toml", "cadmpeg-ir"),
            ("crates/cadmpeg-core/Cargo.toml", "cadmpeg-core"),
            ("crates/cadmpeg-asm/Cargo.toml", "cadmpeg-asm"),
        ] {
            defaults.push((manifest, crate_manifest(package)));
        }
        for (path, text) in defaults {
            if !files.iter().any(|(stated, _)| *stated == path) {
                files.push((path, text));
            }
        }
        for (path, text) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().expect("a fixture file has a parent"))
                .expect("create a fixture directory");
            std::fs::write(&file, text).expect("write a fixture file");
        }
        Self { root }
    }

    fn offenders(&self) -> Vec<String> {
        optional_absence_census(&self.root).1
    }
}

/// The workspace a fixture's crates belong to: each wire crate a reader path
/// can name, and the registry crates its paths spell.
const WORKSPACE_MANIFEST: &str = r#"[workspace]
[workspace.dependencies]
cadmpeg-ir = { path = "crates/cadmpeg-ir" }
cadmpeg-core = { path = "crates/cadmpeg-core" }
cadmpeg-asm = { path = "crates/cadmpeg-asm" }
serde = "1"
serde_json = "1"
serde_bytes = "0.11"
"#;

/// A wire crate's manifest, depending on every other crate the workspace states.
fn crate_manifest(package: &str) -> String {
    [
        "cadmpeg-ir",
        "cadmpeg-core",
        "cadmpeg-asm",
        "serde",
        "serde_json",
        "serde_bytes",
    ]
    .into_iter()
    .filter(|dependency| *dependency != package)
    .fold(
        format!("[package]\nname = \"{package}\"\n[dependencies]\n"),
        |mut manifest, dependency| {
            writeln!(manifest, "{dependency}.workspace = true")
                .expect("writing to a String does not fail");
            manifest
        },
    )
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            eprintln!("remove {}: {error}", self.root.display());
        }
    }
}

const NULLABLE_NAME: &str = r#"
        #[derive(Serialize, Deserialize)]
        pub struct Sketch {
            #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
            pub name: Option<String>,
        }
    "#;

#[test]
fn same_named_types_in_two_modules_are_each_their_own_writer() {
    let fixture = Fixture::new(
        "same-named-writers",
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod sketches; mod design;"),
            ("crates/cadmpeg-ir/src/sketches.rs", NULLABLE_NAME),
            (
                "crates/cadmpeg-ir/src/design.rs",
                &NULLABLE_NAME.replace(
                    "#[serde(deserialize_with",
                    "#[serde(skip_serializing_if = \"Option::is_none\", deserialize_with",
                ),
            ),
        ],
    );
    let offenders = fixture.offenders();
    assert_eq!(offenders.len(), 1, "{offenders:#?}");
    assert!(
        offenders[0].starts_with("crates/cadmpeg-ir/src/design.rs:")
            && offenders[0]
                .contains("admits null, but cadmpeg_ir::design::Sketch skips the key for None"),
        "{offenders:#?}"
    );
}

/// A field reader named by `cadmpeg_core::` is that crate's reader only
/// where nothing in the reading crate binds the root otherwise. A
/// `named_optional_field!` shim's macro path is resolved the same way.
#[test]
fn a_helper_whose_root_binds_another_item_states_no_spelling() {
    let local = "mod other { pub mod absent_key { pub fn nullable<'de, D, T>(_: D) -> Result<Option<T>, D::Error> where D: serde::Deserializer<'de> { Ok(None) } } }\n";
    let renamed = "[package]\nname = \"cadmpeg-ir\"\n[dependencies]\ncadmpeg_core = { package = \"evil\", path = \"../evil\" }\n";
    let moved =
        "[package]\nname = \"cadmpeg-ir\"\n[dependencies]\ncadmpeg-core = { path = \"../evil\" }\n";
    for (case, prefix, manifest) in [
        (
            "a local module",
            format!("mod cadmpeg_core {{ {local} }}"),
            None,
        ),
        (
            "a use rename",
            format!("{local} use other as cadmpeg_core;"),
            None,
        ),
        (
            "an extern crate rename",
            "extern crate serde_json as cadmpeg_core;".to_owned(),
            None,
        ),
        (
            "a macro that declares the module it is passed",
            "macro_rules! module { ($name:ident) => { mod $name {} }; } module!(cadmpeg_core);"
                .to_owned(),
            None,
        ),
        (
            "a glob from outside the crate",
            "use serde_json::*;".to_owned(),
            None,
        ),
        (
            "a dependency renamed to the root",
            String::new(),
            Some(renamed),
        ),
        (
            "a dependency on another directory",
            String::new(),
            Some(moved),
        ),
    ] {
        let source = format!("{prefix}\n{NULLABLE_NAME}");
        let mut files = vec![("crates/cadmpeg-ir/src/lib.rs", source.as_str())];
        files.extend(manifest.map(|manifest| ("crates/cadmpeg-ir/Cargo.toml", manifest)));
        let offenders = Fixture::new("shadowed-helper-root", &files).offenders();
        assert!(
            offenders.iter().any(|offender| offender.contains(
                "Sketch.name states the unlisted helper cadmpeg_core::absent_key::nullable"
            )),
            "{case}: {offenders:#?}"
        );
    }
    let offenders = lib("unshadowed-helper-root", NULLABLE_NAME).offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");
    let fixture = Fixture::new(
        "shadowed-shim-root",
        &[("crates/cadmpeg-ir/src/lib.rs", "mod cadmpeg_core {}")],
    );
    let declarations = super::Declarations::read(&fixture.root);
    assert!(!declarations.names_external(
        "crates/cadmpeg-ir/src/lib.rs",
        &[],
        "cadmpeg_core::named_optional_field",
        &["cadmpeg_core", "named_optional_field"],
    ));
}

const ROUTED: &str = r#"
        #[derive(Deserialize)]
        struct SketchWire {
            #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
            name: Option<String>,
        }
        #[derive(Serialize, Deserialize)]
        #[serde(try_from = "SketchWire")]
        pub struct Sketch {
            SKIP
            pub name: Option<String>,
        }
    "#;

#[test]
fn each_wire_is_paired_with_the_owner_in_its_own_module() {
    let fixture = Fixture::new(
        "same-named-wires",
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod sketches; mod design;"),
            (
                "crates/cadmpeg-ir/src/sketches.rs",
                &ROUTED.replace(
                    "SKIP",
                    "#[serde(skip_serializing_if = \"Option::is_none\")]",
                ),
            ),
            (
                "crates/cadmpeg-ir/src/design.rs",
                &ROUTED.replace("SKIP", ""),
            ),
        ],
    );
    let offenders = fixture.offenders();
    assert_eq!(offenders.len(), 1, "{offenders:#?}");
    assert!(
        offenders[0].starts_with("crates/cadmpeg-ir/src/sketches.rs:")
            && offenders[0]
                .contains("SketchWire.name admits null, but cadmpeg_ir::sketches::Sketch skips"),
        "{offenders:#?}"
    );
}

#[test]
fn a_conversion_that_resolves_nowhere_leaves_its_wire_unpaired() {
    let fixture = Fixture::new(
        "unresolved-wire",
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod design;"),
            (
                "crates/cadmpeg-ir/src/design.rs",
                &ROUTED.replace("SKIP", "").replace(
                    "try_from = \"SketchWire\"",
                    "try_from = \"elsewhere::SketchWire\"",
                ),
            ),
        ],
    );
    let offenders = fixture.offenders();
    assert_eq!(offenders.len(), 1, "{offenders:#?}");
    assert!(
            offenders[0].contains("SketchWire.name cadmpeg_ir::design::SketchWire may be read for cadmpeg_ir::design::Sketch"),
            "{offenders:#?}"
        );
}

const SPACE: &str = r"
        pub trait Space {
            type Point;
        }
        pub struct Planar;
        impl Space for Planar {
            type Point = PLANAR;
        }
        pub struct Spatial;
        impl Space for Spatial {
            type Point = u8;
        }
        #[derive(Serialize, Deserialize)]
        pub struct Entity<S: Space> {
            pub point: S::Point,
        }
    ";

fn space(name: &str, source: &str) -> Fixture {
    Fixture::new(
        name,
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod space;"),
            ("crates/cadmpeg-ir/src/space.rs", source),
        ],
    )
}

#[test]
fn an_associated_type_no_impl_states_as_an_option_is_no_optional_key() {
    let offenders = space("associated-plain", &SPACE.replace("PLANAR", "u16")).offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn an_associated_type_one_impl_states_as_an_option_is_an_optional_key() {
    for source in [
        SPACE.replace("PLANAR", "Option<u16>"),
        SPACE
            .replace("PLANAR", "Option<u16>")
            .replace("S::Point", "<S as Space>::Point"),
        SPACE
            .replace("PLANAR", "Option<u16>")
            .replace("Entity<S: Space> {", "Entity<S> where S: Space {"),
    ] {
        let offenders = space("associated-option", &source).offenders();
        assert_eq!(offenders.len(), 1, "{offenders:#?}");
        assert!(
            offenders[0].starts_with("crates/cadmpeg-ir/src/space.rs:")
                && offenders[0].contains("Entity.point states no deserialize_with"),
            "{offenders:#?}"
        );
    }
}

#[test]
fn an_associated_type_the_census_cannot_read_is_an_offender() {
    let plain = SPACE.replace("PLANAR", "u16");
    for (source, reason) in [
        (
            plain.replace("type Point = u8;", ""),
            "impl Space for Spatial at crates/cadmpeg-ir/src/space.rs:",
        ),
        (
            plain.replace("Entity<S: Space>", "Entity<S>"),
            "S::Point names a type parameter with no trait bound that declares Point",
        ),
        (
            plain.replace("Entity<S: Space>", "Entity<S: Space + external::Mystery>"),
            "the bound external::Mystery of S is no wire-crate trait",
        ),
        (
            plain.replace("type Point;", "type Point = u8;"),
            "cadmpeg_ir::space::Space::Point states a default",
        ),
        (
            plain.replace("type Point = u8;", "type Point = Self::Other;"),
            "the type Self::Other depends on what fills the impl",
        ),
        (
            plain
                .replace(
                    "impl Space for Planar {\n            type Point = u16;\n        }",
                    "",
                )
                .replace(
                    "impl Space for Spatial {\n            type Point = u8;\n        }",
                    "",
                ),
            "cadmpeg_ir::space::Space has no impl in the wire crates",
        ),
    ] {
        let offenders = space("associated-unread", &source).offenders();
        assert_eq!(offenders.len(), 1, "{reason}: {offenders:#?}");
        assert!(offenders[0].contains(reason), "{reason}: {offenders:#?}");
    }
}

#[test]
fn an_impl_outside_the_wire_crates_leaves_the_associated_type_unread() {
    let fixture = Fixture::new(
        "associated-foreign",
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod space;"),
            (
                "crates/cadmpeg-ir/src/space.rs",
                &SPACE.replace("PLANAR", "u16"),
            ),
            (
                "crates/codec/src/lib.rs",
                "pub struct Codec; impl cadmpeg_ir::space::Space for Codec { type Point = u8; }",
            ),
        ],
    );
    let offenders = fixture.offenders();
    assert_eq!(offenders.len(), 1, "{offenders:#?}");
    assert!(
        offenders[0]
            .contains("an impl of Space at crates/codec/src/lib.rs:1 is outside the wire crates"),
        "{offenders:#?}"
    );
}

#[test]
fn a_macro_call_naming_the_trait_leaves_its_impls_unread() {
    let source = SPACE.replace("PLANAR", "u16")
        + r"
                macro_rules! implement {
                    ($trait:ident, $name:ident) => {
                        impl $trait for $name { type Point = Option<u8>; }
                    };
                }
                pub struct Extra;
                implement!(Space, Extra);
            ";
    let offenders = space("associated-macro", &source).offenders();
    assert_eq!(offenders.len(), 1, "{offenders:#?}");
    assert!(
        offenders[0].contains("the call to implement! at crates/cadmpeg-ir/src/space.rs:"),
        "{offenders:#?}"
    );
}

fn assert_one_offender(fixture: &Fixture, reason: &str) {
    let offenders = fixture.offenders();
    assert_eq!(offenders.len(), 1, "{reason}: {offenders:#?}");
    assert!(offenders[0].contains(reason), "{reason}: {offenders:#?}");
}

#[test]
fn an_alias_over_an_associated_type_holds_what_the_impls_state() {
    for (planar, alias) in [
        ("Option<u16>", "pub type PointOf<S: Space> = S::Point;"),
        ("u16", "pub type PointOf<S: Space> = Option<S::Point>;"),
        ("u16", "pub type PointOf<S> = Option<<S as Space>::Point>;"),
    ] {
        let source = SPACE
            .replace("PLANAR", planar)
            .replace("S::Point,", "PointOf<S>,")
            + alias;
        assert_one_offender(
            &space("alias-associated", &source),
            "Entity.point states no deserialize_with",
        );
    }
    let plain = SPACE
        .replace("PLANAR", "u16")
        .replace("S::Point,", "PointOf<S>,")
        + "pub type PointOf<S: Space> = S::Point;";
    let offenders = space("alias-associated-plain", &plain).offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn an_alias_of_an_option_is_an_optional_key_wherever_it_is_named() {
    let field = SPACE
        .replace("PLANAR", "u16")
        .replace("S::Point,", "Maybe,")
        + "pub type Maybe = Option<u8>;";
    assert_one_offender(
        &space("alias-option-field", &field),
        "Entity.point states no deserialize_with",
    );
    let stated = SPACE.replace("PLANAR", "Maybe") + "pub type Maybe = Option<u8>;";
    assert_one_offender(
        &space("alias-option-impl", &stated),
        "Entity.point states no deserialize_with",
    );
    let fixture = Fixture::new(
        "alias-option-reexported",
        &[
            (
                "crates/cadmpeg-ir/src/lib.rs",
                "mod space; mod api; mod kinds;",
            ),
            (
                "crates/cadmpeg-ir/src/space.rs",
                &SPACE
                    .replace("PLANAR", "u16")
                    .replace("S::Point,", "crate::api::Maybe,"),
            ),
            (
                "crates/cadmpeg-ir/src/api.rs",
                "pub use crate::kinds::Maybe;",
            ),
            (
                "crates/cadmpeg-ir/src/kinds.rs",
                "pub type Maybe = Option<u8>;",
            ),
        ],
    );
    assert_one_offender(&fixture, "Entity.point states no deserialize_with");
}

#[test]
fn an_alias_that_names_its_parameter_holds_the_argument() {
    let source = SPACE
        .replace("PLANAR", "u16")
        .replace("S::Point,", "Same<Option<u8>>,")
        + "pub type Same<T> = T;";
    assert_one_offender(
        &space("alias-parameter", &source),
        "Entity.point states no deserialize_with",
    );
    let source = SPACE
        .replace("PLANAR", "u16")
        .replace("S::Point,", "Same<u8>,")
        + "pub type Same<T> = T;";
    let offenders = space("alias-parameter-plain", &source).offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");
    let forwarded = SPACE
        .replace("PLANAR", "u16")
        .replace("S::Point,", "Outer<Option<u8>>,")
        + "pub type Same<T> = T; pub type Outer<T> = Same<T>;";
    assert_one_offender(
        &space("alias-forwarded", &forwarded),
        "cadmpeg_ir::space::Outer passes its parameters on to another alias",
    );
}

#[test]
fn an_impl_anywhere_outside_the_wire_sources_leaves_the_type_unread() {
    for path in ["examples/demo/src/lib.rs", "examples/demo.rs", "build.rs"] {
        let fixture = Fixture::new(
                "associated-examples",
                &[
                    ("crates/cadmpeg-ir/src/lib.rs", "mod space;"),
                    ("crates/cadmpeg-ir/src/space.rs", &SPACE.replace("PLANAR", "u16")),
                    (
                        path,
                        "pub struct Demo; impl cadmpeg_ir::space::Space for Demo { type Point = Option<u8>; }",
                    ),
                ],
            );
        assert_one_offender(
            &fixture,
            &format!("an impl of Space at {path}:1 is outside the wire crates"),
        );
    }
}

#[test]
fn a_foreign_call_to_an_implementing_macro_leaves_the_type_unread() {
    let fixture = Fixture::new(
            "associated-foreign-macro",
            &[
                ("crates/cadmpeg-ir/src/lib.rs", "mod space;"),
                ("crates/cadmpeg-ir/src/space.rs", &SPACE.replace("PLANAR", "u16")),
                (
                    "crates/codec/src/macros.rs",
                    "macro_rules! implement { ($t:path, $n:ident) => { impl $t for $n { type Point = Option<u8>; } }; }",
                ),
                (
                    "crates/codec/src/lib.rs",
                    "pub struct Codec;\nfn expand() { implement!(cadmpeg_ir::space::Space, Codec); }",
                ),
            ],
        );
    assert_one_offender(
        &fixture,
        "the call to implement! at crates/codec/src/lib.rs:2, which names Space",
    );
}

#[test]
fn build_output_hidden_and_test_trees_hold_no_impl() {
    let impl_text =
        "pub struct Demo; impl cadmpeg_ir::space::Space for Demo { type Point = Option<u8>; }";
    let fixture = Fixture::new(
        "associated-ignored-trees",
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod space;"),
            (
                "crates/cadmpeg-ir/src/space.rs",
                &SPACE.replace("PLANAR", "u16"),
            ),
            ("target/debug/build/out.rs", impl_text),
            (".git/hooks/demo.rs", impl_text),
            ("tests/demo.rs", impl_text),
            ("crates/codec/tests/demo.rs", impl_text),
        ],
    );
    let offenders = fixture.offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn a_same_named_trait_in_another_module_is_another_trait() {
    let fixture = Fixture::new(
            "associated-same-named-trait",
            &[
                ("crates/cadmpeg-ir/src/lib.rs", "mod space; mod other;"),
                ("crates/cadmpeg-ir/src/space.rs", &SPACE.replace("PLANAR", "u16")),
                (
                    "crates/cadmpeg-ir/src/other.rs",
                    "pub trait Space { type Point; } pub struct Loose; impl Space for Loose { type Point = Option<u8>; }",
                ),
            ],
        );
    let offenders = fixture.offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");
}

fn lib(name: &str, source: &str) -> Fixture {
    Fixture::new(name, &[("crates/cadmpeg-ir/src/lib.rs", source)])
}

#[test]
fn a_field_typed_by_a_bare_parameter_holds_what_fills_it() {
    for source in [
        "#[derive(Serialize, Deserialize)] pub struct Slot<T = Option<u16>> { pub value: T }",
        "#[derive(Serialize, Deserialize)] pub struct Slot<T> { pub value: T }
         #[derive(Serialize, Deserialize)] pub struct Host { pub nested: Slot<Option<u16>> }",
        "#[derive(Serialize, Deserialize)] pub struct Slot<T> { pub value: T }
         #[derive(Serialize, Deserialize)] pub struct Host { pub nested: Slot::<Option<u16>> }",
        "#[derive(Serialize, Deserialize)] pub struct Slot<T> { pub value: T }
         #[derive(Serialize, Deserialize)] pub struct Host<U> { pub nested: Slot<U> }
         #[derive(Serialize, Deserialize)] pub struct Doc { pub host: Host<Option<u16>> }",
    ] {
        assert_one_offender(
            &lib("bare-parameter", source),
            "Slot.value states no deserialize_with",
        );
    }
    let source = "pub type Id<T> = T;
        #[derive(Serialize, Deserialize)] pub struct Host<T> { pub point: Id<T> }
        #[derive(Serialize, Deserialize)] pub struct Doc { pub host: Host<Option<u16>> }";
    assert_one_offender(
        &lib("bare-through-alias", source),
        "Host.point states no deserialize_with",
    );
    let plain = "#[derive(Serialize, Deserialize)] pub struct Slot<T> { pub value: T }
        #[derive(Serialize, Deserialize)] pub struct Host { pub nested: Slot<u16> }";
    let offenders = lib("bare-parameter-plain", plain).offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn a_bare_parameter_the_census_cannot_enumerate_is_an_offender() {
    for (source, reason) in [
        (
            "#[derive(Serialize, Deserialize)] pub struct Slot<T> { pub value: T }",
            "cadmpeg_ir::Slot has no default and no instantiation in the wire crates for its parameter T",
        ),
        (
            "#[derive(Serialize, Deserialize)] pub struct Slot<T = u8> { pub value: T }
             #[derive(Serialize, Deserialize)] pub struct Host { pub nested: elsewhere::Slot<u8> }",
            "the use of elsewhere::Slot at crates/cadmpeg-ir/src/lib.rs:2 resolves to no one declaration",
        ),
        (
            "#[derive(Serialize, Deserialize)] pub struct Slot<T = u8> { pub value: T }
             macro_rules! host { ($t:ty) => { #[derive(Serialize, Deserialize)] pub struct Host { pub nested: $t } }; }
             host!(Slot<Option<u16>>);",
            "the call to host! at crates/cadmpeg-ir/src/lib.rs:3 names Slot, and the census does not expand it",
        ),
        (
            "#[derive(Serialize, Deserialize)] pub struct Slot<T = u8> { pub value: T }
             pub trait Space { type Point; }
             impl<U> Space for Vec<U> { type Point = Slot<U>; }",
            "the impl at crates/cadmpeg-ir/src/lib.rs fills U with what fills its self type, which the census does not read",
        ),
    ] {
        let offenders = lib("bare-parameter-unread", source).offenders();
        assert!(
            offenders.iter().any(|offender| offender.contains("Slot.value") && offender.contains(reason)),
            "{reason}: {offenders:#?}"
        );
    }
}

/// `Slot<T>` beside one safe instantiation, `Plain { nested: Slot<u16> }`.
const SLOT_BESIDE_A_SAFE_USE: &str =
    "#[derive(Serialize, Deserialize)] pub struct Slot<T> { pub value: T }
    #[derive(Serialize, Deserialize)] pub struct Plain { pub nested: Slot<u16> }
";

#[test]
fn an_instantiation_outside_a_declaration_is_read_beside_a_safe_one() {
    for written in [
        "pub fn read() -> Slot<Option<u16>> { todo!() }",
        "pub fn take(value: Slot<Option<u16>>) {}",
        "pub fn read<F: Fn() -> Slot<Option<u16>>>(f: F) {}",
        "pub struct Bag<const N: usize>; impl Bag<1> { pub fn value() -> Slot<Option<u16>> { todo!() } }",
        "pub trait Read { fn read(text: &str) -> Slot<Option<u16>>; }",
        "pub static READ: Option<Slot<Option<u16>>> = None;",
        "pub const READ: fn() -> Slot<Option<u16>> = next;",
        "const _: () = { let _ = Slot::<Option<u16>> { value: None }; };",
        "pub struct Doc; impl From<Slot<Option<u16>>> for Doc { fn from(_: Slot<Option<u16>>) -> Self { Doc } }",
        "macro_rules! reader { () => { pub fn read(t: &str) -> Slot<Option<u16>> { serde_json::from_str(t).unwrap() } }; }
         reader!();",
        "macro_rules! reader { ($name:ident, $t:ty) => { pub fn $name() -> Option<$t> { None } }; }
         reader!(read, Slot<Option<u16>>);",
    ] {
        assert_one_offender(
            &lib("written-instantiation", &format!("{SLOT_BESIDE_A_SAFE_USE}{written}")),
            "Slot.value states no deserialize_with",
        );
    }
}

#[test]
fn a_forwarded_signature_parameter_states_nothing_and_a_wrapped_one_is_read() {
    let forwarded = "pub fn read<U>() -> Slot<U> { todo!() }
        pub struct Doc<V>(V);
        impl<V> Doc<V> { pub fn get(&self) -> Slot<V> { todo!() } }
        macro_rules! reader { ($name:ident, $t:ty) => { pub fn $name<G>() -> Option<$t> { None } }; }
        reader!(read_generic, Slot<G>);";
    let offenders = lib(
        "forwarded-signature",
        &format!("{SLOT_BESIDE_A_SAFE_USE}{forwarded}"),
    )
    .offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");
    assert_one_offender(
        &lib(
            "wrapped-signature",
            &format!("{SLOT_BESIDE_A_SAFE_USE}pub fn read<U>() -> Slot<Option<U>> {{ todo!() }}"),
        ),
        "Slot.value states no deserialize_with",
    );
}

#[test]
fn a_macro_body_spelling_a_fragment_leaves_the_parameter_unread() {
    assert_one_offender(
        &lib(
            "fragment-instantiation",
            &format!(
                "{SLOT_BESIDE_A_SAFE_USE}macro_rules! reader {{ ($t:ty) => {{ pub fn read() -> Slot<$t> {{ todo!() }} }}; }}"
            ),
        ),
        "the macro_rules! reader at crates/cadmpeg-ir/src/lib.rs:3 names Slot, and the census does not expand it",
    );
}

#[test]
fn a_declaration_in_a_const_block_is_read() {
    assert_one_offender(
        &lib(
            "const-block-declaration",
            "const _: () = { #[derive(Serialize, Deserialize)] struct Host { point: Option<u16> } };",
        ),
        "Host.point states no deserialize_with",
    );
    assert_one_offender(
        &lib(
            "const-block-instantiation",
            &format!(
                "{SLOT_BESIDE_A_SAFE_USE}const _: () = {{ #[derive(Serialize, Deserialize)] struct Host {{ nested: Slot<Option<u16>> }} }};"
            ),
        ),
        "Slot.value states no deserialize_with",
    );
    let source = SPACE.replace("PLANAR", "u16")
        + "pub struct Hidden; const _: () = { impl Space for Hidden { type Point = Option<u8>; } };";
    assert_one_offender(&space("const-block-impl", &source), OPEN_POINT);
}

#[test]
fn a_use_under_a_renamed_import_or_in_another_wire_crate_fills_the_parameter() {
    let renamed = "#[derive(Serialize, Deserialize)] pub struct Slot<T = u8> { pub value: T }
        pub use Slot as Cell;
        #[derive(Serialize, Deserialize)] pub struct Host { pub nested: Cell<Option<u16>> }";
    assert_one_offender(
        &lib("bare-renamed-use", renamed),
        "Slot.value states no deserialize_with",
    );
    let fixture = Fixture::new(
        "bare-across-crates",
        &[
            (
                "crates/cadmpeg-ir/src/lib.rs",
                "#[derive(Serialize, Deserialize)] pub struct Slot<T = u8> { pub value: T }",
            ),
            (
                "crates/cadmpeg-asm/src/lib.rs",
                "use cadmpeg_ir::Slot;
                 #[derive(Serialize, Deserialize)] pub struct Host { pub nested: Slot<Option<u16>> }",
            ),
        ],
    );
    assert_one_offender(&fixture, "Slot.value states no deserialize_with");
}

#[test]
fn an_impl_parameter_holds_what_fills_the_self_type() {
    let source = "#[derive(Serialize, Deserialize)] pub struct Slot<T = u8> { pub value: T }
        pub trait Space { type Point; }
        pub struct Holder<U>(U);
        impl<U> Space for Holder<U> { type Point = Slot<U>; }
        pub type Filled = Holder<Option<u16>>;";
    assert_one_offender(
        &lib("impl-parameter-filled", source),
        "Slot.value states no deserialize_with",
    );
}

const FOREIGN_IMPLEMENT: &str = "macro_rules! implement { ($t:path, $n:ident) => { impl $t for $n { type Point = Option<u8>; } }; }
pub struct Codec;
";

fn foreign(name: &str, codec: &str) -> Fixture {
    Fixture::new(
        name,
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod space;"),
            (
                "crates/cadmpeg-ir/src/space.rs",
                &SPACE.replace("PLANAR", "u16"),
            ),
            (
                "crates/codec/src/lib.rs",
                &format!("{FOREIGN_IMPLEMENT}{codec}"),
            ),
        ],
    )
}

#[test]
fn a_foreign_wrapper_macro_naming_the_trait_leaves_the_type_unread() {
    let fixture = foreign(
        "foreign-wrapper",
        "macro_rules! expand { ($t:path, $n:ident) => { implement!($t, $n); }; }
fn go() { expand!(cadmpeg_ir::space::Space, Codec); }",
    );
    assert_one_offender(
        &fixture,
        "the call to expand! at crates/codec/src/lib.rs:4, which names Space, is outside the wire crates",
    );
}

#[test]
fn a_renamed_macro_naming_the_trait_leaves_the_type_unread() {
    let fixture = foreign(
        "foreign-renamed-macro",
        "use implement as expand;
fn go() { expand!(cadmpeg_ir::space::Space, Codec); }",
    );
    assert_one_offender(
        &fixture,
        "the call to expand! at crates/codec/src/lib.rs:4, which names Space, is outside the wire crates",
    );
    let wire = SPACE.replace("PLANAR", "u16")
        + "macro_rules! implement { ($t:ident, $n:ident) => { impl $t for $n { type Point = Option<u8>; } }; }
           use implement as expand;
           pub struct Extra;
           expand!(Space, Extra);";
    assert_one_offender(
        &space("wire-renamed-macro", &wire),
        "names Space, and the census does not expand it, so an impl it expands is unread",
    );
}

#[test]
fn a_wire_wrapper_macro_naming_the_trait_leaves_its_impls_unread() {
    let source = SPACE.replace("PLANAR", "u16")
        + "macro_rules! implement { ($t:ident, $n:ident) => { impl $t for $n { type Point = Option<u8>; } }; }
           macro_rules! expand { ($t:ident, $n:ident) => { implement!($t, $n); }; }
           pub struct Extra;
           expand!(Space, Extra);";
    assert_one_offender(
        &space("wire-wrapper-macro", &source),
        "the call to expand! at crates/cadmpeg-ir/src/space.rs:",
    );
}

#[test]
fn an_impl_through_a_renamed_trait_is_read() {
    let source = SPACE.replace("PLANAR", "u16")
        + "use Space as Plane; pub struct Decoy; impl Plane for Decoy { type Point = Option<u8>; }";
    assert_one_offender(
        &space("renamed-trait-impl", &source),
        "Entity.point states no deserialize_with",
    );
    let fixture = foreign(
        "foreign-renamed-trait",
        "use cadmpeg_ir::space::Space as Plane;
impl Plane for Codec { type Point = u8; }",
    );
    assert_one_offender(
        &fixture,
        "an impl of Plane at crates/codec/src/lib.rs:4 is outside the wire crates",
    );
}

#[test]
fn an_impl_the_item_walk_does_not_reach_is_unread() {
    let source = SPACE.replace("PLANAR", "u16")
        + "pub struct Hidden; fn hidden() { { impl Space for Hidden { type Point = Option<u8>; } } }";
    assert_one_offender(
        &space("impl-in-a-block", &source),
        "sits where the census reads no impl, so its types are unread",
    );
}

const OPEN_POINT: &str = "Entity.point states no deserialize_with";
const OPEN_SLOT: &str = "Slot.value states no deserialize_with";
const SLOT: &str = "#[derive(Serialize, Deserialize)] pub struct Slot<T = u8> { pub value: T }\n";
const MAYBE: &str = "macro_rules! maybe { ($t:ty) => { Option<$t> }; }\n";
const HOST: &str = "macro_rules! host { ($w:ident, $t:ty) => { #[derive(Serialize, Deserialize)] pub struct Host { pub nested: $w<$t> } }; }\n";
const PLANAR_OPTION: &str = "pub trait Space { type Point; }
pub struct Planar;
impl Space for Planar { type Point = Option<u16>; }
";

#[test]
fn a_raw_identifier_trait_impl_is_read() {
    let source = SPACE.replace("PLANAR", "u16")
        + "pub struct Extra; impl r#Space for Extra { type Point = Option<u8>; }";
    assert_one_offender(&space("raw-trait-impl", &source), OPEN_POINT);
}

#[test]
fn a_foreign_raw_identifier_impl_is_outside_the_wire_crates() {
    let fixture = foreign(
        "foreign-raw-trait",
        "impl cadmpeg_ir::space::r#Space for Codec { type Point = u8; }",
    );
    assert_one_offender(
        &fixture,
        "an impl of Space at crates/codec/src/lib.rs:3 is outside the wire crates",
    );
}

#[test]
fn an_alias_renamed_by_use_holds_what_it_names() {
    let fixture = Fixture::new(
        "alias-renamed-by-use",
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod kinds; mod space;"),
            (
                "crates/cadmpeg-ir/src/kinds.rs",
                "pub type Maybe = Option<u16>;",
            ),
            (
                "crates/cadmpeg-ir/src/space.rs",
                "use crate::kinds::Maybe as Point;
                 #[derive(Serialize, Deserialize)] pub struct Entity { pub point: Point }",
            ),
        ],
    );
    assert_one_offender(&fixture, OPEN_POINT);
}

#[test]
fn an_alias_reexported_under_another_name_holds_what_it_names() {
    let fixture = Fixture::new(
        "alias-reexported-renamed",
        &[
            (
                "crates/cadmpeg-ir/src/lib.rs",
                "mod kinds; mod api; mod space;",
            ),
            (
                "crates/cadmpeg-ir/src/kinds.rs",
                "pub type Maybe = Option<u16>;",
            ),
            (
                "crates/cadmpeg-ir/src/api.rs",
                "pub use crate::kinds::Maybe as Point;",
            ),
            (
                "crates/cadmpeg-ir/src/space.rs",
                "use crate::api;
                 #[derive(Serialize, Deserialize)] pub struct Entity { pub point: api::Point }",
            ),
        ],
    );
    assert_one_offender(&fixture, OPEN_POINT);
}

#[test]
fn an_impl_stating_a_renamed_alias_holds_what_it_names() {
    let source = SPACE.replace("PLANAR", "u16")
        + "pub mod kinds { pub type Maybe = Option<u8>; }
           use kinds::Maybe as Chosen;
           pub struct Extra; impl Space for Extra { type Point = Chosen; }";
    assert_one_offender(&space("impl-renamed-alias", &source), OPEN_POINT);
}

#[test]
fn an_argument_spelled_by_a_renamed_alias_holds_what_it_names() {
    let source = format!(
        "{SLOT}pub mod kinds {{ pub type Maybe = Option<u16>; }}
         use kinds::Maybe as Chosen;
         #[derive(Serialize, Deserialize)] pub struct Host {{ pub nested: Slot<Chosen> }}"
    );
    assert_one_offender(&lib("argument-renamed-alias", &source), OPEN_SLOT);
}

#[test]
fn a_const_argument_spelled_by_name_keeps_its_position() {
    let source = "pub const SIZE: usize = 2;
        #[derive(Serialize, Deserialize)] pub struct Slot<const N: usize, T> { pub value: T, pub pad: [u8; N] }
        #[derive(Serialize, Deserialize)] pub struct Host { pub nested: Slot<SIZE, Option<u16>> }";
    assert_one_offender(&lib("const-argument-by-name", source), OPEN_SLOT);
}

#[test]
fn a_const_argument_spelled_by_name_keeps_its_alias_position() {
    let source = "pub const SIZE: usize = 2;
        pub type Pick<const N: usize, B> = B;
        #[derive(Serialize, Deserialize)] pub struct Host { pub point: Pick<SIZE, Option<u16>> }";
    assert_one_offender(
        &lib("const-alias-argument-by-name", source),
        "Host.point states no deserialize_with",
    );
}

#[test]
fn a_parameter_forwarded_to_one_nothing_fills_is_unread() {
    let source = "#[derive(Serialize, Deserialize)] pub struct Slot<T> { pub value: T }
        #[derive(Serialize, Deserialize)] pub struct Host<U> { pub nested: Slot<U> }";
    let offenders = lib("forwarded-to-nothing", source).offenders();
    assert!(
        offenders.iter().any(|offender| offender.contains("Slot.value")
            && offender.contains(
                "cadmpeg_ir::Host has no default and no instantiation in the wire crates for its parameter U"
            )),
        "{offenders:#?}"
    );
}

#[test]
fn a_macro_call_naming_the_reader_beside_its_argument_leaves_it_unread() {
    let source = format!("{SLOT}{HOST}host!(Slot, Option<u16>);");
    assert_one_offender(
        &lib("macro-reader-beside-argument", &source),
        "the call to host! at crates/cadmpeg-ir/src/lib.rs:3 names Slot, and the census does not expand it",
    );
}

#[test]
fn a_foreign_macro_call_naming_the_reader_leaves_it_unread() {
    let source = format!("{SLOT}other_crate::host!(Slot, Option<u16>);");
    assert_one_offender(
        &lib("foreign-macro-reader", &source),
        "the call to host! at crates/cadmpeg-ir/src/lib.rs:2 names Slot, and the census does not expand it",
    );
}

#[test]
fn an_impl_stating_a_macro_type_is_unread() {
    let source = SPACE.replace("PLANAR", "u16")
        + MAYBE
        + "pub struct Extra; impl Space for Extra { type Point = maybe!(u8); }";
    assert_one_offender(
        &space("impl-macro-type", &source),
        "the type maybe!(..) is a macro call, which the census does not expand",
    );
}

#[test]
fn an_argument_spelled_by_a_macro_is_unread() {
    let source = format!(
        "{SLOT}{MAYBE}#[derive(Serialize, Deserialize)] pub struct Host {{ pub nested: Slot<maybe!(u16)> }}"
    );
    assert_one_offender(
        &lib("argument-macro-type", &source),
        "the type maybe!(..) is a macro call, which the census does not expand",
    );
}

#[test]
fn a_field_typed_by_a_concrete_projection_is_unread() {
    let source = format!(
        "{PLANAR_OPTION}#[derive(Serialize, Deserialize)] pub struct Entity {{ pub point: <Planar as Space>::Point }}"
    );
    assert_one_offender(
        &lib("field-concrete-projection", &source),
        "the type <Planar as Space>::Point names an associated type of a concrete type",
    );
}

#[test]
fn an_alias_of_a_concrete_projection_is_unread() {
    let source = format!(
        "{PLANAR_OPTION}pub type PlanarPoint = <Planar as Space>::Point;
         #[derive(Serialize, Deserialize)] pub struct Entity {{ pub point: PlanarPoint }}"
    );
    assert_one_offender(
        &lib("alias-concrete-projection", &source),
        "the type <Planar as Space>::Point names an associated type of a concrete type",
    );
}

#[test]
fn an_argument_spelled_by_a_concrete_projection_is_unread() {
    let source = format!(
        "{SLOT}{PLANAR_OPTION}#[derive(Serialize, Deserialize)] pub struct Host {{ pub nested: Slot<<Planar as Space>::Point> }}"
    );
    assert_one_offender(
        &lib("argument-concrete-projection", &source),
        "the type <Planar as Space>::Point names an associated type of a concrete type",
    );
}

#[test]
fn an_impl_on_the_line_of_a_read_impl_is_unread() {
    let source = SPACE.replace("PLANAR", "u16")
        + "pub struct Other; pub struct Extra; impl Space for Other { type Point = u8; } fn hidden() { { impl Space for Extra { type Point = Option<u8>; } } }";
    assert_one_offender(
        &space("impl-beside-read-impl", &source),
        "the impl of Space at crates/cadmpeg-ir/src/space.rs:17 sits where the census reads no impl",
    );
}

#[test]
fn an_impl_on_the_line_of_a_test_module_is_unread() {
    let source = SPACE.replace("PLANAR", "u16")
        + "pub struct Extra; #[cfg(test)] mod tests {} fn hidden() { { impl Space for Extra { type Point = Option<u8>; } } }";
    assert_one_offender(
        &space("impl-beside-test-module", &source),
        "the impl of Space at crates/cadmpeg-ir/src/space.rs:17 sits where the census reads no impl",
    );
}

/// A fixture whose `crates/cadmpeg-ir/src/features.rs` holds `source`.
///
/// The census derives its null-refusing shims from the repository's own
/// sources, so a fixture that reads through one names a shim that file
/// declares.
fn features(name: &str, source: &str, extra: &[(&str, &str)]) -> Fixture {
    let mut files = vec![
        ("crates/cadmpeg-ir/src/lib.rs", "mod features;"),
        ("crates/cadmpeg-ir/src/features.rs", source),
    ];
    files.extend_from_slice(extra);
    Fixture::new(name, &files)
}

/// A fixture whose `crates/cadmpeg-ir/src/features/<module>.rs` holds
/// `source`: the module in which the repository declares the shim the
/// fixture reads through.
fn feature_module(name: &str, module: &str, source: &str) -> Fixture {
    let path = format!("crates/cadmpeg-ir/src/features/{module}.rs");
    let parent = format!("mod {module};");
    Fixture::new(
        name,
        &[
            ("crates/cadmpeg-ir/src/lib.rs", "mod features;"),
            ("crates/cadmpeg-ir/src/features.rs", parent.as_str()),
            (path.as_str(), source),
        ],
    )
}

const CONTINUITY_STATE: &str = r#"
        cadmpeg_core::named_optional_field!(deserialize_continuity, Continuity, "continuity");
        #[derive(Serialize, Deserialize)]
        pub struct Continuity { pub order: u8 }
        #[derive(Serialize, Deserialize)]
        pub struct ContinuityState(FIELDS);
    "#;

#[test]
fn a_newtype_struct_leaves_the_absence_of_its_option_to_its_holders() {
    let read = r#"#[serde(deserialize_with = "deserialize_continuity")] Option<Continuity>"#;
    let offenders = feature_module(
        "newtype-holder",
        "surface_inputs",
        &CONTINUITY_STATE.replace("FIELDS", read),
    )
    .offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");

    let pair = feature_module(
        "newtype-holder-pair",
        "surface_inputs",
        &CONTINUITY_STATE.replace("FIELDS", &format!("{read}, u8")),
    )
    .offenders();
    assert_eq!(pair.len(), 2, "{pair:#?}");
    assert!(
        pair[0].contains("ContinuityState.<tuple field> reads the absent key as its one spelling of None but states no default")
            && pair[1].contains("ContinuityState.<tuple field> refuses null, but cadmpeg_ir::features::surface_inputs::ContinuityState states no skip_serializing_if and writes null for None"),
        "{pair:#?}"
    );

    let nullable = r#"#[serde(skip_serializing_if = "Option::is_none", deserialize_with = "cadmpeg_core::absent_key::nullable")] Option<Continuity>"#;
    assert_one_offender(
        &feature_module(
            "newtype-holder-nullable",
            "surface_inputs",
            &CONTINUITY_STATE.replace("FIELDS", nullable),
        ),
        "ContinuityState.<tuple field> admits null, but cadmpeg_ir::features::surface_inputs::ContinuityState skips the key for None",
    );
}

const SWEEP_SECTION: &str = r#"
        cadmpeg_core::named_optional_field!(deserialize_section_native, String, "value");
        #[derive(Deserialize)]
        #[serde(TAGGING)]
        pub enum SweepSection {
            VARIANT
            Unresolved(#[serde(deserialize_with = "deserialize_section_native")] Option<String>),
            Profile(u32),
        }
    "#;

fn sweep_section(name: &str, tagging: &str, variant: &str) -> Fixture {
    feature_module(
        name,
        "sweeps",
        &SWEEP_SECTION
            .replace("TAGGING", tagging)
            .replace("VARIANT", variant),
    )
}

#[test]
fn an_adjacently_tagged_newtype_variant_reads_an_absent_content_as_none() {
    let adjacent = r#"tag = "kind", content = "value""#;
    let offenders = sweep_section("adjacent-content", adjacent, "").offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");

    let no_default = "SweepSection.<tuple field> reads the absent key as its one spelling of None but states no default";
    assert_one_offender(
        &sweep_section("external-content", "deny_unknown_fields", ""),
        no_default,
    );
    assert_one_offender(
        &sweep_section(
            "adjacent-variant-reader",
            adjacent,
            r#"#[serde(deserialize_with = "read_unresolved")]"#,
        ),
        no_default,
    );
}

#[test]
fn a_hand_written_key_names_its_own_reading_type() {
    let source = SWEEP_SECTION
        .replace("TAGGING", r#"tag = "kind", content = "value""#)
        .replace("VARIANT", "")
        .replace("SweepSection", "OtherSection");
    assert_one_offender(
        &feature_module("unlisted-hand-written-key", "sweeps", &source),
        "OtherSection.<tuple field> cadmpeg_ir::features::sweeps::OtherSection writes no keys of its own and no single container attribute routes a read through it",
    );
}

const FEATURE_ROW: &str = r#"
        pub(crate) mod serialization;
        cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
        #[derive(Deserialize)]
        pub(crate) struct FeatureRowWire {
            #[serde(default, deserialize_with = "deserialize_name")]
            name: Option<String>,
        }
    "#;

const FEATURE_WRITE: &str = r#"
        #[derive(Serialize)]
        pub(crate) struct FeatureWriteWire<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            name: &'a Option<String>,
        }
    "#;

#[test]
fn a_listed_writer_is_found_in_its_own_module() {
    let offenders = features(
        "listed-writer",
        FEATURE_ROW,
        &[(
            "crates/cadmpeg-ir/src/features/serialization.rs",
            FEATURE_WRITE,
        )],
    )
    .offenders();
    assert!(offenders.is_empty(), "{offenders:#?}");

    assert_one_offender(
        &features(
            "listed-writer-elsewhere",
            &format!("{FEATURE_ROW}pub(crate) mod wire;"),
            &[
                ("crates/cadmpeg-ir/src/features/serialization.rs", ""),
                ("crates/cadmpeg-ir/src/features/wire.rs", FEATURE_WRITE),
            ],
        ),
        "FeatureRowWire.name serialization::FeatureWriteWire, named from cadmpeg_ir::features::FeatureRowWire, resolves to no one declaration",
    );
}
