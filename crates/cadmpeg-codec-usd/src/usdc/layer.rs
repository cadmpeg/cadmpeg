// SPDX-License-Identifier: Apache-2.0
//! The spec tree of a USDC layer as the parsed form of a text layer
//! (spec §9.8).
//!
//! Each spec maps to the statement the text encoding states for it, so
//! composition and the model read both encodings through one tree.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::sections::{PathNode, Structure, FIELD_RUN_END};
use super::value::{real, Datum, Item, ListOpValue, Unpacker};
use crate::text::grammar::DOCUMENTATION_KEY;
use crate::text::syntax::{
    AttributeField, AttributeSpec, AttributeValue, Dictionary, DictionaryEntry, DictionaryValue,
    Layer, MetaItem, MetaValue, Number, PathList, PrimBody, PrimSpec, PropertySpec, Reference,
    RelationshipField, RelationshipSpec, Specifier, SubLayer, TimeSample, Value, Variability,
    Variant, VariantSet,
};

/// Spec types (spec §9.5).
const ATTRIBUTE: u32 = 1;
const PRIM: u32 = 6;
const PSEUDO_ROOT: u32 = 7;
const RELATIONSHIP: u32 = 8;
const VARIANT: u32 = 10;
const VARIANT_SET: u32 = 11;

fn malformed(what: &str) -> CodecError {
    CodecError::malformed(format_args!("corrupt USDC layer: {what}"))
}

/// A number literal: integral values carry their exact integer, as their
/// text form does.
fn number(float: f64) -> Number {
    let integer = (float.is_finite() && float.fract() == 0.0)
        .then(|| cadmpeg_core::convert::truncate_f64_to_i64(float))
        .flatten()
        .map(i128::from);
    Number { float, integer }
}

/// The text form of the path of `node` under `parent`.
fn join(parent: &str, element: &str, property: bool) -> String {
    if property {
        format!("{parent}.{element}")
    } else if parent == "/" {
        format!("/{element}")
    } else if element.starts_with(['{', '[']) || parent.ends_with('}') {
        format!("{parent}{element}")
    } else {
        format!("{parent}/{element}")
    }
}

/// The text form of every path of the table.
fn path_strings(
    ctx: &DecodeContext<'_>,
    structure: &Structure<'_>,
) -> Result<Vec<Option<String>>, CodecError> {
    let nodes = &structure.paths;
    let mut out: Vec<Option<String>> = ctx.alloc_filled(nodes.len(), None, "usdc_path_text")?;
    for start in 0..nodes.len() {
        let mut pending = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            if out[index].is_some() {
                break;
            }
            let Some(node) = &nodes[index] else {
                break;
            };
            pending.push(index);
            if pending.len() > nodes.len() {
                return Err(malformed("the path parents form a cycle"));
            }
            current = node.parent;
        }
        while let Some(index) = pending.pop() {
            let Some(PathNode {
                parent,
                element,
                property,
            }) = &nodes[index]
            else {
                continue;
            };
            let text = match parent {
                None => "/".to_owned(),
                Some(parent) => {
                    let Some(parent) = &out[*parent] else {
                        continue;
                    };
                    let element = structure
                        .tokens
                        .get(*element)
                        .ok_or_else(|| malformed("a path element names no token"))?;
                    join(parent, element, *property)
                }
            };
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(text.len()),
                "usdc_path_text",
            )?;
            out[index] = Some(text);
        }
    }
    Ok(out)
}

/// One spec with its decoded fields.
struct Node {
    spec_type: u32,
    /// The last path element.
    element: String,
    fields: Vec<(String, Datum)>,
    /// Child specs, in path-table order.
    children: Vec<usize>,
}

impl Node {
    fn take(&mut self, name: &str) -> Option<Datum> {
        let index = self.fields.iter().position(|(key, _)| key == name)?;
        Some(self.fields.remove(index).1)
    }
}

/// A token list field, or an empty list.
fn names(datum: Option<Datum>) -> Vec<String> {
    match datum {
        Some(Datum::Tokens(names) | Datum::Strings(names)) => names,
        _ => Vec::new(),
    }
}

struct Builder<'s, 'a> {
    ctx: &'s DecodeContext<'a>,
    nodes: Vec<Option<Node>>,
    unread: &'s mut BTreeSet<&'static str>,
}

/// The value text of a token or string value.
fn text(datum: Datum) -> Option<String> {
    match datum {
        Datum::Value {
            value: Value::String(text) | Value::Asset(text),
            ..
        } => Some(text),
        _ => None,
    }
}

