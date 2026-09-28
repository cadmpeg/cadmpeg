// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use crate::native::frame::FiniteFrame;
use crate::native::joint::JointRecord;
use crate::placement::{placement_components, placement_matrix};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroUsize;

use crate::brep::ShapePayloadRecord;
use crate::layout::link_array_side_entry_header as link_array;
use crate::native::{
    malformed, parse_bool, sole_named_property, ContainerNode,
    CopyOnChangePolicy as NativeCopyOnChangePolicy, LinkArrayCardinality, LinkOccurrence,
    ObjectRecord, ProductNode, ProductNodeRecord, PropertyRecord,
};
use crate::resource::{
    collection_allocation_failed, collection_vec, reserve_vec_items, retained_format,
    retained_string, retained_strings,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    CopyOnChange, CopyOnChangePolicy, ExternalDocument, LinkState, Occurrence, OccurrenceParent,
    ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FiniteVector;

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<ProductNodeRecord>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            ctx.charge_collection_items(1, "fcstd product owner index")?;
            by_owner
                .try_reserve(1)
                .map_err(|_| collection_allocation_failed(ctx, 1, "fcstd product owner index"))?;
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            reserve_vec_items(ctx, owned, 1, "fcstd product owner properties")?;
            owned.push(property);
        }
    }
    let mut output = Vec::new();
    for object in objects {
        let Some(kind) = product_kind(&object.type_name) else {
            continue;
        };
        let source = by_owner
            .get(object.id.as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut owned = collection_vec(ctx, source.len(), "fcstd product selected properties")?;
        owned.extend_from_slice(source);
        let group = sole_named_property(ctx, "product", &owned, "Group")?;
        let members = group
            .map(|property| {
                linked_object_names(
                    ctx,
                    link_list(ctx, property, "App::PropertyLinkList", "Group")?,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let linked = sole_named_property(ctx, "product", &owned, "LinkedObject")?;
        let prototype_link = linked
            .map(|property| {
                single_link(ctx, property, "App::PropertyXLink", "XLink", "LinkedObject")
            })
            .transpose()?
            .flatten();
        let placement = selected_placement(ctx, &owned)?;
        let local_transform = placement
            .map(|property| placement_matrix(ctx, property))
            .transpose()?
            .flatten();
        let link_transform = bool_property(ctx, &owned, "LinkTransform")?;
        let element_count = integer_property(ctx, &owned, "ElementCount")?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| malformed("negative ElementCount"))?;
        let claim_child = bool_property(ctx, &owned, "LinkClaimChild")?;
        let copy_on_change = copy_on_change_property(ctx, &owned)?;
        let copy_on_change_source = linked_target(
            ctx,
            &owned,
            "LinkCopyOnChangeSource",
            "App::PropertyXLink",
            "XLink",
        )?;
        let copy_on_change_group = linked_target(
            ctx,
            &owned,
            "LinkCopyOnChangeGroup",
            "App::PropertyLink",
            "Link",
        )?;
        let copy_on_change_touched = bool_property(ctx, &owned, "LinkCopyOnChangeTouched")?;
        let scale = scale_property(ctx, &owned)?;
        let element_visibility = bool_list(ctx, &owned, "VisibilityList")?;
        let element_objects = sole_named_property(ctx, "product", &owned, "ElementList")?
            .map(|property| {
                linked_object_names(
                    ctx,
                    link_list(ctx, property, "App::PropertyLinkList", "ElementList")?,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let placement_property = placement
            .map(|property| retained_string(ctx, &property.id, "fcstd product placement property"))
            .transpose()?;
        let node = match kind {
            ProductKind::Occurrence => ProductNode::Occurrence(LinkOccurrence {
                members,
                prototype: prototype_link
                    .and_then(|link| link.object())
                    .map(|name| retained_string(ctx, name, "fcstd product prototype"))
                    .transpose()?,
                external_document: prototype_link
                    .and_then(|link| link.document())
                    .map(|document| document.clone_with_context(ctx))
                    .transpose()?,
                local_transform,
                placement_property,
                array: crate::native::LinkArray::try_new(
                    element_count,
                    parse_placement_list(ctx, &owned, entries)?,
                    parse_vector_list(ctx, &owned, entries)?,
                    element_visibility,
                    element_objects,
                )
                .map_err(malformed)?,
                link_transform,
                linked_subelements: prototype_link
                    .map(|link| nonempty_subelements(ctx, link.subelements()))
                    .transpose()?
                    .unwrap_or_default(),
                claim_child,
                copy_on_change: crate::native::CopyOnChange::from_admitted(
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
        reserve_vec_items(ctx, &mut output, 1, "fcstd product records")?;
        output.push(ProductNodeRecord {
            id: crate::native::native_id_charged(ctx, "product", &object.name)?,
            object: retained_string(ctx, &object.id, "fcstd product object")?,
            node,
        });
    }
    Ok(output)
}

fn linked_object_names(
    ctx: &DecodeContext<'_>,
    links: &[Option<crate::native::LinkTarget>],
) -> Result<Vec<String>, CodecError> {
    let count = links
        .iter()
        .flatten()
        .filter(|link| link.object().is_some())
        .count();
    let mut names = collection_vec(ctx, count, "fcstd product linked object names")?;
    for link in links.iter().flatten() {
        if let Some(name) = link.object() {
            names.push(retained_string(
                ctx,
                name,
                "fcstd product linked object name",
            )?);
        }
    }
    Ok(names)
}

fn nonempty_subelements(
    ctx: &DecodeContext<'_>,
    values: &[String],
) -> Result<Vec<String>, CodecError> {
    let count = values.iter().filter(|value| !value.is_empty()).count();
    let mut subelements = collection_vec(ctx, count, "fcstd product linked subelements")?;
    for value in values.iter().filter(|value| !value.is_empty()) {
        subelements.push(retained_string(
            ctx,
            value,
            "fcstd product linked subelement",
        )?);
    }
    Ok(subelements)
}

fn product_record_index<'a>(
    ctx: &DecodeContext<'_>,
    records: &'a [ProductNodeRecord],
) -> Result<HashMap<&'a str, &'a ProductNodeRecord>, CodecError> {
    let mut index = HashMap::new();
    ctx.charge_collection_items(records.len() as u64, "fcstd product record index")?;
    index.try_reserve(records.len()).map_err(|_| {
        collection_allocation_failed(ctx, records.len() as u64, "fcstd product record index")
    })?;
    for record in records {
        if index.insert(record.object.as_str(), record).is_some() {
            return Err(CodecError::Malformed(retained_format(
                ctx,
                format_args!(
                    "product object {} has duplicate product records",
                    record.object
                ),
                "fcstd product duplicate record",
            )?));
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
    let record_by_object = product_record_index(ctx, records)?;
    let mut component_objects = Vec::new();
    let mut occurrence_objects = HashSet::new();
    for record in records {
        if matches!(record.node, ProductNode::Occurrence(_)) {
            crate::resource::insert_hash_set(
                ctx,
                &mut occurrence_objects,
                record.object.as_str(),
                "fcstd product occurrence names",
            )?;
        } else {
            reserve_vec_items(
                ctx,
                &mut component_objects,
                1,
                "fcstd product component names",
            )?;
            component_objects.push(record.object.as_str());
        }
    }
    for record in records {
        for member in record
            .members()
            .iter()
            .filter(|member| !occurrence_objects.contains(member.as_str()))
        {
            reserve_vec_items(
                ctx,
                &mut component_objects,
                1,
                "fcstd product component names",
            )?;
            component_objects.push(member.as_str());
        }
        if record.external_document().is_none() {
            if let Some(prototype) = record.prototype() {
                reserve_vec_items(
                    ctx,
                    &mut component_objects,
                    1,
                    "fcstd product component names",
                )?;
                component_objects.push(prototype);
            }
        }
        for target in [
            record.copy_on_change_source(),
            record.copy_on_change_group(),
        ]
        .into_iter()
        .flatten()
        {
            if target.document().is_none() {
                if let Some(name) = target.object() {
                    reserve_vec_items(
                        ctx,
                        &mut component_objects,
                        1,
                        "fcstd product component names",
                    )?;
                    component_objects.push(name);
                }
            }
        }
        for name in record.element_objects() {
            reserve_vec_items(
                ctx,
                &mut component_objects,
                1,
                "fcstd product component names",
            )?;
            component_objects.push(name);
        }
    }
    for joint in joints {
        for reference in joint.references() {
            if reference.document().is_none() {
                if let Some(name) = reference
                    .object()
                    .filter(|name| !occurrence_objects.contains(*name))
                {
                    reserve_vec_items(
                        ctx,
                        &mut component_objects,
                        1,
                        "fcstd product component names",
                    )?;
                    component_objects.push(name);
                }
            }
        }
    }
    component_objects.sort_unstable();
    component_objects.dedup();

    let mut properties_by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !properties_by_owner.contains_key(property.owner.as_str()) {
            ctx.charge_collection_items(1, "fcstd product neutral owner index")?;
            properties_by_owner.try_reserve(1).map_err(|_| {
                collection_allocation_failed(ctx, 1, "fcstd product neutral owner index")
            })?;
            properties_by_owner.insert(property.owner.as_str(), Vec::new());
        }
        if let Some(owned) = properties_by_owner.get_mut(property.owner.as_str()) {
            reserve_vec_items(ctx, owned, 1, "fcstd product neutral owner properties")?;
            owned.push(property);
        }
    }
    let mut placements_by_object = HashMap::new();
    for (&owner, owned) in &properties_by_owner {
        if let Some(property) = selected_placement(ctx, owned)? {
            if let Some(placement) = placement_matrix(ctx, property)? {
                ctx.charge_collection_items(1, "fcstd product placements")?;
                placements_by_object.try_reserve(1).map_err(|_| {
                    collection_allocation_failed(ctx, 1, "fcstd product placements")
                })?;
                placements_by_object.insert(owner, placement.transform());
            }
        }
    }

    let definition_id = |object: &str| -> Result<ProductDefinitionId, CodecError> {
        ProductDefinitionId::mint(crate::native::model_id_charged(
            ctx,
            "product_definition",
            object,
            "definition",
        )?)
        .map_err(CodecError::malformed)
    };
    let container_occurrence_id = |object: &str| -> Result<OccurrenceId, CodecError> {
        OccurrenceId::mint(crate::native::model_id_charged(
            ctx,
            "occurrence",
            object,
            "container",
        )?)
        .map_err(CodecError::malformed)
    };
    let mut parent_by_object = HashMap::<&str, &str>::new();
    for record in records
        .iter()
        .filter(|record| !matches!(record.node, ProductNode::Occurrence(_)))
    {
        for member in record.members() {
            let member = member.as_str();
            match parent_by_object.get(member) {
                None => {
                    ctx.charge_collection_items(1, "fcstd product parent index")?;
                    parent_by_object.try_reserve(1).map_err(|_| {
                        collection_allocation_failed(ctx, 1, "fcstd product parent index")
                    })?;
                    parent_by_object.insert(member, record.object.as_str());
                }
                Some(previous) if *previous != record.object.as_str() => {
                    return Err(CodecError::Malformed(retained_format(
                        ctx,
                        format_args!("product member {member} has multiple parent containers"),
                        "fcstd product parent conflict",
                    )?));
                }
                Some(_) => {}
            }
        }
    }

    let mut occurrences = Vec::new();
    for record in records
        .iter()
        .filter(|record| matches!(record.node, ProductNode::Occurrence(_)))
    {
        let count = occurrence_count(ctx, record)?.get();
        let parent = parent_by_object
            .get(record.object.as_str())
            .map(|object| container_occurrence_id(object))
            .transpose()?;
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
                .map_or([1.0; 3], cadmpeg_ir::units::FiniteVector::get);
            let base_scale = record.scale().unwrap_or([1.0; 3]);
            let scale: [f64; 3] =
                std::array::from_fn(|axis| base_scale[axis] * element_scale[axis]);
            let [x, y, z] = scale.map(|value| {
                cadmpeg_ir::scalar::FiniteReal::new(value)
                    .ok_or_else(|| CodecError::Malformed("occurrence scale must be finite".into()))
            });
            let scale = [x?, y?, z?];
            let copy_on_change = record
                .copy_on_change_policy()
                .map(|policy| {
                    Ok::<_, CodecError>(CopyOnChange {
                        policy: copy_on_change_policy(ctx, policy)?,
                        source: record
                            .copy_on_change_source()
                            .map(|target| neutral_link_target(ctx, target))
                            .transpose()?
                            .flatten(),
                        group: record
                            .copy_on_change_group()
                            .map(|target| neutral_link_target(ctx, target))
                            .transpose()?
                            .flatten(),
                        touched: record.copy_on_change_touched(),
                    })
                })
                .transpose()?;
            let occurrence_id = if element {
                crate::native::model_id_charged(
                    ctx,
                    "occurrence",
                    &record.object,
                    &index.to_string(),
                )?
            } else {
                crate::native::model_id_charged(ctx, "occurrence", &record.object, "instance")?
            };
            reserve_vec_items(ctx, &mut occurrences, 1, "fcstd product occurrences")?;
            occurrences.push(Occurrence {
                id: OccurrenceId::mint(occurrence_id).map_err(CodecError::malformed)?,
                prototype: if let Some(document) = record.external_document() {
                    PrototypeReference::External {
                        document: external_document_reference_charged(
                            ctx,
                            document.as_str(),
                            document.attribute(),
                        )?,
                        object: record
                            .prototype()
                            .map(|value| {
                                retained_string(ctx, value, "fcstd product external prototype")
                            })
                            .transpose()?,
                    }
                } else if let Some(prototype) = record.prototype() {
                    PrototypeReference::Local {
                        definition: definition_id(prototype)?,
                    }
                } else {
                    PrototypeReference::Unresolved {}
                },
                parent: parent
                    .as_ref()
                    .map(|occurrence| {
                        OccurrenceId::mint(retained_string(
                            ctx,
                            occurrence.as_str(),
                            "fcstd product parent identity",
                        )?)
                        .map(|occurrence| OccurrenceParent::Occurrence { occurrence })
                        .map_err(CodecError::malformed)
                    })
                    .transpose()?
                    .unwrap_or(OccurrenceParent::Root {}),
                ordinal: u32::try_from(index).map_err(|_| {
                    CodecError::malformed(format_args!(
                        "product occurrence {} element index exceeds u32",
                        record.id
                    ))
                })?,
                transform: local_transform,
                linked_prototype: (record.link_transform() == Some(true))
                    .then_some(prototype_transform),
                scale,
                name: Some(retained_string(
                    ctx,
                    &record.object,
                    "fcstd product occurrence name",
                )?),
                visible: None,
                link: LinkState::new(
                    retained_strings(
                        ctx,
                        record.linked_subelements(),
                        "fcstd product occurrence subelements",
                    )?,
                    record
                        .element_objects()
                        .get(index)
                        .map(|object| definition_id(object))
                        .transpose()?,
                    record.claim_child(),
                    copy_on_change,
                ),
                native_ref: Some(retained_string(
                    ctx,
                    &record.object,
                    "fcstd product occurrence native reference",
                )?),
            });
        }
    }

    let mut object_by_id = HashMap::new();
    ctx.charge_collection_items(objects.len() as u64, "fcstd product object index")?;
    object_by_id.try_reserve(objects.len()).map_err(|_| {
        collection_allocation_failed(ctx, objects.len() as u64, "fcstd product object index")
    })?;
    for object in objects {
        object_by_id.insert(object.id.as_str(), object);
    }
    let mut property_owner = HashMap::new();
    ctx.charge_collection_items(properties.len() as u64, "fcstd product property owners")?;
    property_owner.try_reserve(properties.len()).map_err(|_| {
        collection_allocation_failed(
            ctx,
            properties.len() as u64,
            "fcstd product property owners",
        )
    })?;
    for property in properties {
        property_owner.insert(property.id.as_str(), property.owner.as_str());
    }
    let mut body_owners = Vec::new();
    for payload in payloads {
        if let Some(owner) = property_owner.get(payload.property.as_str()) {
            reserve_vec_items(ctx, &mut body_owners, 1, "fcstd product body owners")?;
            body_owners.push((
                crate::native::model_id_charged(ctx, "body", &payload.id, "")?,
                *owner,
            ));
        }
    }
    let mut definitions =
        collection_vec(ctx, component_objects.len(), "fcstd product definitions")?;
    for &object in &component_objects {
        let record = record_by_object.get(object).copied();
        let kind = match record.map(|record| &record.node) {
            Some(ProductNode::Part(_)) => ProductDefinitionKind::Part,
            Some(ProductNode::Group(_)) => ProductDefinitionKind::Group,
            Some(ProductNode::LinkGroup { .. }) => ProductDefinitionKind::LinkGroup,
            _ => ProductDefinitionKind::Object,
        };
        let source_object = object_by_id.get(object).copied();
        let owned = properties_by_owner
            .get(object)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut bom_properties = BTreeMap::new();
        for name in [
            cadmpeg_core::nonblank_literal!("Label2"),
            cadmpeg_core::nonblank_literal!("StockCode"),
            cadmpeg_core::nonblank_literal!("Vendor"),
            cadmpeg_core::nonblank_literal!("Manufacturer"),
        ] {
            if let Some(value) = metadata_string(ctx, owned, name.as_str())? {
                bom_properties.insert(name, value);
            }
        }
        let id_part_number = if source_object.is_some_and(|object| {
            matches!(
                object.type_name.as_str(),
                "Assembly::AssemblyObject" | "Assembly::AssemblyLink" | "App::Part"
            )
        }) {
            metadata_string(ctx, owned, "Id")?.filter(|value| !value.is_empty())
        } else {
            None
        };
        let mut definition_bodies = Vec::new();
        for body in bodies.iter().filter(|body| {
            body_owners
                .iter()
                .any(|(prefix, owner)| *owner == object && body.id.as_str().starts_with(prefix))
        }) {
            reserve_vec_items(
                ctx,
                &mut definition_bodies,
                1,
                "fcstd product definition bodies",
            )?;
            definition_bodies.push(
                cadmpeg_ir::ids::BodyId::mint(retained_string(
                    ctx,
                    body.id.as_str(),
                    "fcstd product body identity",
                )?)
                .map_err(CodecError::malformed)?,
            );
        }
        definitions.push(ProductDefinition {
            id: definition_id(object)?,
            kind,
            source_name: source_object
                .map(|object| retained_string(ctx, &object.name, "fcstd product source name"))
                .transpose()?,
            label: metadata_string(ctx, owned, "Label")?,
            description: metadata_string(ctx, owned, "Description")?,
            part_number: metadata_string(ctx, owned, "PartNumber")?
                .filter(|value| !value.is_empty())
                .or(id_part_number),
            bom_properties,
            bodies: definition_bodies,
            native_ref: Some(retained_string(
                ctx,
                object,
                "fcstd product definition native reference",
            )?),
        });
    }

    for object in &component_objects {
        let record = record_by_object.get(*object).copied();
        let local_transform = record
            .and_then(ProductNodeRecord::local_transform)
            .map(crate::native::frame::FiniteFrame::transform)
            .or_else(|| placements_by_object.get(*object).copied())
            .unwrap_or_default();
        let parent = parent_by_object.get(*object).copied();
        let parent = match parent {
            Some(parent) => OccurrenceParent::Occurrence {
                occurrence: container_occurrence_id(parent)?,
            },
            None => OccurrenceParent::Root {},
        };
        reserve_vec_items(ctx, &mut occurrences, 1, "fcstd product occurrences")?;
        occurrences.push(Occurrence {
            id: container_occurrence_id(object)?,
            prototype: PrototypeReference::Local {
                definition: definition_id(object)?,
            },
            parent,
            ordinal: 0,
            transform: local_transform,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: Some(retained_string(
                ctx,
                object,
                "fcstd product container name",
            )?),
            visible: None,
            link: None,
            native_ref: Some(retained_string(
                ctx,
                object,
                "fcstd product container native reference",
            )?),
        });
    }
    let mut next_ordinal = HashMap::<Option<String>, u32>::new();
    for occurrence in &mut occurrences {
        let parent = match &occurrence.parent {
            OccurrenceParent::Root {} => None,
            OccurrenceParent::Occurrence { occurrence } => Some(retained_string(
                ctx,
                occurrence.as_str(),
                "fcstd product ordinal parent",
            )?),
        };
        if !next_ordinal.contains_key(&parent) {
            ctx.charge_collection_items(1, "fcstd product ordinal index")?;
            next_ordinal
                .try_reserve(1)
                .map_err(|_| collection_allocation_failed(ctx, 1, "fcstd product ordinal index"))?;
        }
        let ordinal = next_ordinal.entry(parent).or_default();
        occurrence.ordinal = *ordinal;
        *ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("product occurrence ordinal exceeds u32"))?;
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
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("nested link cycle reaches {}", record.object),
            "fcstd nested product cycle",
        )?));
    }
    reserve_vec_items(ctx, stack, 1, "fcstd nested product stack")?;
    stack.push(retained_string(
        ctx,
        &record.object,
        "fcstd nested product identity",
    )?);
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

/// Occurrences one product node contributes.
///
/// A link that states no array, or states a zero `ElementCount`, is scalar and
/// contributes its single occurrence; the count is the cardinality the node's
/// own type carries, never a floored zero.
fn occurrence_count(
    ctx: &DecodeContext<'_>,
    record: &ProductNodeRecord,
) -> Result<NonZeroUsize, CodecError> {
    let elements = match record.element_cardinality() {
        None | Some(LinkArrayCardinality::Scalar) => return Ok(NonZeroUsize::MIN),
        Some(LinkArrayCardinality::Elements(elements)) => elements,
    };
    let count = NonZeroUsize::try_from(elements).or_else(|_| {
        Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("{} element count exceeds addressable size", record.id),
            "fcstd product element count",
        )?))
    })?;
    if count.get() > 1_000_000 || u32::try_from(count.get()).is_err() {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("{} link-array count limit exceeded", record.id),
            "fcstd product array count limit",
        )?));
    }
    Ok(count)
}

