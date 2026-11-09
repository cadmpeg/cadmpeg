// SPDX-License-Identifier: Apache-2.0
//! Neutral product definitions, placed instances, and assembly constraints.

mod joints;

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::products::{
    CopyOnChangePolicy, ExternalDocument, OccurrenceParent, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, HashMap, HashSet};

fn link_list(property: &str, members: &[String]) -> String {
    let content = members
        .iter()
        .map(|name| xml::element("Link", &[("value", name.clone())]))
        .collect::<String>();
    xml::property(
        property,
        "App::PropertyLinkList",
        &format!("<LinkList count=\"{}\">{content}</LinkList>", members.len()),
    )
}

fn bool_property(object: &mut Object, name: &str, value: bool) {
    object.set(name, xml::scalar(name, "App::PropertyBool", "Bool", value));
}

pub(super) fn xlink(name: &str, file: &str, subelements: &[String]) -> String {
    let mut attributes = vec![("name", name.to_owned()), ("file", file.to_owned())];
    if subelements.is_empty() {
        return xml::element("XLink", &attributes);
    }
    attributes.push(("count", subelements.len().to_string()));
    let root = xml::element("XLink", &attributes);
    let content = subelements
        .iter()
        .map(|selection| xml::element("Sub", &[("value", selection.clone())]))
        .collect::<String>();
    format!(
        "{}{content}</XLink>",
        root.trim_end_matches("/>").to_owned() + ">"
    )
}

fn prototype(
    reference: &PrototypeReference,
    occurrence: &str,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> (String, String) {
    match reference {
        PrototypeReference::Local { definition } => (
            names.get(definition.as_str()).cloned().unwrap_or_else(|| {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "occurrence {occurrence} references absent prototype {definition}"
                )));
                String::new()
            }),
            String::new(),
        ),
        PrototypeReference::External { document, object } => (
            object.clone().unwrap_or_default(),
            external(document, &format!("occurrence {occurrence}"), losses),
        ),
        PrototypeReference::Unresolved {} => (String::new(), String::new()),
    }
}

/// The file token of `document`, which `owner` names in losses.
fn external(document: &ExternalDocument, owner: &str, losses: &mut Vec<LossNote>) -> String {
    match document {
        ExternalDocument::Path { path } => path.as_str().to_owned(),
        ExternalDocument::DocumentId { document_id } => {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "external document identity {document_id} of {owner} is written as a file token"
            )));
            document_id.as_str().to_owned()
        }
        ExternalDocument::Missing {} => {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "{owner} has no external document and is written as an unresolved link"
            )));
            String::new()
        }
    }
}

fn fresh_name(proposed: &str, used: &mut HashSet<String>) -> String {
    let mut candidate = proposed.to_owned();
    // With n occupied names, one of these n + 1 distinct candidates is free.
    for index in 1..=used.len() {
        if used.insert(candidate.clone()) {
            return candidate;
        }
        candidate = format!("{proposed}_{index}");
    }
    used.insert(candidate.clone());
    candidate
}

/// Output Group members per container object, and the native object ids its
/// source `Group` lists in order.
#[derive(Default)]
struct Groups {
    members: BTreeMap<String, Vec<String>>,
    listed: HashMap<String, Vec<String>>,
}

/// `members` with each occurrence child in a native `Group` slot. Native-only
/// members keep their own slots; occurrence slots take the children in
/// ordinal order, and children past the native slots follow.
fn slotted(extras: &[String], children: &[String], listed: &[String]) -> Vec<String> {
    let mut queue = children.iter();
    let mut result: Vec<String> = Vec::new();
    for name in listed {
        if extras.contains(name) {
            if !result.contains(name) {
                result.push(name.clone());
            }
        } else if children.contains(name) {
            if let Some(next) = queue.next() {
                result.push(next.clone());
            }
        }
    }
    result.extend(queue.cloned());
    for extra in extras {
        if !result.contains(extra) {
            result.push(extra.clone());
        }
    }
    result
}

