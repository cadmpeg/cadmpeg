// SPDX-License-Identifier: Apache-2.0
//! Typed native sketch records have content; retained XML does not.
use super::super::super::xml;
use super::constraints::Indices;
use crate::native::PropertyRecord;
use cadmpeg_ir::{
    document::CadIr,
    sketches::{
        SketchConstraint, SketchConstraintDefinitionInput, SketchEntity, SketchGeometryDefinition,
    },
};
fn typed_row(ir: &CadIr, reference: &str, ordinal: usize, tag: &str) -> Option<String> {
    let properties = ir
        .native
        .namespace("fcstd")?
        .arena_as::<PropertyRecord>("properties")
        .ok()?;
    let property = properties
        .iter()
        .find(|property| property.id == reference)?;
    // native() serializes only the validated typed value tree and rejects opaque entries.
    let generated = xml::native(property).ok()??;
    let document = roxmltree::Document::parse(&generated).ok()?;
    let row = document
        .descendants()
        .filter(|node| node.has_tag_name(tag))
        .nth(ordinal)?;
    Some(serialize(row))
}
fn serialize(node: roxmltree::Node<'_, '_>) -> String {
    // This tree was generated above from typed values, never read from retained XML.
    node.document().input_text()[node.range()].to_owned()
}
pub(super) fn geometry(ir: &CadIr, entity: &SketchEntity) -> Option<String> {
    let SketchGeometryDefinition::Native { native_kind } = entity.geometry.definition() else {
        return None;
    };
    let suffix = entity
        .id()
        .as_str()
        .rsplit(':')
        .next()?
        .parse::<usize>()
        .ok()?;
    let ordinal = if entity.id().as_str().contains(":external:") {
        suffix.checked_add(2)?
    } else {
        suffix.checked_sub(1)?
    };
    let row = typed_row(ir, entity.native_ref.as_deref()?, ordinal, "Geometry")?;
    let document = roxmltree::Document::parse(&row).ok()?;
    let root = document.root_element();
    if root.attribute("type") != Some(native_kind.as_str()) {
        return None;
    }
    let carrier = root
        .children()
        .filter(|child| child.is_element() && !child.has_tag_name("Construction"))
        .map(serialize)
        .collect::<String>();
    Some(format!(
        "<Geometry type=\"{}\">{carrier}<Construction value=\"{}\"/></Geometry>",
        xml::escaped(native_kind.as_str()),
        u8::from(entity.construction)
    ))
}
pub(super) fn constraint(
    ir: &CadIr,
    constraint: &SketchConstraint,
    indices: &Indices,
) -> Option<String> {
    let SketchConstraintDefinitionInput::Native {
        entities,
        parameter,
        ..
    } = constraint.definition.kind()
    else {
        return None;
    };
    let ordinal = constraint
        .id
        .as_str()
        .rsplit(':')
        .next()?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    let row = typed_row(ir, constraint.native_ref.as_deref()?, ordinal, "Constrain")?;
    let document = roxmltree::Document::parse(&row).ok()?;
    let root = document.root_element();
    let mut attributes = root
        .attributes()
        .map(|a| (a.name().to_owned(), a.value().to_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    // A native relation with unresolved positive indices cannot be relocated safely.
    // All typed referents must still exist and source numbering must agree.
    for key in ["First", "Second", "Third"] {
        if let Some(value) = attributes.get_mut(key) {
            let old = value.parse::<i64>().ok()?;
            if old >= 0 {
                let id = entities.iter().find(|id| {
                    id.as_str()
                        .rsplit(':')
                        .next()
                        .and_then(|v| v.parse::<i64>().ok())
                        == Some(old + 1)
                })?;
                *value = indices.get(id.as_str())?.0.to_string();
            }
        }
    }
    if attributes.contains_key("ElementIds") {
        return None;
    }
    if let Some(parameter) = parameter {
        use cadmpeg_ir::features::ParameterValue;
        let p = ir.model.parameters.iter().find(|p| &p.id == parameter)?;
        let value = match p.value.as_ref()? {
            ParameterValue::Length(v) => v.get(),
            ParameterValue::Angle(v) => v.get(),
            ParameterValue::Real(v) => v.get(),
            _ => return None,
        };
        attributes.insert("Value".into(), value.to_string());
    }
    let mut common = Vec::new();
    super::constraints::common(ir, constraint, &mut common);
    for (key, value) in common {
        attributes.insert(key.to_owned(), value);
    }
    Some(xml::element(
        "Constrain",
        &attributes
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect::<Vec<_>>(),
    ))
}

/// The typed cache-to-link identity disambiguates equal selectors on distinct objects.
pub(super) fn cached_reference(ir: &CadIr, entity: &SketchEntity) -> Option<String> {
    if !entity.id().as_str().contains(":external:") {
        return None;
    }
    let ordinal = entity
        .id()
        .as_str()
        .rsplit(':')
        .next()?
        .parse::<usize>()
        .ok()?
        .checked_add(2)?;
    let row = typed_row(ir, entity.native_ref.as_deref()?, ordinal, "Geometry")?;
    let tree = roxmltree::Document::parse(&row).ok()?;
    tree.root_element()
        .attribute("ref")
        .or_else(|| {
            tree.descendants()
                .find(|node| {
                    node.has_tag_name("GeoExtension")
                        && node.attribute("type") == Some("Sketcher::ExternalGeometryExtension")
                })
                .and_then(|node| node.attribute("Ref"))
        })
        .map(ToOwned::to_owned)
}
