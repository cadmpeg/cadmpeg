// SPDX-License-Identifier: Apache-2.0
//! Composition of a root layer stack into a stage (spec §8).
//!
//! Each stage prim holds its sites: the prim and variant bodies that give it
//! opinions, from strong to weak. Arcs add sites from the layer stack of
//! their layer, or from the layer stack of the layer that their asset
//! names; arcs to layers that did not open stay as [`External`] arcs on the
//! prim. Attribute and metadata lookups resolve the strongest opinion over
//! the sites.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::layers::Layers;
use crate::text::syntax::{
    AttributeField, AttributeSpec, AttributeValue, DictionaryValue, Layer, ListOp, MetaItem,
    MetaValue, PathList, PrimBody, PrimSpec, PropertySpec, Reference, RelationshipField,
    RelationshipSpec, Specifier, Value,
};

/// Passes that recompute variant selections after a pass adds sites.
const SELECTION_PASSES: usize = 4;

/// How a site joins its prim (§8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The prim's own specification in its namespace, or a namespace
    /// child of one.
    Local,
    /// A site an arc adds.
    Arc,
}

/// One source of opinions for a prim.
#[derive(Debug, Clone)]
pub(crate) struct Site<'l> {
    pub(crate) metadata: &'l [MetaItem],
    pub(crate) body: &'l PrimBody,
    /// The specifier, or `None` for a variant body.
    pub(crate) specifier: Option<Specifier>,
    /// The type name a prim specification states.
    pub(crate) type_name: Option<&'l str>,
    /// The layer path of the specification.
    pub(crate) spec_path: String,
    /// The arc root that maps layer paths of this site to stage paths: a
    /// path under `from` maps to the same path under `to`.
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) origin: Origin,
    /// The layer of the specification.
    pub(crate) layer: usize,
    /// The root layer of the layer stack that the site's arcs resolve in.
    pub(crate) stack: usize,
    /// Whether a path outside `from` names the same stage path. A path
    /// outside the root of a reference or payload names nothing.
    pub(crate) keeps_outside: bool,
}

impl Site<'_> {
    /// The stage path of a path that a property of this site names.
    pub(crate) fn map_path(&self, path: &str) -> Option<String> {
        let absolute = resolve_relative(&self.spec_path, path);
        match strip_path_prefix(&absolute, &self.from) {
            Some(rest) => Some(format!("{}{rest}", self.to)),
            None => self.keeps_outside.then_some(absolute),
        }
    }
}

/// A reference or payload to another layer (§8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct External {
    pub(crate) asset: String,
    pub(crate) prim_path: Option<String>,
    pub(crate) payload: bool,
}

/// One composed prim.
#[derive(Debug, Clone)]
pub(crate) struct Prim<'l> {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) parent: Option<usize>,
    pub(crate) children: Vec<usize>,
    pub(crate) sites: Vec<Site<'l>>,
    pub(crate) external: Vec<External>,
    /// For an instance prim, the key its prototype shares with every other
    /// instance of the same arcs (§8.3).
    pub(crate) instance_key: Option<String>,
    /// For a descendant of an instance, the instance prim and the path
    /// relative to it.
    pub(crate) in_instance: Option<(usize, String)>,
}

/// Counts of composition features that the stage does not carry.
#[derive(Debug, Default, Clone)]
pub(crate) struct Tally {
    pub(crate) unselected_variants: usize,
    pub(crate) undefined_overs: BTreeSet<String>,
    pub(crate) missing_targets: BTreeSet<String>,
    pub(crate) cycles: BTreeSet<String>,
    pub(crate) instance_overrides: BTreeSet<String>,
    pub(crate) sublayers: Vec<String>,
}

/// A composed layer.
#[derive(Debug, Clone)]
pub(crate) struct Stage<'l> {
    pub(crate) prims: Vec<Prim<'l>>,
    pub(crate) roots: Vec<usize>,
    pub(crate) tally: Tally,
    by_path: BTreeMap<String, usize>,
}

impl<'l> Stage<'l> {
    /// The prim at stage path `path`.
    pub(crate) fn find(&self, path: &str) -> Option<&Prim<'l>> {
        self.by_path
            .get(path)
            .and_then(|index| self.prims.get(*index))
    }

    /// The prim that holds the property at stage path `path`, and the
    /// property name.
    pub(crate) fn find_property<'p>(&self, path: &'p str) -> Option<(&Prim<'l>, &'p str)> {
        let (prim, property) = path.rsplit_once('.')?;
        Some((self.find(prim)?, property))
    }
}