fn copy_on_change_policy(
    ctx: &DecodeContext<'_>,
    value: &NativeCopyOnChangePolicy,
) -> Result<CopyOnChangePolicy, CodecError> {
    Ok(match value.index() {
        0 => CopyOnChangePolicy::Disabled,
        1 => CopyOnChangePolicy::Enabled,
        2 => CopyOnChangePolicy::Owned,
        3 => CopyOnChangePolicy::Tracking,
        _ => CopyOnChangePolicy::Native(retained_string(
            ctx,
            value.as_str(),
            "fcstd product copy on change",
        )?),
    })
}

pub(crate) fn external_document_reference_charged(
    ctx: &DecodeContext<'_>,
    value: &str,
    attribute: Option<&str>,
) -> Result<ExternalDocument, CodecError> {
    let value = crate::resource::retained_string(ctx, value, "fcstd external document reference")?;
    Ok(
        if attribute.is_some_and(|name| name.eq_ignore_ascii_case("file")) {
            ExternalDocument::path(value)
        } else {
            ExternalDocument::document_id(value)
        },
    )
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
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<FiniteFrame>, CodecError> {
    let Some(property) = sole_named_property(ctx, "product", properties, "PlacementList")? else {
        return Ok(Vec::new());
    };
    let Some(view) = side_bytes(
        ctx,
        property,
        "App::PropertyPlacementList",
        "PlacementList",
        entries,
    )?
    else {
        return Ok(Vec::new());
    };
    let positions = list_layout::<7>(view, "PlacementList")?;
    let mut placements = collection_vec(ctx, positions.len(), "fcstd product placement list")?;
    for positions in positions {
        let [px, py, pz, qx, qy, qz, qw] = positions.map(read_real);
        let values = [px?, py?, pz?, qx?, qy?, qz?, qw?];
        placements.push(placement_components(&values).ok_or_else(|| {
            CodecError::Malformed("PlacementList contains an invalid placement value".into())
        })?);
    }
    Ok(placements)
}

fn parse_vector_list(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<cadmpeg_ir::units::FiniteVector<3>>, CodecError> {
    let Some(property) = sole_named_property(ctx, "product", properties, "ScaleList")? else {
        return Ok(Vec::new());
    };
    let Some(view) = side_bytes(
        ctx,
        property,
        "App::PropertyVectorList",
        "VectorList",
        entries,
    )?
    else {
        return Ok(Vec::new());
    };
    let positions = list_layout::<3>(view, "ScaleList")?;
    let mut vectors = collection_vec(ctx, positions.len(), "fcstd product scale list")?;
    for positions in positions {
        let [x, y, z] = positions.map(read_real);
        vectors.push(
            cadmpeg_ir::units::FiniteVector::new([x?, y?, z?]).ok_or_else(|| {
                malformed("element_scales: scale vector components must be finite")
            })?,
        );
    }
    Ok(vectors)
}

fn side_bytes<'a>(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_type: &str,
    name: &str,
    entries: &BTreeMap<String, View<'a>>,
) -> Result<Option<View<'a>>, CodecError> {
    require_root(ctx, property, expected_type, name, name)?;
    if property.side_entries().len() > 1 {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} has multiple {name} side entries",
                property.id
            ),
            "fcstd product side entry count",
        )?));
    }
    let Some(entry) = property.side_entries().first() else {
        return Ok(None);
    };
    let Some(view) = entries.get(entry).copied() else {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "{property_id} references missing {entry}",
                property_id = property.id
            ),
            "fcstd product missing side entry",
        )?));
    };
    Ok(Some(view))
}

