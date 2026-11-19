// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph recovery.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::scalar::{Angle, FiniteReal};
use cadmpeg_ir::semantic_annotations::{
    AnnotationDimensionKind, LineDirections, MeasuredOn, NumberFormat, SemanticAnnotation,
    SemanticAnnotationId, SemanticAnnotationKind, ViewAxis,
};

use crate::native::{
    sole_named_property, AnnotationRuntimeType, ObjectRecord, PropertyRecord,
    SemanticAnnotationRecord,
};

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<SemanticAnnotationRecord>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            owned.push(property);
        }
    }
    let mut records = Vec::new();
    for object in objects {
        if let Some(kind) = AnnotationRuntimeType::from_label(&object.type_name) {
            let schema = annotation_schema(kind);
            let source = by_owner
                .get(object.id().as_str())
                .map_or(&[][..], Vec::as_slice);
            let mut owned = Vec::new();
            owned.extend_from_slice(source);
            owned.sort_by(|left, right| {
                (left.xml.start(), left.xml.end()).cmp(&(right.xml.start(), right.xml.end()))
            });
            let mut references = BTreeMap::new();
            let mut parameters = BTreeMap::new();
            for property in &owned {
                let name = property.name.clone();
                if property.links().is_empty() {
                    parameters.insert(name, property.xml.text().to_owned());
                } else {
                    let mut links = Vec::new();
                    for link in property.links() {
                        links.push(link.clone());
                    }
                    references.insert(name, links);
                }
            }
            let mut text = Vec::new();
            if let Some(carrier) = schema.text {
                for property in owned
                    .iter()
                    .filter(|property| property.name == carrier.property)
                {
                    match strict_text_values(ctx, property, carrier.type_name) {
                        Ok(values) => {
                            text.extend(values);
                        }
                        Err(CodecError::ResourceLimit(limit)) => {
                            return Err(CodecError::ResourceLimit(limit))
                        }
                        Err(_) => {}
                    }
                }
            }
            let mut side_entries = Vec::new();
            for property in &owned {
                for name in property.side_entries() {
                    side_entries.push(name.to_owned());
                }
            }
            records.push(SemanticAnnotationRecord {
                id: crate::native::native_id("annotation", object.name()),
                object: object.id().to_owned(),
                kind,
                text,
                references,
                parameters,
                side_entries,
            });
        }
    }
    Ok(records)
}

pub(crate) fn transfer_neutral(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    records: &[SemanticAnnotationRecord],
    properties: &[PropertyRecord],
) -> Result<(), CodecError> {
    for (order, record) in records.iter().enumerate() {
        let schema = annotation_schema(record.kind);
        let mut owned = Vec::new();
        owned.extend(
            properties
                .iter()
                .filter(|property| property.owner == record.object),
        );
        validate_text_carriers(ctx, &owned, &schema)?;
        let mut references = BTreeMap::new();
        for (role, targets) in &record.references {
            let mut selections = Vec::new();
            for link in targets {
                selections.extend(crate::drawing::link_references(link.as_ref())?);
            }
            references.insert(role.to_owned(), selections);
        }
        let mut parameters = BTreeMap::new();
        for (name, value) in &record.parameters {
            parameters.insert(name.to_owned(), value.to_owned());
        }
        let mut assets = Vec::new();
        for name in &record.side_entries {
            assets.push(crate::native::native_id("entry", name));
        }
        model.semantic_annotations.push(SemanticAnnotation {
            text: record.text.clone(),
            references: cadmpeg_core::text::named_entries(&record.object, references)?,
            value: None,
            format: match schema.text {
                Some(carrier) if carrier.has_format_spec => {
                    string_property(ctx, &owned, carrier.property, carrier.type_name)?
                }
                _ => None,
            },
            position: annotation_position(ctx, &owned, schema.position)?,
            parameters: cadmpeg_core::text::named_entries(&record.object, parameters)?,
            assets,
            ..SemanticAnnotation::new(
                SemanticAnnotationId::mint(crate::native::model_id(
                    "semantic-annotation",
                    &record.object,
                    "content",
                ))
                .map_err(CodecError::malformed)?,
                record.object.clone(),
                schema.kind.clone(),
                record.kind.as_str().to_owned(),
                u32::try_from(order).map_err(|_| {
                    CodecError::malformed(format_args!("FreeCAD ordinal {order} exceeds u32"))
                })?,
                record.id.clone(),
            )
        });
    }
    Ok(())
}

pub(crate) fn is_annotation_type(type_name: &str) -> bool {
    AnnotationRuntimeType::from_label(type_name).is_some()
}

#[derive(Clone)]
struct AnnotationSchema {
    kind: SemanticAnnotationKind,
    text: Option<TextCarrier>,
    position: PositionCarrier,
}

#[derive(Clone, Copy)]
struct TextCarrier {
    property: &'static str,
    type_name: &'static str,
    has_format_spec: bool,
}

#[derive(Clone, Copy)]
enum PositionCarrier {
    Vector {
        name: &'static str,
        type_name: &'static str,
    },
    Coordinates {
        x_name: &'static str,
        y_name: &'static str,
        type_names: &'static [&'static str],
    },
}

const TECHDRAW_POSITION_TYPES: &[&str] = &[
    "App::PropertyDistance",
    "App::PropertyLength",
    "App::PropertyFloat",
];

