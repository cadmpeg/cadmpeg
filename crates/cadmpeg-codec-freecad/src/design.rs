// SPDX-License-Identifier: Apache-2.0
//! Transfer of `FCStd` construction history into neutral design entities.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BooleanOp, ChamferSpec, DesignParameter, EdgeSelection, Extent, Feature, FeatureDefinition,
    FeatureId, FeatureTreeNodeRole, Length, ParameterId, ParameterValue, ProfileRef, RadiusSpec,
    RevolutionAxis, RevolutionConstruction, SketchSpace,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus, SketchNativeOperand,
};

use crate::brep::ShapePayloadRecord;
use crate::native::{ObjectRecord, PropertyRecord, ValueRecord};

const MAX_SKETCH_RECORDS: usize = 1_000_000;

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
    let parent_by_member = objects
        .iter()
        .filter(|object| is_body(&object.type_name))
        .flat_map(|body| {
            properties_by_owner
                .get(body.id.as_str())
                .into_iter()
                .flatten()
                .filter(|property| property.name == "Group")
                .flat_map(|property| &property.links)
                .filter_map(|link| link.object.as_deref())
                .map(move |member| (member, feature_id(body)))
        })
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
        let definition = if is_body(&object.type_name) {
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::SolidBodies,
            }
        } else if is_sketch(&object.type_name) {
            let decoded = parse_sketch(object, &owned)?;
            let sketch = decoded.sketch;
            let sketch_id = sketch.id.clone();
            sketch_ids.insert(object.id.as_str(), sketch_id.clone());
            ir.model.sketches.push(sketch);
            ir.model.sketch_entities.extend(decoded.entities);
            ir.model.sketch_constraints.extend(decoded.constraints);
            ir.model.parameters.extend(decoded.parameters);
            FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch_id),
            }
        } else if is_extrusion(&object.type_name) {
            let profile = profile_ref(&owned, &sketch_ids);
            let length_property = property(&owned, "Length");
            let length = length_property.and_then(scalar_value).unwrap_or(0.0);
            if let Some(property) = length_property {
                let expression = expression_binding(&owned, "Length");
                ir.model.parameters.push(DesignParameter {
                    id: ParameterId(format!("fcstd:design:parameter#{}:Length", object.name)),
                    owner: id.clone(),
                    ordinal: 0,
                    name: "Length".into(),
                    expression: expression.as_ref().map_or_else(
                        || scalar_text(property).unwrap_or_else(|| length.to_string()),
                        |(_, expression)| expression.clone(),
                    ),
                    display: None,
                    value: Some(ParameterValue::Length(Length(length))),
                    dependencies: Vec::new(),
                    properties: expression
                        .map(|(native_ref, _)| {
                            [("expression_native_ref".into(), native_ref)].into()
                        })
                        .unwrap_or_default(),
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
        } else if is_revolution(&object.type_name) {
            FeatureDefinition::Revolve {
                construction: RevolutionConstruction {
                    profile: Some(profile_ref(&owned, &sketch_ids)),
                    axis: revolution_axis(&owned),
                    extent: revolution_extent(&owned),
                },
                op: if object.type_name.contains("Groove") {
                    BooleanOp::Cut
                } else {
                    BooleanOp::Join
                },
            }
        } else if object.type_name.contains("Fillet") {
            FeatureDefinition::Fillet {
                edges: native_edge_selection(&owned),
                radius: property(&owned, "Radius").and_then(scalar_value).map_or(
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::RadiusForm::Constant),
                    },
                    |radius| RadiusSpec::Constant {
                        radius: Length(radius),
                    },
                ),
            }
        } else if object.type_name.contains("Chamfer") {
            FeatureDefinition::Chamfer {
                edges: native_edge_selection(&owned),
                spec: chamfer_spec(&owned),
            }
        } else {
            FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            }
        };
        append_operation_parameters(&mut ir.model.parameters, object, &owned);
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
            suppressed: bool_property(&owned, "Suppressed").unwrap_or(false),
            parent: parent_by_member.get(object.id.as_str()).cloned(),
            dependencies,
            source_properties: feature_state(&owned),
            source_tag: Some(object.type_name.clone()),
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref: Some(object.id.clone()),
        });
    }
    bind_parameter_dependencies(&mut ir.model.parameters, objects);
    Ok(())
}

