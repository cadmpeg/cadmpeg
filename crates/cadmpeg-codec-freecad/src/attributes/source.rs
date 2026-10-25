// SPDX-License-Identifier: Apache-2.0
//! Neutral attributes constructed by getters on their original admitted fields.

use super::*;
use crate::design::ExpressionBindings;
use crate::fidelity::ValueSourceRanges;
use crate::native::{ValueRecord, ValueSlots};
use crate::property_reader::PropertyReader;
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::{DecodeContext, RecordOwner, View};
use cadmpeg_ir::attributes::{AttributeExpression, AttributeNumericList, AttributePlacement};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn transfer_source(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    document: View<'_>,
    ranges: &BTreeMap<String, ValueSourceRanges>,
    reader: &PropertyReader<'_, '_>,
    bindings: &ExpressionBindings,
) -> Result<(Vec<LossNote>, Vec<crate::design::UnformedProjection>), CodecError> {
    let source = XmlSource::new(ctx, document)?;
    let mut indices = BTreeMap::new();
    for (index, element) in source.elements().iter().enumerate() {
        indices.insert(element.extent().start() - document.start(), index);
    }
    let mut projected = Vec::new();
    let mut unformed = Vec::new();
    for property in properties {
        if ir
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.native_ref.as_deref() == Some(property.id.as_str()))
        {
            continue;
        }
        let Some(ranges) = ranges.get(&property.id) else {
            continue;
        };
        let Some(index) = indices.get(&ranges.record_range().start) else {
            continue;
        };
        if let Some(attribute) = project_source(
            ctx, ir, objects, property, &source, *index, ranges, reader, bindings,
        )? {
            projected.push(attribute);
        } else if (property.body.is_none() || values(property).is_none())
            && projection_held(ir)
            && !consumed(ir, property, objects)
            && target(ir, &property.owner)?.is_some()
        {
            if let Some(id) = source_property_identity(ctx, &source, *index, property)? {
                reader.begin_projection(&property.owner)?;
                if property.type_name != "Mesh::PropertyMaterial"
                    || !reader.source_mesh_binding_failure(property)?
                {
                    reader.unavailable(property)?;
                }
                let homes = reader.take_unavailable();
                if !homes.is_empty() {
                    unformed.push(crate::design::UnformedProjection {
                        ids: vec![id],
                        homes,
                    });
                }
            }
        }
    }
    ir.model.attributes.extend(projected);
    Ok((Vec::new(), unformed))
}

/// Navigation selects a prospective value form; only actual failed source
/// helpers can sponsor the returned identity. No value read is claimed here.
pub(crate) fn source_property_identity(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    property: &PropertyRecord,
) -> Result<Option<String>, CodecError> {
    let extent = source.extent(index)?;
    let mut generic_value = false;
    let mut registered_json = false;
    let children_start = index
        .checked_add(1)
        .ok_or_else(|| CodecError::malformed("attribute source index overflows"))?;
    for (child, element) in source.elements().iter().enumerate().skip(children_start) {
        if element.extent().start() >= extent.end() {
            break;
        }
        let tag = match source.name(child) {
            Ok(tag) => tag,
            Err(CodecError::Malformed(_)) => continue,
            Err(error) => return Err(error),
        };
        generic_value |= matches!(
            tag,
            "String"
                | "Unicode"
                | "Uuid"
                | "Path"
                | "Bool"
                | "Integer"
                | "Int"
                | "Unsigned"
                | "I"
                | "PropertyColor"
                | "Float"
                | "Double"
                | "Quantity"
                | "PropertyFloat"
                | "BoolList"
                | "PropertyMatrix"
                | "PropertyRotation"
                | "PropertyPlacement"
                | "PropertyVector"
                | "Vector"
                | "StringList"
                | "IntegerList"
                | "IntegerSet"
        );
        if property.type_name == "App::PropertyPythonObject" && tag == "Python" {
            match peek_value_attribute(ctx, source, child, "json") {
                Ok(mode) => registered_json |= mode.as_deref() == Some("yes"),
                Err(CodecError::Malformed(_)) => {}
                Err(error) => return Err(error),
            }
        }
    }
    let identity = if property.owner == crate::native::native_id("document", "0") {
        document_property_identity_with_json_mode(property, registered_json)
    } else {
        property_identity_with_json_mode(property, registered_json)
    };
    Ok(identity.or_else(|| {
        property_identity_with_source_form(
            property,
            registered_json,
            generic_value && property.family != crate::native::PropertyFamily::Unknown,
        )
    }))
}

