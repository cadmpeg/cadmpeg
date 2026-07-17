// SPDX-License-Identifier: Apache-2.0
//! Single source of truth for the CATIA `b5 03` record-class dispatch.
//!
//! Every `b5 03` record carries a one-byte type/class code in its header
//! ([`B5Record::class`](crate::b5::B5Record::class)). The [`b5`](crate::b5)
//! stream walk dispatches on that byte to resolve faces, loops, pcurves, edges,
//! surfaces, and profiles. That dispatch was a scatter of magic-number match
//! arms; this table names each recognized class once, and the dispatch matches
//! against the named constants here. The record-class reference doc under
//! `docs/formats/` and the `catia_b5` fuzz dictionary are generated from
//! [`CLASSES`]; a checked test regenerates the derived artifacts and diffs them,
//! and a drift test asserts the surface-class predicate the topology binder uses
//! agrees with the table's [`RecordClass::is_surface`] flags.
//!
//! The class byte is distinct from the reference-encoding lead bytes the pointer
//! reader consumes (`0x08`/`0x10`/`0x18`/`0x30`/`0x38`); a value such as `0x18`
//! is a line-pcurve class here but an inline 16-bit reference lead in a payload,
//! and the two roles never share a decode site.

/// `b5 03 5f` face node.
pub const FACE: u8 = 0x5f;
/// `b5 03 62` loop node.
pub const LOOP: u8 = 0x62;
/// `b5 03 21` parametric-curve (pcurve) node.
pub const PCURVE: u8 = 0x21;
/// `b5 03 18` straight-line pcurve node (a loop member alongside [`PCURVE`]).
pub const LINE_PCURVE: u8 = 0x18;
/// `b5 03 5e` edge node.
pub const EDGE: u8 = 0x5e;
/// `b5 03 27` planar surface.
pub const SURFACE_PLANE: u8 = 0x27;
/// `b5 03 28` cylindrical surface.
pub const SURFACE_CYLINDER: u8 = 0x28;
/// `b5 03 2d` surface of revolution.
pub const SURFACE_REVOLUTION: u8 = 0x2d;
/// `b5 03 34` NURBS surface (its geometry is carried by the `a8 03` stream; the
/// topology binder recognizes the class so a face may reference it).
pub const SURFACE_NURBS: u8 = 0x34;
/// `b5 03 0e` straight-line profile.
pub const PROFILE_LINE: u8 = 0x0e;
/// `b5 03 0f` circular-arc profile.
pub const PROFILE_ARC: u8 = 0x0f;

/// One recognized `b5 03` record class.
#[derive(Debug, Clone, Copy)]
pub struct RecordClass {
    /// The record's third header byte.
    pub code: u8,
    /// Stable short name used in the reference doc.
    pub name: &'static str,
    /// Which dispatch resolves the record.
    pub role: &'static str,
    /// Whether the topology binder accepts this class where a surface reference
    /// is required (face and loop surface slots).
    pub is_surface: bool,
    /// One-line description of the resolved geometry or topology node.
    pub summary: &'static str,
}

/// Every recognized `b5 03` record class, in ascending code order.
pub const CLASSES: &[RecordClass] = &[
    RecordClass {
        code: PROFILE_LINE,
        name: "profile_line",
        role: "profile",
        is_surface: false,
        summary: "straight-line trim profile (point + direction)",
    },
    RecordClass {
        code: PROFILE_ARC,
        name: "profile_arc",
        role: "profile",
        is_surface: false,
        summary: "circular-arc trim profile (center, two axes, radius)",
    },
    RecordClass {
        code: LINE_PCURVE,
        name: "line_pcurve",
        role: "loop_member",
        is_surface: false,
        summary: "straight-line pcurve loop member",
    },
    RecordClass {
        code: PCURVE,
        name: "pcurve",
        role: "pcurve",
        is_surface: false,
        summary: "parameter-space NURBS pcurve lifted onto its surface",
    },
    RecordClass {
        code: SURFACE_PLANE,
        name: "surface_plane",
        role: "surface",
        is_surface: true,
        summary: "planar surface (origin + two directions)",
    },
    RecordClass {
        code: SURFACE_CYLINDER,
        name: "surface_cylinder",
        role: "surface",
        is_surface: true,
        summary: "cylindrical surface (origin, axis, radius)",
    },
    RecordClass {
        code: SURFACE_REVOLUTION,
        name: "surface_revolution",
        role: "surface",
        is_surface: true,
        summary: "surface of revolution (profile curve, axis, gauge radius)",
    },
    RecordClass {
        code: SURFACE_NURBS,
        name: "surface_nurbs",
        role: "surface",
        is_surface: true,
        summary: "NURBS surface; geometry sourced from the a8 03 stream",
    },
    RecordClass {
        code: EDGE,
        name: "edge",
        role: "edge",
        is_surface: false,
        summary: "edge node binding two vertex points",
    },
    RecordClass {
        code: FACE,
        name: "face",
        role: "face",
        is_surface: false,
        summary: "face node referencing one surface and its loops",
    },
    RecordClass {
        code: LOOP,
        name: "loop",
        role: "loop",
        is_surface: false,
        summary: "loop node: (pcurve edge)* pairs plus a surface reference",
    },
];

