// SPDX-License-Identifier: Apache-2.0
//! The writing declaration of each read type in the three wire crates,
//! addressed by module path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::declarations::{module_scope, Declaration, Declarations};
use crate::{
    collect_rust_sources, derive_list_names_deserialize, derive_list_names_serialize,
    is_test_module, is_test_path, read_source, skip_meta_value, write_spelling, WriteSpelling,
};

/// The writing declaration of a read type the routing attributes do not name.
///
/// A read-only wire whose owner states `try_from` or `from` is paired by that
/// attribute, and a type that derives `Serialize` writes its own keys. What is
/// left is a wire named in a hand-written reader, a hand-written writer, or
/// an enum variant. Each entry gives the file, reading type, writing type,
/// and optional writing variant. The writing type resolves from the reading
/// type's own module.
const WRITING_DECLARATIONS: &[(&str, &str, &str, Option<&str>)] = &[
    // `Feature` writes through `serialization::FeatureWriteWire` on both
    // routes: the standalone `Serialize` impl builds one, and the model route
    // builds a `Vec<FeatureWriteWire>` beside the `Vec<FeatureRowWire>` it
    // reads.
    (
        "crates/cadmpeg-ir/src/features.rs",
        "FeatureReadWire",
        "serialization::FeatureWriteWire",
        None,
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "FeatureRowWire",
        "serialization::FeatureWriteWire",
        None,
    ),
    // `CadIr` writes through `CadIrWriteWire` in its hand-written `Serialize`.
    (
        "crates/cadmpeg-ir/src/document.rs",
        "CadIrReadWire",
        "CadIrWriteWire",
        None,
    ),
    // Each of these is read by a hand-written `Deserialize` impl for the type
    // named beside it, which derives `Serialize` and writes the keys.
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShapeWire",
        "HoleShape",
        None,
    ),
    (
        "crates/cadmpeg-ir/src/features/replace_face.rs",
        "ReplaceFaceOperandsWire",
        "ReplaceFaceOperands",
        None,
    ),
    (
        "crates/cadmpeg-ir/src/geometry/nurbs.rs",
        "NurbsSurfaceWire",
        "NurbsSurface",
        None,
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SweepCircularRegionWire",
        "SweepCircularRegion",
        None,
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "TreeChildrenWire",
        "TreeChildren",
        None,
    ),
    // `TSplineSubtransform` writes its `Inline` form by delegating to
    // `InlineTSplineSubtransform`, which derives `Serialize`.
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "TSplineSubtransformWire",
        "InlineTSplineSubtransform",
        None,
    ),
    (
        "crates/cadmpeg-ir/src/design/names.rs",
        "SplitName",
        "TopoName",
        Some("Split"),
    ),
    (
        "crates/cadmpeg-ir/src/design/names.rs",
        "MergedName",
        "TopoName",
        Some("Merged"),
    ),
];

type FieldSpellings = BTreeMap<String, Result<WriteSpelling, String>>;

pub(crate) struct WritingDeclaration {
    pub(crate) owner: Declaration,
    pub(crate) variant: Option<&'static str>,
}

impl From<Declaration> for WritingDeclaration {
    fn from(owner: Declaration) -> Self {
        Self {
            owner,
            variant: None,
        }
    }
}

/// Keys whose writing declaration is hand-written code, with the spelling read
/// at that code, as (file, reading type, key, spelling).
///
/// `WireMembers` serializes a map by hand: it writes `free_vertex` only for
/// the `Vertex` variant, so the key is left out for every other state.
///
/// `SweepSection` serializes through a hand-written `SectionWire`: it writes
/// `value` beside `kind: "unresolved"` only for a named section, so the
/// `Unresolved` payload's key is left out for an unnamed one.
pub(crate) const HAND_WRITTEN_KEYS: &[(&str, &str, &str, WriteSpelling)] = &[
    (
        "crates/cadmpeg-asm/src/brep/records.rs",
        "WireMembersWire",
        "free_vertex",
        WriteSpelling::Omitted,
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SweepSection",
        "<tuple field>",
        WriteSpelling::Omitted,
    ),
];

