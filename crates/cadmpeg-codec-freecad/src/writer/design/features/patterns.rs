// SPDX-License-Identifier: Apache-2.0
//! Pattern transforms and their auxiliary construction objects.

use super::super::super::xml::{self, Object};
use super::construction::axis_placement;
use super::properties::{
    angle, enumeration, length, link_names, number, selection, tag, unsupported, vector,
};
use cadmpeg_core::CodecError;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn pattern(
    value: &Value,
    object: &mut Object,
    names: &HashMap<String, String>,
    extras: &mut Vec<Object>,
    entries: &mut Vec<(String, Vec<u8>)>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let count = value["count"].as_u64().unwrap_or(2);
    match tag(value) {
        "linear" | "linear_offsets" => {
            object.kind = "PartDesign::LinearPattern".into();
            vector(object, "Direction", &value["direction"])?;
            if tag(value) == "linear" {
                let distance = &value["distance"];
                let span = tag(distance) == "span";
                enumeration(object, "Mode", if span { 0 } else { 1 });
                object.set(
                    "Occurrences",
                    xml::scalar(
                        "Occurrences",
                        "App::PropertyIntegerConstraint",
                        "Integer",
                        count,
                    ),
                );
                length(
                    object,
                    if span { "Length" } else { "Offset" },
                    number(&distance["distance"], "linear pattern distance")?,
                );
                if !value["second"].is_null() {
                    let second = &value["second"];
                    vector(object, "Direction2", &second["direction"])?;
                    let count = second["count"].as_u64().ok_or_else(|| {
                        CodecError::UnsupportedContent("second pattern count absent".into())
                    })?;
                    let distance = &second["distance"];
                    let span = tag(distance) == "span";
                    enumeration(object, "Mode2", if span { 0 } else { 1 });
                    object.set(
                        "Occurrences2",
                        xml::scalar(
                            "Occurrences2",
                            "App::PropertyIntegerConstraint",
                            "Integer",
                            count,
                        ),
                    );
                    length(
                        object,
                        if span { "Length2" } else { "Offset2" },
                        number(&distance["distance"], "second pattern distance")?,
                    );
                }
            } else {
                pattern_offsets(value, "offsets", object, false, entries)?;
            }
        }
        "circular" | "circular_angles" => {
            object.kind = "PartDesign::PolarPattern".into();
            let axis = auxiliary(
                object,
                "Axis",
                "App::Line",
                &value["axis_origin"],
                &value["axis_dir"],
                names,
                extras,
            )?;
            object.set(
                "Axis",
                xml::scalar("Axis", "App::PropertyLink", "Link", &axis),
            );
            if tag(value) == "circular" {
                let angle_form = &value["angle"];
                let span = tag(angle_form) == "span";
                let periodic = span && angle_form["distribution"] == "periodic";
                enumeration(object, "Mode", if span && !periodic { 0 } else { 1 });
                object.set(
                    "Occurrences",
                    xml::scalar(
                        "Occurrences",
                        "App::PropertyIntegerConstraint",
                        "Integer",
                        count,
                    ),
                );
                let authored = number(&angle_form["angle"], "polar pattern angle")?;
                let encoded = if periodic {
                    if count == 0 {
                        return unsupported("a periodic pattern has no instances");
                    }
                    let count = u32::try_from(count).map_err(|_| {
                        CodecError::InvalidInput("(O-capacity) pattern count exceeds u32".into())
                    })?;
                    losses.push(crate::loss::FreecadLossCode::ExportFeatureNotTransferred.note(
                        format!("pattern {} periodic total-span control is encoded as an adjacent-angle control",object.name),
                    ));
                    authored / f64::from(count)
                } else {
                    authored
                };
                angle(
                    object,
                    if span && !periodic { "Angle" } else { "Offset" },
                    encoded,
                );
            } else {
                pattern_offsets(value, "angles", object, true, entries)?;
            }
        }
        "mirror" => {
            object.kind = "PartDesign::Mirrored".into();
            let plane = auxiliary(
                object,
                "Plane",
                "App::Plane",
                &value["plane_origin"],
                &value["plane_normal"],
                names,
                extras,
            )?;
            object.set(
                "MirrorPlane",
                xml::scalar("MirrorPlane", "App::PropertyLink", "Link", &plane),
            );
        }
        "mirror_reference" => {
            object.kind = "PartDesign::Mirrored".into();
            selection(object, "MirrorPlane", &value["plane"], names)?;
        }
        "scale" => {
            if tag(&value["center"]) != "first_seed_centroid"
                && value["center"] != "first_seed_centroid"
            {
                return unsupported("FreeCAD scaled patterns use the first seed centroid");
            }
            object.kind = "PartDesign::Scaled".into();
            object.set(
                "Occurrences",
                xml::scalar("Occurrences", "App::PropertyInteger", "Integer", count),
            );
            length(
                object,
                "Factor",
                number(&value["final_factor"], "scale factor")?,
            );
        }
        "composite" => {
            object.kind = "PartDesign::MultiTransform".into();
            let stages = value["stages"].as_array().ok_or_else(|| {
                CodecError::UnsupportedContent("composite pattern stages absent".into())
            })?;
            let mut targets = Vec::new();
            for (index, stage) in stages.iter().enumerate() {
                let name = unique_name(&format!("{}_Transform{index}", object.name), names, extras);
                let mut stage_object = Object::new(name.clone(), "PartDesign::Feature", &name);
                pattern(
                    &stage["pattern"],
                    &mut stage_object,
                    names,
                    extras,
                    entries,
                    losses,
                )?;
                targets.push(name);
                extras.push(stage_object);
            }
            link_names(object, "Transformations", &targets);
        }
        _ => return unsupported(format!("pattern `{}` has no FreeCAD transform", tag(value))),
    }
    Ok(())
}
pub(super) fn pattern_offsets(
    value: &Value,
    key: &str,
    object: &mut Object,
    angular: bool,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), CodecError> {
    let values = value[key]
        .as_array()
        .ok_or_else(|| CodecError::UnsupportedContent("pattern offsets absent".into()))?;
    if values.is_empty() {
        return unsupported("pattern offset list empty");
    }
    enumeration(object, "Mode", 1);
    object.set(
        "Occurrences",
        xml::scalar(
            "Occurrences",
            "App::PropertyIntegerConstraint",
            "Integer",
            values.len(),
        ),
    );
    let offsets = values
        .iter()
        .map(|value| number(value, "pattern offset"))
        .collect::<Result<Vec<_>, _>>()?;
    let count = u32::try_from(
        offsets
            .len()
            .checked_sub(1)
            .ok_or_else(|| CodecError::malformed("pattern has no offsets"))?,
    )
    .map_err(|_| {
        CodecError::InvalidInput("(O-capacity) pattern spacing count exceeds u32".into())
    })?;
    let mut bytes = count.to_le_bytes().to_vec();
    for pair in offsets.windows(2) {
        let delta = pair[1] - pair[0];
        let value = if angular { delta.to_degrees() } else { delta };
        if !value.is_finite() {
            return unsupported("pattern spacing overflows finite length");
        }
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let file = format!("{}_Spacings.bin", object.name);
    object.set(
        "Spacings",
        xml::property(
            "Spacings",
            "App::PropertyFloatList",
            &xml::element("FloatList", &[("file", file.clone())]),
        ),
    );
    entries.push((file, bytes));
    length(object, "Offset", 0.0);
    Ok(())
}
pub(super) fn unique_name(
    base: &str,
    names: &HashMap<String, String>,
    extras: &[Object],
) -> String {
    let mut candidate = base.to_owned();
    let mut suffix = 0usize;
    while names.values().any(|name| name == &candidate)
        || extras.iter().any(|object| object.name == candidate)
    {
        suffix += 1;
        candidate = format!("{base}_{suffix}");
    }
    candidate
}
pub(super) fn auxiliary(
    owner: &Object,
    suffix: &str,
    kind: &str,
    origin: &Value,
    direction: &Value,
    names: &HashMap<String, String>,
    extras: &mut Vec<Object>,
) -> Result<String, CodecError> {
    let name = unique_name(&format!("{}_{suffix}", owner.name), names, extras);
    let mut object = Object::new(name.clone(), kind, &name);
    axis_placement(&mut object, origin, direction)?;
    extras.push(object);
    Ok(name)
}