/// Whether the class code names a surface the topology binder may reference from
/// a face or loop surface slot. Derived from the table so the binder cannot
/// drift from the reference doc.
#[must_use]
pub fn is_surface(code: u8) -> bool {
    CLASSES
        .iter()
        .any(|class| class.code == code && class.is_surface)
}

/// Render the record-class reference doc: a deterministic Markdown table
/// generated from [`CLASSES`]. `docs/formats/catia_b5_record_classes.md` is this
/// output verbatim; the checked test regenerates and diffs it.
#[must_use]
pub fn render_reference() -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("<!-- Generated from crates/cadmpeg-codec-catia/src/b5_record_class.rs. ");
    out.push_str("Do not edit by hand; run `cargo test -p cadmpeg-codec-catia`. -->\n\n");
    out.push_str("# CATIA `b5 03` record classes\n\n");
    out.push_str(
        "Each `b5 03` record's third header byte is its type/class code. The \
stream walk dispatches on this byte to resolve topology and geometry nodes. The \
surface column marks classes the topology binder accepts where a surface \
reference is required.\n\n",
    );
    out.push_str("| Code | Name | Role | Surface | Description |\n");
    out.push_str("|---|---|---|---|---|\n");
    for class in CLASSES {
        let _ = writeln!(
            out,
            "| `0x{:02x}` | {} | {} | {} | {} |",
            class.code,
            class.name,
            class.role,
            if class.is_surface { "yes" } else { "no" },
            class.summary,
        );
    }
    out
}

/// Render the `catia_b5` fuzz dictionary: one quoted single-byte token per
/// recognized class code, in code order, for structure-aware fuzzing of the
/// `b5 03` walk. libFuzzer dictionary syntax is `"token"` per line, and a
/// non-printable class byte is written as a `\xNN` escape.
#[must_use]
pub fn render_dictionary() -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("# Generated from crates/cadmpeg-codec-catia/src/b5_record_class.rs.\n");
    out.push_str("# Do not edit by hand; run `cargo test -p cadmpeg-codec-catia`.\n");
    out.push_str("# CATIA b5 03 record-class codes, for structure-aware fuzzing.\n");
    for class in CLASSES {
        let _ = writeln!(out, "\"\\x{:02x}\" # {}", class.code, class.name);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    }

    fn check_generated(relative: &str, expected: &str) {
        let path = workspace_root().join(relative);
        if std::env::var_os("CADMPEG_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().expect("artifact has a parent"))
                .expect("create artifact directory");
            std::fs::write(&path, expected).expect("write blessed artifact");
            return;
        }
        let actual = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}; run with CADMPEG_BLESS=1", path.display()));
        assert_eq!(
            actual,
            expected,
            "{} is stale; run `CADMPEG_BLESS=1 cargo test -p cadmpeg-codec-catia`",
            path.display()
        );
    }

    #[test]
    fn reference_doc_matches_table() {
        check_generated(
            "docs/formats/catia_b5_record_classes.md",
            &render_reference(),
        );
    }

    #[test]
    fn fuzz_dictionary_matches_table() {
        check_generated(
            "crates/cadmpeg-fuzz/dictionaries/catia_b5.dict",
            &render_dictionary(),
        );
    }

    /// Class codes are distinct; a duplicated code would make the table and the
    /// dispatch it feeds ambiguous.
    #[test]
    fn class_codes_are_distinct() {
        let mut codes: Vec<u8> = CLASSES.iter().map(|class| class.code).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "duplicate class code in table");
    }

    /// The surface predicate the topology binder calls must accept exactly the
    /// table's surface-flagged classes and reject every other recognized class,
    /// so the binder cannot drift from the reference doc.
    #[test]
    fn surface_predicate_matches_flags() {
        for class in CLASSES {
            assert_eq!(
                is_surface(class.code),
                class.is_surface,
                "is_surface({:#04x}) disagrees with table flag for {}",
                class.code,
                class.name
            );
        }
    }
}
