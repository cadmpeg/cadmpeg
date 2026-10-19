// SPDX-License-Identifier: Apache-2.0
//! Profile extrusion, revolution, loft, and sweep construction.

use super::super::super::xml::{self, Object};
use super::properties::{
    angle, boolean, enumeration, length, link, link_names, links, number, references, selection,
    selector, tag, unsupported, vector,
};
use cadmpeg_core::CodecError;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn extrude(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    if value["op"] == "new_body" {
        return standalone_extrude(value, object, names);
    }
    object.kind = if value["op"] == "cut" {
        "PartDesign::Pocket"
    } else {
        "PartDesign::Pad"
    }
    .into();
    link(object, "Profile", &value["profile"], names)?;
    let direction = &value["direction"];
    match tag(direction) {
        "explicit" => {
            if direction["source"]["kind"] == "profile_normal" {
                boolean(object, "UseCustomVector", false);
            } else if direction["source"]["kind"] == "edge" {
                boolean(object, "UseCustomVector", false);
                link(
                    object,
                    "ReferenceAxis",
                    &direction["source"]["reference"],
                    names,
                )?;
                vector(object, "Direction", &direction["vector"])?;
            } else {
                boolean(object, "UseCustomVector", true);
                vector(object, "Direction", &direction["vector"])?;
            }
        }
        "profile_normal" => boolean(object, "UseCustomVector", false),
        "reversed_profile_normal" => {
            boolean(object, "UseCustomVector", false);
            boolean(object, "Reversed", true);
        }
        _ => return unsupported("extrusion direction unresolved"),
    }
    let extent = &value["extent"];
    let two = tag(extent) == "two_sided";
    enumeration(
        object,
        "SideType",
        if two {
            1
        } else if tag(extent) == "symmetric" {
            2
        } else {
            0
        },
    );
    boolean(object, "Midplane", tag(extent) == "symmetric");
    linear_side(
        if two {
            &extent["first"]
        } else {
            &extent["side"]
        },
        object,
        "",
        names,
    )?;
    if two {
        linear_side(&extent["second"], object, "2", names)?;
    }
    for (source, target) in [
        ("length_along_profile_normal", "AlongSketchNormal"),
        ("allow_multi_profile_faces", "AllowMultiFace"),
    ] {
        if let Some(value) = value[source].as_bool() {
            boolean(object, target, value);
        }
    }
    Ok(())
}
pub(super) fn linear_side(
    side: &Value,
    object: &mut Object,
    suffix: &str,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let term = side.get("termination").unwrap_or(side);
    let kind = tag(term);
    let mode = match kind {
        "blind" => 0,
        "through_all" | "to_last" => 1,
        "to_first" => 2,
        "to_face" => 3,
        "to_shape" => 5,
        _ => return unsupported(format!("linear termination `{kind}` unsupported")),
    };
    enumeration(object, &format!("Type{suffix}"), mode);
    if kind == "blind" {
        length(
            object,
            &format!("Length{suffix}"),
            number(&term["length"], "extrusion length")?,
        );
    }
    for (source, target) in [("face", "UpToFace"), ("target", "UpToShape")] {
        if !term[source].is_null() {
            selection(object, &format!("{target}{suffix}"), &term[source], names)?;
        }
    }
    if !term["offset"].is_null() {
        length(
            object,
            &format!("Offset{suffix}"),
            number(&term["offset"], "extrusion offset")?,
        );
    }
    if !side["draft"].is_null() {
        angle(
            object,
            if suffix.is_empty() {
                "TaperAngle"
            } else {
                "TaperAngle2"
            },
            number(&side["draft"], "draft angle")?,
        );
    }
    Ok(())
}
pub(super) fn revolve(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let construction = &value["construction"];
    if construction["state"] != "resolved" {
        return unsupported("revolution construction unresolved");
    }
    object.kind = if value["op"] == "cut" {
        "PartDesign::Groove"
    } else if value["op"] == "new_body" {
        "Part::Revolution"
    } else {
        "PartDesign::Revolution"
    }
    .into();
    link(object, "Profile", &construction["profile"], names)?;
    vector(object, "Base", &construction["axis"]["origin"])?;
    vector(object, "Axis", &construction["axis"]["direction"])?;
    let extent = &construction["extent"];
    boolean(object, "Symmetric", tag(extent) == "symmetric");
    boolean(
        object,
        "Solid",
        construction["solid"].as_bool().unwrap_or(true),
    );
    boolean(object, "Midplane", tag(extent) == "symmetric");
    if tag(extent) == "two_sided" {
        enumeration(object, "Type", 4);
        angular_side(&extent["first"], object, "", names)?;
        angular_side(&extent["second"], object, "2", names)?;
    } else {
        angular_side(&extent["termination"], object, "", names)?;
    }
    Ok(())
}
pub(super) fn angular_side(
    term: &Value,
    object: &mut Object,
    suffix: &str,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    match tag(term) {
        "angle" => {
            if suffix.is_empty() && !object.properties.contains_key("Type") {
                enumeration(object, "Type", 0);
            }
            angle(
                object,
                &format!("Angle{suffix}"),
                number(&term["angle"], "revolution angle")?,
            );
        }
        "through_all" => enumeration(object, "Type", 1),
        "to_first" => enumeration(object, "Type", 2),
        "to_face" => {
            enumeration(object, "Type", 3);
            selection(object, "UpToFace", &term["face"], names)?;
        }
        _ => return unsupported("revolution termination has no FreeCAD encoding"),
    }
    Ok(())
}

