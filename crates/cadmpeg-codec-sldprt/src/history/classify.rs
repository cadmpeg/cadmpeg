// SPDX-License-Identifier: Apache-2.0
//! Feature-family, input-class, and history-record classifiers.

use crate::classification::{
    classify, classify_type_token, classify_xml_element, native_object_class,
    principal_plane_with_siblings, FeatureClass, NativeClassKind,
};
use crate::records::Feature;
use cadmpeg_ir::features::{BooleanOp, FeatureTreeNodeRole};
use std::collections::HashMap;

use crate::history::literals::parse_dimension_length_mm;
use crate::records::FeatureSource;

pub(super) fn is_custom_property(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("CustomProperty")
}

pub(super) fn is_semantic_note(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("Note")
        && feature.kind.eq_ignore_ascii_case("Note")
        && feature.text.as_ref().is_some_and(|text| !text.is_empty())
        && feature.parameters.is_empty()
        && feature.properties.is_empty()
}

fn is_attribute_definition(feature: &Feature) -> bool {
    feature.input_class.is_none()
        && feature.source_id == Some(FeatureSource::Reserved)
        && feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.kind.eq_ignore_ascii_case("Attribute-Definition")
        && !feature.name.is_empty()
}

pub(crate) fn is_history_metadata_record(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Feature, features: &[Feature]) -> Result<bool, cadmpeg_core::CodecError> {
    if is_custom_property(feature)
        || is_semantic_note(feature)
        || is_attribute_definition(feature)
        || matches!(
            feature.input_class.as_deref(),
            Some("moAlignGroup_c" | "moAttribute_c" | "moConfigCommentsFolder_c")
        )
    {
        return Ok(true);
    }
    Ok(feature.input_class.is_none()
        && feature.source_id == Some(FeatureSource::Reserved)
        && !feature.name.is_empty()
        && ctx.admit_iter(features, "scan SLDPRT metadata candidates")?.any(|candidate| {
            candidate.input_class.as_deref() == Some("moAttribute_c")
                && candidate.name.starts_with(&feature.name)
        }))
}

pub(super) fn feature_tree_node_role(ctx: &cadmpeg_core::decode::DecodeContext<'_>, 
    feature: &Feature,
    history_features: &[Feature],
) -> Result<Option<FeatureTreeNodeRole>, cadmpeg_core::CodecError> {
    Ok(reserved_feature_tree_node_role(ctx, feature, history_features)?
        .or_else(|| native_object_class(feature.input_class.as_deref()?).tree_node())
        .or_else(|| equation_container_role(feature)))
}

/// Keywords operation-family token of the equations container.
pub(super) const EQUATION_DRIVEN_TOKEN: &str = "EquationDriven";

/// The equations container identified by its Keywords operation-family token.
///
/// The token is a role code: it identifies the container without a native class
/// or a reserved source identifier.
fn equation_container_role(feature: &Feature) -> Option<FeatureTreeNodeRole> {
    (feature.input_class.is_none()
        && feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.kind.eq_ignore_ascii_case(EQUATION_DRIVEN_TOKEN))
    .then_some(FeatureTreeNodeRole::Equations)
}

fn reserved_feature_tree_node_role(ctx: &cadmpeg_core::decode::DecodeContext<'_>, 
    feature: &Feature,
    history_features: &[Feature],
) -> Result<Option<FeatureTreeNodeRole>, cadmpeg_core::CodecError> {
    let layout = match feature_manager_layout(ctx, history_features)? { Some(value) => value, None => return Ok(None) };
    if !classless_builtin_node(feature) {
        return Ok(None);
    }
    let source = match feature.source_id { Some(value) => value, None => return Ok(None) }.value();
    Ok(match (layout, feature.xml_tag.as_str(), source) {
        (FeatureManagerLayout::Current, tag, Some(1)) if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::Annotations)
        }
        (FeatureManagerLayout::Current, tag, Some(5)) if tag.eq_ignore_ascii_case("Sketch") => {
            Some(FeatureTreeNodeRole::ModelOrigin)
        }
        (FeatureManagerLayout::Current, tag, Some(6)) if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::Current, tag, Some(12)) if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::Current, tag, Some(13..=15))
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::Legacy, tag, Some(2)) if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::Legacy, tag, Some(7)) if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::Legacy, tag, Some(8)) if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (
            FeatureManagerLayout::LightsAtSix | FeatureManagerLayout::FoldersAtSeven,
            tag,
            Some(6),
        ) if tag.eq_ignore_ascii_case("Feature") => Some(FeatureTreeNodeRole::LightsAndCameras),
        (FeatureManagerLayout::LightsAtSix, tag, Some(7))
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::LightsAtSix, tag, Some(8))
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::FoldersAtSeven, tag, Some(10))
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::FoldersAtSeven, tag, Some(11 | 12))
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::OriginAtSix, tag, Some(2))
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::OriginAtSix, tag, Some(7))
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::OriginAtSix, tag, Some(8))
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (_, tag, _)
            if tag.eq_ignore_ascii_case("Feature")
                && repeated_builtin_node_kind(ctx, 
                    feature,
                    history_features,
                    layout,
                    FeatureTreeNodeRole::AmbientLight,
                )? =>
        {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (_, tag, _)
            if tag.eq_ignore_ascii_case("Feature")
                && repeated_builtin_node_kind(ctx, 
                    feature,
                    history_features,
                    layout,
                    FeatureTreeNodeRole::DirectionalLight,
                )? =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (_, tag, None) if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::SheetMetal)
        }
        (_, _, _) if empty_feature_tree_node(feature) => Some(FeatureTreeNodeRole::ExplodedViews),
        _ => None,
    })
}

