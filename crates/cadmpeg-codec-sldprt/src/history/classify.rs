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

pub(crate) fn is_custom_property(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("CustomProperty")
}

pub(crate) fn is_semantic_note(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("Note")
        && feature.kind.eq_ignore_ascii_case("Note")
        && feature.text.as_ref().is_some_and(|text| !text.is_empty())
        && feature.parameters.is_empty()
        && feature.properties.is_empty()
}

pub(crate) fn is_attribute_definition(feature: &Feature) -> bool {
    feature.input_class.is_none()
        && feature.source_id.as_deref() == Some("-1")
        && feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.kind.eq_ignore_ascii_case("Attribute-Definition")
        && !feature.name.is_empty()
}

pub(crate) fn is_history_metadata_record(feature: &Feature, features: &[Feature]) -> bool {
    if is_custom_property(feature)
        || is_semantic_note(feature)
        || is_attribute_definition(feature)
        || matches!(
            feature.input_class.as_deref(),
            Some("moAlignGroup_c" | "moAttribute_c" | "moConfigCommentsFolder_c")
        )
    {
        return true;
    }
    feature.input_class.is_none()
        && feature.source_id.as_deref() == Some("-1")
        && !feature.name.is_empty()
        && features.iter().any(|candidate| {
            candidate.input_class.as_deref() == Some("moAttribute_c")
                && candidate.name.starts_with(&feature.name)
        })
}

pub(crate) fn feature_tree_node_role(
    feature: &Feature,
    history_features: &[Feature],
) -> Option<FeatureTreeNodeRole> {
    reserved_feature_tree_node_role(feature, history_features)
        .or_else(|| native_object_class(feature.input_class.as_deref()?).tree_node)
        .or_else(|| equation_container_role(feature))
}

/// Keywords operation-family token of the equations container.
pub(crate) const EQUATION_DRIVEN_TOKEN: &str = "EquationDriven";

/// The equations container identified by its Keywords operation-family token.
///
/// The token is a role code: it identifies the container without a native class
/// or a reserved source identifier.
pub(crate) fn equation_container_role(feature: &Feature) -> Option<FeatureTreeNodeRole> {
    (feature.input_class.is_none()
        && feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.kind.eq_ignore_ascii_case(EQUATION_DRIVEN_TOKEN))
    .then_some(FeatureTreeNodeRole::Equations)
}

pub(crate) fn reserved_feature_tree_node_role(
    feature: &Feature,
    history_features: &[Feature],
) -> Option<FeatureTreeNodeRole> {
    let layout = feature_manager_layout(history_features)?;
    if !classless_builtin_node(feature) {
        return None;
    }
    let source = feature.source_id.as_deref()?;
    match (layout, feature.xml_tag.as_str(), source) {
        (FeatureManagerLayout::Current, tag, "1") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::Annotations)
        }
        (FeatureManagerLayout::Current, tag, "5") if tag.eq_ignore_ascii_case("Sketch") => {
            Some(FeatureTreeNodeRole::ModelOrigin)
        }
        (FeatureManagerLayout::Current, tag, "6") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::Current, tag, "12") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::Current, tag, "13" | "14" | "15")
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::Legacy, tag, "2") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::Legacy, tag, "7") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::Legacy, tag, "8") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::LightsAtSix | FeatureManagerLayout::FoldersAtSeven, tag, "6")
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::LightsAtSix, tag, "7") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::LightsAtSix, tag, "8") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::FoldersAtSeven, tag, "10")
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::FoldersAtSeven, tag, "11" | "12")
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::OriginAtSix, tag, "2") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::OriginAtSix, tag, "7") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::OriginAtSix, tag, "8") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (_, tag, _)
            if tag.eq_ignore_ascii_case("Feature")
                && repeated_builtin_node_kind(
                    feature,
                    history_features,
                    layout,
                    FeatureTreeNodeRole::AmbientLight,
                ) =>
        {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (_, tag, _)
            if tag.eq_ignore_ascii_case("Feature")
                && repeated_builtin_node_kind(
                    feature,
                    history_features,
                    layout,
                    FeatureTreeNodeRole::DirectionalLight,
                ) =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (_, tag, "-1") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::SheetMetal)
        }
        (_, _, _) if empty_feature_tree_node(feature) => Some(FeatureTreeNodeRole::ExplodedViews),
        _ => None,
    }
}

pub(crate) fn classless_builtin_node(feature: &Feature) -> bool {
    feature.input_class.is_none() && builtin_node_payload(feature)
}

pub(crate) fn builtin_node_payload(feature: &Feature) -> bool {
    feature.parameters.is_empty()
        && feature.dimension_properties.is_empty()
        && feature.properties.is_empty()
        && feature.text.is_none()
        && feature.content.is_empty()
}