fn single_link<'a>(
    ctx: &DecodeContext<'_>,
    property: &'a PropertyRecord,
    expected_type: &str,
    root: &str,
    name: &str,
) -> Result<Option<&'a crate::native::LinkTarget>, CodecError> {
    require_root(ctx, property, expected_type, name, root)?;
    if property.links().len() != 1 {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} requires one {name} target, found {}",
                property.id,
                property.links().len()
            ),
            "fcstd product link target count",
        )?));
    }
    Ok(property.links()[0].as_ref())
}

fn link_list<'a>(
    ctx: &DecodeContext<'_>,
    property: &'a PropertyRecord,
    expected_type: &str,
    name: &str,
) -> Result<&'a [Option<crate::native::LinkTarget>], CodecError> {
    require_root(ctx, property, expected_type, name, "LinkList")?;
    if property
        .values()
        .iter()
        .skip(1)
        .any(|value| value.tag != "Link")
    {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} has a non-Link child in {name}",
                property.id
            ),
            "fcstd product link list child",
        )?));
    }
    Ok(property.links())
}

fn require_root(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_type: &str,
    name: &str,
    root: &str,
) -> Result<(), CodecError> {
    if property.type_name != expected_type {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} has runtime type {}, expected {expected_type} for {name}",
                property.id, property.type_name
            ),
            "fcstd product runtime type",
        )?));
    }
    if property.values().first().map(|value| value.tag.as_str()) != Some(root)
        || property
            .values()
            .iter()
            .filter(|value| value.tag == root)
            .count()
            != 1
    {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} requires one {root} value for {name}",
                property.id
            ),
            "fcstd product root value",
        )?));
    }
    Ok(())
}

