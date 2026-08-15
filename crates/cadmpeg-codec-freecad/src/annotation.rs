// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph recovery.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind, SemanticAnnotationTarget,
};

use crate::native::{
    DrawingRecord, ObjectRecord, PropertyRecord, SemanticAnnotationRecord, ValueRecord,
};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Vec<SemanticAnnotationRecord> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    objects
        .iter()
        .filter_map(|object| annotation_schema(&object.type_name).map(|schema| (object, schema)))
        .map(|(object, schema)| {
            let mut owned = by_owner
                .get(object.id.as_str())
                .cloned()
                .unwrap_or_default();
            owned.sort_by_key(|property| (property.byte_start, property.byte_end));
            let references = owned
                .iter()
                .filter(|property| !property.links.is_empty())
                .map(|property| (property.name.clone(), property.links.clone()))
                .collect();
            let parameters = owned
                .iter()
                .filter(|property| property.links.is_empty())
                .map(|property| (property.name.clone(), property.raw_xml.clone()))
                .collect();
            SemanticAnnotationRecord {
                id: crate::native::native_id("annotation", &object.name),
                object: object.id.clone(),
                kind: object.type_name.clone(),
                text: owned
                    .iter()
                    .filter(|property| schema.text.contains(&property.name.as_str()))
                    .flat_map(|property| property.values.iter())
                    .filter_map(text_value)
                    .collect(),
                references,
                parameters,
                side_entries: owned
                    .iter()
                    .flat_map(|property| &property.side_entries)
                    .cloned()
                    .collect(),
            }
        })
        .collect()
}

pub(crate) fn transfer_neutral(
    model: &mut Model,
    records: &[SemanticAnnotationRecord],
    properties: &[PropertyRecord],
    drawings: &[DrawingRecord],
) -> Result<(), CodecError> {
    let drawing_ids = drawings
        .iter()
        .map(|drawing| {
            (
                drawing.object.as_str(),
                crate::native::model_id("drawing", &drawing.object, "entity"),
            )
        })
        .collect::<HashMap<_, _>>();
    for (order, record) in records.iter().enumerate() {
        let schema = annotation_schema(&record.kind).ok_or_else(|| {
            CodecError::Malformed(format!(
                "semantic annotation {} has unsupported runtime type {}",
                record.id, record.kind
            ))
        })?;
        let owned = properties
            .iter()
            .filter(|property| property.owner == record.object)
            .collect::<Vec<_>>();
        validate_text_carriers(&owned, &schema)?;
        let target = |link: &crate::native::LinkTarget| SemanticAnnotationTarget {
            target: link
                .document
                .is_none()
                .then(|| {
                    link.object
                        .as_ref()
                        .filter(|object| !object.is_empty())
                        .map(|object| {
                            drawing_ids
                                .get(object.as_str())
                                .cloned()
                                .unwrap_or_else(|| object.clone())
                        })
                })
                .flatten(),
            external_document: link.document.clone(),
            external_object: link.document.as_ref().and(link.object.clone()),
            is_null: link.document.is_none() && link.object.as_deref() == Some(""),
            subelements: link.subelements.clone(),
        };
        model.semantic_annotations.push(SemanticAnnotation {
            id: SemanticAnnotationId(crate::native::model_id(
                "semantic-annotation",
                &record.object,
                "content",
            )),
            object: record.object.clone(),
            kind: schema.kind.clone(),
            runtime_type: record.kind.clone(),
            order: order as u32,
            text: record.text.clone(),
            references: record
                .references
                .iter()
                .map(|(role, references)| (role.clone(), references.iter().map(target).collect()))
                .collect(),
            value: None,
            format: match schema.format {
                Some(name) => string_property(&owned, name, "App::PropertyString")?,
                None => None,
            },
            position: annotation_position(&owned, schema.position)?,
            parameters: record.parameters.clone(),
            assets: record
                .side_entries
                .iter()
                .map(|name| crate::native::native_id("entry", name))
                .collect(),
            native_ref: record.id.clone(),
        });
    }
    Ok(())
}

pub(crate) fn is_annotation_type(type_name: &str) -> bool {
    annotation_schema(type_name).is_some()
}

#[derive(Clone)]
struct AnnotationSchema {
    kind: SemanticAnnotationKind,
    text: &'static [&'static str],
    text_type: Option<&'static str>,
    format: Option<&'static str>,
    position: PositionCarrier,
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