fn append_operation_parameters(
    parameters: &mut Vec<DesignParameter>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) {
    const NAMES: &[&str] = &[
        "Angle", "Angle2", "Radius", "Size", "Size2", "Length", "Length2",
    ];
    for property in properties
        .iter()
        .copied()
        .filter(|property| NAMES.contains(&property.name.as_str()))
    {
        if parameters.iter().any(|parameter| {
            parameter.owner == feature_id(object) && parameter.name == property.name
        }) {
            continue;
        }
        let Some(value) = scalar_value(property) else {
            continue;
        };
        let expression = expression_binding(properties, &property.name);
        let is_angle = property.type_name.contains("Angle");
        let mut retained = BTreeMap::new();
        if let Some((native_ref, _)) = &expression {
            retained.insert("expression_native_ref".into(), native_ref.clone());
        }
        parameters.push(DesignParameter {
            id: ParameterId(format!(
                "fcstd:design:parameter#{}:{}",
                object.name, property.name
            )),
            owner: feature_id(object),
            ordinal: property.order as u32,
            name: property.name.clone(),
            expression: expression.map_or_else(
                || scalar_text(property).unwrap_or_else(|| value.to_string()),
                |(_, expression)| expression,
            ),
            display: None,
            value: Some(if is_angle {
                ParameterValue::Angle(cadmpeg_ir::features::Angle(value.to_radians()))
            } else {
                ParameterValue::Length(Length(value))
            }),
            dependencies: Vec::new(),
            properties: retained,
            pmi: None,
            native_ref: Some(property.id.clone()),
        });
    }
}

struct SketchTransfer {
    sketch: Sketch,
    entities: Vec<SketchEntity>,
    constraints: Vec<SketchConstraint>,
    parameters: Vec<DesignParameter>,
}