fn annotation_schema(runtime_type: AnnotationRuntimeType) -> AnnotationSchema {
    use SemanticAnnotationKind as Kind;
    match runtime_type {
        AnnotationRuntimeType::Annotation => AnnotationSchema {
            kind: Kind::Text,
            text: Some(TextCarrier {
                property: "LabelText",
                type_name: "App::PropertyStringList",
                has_format_spec: false,
            }),
            position: PositionCarrier::Vector {
                name: "Position",
                type_name: "App::PropertyVector",
            },
        },
        AnnotationRuntimeType::AnnotationLabel => AnnotationSchema {
            kind: Kind::Text,
            text: Some(TextCarrier {
                property: "LabelText",
                type_name: "App::PropertyStringList",
                has_format_spec: false,
            }),
            position: PositionCarrier::Vector {
                name: "TextPosition",
                type_name: "App::PropertyVector",
            },
        },
        AnnotationRuntimeType::DrawViewAnnotation
        | AnnotationRuntimeType::DrawViewAnnotationPython => AnnotationSchema {
            kind: Kind::Text,
            text: Some(TextCarrier {
                property: "Text",
                type_name: "App::PropertyStringList",
                has_format_spec: false,
            }),
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        AnnotationRuntimeType::DrawRichAnno | AnnotationRuntimeType::DrawRichAnnoPython => {
            AnnotationSchema {
                kind: Kind::Text,
                text: Some(TextCarrier {
                    property: "AnnoText",
                    type_name: "App::PropertyString",
                    has_format_spec: false,
                }),
                position: PositionCarrier::Coordinates {
                    x_name: "X",
                    y_name: "Y",
                    type_names: TECHDRAW_POSITION_TYPES,
                },
            }
        }
        AnnotationRuntimeType::DrawViewDimension
        | AnnotationRuntimeType::DrawViewDimExtent
        | AnnotationRuntimeType::LandmarkDimension => AnnotationSchema {
            kind: Kind::Dimension,
            text: Some(TextCarrier {
                property: "FormatSpec",
                type_name: "App::PropertyString",
                has_format_spec: true,
            }),
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        AnnotationRuntimeType::DrawViewBalloon => AnnotationSchema {
            kind: Kind::Balloon,
            text: Some(TextCarrier {
                property: "Text",
                type_name: "App::PropertyString",
                has_format_spec: false,
            }),
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        AnnotationRuntimeType::DrawLeaderLine | AnnotationRuntimeType::DrawLeaderLinePython => {
            AnnotationSchema {
                kind: Kind::Leader,
                text: None,
                position: PositionCarrier::Coordinates {
                    x_name: "X",
                    y_name: "Y",
                    type_names: TECHDRAW_POSITION_TYPES,
                },
            }
        }
        AnnotationRuntimeType::DrawViewSymbol
        | AnnotationRuntimeType::DrawViewSymbolPython
        | AnnotationRuntimeType::DrawWeldSymbol
        | AnnotationRuntimeType::DrawWeldSymbolPython => AnnotationSchema {
            kind: Kind::Symbol,
            text: Some(TextCarrier {
                property: "TailText",
                type_name: "App::PropertyString",
                has_format_spec: false,
            }),
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
    }
}

fn annotation_position(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    carrier: PositionCarrier,
) -> Result<Option<cadmpeg_ir::units::FiniteVector<3>>, CodecError> {
    match carrier {
        PositionCarrier::Vector { name, type_name } => {
            let position = optional_vector_property(ctx, properties, name, &[type_name])?;
            position
                .map(|position| {
                    cadmpeg_ir::units::FiniteVector::new(position).ok_or_else(|| {
                        CodecError::Malformed(
                            "annotation position contains a non-finite coordinate".into(),
                        )
                    })
                })
                .transpose()
        }
        PositionCarrier::Coordinates {
            x_name,
            y_name,
            type_names,
        } => {
            let x = optional_scalar_property(ctx, properties, x_name, type_names)?;
            let y = optional_scalar_property(ctx, properties, y_name, type_names)?;
            match (x, y) {
                (None, None) => Ok(None),
                (Some(x), Some(y)) => cadmpeg_ir::units::FiniteVector::new([x, y, 0.0])
                    .map(Some)
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "annotation position contains a non-finite coordinate".into(),
                        )
                    }),
                _ => Err(CodecError::malformed(format_args!(
                    "annotation position requires both {x_name} and {y_name}"
                ))),
            }
        }
    }
}

pub(crate) fn optional_scalar_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    type_names: &[&str],
) -> Result<Option<f64>, CodecError> {
    let Some(property) = typed_property(properties, name, type_names)? else {
        return Ok(None);
    };
    let attributes = direct_value_attributes(ctx, property, "Float", &["value"])?;
    let value = attributes
        .get("value")
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "annotation property {} is not a scalar",
                property.id
            ))
        })?;
    Ok(Some(value))
}

fn optional_vector_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    type_names: &[&str],
) -> Result<Option<[f64; 3]>, CodecError> {
    let Some(property) = typed_property(properties, name, type_names)? else {
        return Ok(None);
    };
    let attributes = direct_value_attributes(
        ctx,
        property,
        "PropertyVector",
        &["valueX", "valueY", "valueZ"],
    )?;
    let x = attributes
        .get("valueX")
        .and_then(|value| value.parse::<f64>().ok());
    let y = attributes
        .get("valueY")
        .and_then(|value| value.parse::<f64>().ok());
    let z = attributes
        .get("valueZ")
        .and_then(|value| value.parse::<f64>().ok());
    match (x, y, z) {
        (Some(x), Some(y), Some(z)) => Ok(Some([x, y, z])),
        _ => Err(CodecError::malformed(format_args!(
            "annotation property {} is not a vector",
            property.id
        ))),
    }
}

fn string_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<String>, CodecError> {
    let Some(property) = typed_property(properties, name, &[type_name])? else {
        return Ok(None);
    };
    let attributes = direct_value_attributes(ctx, property, "String", &["value"])?;
    attributes
        .into_iter()
        .find_map(|(name, value)| (name == "value").then_some(value))
        .map(Some)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "annotation property {} string value is missing value",
                property.id
            ))
        })
}