fn definitions(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    body_names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<Groups, CodecError> {
    let mut groups = Groups::default();
    let mut native_properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    let native_objects = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::ObjectRecord>("objects"))
        .transpose()?
        .unwrap_or_default();
    let available = ir
        .native
        .namespace("fcstd")
        .map(super::xml::retained_entry_names)
        .unwrap_or_default();
    native_properties.retain(|property| xml::validate(property, &available).is_ok());
    for definition in &ir.model.product_definitions {
        let Some(name) = names.get(definition.id.as_str()).cloned() else {
            debug_assert!(false, "product has no output identity");
            continue;
        };
        let position = objects.iter().position(|object| object.name == name);
        let mut kind = match definition.kind {
            ProductDefinitionKind::Part => "App::Part",
            ProductDefinitionKind::Group => "App::DocumentObjectGroup",
            ProductDefinitionKind::LinkGroup => "App::LinkGroup",
            ProductDefinitionKind::Object => "Part::Feature",
        };
        // An assembly definition keeps its native construction type; only a
        // derived type with no native record behind it falls back (#238 §5.5).
        if definition.kind == ProductDefinitionKind::Part {
            if let Some(native) = definition
                .native_ref
                .as_deref()
                .and_then(|id| {
                    native_objects
                        .iter()
                        .find(|native| native.id().as_str() == id)
                })
                .filter(|native| native.type_name.starts_with("Assembly::"))
            {
                kind = native.type_name.as_str();
            }
        }
        // A `None` label is written as the object Name and re-decodes as
        // `Some`, an addition (#238 §5.5).
        let label = definition
            .label
            .as_deref()
            .filter(|label| !label.is_empty())
            .unwrap_or(name.as_str());
        let object = match position {
            Some(index) => &mut objects[index],
            None => {
                let index = objects.len();
                objects.push(Object::new(name.clone(), kind, label));
                let object = &mut objects[index];
                // A regenerated container keeps the native declaration facts
                // no neutral record owns, as the synthesis path does for
                // unrepresented objects. Pending execution is such a fact.
                if let Some(native) = definition.native_ref.as_deref().and_then(|id| {
                    native_objects
                        .iter()
                        .find(|native| native.id().as_str() == id)
                }) {
                    object.declaration_attributes.clone_from(&native.attributes);
                    if let Some(view_type) = &native.view_type {
                        object
                            .declaration_attributes
                            .insert("ViewType".into(), view_type.clone());
                    }
                }
                object
            }
        };
        if definition.kind != ProductDefinitionKind::Object {
            object.kind = kind.into();
        }
        if !ir.model.features.iter().any(|feature| {
            feature.native_ref.is_some() && feature.native_ref == definition.native_ref
        }) {
            if let Some(native) = native_objects
                .iter()
                .find(|native| definition.native_ref.as_deref() == Some(native.id().as_str()))
            {
                object.dependencies = native
                    .dependencies
                    .iter()
                    .filter_map(|id| names.get(id).cloned())
                    .collect();
                if object.dependencies.len() != native.dependencies.len() {
                    losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                        "object {} ({}) dependencies on omitted records are removed",
                        object.name,
                        native.id().as_str()
                    )));
                }
            }
        }
        for property in native_properties
            .iter()
            .filter(|property| definition.native_ref.as_deref() == Some(property.owner.as_str()))
        {
            if property.type_name == "Part::PropertyPartShape"
                || !super::native::property_held(ir, property, &native_objects)
            {
                continue;
            }
            // The object may also carry a neutral feature, sketch, or
            // parameter. Product metadata cannot overwrite those current lanes.
            let construction_owner =
                ir.model
                    .features
                    .iter()
                    .any(|feature| feature.native_ref.as_deref() == Some(property.owner.as_str()))
                    || ir.model.sketches.iter().any(|sketch| {
                        sketch.native_ref.as_deref() == Some(property.owner.as_str())
                    });
            if object.properties.contains_key(&property.name)
                || (construction_owner && crate::property_policy::has_neutral_form(&property.name))
            {
                continue;
            }
            if matches!(
                property.name.as_str(),
                "Label"
                    | "Description"
                    | "PartNumber"
                    | "Placement"
                    | "Group"
                    | "ScaleVector"
                    | "Visibility"
            ) || definition
                .bom_properties
                .keys()
                .any(|key| key.as_str() == property.name)
            {
                continue;
            }
            if let Some(value) = xml::native_named(property, names, &available) {
                object.set(&property.name, value);
            } else {
                losses.push(
                    FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                        "product property {} has no writable typed payload",
                        property.id
                    )),
                );
            }
        }
        if let Some(source) = definition.source_name.as_deref() {
            let valid = {
                let mut characters = source.chars();
                characters
                    .next()
                    .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
                    && characters
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
            };
            if !valid || source != name {
                losses.push(
                    FreecadLossCode::ProductSourceNameNotTransferred.note(format!(
                        "product {} source name is emitted as {name}",
                        definition.id
                    )),
                );
            }
        }
        object.set("Label", xml::string("Label", label));
        // A part container states its part number in `Id`.
        let part_number = if definition.kind == ProductDefinitionKind::Part {
            "Id"
        } else {
            "PartNumber"
        };
        for (key, value) in [
            ("Description", definition.description.as_ref()),
            (part_number, definition.part_number.as_ref()),
        ] {
            if let Some(value) = value {
                object.set(key, xml::string(key, value));
            } else if object
                .properties
                .get(key)
                .and_then(|xml| xml::property_string_value(xml))
                .is_some_and(|text| !text.is_empty())
            {
                object.properties.shift_remove(key);
            }
        }
        for (key, value) in &definition.bom_properties {
            object.set(key.as_str(), xml::string(key.as_str(), value));
        }
        let mut members: Vec<_> = definition
            .bodies
            .iter()
            .filter_map(|body| body_names.get(body.as_str()))
            .filter(|member| **member != name)
            .cloned()
            .collect();
        for property in native_properties.iter().filter(|property| {
            definition.native_ref.as_deref() == Some(property.owner.as_str())
                && property.name == "Group"
        }) {
            for reference in property
                .links()
                .iter()
                .flatten()
                .filter(|reference| reference.document().is_none())
            {
                let Some(target) = reference.object() else {
                    continue;
                };
                // Canonical occurrence parents own instance membership. A
                // plain group parents no occurrence, so its native list keeps
                // members whose occurrences stay at the root. Other typed
                // members (features, sketches, workbench objects) have no
                // neutral product-membership lane and remain native content.
                if ir.model.occurrences.iter().any(|occurrence| {
                    occurrence.native_ref.as_deref() == Some(target)
                        && (definition.kind != ProductDefinitionKind::Group
                            || matches!(occurrence.parent, OccurrenceParent::Occurrence { .. }))
                }) {
                    continue;
                }
                if let Some(member) = names.get(target).filter(|member| **member != name) {
                    if !members.contains(member) {
                        members.push(member.clone());
                    }
                }
            }
        }
        if definition.kind != ProductDefinitionKind::Object {
            groups.listed.insert(
                name.clone(),
                native_properties
                    .iter()
                    .filter(|property| {
                        definition.native_ref.as_deref() == Some(property.owner.as_str())
                            && property.name == "Group"
                    })
                    .flat_map(|property| property.links().iter().flatten())
                    .filter(|reference| reference.document().is_none())
                    .filter_map(|reference| reference.object().map(str::to_owned))
                    .collect(),
            );
            groups.members.insert(name.clone(), members);
        }
        if let Some(native_ref) = &definition.native_ref {
            names.insert(native_ref.clone(), name);
        }
    }
    Ok(groups)
}