fn annotation_schema(runtime_type: &str) -> Option<AnnotationSchema> {
    use SemanticAnnotationKind as Kind;
    let schema = match runtime_type {
        "App::Annotation" => AnnotationSchema {
            kind: Kind::Text,
            text: &["LabelText"],
            text_type: Some("App::PropertyStringList"),
            format: None,
            position: PositionCarrier::Vector {
                name: "Position",
                type_name: "App::PropertyVector",
            },
        },
        "App::AnnotationLabel" => AnnotationSchema {
            kind: Kind::Text,
            text: &["LabelText"],
            text_type: Some("App::PropertyStringList"),
            format: None,
            position: PositionCarrier::Vector {
                name: "TextPosition",
                type_name: "App::PropertyVector",
            },
        },
        "TechDraw::DrawViewAnnotation" | "TechDraw::DrawViewAnnotationPython" => AnnotationSchema {
            kind: Kind::Text,
            text: &["Text"],
            text_type: Some("App::PropertyStringList"),
            format: None,
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        "TechDraw::DrawRichAnno" | "TechDraw::DrawRichAnnoPython" => AnnotationSchema {
            kind: Kind::Text,
            text: &["AnnoText"],
            text_type: Some("App::PropertyString"),
            format: None,
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        "TechDraw::DrawViewDimension"
        | "TechDraw::DrawViewDimExtent"
        | "TechDraw::LandmarkDimension" => AnnotationSchema {
            kind: Kind::Dimension,
            text: &["FormatSpec"],
            text_type: Some("App::PropertyString"),
            format: Some("FormatSpec"),
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        "TechDraw::DrawViewBalloon" => AnnotationSchema {
            kind: Kind::Balloon,
            text: &["Text"],
            text_type: Some("App::PropertyString"),
            format: None,
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        "TechDraw::DrawLeaderLine" | "TechDraw::DrawLeaderLinePython" => AnnotationSchema {
            kind: Kind::Leader,
            text: &[],
            text_type: None,
            format: None,
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        "TechDraw::DrawViewSymbol"
        | "TechDraw::DrawViewSymbolPython"
        | "TechDraw::DrawWeldSymbol"
        | "TechDraw::DrawWeldSymbolPython" => AnnotationSchema {
            kind: Kind::Symbol,
            text: &["TailText"],
            text_type: Some("App::PropertyString"),
            format: None,
            position: PositionCarrier::Coordinates {
                x_name: "X",
                y_name: "Y",
                type_names: TECHDRAW_POSITION_TYPES,
            },
        },
        _ => return None,
    };
    Some(schema)
}

fn annotation_position(
    properties: &[&PropertyRecord],
    carrier: PositionCarrier,
) -> Result<Option<[f64; 3]>, CodecError> {
    match carrier {
        PositionCarrier::Vector { name, type_name } => {
            let position = optional_vector_property(properties, name, &[type_name])?;
            if position
                .is_some_and(|position| position.iter().any(|component| !component.is_finite()))
            {
                return Err(CodecError::Malformed(
                    "annotation position contains a non-finite coordinate".into(),
                ));
            }
            Ok(position)
        }
        PositionCarrier::Coordinates {
            x_name,
            y_name,
            type_names,
        } => {
            let x = optional_scalar_property(properties, x_name, type_names)?;
            let y = optional_scalar_property(properties, y_name, type_names)?;
            match (x, y) {
                (None, None) => Ok(None),
                (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok(Some([x, y, 0.0])),
                (Some(_), Some(_)) => Err(CodecError::Malformed(
                    "annotation position contains a non-finite coordinate".into(),
                )),
                _ => Err(CodecError::Malformed(format!(
                    "annotation position requires both {x_name} and {y_name}"
                ))),
            }
        }
    }
}

fn optional_scalar_property(
    properties: &[&PropertyRecord],
    name: &str,
    type_names: &[&str],
) -> Result<Option<f64>, CodecError> {
    let Some(property) = typed_property(properties, name, type_names)? else {
        return Ok(None);
    };
    let Some(value) = unique_value(property)? else {
        return Err(CodecError::Malformed(format!(
            "annotation property {} has no scalar value",
            property.id
        )));
    };
    scalar_value(value).map(Some).ok_or_else(|| {
        CodecError::Malformed(format!(
            "annotation property {} is not a scalar",
            property.id
        ))
    })
}

fn optional_vector_property(
    properties: &[&PropertyRecord],
    name: &str,
    type_names: &[&str],
) -> Result<Option<[f64; 3]>, CodecError> {
    let Some(property) = typed_property(properties, name, type_names)? else {
        return Ok(None);
    };
    let Some(value) = unique_value(property)? else {
        return Err(CodecError::Malformed(format!(
            "annotation property {} has no vector value",
            property.id
        )));
    };
    vector_value(value).map(Some).ok_or_else(|| {
        CodecError::Malformed(format!(
            "annotation property {} is not a vector",
            property.id
        ))
    })
}

fn string_property(
    properties: &[&PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<String>, CodecError> {
    let Some(property) = typed_property(properties, name, &[type_name])? else {
        return Ok(None);
    };
    let Some(value) = unique_value(property)? else {
        return Err(CodecError::Malformed(format!(
            "annotation property {} has no string value",
            property.id
        )));
    };
    Ok(property_attribute(value, &["value", "Value", "string", "String"]).map(str::to_owned))
}

fn typed_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
    type_names: &[&str],
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(None);
    };
    if !type_names.contains(&property.type_name.as_str()) {
        let expected = type_names.join(" or ");
        return Err(CodecError::Malformed(format!(
            "annotation property {name} has runtime type {}, expected {expected}",
            property.type_name
        )));
    }
    Ok(Some(property))
}

fn validate_text_carriers(
    properties: &[&PropertyRecord],
    schema: &AnnotationSchema,
) -> Result<(), CodecError> {
    let Some(type_name) = schema.text_type else {
        return Ok(());
    };
    for name in schema.text {
        let Some(property) = typed_property(properties, name, &[type_name])? else {
            continue;
        };
        if type_name == "App::PropertyString" && property.values.len() > 1 {
            return Err(CodecError::Malformed(format!(
                "annotation property {} has multiple string values",
                property.id
            )));
        }
    }
    Ok(())
}

fn unique_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let mut matches = properties
        .iter()
        .copied()
        .filter(|property| property.name == name);
    let Some(property) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "annotation property {name} occurs more than once"
        )));
    }
    Ok(Some(property))
}

fn unique_value(property: &PropertyRecord) -> Result<Option<&ValueRecord>, CodecError> {
    match property.values.as_slice() {
        [] => Ok(None),
        [value] => Ok(Some(value)),
        _ => Err(CodecError::Malformed(format!(
            "annotation property {} has multiple values",
            property.id
        ))),
    }
}

fn property_attribute<'a>(value: &'a ValueRecord, attributes: &[&str]) -> Option<&'a str> {
    attributes
        .iter()
        .find_map(|attribute| value.attributes.get(*attribute).map(String::as_str))
        .or(value.text.as_deref())
}