pub(super) fn classless_builtin_node(feature: &Feature) -> bool {
    feature.input_class.is_none() && builtin_node_payload(feature)
}

fn builtin_node_payload(feature: &Feature) -> bool {
    feature.parameters.is_empty()
        && feature.dimension_properties.is_empty()
        && feature.properties.is_empty()
        && feature.text.is_none()
        && feature.content.is_empty()
}

fn classless_or_scene_builtin_node(feature: &Feature) -> bool {
    builtin_node_payload(feature)
        && feature.input_class.as_deref().is_none_or(|class| {
            matches!(
                native_object_class(class).tree_node(),
                Some(
                    FeatureTreeNodeRole::AmbientLight
                        | FeatureTreeNodeRole::DirectionalLight
                        | FeatureTreeNodeRole::PointLight
                        | FeatureTreeNodeRole::SpotLight
                )
            )
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureManagerLayout {
    OriginAtSix,
    LightsAtSix,
    FoldersAtSeven,
    Legacy,
    Current,
}

fn feature_manager_layout(ctx: &cadmpeg_core::decode::DecodeContext<'_>, features: &[Feature]) -> Result<Option<FeatureManagerLayout>, cadmpeg_core::CodecError> {
    let matches_roster = |roster: &[(u32, &str)]| -> Result<bool, cadmpeg_core::CodecError> {
        ctx.admit_iter(roster, "scan SLDPRT builtin class roster")?.try_fold(true, |matches, (source, class)| {
            if !matches { return Ok(false); }
            let mut matches = ctx.admit_iter(features, "scan SLDPRT builtin feature roster")?.filter(|feature| {
                feature.source_value() == Some(*source)
                    && feature.input_class.as_deref() == Some(*class)
            });
            Ok(matches.next().is_some() && matches.next().is_none())
        })
    };
    let matches_builtin_sources = |sources: &[u32]| -> Result<bool, cadmpeg_core::CodecError> {
        ctx.admit_iter(sources, "scan SLDPRT builtin source roster")?.try_fold(true, |matches, source| {
            if !matches { return Ok(false); }
            let mut matches = ctx.admit_iter(features, "scan SLDPRT builtin feature roster")?.filter(|feature| {
                feature.source_value() == Some(*source) && classless_or_scene_builtin_node(feature)
            });
            Ok(matches.next().is_some() && matches.next().is_none())
        })
    };
    let legacy = matches_roster(&[
        (6, "moOriginProfileFeature_c"),
        (9, "moSurfaceBodyFolder_c"),
        (10, "moSolidBodyFolder_c"),
        (12, "moDocsFolder_c"),
        (13, "moCommentsFolder_c"),
    ])?;
    let current = matches_roster(&[
        (7, "moDocsFolder_c"),
        (8, "moCommentsFolder_c"),
        (9, "moSolidBodyFolder_c"),
        (10, "moSurfaceBodyFolder_c"),
    ])?;
    let default_frame = matches_roster(&[
        (1, "moDetailCabinet_c"),
        (2, "moRefPlane_c"),
        (3, "moRefPlane_c"),
        (4, "moRefPlane_c"),
        (5, "moOriginProfileFeature_c"),
    ])?;
    let origin_at_six = matches_roster(&[
        (1, "moDetailCabinet_c"),
        (3, "moRefPlane_c"),
        (4, "moRefPlane_c"),
        (5, "moRefPlane_c"),
        (6, "moOriginProfileFeature_c"),
    ])? && matches_builtin_sources(&[2, 7, 8])?
        && !legacy;
    let lights_at_six = default_frame && matches_builtin_sources(&[6, 7, 8])?;
    let folders_at_seven = default_frame
        && matches_roster(&[(7, "moSolidBodyFolder_c"), (8, "moSurfaceBodyFolder_c")])?
        && matches_builtin_sources(&[6, 10, 11, 12])?;
    let layouts = [
        (origin_at_six, FeatureManagerLayout::OriginAtSix),
        (lights_at_six, FeatureManagerLayout::LightsAtSix),
        (folders_at_seven, FeatureManagerLayout::FoldersAtSeven),
        (legacy, FeatureManagerLayout::Legacy),
        (current, FeatureManagerLayout::Current),
    ];
    let mut layouts = ctx.admit_iter(&layouts, "scan SLDPRT feature manager layouts")?.copied().filter_map(|(matches, layout)| matches.then_some(layout));
    let layout = match layouts.next() { Some(value) => value, None => return Ok(None) };
    Ok(layouts.next().is_none().then_some(layout))
}

fn repeated_builtin_node_kind(ctx: &cadmpeg_core::decode::DecodeContext<'_>, 
    feature: &Feature,
    features: &[Feature],
    layout: FeatureManagerLayout,
    role: FeatureTreeNodeRole,
) -> Result<bool, cadmpeg_core::CodecError> {
    let reserved_source = match (layout, role) {
        (
            FeatureManagerLayout::OriginAtSix | FeatureManagerLayout::LightsAtSix,
            FeatureTreeNodeRole::AmbientLight,
        ) => 7,
        (
            FeatureManagerLayout::OriginAtSix | FeatureManagerLayout::LightsAtSix,
            FeatureTreeNodeRole::DirectionalLight,
        ) => 8,
        (FeatureManagerLayout::FoldersAtSeven, FeatureTreeNodeRole::AmbientLight) => 10,
        (FeatureManagerLayout::FoldersAtSeven, FeatureTreeNodeRole::DirectionalLight) => 11,
        (FeatureManagerLayout::Legacy, FeatureTreeNodeRole::AmbientLight) => 7,
        (FeatureManagerLayout::Legacy, FeatureTreeNodeRole::DirectionalLight) => 8,
        (FeatureManagerLayout::Current, FeatureTreeNodeRole::AmbientLight) => 12,
        (FeatureManagerLayout::Current, FeatureTreeNodeRole::DirectionalLight) => 13,
        _ => return Ok(false),
    };
    let mut anchors = ctx.admit_iter(features, "scan SLDPRT builtin node anchors")?.filter(|candidate| {
        candidate.source_value() == Some(reserved_source) && classless_builtin_node(candidate)
    });
    let Some(anchor) = anchors.next() else {
        return Ok(false);
    };
    Ok(anchors.next().is_none() && !anchor.kind.is_empty() && feature.kind == anchor.kind)
}

fn empty_feature_tree_node(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.name.is_empty()
        && feature.dimension_properties.is_empty()
        && feature.text.is_none()
        && feature.content.is_empty()
}

pub(super) fn feature_family(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Feature, family: &str) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(feature.xml_tag.eq_ignore_ascii_case(family)
        || feature.kind.eq_ignore_ascii_case(family)
        || match classify_type_token(family)
            .or_else(|| classify_xml_element(family)) { Some(expected) => classify(ctx, feature)? == Some(expected), None => false })
}

pub(super) fn feature_input_class(feature: &Feature, class: NativeClassKind) -> bool {
    feature.input_class.as_deref().map(native_object_class) == Some(class)
}

pub(super) fn is_fillet(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Feature) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(classify(ctx, feature)? == Some(FeatureClass::Fillet))
}

pub(super) fn is_chamfer(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Feature) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(classify(ctx, feature)? == Some(FeatureClass::Chamfer))
}

