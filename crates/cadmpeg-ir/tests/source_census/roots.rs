// SPDX-License-Identifier: Apache-2.0
//! Whether the crate root a path spells binds the crate the census reads it as.
//!
//! A reader the census accepts by its path, such as
//! `cadmpeg_core::absent_key::nullable` or `serde_json::Value`, is that
//! crate's reader only where the path's first segment binds that crate. The
//! using crate's manifest maps a wire-crate root to the workspace's own
//! package of that name, a registry root to the registry package of that
//! name, and a standard root to no dependency. A token of the using crate's
//! sources that spells the root other than as a path's first segment or a
//! `use` rename's original can bind it to something else: a module, a `use`
//! rename, an `extern crate .. as`, an item, a macro argument. A glob import
//! from outside the crate can bring in a module of that name. An absolute
//! path starts at the extern prelude, which only `extern crate .. as`
//! rebinds, so for it only an `as` and a macro argument are read.

use std::collections::BTreeMap;
use std::path::Path;

use proc_macro2::{TokenStream, TokenTree};

/// Every crate root a path the census accepts can start with.
pub(crate) const CRATE_ROOTS: &[&str] = &[
    "std",
    "core",
    "alloc",
    "serde",
    "serde_json",
    "serde_bytes",
    "cadmpeg_core",
    "cadmpeg_ir",
    "cadmpeg_asm",
];

/// The standard crates, which no dependency names.
const STANDARD_ROOTS: &[&str] = &["std", "core", "alloc"];

/// The workspace package a wire-crate root names.
fn wire_package(root: &str) -> Option<&'static str> {
    match root {
        "cadmpeg_core" => Some("cadmpeg-core"),
        "cadmpeg_ir" => Some("cadmpeg-ir"),
        "cadmpeg_asm" => Some("cadmpeg-asm"),
        _ => None,
    }
}

/// The crate a repository-relative source belongs to: `crates/<name>`, or
/// the path itself outside `crates/`.
pub(crate) fn crate_of(path: &str) -> String {
    let mut parts = path.splitn(3, '/');
    match (parts.next(), parts.next()) {
        (Some("crates"), Some(name)) => format!("crates/{name}"),
        _ => path.to_owned(),
    }
}

/// Why each crate root can name another item in a crate: one reason for a
/// relative path and one for an absolute path, keyed by crate and root.
#[derive(Default)]
pub(crate) struct RootShadows {
    relative: BTreeMap<(String, String), String>,
    absolute: BTreeMap<(String, String), String>,
}

/// One token of a flattened stream: a name, a punctuation character, or a
/// delimiter, with whether it sits in a macro's token tree.
enum Flat {
    Ident(String, usize, bool),
    Punct(char),
    Delimiter,
}

fn flatten(stream: TokenStream, in_macro: bool, out: &mut Vec<Flat>) {
    for tree in stream {
        match tree {
            TokenTree::Ident(ident) => {
                out.push(Flat::Ident(
                    ident.to_string(),
                    ident.span().start().line,
                    in_macro,
                ));
            }
            TokenTree::Punct(punct) => out.push(Flat::Punct(punct.as_char())),
            TokenTree::Literal(_) => out.push(Flat::Delimiter),
            TokenTree::Group(group) => {
                let called = matches!(out.last(), Some(Flat::Punct('!')))
                    || matches!(
                        out.get(out.len().wrapping_sub(2)..),
                        Some([Flat::Punct('!'), Flat::Ident(..)])
                    );
                out.push(Flat::Delimiter);
                flatten(group.stream(), in_macro || called, out);
                out.push(Flat::Delimiter);
            }
        }
    }
}

impl RootShadows {
    /// Records each token of the source `text` of `path` that can bind one
    /// of [`CRATE_ROOTS`] in its crate.
    pub(crate) fn record_tokens(&mut self, path: &str, text: &str) {
        let Ok(stream) = text.parse::<TokenStream>() else {
            for root in CRATE_ROOTS {
                self.record(path, root, false, format!("{path} does not lex"));
                self.record(path, root, true, format!("{path} does not lex"));
            }
            return;
        };
        let mut tokens = Vec::new();
        flatten(stream, false, &mut tokens);
        let punct = |index: usize, wanted: char| matches!(tokens.get(index), Some(Flat::Punct(found)) if *found == wanted);
        let word = |index: usize, wanted: &str| matches!(tokens.get(index), Some(Flat::Ident(found, ..)) if found == wanted);
        for (index, token) in tokens.iter().enumerate() {
            let Flat::Ident(name, line, in_macro) = token else {
                continue;
            };
            if !CRATE_ROOTS.contains(&name.as_str()) {
                continue;
            }
            let head = (punct(index + 1, ':') && punct(index + 2, ':')) || word(index + 1, "as");
            let nested = index >= 3
                && punct(index - 1, ':')
                && punct(index - 2, ':')
                && matches!(
                    tokens.get(index - 3),
                    Some(Flat::Ident(..) | Flat::Punct('>'))
                );
            let renamed = index >= 1 && word(index - 1, "as");
            let reason = format!("{path}:{line} spells {name} where it can bind another item");
            if !head || nested {
                self.record(path, name, false, reason.clone());
            }
            if renamed || (!head && *in_macro) {
                self.record(path, name, true, reason);
            }
        }
    }

