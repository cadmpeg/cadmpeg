// SPDX-License-Identifier: Apache-2.0
//! Author a Canvas and its embedded image from the neutral model.

use cadmpeg_ir::assets::{AssetContent, AssetId};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Dimension, FeatureDefinition, FeatureId, FeatureOperation};

use super::presentation::{GeneratedDesignRegistry, GeneratedDesignType};
use super::records::{encode_design_metastream, native_lp_ascii, native_lp_utf16};
use crate::native::F3dNative;
use crate::records::canvas::DesignCanvasImage;
use crate::records::entity_header::SegmentType;
use crate::records::feature::scope::DesignParameterScope;

pub(in crate::writer) struct Canvas<'a> {
    pub(in crate::writer) feature: FeatureId,
    pub(in crate::writer) asset: AssetId,
    image: &'a DesignCanvasImage,
    scope: &'a DesignParameterScope,
    types: Vec<&'a SegmentType>,
    name: String,
    path: String,
    data: &'a [u8],
    frame: cadmpeg_ir::features::FeatureUnitPlaneFrame,
    width: f64,
    height: f64,
    visible: bool,
}

pub(in crate::writer) fn select<'a>(
    target: &'a CadIr,
    native: &'a F3dNative,
) -> Option<Canvas<'a>> {
    let mut features = target.model.features.iter().filter(|feature| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::ReferenceImage { .. })
        )
    });
    let feature = features.next()?;
    if features.next().is_some()
        || feature.ordinal != 0
        || feature.suppressed != Some(false)
        || feature.product_definition.is_some()
        || !feature.dependencies.is_empty()
        || feature.source_text.is_some()
        || !feature.source_content.is_empty()
        || !feature.evaluation.outputs().is_empty()
        || feature.name.as_deref() != Some("Canvas 1")
        || feature.source_tag.as_deref() != Some("Canvas")
        || feature.source_properties.len() != 1
    {
        return None;
    }
    let FeatureDefinition::Operation(FeatureOperation::ReferenceImage {
        asset,
        visible,
        mirror_u: false,
        mirror_v: false,
        frame,
        bounds,
        opacity: Some(Dimension::Literal(opacity)),
    }) = feature.evaluation.definition()
    else {
        return None;
    };
    if opacity.get() != 1.0 {
        return None;
    }
    let [first, second] = bounds.corners().map(cadmpeg_ir::units::FinitePoint2::get);
    if first.u != 0.0 || first.v != 0.0 || second.u <= 0.0 || second.v <= 0.0 {
        return None;
    }
    let origin = frame.origin().get();
    if [origin.x, origin.y, origin.z, second.u, second.v]
        .into_iter()
        .any(|value| value / 10.0 * 10.0 != value)
    {
        return None;
    }
    let u = *frame.u_axis().as_raw();
    let v = *frame.v_axis().as_raw();
    if u.norm() != 1.0 || v.norm() != 1.0 || u.dot(v) != 0.0 {
        return None;
    }
    let resource = target
        .model
        .assets
        .iter()
        .find(|resource| resource.id == *asset)?;
    let AssetContent::Embedded { data } = &resource.content else {
        return None;
    };
    let name = resource.name.as_ref()?.as_str();
    if name.contains(['/', '\\']) || name.encode_utf16().count() > 1024 {
        return None;
    }
    let extension = std::path::Path::new(name).extension()?.to_str()?;
    let media = if extension.eq_ignore_ascii_case("png") {
        "image/png"
    } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        "image/jpeg"
    } else {
        return None;
    };
    if resource.media_type.as_ref()?.as_str() != media {
        return None;
    }
    let path = format!(
        "{}/Design1/Images.BlobParts/{name}",
        crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
    );
    if resource.id != crate::ids::neutral_asset_id(&path) {
        return None;
    }
    let scope = native
        .design_parameter_scopes
        .iter()
        .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
    let image = native.design_canvas_images.iter().find(|image| {
        resource.native_ref.as_deref() == Some(image.id.as_str())
            && image.scope_record_index == scope.record_index
    })?;
    if image.asset_name() != name
        || image.geometry().label().encode_utf16().count() > 256
        || crate::ids::design_segment(&image.id) != crate::ids::design_segment(&scope.id)
        || scope.kind() != crate::records::feature::scope::DesignFeatureKind::Canvas
        || !scope
            .reference_members()
            .values()
            .copied()
            .eq([image.geometry().record_index()])
    {
        return None;
    }
    let (key, value) = feature.source_properties.iter().next()?;
    if key.as_str() != "reference:0" || value != &image.geometry().record_index().to_string() {
        return None;
    }
    let indexes = [
        image.geometry().record_index(),
        image.asset_record_index(),
        scope.record_index,
        image.plane_entity_suffix,
        image.component_entity_suffix,
    ];
    let unique: std::collections::BTreeSet<_> = indexes.into_iter().collect();
    if unique.len() != indexes.len()
        || unique.contains(&0)
        || scope.record_index.checked_add(6).is_none()
        || (4..=6).any(|offset| {
            scope
                .record_index
                .checked_add(offset)
                .is_some_and(|index| unique.contains(&index))
        })
    {
        return None;
    }
    let stream = crate::ids::design_segment(&scope.id)?;
    let find_tag = |tag: &str| {
        native.design_types.iter().find(|ty| {
            crate::ids::design_segment(ty.id()) == Some(stream) && ty.class_tag.as_str() == tag
        })
    };
    let find_entity = |index: u32| {
        native.design_types.iter().find(|ty| {
            crate::ids::design_segment(ty.id()) == Some(stream)
                && ty.entities.values().any(|id| *id == u64::from(index))
        })
    };
    let tags = image.geometry().class_tags();
    let types = vec![
        find_tag(&tags[0])?,
        find_tag(&tags[1])?,
        find_tag(image.asset_class_tag())?,
        find_tag(scope.class_tag.as_str())?,
        find_tag(scope.paired_class_tag.as_str())?,
        find_entity(image.plane_entity_suffix)?,
        find_entity(image.component_entity_suffix)?,
    ];
    let type_guids: std::collections::BTreeSet<_> =
        types.iter().map(|ty| ty.type_guid.as_str()).collect();
    if type_guids.len() != types.len() {
        return None;
    }
    Some(Canvas {
        feature: feature.id.clone(),
        asset: asset.clone(),
        image,
        scope,
        types,
        name: name.to_owned(),
        path,
        data: data.as_bytes(),
        frame: *frame,
        width: second.u,
        height: second.v,
        visible: *visible,
    })
}