pub(super) fn loft(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    object.kind = match value["op"].as_str() {
        Some("cut") => "PartDesign::SubtractiveLoft",
        Some("new_body") => "Part::Loft",
        _ => "PartDesign::AdditiveLoft",
    }
    .into();
    let sections = value["sections"]
        .as_array()
        .ok_or_else(|| CodecError::UnsupportedContent("loft sections absent".into()))?;
    let profiles = sections
        .iter()
        .map(|section| references(section.get("value").unwrap_or(section), names))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if profiles.len() < 2 {
        return unsupported("loft needs at least two profiles");
    }
    if object.kind == "Part::Loft" {
        link_names(object, "Sections", &profiles);
    } else {
        object.set(
            "Profile",
            xml::scalar("Profile", "App::PropertyLink", "Link", &profiles[0]),
        );
        link_names(object, "Sections", &profiles[1..]);
    }
    for (key, target) in [
        ("closed", "Closed"),
        ("solid", "Solid"),
        ("ruled", "Ruled"),
        ("linearize", "Linearize"),
        ("allow_multi_profile_faces", "AllowMultiFace"),
    ] {
        if let Some(value) = value[key].as_bool() {
            boolean(object, target, value);
        }
    }
    if let Some(degree) = value["max_degree"].as_u64() {
        object.set(
            "MaxDegree",
            xml::scalar("MaxDegree", "App::PropertyInteger", "Integer", degree),
        );
    }
    Ok(())
}
pub(super) fn sweep(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    for field in ["twist", "path_extent", "guide_rail", "taper", "scale"] {
        if !value[field].is_null() {
            return unsupported(format!("sweep modifier `{field}` has no FreeCAD mapping"));
        }
    }
    let shape = &value["shape"];
    object.kind = if tag(shape) == "surface" || shape["op"] == "new_body" {
        "Part::Sweep"
    } else if shape["op"] == "cut" {
        "PartDesign::SubtractivePipe"
    } else {
        "PartDesign::AdditivePipe"
    }
    .into();
    link(object, "Profile", &shape["section"], names)?;
    links(object, "Sections", &shape["sections"], names)?;
    selection(object, "Spine", &value["path"], names)?;
    boolean(object, "Solid", tag(shape) != "surface");
    boolean(
        object,
        "SpineTangent",
        value["path_tangent"].as_bool().unwrap_or(false),
    );
    boolean(
        object,
        "Linearize",
        value["linearize"].as_bool().unwrap_or(false),
    );
    if !value["transition"].is_null() {
        enumeration(
            object,
            "Transition",
            selector(
                &value["transition"],
                &["transformed", "right_corner", "round_corner"],
            )?,
        );
    }
    let orientation = &value["orientation"];
    let mode = match tag(orientation) {
        "" | "corrected_frenet" => 0,
        "fixed" => 1,
        "frenet" => 2,
        "auxiliary" => 3,
        "binormal" => 4,
        _ => return unsupported("sweep orientation has no FreeCAD encoding"),
    };
    enumeration(object, "Mode", mode);
    if mode == 3 {
        selection(object, "AuxiliarySpine", &orientation["path"], names)?;
        boolean(
            object,
            "AuxiliarySpineTangent",
            orientation["tangent"].as_bool().unwrap_or(false),
        );
        boolean(
            object,
            "AuxiliaryCurvilinear",
            orientation["curvilinear"].as_bool().unwrap_or(true),
        );
    }
    if mode == 4 {
        vector(object, "Binormal", &orientation["direction"])?;
    }
    if !value["transformation"].is_null() {
        enumeration(
            object,
            "Transformation",
            selector(
                &value["transformation"],
                &[
                    "constant",
                    "multi_section",
                    "linear",
                    "s_shape",
                    "interpolation",
                ],
            )?,
        );
    }
    if object.kind == "Part::Sweep" {
        boolean(object, "Frenet", mode == 2);
    }
    if let Some(value) = value["allow_multi_profile_faces"].as_bool() {
        boolean(object, "AllowMultiFace", value);
    }
    Ok(())
}