/// The value of a Boolean property.
pub(crate) fn bool_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<bool>, CodecError> {
    let Some(property) = typed_property(properties, name, &["App::PropertyBool"])? else {
        return Ok(None);
    };
    let attributes = direct_value_attributes(ctx, property, "Bool", &["value"])?;
    match attributes.get("value").map(String::as_str) {
        Some("true" | "1") => Ok(Some(true)),
        Some("false" | "0") => Ok(Some(false)),
        _ => Err(CodecError::malformed(format_args!(
            "property {} is not a Boolean",
            property.id
        ))),
    }
}

/// The selected index of an enumeration property, with its label where the
/// file stores the enumeration's list.
pub(crate) fn enumeration_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<(usize, Option<String>)>, CodecError> {
    let Some(property) = typed_property(properties, name, &["App::PropertyEnumeration"])? else {
        return Ok(None);
    };
    let xml = ctx.parse_xml(property.xml.text(), "enumeration property")?;
    let root = xml.root_element();
    let index = root
        .children()
        .find(|node| node.has_tag_name("Integer"))
        .and_then(|node| node.attribute("value"))
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "property {} has no enumeration index",
                property.id
            ))
        })?;
    let label = root
        .children()
        .find(|node| node.has_tag_name("CustomEnumList"))
        .and_then(|list| {
            list.children()
                .filter(|node| node.has_tag_name("Enum"))
                .nth(index)?
                .attribute("value")
                .map(str::to_owned)
        });
    Ok(Some((index, label)))
}

/// The selected entry of an enumeration property: its stored label, or the
/// built-in list's entry at its index.
pub(crate) fn enumeration_entry<'a>(
    selected: Option<&'a (usize, Option<String>)>,
    built_in: &[&'a str],
) -> Option<&'a str> {
    let (index, label) = selected?;
    label.as_deref().or_else(|| built_in.get(*index).copied())
}

fn typed_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
    type_names: &[&str],
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let Some(property) = sole_named_property("annotation", properties, name)? else {
        return Ok(None);
    };
    if !type_names.contains(&property.type_name.as_str()) {
        let expected = type_names.join(" or ");
        return Err(CodecError::malformed(format_args!(
            "annotation property {name} has runtime type {}, expected {expected}",
            property.type_name
        )));
    }
    Ok(Some(property))
}

fn validate_text_carriers(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    schema: &AnnotationSchema,
) -> Result<(), CodecError> {
    let Some(carrier) = &schema.text else {
        return Ok(());
    };
    if let Some(property) = typed_property(properties, carrier.property, &[carrier.type_name])? {
        strict_text_values(ctx, property, carrier.type_name)?;
    }
    Ok(())
}

fn direct_value_attributes(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_tag: &str,
    allowed_attributes: &[&str],
) -> Result<BTreeMap<String, String>, CodecError> {
    let document = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "annotation property {} has invalid XML: {error}",
                property.id
            ))
        })?;
    let root = document.root_element();
    if has_non_whitespace_text(root) {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} has unexpected text",
            property.id
        )));
    }
    let mut values = root.children().filter(roxmltree::Node::is_element);
    let Some(value) = values.next() else {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} requires one direct {expected_tag} value",
            property.id
        )));
    };
    if values.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} requires one direct {expected_tag} value",
            property.id
        )));
    }
    if !value.has_tag_name(expected_tag) {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} has root {}, expected {expected_tag}",
            property.id,
            value.tag_name().name()
        )));
    }
    validate_leaf_value(value, property, allowed_attributes)?;
    let mut attributes = BTreeMap::new();
    for attribute in value.attributes() {
        attributes.insert(attribute.name().to_owned(), attribute.value().to_owned());
    }
    Ok(attributes)
}

fn strict_text_values(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_type: &str,
) -> Result<Vec<String>, CodecError> {
    if property.type_name != expected_type {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} has runtime type {}, expected {expected_type}",
            property.id, property.type_name
        )));
    }
    if expected_type == "App::PropertyString" {
        let attributes = direct_value_attributes(ctx, property, "String", &["value"])?;
        let value = attributes
            .into_iter()
            .find_map(|(name, value)| (name == "value").then_some(value))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "annotation property {} string value is missing value",
                    property.id
                ))
            })?;
        let mut values = Vec::new();
        if !value.trim().is_empty() {
            values.push(value);
        }
        return Ok(values);
    }
    let document = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "annotation property {} has invalid XML: {error}",
                property.id
            ))
        })?;
    let root = document.root_element();
    if has_non_whitespace_text(root) {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} has unexpected text",
            property.id
        )));
    }
    let mut values = root.children().filter(roxmltree::Node::is_element);
    let Some(string_list) = values.next() else {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} requires one direct StringList value",
            property.id
        )));
    };
    if values.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} requires one direct StringList value",
            property.id
        )));
    }
    if !string_list.has_tag_name("StringList") {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} has root {}, expected StringList",
            property.id,
            string_list.tag_name().name()
        )));
    }
    validate_attributes(string_list, property, &["count"])?;
    let count = string_list
        .attribute("count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "annotation property {} StringList has an invalid count",
                property.id
            ))
        })?;
    if has_non_whitespace_text(string_list) {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} StringList has unexpected text",
            property.id
        )));
    }
    let found = string_list
        .children()
        .filter(roxmltree::Node::is_element)
        .count();
    if found != count {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} StringList count={count} but {} direct String values were found",
            property.id,
            found
        )));
    }
    let mut texts = Vec::new();
    for string in string_list.children().filter(roxmltree::Node::is_element) {
        if !string.has_tag_name("String") {
            return Err(CodecError::malformed(format_args!(
                "annotation property {} StringList has an unexpected child {}",
                property.id,
                string.tag_name().name()
            )));
        }
        validate_leaf_value(string, property, &["value"])?;
        let value = string.attribute("value").ok_or_else(|| {
            CodecError::malformed(format_args!(
                "annotation property {} String value is missing value",
                property.id
            ))
        })?;
        if !value.trim().is_empty() {
            texts.push(value.to_owned());
        }
    }
    Ok(texts)
}