pub(super) struct EncodedCanvas<'a> {
    pub(super) bulk: Vec<u8>,
    pub(super) meta: Vec<u8>,
    pub(super) path: String,
    pub(super) data: &'a [u8],
    pub(super) feature: FeatureId,
    pub(super) asset: AssetId,
}

fn header(out: &mut Vec<u8>, tag: u32, index: u32) -> Result<(), crate::writer::WriteError> {
    native_lp_ascii(out, &tag.to_string())?;
    out.extend_from_slice(&index.to_le_bytes());
    Ok(())
}
fn reference(out: &mut Vec<u8>, index: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(index).to_le_bytes());
    out.extend_from_slice(&[0; 2]);
}

pub(super) fn encode<'a>(
    canvas: Canvas<'a>,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<EncodedCanvas<'a>, crate::writer::WriteError> {
    use crate::layout::{
        canvas_geometry_prefix as geometry_layout, canvas_scope_tail_98 as scope_layout,
    };
    let geometry = canvas.image.geometry().record_index();
    let asset = canvas.image.asset_record_index();
    let scope = canvas.scope.record_index;
    let plane = canvas.image.plane_entity_suffix;
    let component = canvas.image.component_entity_suffix;
    let mut out = Vec::new();
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mark = |records: &mut Vec<crate::metastream::RecordIndexEntry>, index, offset| {
        records.push(crate::metastream::RecordIndexEntry {
            entity_id: u64::from(index),
            bulk_offset: cadmpeg_core::decode::u64_from_index(offset),
        });
    };
    ctx.charge_entities(3, "F3D Canvas records")?;
    mark(&mut primary, geometry, out.len());
    header(&mut out, 256, geometry)?;
    out.resize(geometry_layout::LABEL_CODE_UNIT_COUNT, 0);
    out[21] = u8::from(canvas.image.geometry().prologue.first_flag());
    out[25] = u8::from(canvas.visible);
    let put_f64 = |out: &mut [u8], at: usize, value: f64| {
        out[at..at + 8].copy_from_slice(&value.to_le_bytes());
    };
    for (at, value) in [
        (geometry_layout::FIRST_BOUNDARY, 0.0),
        (geometry_layout::FIRST_BOUNDARY + 8, 0.0),
        (geometry_layout::FIRST_BOUNDARY + 16, canvas.width / 10.0),
        (geometry_layout::FIRST_BOUNDARY + 24, 0.0),
        (geometry_layout::SECOND_BOUNDARY, 0.0),
        (geometry_layout::SECOND_BOUNDARY + 8, canvas.height / 10.0),
        (geometry_layout::SECOND_BOUNDARY + 16, canvas.width / 10.0),
        (geometry_layout::SECOND_BOUNDARY + 24, canvas.height / 10.0),
    ] {
        put_f64(&mut out, at, value);
    }
    for (at, value) in [
        (geometry_layout::PLANE_REFERENCE, plane),
        (geometry_layout::SCOPE_REFERENCE, scope),
        (geometry_layout::COMPONENT_REFERENCE, component),
        (geometry_layout::ASSET_REFERENCE, asset),
    ] {
        out[at] = 1;
        out[at + 1..at + 9].copy_from_slice(&u64::from(value).to_le_bytes());
    }
    out[geometry_layout::GEOMETRY_PAYLOAD..geometry_layout::GEOMETRY_PAYLOAD + 4]
        .copy_from_slice(&1_f32.to_le_bytes());
    let origin = canvas.frame.origin().get();
    let u = *canvas.frame.u_axis().as_raw();
    let v = *canvas.frame.v_axis().as_raw();
    for (at, value) in [
        (geometry_layout::GEOMETRY_PAYLOAD + 5, origin.x / 10.0),
        (geometry_layout::GEOMETRY_PAYLOAD + 13, origin.y / 10.0),
        (geometry_layout::GEOMETRY_PAYLOAD + 21, origin.z / 10.0),
        (geometry_layout::GEOMETRY_PAYLOAD + 29, u.x),
        (geometry_layout::GEOMETRY_PAYLOAD + 37, u.y),
        (geometry_layout::GEOMETRY_PAYLOAD + 45, u.z),
        (geometry_layout::GEOMETRY_PAYLOAD + 53, v.x),
        (geometry_layout::GEOMETRY_PAYLOAD + 61, v.y),
        (geometry_layout::GEOMETRY_PAYLOAD + 69, v.z),
    ] {
        put_f64(&mut out, at, value);
    }
    out[geometry_layout::SECOND_BOUNDARY_PRESENT] = 1;
    native_lp_utf16(&mut out, canvas.image.geometry().label())?;
    mark(&mut secondary, geometry, out.len());
    header(&mut out, 257, geometry)?;
    out.extend_from_slice(&[0; 8]);
    reference(&mut out, component);
    mark(&mut primary, asset, out.len());
    header(&mut out, 258, asset)?;
    out.extend_from_slice(&[0; 10]);
    native_lp_utf16(&mut out, &canvas.name)?;
    mark(&mut primary, scope, out.len());
    header(&mut out, 259, scope)?;
    out.extend_from_slice(&[0; 10]);
    out.push(1);
    out.extend_from_slice(&geometry.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&1_u32.to_le_bytes());
    reference(&mut out, geometry);
    out.extend_from_slice(&u32::MAX.to_le_bytes());
    native_lp_utf16(&mut out, "Canvas")?;
    let mut tail = [0_u8; scope_layout::LEN];
    tail[scope_layout::FEATURE_ORDINAL..scope_layout::FEATURE_ORDINAL + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    tail[scope_layout::MEMBER_COUNT..scope_layout::MEMBER_COUNT + 4]
        .copy_from_slice(&scope_layout::MEMBER_COUNT_VALUE.to_le_bytes());
    for (at, index) in [
        (scope_layout::SELF_REFERENCE, scope),
        (scope_layout::FIRST_COMPANION, scope + 4),
        (scope_layout::SECOND_COMPANION, scope + 5),
        (scope_layout::THIRD_COMPANION, scope + 6),
        (scope_layout::CLOSING_SELF_REFERENCE, scope),
    ] {
        tail[at] = 1;
        tail[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
    }
    tail[scope_layout::HISTORY_STATE..scope_layout::HISTORY_STATE + 4]
        .copy_from_slice(&scope_layout::HISTORY_STATE_VALUE.to_le_bytes());
    out.extend_from_slice(&tail);
    mark(&mut secondary, scope, out.len());
    header(&mut out, 260, scope)?;
    let entities = [
        vec![u64::from(geometry)],
        vec![],
        vec![u64::from(asset)],
        vec![u64::from(scope)],
        vec![],
        vec![u64::from(plane)],
        vec![u64::from(component)],
    ];
    let types = canvas
        .types
        .into_iter()
        .zip(entities)
        .map(|(source, entity_ids)| {
            let mut ty = GeneratedDesignType::from(source);
            ty.entity_ids = entity_ids;
            ty
        })
        .collect();
    let registry = GeneratedDesignRegistry {
        types,
        body_map: None,
        browser_nodes: None,
    };
    let meta = encode_design_metastream(&registry, &primary, &secondary)?
        .ok_or_else(|| crate::writer::WriteError::Unwritable("missing Canvas metadata".into()))?;
    Ok(EncodedCanvas {
        bulk: out,
        meta,
        path: canvas.path,
        data: canvas.data,
        feature: canvas.feature,
        asset: canvas.asset,
    })
}
