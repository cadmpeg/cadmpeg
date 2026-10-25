// SPDX-License-Identifier: Apache-2.0
//! Fixed numeric wire shapes selected by current registered property types.

use super::*;
use cadmpeg_ir::attributes::AttributeNumericList;

pub(super) struct Encoded {
    pub(super) runtime: String,
    pub(super) tag: &'static str,
    pub(super) bytes: Vec<u8>,
    pub(super) inline: Option<String>,
}

pub(super) enum Omission {
    Missing,
    OutOfRange,
}

pub(super) fn encode(
    list: &AttributeNumericList,
    original: Option<&crate::native::PropertyRecord>,
    identity: &str,
    losses: &mut Vec<LossNote>,
) -> Result<Result<Encoded, Omission>, CodecError> {
    let source_kind = original.map(|property| property.type_name.as_str());
    let narrow = matches!(
        (list, source_kind),
        (
            AttributeNumericList::Float(_),
            Some("Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList")
        ) | (
            AttributeNumericList::Vector(_),
            Some("Points::PropertyNormalList" | "Mesh::PropertyNormalList")
        )
    ) || matches!(list, AttributeNumericList::Curvature(_));
    let mut components = Vec::new();
    let (tag, default_runtime, count) = match list {
        AttributeNumericList::Float(Some(values)) if values.iter().all(Option::is_some) => {
            components.extend(values.iter().flatten().map(|value| vec![value.get()]));
            ("FloatList", "App::PropertyFloatList", values.len())
        }
        AttributeNumericList::Vector(Some(values)) if values.iter().all(Option::is_some) => {
            components.extend(values.iter().flatten().map(|value| value.as_raw().to_vec()));
            ("VectorList", "App::PropertyVectorList", values.len())
        }
        AttributeNumericList::Placement(Some(values)) if values.iter().all(Option::is_some) => {
            components.extend(values.iter().flatten().map(|value| {
                value
                    .position
                    .as_raw()
                    .iter()
                    .chain(value.quaternion.as_raw())
                    .copied()
                    .collect::<Vec<_>>()
            }));
            ("PlacementList", "App::PropertyPlacementList", values.len())
        }
        AttributeNumericList::Curvature(Some(values)) if values.iter().all(Option::is_some) => {
            components.extend(values.iter().flatten().map(|value| value.as_raw().to_vec()));
            (
                "CurvatureList",
                "Points::PropertyCurvatureList",
                values.len(),
            )
        }
        AttributeNumericList::Color(Some(values)) if values.iter().all(Option::is_some) => {
            let mut bytes = u32::try_from(values.len())
                .map_err(CodecError::malformed)?
                .to_le_bytes()
                .to_vec();
            for value in values.iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            return Ok(Ok(Encoded {
                runtime: "App::PropertyColorList".into(),
                tag: "ColorList",
                bytes,
                inline: None,
            }));
        }
        _ => return Ok(Err(Omission::Missing)),
    };
    let mut rounded = false;
    let mut bytes = u32::try_from(count)
        .map_err(CodecError::malformed)?
        .to_le_bytes()
        .to_vec();
    let mut inline_rows = String::new();
    for row in components {
        for value in row {
            if narrow {
                let Some(value32) = cadmpeg_core::convert::f32_from_f64(value) else {
                    losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!("numeric attribute {identity} has a value outside its registered float32 range")));
                    return Ok(Err(Omission::OutOfRange));
                };
                rounded |= f64::from(value32) != value;
                bytes.extend_from_slice(&value32.to_le_bytes());
                inline_rows.push_str(&xml::element("F", &[("v", value32.to_string())]));
            } else {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    if rounded {
        losses.push(FreecadLossCode::ExportCoordinateQuantized.note(format!("numeric attribute {identity} values are rounded to their registered float32 representation")));
    }
    let source_compatible = matches!(
        (tag, source_kind),
        (
            "FloatList",
            Some("Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList")
        ) | (
            "VectorList",
            Some("Points::PropertyNormalList" | "Mesh::PropertyNormalList")
        ) | (
            "CurvatureList",
            Some("Points::PropertyCurvatureList" | "Mesh::PropertyCurvatureList")
        )
    );
    let runtime = if source_compatible {
        source_kind.unwrap_or(default_runtime)
    } else {
        default_runtime
    }
    .to_owned();
    let inline = source_compatible
        && tag == "FloatList"
        && original.is_some_and(|property| {
            property
                .values()
                .iter()
                .any(|value| value.parent.is_none() && !value.attributes.contains_key("file"))
        });
    Ok(Ok(Encoded {
        runtime,
        tag,
        bytes,
        inline: inline.then(|| format!("<FloatList count=\"{count}\">{inline_rows}</FloatList>")),
    }))
}
