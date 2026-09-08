// SPDX-License-Identifier: Apache-2.0
//! Rendering and code generation for the record-layout tables.
//!
//! Two emitters over one validated table: `render` writes the `<format>.md`
//! page the specification links to, and `emit_layout_rs` writes the owning
//! crate's `src/layout.rs` constants. Neither validates; both assume the table
//! already passed `validate`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as IoWrite;
use std::process::{Command, Stdio};

use crate::{
    decode_field_value, is_multibyte_scalar, normalize, parse_token_tag, rust_byte_array, rust_hex,
    rust_token_ty, token_const_name, type_width, DiscrepancyKind, Field, LayoutFile, Record,
    RecordKind, Source, TokenConst, TokenDecl,
};

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render a layout file as the markdown page the specification links to.
pub(crate) fn render(file: &LayoutFile) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!-- Generated from docs/layouts/{}.toml by",
        file.format
    );
    let _ = writeln!(
        out,
        "     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;"
    );
    let _ = writeln!(
        out,
        "     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "# `{}` record layouts", file.format);
    let _ = writeln!(out);
    let _ = writeln!(out, "Source of truth: [`{0}`](../../{0}).", file.spec);
    let _ = writeln!(out, "Table source: `docs/layouts/{}.toml`.", file.format);
    if !file.note.trim().is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", file.note.trim());
    }

    if !file.types.is_empty() {
        let _ = writeln!(out, "\n## Composite types\n");
        let _ = writeln!(out, "| Type | Bytes | Endianness | Meaning |");
        let _ = writeln!(out, "| ---- | ----: | ---------- | ------- |");
        for decl in &file.types {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} |",
                decl.name,
                decl.bytes,
                decl.endianness,
                cell(&decl.note)
            );
        }
    }

    if !file.tokens.is_empty() {
        let _ = writeln!(out, "\n## Tag inventory\n");
        let _ = writeln!(out, "| Tag | Name | Payload | Meaning | Spec |");
        let _ = writeln!(out, "| --- | ---- | ------: | ------- | ---- |");
        for token in &file.tokens {
            let payload = token
                .payload_bytes
                .map_or_else(|| "variable".to_string(), |n| format!("{n} B"));
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} | §{} |",
                token.tag,
                cell(&token.name),
                payload,
                cell(&token.note),
                token.section
            );
        }
    }

    for record in &file.records {
        let _ = writeln!(out, "\n## `{}`\n", record.name);
        let kind = match record.kind {
            RecordKind::Byte => "byte offsets",
            RecordKind::Slot => "ordered slots (no stated byte offsets)",
            RecordKind::Column => "1-based character columns",
        };
        let size = record
            .size
            .map_or_else(|| "not stated".to_string(), |s| format!("{s} B"));
        let _ = writeln!(
            out,
            "Spec §{} · layout: {kind} · size: {size}",
            record.section
        );
        if !record.dialects.is_empty() {
            let _ = writeln!(
                out,
                "\nDialects: {}",
                record
                    .dialects
                    .iter()
                    .map(|id| format!("`{id}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !record.note.trim().is_empty() {
            let _ = writeln!(out, "\n{}", record.note.trim());
        }
        if !record.parsed_by.is_empty() {
            let _ = writeln!(out, "\nParsed by:");
            for path in &record.parsed_by {
                let _ = writeln!(out, "- `{path}`");
            }
        }
        let _ = writeln!(out);
        match record.kind {
            RecordKind::Byte => {
                let _ = writeln!(
                    out,
                    "| Offset | Size | Field | Type | Endian | Src | Meaning |"
                );
                let _ = writeln!(
                    out,
                    "| -----: | ---: | ----- | ---- | ------ | --- | ------- |"
                );
            }
            RecordKind::Slot => {
                let _ = writeln!(out, "| # | Slot | Type | Endian | Src | Meaning |");
                let _ = writeln!(out, "| -: | ---- | ---- | ------ | --- | ------- |");
            }
            RecordKind::Column => {
                let _ = writeln!(out, "| Columns | Field | Type | Src | Meaning |");
                let _ = writeln!(out, "| ------- | ----- | ---- | --- | ------- |");
            }
        }
        let custom: BTreeMap<String, u64> = file
            .types
            .iter()
            .map(|t| (t.name.clone(), t.bytes))
            .collect();
        let mut rows: Vec<(u64, String)> = Vec::new();
        for (index, field) in record.fields.iter().enumerate() {
            let endian = field
                .endianness
                .as_deref()
                .or(record.endianness.as_deref())
                .or(file.endianness.as_deref())
                .unwrap_or("—");
            let src = match field.source {
                Source::Spec => "spec",
                Source::Derived => "derived",
                Source::Code => "code",
            };
            let mut meaning = if field.note.trim().is_empty() {
                cell(&field.anchor)
            } else {
                cell(&field.note)
            };
            if let Some(raw) = &field.value {
                let _ = write!(meaning, " · value `{raw}`");
            }
            match record.kind {
                RecordKind::Byte => {
                    let offset = field.offset.unwrap_or(0);
                    let width = type_width(&field.ty, &custom)
                        .ok()
                        .flatten()
                        .map_or_else(|| "?".to_string(), |w| w.to_string());
                    rows.push((
                        offset,
                        format!(
                            "| {offset} | {width} | `{}` | `{}` | {endian} | {src} | {meaning} |",
                            field.name, field.ty
                        ),
                    ));
                }
                RecordKind::Slot => {
                    rows.push((
                        index as u64,
                        format!(
                            "| {index} | `{}` | `{}` | {endian} | {src} | {meaning} |",
                            field.name, field.ty
                        ),
                    ));
                }
                RecordKind::Column => {
                    let columns = field.columns.clone().unwrap_or_default();
                    let start = columns
                        .split_once('-')
                        .and_then(|(a, _)| a.trim().parse().ok())
                        .unwrap_or(0);
                    rows.push((
                        start,
                        format!(
                            "| {columns} | `{}` | `{}` | {src} | {meaning} |",
                            field.name, field.ty
                        ),
                    ));
                }
            }
        }
        if record.kind != RecordKind::Slot {
            rows.sort_by_key(|(key, _)| *key);
        }
        for (_, row) in &rows {
            let _ = writeln!(out, "{row}");
        }
        if !record.gaps.is_empty() {
            let _ = writeln!(out, "\nUnstated regions:\n");
            for gap in &record.gaps {
                let _ = writeln!(
                    out,
                    "- `{}..{}` ({} B): {}",
                    gap.offset,
                    gap.offset + gap.size,
                    gap.size,
                    gap.note.trim()
                );
            }
        }
        if !record.discrepancies.is_empty() {
            let _ = writeln!(out, "\n**Discrepancies:**\n");
            for item in &record.discrepancies {
                let _ = writeln!(out, "- {}", item.note.trim());
            }
        }
        if !record.code.is_empty() {
            let _ = writeln!(out, "\nCross-checked against code:\n");
            for check in &record.code {
                let _ = writeln!(out, "- `{}` — {}", check.path, check.note.trim());
            }
        }
    }

    if !file.not_applicable.is_empty() {
        let _ = writeln!(out, "\n## Not tabulated\n");
        let _ = writeln!(out, "| Area | Spec | Reason |");
        let _ = writeln!(out, "| ---- | ---- | ------ |");
        for entry in &file.not_applicable {
            let _ = writeln!(
                out,
                "| {} | §{} | {} |",
                cell(&entry.area),
                entry.section,
                cell(&entry.reason)
            );
        }
    }

    out
}

/// Escape a value for a markdown table cell.
fn cell(text: &str) -> String {
    normalize(text).replace('|', "\\|")
}

// ---------------------------------------------------------------------------
// Generated layout constants
// ---------------------------------------------------------------------------

/// Table stem → generated Rust path, repo-relative. `step` is absent because
/// it has no byte-layout records.
pub(crate) const GENERATED_LAYOUT_RS: &[(&str, &str)] = &[
    ("asm", "crates/cadmpeg-asm/src/layout.rs"),
    ("catia", "crates/cadmpeg-codec-catia/src/layout.rs"),
    ("creo", "crates/cadmpeg-codec-creo/src/layout.rs"),
    ("f3d", "crates/cadmpeg-codec-f3d/src/layout.rs"),
    ("freecad", "crates/cadmpeg-codec-freecad/src/layout.rs"),
    ("iges", "crates/cadmpeg-codec-iges/src/layout.rs"),
    ("inventor", "crates/cadmpeg-codec-inventor/src/layout.rs"),
    ("nx", "crates/cadmpeg-codec-nx/src/layout.rs"),
    ("protein", "crates/cadmpeg-protein/src/layout.rs"),
    ("rhino", "crates/cadmpeg-codec-rhino/src/layout.rs"),
    ("sldprt", "crates/cadmpeg-codec-sldprt/src/layout.rs"),
];

const RUST_KEYWORDS: &[&str] = &[
    "Self", "abstract", "as", "async", "await", "become", "box", "break", "const", "continue",
    "crate", "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "union", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

fn is_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut prev_underscore = false;
    for c in chars {
        if c == '_' {
            if prev_underscore {
                return false;
            }
            prev_underscore = true;
            continue;
        }
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() {
            return false;
        }
        prev_underscore = false;
    }
    !prev_underscore
}

fn check_ident(kind: &str, name: &str, at: &str, errors: &mut Vec<String>) {
    if !is_snake_case(name) {
        errors.push(format!("{at}: {kind} `{name}` is not snake_case"));
    }
    if RUST_KEYWORDS.contains(&name) {
        errors.push(format!("{at}: {kind} `{name}` is a Rust keyword"));
    }
}

fn resolved_endian<'a>(
    file: &'a LayoutFile,
    record: &'a Record,
    field: &'a Field,
) -> Option<&'a str> {
    field
        .endianness
        .as_deref()
        .or(record.endianness.as_deref())
        .or(file.endianness.as_deref())
}

