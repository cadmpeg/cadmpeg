// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::brep::ShapePayloadRecord;
use crate::layout::link_array_side_entry_header as link_array;
use crate::native::{JointRecord, ObjectRecord, ProductNodeRecord, PropertyRecord};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    CopyOnChangePolicy, ExternalDocumentReference, ExternalResolution, Occurrence,
    OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::transform::Transform;

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
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
                .map(|link| {
                    link.subelements
                        .iter()
                        .filter(|subelement| !subelement.is_empty())
                        .cloned()
                        .collect()
                })
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
    ctx: &DecodeContext<'_>,
    records: &[ProductNodeRecord],
    joints: &[JointRecord],
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    bodies: &[Body],
) -> Result<(Vec<ProductDefinition>, Vec<Occurrence>), CodecError> {
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

    let definition_id = |object: &str| {
        ProductDefinitionId(crate::native::model_id(
            "product_definition",
            object,
            "definition",
        ))
    };
    let container_occurrence_id =
        |object: &str| OccurrenceId(crate::native::model_id("occurrence", object, "container"));
    let mut parent_by_object = HashMap::<&str, &str>::new();
    for record in records.iter().filter(|record| record.kind != "occurrence") {
        for member in &record.members {
            parent_by_object.entry(member).or_insert(&record.object);
        }
    }

    let mut occurrences = Vec::new();
    for record in records.iter().filter(|record| record.kind == "occurrence") {
        let count = occurrence_count(record)?;
        let parent = parent_by_object
            .get(record.object.as_str())
            .map(|object| container_occurrence_id(object));
        for index in 0..count {
            let element = count > 1;
            let element_transform = record.element_transforms.get(index).copied();
            let local_transform = multiply(
                record.local_transform.unwrap_or_else(identity),
                element_transform.unwrap_or_else(identity),
            );
            let prototype_transform = linked_prototype_transform(
                ctx,
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
                    PrototypeReference::External {
                        document: external_document_reference(
                            document,
                            record.external_document_attribute.as_deref(),
                        ),
                        object: record.prototype.clone(),
                    }
                } else if let Some(prototype) = &record.prototype {
                    PrototypeReference::Local {
                        definition: definition_id(prototype),
                    }
                } else {
                    PrototypeReference::Unresolved
                },
                parent: parent.clone().map_or(OccurrenceParent::Root, |occurrence| {
                    OccurrenceParent::Occurrence { occurrence }
                }),
                ordinal: u32::try_from(index).unwrap_or(u32::MAX),
                transform: Transform {
                    rows: local_transform,
                },
                prototype_transform: Transform {
                    rows: prototype_transform,
                },
                scale,
                name: Some(record.object.clone()),
                linked_subelements: record.linked_subelements.clone(),
                visible: record.element_visibility.get(index).copied(),
                element_component: record
                    .element_objects
                    .get(index)
                    .map(|object| definition_id(object)),
                claim_child: record.claim_child,
                copy_on_change: record.copy_on_change.as_deref().map(copy_on_change_policy),
                copy_on_change_source: record.copy_on_change_source.as_deref().map(&definition_id),
                copy_on_change_group: record.copy_on_change_group.as_deref().map(&definition_id),
                copy_on_change_touched: record.copy_on_change_touched,
                link_transform: record.link_transform,
                native_ref: Some(record.object.clone()),
            });
        }
    }

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
    let property_owner = properties
        .iter()
        .map(|property| (property.id.as_str(), property.owner.as_str()))
        .collect::<HashMap<_, _>>();
    let body_owners = payloads
        .iter()
        .filter_map(|payload| {
            property_owner
                .get(payload.property.as_str())
                .map(|owner| (crate::native::model_id("body", &payload.id, ""), *owner))
        })
        .collect::<Vec<_>>();
    let definitions = component_objects
        .iter()
        .map(|object| {
            let record = record_by_object.get(object.as_str()).copied();
            let kind = match record.map(|record| record.kind.as_str()) {
                Some("part") => ProductDefinitionKind::Part,
                Some("group") => ProductDefinitionKind::Group,
                Some("link_group") => ProductDefinitionKind::LinkGroup,
                _ => ProductDefinitionKind::Object,
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
            ProductDefinition {
                id: definition_id(object),
                kind,
                source_name: source_object.map(|object| object.name.clone()),
                label: scalar(owned, "Label").map(str::to_owned),
                description: scalar(owned, "Description").map(str::to_owned),
                part_number: scalar(owned, "PartNumber").map(str::to_owned),
                bom_properties,
                bodies: bodies
                    .iter()
                    .filter(|body| {
                        body_owners.iter().any(|(prefix, owner)| {
                            *owner == object.as_str() && body.id.0.starts_with(prefix)
                        })
                    })
                    .map(|body| body.id.clone())
                    .collect(),
                native_ref: Some(object.clone()),
            }
        })
        .collect::<Vec<_>>();

    for object in &component_objects {
        let record = record_by_object.get(object.as_str()).copied();
        let local_transform = record
            .and_then(|record| record.local_transform)
            .or_else(|| placements_by_object.get(object.as_str()).copied())
            .unwrap_or_else(identity);
        let parent = parent_by_object.get(object.as_str()).copied();
        occurrences.push(Occurrence {
            id: container_occurrence_id(object),
            prototype: PrototypeReference::Local {
                definition: definition_id(object),
            },
            parent: parent.map_or(OccurrenceParent::Root, |parent| {
                OccurrenceParent::Occurrence {
                    occurrence: container_occurrence_id(parent),
                }
            }),
            ordinal: 0,
            transform: Transform {
                rows: local_transform,
            },
            prototype_transform: Transform::identity(),
            scale: [1.0; 3],
            name: Some(object.clone()),
            linked_subelements: Vec::new(),
            visible: None,
            element_component: None,
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            link_transform: Some(false),
            native_ref: Some(object.clone()),
        });
    }
    let mut next_ordinal = HashMap::<Option<String>, u32>::new();
    for occurrence in &mut occurrences {
        let parent = match &occurrence.parent {
            OccurrenceParent::Root => None,
            OccurrenceParent::Occurrence { occurrence } => Some(occurrence.0.clone()),
        };
        let ordinal = next_ordinal.entry(parent).or_default();
        occurrence.ordinal = *ordinal;
        *ordinal = ordinal.saturating_add(1);
    }
    Ok((definitions, occurrences))
}