pub(super) fn is_extrude(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Feature) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(classify(ctx, feature)? == Some(FeatureClass::Extrude))
}

pub(super) fn extrude_feature_op(feature: &Feature) -> Option<BooleanOp> {
    // DI-58: the native cut class is authoritative over the localized
    // Keywords type token. A localized BossExtrude token can remain on a
    // feature whose feature-input object is the cut class.
    (feature.input_class.as_deref() == Some("moCut_c"))
        .then_some(BooleanOp::Cut)
        .or_else(|| extrude_op(&feature.kind))
}

pub(super) fn is_offset_plane(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Feature) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(classify(ctx, feature)? == Some(FeatureClass::ReferencePlane)
        && feature
            .parameters
            .get("D1")
            .and_then(|value| parse_dimension_length_mm(value))
            .is_some())
}

pub(super) fn principal_plane_in_history(ctx: &cadmpeg_core::decode::DecodeContext<'_>, 
    feature: &Feature,
    features_by_source: &HashMap<FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Result<Option<cadmpeg_ir::features::PrincipalPlane>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::PrincipalPlane;

    if let Some(plane) = principal_plane_with_siblings(ctx, feature, history_features)? {
        return Ok(Some(plane));
    }
    let legacy_shape = |record: &Feature| {
        record.input_class.is_none()
            && record.xml_tag.eq_ignore_ascii_case("Feature")
            && record.parameters.is_empty()
            && record.properties.is_empty()
            && !record.kind.is_empty()
    };
    let source_triplet = [2, 3, 4].map(|source| -> Result<_, cadmpeg_core::CodecError> {
        Ok::<_, cadmpeg_core::CodecError>(FeatureSource::from_value(source).map(|source| {Ok::<_, cadmpeg_core::CodecError>(ctx.get_hash_map(&(features_by_source), &source, "look up SLDPRT hash key")?.copied())}).transpose()?.flatten())
    });
        let [front, top, right] = source_triplet;
        let source_triplet = [front?, top?, right?];
    if let [Some(front), Some(top), Some(right)] = source_triplet {
        if [front, top, right].into_iter().all(legacy_shape)
            && front.kind == top.kind
            && front.kind == right.kind
        {
            return Ok(match feature.source_value() {
                Some(2) => Some(PrincipalPlane::Front),
                Some(3) => Some(PrincipalPlane::Top),
                Some(4) => Some(PrincipalPlane::Right),
                _ => None,
            });
        }
    }

    let mut triplets = history_features.windows(4).filter_map(|records| {
        let [front, top, right, successor] = records else {
            return None;
        };
        let triplet = [front, top, right];
        if !triplet.into_iter().all(|record| {
            record.xml_tag.eq_ignore_ascii_case("Feature")
                && record.parameters.is_empty()
                && !record.kind.is_empty()
                && match record.input_class.as_deref() {
                    Some(class) => native_object_class(class) == NativeClassKind::ReferencePlane,
                    None => record.properties.is_empty(),
                }
                && record.source_id.is_none()
                && record.tree_parent.is_none()
        }) || front.kind != top.kind
            || front.kind != right.kind
            || top.ordinal != front.ordinal + 1
            || right.ordinal != top.ordinal + 1
            || !successor.xml_tag.eq_ignore_ascii_case("Feature")
            || !successor.parameters.is_empty()
            || !successor.properties.is_empty()
            || successor.kind.is_empty()
            || successor.input_class.as_deref().is_some_and(|class| {
                native_object_class(class) != NativeClassKind::OriginProfileFeature
            })
            || successor.source_id.is_some()
            || successor.tree_parent.is_some()
            || successor.ordinal != right.ordinal + 1
            || successor.kind == front.kind
        {
            return None;
        }
        Some([front, top, right])
    });
    let [front, top, right] = match triplets.next() { Some(value) => value, None => return Ok(None) };
    if triplets.next().is_some() {
        return Ok(None);
    }
    Ok(match feature.id.as_str() {
        id if id == front.id => Some(PrincipalPlane::Front),
        id if id == top.id => Some(PrincipalPlane::Top),
        id if id == right.id => Some(PrincipalPlane::Right),
        _ => None,
    })
}