fn validate_leaf_value(
    value: roxmltree::Node<'_, '_>,
    property: &PropertyRecord,
    allowed_attributes: &[&str],
) -> Result<(), CodecError> {
    validate_attributes(value, property, allowed_attributes)?;
    if value.children().any(|child| child.is_element()) || has_non_whitespace_text(value) {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} value {} has nested content",
            property.id,
            value.tag_name().name()
        )));
    }
    Ok(())
}

fn validate_attributes(
    value: roxmltree::Node<'_, '_>,
    property: &PropertyRecord,
    allowed_attributes: &[&str],
) -> Result<(), CodecError> {
    if value
        .attributes()
        .any(|attribute| !allowed_attributes.contains(&attribute.name()))
    {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} value {} has unsupported attributes",
            property.id,
            value.tag_name().name()
        )));
    }
    Ok(())
}

fn has_non_whitespace_text(value: roxmltree::Node<'_, '_>) -> bool {
    value
        .children()
        .any(|child| child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty()))
}

pub(crate) fn enrich_dimensions(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    properties: &[PropertyRecord],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    use crate::loss::FreecadLossCode;
    use cadmpeg_ir::semantic_annotations::AnnotationDimensionKind as DimensionKind;

    for annotation in &mut model.semantic_annotations {
        if annotation.kind != SemanticAnnotationKind::Dimension {
            continue;
        }
        let owned: Vec<_> = properties
            .iter()
            .filter(|p| p.owner == annotation.object)
            .collect();
        if let Some(property) = typed_property(&owned, "Type", &["App::PropertyEnumeration"])? {
            let xml = ctx.parse_xml(property.xml.text(), "dimension type")?;
            let selected = xml
                .root_element()
                .children()
                .find(|n| n.has_tag_name("Integer"))
                .and_then(|n| n.attribute("value"))
                .unwrap_or("");
            let custom = xml
                .root_element()
                .children()
                .find(|n| n.has_tag_name("CustomEnumList"));
            let label = custom.and_then(|list| {
                let index = selected.parse::<usize>().ok()?;
                list.children()
                    .filter(|n| n.has_tag_name("Enum"))
                    .nth(index)?
                    .attribute("value")
            });
            let kind = match label {
                Some("Distance") => Some(DimensionKind::Distance),
                Some("DistanceX") => Some(DimensionKind::HorizontalDistance),
                Some("DistanceY") => Some(DimensionKind::VerticalDistance),
                Some("Radius") => Some(DimensionKind::Radius),
                Some("Diameter") => Some(DimensionKind::Diameter),
                Some("Angle") => Some(DimensionKind::Angle),
                Some("Angle3Pt") => Some(DimensionKind::ThreePointAngle),
                Some("Area") => Some(DimensionKind::Area {
                    holes_excluded: None,
                }),
                None if custom.is_none() && selected == "0" => Some(DimensionKind::Distance),
                None if custom.is_none() => match selected {
                    "1" => Some(DimensionKind::HorizontalDistance),
                    "2" => Some(DimensionKind::VerticalDistance),
                    "4" => Some(DimensionKind::Radius),
                    "5" => Some(DimensionKind::Diameter),
                    "6" => Some(DimensionKind::Angle),
                    "7" => Some(DimensionKind::ThreePointAngle),
                    "8" => Some(DimensionKind::Area {
                        holes_excluded: None,
                    }),
                    _ => None,
                },
                _ => None,
            };
            annotation.dimension_kind = Some(kind.unwrap_or_else(|| {
                losses.push(
                    FreecadLossCode::DrawingDimensionKindNotTransferred.note(property.id.clone()),
                );
                DimensionKind::Other {
                    native_type: label.unwrap_or(selected).to_owned(),
                }
            }));
        }
        let mut arbitrary_tolerances = false;
        for name in ["Arbitrary", "ArbitraryTolerances"] {
            if let Some(property) = typed_property(&owned, name, &["App::PropertyBool"])? {
                let attributes = direct_value_attributes(ctx, property, "Bool", &["value"])?;
                let enabled = match attributes.get("value").map(String::as_str) {
                    Some("true" | "1") => true,
                    Some("false" | "0") => false,
                    _ => return Err(CodecError::malformed("invalid dimension arbitrary flag")),
                };
                if name == "Arbitrary" && enabled {
                    annotation.text_override.clone_from(&annotation.format);
                }
                if name == "ArbitraryTolerances" {
                    arbitrary_tolerances = enabled;
                }
            }
        }
        let angular = matches!(
            annotation.dimension_kind,
            Some(DimensionKind::Angle | DimensionKind::ThreePointAngle)
        );
        for (name, slot) in [
            ("OverTolerance", &mut annotation.upper_tolerance),
            ("UnderTolerance", &mut annotation.lower_tolerance),
        ] {
            let value =
                optional_scalar_property(ctx, &owned, name, &["App::PropertyQuantityConstraint"])?;
            if let Some(value) = value {
                let value = FiniteReal::new(if angular { value.to_radians() } else { value })
                    .ok_or_else(|| CodecError::malformed("dimension tolerance must be finite"))?;
                if value.get() != 0.0 {
                    if arbitrary_tolerances {
                        losses.push(
                            FreecadLossCode::DrawingDimensionToleranceNotTransferred
                                .note(format!("{}: {name}", annotation.id)),
                        );
                    } else {
                        *slot = Some(value);
                    }
                }
            }
        }
        if arbitrary_tolerances {
            for name in ["FormatSpecOverTolerance", "FormatSpecUnderTolerance"] {
                if string_property(ctx, &owned, name, "App::PropertyString")?.is_some() {
                    losses.push(
                        FreecadLossCode::DrawingDimensionToleranceNotTransferred
                            .note(format!("{}: {name}", annotation.id)),
                    );
                }
            }
        }
        dimension_semantics(ctx, annotation, &owned)?;
    }
    Ok(())
}