/// The value of an attribute at the default time, or why it has none.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Resolved<'l> {
    /// The default value.
    Default(&'l crate::text::syntax::Value, &'l AttributeSpec),
    /// No default value; the first time sample's value.
    FirstSample(&'l crate::text::syntax::Value, &'l AttributeSpec),
    /// The attribute is authored and has no value.
    Blocked,
}

impl<'l> Prim<'l> {
    /// The strongest metadata item named `key`.
    pub(crate) fn metadata(&self, key: &str) -> Option<&'l MetaItem> {
        self.sites.iter().find_map(|site| {
            site.metadata
                .iter()
                .rev()
                .find(|item| item.key == key && item.list_op.is_none())
        })
    }

    /// The strongest boolean metadata value named `key`.
    pub(crate) fn metadata_bool(&self, key: &str) -> Option<bool> {
        match &self.metadata(key)?.value {
            MetaValue::Value(Value::Identifier(word)) => match word.to_ascii_lowercase().as_str() {
                "true" | "yes" => Some(true),
                "false" | "no" => Some(false),
                _ => None,
            },
            MetaValue::Value(Value::Number(number)) => Some(number.float != 0.0),
            _ => None,
        }
    }

    /// The strongest string metadata value named `key`.
    pub(crate) fn metadata_text(&self, key: &str) -> Option<&'l str> {
        match &self.metadata(key)?.value {
            MetaValue::Value(Value::String(text)) => Some(text),
            _ => None,
        }
    }

    /// The strongest `customData` string entry named `key`.
    pub(crate) fn custom_data_text(&self, key: &str) -> Option<&'l str> {
        self.sites.iter().find_map(|site| {
            site.metadata
                .iter()
                .rev()
                .filter(|item| item.key == "customData")
                .find_map(|item| match &item.value {
                    MetaValue::Dictionary(dictionary) => {
                        dictionary
                            .entries
                            .iter()
                            .find_map(|entry| match &entry.value {
                                crate::text::syntax::DictionaryValue::Typed {
                                    value: Value::String(text),
                                    ..
                                } if entry.key == key => Some(text.as_str()),
                                _ => None,
                            })
                    }
                    _ => None,
                })
        })
    }

    /// The applied API schemas: the `apiSchemas` list operations of the
    /// sites, weakest first.
    pub(crate) fn api_schemas(&self) -> BTreeSet<&'l str> {
        let mut schemas = BTreeSet::new();
        for site in self.sites.iter().rev() {
            for item in site.metadata.iter().filter(|item| item.key == "apiSchemas") {
                let MetaValue::Value(Value::List(values)) = &item.value else {
                    continue;
                };
                let names = values.iter().filter_map(|value| match value {
                    Value::String(name) | Value::Identifier(name) => Some(name.as_str()),
                    _ => None,
                });
                match item.list_op {
                    None => {
                        schemas.clear();
                        schemas.extend(names);
                    }
                    Some(ListOp::Delete) => {
                        for name in names {
                            schemas.remove(name);
                        }
                    }
                    Some(ListOp::Reorder) => {}
                    Some(_) => schemas.extend(names),
                }
            }
        }
        schemas
    }

    /// The strongest authored type name.
    pub(crate) fn type_name(&self) -> Option<&'l str> {
        self.sites.iter().find_map(|site| site.type_name)
    }

    /// Every statement for attribute `name`, strong to weak, with its site.
    fn attribute_specs<'p>(
        &'p self,
        name: &'p str,
    ) -> impl Iterator<Item = (&'p Site<'l>, &'l AttributeSpec)> + 'p {
        self.sites.iter().flat_map(move |site| {
            site.body
                .properties
                .iter()
                .rev()
                .filter_map(move |property| match property {
                    PropertySpec::Attribute(attribute) if attribute.name == name => {
                        Some((site, attribute))
                    }
                    _ => None,
                })
        })
    }

    /// Whether any site states attribute `name`.
    pub(crate) fn has_attribute(&self, name: &str) -> bool {
        self.attribute_specs(name).next().is_some()
    }

    /// The value of attribute `name` at the default time (§6.2.1). A
    /// time-sampled attribute with no default value resolves to its first
    /// sample.
    pub(crate) fn resolve(&self, name: &str) -> Option<Resolved<'l>> {
        let mut first_sample = None;
        for (_, spec) in self.attribute_specs(name) {
            match &spec.field {
                AttributeField::Default(AttributeValue::Value(value)) => {
                    return Some(Resolved::Default(value, spec));
                }
                AttributeField::Default(AttributeValue::None | AttributeValue::AnimationBlock) => {
                    return first_sample.or(Some(Resolved::Blocked));
                }
                AttributeField::TimeSamples(samples) if first_sample.is_none() => {
                    first_sample = samples
                        .iter()
                        .filter(|sample| !sample.time.float.is_nan())
                        .min_by(|left, right| left.time.float.total_cmp(&right.time.float))
                        .and_then(|sample| match &sample.value {
                            AttributeValue::Value(value) => {
                                Some(Resolved::FirstSample(value, spec))
                            }
                            _ => None,
                        });
                }
                _ => {}
            }
        }
        first_sample
    }

    /// Whether attribute `name` has time samples or a spline.
    pub(crate) fn is_animated(&self, name: &str) -> bool {
        self.attribute_specs(name).any(|(_, spec)| {
            matches!(
                spec.field,
                AttributeField::TimeSamples(_) | AttributeField::Spline(_)
            )
        })
    }

    /// The strongest metadata item named `key` on attribute `name`.
    pub(crate) fn attribute_metadata(&self, name: &str, key: &str) -> Option<&'l MetaItem> {
        self.attribute_specs(name)
            .find_map(|(_, spec)| spec.metadata.iter().rev().find(|item| item.key == key))
    }

    /// The strongest string metadata value named `key` on attribute `name`.
    pub(crate) fn attribute_metadata_text(&self, name: &str, key: &str) -> Option<&'l str> {
        match &self.attribute_metadata(name, key)?.value {
            MetaValue::Value(Value::String(text)) => Some(text),
            _ => None,
        }
    }

    /// The connection targets of attribute `name`, as stage paths.
    pub(crate) fn connections(&self, name: &str) -> Vec<String> {
        for (site, spec) in self.attribute_specs(name) {
            if let AttributeField::Connect(list) = &spec.field {
                return match list {
                    PathList::None => Vec::new(),
                    PathList::Paths(paths) => paths
                        .iter()
                        .filter_map(|path| site.map_path(path))
                        .collect(),
                };
            }
        }
        Vec::new()
    }

    /// The targets of relationship `name`, as stage paths: the strongest
    /// explicit list, or the prepended and appended targets of the sites.
    pub(crate) fn relationship_targets(&self, name: &str) -> Vec<String> {
        let mut prepended = Vec::new();
        let mut appended = Vec::new();
        for site in &self.sites {
            for property in site.body.properties.iter().rev() {
                let PropertySpec::Relationship(relationship) = property else {
                    continue;
                };
                if relationship.name != name {
                    continue;
                }
                let paths = relationship_paths(relationship);
                match relationship.list_op {
                    None => {
                        if matches!(relationship.field, RelationshipField::Declared) {
                            continue;
                        }
                        let explicit: Vec<String> = paths
                            .iter()
                            .filter_map(|path| site.map_path(path))
                            .collect();
                        if prepended.is_empty() && appended.is_empty() {
                            return explicit;
                        }
                        prepended.extend(explicit);
                        return prepended.into_iter().chain(appended).collect();
                    }
                    Some(ListOp::Prepend) => {
                        prepended.extend(paths.iter().filter_map(|path| site.map_path(path)));
                    }
                    Some(ListOp::Append | ListOp::Add) => {
                        appended.extend(paths.iter().filter_map(|path| site.map_path(path)));
                    }
                    Some(ListOp::Delete | ListOp::Reorder) => {}
                }
            }
        }
        prepended.into_iter().chain(appended).collect()
    }

    /// Names of the properties of this prim, strong site first.
    /// The layer of the strongest spec of attribute `name`.
    pub(crate) fn attribute_layer(&self, name: &str) -> Option<usize> {
        self.attribute_specs(name)
            .next()
            .map(|(site, _)| site.layer)
    }

    /// The declared value type of attribute `name` in its strongest spec.
    pub(crate) fn attribute_type(&self, name: &str) -> Option<&'l str> {
        self.attribute_specs(name)
            .next()
            .map(|(_, spec)| spec.type_name.as_str())
    }

    pub(crate) fn property_names(&self) -> BTreeSet<&'l str> {
        self.sites
            .iter()
            .flat_map(|site| site.body.properties.iter().map(PropertySpec::name))
            .collect()
    }
}