pub(super) fn extrude_op(kind: &str) -> Option<BooleanOp> {
    if matches_alnum_ascii(kind, b"bossextrude") {
        Some(BooleanOp::Join)
    } else if matches_alnum_ascii(kind, b"cutextrude")
        || matches_alnum_ascii(kind, b"cutextrudethin")
    {
        Some(BooleanOp::Cut)
    } else {
        None
    }
}

pub(crate) fn matches_alnum_ascii(value: &str, expected: &[u8]) -> bool {
    value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .eq(expected.iter().copied())
}

pub(super) fn loft_op(kind: &str) -> Option<BooleanOp> {
    if ["bossloft", "boundaryboss"]
        .iter()
        .any(|name| kind.eq_ignore_ascii_case(name))
    {
        Some(BooleanOp::Join)
    } else if ["cutloft", "boundarycut"]
        .iter()
        .any(|name| kind.eq_ignore_ascii_case(name))
    {
        Some(BooleanOp::Cut)
    } else {
        None
    }
}

pub(super) fn indexed_name(ctx: &cadmpeg_core::decode::DecodeContext<'_>, name: &str, prefix: &str) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(match name.strip_prefix(prefix) {
        Some(suffix) => !suffix.is_empty() && ctx.admit_iter(suffix.as_bytes(), "scan SLDPRT indexed name digits")?.all(u8::is_ascii_digit),
        None => false,
    })
}