fn list_values(items: Vec<Item>) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Item::Value(value) => Some(value),
            Item::Path(path) => Some(Value::Path(path)),
            Item::Reference(_) => None,
        })
        .collect()
}

fn item_paths(items: Vec<Item>) -> Vec<String> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Item::Path(path) => Some(path),
            _ => None,
        })
        .collect()
}

fn item_strings(items: Vec<Item>) -> Vec<String> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Item::Value(Value::String(text)) => Some(text),
            _ => None,
        })
        .collect()
}

fn item_references(items: Vec<Item>) -> Vec<Reference> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Item::Reference(reference) => Some(reference),
            _ => None,
        })
        .collect()
}

/// One metadata item per part of a list operation.
fn list_items(
    key: &str,
    list: ListOpValue<Item>,
    value: impl Fn(Vec<Item>) -> MetaValue,
) -> Vec<MetaItem> {
    list.parts
        .into_iter()
        .map(|(list_op, items)| MetaItem {
            list_op,
            key: key.to_owned(),
            value: value(items),
        })
        .collect()
}

fn item(key: &str, value: MetaValue) -> Vec<MetaItem> {
    vec![MetaItem {
        list_op: None,
        key: key.to_owned(),
        value,
    }]
}

fn pairs(dictionary: Dictionary) -> Vec<(String, String)> {
    dictionary
        .entries
        .into_iter()
        .filter_map(|entry| match entry.value {
            DictionaryValue::Typed {
                value: Value::String(text),
                ..
            } => Some((entry.key, text)),
            _ => None,
        })
        .collect()
}

impl Builder<'_, '_> {
    /// The metadata items of field `key`.
    fn metadata(&mut self, key: &str, datum: Datum) -> Vec<MetaItem> {
        match (key, datum) {
            ("documentation", datum) => match text(datum) {
                Some(doc) => item("doc", MetaValue::Value(Value::String(doc))),
                None => Vec::new(),
            },
            ("comment", datum) => match text(datum) {
                Some(comment) => item(DOCUMENTATION_KEY, MetaValue::Value(Value::String(comment))),
                None => Vec::new(),
            },
            ("references", Datum::ListOp(list)) => list_items(key, list, |items| {
                MetaValue::References(item_references(items))
            }),
            ("payload", Datum::ListOp(list)) => list_items(key, list, |items| {
                MetaValue::Payloads(item_references(items))
            }),
            ("payload", Datum::Payload(payload)) => item(key, MetaValue::Payloads(vec![payload])),
            ("inherits" | "specializes", Datum::ListOp(list)) => {
                list_items(key, list, |items| MetaValue::Paths(item_paths(items)))
            }
            ("variantSetNames", Datum::ListOp(list)) => list_items("variantSets", list, |items| {
                MetaValue::Names(item_strings(items))
            }),
            ("variantSelection", Datum::Selections(selections)) => item(
                "variants",
                MetaValue::Dictionary(Dictionary {
                    entries: selections
                        .into_iter()
                        .map(|(set, variant)| DictionaryEntry {
                            key: set,
                            value: DictionaryValue::Typed {
                                type_name: "string".to_owned(),
                                array: false,
                                value: Value::String(variant),
                            },
                        })
                        .collect(),
                }),
            ),
            ("relocates" | "layerRelocates", Datum::Relocates(pairs)) => {
                item("relocates", MetaValue::Relocates(pairs))
            }
            ("prefixSubstitutions" | "suffixSubstitutions", Datum::Dictionary(dictionary)) => {
                item(key, MetaValue::StringMap(pairs(dictionary)))
            }
            ("permission", Datum::Permission(permission)) => item(
                key,
                MetaValue::Identifier(Some(
                    if permission == 0 { "public" } else { "private" }.to_owned(),
                )),
            ),
            ("symmetryFunction" | "displayUnit", datum) => item(
                key,
                MetaValue::Identifier(text(datum).filter(|name| !name.is_empty())),
            ),
            (_, datum) => self.generic(key, datum),
        }
    }

