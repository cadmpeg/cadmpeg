// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use std::collections::{BTreeMap, HashMap};

use crate::native::{ObjectRecord, ProductNodeRecord, PropertyRecord};
use cadmpeg_ir::codec::CodecError;

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<ProductNodeRecord>, CodecError> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    let mut output = Vec::new();
    for object in objects {
        let Some(kind) = product_kind(&object.type_name) else {
            continue;
        };
        let owned = by_owner
            .get(object.id.as_str())
            .cloned()
            .unwrap_or_default();
        let group = owned.iter().find(|property| property.name == "Group");
        let linked = owned
            .iter()
            .find(|property| property.name == "LinkedObject");
        let prototype_link = linked.and_then(|property| property.links.first());
        let placement = owned
            .iter()
            .find(|property| property.name == "LinkPlacement")
            .or_else(|| owned.iter().find(|property| property.name == "Placement"));
        output.push(ProductNodeRecord {
            id: format!("fcstd:product:{}", object.name),
            object: object.id.clone(),
            kind: kind.into(),
            members: group
                .into_iter()
                .flat_map(|property| &property.links)
                .filter_map(|link| link.object.clone())
                .collect(),
            prototype: prototype_link.and_then(|link| link.object.clone()),
            external_document: prototype_link.and_then(|link| link.document.clone()),
            local_transform: placement.and_then(|property| placement_matrix(property)),
            placement_property: placement.map(|property| property.id.clone()),
            element_count: scalar(&owned, "ElementCount").and_then(|value| value.parse().ok()),
            link_transform: scalar(&owned, "LinkTransform").and_then(parse_bool),
            element_transforms: parse_placement_list(&owned, entries)?,
            element_scales: parse_vector_list(&owned, entries)?,
        });
    }
    Ok(output)
}

fn parse_placement_list(
    properties: &[&PropertyRecord],
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<[[f64; 4]; 4]>, CodecError> {
    let Some(bytes) = side_bytes(properties, "PlacementList", entries)? else {
        return Ok(Vec::new());
    };
    let (count, width) = list_layout(bytes, 7, "PlacementList")?;
    (0..count)
        .map(|index| {
            let offset = 4 + index * width * 7;
            let values = (0..7)
                .map(|component| read_real(bytes, offset + component * width, width))
                .collect::<Vec<_>>();
            placement_components(&values).ok_or_else(|| {
                CodecError::Malformed("PlacementList contains a zero quaternion".into())
            })
        })
        .collect()
}

fn parse_vector_list(
    properties: &[&PropertyRecord],
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<[f64; 3]>, CodecError> {
    let Some(bytes) = side_bytes(properties, "ScaleList", entries)? else {
        return Ok(Vec::new());
    };
    let (count, width) = list_layout(bytes, 3, "ScaleList")?;
    (0..count)
        .map(|index| {
            let offset = 4 + index * width * 3;
            Ok([
                read_real(bytes, offset, width),
                read_real(bytes, offset + width, width),
                read_real(bytes, offset + 2 * width, width),
            ])
        })
        .collect()
}

fn side_bytes<'a>(
    properties: &[&PropertyRecord],
    name: &str,
    entries: &'a BTreeMap<String, Vec<u8>>,
) -> Result<Option<&'a [u8]>, CodecError> {
    let Some(property) = properties.iter().find(|property| property.name == name) else {
        return Ok(None);
    };
    let Some(entry) = property.side_entries.first() else {
        return Ok(None);
    };
    entries
        .get(entry)
        .map(Vec::as_slice)
        .map(Some)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "{property_id} references missing {entry}",
                property_id = property.id
            ))
        })
}

fn list_layout(bytes: &[u8], components: usize, name: &str) -> Result<(usize, usize), CodecError> {
    if bytes.len() < 4 {
        return Err(CodecError::Malformed(format!("{name} is truncated")));
    }
    let count = u32::from_le_bytes(bytes[..4].try_into().expect("four-byte count")) as usize;
    let double_len = 4_usize.saturating_add(count.saturating_mul(components).saturating_mul(8));
    let float_len = 4_usize.saturating_add(count.saturating_mul(components).saturating_mul(4));
    if bytes.len() == double_len {
        Ok((count, 8))
    } else if bytes.len() == float_len {
        Ok((count, 4))
    } else {
        Err(CodecError::Malformed(format!(
            "{name} count {count} does not match {} bytes",
            bytes.len()
        )))
    }
}

fn read_real(bytes: &[u8], offset: usize, width: usize) -> f64 {
    if width == 8 {
        f64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("bounded f64"))
    } else {
        f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("bounded f32")) as f64
    }
}

fn product_kind(kind: &str) -> Option<&'static str> {
    if kind.contains("LinkGroup") {
        Some("link_group")
    } else if kind.contains("App::Link") {
        Some("occurrence")
    } else if kind.contains("App::Part") {
        Some("part")
    } else if kind.contains("Group") {
        Some("group")
    } else {
        None
    }
}

fn scalar<'a>(properties: &'a [&PropertyRecord], name: &str) -> Option<&'a str> {
    properties
        .iter()
        .find(|property| property.name == name)?
        .values
        .iter()
        .find_map(|value| value.attributes.get("value").map(String::as_str))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

pub(crate) fn placement_matrix(property: &PropertyRecord) -> Option<[[f64; 4]; 4]> {
    let value = property
        .values
        .iter()
        .find(|value| value.tag == "PropertyPlacement")?;
    let number = |name: &str, default: f64| {
        value
            .attributes
            .get(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    placement_components(&[
        number("Px", 0.0),
        number("Py", 0.0),
        number("Pz", 0.0),
        number("Q0", 0.0),
        number("Q1", 0.0),
        number("Q2", 0.0),
        number("Q3", 1.0),
    ])
}

fn placement_components(values: &[f64]) -> Option<[[f64; 4]; 4]> {
    let [px, py, pz, x, y, z, w] = *<&[f64; 7]>::try_from(values).ok()?;
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    if norm <= f64::EPSILON {
        return None;
    }
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);
    Some([
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
            px,
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
            py,
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
            pz,
        ],
        [0.0, 0.0, 0.0, 1.0],
    ])
}
