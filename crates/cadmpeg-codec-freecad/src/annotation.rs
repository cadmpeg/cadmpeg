// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph recovery.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
};
use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};

use crate::native::{
    AnnotationRuntimeType, DrawingRecord, ObjectRecord, PropertyRecord, SemanticAnnotationRecord,
};

fn annotation_malformed(ctx: &DecodeContext<'_>, message: std::fmt::Arguments<'_>) -> CodecError {
    crate::resource::malformed_charged(ctx, message, "fcstd annotation diagnostic")
}

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<SemanticAnnotationRecord>, CodecError> {
    if objects.is_empty() {
        ctx.reserve_scoped(0, "fcstd annotation object search")?;
        return Ok(Vec::new());
    }
    if !ctx.any_by(
        objects,
        |object| Ok(is_annotation_type(&object.type_name)),
        "fcstd annotation object search",
    )? {
        return Ok(Vec::new());
    }
    let by_owner = ctx.collect_scoped_btree_groups(
        properties
            .iter()
            .map(|property| (property.owner.as_str(), property)),
        "fcstd annotation owner properties",
    )?;
    let _owner_storage = by_owner.1;
    let by_owner = by_owner.0;
    let mut records = Vec::new();
    let mut object_iter = objects.iter();
    while object_iter.len() != 0 {
        let Some(object) = ctx.next_charged(&mut object_iter, "fcstd annotation objects")? else {
            break;
        };
        if let Some(kind) = AnnotationRuntimeType::from_label(&object.type_name) {
            let schema = annotation_schema(kind);
            let source = ctx
                .get_btree_map(
                    &by_owner,
                    object.id().as_str(),
                    "fcstd annotation owner lookup",
                )?
                .map_or(&[][..], Vec::as_slice);
            let owned = ctx.with_scoped_storage("fcstd annotation selected properties", || {
                ctx.copy_slice(source, "fcstd annotation selected properties")
            })?;
            let _selected_storage = owned.1;
            let mut owned = owned.0;
            ctx.stable_sort_by_key(
                &mut owned,
                |value| (value.xml.start(), value.xml.end()),
                Ord::cmp,
                "fcstd annotation selected properties sort",
            )?;
            let mut references = BTreeMap::new();
            let mut parameters = BTreeMap::new();
            let mut property_iter = owned.iter();
            while property_iter.len() != 0 {
                let Some(property) =
                    ctx.next_charged(&mut property_iter, "fcstd annotation properties")?
                else {
                    break;
                };
                let name =
                    ctx.copy_retained_text(&property.name, "fcstd annotation property name")?;
                if property.links().is_empty() {
                    ctx.insert_btree_map(
                        &mut parameters,
                        name,
                        ctx.copy_retained_text(
                            property.xml.text(),
                            "fcstd annotation parameter XML",
                        )?,
                        "fcstd annotation property map",
                    )?;
                } else {
                    let mut links =
                        ctx.collection_vec(property.links().len(), "fcstd annotation links")?;
                    let mut link_iter = property.links().iter();
                    while link_iter.len() != 0 {
                        let Some(link) =
                            ctx.next_charged(&mut link_iter, "fcstd annotation link visits")?
                        else {
                            break;
                        };
                        links.push(
                            link.as_ref()
                                .map(|link| link.clone_with_context(ctx))
                                .transpose()?,
                        );
                    }
                    ctx.insert_btree_map(
                        &mut references,
                        name,
                        links,
                        "fcstd annotation property map",
                    )?;
                }
            }
            let mut text = Vec::new();
            if let Some(carrier) = schema.text {
                let mut property_iter = owned.iter();
                while property_iter.len() != 0 {
                    let Some(property) =
                        ctx.next_charged(&mut property_iter, "fcstd annotation text properties")?
                    else {
                        break;
                    };
                    if property.name != carrier.property {
                        continue;
                    }
                    match strict_text_values(ctx, property, carrier.type_name, Some(&mut text)) {
                        Ok(()) => {}
                        Err(CodecError::ResourceLimit(limit)) => {
                            return Err(CodecError::ResourceLimit(limit))
                        }
                        Err(_) => {}
                    }
                }
            }
            let mut side_entries = Vec::new();
            let mut property_iter = owned.iter();
            while property_iter.len() != 0 {
                let Some(property) =
                    ctx.next_charged(&mut property_iter, "fcstd annotation properties")?
                else {
                    break;
                };
                let mut side_entry_iter = property.side_entries().iter();
                while side_entry_iter.len() != 0 {
                    let Some(name) = ctx
                        .next_charged(&mut side_entry_iter, "fcstd annotation side-entry names")?
                    else {
                        break;
                    };
                    ctx.reserve_vec(&mut side_entries, 1, "fcstd annotation side entries")?;
                    side_entries.push(ctx.copy_retained_text(name, "fcstd annotation side entry")?);
                }
            }
            ctx.push_vec(
                &mut records,
                SemanticAnnotationRecord {
                    id: crate::native::native_id_charged(ctx, "annotation", object.name())?,
                    object: ctx.copy_retained_text(object.id(), "fcstd annotation object")?,
                    kind,
                    text,
                    references,
                    parameters,
                    side_entries,
                },
                "fcstd annotation records",
            )?;
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
    let mut lookup_storage = ctx.reserve_scoped(0, "FreeCAD annotation neutral lookup")?;
    if records.is_empty() {
        return Ok(());
    }
    let by_owner = ctx.collect_scoped_btree_groups(
        properties
            .iter()
            .map(|property| (property.owner.as_str(), property)),
        "fcstd neutral annotation properties",
    )?;
    let _owner_storage = by_owner.1;
    let by_owner = by_owner.0;
    let mut drawing_ids = BTreeMap::new();
    let mut drawing_ids_built = false;
    let mut record_iter = records.iter().enumerate();
    while record_iter.len() != 0 {
        let Some((order, record)) =
            ctx.next_charged(&mut record_iter, "fcstd neutral annotation records")?
        else {
            break;
        };
        let schema = annotation_schema(record.kind);
        let owned = ctx
            .get_btree_map(
                &by_owner,
                record.object.as_str(),
                "fcstd neutral annotation owner lookup",
            )?
            .map_or(&[][..], Vec::as_slice);
        let format = validate_text_carriers(ctx, owned, &schema)?;
        let mut target = |link: &Option<crate::native::LinkTarget>| {
            let Some(link) = link.as_ref() else {
                return Ok(ReferenceSelection::new(ReferenceTarget::Null, Vec::new()));
            };
            let target = match (link.document_name(), link.object()) {
                (Some(document), Some(object)) => ReferenceTarget::External {
                    document: ctx
                        .copy_retained_text(document, "fcstd annotation external document")?,
                    object: ctx.copy_retained_text(object, "fcstd annotation external object")?,
                },
                (None, None) => ReferenceTarget::Null,
                (None, Some(object)) => {
                    if !drawing_ids_built {
                        let mut drawing_iter = drawings.iter();
                        while drawing_iter.len() != 0 {
                            let Some(drawing) = ctx.next_charged(
                                &mut drawing_iter,
                                "fcstd annotation drawing identities",
                            )?
                            else {
                                break;
                            };
                            lookup_storage.with_storage(|| {
                                ctx.insert_btree_map(
                                    &mut drawing_ids,
                                    drawing.object.as_str(),
                                    crate::native::model_id_charged(
                                        ctx,
                                        "drawing",
                                        &drawing.object,
                                        "entity",
                                    )?,
                                    "fcstd annotation drawing index",
                                )
                            })?;
                        }
                        drawing_ids_built = true;
                    }
                    ReferenceTarget::Local(
                        ctx.copy_retained_text(
                            ctx.get_btree_map(
                                &drawing_ids,
                                object,
                                "fcstd annotation drawing lookup",
                            )?
                            .map_or(object, String::as_str),
                            "fcstd annotation local reference",
                        )?,
                    )
                }
                _ => {
                    return Err(CodecError::malformed(
                        "semantic annotation reference has no complete target",
                    ));
                }
            };
            Ok(ReferenceSelection::new(
                target,
                ctx.copy_retained_strings(link.subelements(), "fcstd annotation subelements")?,
            ))
        };
        let mut references = BTreeMap::new();
        let mut role_iter = record.references.iter();
        while role_iter.len() != 0 {
            let Some((role, targets)) =
                ctx.next_charged(&mut role_iter, "fcstd annotation reference roles")?
            else {
                break;
            };
            let mut selections =
                ctx.collection_vec(targets.len(), "fcstd annotation reference selections")?;
            let mut target_iter = targets.iter();
            while target_iter.len() != 0 {
                let Some(link) =
                    ctx.next_charged(&mut target_iter, "fcstd annotation reference targets")?
                else {
                    break;
                };
                selections.push(target(link)?);
            }
            ctx.insert_btree_map(
                &mut references,
                ctx.copy_retained_text(role, "fcstd annotation reference role")?,
                selections,
                "fcstd annotation reference roles",
            )?;
        }
        ctx.reserve_vec(
            &mut model.semantic_annotations,
            1,
            "fcstd neutral annotations",
        )?;
        let mut parameters = BTreeMap::new();
        let mut parameter_iter = record.parameters.iter();
        while parameter_iter.len() != 0 {
            let Some((name, value)) =
                ctx.next_charged(&mut parameter_iter, "fcstd annotation parameters")?
            else {
                break;
            };
            ctx.insert_btree_map(
                &mut parameters,
                ctx.copy_retained_text(name, "fcstd annotation parameter name")?,
                ctx.copy_retained_text(value, "fcstd annotation parameter value")?,
                "fcstd annotation neutral parameters",
            )?;
        }
        let mut assets =
            ctx.collection_vec(record.side_entries.len(), "fcstd annotation assets")?;
        let mut asset_iter = record.side_entries.iter();
        while asset_iter.len() != 0 {
            let Some(name) = ctx.next_charged(&mut asset_iter, "fcstd annotation asset names")?
            else {
                break;
            };
            assets.push(crate::native::native_id_charged(ctx, "entry", name)?);
        }
        model.semantic_annotations.push(SemanticAnnotation {
            id: SemanticAnnotationId::mint(crate::native::model_id_charged(
                ctx,
                "semantic-annotation",
                &record.object,
                "content",
            )?)
            .map_err(CodecError::malformed)?,
            object: ctx.copy_retained_text(&record.object, "fcstd neutral annotation object")?,
            kind: schema.kind.clone(),
            runtime_type: ctx
                .copy_retained_text(record.kind.as_str(), "fcstd annotation runtime type")?,
            order: u32::try_from(order).map_err(|_| {
                ctx.refuse_codec_limit(
                    "FreeCAD ordinal",
                    u64::from(u32::MAX),
                    cadmpeg_core::decode::u64_from_index(order),
                )
            })?,
            text: ctx.copy_retained_strings(&record.text, "fcstd annotation neutral text")?,
            references: cadmpeg_core::text::named_entries_for_decode(
                ctx,
                &record.object,
                references,
            )?,
            value: None,
            format,
            position: annotation_position(ctx, owned, schema.position)?,
            parameters: cadmpeg_core::text::named_entries_for_decode(
                ctx,
                &record.object,
                parameters,
            )?,
            assets,
            native_ref: ctx.copy_retained_text(&record.id, "fcstd annotation native reference")?,
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
                _ => Err(annotation_malformed(
                    ctx,
                    format_args!("annotation position requires both {x_name} and {y_name}"),
                )),
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
    let Some(property) = typed_property(ctx, properties, name, type_names)? else {
        return Ok(None);
    };
    direct_value(ctx, property, "Float", &["value"], |[text]| {
        let value = text
            .map(|text| ctx.parse_text::<f64>(text, "fcstd annotation scalar"))
            .transpose()?
            .and_then(Result::ok)
            .ok_or_else(|| {
                annotation_malformed(
                    ctx,
                    format_args!("annotation property {} is not a scalar", property.id),
                )
            })?;
        Ok(Some(value))
    })
}

fn optional_vector_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    type_names: &[&str],
) -> Result<Option<[f64; 3]>, CodecError> {
    let Some(property) = typed_property(ctx, properties, name, type_names)? else {
        return Ok(None);
    };
    direct_value(
        ctx,
        property,
        "PropertyVector",
        &["valueX", "valueY", "valueZ"],
        |attributes| {
            let mut vector = [0.0; 3];
            for (index, text) in attributes.into_iter().enumerate() {
                vector[index] = text
                    .map(|text| ctx.parse_text::<f64>(text, "fcstd annotation vector scalar"))
                    .transpose()?
                    .and_then(Result::ok)
                    .ok_or_else(|| {
                        annotation_malformed(
                            ctx,
                            format_args!("annotation property {} is not a vector", property.id),
                        )
                    })?;
            }
            Ok(Some(vector))
        },
    )
}

fn string_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<String>, CodecError> {
    let Some(property) = typed_property(ctx, properties, name, &[type_name])? else {
        return Ok(None);
    };
    direct_value(ctx, property, "String", &["value"], |[text]| {
        let text = text.ok_or_else(|| {
            annotation_malformed(
                ctx,
                format_args!(
                    "annotation property {} string value is missing value",
                    property.id
                ),
            )
        })?;
        Ok(Some(ctx.copy_retained_text(
            text,
            "fcstd annotation string value",
        )?))
    })
}

fn typed_property<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
    name: &str,
    type_names: &[&str],
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let Some(index) = ctx.position_by(
        properties,
        |property| Ok(property.name == name),
        "fcstd annotation carrier lookup",
    )?
    else {
        return Ok(None);
    };
    if ctx.any_by(
        &properties[index + 1..],
        |property| Ok(property.name == name),
        "fcstd annotation duplicate carrier lookup",
    )? {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("annotation property {name} occurs more than once"),
            "FreeCAD duplicate property diagnostic",
        )?));
    }
    let property = properties[index];
    if !type_names.contains(&property.type_name.as_str()) {
        let expected = type_names.join(" or ");
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {name} has runtime type {}, expected {expected}",
                property.type_name
            ),
        ));
    }
    Ok(Some(property))
}