    /// Records that a glob import of `target` in `path` can bring in a
    /// module named by any root, for a relative path.
    pub(crate) fn record_glob(&mut self, path: &str, target: &str) {
        for root in CRATE_ROOTS {
            self.record(
                path,
                root,
                false,
                format!("{path} imports every name of {target}, which can bring in a {root}"),
            );
        }
    }

    /// Records, for each of `crates` under the workspace `root`, every
    /// crate root its manifest does not bind to its real crate.
    pub(crate) fn record_manifests(&mut self, root: &Path, crates: &[&str]) {
        for krate in crates {
            for name in CRATE_ROOTS {
                if let Some(reason) = manifest_binding(root, krate, name) {
                    self.record(krate, name, false, reason.clone());
                    self.record(krate, name, true, reason);
                }
            }
        }
    }

    fn record(&mut self, path: &str, root: &str, absolute: bool, reason: String) {
        let map = if absolute {
            &mut self.absolute
        } else {
            &mut self.relative
        };
        map.entry((crate_of(path), root.to_owned()))
            .or_insert(reason);
    }

    /// Why `root`, written in `path` and starting an `absolute` path or
    /// not, can name another item than its real crate, or `None`.
    pub(crate) fn failure(&self, path: &str, root: &str, absolute: bool) -> Option<&String> {
        let key = (crate_of(path), root.to_owned());
        self.absolute
            .get(&key)
            .or_else(|| (!absolute).then(|| self.relative.get(&key)).flatten())
    }
}

/// One dependency entry a manifest states.
#[derive(Default, Clone)]
struct Dependency {
    key: String,
    package: Option<String>,
    path: Option<String>,
    workspace: bool,
    git: bool,
    /// Whether every part of the entry is one this reader reads.
    readable: bool,
}

/// The string a TOML value spells, or `None` for any other value.
fn string_value(value: &str) -> Option<String> {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
}

/// Split `text` at `separator` outside quotes, brackets and braces.
fn split_top(text: &str, separator: char) -> Vec<String> {
    let (mut parts, mut current, mut depth, mut quoted) = (Vec::new(), String::new(), 0_i32, false);
    for character in text.chars() {
        match character {
            '"' => quoted = !quoted,
            '[' | '{' if !quoted => depth += 1,
            ']' | '}' if !quoted => depth -= 1,
            _ => {}
        }
        if character == separator && depth == 0 && !quoted {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    parts.push(current);
    parts
}

/// Applies one `name = value` attribute to a dependency entry.
fn apply(entry: &mut Dependency, name: &str, value: &str) {
    match name.trim() {
        "package" => match string_value(value) {
            Some(package) => entry.package = Some(package),
            None => entry.readable = false,
        },
        "path" => match string_value(value) {
            Some(path) => entry.path = Some(path),
            None => entry.readable = false,
        },
        "workspace" => entry.workspace = value.trim() == "true",
        "git" => entry.git = true,
        _ => {}
    }
}

/// `line` up to a `#` outside a string.
fn without_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..index],
            _ => {}
        }
    }
    line
}

/// Whether `text` opens more brackets and braces than it closes, outside
/// strings: a statement that continues on the next line.
fn continues(text: &str) -> bool {
    let (mut depth, mut quoted) = (0_i32, false);
    for character in text.chars() {
        match character {
            '"' => quoted = !quoted,
            '[' | '{' if !quoted => depth += 1,
            ']' | '}' if !quoted => depth -= 1,
            _ => {}
        }
    }
    depth > 0
}