/// Every declaration of the three wire crates, indexed for writer resolution
/// by its module path: two modules' same-named types are two declarations.
pub(crate) struct WireIndex {
    pub(crate) declarations: Declarations,
    /// Per declaration that derives `Serialize`, the `None` spelling of each
    /// named field. A key one type writes two ways is absent here and present
    /// in `split_keys`.
    written_keys: BTreeMap<Declaration, BTreeMap<String, WriteSpelling>>,
    written_variants: BTreeMap<Declaration, BTreeMap<String, FieldSpellings>>,
    /// The `(type, key)` pairs one declaration writes two ways.
    split_keys: BTreeSet<(Declaration, String)>,
    /// The `(type, key)` pairs whose optionality the census cannot read,
    /// with the reason.
    unread_keys: BTreeMap<(Declaration, String), String>,
    /// Declarations that derive `Serialize`.
    writes_own_keys: BTreeSet<Declaration>,
    /// Declaration -> the type its container `into` or `remote` names, or
    /// why that name resolves to no one declaration.
    writes_through: BTreeMap<Declaration, Result<Declaration, String>>,
    /// Wire declaration -> every type whose container `try_from` or `from`
    /// routes a read through it.
    read_routers: BTreeMap<Declaration, BTreeSet<Declaration>>,
    /// Per last path segment, each type whose `try_from` or `from` names a
    /// wire that resolves to no one declaration: any wire of that name may be
    /// the one it reads through.
    unresolved_routes: BTreeMap<String, BTreeSet<Declaration>>,
    /// Declarations stated twice in one module, which makes their path no
    /// address.
    twice_declared: BTreeSet<Declaration>,
}

impl WireIndex {
    /// The writing declaration for the type `reader`, or why there is none.
    pub(crate) fn writing_declaration(
        &self,
        reader: &Declaration,
    ) -> Result<WritingDeclaration, String> {
        if self.twice_declared.contains(reader) {
            return Err(format!(
                "{reader} is declared twice in one module, so its path addresses no declaration"
            ));
        }
        if let Some(writer) = self.listed_writer(reader) {
            return writer;
        }
        if let Some(target) = self.writes_through.get(reader) {
            return target.clone().map(Into::into);
        }
        if self.writes_own_keys.contains(reader) {
            return Ok(reader.clone().into());
        }
        if let Some(owner) = self
            .unresolved_routes
            .get(&reader.name)
            .and_then(|owners| owners.iter().next())
        {
            return Err(format!(
                "{reader} may be read for {owner}, whose conversion names a {} that resolves to no one declaration",
                reader.name
            ));
        }
        let mut owners = self.read_routers.get(reader).into_iter().flatten();
        let (Some(owner), None) = (owners.next(), owners.next()) else {
            return Err(format!(
                "{reader} writes no keys of its own and no single container attribute routes a read through it"
            ));
        };
        if let Some(writer) = self.listed_writer(owner) {
            return writer;
        }
        if let Some(target) = self.writes_through.get(owner) {
            return target.clone().map(Into::into);
        }
        if self.writes_own_keys.contains(owner) {
            return Ok(owner.clone().into());
        }
        Err(format!(
            "{reader} is read for {owner}, which derives no Serialize and names no writing declaration"
        ))
    }

    /// The writer [`WRITING_DECLARATIONS`] states for `reader`, if listed.
    fn listed_writer(&self, reader: &Declaration) -> Option<Result<WritingDeclaration, String>> {
        WRITING_DECLARATIONS
            .iter()
            .find(|(file, named, _, _)| *file == reader.path && *named == reader.name)
            .map(|(_, _, writer, variant)| {
                self.declarations
                    .resolve_named(reader, writer)
                    .map(|owner| WritingDeclaration {
                        owner,
                        variant: *variant,
                    })
            })
    }

    /// How the writing declaration `writer` spells `None` at `key`.
    pub(crate) fn spelling(
        &self,
        writer: &WritingDeclaration,
        key: &str,
    ) -> Result<WriteSpelling, String> {
        let owner = &writer.owner;
        if let Some(variant) = writer.variant {
            return self
                .written_variants
                .get(owner)
                .and_then(|variants| variants.get(variant))
                .and_then(|fields| fields.get(key))
                .cloned()
                .unwrap_or_else(|| Err(format!("{owner}::{variant} writes no key named {key}")));
        }
        let writer = owner;
        let pair = (writer.clone(), key.to_owned());
        if self.split_keys.contains(&pair) {
            return Err(format!(
                "the writing declaration {writer} states {key} twice with different skips"
            ));
        }
        if let Some(reason) = self.unread_keys.get(&pair) {
            return Err(format!(
                "the writing declaration {writer} at {key}: {reason}"
            ));
        }
        self.written_keys
            .get(writer)
            .and_then(|keys| keys.get(key))
            .copied()
            .ok_or_else(|| format!("the writing declaration {writer} states no key {key}"))
    }
}

/// Reads every declaration of the three wire crates under `root` into a
/// [`WireIndex`].
pub(crate) fn wire_index(root: &Path) -> WireIndex {
    let mut index = WireIndex {
        declarations: Declarations::read(root),
        written_keys: BTreeMap::new(),
        written_variants: BTreeMap::new(),
        split_keys: BTreeSet::new(),
        unread_keys: BTreeMap::new(),
        writes_own_keys: BTreeSet::new(),
        writes_through: BTreeMap::new(),
        read_routers: BTreeMap::new(),
        unresolved_routes: BTreeMap::new(),
        twice_declared: BTreeSet::new(),
    };
    let mut seen = BTreeSet::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(root)
                .expect("a collected source sits under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let text = read_source(&file);
            let parsed = syn::parse_file(&text)
                .map_err(|error| format!("{relative} does not parse: {error}"))
                .expect("every source file in the wire crates parses");
            index_declarations(
                &parsed.items,
                &relative,
                &module_scope(&relative),
                &mut seen,
                &mut index,
            );
        }
    }
    index
}