/// A `FreeCAD` Placement, which is rigid: any other transform keeps its
/// translation, and `code` names `owner`.
fn placement(
    transform: cadmpeg_ir::transform::Transform,
    owner: &str,
    code: FreecadLossCode,
    losses: &mut Vec<LossNote>,
) -> String {
    let transform = if transform.is_proper_rigid() {
        transform
    } else {
        losses.push(code.note(format!("{owner} affine placement is reduced to its translation because FreeCAD Placement is rigid")));
        let rows = transform.affine_rows();
        let translation = cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, rows[0][3]],
            [0.0, 1.0, 0.0, rows[1][3]],
            [0.0, 0.0, 1.0, rows[2][3]],
        ]);
        debug_assert!(
            translation.is_some(),
            "a finite placement has a finite translation"
        );
        translation.unwrap_or(transform)
    };
    super::design::placement(transform)
}

/// `FreeCAD` stores a rigid Placement and diagonal `ScaleVector`. Split an
/// orthogonal affine transform into those two facts without storing another
/// neutral scale value. Shear remains an explicitly reported reduction.
fn split_scale(
    transform: cadmpeg_ir::transform::Transform,
) -> Option<(cadmpeg_ir::transform::Transform, [f64; 3])> {
    let mut rows = transform.affine_rows();
    let mut scale = std::array::from_fn(|column| {
        rows.iter()
            .map(|row| row[column] * row[column])
            .sum::<f64>()
            .sqrt()
    });
    if scale
        .iter()
        .any(|value| !value.is_finite() || *value == 0.0)
    {
        return None;
    }
    for row in &mut rows {
        for column in 0..3 {
            row[column] /= scale[column];
        }
    }
    let mut rigid = cadmpeg_ir::transform::Transform::affine(rows)?;
    if !rigid.is_proper_rigid() {
        for row in &mut rows {
            row[2] = -row[2];
        }
        scale[2] = -scale[2];
        rigid = cadmpeg_ir::transform::Transform::affine(rows)?;
    }
    rigid.is_proper_rigid().then_some((rigid, scale))
}