fn peek_value_attribute(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    local: &str,
) -> Result<Option<String>, CodecError> {
    let name = source
        .attribute_names(index)?
        .into_iter()
        .rev()
        .find(|key| {
            *key != "xmlns" && !key.starts_with("xmlns:") && key.rsplit(':').next() == Some(local)
        });
    name.map_or(Ok(None), |name| source.peek_attribute(ctx, index, name))
}

pub(crate) fn project_source(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    objects: &[ObjectRecord],
    property: &PropertyRecord,
    source: &XmlSource<'_>,
    index: usize,
    ranges: &ValueSourceRanges,
    reader: &PropertyReader<'_, '_>,
    bindings: &ExpressionBindings,
) -> Result<Option<SourceAttribute>, CodecError> {
    if !projection_held(ir)
        || consumed(ir, property, objects)
        || property.body.is_none()
        || property.is_transient()
    {
        return Ok(None);
    }
    let Some(target) = target(ir, &property.owner)? else {
        return Ok(None);
    };
    let numeric = numeric_kind(&property.type_name);
    let mesh_material = property.type_name == "Mesh::PropertyMaterial";
    if !numeric && !mesh_material && values(property).is_none() {
        return Ok(None);
    }
    let id = crate::native::model_id("attribute", &property.id, "property");
    let extent = source.extent(index)?;
    let ((name, kind, file), owner) = ctx.record_owner(&id, extent, || {
        let name = required(ctx, source, index, "name")?;
        let kind = required(ctx, source, index, "type")?;
        if kind != property.type_name {
            return Err(CodecError::malformed(
                "attribute runtime differs from its admitted source",
            ));
        }
        let file = if numeric || mesh_material {
            let root = source
                .children(index)
                .next()
                .ok_or_else(|| CodecError::malformed("numeric attribute has no source carrier"))?;
            crate::persistence::source::value_attribute(ctx, source, root, "file")?
        } else {
            None
        };
        Ok((name, kind, file))
    })?;
    let projected = if mesh_material {
        if file.as_deref().is_some_and(|file| {
            !file.is_empty() && property.side_entries().iter().any(|entry| entry == file)
        }) {
            mesh_material_values(ctx, reader, &owner, property)?
        } else {
            None
        }
    } else if numeric {
        let root = source
            .children(index)
            .next()
            .ok_or_else(|| CodecError::malformed("numeric attribute has no root"))?;
        let inline = matches!(
            kind.as_str(),
            "Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList"
        ) && file.is_none();
        let list = if inline {
            ctx.record_fragment(&owner, extent, || {
                inline_values(ctx, source, root, property, ranges)
            })?
        } else {
            numeric_values(
                ctx,
                reader,
                &owner,
                property,
                &kind,
                file.as_deref().is_some_and(str::is_empty),
            )?
        };
        Some(vec![AttributeValue::NumericList(list)])
    } else {
        ctx.record_fragment(&owner, extent, || {
            let current = read_property(ctx, source, index, property, ranges, bindings, ir)?;
            let mut projected = values(&current);
            if kind == "App::PropertyExpressionEngine" {
                if let Some(projected) = &mut projected {
                    let held = ir
                        .model
                        .parameters
                        .iter()
                        .map(|parameter| parameter.id.clone())
                        .collect::<BTreeSet<_>>();
                    let feature = ir.model.features.iter().find(|feature| {
                        feature.native_ref.as_deref() == Some(property.owner.as_str())
                    });
                    for value in projected {
                        if let AttributeValue::Expression(AttributeExpression {
                            expression, ..
                        }) = value
                        {
                            let text = expression.literal_text().unwrap_or_default();
                            *expression = bindings.expression(
                                &text,
                                feature.map(|feature| &feature.id),
                                &held,
                            )?;
                        }
                    }
                }
            }
            Ok(projected)
        })?
    };
    let Some(values) = projected else {
        ctx.retire_record_owner(&owner)?;
        return Ok(None);
    };
    Ok(Some(SourceAttribute {
        id: AttributeId::mint(id).map_err(CodecError::malformed)?,
        native_ref: Some(property.id.clone()),
        target,
        name,
        values,
    }))
}