fn relationship_paths(relationship: &RelationshipSpec) -> Vec<&str> {
    match &relationship.field {
        RelationshipField::Declared | RelationshipField::Targets(PathList::None) => Vec::new(),
        RelationshipField::Targets(PathList::Paths(paths)) => {
            paths.iter().map(String::as_str).collect()
        }
        RelationshipField::DefaultTarget(path) | RelationshipField::BracketTarget(path) => {
            vec![path.as_str()]
        }
    }
}

/// `path` resolved against the prim at `anchor` (§2.9).
fn resolve_relative(anchor: &str, path: &str) -> String {
    if path.starts_with('/') || path.is_empty() {
        return path.to_owned();
    }
    let mut parts: Vec<&str> = anchor.split('/').filter(|part| !part.is_empty()).collect();
    let mut rest = path;
    loop {
        if let Some(tail) = rest.strip_prefix("../") {
            parts.pop();
            rest = tail;
        } else if rest == ".." {
            parts.pop();
            rest = "";
        } else if let Some(tail) = rest.strip_prefix("./") {
            rest = tail;
        } else {
            break;
        }
    }
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

/// The part of `path` after the prim path `prefix`, when `path` names
/// `prefix` or a prim or property under it.
fn strip_path_prefix<'p>(path: &'p str, prefix: &str) -> Option<&'p str> {
    let rest = path.strip_prefix(prefix)?;
    (rest.is_empty() || rest.starts_with('/') || rest.starts_with('.')).then_some(rest)
}

