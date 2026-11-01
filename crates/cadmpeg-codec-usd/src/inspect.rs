// SPDX-License-Identifier: Apache-2.0
//! Layer inspection: the header, the layer metadata, the prim census, and
//! the layers and assets the layer names.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel, VerbatimSize};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_core::ContainerEntry;
use cadmpeg_ir::{ContainerKind, ContainerSummary};

use crate::detect::Header;
use crate::text::syntax::{Layer, MetaItem, MetaValue, PrimSpec, Value};

/// Name of the entry that describes the layer itself.
const LAYER_ENTRY: &str = "layer";
/// Entry attribute holding the header line.
const ATTRIBUTE_HEADER: &str = "header";
/// Entry attribute counting prim specifications.
const ATTRIBUTE_PRIMS: &str = "prim_specs";
/// Entry attribute counting property specifications.
const ATTRIBUTE_PROPERTIES: &str = "property_specs";
/// Entry attribute naming how the layer names an asset.
const ATTRIBUTE_ARC: &str = "arc";

pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerSummary, CodecError> {
    let mut source = crate::usdz::source::source(ctx, root)?;
    let layer = crate::usdz::source::parse(ctx, &mut source)?;
    let bytes = source.layer.window();
    let mut attributes = BTreeMap::new();
    let header = match &source.header {
        Header::Text { .. } => bytes
            .split(|byte| matches!(byte, b'\n' | b'\r'))
            .next()
            .map(|line| String::from_utf8_lossy(line).into_owned()),
        Header::Binary {
            version: [major, minor, patch],
        } => Some(format!("PXR-USDC {major}.{minor}.{patch}")),
    };
    if let Some(header) = header {
        attributes.insert(ATTRIBUTE_HEADER.to_owned(), header);
    }
    for item in &layer.metadata {
        if let Some(text) = scalar_text(item) {
            attributes.insert(item.key.clone(), text);
        }
    }
    let mut census = Census::default();
    for prim in &layer.prims {
        census.prim(prim);
    }
    attributes.insert(ATTRIBUTE_PRIMS.to_owned(), census.prims.to_string());
    attributes.insert(
        ATTRIBUTE_PROPERTIES.to_owned(),
        census.properties.to_string(),
    );
    let mut entries = vec![ContainerEntry {
        name: LAYER_ENTRY.to_owned(),
        role: ContainerRole::Document,
        storage: EntryStorage::verbatim(VerbatimLabel::Stored, u64_from_index(bytes.len())),
        attributes,
    }];
    let mut assets = BTreeMap::<String, BTreeSet<&'static str>>::new();
    collect_layer_assets(&layer, &mut assets);
    for prim in &layer.prims {
        collect_prim_assets(prim, &mut assets);
    }
    let kind = match &source.package {
        Some(package) => {
            let mut members = package.snapshot.container_entries(|name| {
                if name == package.root_name {
                    ContainerRole::Document
                } else if crate::usdz::is_layer_name(name) {
                    ContainerRole::ExternalReference
                } else {
                    ContainerRole::Auxiliary
                }
            });
            assets.retain(|asset, arcs| {
                let Some(member) = crate::usdz::member_name(&package.root_name, asset) else {
                    return true;
                };
                let Some(entry) = members.iter_mut().find(|entry| entry.name == member) else {
                    return true;
                };
                entry.attributes.insert(
                    ATTRIBUTE_ARC.to_owned(),
                    arcs.iter().copied().collect::<Vec<_>>().join(","),
                );
                false
            });
            entries.extend(members);
            ContainerKind::Zip
        }
        None => ContainerKind::Flat,
    };
    for (asset, arcs) in assets {
        entries.push(ContainerEntry {
            name: asset,
            role: ContainerRole::ExternalReference,
            storage: EntryStorage::Verbatim {
                label: VerbatimLabel::None,
                size: VerbatimSize::Unreported,
            },
            attributes: BTreeMap::from([(
                ATTRIBUTE_ARC.to_owned(),
                arcs.into_iter().collect::<Vec<_>>().join(","),
            )]),
        });
    }
    let types = census
        .types
        .iter()
        .map(|(name, count)| format!("{name} {count}"))
        .collect::<Vec<_>>()
        .join(", ");
    let notes = vec![format!("prim types: {types}")];
    Ok(ContainerSummary::classified(
        source.layers,
        kind,
        entries,
        source.losses,
        notes,
    ))
}

/// The text of a scalar layer metadata value.
fn scalar_text(item: &MetaItem) -> Option<String> {
    if item.list_op.is_some() {
        return None;
    }
    match &item.value {
        MetaValue::Value(Value::String(text) | Value::Asset(text) | Value::Identifier(text)) => {
            Some(text.clone())
        }
        MetaValue::Value(Value::Number(number)) => Some(match number.integer {
            Some(integer) => integer.to_string(),
            None => number.float.to_string(),
        }),
        _ => None,
    }
}

#[derive(Default)]
struct Census {
    prims: usize,
    properties: usize,
    types: BTreeMap<String, usize>,
}

impl Census {
    fn prim(&mut self, prim: &PrimSpec) {
        self.prims += 1;
        let name = prim
            .type_name
            .clone()
            .unwrap_or_else(|| "untyped".to_owned());
        *self.types.entry(name).or_default() += 1;
        self.body(&prim.body);
    }

    fn body(&mut self, body: &crate::text::syntax::PrimBody) {
        self.properties += body.properties.len();
        for child in &body.children {
            self.prim(child);
        }
        for set in &body.variant_sets {
            for variant in &set.variants {
                self.body(&variant.body);
            }
        }
    }
}

fn collect_layer_assets(layer: &Layer, assets: &mut BTreeMap<String, BTreeSet<&'static str>>) {
    for item in &layer.metadata {
        if let MetaValue::SubLayers(layers) = &item.value {
            for sublayer in layers {
                assets
                    .entry(sublayer.asset.clone())
                    .or_default()
                    .insert("subLayers");
            }
        }
    }
}

fn collect_prim_assets(prim: &PrimSpec, assets: &mut BTreeMap<String, BTreeSet<&'static str>>) {
    collect_meta_assets(&prim.metadata, assets);
    collect_body_assets(&prim.body, assets);
}

fn collect_body_assets(
    body: &crate::text::syntax::PrimBody,
    assets: &mut BTreeMap<String, BTreeSet<&'static str>>,
) {
    for child in &body.children {
        collect_prim_assets(child, assets);
    }
    for set in &body.variant_sets {
        for variant in &set.variants {
            collect_meta_assets(&variant.metadata, assets);
            collect_body_assets(&variant.body, assets);
        }
    }
}

fn collect_meta_assets(
    metadata: &[MetaItem],
    assets: &mut BTreeMap<String, BTreeSet<&'static str>>,
) {
    for item in metadata {
        let (arc, references) = match &item.value {
            MetaValue::References(references) => ("references", references),
            MetaValue::Payloads(payloads) => ("payload", payloads),
            _ => continue,
        };
        for reference in references {
            if let Some(asset) = &reference.asset {
                assets.entry(asset.clone()).or_default().insert(arc);
            }
        }
    }
}

#[cfg(test)]
mod tests;
