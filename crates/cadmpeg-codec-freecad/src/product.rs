// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use crate::native::frame::FiniteFrame;
use crate::native::joint::JointRecord;
use crate::placement::{placement_components, placement_matrix};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroUsize;

use crate::brep::ShapePayloadRecord;
use crate::layout::link_array_side_entry_header as link_array;
use crate::native::{
    malformed, parse_bool, sole_named_property, ContainerNode,
    CopyOnChangePolicy as NativeCopyOnChangePolicy, LinkArrayCardinality, LinkOccurrence,
    ObjectRecord, ProductNode, ProductNodeRecord, PropertyRecord,
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut owner_index = None;
    let mut output = Vec::new();
    let mut object_visits = objects.iter();
    while object_visits.len() != 0 {
        let Some(object) = ctx.next_charged(&mut object_visits, "fcstd product object records")?
        else {
            break;
        };
        let Some(kind) = product_kind(&object.type_name) else {
            continue;
        };
        let owner_index = match &mut owner_index {
            Some(index) => index,
            empty @ None => empty.insert(
                ctx.collect_scoped_btree_groups(
                    properties
                        .iter()
                        .map(|property| (property.owner.as_str(), property)),
                    "fcstd product owner index",
                )?,
            ),
        };
        let owned = ctx
            .get_btree_map(
                &owner_index.0,
                object.id().as_str(),
                "fcstd product owner lookup",
            )?
            .map_or(&[][..], Vec::as_slice);
        let group = sole_named_property(ctx, "product", owned, "Group")?;
        let members = group
            .map(|property| {
                linked_object_names(
                    ctx,
                    link_list(ctx, property, "App::PropertyLinkList", "Group")?,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let linked = sole_named_property(ctx, "product", owned, "LinkedObject")?;
        let prototype_link = linked
            .map(|property| {
                single_link(ctx, property, "App::PropertyXLink", "XLink", "LinkedObject")
            })
            .transpose()?
            .flatten();
        let placement = selected_placement(ctx, owned)?;
        let local_transform = placement.and_then(|(_, transform)| transform);
        let link_transform = bool_property(ctx, owned, "LinkTransform")?;
        let element_count = integer_property(ctx, owned, "ElementCount")?
            .map(u64::try_from)
            .transpose()
            .map_err(|_| malformed("negative ElementCount"))?;
        let claim_child = bool_property(ctx, owned, "LinkClaimChild")?;
        let copy_on_change = copy_on_change_property(ctx, owned)?;
        let copy_on_change_source = linked_target(
            ctx,
            owned,
            "LinkCopyOnChangeSource",
            "App::PropertyXLink",
            "XLink",
        )?;
        let copy_on_change_group = linked_target(
            ctx,
            owned,
            "LinkCopyOnChangeGroup",
            "App::PropertyLink",
            "Link",
        )?;
        let copy_on_change_touched = bool_property(ctx, owned, "LinkCopyOnChangeTouched")?;
        let scale = scale_property(ctx, owned)?;
        let element_visibility = bool_list(ctx, owned, "VisibilityList")?;
        let element_objects = sole_named_property(ctx, "product", owned, "ElementList")?
            .map(|property| {
                linked_object_names(
                    ctx,
                    link_list(ctx, property, "App::PropertyLinkList", "ElementList")?,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let placement_property = placement
            .map(|(property, _)| {
                ctx.copy_retained_text(&property.id, "fcstd product placement property")
            })
            .transpose()?;
        let node = match kind {
            ProductKind::Occurrence => ProductNode::Occurrence(Box::new(LinkOccurrence {
                members,
                prototype: prototype_link
                    .and_then(|link| link.object())
                    .map(|name| ctx.copy_retained_text(name, "fcstd product prototype"))
                    .transpose()?,
                external_document: prototype_link
                    .and_then(|link| link.document())
                    .map(|document| document.clone_with_context(ctx))
                    .transpose()?,
                local_transform,
                placement_property,
                array: crate::native::LinkArray::try_new(
                    element_count,
                    parse_placement_list(ctx, owned, entries)?,
                    parse_vector_list(ctx, owned, entries)?,
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
            })),
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
        ctx.reserve_vec(&mut output, 1, "fcstd product records")?;
        output.push(ProductNodeRecord {
            id: crate::native::native_id_charged(ctx, "product", object.name())?,
            object: ctx.copy_retained_text(object.id(), "fcstd product object")?,
            node,
        });
    }
    Ok(output)
}

fn linked_object_names(
    ctx: &DecodeContext<'_>,
    links: &[Option<crate::native::LinkTarget>],
) -> Result<Vec<String>, CodecError> {
    let mut names = Vec::new();
    ctx.charge_work(0, "fcstd product linked objects")?;
    let mut link_visits = links.iter();
    while link_visits.len() != 0 {
        let Some(link) = ctx.next_charged(&mut link_visits, "fcstd product linked objects")? else {
            break;
        };
        let Some(link) = link else {
            continue;
        };
        if let Some(name) = link.object() {
            ctx.push_vec(
                &mut names,
                ctx.copy_retained_text(name, "fcstd product linked object name")?,
                "fcstd product linked object names",
            )?;
        }
    }
    Ok(names)
}

fn nonempty_subelements(
    ctx: &DecodeContext<'_>,
    values: &[String],
) -> Result<Vec<String>, CodecError> {
    ctx.charge_work(0, "fcstd product subelements")?;
    let mut subelements = Vec::new();
    let mut value_visits = values.iter();
    while value_visits.len() != 0 {
        let Some(value) = ctx.next_charged(&mut value_visits, "fcstd product subelements")? else {
            break;
        };
        if !value.is_empty() {
            ctx.push_vec(
                &mut subelements,
                ctx.copy_retained_text(value, "fcstd product linked subelement")?,
                "fcstd product linked subelements",
            )?;
        }
    }
    Ok(subelements)
}

fn product_record_index<'a>(
    ctx: &DecodeContext<'_>,
    records: &'a [ProductNodeRecord],
) -> Result<HashMap<&'a str, &'a ProductNodeRecord>, CodecError> {
    ctx.charge_work(0, "fcstd product record index")?;
    let mut index = HashMap::new();
    let mut record_visits = records.iter();
    while record_visits.len() != 0 {
        let Some(record) = ctx.next_charged(&mut record_visits, "fcstd product record index")?
        else {
            break;
        };
        if ctx
            .insert_hash_map(
                &mut index,
                record.object.as_str(),
                record,
                "fcstd product record index",
            )?
            .is_some()
        {
            return Err(CodecError::Malformed(ctx.format_retained(
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut storage = ctx.reserve_scoped(0, "fcstd neutral product lookups")?;
    let record_by_object = storage.with_storage(|| product_record_index(ctx, records))?;
    let mut component_objects = Vec::new();
    let mut occurrence_objects = HashSet::new();
    let mut has_container_record = false;
    let mut record_visits = records.iter();
    while record_visits.len() != 0 {
        let Some(record) =
            ctx.next_charged(&mut record_visits, "fcstd product component records")?
        else {
            break;
        };
        if matches!(record.node, ProductNode::Occurrence(_)) {
            storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut occurrence_objects,
                    record.object.as_str(),
                    "fcstd product occurrence names",
                )
            })?;
        } else {
            has_container_record = true;
            storage.with_storage(|| {
                ctx.reserve_vec(&mut component_objects, 1, "fcstd product component names")
            })?;
            component_objects.push(record.object.as_str());
        }
    }
    let mut record_visits = records.iter();
    while record_visits.len() != 0 {
        let Some(record) =
            ctx.next_charged(&mut record_visits, "fcstd product component records")?
        else {
            break;
        };
        let mut member_visits = record.members().iter();
        while member_visits.len() != 0 {
            let Some(member) =
                ctx.next_charged(&mut member_visits, "fcstd product component members")?
            else {
                break;
            };
            if ctx.contains_hash_set(
                &occurrence_objects,
                member.as_str(),
                "fcstd product occurrence lookup",
            )? {
                continue;
            }
            storage.with_storage(|| {
                ctx.reserve_vec(&mut component_objects, 1, "fcstd product component names")
            })?;
            component_objects.push(member.as_str());
        }
        if record.external_document().is_none() {
            if let Some(prototype) = record.prototype() {
                storage.with_storage(|| {
                    ctx.reserve_vec(&mut component_objects, 1, "fcstd product component names")
                })?;
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
                    storage.with_storage(|| {
                        ctx.reserve_vec(&mut component_objects, 1, "fcstd product component names")
                    })?;
                    component_objects.push(name);
                }
            }
        }
        let mut element_visits = record.element_objects().iter();
        while element_visits.len() != 0 {
            let Some(name) =
                ctx.next_charged(&mut element_visits, "fcstd product element objects")?
            else {
                break;
            };
            storage.with_storage(|| {
                ctx.reserve_vec(&mut component_objects, 1, "fcstd product component names")
            })?;
            component_objects.push(name);
        }
    }
    let mut joint_visits = joints.iter();
    while joint_visits.len() != 0 {
        let Some(joint) = ctx.next_charged(&mut joint_visits, "fcstd product joint records")?
        else {
            break;
        };
        for reference in joint.references() {
            if reference.document().is_none() {
                if let Some(name) = reference.object() {
                    if ctx.contains_hash_set(
                        &occurrence_objects,
                        name,
                        "fcstd product occurrence lookup",
                    )? {
                        continue;
                    }
                    storage.with_storage(|| {
                        ctx.reserve_vec(&mut component_objects, 1, "fcstd product component names")
                    })?;
                    component_objects.push(name);
                }
            }
        }
    }
    ctx.stable_sort_by(
        &mut component_objects,
        |value| value,
        Ord::cmp,
        "fcstd product component name sort",
    )?;
    ctx.dedup_vec(&mut component_objects, "fcstd product component name dedup")?;

    let owner_index = ctx.collect_scoped_btree_groups(
        properties
            .iter()
            .map(|property| (property.owner.as_str(), property)),
        "fcstd product neutral owner index",
    )?;
    let _owner_storage = owner_index.1;
    let properties_by_owner = owner_index.0;
    let mut placements_by_object = BTreeMap::new();
    let mut owner_visits = properties_by_owner.iter();
    while owner_visits.len() != 0 {
        let Some((&owner, owned)) =
            ctx.next_charged(&mut owner_visits, "fcstd product placement owners")?
        else {
            break;
        };
        if let Some((_, Some(placement))) = selected_placement(ctx, owned)? {
            // Keep validating every placement, but retain transforms only when
            // a local component or prototype can query this index below.
            if !component_objects.is_empty() {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut placements_by_object,
                        owner,
                        placement.transform(),
                        "fcstd product placements",
                    )
                })?;
            }
        }
    }

    if records.is_empty() && component_objects.is_empty() {
        return Ok((Vec::new(), Vec::new()));
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
    if has_container_record {
        let mut record_visits = records.iter();
        while record_visits.len() != 0 {
            let Some(record) =
                ctx.next_charged(&mut record_visits, "fcstd product projection records")?
            else {
                break;
            };
            if matches!(record.node, ProductNode::Occurrence(_)) {
                continue;
            }
            let mut member_visits = record.members().iter();
            while member_visits.len() != 0 {
                let Some(member) =
                    ctx.next_charged(&mut member_visits, "fcstd product parent members")?
                else {
                    break;
                };
                let member = member.as_str();
                match ctx.get_hash_map(&parent_by_object, member, "fcstd product parent lookup")? {
                    None => {
                        storage.with_storage(|| {
                            ctx.insert_hash_map(
                                &mut parent_by_object,
                                member,
                                record.object.as_str(),
                                "fcstd product parent index",
                            )
                        })?;
                    }
                    Some(previous)
                        if !ctx.equal(
                            *previous,
                            record.object.as_str(),
                            "fcstd product parent comparison",
                        )? =>
                    {
                        return Err(CodecError::Malformed(ctx.format_retained(
                            format_args!("product member {member} has multiple parent containers"),
                            "fcstd product parent conflict",
                        )?));
                    }
                    Some(_) => {}
                }
            }
        }
    }

    let mut occurrences = Vec::new();
    let mut record_visits = records.iter();
    while record_visits.len() != 0 {
        let Some(record) =
            ctx.next_charged(&mut record_visits, "fcstd product projection records")?
        else {
            break;
        };
        if !matches!(record.node, ProductNode::Occurrence(_)) {
            continue;
        }
        let count = occurrence_count(ctx, record)?.get();
        let parent = ctx
            .get_hash_map(
                &parent_by_object,
                record.object.as_str(),
                "fcstd product parent lookup",
            )?
            .copied();
        let prototype_transform = storage.with_storage(|| {
            linked_prototype_transform(
                ctx,
                record,
                &record_by_object,
                &placements_by_object,
                &mut Vec::new(),
            )
        })?;
        let mut element_visits = 0..count;
        while element_visits.len() != 0 {
            let Some(index) =
                ctx.next_charged(&mut element_visits, "fcstd product occurrence elements")?
            else {
                break;
            };
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
                let ordinal_result =
                    ctx.format_scoped(format_args!("{index}"), "fcstd product occurrence ordinal")?;
                let ordinal_storage = ordinal_result.1;
                let ordinal = ordinal_result.0;
                let id =
                    crate::native::model_id_charged(ctx, "occurrence", &record.object, &ordinal)?;
                drop((ordinal, ordinal_storage));
                id
            } else {
                crate::native::model_id_charged(ctx, "occurrence", &record.object, "instance")?
            };
            ctx.reserve_vec(&mut occurrences, 1, "fcstd product occurrences")?;
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
                                ctx.copy_retained_text(value, "fcstd product external prototype")
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
                    .map(|object| {
                        container_occurrence_id(object)
                            .map(|occurrence| OccurrenceParent::Occurrence { occurrence })
                    })
                    .transpose()?
                    .unwrap_or(OccurrenceParent::Root {}),
                ordinal: 0,
                transform: local_transform,
                linked_prototype: (record.link_transform() == Some(true))
                    .then_some(prototype_transform),
                scale,
                name: Some(
                    ctx.copy_retained_text(&record.object, "fcstd product occurrence name")?,
                ),
                visible: None,
                link: LinkState::new(
                    ctx.copy_retained_strings(
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
                native_ref: Some(ctx.copy_retained_text(
                    &record.object,
                    "fcstd product occurrence native reference",
                )?),
            });
        }
    }

    let mut object_by_id = HashMap::new();
    if !component_objects.is_empty() {
        let mut object_visits = objects.iter();
        while object_visits.len() != 0 {
            let Some(object) =
                ctx.next_charged(&mut object_visits, "fcstd product source objects")?
            else {
                break;
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut object_by_id,
                    object.id().as_str(),
                    object,
                    "fcstd product object index",
                )
            })?;
        }
    }
    let mut body_storage = ctx.reserve_scoped(0, "fcstd product owner bodies")?;
    let mut bodies_by_owner = BTreeMap::new();
    if !component_objects.is_empty() && !bodies.is_empty() {
        let mut body_lookup_storage = ctx.reserve_scoped(0, "fcstd product body owner lookups")?;
        let mut property_owner = HashMap::new();
        let mut property_visits = properties.iter();
        while property_visits.len() != 0 {
            let Some(property) =
                ctx.next_charged(&mut property_visits, "fcstd product source properties")?
            else {
                break;
            };
            body_lookup_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut property_owner,
                    property.id.as_str(),
                    property.owner.as_str(),
                    "fcstd product property owners",
                )
            })?;
        }
        let mut body_owners = BTreeMap::new();
        let mut payload_visits = payloads.iter();
        while payload_visits.len() != 0 {
            let Some(payload) =
                ctx.next_charged(&mut payload_visits, "fcstd product shape payloads")?
            else {
                break;
            };
            if let Some(owner) = ctx.get_hash_map(
                &property_owner,
                payload.property.as_str(),
                "fcstd product payload owner",
            )? {
                let prefix = body_lookup_storage.with_storage(|| {
                    crate::native::model_id_charged(ctx, "body", &payload.id, "")
                })?;
                body_lookup_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut body_owners,
                        prefix,
                        *owner,
                        "fcstd product body owners",
                    )
                })?;
            }
        }
        let mut body_visits = bodies.iter();
        while body_visits.len() != 0 {
            let Some(body) = ctx.next_charged(&mut body_visits, "fcstd product source bodies")?
            else {
                break;
            };
            // The child label is one encoded segment after the payload key.
            let Some(separator) = ctx.rposition_by(
                body.id.as_str().as_bytes(),
                |byte| Ok(*byte == b':'),
                "fcstd product body payload prefix",
            )?
            else {
                continue;
            };
            let prefix = &body.id.as_str()[..=separator];
            if let Some(&owner) =
                ctx.get_btree_map(&body_owners, prefix, "fcstd product body owner lookup")?
            {
                body_storage.with_storage(|| {
                    ctx.push_btree_group(
                        &mut bodies_by_owner,
                        owner,
                        body,
                        "fcstd product body owner index",
                        "fcstd product owner bodies",
                    )
                })?;
            }
        }
    }
    let mut definitions =
        ctx.collection_vec(component_objects.len(), "fcstd product definitions")?;
    let mut definition_visits = component_objects.iter();
    while definition_visits.len() != 0 {
        let Some(&object) =
            ctx.next_charged(&mut definition_visits, "fcstd product definition objects")?
        else {
            break;
        };
        let record = ctx
            .get_hash_map(&record_by_object, object, "fcstd product record lookup")?
            .copied();
        let kind = match record.map(|record| &record.node) {
            Some(ProductNode::Part(_)) => ProductDefinitionKind::Part,
            Some(ProductNode::Group(_)) => ProductDefinitionKind::Group,
            Some(ProductNode::LinkGroup { .. }) => ProductDefinitionKind::LinkGroup,
            _ => ProductDefinitionKind::Object,
        };
        let source_object = ctx
            .get_hash_map(&object_by_id, object, "fcstd product source object lookup")?
            .copied();
        let owned = ctx
            .get_btree_map(&properties_by_owner, object, "fcstd product owner lookup")?
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
                ctx.insert_btree_map(
                    &mut bom_properties,
                    name,
                    value,
                    "fcstd product BOM properties",
                )?;
            }
        }
        let mut part_number =
            metadata_string(ctx, owned, "PartNumber")?.filter(|value| !value.is_empty());
        if part_number.is_none()
            && source_object.is_some_and(|object| {
                matches!(
                    object.type_name.as_str(),
                    "Assembly::AssemblyObject" | "Assembly::AssemblyLink" | "App::Part"
                )
            })
        {
            part_number = metadata_string(ctx, owned, "Id")?.filter(|value| !value.is_empty());
        }
        let mut definition_bodies = Vec::new();
        if let Some(owned_bodies) = ctx.get_btree_map(
            &bodies_by_owner,
            object,
            "fcstd product definition body lookup",
        )? {
            let mut body_visits = owned_bodies.iter();
            while body_visits.len() != 0 {
                let Some(body) =
                    ctx.next_charged(&mut body_visits, "fcstd product definition body records")?
                else {
                    break;
                };
                ctx.push_vec(
                    &mut definition_bodies,
                    body.id
                        .try_clone_for_decode(ctx, "fcstd product body identity")?,
                    "fcstd product definition bodies",
                )?;
            }
        }
        definitions.push(ProductDefinition {
            id: definition_id(object)?,
            kind,
            source_name: source_object
                .map(|object| ctx.copy_retained_text(object.name(), "fcstd product source name"))
                .transpose()?,
            label: metadata_string(ctx, owned, "Label")?,
            description: metadata_string(ctx, owned, "Description")?,
            part_number,
            bom_properties,
            bodies: definition_bodies,
            native_ref: Some(
                ctx.copy_retained_text(object, "fcstd product definition native reference")?,
            ),
        });
    }

    let mut container_visits = component_objects.iter();
    while container_visits.len() != 0 {
        let Some(object) =
            ctx.next_charged(&mut container_visits, "fcstd product container objects")?
        else {
            break;
        };
        let record = ctx
            .get_hash_map(&record_by_object, *object, "fcstd product record lookup")?
            .copied();
        let local_transform = match record.and_then(ProductNodeRecord::local_transform) {
            Some(placement) => placement.transform(),
            None => ctx
                .get_btree_map(
                    &placements_by_object,
                    *object,
                    "fcstd product placement lookup",
                )?
                .copied()
                .unwrap_or_default(),
        };
        let parent = ctx
            .get_hash_map(&parent_by_object, *object, "fcstd product parent lookup")?
            .copied();
        let parent = match parent {
            Some(parent) => OccurrenceParent::Occurrence {
                occurrence: container_occurrence_id(parent)?,
            },
            None => OccurrenceParent::Root {},
        };
        ctx.reserve_vec(&mut occurrences, 1, "fcstd product occurrences")?;
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
            name: Some(ctx.copy_retained_text(object, "fcstd product container name")?),
            visible: None,
            link: None,
            native_ref: Some(
                ctx.copy_retained_text(object, "fcstd product container native reference")?,
            ),
        });
    }
    let mut next_ordinal = HashMap::<Option<&str>, u32>::new();
    let mut ordinal_visits = occurrences.iter_mut();
    while ordinal_visits.len() != 0 {
        let Some(occurrence) =
            ctx.next_charged(&mut ordinal_visits, "fcstd product ordinal occurrences")?
        else {
            break;
        };
        let parent = match &occurrence.parent {
            OccurrenceParent::Root {} => None,
            OccurrenceParent::Occurrence { occurrence } => Some(occurrence.as_str()),
        };
        let ordinal = ctx
            .get_hash_map(&next_ordinal, &parent, "fcstd product ordinal lookup")?
            .copied()
            .unwrap_or_default();
        occurrence.ordinal = ordinal;
        let next = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("product occurrence ordinal exceeds u32"))?;
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut next_ordinal,
                parent,
                next,
                "fcstd product ordinal index",
            )
        })?;
    }
    Ok((definitions, occurrences))
}

