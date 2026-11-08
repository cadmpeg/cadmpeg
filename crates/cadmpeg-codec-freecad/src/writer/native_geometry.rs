// SPDX-License-Identifier: Apache-2.0
//! Geometry held only in typed native `BRep` records still belongs to CADIR.
//!
//! This temporary projection is used when the neutral Model class is not
//! held, and to find the payloads whose typed tables still agree with a held
//! Model. It consumes typed records, never archive entries or retained bytes.

use crate::{brep, native};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Option<(CadIr, Vec<LossNote>)>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if ir.scope.model {
        return Ok(None);
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(None);
    };
    let payloads = namespace.arena_as::<brep::ShapePayloadRecord>("shape_payloads")?;
    if payloads.is_empty() {
        return Ok(None);
    }
    let properties = namespace.arena_as::<native::PropertyRecord>("properties")?;
    let property_reader = crate::property_reader::PropertyReader::typed(ctx);
    let mut typed_properties = Vec::new();
    for property in properties {
        if super::xml::native(&property, &std::collections::HashSet::new())?.is_some() {
            typed_properties.push(property);
        }
    }
    let mut projected = ir.clone();
    let mut scope = projected.scope;
    scope.model = true;
    // Typed records that do not admit a Model projection leave the document
    // as it holds itself.
    if projected.set_scope(scope).is_err() {
        return Ok(None);
    }
    let mut curves = brep::transfer_text_curves(ctx, &payloads, &typed_properties)?;
    let surfaces = brep::transfer_text_surfaces(ctx, &payloads, &typed_properties, &mut curves)?;
    projected.model.curves = curves.curves;
    if projected
        .model
        .add_procedural_curves(curves.procedural)?
        .is_err()
    {
        return Ok(None);
    }
    projected.model.surfaces = surfaces.surfaces;
    if projected
        .model
        .add_procedural_surfaces(surfaces.procedural)?
        .is_err()
    {
        return Ok(None);
    }
    let mut losses = Vec::new();
    let topology_occurrences = crate::topology_transfer::transfer_projection(
        ctx,
        &mut projected,
        &payloads,
        &typed_properties,
        &mut losses,
    )?;
    let objects = namespace.arena_as::<native::ObjectRecord>("objects")?;
    let products = namespace.arena_as::<native::ProductNodeRecord>("product_nodes")?;
    let joints = namespace.arena_as::<native::joint::JointRecord>("joints")?;
    let (definitions, occurrences) = crate::product::transfer_neutral(
        &property_reader,
        &products,
        &joints,
        &objects,
        &typed_properties,
        &payloads,
        &projected.model.bodies,
    )?;
    projected.model.product_definitions = definitions;
    projected.model.occurrences = occurrences;
    projected.model.assembly_joints =
        crate::joint::transfer_neutral(&joints, &projected.model.occurrences)?;
    let drawings = namespace.arena_as::<native::DrawingRecord>("drawings")?;
    let annotations = namespace.arena_as::<native::SemanticAnnotationRecord>("annotations")?;
    crate::drawing::transfer_neutral(
        &property_reader,
        &mut projected.model,
        &drawings,
        &typed_properties,
    )?;
    crate::annotation::transfer_neutral(
        &property_reader,
        &mut projected.model,
        &annotations,
        &typed_properties,
        &drawings,
    )?;
    if let Some(gui) = super::target::projection::gui_xml_lossy(ctx, namespace, &mut losses)? {
        let documents = namespace.arena_as::<native::DocumentFacts>("document")?;
        let version = documents
            .first()
            .and_then(|document| document.program_version.as_deref());
        let entries = std::collections::BTreeMap::new();
        let graph = crate::gui::transfer_rendered(
            ctx,
            &mut projected,
            gui.xml.as_bytes(),
            &crate::gui::GuiSources {
                entries: &entries,
                objects: &objects,
                properties: &typed_properties,
                payloads: &payloads,
                topology_occurrences: &topology_occurrences,
                requires_alpha_conversion: crate::gui::requires_alpha_conversion(version),
            },
            &gui.origins,
            None,
        )?;
        losses.extend(graph.losses);
    }
    crate::attributes::transfer(ctx, &mut projected, &objects, &typed_properties)?;
    Ok(Some((projected, losses)))
}

/// Model arenas a shape payload projects, keyed by the payload identity.
const PROJECTED_ARENAS: [&str; 15] = [
    "bodies",
    "regions",
    "shells",
    "faces",
    "loops",
    "coedges",
    "edges",
    "vertices",
    "points",
    "curves",
    "surfaces",
    "pcurves",
    "procedural_curves",
    "procedural_surfaces",
    "tessellations",
];

/// Body fields that later decode stages own: presentation state.
const BODY_STAGE_FIELDS: [&str; 3] = ["name", "color", "visible"];

