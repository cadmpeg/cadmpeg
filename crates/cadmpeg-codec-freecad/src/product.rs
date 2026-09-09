// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use crate::native::frame::FiniteFrame;
use crate::native::joint::JointRecord;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::brep::ShapePayloadRecord;
use crate::layout::link_array_side_entry_header as link_array;
use crate::native::{
    ContainerNode, LinkOccurrence, ObjectRecord, ProductNode, ProductNodeRecord, PropertyRecord,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    CopyOnChange, CopyOnChangePolicy, ExternalDocumentReference, LinkState, Occurrence,
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
        let group = unique_property(&owned, "Group")?;
        let members = group
            .map(|property| {
                link_list(property, "App::PropertyLinkList", "Group").map(|links| {
                    links
                        .iter()
                        .filter_map(|link| link.object().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
            })
            .transpose()?
            .unwrap_or_default();
        let linked = unique_property(&owned, "LinkedObject")?;
        let prototype_link = linked
            .map(|property| single_link(property, "App::PropertyXLink", "XLink", "LinkedObject"))
            .transpose()?;
        let placement = selected_placement(&owned)?;
        let local_transform = placement.map(placement_matrix).transpose()?.flatten();
        let link_transform = bool_property(&owned, "LinkTransform")?;
        let element_count = integer_property(&owned, "ElementCount")?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| malformed("negative ElementCount"))?;
        let claim_child = bool_property(&owned, "LinkClaimChild")?;
        let copy_on_change = copy_on_change_property(&owned)?;
        let copy_on_change_source = linked_object(
            &owned,
            "LinkCopyOnChangeSource",
            "App::PropertyXLink",
            "XLink",
        )?;
        let copy_on_change_group =
            linked_object(&owned, "LinkCopyOnChangeGroup", "App::PropertyLink", "Link")?;
        let copy_on_change_touched = bool_property(&owned, "LinkCopyOnChangeTouched")?;
        let scale = scale_property(&owned)?
            .map(crate::native::frame::FiniteVec3::try_from)
            .transpose()
            .map_err(malformed)?;
        let element_visibility_count = bool_list_count(&owned, "VisibilityList")?;
        if let Some(count) = element_count {
            let count = usize::try_from(count).map_err(|_| {
                malformed(format!(
                    "product object {} has a negative ElementCount",
                    object.id
                ))
            })?;
            if element_visibility_count != 0 && element_visibility_count != count {
                return Err(malformed(format!(
                    "product object {} has inconsistent link-array counts",
                    object.id
                )));
            }
        }
        let element_objects = unique_property(&owned, "ElementList")?
            .map(|property| {
                link_list(property, "App::PropertyLinkList", "ElementList").map(|links| {
                    links
                        .iter()
                        .filter_map(|link| link.object().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
            })
            .transpose()?
            .unwrap_or_default();
        let placement_property = placement.map(|property| property.id.clone());
        let node = match kind {
            ProductKind::Occurrence => ProductNode::Occurrence(LinkOccurrence {
                members,
                prototype: prototype_link.and_then(|link| link.object().map(str::to_owned)),
                external_document: prototype_link.and_then(|link| link.document().cloned()),
                local_transform,
                placement_property,
                array: crate::native::LinkArray::try_new(
                    element_count,
                    parse_placement_list(&owned, entries)?,
                    parse_vector_list(&owned, entries)?,
                    element_objects,
                )
                .map_err(malformed)?,
                link_transform,
                linked_subelements: prototype_link
                    .map(|link| {
                        link.subelements()
                            .iter()
                            .filter(|subelement| !subelement.is_empty())
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default(),
                claim_child,
                copy_on_change: crate::native::CopyOnChange::from_wire(
                    copy_on_change,
                    copy_on_change_source,
                    copy_on_change_group,
                    copy_on_change_touched,
                )
                .map_err(malformed)?,
                scale,
            }),
            ProductKind::Group => ProductNode::Group(ContainerNode {
                members,
                local_transform,
                placement_property,
            }),
            ProductKind::Part => ProductNode::Part(ContainerNode {
                members,
                local_transform,
                placement_property,
            }),
            ProductKind::LinkGroup => ProductNode::LinkGroup {
                container: ContainerNode {
                    members,
                    local_transform,
                    placement_property,
                },
                element_objects,
            },
        };
        output.push(ProductNodeRecord {
            id: crate::native::native_id("product", &object.name),
            object: object.id.clone(),
            node,
        });
    }
    Ok(output)
}

fn product_record_index(
    records: &[ProductNodeRecord],
) -> Result<HashMap<&str, &ProductNodeRecord>, CodecError> {
    let mut index = HashMap::with_capacity(records.len());
    for record in records {
        if index.insert(record.object.as_str(), record).is_some() {
            return Err(CodecError::malformed(format_args!(
                "product object {} has duplicate product records",
                record.object
            )));
        }
    }
    Ok(index)
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
    let record_by_object = product_record_index(records)?;
    let mut component_objects = records
        .iter()
        .filter(|record| !matches!(record.node, ProductNode::Occurrence(_)))
        .map(|record| record.object.clone())
        .collect::<Vec<_>>();
    let occurrence_objects = records
        .iter()
        .filter(|record| matches!(record.node, ProductNode::Occurrence(_)))
        .map(|record| record.object.as_str())
        .collect::<std::collections::HashSet<_>>();
    for record in records {
        component_objects.extend(
            record
                .members()
                .iter()
                .filter(|member| !occurrence_objects.contains(member.as_str()))
                .cloned(),
        );
        if record.external_document().is_none() {
            component_objects.extend(record.prototype().map(str::to_owned));
        }
        component_objects.extend(record.copy_on_change_source().map(str::to_owned));
        component_objects.extend(record.copy_on_change_group().map(str::to_owned));
        component_objects.extend(record.element_objects().iter().cloned());
    }
    component_objects.extend(
        joints
            .iter()
            .flat_map(|joint| joint.references().into_iter().cloned())
            .filter(|reference| reference.document().is_none())
            .filter_map(|reference| reference.object().map(str::to_owned))
            .filter(|object| !occurrence_objects.contains(object.as_str())),
    );
    component_objects.sort();
    component_objects.dedup();

    let properties_by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(property.owner.as_str())
                .or_default()
                .push(property);
            map
        },
    );
    let mut placements_by_object = HashMap::new();
    for (&owner, owned) in &properties_by_owner {
        if let Some(property) = selected_placement(owned)? {
            if let Some(placement) = placement_matrix(property)? {
                placements_by_object.insert(owner, placement.transform());
            }
        }
    }

    let definition_id = |object: &str| {
        ProductDefinitionId::mint(crate::native::model_id(
            "product_definition",
            object,
            "definition",
        ))
        .expect("identity grammar")
    };
    let container_occurrence_id = |object: &str| {
        OccurrenceId::mint(crate::native::model_id("occurrence", object, "container"))
            .expect("identity grammar")
    };
    let mut parent_by_object = HashMap::<&str, &str>::new();
    for record in records
        .iter()
        .filter(|record| !matches!(record.node, ProductNode::Occurrence(_)))
    {
        for member in record.members() {
            let member = member.as_str();
            match parent_by_object.entry(member) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(&record.object);
                }
                std::collections::hash_map::Entry::Occupied(entry)
                    if *entry.get() != record.object.as_str() =>
                {
                    return Err(CodecError::malformed(format_args!(
                        "product member {member} has multiple parent containers"
                    )));
                }
                std::collections::hash_map::Entry::Occupied(_) => {}
            }
        }
    }

    let mut occurrences = Vec::new();
    for record in records
        .iter()
        .filter(|record| matches!(record.node, ProductNode::Occurrence(_)))
    {
        let count = occurrence_count(record)?;
        let parent = parent_by_object
            .get(record.object.as_str())
            .map(|object| container_occurrence_id(object));
        for index in 0..count {
            let element = count > 1;
            let element_transform = record.element_transforms().get(index).copied();
            let local_transform = record
                .local_transform()
                .map(crate::native::frame::FiniteFrame::transform)
                .unwrap_or_default()
                .compose(
                    element_transform
                        .map(crate::native::frame::FiniteFrame::transform)
                        .unwrap_or_default(),
                )
                .map_err(|error| malformed(error.to_string()))?;
            let prototype_transform = linked_prototype_transform(
                ctx,
                record,
                &record_by_object,
                &placements_by_object,
                &mut Vec::new(),
            )?;
            let element_scale = record
                .element_scales()
                .get(index)
                .copied()
                .map_or([1.0; 3], crate::native::frame::FiniteVec3::values);
            let base_scale = record.scale().unwrap_or([1.0; 3]);
            let scale: [f64; 3] =
                std::array::from_fn(|axis| base_scale[axis] * element_scale[axis]);
            let [x, y, z] = scale.map(|value| {
                cadmpeg_ir::scalar::FiniteReal::new(value)
                    .ok_or_else(|| CodecError::Malformed("occurrence scale must be finite".into()))
            });
            let scale = [x?, y?, z?];
            let copy_on_change = record.copy_on_change().map(|policy| CopyOnChange {
                policy: copy_on_change_policy(policy),
                source: record.copy_on_change_source().map(definition_id),
                group: record.copy_on_change_group().map(definition_id),
                touched: record.copy_on_change_touched(),
            });
            occurrences.push(Occurrence {
                id: OccurrenceId::mint(crate::native::model_id(
                    "occurrence",
                    &record.object,
                    if element {
                        index.to_string()
                    } else {
                        "instance".into()
                    },
                ))
                .expect("identity grammar"),
                prototype: if let Some(document) = record.external_document() {
                    PrototypeReference::External {
                        document: match document {
                            crate::native::ExternalDocument::File(path) => {
                                cadmpeg_ir::products::ExternalDocumentReference::path(path.as_str())
                            }
                            crate::native::ExternalDocument::Name(name) => {
                                cadmpeg_ir::products::ExternalDocumentReference::document_id(
                                    name.as_str(),
                                )
                            }
                        },
                        object: record.prototype().map(str::to_owned),
                    }
                } else if let Some(prototype) = record.prototype() {
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
                transform: local_transform,
                linked_prototype: (record.link_transform() == Some(true))
                    .then_some(prototype_transform),
                scale,
                name: Some(record.object.clone()),
                visible: None,
                link: LinkState::new(
                    record.linked_subelements().to_vec(),
                    record
                        .element_objects()
                        .get(index)
                        .map(|object| definition_id(object)),
                    record.claim_child(),
                    copy_on_change,
                ),
                native_ref: Some(record.object.clone()),
            });
        }
    }

    let object_by_id = objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<HashMap<_, _>>();
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
            let kind = match record.map(|record| &record.node) {
                Some(ProductNode::Part(_)) => ProductDefinitionKind::Part,
                Some(ProductNode::Group(_)) => ProductDefinitionKind::Group,
                Some(ProductNode::LinkGroup { .. }) => ProductDefinitionKind::LinkGroup,
                _ => ProductDefinitionKind::Object,
            };
            let source_object = object_by_id.get(object.as_str()).copied();
            let owned = properties_by_owner
                .get(object.as_str())
                .map(Vec::as_slice)
                .unwrap_or_default();
            let bom_properties = ["Label2", "StockCode", "Vendor", "Manufacturer"]
                .into_iter()
                .filter_map(|name| metadata_string(owned, name).map(|value| (name.into(), value)))
                .collect();
            let id_part_number = source_object.and_then(|object| {
                matches!(
                    object.type_name.as_str(),
                    "Assembly::AssemblyObject" | "Assembly::AssemblyLink" | "App::Part"
                )
                .then(|| metadata_string(owned, "Id"))
                .flatten()
                .filter(|value| !value.is_empty())
            });
            ProductDefinition {
                id: definition_id(object),
                kind,
                source_name: source_object.map(|object| object.name.clone()),
                label: metadata_string(owned, "Label"),
                description: metadata_string(owned, "Description"),
                part_number: metadata_string(owned, "PartNumber")
                    .filter(|value| !value.is_empty())
                    .or(id_part_number),
                bom_properties,
                bodies: bodies
                    .iter()
                    .filter(|body| {
                        body_owners.iter().any(|(prefix, owner)| {
                            *owner == object.as_str() && body.id.as_str().starts_with(prefix)
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
            .and_then(ProductNodeRecord::local_transform)
            .map(crate::native::frame::FiniteFrame::transform)
            .or_else(|| placements_by_object.get(object.as_str()).copied())
            .unwrap_or_default();
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
            transform: local_transform,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: Some(object.clone()),
            visible: None,
            link: None,
            native_ref: Some(object.clone()),
        });
    }
    let mut next_ordinal = HashMap::<Option<String>, u32>::new();
    for occurrence in &mut occurrences {
        let parent = match &occurrence.parent {
            OccurrenceParent::Root => None,
            OccurrenceParent::Occurrence { occurrence } => Some(occurrence.as_str().to_owned()),
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
    records: &HashMap<&str, &ProductNodeRecord>,
    placements: &HashMap<&str, Transform>,
    stack: &mut Vec<String>,
) -> Result<Transform, CodecError> {
    let _depth = ctx.enter_nested("resolve FCStd nested link transform")?;
    if record.link_transform() != Some(true) || record.external_document().is_some() {
        return Ok(Transform::identity());
    }
    let Some(prototype) = record.prototype() else {
        return Ok(Transform::identity());
    };
    if stack.iter().any(|object| object == &record.object) {
        return Err(CodecError::malformed(format_args!(
            "nested link cycle reaches {}",
            record.object
        )));
    }
    stack.push(record.object.clone());
    let target_record = records.get(prototype).copied();
    let placement = target_record
        .and_then(ProductNodeRecord::local_transform)
        .map(crate::native::frame::FiniteFrame::transform)
        .or_else(|| placements.get(prototype).copied())
        .unwrap_or_default();
    let nested = target_record.map_or(Ok(Transform::identity()), |target| {
        linked_prototype_transform(ctx, target, records, placements, stack)
    });
    stack.pop();
    placement
        .compose(nested?)
        .map_err(|error| malformed(error.to_string()))
}

fn occurrence_count(record: &ProductNodeRecord) -> Result<usize, CodecError> {
    let declared_count = record
        .element_count()
        .map(usize::try_from)
        .transpose()
        .map_err(|_| {
            CodecError::malformed(format_args!(
                "{} element count exceeds addressable size",
                record.id
            ))
        })?;
    let count = declared_count.unwrap_or_else(|| {
        [
            record.element_transforms().len(),
            record.element_scales().len(),
            record.element_objects().len(),
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
    if is_path {
        ExternalDocumentReference::path(value)
    } else {
        ExternalDocumentReference::document_id(value)
    }
}

pub(crate) fn multiply(left: [[f64; 4]; 4], right: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
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
) -> Result<Vec<FiniteFrame>, CodecError> {
    let Some(property) = unique_property(properties, "PlacementList")? else {
        return Ok(Vec::new());
    };
    let Some(view) = side_bytes(
        property,
        "App::PropertyPlacementList",
        "PlacementList",
        entries,
    )?
    else {
        return Ok(Vec::new());
    };
    list_layout::<7>(view, "PlacementList")?
        .map(|positions| {
            let values = positions
                .into_iter()
                .map(read_real)
                .collect::<Result<Vec<_>, _>>()?;
            placement_components(&values).ok_or_else(|| {
                CodecError::Malformed("PlacementList contains an invalid placement value".into())
            })
        })
        .collect()
}

fn parse_vector_list(
    properties: &[&PropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<crate::native::frame::FiniteVec3>, CodecError> {
    let Some(property) = unique_property(properties, "ScaleList")? else {
        return Ok(Vec::new());
    };
    let Some(view) = side_bytes(property, "App::PropertyVectorList", "VectorList", entries)? else {
        return Ok(Vec::new());
    };
    list_layout::<3>(view, "ScaleList")?
        .map(|positions| {
            let [x, y, z] = positions.map(read_real);
            crate::native::frame::FiniteVec3::try_from([x?, y?, z?]).map_err(malformed)
        })
        .collect()
}

fn side_bytes<'a>(
    property: &PropertyRecord,
    expected_type: &str,
    name: &str,
    entries: &BTreeMap<String, View<'a>>,
) -> Result<Option<View<'a>>, CodecError> {
    require_root(property, expected_type, name, name)?;
    if property.side_entries().len() > 1 {
        return Err(malformed(format!(
            "product property {} has multiple {name} side entries",
            property.id
        )));
    }
    let Some(entry) = property.side_entries().first() else {
        return Ok(None);
    };
    entries.get(entry).copied().map(Some).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{property_id} references missing {entry}",
            property_id = property.id
        ))
    })
}

fn single_link<'a>(
    property: &'a PropertyRecord,
    expected_type: &str,
    root: &str,
    name: &str,
) -> Result<&'a crate::native::LinkTarget, CodecError> {
    require_root(property, expected_type, name, root)?;
    if property.links().len() != 1 {
        return Err(malformed(format!(
            "product property {} requires one {name} target, found {}",
            property.id,
            property.links().len()
        )));
    }
    Ok(&property.links()[0])
}

fn link_list<'a>(
    property: &'a PropertyRecord,
    expected_type: &str,
    name: &str,
) -> Result<&'a [crate::native::LinkTarget], CodecError> {
    require_root(property, expected_type, name, "LinkList")?;
    if property
        .values()
        .iter()
        .skip(1)
        .any(|value| value.tag != "Link")
    {
        return Err(malformed(format!(
            "product property {} has a non-Link child in {name}",
            property.id
        )));
    }
    Ok(property.links())
}

fn require_root(
    property: &PropertyRecord,
    expected_type: &str,
    name: &str,
    root: &str,
) -> Result<(), CodecError> {
    if property.type_name != expected_type {
        return Err(malformed(format!(
            "product property {} has runtime type {}, expected {expected_type} for {name}",
            property.id, property.type_name
        )));
    }
    if property.values().first().map(|value| value.tag.as_str()) != Some(root)
        || property
            .values()
            .iter()
            .filter(|value| value.tag == root)
            .count()
            != 1
    {
        return Err(malformed(format!(
            "product property {} requires one {root} value for {name}",
            property.id
        )));
    }
    Ok(())
}

fn single_value<'a>(
    property: &'a PropertyRecord,
    expected_type: &str,
    name: &str,
    root: &str,
) -> Result<&'a crate::native::ValueRecord, CodecError> {
    require_root(property, expected_type, name, root)?;
    if property.values().len() != 1 {
        return Err(malformed(format!(
            "product property {} has multiple values for {name}",
            property.id
        )));
    }
    Ok(&property.values()[0])
}

