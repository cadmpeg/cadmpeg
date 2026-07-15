// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use std::collections::{BTreeMap, HashMap};

use crate::native::{JointRecord, ObjectRecord, ProductNodeRecord, PropertyRecord};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::products::{
    Component, ComponentId, ComponentKind, ComponentReference, CopyOnChangePolicy,
    ExternalDocumentReference, ExternalResolution, Occurrence, OccurrenceId,
};

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
            id: crate::native::native_id("product", &object.name),
            object: object.id.clone(),
            kind: kind.into(),
            members: group
                .into_iter()
                .flat_map(|property| &property.links)
                .filter_map(|link| link.object.clone())
                .collect(),
            prototype: prototype_link.and_then(|link| link.object.clone()),
            external_document: prototype_link.and_then(|link| link.document.clone()),
            external_document_attribute: prototype_link
                .and_then(|link| link.document_attribute.clone()),
            local_transform: placement.and_then(|property| placement_matrix(property)),
            placement_property: placement.map(|property| property.id.clone()),
            element_count: scalar(&owned, "ElementCount").and_then(|value| value.parse().ok()),
            link_transform: scalar(&owned, "LinkTransform").and_then(parse_bool),
            element_transforms: parse_placement_list(&owned, entries)?,
            element_scales: parse_vector_list(&owned, entries)?,
            linked_subelements: prototype_link
                .map(|link| link.subelements.clone())
                .unwrap_or_default(),
            claim_child: scalar(&owned, "LinkClaimChild").and_then(parse_bool),
            copy_on_change: owned
                .iter()
                .find(|property| property.name == "LinkCopyOnChange")
                .and_then(|property| enumeration_value(property)),
            copy_on_change_source: linked_object(&owned, "LinkCopyOnChangeSource"),
            copy_on_change_group: linked_object(&owned, "LinkCopyOnChangeGroup"),
            copy_on_change_touched: scalar(&owned, "LinkCopyOnChangeTouched").and_then(parse_bool),
            scale: vector(&owned, "ScaleVector").or_else(|| {
                scalar(&owned, "Scale")
                    .and_then(|value| value.parse().ok())
                    .map(|value| [value; 3])
            }),
            element_visibility: bool_list(&owned, "VisibilityList"),
            element_objects: owned
                .iter()
                .find(|property| property.name == "ElementList")
                .into_iter()
                .flat_map(|property| &property.links)
                .filter_map(|link| link.object.clone())
                .collect(),
        });
    }
    Ok(output)
}

