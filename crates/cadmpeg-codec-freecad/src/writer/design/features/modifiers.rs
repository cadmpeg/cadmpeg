// SPDX-License-Identifier: Apache-2.0
//! Edge treatments, thickness, offsets, and typed binders.

use super::super::super::xml::{self, Object};
use super::properties::{
    angle, boolean, enumeration, length, link, number, references, selection, selector, tag,
    unsupported,
};
use cadmpeg_core::CodecError;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn dress_up(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let groups = value["groups"]
        .as_array()
        .ok_or_else(|| CodecError::UnsupportedContent("edge treatment groups absent".into()))?;
    if groups.len() != 1 {
        return unsupported("FreeCAD edge treatment requires one uniform edge group");
    }
    let group = &groups[0];
    let fillet = tag(value) == "fillet";
    object.kind = if fillet {
        "PartDesign::Fillet"
    } else {
        "PartDesign::Chamfer"
    }
    .into();
    if tag(&group["edges"]) == "all" {
        boolean(object, "UseAllEdges", true);
        let source = object.dependencies.last().cloned().ok_or_else(|| {
            CodecError::UnsupportedContent("all-edge treatment has no predecessor".into())
        })?;
        object.set(
            "Base",
            xml::scalar("Base", "App::PropertyLink", "Link", &source),
        );
    } else {
        selection(object, "Base", &group["edges"], names)?;
        boolean(object, "UseAllEdges", false);
    }
    if fillet {
        if tag(&group["radius"]) != "constant" {
            return unsupported("variable fillet needs native edge-radius law");
        }
        length(
            object,
            "Radius",
            number(&group["radius"]["radius"], "fillet radius")?,
        );
    } else {
        let spec = &group["spec"];
        let mode = match tag(spec) {
            "distance" => 0,
            "two_distances" => 1,
            "distance_angle" => 2,
            _ => return unsupported("chamfer law has no FreeCAD encoding"),
        };
        enumeration(object, "ChamferType", mode);
        match mode {
            0 => length(
                object,
                "Size",
                number(&spec["distance"], "chamfer distance")?,
            ),
            1 => {
                length(
                    object,
                    "Size",
                    number(&spec["first"], "first chamfer distance")?,
                );
                length(
                    object,
                    "Size2",
                    number(&spec["second"], "second chamfer distance")?,
                );
            }
            _ => {
                length(
                    object,
                    "Size",
                    number(&spec["distance"], "chamfer distance")?,
                );
                angle(object, "Angle", number(&spec["angle"], "chamfer angle")?);
            }
        }
        boolean(
            object,
            "FlipDirection",
            value["flip_direction"].as_bool().unwrap_or(false),
        );
    }
    Ok(())
}
pub(super) fn shell(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    if tag(value) == "shell" {
        object.kind = "PartDesign::Thickness".into();
        selection(object, "Base", &value["removed_faces"], names)?;
        length(
            object,
            "Value",
            number(&value["thickness"], "wall thickness")?,
        );
        boolean(
            object,
            "Reversed",
            !value["outward"].as_bool().unwrap_or(true),
        );
    } else {
        object.kind = if value["planar"] == true {
            "Part::Offset2D"
        } else {
            "Part::Offset"
        }
        .into();
        link(object, "Source", &value["source"], names)?;
        length(
            object,
            "Value",
            number(&value["distance"], "offset distance")?,
        );
        boolean(object, "Fill", value["fill"].as_bool().unwrap_or(false));
    }
    if !value["mode"].is_null() {
        enumeration(
            object,
            "Mode",
            selector(&value["mode"], &["skin", "pipe", "both_sides"])?,
        );
    }
    if !value["join"].is_null() {
        let variants = if tag(value) == "shell" {
            &["arc", "intersection"][..]
        } else {
            &["arc", "tangent", "intersection"][..]
        };
        enumeration(object, "Join", selector(&value["join"], variants)?);
    }
    boolean(
        object,
        "Intersection",
        value["resolve_intersections"].as_bool().unwrap_or(false),
    );
    boolean(
        object,
        "SelfIntersection",
        value["allow_self_intersections"].as_bool().unwrap_or(false),
    );
    Ok(())
}
pub(super) fn binder(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let construction = &value["construction"];
    object.kind = if tag(construction) == "shape" {
        "PartDesign::ShapeBinder"
    } else {
        "PartDesign::SubShapeBinder"
    }
    .into();
    let sources = value["sources"]
        .as_array()
        .ok_or_else(|| CodecError::UnsupportedContent("binder sources absent".into()))?;
    let mut content = format!("<LinkSubList count=\"{}\">", sources.len());
    for source in sources {
        let targets = references(&source["target"], names)?;
        if targets.len() != 1 {
            return unsupported("binder source has no singular local target");
        }
        let subelements = source["subelements"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        content.push_str(&xml::element(
            "Link",
            &[("obj", targets[0].clone()), ("sub", subelements)],
        ));
    }
    content.push_str("</LinkSubList>");
    object.set(
        "Support",
        xml::property("Support", "App::PropertyLinkSubList", &content),
    );
    if tag(construction) == "shape" {
        boolean(
            object,
            "TraceSupport",
            construction["trace_support"].as_bool().unwrap_or(false),
        );
    } else {
        enumeration(
            object,
            "BindMode",
            selector(
                &construction["lifecycle"],
                &["synchronized", "frozen", "detached"],
            )?,
        );
        enumeration(
            object,
            "BindCopyOnChange",
            selector(
                &construction["copy_on_change"],
                &["disabled", "enabled", "mutated"],
            )?,
        );
        boolean(object, "Relative", construction["placement"] == "relative");
        for (key, target) in [
            ("claim_children", "ClaimChildren"),
            ("fuse", "Fuse"),
            ("make_face", "MakeFace"),
            ("partial_load", "PartialLoad"),
            ("refine", "Refine"),
        ] {
            if let Some(value) = construction[key].as_bool() {
                boolean(object, target, value);
            }
        }
        if !construction["context"].is_null() {
            link(object, "Context", &construction["context"], names)?;
        }
        let offset = &construction["offset"];
        if !offset.is_null() {
            object.set(
                "Offset",
                xml::scalar(
                    "Offset",
                    "App::PropertyFloat",
                    "Float",
                    number(&offset["distance"], "binder offset")?,
                ),
            );
            enumeration(
                object,
                "OffsetJoinType",
                selector(&offset["join"], &["arcs", "tangent", "intersection"])?,
            );
            for (key, target) in [
                ("fill", "OffsetFill"),
                ("open_result", "OffsetOpenResult"),
                ("intersection", "OffsetIntersection"),
            ] {
                boolean(object, target, offset[key].as_bool().unwrap_or(false));
            }
        }
    }
    Ok(())
}