fn unique_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    crate::native::unique_property(properties.iter().copied(), |property| property.name == name)
        .map_err(|_| CodecError::malformed(format_args!("{name} has duplicate carriers")))
}

fn selected_placement<'a>(
    properties: &[&'a PropertyRecord],
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let link_placement = unique_property(properties, "LinkPlacement")?;
    let placement = unique_property(properties, "Placement")?;
    for property in [link_placement, placement].into_iter().flatten() {
        placement_matrix(property)?;
    }
    match (link_placement, placement) {
        (Some(link_placement), Some(placement)) => {
            let use_link_placement =
                bool_property(properties, "LinkTransform")?.ok_or_else(|| {
                    malformed("LinkPlacement and Placement require a valid LinkTransform policy")
                })?;
            Ok(Some(if use_link_placement {
                link_placement
            } else {
                placement
            }))
        }
        (Some(link_placement), None) => Ok(Some(link_placement)),
        (None, Some(placement)) => Ok(Some(placement)),
        (None, None) => Ok(None),
    }
}

#[derive(Clone, Copy)]
enum RealWidth {
    Single,
    Double,
}

impl RealWidth {
    fn bytes(self) -> usize {
        match self {
            Self::Single => 4,
            Self::Double => 8,
        }
    }
}

#[derive(Clone, Copy)]
struct RealPosition<'a> {
    view: View<'a>,
    offset: usize,
    width: RealWidth,
}

