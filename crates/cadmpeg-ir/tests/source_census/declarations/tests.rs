// SPDX-License-Identifier: Apache-2.0
//! Fixture trees for the declaration index and the optional-field census.

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
        let mut files = files.to_vec();
        files.push(("crates/cadmpeg-core/src/lib.rs", ""));
        files.push(("crates/cadmpeg-asm/src/lib.rs", ""));
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