/// The `TechDraw` 1.1.4 dimension properties that state what a dimension
/// measures and how it shows its value (`DrawViewDimension.cpp`,
/// `DimensionFormatter.cpp`).
fn dimension_semantics(
    ctx: &DecodeContext<'_>,
    annotation: &mut SemanticAnnotation,
    owned: &[&PropertyRecord],
) -> Result<(), CodecError> {
    if annotation.runtime_type == "TechDraw::DrawViewDimExtent" {
        let direction = enumeration_property(ctx, owned, "DirExtent")?;
        let axis = match enumeration_entry(direction.as_ref(), &["Horizontal", "Vertical"]) {
            Some("Horizontal") => Some(ViewAxis::X),
            Some("Vertical") => Some(ViewAxis::Y),
            _ => None,
        };
        if let Some(axis) = axis {
            annotation.dimension_kind = Some(AnnotationDimensionKind::Extent { axis });
        }
    }
    if let Some(AnnotationDimensionKind::Area { holes_excluded }) = &mut annotation.dimension_kind {
        *holes_excluded = bool_property(ctx, owned, "UseActualArea")?;
    }
    annotation.measured_on = if annotation.runtime_type == "TechDraw::LandmarkDimension" {
        Some(MeasuredOn::Model)
    } else {
        let measure = enumeration_property(ctx, owned, "MeasureType")?;
        match enumeration_entry(measure.as_ref(), &["True", "Projected"]) {
            Some("True") => Some(MeasuredOn::Model),
            Some("Projected") => Some(MeasuredOn::View),
            _ => None,
        }
    };
    annotation.basic = bool_property(ctx, owned, "TheoreticalExact")?;
    if let Some(inverted) = bool_property(ctx, owned, "Inverted")? {
        if matches!(
            annotation.dimension_kind,
            Some(AnnotationDimensionKind::Angle | AnnotationDimensionKind::ThreePointAngle)
        ) {
            annotation.reflex = Some(inverted);
        } else {
            annotation.negated = Some(inverted);
        }
    }
    if bool_property(ctx, owned, "AngleOverride")? == Some(true) {
        let angle = |name| -> Result<Option<Angle>, CodecError> {
            Ok(
                optional_scalar_property(ctx, owned, name, &["App::PropertyAngle"])?
                    .and_then(|degrees| Angle::new(degrees.to_radians())),
            )
        };
        if let (Some(dimension_line), Some(extension_lines)) =
            (angle("LineAngle")?, angle("ExtensionAngle")?)
        {
            annotation.line_directions = Some(LineDirections {
                dimension_line,
                extension_lines,
            });
        }
    }
    if bool_property(ctx, owned, "EqualTolerance")? == Some(true) {
        if let Some(upper) = annotation.upper_tolerance {
            annotation.lower_tolerance = FiniteReal::new(-upper.get());
        }
    }
    annotation.number_format = annotation.format.as_deref().and_then(number_format);
    if let Some(format) = &mut annotation.number_format {
        format.unit_shown = bool_property(ctx, owned, "ShowUnits")?;
    }
    Ok(())
}