fn list_layout<'a, const N: usize>(
    view: View<'a>,
    name: &str,
) -> Result<impl Iterator<Item = [RealPosition<'a>; N]>, CodecError> {
    let len = view.end() - view.start();
    if len < link_array::LEN {
        return Err(CodecError::malformed(format_args!("{name} is truncated")));
    }
    let mut head = view;
    head.seek(view.start())
        .ok_or_else(|| CodecError::malformed(format_args!("{name} header is out of bounds")))?;
    let count = usize::try_from(
        head.req_u32_le()
            .map_err(|error| CodecError::malformed(format_args!("link-array count: {error:?}")))?,
    )
    .map_err(CodecError::malformed)?;
    let encoded_len = |width: RealWidth| {
        count
            .checked_mul(N)
            .and_then(|count| count.checked_mul(width.bytes()))
            .and_then(|bytes| link_array::LEN.checked_add(bytes))
    };
    let width = if encoded_len(RealWidth::Double) == Some(len) {
        RealWidth::Double
    } else if encoded_len(RealWidth::Single) == Some(len) {
        RealWidth::Single
    } else {
        return Err(CodecError::malformed(format_args!(
            "{name} count {count} does not match {len} bytes"
        )));
    };
    Ok((0..count).map(move |index| {
        std::array::from_fn(|component| RealPosition {
            view,
            offset: view.start() + link_array::LEN + (index * N + component) * width.bytes(),
            width,
        })
    }))
}