/// Text payloads whose typed tables still project to exactly the held
/// Model's topology and carriers, so the writer can emit the tables
/// themselves (W1) instead of regenerating them.
pub(super) fn unchanged_payloads(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<std::collections::HashSet<String>, CodecError> {
    let mut unchanged = std::collections::HashSet::new();
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(unchanged);
    };
    let payloads = namespace.arena_as::<brep::ShapePayloadRecord>("shape_payloads")?;
    if !ir.scope.model || payloads.is_empty() {
        return Ok(unchanged);
    }
    let properties = namespace.arena_as::<native::PropertyRecord>("properties")?;
    let mut typed_properties = Vec::new();
    for property in properties {
        if super::xml::native(&property, &std::collections::HashSet::new())?.is_some() {
            typed_properties.push(property);
        }
    }
    let mut projected = ir.clone();
    let model = &mut projected.model;
    model.bodies.clear();
    model.regions.clear();
    model.shells.clear();
    model.faces.clear();
    model.loops.clear();
    model.coedges.clear();
    model.edges.clear();
    model.vertices.clear();
    model.points.clear();
    model.pcurves.clear();
    model.procedural_curves.clear();
    model.procedural_surfaces.clear();
    model.tessellations.clear();
    let projection = (|| -> Result<bool, CodecError> {
        let mut curves = brep::transfer_text_curves(ctx, &payloads, &typed_properties)?;
        let surfaces =
            brep::transfer_text_surfaces(ctx, &payloads, &typed_properties, &mut curves)?;
        projected.model.curves = curves.curves;
        if projected
            .model
            .add_procedural_curves(curves.procedural)?
            .is_err()
        {
            return Ok(false);
        }
        projected.model.surfaces = surfaces.surfaces;
        if projected
            .model
            .add_procedural_surfaces(surfaces.procedural)?
            .is_err()
        {
            return Ok(false);
        }
        let mut losses = Vec::new();
        crate::topology_transfer::transfer_projection(
            ctx,
            &mut projected,
            &payloads,
            &typed_properties,
            &mut losses,
        )?;
        Ok(true)
    })();
    match projection {
        Ok(true) => {}
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Ok(false) | Err(_) => return Ok(unchanged),
    }
    let (Some(held), Some(fresh)) = (
        projected_arenas(&ir.model),
        projected_arenas(&projected.model),
    ) else {
        debug_assert!(false, "Model arenas serialize");
        return Ok(unchanged);
    };
    for payload in &payloads {
        let Some(key) = payload
            .id
            .split_once('#')
            .map(|(_, key)| format!("#{key}:"))
        else {
            continue;
        };
        if PROJECTED_ARENAS
            .iter()
            .all(|arena| arena_agrees(&held, &fresh, arena, &key))
        {
            unchanged.insert(payload.id.clone());
        }
    }
    Ok(unchanged)
}

/// The arenas a payload projects, as one JSON object.
fn projected_arenas(model: &cadmpeg_ir::document::Model) -> Option<serde_json::Value> {
    Some(serde_json::Value::Object(
        [
            ("bodies", serde_json::to_value(&model.bodies).ok()?),
            ("regions", serde_json::to_value(&model.regions).ok()?),
            ("shells", serde_json::to_value(&model.shells).ok()?),
            ("faces", serde_json::to_value(&model.faces).ok()?),
            ("loops", serde_json::to_value(&model.loops).ok()?),
            ("coedges", serde_json::to_value(&model.coedges).ok()?),
            ("edges", serde_json::to_value(&model.edges).ok()?),
            ("vertices", serde_json::to_value(&model.vertices).ok()?),
            ("points", serde_json::to_value(&model.points).ok()?),
            ("curves", serde_json::to_value(&model.curves).ok()?),
            ("surfaces", serde_json::to_value(&model.surfaces).ok()?),
            ("pcurves", serde_json::to_value(&model.pcurves).ok()?),
            (
                "procedural_curves",
                serde_json::to_value(&model.procedural_curves).ok()?,
            ),
            (
                "procedural_surfaces",
                serde_json::to_value(&model.procedural_surfaces).ok()?,
            ),
            (
                "tessellations",
                serde_json::to_value(&model.tessellations).ok()?,
            ),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect(),
    ))
}

/// The held and projected records of one payload in one arena agree, by
/// identity and value. A held body's stage fields are not compared, nor its
/// absent root placement, which an occurrence carries instead.
fn arena_agrees(
    held: &serde_json::Value,
    fresh: &serde_json::Value,
    arena: &str,
    key: &str,
) -> bool {
    let rows = |model: &serde_json::Value| {
        model
            .get(arena)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|row| {
                let id = row.get("id")?.as_str()?;
                id.contains(key).then(|| (id.to_owned(), row.clone()))
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let mut held = rows(held);
    let mut fresh = rows(fresh);
    if held.len() != fresh.len() {
        return false;
    }
    // A later decode stage attributes carriers to the payload's owner object
    // instead of its property; the payload identity fixes both.
    for row in held.values_mut().chain(fresh.values_mut()) {
        if let Some(object) = row.as_object_mut() {
            object.remove("source_object");
        }
    }
    if arena == "bodies" {
        for (id, row) in &mut held {
            let Some(peer) = fresh.get_mut(id) else {
                return false;
            };
            let localized = row.get("transform").is_none_or(serde_json::Value::is_null);
            for object in [row.as_object_mut(), peer.as_object_mut()]
                .into_iter()
                .flatten()
            {
                for field in BODY_STAGE_FIELDS {
                    object.remove(field);
                }
                if localized {
                    object.remove("transform");
                }
            }
        }
    }
    held == fresh
}

#[cfg(test)]
mod tests;