fn linked_prototype_transform<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a ProductNodeRecord,
    records: &HashMap<&str, &'a ProductNodeRecord>,
    placements: &BTreeMap<&str, Transform>,
    stack: &mut Vec<&'a str>,
) -> Result<Transform, CodecError> {
    let _depth = ctx.enter_nested("resolve FCStd nested link transform")?;
    if record.link_transform() != Some(true) || record.external_document().is_some() {
        return Ok(Transform::identity());
    }
    let Some(prototype) = record.prototype() else {
        return Ok(Transform::identity());
    };
    if ctx.any_by(
        stack.iter(),
        |object| {
            ctx.equal(
                *object,
                record.object.as_str(),
                "fcstd nested product identity comparison",
            )
        },
        "fcstd nested product stack search",
    )? {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("nested link cycle reaches {}", record.object),
            "fcstd nested product cycle",
        )?));
    }
    ctx.reserve_vec(stack, 1, "fcstd nested product stack")?;
    stack.push(record.object.as_str());
    let target_record = ctx
        .get_hash_map(records, prototype, "fcstd nested product record lookup")?
        .copied();
    let placement = match target_record.and_then(ProductNodeRecord::local_transform) {
        Some(placement) => placement.transform(),
        None => ctx
            .get_btree_map(
                placements,
                prototype,
                "fcstd nested product placement lookup",
            )?
            .copied()
            .unwrap_or_default(),
    };
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
        Err(CodecError::Malformed(ctx.format_retained(
            format_args!("{} element count exceeds addressable size", record.id),
            "fcstd product element count",
        )?))
    })?;
    if count.get() > 1_000_000 || u32::try_from(count.get()).is_err() {
        return Err(CodecError::Malformed(ctx.format_retained(
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
        _ => CopyOnChangePolicy::Native(
            ctx.copy_retained_text(value.as_str(), "fcstd product copy on change")?,
        ),
    })
}

pub(crate) fn external_document_reference_charged(
    ctx: &DecodeContext<'_>,
    value: &str,
    attribute: Option<&str>,
) -> Result<ExternalDocument, CodecError> {
    let value = ctx.copy_retained_text(value, "fcstd external document reference")?;
    Ok(
        if attribute.is_some_and(|name| name.eq_ignore_ascii_case("file")) {
            ExternalDocument::path(ctx, value)?
        } else {
            ExternalDocument::document_id(ctx, value)?
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
    let mut positions = list_layout::<7>(view, "PlacementList")?;
    let mut placements = ctx.collection_vec(positions.len(), "fcstd product placement list")?;
    while positions.len() != 0 {
        let Some(positions) =
            ctx.next_charged(&mut positions, "fcstd product placement positions")?
        else {
            break;
        };
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
    let mut positions = list_layout::<3>(view, "ScaleList")?;
    let mut vectors = ctx.collection_vec(positions.len(), "fcstd product scale list")?;
    while positions.len() != 0 {
        let Some(positions) = ctx.next_charged(&mut positions, "fcstd product scale positions")?
        else {
            break;
        };
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
        return Err(CodecError::Malformed(ctx.format_retained(
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
    let Some(view) = ctx
        .get_btree_map(entries, entry, "fcstd product side entry lookup")?
        .copied()
    else {
        return Err(CodecError::Malformed(ctx.format_retained(
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
        return Err(CodecError::Malformed(ctx.format_retained(
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
    if ctx.any_by(
        &property.values()[1..],
        |value| Ok(value.tag != "Link"),
        "fcstd product LinkList children",
    )? {
        return Err(CodecError::Malformed(ctx.format_retained(
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
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "product property {} has runtime type {}, expected {expected_type} for {name}",
                property.id, property.type_name
            ),
            "fcstd product runtime type",
        )?));
    }
    if property.values().first().map(|value| value.tag.as_str()) != Some(root)
        || ctx.any_by(
            &property.values()[1..],
            |value| Ok(value.tag == root),
            "fcstd product duplicate roots",
        )?
    {
        return Err(CodecError::Malformed(ctx.format_retained(
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
        return Err(CodecError::Malformed(ctx.format_retained(
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
) -> Result<Option<(&'a PropertyRecord, Option<FiniteFrame>)>, CodecError> {
    let link_placement = sole_named_property(ctx, "product", properties, "LinkPlacement")?;
    let placement = sole_named_property(ctx, "product", properties, "Placement")?;
    let link_placement = link_placement
        .map(|property| Ok::<_, CodecError>((property, placement_matrix(ctx, property)?)))
        .transpose()?;
    let placement = placement
        .map(|property| Ok::<_, CodecError>((property, placement_matrix(ctx, property)?)))
        .transpose()?;
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
    let Some(property) = ctx.find_by(
        properties.iter(),
        |property| Ok(property.name == name),
        "fcstd product metadata property search",
    )?
    else {
        return Ok(None);
    };
    if property.type_name != "App::PropertyString" {
        return Ok(None);
    }
    let admitted_document = match ctx.parse_xml(property.xml.text(), "FreeCAD XML tree") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(None);
        }
    };
    let document = admitted_document.document();
    let root = ctx.xml_root_element(document, "fcstd product metadata root")?;
    if !ctx.xml_has_tag_name(root, "Property", "fcstd product metadata tag")? {
        return Ok(None);
    }
    let mut values = root.children();
    let Some(value) = ctx.find_by(
        values.by_ref(),
        |node| Ok(node.is_element()),
        "fcstd product metadata children",
    )?
    else {
        return Ok(None);
    };
    if ctx.any_by(
        values,
        |node| Ok(node.is_element()),
        "fcstd product metadata children",
    )? || !ctx.xml_has_tag_name(value, "String", "fcstd product metadata tag")?
        || ctx.any_by(
            value.children(),
            |node| Ok(node.is_element()),
            "fcstd product metadata value children",
        )?
    {
        return Ok(None);
    }
    ctx.xml_attribute(value, "value", "fcstd product metadata attribute")?
        .map(|value| ctx.copy_retained_text(value, "fcstd product metadata"))
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
    let Some(value) =
        ctx.get_btree_map(&value.attributes, "value", "fcstd product value attribute")?
    else {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("product property {} has no Bool value", property.id),
            "fcstd product missing boolean",
        )?));
    };
    let Some(value) = parse_bool(value) else {
        return Err(CodecError::Malformed(ctx.format_retained(
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
    let Some(value) =
        ctx.get_btree_map(&value.attributes, "value", "fcstd product value attribute")?
    else {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("product property {} has no Integer value", property.id),
            "fcstd product missing integer",
        )?));
    };
    match ctx.parse_text::<i64>(value, "fcstd product integer parse")? {
        Ok(value) => Ok(Some(value)),
        Err(_) => Err(CodecError::Malformed(ctx.format_retained(
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
    let Some(raw) =
        ctx.get_btree_map(&value.attributes, "value", "fcstd product value attribute")?
    else {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("product property {} has no enumeration value", property.id),
            "fcstd product missing enumeration",
        )?));
    };
    NativeCopyOnChangePolicy::from_raw_with_admission(
        ctx.copy_retained_text(raw, "fcstd copy on change policy")?,
        |length, operation| {
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(length), operation)
        },
    )?
    .map(Some)
    .or_else(|_| {
        Err(CodecError::Malformed(ctx.format_retained(
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
                .map(|name| ctx.copy_retained_text(name, "fcstd product external target"))
                .transpose()?,
        }));
    }
    let Some(object) = target.object() else {
        return Ok(ctx
            .any_by(
                target.subelements(),
                |subelement| Ok(!subelement.is_empty()),
                "fcstd product unresolved subelements",
            )?
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
    let Some(value) =
        ctx.get_btree_map(&value.attributes, "value", "fcstd product value attribute")?
    else {
        return Err(CodecError::Malformed(ctx.format_retained(
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
        let Some(value) =
            ctx.get_btree_map(&value.attributes, name, "fcstd product vector attribute")?
        else {
            return Err(CodecError::Malformed(ctx.format_retained(
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
    let Some(value) = ctx
        .parse_text::<f64>(value, "fcstd product finite parse")?
        .ok()
        .and_then(FiniteReal::new)
    else {
        return Err(CodecError::Malformed(ctx.format_retained(
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
    let Some(encoded) =
        ctx.get_btree_map(&value.attributes, "value", "fcstd product value attribute")?
    else {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("product property {} has no BoolList value", property.id),
            "fcstd product missing visibility list",
        )?));
    };
    if ctx.any_by(
        encoded.as_bytes(),
        |byte| Ok(!matches!(byte, b'0' | b'1')),
        "fcstd product visibility validation",
    )? {
        return Err(CodecError::Malformed(ctx.format_retained(
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
    let mut values = ctx.collection_vec(encoded.len(), "fcstd product visibility list")?;
    values.extend(
        ctx.admit_iter(encoded.as_bytes(), "fcstd product visibility bits")?
            .rev()
            .map(|byte| *byte == b'1'),
    );
    Ok(values)
}

pub(crate) fn product_cycle_nodes<'a>(
    ctx: &DecodeContext<'_>,
    records: &'a [ProductNodeRecord],
) -> Result<BTreeSet<&'a str>, CodecError> {
    let node_index = ctx.collect_scoped_btree_map(
        records.iter().map(|node| (node.object.as_str(), node)),
        "fcstd product cycle index",
    )?;
    let _node_storage = node_index.1;
    let nodes = node_index.0;
    let mut storage = ctx.reserve_scoped(0, "fcstd product cycle workspace")?;
    let mut forward = BTreeMap::new();
    let mut reverse = BTreeMap::<&str, Vec<&str>>::new();
    let mut node_visits = nodes.iter();
    while node_visits.len() != 0 {
        let Some((&source, _)) =
            ctx.next_charged(&mut node_visits, "fcstd product reverse nodes")?
        else {
            break;
        };
        storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut reverse,
                source,
                Vec::new(),
                "fcstd product reverse graph",
            )
        })?;
    }
    let mut node_visits = nodes.iter();
    while node_visits.len() != 0 {
        let Some((&source, node)) =
            ctx.next_charged(&mut node_visits, "fcstd product cycle nodes")?
        else {
            break;
        };
        let mut targets = Vec::new();
        let mut member_visits = node.members().iter();
        let mut prototype = node
            .prototype()
            .filter(|_| node.external_document().is_none());
        while member_visits.len() != 0 || prototype.is_some() {
            let target = if member_visits.len() != 0 {
                ctx.next_charged(&mut member_visits, "fcstd product cycle members")?
                    .map(String::as_str)
            } else {
                prototype.take()
            };
            let Some(target) = target else {
                break;
            };
            if ctx.contains_key_btree_map(&nodes, target, "fcstd product cycle target lookup")? {
                storage.with_storage(|| {
                    ctx.push_vec(&mut targets, target, "fcstd product cycle targets")
                })?;
                if let Some(sources) =
                    ctx.get_mut_btree_map(&mut reverse, target, "fcstd product reverse lookup")?
                {
                    storage.with_storage(|| {
                        ctx.push_vec(sources, source, "fcstd product reverse sources")
                    })?;
                }
            }
        }
        storage.with_storage(|| {
            ctx.insert_btree_map(&mut forward, source, targets, "fcstd product forward graph")
        })?;
    }

    let mut visited = HashSet::new();
    let mut finish = Vec::new();
    let mut root_visits = nodes.iter();
    while root_visits.len() != 0 {
        let Some((&root, _)) = ctx.next_charged(&mut root_visits, "fcstd product forward roots")?
        else {
            break;
        };
        if ctx.contains_hash_set(&visited, root, "fcstd product visited lookup")? {
            continue;
        }
        let mut traversal_storage = ctx.reserve_scoped(0, "fcstd product forward workspace")?;
        let mut stack = Vec::new();
        traversal_storage.with_storage(|| {
            ctx.push_vec(&mut stack, (root, false), "fcstd product forward stack")
        })?;
        while !stack.is_empty() {
            let Some((current, exiting)) = ctx.next_charged(
                &mut std::iter::from_fn(|| stack.pop()),
                "fcstd product forward traversal",
            )?
            else {
                break;
            };
            if exiting {
                storage.with_storage(|| {
                    ctx.push_vec(&mut finish, current, "fcstd product finish order")
                })?;
                continue;
            }
            if !storage.with_storage(|| {
                ctx.insert_hash_set(&mut visited, current, "fcstd product visited nodes")
            })? {
                continue;
            }
            traversal_storage.with_storage(|| {
                ctx.push_vec(&mut stack, (current, true), "fcstd product forward stack")
            })?;
            if let Some(targets) =
                ctx.get_btree_map(&forward, current, "fcstd product forward lookup")?
            {
                let mut target_visits = targets.iter().rev();
                while target_visits.len() != 0 {
                    let Some(&target) =
                        ctx.next_charged(&mut target_visits, "fcstd product forward edges")?
                    else {
                        break;
                    };
                    traversal_storage.with_storage(|| {
                        ctx.push_vec(&mut stack, (target, false), "fcstd product forward stack")
                    })?;
                }
            }
        }
    }

    let mut assigned = HashSet::new();
    let mut cyclic = BTreeSet::new();
    let mut root_visits = finish.into_iter().rev();
    while root_visits.len() != 0 {
        let Some(root) = ctx.next_charged(&mut root_visits, "fcstd product reverse roots")? else {
            break;
        };
        if !storage.with_storage(|| {
            ctx.insert_hash_set(&mut assigned, root, "fcstd product assigned nodes")
        })? {
            continue;
        }
        let mut component_storage = ctx.reserve_scoped(0, "fcstd product component workspace")?;
        let mut component = Vec::new();
        let mut stack = Vec::new();
        component_storage
            .with_storage(|| ctx.push_vec(&mut stack, root, "fcstd product reverse stack"))?;
        while !stack.is_empty() {
            let Some(current) = ctx.next_charged(
                &mut std::iter::from_fn(|| stack.pop()),
                "fcstd product reverse traversal",
            )?
            else {
                break;
            };
            component_storage.with_storage(|| {
                ctx.push_vec(&mut component, current, "fcstd product component nodes")
            })?;
            if let Some(sources) =
                ctx.get_btree_map(&reverse, current, "fcstd product reverse lookup")?
            {
                let mut source_visits = sources.iter();
                while source_visits.len() != 0 {
                    let Some(&source) =
                        ctx.next_charged(&mut source_visits, "fcstd product reverse edges")?
                    else {
                        break;
                    };
                    if storage.with_storage(|| {
                        ctx.insert_hash_set(&mut assigned, source, "fcstd product assigned nodes")
                    })? {
                        component_storage.with_storage(|| {
                            ctx.push_vec(&mut stack, source, "fcstd product reverse stack")
                        })?;
                    }
                }
            }
        }
        let self_cycle = if component.len() == 1 {
            match ctx.get_btree_map(&forward, root, "fcstd product self-cycle lookup")? {
                Some(targets) => ctx.any_by(
                    targets,
                    |target| ctx.equal(*target, root, "fcstd product self-cycle comparison"),
                    "fcstd product self-cycle edges",
                )?,
                None => false,
            }
        } else {
            false
        };
        if component.len() > 1 || self_cycle {
            let mut component_visits = component.iter();
            while component_visits.len() != 0 {
                let Some(member) =
                    ctx.next_charged(&mut component_visits, "fcstd product cyclic component")?
                else {
                    break;
                };
                ctx.insert_btree_set(&mut cyclic, *member, "fcstd product cyclic nodes")?;
            }
        }
    }
    Ok(cyclic)
}

#[cfg(test)]
pub(crate) mod tests;
