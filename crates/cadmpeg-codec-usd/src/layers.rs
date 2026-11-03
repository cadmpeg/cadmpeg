// SPDX-License-Identifier: Apache-2.0
//! The layers that a root layer names: its sublayers and the layers of its
//! references and payloads, opened through the decode's companion source
//! (spec §8.4).

use std::collections::BTreeMap;

use cadmpeg_core::decode::{CompanionLoad, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::text::syntax::{Layer, MetaItem, MetaValue, PrimBody, PrimSpec};
use crate::usdz::Package;

/// Where a resource that a layer names is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resource {
    /// A package member, with its identity and bytes.
    Embedded { id: String, bytes: Vec<u8> },
    /// A companion file, by identity.
    Located(String),
    /// Neither: the path is kept as written.
    Unlocated,
}

/// One layer of the decode.
#[derive(Debug)]
pub(crate) struct Loaded {
    /// The identity of the layer, or `None` for the root layer.
    pub(crate) id: Option<String>,
    pub(crate) layer: Layer,
    /// The layer index of each asset path that the layer names.
    pub(crate) assets: BTreeMap<String, usize>,
    /// The companion identity of the file that holds the layer, or `None`
    /// for the input.
    file: Option<String>,
    /// The package and member name of a layer inside a package.
    member: Option<(usize, String)>,
}

/// The layers of a decode; index 0 is the root layer.
#[derive(Debug)]
pub(crate) struct Layers<'a> {
    pub(crate) all: Vec<Loaded>,
    /// The packages that hold layers.
    packages: Vec<Package<'a>>,
    /// The asset paths that did not open, with the reason.
    pub(crate) unavailable: BTreeMap<String, String>,
    /// The losses of reading the layers after the root layer.
    pub(crate) losses: Vec<LossNote>,
}

impl Layers<'_> {
    /// The root layer alone.
    pub(crate) fn single(layer: Layer) -> Self {
        Self {
            all: vec![Loaded {
                id: None,
                layer,
                assets: BTreeMap::new(),
                file: None,
                member: None,
            }],
            packages: Vec::new(),
            unavailable: BTreeMap::new(),
            losses: Vec::new(),
        }
    }

    pub(crate) fn root(&self) -> &Layer {
        &self.all[0].layer
    }

    /// The content of the image that `path` names from layer `from`: the
    /// bytes of a member of the package that holds the layer, or the
    /// identity of a companion file. A path that neither names is kept as
    /// written.
    pub(crate) fn texture(
        &self,
        ctx: &DecodeContext<'_>,
        from: Option<usize>,
        path: &str,
    ) -> Result<Resource, CodecError> {
        let Some(loaded) = from.and_then(|from| self.all.get(from)) else {
            return Ok(Resource::Unlocated);
        };
        if let Some((index, member)) = &loaded.member {
            let name = crate::usdz::member_name(member, path);
            if let (Some(package), Some(name)) = (self.packages.get(*index), name) {
                if package
                    .snapshot
                    .entries()
                    .iter()
                    .any(|entry| entry.name == name)
                {
                    let view = package.snapshot.open(ctx, &name)?;
                    return Ok(Resource::Embedded {
                        id: format!("{}[{name}]", loaded.file.as_deref().unwrap_or("")),
                        bytes: view.window().to_vec(),
                    });
                }
            }
        }
        Ok(match ctx.locate_companion(loaded.file.as_deref(), path) {
            Ok(id) => Resource::Located(id),
            Err(_) => Resource::Unlocated,
        })
    }

    /// The layer that `asset` names from layer `from`.
    pub(crate) fn resolve(&self, from: usize, asset: &str) -> Option<usize> {
        self.all.get(from)?.assets.get(asset).copied()
    }

    /// The layer stack of layer `index`: the layer, then the stacks of its
    /// sublayers in order, each layer once.
    pub(crate) fn stack(&self, index: usize) -> Vec<usize> {
        let mut out = Vec::new();
        self.collect_stack(index, &mut out);
        out
    }

    fn collect_stack(&self, index: usize, out: &mut Vec<usize>) {
        if out.contains(&index) {
            return;
        }
        let Some(loaded) = self.all.get(index) else {
            return;
        };
        out.push(index);
        for asset in sublayer_assets(&loaded.layer) {
            if let Some(sublayer) = loaded.assets.get(asset) {
                self.collect_stack(*sublayer, out);
            }
        }
    }
}

fn sublayer_assets(layer: &Layer) -> impl Iterator<Item = &str> {
    layer.metadata.iter().flat_map(|item| match &item.value {
        MetaValue::SubLayers(sublayers) => sublayers
            .iter()
            .map(|sublayer| sublayer.asset.as_str())
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    })
}

