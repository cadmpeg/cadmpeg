// SPDX-License-Identifier: Apache-2.0
//! Text and leader objects regenerated from current semantic annotations.

use super::*;
use cadmpeg_ir::semantic_annotations::{SemanticAnnotation, SemanticAnnotationKind};

const TEXT_CLASS: [u8; 16] = [
    0x49, 0x63, 0x37, 0x57, 0xa9, 0x62, 0x16, 0x4a, 0xb4, 0x11, 0xa4, 0x6b, 0xcd, 0x54, 0x47, 0x90,
];
const LEADER_CLASS: [u8; 16] = [
    0x94, 0xf5, 0x5b, 0x94, 0xf9, 0x6f, 0x5c, 0x4f, 0xbf, 0xc0, 0xb3, 0xaf, 0x52, 0x8f, 0x29, 0xd2,
];
const DOT_CLASS: [u8; 16] = [
    0x02, 0x83, 0x19, 0x74, 0xf4, 0xcd, 0x95, 0x4f, 0x96, 0x09, 0x6d, 0x68, 0x4f, 0x22, 0xab, 0x37,
];
const ARROW_CLASS: [u8; 16] = [
    0x1a, 0x4e, 0xd9, 0x8b, 0xe1, 0x59, 0xd4, 0x11, 0x80, 0x18, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

fn finite(fields: Option<&Map<String, Value>>, key: &str, default: f64) -> f64 {
    fields
        .and_then(|fields| fields.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

fn flag(fields: Option<&Map<String, Value>>, key: &str, default: bool) -> bool {
    fields
        .and_then(|fields| fields.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn axes(fields: Option<&Map<String, Value>>, key: &str, default: [f64; 3]) -> [f64; 3] {
    fields
        .and_then(|fields| fields.get(key))
        .and_then(Value::as_array)
        .filter(|values| values.len() == 3)
        .and_then(|values| {
            Some([
                values[0].as_f64()?,
                values[1].as_f64()?,
                values[2].as_f64()?,
            ])
        })
        .unwrap_or(default)
}

fn plane(position: [f64; 3], fields: Option<&Map<String, Value>>) -> Vec<u8> {
    let x = axes(fields, "plane_x_axis", [1.0, 0.0, 0.0]);
    let y = axes(fields, "plane_y_axis", [0.0, 1.0, 0.0]);
    let z = axes(fields, "plane_z_axis", [0.0, 0.0, 1.0]);
    let mut output = Vec::new();
    for value in position.into_iter().chain(x).chain(y).chain(z).chain([
        z[0],
        z[1],
        z[2],
        -(z[0] * position[0] + z[1] * position[1] + z[2] * position[2]),
    ]) {
        output.extend(value.to_le_bytes());
    }
    output
}

fn text(
    annotation: &SemanticAnnotation,
    position: [f64; 3],
    fields: Option<&Map<String, Value>>,
    leader: bool,
) -> Result<Vec<u8>, CodecError> {
    let plane = plane(position, fields);
    let mut content = utf16(&annotation.text.join("\n"))?;
    content.extend(&plane);
    content.extend(finite(fields, "text_rectangle_width", 0.0).to_le_bytes());
    content.extend(finite(fields, "text_rotation_radians", 0.0).to_le_bytes());
    for name in ["horizontal_alignment", "vertical_alignment"] {
        content.extend(integer(fields, name, 0).to_le_bytes());
    }
    content.extend(0.0f64.to_le_bytes());
    content.push(u8::from(flag(fields, "wrapped", false)));
    let mut base = anonymous(1, 0, &content)?;
    let text_end = base.len();
    base.extend([0; 16]);
    base.extend(&plane);
    base.extend(integer(fields, "annotation_type", if leader { 4 } else { 3 }).to_le_bytes());
    let override_start = base.len();
    base.extend(anonymous(1, 0, &[0])?);
    let override_end = base.len();
    let direction = fields
        .and_then(|fields| fields.get("horizontal_direction"))
        .and_then(Value::as_array)
        .filter(|values| values.len() == 2);
    for (index, default) in [1.0f64, 0.0].into_iter().enumerate() {
        base.extend(
            direction
                .and_then(|values| values[index].as_f64())
                .unwrap_or(default)
                .to_le_bytes(),
        );
    }
    base.push(u8::from(flag(fields, "allow_text_scaling", true)));
    let mut outer =
        anonymous_with_children(1, 4, &base, &[0..text_end, override_start..override_end])?;
    let base_end = outer.len();
    if leader {
        let points = fields
            .and_then(|fields| fields.get("leader_points"))
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        outer.extend(wire_index(points.len())?.to_le_bytes());
        for point in points {
            let pair = point
                .as_array()
                .filter(|pair| pair.len() == 2)
                .ok_or_else(|| CodecError::malformed("Rhino leader point needs two coordinates"))?;
            for value in pair {
                outer.extend(
                    value
                        .as_f64()
                        .ok_or_else(|| CodecError::malformed("Rhino leader point is nonnumeric"))?
                        .to_le_bytes(),
                );
            }
        }
    }
    anonymous_with_children(1, i32::from(leader), &outer, &[0..base_end])
}

fn dot(
    annotation: &SemanticAnnotation,
    position: [f64; 3],
    fields: Option<&Map<String, Value>>,
) -> Result<Vec<u8>, CodecError> {
    let mut payload = vec![0x11];
    for value in position {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(integer(fields, "height_points", 12).to_le_bytes());
    payload.extend(utf16(annotation.text.first().map_or("", String::as_str))?);
    payload.extend(utf16(
        fields
            .and_then(|fields| fields.get("font_face"))
            .and_then(Value::as_str)
            .unwrap_or(""),
    )?);
    let mut flags = 0i32;
    for (bit, key) in [
        (1, "always_on_top"),
        (2, "transparent"),
        (4, "bold"),
        (8, "italic"),
    ] {
        if flag(fields, key, false) {
            flags |= bit;
        }
    }
    payload.extend(flags.to_le_bytes());
    payload.extend(utf16(annotation.text.get(1).map_or("", String::as_str))?);
    Ok(payload)
}

pub(in crate::writer) fn records(ir: &CadIr) -> Result<(Vec<Vec<u8>>, Vec<LossNote>), CodecError> {
    let mut output = Vec::new();
    let mut losses = Vec::new();
    for annotation in &ir.model.semantic_annotations {
        let Some(position) = annotation.position else {
            losses.push(RhinoLossCode::WriterAnnotationOmitted.note(format!(
                "{}: no annotation placement is held",
                annotation.id
            )));
            continue;
        };
        let native = ["annotations", "text_dots", "annotation_arrows"]
            .iter()
            .find_map(|arena| {
                super::records(ir, arena)
                    .iter()
                    .find(|record| record.id() == annotation.native_ref)
            })
            .map(cadmpeg_ir::NativeRecord::fields)
            .transpose()?;
        let fields = native.as_ref();
        let is_dot = annotation.runtime_type == "rhino::text_dots";
        let is_arrow = annotation.runtime_type == "rhino::annotation_arrows";
        let (class, payload) = match annotation.kind {
            SemanticAnnotationKind::Text if is_dot => {
                (DOT_CLASS, dot(annotation, position.get(), fields)?)
            }
            SemanticAnnotationKind::Text => {
                (TEXT_CLASS, text(annotation, position.get(), fields, false)?)
            }
            SemanticAnnotationKind::Leader if is_arrow => {
                let head = fields
                    .and_then(|fields| fields.get("head"))
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 3)
                    .and_then(|values| {
                        Some([
                            values[0].as_f64()?,
                            values[1].as_f64()?,
                            values[2].as_f64()?,
                        ])
                    });
                let Some(head) = head.filter(|head| head.iter().all(|value| value.is_finite()))
                else {
                    losses.push(
                        RhinoLossCode::WriterAnnotationOmitted
                            .note(format!("{}: arrow endpoint is not held", annotation.id)),
                    );
                    continue;
                };
                if annotation.text.iter().any(|value| !value.is_empty()) {
                    losses.push(RhinoLossCode::WriterAnnotationOmitted.note(format!(
                        "{}: the native arrow format has no text field",
                        annotation.id
                    )));
                    continue;
                }
                let mut payload = vec![0x10];
                for value in position.get().into_iter().chain(head) {
                    payload.extend(value.to_le_bytes());
                }
                (ARROW_CLASS, payload)
            }
            SemanticAnnotationKind::Leader => (
                LEADER_CLASS,
                text(annotation, position.get(), fields, true)?,
            ),
            _ => {
                losses.push(RhinoLossCode::WriterAnnotationOmitted.note(format!(
                    "{}: annotation kind {:?} has no supported Rhino object encoding",
                    annotation.id, annotation.kind
                )));
                continue;
            }
        };
        if fields
            .and_then(|fields| fields.get("dimstyle_uuid"))
            .is_some_and(|value| !value.is_null())
        {
            losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                "{}: dimension-style association is omitted",
                annotation.id
            )));
        }
        let attributes = object_attributes_payload(ir, annotation.id.as_str(), None, None, None)?;
        output.push(framed_object_record(
            if is_dot { 0x2000 } else { 0x200 },
            class,
            &payload,
            (!is_dot && !is_arrow).then_some(&[][..]),
            Some(attributes),
        )?);
    }
    Ok((output, losses))
}

#[cfg(test)]
mod tests;