/// The meaning of a `FormatSpec`: `%.Nf` is `decimals: N`, `%.Nw` the same
/// with trailing zeros removed, and `%Xr` a rounding to `X`; the text around
/// the specifier is the prefix and suffix. Another conversion has none.
fn number_format(spec: &str) -> Option<NumberFormat> {
    let start = spec.find('%')?;
    let rest = &spec[start + 1..];
    let (decimals, trim_zeros, round_to, length) = if let Some(places) = rest.strip_prefix('.') {
        let digits = places.find(|character: char| !character.is_ascii_digit())?;
        let decimals = places[..digits].parse::<u8>().ok()?;
        let trim_zeros = match places[digits..].chars().next()? {
            'f' => false,
            'w' => true,
            _ => return None,
        };
        (Some(decimals), trim_zeros, None, digits + 2)
    } else {
        let digits =
            rest.find(|character: char| !(character.is_ascii_digit() || character == '.'))?;
        if digits == 0 || !rest[digits..].starts_with('r') {
            return None;
        }
        let increment = FiniteReal::new(rest[..digits].parse().ok()?)?;
        (None, false, Some(increment), digits + 1)
    };
    Some(NumberFormat {
        prefix: spec[..start].to_owned(),
        suffix: rest[length..].to_owned(),
        decimals,
        trim_zeros,
        round_to,
        unit_shown: None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::is_annotation_type;
    use crate::test_support::test_archive::{archive, archive_entries, assert_valid_document};
    use crate::FcstdCodec;
    use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind as Kind;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    #[test]
    fn annotation_registry_uses_exact_runtime_types() {
        for runtime_type in [
            "App::Annotation",
            "App::AnnotationLabel",
            "TechDraw::DrawViewAnnotation",
            "TechDraw::DrawViewAnnotationPython",
            "TechDraw::DrawRichAnno",
            "TechDraw::DrawRichAnnoPython",
            "TechDraw::DrawViewDimension",
            "TechDraw::DrawViewDimExtent",
            "TechDraw::LandmarkDimension",
            "TechDraw::DrawViewBalloon",
            "TechDraw::DrawLeaderLine",
            "TechDraw::DrawLeaderLinePython",
            "TechDraw::DrawViewSymbol",
            "TechDraw::DrawViewSymbolPython",
            "TechDraw::DrawWeldSymbol",
            "TechDraw::DrawWeldSymbolPython",
        ] {
            assert!(is_annotation_type(runtime_type), "{runtime_type}");
        }
        for runtime_type in [
            "Custom::AnnotationCache",
            "TechDraw::DrawViewDatum",
            "TechDraw::DrawViewTolerance",
            "TechDraw::DrawViewDraft",
            "PartDesign::FeatureAddSub",
        ] {
            assert!(!is_annotation_type(runtime_type), "{runtime_type}");
        }
    }

    #[test]
    fn transfers_app_annotation_text_and_position_carriers() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="App::Annotation" name="Note"/>
 <Object type="App::AnnotationLabel" name="Label"/>
</Objects>
<ObjectData Count="2">
 <Object name="Note"><Properties Count="2">
  <Property name="LabelText" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property>
  <Property name="Position" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
 </Properties></Object>
 <Object name="Label"><Properties Count="2">
  <Property name="LabelText" type="App::PropertyStringList"><StringList count="1"><String value="LABEL"/></StringList></Property>
  <Property name="TextPosition" type="App::PropertyVector"><PropertyVector valueX="4" valueY="5" valueZ="6"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("App annotations");

        let annotations = &result.ir().model.semantic_annotations;
        assert_eq!(annotations.len(), 2);
        let note = annotations
            .iter()
            .find(|annotation| annotation.runtime_type == "App::Annotation")
            .expect("note annotation");
        assert_eq!(note.text, ["NOTE"]);
        assert_eq!(
            note.position.map(cadmpeg_ir::units::FiniteVector::get),
            Some([1.0, 2.0, 3.0])
        );
        let label = annotations
            .iter()
            .find(|annotation| annotation.runtime_type == "App::AnnotationLabel")
            .expect("label annotation");
        assert_eq!(label.text, ["LABEL"]);
        assert_eq!(
            label.position.map(cadmpeg_ir::units::FiniteVector::get),
            Some([4.0, 5.0, 6.0])
        );
    }

    #[test]
    fn rejects_noncanonical_annotation_value_roots_and_attributes() {
        fn app_annotation_document(label_text: &str, position: &str) -> String {
            format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Annotation" name="Note"/></Objects>
<ObjectData Count="1"><Object name="Note"><Properties Count="2"><Property name="LabelText" type="App::PropertyStringList">{label_text}</Property><Property name="Position" type="App::PropertyVector">{position}</Property></Properties></Object></ObjectData></Document>"#
            )
        }

        fn techdraw_annotation_document(x: &str) -> String {
            format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewAnnotation" name="Note"/></Objects>
<ObjectData Count="1"><Object name="Note"><Properties Count="3"><Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property><Property name="X" type="App::PropertyDistance">{x}</Property><Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property></Properties></Object></ObjectData></Document>"#
            )
        }

        fn rich_annotation_document(text: &str) -> String {
            format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawRichAnno" name="Rich"/></Objects>
<ObjectData Count="1"><Object name="Rich"><Properties Count="3"><Property name="AnnoText" type="App::PropertyString">{text}</Property><Property name="X" type="App::PropertyDistance"><Float value="10"/></Property><Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property></Properties></ObjectData></Document>"#
            )
        }

        fn dimension_document(format_spec: &str) -> String {
            format!(
                r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects>
<ObjectData Count="1"><Object name="Dimension"><Properties Count="3"><Property name="FormatSpec" type="App::PropertyString">{format_spec}</Property><Property name="X" type="App::PropertyDistance"><Float value="10"/></Property><Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property></Properties></Object></ObjectData></Document>"#
            )
        }

        let documents = [
            (
                "nested string list child",
                app_annotation_document(
                    r#"<StringList count="1"><Wrapper><String value="BAD"/></Wrapper></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2" valueZ="3"/>"#,
                ),
            ),
            (
                "string list count mismatch",
                app_annotation_document(
                    r#"<StringList count="2"><String value="ONE"/></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2" valueZ="3"/>"#,
                ),
            ),
            (
                "string list uppercase attribute",
                app_annotation_document(
                    r#"<StringList count="1"><String Value="BAD"/></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2" valueZ="3"/>"#,
                ),
            ),
            (
                "string list wrong direct child",
                app_annotation_document(
                    r#"<StringList count="1"><Float value="1"/></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2" valueZ="3"/>"#,
                ),
            ),
            (
                "property significant text",
                app_annotation_document(
                    r#"prefix<StringList count="1"><String value="NOTE"/></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2" valueZ="3"/>"#,
                ),
            ),
            (
                "duplicate string list roots",
                app_annotation_document(
                    r#"<StringList count="1"><String value="ONE"/></StringList><StringList count="1"><String value="TWO"/></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2" valueZ="3"/>"#,
                ),
            ),
            (
                "vector wrapper",
                app_annotation_document(
                    r#"<StringList count="1"><String value="NOTE"/></StringList>"#,
                    r#"<Wrapper><PropertyVector valueX="1" valueY="2" valueZ="3"/></Wrapper>"#,
                ),
            ),
            (
                "vector unsupported attribute",
                app_annotation_document(
                    r#"<StringList count="1"><String value="NOTE"/></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2" valueZ="3" Value="4"/>"#,
                ),
            ),
            (
                "vector missing attribute",
                app_annotation_document(
                    r#"<StringList count="1"><String value="NOTE"/></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2"/>"#,
                ),
            ),
            (
                "vector duplicate roots",
                app_annotation_document(
                    r#"<StringList count="1"><String value="NOTE"/></StringList>"#,
                    r#"<PropertyVector valueX="1" valueY="2" valueZ="3"/><PropertyVector valueX="4" valueY="5" valueZ="6"/>"#,
                ),
            ),
            (
                "scalar uppercase attribute",
                techdraw_annotation_document(r#"<Float Value="10"/>"#),
            ),
            (
                "string uppercase attribute",
                rich_annotation_document(r#"<String Value="BAD"/>"#),
            ),
            (
                "format duplicate attribute spellings",
                dimension_document(r#"<String value="A" Value="B"/>"#),
            ),
        ];
        for (case, document) in documents {
            assert!(
                matches!(
                    FcstdCodec.decode(
                        &mut Cursor::new(archive(&document)),
                        &DecodeOptions::default(),
                    ),
                    Err(cadmpeg_ir::DecodeFailure::Codec(
                        cadmpeg_core::CodecError::Malformed(_)
                    ))
                ),
                "{case}"
            );
        }
    }

    #[test]
    fn rejects_incomplete_annotation_position_carriers() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewAnnotation" name="Note"/></Objects>
<ObjectData Count="1"><Object name="Note"><Properties Count="2">
<Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property>
<Property name="X" type="App::PropertyDistance"><Float value="10"/></Property>
</Properties></Object></ObjectData></Document>"#;
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect_err("incomplete annotation position");

        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
                if message.contains("position requires both X and Y")
        ));
    }

    #[test]
    fn accepts_historical_techdraw_annotation_position_types() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="TechDraw::DrawViewAnnotation" name="Note"/>
 <Object type="TechDraw::DrawRichAnno" name="Rich"/>
