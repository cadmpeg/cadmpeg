// SPDX-License-Identifier: Apache-2.0
//! The display state an object's bodies and meshes do not hold: the colour,
//! visibility, and name of a curve, surface, or point that no topology uses,
//! and the layer of every object.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{
    AppearanceBindingId, AppearanceId, AttributeId, BodyId, IdentityKey, LayerId,
};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::Meshed;
use cadmpeg_ir::topology::{Color, ShellMember};
use cadmpeg_ir::{CadIr, SourceObjectAssociation};

use super::ObjectDisplay;
use crate::loss::RhinoLossCode;
use crate::settings::LayerRecord;

/// One decoded object, as its source associations name it: the object UUID
/// and the instance path it was decoded through.
pub(super) type ObjectKey = (String, Vec<String>);

/// The display state one object was decoded with.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ObjectPresentation {
    pub(super) display: ObjectDisplay,
    /// The archive index of the object's layer, where the layer table
    /// resolves it to one layer.
    pub(super) layer_index: Option<i32>,
}

/// The objects decoded, each with its display state; `None` where two
/// objects share a key and state different display.
pub(super) type ObjectPresentations = HashMap<ObjectKey, Option<ObjectPresentation>>;

/// Records the display state of one decoded object.
pub(super) fn record(
    objects: &mut ObjectPresentations,
    key: ObjectKey,
    presentation: ObjectPresentation,
) {
    objects
        .entry(key)
        .and_modify(|held| {
            if held.as_ref() != Some(&presentation) {
                *held = None;
            }
        })
        .or_insert(Some(presentation));
}

/// A carrier no topology uses.
enum Carrier {
    Curve(cadmpeg_ir::ids::CurveId),
    Surface(cadmpeg_ir::ids::SurfaceId),
    Point(cadmpeg_ir::ids::PointId),
}

impl Carrier {
    fn key(&self) -> IdentityKey {
        match self {
            Self::Curve(id) => cadmpeg_ir::identity_key!("curve.").then(id.key()),
            Self::Surface(id) => cadmpeg_ir::identity_key!("surface.").then(id.key()),
            Self::Point(id) => cadmpeg_ir::identity_key!("point.").then(id.key()),
        }
    }

    fn appearance_target(&self) -> AppearanceTarget {
        match self {
            Self::Curve(id) => AppearanceTarget::Curve(id.clone()),
            Self::Surface(id) => AppearanceTarget::Surface(id.clone()),
            Self::Point(id) => AppearanceTarget::Point(id.clone()),
        }
    }

    fn attribute_target(&self) -> AttributeTarget {
        match self {
            Self::Curve(id) => AttributeTarget::Curve(id.clone()),
            Self::Surface(id) => AttributeTarget::Surface(id.clone()),
            Self::Point(id) => AttributeTarget::Point(id.clone()),
        }
    }

    fn presentation_item(&self) -> PresentationItem {
        match self {
            Self::Curve(id) => PresentationItem::Curve { curve: id.clone() },
            Self::Surface(id) => PresentationItem::Surface {
                surface: id.clone(),
            },
            Self::Point(id) => PresentationItem::Point { point: id.clone() },
        }
    }
}