/// Records every struct and enum among `items`, recursing into inline modules.
fn index_declarations(
    items: &[syn::Item],
    relative: &str,
    scope: &[String],
    seen: &mut BTreeSet<Declaration>,
    index: &mut WireIndex,
) {
    for item in items {
        let (name, attrs, generics, fields) = match item {
            syn::Item::Struct(declaration) => (
                declaration.ident.to_string(),
                &declaration.attrs,
                &declaration.generics,
                vec![(None, &declaration.fields)],
            ),
            syn::Item::Enum(declaration) => (
                declaration.ident.to_string(),
                &declaration.attrs,
                &declaration.generics,
                declaration
                    .variants
                    .iter()
                    .map(|variant| (Some(variant.ident.to_string()), &variant.fields))
                    .collect(),
            ),
            syn::Item::Mod(module) => {
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    let mut nested_scope = scope.to_owned();
                    nested_scope.push(module.ident.to_string());
                    index_declarations(nested, relative, &nested_scope, seen, index);
                }
                continue;
            }
            _ => continue,
        };
        let writes = attrs.iter().any(derive_list_names_serialize);
        if !writes && !attrs.iter().any(derive_list_names_deserialize) {
            continue;
        }
        let declared = Declaration {
            path: relative.to_owned(),
            scope: scope.to_owned(),
            name,
        };
        if !seen.insert(declared.clone()) {
            index.twice_declared.insert(declared.clone());
        }
        for key in ["into", "remote"] {
            if let Some(target) = container_conversion(attrs, key) {
                let target = index.declarations.resolve_named(&declared, &target);
                index.writes_through.insert(declared.clone(), target);
            }
        }
        for attribute in attrs {
            for key in ["try_from", "from"] {
                let Some(wire) = container_conversion(std::slice::from_ref(attribute), key) else {
                    continue;
                };
                match index.declarations.resolve_named(&declared, &wire) {
                    Ok(wire) => {
                        index
                            .read_routers
                            .entry(wire)
                            .or_default()
                            .insert(declared.clone());
                    }
                    Err(_) => {
                        let last = wire.rsplit("::").next().unwrap_or(&wire);
                        let last = last.split('<').next().unwrap_or(last).trim().to_owned();
                        index
                            .unresolved_routes
                            .entry(last)
                            .or_default()
                            .insert(declared.clone());
                    }
                }
            }
        }
        if !writes {
            continue;
        }
        index.writes_own_keys.insert(declared.clone());
        for (variant, group) in fields {
            let mut variant_fields = BTreeMap::new();
            for field in group {
                let Some(key) = field.ident.as_ref().map(syn::Ident::to_string) else {
                    continue;
                };
                let optional = index
                    .declarations
                    .is_optional(&declared, generics, &field.ty);
                let spelling = optional.map(|optional| write_spelling(field, optional));
                if variant.is_some() {
                    variant_fields.insert(key.clone(), spelling.clone());
                }
                let spelling = match spelling {
                    Ok(spelling) => spelling,
                    Err(reason) => {
                        index.unread_keys.insert((declared.clone(), key), reason);
                        continue;
                    }
                };
                let keys = index.written_keys.entry(declared.clone()).or_default();
                match keys.insert(key.clone(), spelling) {
                    Some(earlier) if earlier != spelling => {
                        index.split_keys.insert((declared.clone(), key));
                    }
                    Some(_) | None => {}
                }
            }
            if let Some(variant) = variant {
                index
                    .written_variants
                    .entry(declared.clone())
                    .or_default()
                    .insert(variant, variant_fields);
            }
        }
    }
}

/// The string the container-level `serde` key `key` states, if any: the type a
/// conversion names, or the `content` key of an adjacently tagged enum.
pub(crate) fn container_conversion(attrs: &[syn::Attribute], key: &str) -> Option<String> {
    let mut named = None;
    for attribute in attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                let wanted = meta.path.is_ident(key);
                if meta.input.peek(syn::Token![=]) {
                    let literal: syn::Lit = meta.value()?.parse()?;
                    if let syn::Lit::Str(text) = &literal {
                        if wanted {
                            named = Some(text.value());
                        }
                    }
                } else {
                    skip_meta_value(&meta)?;
                }
                Ok(())
            })
            .expect(
                "every container serde attribute in the wire crates parses as a nested meta list",
            );
    }
    named
}
