// SPDX-License-Identifier: Apache-2.0
//! Proof of the bounded version-prefix map delegation grammar.
//!
//! This recognizes a complete forwarding structure, not a helper's name. Every
//! consumed prefix pair and the remaining MapAccess must reach the denied wire.
//! An unrecognized edit is an unresolved route and fails the ordinary census.

use syn::spanned::Spanned;

use super::resolve::{resolve_receiver_path, ResolvedPath};
use super::{
    nodes_from_stream, path_shape_from_syn, signature_deserializer_bindings, source_span_text,
    HandImplSource, InputRoute, PathShape, Receiver, SourceIndex, SymbolKey, SymbolKind, TypeShape,
};

fn forwarding_impl(item: &syn::ItemImpl) -> bool {
    item.trait_
        .as_ref()
        .and_then(|(_, path, _)| path.segments.last())
        .is_some_and(|segment| {
            matches!(
                segment.ident.to_string().as_str(),
                "Visitor" | "MapAccess" | "DeserializeSeed"
            )
        })
}

pub(super) fn collect_module(
    items: &[syn::Item],
    path: &str,
    source: &str,
    scope: &[String],
    index: &mut SourceIndex,
) {
    let targets: std::collections::BTreeSet<_> = items
        .iter()
        .filter_map(|item| {
            let syn::Item::Impl(item) = item else {
                return None;
            };
            if !forwarding_impl(item) {
                return None;
            }
            let syn::Type::Path(ty) = item.self_ty.as_ref() else {
                return None;
            };
            Some(ty.path.segments.last()?.ident.to_string())
        })
        .collect();
    let retained: Vec<_> = items
        .iter()
        .filter(|item| match item {
            syn::Item::Impl(item) => forwarding_impl(item),
            syn::Item::Struct(item) => targets.contains(&item.ident.to_string()),
            syn::Item::Fn(item) => item.block.stmts.iter().any(|statement| {
                matches!(statement,
            syn::Stmt::Item(syn::Item::Impl(item)) if forwarding_impl(item))
            }),
            _ => false,
        })
        .cloned()
        .collect();
    if !retained.is_empty() {
        index
            .module_items
            .insert((path.to_owned(), scope.to_owned()), retained);
        index
            .source_text
            .entry(path.to_owned())
            .or_insert_with(|| source.to_owned());
    }
}

fn tokens(text: &str) -> Vec<super::Node> {
    nodes_from_stream(&text.parse().expect("parse prefix-proof grammar"))
}

fn same(source: &str, span: proc_macro2::Span, expected: &str) -> bool {
    tokens(&source_span_text(source, span)) == tokens(expected)
}

fn context(symbol: &SymbolKey) -> HandImplSource {
    HandImplSource {
        path: symbol.path.clone(),
        scope: symbol.scope.clone(),
        name: symbol.name.clone(),
        body: String::new(),
        method_nodes: Vec::new(),
        method_block: None,
        deserializer_bindings: Default::default(),
        local_denied: Default::default(),
    }
}

fn method<'a>(implementation: &'a syn::ItemImpl, name: &str) -> Option<&'a syn::ImplItemFn> {
    implementation.items.iter().find_map(|item| match item {
        syn::ImplItem::Fn(method) if method.sig.ident == name => Some(method),
        _ => None,
    })
}

fn implementation<'a>(
    items: &'a [syn::Item],
    target: &str,
    trait_name: &str,
) -> Option<&'a syn::ItemImpl> {
    items.iter().find_map(|item| {
        let syn::Item::Impl(item) = item else {
            return None;
        };
        let syn::Type::Path(ty) = item.self_ty.as_ref() else {
            return None;
        };
        (ty.path.is_ident(target) && item.trait_.as_ref()?.1.segments.last()?.ident == trait_name)
            .then_some(item)
    })
}