fn parse_sketch(
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<SketchTransfer, CodecError> {
    let id = SketchId(format!("fcstd:design:sketch#{}", object.name));
    let mut entities = Vec::new();
    if let Some(geometry) = property(properties, "Geometry") {
        let xml = roxmltree::Document::parse(&geometry.raw_xml).map_err(|error| {
            CodecError::Malformed(format!("invalid sketch geometry {}: {error}", geometry.id))
        })?;
        validate_declared_count(&xml, "GeometryList", "Geometry", &geometry.id)?;
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
    let (constraints, parameters) = parse_constraints(object, properties, &id, &entities)?;
    let (origin, normal, u_axis) = sketch_frame(properties);
    Ok(SketchTransfer {
        sketch: Sketch {
            id,
            name: Some(object.name.clone()),
            configuration: None,
            origin,
            normal,
            u_axis,
            profiles,
            native_ref: Some(object.id.clone()),
        },
        entities,
        constraints,
        parameters,
    })
}

fn sketch_frame(properties: &[&PropertyRecord]) -> (Point3, Vector3, Vector3) {
    let Some(value) = property(properties, "Placement")
        .or_else(|| property(properties, "AttachmentOffset"))
        .and_then(|property| {
            property
                .values
                .iter()
                .find(|value| value.tag == "PropertyPlacement")
        })
    else {
        return (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
    };
    let component = |name: &str, default: f64| {
        value
            .attributes
            .get(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let quaternion = [
        component("Q0", 0.0),
        component("Q1", 0.0),
        component("Q2", 0.0),
        component("Q3", 1.0),
    ];
    (
        Point3::new(
            component("Px", 0.0),
            component("Py", 0.0),
            component("Pz", 0.0),
        ),
        rotate_vector(quaternion, [0.0, 0.0, 1.0]),
        rotate_vector(quaternion, [1.0, 0.0, 0.0]),
    )
}

fn rotate_vector(quaternion: [f64; 4], vector: [f64; 3]) -> Vector3 {
    let [x, y, z, w] = quaternion;
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    if norm <= f64::EPSILON {
        return Vector3::new(vector[0], vector[1], vector[2]);
    }
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);
    let [vx, vy, vz] = vector;
    Vector3::new(
        (1.0 - 2.0 * (y * y + z * z)) * vx
            + 2.0 * (x * y - z * w) * vy
            + 2.0 * (x * z + y * w) * vz,
        2.0 * (x * y + z * w) * vx
            + (1.0 - 2.0 * (x * x + z * z)) * vy
            + 2.0 * (y * z - x * w) * vz,
        2.0 * (x * z - y * w) * vx
            + 2.0 * (y * z + x * w) * vy
            + (1.0 - 2.0 * (x * x + y * y)) * vz,
    )
}

fn feature_state(properties: &[&PropertyRecord]) -> BTreeMap<String, String> {
    const STATE_NAMES: &[&str] = &[
        "Active",
        "Frozen",
        "Invalid",
        "MapMode",
        "Support",
        "Suppressed",
        "Tip",
        "Touched",
        "Visibility",
    ];
    properties
        .iter()
        .filter(|property| STATE_NAMES.contains(&property.name.as_str()))
        .map(|property| {
            let value = property
                .links
                .first()
                .and_then(|link| link.object.clone())
                .or_else(|| scalar_text(property))
                .unwrap_or_else(|| property.raw_xml.clone());
            (property.name.clone(), value)
        })
        .collect()
}

fn bool_property(properties: &[&PropertyRecord], name: &str) -> Option<bool> {
    let value = scalar_text(property(properties, name)?)?;
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

fn parse_constraints(
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Result<(Vec<SketchConstraint>, Vec<DesignParameter>), CodecError> {
    let Some(property) = property(properties, "Constraints") else {
        return Ok((Vec::new(), Vec::new()));
    };
    let xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::Malformed(format!(
            "invalid sketch constraints {}: {error}",
            property.id
        ))
    })?;
    validate_declared_count(&xml, "ConstraintList", "Constrain", &property.id)?;
    let mut constraints = Vec::new();
    let mut parameters = Vec::new();
    for (index, node) in xml
        .descendants()
        .filter(|node| node.has_tag_name("Constrain"))
        .enumerate()
    {
        let type_code = int_attr(node, "Type").unwrap_or(0);
        let operands = constraint_operands(node).map_err(|message| {
            CodecError::Malformed(format!(
                "{} constraint {}: {message}",
                property.id,
                index + 1
            ))
        })?;
        let resolved = operands
            .iter()
            .filter_map(|(entity, position)| resolve_operand(*entity, *position, entities))
            .collect::<Vec<_>>();
        let all_resolved = resolved.len() == operands.len();
        let parameter = if matches!(type_code, 6..=9 | 11 | 18 | 19) {
            node.attribute("Value")
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| {
                    let id = ParameterId(format!(
                        "fcstd:design:parameter#{}:constraint:{}",
                        object.name,
                        index + 1
                    ));
                    let angle = type_code == 9;
                    let path = format!("Constraints[{index}]");
                    let expression = expression_binding(properties, &path);
                    let mut parameter_properties = [(
                        "is_driving".into(),
                        node.attribute("IsDriving").unwrap_or("1").to_owned(),
                    )]
                    .into_iter()
                    .collect::<BTreeMap<_, _>>();
                    if let Some((native_ref, _)) = &expression {
                        parameter_properties
                            .insert("expression_native_ref".into(), native_ref.clone());
                    }
                    parameters.push(DesignParameter {
                        id: id.clone(),
                        owner: feature_id(object),
                        ordinal: index as u32,
                        name: node
                            .attribute("Name")
                            .filter(|name| !name.is_empty())
                            .map_or_else(|| format!("Constraint{}", index + 1), str::to_owned),
                        expression: expression.map_or_else(
                            || node.attribute("Value").unwrap_or_default().to_owned(),
                            |(_, expression)| expression,
                        ),
                        display: None,
                        value: Some(if angle {
                            ParameterValue::Angle(cadmpeg_ir::features::Angle(value))
                        } else {
                            ParameterValue::Length(Length(value))
                        }),
                        dependencies: Vec::new(),
                        properties: parameter_properties,
                        pmi: None,
                        native_ref: Some(property.id.clone()),
                    });
                    id
                })
        } else {
            None
        };
        let definition = neutral_constraint(type_code, &resolved, parameter.clone(), all_resolved)
            .unwrap_or_else(|| SketchConstraintDefinition::Native {
                native_kind: constraint_kind(type_code).into(),
                entities: resolved.iter().map(locus_entity).cloned().collect(),
                parameter,
                operands: operands
                    .iter()
                    .filter_map(|(entity, position)| {
                        if *entity < 0 || resolve_operand(*entity, *position, entities).is_none() {
                            Some(SketchNativeOperand {
                                native_kind: format!("position:{position}"),
                                object_index: u32::try_from(*entity).unwrap_or(u32::MAX),
                                native_ref: None,
                            })
                        } else {
                            None
                        }
                    })
                    .collect(),
            });
        constraints.push(SketchConstraint {
            id: SketchConstraintId(format!(
                "fcstd:design:sketch-constraint#{}:{}",
                object.name,
                index + 1
            )),
            sketch: sketch.clone(),
            definition,
            native_ref: Some(property.id.clone()),
        });
    }
    Ok((constraints, parameters))
}

fn expression_binding(properties: &[&PropertyRecord], path: &str) -> Option<(String, String)> {
    let engine = property(properties, "ExpressionEngine")?;
    engine
        .values
        .iter()
        .find(|value| {
            value.tag == "Expression"
                && value
                    .attributes
                    .get("path")
                    .is_some_and(|value| value == path)
        })
        .and_then(|value| {
            Some((
                engine.id.clone(),
                value.attributes.get("expression")?.clone(),
            ))
        })
}

fn bind_parameter_dependencies(parameters: &mut [DesignParameter], objects: &[ObjectRecord]) {
    let object_names = objects
        .iter()
        .map(|object| (feature_id(object), object.name.as_str()))
        .collect::<HashMap<_, _>>();
    let candidates = parameters
        .iter()
        .map(|parameter| {
            (
                parameter.id.clone(),
                parameter.owner.clone(),
                parameter.name.clone(),
            )
        })
        .collect::<Vec<_>>();
    for parameter in parameters {
        parameter.dependencies = candidates
            .iter()
            .filter(|(id, _, _)| *id != parameter.id)
            .filter(|(_, owner, name)| {
                let local =
                    owner == &parameter.owner && contains_identifier(&parameter.expression, name);
                let qualified = object_names.get(owner).is_some_and(|object| {
                    contains_identifier(&parameter.expression, &format!("{object}.{name}"))
                });
                local || qualified
            })
            .map(|(id, _, _)| id.clone())
            .collect();
    }
}

fn contains_identifier(expression: &str, identifier: &str) -> bool {
    expression.match_indices(identifier).any(|(start, _)| {
        let end = start + identifier.len();
        let boundary = |character: Option<char>| {
            character.is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
        };
        boundary(expression[..start].chars().next_back())
            && boundary(expression[end..].chars().next())
    })
}

fn neutral_constraint(
    kind: i64,
    loci: &[SketchLocus],
    parameter: Option<ParameterId>,
    complete: bool,
) -> Option<SketchConstraintDefinition> {
    if !complete {
        return None;
    }
    let entity = |index| loci.get(index).map(locus_entity).cloned();
    let pair = || Some((entity(0)?, entity(1)?));
    Some(match kind {
        1 => SketchConstraintDefinition::CoincidentLoci {
            loci: loci.to_vec(),
        },
        2 => SketchConstraintDefinition::Horizontal { entity: entity(0)? },
        3 => SketchConstraintDefinition::Vertical { entity: entity(0)? },
        4 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Parallel { first, second }
        }
        5 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Tangent { first, second }
        }
        10 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Perpendicular { first, second }
        }
        12 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Equal { first, second }
        }
        17 => SketchConstraintDefinition::Fixed { entity: entity(0)? },
        6 => SketchConstraintDefinition::Distance {
            entities: loci.iter().map(locus_entity).cloned().collect(),
            parameter: parameter?,
        },
        7 => SketchConstraintDefinition::HorizontalDistance {
            first: loci.first()?.clone(),
            second: loci.get(1)?.clone(),
            parameter: parameter?,
        },
        8 => SketchConstraintDefinition::VerticalDistance {
            first: loci.first()?.clone(),
            second: loci.get(1)?.clone(),
            parameter: parameter?,
        },
        9 => SketchConstraintDefinition::Angle {
            first: entity(0)?,
            second: entity(1)?,
            parameter: parameter?,
        },
        11 => SketchConstraintDefinition::Radius {
            entity: entity(0)?,
            parameter: parameter?,
        },
        18 => SketchConstraintDefinition::Diameter {
            entity: entity(0)?,
            parameter: parameter?,
        },
        _ => return None,
    })
}