fn linked_prototype_transform(
    ctx: &DecodeContext<'_>,
    record: &ProductNodeRecord,
    records: &[ProductNodeRecord],
    placements: &HashMap<&str, [[f64; 4]; 4]>,
    stack: &mut Vec<String>,
) -> Result<[[f64; 4]; 4], CodecError> {
    let _depth = ctx.enter_nested("resolve FCStd nested link transform", None)?;
    if record.link_transform != Some(true) || record.external_document.is_some() {
        return Ok(identity());
    }
    let Some(prototype) = record.prototype.as_deref() else {
        return Ok(identity());
    };
    if stack.iter().any(|object| object == &record.object) {
        return Err(CodecError::malformed(format_args!(
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
        linked_prototype_transform(ctx, target, records, placements, stack)
    });
    stack.pop();
    nested.map(|nested| multiply(placement, nested))
}

fn occurrence_count(record: &ProductNodeRecord) -> Result<usize, CodecError> {
    let declared_count = record
        .element_count
        .map(usize::try_from)
        .transpose()
        .map_err(|_| {
            CodecError::malformed(format_args!("{} has negative element count", record.id))
        })?;
    let count = declared_count.unwrap_or_else(|| {
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
        return Err(CodecError::malformed(format_args!(
            "{} link-array count limit exceeded",
            record.id
        )));
    }
    if [
        record.element_transforms.len(),
        record.element_scales.len(),
        record.element_visibility.len(),
        record.element_objects.len(),
    ]
    .into_iter()
    .any(|length| length != 0 && length != count)
    {
        return Err(CodecError::malformed(format_args!(
            "{} has inconsistent link-array counts",
            record.id
        )));
    }
    Ok(count.max(1))
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
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<[[f64; 4]; 4]>, CodecError> {
    let Some(view) = side_bytes(properties, "PlacementList", entries)? else {
        return Ok(Vec::new());
    };
    let (count, width) = list_layout(view, 7, "PlacementList")?;
    (0..count)
        .map(|index| {
            let offset = link_array::LEN + index * width * 7;
            let values = (0..7)
                .map(|component| read_real(view, offset + component * width, width))
                .collect::<Vec<_>>();
            placement_components(&values).ok_or_else(|| {
                CodecError::Malformed("PlacementList contains a zero quaternion".into())
            })
        })
        .collect()
}

fn parse_vector_list(
    properties: &[&PropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<[f64; 3]>, CodecError> {
    let Some(view) = side_bytes(properties, "ScaleList", entries)? else {
        return Ok(Vec::new());
    };
    let (count, width) = list_layout(view, 3, "ScaleList")?;
    (0..count)
        .map(|index| {
            let offset = link_array::LEN + index * width * 3;
            Ok([
                read_real(view, offset, width),
                read_real(view, offset + width, width),
                read_real(view, offset + 2 * width, width),
            ])
        })
        .collect()
}

fn side_bytes<'a>(
    properties: &[&PropertyRecord],
    name: &str,
    entries: &BTreeMap<String, View<'a>>,
) -> Result<Option<View<'a>>, CodecError> {
    let Some(property) = properties.iter().find(|property| property.name == name) else {
        return Ok(None);
    };
    let Some(entry) = property.side_entries.first() else {
        return Ok(None);
    };
    entries.get(entry).copied().map(Some).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{property_id} references missing {entry}",
            property_id = property.id
        ))
    })
}

