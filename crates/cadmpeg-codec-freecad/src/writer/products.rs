// SPDX-License-Identifier: Apache-2.0
//! Neutral product definitions, placed instances, and assembly constraints.

mod joints;
pub(crate) mod metadata;

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::products::{
    AssemblyGraph, CopyOnChangePolicy, ExternalDocument, OccurrenceParent, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, HashMap, HashSet};

fn group(members: &[String]) -> String {
    let content = members
        .iter()
        .map(|name| xml::element("Link", &[("value", name.clone())]))
        .collect::<String>();
    xml::property(
        "Group",
        "App::PropertyLinkList",
        &format!("<LinkList count=\"{}\">{content}</LinkList>", members.len()),
    )
}

fn bool_property(object: &mut Object, name: &str, value: bool) {
    object.set(
        name,
        xml::scalar(name, "App::PropertyBool", "Bool", u8::from(value)),
    );
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
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(String, String), CodecError> {
    Ok(match reference {
        PrototypeReference::Local { definition } => (
            names
                .get(definition.as_str())
                .cloned()
                .ok_or_else(|| CodecError::malformed("occurrence references absent prototype"))?,
            String::new(),
        ),
        PrototypeReference::External { document, object } => (
            object.clone().unwrap_or_default(),
            external(document, losses),
        ),
        PrototypeReference::Unresolved {} => (String::new(), String::new()),
    })
}

fn external(document: &ExternalDocument, losses: &mut Vec<LossNote>) -> String {
    match document {
        ExternalDocument::Path { path } => path.as_str().to_owned(),
        ExternalDocument::DocumentId { document_id } => {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "external document identity {document_id} is written as a file token"
            )));
            document_id.as_str().to_owned()
        }
        ExternalDocument::Missing {} => String::new(),
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

fn definitions(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    body_names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<BTreeMap<String, Vec<String>>, CodecError> {
    let mut groups = BTreeMap::new();
    let native_properties = ir
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
    for definition in &ir.model.product_definitions {
        let name = names
            .get(definition.id.as_str())
            .cloned()
            .ok_or_else(|| CodecError::malformed("product has no output identity"))?;
        let position = objects.iter().position(|object| object.name == name);
        let kind = match definition.kind {
            ProductDefinitionKind::Part => "App::Part",
            ProductDefinitionKind::Group => "App::DocumentObjectGroup",
            ProductDefinitionKind::LinkGroup => "App::LinkGroup",
            ProductDefinitionKind::Object => "Part::Feature",
        };
        let label = definition
            .label
            .as_deref()
            .or(definition.source_name.as_deref())
            .unwrap_or(definition.id.as_str());
        let object = match position {
            Some(index) => &mut objects[index],
            None => {
                objects.push(Object::new(name.clone(), kind, label));
                objects
                    .last_mut()
                    .ok_or_else(|| CodecError::malformed("product output is absent"))?
            }
        };
        if definition.kind != ProductDefinitionKind::Object {
            object.kind = kind.into();
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
            if let Some(value) = xml::native_named(property, names)? {
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
        if definition
            .source_name
            .as_ref()
            .is_some_and(|source| source != &name)
        {
            losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                "product {} source name is emitted as {name}",
                definition.id
            )));
        }
        object.set("Label", xml::string("Label", label));
        for (key, value) in [
            ("Description", definition.description.as_ref()),
            ("PartNumber", definition.part_number.as_ref()),
        ] {
            object.properties.remove(key);
            if let Some(value) = value {
                object.set(key, xml::string(key, value));
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
                // Canonical occurrence parents own instance membership. Other
                // typed members (features, sketches, workbench objects) have
                // no neutral product-membership lane and remain native content.
                if ir
                    .model
                    .occurrences
                    .iter()
                    .any(|occurrence| occurrence.native_ref.as_deref() == Some(target))
                {
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
            groups.insert(name.clone(), members);
        }
        if let Some(native_ref) = &definition.native_ref {
            names.insert(native_ref.clone(), name);
        }
    }
    Ok(groups)
}

fn placement(
    transform: cadmpeg_ir::transform::Transform,
    owner: &str,
    losses: &mut Vec<LossNote>,
) -> Result<String, CodecError> {
    let transform = if transform.is_proper_rigid() {
        transform
    } else {
        losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!("{owner} affine placement is reduced to its translation because FreeCAD Placement is rigid")));
        let rows = transform.affine_rows();
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, rows[0][3]],
            [0.0, 1.0, 0.0, rows[1][3]],
            [0.0, 0.0, 1.0, rows[2][3]],
        ])
        .ok_or_else(|| CodecError::malformed("placement translation is non-finite"))?
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