fn single_value<'a>(
    ctx: &DecodeContext<'_>,
    property: &'a PropertyRecord,
    expected_type: &str,
    name: &str,
    root: &str,
) -> Result<&'a crate::native::ValueRecord, CodecError> {
    require_root(ctx, property, expected_type, name, root)?;
    if property.values().len() != 1 {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} has multiple values for {name}",
                property.id
            ),
            "fcstd product value count",
        )?));
    }
    Ok(&property.values()[0])
}

fn selected_placement<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let link_placement = sole_named_property(ctx, "product", properties, "LinkPlacement")?;
    let placement = sole_named_property(ctx, "product", properties, "Placement")?;
    for property in [link_placement, placement].into_iter().flatten() {
        placement_matrix(ctx, property)?;
    }
    match (link_placement, placement) {
        (Some(link_placement), Some(placement)) => {
            let use_link_placement =
                bool_property(ctx, properties, "LinkTransform")?.ok_or_else(|| {
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
) -> Result<impl ExactSizeIterator<Item = [RealPosition<'a>; N]>, CodecError> {
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

fn metadata_string(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<String>, CodecError> {
    let Some(property) = properties.iter().find(|property| property.name == name) else {
        return Ok(None);
    };
    if property.type_name != "App::PropertyString" {
        return Ok(None);
    }
    let Ok(document) = roxmltree::Document::parse(property.xml.text()) else {
        return Ok(None);
    };
    let root = document.root_element();
    if !root.has_tag_name("Property") {
        return Ok(None);
    }
    let mut values = root.children().filter(roxmltree::Node::is_element);
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some()
        || !value.has_tag_name("String")
        || value.children().any(|node| node.is_element())
    {
        return Ok(None);
    }
    value
        .attribute("value")
        .map(|value| retained_string(ctx, value, "fcstd product metadata"))
        .transpose()
}

fn bool_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<bool>, CodecError> {
    let Some(property) = sole_named_property(ctx, "product", properties, name)? else {
        return Ok(None);
    };
    let value = single_value(ctx, property, "App::PropertyBool", name, "Bool")?;
    let Some(value) = value.attributes.get("value") else {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("product property {} has no Bool value", property.id),
            "fcstd product missing boolean",
        )?));
    };
    let Some(value) = parse_bool(value) else {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("product property {} has an invalid Bool value", property.id),
            "fcstd product invalid boolean",
        )?));
    };
    Ok(Some(value))
}

