// SPDX-License-Identifier: Apache-2.0
//! Neutral bore, entry-treatment, and termination properties.

use super::super::super::xml::{self, Object};
use super::properties::{angle, boolean, enumeration, length, link, number, tag, unsupported};
use cadmpeg_core::CodecError;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn hole(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    object.kind = "PartDesign::Hole".into();
    link(object, "Profile", &value["profile"], names)?;
    let shape = &value["shape"];
    if !shape["exit_kind"].is_null() {
        return unsupported("FreeCAD holes do not encode an independent exit treatment");
    }
    length(
        object,
        "Diameter",
        number(&shape["diameter"], "hole diameter")?,
    );
    let construction = &shape["construction"];
    if construction["construction"] != "form" {
        return unsupported("native-thread hole has no neutral FreeCAD construction");
    }
    let treatment = &construction["kind"];
    let mode = match tag(treatment) {
        "simple" => 0,
        "counterbore" => 1,
        "countersink" => 2,
        "counterdrill" => 3,
        _ => return unsupported("hole treatment has no FreeCAD encoding"),
    };
    enumeration(object, "HoleCutType", mode);
    if mode == 1 || mode == 2 {
        length(
            object,
            "HoleCutDiameter",
            number(&treatment["diameter"], "hole treatment diameter")?,
        );
    }
    if mode == 1 || mode == 3 {
        length(
            object,
            "HoleCutDepth",
            number(&treatment["depth"], "hole treatment depth")?,
        );
    }
    if mode == 2 || mode == 3 {
        angle(
            object,
            "HoleCutCountersinkAngle",
            number(&treatment["angle"], "hole treatment angle")?,
        );
    }
    if mode == 3 {
        length(
            object,
            "HoleCutDiameter",
            number(&treatment["diameter"], "counterdrill recess diameter")?,
        );
    }
    let extent = &value["extent"];
    match tag(extent) {
        "blind" => {
            enumeration(object, "DepthType", 0);
            length(object, "Depth", number(&extent["length"], "hole depth")?);
        }
        "through_all" => enumeration(object, "DepthType", 1),
        _ => return unsupported("hole depth has no FreeCAD encoding"),
    }
    let bottom = &value["bottom"];
    match tag(bottom) {
        "flat" => enumeration(object, "DrillPoint", 0),
        "angled" => {
            enumeration(object, "DrillPoint", 1);
            angle(
                object,
                "DrillPointAngle",
                number(&bottom["included_angle"], "drill point angle")?,
            );
            boolean(
                object,
                "DrillForDepth",
                bottom["depth_to_tip"].as_bool().unwrap_or(false),
            );
        }
        _ => return unsupported("hole bottom has no FreeCAD encoding"),
    }
    boolean(object, "Tapered", !value["taper_angle"].is_null());
    if !value["taper_angle"].is_null() {
        angle(
            object,
            "TaperedAngle",
            number(&value["taper_angle"], "hole taper")?,
        );
    }
    specification(&construction["specification"], object)?;
    let filter = value["profile_filter"]
        .as_str()
        .unwrap_or("circles_and_arcs");
    let filters = [
        "points",
        "circles",
        "points_and_circles",
        "arcs",
        "points_and_arcs",
        "circles_and_arcs",
        "all",
    ];
    let filter = filters
        .iter()
        .position(|item| *item == filter)
        .ok_or_else(|| CodecError::UnsupportedContent("hole profile filter unsupported".into()))?
        + 1;
    object.set(
        "BaseProfileType",
        xml::scalar("BaseProfileType", "App::PropertyInteger", "Integer", filter),
    );
    if let Some(value) = value["allow_multi_profile_faces"].as_bool() {
        boolean(object, "AllowMultiFace", value);
    }
    Ok(())
}

fn specification(value: &Value, object: &mut Object) -> Result<(), CodecError> {
    if value.is_null() {
        enumeration(object, "ThreadType", 0);
        return Ok(());
    }
    let standard = value["standard"]
        .as_str()
        .ok_or_else(|| CodecError::UnsupportedContent("thread standard absent".into()))?;
    let standards = [
        "None",
        "ISO metric",
        "ISO metric fine",
        "UNC",
        "UNF",
        "UNEF",
        "NPT",
        "BSP",
        "BSW",
        "BSF",
        "ISO tyre",
    ];
    let mode = standards
        .iter()
        .position(|item| *item == standard)
        .ok_or_else(|| {
            CodecError::UnsupportedContent(format!(
                "thread standard `{standard}` has no FreeCAD enumeration"
            ))
        })?;
    enumeration(object, "ThreadType", mode);
    boolean(object, "Threaded", tag(value) == "threaded");
    for (key, target) in [
        ("designation", "ThreadSize"),
        ("class", "ThreadClass"),
        ("fit", "ThreadFit"),
    ] {
        if let Some(label) = value[key].as_str() {
            let content=format!("<Integer value=\"0\" CustomEnum=\"true\"/><CustomEnumList count=\"1\">{}</CustomEnumList>",xml::element("Enum",&[("value",label.into())]));
            object.set(
                target,
                xml::property(target, "App::PropertyEnumeration", &content),
            );
        }
    }
    for (key, target) in [("modeled", "ModelThread"), ("cosmetic", "CosmeticThread")] {
        boolean(object, target, value[key].as_bool().unwrap_or(false));
    }
    enumeration(
        object,
        "ThreadDirection",
        usize::from(value["hand"] == "left"),
    );
    for (key, target) in [
        ("pitch", "ThreadPitch"),
        ("major_diameter", "ThreadDiameter"),
    ] {
        if !value[key].is_null() {
            length(object, target, number(&value[key], "thread dimension")?);
        }
    }
    let depth = &value["depth"];
    enumeration(
        object,
        "ThreadDepthType",
        match tag(depth) {
            "hole_depth" => 0,
            "blind" => 1,
            "tapped_standard" => 2,
            _ => return unsupported("thread depth has no FreeCAD rule"),
        },
    );
    if tag(depth) == "blind" {
        length(
            object,
            "ThreadDepth",
            number(&depth["depth"], "thread depth")?,
        );
    }
    boolean(
        object,
        "UseCustomThreadClearance",
        !value["clearance"].is_null(),
    );
    if !value["clearance"].is_null() {
        length(
            object,
            "CustomThreadClearance",
            number(&value["clearance"], "thread clearance")?,
        );
    }
    Ok(())
}