fn validate_text_carriers(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    schema: &AnnotationSchema,
) -> Result<Option<String>, CodecError> {
    let Some(carrier) = &schema.text else {
        return Ok(None);
    };
    if carrier.has_format_spec {
        return string_property(ctx, properties, carrier.property, carrier.type_name);
    }
    if let Some(property) = typed_property(ctx, properties, carrier.property, &[carrier.type_name])?
    {
        strict_text_values(ctx, property, carrier.type_name, None)?;
    }
    Ok(None)
}

fn direct_value<T, const N: usize>(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_tag: &str,
    allowed_attributes: &[&str; N],
    read: impl FnOnce([Option<&str>; N]) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let admitted_document = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            annotation_malformed(
                ctx,
                format_args!(
                    "annotation property {} has invalid XML: {error}",
                    property.id
                ),
            )
        })?;
    let document = admitted_document.document();
    let root = ctx.xml_root_element(document, "fcstd annotation XML root")?;
    if has_non_whitespace_text(ctx, root)? {
        return Err(annotation_malformed(
            ctx,
            format_args!("annotation property {} has unexpected text", property.id),
        ));
    }
    let mut values = root.children();
    let Some(value) = ctx.find_by(
        &mut values,
        |node| Ok(node.is_element()),
        "fcstd annotation direct value",
    )?
    else {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} requires one direct {expected_tag} value",
                property.id
            ),
        ));
    };
    if ctx.any_by(
        &mut values,
        |node| Ok(node.is_element()),
        "fcstd annotation extra direct value",
    )? {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} requires one direct {expected_tag} value",
                property.id
            ),
        ));
    }
    if !ctx.xml_has_tag_name(value, expected_tag, "fcstd annotation value tag")? {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} has root {}, expected {expected_tag}",
                property.id,
                value.tag_name().name()
            ),
        ));
    }
    validate_leaf_value(ctx, value, property, allowed_attributes)?;
    let mut selected = [None; N];
    let mut attributes = value.attributes();
    while attributes.len() != 0 {
        let Some(attribute) =
            ctx.next_charged(&mut attributes, "fcstd annotation direct attributes")?
        else {
            break;
        };
        // Each carrier has one or three literal names. Preserve the last local
        // value when distinct namespaces state the same local attribute name.
        for (index, name) in allowed_attributes.iter().enumerate() {
            if attribute.name() == *name {
                selected[index] = Some(attribute.value());
            }
        }
    }
    read(selected)
}