fn integer_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<i64>, CodecError> {
    let Some(property) = sole_named_property(ctx, "product", properties, name)? else {
        return Ok(None);
    };
    let value = single_value(
        ctx,
        property,
        "App::PropertyIntegerConstraint",
        name,
        "Integer",
    )?;
    let Some(value) = value.attributes.get("value") else {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("product property {} has no Integer value", property.id),
            "fcstd product missing integer",
        )?));
    };
    match value.parse() {
        Ok(value) => Ok(Some(value)),
        Err(_) => Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} has an invalid Integer value",
                property.id
            ),
            "fcstd product invalid integer",
        )?)),
    }
}

fn copy_on_change_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<NativeCopyOnChangePolicy>, CodecError> {
    let Some(property) = sole_named_property(ctx, "product", properties, "LinkCopyOnChange")?
    else {
        return Ok(None);
    };
    let value = single_value(
        ctx,
        property,
        "App::PropertyEnumeration",
        "LinkCopyOnChange",
        "Integer",
    )?;
    let Some(raw) = value.attributes.get("value") else {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("product property {} has no enumeration value", property.id),
            "fcstd product missing enumeration",
        )?));
    };
    NativeCopyOnChangePolicy::from_raw(retained_string(ctx, raw, "fcstd copy on change policy")?)
        .map(Some)
        .or_else(|_| {
            Err(CodecError::Malformed(retained_format(
                ctx,
                format_args!(
                    "product property {} has an invalid enumeration Integer value",
                    property.id
                ),
                "fcstd product invalid enumeration",
            )?))
        })
}