pub(super) fn helical_sweep(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let construction = &value["construction"];
    object.kind = if matches!(value["op"].as_str(), Some("cut" | "intersect")) {
        "PartDesign::SubtractiveHelix"
    } else {
        "PartDesign::AdditiveHelix"
    }
    .into();
    boolean(object, "Outside", value["op"] == "intersect");
    link(object, "Profile", &construction["profile"], names)?;
    vector(object, "Base", &construction["axis_origin"])?;
    vector(object, "Axis", &construction["axis_direction"])?;
    enumeration(
        object,
        "Mode",
        selector(
            &construction["law"],
            &[
                "pitch_height_angle",
                "pitch_turns_angle",
                "height_turns_angle",
                "height_turns_growth",
            ],
        )?,
    );
    length(
        object,
        "Pitch",
        number(&construction["pitch"], "helical pitch")?,
    );
    length(
        object,
        "Height",
        construction["travel"]["height"].as_f64().unwrap_or(0.0),
    );
    length(
        object,
        "Growth",
        construction["travel"]["radial_growth"]
            .as_f64()
            .unwrap_or(0.0),
    );
    length(
        object,
        "Turns",
        number(&construction["turns"], "helical turns")?,
    );
    angle(
        object,
        "Angle",
        number(&construction["cone_angle"], "helical cone angle")?,
    );
    for (source, target) in [
        ("left_handed", "LeftHanded"),
        ("reversed", "Reversed"),
        ("allow_multi_profile_faces", "AllowMultiFace"),
    ] {
        if let Some(value) = construction[source].as_bool() {
            boolean(object, target, value);
        }
    }
    if !construction["tolerance"].is_null() {
        object.set(
            "Tolerance",
            xml::scalar(
                "Tolerance",
                "App::PropertyFloatConstraint",
                "Float",
                number(&construction["tolerance"], "helical tolerance")?,
            ),
        );
    }
    Ok(())
}

pub(super) fn standalone_extrude(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    object.kind = "Part::Extrusion".into();
    link(object, "Base", &value["profile"], names)?;
    match tag(&value["direction"]) {
        "explicit" => {
            enumeration(object, "DirMode", 0);
            vector(object, "Dir", &value["direction"]["vector"])?;
        }
        "profile_normal" => enumeration(object, "DirMode", 2),
        "reversed_profile_normal" => {
            enumeration(object, "DirMode", 2);
            boolean(object, "Reversed", true);
        }
        _ => return unsupported("standalone extrusion direction unresolved"),
    }
    let extent = &value["extent"];
    let two = tag(extent) == "two_sided";
    let side = if two {
        &extent["first"]
    } else {
        &extent["side"]
    };
    if tag(&side["termination"]) != "blind" {
        return unsupported("standalone extrusion requires a finite length");
    }
    length(
        object,
        "LengthFwd",
        number(&side["termination"]["length"], "extrusion length")?,
    );
    if !side["draft"].is_null() {
        angle(
            object,
            "TaperAngle",
            number(&side["draft"], "extrusion draft")?,
        );
    }
    if two {
        let second = &extent["second"];
        if tag(&second["termination"]) != "blind" {
            return unsupported("standalone reverse extrusion requires finite length");
        }
        length(
            object,
            "LengthRev",
            -number(&second["termination"]["length"], "reverse extrusion length")?,
        );
        if !second["draft"].is_null() {
            angle(
                object,
                "TaperAngleRev",
                number(&second["draft"], "reverse extrusion draft")?,
            );
        }
    }
    boolean(object, "Symmetric", tag(extent) == "symmetric");
    boolean(object, "Solid", value["solid"].as_bool().unwrap_or(true));
    if let Some(maker) = value["face_maker_class"].as_str() {
        object.set("FaceMakerClass", xml::string("FaceMakerClass", maker));
    }
    if let Some(taper) = value["inner_wire_taper"].as_str() {
        object.set(
            "InnerWireTaper",
            xml::scalar(
                "InnerWireTaper",
                "App::PropertyInteger",
                "Integer",
                usize::from(taper == "same_as_outer"),
            ),
        );
    }
    Ok(())
}