const VISIT_MAP: &str = r#"{
    use serde::Deserialize;
    let mut prefix = Vec::new();
    while let Some(key) = map.next_key::<String>()? {
        let value = map.next_value_seed(prefix::PrefixValue)?;
        let version_found = key == "ir_version";
        if version_found { super::check_ir_version(Some(&value))?; }
        prefix.push((key, value));
        if version_found {
            return super::CadIrReadWire::deserialize(
                serde::de::value::MapAccessDeserializer::new(VersionedFields {
                    prefix: prefix.into_iter(), pending: None, remaining: map,
                }),
            );
        }
    }
    Err(serde::de::Error::custom(unsupported_version(None)))
}"#;

const NEXT_KEY: &str = r#"{
    use serde::de::IntoDeserializer;
    if let Some((key, value)) = self.prefix.next() {
        self.pending = Some(value);
        seed.deserialize(key.into_deserializer()).map(Some)
    } else { self.remaining.next_key_seed(seed) }
}"#;

const NEXT_VALUE: &str = r#"{
    if let Some(value) = self.pending.take() {
        seed.deserialize(value).map_err(serde::de::Error::custom)
    } else { self.remaining.next_value_seed(seed) }
}"#;

fn adapter_is_forwarding(items: &[syn::Item], source: &str) -> bool {
    let Some(adapter) = implementation(items, "VersionedFields", "MapAccess") else {
        return false;
    };
    let Some(key) = method(adapter, "next_key_seed") else {
        return false;
    };
    let Some(value) = method(adapter, "next_value_seed") else {
        return false;
    };
    adapter.items.len() == 3
        && adapter.items.iter().any(|item| matches!(item, syn::ImplItem::Type(item)
            if same(source, item.span(), "type Error = A::Error;")))
        && same(source, key.block.span(), NEXT_KEY)
        && same(source, value.block.span(), NEXT_VALUE)
        && items.iter().any(|item| matches!(item, syn::Item::Struct(item)
            if same(source, item.span(), "struct VersionedFields<A> { prefix: std::vec::IntoIter<(String, serde_json::Value)>, pending: Option<serde_json::Value>, remaining: A, }")))
}

fn prefix_preserves_keys(symbol: &SymbolKey, index: &SourceIndex) -> bool {
    let route = context(symbol);
    let resolved = resolve_receiver_path(
        &route,
        &PathShape {
            segments: vec!["prefix".into(), "PrefixValue".into()],
            absolute: false,
        },
        index,
    );
    let ResolvedPath::Local(prefix) = resolved else {
        return false;
    };
    let Some(items) = index
        .module_items
        .get(&(prefix.path.clone(), prefix.scope.clone()))
    else {
        return false;
    };
    let Some(source) = index.source_text.get(&prefix.path) else {
        return false;
    };
    let Some(seed) = implementation(items, "PrefixValue", "DeserializeSeed") else {
        return false;
    };
    let Some(read) = method(seed, "deserialize") else {
        return false;
    };
    let Some(visitor) = implementation(items, "PrefixValue", "Visitor") else {
        return false;
    };
    let Some(map) = method(visitor, "visit_map") else {
        return false;
    };
    let Some(sequence) = method(visitor, "visit_seq") else {
        return false;
    };
    let prefix_route = context(&prefix);
    if !external_path(
        &prefix_route,
        index,
        &["serde", "de", "value", "MapAccessDeserializer"],
        &["serde", "de", "value", "MapAccessDeserializer"],
    ) || !external_path(&prefix_route, index, &["Value"], &["serde_json", "Value"])
        || !external_path(&prefix_route, index, &["Vec"], &["std", "vec", "Vec"])
    {
        return false;
    }
    same(source, read.block.span(), "{ deserializer.deserialize_any(self) }")
        && same(source, map.block.span(), "{ cadmpeg_core::distinct_keys::json_object(serde::de::value::MapAccessDeserializer::new(access)).map(Value::Object) }")
        && same(source, sequence.block.span(), "{ let mut values = Vec::new(); while let Some(value) = access.next_element_seed(Self)? { values.push(value); } Ok(Value::Array(values)) }")
        && matches!(resolve_receiver_path(&context(&prefix), &PathShape {
            segments: vec!["cadmpeg_core".into(), "distinct_keys".into(), "json_object".into()], absolute: false,
        }, index), ResolvedPath::External(path) if super::scan::path_matches(&path, &["cadmpeg_core", "distinct_keys", "json_object"]))
}

