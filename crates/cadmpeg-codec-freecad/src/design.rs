// SPDX-License-Identifier: Apache-2.0
//! Transfer of `FCStd` construction history into neutral design entities.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BooleanOp, DesignParameter, Extent, Feature, FeatureDefinition, FeatureId, Length, ParameterId,
    ParameterValue, ProfileRef, SketchSpace,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
};

use crate::brep::ShapePayloadRecord;
use crate::native::{ObjectRecord, PropertyRecord, ValueRecord};

pub(crate) fn transfer(
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
) -> Result<(), CodecError> {
    let properties_by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    let feature_ids = objects
        .iter()
        .filter(|object| is_design_object(&object.type_name))
        .map(|object| (object.id.as_str(), feature_id(object)))
        .collect::<HashMap<_, _>>();
    let mut sketch_ids = HashMap::<&str, SketchId>::new();
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();

    for object in objects {
        if !is_design_object(&object.type_name) {
            continue;
        }
        let owned = properties_by_owner
            .get(object.id.as_str())
            .cloned()
            .unwrap_or_default();
        let id = feature_id(object);
        let definition = if is_sketch(&object.type_name) {
            let (sketch, entities) = parse_sketch(object, &owned)?;
            let sketch_id = sketch.id.clone();
            sketch_ids.insert(object.id.as_str(), sketch_id.clone());
            ir.model.sketches.push(sketch);
            ir.model.sketch_entities.extend(entities);
            FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch_id),
            }
        } else if is_extrusion(&object.type_name) {
            let profile = profile_ref(&owned, &sketch_ids);
            let length_property = property(&owned, "Length");
            let length = length_property.and_then(scalar_value).unwrap_or(0.0);
            if let Some(property) = length_property {
                ir.model.parameters.push(DesignParameter {
                    id: ParameterId(format!("fcstd:design:parameter#{}:Length", object.name)),
                    owner: id.clone(),
                    ordinal: 0,
                    name: "Length".into(),
                    expression: scalar_text(property).unwrap_or_else(|| length.to_string()),
                    display: None,
                    value: Some(ParameterValue::Length(Length(length))),
                    dependencies: Vec::new(),
                    properties: BTreeMap::new(),
                    pmi: None,
                    native_ref: Some(property.id.clone()),
                });
            }
            FeatureDefinition::Extrude {
                profile,
                direction: None,
                extent: Extent::Blind {
                    length: Length(length),
                },
                op: if object.type_name.contains("Pocket") {
                    BooleanOp::Cut
                } else if object.type_name.contains("PartDesign") {
                    BooleanOp::Join
                } else {
                    BooleanOp::NewBody
                },
                draft: None,
            }
        } else {
            FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            }
        };
        let outputs = payloads
            .iter()
            .filter(|payload| owned.iter().any(|property| property.id == payload.property))
            .flat_map(|payload| {
                body_ids
                    .iter()
                    .filter(move |body| body.0.starts_with(&format!("{}:body#", payload.id)))
                    .cloned()
            })
            .collect();
        let dependencies = object
            .dependencies
            .iter()
            .filter_map(|dependency| feature_ids.get(dependency.as_str()).cloned())
            .filter(|dependency| {
                objects.iter().any(|candidate| {
                    feature_id(candidate) == *dependency && candidate.order < object.order
                })
            })
            .collect();
        ir.model.features.push(Feature {
            id,
            ordinal: object.order as u64,
            name: Some(object.name.clone()),
            suppressed: false,
            parent: None,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: Some(object.type_name.clone()),
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref: Some(object.id.clone()),
        });
    }
    Ok(())
}

fn parse_sketch(
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<(Sketch, Vec<SketchEntity>), CodecError> {
    let id = SketchId(format!("fcstd:design:sketch#{}", object.name));
    let mut entities = Vec::new();
    if let Some(geometry) = property(properties, "Geometry") {
        let xml = roxmltree::Document::parse(&geometry.raw_xml).map_err(|error| {
            CodecError::Malformed(format!("invalid sketch geometry {}: {error}", geometry.id))
        })?;
        for (index, node) in xml
            .descendants()
            .filter(|node| node.has_tag_name("Geometry"))
            .enumerate()
        {
            let carrier = node
                .children()
                .find(|child| child.is_element() && !child.has_tag_name("Construction"));
            let native_kind = node
                .attribute("type")
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown")
                .to_owned();
            let attributes = carrier.map_or_else(BTreeMap::new, |child| {
                child
                    .attributes()
                    .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                    .collect()
            });
            let geometry_value = sketch_geometry(&native_kind, &attributes);
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "fcstd:design:sketch-entity#{}:{}",
                    object.name,
                    index + 1
                )),
                sketch: id.clone(),
                construction: node.descendants().any(|child| {
                    child.has_tag_name("Construction")
                        && child.attribute("value").is_some_and(|value| value != "0")
                }),
                native_ref: Some(geometry.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: geometry_value,
            });
        }
    }
    let profiles = build_profiles(&entities);
    Ok((
        Sketch {
            id,
            name: Some(object.name.clone()),
            configuration: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles,
            native_ref: Some(object.id.clone()),
        },
        entities,
    ))
}