fn read_real(position: RealPosition<'_>) -> Result<f64, CodecError> {
    let mut cursor = position.view;
    cursor
        .seek(position.offset)
        .ok_or_else(|| CodecError::malformed("real position is out of bounds"))?;
    match position.width {
        RealWidth::Double => cursor.req_f64_le().map_err(|error| {
            CodecError::malformed(format_args!("link-array component: {error:?}"))
        }),
        RealWidth::Single => cursor.req_f32_le().map(f64::from).map_err(|error| {
            CodecError::malformed(format_args!("link-array component: {error:?}"))
        }),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ProductKind {
    Group,
    Part,
    LinkGroup,
    Occurrence,
}

fn product_kind(kind: &str) -> Option<ProductKind> {
    match kind {
        "Assembly::AssemblyObject" | "Assembly::AssemblyLink" | "App::Part" => {
            Some(ProductKind::Part)
        }
        "App::DocumentObjectGroup" => Some(ProductKind::Group),
        "App::LinkGroup" => Some(ProductKind::LinkGroup),
        "App::Link" | "App::LinkElement" => Some(ProductKind::Occurrence),
        _ => None,
    }
}

fn metadata_string(properties: &[&PropertyRecord], name: &str) -> Option<String> {
    let property = properties.iter().find(|property| property.name == name)?;
    if property.type_name != "App::PropertyString" {
        return None;
    }
    let document = roxmltree::Document::parse(property.xml.text()).ok()?;
    let root = document.root_element();
    if !root.has_tag_name("Property") {
        return None;
    }
    let mut values = root.children().filter(roxmltree::Node::is_element);
    let value = values.next()?;
    if values.next().is_some()
        || !value.has_tag_name("String")
        || value.children().any(|node| node.is_element())
    {
        return None;
    }
    value.attribute("value").map(str::to_owned)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn bool_property(properties: &[&PropertyRecord], name: &str) -> Result<Option<bool>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(None);
    };
    let value = single_value(property, "App::PropertyBool", name, "Bool")?;
    let value = value.attributes.get("value").ok_or_else(|| {
        malformed(format!(
            "product property {} has no Bool value",
            property.id
        ))
    })?;
    parse_bool(value).map(Some).ok_or_else(|| {
        malformed(format!(
            "product property {} has an invalid Bool value",
            property.id
        ))
    })
}

fn integer_property(properties: &[&PropertyRecord], name: &str) -> Result<Option<i64>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(None);
    };
    let value = single_value(property, "App::PropertyIntegerConstraint", name, "Integer")?;
    let value = value.attributes.get("value").ok_or_else(|| {
        malformed(format!(
            "product property {} has no Integer value",
            property.id
        ))
    })?;
    value.parse().map(Some).map_err(|_| {
        malformed(format!(
            "product property {} has an invalid Integer value",
            property.id
        ))
    })
}

