// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph recovery.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind, SemanticAnnotationTarget,
};

use crate::native::{DrawingRecord, ObjectRecord, PropertyRecord, SemanticAnnotationRecord};

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
            CodecError::malformed(format_args!(
                "semantic annotation {} has unsupported runtime type {}",
                record.id, record.kind
            ))
        })?;
        let owned = properties
            .iter()
            .filter(|property| property.owner == record.object)
            .collect::<Vec<_>>();
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
            format: schema.format.and_then(|name| string_property(&owned, name)),
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
    format: Option<&'static str>,
    position: PositionCarrier,
}

#[derive(Clone, Copy)]
enum PositionCarrier {
    Vector(&'static str),
    Coordinates(&'static str, &'static str),
}

fn annotation_schema(runtime_type: &str) -> Option<AnnotationSchema> {
    use SemanticAnnotationKind as Kind;
    let schema = match runtime_type {
        "App::Annotation" => AnnotationSchema {
            kind: Kind::Text,
            text: &["LabelText"],
            format: None,
            position: PositionCarrier::Vector("Position"),
        },
        "App::AnnotationLabel" => AnnotationSchema {
            kind: Kind::Text,
            text: &["LabelText"],
            format: None,
            position: PositionCarrier::Vector("TextPosition"),
        },
        "TechDraw::DrawViewAnnotation" | "TechDraw::DrawViewAnnotationPython" => AnnotationSchema {
            kind: Kind::Text,
            text: &["Text"],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawRichAnno" | "TechDraw::DrawRichAnnoPython" => AnnotationSchema {
            kind: Kind::Text,
            text: &["AnnoText"],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawViewDimension"
        | "TechDraw::DrawViewDimExtent"
        | "TechDraw::LandmarkDimension" => AnnotationSchema {
            kind: Kind::Dimension,
            text: &["FormatSpec"],
            format: Some("FormatSpec"),
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawViewBalloon" => AnnotationSchema {
            kind: Kind::Balloon,
            text: &["Text"],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawLeaderLine" | "TechDraw::DrawLeaderLinePython" => AnnotationSchema {
            kind: Kind::Leader,
            text: &[],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawViewSymbol"
        | "TechDraw::DrawViewSymbolPython"
        | "TechDraw::DrawWeldSymbol"
        | "TechDraw::DrawWeldSymbolPython" => AnnotationSchema {
            kind: Kind::Symbol,
            text: &["TailText"],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
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
        PositionCarrier::Vector(name) => optional_vector_property(properties, name),
        PositionCarrier::Coordinates(x_name, y_name) => {
            let x = optional_scalar_property(properties, x_name)?;
            let y = optional_scalar_property(properties, y_name)?;
            match (x, y) {
                (None, None) => Ok(None),
                (Some(x), Some(y)) => Ok(Some([x, y, 0.0])),
                _ => Err(CodecError::malformed(format_args!(
                    "annotation position requires both {x_name} and {y_name}"
                ))),
            }
        }
    }
}

fn optional_scalar_property(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<f64>, CodecError> {
    let Some(property) = properties.iter().find(|property| property.name == name) else {
        return Ok(None);
    };
    scalar_property(properties, name).map(Some).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "annotation property {} is not a scalar",
            property.id
        ))
    })
}

fn optional_vector_property(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<[f64; 3]>, CodecError> {
    let Some(property) = properties.iter().find(|property| property.name == name) else {
        return Ok(None);
    };
    vector_property(properties, name).map(Some).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "annotation property {} is not a vector",
            property.id
        ))
    })
}

fn scalar_property(properties: &[&PropertyRecord], name: &str) -> Option<f64> {
    property_attribute(properties, name, &["value", "Value"])?
        .parse()
        .ok()
}

fn string_property(properties: &[&PropertyRecord], name: &str) -> Option<String> {
    property_attribute(properties, name, &["value", "Value", "string", "String"]).map(str::to_owned)
}

fn property_attribute<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
    attributes: &[&str],
) -> Option<&'a str> {
    let value = properties
        .iter()
        .find(|property| property.name == name)?
        .values
        .first()?;
    attributes
        .iter()
        .find_map(|attribute| value.attributes.get(*attribute).map(String::as_str))
        .or(value.text.as_deref())
}

fn vector_property(properties: &[&PropertyRecord], name: &str) -> Option<[f64; 3]> {
    let attributes = &properties
        .iter()
        .find(|property| property.name == name)?
        .values
        .first()?
        .attributes;
    Some([
        attributes.get("valueX")?.parse().ok()?,
        attributes.get("valueY")?.parse().ok()?,
        attributes.get("valueZ")?.parse().ok()?,
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
 <Object name="Dimension"><Properties Count="5">
  <Property name="BaseView" type="App::PropertyLink"><Link value="View"/></Property>
  <Property name="References2D" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Model" sub="Edge1"/></LinkSubList></Property>
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
