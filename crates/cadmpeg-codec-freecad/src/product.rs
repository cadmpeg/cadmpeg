// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use crate::native::frame::FiniteFrame;
use crate::native::joint::JointRecord;
use crate::placement::placement_matrix;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroUsize;

use crate::brep::ShapePayloadRecord;
use crate::native::{
    malformed, parse_bool, sole_named_property, ContainerNode,
    CopyOnChangePolicy as NativeCopyOnChangePolicy, LinkArrayCardinality, LinkOccurrence,
    ObjectRecord, ProductNode, ProductNodeRecord, PropertyRecord,
};
use cadmpeg_core::decode::DecodeContext;
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

/// Elements one link array may state or infer. A larger count is malformed.
const MAX_LINK_ARRAY_ELEMENTS: usize = 1_000_000;

mod payloads;

pub(crate) fn transfer_source<'source>(
    ctx: &DecodeContext<'source>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &[crate::native::EntryRecord],
    sources: &BTreeMap<String, crate::persistence::object_parts::ObjectSource<'source>>,
    payload_sources: &crate::gui::payloads::sources::PayloadSources<'source>,
) -> Result<Vec<ProductNodeRecord>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            owned.push(property);
        }
    }
    let mut output = Vec::new();
    for object in objects {
        let Some(kind) = product_kind(&object.type_name) else {
            continue;
        };
        let source = by_owner
            .get(object.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut owned = Vec::new();
        owned.extend_from_slice(source);
        if owned.iter().any(|property| {
            property.body.is_none()
                && matches!(
                    property.name.as_str(),
                    "Group"
                        | "LinkedObject"
                        | "ElementList"
                        | "LinkCopyOnChangeSource"
                        | "LinkCopyOnChangeGroup"
                        | "LinkCopyOnChange"
                        | "LinkCopyOnChangeTouched"
                        | "LinkTransform"
                        | "ElementCount"
                        | "LinkClaimChild"
                        | "Scale"
                        | "ScaleVector"
                        | "VisibilityList"
                        | "PlacementList"
                        | "ScaleList"
                )
        }) {
            // A present field that gave up cannot supply a product default.
            continue;
        }
        owned.retain(|property| semantic_property_issue(property).is_none());
        let id = crate::native::native_id("product", object.name());
        let (actual_object, producer) = sources
            .get(object.id())
            .ok_or_else(|| malformed("product owner has no original source authority"))?
            .record_owner(ctx, &id)?;
        let placements =
            payloads::placement_list(ctx, &owned, entries, &producer, payload_sources)?;
        let scales = payloads::vector_list(ctx, &owned, entries, &producer, payload_sources)?;
        let construct = |actual_object: String| -> Result<ProductNodeRecord, CodecError> {
            let group = sole_named_property("product", &owned, "Group")?;
            let members = group
                .map(|property| link_list(property, "App::PropertyLinkList", "Group"))
                .transpose()?
                .map(linked_object_names)
                .unwrap_or_default();
            let linked = sole_named_property("product", &owned, "LinkedObject")?;
            let prototype_link = linked
                .map(|property| {
                    single_link(property, "App::PropertyXLink", "XLink", "LinkedObject")
                })
                .transpose()?
                .flatten();
            let placement = selected_placement(&owned)?;
            let local_transform = placement
                .filter(|property| property.body.is_some())
                .map(placement_matrix)
                .transpose()?
                .flatten();
            let link_transform = bool_property(&owned, "LinkTransform")?;
            let element_count = integer_property(&owned, "ElementCount")?
                .and_then(|count| u64::try_from(count).ok());
            let claim_child = bool_property(&owned, "LinkClaimChild")?;
            let copy_on_change = copy_on_change_property(&owned)?;
            let copy_on_change_source = linked_target(
                &owned,
                "LinkCopyOnChangeSource",
                "App::PropertyXLink",
                "XLink",
            )?;
            let copy_on_change_group =
                linked_target(&owned, "LinkCopyOnChangeGroup", "App::PropertyLink", "Link")?;
            let copy_on_change_touched = bool_property(&owned, "LinkCopyOnChangeTouched")?;
            let scale = scale_property(&owned)?;
            let element_visibility = bool_list(&owned, "VisibilityList")?;
            let element_objects = sole_named_property("product", &owned, "ElementList")?
                .map(|property| link_list(property, "App::PropertyLinkList", "ElementList"))
                .transpose()?
                .map(linked_object_names)
                .unwrap_or_default();
            let placement_property = placement.map(|property| property.id.clone());
            let node = match kind {
                ProductKind::Occurrence => ProductNode::Occurrence(Box::new(LinkOccurrence {
                    members,
                    prototype: prototype_link
                        .and_then(|link| link.object())
                        .map(ToOwned::to_owned),
                    external_document: prototype_link.and_then(|link| link.document()).cloned(),
                    local_transform,
                    placement_property,
                    scale,
                    array: crate::native::LinkArray::from_admitted(
                        element_count,
                        placements,
                        scales,
                        element_visibility,
                        element_objects,
                    ),
                    link_transform,
                    linked_subelements: prototype_link
                        .map(|link| nonempty_subelements(link.subelements()))
                        .unwrap_or_default(),
                    claim_child,
                    copy_on_change: crate::native::CopyOnChange::from_admitted(
                        copy_on_change,
                        copy_on_change_source,
                        copy_on_change_group,
                        copy_on_change_touched,
                    ),
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
            Ok(ProductNodeRecord {
                id: id.clone(),
                object: actual_object,
                node,
            })
        };
        let record = construct(actual_object)?;
        output.push(record);
    }
    Ok(output)
}

fn semantic_property_issue(property: &PropertyRecord) -> Option<String> {
    let (expected_type, root) = match property.name.as_str() {
        "Group" | "ElementList" => ("App::PropertyLinkList", "LinkList"),
        "LinkedObject" | "LinkCopyOnChangeSource" => ("App::PropertyXLink", "XLink"),
        "LinkCopyOnChangeGroup" => ("App::PropertyLink", "Link"),
        "LinkTransform" | "LinkClaimChild" | "LinkCopyOnChangeTouched" => {
            ("App::PropertyBool", "Bool")
        }
        "ElementCount" => ("App::PropertyIntegerConstraint", "Integer"),
        "LinkCopyOnChange" => ("App::PropertyEnumeration", "Integer"),
        "Scale" => ("App::PropertyFloat", "Float"),
        "ScaleVector" => ("App::PropertyVector", "PropertyVector"),
        "Placement" | "LinkPlacement" => ("App::PropertyPlacement", "PropertyPlacement"),
        "VisibilityList" => ("App::PropertyBoolList", "BoolList"),
        "PlacementList" => ("App::PropertyPlacementList", "PlacementList"),
        "ScaleList" => ("App::PropertyVectorList", "VectorList"),
        _ => return None,
    };
    if property.type_name != expected_type {
        return Some(format!(
            "product property {} has runtime type {}, expected {expected_type} for {}",
            property.id, property.type_name, property.name
        ));
    }
    if property.body.is_none() {
        return None;
    }
    if property.body.is_some()
        && property.values().is_empty()
        && !property.links().is_empty()
        && property.links().iter().all(Option::is_none)
        && crate::persistence::link_grammar(expected_type).is_some()
    {
        return None;
    }
    let mut roots = property
        .values()
        .iter()
        .filter(|value| value.parent.is_none() && value.tag == root);
    let Some(value) = roots.next() else {
        return Some(format!(
            "product property {} requires one {root} value for {}",
            property.id, property.name
        ));
    };
    if roots.next().is_some() {
        return Some(format!(
            "product property {} requires one {root} value for {}",
            property.id, property.name
        ));
    }
    let value_text = || value.attributes.get("value").map(String::as_str);
    let invalid = match root {
        "Bool" => value_text().and_then(parse_bool).is_none(),
        "Integer" => value_text()
            .and_then(|text| text.parse::<i64>().ok())
            .is_none(),
        "Float" => value_text()
            .and_then(|text| text.parse::<f64>().ok())
            .and_then(FiniteReal::new)
            .is_none(),
        "BoolList" => {
            value_text().is_none_or(|text| text.bytes().any(|byte| !matches!(byte, b'0' | b'1')))
        }
        "PropertyVector" => ["valueX", "valueY", "valueZ"].iter().any(|name| {
            value
                .attributes
                .get(*name)
                .and_then(|text| text.parse::<f64>().ok())
                .and_then(FiniteReal::new)
                .is_none()
        }),
        "PropertyPlacement" => crate::placement::placement_matrix_unreported(property).is_none(),
        "XLink" | "Link" => property.links().len() != 1,
        _ => false,
    };
    invalid.then(|| {
        format!(
            "product property {} has an invalid {root} value for {}",
            property.id, property.name
        )
    })
}

pub(crate) fn semantic_issues(
    nodes: &[ProductNodeRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<(String, String)>, CodecError> {
    let mut issues = Vec::new();
    for node in nodes {
        issues.extend(
            node.semantic_issues()
                .into_iter()
                .map(|detail| (node.id.clone(), detail)),
        );
        if node.element_cardinality().is_some_and(|cardinality| matches!(cardinality, crate::native::LinkArrayCardinality::Elements(count) if count.get() > cadmpeg_core::decode::u64_from_index(MAX_LINK_ARRAY_ELEMENTS))) {
            issues.push((node.id.clone(), format!("link element count exceeds {MAX_LINK_ARRAY_ELEMENTS}")));
        }
        let mut owned: Vec<_> = properties
            .iter()
            .filter(|property| property.owner == node.object)
            .collect();
        for property in &owned {
            if let Some(detail) = semantic_property_issue(property) {
                issues.push((node.id.clone(), detail));
            }
        }
        owned.retain(|property| semantic_property_issue(property).is_none());
        if integer_property(&owned, "ElementCount")?.is_some_and(|count| count < 0) {
            issues.push((node.id.clone(), "negative ElementCount".into()));
        }
        if sole_named_property("product", &owned, "LinkPlacement")?.is_some()
            && sole_named_property("product", &owned, "Placement")?.is_some()
            && bool_property(&owned, "LinkTransform")?.is_none()
        {
            issues.push((
                node.id.clone(),
                "LinkPlacement and Placement require a valid LinkTransform policy".into(),
            ));
        }
    }
    let mut member_owners = HashMap::<&str, Vec<&ProductNodeRecord>>::new();
    for node in nodes.iter().filter(|node| {
        matches!(
            node.node,
            ProductNode::Part(_) | ProductNode::LinkGroup { .. } | ProductNode::Occurrence(_)
        )
    }) {
        for member in node.members() {
            member_owners.entry(member).or_default().push(node);
        }
    }
    for (member, owners) in member_owners {
        if owners
            .iter()
            .map(|owner| owner.object.as_str())
            .collect::<HashSet<_>>()
            .len()
            > 1
        {
            for owner in owners {
                issues.push((
                    owner.id.clone(),
                    format!("product member {member} has multiple parent containers"),
                ));
            }
        }
    }
    Ok(issues)
}

pub(crate) fn for_projection(
    mut nodes: Vec<ProductNodeRecord>,
    properties: &[PropertyRecord],
) -> Result<(Vec<ProductNodeRecord>, HashSet<String>), CodecError> {
    let mut invalid: HashSet<_> = semantic_issues(&nodes, properties)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let by_object = product_record_index(&nodes)?;
    let cycles = product_cycle_nodes(&by_object)?;
    invalid.extend(
        nodes
            .iter()
            .filter(|node| cycles.contains(node.object.as_str()))
            .map(|node| node.id.clone()),
    );
    let mut unavailable: HashSet<_> = nodes
        .iter()
        .filter(|node| invalid.contains(&node.id))
        .map(|node| node.object.clone())
        .collect();
    nodes.retain(|node| !invalid.contains(&node.id));
    loop {
        let mut removed = Vec::new();
        nodes.retain_mut(|node| {
            let keep = node.retain_projection_peers(&unavailable);
            if !keep {
                removed.push(node.object.clone());
            }
            keep
        });
        if removed.is_empty() {
            break;
        }
        unavailable.extend(removed);
    }
    Ok((nodes, unavailable))
}

fn linked_object_names(links: &[Option<crate::native::LinkTarget>]) -> Vec<String> {
    links
        .iter()
        .flatten()
        .filter_map(|link| link.object().map(ToOwned::to_owned))
        .collect()
}

fn nonempty_subelements(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter(|value| !value.is_empty())
        .cloned()
        .collect()
}

fn product_record_index(
    records: &[ProductNodeRecord],
) -> Result<HashMap<&str, &ProductNodeRecord>, CodecError> {
    let mut index = HashMap::new();
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
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    records: &[ProductNodeRecord],
    joints: &[JointRecord],
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    bodies: &[Body],
    unavailable: &HashSet<String>,
) -> Result<(Vec<ProductDefinition>, Vec<Occurrence>), CodecError> {
    let record_by_object = product_record_index(records)?;
    let mut component_objects = Vec::new();
    let mut occurrence_objects = HashSet::new();
    for record in records {
        if matches!(record.node, ProductNode::Occurrence(_)) {
            occurrence_objects.insert(record.object.as_str());
        } else {
            component_objects.push(record.object.as_str());
        }
    }
    for record in records {
        for member in record
            .members()
            .iter()
            .filter(|member| !occurrence_objects.contains(member.as_str()))
        {
            component_objects.push(member.as_str());
        }
        if record.external_document().is_none() {
            if let Some(prototype) = record.prototype() {
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
                    component_objects.push(name);
                }
            }
        }
        for name in record.element_objects() {
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
                    component_objects.push(name);
                }
            }
        }
    }
    // Explicit typed controls distinguish standalone product definitions from
    // application objects used only to carry geometry or native organization.
    for object in objects {
        if crate::writer::products::metadata::included(properties, object.id().as_str(), true)?
            == Some(true)
        {
            component_objects.push(object.id().as_str());
        }
    }
    let mut controlled_components = Vec::new();
    for object in component_objects {
        if !unavailable.contains(object)
            && crate::writer::products::metadata::included(properties, object, true)? != Some(false)
        {
            controlled_components.push(object);
        }
    }
    let mut component_objects = controlled_components;
    component_objects.sort_unstable_by(Ord::cmp);
    component_objects.dedup();

    let mut properties_by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !properties_by_owner.contains_key(property.owner.as_str()) {
            properties_by_owner.insert(property.owner.as_str(), Vec::new());
        }
        if let Some(owned) = properties_by_owner.get_mut(property.owner.as_str()) {
            owned.push(property);
        }
    }
    let mut placements_by_object = HashMap::new();
    for (&owner, owned) in &properties_by_owner {
        if unavailable.contains(owner) {
            continue;
        }
        let usable: Vec<_> = owned
            .iter()
            .copied()
            .filter(|property| {
                property.body.is_some() && semantic_property_issue(property).is_none()
            })
            .collect();
        if let Some(property) = selected_placement(&usable)? {
            if let Some(placement) = placement_matrix(property)? {
                placements_by_object.insert(owner, placement.transform());
            }
        }
    }

    let definition_id = |object: &str| -> Result<ProductDefinitionId, CodecError> {
        ProductDefinitionId::mint(crate::native::model_id(
            "product_definition",
            object,
            "definition",
        ))
        .map_err(CodecError::malformed)
    };
    let container_occurrence_id = |object: &str| -> Result<OccurrenceId, CodecError> {
        OccurrenceId::mint(crate::native::model_id("occurrence", object, "container"))
            .map_err(CodecError::malformed)
    };
    let parent_occurrence_id = |object: &str| -> Result<OccurrenceId, CodecError> {
        if let Some(record) = record_by_object
            .get(object)
            .filter(|record| matches!(record.node, ProductNode::Occurrence(_)))
        {
            let key = if occurrence_count(record)?.is_some_and(|count| count.get() > 1) {
                "0"
            } else {
                "instance"
            };
            OccurrenceId::mint(crate::native::model_id("occurrence", object, key))
                .map_err(CodecError::malformed)
        } else {
            container_occurrence_id(object)
        }
    };
    let mut parent_by_object = HashMap::<&str, &str>::new();
    for record in records.iter().filter(|record| {
        matches!(
            record.node,
            ProductNode::Part(_) | ProductNode::LinkGroup { .. } | ProductNode::Occurrence(_)
        )
    }) {
        // Ordinary document groups organize objects without owning placements.
        for member in record.members() {
            let member = member.as_str();
            match parent_by_object.get(member) {
                None => {
                    parent_by_object.insert(member, record.object.as_str());
                }
                Some(previous) if *previous != record.object.as_str() => {
                    return Err(CodecError::malformed(format_args!(
                        "product member {member} has multiple parent containers"
                    )));
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
        if crate::writer::products::metadata::included(properties, &record.object, false)?
            == Some(false)
        {
            continue;
        }
        let Some(count) = occurrence_count(record)? else {
            continue;
        };
        let count = count.get();
        let parent = parent_by_object
            .get(record.object.as_str())
            .map(|object| parent_occurrence_id(object))
            .transpose()?;
        let mut record_prototype_transform = None;
        for index in 0..count {
            let element = count > 1;
            let occurrence_id = crate::native::model_id(
                "occurrence",
                &record.object,
                if element {
                    index.to_string()
                } else {
                    "instance".to_owned()
                },
            );
            let mut producer = None;
            let actual_count = payloads::occurrence_arity(
                ctx,
                record,
                properties,
                &occurrence_id,
                count,
                &mut producer,
            )?;
            if index >= actual_count {
                return Err(malformed(
                    "occurrence ordinal exceeds its actual source arity",
                ));
            }
            let element = actual_count > 1;
            let missing_frame = !record.element_transforms_known()
                || record
                    .element_transforms()
                    .get(index)
                    .is_some_and(Option::is_none);
            let missing_scale = !record.element_scales_known()
                || record
                    .element_scales()
                    .get(index)
                    .is_some_and(Option::is_none);
            let local = record.local_transform().map(FiniteFrame::transform);
            let element_transform = if local.is_some() {
                record
                    .element_transforms()
                    .get(index)
                    .copied()
                    .flatten()
                    .map(|value| {
                        payloads::placement(
                            ctx,
                            properties,
                            record,
                            (&occurrence_id, index, &mut producer),
                            value,
                        )
                    })
                    .transpose()?
            } else {
                None
            };
            let local_transform = match (local, element_transform.map(FiniteFrame::transform)) {
                _ if missing_frame => None,
                (Some(local), Some(element)) => Some(
                    local
                        .compose(element)
                        .map_err(|error| malformed(error.to_string()))?,
                ),
                (Some(local), None) if !element => Some(local),
                _ => None,
            };
            let prototype_transform = match record_prototype_transform {
                Some(transform) => transform,
                None => *record_prototype_transform.insert(linked_prototype_transform(
                    ctx,
                    record,
                    &record_by_object,
                    &placements_by_object,
                    &mut Vec::new(),
                )?),
            };
            let element_scale = if local_transform.is_some() && !missing_scale {
                record
                    .element_scales()
                    .get(index)
                    .copied()
                    .flatten()
                    .map(|value| {
                        payloads::vector(
                            ctx,
                            properties,
                            record,
                            (&occurrence_id, index, &mut producer),
                            value,
                        )
                    })
                    .transpose()?
                    .map_or([1.0; 3], FiniteVector::get)
            } else {
                [1.0; 3]
            };
            let base_scale = record.scale().unwrap_or([1.0; 3]);
            let scale: [f64; 3] =
                std::array::from_fn(|axis| base_scale[axis] * element_scale[axis]);
            let scale_transform = Transform::affine([
                [scale[0], 0.0, 0.0, 0.0],
                [0.0, scale[1], 0.0, 0.0],
                [0.0, 0.0, scale[2], 0.0],
            ])
            .ok_or_else(|| malformed("occurrence scale must be finite"))?;
            let local_transform = local_transform
                .filter(|_| !missing_scale)
                .map(|transform| transform.compose(scale_transform))
                .transpose()
                .map_err(|error| malformed(error.to_string()))?;
            let copy_on_change = record
                .copy_on_change_policy()
                .map(|policy| {
                    Ok::<_, CodecError>(CopyOnChange {
                        policy: copy_on_change_policy(policy),
                        source: record
                            .copy_on_change_source()
                            .map(neutral_link_target)
                            .transpose()?
                            .flatten(),
                        group: record
                            .copy_on_change_group()
                            .map(neutral_link_target)
                            .transpose()?
                            .flatten(),
                        touched: record.copy_on_change_touched(),
                    })
                })
                .transpose()?;
            occurrences.push(Occurrence {
                id: OccurrenceId::mint(occurrence_id).map_err(CodecError::malformed)?,
                prototype: if let Some(document) = record.external_document() {
                    PrototypeReference::External {
                        document: external_document_reference(
                            document.as_str(),
                            document.attribute(),
                        ),
                        object: record.prototype().map(ToOwned::to_owned),
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
                    .map_or(OccurrenceParent::Root {}, |occurrence| {
                        OccurrenceParent::Occurrence {
                            occurrence: occurrence.clone(),
                        }
                    }),
                ordinal: u32::try_from(index).map_err(|_| {
                    CodecError::malformed(format_args!(
                        "product occurrence {} element index exceeds u32",
                        record.id
                    ))
                })?,
                transform: if record.link_transform() == Some(true) && prototype_transform.is_none()
                {
                    None
                } else {
                    local_transform
                },
                linked_prototype: if record.link_transform() == Some(true) {
                    prototype_transform
                } else {
                    None
                },
                name: Some(record.object.clone()),
                visible: record.element_visibility().get(index).copied(),
                link: LinkState::new(
                    record.linked_subelements().to_vec(),
                    record
                        .element_objects()
                        .get(index)
                        .map(|object| definition_id(object))
                        .transpose()?,
                    record.claim_child(),
                    copy_on_change,
                ),
                native_ref: Some(record.object.clone()),
            });
        }
    }

    let mut object_by_id = HashMap::new();
    for object in objects {
        object_by_id.insert(object.id().as_str(), object);
    }
    let mut property_owner = HashMap::new();
    for property in properties {
        property_owner.insert(property.id.as_str(), property.owner.as_str());
    }
    let mut body_owners = Vec::new();
    for payload in payloads {
        if let Some(owner) = property_owner.get(payload.property.as_str()) {
            body_owners.push((crate::native::model_id("body", &payload.id, ""), *owner));
        }
    }
    let mut definitions = Vec::new();
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
            if let Some(value) = metadata_string(owned, name.as_str()) {
                bom_properties.insert(name, value);
            }
        }
        let id_part_number = if source_object.is_some_and(|object| {
            matches!(
                object.type_name.as_str(),
                "Assembly::AssemblyObject" | "Assembly::AssemblyLink" | "App::Part"
            )
        }) {
            metadata_string(owned, "Id").filter(|value| !value.is_empty())
        } else {
            None
        };
        let mut definition_bodies = Vec::new();
        for body in bodies.iter().filter(|body| {
            body_owners
                .iter()
                .any(|(prefix, owner)| *owner == object && body.id.as_str().starts_with(prefix))
        }) {
            definition_bodies.push(body.id.clone());
        }
        definitions.push(ProductDefinition {
            id: definition_id(object)?,
            kind,
            source_name: source_object.map(|object| object.name().to_owned()),
            label: match metadata_string(owned, "CadmpegProductLabel") {
                Some(label) => {
                    serde_json::from_str::<Option<String>>(&label).unwrap_or(Some(label))
                }
                None => metadata_string(owned, "Label"),
            },
            description: metadata_string(owned, "Description"),
            part_number: metadata_string(owned, "PartNumber")
                .filter(|value| !value.is_empty())
                .or(id_part_number),
            bom_properties,
            bodies: definition_bodies,
            native_ref: Some((*object).to_owned()),
        });
    }

    for object in &component_objects {
        if crate::writer::products::metadata::included(properties, object, false)? == Some(false) {
            continue;
        }
        let record = record_by_object.get(*object).copied();
        let local_transform = record
            .and_then(ProductNodeRecord::local_transform)
            .map(crate::native::frame::FiniteFrame::transform)
            .or_else(|| placements_by_object.get(*object).copied());
        let parent = parent_by_object.get(*object).copied();
        let parent = match parent {
            Some(parent) => OccurrenceParent::Occurrence {
                occurrence: parent_occurrence_id(parent)?,
            },
            None => OccurrenceParent::Root {},
        };
        occurrences.push(Occurrence {
            id: container_occurrence_id(object)?,
            prototype: PrototypeReference::Local {
                definition: definition_id(object)?,
            },
            parent,
            ordinal: 0,
            transform: local_transform,
            linked_prototype: None,
            name: Some((*object).to_owned()),
            visible: None,
            link: None,
            native_ref: Some((*object).to_owned()),
        });
    }
    let mut next_ordinal = HashMap::<Option<String>, u32>::new();
    for occurrence in &mut occurrences {
        let parent = match &occurrence.parent {
            OccurrenceParent::Root {} => None,
            OccurrenceParent::Occurrence { occurrence } => Some(occurrence.as_str().to_owned()),
        };
        let ordinal = next_ordinal.entry(parent).or_default();
        occurrence.ordinal = *ordinal;
        *ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("product occurrence ordinal exceeds u32"))?;
    }
    crate::writer::products::metadata::apply(properties, &mut definitions, &mut occurrences)?;
    Ok((definitions, occurrences))
}

fn linked_prototype_transform(
    ctx: &DecodeContext<'_>,
    record: &ProductNodeRecord,
    records: &HashMap<&str, &ProductNodeRecord>,
    placements: &HashMap<&str, Transform>,
    stack: &mut Vec<String>,
) -> Result<Option<Transform>, CodecError> {
    let _depth = ctx.enter_nested("resolve FCStd nested link transform")?;
    if record.link_transform() != Some(true) || record.external_document().is_some() {
        return Ok(None);
    }
    let Some(prototype) = record.prototype() else {
        return Ok(None);
    };
    if stack.iter().any(|object| object == &record.object) {
        return Err(CodecError::malformed(format_args!(
            "nested link cycle reaches {}",
            record.object
        )));
    }
    stack.push(record.object.clone());
    let target = records.get(prototype).copied();
    let placement = target
        .and_then(ProductNodeRecord::local_transform)
        .map(crate::native::frame::FiniteFrame::transform)
        .or_else(|| placements.get(prototype).copied());
    let nested = if let Some(target) = target.filter(|target| target.link_transform() == Some(true))
    {
        Some(linked_prototype_transform(
            ctx, target, records, placements, stack,
        ))
    } else {
        None
    };
    stack.pop();
    match nested {
        Some(nested) => match (placement, nested?) {
            (Some(placement), Some(nested)) => placement
                .compose(nested)
                .map(Some)
                .map_err(|error| malformed(error.to_string())),
            _ => Ok(None),
        },
        None => Ok(placement),
    }
}

/// Occurrences one product node contributes.
///
/// A link that states no array, or states a zero `ElementCount`, is scalar and
/// contributes its single occurrence; the count is the cardinality the node's
/// own type carries, never a floored zero.
fn occurrence_count(record: &ProductNodeRecord) -> Result<Option<NonZeroUsize>, CodecError> {
    let elements = match record.element_cardinality() {
        Some(LinkArrayCardinality::Unknown) => return Ok(None),
        None | Some(LinkArrayCardinality::Scalar) => return Ok(Some(NonZeroUsize::MIN)),
        Some(LinkArrayCardinality::Elements(elements)) => elements,
    };
    let count = NonZeroUsize::try_from(elements).map_err(|_| {
        CodecError::malformed(format_args!(
            "{} element count exceeds addressable size",
            record.id
        ))
    })?;
    if count.get() > MAX_LINK_ARRAY_ELEMENTS || u32::try_from(count.get()).is_err() {
        return Err(CodecError::malformed(format_args!(
            "{} link-array element count {count} exceeds {MAX_LINK_ARRAY_ELEMENTS}",
            record.id
        )));
    }
    Ok(Some(count))
}

/// Link occurrences that [`transfer_neutral`] builds from these records.
///
/// The decoder admits this count against the entity ceiling before it builds
/// any occurrence.
pub(crate) fn link_occurrence_count(records: &[ProductNodeRecord]) -> Result<u64, CodecError> {
    let mut total = 0_u64;
    for record in records
        .iter()
        .filter(|record| matches!(record.node, ProductNode::Occurrence(_)))
    {
        let count = cadmpeg_core::decode::u64_from_index(
            occurrence_count(record)?.map_or(0, NonZeroUsize::get),
        );
        total = total
            .checked_add(count)
            .ok_or_else(|| CodecError::malformed("FCStd link occurrence count overflows u64"))?;
    }
    Ok(total)
}

fn copy_on_change_policy(value: &NativeCopyOnChangePolicy) -> CopyOnChangePolicy {
    match value.index() {
        0 => CopyOnChangePolicy::Disabled,
        1 => CopyOnChangePolicy::Enabled,
        2 => CopyOnChangePolicy::Owned,
        3 => CopyOnChangePolicy::Tracking,
        _ => CopyOnChangePolicy::Native(value.as_str().to_owned()),
    }
}

pub(crate) fn external_document_reference(
    value: &str,
    attribute: Option<&str>,
) -> ExternalDocument {
    let value = value.to_owned();
    if attribute.is_some_and(|name| name.eq_ignore_ascii_case("file")) {
        ExternalDocument::path(value)
    } else {
        ExternalDocument::document_id(value)
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

fn single_link<'a>(
    property: &'a PropertyRecord,
    expected_type: &str,
    root: &str,
    name: &str,
) -> Result<Option<&'a crate::native::LinkTarget>, CodecError> {
    require_root(property, expected_type, name, root)?;
    if property.links().len() != 1 {
        return Err(CodecError::malformed(format_args!(
            "product property {} requires one {name} target, found {}",
            property.id,
            property.links().len()
        )));
    }
    Ok(property.links()[0].as_ref())
}

fn link_list<'a>(
    property: &'a PropertyRecord,
    expected_type: &str,
    name: &str,
) -> Result<&'a [Option<crate::native::LinkTarget>], CodecError> {
    require_root(property, expected_type, name, "LinkList")?;
    if property
        .values()
        .iter()
        .skip(1)
        .any(|value| value.tag != "Link")
    {
        return Err(CodecError::malformed(format_args!(
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
        return Err(CodecError::malformed(format_args!(
            "product property {} has runtime type {}, expected {expected_type} for {name}",
            property.id, property.type_name
        )));
    }
    if property.body.is_some()
        && property.values().is_empty()
        && !property.links().is_empty()
        && property.links().iter().all(Option::is_none)
        && crate::persistence::link_grammar(expected_type).is_some()
    {
        return Ok(());
    }
    if property.values().first().map(|value| value.tag.as_str()) != Some(root)
        || property
            .values()
            .iter()
            .filter(|value| value.tag == root)
            .count()
            != 1
    {
        return Err(CodecError::malformed(format_args!(
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
        return Err(CodecError::malformed(format_args!(
            "product property {} has multiple values for {name}",
            property.id
        )));
    }
    Ok(&property.values()[0])
}

fn selected_placement<'a>(
    properties: &[&'a PropertyRecord],
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let link_placement = sole_named_property("product", properties, "LinkPlacement")?;
    let placement = sole_named_property("product", properties, "Placement")?;
    for property in [link_placement, placement]
        .into_iter()
        .flatten()
        .filter(|property| property.body.is_some())
    {
        placement_matrix(property)?;
    }
    match (link_placement, placement) {
        (Some(link_placement), Some(placement)) => {
            let Some(use_link_placement) = bool_property(properties, "LinkTransform")? else {
                return Ok(None);
            };
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

#[derive(Debug, PartialEq, Eq)]
enum ProductKind {
    Group,
    Part,
    LinkGroup,
    Occurrence,
}

/// Whether the current native product facts assign an object a product role.
pub(crate) fn has_product_role(
    object: &str,
    records: &[ProductNodeRecord],
    joints: &[crate::native::joint::JointRecord],
) -> bool {
    records.iter().any(|record| {
        record.object == object
            || record.members().iter().any(|member| member == object)
            || (record.external_document().is_none() && record.prototype() == Some(object))
            || record
                .element_objects()
                .iter()
                .any(|member| member == object)
            || [
                record.copy_on_change_source(),
                record.copy_on_change_group(),
            ]
            .into_iter()
            .flatten()
            .any(|target| target.document().is_none() && target.object() == Some(object))
    }) || joints.iter().any(|joint| {
        joint
            .references()
            .any(|target| target.document().is_none() && target.object() == Some(object))
    })
}

pub(crate) fn is_occurrence_kind(kind: &str) -> bool {
    matches!(product_kind(kind), Some(ProductKind::Occurrence))
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
    if property.type_name != "App::PropertyString" || property.body.is_none() {
        return None;
    }
    let [value] = property.values() else {
        return None;
    };
    if value.parent.is_some() || value.tag != "String" {
        return None;
    }
    value.attributes.get("value").cloned()
}

fn bool_property(properties: &[&PropertyRecord], name: &str) -> Result<Option<bool>, CodecError> {
    let Some(property) = sole_named_property("product", properties, name)? else {
        return Ok(None);
    };
    let value = single_value(property, "App::PropertyBool", name, "Bool")?;
    let Some(value) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no Bool value",
            property.id
        )));
    };
    let Some(value) = parse_bool(value) else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has an invalid Bool value",
            property.id
        )));
    };
    Ok(Some(value))
}

fn integer_property(properties: &[&PropertyRecord], name: &str) -> Result<Option<i64>, CodecError> {
    let Some(property) = sole_named_property("product", properties, name)? else {
        return Ok(None);
    };
    let value = single_value(property, "App::PropertyIntegerConstraint", name, "Integer")?;
    let Some(value) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no Integer value",
            property.id
        )));
    };
    match value.parse() {
        Ok(value) => Ok(Some(value)),
        Err(_) => Err(CodecError::malformed(format_args!(
            "product property {} has an invalid Integer value",
            property.id
        ))),
    }
}

