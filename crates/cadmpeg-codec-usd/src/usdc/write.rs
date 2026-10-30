// SPDX-License-Identifier: Apache-2.0
//! Writes a parsed layer as a USDC 0.8.0 layer (spec §9).
//!
//! Each statement becomes the spec and fields that §9.8 maps to it. Value
//! bytes follow the bootstrap header in the order the specs need them; the
//! structural sections and the table of contents follow the values.
//! Structural sections are compressed buffers of literals; arrays are not
//! compressed.

mod encode;
mod values;

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;

use crate::text::grammar::DOCUMENTATION_KEY;
use crate::text::syntax::{
    AttributeField, AttributeSpec, AttributeValue, Dictionary, DictionaryValue, Layer, ListOp,
    MetaItem, MetaValue, PathList, PrimBody, PrimSpec, PropertySpec, RelationshipField,
    RelationshipSpec, Specifier, Value, Variability,
};
use values::{rep, ty, ListItem};

/// The crate version this writer writes.
pub(crate) const VERSION: [u8; 3] = [0, 8, 0];
/// The bootstrap header identifier.
const IDENTIFIER: &[u8; 8] = b"PXR-USDC";
/// The length of the bootstrap header.
const HEADER_LEN: usize = 88;
/// The offset of the table-of-contents offset in the header.
const TOC_OFFSET: usize = 16;
/// The path index that no path tree item names: the empty path.
const EMPTY_PATH: u32 = 1;
/// The end of a field-set run.
const RUN_END: u32 = u32::MAX;

/// Spec types (spec §9.5).
const ATTRIBUTE: u32 = 1;
const PRIM: u32 = 6;
const PSEUDO_ROOT: u32 = 7;
const RELATIONSHIP: u32 = 8;
const VARIANT: u32 = 10;
const VARIANT_SET: u32 = 11;

pub(super) fn unsupported(what: &str) -> CodecError {
    CodecError::NotImplemented(format!("{what} has no USDC 0.8.0 form in this writer"))
}

/// One node of the path tree.
struct PathNode {
    element: u32,
    property: bool,
    children: Vec<usize>,
}

/// The tables and value bytes of a layer being written.
pub(super) struct Crate {
    out: Vec<u8>,
    tokens: Vec<String>,
    token_index: BTreeMap<String, u32>,
    /// The token index of each string.
    strings: Vec<u32>,
    string_index: BTreeMap<String, u32>,
    paths: Vec<PathNode>,
    path_index: BTreeMap<String, u32>,
    fields: Vec<(u32, u64)>,
    field_index: BTreeMap<(u32, u64), u32>,
    field_sets: Vec<u32>,
    field_set_index: BTreeMap<Vec<u32>, u32>,
    specs: Vec<(u32, u32, u32)>,
    /// A token or string table outgrew 32-bit indices.
    overflowed: bool,
}

/// The path index, signed element token, and jump of each path item.
type PathItems = (Vec<u32>, Vec<i32>, Vec<i32>);

/// The fields of one spec, by name.
type Fields = Vec<(String, u64)>;

fn index(value: usize) -> Result<u32, CodecError> {
    u32::try_from(value).map_err(|_| unsupported("a table above 2^32 entries"))
}

/// The elements of an absolute path, each with whether it names a
/// property; a variant selection `{set=variant}` is its own element.
fn elements(path: &str) -> Result<Vec<(String, bool)>, CodecError> {
    let malformed = || CodecError::malformed(format_args!("the path <{path}> is not absolute"));
    let body = path.strip_prefix('/').ok_or_else(malformed)?;
    let (prims, property) = match body.find('.') {
        Some(at) => (&body[..at], Some(&body[at + 1..])),
        None => (body, None),
    };
    let mut out = Vec::new();
    for segment in prims.split('/').filter(|segment| !segment.is_empty()) {
        let mut rest = segment;
        while !rest.is_empty() {
            if rest.starts_with('{') {
                let end = rest.find('}').ok_or_else(malformed)? + 1;
                out.push((rest[..end].to_owned(), false));
                rest = &rest[end..];
            } else {
                let end = rest.find('{').unwrap_or(rest.len());
                out.push((rest[..end].to_owned(), false));
                rest = &rest[end..];
            }
        }
    }
    if let Some(property) = property {
        out.push((property.to_owned(), true));
    }
    Ok(out)
}