fn strict_text_values(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
    expected_type: &str,
    texts: Option<&mut Vec<String>>,
) -> Result<(), CodecError> {
    if property.type_name != expected_type {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} has runtime type {}, expected {expected_type}",
                property.id, property.type_name
            ),
        ));
    }
    if expected_type == "App::PropertyString" {
        return direct_value(ctx, property, "String", &["value"], |[text]| {
            let value = text.ok_or_else(|| {
                annotation_malformed(
                    ctx,
                    format_args!(
                        "annotation property {} string value is missing value",
                        property.id
                    ),
                )
            })?;
            if let Some(texts) = texts {
                if ctx.any_by(
                    value.chars(),
                    |character| Ok(!character.is_whitespace()),
                    "fcstd annotation blank text",
                )? {
                    ctx.push_vec(
                        texts,
                        ctx.copy_retained_text(value, "fcstd annotation single text value")?,
                        "fcstd annotation text",
                    )?;
                }
            }
            Ok(())
        });
    }
    let admitted_document = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            annotation_malformed(
                ctx,
                format_args!(
                    "annotation property {} has invalid XML: {error}",
                    property.id
                ),
            )
        })?;
    let document = admitted_document.document();
    let root = ctx.xml_root_element(document, "fcstd annotation XML root")?;
    if has_non_whitespace_text(ctx, root)? {
        return Err(annotation_malformed(
            ctx,
            format_args!("annotation property {} has unexpected text", property.id),
        ));
    }
    let mut values = root.children();
    let Some(string_list) = ctx.find_by(
        &mut values,
        |node| Ok(node.is_element()),
        "fcstd annotation direct list",
    )?
    else {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} requires one direct StringList value",
                property.id
            ),
        ));
    };
    if ctx.any_by(
        &mut values,
        |node| Ok(node.is_element()),
        "fcstd annotation extra direct value",
    )? {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} requires one direct StringList value",
                property.id
            ),
        ));
    }
    if !ctx.xml_has_tag_name(string_list, "StringList", "fcstd annotation list tag")? {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} has root {}, expected StringList",
                property.id,
                string_list.tag_name().name()
            ),
        ));
    }
    validate_attributes(ctx, string_list, property, &["count"])?;
    let count = ctx
        .xml_attribute(
            string_list,
            "count",
            "fcstd annotation list count attribute",
        )?
        .map(|value| ctx.parse_text::<usize>(value, "fcstd annotation list count"))
        .transpose()?
        .and_then(Result::ok)
        .ok_or_else(|| {
            annotation_malformed(
                ctx,
                format_args!(
                    "annotation property {} StringList has an invalid count",
                    property.id
                ),
            )
        })?;
    if has_non_whitespace_text(ctx, string_list)? {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} StringList has unexpected text",
                property.id
            ),
        ));
    }
    let mut selected_storage = ctx.reserve_scoped(0, "fcstd annotation selected text")?;
    let mut selected = Vec::new();
    let mut found = 0;
    let mut children = string_list.children();
    while let Some(string) = ctx.next_charged(&mut children, "fcstd annotation list children")? {
        if !string.is_element() {
            continue;
        }
        found += 1;
        if !ctx.xml_has_tag_name(string, "String", "fcstd annotation string tag")? {
            return Err(annotation_malformed(
                ctx,
                format_args!(
                    "annotation property {} StringList has an unexpected child {}",
                    property.id,
                    string.tag_name().name()
                ),
            ));
        }
        validate_leaf_value(ctx, string, property, &["value"])?;
        let value = ctx
            .xml_attribute(string, "value", "fcstd annotation list value attribute")?
            .ok_or_else(|| {
                annotation_malformed(
                    ctx,
                    format_args!(
                        "annotation property {} String value is missing value",
                        property.id
                    ),
                )
            })?;
        if texts.is_some()
            && ctx.any_by(
                value.chars(),
                |character| Ok(!character.is_whitespace()),
                "fcstd annotation blank text",
            )?
        {
            selected_storage.with_storage(|| {
                ctx.push_vec(&mut selected, value, "fcstd annotation selected text")
            })?;
        }
    }
    if found != count {
        return Err(annotation_malformed(ctx, format_args!(
            "annotation property {} StringList count={count} but {} direct String values were found", property.id, found
        )));
    }
    if let Some(texts) = texts {
        let mut selected_iter = selected.iter();
        while selected_iter.len() != 0 {
            let Some(value) =
                ctx.next_charged(&mut selected_iter, "fcstd annotation selected text visits")?
            else {
                break;
            };
            ctx.push_vec(
                texts,
                ctx.copy_retained_text(value, "fcstd annotation StringList text")?,
                "fcstd annotation text",
            )?;
        }
    }
    Ok(())
}