/// Project the lossless native product records into reusable definitions and placed uses.
pub(crate) fn transfer_neutral(
    records: &[ProductNodeRecord],
    joints: &[JointRecord],
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<(Vec<Component>, Vec<Occurrence>), CodecError> {
    let mut component_objects = records
        .iter()
        .filter(|record| record.kind != "occurrence")
        .map(|record| record.object.clone())
        .collect::<Vec<_>>();
    let occurrence_objects = records
        .iter()
        .filter(|record| record.kind == "occurrence")
        .map(|record| record.object.as_str())
        .collect::<std::collections::HashSet<_>>();
    for record in records {
        component_objects.extend(
            record
                .members
                .iter()
                .filter(|member| !occurrence_objects.contains(member.as_str()))
                .cloned(),
        );
        if record.external_document.is_none() {
            component_objects.extend(record.prototype.iter().cloned());
        }
        component_objects.extend(record.copy_on_change_source.iter().cloned());
        component_objects.extend(record.copy_on_change_group.iter().cloned());
        component_objects.extend(record.element_objects.iter().cloned());
    }
    component_objects.extend(
        joints
            .iter()
            .flat_map(|joint| &joint.references)
            .filter(|reference| reference.document.is_none())
            .filter_map(|reference| reference.object.clone())
            .filter(|object| !object.is_empty() && !occurrence_objects.contains(object.as_str())),
    );
    component_objects.sort();
    component_objects.dedup();

    let placements_by_object = properties
        .iter()
        .filter(|property| matches!(property.name.as_str(), "LinkPlacement" | "Placement"))
        .filter_map(|property| {
            placement_matrix(property).map(|placement| (property.owner.as_str(), placement))
        })
        .collect::<HashMap<_, _>>();

    let component_id =
        |object: &str| ComponentId(crate::native::model_id("component", object, "definition"));
    let occurrence_records = records
        .iter()
        .filter(|record| record.kind == "occurrence")
        .map(|record| (record.object.as_str(), record))
        .collect::<HashMap<_, _>>();
    let mut parent_by_object = HashMap::<&str, &str>::new();
    for record in records.iter().filter(|record| record.kind != "occurrence") {
        for member in &record.members {
            if parent_by_object.insert(member, &record.object).is_some() {
                return Err(CodecError::Malformed(format!(
                    "product object {member} has multiple direct containers"
                )));
            }
        }
    }

    let mut occurrences = Vec::new();
    for record in occurrence_records.values() {
        let count = occurrence_count(record)?;
        let parent = parent_by_object
            .get(record.object.as_str())
            .map(|object| component_id(object));
        for index in 0..count {
            let element = count > 1;
            let element_transform = record.element_transforms.get(index).copied();
            let local_transform = multiply(
                record.local_transform.unwrap_or_else(identity),
                element_transform.unwrap_or_else(identity),
            );
            let prototype_transform = linked_prototype_transform(
                record,
                records,
                &placements_by_object,
                &mut Vec::new(),
            )?;
            let element_scale = record
                .element_scales
                .get(index)
                .copied()
                .unwrap_or([1.0; 3]);
            let base_scale = record.scale.unwrap_or([1.0; 3]);
            let scale = std::array::from_fn(|axis| base_scale[axis] * element_scale[axis]);
            let resolved_transform = multiply(
                resolve_container_transform(
                    parent_by_object.get(record.object.as_str()).copied(),
                    records,
                    &parent_by_object,
                    local_transform,
                    &mut Vec::new(),
                )?,
                prototype_transform,
            );
            occurrences.push(Occurrence {
                id: OccurrenceId(crate::native::model_id(
                    "occurrence",
                    &record.object,
                    if element {
                        index.to_string()
                    } else {
                        "instance".into()
                    },
                )),
                prototype: if let Some(document) = &record.external_document {
                    ComponentReference::External {
                        document: external_document_reference(
                            document,
                            record.external_document_attribute.as_deref(),
                        ),
                        object: record.prototype.clone(),
                    }
                } else if let Some(prototype) = &record.prototype {
                    ComponentReference::Local {
                        component: component_id(prototype),
                    }
                } else {
                    ComponentReference::Unresolved
                },
                parent: parent.clone(),
                array_index: element.then_some(index as u32),
                local_transform,
                prototype_transform,
                resolved_transform,
                scale,
                linked_subelements: record.linked_subelements.clone(),
                visible: record.element_visibility.get(index).copied(),
                element_component: record
                    .element_objects
                    .get(index)
                    .map(|object| component_id(object)),
                claim_child: record.claim_child,
                copy_on_change: record.copy_on_change.as_deref().map(copy_on_change_policy),
                copy_on_change_source: record.copy_on_change_source.as_deref().map(&component_id),
                copy_on_change_group: record.copy_on_change_group.as_deref().map(&component_id),
                copy_on_change_touched: record.copy_on_change_touched,
                link_transform: record.link_transform,
                native_ref: Some(record.object.clone()),
            });
        }
    }

    let occurrence_ids = occurrences.iter().fold(
        HashMap::<&str, Vec<OccurrenceId>>::new(),
        |mut map, item| {
            if let Some(native) = item.native_ref.as_deref() {
                map.entry(native).or_default().push(item.id.clone());
            }
            map
        },
    );
    let record_by_object = records
        .iter()
        .map(|record| (record.object.as_str(), record))
        .collect::<HashMap<_, _>>();
    let object_by_id = objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<HashMap<_, _>>();
    let properties_by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(property.owner.as_str())
                .or_default()
                .push(property);
            map
        },
    );
    let components = component_objects
        .into_iter()
        .map(|object| {
            let record = record_by_object.get(object.as_str()).copied();
            let kind = match record.map(|record| record.kind.as_str()) {
                Some("part") => ComponentKind::Part,
                Some("group") => ComponentKind::Group,
                Some("link_group") => ComponentKind::LinkGroup,
                _ => ComponentKind::Object,
            };
            let members = record.map_or(&[][..], |record| record.members.as_slice());
            let local_transform = record
                .and_then(|record| record.local_transform)
                .or_else(|| placements_by_object.get(object.as_str()).copied())
                .unwrap_or_else(identity);
            let parent_object = if occurrence_records.contains_key(object.as_str()) {
                None
            } else {
                parent_by_object.get(object.as_str()).copied()
            };
            let source_object = object_by_id.get(object.as_str()).copied();
            let owned = properties_by_owner
                .get(object.as_str())
                .map(Vec::as_slice)
                .unwrap_or_default();
            let bom_properties = ["Label2", "StockCode", "Vendor", "Manufacturer"]
                .into_iter()
                .filter_map(|name| scalar(owned, name).map(|value| (name.into(), value.into())))
                .collect();
            Ok(Component {
                id: component_id(&object),
                kind,
                source_name: source_object.map(|object| object.name.clone()),
                label: scalar(owned, "Label").map(str::to_owned),
                description: scalar(owned, "Description").map(str::to_owned),
                part_number: scalar(owned, "PartNumber").map(str::to_owned),
                bom_properties,
                parent: parent_object.map(&component_id),
                local_transform,
                resolved_transform: resolve_container_transform(
                    parent_object,
                    records,
                    &parent_by_object,
                    local_transform,
                    &mut Vec::new(),
                )?,
                components: members
                    .iter()
                    .filter(|member| !occurrence_records.contains_key(member.as_str()))
                    .map(|member| component_id(member))
                    .collect(),
                occurrences: members
                    .iter()
                    .flat_map(|member| occurrence_ids.get(member.as_str()).into_iter().flatten())
                    .cloned()
                    .collect(),
                native_ref: Some(object),
            })
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    Ok((components, occurrences))
}

fn linked_prototype_transform(
    record: &ProductNodeRecord,
    records: &[ProductNodeRecord],
    placements: &HashMap<&str, [[f64; 4]; 4]>,
    stack: &mut Vec<String>,
) -> Result<[[f64; 4]; 4], CodecError> {
    if stack.len() >= 256 {
        return Err(CodecError::Malformed(
            "nested link transform depth limit exceeded".into(),
        ));
    }
    if record.link_transform != Some(true) || record.external_document.is_some() {
        return Ok(identity());
    }
    let Some(prototype) = record.prototype.as_deref() else {
        return Ok(identity());
    };
    if stack.iter().any(|object| object == &record.object) {
        return Err(CodecError::Malformed(format!(
            "nested link cycle reaches {}",
            record.object
        )));
    }
    stack.push(record.object.clone());
    let target_record = records
        .iter()
        .find(|candidate| candidate.object == prototype);
    let placement = target_record
        .and_then(|target| target.local_transform)
        .or_else(|| placements.get(prototype).copied())
        .unwrap_or_else(identity);
    let nested = target_record.map_or(Ok(identity()), |target| {
        linked_prototype_transform(target, records, placements, stack)
    });
    stack.pop();
    nested.map(|nested| multiply(placement, nested))
}

fn occurrence_count(record: &ProductNodeRecord) -> Result<usize, CodecError> {
    let count = record
        .element_count
        .map(usize::try_from)
        .transpose()
        .map_err(|_| CodecError::Malformed(format!("{} has negative element count", record.id)))?
        .filter(|count| *count > 0)
        .unwrap_or_else(|| {
            [
                record.element_transforms.len(),
                record.element_scales.len(),
                record.element_visibility.len(),
                record.element_objects.len(),
                1,
            ]
            .into_iter()
            .max()
            .expect("nonempty lengths")
        });
    if count > 1_000_000 || u32::try_from(count).is_err() {
        return Err(CodecError::Malformed(format!(
            "{} link-array count limit exceeded",
            record.id
        )));
    }
    if count == 0
        || [
            record.element_transforms.len(),
            record.element_scales.len(),
            record.element_visibility.len(),
            record.element_objects.len(),
        ]
        .into_iter()
        .any(|length| length != 0 && length != count)
    {
        return Err(CodecError::Malformed(format!(
            "{} has inconsistent link-array counts",
            record.id
        )));
    }
    Ok(count)
}

fn copy_on_change_policy(value: &str) -> CopyOnChangePolicy {
    match value.to_ascii_lowercase().as_str() {
        "disabled" | "0" => CopyOnChangePolicy::Disabled,
        "enabled" | "1" => CopyOnChangePolicy::Enabled,
        "owned" | "2" => CopyOnChangePolicy::Owned,
        "tracking" | "3" => CopyOnChangePolicy::Tracking,
        _ => CopyOnChangePolicy::Native(value.to_owned()),
    }
}

pub(crate) fn external_document_reference(
    value: &str,
    attribute: Option<&str>,
) -> ExternalDocumentReference {
    let is_path = attribute.is_some_and(|name| name.eq_ignore_ascii_case("file"));
    ExternalDocumentReference {
        path: is_path.then(|| value.to_owned()),
        document_id: (!is_path).then(|| value.to_owned()),
        resolution: if value.is_empty() {
            ExternalResolution::MissingReference
        } else {
            ExternalResolution::Unresolved
        },
    }
}

fn resolve_container_transform(
    parent: Option<&str>,
    records: &[ProductNodeRecord],
    parent_by_object: &HashMap<&str, &str>,
    local: [[f64; 4]; 4],
    stack: &mut Vec<String>,
) -> Result<[[f64; 4]; 4], CodecError> {
    if stack.len() >= 256 {
        return Err(CodecError::Malformed(
            "product container depth limit exceeded".into(),
        ));
    }
    let Some(parent) = parent else {
        return Ok(local);
    };
    if stack.iter().any(|object| object == parent) {
        return Err(CodecError::Malformed(format!(
            "product container cycle reaches {parent}"
        )));
    }
    stack.push(parent.to_owned());
    let transform = records
        .iter()
        .find(|record| record.object == parent)
        .and_then(|record| record.local_transform)
        .unwrap_or_else(identity);
    let result = resolve_container_transform(
        parent_by_object.get(parent).copied(),
        records,
        parent_by_object,
        multiply(transform, local),
        stack,
    );
    stack.pop();
    result
}

pub(crate) fn identity() -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn multiply(left: [[f64; 4]; 4], right: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| left[row][index] * right[index][column])
                .sum()
        })
    })
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
    if kind.contains("AssemblyObject") {
        Some("part")
    } else if kind.contains("LinkGroup") {
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

fn enumeration_value(property: &PropertyRecord) -> Option<String> {
    let index = property
        .values
        .iter()
        .find(|value| value.tag == "Integer")?
        .attributes
        .get("value")?
        .parse::<usize>()
        .ok()?;
    property
        .values
        .iter()
        .filter(|value| value.tag == "Enum")
        .nth(index)
        .and_then(|value| value.attributes.get("value"))
        .cloned()
        .or_else(|| Some(index.to_string()))
}

fn linked_object(properties: &[&PropertyRecord], name: &str) -> Option<String> {
    properties
        .iter()
        .find(|property| property.name == name)?
        .links
        .first()?
        .object
        .as_ref()
        .filter(|object| !object.is_empty())
        .cloned()
}

fn vector(properties: &[&PropertyRecord], name: &str) -> Option<[f64; 3]> {
    let value = properties
        .iter()
        .find(|property| property.name == name)?
        .values
        .iter()
        .find(|value| value.attributes.contains_key("valueX"))?;
    Some([
        value.attributes.get("valueX")?.parse().ok()?,
        value.attributes.get("valueY")?.parse().ok()?,
        value.attributes.get("valueZ")?.parse().ok()?,
    ])
}

fn bool_list(properties: &[&PropertyRecord], name: &str) -> Vec<bool> {
    properties
        .iter()
        .find(|property| property.name == name)
        .into_iter()
        .flat_map(|property| &property.values)
        .filter(|value| value.tag == "Bool")
        .filter_map(|value| {
            value
                .attributes
                .get("value")
                .and_then(|value| parse_bool(value))
        })
        .collect()
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