/// The text path of `element` under `parent`.
fn child_path(parent: &str, element: &str, property: bool) -> String {
    if property {
        format!("{parent}.{element}")
    } else if parent == "/" {
        format!("/{element}")
    } else if element.starts_with('{') || parent.ends_with('}') {
        format!("{parent}{element}")
    } else {
        format!("{parent}/{element}")
    }
}

/// `path` made absolute against the prim at `anchor`.
fn absolute(anchor: &str, path: &str) -> String {
    if path.starts_with('/') {
        return path.to_owned();
    }
    let mut parts: Vec<&str> = anchor.split('/').filter(|part| !part.is_empty()).collect();
    let mut rest = path;
    while let Some(next) = rest.strip_prefix("../") {
        parts.pop();
        rest = next;
    }
    if rest == ".." {
        parts.pop();
        rest = "";
    }
    let rest = rest.strip_prefix("./").unwrap_or(rest);
    let base = format!("/{}", parts.join("/"));
    if rest.is_empty() {
        base
    } else if rest.starts_with('.') {
        format!("{base}{rest}")
    } else if base == "/" {
        format!("/{rest}")
    } else {
        format!("{base}/{rest}")
    }
}

impl Crate {
    fn new() -> Self {
        let mut out = Self {
            out: [0_u8; HEADER_LEN].to_vec(),
            tokens: Vec::new(),
            token_index: BTreeMap::new(),
            strings: Vec::new(),
            string_index: BTreeMap::new(),
            paths: Vec::new(),
            path_index: BTreeMap::new(),
            fields: Vec::new(),
            field_index: BTreeMap::new(),
            field_sets: Vec::new(),
            field_set_index: BTreeMap::new(),
            specs: Vec::new(),
            overflowed: false,
        };
        // Token 0 is the empty element of the root path, so that no
        // property name has token index 0.
        out.token("");
        out.paths.push(PathNode {
            element: 0,
            property: false,
            children: Vec::new(),
        });
        out.path_index.insert("/".to_owned(), 0);
        // Path 1 is in no tree, so it reads as the empty path.
        out.paths.push(PathNode {
            element: 0,
            property: false,
            children: Vec::new(),
        });
        out
    }

    pub(super) fn token(&mut self, text: &str) -> u32 {
        if let Some(index) = self.token_index.get(text) {
            return *index;
        }
        let Ok(index) = u32::try_from(self.tokens.len()) else {
            self.overflowed = true;
            return 0;
        };
        self.tokens.push(text.to_owned());
        self.token_index.insert(text.to_owned(), index);
        index
    }

    pub(super) fn string(&mut self, text: &str) -> u32 {
        if let Some(index) = self.string_index.get(text) {
            return *index;
        }
        let token = self.token(text);
        let Ok(index) = u32::try_from(self.strings.len()) else {
            self.overflowed = true;
            return 0;
        };
        self.strings.push(token);
        self.string_index.insert(text.to_owned(), index);
        index
    }

    /// The index of the empty path, which names no prim.
    pub(super) const fn empty_path() -> u32 {
        EMPTY_PATH
    }

    /// The index of absolute path `path`, adding it and its ancestors.
    pub(super) fn path(&mut self, path: &str) -> Result<u32, CodecError> {
        if let Some(index) = self.path_index.get(path) {
            return Ok(*index);
        }
        let mut parent = 0_usize;
        let mut text = "/".to_owned();
        for (element, property) in elements(path)? {
            text = child_path(&text, &element, property);
            parent = match self.path_index.get(&text) {
                Some(index) => usize::try_from(*index)
                    .map_err(|_| unsupported("a path table above the address space"))?,
                None => {
                    let token = self.token(&element);
                    let index = self.paths.len();
                    self.paths.push(PathNode {
                        element: token,
                        property,
                        children: Vec::new(),
                    });
                    self.paths[parent].children.push(index);
                    self.path_index.insert(text.clone(), index_value(index)?);
                    index
                }
            };
        }
        index_value(parent)
    }