fn constraint_operands(node: roxmltree::Node<'_, '_>) -> Result<Vec<(i64, i64)>, &'static str> {
    let ids = node
        .attribute("ElementIds")
        .map(split_ints)
        .unwrap_or_default();
    let positions = node
        .attribute("ElementPositions")
        .map(split_ints)
        .unwrap_or_default();
    if node.attribute("ElementIds").is_some() || node.attribute("ElementPositions").is_some() {
        if ids.len() != positions.len() {
            return Err("ElementIds and ElementPositions counts differ");
        }
        return Ok(ids.into_iter().zip(positions).collect());
    }
    Ok(["First", "Second", "Third"]
        .into_iter()
        .zip(["FirstPos", "SecondPos", "ThirdPos"])
        .filter_map(|(entity, position)| Some((int_attr(node, entity)?, int_attr(node, position)?)))
        .filter(|(entity, _)| *entity != -2000)
        .collect())
}

fn validate_declared_count(
    xml: &roxmltree::Document<'_>,
    container_tag: &str,
    record_tag: &str,
    owner: &str,
) -> Result<(), CodecError> {
    let Some(container) = xml
        .descendants()
        .find(|node| node.has_tag_name(container_tag))
    else {
        return Err(CodecError::Malformed(format!(
            "{owner} has no {container_tag} value"
        )));
    };
    let declared = container
        .attribute("count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::Malformed(format!("{owner} has an invalid record count")))?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(CodecError::Malformed(format!(
            "{owner} record count exceeds {MAX_SKETCH_RECORDS}"
        )));
    }
    let actual = container
        .children()
        .filter(|node| node.has_tag_name(record_tag))
        .count();
    if declared != actual {
        return Err(CodecError::Malformed(format!(
            "{owner} declares {declared} records but contains {actual}"
        )));
    }
    Ok(())
}