fn replay_list_properties(
    ir: &CadIr,
    occurrence: &cadmpeg_ir::products::Occurrence,
    object: &mut Object,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(owner) = occurrence.native_ref.as_deref() else {
        return Ok(());
    };
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let available = super::xml::retained_entry_names(namespace);
    let properties = namespace.arena_as::<crate::native::PropertyRecord>("properties")?;
    for property in properties
        .iter()
        .filter(|property| property.owner == owner)
        .filter(|property| {
            matches!(
                property.name.as_str(),
                "ElementList" | "PlacementList" | "ScaleList"
            )
        })
        .filter(|property| xml::validate(property, &available).is_ok())
    {
        if object.properties.contains_key(&property.name) {
            continue;
        }
        match super::xml::native_named(property, names, &available) {
            Some(value) => object.set(&property.name, value),
            None => losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "link list property {} has no writable typed payload",
                    property.id
                )),
            ),
        }
    }
    Ok(())
}

fn link_state(
    object: &mut Object,
    reference: &cadmpeg_ir::products::LinkState,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) {
    if let Some(claim) = reference.claim_child() {
        bool_property(object, "LinkClaimChild", claim);
    }
    if let Some(component) = reference.element_component() {
        let elements: Vec<String> = names.get(component.as_str()).cloned().into_iter().collect();
        if elements.is_empty() {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "link {} element component {component} is absent and is omitted",
                object.name
            )));
        }
        object.set("ElementList", link_list("ElementList", &elements));
    }
    if let Some(copy) = reference.copy_on_change() {
        let policy = match &copy.policy {
            CopyOnChangePolicy::Disabled => "0",
            CopyOnChangePolicy::Enabled => "1",
            CopyOnChangePolicy::Owned => "2",
            CopyOnChangePolicy::Tracking => "3",
            CopyOnChangePolicy::Native(value) => value,
        };
        object.set(
            "LinkCopyOnChange",
            xml::scalar(
                "LinkCopyOnChange",
                "App::PropertyEnumeration",
                "Integer",
                policy,
            ),
        );
        for (key, value, kind) in [
            (
                "LinkCopyOnChangeSource",
                copy.source.as_ref(),
                "App::PropertyXLink",
            ),
            (
                "LinkCopyOnChangeGroup",
                copy.group.as_ref(),
                "App::PropertyLink",
            ),
        ] {
            if let Some(reference) = value {
                let (name, file) = prototype(reference, &object.name, names, losses);
                let content = if kind == "App::PropertyLink" && file.is_empty() {
                    xml::element("Link", &[("value", name)])
                } else if kind == "App::PropertyLink" {
                    losses.push(
                        FreecadLossCode::ExportFeatureNotTransferred
                            .note("external copy-on-change group is omitted"),
                    );
                    continue;
                } else {
                    xlink(&name, &file, &[])
                };
                object.set(key, xml::property(key, kind, &content));
            }
        }
        if let Some(touched) = copy.touched {
            bool_property(object, "LinkCopyOnChangeTouched", touched);
        }
    }
}

fn order_siblings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let mut siblings = Vec::new();
    let mut slots = Vec::new();
    ctx.reserve_working(
        &mut siblings,
        ir.model.occurrences.len(),
        "FreeCAD occurrence sibling order keys",
    )?;
    ctx.reserve_working(
        &mut slots,
        ir.model.occurrences.len(),
        "FreeCAD occurrence sibling order permutation",
    )?;
    for (position, object) in objects.iter().enumerate() {
        let owner = ir
            .model
            .occurrences
            .iter()
            .enumerate()
            .filter(|(_, occurrence)| names.get(occurrence.id.as_str()) == Some(&object.name))
            .min_by_key(|(index, occurrence)| (occurrence.ordinal, *index));
        if let Some((index, occurrence)) = owner {
            let parent = match &occurrence.parent {
                OccurrenceParent::Root {} => None,
                OccurrenceParent::Occurrence { occurrence } => Some(occurrence.as_str()),
            };
            siblings.push((parent, occurrence.ordinal, index, position));
        }
    }
    // Keep each sibling set in its existing output slots while matching
    // authored order (#238 §5.5); native Group lists already carry that order.
    siblings.sort_unstable_by_key(|(parent, _, _, position)| (*parent, *position));
    for (source, (_, _, _, position)) in siblings.iter_mut().enumerate() {
        slots.push((*position, source));
        *position = source;
    }
    siblings.sort_unstable_by_key(|(parent, ordinal, index, _)| (*parent, *ordinal, *index));
    for (destination, (_, _, _, source)) in siblings.into_iter().enumerate() {
        slots[source].1 = destination;
    }
    for index in 0..slots.len() {
        while slots[index].1 != index {
            let destination = slots[index].1;
            objects.swap(slots[index].0, slots[destination].0);
            let displaced = slots[destination].1;
            slots[destination].1 = slots[index].1;
            slots[index].1 = displaced;
        }
    }
    Ok(())
}