fn mesh_material_values(
    ctx: &DecodeContext<'_>,
    reader: &PropertyReader<'_, '_>,
    owner: &RecordOwner<'_>,
    property: &PropertyRecord,
) -> Result<Option<Vec<AttributeValue>>, CodecError> {
    let Some(material) = reader.mesh_material_slots(property) else {
        return Ok(None);
    };
    let Some(binding) = reader.source_mesh_binding(owner, property)? else {
        if material.binding.is_some() {
            return Err(CodecError::malformed(
                "held mesh material binding has no source supplier",
            ));
        }
        return Ok(None);
    };
    if material.binding != Some(binding) {
        return Err(CodecError::malformed(
            "mesh material binding differs from its source",
        ));
    }
    let mut values = vec![AttributeValue::Integer(i64::from(binding))];
    macro_rules! array {
        ($role:expr, $original:expr, $variant:ident, $getter:ident) => {{
            let header = reader.source_mesh_header(owner, property, $role)?;
            let rows = match (header, $original) {
                (None, None) => None,
                (Some(header), Some(original)) if header.count() == original.len() => {
                    let mut rows =
                        ctx.alloc_filled(header.count(), None, "mesh material attribute slots")?;
                    for (ordinal, admitted) in original.iter().enumerate() {
                        if admitted.is_some() {
                            rows[ordinal] = Some(
                                reader
                                    .$getter(owner, property, $role, ordinal, &header)?
                                    .ok_or_else(|| {
                                        CodecError::malformed(
                                            "held mesh material row has no source supplier",
                                        )
                                    })?,
                            );
                        }
                    }
                    Some(rows)
                }
                _ => {
                    return Err(CodecError::malformed(
                        "mesh material array differs from its source header",
                    ))
                }
            };
            values.push(AttributeValue::NumericList(AttributeNumericList::$variant(
                rows,
            )));
        }};
    }
    for role in 0..4 {
        array!(role, material.colors[role], Color, source_mesh_color);
    }
    array!(4, material.shininess, Float, source_mesh_float);
    array!(5, material.transparency, Float, source_mesh_float);
    Ok(Some(values))
}

fn required(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    field: &str,
) -> Result<String, CodecError> {
    source
        .attribute(ctx, index, field)?
        .ok_or_else(|| CodecError::malformed(format_args!("attribute source has no {field}")))
}

fn required_value(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    field: &str,
) -> Result<String, CodecError> {
    crate::persistence::source::value_attribute(ctx, source, index, field)?
        .ok_or_else(|| CodecError::malformed(format_args!("attribute source has no {field}")))
}