/// The items a list-op field states, in effective order (§4.2).
fn list_items<'m, T>(
    metadata: &'m [MetaItem],
    key: &str,
    items: impl Fn(&'m MetaValue) -> Option<&'m [T]>,
) -> Vec<&'m T> {
    let mut explicit = None;
    let mut prepended = Vec::new();
    let mut appended = Vec::new();
    for item in metadata.iter().filter(|item| item.key == key) {
        let values: Vec<&T> = items(&item.value)
            .map(|slice| slice.iter().collect())
            .unwrap_or_default();
        match item.list_op {
            None => explicit = Some(values),
            Some(ListOp::Prepend) => prepended = values,
            Some(ListOp::Append | ListOp::Add) => appended.extend(values),
            Some(ListOp::Delete | ListOp::Reorder) => {}
        }
    }
    explicit.unwrap_or_else(|| prepended.into_iter().chain(appended).collect())
}

fn references(metadata: &[MetaItem], key: &str) -> Vec<Reference> {
    list_items(metadata, key, |value| match value {
        MetaValue::References(items) | MetaValue::Payloads(items) => Some(items.as_slice()),
        _ => None,
    })
    .into_iter()
    .cloned()
    .collect()
}

fn class_paths(metadata: &[MetaItem], key: &str) -> Vec<String> {
    list_items(metadata, key, |value| match value {
        MetaValue::Paths(paths) => Some(paths.as_slice()),
        _ => None,
    })
    .into_iter()
    .cloned()
    .collect()
}