fn validate_leaf_value(
    ctx: &DecodeContext<'_>,
    value: roxmltree::Node<'_, '_>,
    property: &PropertyRecord,
    allowed_attributes: &[&str],
) -> Result<(), CodecError> {
    validate_attributes(ctx, value, property, allowed_attributes)?;
    if ctx.any_by(
        value.children(),
        |child| Ok(child.is_element()),
        "fcstd annotation leaf children",
    )? || has_non_whitespace_text(ctx, value)?
    {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} value {} has nested content",
                property.id,
                value.tag_name().name()
            ),
        ));
    }
    Ok(())
}

fn validate_attributes(
    ctx: &DecodeContext<'_>,
    value: roxmltree::Node<'_, '_>,
    property: &PropertyRecord,
    allowed_attributes: &[&str],
) -> Result<(), CodecError> {
    if ctx.any_by(
        value.attributes(),
        |attribute| Ok(!allowed_attributes.contains(&attribute.name())),
        "fcstd annotation allowed attributes",
    )? {
        return Err(annotation_malformed(
            ctx,
            format_args!(
                "annotation property {} value {} has unsupported attributes",
                property.id,
                value.tag_name().name()
            ),
        ));
    }
    Ok(())
}

fn has_non_whitespace_text(
    ctx: &DecodeContext<'_>,
    value: roxmltree::Node<'_, '_>,
) -> Result<bool, CodecError> {
    ctx.any_by(
        value.children(),
        |child| {
            if !child.is_text() {
                return Ok(false);
            }
            let Some(text) = child.text() else {
                return Ok(false);
            };
            ctx.any_by(
                text.chars(),
                |character| Ok(!character.is_whitespace()),
                "fcstd annotation XML whitespace",
            )
        },
        "fcstd annotation text children",
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::is_annotation_type;
    use crate::test_support::test_archive::{archive, archive_entries, assert_valid_document};
    use crate::FcstdCodec;
    use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind as Kind;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    fn carrier_property(name: &str, type_name: &str, xml: &str) -> crate::native::PropertyRecord {
        crate::native::PropertyRecord {
            id: "fcstd:native:property#Note:Carrier".into(),
            owner: "fcstd:native:object#Note".into(),
            name: name.into(),
            type_name: type_name.into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Transient,
            order: 0,
            xml: crate::native::RetainedXml::from_text(xml.into(), 0).expect("XML"),
        }
    }

    fn neutral_annotation_record(object: &str) -> crate::native::SemanticAnnotationRecord {
        crate::native::SemanticAnnotationRecord {
            id: "fcstd:native:annotation#fixture".into(),
            object: object.to_owned(),
            kind: crate::native::AnnotationRuntimeType::Annotation,
            text: Vec::new(),
            references: std::collections::BTreeMap::default(),
            parameters: std::collections::BTreeMap::default(),
            side_entries: Vec::new(),
        }
    }

    fn drawing_record(object: &str) -> crate::native::DrawingRecord {
        crate::native::DrawingRecord {
            id: "fcstd:native:drawing#fixture".into(),
            object: object.to_owned(),
            kind: crate::native::TechDrawKind::Page {
                runtime: crate::native::TechDrawPageKind::Page,
                views: Vec::new(),
                template: None,
            },
            sources: Vec::new(),
            relationships: std::collections::BTreeMap::default(),
            parameters: std::collections::BTreeMap::default(),
            side_entries: Vec::new(),
        }
    }

    fn reference_link(
        document: Option<&str>,
        document_attribute: Option<&str>,
        object: Option<&str>,
    ) -> crate::native::LinkTarget {
        serde_json::from_value(serde_json::json!({
            "document": document,
            "document_attribute": document_attribute,
            "object": object,
            "subelements": [],
        }))
        .expect("valid reference link")
    }

    fn work_before_annotation_follow_up(
        records: &[crate::native::SemanticAnnotationRecord],
        drawings: &[crate::native::DrawingRecord],
    ) -> u64 {
        let error = crate::test_support::refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            &[],
            "after annotation neutral record",
            |ctx| {
                let mut model = cadmpeg_ir::document::Model::default();
                super::transfer_neutral(ctx, &mut model, records, &[], drawings)?;
                ctx.charge_work(1, "after annotation neutral record")
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal")
        };
        assert_eq!(limit.operation, "after annotation neutral record");
        limit.used
    }

    fn work_before_annotation_malformed_suffix(
        records: &[crate::native::SemanticAnnotationRecord],
        properties: &[crate::native::PropertyRecord],
    ) -> u64 {
        let error = crate::test_support::refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            &[],
            "after malformed annotation record",
            |ctx| {
                let mut model = cadmpeg_ir::document::Model::default();
                assert!(matches!(
                    super::transfer_neutral(ctx, &mut model, records, properties, &[]),
                    Err(cadmpeg_core::CodecError::Malformed(message))
                        if message.contains("position requires both X and Y")
                ));
                ctx.charge_work(1, "after malformed annotation record")
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal")
        };
        assert_eq!(limit.operation, "after malformed annotation record");
        limit.used
    }

    #[test]
    fn annotation_numeric_attributes_are_borrowed() {
        let scalar = carrier_property(
            "X",
            "App::PropertyFloat",
            r#"<Property><Float value="1.5"/></Property>"#,
        );
        let vector = carrier_property(
            "Position",
            "App::PropertyVector",
            r#"<Property><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>"#,
        );
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("context");
        assert_eq!(
            super::optional_scalar_property(&ctx, &[&scalar], "X", &["App::PropertyFloat"])
                .expect("scalar"),
            Some(1.5)
        );
        assert_eq!(
            super::optional_vector_property(&ctx, &[&vector], "Position", &["App::PropertyVector"])
                .expect("vector"),
            Some([1.0, 2.0, 3.0])
        );
        let error =
            crate::test_support::materialized_refusal_at("released annotation XML", |ctx| {
                super::optional_scalar_property(ctx, &[&scalar], "X", &["App::PropertyFloat"])?;
                super::optional_vector_property(
                    ctx,
                    &[&vector],
                    "Position",
                    &["App::PropertyVector"],
                )?;
                ctx.reserve_scoped(u64::MAX, "released annotation XML")
                    .map(|_| ())
            });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.used == 0));
    }

    #[test]
    fn annotation_direct_values_keep_the_last_local_attribute() {
        let scalar = carrier_property(
            "X",
            "App::PropertyFloat",
            r#"<Property><Float xmlns:n="urn:test" n:value="9" value="1.5"/></Property>"#,
        );
        let vector = carrier_property(
            "Position",
            "App::PropertyVector",
            r#"<Property><PropertyVector xmlns:n="urn:test" n:valueX="9" valueX="1" valueY="2" valueZ="3"/></Property>"#,
        );
        let text = carrier_property(
            "FormatSpec",
            "App::PropertyString",
            r#"<Property><String xmlns:n="urn:test" n:value="OLD" value="FINAL"/></Property>"#,
        );
        crate::test_support::with_service_context(&[], |ctx| {
            assert_eq!(
                super::optional_scalar_property(ctx, &[&scalar], "X", &["App::PropertyFloat"])
                    .expect("scalar"),
                Some(1.5)
            );
            assert_eq!(
                super::optional_vector_property(
                    ctx,
                    &[&vector],
                    "Position",
                    &["App::PropertyVector"]
                )
                .expect("vector"),
                Some([1.0, 2.0, 3.0])
            );
            assert_eq!(
                super::string_property(ctx, &[&text], "FormatSpec", "App::PropertyString")
                    .expect("format")
                    .as_deref(),
                Some("FINAL")
            );
            let mut values = Vec::new();
            super::strict_text_values(ctx, &text, "App::PropertyString", Some(&mut values))
                .expect("text");
            assert_eq!(values, ["FINAL"]);
        });
    }

    #[test]
    fn annotation_invalid_lists_do_not_append_partial_text() {
        for xml in [
            r#"<Property><StringList count="2"><String value="FIRST"/></StringList></Property>"#,
            r#"<Property><StringList count="2"><String value="FIRST"/><Invalid/></StringList></Property>"#,
        ] {
            let property = carrier_property("Text", "App::PropertyStringList", xml);
            crate::test_support::with_service_context(&[], |ctx| {
                let mut values = vec!["BEFORE".to_owned()];
                assert!(matches!(
                    super::strict_text_values(
                        ctx,
                        &property,
                        "App::PropertyStringList",
                        Some(&mut values)
                    ),
                    Err(cadmpeg_core::CodecError::Malformed(_))
                ));
                assert_eq!(values, ["BEFORE"]);
            });
        }
    }

    #[test]
    fn annotation_xml_queries_and_numbers_refuse_at_caller_limit() {
        let property = carrier_property(
            "Text",
            "App::PropertyStringList",
            r#"<Property><StringList count="1"><String value="NOTE"/></StringList></Property>"#,
        );
        for operation in [
            "fcstd annotation XML root",
            "fcstd annotation direct list",
            "fcstd annotation allowed attributes",
            "fcstd annotation list count attribute",
            "fcstd annotation list count",
            "fcstd annotation list children",
            "fcstd annotation list value attribute",
            "fcstd annotation blank text",
        ] {
            crate::test_support::refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                &[],
                operation,
                |ctx| {
                    super::strict_text_values(
                        ctx,
                        &property,
                        "App::PropertyStringList",
                        Some(&mut Vec::new()),
                    )
                },
            );
        }
        let property = carrier_property(
            "X",
            "App::PropertyFloat",
            r#"<Property><Float value="1.5"/></Property>"#,
        );
        for operation in [
            "fcstd annotation carrier lookup",
            "fcstd annotation direct attributes",
            "fcstd annotation scalar",
        ] {
            crate::test_support::refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                &[],
                operation,
                |ctx| {
                    super::optional_scalar_property(ctx, &[&property], "X", &["App::PropertyFloat"])
                },
            );
        }
    }

    #[test]
    fn annotation_diagnostic_refuses_at_matching_retained_limit() {
        crate::test_support::assert_retained_refusal_at(
            &[],
            "fcstd annotation diagnostic",
            |ctx| {
                Err::<(), _>(super::annotation_malformed(
                    ctx,
                    format_args!("annotation property {} has invalid XML", "Note"),
                ))
            },
        );
    }

    #[test]
    fn annotation_record_collection_refuses_at_caller_limit() {
        let object = crate::native::ObjectRecord {
            identity: crate::native::object_identity::ObjectIdentity::try_new(
                "fcstd:native:object#Note".into(),
                "Note".into(),
            )
            .expect("object identity"),
            type_name: "App::Annotation".into(),
            persistent_id: None,
            view_type: None,
            attributes: std::collections::BTreeMap::default(),
            dependencies: Vec::new(),
            dependency_allow_partial: None,
            order: 0,
            data: None,
        };
        crate::test_support::assert_collection_refusal_at(&[], "fcstd annotation records", |ctx| {
            super::transfer(ctx, std::slice::from_ref(&object), &[])
        });
    }

    #[test]
    fn annotation_identity_refuses_at_retained_limit() {
        let object = crate::native::ObjectRecord {
            identity: crate::native::object_identity::ObjectIdentity::try_new(
                "fcstd:native:object#Note".into(),
                "Note".into(),
            )
            .expect("object identity"),
            type_name: "App::Annotation".into(),
            persistent_id: None,
            view_type: None,
            attributes: std::collections::BTreeMap::default(),
            dependencies: Vec::new(),
            dependency_allow_partial: None,
            order: 0,
            data: None,
        };
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD native identity", |ctx| {
            super::transfer(ctx, std::slice::from_ref(&object), &[])
        });
    }

    #[test]
    fn annotation_keyed_references_refuse_at_collection_limit() {
        let record = crate::native::SemanticAnnotationRecord {
            id: "fcstd:native:annotation#Note".into(),
            object: "fcstd:native:object#Note".into(),
            kind: crate::native::AnnotationRuntimeType::Annotation,
            text: Vec::new(),
            references: std::collections::BTreeMap::from([("role".into(), Vec::new())]),
            parameters: std::collections::BTreeMap::default(),
            side_entries: Vec::new(),
        };
        crate::test_support::assert_collection_refusal_at(&[], "named entry map nodes", |ctx| {
            super::transfer_neutral(
                ctx,
                &mut cadmpeg_ir::document::Model::default(),
                std::slice::from_ref(&record),
                &[],
                &[],
            )
        });
    }

    #[test]
    fn annotation_keyed_parameters_refuse_at_collection_limit() {
        let record = crate::native::SemanticAnnotationRecord {
            id: "fcstd:native:annotation#Note".into(),
            object: "fcstd:native:object#Note".into(),
            kind: crate::native::AnnotationRuntimeType::Annotation,
            text: Vec::new(),
            references: std::collections::BTreeMap::default(),
            parameters: std::collections::BTreeMap::from([("role".into(), "value".into())]),
            side_entries: Vec::new(),
        };
        crate::test_support::assert_collection_refusal_at(&[], "named entry map nodes", |ctx| {
            super::transfer_neutral(
                ctx,
                &mut cadmpeg_ir::document::Model::default(),
                std::slice::from_ref(&record),
                &[],
                &[],
            )
        });
    }

    #[test]
    fn annotation_neutral_identity_refuses_at_retained_limit() {
        let record = crate::native::SemanticAnnotationRecord {
            id: "fcstd:native:annotation#Note".into(),
            object: "fcstd:native:object#Note".into(),
            kind: crate::native::AnnotationRuntimeType::Annotation,
            text: Vec::new(),
            references: std::collections::BTreeMap::default(),
            parameters: std::collections::BTreeMap::default(),
            side_entries: Vec::new(),
        };
        crate::test_support::assert_retained_refusal_at(&[], "FreeCAD model identity", |ctx| {
            super::transfer_neutral(
                ctx,
                &mut cadmpeg_ir::document::Model::default(),
                std::slice::from_ref(&record),
                &[],
                &[],
            )
        });
    }

    #[test]
    fn annotation_empty_neutral_transfer_is_zero_work_and_preserves_sticky_refusal() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("context");
        let mut model = cadmpeg_ir::document::Model::default();
        let unused_drawings = [drawing_record("fcstd:native:object#Unused")];
        super::transfer_neutral(&ctx, &mut model, &[], &[], &unused_drawings)
            .expect("empty annotations do no positive work");
        assert!(model.semantic_annotations.is_empty());

        let prior = ctx
            .charge_work(1, "prior annotation refusal")
            .expect_err("work limit");
        let error = super::transfer_neutral(&ctx, &mut model, &[], &[], &unused_drawings)
            .expect_err("empty annotation transfer preserves sticky refusal");
        let (
            cadmpeg_core::CodecError::ResourceLimit(prior),
            cadmpeg_core::CodecError::ResourceLimit(error),
        ) = (prior, error)
        else {
            panic!("resource refusal")
        };
        assert_eq!(error, prior);
    }

    #[test]
    fn annotation_empty_object_transfer_is_zero_work_and_preserves_sticky_refusal() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("context");
        assert!(super::transfer(&ctx, &[], &[])
            .expect("empty annotation object scan does no positive work")
            .is_empty());

        let prior = ctx
            .charge_work(1, "prior annotation object refusal")
            .expect_err("work limit");
        let error = super::transfer(&ctx, &[], &[])
            .expect_err("empty annotation object scan preserves sticky refusal");
        let (
            cadmpeg_core::CodecError::ResourceLimit(prior),
            cadmpeg_core::CodecError::ResourceLimit(error),
        ) = (prior, error)
        else {
            panic!("resource refusal")
        };
        assert_eq!(error, prior);
    }

    #[test]
    fn annotation_null_and_external_links_skip_drawing_identity_work() {
        let mut record = neutral_annotation_record("fcstd:native:object#Note");
        record.references = std::collections::BTreeMap::from([
            ("Null".into(), vec![None]),
            (
                "External".into(),
                vec![Some(reference_link(
                    Some("external.FCStd"),
                    Some("file"),
                    Some("Model"),
                ))],
            ),
        ]);
        let records = [
            record,
            neutral_annotation_record("fcstd:native:object#NoReferences"),
        ];
        let unused_drawings = (0..32)
            .map(|index| drawing_record(&format!("fcstd:native:object#Unused{index}")))
            .collect::<Vec<_>>();
        let no_drawings_work = work_before_annotation_follow_up(&records, &[]);
        let unused_drawings_work = work_before_annotation_follow_up(&records, &unused_drawings);
        assert_eq!(unused_drawings_work, no_drawings_work);
        let no_reference = std::slice::from_ref(&records[1]);
        assert_eq!(
            work_before_annotation_follow_up(no_reference, &[]),
            work_before_annotation_follow_up(no_reference, &unused_drawings)
        );

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        // Two target slots, two transient reference keys, two neutral keys, and two outputs.
        policy.limits.max_collection_items = 8;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("context");
        let mut model = cadmpeg_ir::document::Model::default();
        super::transfer_neutral(&ctx, &mut model, &records, &[], &unused_drawings)
            .expect("null, external, and no-reference annotations need no drawing identities");
        assert_eq!(
            &model.semantic_annotations[0].references["Null"][0].target,
            &cadmpeg_ir::ReferenceTarget::Null
        );
        assert!(matches!(
            &model.semantic_annotations[0].references["External"][0].target,
            cadmpeg_ir::ReferenceTarget::External { document, object }
                if document == "external.FCStd" && object == "Model"
        ));
        assert!(model.semantic_annotations[1].references.is_empty());
    }

    #[test]
    fn annotation_local_reference_resolves_through_lazy_drawing_identity_index() {
        let object = "fcstd:native:object#View";
        let mut record = neutral_annotation_record("fcstd:native:object#Note");
        record.references.insert(
            "View".into(),
            vec![Some(reference_link(None, None, Some(object)))],
        );
        let drawings = [drawing_record(object)];
        crate::test_support::with_service_context(&[], |ctx| {
            let mut model = cadmpeg_ir::document::Model::default();
            super::transfer_neutral(ctx, &mut model, &[record], &[], &drawings)
                .expect("local reference resolves");
            let expected = cadmpeg_ir::drawings::DrawingId::mint(crate::native::model_id(
                "drawing", object, "entity",
            ))
            .expect("drawing identity");
            assert_eq!(
                model.semantic_annotations[0].references["View"][0].local_target(),
                Some(expected.as_str())
            );
        });
    }

    #[test]
    fn annotation_local_reference_identity_refusal_is_incremental_and_sticky() {
        let object = "fcstd:native:object#View";
        let mut record = neutral_annotation_record("fcstd:native:object#Note");
        record.references.insert(
            "View".into(),
            vec![Some(reference_link(None, None, Some(object)))],
        );
        let drawings = (0..32)
            .map(|index| drawing_record(&format!("fcstd:native:object#Unused{index}")))
            .collect::<Vec<_>>();
        let error = crate::test_support::refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            &[],
            "fcstd annotation drawing identities",
            |ctx| {
                let mut model = cadmpeg_ir::document::Model::default();
                let error = super::transfer_neutral(
                    ctx,
                    &mut model,
                    std::slice::from_ref(&record),
                    &[],
                    &drawings,
                )
                .expect_err("local reference needs the drawing identity map");
                let sticky = ctx
                    .charge_work(0, "annotation identity sticky probe")
                    .expect_err("original work refusal stays sticky");
                assert!(matches!(
                    (&sticky, &error),
                    (
                        cadmpeg_core::CodecError::ResourceLimit(sticky),
                        cadmpeg_core::CodecError::ResourceLimit(error)
                    ) if sticky == error
                ));
                Err::<(), _>(error)
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "fcstd annotation drawing identities"
                    && limit.additional == 1
        ));
    }

    #[test]
    fn annotation_malformed_first_record_does_not_charge_unvisited_record_suffix() {
        let first = neutral_annotation_record("fcstd:native:object#Note");
        let mut records = vec![first.clone()];
        records.push(neutral_annotation_record("fcstd:native:object#Suffix0"));
        let small = records.clone();
        for index in 1..32 {
            records.push(neutral_annotation_record(&format!(
                "fcstd:native:object#Suffix{index}"
            )));
        }
        let property = carrier_property(
            "X",
            "App::PropertyDistance",
            "<Property><Float value=\"10\"/></Property>",
        );
        records[0].kind = crate::native::AnnotationRuntimeType::DrawViewAnnotation;
        let mut small = small;
        small[0].kind = crate::native::AnnotationRuntimeType::DrawViewAnnotation;
        let properties = [property];
        assert_eq!(
            work_before_annotation_malformed_suffix(&records, &properties),
            work_before_annotation_malformed_suffix(&small, &properties)
        );
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