fn copy_on_change_property(properties: &[&PropertyRecord]) -> Result<Option<String>, CodecError> {
    let Some(property) = unique_property(properties, "LinkCopyOnChange")? else {
        return Ok(None);
    };
    let value = single_value(
        property,
        "App::PropertyEnumeration",
        "LinkCopyOnChange",
        "Integer",
    )?;
    Ok(Some(
        value
            .attributes
            .get("value")
            .ok_or_else(|| {
                malformed(format!(
                    "product property {} has no enumeration value",
                    property.id
                ))
            })?
            .to_owned(),
    ))
}

fn linked_object(
    properties: &[&PropertyRecord],
    name: &str,
    expected_type: &str,
    root: &str,
) -> Result<Option<String>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(None);
    };
    let link = single_link(property, expected_type, root, name)?;
    Ok(link.object().map(str::to_owned))
}

fn scale_property(properties: &[&PropertyRecord]) -> Result<Option<[f64; 3]>, CodecError> {
    if let Some(property) = unique_property(properties, "ScaleVector")? {
        return vector_property(property).map(Some);
    }
    let Some(property) = unique_property(properties, "Scale")? else {
        return Ok(None);
    };
    let value = single_value(property, "App::PropertyFloat", "Scale", "Float")?;
    let value = value.attributes.get("value").ok_or_else(|| {
        malformed(format!(
            "product property {} has no Float value",
            property.id
        ))
    })?;
    let value = parse_finite(value, property, "Scale")?;
    Ok(Some([value; 3]))
}