/// The asset paths that a layer names: its sublayers, then the assets of
/// its references and payloads.
fn asset_paths(layer: &Layer) -> Vec<String> {
    let mut out: Vec<String> = sublayer_assets(layer).map(str::to_owned).collect();
    for prim in &layer.prims {
        prim_assets(prim, &mut out);
    }
    out
}

fn prim_assets(prim: &PrimSpec, out: &mut Vec<String>) {
    metadata_assets(&prim.metadata, out);
    body_assets(&prim.body, out);
}

fn body_assets(body: &PrimBody, out: &mut Vec<String>) {
    for child in &body.children {
        prim_assets(child, out);
    }
    for set in &body.variant_sets {
        for variant in &set.variants {
            metadata_assets(&variant.metadata, out);
            body_assets(&variant.body, out);
        }
    }
}

fn metadata_assets(metadata: &[MetaItem], out: &mut Vec<String>) {
    for item in metadata {
        if let MetaValue::References(items) | MetaValue::Payloads(items) = &item.value {
            out.extend(items.iter().filter_map(|item| item.asset.clone()));
        }
    }
}

/// A layer that an asset path names: its identity, the file and package
/// member that hold it, and its bytes.
struct Named<'a> {
    id: String,
    file: Option<String>,
    member: Option<(usize, String)>,
    view: View<'a>,
}

/// Opens the layer that `asset` names from `from`: a member of the package
/// that holds `from`, or a companion file.
fn open<'a>(
    ctx: &DecodeContext<'a>,
    packages: &[Package<'a>],
    from: &Loaded,
    asset: &str,
) -> Result<Result<Named<'a>, String>, CodecError> {
    if let Some((index, member)) = &from.member {
        let name = crate::usdz::member_name(member, asset);
        if let (Some(package), Some(name)) = (packages.get(*index), name) {
            if package
                .snapshot
                .entries()
                .iter()
                .any(|entry| entry.name == name)
            {
                let view = package.snapshot.open(ctx, &name)?;
                return Ok(Ok(Named {
                    id: format!("{}[{name}]", from.file.as_deref().unwrap_or("")),
                    file: from.file.clone(),
                    member: Some((*index, name)),
                    view,
                }));
            }
        }
    }
    Ok(match ctx.open_companion(from.file.as_deref(), asset)? {
        CompanionLoad::Loaded { id, view } => Ok(Named {
            file: Some(id.clone()),
            id,
            member: None,
            view,
        }),
        CompanionLoad::Unavailable(reason) => Err(reason),
    })
}

/// Opens every layer that `root` names, directly or through the layers it
/// opens. `package` is the package that holds `root`.
pub(crate) fn load<'a>(
    ctx: &DecodeContext<'a>,
    root: Layer,
    package: Option<Package<'a>>,
) -> Result<Layers<'a>, CodecError> {
    let mut layers = Layers::single(root);
    let mut packages: Vec<Package<'a>> = Vec::new();
    if let Some(package) = package {
        layers.all[0].member = Some((0, package.root_name.clone()));
        packages.push(package);
    }
    let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
    let mut next = 0;
    while let Some(loaded) = layers.all.get(next) {
        let assets = asset_paths(&loaded.layer);
        let mut resolved = BTreeMap::new();
        for asset in assets {
            if resolved.contains_key(&asset) || asset.contains('[') {
                continue;
            }
            let Some(loaded) = layers.all.get(next) else {
                break;
            };
            let named = match open(ctx, &packages, loaded, &asset)? {
                Ok(named) => named,
                Err(reason) => {
                    layers.unavailable.insert(asset, reason);
                    continue;
                }
            };
            if let Some(index) = by_id.get(&named.id) {
                resolved.insert(asset, *index);
                continue;
            }
            let mut source = match crate::usdz::source::source(ctx, named.view) {
                Ok(source) => source,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    layers
                        .unavailable
                        .insert(asset, format!("{}: {error}", named.id));
                    continue;
                }
            };
            let layer = match crate::usdz::source::parse(ctx, &mut source) {
                Ok(layer) => layer,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    layers
                        .unavailable
                        .insert(asset, format!("{}: {error}", named.id));
                    continue;
                }
            };
            ctx.charge_entities(1, "usd_companion_layer")?;
            let member = match source.package {
                Some(package) => {
                    let name = package.root_name.clone();
                    packages.push(package);
                    Some((packages.len() - 1, name))
                }
                None => named.member,
            };
            layers.losses.extend(source.losses);
            let index = layers.all.len();
            by_id.insert(named.id.clone(), index);
            resolved.insert(asset, index);
            layers.all.push(Loaded {
                id: Some(named.id),
                layer,
                assets: BTreeMap::new(),
                file: named.file,
                member,
            });
        }
        if let Some(loaded) = layers.all.get_mut(next) {
            loaded.assets = resolved;
        }
        next += 1;
    }
    layers.packages = packages;
    Ok(layers)
}

#[cfg(test)]
mod tests;