    fn generic(&mut self, key: &str, datum: Datum) -> Vec<MetaItem> {
        let list = |values: Vec<Value>| MetaValue::Value(Value::List(values));
        let value = match datum {
            Datum::Value { value, .. } => MetaValue::Value(value),
            Datum::Dictionary(dictionary) => MetaValue::Dictionary(dictionary),
            Datum::ListOp(list_op) => {
                return list_items(key, list_op, |items| {
                    MetaValue::Value(Value::List(list_values(items)))
                })
            }
            Datum::Paths(paths) => list(paths.into_iter().map(Value::Path).collect()),
            Datum::Tokens(texts) | Datum::Strings(texts) => {
                list(texts.into_iter().map(Value::String).collect())
            }
            Datum::Doubles(values) => list(
                values
                    .into_iter()
                    .map(|value| Value::Number(number(value)))
                    .collect(),
            ),
            Datum::Selections(pairs) => MetaValue::StringMap(pairs),
            Datum::Relocates(pairs) => MetaValue::Relocates(pairs),
            Datum::Block => MetaValue::None,
            Datum::Unread(what) => {
                self.unread.insert(what);
                return Vec::new();
            }
            Datum::LayerOffsets(_)
            | Datum::Specifier(_)
            | Datum::Permission(_)
            | Datum::Variability(_)
            | Datum::TimeSamples(_)
            | Datum::Payload(_)
            | Datum::AnimationBlock => {
                self.unread.insert("metadata value");
                return Vec::new();
            }
        };
        item(key, value)
    }

    fn take(&mut self, index: usize) -> Result<Node, CodecError> {
        self.nodes
            .get_mut(index)
            .and_then(Option::take)
            .ok_or_else(|| malformed("a spec is reached twice"))
    }

    /// The children of `node` that `accept` admits, in the order `order`
    /// names them, then the rest in table order.
    fn ordered(&self, node: &Node, accept: impl Fn(&Node) -> bool, order: &[String]) -> Vec<usize> {
        let mut children: Vec<(usize, usize)> = node
            .children
            .iter()
            .filter_map(|child| {
                let spec = self.nodes.get(*child)?.as_ref()?;
                accept(spec).then(|| {
                    let rank = order
                        .iter()
                        .position(|name| *name == variant_name(&spec.element))
                        .unwrap_or(order.len());
                    (rank, *child)
                })
            })
            .collect();
        children.sort_by_key(|(rank, _)| *rank);
        children.into_iter().map(|(_, child)| child).collect()
    }

    fn body(&mut self, node: &mut Node) -> Result<PrimBody, CodecError> {
        let ctx = self.ctx;
        let _depth = ctx.enter_nested("usdc_prim_nesting")?;
        let child_names = names(node.take("primChildren"));
        let property_names = names(node.take("properties"));
        let set_names = names(node.take("variantSetChildren"));
        let mut body = PrimBody::default();
        if let Some(order) = node.take("primOrder") {
            body.child_orders.push(names(Some(order)));
        }
        if let Some(order) = node.take("propertyOrder") {
            body.property_orders.push(names(Some(order)));
        }
        for child in self.ordered(node, |spec| spec.spec_type == PRIM, &child_names) {
            let child = self.take(child)?;
            body.children.push(self.prim(child)?);
        }
        let properties = self.ordered(
            node,
            |spec| matches!(spec.spec_type, ATTRIBUTE | RELATIONSHIP),
            &property_names,
        );
        for child in properties {
            let child = self.take(child)?;
            if child.spec_type == ATTRIBUTE {
                self.attribute(child, &mut body.properties);
            } else {
                self.relationship(child, &mut body.properties);
            }
        }
        for set in self.ordered(node, |spec| spec.spec_type == VARIANT_SET, &set_names) {
            let mut set = self.take(set)?;
            let set_name = variant_name(&set.element).to_owned();
            let variant_names = names(set.take("variantChildren"));
            // A variant path is a child of the prim path, not of the variant
            // set path.
            let in_set = |spec: &Node| {
                spec.spec_type == VARIANT && variant_set_name(&spec.element) == set_name
            };
            let mut variants = Vec::new();
            for variant in self.ordered(node, in_set, &variant_names) {
                let mut variant = self.take(variant)?;
                let body = self.body(&mut variant)?;
                let name = variant_name(&variant.element).to_owned();
                variants.push(Variant {
                    name,
                    metadata: self.remaining_metadata(&mut variant),
                    body,
                });
            }
            body.variant_sets.push(VariantSet {
                name: set_name,
                variants,
            });
        }
        Ok(body)
    }

    fn remaining_metadata(&mut self, node: &mut Node) -> Vec<MetaItem> {
        let mut out = Vec::new();
        for (key, datum) in std::mem::take(&mut node.fields) {
            out.extend(self.metadata(&key, datum));
        }
        out
    }

    fn prim(&mut self, mut node: Node) -> Result<PrimSpec, CodecError> {
        let specifier = match node.take("specifier") {
            Some(Datum::Specifier(0)) => Specifier::Def,
            Some(Datum::Specifier(2)) => Specifier::Class,
            _ => Specifier::Over,
        };
        let type_name = node
            .take("typeName")
            .and_then(text)
            .filter(|name| !name.is_empty());
        let body = self.body(&mut node)?;
        Ok(PrimSpec {
            specifier,
            type_name,
            name: node.element.clone(),
            metadata: self.remaining_metadata(&mut node),
            body,
            offset: 0,
        })
    }

