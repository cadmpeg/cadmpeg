// SPDX-License-Identifier: Apache-2.0
//! External sketch dependencies and solved caches are typed document content.
use super::super::super::xml::{self, Object};
use super::{constraints::Indices, geometry, native};
use crate::{loss::FreecadLossCode, native::PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    document::CadIr,
    report::loss::LossNote,
    sketches::{Sketch, SketchEntity, SketchGeometryDefinition as Geometry},
};
use std::collections::HashMap;
pub(super) fn is_external(entity: &SketchEntity) -> bool {
    entity.id().as_str().contains(":external:")
        || entity.id().as_str().contains(":external-link:")
        || matches!(
            entity.geometry.definition(),
            Geometry::ExternalReference { .. }
        )
}
fn reference(
    ir: &CadIr,
    entity: &SketchEntity,
    names: &HashMap<String, String>,
) -> Option<(Option<String>, String, Vec<String>)> {
    if let Geometry::ExternalReference {
        document,
        object,
        subelements,
    } = entity.geometry.definition()
    {
        return Some((
            document.clone(),
            names
                .get(object.as_str())
                .cloned()
                .unwrap_or_else(|| object.as_str().to_owned()),
            subelements.clone(),
        ));
    }
    let property_id = entity.geometry_ref.as_deref()?;
    let properties = ir
        .native
        .namespace("fcstd")?
        .arena_as::<PropertyRecord>("properties")
        .ok()?;
    let property = properties
        .iter()
        .find(|property| property.id == property_id)?;
    // Cache order and live-link order are independent. A typed identity
    // disambiguates equal Edge1 selectors on different source objects.
    if entity.endpoint_refs.is_empty() {
        return None;
    }
    let key = native::cached_reference(ir, entity);
    let mut matches = property.links().iter().flatten().filter(|link| {
        if let Some(key) = key.as_deref() {
            link.object()
                .zip(link.subelements().first())
                .is_some_and(|(object, sub)| {
                    format!("{}.{sub}", crate::native::id_key(object)) == key
                })
        } else {
            link.subelements() == entity.endpoint_refs
        }
    });
    let link = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let object = link.object()?;
    Some((
        link.document_name().map(ToOwned::to_owned),
        names
            .get(object)
            .cloned()
            .unwrap_or_else(|| object.to_owned()),
        entity.endpoint_refs.clone(),
    ))
}
pub(super) fn write(
    ir: &CadIr,
    sketch: &Sketch,
    object: &mut Object,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<Indices, CodecError> {
    let mut indices = Indices::new();
    let mut entities = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && is_external(entity))
        .collect::<Vec<_>>();
    entities.sort_by_key(|entity| {
        let ordinal = entity
            .id()
            .as_str()
            .rsplit(':')
            .next()
            .and_then(|value| value.parse::<usize>().ok());
        (ordinal.is_none(), ordinal)
    });
    let mut links = Vec::new();
    let mut link_indices = HashMap::new();
    let mut rows = Vec::new();
    for entity in entities {
        let reference = reference(ir, entity, names).filter(|(document, _, subelements)| {
            // SketchObject::ExternalGeometry is a local LinkSubList: one
            // subelement per link. Do not silently erase a document token or
            // select the first of several neutral selectors.
            document.is_none() && subelements.len() == 1
        });
        if reference.is_none() && entity.geometry_ref.is_some() {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!("external sketch entity {} retains its solved carrier but its live dependency cannot be resolved and is omitted", entity.id())));
        }
        let reference_key = reference.as_ref().and_then(|(_, object, subelements)| {
            subelements
                .first()
                .map(|sub| format!("{}.{sub}", crate::native::id_key(object)))
        });
        let mut live_index = None;
        if let Some((document, target, subelements)) = reference {
            let key = (document.clone(), target.clone(), subelements.clone());
            live_index = Some(if let Some(index) = link_indices.get(&key) {
                *index
            } else {
                let index = links.len();
                let mut attrs = vec![("obj", target)];
                if let Some(document) = document {
                    attrs.push(("document", document));
                }
                if let Some(sub) = subelements.first() {
                    attrs.push(("sub", sub.clone()));
                }
                links.push(xml::element("Link", &attrs));
                link_indices.insert(key, index);
                index
            });
        }
        if let Some(mut row) = geometry::row(entity).or_else(|| native::geometry(ir, entity)) {
            // Ref binds the solved carrier to the live dependency without supplying geometry.
            if let Some(key) = reference_key {
                row = row.replacen(
                    "<Geometry ",
                    &format!("<Geometry ref=\"{}\" ", xml::escaped(&key)),
                    1,
                );
            }
            indices.insert(
                entity.id().as_str().to_owned(),
                (external_index(rows.len())?, 0),
            );
            rows.push(row);
        } else if let Some(live_index) = live_index.filter(|_| {
            matches!(
                entity.geometry.definition(),
                Geometry::ExternalReference { .. }
            )
        }) {
            indices.insert(
                entity.id().as_str().to_owned(),
                (external_index(live_index)?, 0),
            );
        } else {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!("external sketch entity {} has no typed carrier or resolvable dependency and is omitted",entity.id())));
        }
    }
    if !links.is_empty() {
        object.set(
            "ExternalGeometry",
            xml::property(
                "ExternalGeometry",
                "App::PropertyLinkSubList",
                &format!(
                    "<LinkSubList count=\"{}\">{}</LinkSubList>",
                    links.len(),
                    links.join("")
                ),
            ),
        );
    }
    if !rows.is_empty() {
        object.set("ExternalGeo",xml::property("ExternalGeo","Part::PropertyGeometryList",&format!("<GeometryList count=\"{}\"><Geometry id=\"-1\"/><Geometry id=\"-2\"/>{}</GeometryList>",rows.len()+2,rows.join(""))));
    }
    Ok(indices)
}

// (O-capacity) Reserved sketch indices occupy -1 and -2. External geometry
// starts at -3; conversion and subtraction must both fit the signed lane.
fn external_index(ordinal: usize) -> Result<i64, CodecError> {
    i64::try_from(ordinal)
        .ok()
        .and_then(|ordinal| (-3_i64).checked_sub(ordinal))
        .ok_or_else(|| CodecError::InvalidInput("FreeCAD external sketch index exceeds i64".into()))
}