fn read_property(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    property_index: usize,
    property: &PropertyRecord,
    ranges: &ValueSourceRanges,
    bindings: &ExpressionBindings,
    ir: &CadIr,
) -> Result<PropertyRecord, CodecError> {
    let held = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect::<BTreeSet<_>>();
    let mut values = Vec::new();
    let mut extents = Vec::new();
    for (order, range) in ranges.value_ranges() {
        let Some(admitted) = property.values().iter().find(|value| value.order == order) else {
            continue;
        };
        if matches!(admitted.tag.as_str(), "Enum" | "CustomEnumList")
            || (admitted.tag == "Expression" && bindings.consumes(&property.id, order, &held))
        {
            continue;
        }
        let start = source.view().start() + range.start;
        let end = source.view().start() + range.end;
        let index = source
            .elements()
            .iter()
            .position(|element| element.extent().start() == start && element.extent().end() == end)
            .ok_or_else(|| {
                CodecError::malformed("admitted attribute value has no original source element")
            })?;
        let tag = source.read_name(index)?.to_owned();
        let mut attributes = BTreeMap::new();
        for field in fields(&tag) {
            if tag == "PropertyPlacement" {
                let selected_axis = admitted.attributes.contains_key("A");
                if (selected_axis && field.starts_with('Q'))
                    || (!selected_axis && matches!(*field, "Ox" | "Oy" | "Oz" | "A"))
                {
                    continue;
                }
            }
            if let Some(value) =
                crate::persistence::source::value_attribute(ctx, source, index, field)?
            {
                attributes.insert((*field).to_owned(), value);
            }
        }
        let decoded_json =
            crate::persistence::document_properties::value_parts::python_json(&tag, &attributes)
                .map(Into::into);
        let text = if matches!(tag.as_str(), "String" | "Unicode" | "Path" | "Uuid")
            && !attributes.contains_key("value")
            && !attributes.contains_key("Value")
        {
            Some(source.text(ctx, index)?)
        } else {
            None
        };
        values.push(ValueRecord {
            tag,
            order,
            parent: admitted.parent,
            link_index: admitted.link_index,
            item_index: admitted.item_index,
            attributes,
            decoded_json,
            text,
        });
        extents.push(start..end);
    }
    let root_index = source
        .children(property_index)
        .next()
        .ok_or_else(|| CodecError::malformed("attribute property has no source value root"))?;
    let links = crate::persistence::document_properties::value_parts::links::from_values(
        &property.type_name,
        &values,
        source,
        root_index,
        &extents,
    )?;
    let mut current = property.clone();
    current.body = Some(PropertyBody::Persisted {
        values: ValueSlots::new(
            values,
            match &property.body {
                Some(PropertyBody::Persisted { values, .. }) => values.slot_count(),
                _ => 0,
            },
        )
        .map_err(CodecError::malformed)?,
        links,
        side_entries: property.side_entries().to_vec(),
    });
    Ok(current)
}

fn fields(tag: &str) -> &[&str] {
    match tag {
        "Map" | "ExpressionEngine" => &[],
        "Item" => &["key", "value"],
        "Expression" => &["path", "expression", "comment"],
        "PropertyVector" | "Vector" => &["valueX", "valueY", "valueZ"],
        "PropertyRotation" => &["Ox", "Oy", "Oz", "A"],
        "PropertyPlacement" => &[
            "Px", "Py", "Pz", "Q0", "Q1", "Q2", "Q3", "Ox", "Oy", "Oz", "A",
        ],
        "PropertyMatrix" => &[
            "a11", "a12", "a13", "a14", "a21", "a22", "a23", "a24", "a31", "a32", "a33", "a34",
            "a41", "a42", "a43", "a44",
        ],
        "Python" => &["json", "encoded", "value"],
        "I" => &["v"],
        "Link" | "LinkSub" | "XLink" | "Sub" => {
            &["value", "obj", "name", "file", "sub", "shadowed", "count"]
        }
        "StringList" | "IntegerList" | "IntegerSet" | "LinkList" | "LinkSubList"
        | "XLinkSubList" | "XLinks" => &["count"],
        _ => &["value", "Value"],
    }
}