/// States what the bodies and meshes of the decoded objects do not: each
/// layer as a presentation layer listing the bodies, meshes, and free carriers
/// of its objects; and the colour and visibility of a carrier no topology uses
/// as an appearance binding, and its object's name as its `name` attribute.
pub(super) fn state(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    layers: &[LayerRecord],
    objects: &ObjectPresentations,
) -> Result<Vec<LossNote>, CodecError> {
    let lookup = |association: Option<&SourceObjectAssociation>| {
        association
            .filter(|association| association.format == cadmpeg_ir::codec_id!("rhino"))
            .and_then(|association| {
                objects
                    .get(&(
                        association.object_id.as_str().to_owned(),
                        association.instance_path.clone(),
                    ))
                    .and_then(Option::as_ref)
            })
    };

    let mut on_layer: BTreeMap<i32, Vec<PresentationItem>> = BTreeMap::new();
    let mut place = |presentation: &ObjectPresentation, item: PresentationItem| {
        if let Some(index) = presentation.layer_index {
            on_layer.entry(index).or_default().push(item);
        }
    };

    for (body, presentation) in body_objects(ir, &lookup) {
        place(presentation, PresentationItem::Body { body });
    }
    for tessellation in &ir.model.tessellations {
        if matches!(tessellation.of, Meshed::Definition { .. }) {
            if let Some(presentation) = lookup(tessellation.source_object.as_ref()) {
                place(
                    presentation,
                    PresentationItem::Tessellation {
                        tessellation: tessellation.id.clone(),
                    },
                );
            }
        }
    }

    let used = UsedCarriers::of(ir);
    let mut carriers = Vec::new();
    for curve in &ir.geometry.curves {
        if !used.curves.contains(curve.id.as_str()) {
            if let Some(presentation) = lookup(curve.source_object.as_ref()) {
                carriers.push((Carrier::Curve(curve.id.clone()), presentation));
            }
        }
    }
    for surface in &ir.geometry.surfaces {
        if !used.surfaces.contains(surface.id.as_str()) {
            if let Some(presentation) = lookup(surface.source_object.as_ref()) {
                carriers.push((Carrier::Surface(surface.id.clone()), presentation));
            }
        }
    }
    for point in &ir.geometry.points {
        if !used.points.contains(point.id.as_str()) {
            if let Some(presentation) = lookup(point.source_object.as_ref()) {
                carriers.push((Carrier::Point(point.id.clone()), presentation));
            }
        }
    }

    let mut appearances = BTreeMap::new();
    let mut bindings = Vec::new();
    let mut attributes = Vec::new();
    let mut hidden_without_color = 0_usize;
    for (carrier, presentation) in &carriers {
        place(presentation, carrier.presentation_item());
        let display = &presentation.display;
        if let Some(name) = &display.name {
            ctx.charge_entities(1, "rhino_object_presentation")?;
            attributes.push(SourceAttribute {
                id: AttributeId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "model", "attribute"),
                    carrier.key(),
                ),
                target: carrier.attribute_target().into(),
                name: "name".to_owned(),
                values: vec![Some(AttributeValue::String(name.clone()))],
                native_ref: None,
            });
        }
        let Some(color) = display.color else {
            if display.visible == Some(false) {
                hidden_without_color += 1;
            }
            continue;
        };
        let appearance = appearance_id(color);
        if !appearances.contains_key(&appearance) {
            ctx.charge_entities(1, "rhino_object_presentation")?;
            appearances.insert(appearance.clone(), color);
        }
        ctx.charge_entities(1, "rhino_object_presentation")?;
        bindings.push(AppearanceBinding {
            id: AppearanceBindingId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "model", "appearance-binding"),
                carrier.key(),
            ),
            target: carrier.appearance_target().into(),
            appearance,
            precedence: None,
            source_entity_id: None,
            object_type: None,
            visible: display.visible,
            channels: BTreeMap::new(),
            native_ref: None,
        });
    }

    let presentation_layers = presentation_layers(ctx, layers, on_layer)?;
    ir.model
        .appearances
        .extend(appearances.into_iter().map(|(id, color)| Appearance {
            id,
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("rhino_object_color".to_owned()),
            category: None,
            base_color: Some(color),
            properties: BTreeMap::new(),
            textures: Vec::new(),
            native_ref: None,
        }));
    ir.model.appearance_bindings.extend(bindings);
    ir.model.attributes.extend(attributes);
    ir.model.presentation_layers.extend(presentation_layers);

    let mut losses = Vec::new();
    if hidden_without_color > 0 {
        losses.push(RhinoLossCode::FreeGeometryVisibilityNotStated.note(format_args!(
            "{hidden_without_color} hidden curve, surface, or point carrier(s) have no display colour, so no appearance binding states their visibility"
        )));
    }
    Ok(losses)
}

/// One presentation layer per layer record, each listing the items placed on
/// it, under the layer its hierarchy names as parent.
fn presentation_layers(
    ctx: &DecodeContext<'_>,
    layers: &[LayerRecord],
    mut on_layer: BTreeMap<i32, Vec<PresentationItem>>,
) -> Result<Vec<PresentationLayer>, CodecError> {
    let mut index_counts = HashMap::<i32, usize>::new();
    let mut id_counts = HashMap::new();
    for layer in layers {
        *index_counts.entry(layer.index).or_default() += 1;
        if let Some(id) = layer.id {
            *id_counts.entry(id).or_insert(0_usize) += 1;
        }
    }
    let unique_id =
        |layer: &LayerRecord| layer.id.filter(|id| id_counts.get(id).copied() == Some(1));
    let layer_id = |layer: &LayerRecord| {
        LayerId::from(cadmpeg_ir::ids::Identity::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "model", "layer"),
            IdentityKey::encode_key_text(&crate::presentation::layer_record_key(
                layer,
                unique_id(layer).is_some(),
            )),
        ))
    };
    let by_uuid = layers
        .iter()
        .filter_map(|layer| unique_id(layer).map(|id| (id, layer_id(layer))))
        .collect::<HashMap<_, _>>();
    let mut stated = Vec::new();
    for layer in layers {
        ctx.charge_entities(1, "rhino_object_presentation")?;
        let items = if index_counts.get(&layer.index).copied() == Some(1) {
            on_layer.remove(&layer.index).unwrap_or_default()
        } else {
            Vec::new()
        };
        stated.push(PresentationLayer {
            id: layer_id(layer),
            name: layer.name.clone(),
            parent: layer
                .hierarchy
                .as_ref()
                .and_then(|hierarchy| by_uuid.get(&hierarchy.parent_id))
                .cloned(),
            description: layer.description.clone(),
            visible: Some(layer.visible),
            items: items.into_iter().map(Into::into).collect(),
            native_ref: Some(format!(
                "rhino:presentation:layer#{}",
                crate::presentation::layer_record_key(layer, unique_id(layer).is_some())
            )),
        });
    }
    Ok(stated)
}