fn external_path(
    route: &HandImplSource,
    index: &SourceIndex,
    names: &[&str],
    expected: &[&str],
) -> bool {
    matches!(resolve_receiver_path(route, &PathShape {
        segments: names.iter().map(|name| (*name).to_owned()).collect(), absolute: false,
    }, index), ResolvedPath::External(path) if super::scan::path_matches(&path, expected))
}

pub(super) fn closed_route(resolved: &ResolvedPath, index: &SourceIndex) -> Option<InputRoute> {
    let ResolvedPath::Local(symbol) = resolved else {
        return None;
    };
    if symbol.kind != SymbolKind::Function {
        return None;
    }
    let items = index
        .module_items
        .get(&(symbol.path.clone(), symbol.scope.clone()))?;
    let source = index.source_text.get(&symbol.path)?;
    let function = items.iter().find_map(|item| match item {
        syn::Item::Fn(function) if function.sig.ident == symbol.name => Some(function),
        _ => None,
    })?;
    if function.sig.inputs.len() != 1
        || signature_deserializer_bindings(&function.sig)
            != std::collections::BTreeSet::from(["deserializer".to_owned()])
    {
        return None;
    }
    let [syn::Stmt::Item(syn::Item::Struct(marker)), syn::Stmt::Item(syn::Item::Impl(visitor)), syn::Stmt::Expr(tail, None)] =
        function.block.stmts.as_slice()
    else {
        return None;
    };
    if visitor
        .trait_
        .as_ref()?
        .1
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        != ["serde", "de", "Visitor"]
        || !matches!(visitor.self_ty.as_ref(), syn::Type::Path(ty) if ty.path.is_ident("VersionedDocument"))
        || !same(source, marker.span(), "struct VersionedDocument;")
        || !same(
            source,
            tail.span(),
            "deserializer.deserialize_map(VersionedDocument)",
        )
        || visitor.items.len() != 3
        || !same(
            source,
            method(visitor, "visit_map")?.block.span(),
            VISIT_MAP,
        )
        || !adapter_is_forwarding(items, source)
        || !prefix_preserves_keys(symbol, index)
    {
        return None;
    }
    let value = visitor.items.iter().find_map(|item| match item {
        syn::ImplItem::Type(value) if value.ident == "Value" => Some(&value.ty),
        _ => None,
    })?;
    let syn::Type::Path(value) = value else {
        return None;
    };
    let TypeShape::Path(value) = path_shape_from_syn(&value.path)? else {
        return None;
    };
    if value.segments != ["super", "CadIrReadWire"] {
        return None;
    }
    let route = context(symbol);
    for names in [
        &["serde", "de", "Visitor"][..],
        &["serde", "de", "value", "MapAccessDeserializer"][..],
        &["serde_json", "Value"][..],
    ] {
        if !external_path(&route, index, names, names) {
            return None;
        }
    }
    if !external_path(&route, index, &["Vec"], &["std", "vec", "Vec"])
        || !external_path(&route, index, &["String"], &["std", "string", "String"])
    {
        return None;
    }
    let ResolvedPath::Local(wire) = resolve_receiver_path(&route, &value, index) else {
        return None;
    };
    let ResolvedPath::Local(gate) = resolve_receiver_path(
        &route,
        &PathShape {
            segments: vec!["super".into(), "check_ir_version".into()],
            absolute: false,
        },
        index,
    ) else {
        return None;
    };
    if gate.path != "crates/cadmpeg-ir/src/document.rs"
        || gate.name != "check_ir_version"
        || gate.kind != SymbolKind::Function
        || !index
            .denied
            .iter()
            .any(|key| key.path == wire.path && key.scope == wire.scope && key.name == wire.name)
    {
        return None;
    }
    let mut segments = vec!["crate".to_owned()];
    segments.extend(wire.scope);
    segments.push(wire.name);
    Some(InputRoute::Wire(Receiver::Path(PathShape {
        segments,
        absolute: false,
    })))
}
