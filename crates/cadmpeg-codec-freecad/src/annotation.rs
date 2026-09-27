// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph recovery.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
};
use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};

use crate::native::{
    sole_named_property, AnnotationRuntimeType, DrawingRecord, ObjectRecord, PropertyRecord,
    SemanticAnnotationRecord,
};
use crate::resource::{collection_allocation_failed, collection_vec, reserve_vec_items, retained_string, retained_strings};

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<SemanticAnnotationRecord>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            ctx.charge_collection_items(1, "fcstd annotation owner index")?;
            by_owner.try_reserve(1).map_err(|_| collection_allocation_failed(ctx, 1, "fcstd annotation owner index"))?;
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            reserve_vec_items(ctx, owned, 1, "fcstd annotation owner properties")?;
            owned.push(property);
        }
    }
    let mut records = collection_vec(ctx, objects.len(), "fcstd annotation records")?;
    for object in objects {
        if let Some(kind) = AnnotationRuntimeType::from_label(&object.type_name) {
            let schema = annotation_schema(kind);
            let source = by_owner.get(object.id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
            let mut owned = collection_vec(ctx, source.len(), "fcstd annotation selected properties")?;
            owned.extend_from_slice(source);
            owned.sort_by_key(|property| (property.xml.start(), property.xml.end()));
            let mut references = BTreeMap::new();
            let mut parameters = BTreeMap::new();
            for property in &owned {
                let name = retained_string(ctx, &property.name, "fcstd annotation property name")?;
                ctx.charge_collection_items(1, "fcstd annotation property map")?;
                if property.links().is_empty() {
                    parameters.insert(name, retained_string(ctx, property.xml.text(), "fcstd annotation parameter XML")?);
                } else {
                    let mut links = collection_vec(ctx, property.links().len(), "fcstd annotation links")?;
                    for link in property.links() {
                        links.push(link.as_ref().map(|link| link.clone_with_context(ctx)).transpose()?);
                    }
                    references.insert(name, links);
                }
            }
            let mut text = Vec::new();
            if let Some(carrier) = schema.text {
                for property in owned.iter().filter(|property| property.name == carrier.property) {
                    match strict_text_values(ctx, property, carrier.type_name) {
                        Ok(values) => {
                            reserve_vec_items(ctx, &mut text, values.len(), "fcstd annotation text")?;
                            text.extend(values);
                        }
                        Err(CodecError::ResourceLimit(limit)) => return Err(CodecError::ResourceLimit(limit)),
                        Err(_) => {}
                    }
                }
            }
            let mut side_entries = Vec::new();
            for property in &owned {
                for name in property.side_entries() {
                    reserve_vec_items(ctx, &mut side_entries, 1, "fcstd annotation side entries")?;
                    side_entries.push(retained_string(ctx, name, "fcstd annotation side entry")?);
                }
            }
            records.push(SemanticAnnotationRecord {
                id: crate::native::native_id_charged(ctx, "annotation", &object.name)?,
                object: retained_string(ctx, &object.id, "fcstd annotation object")?,
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
    drawings: &[DrawingRecord],
) -> Result<(), CodecError> {
    let mut drawing_ids = HashMap::new();
    ctx.charge_collection_items(drawings.len() as u64, "fcstd annotation drawing index")?;
    drawing_ids.try_reserve(drawings.len()).map_err(|_| collection_allocation_failed(ctx, drawings.len() as u64, "fcstd annotation drawing index"))?;
    for drawing in drawings {
        drawing_ids.insert(drawing.object.as_str(), crate::native::model_id_charged(ctx, "drawing", &drawing.object, "entity")?);
    }
    for (order, record) in records.iter().enumerate() {
        let schema = annotation_schema(record.kind);
        let count = properties.iter().filter(|property| property.owner == record.object).count();
        let mut owned = collection_vec(ctx, count, "fcstd neutral annotation properties")?;
        owned.extend(properties.iter().filter(|property| property.owner == record.object));
        validate_text_carriers(ctx, &owned, &schema)?;
        let target = |link: &Option<crate::native::LinkTarget>| {
            let Some(link) = link.as_ref() else {
                return Ok(ReferenceSelection::new(ReferenceTarget::Null, Vec::new()));
            };
            let target = match (link.document_name(), link.object()) {
                (Some(document), Some(object)) => ReferenceTarget::External {
                    document: retained_string(ctx, document, "fcstd annotation external document")?,
                    object: retained_string(ctx, object, "fcstd annotation external object")?,
                },
                (None, None) => ReferenceTarget::Null,
                (None, Some(object)) => ReferenceTarget::Local(retained_string(
                    ctx,
                    drawing_ids.get(object).map(String::as_str).unwrap_or(object),
                    "fcstd annotation local reference",
                )?),
                _ => {
                    return Err(CodecError::malformed(
                        "semantic annotation reference has no complete target",
                    ));
                }
            };
            Ok(ReferenceSelection::new(target, retained_strings(ctx, link.subelements(), "fcstd annotation subelements")?))
        };
        let mut references = BTreeMap::new();
        for (role, targets) in &record.references {
            let mut selections = collection_vec(ctx, targets.len(), "fcstd annotation reference selections")?;
            for link in targets {
                selections.push(target(link)?);
            }
            ctx.charge_collection_items(1, "fcstd annotation reference roles")?;
            references.insert(retained_string(ctx, role, "fcstd annotation reference role")?, selections);
        }
        reserve_vec_items(ctx, &mut model.semantic_annotations, 1, "fcstd neutral annotations")?;
        let mut parameters = BTreeMap::new();
        for (name, value) in &record.parameters {
            ctx.charge_collection_items(1, "fcstd annotation neutral parameters")?;
            parameters.insert(retained_string(ctx, name, "fcstd annotation parameter name")?, retained_string(ctx, value, "fcstd annotation parameter value")?);
        }
        let mut assets = collection_vec(ctx, record.side_entries.len(), "fcstd annotation assets")?;
        for name in &record.side_entries {
            assets.push(crate::native::native_id_charged(ctx, "entry", name)?);
        }
        model.semantic_annotations.push(SemanticAnnotation {
            id: SemanticAnnotationId::compose(
                &cadmpeg_ir::identity_namespace!("fcstd", "model", "semantic-annotation"),
                crate::native::model_key(&record.object, "content")
                    .map_err(CodecError::malformed)?,
            ),
            object: retained_string(ctx, &record.object, "fcstd neutral annotation object")?,
            kind: schema.kind.clone(),
            runtime_type: retained_string(ctx, record.kind.as_str(), "fcstd annotation runtime type")?,
            order: order as u32,
            text: retained_strings(ctx, &record.text, "fcstd annotation neutral text")?,
            references: cadmpeg_core::text::named_entries(&record.object, references)?,
            value: None,
            format: match schema.text {
                Some(carrier) if carrier.has_format_spec => {
                    string_property(ctx, &owned, carrier.property, carrier.type_name)?
                }
                _ => None,
            },
            position: annotation_position(ctx, &owned, schema.position)?,
            parameters: cadmpeg_core::text::named_entries(
                &record.object,
                parameters,
            )?,
            assets,
            native_ref: retained_string(ctx, &record.id, "fcstd annotation native reference")?,
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

fn optional_scalar_property(
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
    let attributes =
        direct_value_attributes(ctx, property, "PropertyVector", &["valueX", "valueY", "valueZ"])?;
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
    attributes.into_iter().find_map(|(name, value)| (name == "value").then_some(value)).map(Some).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "annotation property {} string value is missing value",
            property.id
        ))
    })
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
    let document = roxmltree::Document::parse(property.xml.text()).map_err(|error| {
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
        ctx.charge_collection_items(1, "fcstd annotation value attributes")?;
        attributes.insert(
            retained_string(ctx, attribute.name(), "fcstd annotation attribute name")?,
            retained_string(ctx, attribute.value(), "fcstd annotation attribute value")?,
        );
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
        let value = attributes.into_iter().find_map(|(name, value)| (name == "value").then_some(value)).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "annotation property {} string value is missing value",
                property.id
            ))
        })?;
        let mut values = Vec::new();
        if !value.trim().is_empty() {
            reserve_vec_items(ctx, &mut values, 1, "fcstd annotation single text")?;
            values.push(value);
        }
        return Ok(values);
    }
    let document = roxmltree::Document::parse(property.xml.text()).map_err(|error| {
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
    let found = string_list.children().filter(roxmltree::Node::is_element).count();
    if found != count {
        return Err(CodecError::malformed(format_args!(
            "annotation property {} StringList count={count} but {} direct String values were found",
            property.id,
            found
        )));
    }
    let mut texts = collection_vec(ctx, count, "fcstd annotation StringList values")?;
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
                texts.push(retained_string(ctx, value, "fcstd annotation StringList text")?);
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

#[cfg(test)]
pub(crate) mod tests {
    use super::is_annotation_type;
    use crate::test_support::test_archive::{archive, archive_entries, assert_valid_document};
    use crate::FcstdCodec;
    use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind as Kind;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    #[test]
    fn annotation_record_collection_refuses_at_caller_limit() {
        let object = crate::native::ObjectRecord {
            id: "fcstd:native:object#Note".into(),
            name: "Note".into(),
            type_name: "App::Annotation".into(),
            persistent_id: None,
            view_type: None,
            attributes: Default::default(),
            dependencies: Vec::new(),
            dependency_allow_partial: None,
            order: 0,
            data: None,
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::transfer(&ctx, &[object], &[]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "fcstd annotation records"));
    }

    #[test]
    fn annotation_identity_refuses_at_retained_limit() {
        let object = crate::native::ObjectRecord {
            id: "fcstd:native:object#Note".into(),
            name: "Note".into(),
            type_name: "App::Annotation".into(),
            persistent_id: None,
            view_type: None,
            attributes: Default::default(),
            dependencies: Vec::new(),
            dependency_allow_partial: None,
            order: 0,
            data: None,
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = crate::native::native_id("annotation", &object.name).len() as u64 - 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::transfer(&ctx, &[object], &[]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD native identity"));
    }

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
            semantic_dimension.references["References2D"][0].subelements,
            ["Edge1"]
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
            semantic_note.references["View"][0].local_target(),
            Some(neutral_view.id.as_str())
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
        assert_eq!(balloon.references["Source"][0].subelements, ["Face1"]);
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
