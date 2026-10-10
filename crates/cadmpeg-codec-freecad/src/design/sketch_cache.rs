// SPDX-License-Identifier: Apache-2.0
//! Atomic projection of optional cached external sketch geometry.

use super::{
    design_identity_text, direct_counted_records, external_geometry_metadata,
    external_link_indices, malformed_design, sketch_attributes, sketch_carrier, sketch_geometry,
    sketch_nurbs, validate_external_geo_prefix, validate_sketch_carrier, EXTERNAL_GEO_AXIS_COUNT,
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
        return Err(malformed_design(
            ctx,
            format_args!(
                "{} has runtime type {}, expected Part::PropertyGeometryList",
                external_geometry.id, external_geometry.type_name
            ),
        ));
    }
    let admitted_xml = ctx
        .parse_xml(external_geometry.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            malformed_design(
                ctx,
                format_args!(
                    "invalid external sketch geometry {}: {error}",
                    external_geometry.id
                ),
            )
        })?;
    let xml = admitted_xml.document();
    let records =
        direct_counted_records(ctx, xml, "GeometryList", "Geometry", &external_geometry.id)?;
    validate_external_geo_prefix(ctx, &records, &external_geometry.id)?;
    if let Some(references) = references {
        if references.type_name != "App::PropertyLinkSubList" {
            return Err(malformed_design(
                ctx,
                format_args!(
                    "{} has runtime type {}, expected App::PropertyLinkSubList",
                    references.id, references.type_name
                ),
            ));
        }
    }
    let link_indices = external_link_indices(ctx, references)?;
    for (external_index, node) in records
        .into_iter()
        .skip(EXTERNAL_GEO_AXIS_COUNT)
        .enumerate()
    {
        let (cache_reference, missing) = external_geometry_metadata(ctx, node, external_index + 3)?;
        let reference_index = cache_reference
            .as_deref()
            .and_then(|cache_reference| link_indices.get(cache_reference).copied());
        if let (Some(cache_reference), None) = (cache_reference.as_deref(), reference_index) {
            if !missing {
                ctx.reserve_vec(&mut losses, 1, "fcstd design losses")?;
                losses.push(crate::loss::FreecadLossCode::SketchExternalReferenceUnresolved.note(
                    ctx.format_retained(format_args!(
                        "sketch {} cached external geometry {} reference {cache_reference} has no matching live link; cached geometry retained without a dependency",
                        object.name(), external_index + 3), "fcstd design loss text")?));
            }
        }
        if let Some(reference_index) = reference_index {
            ctx.insert_btree_set(
                &mut matched_references,
                reference_index,
                "fcstd sketch matched references",
            )?;
        }
        let carrier = sketch_carrier(node);
        if let (Some(kind), Some(carrier)) = (node.attribute("type"), carrier.as_ref()) {
            validate_sketch_carrier(ctx, kind, carrier, external_index + 3)?;
        }
        let native_kind = node
            .attribute("type")
            .or_else(|| carrier.map(|child| child.tag_name().name()))
            .unwrap_or("unknown");
        let native_kind =
            ctx.copy_retained_text(native_kind, "fcstd external sketch geometry kind")?;
        let attributes = sketch_attributes(ctx, carrier)?;
        let geometry = match carrier
            .map(|carrier| sketch_nurbs(ctx, &native_kind, carrier))
            .transpose()?
            .flatten()
        {
            Some(nurbs) => nurbs,
            None => sketch_geometry(ctx, &native_kind, &attributes)?,
        };
        ctx.reserve_vec(&mut entities, 1, "fcstd sketch entities")?;
        entities.push(
            SketchEntity::new(
                SketchEntityId::mint(design_identity_text(
                    ctx,
                    "sketch-entity",
                    object,
                    format_args!(":external:{external_index}"),
                    "fcstd sketch external geometry identity",
                )?)
                .map_err(CodecError::malformed)?,
                id.try_clone_for_decode(ctx, "fcstd sketch entity parent")?,
                geometry,
            )
            .with_construction(true)
            .with_native_ref(Some(ctx.copy_retained_text(
                &external_geometry.id,
                "fcstd external geometry native reference",
            )?))
            .with_geometry_ref(
                references
                    .filter(|_| reference_index.is_some())
                    .map(|property| {
                        ctx.copy_retained_text(
                            &property.id,
                            "fcstd external geometry reference property",
                        )
                    })
                    .transpose()?,
            )
            .with_endpoint_refs(
                reference_index
                    .and_then(|index| references.and_then(|property| property.links().get(index)))
                    .and_then(Option::as_ref)
                    .map(|reference| {
                        ctx.copy_retained_strings(
                            reference.subelements(),
                            "fcstd sketch external endpoint refs",
                        )
                    })
                    .transpose()?
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