fn copy_on_change_property(
    properties: &[&PropertyRecord],
) -> Result<Option<NativeCopyOnChangePolicy>, CodecError> {
    let Some(property) = sole_named_property("product", properties, "LinkCopyOnChange")? else {
        return Ok(None);
    };
    require_root(
        property,
        "App::PropertyEnumeration",
        "LinkCopyOnChange",
        "Integer",
    )?;
    let value = property
        .values()
        .iter()
        .find(|value| value.tag == "Integer" && value.parent.is_none())
        .ok_or_else(|| malformed("copy-on-change has no Integer value"))?;
    let Some(raw) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no enumeration value",
            property.id
        )));
    };
    NativeCopyOnChangePolicy::from_raw(raw.to_owned())
        .map(Some)
        .map_err(|_| {
            CodecError::malformed(format_args!(
                "product property {} has an invalid enumeration Integer value",
                property.id
            ))
        })
}

fn linked_target(
    properties: &[&PropertyRecord],
    name: &str,
    expected_type: &str,
    root: &str,
) -> Result<Option<crate::native::LinkTarget>, CodecError> {
    let Some(property) = sole_named_property("product", properties, name)? else {
        return Ok(None);
    };
    Ok(single_link(property, expected_type, root, name)?.cloned())
}

fn neutral_link_target(
    target: &crate::native::LinkTarget,
) -> Result<Option<cadmpeg_ir::products::PrototypeReference>, CodecError> {
    if let Some(document) = target.document() {
        return Ok(Some(cadmpeg_ir::products::PrototypeReference::External {
            document: external_document_reference(document.as_str(), document.attribute()),
            object: target.object().map(ToOwned::to_owned),
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
        definition: ProductDefinitionId::mint(crate::native::model_id(
            "product_definition",
            object,
            "definition",
        ))
        .map_err(CodecError::malformed)?,
    }))
}

fn scale_property(properties: &[&PropertyRecord]) -> Result<Option<FiniteVector<3>>, CodecError> {
    if let Some(property) = sole_named_property("product", properties, "ScaleVector")? {
        return vector_property(property).map(Some);
    }
    let Some(property) = sole_named_property("product", properties, "Scale")? else {
        return Ok(None);
    };
    let value = single_value(property, "App::PropertyFloat", "Scale", "Float")?;
    let Some(value) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no Float value",
            property.id
        )));
    };
    let value = parse_finite(value, property, "Scale")?;
    Ok(Some([value; 3].into()))
}