pub(super) fn write(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    body_names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Groups {
        members: mut groups,
        listed,
    } = definitions(ir, objects, names, body_names, losses)?;
    let extras: HashMap<String, usize> = groups
        .iter()
        .map(|(name, members)| (name.clone(), members.len()))
        .collect();
    let mut used: HashSet<_> = objects.iter().map(|object| object.name.clone()).collect();
    // Root occurrence objects enter `<Objects>` in ascending occurrence
    // ordinal order, ties by arena order (#238 §5.5).
    let mut occurrences: Vec<_> = ir.model.occurrences.iter().enumerate().collect();
    occurrences.sort_by_key(|(index, occurrence)| (occurrence.ordinal, *index));
    // Linked prototype placements resolve per prototype after every Link is
    // emitted, so agreement spans the whole prototype use.
    let mut linked_uses: Vec<(String, String, String, cadmpeg_ir::transform::Transform)> =
        Vec::new();
    // One `App::Link` holds every element occurrence of an array link; its own
    // placement is the native link placement the elements compose with.
    let native_nodes = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::ProductNodeRecord>("product_nodes"))
        .transpose()?
        .unwrap_or_default();
    let mut shared_uses = HashMap::<&str, usize>::new();
    for occurrence in &ir.model.occurrences {
        if let Some(native) = occurrence.native_ref.as_deref() {
            *shared_uses.entry(native).or_default() += 1;
        }
    }
    let native_properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    let mut emitted_links = HashMap::<String, String>::new();
    let mut array_elements = HashMap::<String, Vec<String>>::new();
    let native_types = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::ObjectRecord>("objects"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|object| (object.id().as_str().to_owned(), object.type_name.clone()))
        .collect::<HashMap<_, _>>();
    for (_, occurrence) in occurrences {
        let array = occurrence
            .native_ref
            .as_deref()
            .is_some_and(|native| shared_uses.get(native).copied().unwrap_or(0) > 1);
        // Container occurrences are the placement of their own definition,
        // rather than an additional use of that same container.
        let container = match &occurrence.prototype {
            PrototypeReference::Local { definition } => ir
                .model
                .product_definitions
                .iter()
                .find(|candidate| candidate.id == *definition)
                .filter(|definition| {
                    definition.native_ref.is_some()
                        && definition.native_ref == occurrence.native_ref
                })
                .and_then(|definition| names.get(definition.id.as_str()))
                .cloned(),
            _ => None,
        };
        // A link that is also another link's prototype is declared as a
        // definition too; that definition's object is this link.
        let link_definition = occurrence
            .native_ref
            .as_ref()
            .and_then(|native| {
                ir.model.product_definitions.iter().find(|definition| {
                    definition.kind == ProductDefinitionKind::Object
                        && definition.bodies.is_empty()
                        && definition.native_ref.as_ref() == Some(native)
                })
            })
            .and_then(|definition| names.get(definition.id.as_str()))
            .cloned();
        let peer = occurrence
            .native_ref
            .as_ref()
            .and_then(|native| emitted_links.get(native))
            .cloned();
        let name = if let Some(name) = container {
            name
        } else if let Some(name) = peer {
            name
        } else if let Some(name) = link_definition {
            if let Some(object) = objects.iter_mut().find(|object| object.name == name) {
                object.kind = occurrence
                    .native_ref
                    .as_ref()
                    .and_then(|native| native_types.get(native))
                    .filter(|kind| kind.as_str() == "App::LinkElement")
                    .map_or_else(|| "App::Link".to_owned(), Clone::clone);
            }
            name
        } else {
            let proposed = names
                .get(occurrence.id.as_str())
                .cloned()
                .unwrap_or_else(|| format!("Instance{}", objects.len()));
            let name = fresh_name(&proposed, &mut used);
            let mut object = Object::new(
                name.clone(),
                "App::Link",
                occurrence.name.as_deref().unwrap_or_default(),
            );
            // FreeCAD restores an absent Label as the object name.
            if occurrence.name.is_none() {
                object.properties.shift_remove("Label");
            }
            objects.push(object);
            name
        };
        names.insert(occurrence.id.as_str().to_owned(), name.clone());
        let first_use = match occurrence.native_ref.as_ref() {
            Some(native) => emitted_links.insert(native.clone(), name.clone()).is_none(),
            None => true,
        };
        let missing_prototype = matches!(
            &occurrence.prototype,
            PrototypeReference::Local { definition } if !names.contains_key(definition.as_str())
        );
        let Some(object) = objects.iter_mut().find(|object| object.name == name) else {
            debug_assert!(false, "instance output is absent");
            continue;
        };
        if array {
            let component = occurrence
                .link
                .as_ref()
                .and_then(cadmpeg_ir::products::LinkState::element_component);
            if let Some(component) = component {
                match names.get(component.as_str()) {
                    Some(element) => array_elements
                        .entry(name.clone())
                        .or_default()
                        .push(element.clone()),
                    None if !first_use => {
                        losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                            "link {name} element component {component} is absent and is omitted"
                        )));
                    }
                    None => {}
                }
            }
            if first_use {
                let (prototype, file) =
                    prototype(&occurrence.prototype, occurrence.id.as_str(), names, losses);
                object.set(
                    "LinkedObject",
                    xml::property(
                        "LinkedObject",
                        "App::PropertyXLink",
                        &xlink(&prototype, &file, &[]),
                    ),
                );
                if let Some(reference) = &occurrence.link {
                    link_state(object, reference, names, losses);
                }
                replay_list_properties(ir, occurrence, object, names, losses)?;
                if let Some(frame) = native_nodes
                    .iter()
                    .find(|node| occurrence.native_ref.as_deref() == Some(node.object.as_str()))
                    .and_then(crate::native::ProductNodeRecord::local_transform)
                {
                    let (rigid, _) =
                        split_scale(frame.transform()).unwrap_or((frame.transform(), [1.0; 3]));
                    object.set(
                        "Placement",
                        placement(
                            rigid,
                            occurrence.id.as_str(),
                            FreecadLossCode::ProductPlacementNotRigid,
                            losses,
                        ),
                    );
                }
            }
            continue;
        }
        if matches!(object.kind.as_str(), "App::Link" | "App::LinkElement") {
            let (prototype, file) =
                prototype(&occurrence.prototype, occurrence.id.as_str(), names, losses);
            let subelements = occurrence
                .link
                .as_ref()
                .map_or(&[][..], |link| link.linked_subelements());
            object.set(
                "LinkedObject",
                xml::property(
                    "LinkedObject",
                    "App::PropertyXLink",
                    &xlink(&prototype, &file, subelements),
                ),
            );
            if let Some(reference) = &occurrence.link {
                link_state(object, reference, names, losses);
            }
            // Array element lists have no neutral home: the occurrence model
            // carries the link policy but not the per-element placements and
            // scales. A rebuilt link replays its native list properties with
            // their retained entries, so the placement graph re-decodes. A
            // neutral element component already owns `ElementList` above.
            replay_list_properties(ir, occurrence, object, names, losses)?;
            // Links without a linked prototype placement never transform
            // their prototype; linked uses resolve their policy below. An
            // external prototype's placement is unknown, so a transforming
            // link to it has no transform of its own.
            let external_transforming = occurrence.transform.is_none()
                && matches!(occurrence.prototype, PrototypeReference::External { .. });
            if external_transforming {
                bool_property(object, "LinkTransform", true);
            } else if occurrence.linked_prototype.is_none() || missing_prototype {
                bool_property(object, "LinkTransform", false);
            }
            if let Some(linked) = occurrence.linked_prototype.filter(|_| !missing_prototype) {
                linked_uses.push((prototype, name.clone(), occurrence.id.to_string(), linked));
            }
        }
        // Links with a linked prototype placement resolve below; every other
        // object carries the occurrence's own transform, never the composed
        // effective transform. An occurrence with no placement omits
        // Placement (#238 §5.5, #213).
        let deferred = occurrence.linked_prototype.is_some()
            && matches!(object.kind.as_str(), "App::Link" | "App::LinkElement")
            && !missing_prototype;
        if !deferred {
            let transform = if missing_prototype && occurrence.linked_prototype.is_some() {
                losses.push(FreecadLossCode::ProductLinkedPlacementFolded.note(format!(
                    "occurrence {} has no prototype object for its linked placement",
                    occurrence.id
                )));
                occurrence.effective_transform().ok().flatten()
            } else {
                occurrence.transform
            };
            if let Some(transform) = transform {
                let (rigid, scale) = split_scale(transform).unwrap_or((transform, [1.0; 3]));
                let written = placement(
                    rigid,
                    occurrence.id.as_str(),
                    FreecadLossCode::ProductPlacementNotRigid,
                    losses,
                );
                // A sketch or feature sharing the object writes the same
                // Placement; a value that differs from the stored one wins.
                let stored = occurrence.native_ref.as_deref().and_then(|owner| {
                    native_properties
                        .iter()
                        .find(|property| property.owner == owner && property.name == "Placement")
                        .and_then(xml::native_placement)
                });
                let edited_elsewhere = stored.is_some_and(|stored| {
                    xml::written_placement(&written) == Some(stored)
                        && object
                            .properties
                            .get("Placement")
                            .and_then(|existing| xml::written_placement(existing))
                            .is_some_and(|existing| existing != stored)
                });
                if !edited_elsewhere {
                    object.set("Placement", written);
                }
                // Only a link stores a scale beside its Placement.
                let link = matches!(object.kind.as_str(), "App::Link" | "App::LinkElement");
                if scale != [1.0; 3] && !link {
                    losses.push(FreecadLossCode::ProductPlacementNotRigid.note(format!(
                        "{} scale is dropped because FreeCAD Placement is rigid",
                        occurrence.id
                    )));
                } else if scale != [1.0; 3] {
                    object.set(
                        "ScaleVector",
                        xml::property(
                            "ScaleVector",
                            "App::PropertyVector",
                            &xml::element(
                                "PropertyVector",
                                &[
                                    ("valueX", scale[0].to_string()),
                                    ("valueY", scale[1].to_string()),
                                    ("valueZ", scale[2].to_string()),
                                ],
                            ),
                        ),
                    );
                }
            }
        }
    }
    for (name, elements) in array_elements {
        if let Some(object) = objects.iter_mut().find(|object| object.name == name) {
            object.set("ElementList", link_list("ElementList", &elements));
        }
    }
    resolve_linked_transforms(ir, objects, &linked_uses, losses);
    let mut children: Vec<_> = ir.model.occurrences.iter().collect();
    children.sort_by_key(|occurrence| occurrence.ordinal);
    for occurrence in children {
        if let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent {
            let Some(parent_name) = names.get(parent.as_str()) else {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "occurrence {} references absent parent {parent} and is written at the root",
                    occurrence.id
                )));
                continue;
            };
            let Some(child) = names.get(occurrence.id.as_str()) else {
                debug_assert!(false, "instance child has no output identity");
                continue;
            };
            let members = groups.entry(parent_name.clone()).or_default();
            if !members.contains(child) {
                members.push(child.clone());
            }
        }
    }
    for (name, members) in &mut groups {
        let Some(listed) = listed.get(name) else {
            continue;
        };
        let listed: Vec<String> = listed
            .iter()
            .filter_map(|target| {
                names.get(target).or_else(|| {
                    ir.model
                        .occurrences
                        .iter()
                        .find(|occurrence| occurrence.native_ref.as_deref() == Some(target))
                        .and_then(|occurrence| names.get(occurrence.id.as_str()))
                })
            })
            .cloned()
            .collect();
        let (own, children) = members.split_at(extras.get(name).copied().unwrap_or(0));
        *members = slotted(own, children, &listed);
    }
    let mut parents = HashMap::<String, String>::new();
    for (name, mut members) in groups {
        for member in &mut members {
            if parents.get(member).is_some_and(|parent| parent != &name) {
                let original = member.clone();
                let alias = fresh_name(&format!("{original}_Link"), &mut used);
                let mut link = Object::new(alias.clone(), "App::Link", &original);
                link.set(
                    "LinkedObject",
                    xml::property(
                        "LinkedObject",
                        "App::PropertyXLink",
                        &xlink(&original, "", &[]),
                    ),
                );
                bool_property(&mut link, "LinkTransform", false);
                objects.push(link);
                let mut charged = false;
                if let Some(definition) = ir
                    .model
                    .product_definitions
                    .iter()
                    .find(|definition| names.get(definition.id.as_str()) == Some(&name))
                {
                    for body in &definition.bodies {
                        if body_names.get(body.as_str()) == Some(&original) {
                            charged = true;
                            losses.push(FreecadLossCode::ProductMembershipNotTransferred.note(
                                format!(
                                    "product {} body {} is represented by link {alias}",
                                    definition.id, body
                                ),
                            ));
                        }
                    }
                }
                if !charged {
                    let source = ir
                        .model
                        .occurrences
                        .iter()
                        .find(|occurrence| names.get(occurrence.id.as_str()) == Some(&original))
                        .map_or(original.as_str(), |occurrence| occurrence.id.as_str());
                    losses.push(
                        FreecadLossCode::ProductMembershipNotTransferred.note(format!(
                            "member {source} in group {name} is represented by link {alias}"
                        )),
                    );
                }
                *member = alias;
            }
            parents.insert(member.clone(), name.clone());
        }
        let Some(object) = objects.iter_mut().find(|object| object.name == name) else {
            debug_assert!(false, "product group has no output object");
            continue;
        };
        object.set("Group", link_list("Group", &members));
    }
    joints::write(ir, objects, names, losses)?;
    order_siblings(ctx, ir, objects, names)?;
    Ok(())
}