fn link_state(
    object: &mut Object,
    reference: &cadmpeg_ir::products::LinkState,
    names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if let Some(claim) = reference.claim_child() {
        bool_property(object, "LinkClaimChild", claim);
    }
    if let Some(component) = reference.element_component() {
        let name = names
            .get(component.as_str())
            .ok_or_else(|| CodecError::malformed("link element component is absent"))?;
        object.set(
            "ElementList",
            xml::property(
                "ElementList",
                "App::PropertyLinkList",
                &format!(
                    "<LinkList count=\"1\">{}</LinkList>",
                    xml::element("Link", &[("value", name.clone())])
                ),
            ),
        );
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
                let (name, file) = prototype(reference, names, losses)?;
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
    Ok(())
}

pub(super) fn write(
    ir: &CadIr,
    objects: &mut Vec<Object>,
    names: &mut HashMap<String, String>,
    body_names: &HashMap<String, String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let graph = AssemblyGraph::new(&ir.model.occurrences).map_err(CodecError::malformed)?;
    let mut groups = definitions(ir, objects, names, body_names, losses)?;
    let mut used: HashSet<_> = objects.iter().map(|object| object.name.clone()).collect();
    for occurrence in &ir.model.occurrences {
        if graph.effective_transform(&occurrence.id).is_none() {
            names.remove(occurrence.id.as_str());
            losses.push(
                FreecadLossCode::ExportOccurrenceNotTransferred.note(format!(
                    "occurrence {} omitted because its placement or an ancestor placement is unknown",
                    occurrence.id
                )),
            );
            continue;
        }
        // Admission uses the whole parent chain; native Placement stays local.
        let Some(effective) = occurrence
            .effective_transform()
            .map_err(CodecError::malformed)?
        else {
            continue;
        };
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
        let name = if let Some(name) = container {
            name
        } else {
            let proposed = names
                .get(occurrence.id.as_str())
                .cloned()
                .unwrap_or_else(|| format!("Instance{}", objects.len()));
            let name = fresh_name(&proposed, &mut used);
            objects.push(Object::new(
                name.clone(),
                "App::Link",
                occurrence.name.as_deref().unwrap_or(occurrence.id.as_str()),
            ));
            name
        };
        names.insert(occurrence.id.as_str().to_owned(), name.clone());
        let object = objects
            .iter_mut()
            .find(|object| object.name == name)
            .ok_or_else(|| CodecError::malformed("instance output is absent"))?;
        if object.kind == "App::Link" {
            let (prototype, file) = prototype(&occurrence.prototype, names, losses)?;
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
            bool_property(object, "LinkTransform", false);
            if let Some(reference) = &occurrence.link {
                link_state(object, reference, names, losses)?;
            }
        }
        let (rigid, scale) = split_scale(effective).unwrap_or((effective, [1.0; 3]));
        object.set(
            "Placement",
            placement(rigid, occurrence.id.as_str(), losses)?,
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
    let mut children: Vec<_> = ir.model.occurrences.iter().collect();
    children.sort_by_key(|occurrence| occurrence.ordinal);
    for occurrence in children {
        if graph.effective_transform(&occurrence.id).is_none() {
            continue;
        }
        if let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent {
            let parent = names
                .get(parent.as_str())
                .ok_or_else(|| CodecError::malformed("instance references absent parent"))?;
            let child = names
                .get(occurrence.id.as_str())
                .ok_or_else(|| CodecError::malformed("instance child has no output identity"))?;
            let members = groups.entry(parent.clone()).or_default();
            if !members.contains(child) {
                members.push(child.clone());
            }
        }
    }
    for (name, members) in groups {
        let object = objects
            .iter_mut()
            .find(|object| object.name == name)
            .ok_or_else(|| CodecError::malformed("product group has no output object"))?;
        object.dependencies.clone_from(&members);
        object.set("Group", group(&members));
    }
    joints::write(ir, objects, names, losses)?;
    metadata::preserve(ir, objects, names)
}

#[cfg(test)]
mod tests;