/// The variant selections a site states.
fn selections(metadata: &[MetaItem]) -> Vec<(String, String)> {
    metadata
        .iter()
        .rev()
        .filter(|item| item.key == "variants")
        .flat_map(|item| match &item.value {
            MetaValue::Dictionary(dictionary) => dictionary
                .entries
                .iter()
                .filter_map(|entry| match &entry.value {
                    DictionaryValue::Typed {
                        value: Value::String(choice),
                        ..
                    } => Some((entry.key.clone(), choice.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

struct Composer<'c, 'a, 'l, 'p> {
    ctx: &'c DecodeContext<'a>,
    layers: &'l Layers<'p>,
    /// Every prim specification of each layer's namespace by path.
    specs: Vec<BTreeMap<String, &'l PrimSpec>>,
    /// The layer stack of each stack root.
    stacks: BTreeMap<usize, Vec<usize>>,
    prims: Vec<Prim<'l>>,
    tally: Tally,
}

/// A layer path in the namespace of a layer stack.
type StackPath = (usize, String);

/// The default prim that `layer` names.
fn default_prim(layer: &Layer) -> Option<String> {
    layer
        .metadata
        .iter()
        .rev()
        .find_map(|item| match &item.value {
            MetaValue::Value(Value::String(name)) if item.key == "defaultPrim" => {
                Some(if name.starts_with('/') {
                    name.clone()
                } else {
                    format!("/{name}")
                })
            }
            _ => None,
        })
}

/// Composes the root layer stack of `layers` into a stage.
pub(crate) fn compose<'l>(
    ctx: &DecodeContext<'_>,
    layers: &'l Layers<'_>,
) -> Result<Stage<'l>, CodecError> {
    let specs = layers
        .all
        .iter()
        .map(|loaded| {
            let mut specs = BTreeMap::new();
            for prim in &loaded.layer.prims {
                index_specs(prim, "", &mut specs);
            }
            specs
        })
        .collect();
    let mut composer = Composer {
        ctx,
        layers,
        specs,
        stacks: BTreeMap::new(),
        prims: Vec::new(),
        tally: Tally::default(),
    };
    let root_stack = composer.stack(0);
    for index in &root_stack {
        let Some(loaded) = layers.all.get(*index) else {
            continue;
        };
        for item in &loaded.layer.metadata {
            if let MetaValue::SubLayers(sublayers) = &item.value {
                composer.tally.sublayers.extend(
                    sublayers
                        .iter()
                        .filter(|sublayer| layers.resolve(*index, &sublayer.asset).is_none())
                        .map(|sublayer| sublayer.asset.clone()),
                );
            }
        }
    }
    let mut roots = Vec::new();
    let mut root_names: Vec<&str> = Vec::new();
    let mut root_sites: BTreeMap<&str, Vec<(usize, &'l PrimSpec)>> = BTreeMap::new();
    let mut root_order = None;
    for index in &root_stack {
        let Some(loaded) = layers.all.get(*index) else {
            continue;
        };
        for prim in &loaded.layer.prims {
            if !root_names.contains(&prim.name.as_str()) {
                root_names.push(&prim.name);
            }
            root_sites
                .entry(&prim.name)
                .or_default()
                .push((*index, prim));
        }
        if root_order.is_none() {
            root_order = loaded.layer.root_order.as_ref();
        }
    }
    if let Some(order) = root_order {
        root_names = reorder(root_names, order);
    }
    for name in root_names {
        let path = format!("/{name}");
        let seeds: Vec<Site<'l>> = root_sites
            .get(name)
            .into_iter()
            .flatten()
            .map(|(layer, spec)| local_site(spec, &path, *layer))
            .collect();
        if let Some(index) = composer.prim(&path, name, None, &seeds, None)? {
            roots.push(index);
        }
    }
    let tally = composer.tally;
    let by_path = composer
        .prims
        .iter()
        .enumerate()
        .map(|(index, prim)| (prim.path.clone(), index))
        .collect();
    Ok(Stage {
        prims: composer.prims,
        roots,
        tally,
        by_path,
    })
}

fn index_specs<'l>(spec: &'l PrimSpec, parent: &str, out: &mut BTreeMap<String, &'l PrimSpec>) {
    let path = format!("{parent}/{}", spec.name);
    for child in &spec.body.children {
        index_specs(child, &path, out);
    }
    out.entry(path).or_insert(spec);
}

fn local_site<'l>(spec: &'l PrimSpec, path: &str, layer: usize) -> Site<'l> {
    Site {
        metadata: &spec.metadata,
        body: &spec.body,
        specifier: Some(spec.specifier),
        type_name: spec.type_name.as_deref(),
        spec_path: path.to_owned(),
        from: path.to_owned(),
        to: path.to_owned(),
        origin: Origin::Local,
        layer,
        stack: 0,
        keeps_outside: true,
    }
}

/// `names` in the order a reorder list states (§5.2).
fn reorder<'n>(names: Vec<&'n str>, order: &[String]) -> Vec<&'n str> {
    let mut ordered: Vec<&'n str> = order
        .iter()
        .filter_map(|name| names.iter().copied().find(|candidate| candidate == name))
        .collect();
    ordered.extend(
        names
            .into_iter()
            .filter(|name| !order.iter().any(|kept| kept == name)),
    );
    ordered
}

impl<'l> Composer<'_, '_, 'l, '_> {
    /// The layer stack whose root is layer `root`.
    fn stack(&mut self, root: usize) -> Vec<usize> {
        self.stacks
            .entry(root)
            .or_insert_with(|| self.layers.stack(root))
            .clone()
    }

    /// The stack root and layer path of the prim that an arc to another
    /// layer targets, or `None` when the layer did not open.
    fn external_target(&self, layer: usize, reference: &Reference) -> Option<StackPath> {
        let target = self.layers.resolve(layer, reference.asset.as_deref()?)?;
        let path = match &reference.prim_path {
            Some(path) => path.clone(),
            None => self
                .layers
                .all
                .get(target)
                .and_then(|loaded| default_prim(&loaded.layer))
                .unwrap_or_default(),
        };
        Some((target, path))
    }

    /// Composes the prim at `path` from its seed sites and adds it, with its
    /// descendants, when it is in the traversal (§8.1).
    fn prim(
        &mut self,
        path: &str,
        name: &str,
        parent: Option<usize>,
        seeds: &[Site<'l>],
        in_instance: Option<(usize, String)>,
    ) -> Result<Option<usize>, CodecError> {
        let _depth = self.ctx.enter_nested("usd_compose_prim")?;
        let sites = self.expand_all(path, seeds)?;
        let specifier = sites
            .iter()
            .filter_map(|site| site.specifier)
            .find(|specifier| *specifier != Specifier::Over);
        match specifier {
            None => {
                self.tally.undefined_overs.insert(path.to_owned());
                return Ok(None);
            }
            Some(Specifier::Class) => return Ok(None),
            Some(_) => {}
        }
        let mut prim = Prim {
            path: path.to_owned(),
            name: name.to_owned(),
            parent,
            children: Vec::new(),
            sites,
            external: Vec::new(),
            instance_key: None,
            in_instance,
        };
        if prim.metadata_bool("active") == Some(false) {
            return Ok(None);
        }
        self.ctx.charge_entities(1, "usd_stage_prim")?;
        prim.external = self.external_arcs(&prim);
        let instance = prim.metadata_bool("instanceable") == Some(true)
            && prim.in_instance.is_none()
            && (prim.sites.iter().any(|site| site.origin == Origin::Arc)
                || !prim.external.is_empty());
        if instance {
            prim.instance_key = Some(instance_key(&prim, self.layers));
        }
        let index = self.prims.len();
        let child_scope = if instance {
            Some((index, String::new()))
        } else {
            prim.in_instance.clone()
        };
        let children = self.child_seeds(&prim, instance);
        self.prims.push(prim);
        let mut child_indices = Vec::new();
        for (child_name, seeds) in children {
            let child_path = if path == "/" {
                format!("/{child_name}")
            } else {
                format!("{path}/{child_name}")
            };
            let scope = child_scope.as_ref().map(|(root, relative)| {
                let relative = if relative.is_empty() {
                    child_name.clone()
                } else {
                    format!("{relative}/{child_name}")
                };
                (*root, relative)
            });
            if let Some(child) = self.prim(&child_path, &child_name, Some(index), &seeds, scope)? {
                child_indices.push(child);
            }
        }
        if let Some(prim) = self.prims.get_mut(index) {
            prim.children = child_indices;
        }
        Ok(Some(index))
    }

    /// The child names and seed sites of a composed prim, in name order.
    fn child_seeds(&mut self, prim: &Prim<'l>, instance: bool) -> Vec<(String, Vec<Site<'l>>)> {
        let mut names: Vec<String> = Vec::new();
        let mut seeds: BTreeMap<String, Vec<Site<'l>>> = BTreeMap::new();
        let mut order: Option<&'l [String]> = None;
        for site in &prim.sites {
            if instance && site.origin == Origin::Local && site.specifier.is_some() {
                if !site.body.children.is_empty() {
                    self.tally.instance_overrides.insert(prim.path.clone());
                }
                continue;
            }
            if order.is_none() {
                order = site.body.child_orders.last().map(Vec::as_slice);
            }
            for child in &site.body.children {
                if !names.contains(&child.name) {
                    names.push(child.name.clone());
                }
                let spec_path = format!("{}/{}", site.spec_path, child.name);
                seeds.entry(child.name.clone()).or_default().push(Site {
                    metadata: &child.metadata,
                    body: &child.body,
                    specifier: Some(child.specifier),
                    type_name: child.type_name.as_deref(),
                    spec_path,
                    from: site.from.clone(),
                    to: site.to.clone(),
                    origin: if site.origin == Origin::Local && site.from == site.to {
                        Origin::Local
                    } else {
                        Origin::Arc
                    },
                    layer: site.layer,
                    stack: site.stack,
                    keeps_outside: site.keeps_outside,
                });
            }
        }
        let names = match order {
            Some(order) => reorder(names.iter().map(String::as_str).collect(), order)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            None => names,
        };
        names
            .into_iter()
            .map(|name| {
                let sites = seeds.remove(&name).unwrap_or_default();
                (name, sites)
            })
            .collect()
    }

    /// Expands seed sites through their arcs, recomputing variant
    /// selections until they settle.
    fn expand_all(&mut self, path: &str, seeds: &[Site<'l>]) -> Result<Vec<Site<'l>>, CodecError> {
        let mut chosen: BTreeMap<String, String> = BTreeMap::new();
        let mut sites = Vec::new();
        for _ in 0..SELECTION_PASSES {
            sites.clear();
            let mut unselected = 0;
            for seed in seeds {
                let mut stack = vec![(0, path.to_owned())];
                self.expand(
                    seed.clone(),
                    path,
                    &chosen,
                    &mut stack,
                    &mut sites,
                    &mut unselected,
                )?;
            }
            let mut next = BTreeMap::new();
            for site in &sites {
                for (set, choice) in selections(site.metadata) {
                    next.entry(set).or_insert(choice);
                }
            }
            if next == chosen {
                self.tally.unselected_variants += unselected;
                return Ok(sites);
            }
            chosen = next;
        }
        Ok(sites)
    }

    /// Appends `site` and, in strength order, the sites its arcs add
    /// (§8.1): inherits, selected variants, references, payloads, and
    /// specializes.
    fn expand(
        &mut self,
        site: Site<'l>,
        stage_path: &str,
        chosen: &BTreeMap<String, String>,
        stack: &mut Vec<StackPath>,
        out: &mut Vec<Site<'l>>,
        unselected: &mut usize,
    ) -> Result<(), CodecError> {
        let _depth = self.ctx.enter_nested("usd_compose_arc")?;
        let metadata = site.metadata;
        let body = site.body;
        let anchor = site.spec_path.clone();
        let (from, to, origin) = (site.from.clone(), site.to.clone(), site.origin);
        let (layer, layer_stack, keeps_outside) = (site.layer, site.stack, site.keeps_outside);
        out.push(site);
        for target in class_paths(metadata, "inherits") {
            self.arc_site(
                (
                    layer_stack,
                    &resolve_relative(&anchor, &target),
                    keeps_outside,
                ),
                stage_path,
                chosen,
                stack,
                out,
                unselected,
            )?;
        }
        for set in &body.variant_sets {
            let choice = chosen.get(&set.name);
            for variant in &set.variants {
                if Some(&variant.name) != choice {
                    *unselected += 1;
                    continue;
                }
                let variant_site = Site {
                    metadata: &variant.metadata,
                    body: &variant.body,
                    specifier: None,
                    type_name: None,
                    spec_path: anchor.clone(),
                    from: from.clone(),
                    to: to.clone(),
                    origin,
                    layer,
                    stack: layer_stack,
                    keeps_outside,
                };
                self.expand(variant_site, stage_path, chosen, stack, out, unselected)?;
            }
        }
        for key in ["references", "payload"] {
            for reference in references(metadata, key) {
                if let Some(asset) = &reference.asset {
                    let Some((target_stack, target)) = self.external_target(layer, &reference)
                    else {
                        continue;
                    };
                    if target.is_empty() {
                        self.tally.missing_targets.insert(format!("@{asset}@"));
                        continue;
                    }
                    self.arc_site(
                        (target_stack, &target, false),
                        stage_path,
                        chosen,
                        stack,
                        out,
                        unselected,
                    )?;
                    continue;
                }
                let Some(target) = reference.prim_path.as_deref() else {
                    continue;
                };
                self.arc_site(
                    (layer_stack, &resolve_relative(&anchor, target), false),
                    stage_path,
                    chosen,
                    stack,
                    out,
                    unselected,
                )?;
            }
        }
        for target in class_paths(metadata, "specializes") {
            self.arc_site(
                (
                    layer_stack,
                    &resolve_relative(&anchor, &target),
                    keeps_outside,
                ),
                stage_path,
                chosen,
                stack,
                out,
                unselected,
            )?;
        }
        Ok(())
    }

    /// Expands the prim specifications at layer path `target` of a layer
    /// stack as arc sites of the stage prim at `stage_path`.
    fn arc_site(
        &mut self,
        (target_stack, target, keeps_outside): (usize, &str, bool),
        stage_path: &str,
        chosen: &BTreeMap<String, String>,
        stack: &mut Vec<StackPath>,
        out: &mut Vec<Site<'l>>,
        unselected: &mut usize,
    ) -> Result<(), CodecError> {
        let target = match target.split_once(['.', '{']) {
            Some((prim, _)) => prim,
            None => target,
        };
        if stack
            .iter()
            .any(|(visited_stack, visited)| *visited_stack == target_stack && visited == target)
        {
            self.tally.cycles.insert(stage_path.to_owned());
            return Ok(());
        }
        let specs: Vec<(usize, &'l PrimSpec)> = self
            .stack(target_stack)
            .into_iter()
            .filter_map(|layer| {
                let spec = self.specs.get(layer)?.get(target).copied()?;
                Some((layer, spec))
            })
            .collect();
        if specs.is_empty() {
            self.tally.missing_targets.insert(target.to_owned());
            return Ok(());
        }
        stack.push((target_stack, target.to_owned()));
        for (layer, spec) in specs {
            let site = Site {
                metadata: &spec.metadata,
                body: &spec.body,
                specifier: Some(spec.specifier),
                type_name: spec.type_name.as_deref(),
                spec_path: target.to_owned(),
                from: target.to_owned(),
                to: stage_path.to_owned(),
                origin: Origin::Arc,
                layer,
                stack: target_stack,
                keeps_outside,
            };
            if let Err(error) = self.expand(site, stage_path, chosen, stack, out, unselected) {
                stack.pop();
                return Err(error);
            }
        }
        stack.pop();
        Ok(())
    }

    /// The references and payloads of a composed prim to layers that did
    /// not open.
    fn external_arcs(&self, prim: &Prim<'l>) -> Vec<External> {
        let mut external = Vec::new();
        for site in &prim.sites {
            for (key, payload) in [("references", false), ("payload", true)] {
                for reference in references(site.metadata, key) {
                    let Some(asset) = reference.asset.clone() else {
                        continue;
                    };
                    if self.external_target(site.layer, &reference).is_some() {
                        continue;
                    }
                    let arc = External {
                        asset,
                        prim_path: reference.prim_path.clone(),
                        payload,
                    };
                    if !external.contains(&arc) {
                        external.push(arc);
                    }
                }
            }
        }
        external
    }
}

/// The prototype key of an instance prim: its arcs and variant selections
/// (§8.3).
fn instance_key(prim: &Prim<'_>, layers: &Layers<'_>) -> String {
    let mut parts = Vec::new();
    for site in prim.sites.iter().filter(|site| site.origin == Origin::Arc) {
        if site.specifier.is_some() {
            match layers
                .all
                .get(site.stack)
                .and_then(|loaded| loaded.id.as_deref())
            {
                Some(id) => parts.push(format!("site @{id}@{}", site.spec_path)),
                None => parts.push(format!("site {}", site.spec_path)),
            }
        }
    }
    for arc in &prim.external {
        parts.push(format!(
            "{} @{}@{}",
            if arc.payload { "payload" } else { "reference" },
            arc.asset,
            arc.prim_path.as_deref().unwrap_or("")
        ));
    }
    let mut chosen: Vec<(String, String)> = prim
        .sites
        .iter()
        .flat_map(|site| selections(site.metadata))
        .collect();
    chosen.sort();
    chosen.dedup_by(|left, right| left.0 == right.0);
    for (set, choice) in chosen {
        parts.push(format!("variant {set}={choice}"));
    }
    parts.join(";")
}

#[cfg(test)]
mod tests;