fn linked_target(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    expected_type: &str,
    root: &str,
) -> Result<Option<crate::native::LinkTarget>, CodecError> {
    let Some(property) = sole_named_property(ctx, "product", properties, name)? else {
        return Ok(None);
    };
    let link = single_link(ctx, property, expected_type, root, name)?;
    link.map(|link| link.clone_with_context(ctx)).transpose()
}

fn neutral_link_target(
    ctx: &DecodeContext<'_>,
    target: &crate::native::LinkTarget,
) -> Result<Option<cadmpeg_ir::products::PrototypeReference>, CodecError> {
    if let Some(document) = target.document() {
        return Ok(Some(cadmpeg_ir::products::PrototypeReference::External {
            document: external_document_reference_charged(
                ctx,
                document.as_str(),
                document.attribute(),
            )?,
            object: target
                .object()
                .map(|name| retained_string(ctx, name, "fcstd product external target"))
                .transpose()?,
        }));
    }
    let Some(object) = target.object() else {
        return Ok(target
            .subelements()
            .iter()
            .any(|subelement| !subelement.is_empty())
            .then_some(PrototypeReference::Unresolved {}));
    };
    Ok(Some(cadmpeg_ir::products::PrototypeReference::Local {
        definition: ProductDefinitionId::mint(crate::native::model_id_charged(
            ctx,
            "product_definition",
            object,
            "definition",
        )?)
        .map_err(CodecError::malformed)?,
    }))
}

