// SPDX-License-Identifier: Apache-2.0
//! Text and leader objects regenerated from current semantic annotations.

use super::{
    anonymous, anonymous_with_children, framed_object_record, integer, object_attributes_payload,
    utf16, wire_index, CadIr, CodecError, LossNote, Map, RhinoLossCode, Value,
};
use crate::writer::WriteError;
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
const LEGACY_TEXT_CLASS: [u8; 16] = [
    0x41, 0x55, 0xf7, 0x46, 0x6b, 0xf4, 0xbe, 0x48, 0xaa, 0x7e, 0xb3, 0x53, 0xbb, 0xe0, 0x68, 0xa7,
];
const V2_LINEAR_CLASS: [u8; 16] = [
    0x0d, 0xb2, 0xe6, 0x5d, 0x6b, 0x48, 0xd4, 0x11, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const V2_RADIAL_CLASS: [u8; 16] = [
    0x0e, 0xb2, 0xe6, 0x5d, 0x6b, 0x48, 0xd4, 0x11, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const V2_ANGULAR_CLASS: [u8; 16] = [
    0x0f, 0xb2, 0xe6, 0x5d, 0x6b, 0x48, 0xd4, 0x11, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
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

fn legacy_text_payload(
    annotation: &SemanticAnnotation,
    position: [f64; 3],
    fields: &Map<String, Value>,
    leader: bool,
) -> Option<Vec<u8>> {
    let justification = fields.get("legacy_justification")?.as_i64()? as i32;
    let style_index = fields
        .get("legacy_style_index")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let text_display_mode = fields
        .get("legacy_text_display_mode")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let text_height = fields
        .get("legacy_text_height")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())?;
    let user_positioned = fields
        .get("legacy_user_positioned_text")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let user_text = fields
        .get("legacy_user_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let rich_text = annotation.text.join("\n");
    let kind: i32 = if leader { 6 } else { 7 };

    let mut annotation_fields = kind.to_le_bytes().to_vec();
    annotation_fields.extend(text_display_mode.to_le_bytes());
    annotation_fields.extend(plane(position, Some(fields)));

    let points: Vec<[f64; 2]> = if leader {
        fields
            .get("leader_points")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let a = p.as_array()?;
                if a.len() >= 2 {
                    Some([a[0].as_f64()?, a[1].as_f64()?])
                } else {
                    None
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    annotation_fields.extend(i32::try_from(points.len()).ok()?.to_le_bytes());
    for point in &points {
        annotation_fields.extend(point[0].to_le_bytes());
        annotation_fields.extend(point[1].to_le_bytes());
    }
    annotation_fields.extend(utf16(&rich_text).ok()?);
    annotation_fields.extend(i32::from(user_positioned).to_le_bytes());
    annotation_fields.extend(style_index.to_le_bytes());
    annotation_fields.extend(text_height.to_le_bytes());
    annotation_fields.extend(justification.to_le_bytes());
    // minor >= 1: stored_text_scaling (1 byte bool)
    annotation_fields.push(0);
    // minor >= 2: user_text
    annotation_fields.extend(utf16(user_text).ok()?);
    // minor >= 3: text_style_index, dimension_style_index (for kind==7)
    if kind == 7 {
        annotation_fields.extend(style_index.to_le_bytes());
        annotation_fields.extend((-1i32).to_le_bytes());
    } else {
        annotation_fields.extend((-1i32).to_le_bytes());
        annotation_fields.extend(style_index.to_le_bytes());
    }

    let inner = anonymous(1, 3, &annotation_fields).ok()?;
    let outer = anonymous(1, 0, &inner).ok()?;
    Some(outer)
}

fn text(
    annotation: &SemanticAnnotation,
    position: [f64; 3],
    fields: Option<&Map<String, Value>>,
    leader: bool,
) -> Result<Vec<u8>, WriteError> {
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
        let points = match fields.and_then(|fields| fields.get("leader_points")) {
            Some(value) => value
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(|| WriteError::Omit("Rhino leader points must be a list".into()))?,
            None => &[],
        };
        outer.extend(wire_index(points.len())?.to_le_bytes());
        for point in points {
            let pair = point
                .as_array()
                .filter(|pair| pair.len() == 2)
                .ok_or_else(|| {
                    WriteError::Omit("Rhino leader point needs two coordinates".into())
                })?;
            for value in pair {
                outer.extend(
                    value
                        .as_f64()
                        .filter(|value| value.is_finite())
                        .ok_or_else(|| WriteError::Omit("Rhino leader point is nonnumeric".into()))?
                        .to_le_bytes(),
                );
            }
        }
    }
    Ok(anonymous_with_children(
        1,
        i32::from(leader),
        &outer,
        std::slice::from_ref(&(0..base_end)),
    )?)
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

fn v1_annotation_fields(
    record: &Map<String, Value>,
    annotation_kind: &SemanticAnnotationKind,
) -> Option<Map<String, Value>> {
    let payload = record.get("payload")?.as_object()?;
    if payload.get("kind")?.as_str()? != "Annotation" {
        return None;
    }
    let payload = payload.get("fields")?.as_object()?;
    let kind = payload.get("kind")?.as_str()?;
    if !matches!(
        (kind, annotation_kind),
        ("TextBlock", SemanticAnnotationKind::Text)
            | (
                "Leader",
                SemanticAnnotationKind::Text | SemanticAnnotationKind::Leader
            )
            | (
                "LinearDimension" | "AngularDimension" | "RadialDimension",
                SemanticAnnotationKind::Text | SemanticAnnotationKind::Dimension
            )
    ) {
        return None;
    }
    let fields = payload.get("fields")?.as_object()?;
    let plane = fields.get("plane")?.as_object()?;
    let axis = |key: &str| {
        let values = plane.get(key)?.as_array()?;
        if values.len() != 3 {
            return None;
        }
        cadmpeg_ir::math::Vector3::new(
            values[0].as_f64()?,
            values[1].as_f64()?,
            values[2].as_f64()?,
        )
        .unit()
    };
    let x = axis("x_axis")?;
    let y = axis("y_axis")?;
    let z = x.cross(y).unit()?;
    let mut output = Map::new();
    for (key, vector) in [
        ("plane_x_axis", x),
        ("plane_y_axis", y),
        ("plane_z_axis", z),
    ] {
        output.insert(
            key.into(),
            serde_json::json!([vector.x, vector.y, vector.z]),
        );
    }
    if kind == "Leader" && *annotation_kind == SemanticAnnotationKind::Leader {
        let scale = record.get("document_scale")?.as_f64()?;
        let points = fields.get("points")?.as_array()?;
        if points.len() < 2 || !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let points = points
            .iter()
            .map(|point| {
                let point = point.as_array()?;
                if point.len() != 3 {
                    return None;
                }
                let x = point[0].as_f64()? * scale;
                let y = point[1].as_f64()? * scale;
                if !x.is_finite() || !y.is_finite() {
                    return None;
                }
                Some(serde_json::json!([x, y]))
            })
            .collect::<Option<Vec<_>>>()?;
        output.insert("leader_points".into(), Value::Array(points));
    }
    Some(output)
}

type V1DimensionPayload = ([u8; 16], Vec<u8>, f64);

fn v1_dimension(
    annotation: &SemanticAnnotation,
    position: [f64; 3],
    record: &Map<String, Value>,
    plane_fields: &Map<String, Value>,
) -> Result<Option<V1DimensionPayload>, CodecError> {
    let Some(fields) = record.get("payload").and_then(|v| v.get("fields")) else {
        return Ok(None);
    };
    let Some(kind) = fields.get("kind").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(fields) = fields.get("fields").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(scale) = record
        .get("document_scale")
        .and_then(Value::as_f64)
        .filter(|scale| scale.is_finite() && *scale > 0.0)
    else {
        return Ok(None);
    };
    let Some(points) = fields.get("points").and_then(Value::as_array) else {
        return Ok(None);
    };
    let Some(original_kind) = fields
        .get("annotation_type")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
    else {
        return Ok(None);
    };
    let Some(user_positioned_text) = fields.get("user_positioned_text").and_then(Value::as_i64)
    else {
        return Ok(None);
    };
    let (class, kind, minimum) = match kind {
        "LinearDimension" => (
            V2_LINEAR_CLASS,
            if matches!(original_kind, 2 | 3 | 5 | 10) {
                1i32
            } else {
                2i32
            },
            4,
        ),
        "AngularDimension" => (V2_ANGULAR_CLASS, 3i32, 2),
        "RadialDimension" if original_kind == 7 => (V2_RADIAL_CLASS, 4i32, 3),
        "RadialDimension" if original_kind == 8 => (V2_RADIAL_CLASS, 5i32, 3),
        _ => return Ok(None),
    };
    if points.len() < minimum
        || position
            .iter()
            .any(|value| value.abs() > crate::dimensions::V2_REALLY_BIG_NUMBER)
    {
        return Ok(None);
    }
    let mut output = vec![0x10];
    output.extend(kind.to_le_bytes());
    output.extend(plane(position, Some(plane_fields)));
    output.extend(wire_index(points.len())?.to_le_bytes());
    let mut first_points = [[0.0; 2]; 4];
    for (index, point) in points.iter().enumerate() {
        let Some(point) = point.as_array().filter(|point| point.len() == 3) else {
            return Ok(None);
        };
        for (axis, coordinate) in point[..2].iter().enumerate() {
            let Some(value) = coordinate
                .as_f64()
                .map(|value| value * scale)
                .filter(|value| {
                    value.is_finite() && value.abs() <= crate::dimensions::V2_REALLY_BIG_NUMBER
                })
            else {
                return Ok(None);
            };
            output.extend(value.to_le_bytes());
            if index < first_points.len() {
                first_points[index][axis] = value;
            }
        }
    }
    let user_was_empty = fields
        .get("user_text")
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
        .is_some_and(str::is_empty);
    let (user, default) = if user_was_empty {
        (String::new(), annotation.text.join("\n"))
    } else {
        (
            annotation.text.first().cloned().unwrap_or_default(),
            annotation
                .text
                .iter()
                .skip(1)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };
    output.extend(utf16(&user)?);
    output.extend(utf16(&default)?);
    output.extend(i32::from(user_positioned_text != 0).to_le_bytes());
    let measurement = if class == V2_ANGULAR_CLASS {
        let Some(angle) = fields.get("angle").and_then(Value::as_f64).filter(|value| {
            value.is_finite() && *value > 0.0 && *value <= crate::dimensions::V2_REALLY_BIG_NUMBER
        }) else {
            return Ok(None);
        };
        let Some(radius) = fields
            .get("radius")
            .and_then(Value::as_f64)
            .map(|value| value * scale)
            .filter(|value| {
                value.is_finite()
                    && *value > 0.0
                    && *value <= crate::dimensions::V2_REALLY_BIG_NUMBER
            })
        else {
            return Ok(None);
        };
        output.extend(angle.to_le_bytes());
        output.extend(radius.to_le_bytes());
        angle
    } else {
        let (a, b) = if class == V2_LINEAR_CLASS {
            (first_points[1], first_points[3])
        } else {
            (first_points[1], first_points[0])
        };
        (a[0] - b[0]).hypot(a[1] - b[1]) * if kind == 4 { 2.0 } else { 1.0 }
    };
    if !measurement.is_finite() {
        return Ok(None);
    }
    Ok(Some((class, output, measurement)))
}

/// Emitted annotation objects with the neutral annotations they carry.
pub(in crate::writer) struct AnnotationExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) losses: Vec<LossNote>,
    /// Neutral semantic-annotation ids actually emitted as annotation objects.
    pub(in crate::writer) written: std::collections::BTreeSet<String>,
}

pub(in crate::writer) fn records(
    ir: &CadIr,
    materials: &super::materials::MaterialPlan,
) -> Result<AnnotationExport, WriteError> {
    let mut output = Vec::new();
    let mut losses = Vec::new();
    let mut written = std::collections::BTreeSet::new();
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
        let legacy = super::records(ir, "legacy_v1_records")
            .iter()
            .find(|record| record.id() == annotation.native_ref)
            .map(cadmpeg_ir::NativeRecord::fields)
            .transpose()?;
        let legacy_fields = legacy
            .as_ref()
            .and_then(|record| v1_annotation_fields(record, &annotation.kind));
        if legacy.is_some()
            && legacy_fields.is_none()
            && matches!(
                annotation.kind,
                SemanticAnnotationKind::Text | SemanticAnnotationKind::Leader
            )
        {
            losses.push(RhinoLossCode::WriterAnnotationOmitted.note(format!(
                "{}: V1 annotation record {} has no writable plane or leader points",
                annotation.id, annotation.native_ref
            )));
            continue;
        }
        if let Some(record) = legacy
            .as_ref()
            .filter(|_| annotation.kind == SemanticAnnotationKind::Text)
        {
            let kind = record
                .get("payload")
                .and_then(|value| value.get("fields"))
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str);
            let reduced = match kind {
                Some("Leader") => {
                    "leader points are omitted when the annotation is written as text"
                }
                Some("LinearDimension" | "AngularDimension" | "RadialDimension") => {
                    "dimension definition is omitted when the annotation is written as text"
                }
                _ => "font and text height are omitted",
            };
            losses.push(RhinoLossCode::WriterV1AnnotationReduced.note(format!(
                "{}: V1 annotation record {} {reduced}",
                annotation.id, annotation.native_ref
            )));
        }
        let fields = native.as_ref().or(legacy_fields.as_ref());
        let is_dot = annotation.runtime_type == "rhino::text_dots";
        let is_arrow = annotation.runtime_type == "rhino::annotation_arrows";
        let (class, payload) = match annotation.kind {
            SemanticAnnotationKind::Dimension if legacy.is_some() => {
                let value = legacy
                    .as_ref()
                    .zip(legacy_fields.as_ref())
                    .map(|(record, plane)| v1_dimension(annotation, position.get(), record, plane))
                    .transpose()?
                    .flatten();
                let Some((class, payload, measurement)) = value else {
                    losses.push(RhinoLossCode::WriterAnnotationOmitted.note(format!(
                        "{}: V1 dimension record {} has no writable definition",
                        annotation.id, annotation.native_ref
                    )));
                    continue;
                };
                losses.push(RhinoLossCode::WriterV1AnnotationReduced.note(format!(
                    "{}: V1 dimension record {} legacy-specific fields are omitted",
                    annotation.id, annotation.native_ref
                )));
                if annotation
                    .value
                    .is_some_and(|value| value.get() != measurement)
                {
                    losses.push(RhinoLossCode::WriterDimensionValueReduced.note(format!(
                        "{}: the dimension value is derived from its definition points instead of written independently",
                        annotation.id
                    )));
                }
                (class, payload)
            }
            SemanticAnnotationKind::Text if is_dot => {
                (DOT_CLASS, dot(annotation, position.get(), fields)?)
            }
            SemanticAnnotationKind::Text => {
                if let Some(native_fields) = native.as_ref() {
                    if native_fields
                        .get("legacy_justification")
                        .is_some_and(|v| !v.is_null())
                    {
                        if let Some(payload) =
                            legacy_text_payload(annotation, position.get(), native_fields, false)
                        {
                            (LEGACY_TEXT_CLASS, payload)
                        } else {
                            (TEXT_CLASS, text(annotation, position.get(), fields, false)?)
                        }
                    } else {
                        (TEXT_CLASS, text(annotation, position.get(), fields, false)?)
                    }
                } else {
                    (TEXT_CLASS, text(annotation, position.get(), fields, false)?)
                }
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
            SemanticAnnotationKind::Leader => {
                match text(annotation, position.get(), fields, true) {
                    Ok(payload) => (LEADER_CLASS, payload),
                    Err(WriteError::Omit(reason)) => {
                        losses.push(RhinoLossCode::WriterAnnotationOmitted.note(format!(
                            "{}: native annotation {} omitted: {reason}",
                            annotation.id, annotation.native_ref
                        )));
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
            _ => {
                losses.push(RhinoLossCode::WriterAnnotationOmitted.note(format!(
                    "{}: annotation kind {:?} has no supported Rhino object encoding",
                    annotation.id, annotation.kind
                )));
                continue;
            }
        };
        if (annotation.kind == SemanticAnnotationKind::Dimension && annotation.text.len() > 2)
            || (annotation.kind != SemanticAnnotationKind::Dimension && annotation.text.len() != 1)
        {
            losses.push(RhinoLossCode::WriterAnnotationTextJoined.note(format!(
                "{}: multi-line text is joined into one native string",
                annotation.id
            )));
        }
        if fields
            .and_then(|fields| fields.get("dimstyle_uuid"))
            .is_some_and(|value| !value.is_null())
        {
            losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                "{}: dimension-style association is omitted",
                annotation.id
            )));
        }
        let mut attributes =
            object_attributes_payload(ir, annotation.id.as_str(), None, None, None, materials)?;
        if native
            .as_ref()
            .and_then(|fields| fields.get("source_uuid"))
            .and_then(Value::as_str)
            .and_then(crate::presentation::parse_uuid_text)
            .is_some_and(crate::wire::Uuid::is_nil)
        {
            attributes[1..17].fill(0);
        } else if let Some(uuid) = annotation
            .native_ref
            .as_str()
            .split_once('#')
            .and_then(|(_, key)| crate::presentation::parse_uuid_text(key))
        {
            attributes[1..17].copy_from_slice(&uuid.to_wire());
        }
        output.push(framed_object_record(
            if is_dot { 0x2000 } else { 0x200 },
            class,
            &payload,
            (!is_dot && !is_arrow && annotation.kind != SemanticAnnotationKind::Dimension)
                .then_some(&[][..]),
            Some(attributes),
        )?);
        written.insert(annotation.id.to_string());
    }
    Ok(AnnotationExport {
        records: output,
        losses,
        written,
    })
}

#[cfg(test)]
mod tests;