fn split_ints(value: &str) -> Vec<i64> {
    value
        .split(|character: char| character == ',' || character.is_ascii_whitespace())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn int_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<i64> {
    node.attribute(name)?.parse().ok()
}

fn resolve_operand(entity: i64, position: i64, entities: &[SketchEntity]) -> Option<SketchLocus> {
    let id = entities.get(usize::try_from(entity).ok()?)?.id.clone();
    Some(match position {
        0 => SketchLocus::Entity(id),
        1 => SketchLocus::Start(id),
        2 => SketchLocus::End(id),
        3 => SketchLocus::Center(id),
        _ => return None,
    })
}

fn locus_entity(locus: &SketchLocus) -> &SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

fn constraint_kind(kind: i64) -> &'static str {
    match kind {
        0 => "none",
        1 => "coincident",
        2 => "horizontal",
        3 => "vertical",
        4 => "parallel",
        5 => "tangent",
        6 => "distance",
        7 => "distance_x",
        8 => "distance_y",
        9 => "angle",
        10 => "perpendicular",
        11 => "radius",
        12 => "equal",
        13 => "point_on_object",
        14 => "symmetric",
        15 => "internal_alignment",
        16 => "snells_law",
        17 => "block",
        18 => "diameter",
        19 => "weight",
        20 => "group",
        21 => "text",
        _ => "unknown_future_constraint",
    }
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

fn revolution_axis(properties: &[&PropertyRecord]) -> Option<RevolutionAxis> {
    Some(RevolutionAxis {
        origin: vector_property(properties, "Base").map_or_else(
            || Point3::new(0.0, 0.0, 0.0),
            |vector| Point3::new(vector.x, vector.y, vector.z),
        ),
        direction: vector_property(properties, "Axis")?,
    })
}

fn revolution_extent(properties: &[&PropertyRecord]) -> Option<Extent> {
    let mode = property(properties, "Type")
        .and_then(scalar_value)
        .unwrap_or(0.0) as i64;
    let first = property(properties, "Angle").and_then(scalar_value)?;
    if mode == 4 {
        Some(Extent::TwoSidedAngles {
            first: cadmpeg_ir::features::Angle(first.to_radians()),
            second: cadmpeg_ir::features::Angle(
                property(properties, "Angle2")
                    .and_then(scalar_value)
                    .unwrap_or(0.0)
                    .to_radians(),
            ),
        })
    } else if mode == 0 {
        Some(Extent::Angle {
            angle: cadmpeg_ir::features::Angle(first.to_radians()),
        })
    } else {
        None
    }
}

fn vector_property(properties: &[&PropertyRecord], name: &str) -> Option<Vector3> {
    let value = property(properties, name)?
        .values
        .iter()
        .find(|value| value.attributes.contains_key("x") || value.attributes.contains_key("X"))?;
    let component = |lower: &str, upper: &str| {
        value
            .attributes
            .get(lower)
            .or_else(|| value.attributes.get(upper))?
            .parse::<f64>()
            .ok()
    };
    Some(Vector3::new(
        component("x", "X")?,
        component("y", "Y")?,
        component("z", "Z")?,
    ))
}

fn native_edge_selection(properties: &[&PropertyRecord]) -> EdgeSelection {
    property(properties, "Base").map_or(EdgeSelection::Unresolved, |property| {
        EdgeSelection::Native(property.id.clone())
    })
}

fn chamfer_spec(properties: &[&PropertyRecord]) -> ChamferSpec {
    let mode = property(properties, "ChamferType")
        .and_then(scalar_value)
        .unwrap_or(-1.0) as i64;
    let first = property(properties, "Size").and_then(scalar_value);
    match (mode, first) {
        (0, Some(distance)) => ChamferSpec::Distance {
            distance: Length(distance),
        },
        (1, Some(first)) => property(properties, "Size2").and_then(scalar_value).map_or(
            ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::ChamferForm::TwoDistances),
            },
            |second| ChamferSpec::TwoDistances {
                first: Length(first),
                second: Length(second),
            },
        ),
        (2, Some(distance)) => property(properties, "Angle").and_then(scalar_value).map_or(
            ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::ChamferForm::DistanceAngle),
            },
            |angle| ChamferSpec::DistanceAngle {
                distance: Length(distance),
                angle: cadmpeg_ir::features::Angle(angle.to_radians()),
            },
        ),
        _ => ChamferSpec::Unresolved { form: None },
    }
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
fn is_revolution(kind: &str) -> bool {
    kind.contains("PartDesign::Revolution")
        || kind.contains("PartDesign::Groove")
        || kind.contains("Part::Revolution")
}
fn is_dress_up(kind: &str) -> bool {
    kind.contains("Fillet") || kind.contains("Chamfer")
}
fn is_body(kind: &str) -> bool {
    kind.contains("PartDesign::Body")
}
fn is_design_object(kind: &str) -> bool {
    is_body(kind)
        || is_sketch(kind)
        || is_extrusion(kind)
        || is_revolution(kind)
        || is_dress_up(kind)
        || kind.contains("PartDesign::Feature")
}