fn scale_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<Option<FiniteVector<3>>, CodecError> {
    if let Some(property) = sole_named_property(ctx, "product", properties, "ScaleVector")? {
        return vector_property(ctx, property).map(Some);
    }
    let Some(property) = sole_named_property(ctx, "product", properties, "Scale")? else {
        return Ok(None);
    };
    let value = single_value(ctx, property, "App::PropertyFloat", "Scale", "Float")?;
    let Some(value) = value.attributes.get("value") else {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("product property {} has no Float value", property.id),
            "fcstd product missing scale",
        )?));
    };
    let value = parse_finite(ctx, value, property, "Scale")?;
    Ok(Some([value; 3].into()))
}

fn vector_property(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<FiniteVector<3>, CodecError> {
    let value = single_value(
        ctx,
        property,
        "App::PropertyVector",
        "ScaleVector",
        "PropertyVector",
    )?;
    let component = |name: &str| {
        let Some(value) = value.attributes.get(name) else {
            return Err(CodecError::Malformed(retained_format(
                ctx,
                format_args!(
                    "product property {} has no {name} vector component",
                    property.id
                ),
                "fcstd product missing scale component",
            )?));
        };
        parse_finite(ctx, value, property, "ScaleVector")
    };
    Ok([
        component("valueX")?,
        component("valueY")?,
        component("valueZ")?,
    ]
    .into())
}

fn parse_finite(
    ctx: &DecodeContext<'_>,
    value: &str,
    property: &PropertyRecord,
    name: &str,
) -> Result<FiniteReal, CodecError> {
    let Some(value) = value.parse::<f64>().ok().and_then(FiniteReal::new) else {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} has an invalid finite value for {name}",
                property.id
            ),
            "fcstd product invalid finite scale",
        )?));
    };
    Ok(value)
}