</Objects>
<ObjectData Count="2">
 <Object name="Note"><Properties Count="3">
  <Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property>
  <Property name="X" type="App::PropertyFloat"><Float value="10"/></Property>
  <Property name="Y" type="App::PropertyLength"><Float value="20"/></Property>
 </Properties></Object>
 <Object name="Rich"><Properties Count="3">
  <Property name="AnnoText" type="App::PropertyString"><String value="RICH"/></Property>
  <Property name="X" type="App::PropertyLength"><Float value="30"/></Property>
  <Property name="Y" type="App::PropertyFloat"><Float value="40"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("historical annotation positions");

        assert_eq!(
            result
                .ir()
                .model
                .semantic_annotations
                .iter()
                .map(|annotation| annotation
                    .position
                    .map(cadmpeg_ir::units::FiniteVector::get))
                .collect::<Vec<_>>(),
            [Some([10.0, 20.0, 0.0]), Some([30.0, 40.0, 0.0])]
        );
        assert!(crate::test_support::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
    }

    #[test]
    fn rejects_duplicate_annotation_carrier_properties_and_values() {
        let documents = [
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewAnnotation" name="Note"/></Objects>
<ObjectData Count="1"><Object name="Note"><Properties Count="3">
<Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property>
<Property name="X" type="App::PropertyDistance"><Float value="10"/><Float value="11"/></Property>
<Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property>
</Properties></Object></ObjectData></Document>"#,
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewAnnotation" name="Note"/></Objects>
<ObjectData Count="1"><Object name="Note"><Properties Count="4">
<Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property>
<Property name="X" type="App::PropertyDistance"><Float value="10"/></Property>
<Property name="X" type="App::PropertyDistance"><Float value="11"/></Property>
<Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property>
</Properties></Object></ObjectData></Document>"#,
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects>
<ObjectData Count="1"><Object name="Dimension"><Properties Count="3">
<Property name="FormatSpec" type="App::PropertyString"><String value="A"/><String value="B"/></Property>
<Property name="X" type="App::PropertyDistance"><Float value="10"/></Property>
<Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property>
</Properties></Object></ObjectData></Document>"#,
        ];
        for document in documents {
            assert!(matches!(
                FcstdCodec.decode(
                    &mut Cursor::new(archive(document)),
                    &DecodeOptions::default(),
                ),
                Err(cadmpeg_ir::DecodeFailure::Codec(
                    cadmpeg_core::CodecError::Malformed(_)
                ))
            ));
        }
    }

    #[test]
    fn rejects_wrong_annotation_carrier_types() {
        let documents = [
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Annotation" name="Note"/></Objects>
<ObjectData Count="1"><Object name="Note"><Properties Count="2">
<Property name="LabelText" type="App::PropertyString"><String value="NOTE"/></Property>
<Property name="Position" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
</Properties></Object></ObjectData></Document>"#,
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Annotation" name="Note"/></Objects>
<ObjectData Count="1"><Object name="Note"><Properties Count="2">
<Property name="LabelText" type="App::PropertyStringList"><StringList count="1"><String value="NOTE"/></StringList></Property>
<Property name="Position" type="App::PropertyString"><String value="1,2,3"/></Property>
</Properties></Object></ObjectData></Document>"#,
        ];
        for document in documents {
            assert!(matches!(
                FcstdCodec.decode(
                    &mut Cursor::new(archive(document)),
                    &DecodeOptions::default(),
                ),
                Err(cadmpeg_ir::DecodeFailure::Codec(
                    cadmpeg_core::CodecError::Malformed(_)
                ))
            ));
        }
    }

    #[test]
    fn separates_semantic_annotations_from_drawing_relationships() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawViewPart" name="View" id="2"/>
 <Object type="TechDraw::DrawViewDimension" name="Dimension" id="3"/>
 <Object type="TechDraw::DrawViewAnnotation" name="Note" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="View"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Model"/></Property></Properties></Object>
 <Object name="Dimension"><Properties Count="6">
  <Property name="BaseView" type="App::PropertyLink"><Link value="View"/></Property>
  <Property name="References2D" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Model" sub="Edge1"/></LinkSubList></Property>
  <Property name="Source3d" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Model" sub="Edge2"/></LinkSubList></Property>
  <Property name="FormatSpec" type="App::PropertyString"><String value="12.5 mm"/></Property>
  <Property name="X" type="App::PropertyDistance"><Float value="10"/></Property>
  <Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property>
 </Properties></Object>
 <Object name="Note"><Properties Count="2">
  <Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="INSPECT"/></StringList></Property>
  <Property name="View" type="App::PropertyLink"><Link value="View"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("semantic annotations");
        let namespace = result.ir().native.namespace("fcstd").expect("native");
        let annotations = namespace
            .arena_as::<crate::native::SemanticAnnotationRecord>("annotations")
            .expect("annotations");
        let drawings = namespace
            .arena_as::<crate::native::DrawingRecord>("drawings")
            .expect("drawings");
        assert_eq!(annotations.len(), 2);
        let dimension = annotations
            .iter()
            .find(|annotation| annotation.object.ends_with("#Dimension"))
            .expect("dimension");
        assert_eq!(dimension.text, ["12.5 mm"]);
        assert_eq!(
            dimension.references["References2D"][0]
                .as_ref()
                .expect("reference")
                .subelements(),
            ["Edge1"]
        );
        let note = annotations
            .iter()
            .find(|annotation| annotation.object.ends_with("#Note"))
            .expect("note");
        assert_eq!(note.text, ["INSPECT"]);
        let drawing_dimension = drawings
            .iter()
            .find(|drawing| drawing.object.ends_with("#Dimension"))
            .expect("drawing dimension");
        assert_eq!(
            drawing_dimension.relationships["BaseView"][0]
                .as_ref()
                .expect("relationship")
                .object(),
            Some("fcstd:native:object#View")
        );
        assert_eq!(drawing_dimension.sources.len(), 2);
        assert_eq!(
            drawing_dimension.sources[1]
                .as_ref()
                .expect("source")
                .subelements(),
            ["Edge2"]
        );
        let neutral_dimension = result
            .ir()
            .model
            .drawings
            .iter()
            .find(|drawing| drawing.object.ends_with("#Dimension"))
            .expect("neutral drawing dimension");
        assert_eq!(
            neutral_dimension.kind,
            cadmpeg_ir::drawings::DrawingKind::Dimension
        );
        assert!(neutral_dimension.relationships.contains_key("BaseView"));
        assert!(neutral_dimension.relationships.contains_key("References2D"));
        assert_eq!(result.ir().model.semantic_annotations.len(), 2);
        let semantic_dimension = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.object.ends_with("#Dimension"))
            .expect("semantic dimension");
        assert_eq!(
            semantic_dimension.kind,
            cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Dimension
        );
        assert_eq!(semantic_dimension.text, ["12.5 mm"]);
        assert_eq!(semantic_dimension.format.as_deref(), Some("12.5 mm"));
        assert_eq!(semantic_dimension.value, None);
        assert_eq!(
            semantic_dimension
                .position
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([10.0, 20.0, 0.0])
        );
        assert_eq!(
            semantic_dimension.references["References2D"][0].native_text(),
            Some("Model.Edge1")
        );
        let semantic_note = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.object.ends_with("#Note"))
            .expect("semantic note");
        assert_eq!(
            semantic_note.kind,
            cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text
        );
        assert_eq!(semantic_note.text, ["INSPECT"]);
        let neutral_view = result
            .ir()
            .model
            .drawings
            .iter()
            .find(|drawing| drawing.object.ends_with("#View"))
            .expect("neutral view");
        assert_eq!(
            semantic_note.references["View"][0].native_text(),
            Some(crate::native::id_key(&neutral_view.object))
        );
        assert!(crate::test_support::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
    }

    #[test]
    pub(crate) fn transfers_remaining_semantic_annotation_families_and_assets() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawViewBalloon" name="Balloon" id="2"/>
 <Object type="TechDraw::DrawLeaderLine" name="Leader" id="3"/>
 <Object type="TechDraw::DrawViewSymbol" name="Symbol" id="4"/>
 <Object type="TechDraw::DrawViewDatum" name="Datum" id="5"/>
 <Object type="TechDraw::DrawViewTolerance" name="Tolerance" id="6"/>