    fn field_set(&mut self, fields: Fields) -> Result<u32, CodecError> {
        let mut run = Vec::new();
        for (name, value) in fields {
            let name = self.token(&name);
            let field = match self.field_index.get(&(name, value)) {
                Some(field) => *field,
                None => {
                    let field = index(self.fields.len())?;
                    self.fields.push((name, value));
                    self.field_index.insert((name, value), field);
                    field
                }
            };
            run.push(field);
        }
        if let Some(set) = self.field_set_index.get(&run) {
            return Ok(*set);
        }
        let set = index(self.field_sets.len())?;
        self.field_sets.extend_from_slice(&run);
        self.field_sets.push(RUN_END);
        self.field_set_index.insert(run, set);
        Ok(set)
    }

    fn spec(&mut self, path: &str, fields: Fields, kind: u32) -> Result<(), CodecError> {
        let path = self.path(path)?;
        let set = self.field_set(fields)?;
        self.specs.push((path, set, kind));
        Ok(())
    }

    /// The fields of the metadata `items`, list operations of one key
    /// joined into one field.
    fn metadata(&mut self, items: &[MetaItem], anchor: &str) -> Result<Fields, CodecError> {
        let mut keys: Vec<&str> = Vec::new();
        for item in items {
            if !keys.contains(&item.key.as_str()) {
                keys.push(&item.key);
            }
        }
        let mut out = Vec::new();
        for key in keys {
            let group: Vec<&MetaItem> = items.iter().filter(|item| item.key == key).collect();
            out.extend(self.metadata_field(key, &group, anchor)?);
        }
        Ok(out)
    }