/// Resolve linked prototype placements per prototype.
///
/// When every use of one prototype agrees on the linked placement and the
/// prototype object carries (or can carry) that same Placement, the Links
/// keep `LinkTransform=true` with the prototype Placement and their own
/// local Placements. Otherwise the effective Placement is baked into each
/// Link with `LinkTransform=false` and a fold loss (#238 §5.5).
fn resolve_linked_transforms(
    ir: &CadIr,
    objects: &mut [Object],
    linked_uses: &[(String, String, String, cadmpeg_ir::transform::Transform)],
    losses: &mut Vec<LossNote>,
) {
    let mut groups: BTreeMap<&str, Vec<(String, String, cadmpeg_ir::transform::Transform)>> =
        BTreeMap::new();
    for (prototype, link, occurrence, linked) in linked_uses {
        groups.entry(prototype.as_str()).or_default().push((
            link.clone(),
            occurrence.clone(),
            *linked,
        ));
    }
    for (prototype, uses) in groups {
        let agreed = uses
            .iter()
            .all(|(_, _, linked)| Some(linked) == uses.first().map(|(_, _, first)| first))
            .then(|| uses.first().map(|(_, _, first)| *first))
            .flatten();
        let rendered = agreed.map(|linked| {
            placement(
                linked,
                prototype,
                FreecadLossCode::ProductPlacementNotRigid,
                losses,
            )
        });
        let consistent = match (
            &rendered,
            objects.iter().find(|object| object.name == prototype),
        ) {
            (Some(rendered), Some(object)) => object
                .properties
                .get("Placement")
                .is_none_or(|current| current == rendered),
            (Some(_), None) => true,
            _ => false,
        };
        if consistent {
            if let (Some(rendered), Some(object)) = (
                rendered,
                objects.iter_mut().find(|object| object.name == prototype),
            ) {
                object.set("Placement", rendered);
            }
            for (link, occurrence, _) in &uses {
                let Some(current) = ir
                    .model
                    .occurrences
                    .iter()
                    .find(|candidate| candidate.id.as_str() == occurrence.as_str())
                else {
                    debug_assert!(false, "linked instance has no occurrence");
                    continue;
                };
                let Some(object) = objects.iter_mut().find(|object| object.name == *link) else {
                    debug_assert!(false, "linked instance output is absent");
                    continue;
                };
                bool_property(object, "LinkTransform", true);
                if let Some(transform) = current.transform {
                    let (rigid, scale) = split_scale(transform).unwrap_or((transform, [1.0; 3]));
                    object.set(
                        "Placement",
                        placement(
                            rigid,
                            occurrence.as_str(),
                            FreecadLossCode::ProductPlacementNotRigid,
                            losses,
                        ),
                    );
                    if scale != [1.0; 3] {
                        object.set(
                            "ScaleVector",
                            xml::property(
                                "ScaleVector",
                                "App::PropertyVector",
                                &xml::element(
                                    "PropertyVector",
                                    &[
                                        ("valueX", scale[0].to_string()),
                                        ("valueY", scale[1].to_string()),
                                        ("valueZ", scale[2].to_string()),
                                    ],
                                ),
                            ),
                        );
                    }
                }
            }
            continue;
        }
        losses.push(FreecadLossCode::ProductLinkedPlacementFolded.note(format!(
            "prototype {prototype} linked placements disagree and are folded into occurrence placements where possible"
        )));
        for (link, occurrence, _) in &uses {
            let Some(current) = ir
                .model
                .occurrences
                .iter()
                .find(|candidate| candidate.id.as_str() == occurrence.as_str())
            else {
                debug_assert!(false, "linked instance has no occurrence");
                continue;
            };
            let effective = match current.effective_transform() {
                Ok(effective) => effective,
                Err(_) => {
                    losses.push(FreecadLossCode::ProductLinkedPlacementFolded.note(format!(
                        "occurrence {occurrence} linked placement cannot be composed and is omitted"
                    )));
                    current.transform
                }
            };
            let Some(object) = objects.iter_mut().find(|object| object.name == *link) else {
                debug_assert!(false, "linked instance output is absent");
                continue;
            };
            bool_property(object, "LinkTransform", false);
            object.properties.shift_remove("ScaleVector");
            let Some(effective) = effective else {
                object.properties.shift_remove("Placement");
                continue;
            };
            let (rigid, scale) = split_scale(effective).unwrap_or((effective, [1.0; 3]));
            object.set(
                "Placement",
                placement(
                    rigid,
                    occurrence.as_str(),
                    FreecadLossCode::ProductPlacementNotRigid,
                    losses,
                ),
            );
            if scale != [1.0; 3] {
                object.set(
                    "ScaleVector",
                    xml::property(
                        "ScaleVector",
                        "App::PropertyVector",
                        &xml::element(
                            "PropertyVector",
                            &[
                                ("valueX", scale[0].to_string()),
                                ("valueY", scale[1].to_string()),
                                ("valueZ", scale[2].to_string()),
                            ],
                        ),
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