/// The appearance of one display colour.
fn appearance_id(color: Color) -> AppearanceId {
    AppearanceId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "model", "appearance"),
        IdentityKey::encode_key_text(&format!(
            "color-{:x}-{:x}-{:x}-{:x}",
            color.r().to_bits(),
            color.g().to_bits(),
            color.b().to_bits(),
            color.a().to_bits()
        )),
    )
}

/// Each body with the object its faces, wire edges, free vertices, or cage
/// were decoded from.
fn body_objects<'p>(
    ir: &CadIr,
    lookup: &dyn Fn(Option<&SourceObjectAssociation>) -> Option<&'p ObjectPresentation>,
) -> Vec<(BodyId, &'p ObjectPresentation)> {
    let geometry = &ir.geometry;
    let regions = geometry
        .regions
        .iter()
        .map(|region| (region.id.as_str(), &region.body))
        .collect::<HashMap<_, _>>();
    let shells = geometry
        .shells
        .iter()
        .filter_map(|shell| Some((shell.id.as_str(), *regions.get(shell.region.as_str())?)))
        .collect::<HashMap<_, _>>();
    let surfaces = geometry
        .surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface.source_object.as_ref()))
        .collect::<HashMap<_, _>>();
    let curves = geometry
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve.source_object.as_ref()))
        .collect::<HashMap<_, _>>();
    let points = geometry
        .points
        .iter()
        .map(|point| (point.id.as_str(), point.source_object.as_ref()))
        .collect::<HashMap<_, _>>();
    let subds = geometry
        .subds
        .iter()
        .map(|subd| (subd.id.as_str(), subd.source_object.as_ref()))
        .collect::<HashMap<_, _>>();
    let edges = geometry
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let vertices = geometry
        .vertices
        .iter()
        .map(|vertex| (vertex.id.as_str(), vertex))
        .collect::<HashMap<_, _>>();

    let mut found: BTreeMap<&BodyId, &'p ObjectPresentation> = BTreeMap::new();
    for face in &geometry.faces {
        if let (Some(body), Some(presentation)) = (
            shells.get(face.shell.as_str()),
            lookup(surfaces.get(face.surface.as_str()).copied().flatten()),
        ) {
            found.entry(*body).or_insert(presentation);
        }
    }
    for shell in &geometry.shells {
        let Some(body) = shells.get(shell.id.as_str()) else {
            continue;
        };
        for member in &shell.members {
            let association = match member {
                ShellMember::WireEdge { id } => edges
                    .get(id.as_str())
                    .and_then(|edge| edge.curve())
                    .and_then(|curve| curves.get(curve.as_str()).copied().flatten()),
                ShellMember::FreeVertex { id } => vertices
                    .get(id.as_str())
                    .and_then(|vertex| points.get(vertex.point.as_str()).copied().flatten()),
                ShellMember::Subd { id } => subds.get(id.as_str()).copied().flatten(),
            };
            if let Some(presentation) = lookup(association) {
                found.entry(*body).or_insert(presentation);
            }
        }
    }
    found
        .into_iter()
        .map(|(body, presentation)| (body.clone(), presentation))
        .collect()
}

/// The carriers that edges, faces, coedges, and vertices name.
struct UsedCarriers<'a> {
    curves: BTreeSet<&'a str>,
    surfaces: BTreeSet<&'a str>,
    points: BTreeSet<&'a str>,
}

impl<'a> UsedCarriers<'a> {
    fn of(ir: &'a CadIr) -> Self {
        let geometry = &ir.geometry;
        let mut curves = BTreeSet::new();
        for edge in &geometry.edges {
            if let Some(curve) = edge.curve() {
                curves.insert(curve.as_str());
            }
        }
        for coedge in &geometry.coedges {
            if let Some(use_curve) = &coedge.use_curve {
                curves.insert(use_curve.curve.as_str());
            }
        }
        Self {
            curves,
            surfaces: geometry
                .faces
                .iter()
                .map(|face| face.surface.as_str())
                .collect(),
            points: geometry
                .vertices
                .iter()
                .map(|vertex| vertex.point.as_str())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