fn vector_property(property: &PropertyRecord) -> Result<[f64; 3], CodecError> {
    let value = single_value(
        property,
        "App::PropertyVector",
        "ScaleVector",
        "PropertyVector",
    )?;
    let component = |name: &str| {
        let value = value.attributes.get(name).ok_or_else(|| {
            malformed(format!(
                "product property {} has no {name} vector component",
                property.id
            ))
        })?;
        parse_finite(value, property, "ScaleVector")
    };
    Ok([
        component("valueX")?,
        component("valueY")?,
        component("valueZ")?,
    ])
}

fn parse_finite(value: &str, property: &PropertyRecord, name: &str) -> Result<f64, CodecError> {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            malformed(format!(
                "product property {} has an invalid finite value for {name}",
                property.id
            ))
        })
}

fn bool_list_count(properties: &[&PropertyRecord], name: &str) -> Result<usize, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(0);
    };
    let value = single_value(property, "App::PropertyBoolList", name, "BoolList")?;
    let encoded = value.attributes.get("value").ok_or_else(|| {
        malformed(format!(
            "product property {} has no BoolList value",
            property.id
        ))
    })?;
    if encoded.bytes().any(|byte| !matches!(byte, b'0' | b'1')) {
        return Err(malformed(format!(
            "product property {} has an invalid BoolList bit string",
            property.id
        )));
    }
    Ok(encoded.len())
}

fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}

pub(crate) fn placement_matrix(
    property: &PropertyRecord,
) -> Result<Option<FiniteFrame>, CodecError> {
    if property.type_name != "App::PropertyPlacement" {
        return Err(CodecError::malformed(format_args!(
            "placement property {} has a non-placement runtime type",
            property.id
        )));
    }
    if property.values().len() != 1 {
        return Err(malformed(format!(
            "placement property {} requires one placement value",
            property.id
        )));
    }
    let value = &property.values()[0];
    if value.tag != "PropertyPlacement" {
        return Err(malformed(format!(
            "placement property {} requires one PropertyPlacement value",
            property.id
        )));
    }
    let number = |name: &str| {
        value
            .attributes
            .get(name)
            .and_then(|value| value.parse().ok())
            .filter(|value: &f64| value.is_finite())
    };
    let position = ["Px", "Py", "Pz"]
        .into_iter()
        .map(|name| {
            number(name).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "placement property {} has an invalid {name} component",
                    property.id
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let quaternion = if value.attributes.contains_key("A") {
        let axis = ["Ox", "Oy", "Oz"]
            .into_iter()
            .map(|name| {
                number(name).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "placement property {} has an invalid {name} axis component",
                        property.id
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let angle = number("A").ok_or_else(|| {
            CodecError::malformed(format_args!(
                "placement property {} has an invalid A angle component",
                property.id
            ))
        })?;
        let axis_norm = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        let (x, y, z) = if axis_norm.is_finite() && axis_norm > 0.0 {
            (
                axis[0] / axis_norm,
                axis[1] / axis_norm,
                axis[2] / axis_norm,
            )
        } else if axis_norm == 0.0 {
            (0.0, 0.0, 1.0)
        } else {
            return Err(CodecError::malformed(format_args!(
                "placement property {} has an invalid axis norm",
                property.id
            )));
        };
        let half_angle = angle / 2.0;
        let scale = half_angle.sin();
        vec![x * scale, y * scale, z * scale, half_angle.cos()]
    } else {
        ["Q0", "Q1", "Q2", "Q3"]
            .into_iter()
            .map(|name| {
                number(name).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "placement property {} has an invalid {name} quaternion component",
                        property.id
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let values = position.into_iter().chain(quaternion).collect::<Vec<_>>();
    let matrix = placement_components(&values).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "placement property {} has an invalid rotation",
            property.id
        ))
    })?;
    Ok(Some(matrix))
}

fn placement_components(values: &[f64]) -> Option<FiniteFrame> {
    let [px, py, pz, x, y, z, w] = *<&[f64; 7]>::try_from(values).ok()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);
    FiniteFrame::try_from([
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
    .ok()
}

pub(crate) fn product_cycle_nodes<'a>(
    nodes: &HashMap<&'a str, &'a ProductNodeRecord>,
) -> HashSet<&'a str> {
    let edges = |name: &'a str| {
        nodes.get(name).into_iter().flat_map(|node| {
            node.members()
                .iter()
                .map(String::as_str)
                .chain(node.prototype())
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