/// The dependency entries of a manifest's `[dependencies]`,
/// `[dev-dependencies]` and `[build-dependencies]` tables, every target's
/// included, or of its `[workspace.dependencies]` when `workspace`.
fn dependencies(text: &str, workspace: bool) -> Vec<Dependency> {
    let mut found: Vec<Dependency> = Vec::new();
    let mut section: Option<Option<String>> = None;
    let mut pending = String::new();
    for raw in text.lines() {
        pending.push_str(without_comment(raw));
        pending.push(' ');
        if continues(&pending) {
            continue;
        }
        let statement = std::mem::take(&mut pending);
        let trimmed = statement.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') {
            let header = trimmed.trim_matches(|character| character == '[' || character == ']');
            let parts: Vec<&str> = header.split('.').map(str::trim).collect();
            let tables = ["dependencies", "dev-dependencies", "build-dependencies"];
            section = if workspace {
                match parts.as_slice() {
                    ["workspace", "dependencies"] => Some(None),
                    ["workspace", "dependencies", name] => Some(Some((*name).to_owned())),
                    _ => None,
                }
            } else {
                let tail: Vec<&str> = match parts.first() {
                    Some(&"target") => {
                        let at = parts.iter().position(|part| tables.contains(part));
                        at.map_or_else(Vec::new, |at| parts[at..].to_vec())
                    }
                    _ => parts.clone(),
                };
                match tail.as_slice() {
                    [table] if tables.contains(table) => Some(None),
                    [table, name] if tables.contains(table) => Some(Some((*name).to_owned())),
                    _ => None,
                }
            };
            if let Some(Some(name)) = &section {
                found.push(Dependency {
                    key: name.clone(),
                    readable: true,
                    ..Dependency::default()
                });
            }
            continue;
        }
        let Some(current) = &section else {
            continue;
        };
        let parts = split_top(trimmed, '=');
        if parts.len() < 2 {
            continue;
        }
        let key = parts[0].trim();
        let value = parts[1..].join("=");
        if let Some(name) = current {
            if let Some(entry) = found.iter_mut().rev().find(|entry| entry.key == *name) {
                apply(entry, key, &value);
            }
            continue;
        }
        let (key, attribute) = key
            .split_once('.')
            .map_or((key, None), |(key, attribute)| (key, Some(attribute)));
        let key = key.trim().trim_matches('"').to_owned();
        let position = found.iter().position(|entry| entry.key == key);
        let entry = match position {
            Some(position) => &mut found[position],
            None => {
                found.push(Dependency {
                    key,
                    readable: true,
                    ..Dependency::default()
                });
                found.last_mut().expect("an entry was just pushed")
            }
        };
        if let Some(attribute) = attribute {
            apply(entry, attribute, &value);
            continue;
        }
        let value = value.trim();
        if let Some(inner) = value
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
        {
            for pair in split_top(inner, ',') {
                if let Some((name, value)) = pair.split_once('=') {
                    apply(entry, name, value);
                }
            }
        } else if string_value(value).is_none() {
            entry.readable = false;
        }
    }
    found
}

/// The `[package]` name a manifest states.
fn package_name(text: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package {
            if let Some((key, value)) = trimmed.split_once('=') {
                if key.trim() == "name" {
                    return string_value(value);
                }
            }
        }
    }
    None
}

/// The directory of the workspace's own `package`, relative to `root`.
fn package_directory(root: &Path, package: &str) -> Option<std::path::PathBuf> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let found: Vec<std::path::PathBuf> = dependencies(&text, true)
        .into_iter()
        .filter(|entry| entry.package.as_deref().unwrap_or(&entry.key) == package)
        .filter_map(|entry| entry.path.map(|path| normalize(&root.join(path))))
        .collect();
    let [directory] = found.as_slice() else {
        return None;
    };
    let stated = std::fs::read_to_string(directory.join("Cargo.toml")).ok()?;
    (package_name(&stated).as_deref() == Some(package)).then(|| directory.clone())
}

/// `path` with every `.` and `..` component applied.
fn normalize(path: &Path) -> std::path::PathBuf {
    let mut normal = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normal.pop();
            }
            std::path::Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal
}

/// Why the manifest of `krate`, under the workspace `root`, does not bind
/// `name` to its real crate, or `None`.
fn manifest_binding(root: &Path, krate: &str, name: &str) -> Option<String> {
    let Ok(text) = std::fs::read_to_string(root.join(krate).join("Cargo.toml")) else {
        return Some(format!(
            "{krate} has no Cargo.toml the census reads, so {name} names no crate it can prove"
        ));
    };
    let workspace = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let inherited = dependencies(&workspace, true);
    let entries: Vec<Dependency> = dependencies(&text, false)
        .into_iter()
        .filter(|entry| entry.key.replace('-', "_") == name)
        .collect();
    if STANDARD_ROOTS.contains(&name) {
        return (!entries.is_empty()).then(|| {
            format!("{krate} names a dependency {name}, which shadows the standard crate")
        });
    }
    if entries.is_empty() {
        return Some(format!("{krate} names no dependency {name}"));
    }
    for entry in entries {
        let (entry, base) = if entry.workspace {
            let Some(found) = inherited
                .iter()
                .find(|inherited| inherited.key == entry.key)
            else {
                return Some(format!(
                    "the dependency {} of {krate} is no workspace entry",
                    entry.key
                ));
            };
            let mut merged = found.clone();
            if entry.package.is_some() {
                merged.package = entry.package;
            }
            (merged, root.to_owned())
        } else {
            (entry, root.join(krate))
        };
        if !entry.readable {
            return Some(format!(
                "the dependency {} of {krate} is no entry the census reads",
                entry.key
            ));
        }
        let package = entry.package.clone().unwrap_or_else(|| entry.key.clone());
        if let Some(wanted) = wire_package(name) {
            let directory = entry.path.as_ref().map(|path| normalize(&base.join(path)));
            if package != wanted
                || directory.is_none()
                || directory != package_directory(root, wanted)
            {
                return Some(format!(
                    "the dependency {name} of {krate} is not the workspace's {wanted}"
                ));
            }
        } else if package.replace('-', "_") != name || entry.path.is_some() || entry.git {
            return Some(format!(
                "the dependency {name} of {krate} is not the registry crate {name}"
            ));
        }
    }
    None
}