fn bool_list(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Vec<bool>, CodecError> {
    let Some(property) = sole_named_property(ctx, "product", properties, name)? else {
        return Ok(Vec::new());
    };
    let value = single_value(ctx, property, "App::PropertyBoolList", name, "BoolList")?;
    let Some(encoded) = value.attributes.get("value") else {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("product property {} has no BoolList value", property.id),
            "fcstd product missing visibility list",
        )?));
    };
    if encoded.bytes().any(|byte| !matches!(byte, b'0' | b'1')) {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "product property {} has an invalid BoolList bit string",
                property.id
            ),
            "fcstd product invalid visibility list",
        )?));
    }
    // FreeCAD writes the most-significant bit first: the rightmost source bit
    // belongs to element zero. The raw XML remains on the property record;
    // this projection follows the element order used by the other carriers.
    let mut values = collection_vec(ctx, encoded.len(), "fcstd product visibility list")?;
    values.extend(encoded.bytes().rev().map(|byte| byte == b'1'));
    Ok(values)
}

pub(crate) fn product_cycle_nodes<'a>(
    ctx: &DecodeContext<'_>,
    nodes: &HashMap<&'a str, &'a ProductNodeRecord>,
) -> Result<HashSet<&'a str>, CodecError> {
    let edges = |name: &'a str| {
        nodes.get(name).into_iter().flat_map(|node| {
            node.members()
                .iter()
                .map(String::as_str)
                .chain(node.prototype())
                .filter(|target| nodes.contains_key(target))
        })
    };
    let collect_edges = |name: &'a str| -> Result<Vec<&'a str>, CodecError> {
        let mut targets = Vec::new();
        for target in edges(name) {
            ctx.charge_work(1, "fcstd product cycle edge")?;
            reserve_vec_items(ctx, &mut targets, 1, "fcstd product cycle targets")?;
            targets.push(target);
        }
        Ok(targets)
    };
    let mut reverse = HashMap::<&str, Vec<&str>>::new();
    ctx.charge_collection_items(nodes.len() as u64, "fcstd product reverse graph")?;
    reverse.try_reserve(nodes.len()).map_err(|_| {
        collection_allocation_failed(ctx, nodes.len() as u64, "fcstd product reverse graph")
    })?;
    for &source in nodes.keys() {
        reverse.insert(source, Vec::new());
    }
    for &source in nodes.keys() {
        for target in edges(source) {
            ctx.charge_work(1, "fcstd product reverse edge")?;
            if let Some(sources) = reverse.get_mut(target) {
                reserve_vec_items(ctx, sources, 1, "fcstd product reverse sources")?;
                sources.push(source);
            }
        }
    }

    let mut visited = HashSet::new();
    let mut finish = collection_vec(ctx, nodes.len(), "fcstd product finish order")?;
    for &root in nodes.keys() {
        if !crate::resource::insert_hash_set(
            ctx,
            &mut visited,
            root,
            "fcstd product visited nodes",
        )? {
            continue;
        }
        let mut stack = Vec::new();
        reserve_vec_items(ctx, &mut stack, 1, "fcstd product forward stack")?;
        stack.push((root, collect_edges(root)?, 0_usize));
        while let Some((current, targets, next)) = stack.last_mut() {
            ctx.charge_work(1, "fcstd product forward traversal")?;
            if let Some(&target) = targets.get(*next) {
                *next += 1;
                if crate::resource::insert_hash_set(
                    ctx,
                    &mut visited,
                    target,
                    "fcstd product visited nodes",
                )? {
                    reserve_vec_items(ctx, &mut stack, 1, "fcstd product forward stack")?;
                    stack.push((target, collect_edges(target)?, 0));
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
        if !crate::resource::insert_hash_set(
            ctx,
            &mut assigned,
            root,
            "fcstd product assigned nodes",
        )? {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = Vec::new();
        reserve_vec_items(ctx, &mut stack, 1, "fcstd product reverse stack")?;
        stack.push(root);
        while let Some(current) = stack.pop() {
            ctx.charge_work(1, "fcstd product reverse traversal")?;
            reserve_vec_items(ctx, &mut component, 1, "fcstd product component nodes")?;
            component.push(current);
            for &source in reverse.get(current).into_iter().flatten() {
                if crate::resource::insert_hash_set(
                    ctx,
                    &mut assigned,
                    source,
                    "fcstd product assigned nodes",
                )? {
                    reserve_vec_items(ctx, &mut stack, 1, "fcstd product reverse stack")?;
                    stack.push(source);
                }
            }
        }
        let self_cycle = component.len() == 1 && edges(component[0]).any(|target| target == root);
        if component.len() > 1 || self_cycle {
            for member in component {
                crate::resource::insert_hash_set(
                    ctx,
                    &mut cyclic,
                    member,
                    "fcstd product cyclic nodes",
                )?;
            }
        }
    }
    Ok(cyclic)
}

#[cfg(test)]
pub(crate) mod tests;