pub(crate) fn classless_or_scene_builtin_node(feature: &Feature) -> bool {
    builtin_node_payload(feature)
        && feature.input_class.as_deref().is_none_or(|class| {
            matches!(
                native_object_class(class).tree_node,
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
pub(crate) enum FeatureManagerLayout {
    OriginAtSix,
    LightsAtSix,
    FoldersAtSeven,
    Legacy,
    Current,
}

pub(crate) fn feature_manager_layout(features: &[Feature]) -> Option<FeatureManagerLayout> {
    let matches_roster = |roster: &[(&str, &str)]| {
        roster.iter().all(|(source, class)| {
            let mut matches = features.iter().filter(|feature| {
                feature.source_id.as_deref() == Some(*source)
                    && feature.input_class.as_deref() == Some(*class)
            });
            matches.next().is_some() && matches.next().is_none()
        })
    };
    let matches_builtin_sources = |sources: &[&str]| {
        sources.iter().all(|source| {
            let mut matches = features.iter().filter(|feature| {
                feature.source_id.as_deref() == Some(*source)
                    && classless_or_scene_builtin_node(feature)
            });
            matches.next().is_some() && matches.next().is_none()
        })
    };
    let legacy = matches_roster(&[
        ("6", "moOriginProfileFeature_c"),
        ("9", "moSurfaceBodyFolder_c"),
        ("10", "moSolidBodyFolder_c"),
        ("12", "moDocsFolder_c"),
        ("13", "moCommentsFolder_c"),
    ]);
    let current = matches_roster(&[
        ("7", "moDocsFolder_c"),
        ("8", "moCommentsFolder_c"),
        ("9", "moSolidBodyFolder_c"),
        ("10", "moSurfaceBodyFolder_c"),
    ]);
    let default_frame = matches_roster(&[
        ("1", "moDetailCabinet_c"),
        ("2", "moRefPlane_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moOriginProfileFeature_c"),
    ]);
    let origin_at_six = matches_roster(&[
        ("1", "moDetailCabinet_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moRefPlane_c"),
        ("6", "moOriginProfileFeature_c"),
    ]) && matches_builtin_sources(&["2", "7", "8"])
        && !legacy;
    let lights_at_six = default_frame && matches_builtin_sources(&["6", "7", "8"]);
    let folders_at_seven = default_frame
        && matches_roster(&[("7", "moSolidBodyFolder_c"), ("8", "moSurfaceBodyFolder_c")])
        && matches_builtin_sources(&["6", "10", "11", "12"]);
    let mut layouts = [
        (origin_at_six, FeatureManagerLayout::OriginAtSix),
        (lights_at_six, FeatureManagerLayout::LightsAtSix),
        (folders_at_seven, FeatureManagerLayout::FoldersAtSeven),
        (legacy, FeatureManagerLayout::Legacy),
        (current, FeatureManagerLayout::Current),
    ]
    .into_iter()
    .filter_map(|(matches, layout)| matches.then_some(layout));
    let layout = layouts.next()?;
    layouts.next().is_none().then_some(layout)
}

pub(crate) fn repeated_builtin_node_kind(
    feature: &Feature,
    features: &[Feature],
    layout: FeatureManagerLayout,
    role: FeatureTreeNodeRole,
) -> bool {
    let reserved_source = match (layout, role) {
        (
            FeatureManagerLayout::OriginAtSix | FeatureManagerLayout::LightsAtSix,
            FeatureTreeNodeRole::AmbientLight,
        ) => "7",
        (
            FeatureManagerLayout::OriginAtSix | FeatureManagerLayout::LightsAtSix,
            FeatureTreeNodeRole::DirectionalLight,
        ) => "8",
        (FeatureManagerLayout::FoldersAtSeven, FeatureTreeNodeRole::AmbientLight) => "10",
        (FeatureManagerLayout::FoldersAtSeven, FeatureTreeNodeRole::DirectionalLight) => "11",
        (FeatureManagerLayout::Legacy, FeatureTreeNodeRole::AmbientLight) => "7",
        (FeatureManagerLayout::Legacy, FeatureTreeNodeRole::DirectionalLight) => "8",
        (FeatureManagerLayout::Current, FeatureTreeNodeRole::AmbientLight) => "12",
        (FeatureManagerLayout::Current, FeatureTreeNodeRole::DirectionalLight) => "13",
        _ => return false,
    };
    let mut anchors = features.iter().filter(|candidate| {
        candidate.source_id.as_deref() == Some(reserved_source) && classless_builtin_node(candidate)
    });
    let Some(anchor) = anchors.next() else {
        return false;
    };
    anchors.next().is_none() && !anchor.kind.is_empty() && feature.kind == anchor.kind
}

pub(crate) fn empty_feature_tree_node(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.name.is_empty()
        && feature.dimension_properties.is_empty()
        && feature.text.is_none()
        && feature.content.is_empty()
}

pub(crate) fn feature_family(feature: &Feature, family: &str) -> bool {
    feature.xml_tag.eq_ignore_ascii_case(family)
        || feature.kind.eq_ignore_ascii_case(family)
        || classify_type_token(family)
            .or_else(|| classify_xml_element(family))
            .is_some_and(|expected| classify(feature) == Some(expected))
}

pub(crate) fn feature_input_class(feature: &Feature, class: NativeClassKind) -> bool {
    feature
        .input_class
        .as_deref()
        .map(native_object_class)
        .map(|class| class.kind)
        == Some(class)
}

pub(crate) fn is_fillet(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Fillet)
}

pub(crate) fn is_chamfer(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Chamfer)
}

pub(crate) fn is_extrude(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Extrude)
}

pub(crate) fn extrude_feature_op(feature: &Feature) -> Option<BooleanOp> {
    // DI-58: the native cut class is authoritative over the localized
    // Keywords type token. A localized BossExtrude token can remain on a
    // feature whose feature-input object is the cut class.
    (feature.input_class.as_deref() == Some("moCut_c"))
        .then_some(BooleanOp::Cut)
        .or_else(|| extrude_op(&feature.kind))
}

pub(crate) fn is_offset_plane(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::ReferencePlane)
        && feature
            .parameters
            .get("D1")
            .and_then(|value| parse_dimension_length_mm(value))
            .is_some()
}

