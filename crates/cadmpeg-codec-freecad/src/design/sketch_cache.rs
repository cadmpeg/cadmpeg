// SPDX-License-Identifier: Apache-2.0
//! Atomic projection of optional cached external sketch geometry.

use super::{
    design_identity_text, direct_counted_records, external_geometry_metadata,
    external_link_indices, sketch_attributes, sketch_carrier, sketch_geometry, sketch_nurbs,
    validate_external_geo_prefix, validate_sketch_carrier, EXTERNAL_GEO_AXIS_COUNT,
};
use crate::native::{ObjectRecord, PropertyRecord};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    report::loss::LossNote,
    sketches::{SketchEntity, SketchEntityId, SketchId},
};
use std::collections::BTreeSet;

pub(super) struct CachedSketchGeometry {
    pub(super) entities: Vec<SketchEntity>,
    pub(super) matched_references: BTreeSet<usize>,
    pub(super) losses: Vec<LossNote>,
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    object: &ObjectRecord,
    id: &SketchId,
    external_geometry: &PropertyRecord,
    references: Option<&PropertyRecord>,
) -> Result<CachedSketchGeometry, CodecError> {
    let mut entities = Vec::new();
    let mut matched_references = BTreeSet::new();
    let mut losses = Vec::new();
    if external_geometry.type_name != "Part::PropertyGeometryList" {
        return Err(CodecError::malformed(format_args!(
            "{} has runtime type {}, expected Part::PropertyGeometryList",
            external_geometry.id, external_geometry.type_name
        )));
    }
    let xml = ctx
        .parse_xml(external_geometry.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "invalid external sketch geometry {}: {error}",
                external_geometry.id
            ))
        })?;
    let records = direct_counted_records(&xml, "GeometryList", "Geometry", &external_geometry.id)?;
    validate_external_geo_prefix(&records, &external_geometry.id)?;
    if let Some(references) = references {
        if references.type_name != "App::PropertyLinkSubList" {
            return Err(CodecError::malformed(format_args!(
                "{} has runtime type {}, expected App::PropertyLinkSubList",
                references.id, references.type_name
            )));
        }
    }
    let link_indices = external_link_indices(references)?;
    for (external_index, node) in records
        .into_iter()
        .skip(EXTERNAL_GEO_AXIS_COUNT)
        .enumerate()
    {
        let (cache_reference, missing) = external_geometry_metadata(node, external_index + 3)?;
        let reference_index = cache_reference
            .as_deref()
            .and_then(|cache_reference| link_indices.get(cache_reference).copied());
        if let (Some(cache_reference), None) = (cache_reference.as_deref(), reference_index) {
            if !missing {
                losses.push(crate::loss::FreecadLossCode::SketchExternalReferenceUnresolved.note(
                    format!("sketch {} cached external geometry {} reference {cache_reference} has no matching live link; cached geometry retained without a dependency",
                    object.name(), external_index + 3)));
            }
        }
        if let Some(reference_index) = reference_index {
            matched_references.insert(reference_index);
        }
        let carrier = sketch_carrier(node);
        if let (Some(kind), Some(carrier)) = (node.attribute("type"), carrier.as_ref()) {
            validate_sketch_carrier(kind, carrier, external_index + 3)?;
        }
        let native_kind = node
            .attribute("type")
            .or_else(|| carrier.map(|child| child.tag_name().name()))
            .unwrap_or("unknown");
        let native_kind = native_kind.to_owned();
        let attributes = sketch_attributes(carrier);
        let geometry = match carrier
            .map(|carrier| sketch_nurbs(&native_kind, carrier))
            .transpose()?
            .flatten()
        {
            Some(nurbs) => nurbs,
            None => sketch_geometry(&native_kind, &attributes)?,
        };
        entities.push(
            SketchEntity::new(
                SketchEntityId::mint(design_identity_text(
                    "sketch-entity",
                    object,
                    format_args!(":external:{external_index}"),
                ))
                .map_err(CodecError::malformed)?,
                id.clone(),
                geometry,
            )
            .with_construction(true)
            .with_native_ref(Some(external_geometry.id.clone()))
            .with_geometry_ref(
                references
                    .filter(|_| reference_index.is_some())
                    .map(|property| property.id.clone()),
            )
            .with_endpoint_refs(
                reference_index
                    .and_then(|index| references.and_then(|property| property.links().get(index)))
                    .and_then(Option::as_ref)
                    .map(|reference| reference.subelements().to_vec())
                    .unwrap_or_default(),
            ),
        );
    }
    Ok(CachedSketchGeometry {
        entities,
        matched_references,
        losses,
    })
}