fn scalar_value(value: &ValueRecord) -> Option<f64> {
    property_attribute(value, &["value", "Value"])?.parse().ok()
}

fn vector_value(value: &ValueRecord) -> Option<[f64; 3]> {
    Some([
        value.attributes.get("valueX")?.parse().ok()?,
        value.attributes.get("valueY")?.parse().ok()?,
        value.attributes.get("valueZ")?.parse().ok()?,
    ])
}

fn text_value(value: &crate::native::ValueRecord) -> Option<String> {
    value
        .attributes
        .iter()
        .find(|(name, _)| matches!(name.as_str(), "value" | "Value" | "string" | "String"))
        .map(|(_, value)| value.clone())
        .or_else(|| value.text.clone())
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::is_annotation_type;
    use crate::test_support::*;
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
        assert_eq!(note.position, Some([1.0, 2.0, 3.0]));
        let label = annotations
            .iter()
            .find(|annotation| annotation.runtime_type == "App::AnnotationLabel")
            .expect("label annotation");
        assert_eq!(label.text, ["LABEL"]);
        assert_eq!(label.position, Some([4.0, 5.0, 6.0]));
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
            cadmpeg_core::CodecError::Malformed(message)
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
                .map(|annotation| annotation.position)
                .collect::<Vec<_>>(),
            [Some([10.0, 20.0, 0.0]), Some([30.0, 40.0, 0.0])]
        );
        assert!(crate::validate_native(result.ir()).is_empty());
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
                Err(cadmpeg_core::CodecError::Malformed(_))
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
                Err(cadmpeg_core::CodecError::Malformed(_))
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
            dimension.references["References2D"][0].subelements,
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
                .object
                .as_deref(),
            Some("fcstd:native:object#View")
        );
        assert_eq!(drawing_dimension.sources.len(), 2);
        assert_eq!(drawing_dimension.sources[1].subelements, ["Edge2"]);
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
        assert_eq!(semantic_dimension.position, Some([10.0, 20.0, 0.0]));
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
            semantic_note.references["View"][0].target.as_deref(),
            Some(neutral_view.id.0.as_str())
        );
        assert!(crate::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());

        let mut corrupted = result.ir().clone();
        corrupted.model.semantic_annotations[0].value = Some(f64::INFINITY);
        assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.message
                == "invalid semantic annotation reference, order, or numeric state"));
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
        assert!(crate::validate_native(result.ir()).is_empty());
        assert_valid_document(result.ir());
    }
}