fn endian_phrase(endian: Option<&str>) -> Option<&'static str> {
    match endian {
        Some("little") => Some("little-endian"),
        Some("big") => Some("big-endian"),
        Some("unstated") => Some("endianness unstated"),
        _ => None,
    }
}

fn fence_text(text: &str) -> String {
    normalize(text).replace("```", "'''")
}

/// Turn one validated table into the checked-in `layout.rs` source.
pub(crate) fn emit_layout_rs(file: &LayoutFile) -> Result<String, Vec<String>> {
    let mut errors = Vec::new();
    let mut omitted = Vec::new();
    let mut modules = String::new();

    for record in &file.records {
        let at = format!("{}: record `{}`", file.format, record.name);
        if record.kind != RecordKind::Byte {
            continue;
        }
        if !record.discrepancies.is_empty() {
            let kinds: Vec<&str> = record
                .discrepancies
                .iter()
                .map(|d| match d.kind {
                    DiscrepancyKind::SizeMismatch => "size_mismatch",
                    DiscrepancyKind::Overlap => "overlap",
                })
                .collect();
            let note = record
                .discrepancies
                .first()
                .map(|d| fence_text(&d.note))
                .unwrap_or_default();
            omitted.push(format!(
                "// - `{}` ({}): {note}",
                record.name,
                kinds.join(", ")
            ));
            continue;
        }

        check_ident("record", &record.name, &at, &mut errors);
        let mut seen = BTreeSet::new();
        let mut fields_out = String::new();
        for field in &record.fields {
            let at = format!("{at}, field `{}`", field.name);
            if field.name == "len" {
                errors.push(format!(
                    "{at}: field name `len` collides with the record length constant `LEN`"
                ));
            }
            if !is_snake_case(&field.name) {
                errors.push(format!("{at}: field `{}` is not snake_case", field.name));
            }
            let const_name = field.name.to_ascii_uppercase();
            if !seen.insert(const_name.clone()) {
                errors.push(format!("{at}: constant `{const_name}` already emitted"));
            }
            let Some(offset) = field.offset else {
                errors.push(format!("{at}: byte field has no offset"));
                continue;
            };
            let ty_part = if is_multibyte_scalar(&field.ty) {
                match endian_phrase(resolved_endian(file, record, field)) {
                    Some(endian) => format!("`{}`, {endian}", field.ty),
                    None => format!("`{}`", field.ty),
                }
            } else {
                format!("`{}`", field.ty)
            };
            let _ = writeln!(
                fields_out,
                "    /// Offset of `{0}` ({ty_part}). Spec §{1}.",
                field.name, record.section
            );
            let _ = writeln!(
                fields_out,
                "    pub(crate) const {const_name}: usize = {offset};"
            );
            if let Some(raw) = &field.value {
                let value_name = format!("{const_name}_VALUE");
                if !seen.insert(value_name.clone()) {
                    errors.push(format!("{at}: constant `{value_name}` already emitted"));
                }
                let custom: BTreeMap<String, u64> = file
                    .types
                    .iter()
                    .map(|t| (t.name.clone(), t.bytes))
                    .collect();
                let width = type_width(&field.ty, &custom).ok().flatten();
                match decode_field_value(raw, &field.ty, width) {
                    Ok(binding) => {
                        let _ = writeln!(
                            fields_out,
                            "    /// Stated value of `{0}` (`{1}`). Spec §{2}.",
                            field.name, field.ty, record.section
                        );
                        let _ =
                            writeln!(fields_out, "    pub(crate) const {value_name}: {binding};");
                    }
                    Err(message) => errors.push(format!("{at}: {message}")),
                }
            }
        }

        let _ = writeln!(
            modules,
            "/// Byte offsets for the `{}` record.",
            record.name
        );
        let _ = writeln!(modules, "///");
        match record.size {
            Some(size) => {
                let _ = writeln!(
                    modules,
                    "/// Spec §{}. Record length {size} B.",
                    record.section
                );
            }
            None => {
                let _ = writeln!(modules, "/// Spec §{}.", record.section);
            }
        }
        if !record.dialects.is_empty() {
            let _ = writeln!(modules, "///");
            let _ = writeln!(
                modules,
                "/// Dialects: {}.",
                record
                    .dialects
                    .iter()
                    .map(|id| format!("`{id}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !record.note.trim().is_empty() {
            let _ = writeln!(modules, "///");
            let _ = writeln!(modules, "/// ```text");
            let _ = writeln!(modules, "/// {}", fence_text(&record.note));
            let _ = writeln!(modules, "/// ```");
        }
        let _ = writeln!(modules, "pub(crate) mod {} {{", record.name);
        if let Some(size) = record.size {
            let _ = writeln!(
                modules,
                "    /// Record length in bytes. Spec §{}.",
                record.section
            );
            let _ = writeln!(modules, "    pub(crate) const LEN: usize = {size};");
        }
        modules.push_str(&fields_out);
        let _ = writeln!(modules, "}}");
        let _ = writeln!(modules);
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let mut token_mod = String::new();
    let mut token_consts: Vec<(String, TokenConst, &TokenDecl)> = Vec::new();
    for token in &file.tokens {
        let Some(value) = parse_token_tag(&token.tag) else {
            continue;
        };
        let Some(name) = token_const_name(&token.name) else {
            continue;
        };
        token_consts.push((name, value, token));
    }
    if !token_consts.is_empty() {
        let _ = writeln!(token_mod, "/// Tag constants from the table inventory.");
        let _ = writeln!(token_mod, "pub(crate) mod token {{");
        for (name, value, token) in &token_consts {
            let _ = writeln!(
                token_mod,
                "    /// `{}` (`{}`). Spec §{}.",
                token.name, token.tag, token.section
            );
            match value {
                TokenConst::Bytes(bytes) => {
                    let _ = writeln!(
                        token_mod,
                        "    pub(crate) const {name}: [u8; {}] = {};",
                        bytes.len(),
                        rust_byte_array(bytes)
                    );
                }
                other => {
                    let _ = writeln!(
                        token_mod,
                        "    pub(crate) const {name}: {} = {};",
                        rust_token_ty(other),
                        match other {
                            TokenConst::U8(v) => format!("{v}"),
                            TokenConst::U16(v) => rust_hex(u64::from(*v), 4),
                            TokenConst::U32(v) => rust_hex(u64::from(*v), 8),
                            TokenConst::U64(v) => rust_hex(*v, 16),
                            TokenConst::Bytes(_) => unreachable!(),
                        }
                    );
                }
            }
        }
        let _ = writeln!(token_mod, "}}");
        let _ = writeln!(token_mod);
    }

    let mut out = String::new();
    let _ = writeln!(out, "// SPDX-License-Identifier: Apache-2.0");
    let _ = writeln!(
        out,
        "//! Byte-offset and value constants generated from `docs/layouts/{}.toml`.",
        file.format
    );
    let _ = writeln!(out, "//!");
    let _ = writeln!(out, "//! Do not edit by hand. Regenerate with:");
    let _ = writeln!(
        out,
        "//! `UPDATE_LAYOUT_CODE=1 cargo test -p cadmpeg --test layout_tables`."
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "#![allow(dead_code)] // Not every generated constant is referenced yet."
    );
    let _ = writeln!(out);
    if !omitted.is_empty() {
        let _ = writeln!(
            out,
            "// Records omitted because the table declares a contradiction."
        );
        let _ = writeln!(out, "//");
        for line in &omitted {
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(out);
    }
    out.push_str(&token_mod);
    out.push_str(modules.trim_end());
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(rustfmt_source(&out))
}

/// rustfmt the emitter output so checked-in `layout.rs` files stay equal to
/// `cargo fmt` (long byte arrays wrap) and to `UPDATE_LAYOUT_CODE=1`.
fn rustfmt_source(src: &str) -> String {
    let mut child = Command::new("rustfmt")
        .args(["--emit", "stdout", "--quiet", "--edition", "2021"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rustfmt");
    child
        .stdin
        .as_mut()
        .expect("rustfmt stdin")
        .write_all(src.as_bytes())
        .expect("write rustfmt stdin");
    let output = child.wait_with_output().expect("wait rustfmt");
    assert!(
        output.status.success(),
        "rustfmt rejected generated layout.rs:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("rustfmt stdout utf-8")
}

// ---------------------------------------------------------------------------