    fn attribute(&mut self, mut node: Node, out: &mut Vec<PropertySpec>) {
        let declared = node.take("typeName").and_then(text).unwrap_or_default();
        let (type_name, array) = match declared.strip_suffix("[]") {
            Some(base) => (base.to_owned(), true),
            None => (declared, false),
        };
        let custom = matches!(node.take("custom"), Some(Datum::Value { value: Value::Identifier(word), .. }) if word == "true");
        let variability = match node.take("variability") {
            Some(Datum::Variability(1)) => Variability::Uniform,
            _ => Variability::Varying,
        };
        let default = node
            .take("default")
            .and_then(|datum| self.attribute_value(datum));
        let samples = match node.take("timeSamples") {
            Some(Datum::TimeSamples(samples)) => Some(
                samples
                    .into_iter()
                    .filter_map(|(time, datum)| {
                        Some(TimeSample {
                            time: number(time),
                            value: self.attribute_value(datum)?,
                        })
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let connections = match node.take("connectionPaths") {
            Some(Datum::ListOp(list)) => list.parts,
            _ => Vec::new(),
        };
        if node.take("spline").is_some() {
            self.unread.insert("spline");
        }
        node.take("connectionChildren");
        let metadata = self.remaining_metadata(&mut node);
        let spec = |list_op, field, metadata| {
            PropertySpec::Attribute(AttributeSpec {
                list_op,
                custom,
                variability,
                type_name: type_name.clone(),
                array,
                name: node.element.clone(),
                field,
                metadata,
                offset: 0,
            })
        };
        let primary = match default {
            Some(value) => Some(AttributeField::Default(value)),
            None if !metadata.is_empty() || (samples.is_none() && connections.is_empty()) => {
                Some(AttributeField::Declared)
            }
            None => None,
        };
        let mut metadata = Some(metadata);
        if let Some(field) = primary {
            out.push(spec(None, field, metadata.take().unwrap_or_default()));
        }
        if let Some(samples) = samples {
            out.push(spec(
                None,
                AttributeField::TimeSamples(samples),
                metadata.take().unwrap_or_default(),
            ));
        }
        for (list_op, items) in connections {
            let paths = item_paths(items);
            let list = if paths.is_empty() {
                PathList::None
            } else {
                PathList::Paths(paths)
            };
            out.push(spec(
                list_op,
                AttributeField::Connect(list),
                metadata.take().unwrap_or_default(),
            ));
        }
    }

    fn attribute_value(&mut self, datum: Datum) -> Option<AttributeValue> {
        match datum {
            Datum::Value { value, .. } => Some(AttributeValue::Value(value)),
            Datum::Block => Some(AttributeValue::None),
            Datum::AnimationBlock => Some(AttributeValue::AnimationBlock),
            Datum::Tokens(texts) | Datum::Strings(texts) => Some(AttributeValue::Value(
                Value::List(texts.into_iter().map(Value::String).collect()),
            )),
            Datum::Doubles(values) => Some(AttributeValue::Value(Value::List(
                values.into_iter().map(real).collect(),
            ))),
            Datum::Unread(what) => {
                self.unread.insert(what);
                None
            }
            _ => {
                self.unread.insert("attribute value");
                None
            }
        }
    }

    fn relationship(&mut self, mut node: Node, out: &mut Vec<PropertySpec>) {
        let custom = matches!(node.take("custom"), Some(Datum::Value { value: Value::Identifier(word), .. }) if word == "true");
        let varying = matches!(node.take("variability"), Some(Datum::Variability(0)));
        let targets = match node.take("targetPaths") {
            Some(Datum::ListOp(list)) => list.parts,
            _ => Vec::new(),
        };
        node.take("targetChildren");
        let mut metadata = Some(self.remaining_metadata(&mut node));
        let spec = |list_op, field, metadata| {
            PropertySpec::Relationship(RelationshipSpec {
                list_op,
                custom,
                varying,
                name: node.element.clone(),
                field,
                metadata,
                offset: 0,
            })
        };
        if targets.is_empty() {
            out.push(spec(
                None,
                RelationshipField::Declared,
                metadata.take().unwrap_or_default(),
            ));
        }
        for (list_op, items) in targets {
            let paths = item_paths(items);
            let list = if paths.is_empty() {
                PathList::None
            } else {
                PathList::Paths(paths)
            };
            out.push(spec(
                list_op,
                RelationshipField::Targets(list),
                metadata.take().unwrap_or_default(),
            ));
        }
    }

    fn layer(&mut self, mut root: Node) -> Result<Layer, CodecError> {
        let root_names = names(root.take("primChildren"));
        let root_order = root.take("primOrder").map(|order| names(Some(order)));
        let sublayers = names(root.take("subLayers"));
        let offsets = match root.take("subLayerOffsets") {
            Some(Datum::LayerOffsets(offsets)) => offsets,
            _ => Vec::new(),
        };
        let mut metadata = Vec::new();
        if !sublayers.is_empty() {
            let layers = sublayers
                .into_iter()
                .enumerate()
                .map(|(index, asset)| {
                    let (offset, scale) = offsets.get(index).copied().unwrap_or((0.0, 1.0));
                    SubLayer {
                        asset,
                        offset: (offset != 0.0).then(|| number(offset)),
                        scale: (scale != 1.0).then(|| number(scale)),
                    }
                })
                .collect();
            metadata.push(MetaItem {
                list_op: None,
                key: "subLayers".to_owned(),
                value: MetaValue::SubLayers(layers),
            });
        }
        metadata.extend(self.remaining_metadata(&mut root));
        let mut prims = Vec::new();
        for child in self.ordered(&root, |spec| spec.spec_type == PRIM, &root_names) {
            let child = self.take(child)?;
            prims.push(self.prim(child)?);
        }
        Ok(Layer {
            metadata,
            prims,
            root_order,
        })
    }
}

/// The variant set a variant element `{set=var}` names.
fn variant_set_name(element: &str) -> &str {
    element
        .strip_prefix('{')
        .and_then(|rest| rest.split_once('='))
        .map_or(element, |(set, _)| set)
}

/// The name a variant or variant-set element states: `var` of
/// `{set=var}`, or `set` of `{set=}`.
fn variant_name(element: &str) -> &str {
    let Some(inner) = element
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return element;
    };
    match inner.split_once('=') {
        Some((set, "")) => set,
        Some((_, variant)) => variant,
        None => inner,
    }
}

/// Builds the parsed layer of `structure`, and names the kinds of value it
/// did not read.
pub(super) fn build(
    ctx: &DecodeContext<'_>,
    structure: &Structure<'_>,
) -> Result<(Layer, BTreeSet<&'static str>), CodecError> {
    let paths = path_strings(ctx, structure)?;
    let mut unpacker = Unpacker::new(ctx, structure, &paths);
    let mut by_path: Vec<Option<usize>> = ctx.alloc_filled(paths.len(), None, "usdc_specs")?;
    let mut nodes = Vec::new();
    let mut root = None;
    for spec in &structure.specs {
        let slot = by_path
            .get_mut(spec.path)
            .ok_or_else(|| malformed("a spec names no path"))?;
        if slot.is_some() {
            return Err(malformed("two specs name one path"));
        }
        *slot = Some(nodes.len());
        if spec.kind == PSEUDO_ROOT {
            root = Some(nodes.len());
        }
        let mut fields = Vec::new();
        for field in structure.field_sets.iter().skip(spec.field_set) {
            if *field == FIELD_RUN_END {
                break;
            }
            let field = usize::try_from(*field)
                .ok()
                .and_then(|field| structure.fields.get(field))
                .ok_or_else(|| malformed("a field set names no field"))?;
            let name = unpacker.token(cadmpeg_core::decode::u64_from_index(field.token))?;
            let datum = unpacker.unpack(field.rep)?;
            fields.push((name, datum));
        }
        let element = match &structure.paths.get(spec.path).and_then(Option::as_ref) {
            Some(PathNode {
                parent: Some(_),
                element,
                ..
            }) => structure.tokens.get(*element).cloned().unwrap_or_default(),
            _ => String::new(),
        };
        nodes.push(Some(Node {
            spec_type: spec.kind,
            element,
            fields,
            children: Vec::new(),
        }));
    }
    for (index, spec) in structure.specs.iter().enumerate() {
        let parent = structure
            .paths
            .get(spec.path)
            .and_then(Option::as_ref)
            .and_then(|node| node.parent)
            .and_then(|parent| by_path.get(parent).copied().flatten());
        if let Some(parent) = parent {
            if let Some(Some(node)) = nodes.get_mut(parent) {
                node.children.push(index);
            }
        }
    }
    let mut unread = std::mem::take(&mut unpacker.unread);
    let mut builder = Builder {
        ctx,
        nodes,
        unread: &mut unread,
    };
    let layer = match root {
        Some(root) => {
            let root = builder.take(root)?;
            builder.layer(root)?
        }
        None => Layer::default(),
    };
    Ok((layer, unread))
}

#[cfg(test)]
mod tests;