pub(crate) fn principal_plane_in_history(
    feature: &Feature,
    features_by_source: &HashMap<&str, &Feature>,
    history_features: &[Feature],
) -> Option<cadmpeg_ir::features::PrincipalPlane> {
    use cadmpeg_ir::features::PrincipalPlane;

    if let Some(plane) = principal_plane_with_siblings(feature, history_features) {
        return Some(plane);
    }
    let legacy_shape = |record: &Feature| {
        record.input_class.is_none()
            && record.xml_tag.eq_ignore_ascii_case("Feature")
            && record.parameters.is_empty()
            && record.properties.is_empty()
            && !record.kind.is_empty()
    };
    let source_triplet = ["2", "3", "4"].map(|source| features_by_source.get(source).copied());
    if let [Some(front), Some(top), Some(right)] = source_triplet {
        if [front, top, right].into_iter().all(legacy_shape)
            && front.kind == top.kind
            && front.kind == right.kind
        {
            return match feature.source_id.as_deref() {
                Some("2") => Some(PrincipalPlane::Front),
                Some("3") => Some(PrincipalPlane::Top),
                Some("4") => Some(PrincipalPlane::Right),
                _ => None,
            };
        }
    }

    let triplets = history_features
        .windows(4)
        .filter_map(|records| {
            let [front, top, right, successor] = records else {
                return None;
            };
            let triplet = [front, top, right];
            if !triplet.into_iter().all(|record| {
                record.xml_tag.eq_ignore_ascii_case("Feature")
                    && record.parameters.is_empty()
                    && !record.kind.is_empty()
                    && match record.input_class.as_deref() {
                        Some(class) => {
                            native_object_class(class).kind == NativeClassKind::ReferencePlane
                        }
                        None => record.properties.is_empty(),
                    }
                    && record.source_id.is_none()
                    && record.tree_parent.is_none()
                    && record.parent_source_id.is_none()
            }) || front.kind != top.kind
                || front.kind != right.kind
                || top.ordinal != front.ordinal + 1
                || right.ordinal != top.ordinal + 1
                || !successor.xml_tag.eq_ignore_ascii_case("Feature")
                || !successor.parameters.is_empty()
                || !successor.properties.is_empty()
                || successor.kind.is_empty()
                || successor.input_class.as_deref().is_some_and(|class| {
                    native_object_class(class).kind != NativeClassKind::OriginProfileFeature
                })
                || successor.source_id.is_some()
                || successor.tree_parent.is_some()
                || successor.parent_source_id.is_some()
                || successor.ordinal != right.ordinal + 1
                || successor.kind == front.kind
            {
                return None;
            }
            Some([front, top, right])
        })
        .collect::<Vec<_>>();
    let [triplet] = triplets.as_slice() else {
        return None;
    };
    let [front, top, right] = *triplet;
    match feature.id.as_str() {
        id if id == front.id => Some(PrincipalPlane::Front),
        id if id == top.id => Some(PrincipalPlane::Top),
        id if id == right.id => Some(PrincipalPlane::Right),
        _ => None,
    }
}

pub(crate) fn extrude_op(kind: &str) -> Option<BooleanOp> {
    let kind = kind
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    match kind.as_slice() {
        b"bossextrude" => Some(BooleanOp::Join),
        b"cutextrude" | b"cutextrudethin" => Some(BooleanOp::Cut),
        _ => None,
    }
}

pub(crate) fn loft_op(kind: &str) -> Option<BooleanOp> {
    match kind.to_ascii_lowercase().as_str() {
        "bossloft" | "boundaryboss" => Some(BooleanOp::Join),
        "cutloft" | "boundarycut" => Some(BooleanOp::Cut),
        _ => None,
    }
}

pub(crate) fn indexed_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}