fn list_layout(
    view: View<'_>,
    components: usize,
    name: &str,
) -> Result<(usize, usize), CodecError> {
    let len = view.end().saturating_sub(view.start());
    if len < link_array::LEN {
        return Err(CodecError::malformed(format_args!("{name} is truncated")));
    }
    let mut head = view;
    head.seek(view.start()).expect("window start");
    let count = head.u32_le().expect("four-byte count") as usize;
    let double_len =
        link_array::LEN.saturating_add(count.saturating_mul(components).saturating_mul(8));
    let float_len =
        link_array::LEN.saturating_add(count.saturating_mul(components).saturating_mul(4));
    if len == double_len {
        Ok((count, 8))
    } else if len == float_len {
        Ok((count, 4))
    } else {
        Err(CodecError::malformed(format_args!(
            "{name} count {count} does not match {len} bytes"
        )))
    }
}

fn read_real(view: View<'_>, offset: usize, width: usize) -> f64 {
    let mut cursor = view;
    cursor
        .seek(view.start().saturating_add(offset))
        .expect("bounded real");
    if width == 8 {
        cursor.f64_le().expect("bounded f64")
    } else {
        cursor.f32_le().expect("bounded f32") as f64
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

pub(crate) fn product_cycle_nodes<'a>(
    nodes: &HashMap<&'a str, &'a ProductNodeRecord>,
) -> HashSet<&'a str> {
    let edges = |name: &'a str| {
        nodes.get(name).into_iter().flat_map(|node| {
            node.members
                .iter()
                .map(String::as_str)
                .chain(node.prototype.as_deref())
                .filter(|target| nodes.contains_key(target))
        })
    };
    let mut reverse = HashMap::<&str, Vec<&str>>::new();
    for &source in nodes.keys() {
        reverse.entry(source).or_default();
        for target in edges(source) {
            reverse.entry(target).or_default().push(source);
        }
    }

    let mut visited = HashSet::new();
    let mut finish = Vec::with_capacity(nodes.len());
    for &root in nodes.keys() {
        if !visited.insert(root) {
            continue;
        }
        let mut stack = vec![(root, edges(root).collect::<Vec<_>>(), 0_usize)];
        while let Some((current, targets, next)) = stack.last_mut() {
            if let Some(&target) = targets.get(*next) {
                *next += 1;
                if visited.insert(target) {
                    stack.push((target, edges(target).collect(), 0));
                }
            } else {
                finish.push(*current);
                stack.pop();
            }
        }
    }

    let mut assigned = HashSet::new();
    let mut cyclic = HashSet::new();
    while let Some(root) = finish.pop() {
        if !assigned.insert(root) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![root];
        while let Some(current) = stack.pop() {
            component.push(current);
            for &source in reverse.get(current).into_iter().flatten() {
                if assigned.insert(source) {
                    stack.push(source);
                }
            }
        }
        let self_cycle = component.len() == 1 && edges(component[0]).any(|target| target == root);
        if component.len() > 1 || self_cycle {
            cyclic.extend(component);
        }
    }
    cyclic
}

#[cfg(test)]
pub(crate) mod tests;
