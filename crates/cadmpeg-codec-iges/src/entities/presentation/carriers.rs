// SPDX-License-Identifier: Apache-2.0
//! The Directory display state of curves, surfaces, and points: the color and
//! blank status of a carrier no topology uses, the entity label of every
//! carrier, and the level of every projected record.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, AttributeId, IdentityKey, LayerId};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;

use super::super::geometry::SourceSequences;
use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;

/// A projected carrier and the Directory entry it was decoded from.
enum Carrier {
    Curve(cadmpeg_ir::ids::CurveId),
    Surface(cadmpeg_ir::ids::SurfaceId),
    Point(cadmpeg_ir::ids::PointId),
}

impl Carrier {
    /// The identity key of the records stated about this carrier.
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

/// The lookups [`state`] reads.
pub(super) struct Sources<'a> {
    pub(super) entries: &'a BTreeMap<u32, &'a DirectoryEntry>,
    pub(super) records: &'a BTreeMap<u32, &'a ParameterRecord>,
    /// The Definition Levels properties whose levels decoded.
    pub(super) decoded: &'a BTreeSet<u32>,
    pub(super) sequences: &'a SourceSequences,
}

/// States the Directory display state that a body or a face does not hold:
/// a carrier's entity label as its `name` attribute; the color and blank
/// status of a carrier that no topology uses and that is not physically
/// dependent, as an appearance binding; and each level as a presentation layer
/// that lists the bodies, faces, and carriers on it.
pub(super) fn state(
    ir: &mut CadIr,
    sources: &Sources<'_>,
    resolve: &dyn Fn(i64) -> Result<Option<(AppearanceId, Color)>, CodecError>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let used = UsedCarriers::of(ir);
    let mut carriers = Vec::new();
    for curve in &ir.geometry.curves {
        if let Some(sequence) = sources.sequences.curve(&curve.id) {
            let shown = !used.curves.contains(curve.id.as_str());
            carriers.push((Carrier::Curve(curve.id.clone()), sequence, shown));
        }
    }
    for surface in &ir.geometry.surfaces {
        if let Some(sequence) = sources.sequences.surface(&surface.id) {
            let shown = !used.surfaces.contains(surface.id.as_str());
            carriers.push((Carrier::Surface(surface.id.clone()), sequence, shown));
        }
    }
    for point in &ir.geometry.points {
        if let Some(sequence) = sources.sequences.point(&point.id) {
            let shown = !used.points.contains(point.id.as_str());
            carriers.push((Carrier::Point(point.id.clone()), sequence, shown));
        }
    }

    let mut levels: BTreeMap<i64, Vec<PresentationItem>> = BTreeMap::new();
    for body in &ir.geometry.bodies {
        if let Some(entry) = sources
            .sequences
            .body(&body.id)
            .and_then(|sequence| sources.entries.get(&sequence))
        {
            for level in entry_levels(entry, sources) {
                levels
                    .entry(level)
                    .or_default()
                    .push(PresentationItem::Body {
                        body: body.id.clone(),
                    });
            }
        }
    }
    for face in &ir.geometry.faces {
        if let Some(entry) = sources
            .sequences
            .face(&face.id)
            .and_then(|sequence| sources.entries.get(&sequence))
        {
            for level in entry_levels(entry, sources) {
                levels
                    .entry(level)
                    .or_default()
                    .push(PresentationItem::Face {
                        face: face.id.clone(),
                    });
            }
        }
    }

    let mut hidden_without_color = 0_usize;
    for (carrier, sequence, shown) in carriers {
        let Some(entry) = sources.entries.get(&sequence) else {
            continue;
        };
        for level in entry_levels(entry, sources) {
            levels
                .entry(level)
                .or_default()
                .push(carrier.presentation_item());
        }
        if let Some(label) = entity_label(entry) {
            ctx.charge_entities(1, "iges_geometry_presentation")?;
            ir.model.attributes.push(SourceAttribute {
                id: AttributeId::compose(
                    &cadmpeg_ir::identity_namespace!("iges", "model", "attribute"),
                    carrier.key(),
                ),
                target: carrier.attribute_target().into(),
                name: "name".to_owned(),
                values: vec![Some(AttributeValue::String(label.to_owned()))],
                native_ref: None,
            });
        }
        if !shown || entry.status.is_physically_dependent() {
            continue;
        }
        let visible = entry.status.is_visible();
        let Some((appearance, color)) = entry.color.map(resolve).transpose()?.flatten() else {
            if !visible {
                hidden_without_color += 1;
            }
            continue;
        };
        super::appearance(ir, appearance.clone(), None, color, ctx)?;
        ctx.charge_entities(1, "iges_geometry_presentation")?;
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: AppearanceBindingId::compose(
                &cadmpeg_ir::identity_namespace!("iges", "model", "appearance-binding"),
                carrier.key(),
            ),
            target: carrier.appearance_target().into(),
            appearance,
            precedence: None,
            source_entity_id: None,
            object_type: None,
            visible: (!visible).then_some(false),
            channels: BTreeMap::new(),
            native_ref: None,
        });
    }
    if hidden_without_color > 0 {
        losses.push(IgesLossCode::FreeGeometryBlankStatusNotStated.note(format!(
            "{hidden_without_color} blanked curve, surface, or point carrier(s) state no Directory color, so no appearance binding states their blank status"
        )));
    }

    for (level, items) in levels {
        ctx.charge_entities(1, "iges_geometry_presentation")?;
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId::from(cadmpeg_ir::ids::Identity::compose(
                &cadmpeg_ir::identity_namespace!("iges", "model", "layer"),
                IdentityKey::encode_key_text(&level.to_string()),
            )),
            name: level.to_string(),
            parent: None,
            description: None,
            visible: None,
            items: items.into_iter().map(Into::into).collect(),
            native_ref: None,
        });
    }
    Ok(())
}

/// The levels an entry is on: its Directory level, or the levels of the
/// decoded Definition Levels property its negative level points to.
fn entry_levels(entry: &DirectoryEntry, sources: &Sources<'_>) -> Vec<i64> {
    match entry.level {
        Some(level) if level >= 0 => vec![level],
        Some(level) => {
            let Some(property) = u32::try_from(level.unsigned_abs())
                .ok()
                .filter(|sequence| {
                    sources.decoded.contains(sequence)
                        && sources.entries.get(sequence).is_some_and(|property| {
                            property.entity_type == 406 && property.form == 1
                        })
                })
                .and_then(|sequence| sources.records.get(&sequence))
            else {
                return Vec::new();
            };
            let count = property.count(1).unwrap_or(0);
            (0..count)
                .filter_map(|index| property.integer(2 + index))
                .collect()
        }
        None => Vec::new(),
    }
}

/// The entry's label, trimmed, where it holds text.
fn entity_label(entry: &DirectoryEntry) -> Option<&str> {
    std::str::from_utf8(&entry.label)
        .ok()
        .map(str::trim)
        .filter(|label| !label.is_empty())
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