    fn metadata_field(
        &mut self,
        key: &str,
        group: &[&MetaItem],
        anchor: &str,
    ) -> Result<Fields, CodecError> {
        let last = &group[group.len() - 1].value;
        let field = |name: &str, value: u64| Ok(vec![(name.to_owned(), value)]);
        match (key, last) {
            ("doc", MetaValue::Value(Value::String(text))) => {
                let value = self.string(text);
                field(
                    "documentation",
                    rep(ty::STRING, u64::from(value), false, true),
                )
            }
            (DOCUMENTATION_KEY, MetaValue::Value(Value::String(text))) => {
                let value = self.string(text);
                field("comment", rep(ty::STRING, u64::from(value), false, true))
            }
            ("references" | "payload", _) => {
                let payload = key == "payload";
                let mut parts = Vec::new();
                for item in group {
                    let (MetaValue::References(list) | MetaValue::Payloads(list)) = &item.value
                    else {
                        continue;
                    };
                    let mut absolute_items = Vec::new();
                    for reference in list {
                        let mut reference = reference.clone();
                        if reference.asset.is_none() {
                            reference.prim_path =
                                reference.prim_path.map(|path| absolute(anchor, &path));
                        }
                        absolute_items.push(reference);
                    }
                    parts.push((item.list_op, absolute_items));
                }
                let borrowed: Vec<(Option<ListOp>, Vec<ListItem<'_>>)> = parts
                    .iter()
                    .map(|(op, list)| {
                        (
                            *op,
                            list.iter()
                                .map(|reference| ListItem::Reference(reference, payload))
                                .collect(),
                        )
                    })
                    .collect();
                let list_type = if payload {
                    ty::PAYLOAD_LIST_OP
                } else {
                    ty::REFERENCE_LIST_OP
                };
                field(key, self.list_op(list_type, &borrowed)?)
            }
            ("inherits" | "specializes", _) => {
                let parts: Vec<(Option<ListOp>, Vec<String>)> = group
                    .iter()
                    .map(|item| {
                        let paths = match &item.value {
                            MetaValue::Paths(paths) => {
                                paths.iter().map(|path| absolute(anchor, path)).collect()
                            }
                            _ => Vec::new(),
                        };
                        (item.list_op, paths)
                    })
                    .collect();
                let borrowed = borrow(&parts, ListItem::Path);
                field(key, self.list_op(ty::PATH_LIST_OP, &borrowed)?)
            }
            ("variantSets", _) => {
                let parts: Vec<(Option<ListOp>, Vec<String>)> = group
                    .iter()
                    .map(|item| (item.list_op, names(&item.value)))
                    .collect();
                let borrowed = borrow(&parts, ListItem::String);
                field(
                    "variantSetNames",
                    self.list_op(ty::STRING_LIST_OP, &borrowed)?,
                )
            }
            ("variants", MetaValue::Dictionary(dictionary)) => {
                let pairs: Vec<(String, String)> = dictionary
                    .entries
                    .iter()
                    .filter_map(|entry| match &entry.value {
                        DictionaryValue::Typed {
                            value: Value::String(variant),
                            ..
                        } => Some((entry.key.clone(), variant.clone())),
                        _ => None,
                    })
                    .collect();
                field("variantSelection", self.selections(&pairs))
            }
            ("subLayers", MetaValue::SubLayers(layers)) => {
                let assets: Vec<String> = layers.iter().map(|layer| layer.asset.clone()).collect();
                let offsets: Vec<(f64, f64)> = layers
                    .iter()
                    .map(|layer| {
                        (
                            layer.offset.map_or(0.0, |number| number.float),
                            layer.scale.map_or(1.0, |number| number.float),
                        )
                    })
                    .collect();
                let assets = self.string_vector(&assets);
                let offsets = self.layer_offsets(&offsets);
                Ok(vec![
                    ("subLayers".to_owned(), assets),
                    ("subLayerOffsets".to_owned(), offsets),
                ])
            }
            ("prefixSubstitutions" | "suffixSubstitutions", MetaValue::StringMap(pairs)) => {
                let dictionary = Dictionary {
                    entries: pairs
                        .iter()
                        .map(|(from, to)| crate::text::syntax::DictionaryEntry {
                            key: from.clone(),
                            value: DictionaryValue::Typed {
                                type_name: "string".to_owned(),
                                array: false,
                                value: Value::String(to.clone()),
                            },
                        })
                        .collect(),
                };
                field(key, self.dictionary(&dictionary)?)
            }
            ("permission", MetaValue::Identifier(Some(name))) => field(
                key,
                rep(ty::PERMISSION, u64::from(name == "private"), false, true),
            ),
            ("symmetryFunction" | "displayUnit", MetaValue::Identifier(name)) => {
                let value = self.token(name.as_deref().unwrap_or(""));
                field(key, rep(ty::TOKEN, u64::from(value), false, true))
            }
            (_, MetaValue::Dictionary(dictionary)) => field(key, self.dictionary(dictionary)?),
            (_, MetaValue::None) => field(key, rep(ty::VALUE_BLOCK, 0, false, false)),
            (_, MetaValue::Value(_)) if group.iter().any(|item| item.list_op.is_some()) => {
                let parts: Vec<(Option<ListOp>, Vec<String>)> = group
                    .iter()
                    .map(|item| (item.list_op, names(&item.value)))
                    .collect();
                let borrowed = borrow(&parts, ListItem::Token);
                field(key, self.list_op(ty::TOKEN_LIST_OP, &borrowed)?)
            }
            (_, MetaValue::Value(value)) => field(key, self.metadata_value(key, value)?),
            _ => Err(unsupported(&format!("the metadata `{key}`"))),
        }
    }

    /// A metadata value, typed as the field schema types `key`.
    fn metadata_value(&mut self, key: &str, value: &Value) -> Result<u64, CodecError> {
        let declared = match key {
            "kind" | "upAxis" | "defaultPrim" | "interpolation" | "colorSpace" | "renderType" => {
                "token"
            }
            "metersPerUnit" | "kilogramsPerUnit" | "startTimeCode" | "endTimeCode"
            | "timeCodesPerSecond" | "framesPerSecond" => "double",
            "instanceable" | "active" | "hidden" => "bool",
            "displayName" | "displayGroup" | "documentation" | "comment" => "string",
            "elementSize" => "int",
            "apiSchemas" | "allowedTokens" => {
                return Ok(self.token_vector(&names(&MetaValue::Value(value.clone()))));
            }
            _ => match value {
                Value::String(_) => "string",
                Value::Asset(_) => "asset",
                Value::Identifier(word) if word == "true" || word == "false" => "bool",
                Value::Number(number) if number.integer.is_some() => "int",
                Value::Number(_) => "double",
                Value::List(_) => {
                    return Ok(self.token_vector(&names(&MetaValue::Value(value.clone()))));
                }
                _ => return Err(unsupported(&format!("the metadata `{key}`"))),
            },
        };
        self.typed_value(value, declared, false)
    }

    fn layer(&mut self, layer: &Layer) -> Result<(), CodecError> {
        let mut fields = self.metadata(&layer.metadata, "/")?;
        let names: Vec<String> = layer.prims.iter().map(|prim| prim.name.clone()).collect();
        if !names.is_empty() {
            fields.push(("primChildren".to_owned(), self.token_vector(&names)));
        }
        if let Some(order) = &layer.root_order {
            fields.push(("primOrder".to_owned(), self.token_vector(order)));
        }
        for prim in &layer.prims {
            self.prim(prim, "/")?;
        }
        self.spec("/", fields, PSEUDO_ROOT)
    }

    fn prim(&mut self, prim: &PrimSpec, parent: &str) -> Result<(), CodecError> {
        let path = child_path(parent, &prim.name, false);
        let specifier = match prim.specifier {
            Specifier::Def => 0,
            Specifier::Over => 1,
            Specifier::Class => 2,
        };
        let mut fields = vec![(
            "specifier".to_owned(),
            rep(ty::SPECIFIER, specifier, false, true),
        )];
        if let Some(type_name) = &prim.type_name {
            let token = self.token(type_name);
            fields.push((
                "typeName".to_owned(),
                rep(ty::TOKEN, u64::from(token), false, true),
            ));
        }
        fields.extend(self.metadata(&prim.metadata, &path)?);
        fields.extend(self.body(&prim.body, &path)?);
        self.spec(&path, fields, PRIM)
    }

    /// Writes the children, properties, and variant sets of the prim or
    /// variant at `path`, and returns the fields that order them.
    fn body(&mut self, body: &PrimBody, path: &str) -> Result<Fields, CodecError> {
        let mut fields = Vec::new();
        let children: Vec<String> = body
            .children
            .iter()
            .map(|child| child.name.clone())
            .collect();
        let mut properties: Vec<String> = Vec::new();
        for property in &body.properties {
            if !properties.iter().any(|name| name == property.name()) {
                properties.push(property.name().to_owned());
            }
        }
        let sets: Vec<String> = body
            .variant_sets
            .iter()
            .map(|set| set.name.clone())
            .collect();
        if !children.is_empty() {
            fields.push(("primChildren".to_owned(), self.token_vector(&children)));
        }
        if !properties.is_empty() {
            fields.push(("properties".to_owned(), self.token_vector(&properties)));
        }
        if !sets.is_empty() {
            fields.push(("variantSetChildren".to_owned(), self.token_vector(&sets)));
        }
        if let Some(order) = body.child_orders.last() {
            fields.push(("primOrder".to_owned(), self.token_vector(order)));
        }
        if let Some(order) = body.property_orders.last() {
            fields.push(("propertyOrder".to_owned(), self.token_vector(order)));
        }
        for child in &body.children {
            self.prim(child, path)?;
        }
        for name in &properties {
            let statements: Vec<&PropertySpec> = body
                .properties
                .iter()
                .filter(|property| property.name() == name)
                .collect();
            self.property(&statements, path)?;
        }
        for set in &body.variant_sets {
            let set_path = format!("{path}{{{}=}}", set.name);
            let variants: Vec<String> = set
                .variants
                .iter()
                .map(|variant| variant.name.clone())
                .collect();
            let children = self.token_vector(&variants);
            self.spec(
                &set_path,
                vec![("variantChildren".to_owned(), children)],
                VARIANT_SET,
            )?;
            for variant in &set.variants {
                let variant_path = format!("{path}{{{}={}}}", set.name, variant.name);
                let mut variant_fields = self.metadata(&variant.metadata, &variant_path)?;
                variant_fields.extend(self.body(&variant.body, &variant_path)?);
                self.spec(&variant_path, variant_fields, VARIANT)?;
            }
        }
        Ok(fields)
    }

    fn property(&mut self, statements: &[&PropertySpec], prim: &str) -> Result<(), CodecError> {
        match statements.first() {
            Some(PropertySpec::Attribute(first)) => {
                let attributes: Vec<&AttributeSpec> = statements
                    .iter()
                    .filter_map(|statement| match statement {
                        PropertySpec::Attribute(attribute) => Some(attribute),
                        PropertySpec::Relationship(_) => None,
                    })
                    .collect();
                self.attribute(first, &attributes, prim)
            }
            Some(PropertySpec::Relationship(first)) => {
                let relationships: Vec<&RelationshipSpec> = statements
                    .iter()
                    .filter_map(|statement| match statement {
                        PropertySpec::Relationship(relationship) => Some(relationship),
                        PropertySpec::Attribute(_) => None,
                    })
                    .collect();
                self.relationship(first, &relationships, prim)
            }
            None => Ok(()),
        }
    }

    fn attribute_value(
        &mut self,
        value: &AttributeValue,
        first: &AttributeSpec,
    ) -> Result<u64, CodecError> {
        match value {
            AttributeValue::Value(value) => self.typed_value(value, &first.type_name, first.array),
            AttributeValue::None => Ok(rep(ty::VALUE_BLOCK, 0, false, false)),
            AttributeValue::AnimationBlock => Err(unsupported("an animation block")),
            AttributeValue::ArrayEdit(_) => Err(unsupported("an array edit")),
        }
    }

    fn attribute(
        &mut self,
        first: &AttributeSpec,
        statements: &[&AttributeSpec],
        prim: &str,
    ) -> Result<(), CodecError> {
        let path = format!("{prim}.{}", first.name);
        let declared = if first.array {
            format!("{}[]", first.type_name)
        } else {
            first.type_name.clone()
        };
        let token = self.token(&declared);
        let mut fields = vec![(
            "typeName".to_owned(),
            rep(ty::TOKEN, u64::from(token), false, true),
        )];
        if first.custom {
            fields.push(("custom".to_owned(), rep(ty::BOOL, 1, false, true)));
        }
        if first.variability != Variability::Varying {
            fields.push((
                "variability".to_owned(),
                rep(ty::VARIABILITY, 1, false, true),
            ));
        }
        let mut connections = Vec::new();
        let mut metadata = Vec::new();
        for statement in statements {
            metadata.extend(statement.metadata.iter().cloned());
            match &statement.field {
                AttributeField::Declared => {}
                AttributeField::Default(value) => {
                    let value = self.attribute_value(value, first)?;
                    fields.push(("default".to_owned(), value));
                }
                AttributeField::TimeSamples(samples) => {
                    let mut times = Vec::new();
                    let mut values = Vec::new();
                    for sample in samples {
                        times.push(sample.time.float);
                        values.push(self.attribute_value(&sample.value, first)?);
                    }
                    fields.push(("timeSamples".to_owned(), self.time_samples(&times, &values)));
                }
                AttributeField::Connect(list) => {
                    let paths = match list {
                        PathList::None => Vec::new(),
                        PathList::Paths(paths) => {
                            paths.iter().map(|target| absolute(prim, target)).collect()
                        }
                    };
                    connections.push((statement.list_op, paths));
                }
                AttributeField::Spline(_) => return Err(unsupported("a spline")),
            }
        }
        if !connections.is_empty() {
            let borrowed = borrow(&connections, ListItem::Path);
            fields.push((
                "connectionPaths".to_owned(),
                self.list_op(ty::PATH_LIST_OP, &borrowed)?,
            ));
        }
        fields.extend(self.metadata(&metadata, prim)?);
        self.spec(&path, fields, ATTRIBUTE)
    }

    fn relationship(
        &mut self,
        first: &RelationshipSpec,
        statements: &[&RelationshipSpec],
        prim: &str,
    ) -> Result<(), CodecError> {
        let path = format!("{prim}.{}", first.name);
        let mut fields = Vec::new();
        if first.custom {
            fields.push(("custom".to_owned(), rep(ty::BOOL, 1, false, true)));
        }
        // A relationship spec states its variability: a reader takes an
        // absent one as varying.
        fields.push((
            "variability".to_owned(),
            rep(ty::VARIABILITY, u64::from(!first.varying), false, true),
        ));
        let mut targets = Vec::new();
        let mut metadata = Vec::new();
        for statement in statements {
            metadata.extend(statement.metadata.iter().cloned());
            match &statement.field {
                RelationshipField::Declared => {}
                RelationshipField::Targets(list) => {
                    let paths = match list {
                        PathList::None => Vec::new(),
                        PathList::Paths(paths) => {
                            paths.iter().map(|target| absolute(prim, target)).collect()
                        }
                    };
                    targets.push((statement.list_op, paths));
                }
                RelationshipField::BracketTarget(target) => {
                    targets.push((statement.list_op, vec![absolute(prim, target)]));
                }
                RelationshipField::DefaultTarget(_) => {
                    return Err(unsupported("a relationship default target"));
                }
            }
        }
        if !targets.is_empty() {
            let borrowed = borrow(&targets, ListItem::Path);
            fields.push((
                "targetPaths".to_owned(),
                self.list_op(ty::PATH_LIST_OP, &borrowed)?,
            ));
        }
        fields.extend(self.metadata(&metadata, prim)?);
        self.spec(&path, fields, RELATIONSHIP)
    }

    /// The path table items in depth-first order, with their jumps (spec
    /// §9.5).
    fn path_items(&self) -> Result<PathItems, CodecError> {
        let mut order = Vec::new();
        let mut stack = vec![0_usize];
        while let Some(node) = stack.pop() {
            order.push(node);
            stack.extend(self.paths[node].children.iter().rev());
        }
        let mut position = vec![0_usize; self.paths.len()];
        for (at, node) in order.iter().enumerate() {
            position[*node] = at;
        }
        let mut next_sibling = vec![None; self.paths.len()];
        for node in &self.paths {
            for pair in node.children.windows(2) {
                next_sibling[pair[0]] = Some(pair[1]);
            }
        }
        let mut indices = Vec::new();
        let mut elements = Vec::new();
        let mut jumps = Vec::new();
        for (at, node) in order.iter().enumerate() {
            let entry = &self.paths[*node];
            indices.push(index_value(*node)?);
            let token = i32::try_from(entry.element)
                .map_err(|_| unsupported("a token table above 2^31 entries"))?;
            elements.push(if entry.property { -token } else { token });
            let child = !entry.children.is_empty();
            let jump = match (child, next_sibling[*node]) {
                (true, Some(sibling)) => i32::try_from(position[sibling] - at)
                    .map_err(|_| unsupported("a path table above 2^31 items"))?,
                (true, None) => -1,
                (false, Some(_)) => 0,
                (false, None) => -2,
            };
            jumps.push(jump);
        }
        Ok((indices, elements, jumps))
    }

    /// The structural sections and the table of contents, after the value
    /// bytes.
    fn finish(mut self) -> Result<Vec<u8>, CodecError> {
        if self.overflowed {
            return Err(unsupported("a token table above 2^32 entries"));
        }
        let mut sections: Vec<(&str, usize, usize)> = Vec::new();
        let size = |len: usize| cadmpeg_core::decode::u64_from_index(len).to_le_bytes();

        let start = self.out.len();
        let mut characters = Vec::new();
        for token in &self.tokens {
            characters.extend_from_slice(token.as_bytes());
            characters.push(0);
        }
        let compressed = encode::compress(&characters)?;
        self.out.extend_from_slice(&size(self.tokens.len()));
        self.out.extend_from_slice(&size(characters.len()));
        self.out.extend_from_slice(&size(compressed.len()));
        self.out.extend_from_slice(&compressed);
        sections.push(("TOKENS", start, self.out.len() - start));

        let start = self.out.len();
        self.out.extend_from_slice(&size(self.strings.len()));
        for token in &self.strings {
            self.out.extend_from_slice(&token.to_le_bytes());
        }
        sections.push(("STRINGS", start, self.out.len() - start));

        let start = self.out.len();
        self.out.extend_from_slice(&size(self.fields.len()));
        let names: Vec<u32> = self.fields.iter().map(|(name, _)| *name).collect();
        encode::indices(&names, &mut self.out)?;
        let reps: Vec<u8> = self
            .fields
            .iter()
            .flat_map(|(_, value)| value.to_le_bytes())
            .collect();
        let compressed = encode::compress(&reps)?;
        self.out.extend_from_slice(&size(compressed.len()));
        self.out.extend_from_slice(&compressed);
        sections.push(("FIELDS", start, self.out.len() - start));

        let start = self.out.len();
        self.out.extend_from_slice(&size(self.field_sets.len()));
        encode::indices(&self.field_sets, &mut self.out)?;
        sections.push(("FIELDSETS", start, self.out.len() - start));

        let start = self.out.len();
        let (indices, elements, jumps) = self.path_items()?;
        self.out.extend_from_slice(&size(self.paths.len()));
        self.out.extend_from_slice(&size(indices.len()));
        encode::indices(&indices, &mut self.out)?;
        encode::integers(&elements, &mut self.out)?;
        encode::integers(&jumps, &mut self.out)?;
        sections.push(("PATHS", start, self.out.len() - start));

        let start = self.out.len();
        self.out.extend_from_slice(&size(self.specs.len()));
        let paths: Vec<u32> = self.specs.iter().map(|spec| spec.0).collect();
        let sets: Vec<u32> = self.specs.iter().map(|spec| spec.1).collect();
        let kinds: Vec<u32> = self.specs.iter().map(|spec| spec.2).collect();
        encode::indices(&paths, &mut self.out)?;
        encode::indices(&sets, &mut self.out)?;
        encode::indices(&kinds, &mut self.out)?;
        sections.push(("SPECS", start, self.out.len() - start));

        let toc = self.out.len();
        self.out.extend_from_slice(&size(sections.len()));
        for (name, start, length) in sections {
            let mut record = [0_u8; 16];
            record[..name.len()].copy_from_slice(name.as_bytes());
            self.out.extend_from_slice(&record);
            self.out.extend_from_slice(&size(start));
            self.out.extend_from_slice(&size(length));
        }
        self.out[..IDENTIFIER.len()].copy_from_slice(IDENTIFIER);
        self.out[IDENTIFIER.len()..IDENTIFIER.len() + VERSION.len()].copy_from_slice(&VERSION);
        self.out[TOC_OFFSET..TOC_OFFSET + 8].copy_from_slice(&size(toc));
        Ok(self.out)
    }
}

fn index_value(value: usize) -> Result<u32, CodecError> {
    index(value)
}

/// The names of a token or string list value.
fn names(value: &MetaValue) -> Vec<String> {
    match value {
        MetaValue::Names(names) => names.clone(),
        MetaValue::Value(Value::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) | Value::Identifier(text) => Some(text.clone()),
                _ => None,
            })
            .collect(),
        MetaValue::Value(Value::String(text)) => vec![text.clone()],
        _ => Vec::new(),
    }
}

/// List-operation parts of owned items as borrowed list items.
fn borrow<'a>(
    parts: &'a [(Option<ListOp>, Vec<String>)],
    item: impl Fn(&'a str) -> ListItem<'a>,
) -> Vec<(Option<ListOp>, Vec<ListItem<'a>>)> {
    parts
        .iter()
        .map(|(op, items)| (*op, items.iter().map(|text| item(text)).collect()))
        .collect()
}

/// `layer` as a USDC 0.8.0 layer.
pub(crate) fn write(layer: &Layer) -> Result<Vec<u8>, CodecError> {
    let mut out = Crate::new();
    out.layer(layer)?;
    out.finish()
}

#[cfg(test)]
mod tests;
