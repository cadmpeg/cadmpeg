// SPDX-License-Identifier: Apache-2.0
//! The attributes each written object states: name, colour, visibility and
//! layer, and the losses of the attributes and appearances the archive does
//! not state.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::SourceObjectAssociation;

use super::layers::LayerTable;
use crate::loss::RhinoLossCode;

/// The attributes an object record states.
#[derive(Clone, Copy)]
pub(super) struct ObjectDisplay<'a> {
    pub(super) name: Option<&'a str>,
    pub(super) color: Option<Color>,
    pub(super) visible: Option<bool>,
    pub(super) layer: i32,
}

/// What admitting one object's attributes reduced or used, charged when the
/// object is written.
#[must_use]
pub(super) struct Admitted {
    /// Some stated attribute is not written: a name with a null character,
    /// which a Rhino string cannot state, a layer the table does not write, or
    /// an instance path.
    reduced: bool,
    /// The appearance binding whose base colour is the object's colour.
    binding: Option<String>,
}

/// The attributes of the objects of one document.
pub(super) struct Displays<'a> {
    ir: &'a CadIr,
    layers: LayerTable,
    /// The base colour of the binding that colours each object, by object id,
    /// with the binding id: the first binding to the object in no occurrence
    /// and no state (the precedence of `CadIr::effective_display_color`).
    bound: BTreeMap<&'a str, (Option<Color>, &'a str)>,
    reduced: Vec<String>,
    carried_bindings: BTreeSet<String>,
}

impl<'a> Displays<'a> {
    /// The attributes of the objects of `ir`.
    ///
    /// # Errors
    ///
    /// As [`LayerTable::new`].
    pub(super) fn new(ir: &'a CadIr) -> Result<Self, CodecError> {
        let mut bound = BTreeMap::new();
        for binding in &ir.model.appearance_bindings {
            let target = &binding.target;
            if target.path.is_some() || target.state.is_some() {
                continue;
            }
            let Some(object) = object_id(&target.entity) else {
                continue;
            };
            let color = ir
                .model
                .appearances
                .iter()
                .find(|appearance| appearance.id == binding.appearance)
                .and_then(|appearance| appearance.base_color);
            bound
                .entry(object)
                .or_insert((color, binding.id.as_str()));
        }
        Ok(Self {
            ir,
            layers: LayerTable::new(ir)?,
            bound,
            reduced: Vec::new(),
            carried_bindings: BTreeSet::new(),
        })
    }

    /// The layer table.
    pub(super) const fn layers(&self) -> &LayerTable {
        &self.layers
    }

    /// The attributes of object `object`: its own stated attributes, else its
    /// source object's, and the base colour of the appearance bound to it
    /// where neither states a colour.
    pub(super) fn admit<'b>(
        &self,
        object: &str,
        name: Option<&'b str>,
        color: Option<Color>,
        visible: Option<bool>,
        source: Option<&'b SourceObjectAssociation>,
    ) -> (ObjectDisplay<'b>, Admitted) {
        let name = name.or_else(|| source.and_then(|source| source.name.as_deref()));
        let writable_name = name.filter(|name| !name.contains('\0'));
        let stated = color.or_else(|| source.and_then(|source| source.color));
        let bound = self
            .bound
            .get(object)
            .filter(|_| stated.is_none())
            .and_then(|(color, binding)| color.map(|color| (color, *binding)));
        let (layer, off_layer) = self
            .layers
            .object_layer(object, source.and_then(|source| source.layer.as_deref()));
        (
            ObjectDisplay {
                name: writable_name,
                color: stated.or(bound.map(|(color, _)| color)),
                visible: visible.or_else(|| source.and_then(|source| source.visible)),
                layer,
            },
            Admitted {
                reduced: writable_name != name
                    || off_layer
                    || source.is_some_and(|source| !source.instance_path.is_empty()),
                binding: bound.map(|(_, binding)| binding.to_owned()),
            },
        )
    }

    /// Records the attributes of written object `object`.
    pub(super) fn written(&mut self, object: &str, admitted: Admitted) {
        if admitted.reduced {
            self.reduced.push(object.to_owned());
        }
        self.carried_bindings.extend(admitted.binding);
    }

    /// The losses of the layers, attributes and appearances the archive does
    /// not state.
    pub(super) fn into_losses(mut self) -> Vec<LossNote> {
        let model = &self.ir.model;
        let mut losses = Vec::new();
        if !self.reduced.is_empty() {
            self.reduced.sort_unstable();
            losses.push(RhinoLossCode::WriterObjectAttributesReduced.note(format!(
                "{} object(s) state a layer the archive does not write, an instance path, or a \
                 name with a null character, which the written object attributes omit: {}",
                self.reduced.len(),
                self.reduced.join(", ")
            )));
        }
        if !model.appearances.is_empty() {
            let mut ids = model
                .appearances
                .iter()
                .map(|appearance| appearance.id.as_str())
                .collect::<Vec<_>>();
            ids.sort_unstable();
            losses.push(RhinoLossCode::WriterAppearanceReduced.note(format!(
                "the archive states {} appearance(s) only as the colour of the written objects \
                 bound to them, and writes no material: {}",
                ids.len(),
                ids.join(", ")
            )));
        }
        let mut uncarried = model
            .appearance_bindings
            .iter()
            .map(|binding| binding.id.as_str())
            .filter(|binding| !self.carried_bindings.contains(*binding))
            .collect::<Vec<_>>();
        if !uncarried.is_empty() {
            uncarried.sort_unstable();
            losses.push(RhinoLossCode::WriterAppearanceReduced.note(format!(
                "{} appearance binding(s) are not a written object's colour: a binding to a \
                 face, edge or vertex, in an occurrence or a state, to an object the archive \
                 does not write, to an object that states its own colour, or to an appearance \
                 with no base colour: {}",
                uncarried.len(),
                uncarried.join(", ")
            )));
        }
        losses.extend(self.layers.into_losses());
        losses
    }
}

/// The id of the written object an appearance target names.
fn object_id(target: &AppearanceTarget) -> Option<&str> {
    match target {
        AppearanceTarget::Body(id) => Some(id.as_str()),
        AppearanceTarget::Curve(id) => Some(id.as_str()),
        AppearanceTarget::Point(id) => Some(id.as_str()),
        AppearanceTarget::Surface(id) => Some(id.as_str()),
        AppearanceTarget::Tessellation(id) => Some(id.as_str()),
        _ => None,
    }
}