</Objects>
<ObjectData Count="6">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="Balloon"><Properties Count="2">
  <Property name="Text" type="App::PropertyString"><String value="7"/></Property>
  <Property name="Source" type="App::PropertyLinkSub"><LinkSub value="Model" count="1"><Sub value="Face1"/></LinkSub></Property>
 </Properties></Object>
 <Object name="Leader"><Properties Count="1"><Property name="Text" type="App::PropertyString"><String value="LEAD"/></Property></Properties></Object>
 <Object name="Symbol"><Properties Count="1"><Property name="Symbol" type="App::PropertyFileIncluded"><FileIncluded file="symbol.svg"/></Property></Properties></Object>
 <Object name="Datum"><Properties Count="1"><Property name="LabelText" type="App::PropertyString"><String value="A"/></Property></Properties></Object>
 <Object name="Tolerance"><Properties Count="1"><Property name="Text" type="App::PropertyString"><String value="0.1"/></Property></Properties></Object>
</ObjectData></Document>"#;
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("symbol.svg", b"<svg/>"),
                ])),
                &DecodeOptions::default(),
            )
            .expect("annotation families");
        let kinds = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .map(|annotation| annotation.kind.clone())
            .collect::<Vec<_>>();
        assert_eq!(kinds, [Kind::Balloon, Kind::Leader, Kind::Symbol]);
        assert!(result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .all(|annotation| !annotation.object.ends_with("#Datum")
                && !annotation.object.ends_with("#Tolerance")));
        let balloon = &result.ir().model.semantic_annotations[0];
        assert_eq!(balloon.text, ["7"]);
        assert_eq!(
            balloon.references["Source"][0]
                .native_text()
                .and_then(|text| text.split_once('.'))
                .map(|(_, subelement)| subelement),
            Some("Face1")
        );
        let symbol = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.kind == Kind::Symbol)
            .expect("semantic symbol");
        assert_eq!(symbol.assets.len(), 1);
        assert!(symbol.assets[0].ends_with("symbol.svg"));
        assert!(crate::test_support::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
    }
}