fn vector_property(property: &PropertyRecord) -> Result<FiniteVector<3>, CodecError> {
    let value = single_value(
        property,
        "App::PropertyVector",
        "ScaleVector",
        "PropertyVector",
    )?;
    let component = |name: &str| {
        let Some(value) = value.attributes.get(name) else {
            return Err(CodecError::malformed(format_args!(
                "product property {} has no {name} vector component",
                property.id
            )));
        };
        parse_finite(value, property, "ScaleVector")
    };
    Ok([
        component("valueX")?,
        component("valueY")?,
        component("valueZ")?,
    ]
    .into())
}

fn parse_finite(
    value: &str,
    property: &PropertyRecord,
    name: &str,
) -> Result<FiniteReal, CodecError> {
    let Some(value) = value.parse::<f64>().ok().and_then(FiniteReal::new) else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has an invalid finite value for {name}",
            property.id
        )));
    };
    Ok(value)
}

fn bool_list(properties: &[&PropertyRecord], name: &str) -> Result<Vec<bool>, CodecError> {
    let Some(property) = sole_named_property("product", properties, name)? else {
        return Ok(Vec::new());
    };
    let value = single_value(property, "App::PropertyBoolList", name, "BoolList")?;
    let Some(encoded) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no BoolList value",
            property.id
        )));
    };
    if encoded.bytes().any(|byte| !matches!(byte, b'0' | b'1')) {
        return Err(CodecError::malformed(format_args!(
            "product property {} has an invalid BoolList bit string",
            property.id
        )));
    }
    // FreeCAD writes the most-significant bit first: the rightmost source bit
    // belongs to element zero. The raw XML remains on the property record;
    // this projection follows the element order used by the other carriers.
    let mut values = Vec::new();
    values.extend(encoded.bytes().rev().map(|byte| byte == b'1'));
    Ok(values)
}

pub(crate) fn product_cycle_nodes<'a>(
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
            targets.push(target);
        }
        Ok(targets)
    };
    let mut reverse = HashMap::<&str, Vec<&str>>::new();
    for &source in nodes.keys() {
        reverse.insert(source, Vec::new());
    }
    for &source in nodes.keys() {
        for target in edges(source) {
            if let Some(sources) = reverse.get_mut(target) {
                sources.push(source);
            }
        }
    }

    let mut visited = HashSet::new();
    let mut finish = Vec::new();
    for &root in nodes.keys() {
        if !visited.insert(root) {
            continue;
        }
        let mut stack = Vec::new();
        stack.push((root, collect_edges(root)?, 0_usize));
        while let Some((current, targets, next)) = stack.last_mut() {
            if let Some(&target) = targets.get(*next) {
                *next += 1;
                if visited.insert(target) {
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
        if !assigned.insert(root) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = Vec::new();
        stack.push(root);
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
            for member in component {
                cyclic.insert(member);
            }
        }
    }
    Ok(cyclic)
}

#[cfg(test)]
pub(crate) mod tests;