pub(crate) fn numeric_kind(kind: &str) -> bool {
    matches!(
        kind,
        "App::PropertyFloatList"
            | "App::PropertyVectorList"
            | "App::PropertyPlacementList"
            | "App::PropertyColorList"
            | "Points::PropertyGreyValueList"
            | "Inspection::PropertyDistanceList"
            | "Points::PropertyNormalList"
            | "Mesh::PropertyNormalList"
            | "Points::PropertyCurvatureList"
            | "Mesh::PropertyCurvatureList"
            | "Mesh::PropertyMaterial"
    )
}

fn numeric_values(
    ctx: &DecodeContext<'_>,
    reader: &PropertyReader<'_, '_>,
    owner: &RecordOwner<'_>,
    property: &PropertyRecord,
    kind: &str,
    empty: bool,
) -> Result<AttributeNumericList, CodecError> {
    let header = reader.source_payload_header(owner, property)?;
    macro_rules! list {
        ($variant:ident, $getter:ident, $convert:expr) => {{
            let values = if empty {
                Some(Vec::new())
            } else if let Some(header) = header {
                let mut values =
                    ctx.alloc_filled(header.count(), None, "FreeCAD numeric attribute slots")?;
                for ordinal in reader.payload_ordinals(property) {
                    let slot = values.get_mut(ordinal).ok_or_else(|| {
                        CodecError::malformed("numeric attribute row exceeds original count")
                    })?;
                    *slot = reader
                        .$getter(owner, property, ordinal, &header)?
                        .map($convert);
                }
                Some(values)
            } else {
                None
            };
            AttributeNumericList::$variant(values)
        }};
    }
    Ok(match kind {
        "App::PropertyFloatList"
        | "Points::PropertyGreyValueList"
        | "Inspection::PropertyDistanceList" => list!(Float, source_float, |value| value),
        "App::PropertyVectorList" | "Points::PropertyNormalList" | "Mesh::PropertyNormalList" => {
            list!(Vector, source_vector, |value| value)
        }
        "Points::PropertyCurvatureList" | "Mesh::PropertyCurvatureList" => {
            list!(Curvature, source_curvature, |value| value)
        }
        "App::PropertyColorList" => list!(Color, source_color, |value| value),
        "App::PropertyPlacementList" => list!(
            Placement,
            source_placement_values,
            |value: [cadmpeg_ir::scalar::FiniteReal; 7]| AttributePlacement {
                position: [value[0], value[1], value[2]].into(),
                quaternion: [value[3], value[4], value[5], value[6]].into(),
            }
        ),
        _ => return Err(CodecError::malformed("unknown numeric attribute grammar")),
    })
}

fn inline_values(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    root: usize,
    property: &PropertyRecord,
    ranges: &ValueSourceRanges,
) -> Result<AttributeNumericList, CodecError> {
    let count = required_value(ctx, source, root, "count")?
        .parse::<usize>()
        .map_err(CodecError::malformed)?;
    let mut slots = ctx.alloc_filled(count, None, "inline numeric attribute slots")?;
    let root_order = property
        .values()
        .iter()
        .find(|value| value.parent.is_none())
        .map(|value| value.order);
    for (ordinal, child) in source.children(root).enumerate() {
        let extent = source.extent(child)?;
        let admitted = ranges.value_ranges().any(|(order, range)| {
            property
                .values()
                .iter()
                .any(|value| value.order == order && value.parent == root_order)
                && source.view().start() + range.start == extent.start()
        });
        if !admitted {
            continue;
        }
        let slot = slots
            .get_mut(ordinal)
            .ok_or_else(|| CodecError::malformed("inline numeric row exceeds count"))?;
        let value = required_value(ctx, source, child, "v")?
            .parse::<f32>()
            .map_err(CodecError::malformed)?;
        *slot = Some(
            cadmpeg_ir::scalar::FiniteReal::new(f64::from(value))
                .ok_or_else(|| CodecError::malformed("non-finite inline numeric value"))?,
        );
    }
    Ok(AttributeNumericList::Float(Some(slots)))
}