fn sketch_geometry(kind: &str, attributes: &BTreeMap<String, String>) -> SketchGeometry {
    let number = |name: &str| attributes.get(name).and_then(|value| value.parse().ok());
    if kind.contains("Line") {
        SketchGeometry::Line {
            start: Point2::new(
                number("StartX").unwrap_or(0.0),
                number("StartY").unwrap_or(0.0),
            ),
            end: Point2::new(number("EndX").unwrap_or(0.0), number("EndY").unwrap_or(0.0)),
        }
    } else if kind.contains("Arc") {
        SketchGeometry::Arc {
            center: Point2::new(
                number("CenterX").unwrap_or(0.0),
                number("CenterY").unwrap_or(0.0),
            ),
            radius: Length(number("Radius").unwrap_or(0.0)),
            start_angle: cadmpeg_ir::features::Angle(number("FirstParameter").unwrap_or(0.0)),
            end_angle: cadmpeg_ir::features::Angle(number("LastParameter").unwrap_or(0.0)),
        }
    } else if kind.contains("Circle") {
        SketchGeometry::Circle {
            center: Point2::new(
                number("CenterX").unwrap_or(0.0),
                number("CenterY").unwrap_or(0.0),
            ),
            radius: Length(number("Radius").unwrap_or(0.0)),
        }
    } else {
        SketchGeometry::Native {
            native_kind: kind.to_owned(),
        }
    }
}

fn build_profiles(entities: &[SketchEntity]) -> Vec<Vec<SketchEntityUse>> {
    let mut unused = entities
        .iter()
        .filter(|entity| !entity.construction)
        .map(|entity| entity.id.clone())
        .collect::<BTreeSet<_>>();
    let by_id = entities
        .iter()
        .map(|entity| (entity.id.clone(), entity))
        .collect::<HashMap<_, _>>();
    let mut profiles = Vec::new();
    while let Some(first) = unused.iter().next().cloned() {
        unused.remove(&first);
        let mut chain = vec![SketchEntityUse {
            entity: first.clone(),
            reversed: false,
        }];
        let mut end = endpoints(by_id[&first]).map(|(_, end)| end);
        while let Some(point) = end {
            let next = unused.iter().find_map(|id| {
                let (start, finish) = endpoints(by_id[id])?;
                if near(point, start) {
                    Some((id.clone(), false, finish))
                } else if near(point, finish) {
                    Some((id.clone(), true, start))
                } else {
                    None
                }
            });
            let Some((id, reversed, next_end)) = next else {
                break;
            };
            unused.remove(&id);
            chain.push(SketchEntityUse {
                entity: id,
                reversed,
            });
            end = Some(next_end);
        }
        profiles.push(chain);
    }
    profiles
}

fn endpoints(entity: &SketchEntity) -> Option<(Point2, Point2)> {
    match entity.geometry {
        SketchGeometry::Line { start, end } => Some((start, end)),
        _ => None,
    }
}

fn near(a: Point2, b: Point2) -> bool {
    (a.u - b.u).abs() <= 1e-9 && (a.v - b.v).abs() <= 1e-9
}

fn profile_ref(properties: &[&PropertyRecord], sketches: &HashMap<&str, SketchId>) -> ProfileRef {
    property(properties, "Profile")
        .and_then(|property| {
            property
                .links
                .iter()
                .find_map(|link| link.object.as_deref())
        })
        .and_then(|target| sketches.get(target).cloned())
        .map_or_else(
            || ProfileRef::Native("unresolved FCStd profile".into()),
            ProfileRef::Sketch,
        )
}

fn property<'a>(properties: &'a [&PropertyRecord], name: &str) -> Option<&'a PropertyRecord> {
    properties
        .iter()
        .copied()
        .find(|property| property.name == name)
}

fn scalar_value(property: &PropertyRecord) -> Option<f64> {
    property
        .values
        .iter()
        .find_map(|value| value_attribute(value).and_then(|value| value.parse().ok()))
}

fn scalar_text(property: &PropertyRecord) -> Option<String> {
    property
        .values
        .iter()
        .find_map(value_attribute)
        .map(str::to_owned)
}

fn value_attribute(value: &ValueRecord) -> Option<&str> {
    value
        .attributes
        .get("value")
        .or_else(|| value.attributes.get("Value"))
        .map(String::as_str)
}

fn native_parameters(properties: &[&PropertyRecord]) -> BTreeMap<String, String> {
    properties
        .iter()
        .filter_map(|property| scalar_text(property).map(|value| (property.name.clone(), value)))
        .collect()
}

fn feature_id(object: &ObjectRecord) -> FeatureId {
    FeatureId(format!("fcstd:design:feature#{}", object.name))
}
fn is_sketch(kind: &str) -> bool {
    kind.contains("Sketcher::SketchObject")
}
fn is_extrusion(kind: &str) -> bool {
    kind.contains("PartDesign::Pad")
        || kind.contains("PartDesign::Pocket")
        || kind.contains("Part::Extrusion")
}
fn is_design_object(kind: &str) -> bool {
    is_sketch(kind) || is_extrusion(kind) || kind.contains("PartDesign::Feature")
}
