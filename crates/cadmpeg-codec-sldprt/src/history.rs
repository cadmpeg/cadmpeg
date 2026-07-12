// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

use crate::container::ContainerScan;
use crate::records::{Configuration, Feature, FeatureHistory};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::features::{
    Angle, BodySelection, BooleanOp, ChamferSpec, ConfigurationId, DesignConfiguration,
    DesignParameter, EdgeSelection, Extent, FaceMotion, FaceSelection, FeatureDefinition,
    FeatureId, HoleKind, Length, ParameterId, ParameterValue, PathRef, PatternKind, ProfileRef,
    RadiusSpec, VariableRadius,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

pub fn histories(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<FeatureHistory> {
    scan.blocks
        .iter()
        .filter_map(|block| {
            let text = xml_text(&block.payload)?;
            let doc = roxmltree::Document::parse(&text).ok()?;
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                return None;
            }
            let stream = block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset));
            let parent = format!("sldprt:history:feature-history#{}", block.offset);
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .map(|(ordinal, node)| {
                    let id = format!("sldprt:history:configuration#{}:{ordinal}", block.offset);
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        stream.clone(),
                        node.range().start as u64,
                        "Configuration",
                        Exactness::ByteExact,
                    );
                    Configuration {
                        id,
                        parent: parent.clone(),
                        name: node.attribute("Name").unwrap_or("").into(),
                        material: node
                            .attribute("Material")
                            .filter(|value| !value.is_empty())
                            .map(str::to_string),
                        properties: node
                            .attributes()
                            .filter(|attribute| !matches!(attribute.name(), "Name" | "Material"))
                            .map(|attribute| {
                                (attribute.name().to_string(), attribute.value().to_string())
                            })
                            .collect(),
                    }
                })
                .collect();
            let feature_nodes = root
                .descendants()
                .filter(|node| {
                    node.is_element()
                        && !matches!(
                            node.tag_name().name(),
                            "Keywords" | "Configuration" | "Dimension"
                        )
                })
                .collect::<Vec<_>>();
            let feature_ids = feature_nodes
                .iter()
                .enumerate()
                .map(|(ordinal, node)| {
                    (
                        node.range().start,
                        format!("sldprt:history:feature#{}:{ordinal}", block.offset),
                    )
                })
                .collect::<HashMap<_, _>>();
            let features = feature_nodes
                .into_iter()
                .enumerate()
                .map(|(ordinal, node)| {
                    let id = feature_ids[&node.range().start].clone();
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        stream.clone(),
                        node.range().start as u64,
                        node.tag_name().name(),
                        Exactness::ByteExact,
                    );
                    Feature {
                        id,
                        parent: parent.clone(),
                        xml_tag: node.tag_name().name().into(),
                        tree_parent: node
                            .ancestors()
                            .skip(1)
                            .find_map(|ancestor| feature_ids.get(&ancestor.range().start).cloned()),
                        source_id: node
                            .attribute("id")
                            .filter(|value| !value.is_empty())
                            .map(str::to_string),
                        parent_source_id: node
                            .ancestors()
                            .skip(1)
                            .find_map(|parent| parent.attribute("id").map(str::to_string)),
                        ordinal: ordinal as u32,
                        name: node.attribute("Name").unwrap_or("").into(),
                        kind: node
                            .attribute("Type")
                            .unwrap_or_else(|| node.tag_name().name())
                            .into(),
                        suppressed: node
                            .attribute("Suppressed")
                            .is_some_and(|value| matches!(value, "1" | "true" | "True")),
                        parameters: node
                            .children()
                            .filter(|child| {
                                child.is_element() && child.tag_name().name() == "Dimension"
                            })
                            .filter_map(|dimension| {
                                Some((
                                    dimension.attribute("Name")?.into(),
                                    dimension.text()?.trim().into(),
                                ))
                            })
                            .collect::<BTreeMap<_, _>>(),
                        properties: node
                            .attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "id" | "Name" | "Type" | "Suppressed")
                            })
                            .map(|attribute| {
                                (attribute.name().to_string(), attribute.value().to_string())
                            })
                            .collect(),
                    }
                })
                .collect();
            let id = parent;
            crate::annotations::note(
                annotations,
                id.clone(),
                stream,
                0,
                "Keywords",
                Exactness::ByteExact,
            );
            Some(FeatureHistory {
                id,
                part_name: root
                    .attribute("Name")
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                configurations,
                features,
            })
        })
        .collect()
}

/// Project native Keywords records into the neutral feature arena.
pub fn project_features(histories: &[FeatureHistory]) -> Vec<cadmpeg_ir::features::Feature> {
    histories
        .iter()
        .flat_map(|history| {
            let by_source = history
                .features
                .iter()
                .filter_map(|feature| {
                    feature
                        .source_id
                        .as_deref()
                        .map(|source| (source, neutral_feature_id(&feature.id)))
                })
                .collect::<HashMap<_, _>>();
            let by_native = history
                .features
                .iter()
                .map(|feature| (feature.id.as_str(), neutral_feature_id(&feature.id)))
                .collect::<HashMap<_, _>>();
            let native_by_source = history
                .features
                .iter()
                .filter_map(|feature| {
                    feature
                        .source_id
                        .as_deref()
                        .map(|source| (source, feature.id.as_str()))
                })
                .collect::<HashMap<_, _>>();
            history
                .features
                .iter()
                .map(move |feature| cadmpeg_ir::features::Feature {
                    id: neutral_feature_id(&feature.id),
                    ordinal: u64::from(feature.ordinal),
                    name: (!feature.name.is_empty()).then(|| feature.name.clone()),
                    suppressed: feature.suppressed,
                    parent: feature
                        .tree_parent
                        .as_deref()
                        .and_then(|parent| by_native.get(parent).cloned())
                        .or_else(|| {
                            feature
                                .parent_source_id
                                .as_deref()
                                .and_then(|source| by_source.get(source).cloned())
                        }),
                    outputs: Vec::new(),
                    definition: project_definition(feature, &by_source, &native_by_source),
                    native_ref: Some(feature.id.clone()),
                })
        })
        .collect()
}

/// Project native configuration records into the neutral configuration arena.
pub fn project_configurations(histories: &[FeatureHistory]) -> Vec<DesignConfiguration> {
    histories
        .iter()
        .flat_map(|history| &history.configurations)
        .map(|configuration| DesignConfiguration {
            id: ConfigurationId(format!(
                "sldprt:model:configuration#{}",
                configuration
                    .id
                    .strip_prefix("sldprt:history:configuration#")
                    .unwrap_or(&configuration.id)
            )),
            name: configuration.name.clone(),
            material: configuration.material.clone(),
            properties: configuration.properties.clone(),
            bodies: Vec::new(),
            native_ref: Some(configuration.id.clone()),
        })
        .collect()
}

/// Project every native feature dimension into the neutral parameter arena.
pub fn project_parameters(histories: &[FeatureHistory]) -> Vec<DesignParameter> {
    histories
        .iter()
        .flat_map(|history| &history.features)
        .flat_map(|feature| {
            feature
                .parameters
                .iter()
                .enumerate()
                .map(move |(ordinal, (name, expression))| {
                    let key = feature
                        .id
                        .strip_prefix("sldprt:history:feature#")
                        .unwrap_or(&feature.id);
                    DesignParameter {
                        id: ParameterId(format!("sldprt:model:parameter#{key}:{ordinal}")),
                        owner: neutral_feature_id(&feature.id),
                        name: name.clone(),
                        expression: expression.clone(),
                        value: parse_parameter_literal(expression),
                    }
                })
        })
        .collect()
}

/// Bind a uniquely identified native sketch history node to solved sketch geometry.
pub fn bind_unique_sketch_feature(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
) {
    let feature_indices = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut bindings = Vec::new();
    for index in &feature_indices {
        let Some(name) = features[*index].name.as_deref() else {
            continue;
        };
        if feature_indices
            .iter()
            .filter(|other| features[**other].name.as_deref() == Some(name))
            .count()
            != 1
        {
            continue;
        }
        let matches = sketches
            .iter()
            .filter(|sketch| sketch.name.as_deref() == Some(name))
            .collect::<Vec<_>>();
        let [sketch] = matches.as_slice() else {
            continue;
        };
        if let Some(native_ref) = features[*index].native_ref.clone() {
            bindings.push((*index, native_ref, sketch.id.clone()));
        }
    }
    if bindings.is_empty() {
        if let ([index], [sketch]) = (feature_indices.as_slice(), sketches) {
            if let Some(native_ref) = features[*index].native_ref.clone() {
                bindings.push((*index, native_ref, sketch.id.clone()));
            }
        }
    }
    for (index, _, sketch) in &bindings {
        features[*index].definition = FeatureDefinition::Sketch {
            sketch: Some(sketch.clone()),
        };
    }
    for feature in features {
        for (_, native_ref, sketch) in &bindings {
            bind_definition_sketch(&mut feature.definition, native_ref, sketch);
        }
    }
}

fn bind_definition_sketch(
    definition: &mut FeatureDefinition,
    native_ref: &str,
    sketch: &cadmpeg_ir::sketches::SketchId,
) {
    let bind_profile = |profile: &mut ProfileRef| {
        if matches!(profile, ProfileRef::Native(value) if value == native_ref) {
            *profile = ProfileRef::Sketch(sketch.clone());
        }
    };
    let bind_path = |path: &mut PathRef| {
        if matches!(path, PathRef::Native(value) if value == native_ref) {
            *path = PathRef::Sketch(sketch.clone());
        }
    };
    match definition {
        FeatureDefinition::Extrude { profile, .. }
        | FeatureDefinition::Revolve { profile, .. }
        | FeatureDefinition::Rib { profile, .. } => bind_profile(profile),
        FeatureDefinition::Sweep { profile, path, .. } => {
            bind_profile(profile);
            bind_path(path);
        }
        FeatureDefinition::Loft {
            profiles, guides, ..
        } => {
            profiles.iter_mut().for_each(bind_profile);
            guides.iter_mut().for_each(bind_path);
        }
        _ => {}
    }
}

fn project_definition(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
    native_by_source: &HashMap<&str, &str>,
) -> FeatureDefinition {
    if feature.kind.eq_ignore_ascii_case("Sketch") {
        return FeatureDefinition::Sketch { sketch: None };
    }
    if feature.kind.eq_ignore_ascii_case("ReferencePlane") {
        return project_datum_plane(feature).unwrap_or_else(|| native_definition(feature));
    }
    if feature.kind.eq_ignore_ascii_case("ReferenceAxis") {
        return project_datum_axis(feature).unwrap_or_else(|| native_definition(feature));
    }
    if feature.kind.eq_ignore_ascii_case("ReferencePoint") {
        return project_datum_point(feature).unwrap_or_else(|| native_definition(feature));
    }
    if extrude_op(&feature.kind).is_some() {
        project_extrude(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Fillet") {
        project_fillet(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Chamfer") {
        project_chamfer(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Shell") {
        project_shell(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Draft") {
        project_draft(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Combine") {
        project_combine(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("DeleteFace") {
        project_delete_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("MoveFace") {
        project_move_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Dome") {
        project_dome(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Hole") {
        project_hole(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Revolve") {
        project_revolve(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if pattern_form(feature).is_some() {
        project_pattern(feature, by_source).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Sweep") {
        project_sweep(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Loft") {
        project_loft(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if feature.kind.eq_ignore_ascii_case("Rib") {
        project_rib(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else {
        native_definition(feature)
    }
}

fn project_extrude(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let op = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
        .or_else(|| extrude_op(&feature.kind))?;
    let length = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_length_mm(value))
            .map(Length)
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("Blind") => Extent::Blind {
            length: length("Depth")?,
        },
        Some("Symmetric") => Extent::Symmetric {
            length: length("Depth")?,
        },
        Some("TwoSided") => Extent::TwoSided {
            first: length("Depth")?,
            second: length("Depth2")?,
        },
        Some("ThroughAll") => Extent::ThroughAll,
        Some("ToFace") => Extent::ToFace {
            face: FaceSelection::Native(feature.properties.get("Face")?.clone()),
        },
        Some(_) => return None,
    };
    let direction = match feature.properties.get("Direction") {
        Some(value) => Some(parse_vector3(value)?),
        None => None,
    };
    if direction.is_some_and(|value| !valid_direction(value)) {
        return None;
    }
    let draft = match feature.parameters.get("Draft") {
        Some(value) => Some(Angle(parse_angle_rad(value)?)),
        None => None,
    };
    let profile = feature.properties.get("Profile").map_or_else(
        || Some(feature.id.clone()),
        |source| {
            native_by_source
                .get(source.as_str())
                .map(|id| (*id).to_string())
        },
    )?;
    Some(FeatureDefinition::Extrude {
        profile: ProfileRef::Native(profile),
        direction,
        extent,
        op,
        draft,
    })
}

fn project_datum_plane(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let normal = parse_vector3(feature.properties.get("Normal")?)?;
    let u_axis = parse_vector3(feature.properties.get("UAxis")?)?;
    valid_plane_frame(normal, u_axis).then_some(FeatureDefinition::DatumPlane {
        origin,
        normal,
        u_axis,
    })
}

fn valid_plane_frame(normal: Vector3, u_axis: Vector3) -> bool {
    let normal_length = normal.norm();
    let u_length = u_axis.norm();
    normal_length.is_finite()
        && u_length.is_finite()
        && normal_length > f64::EPSILON
        && u_length > f64::EPSILON
        && (normal.x * u_axis.x + normal.y * u_axis.y + normal.z * u_axis.z).abs()
            <= 1.0e-9 * normal_length * u_length
}

fn project_datum_axis(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let direction = parse_vector3(feature.properties.get("Direction")?)?;
    valid_direction(direction).then_some(FeatureDefinition::DatumAxis { origin, direction })
}

fn project_datum_point(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::DatumPoint {
        position: parse_point3_mm(feature.properties.get("Position")?)?,
    })
}

fn valid_direction(direction: Vector3) -> bool {
    direction.norm().is_finite() && direction.norm() > f64::EPSILON
}

fn project_fillet(feature: &Feature) -> Option<FeatureDefinition> {
    let radius = if let Some(radius) = feature
        .parameters
        .get("Radius")
        .and_then(|value| parse_length_mm(value))
    {
        RadiusSpec::Constant {
            radius: Length(radius),
        }
    } else {
        let mut points = feature
            .parameters
            .iter()
            .filter_map(|(name, radius)| {
                let index = name.strip_prefix("Radius")?.parse::<usize>().ok()?;
                Some((index, radius))
            })
            .map(|(index, radius)| {
                let parameter = feature
                    .parameters
                    .get(&format!("Position{index}"))?
                    .trim()
                    .parse::<f64>()
                    .ok()?;
                let radius = parse_length_mm(radius)?;
                (parameter.is_finite() && (0.0..=1.0).contains(&parameter)).then_some((
                    index,
                    VariableRadius {
                        parameter,
                        radius: Length(radius),
                    },
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        points.sort_by_key(|(index, _)| *index);
        if points.len() < 2
            || points
                .iter()
                .enumerate()
                .any(|(expected, (actual, _))| expected != *actual)
        {
            return None;
        }
        RadiusSpec::Variable {
            points: points.into_iter().map(|(_, point)| point).collect(),
        }
    };
    Some(FeatureDefinition::Fillet {
        edges: EdgeSelection::Native(
            feature
                .properties
                .get("Edges")
                .cloned()
                .unwrap_or_else(|| feature.id.clone()),
        ),
        radius,
    })
}

fn project_rib(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let profile = *native_by_source.get(feature.properties.get("Profile")?.as_str())?;
    let draft = match feature.parameters.get("Draft") {
        Some(value) => Some(Angle(parse_angle_rad(value)?)),
        None => None,
    };
    Some(FeatureDefinition::Rib {
        profile: ProfileRef::Native(profile.to_string()),
        direction: parse_vector3(feature.properties.get("Direction")?)?,
        thickness: Length(
            feature
                .parameters
                .get("Thickness")
                .and_then(|value| parse_length_mm(value))?,
        ),
        both_sides: parse_bool(feature.properties.get("BothSides")?)?,
        draft,
        op: parse_boolean_op(feature.properties.get("Operation")?)?,
    })
}

fn project_loft(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let profiles = resolve_native_refs(feature.properties.get("Profiles")?, native_by_source)?
        .into_iter()
        .map(ProfileRef::Native)
        .collect::<Vec<_>>();
    if profiles.len() < 2 {
        return None;
    }
    let guides = feature.properties.get("Guides").map_or_else(
        || Some(Vec::new()),
        |value| resolve_native_refs(value, native_by_source),
    )?;
    Some(FeatureDefinition::Loft {
        profiles,
        guides: guides.into_iter().map(PathRef::Native).collect(),
        op: parse_boolean_op(feature.properties.get("Operation")?)?,
        closed: parse_bool(feature.properties.get("Closed")?)?,
    })
}

fn resolve_native_refs(value: &str, native_by_source: &HashMap<&str, &str>) -> Option<Vec<String>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(|source| native_by_source.get(source).map(|id| (*id).to_string()))
        .collect()
}

fn project_sweep(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let profile = *native_by_source.get(feature.properties.get("Profile")?.as_str())?;
    let path = *native_by_source.get(feature.properties.get("Path")?.as_str())?;
    let op = parse_boolean_op(feature.properties.get("Operation")?)?;
    let twist = match feature.parameters.get("Twist") {
        Some(value) => Some(Angle(parse_angle_rad(value)?)),
        None => None,
    };
    let scale = match feature.parameters.get("Scale") {
        Some(value) => Some(
            value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| *value > 0.0)?,
        ),
        None => None,
    };
    Some(FeatureDefinition::Sweep {
        profile: ProfileRef::Native(profile.to_string()),
        path: PathRef::Native(path.to_string()),
        op,
        twist,
        scale,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PatternForm {
    Linear,
    Circular,
    Mirror,
}

fn pattern_form(feature: &Feature) -> Option<PatternForm> {
    let form = if feature.kind.eq_ignore_ascii_case("Pattern") {
        feature.properties.get("PatternType")?.as_str()
    } else {
        feature.kind.as_str()
    };
    match form.to_ascii_lowercase().as_str() {
        "linear" | "linearpattern" => Some(PatternForm::Linear),
        "circular" | "circularpattern" => Some(PatternForm::Circular),
        "mirror" => Some(PatternForm::Mirror),
        _ => None,
    }
}

fn project_pattern(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    let seeds = feature
        .properties
        .get("Seeds")?
        .split(',')
        .map(str::trim)
        .map(|source| by_source.get(source).cloned())
        .collect::<Option<Vec<_>>>()?;
    if seeds.is_empty() {
        return None;
    }
    let pattern = match pattern_form(feature)? {
        PatternForm::Linear => PatternKind::Linear {
            direction: parse_vector3(feature.properties.get("Direction")?)?,
            spacing: Length(
                feature
                    .parameters
                    .get("Spacing")
                    .and_then(|value| parse_length_mm(value))?,
            ),
            count: parse_count(feature.parameters.get("Count")?)?,
        },
        PatternForm::Circular => PatternKind::Circular {
            axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
            axis_dir: parse_vector3(feature.properties.get("AxisDirection")?)?,
            angle: Angle(
                feature
                    .parameters
                    .get("Angle")
                    .and_then(|value| parse_angle_rad(value))?,
            ),
            count: parse_count(feature.parameters.get("Count")?)?,
        },
        PatternForm::Mirror => PatternKind::Mirror {
            plane_origin: parse_point3_mm(feature.properties.get("PlaneOrigin")?)?,
            plane_normal: parse_vector3(feature.properties.get("PlaneNormal")?)?,
        },
    };
    Some(FeatureDefinition::Pattern { seeds, pattern })
}

fn parse_count(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|count| *count > 0)
}

fn project_revolve(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let angle = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_angle_rad(value))
            .map(Angle)
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("OneSided") => Extent::Angle {
            angle: angle("Angle")?,
        },
        Some("Symmetric") => Extent::SymmetricAngle {
            angle: angle("Angle")?,
        },
        Some("TwoSided") => Extent::TwoSidedAngles {
            first: angle("Angle")?,
            second: angle("Angle2")?,
        },
        Some(_) => return None,
    };
    let axis_origin = parse_point3_mm(feature.properties.get("AxisOrigin")?)?;
    let axis_dir = parse_vector3(feature.properties.get("AxisDirection")?)?;
    if !(axis_dir.norm().is_finite() && axis_dir.norm() > 0.0) {
        return None;
    }
    let op = parse_boolean_op(feature.properties.get("Operation")?)?;
    let profile = feature.properties.get("Profile").map_or_else(
        || Some(feature.id.clone()),
        |source| {
            native_by_source
                .get(source.as_str())
                .map(|id| (*id).to_string())
        },
    )?;
    Some(FeatureDefinition::Revolve {
        profile: ProfileRef::Native(profile),
        axis_origin,
        axis_dir,
        angle: extent,
        op,
    })
}

fn project_hole(feature: &Feature) -> Option<FeatureDefinition> {
    let diameter = feature
        .parameters
        .get("Diameter")
        .and_then(|value| parse_length_mm(value))?;
    let has_counterbore = feature.parameters.contains_key("CounterboreDiameter")
        || feature.parameters.contains_key("CounterboreDepth");
    let has_countersink = feature.parameters.contains_key("CountersinkDiameter")
        || feature.parameters.contains_key("CountersinkAngle");
    if has_counterbore && has_countersink {
        return None;
    }
    let kind = if has_counterbore {
        HoleKind::Counterbore {
            diameter: Length(
                feature
                    .parameters
                    .get("CounterboreDiameter")
                    .and_then(|value| parse_length_mm(value))?,
            ),
            depth: Length(
                feature
                    .parameters
                    .get("CounterboreDepth")
                    .and_then(|value| parse_length_mm(value))?,
            ),
        }
    } else if has_countersink {
        HoleKind::Countersink {
            diameter: Length(
                feature
                    .parameters
                    .get("CountersinkDiameter")
                    .and_then(|value| parse_length_mm(value))?,
            ),
            angle: Angle(
                feature
                    .parameters
                    .get("CountersinkAngle")
                    .and_then(|value| parse_angle_rad(value))?,
            ),
        }
    } else {
        HoleKind::Simple
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("Blind") => Extent::Blind {
            length: Length(
                feature
                    .parameters
                    .get("Depth")
                    .and_then(|value| parse_length_mm(value))?,
            ),
        },
        Some("ThroughAll") => Extent::ThroughAll,
        Some(_) => return None,
    };
    Some(FeatureDefinition::Hole {
        face: feature
            .properties
            .get("Face")
            .cloned()
            .map(FaceSelection::Native)
            .or_else(|| Some(FaceSelection::Native(feature.id.clone()))),
        position: match feature.properties.get("Position") {
            Some(value) => Some(parse_point3_mm(value)?),
            None => None,
        },
        direction: match feature.properties.get("Direction") {
            Some(value) => {
                let direction = parse_vector3(value)?;
                if !valid_direction(direction) {
                    return None;
                }
                Some(direction)
            }
            None => None,
        },
        kind,
        diameter: Length(diameter),
        extent,
    })
}

fn project_shell(feature: &Feature) -> Option<FeatureDefinition> {
    let thickness = feature
        .parameters
        .get("Thickness")
        .and_then(|value| parse_length_mm(value))?;
    let outward = parse_bool(feature.properties.get("Outward")?)?;
    Some(FeatureDefinition::Shell {
        removed_faces: FaceSelection::Native(
            feature
                .properties
                .get("RemovedFaces")
                .cloned()
                .unwrap_or_else(|| feature.id.clone()),
        ),
        thickness: Length(thickness),
        outward,
    })
}

fn project_draft(feature: &Feature) -> Option<FeatureDefinition> {
    let pull_direction = parse_vector3(feature.properties.get("Direction")?)?;
    if !(pull_direction.norm().is_finite() && pull_direction.norm() > 0.0) {
        return None;
    }
    Some(FeatureDefinition::Draft {
        faces: FaceSelection::Native(
            feature
                .properties
                .get("Faces")
                .cloned()
                .unwrap_or_else(|| feature.id.clone()),
        ),
        neutral_plane: FaceSelection::Native(
            feature
                .properties
                .get("NeutralPlane")
                .cloned()
                .unwrap_or_else(|| feature.id.clone()),
        ),
        pull_direction,
        angle: Angle(
            feature
                .parameters
                .get("Angle")
                .and_then(|value| parse_angle_rad(value))?,
        ),
        outward: parse_bool(feature.properties.get("Outward")?)?,
    })
}

fn project_combine(feature: &Feature) -> Option<FeatureDefinition> {
    let op = parse_boolean_op(feature.properties.get("Operation")?)?;
    if op == BooleanOp::NewBody {
        return None;
    }
    Some(FeatureDefinition::Combine {
        target: BodySelection::Native(feature.properties.get("Target")?.clone()),
        tools: BodySelection::Native(feature.properties.get("Tools")?.clone()),
        op,
    })
}

fn project_delete_face(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::DeleteFace {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        heal: parse_bool(feature.properties.get("Heal")?)?,
    })
}

fn project_move_face(feature: &Feature) -> Option<FeatureDefinition> {
    let motion = match feature
        .properties
        .get("Mode")?
        .to_ascii_lowercase()
        .as_str()
    {
        "offset" => FaceMotion::Offset {
            distance: Length(
                feature
                    .parameters
                    .get("Distance")
                    .and_then(|value| parse_length_mm(value))?,
            ),
        },
        "translate" => FaceMotion::Translate {
            direction: parse_vector3(feature.properties.get("Direction")?)?,
            distance: Length(
                feature
                    .parameters
                    .get("Distance")
                    .and_then(|value| parse_length_mm(value))?,
            ),
        },
        "rotate" => FaceMotion::Rotate {
            axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
            axis_dir: parse_vector3(feature.properties.get("AxisDirection")?)?,
            angle: Angle(
                feature
                    .parameters
                    .get("Angle")
                    .and_then(|value| parse_angle_rad(value))?,
            ),
        },
        _ => return None,
    };
    Some(FeatureDefinition::MoveFace {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        motion,
    })
}

fn project_dome(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::Dome {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        height: Length(
            feature
                .parameters
                .get("Height")
                .and_then(|value| parse_length_mm(value))?,
        ),
        elliptical: parse_bool(feature.properties.get("Elliptical")?)?,
        reverse: parse_bool(feature.properties.get("Reverse")?)?,
    })
}

fn project_chamfer(feature: &Feature) -> Option<FeatureDefinition> {
    let length = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_length_mm(value))
            .map(Length)
    };
    let spec = if let (Some(distance), Some(angle)) = (
        length("Distance"),
        feature
            .parameters
            .get("Angle")
            .and_then(|value| parse_angle_rad(value))
            .map(Angle),
    ) {
        ChamferSpec::DistanceAngle { distance, angle }
    } else if let (Some(first), Some(second)) = (length("Distance1"), length("Distance2")) {
        ChamferSpec::TwoDistances { first, second }
    } else {
        ChamferSpec::Distance {
            distance: length("Distance")?,
        }
    };
    Some(FeatureDefinition::Chamfer {
        edges: EdgeSelection::Native(
            feature
                .properties
                .get("Edges")
                .cloned()
                .unwrap_or_else(|| feature.id.clone()),
        ),
        spec,
    })
}

fn native_definition(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Native {
        kind: feature.kind.clone(),
        parameters: feature.parameters.clone(),
        properties: feature.properties.clone(),
    }
}

fn parse_length_mm(value: &str) -> Option<f64> {
    let value = value.trim();
    for (suffix, scale) in [("mm", 1.0), ("cm", 10.0), ("in", 25.4), ("m", 1000.0)] {
        if let Some(number) = value.strip_suffix(suffix) {
            return number.trim().parse::<f64>().ok().map(|value| value * scale);
        }
    }
    None
}

fn format_length_mm(value: f64) -> String {
    format!("{value}mm")
}

fn parse_angle_rad(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix("deg") {
        return number.trim().parse::<f64>().ok().map(f64::to_radians);
    }
    value
        .strip_suffix("rad")
        .and_then(|number| number.trim().parse::<f64>().ok())
}

fn format_angle_rad(value: f64) -> String {
    format!("{value}rad")
}

fn parse_point3_mm(value: &str) -> Option<Point3> {
    let values = value
        .split(',')
        .map(|component| parse_length_mm(component.trim()))
        .collect::<Option<Vec<_>>>()?;
    (values.len() == 3).then(|| Point3::new(values[0], values[1], values[2]))
}

fn parse_vector3(value: &str) -> Option<Vector3> {
    let values = value
        .split(',')
        .map(|component| component.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (values.len() == 3).then(|| Vector3::new(values[0], values[1], values[2]))
}

fn format_point3_mm(value: Point3) -> String {
    format!("{}mm,{}mm,{}mm", value.x, value.y, value.z)
}

fn format_vector3(value: Vector3) -> String {
    format!("{},{},{}", value.x, value.y, value.z)
}

fn write_native_selection(
    properties: &mut BTreeMap<String, String>,
    key: &str,
    selection: &str,
    fallback: &str,
) {
    if selection != fallback || properties.contains_key(key) {
        properties.insert(key.into(), selection.into());
    } else {
        properties.remove(key);
    }
}

fn parse_boolean_op(value: &str) -> Option<BooleanOp> {
    match value.to_ascii_lowercase().as_str() {
        "join" => Some(BooleanOp::Join),
        "cut" => Some(BooleanOp::Cut),
        "intersect" => Some(BooleanOp::Intersect),
        "newbody" | "new_body" => Some(BooleanOp::NewBody),
        _ => None,
    }
}

fn format_boolean_op(value: BooleanOp) -> &'static str {
    match value {
        BooleanOp::Join => "Join",
        BooleanOp::Cut => "Cut",
        BooleanOp::Intersect => "Intersect",
        BooleanOp::NewBody => "NewBody",
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "True" => Some(true),
        "0" | "false" | "False" => Some(false),
        _ => None,
    }
}

fn parse_parameter_literal(expression: &str) -> Option<ParameterValue> {
    if let Some(value) = parse_bool(expression.trim()) {
        return Some(ParameterValue::Boolean(value));
    }
    if let Some(value) = parse_length_mm(expression) {
        return Some(ParameterValue::Length(Length(value)));
    }
    if let Some(value) = parse_angle_rad(expression) {
        return Some(ParameterValue::Angle(Angle(value)));
    }
    if let Ok(value) = expression.trim().parse::<i64>() {
        return Some(ParameterValue::Integer(value));
    }
    expression
        .trim()
        .parse::<f64>()
        .ok()
        .map(ParameterValue::Real)
}

fn neutral_feature_id(native_id: &str) -> FeatureId {
    let key = native_id
        .strip_prefix("sldprt:history:feature#")
        .unwrap_or(native_id);
    FeatureId(format!("sldprt:model:feature#{key}"))
}

/// Stable hash of the neutral feature projection.
pub fn feature_hash(features: &[cadmpeg_ir::features::Feature]) -> String {
    let mut features = features.to_vec();
    features.sort_by(|left, right| left.id.cmp(&right.id));
    hash_debug(&features)
}

/// Stable hash of the native feature histories.
pub fn history_hash(histories: &[FeatureHistory]) -> String {
    hash_debug(histories)
}

/// Stable hash of neutral configurations.
pub fn configuration_hash(configurations: &[DesignConfiguration]) -> String {
    let mut configurations = configurations.to_vec();
    configurations.sort_by(|left, right| left.id.cmp(&right.id));
    hash_debug(&configurations)
}

/// Stable hash of native configuration records.
pub fn native_configuration_hash(histories: &[FeatureHistory]) -> String {
    let mut configurations = histories
        .iter()
        .flat_map(|history| history.configurations.clone())
        .collect::<Vec<_>>();
    configurations.sort_by(|left, right| left.id.cmp(&right.id));
    hash_debug(&configurations)
}

/// Stable hash of neutral feature parameters.
pub fn parameter_hash(parameters: &[DesignParameter]) -> String {
    let mut parameters = parameters.to_vec();
    parameters.sort_by(|left, right| left.id.cmp(&right.id));
    hash_debug(&parameters)
}

/// Stable hash of native feature parameter maps.
pub fn native_parameter_hash(histories: &[FeatureHistory]) -> String {
    let mut parameters = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.clone(), feature.parameters.clone()))
        .collect::<Vec<_>>();
    parameters.sort_by(|left, right| left.0.cmp(&right.0));
    hash_debug(&parameters)
}

fn hash_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> String {
    let bytes = format!("{value:?}");
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes.as_bytes()) {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}

/// Resolve neutral/native feature edit authority and update the write history.
pub fn prepare_features_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let neutral_hash = feature_hash(&ir.model.features);
    let native_hash = native
        .as_ref()
        .map(|value| history_hash(&value.feature_histories));
    let baseline_neutral = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_neutral_feature_sha256"));
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_history_sha256"));
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &neutral_hash);
    let native_changed = match (&native_hash, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) => true,
        (None, Some(_)) => true,
        (None, None) => false,
    };
    if baseline_neutral.is_none() && baseline_native.is_none() {
        return sync_neutral_features(&ir.model.features, native);
    }
    match (neutral_changed, native_changed) {
        (false, _) => Ok(()),
        (true, true) => {
            let projected = native
                .as_ref()
                .map(|value| project_features(&value.feature_histories))
                .unwrap_or_default();
            if feature_hash(&projected) == neutral_hash {
                Ok(())
            } else {
                Err(CodecError::Malformed(
                    "conflicting neutral and native SLDPRT feature edits".into(),
                ))
            }
        }
        (true, false) => sync_neutral_features(&ir.model.features, native),
    }
}

/// Resolve neutral/native configuration edit authority before writing.
pub fn prepare_configurations_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let neutral_hash = configuration_hash(&ir.model.configurations);
    let native_hash = native
        .as_ref()
        .map(|value| native_configuration_hash(&value.feature_histories));
    let baseline_neutral = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_neutral_configuration_sha256"));
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_configuration_sha256"));
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &neutral_hash);
    let native_changed = match (&native_hash, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    };
    if baseline_neutral.is_none() && baseline_native.is_none() {
        sync_neutral_configurations(&ir.model.configurations, native);
        return Ok(());
    }
    match (neutral_changed, native_changed) {
        (false, _) => Ok(()),
        (true, true) => {
            let projected = native
                .as_ref()
                .map(|value| project_configurations(&value.feature_histories))
                .unwrap_or_default();
            if configuration_hash(&projected) == neutral_hash {
                Ok(())
            } else {
                Err(CodecError::Malformed(
                    "conflicting neutral and native SLDPRT configuration edits".into(),
                ))
            }
        }
        (true, false) => {
            sync_neutral_configurations(&ir.model.configurations, native);
            Ok(())
        }
    }
}

/// Resolve neutral/native parameter edit authority before writing.
pub fn prepare_parameters_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let neutral_hash = parameter_hash(&ir.model.parameters);
    let native_hash = native
        .as_ref()
        .map(|value| native_parameter_hash(&value.feature_histories));
    let baseline_neutral = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_neutral_parameter_sha256"));
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_parameter_sha256"));
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &neutral_hash);
    let native_changed = match (&native_hash, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    };
    if baseline_neutral.is_none() && baseline_native.is_none() {
        if ir.model.parameters.is_empty() {
            return Ok(());
        }
        return sync_neutral_parameters(ir, native);
    }
    match (neutral_changed, native_changed) {
        (false, _) => Ok(()),
        (true, true) => {
            let projected = native
                .as_ref()
                .map(|value| project_parameters(&value.feature_histories))
                .unwrap_or_default();
            if parameter_hash(&projected) == neutral_hash {
                Ok(())
            } else {
                Err(CodecError::Malformed(
                    "conflicting neutral and native SLDPRT parameter edits".into(),
                ))
            }
        }
        (true, false) => sync_neutral_parameters(ir, native),
    }
}

fn sync_neutral_parameters(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    let mut desired = HashMap::<FeatureId, BTreeMap<String, String>>::new();
    for parameter in &ir.model.parameters {
        if parameter.value != parse_parameter_literal(&parameter.expression) {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} has a value inconsistent with its expression",
                parameter.id.0
            )));
        }
        if !features.contains_key(&parameter.owner) {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} references a missing feature",
                parameter.id.0
            )));
        }
        if desired
            .entry(parameter.owner.clone())
            .or_default()
            .insert(parameter.name.clone(), parameter.expression.clone())
            .is_some()
        {
            return Err(CodecError::Malformed(format!(
                "duplicate SLDPRT parameter {} on feature {}",
                parameter.name, parameter.owner
            )));
        }
    }
    let Some(native) = native.as_mut() else {
        return Err(CodecError::NotImplemented(
            "SLDPRT parameters require feature records".into(),
        ));
    };
    for (feature_id, feature) in features {
        let record = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|record| {
                feature.native_ref.as_deref() == Some(record.id.as_str())
                    || record.source_id.as_deref() == Some(feature_id.0.as_str())
            })
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT parameters for feature {feature_id} require a retained feature record"
                ))
            })?;
        record.parameters = desired.remove(feature_id).unwrap_or_default();
    }
    Ok(())
}

fn sync_neutral_configurations(
    configurations: &[DesignConfiguration],
    native: &mut Option<crate::native::SldprtNative>,
) {
    if configurations.is_empty() {
        return;
    }
    if native.is_none() {
        *native = Some(crate::native::SldprtNative::default());
    }
    let native = native.as_mut().expect("initialized above");
    if native.feature_histories.is_empty() {
        native.feature_histories.push(FeatureHistory {
            id: "sldprt:generated:feature-history#0".into(),
            part_name: None,
            configurations: Vec::new(),
            features: Vec::new(),
        });
    }
    for configuration in configurations {
        let existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.configurations)
            .find(|candidate| configuration.native_ref.as_deref() == Some(candidate.id.as_str()));
        if let Some(existing) = existing {
            existing.name.clone_from(&configuration.name);
            existing.material.clone_from(&configuration.material);
            existing.properties.clone_from(&configuration.properties);
        } else {
            let parent = native.feature_histories[0].id.clone();
            native.feature_histories[0]
                .configurations
                .push(Configuration {
                    id: configuration.native_ref.clone().unwrap_or_else(|| {
                        format!("sldprt:generated:configuration#{}", configuration.id.0)
                    }),
                    parent,
                    name: configuration.name.clone(),
                    material: configuration.material.clone(),
                    properties: configuration.properties.clone(),
                });
        }
    }
}

/// Apply neutral native-feature edits to the `SolidWorks` history used for writing.
pub fn sync_neutral_features(
    features: &[cadmpeg_ir::features::Feature],
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    if features.is_empty() {
        return Ok(());
    }
    if native.is_none() {
        *native = Some(crate::native::SldprtNative {
            version: crate::native::SLDPRT_NATIVE_VERSION,
            feature_histories: vec![FeatureHistory {
                id: "sldprt:generated:feature-history#0".into(),
                part_name: None,
                configurations: Vec::new(),
                features: Vec::new(),
            }],
            feature_input_lanes: Vec::new(),
        });
    }
    let native = native.as_mut().expect("initialized above");
    if native.feature_histories.is_empty() {
        native.feature_histories.push(FeatureHistory {
            id: "sldprt:generated:feature-history#0".into(),
            part_name: None,
            configurations: Vec::new(),
            features: Vec::new(),
        });
    }

    let parent_sources = features
        .iter()
        .map(|feature| {
            let source_id = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()))
                .and_then(|candidate| candidate.source_id.clone())
                .unwrap_or_else(|| feature.id.0.clone());
            (feature.id.clone(), source_id)
        })
        .collect::<HashMap<_, _>>();
    let structural_parent_sources = features
        .iter()
        .map(|feature| {
            let source_id = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()))
                .and_then(|candidate| candidate.source_id.clone())
                .or_else(|| feature.native_ref.is_none().then(|| feature.id.0.clone()));
            (feature.id.clone(), source_id)
        })
        .collect::<HashMap<_, _>>();
    let record_ids = features
        .iter()
        .map(|feature| {
            let record_id = feature
                .native_ref
                .clone()
                .unwrap_or_else(|| format!("sldprt:generated:feature#{}", feature.id.0));
            (feature.id.clone(), record_id)
        })
        .collect::<HashMap<_, _>>();
    let record_sources = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| {
            feature
                .source_id
                .as_ref()
                .map(|source| (feature.id.clone(), source.clone()))
        })
        .collect::<HashMap<_, _>>();
    let sketch_sources = features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                sketch: Some(sketch),
            } => parent_sources
                .get(&feature.id)
                .map(|source| (sketch.clone(), source.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();

    for feature in features {
        let mut existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()));
        let (kind, parameters, properties) = match &feature.definition {
            FeatureDefinition::Native {
                kind,
                parameters,
                properties,
            } => (kind.clone(), parameters.clone(), properties.clone()),
            FeatureDefinition::DatumPlane {
                origin,
                normal,
                u_axis,
            } => {
                if !valid_plane_frame(*normal, *u_axis) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported reference-plane semantics",
                        feature.id
                    )));
                }
                if ![origin.x, origin.y, origin.z]
                    .iter()
                    .all(|value| value.is_finite())
                {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a non-finite reference-plane origin",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !record.kind.eq_ignore_ascii_case("ReferencePlane"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = existing
                    .as_deref()
                    .map(|record| record.properties.clone())
                    .unwrap_or_default();
                properties.insert("Origin".into(), format_point3_mm(*origin));
                properties.insert("Normal".into(), format_vector3(*normal));
                properties.insert("UAxis".into(), format_vector3(*u_axis));
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "ReferencePlane".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::DatumAxis { origin, direction } => {
                if !valid_direction(*direction) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported reference-axis semantics",
                        feature.id
                    )));
                }
                if ![origin.x, origin.y, origin.z]
                    .iter()
                    .all(|value| value.is_finite())
                {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a non-finite reference-axis origin",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !record.kind.eq_ignore_ascii_case("ReferenceAxis"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = existing
                    .as_deref()
                    .map(|record| record.properties.clone())
                    .unwrap_or_default();
                properties.insert("Origin".into(), format_point3_mm(*origin));
                properties.insert("Direction".into(), format_vector3(*direction));
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "ReferenceAxis".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::DatumPoint { position } => {
                if ![position.x, position.y, position.z]
                    .iter()
                    .all(|value| value.is_finite())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported reference-point semantics",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !record.kind.eq_ignore_ascii_case("ReferencePoint"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = existing
                    .as_deref()
                    .map(|record| record.properties.clone())
                    .unwrap_or_default();
                properties.insert("Position".into(), format_point3_mm(*position));
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "ReferencePoint".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::Sketch { .. } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !record.kind.eq_ignore_ascii_case("Sketch"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Sketch".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    existing
                        .as_deref()
                        .map(|record| record.properties.clone())
                        .unwrap_or_default(),
                )
            }
            FeatureDefinition::Extrude {
                profile,
                direction,
                extent,
                op,
                draft,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained extrusion record",
                        feature.id
                    )));
                };
                if extrude_op(&record.kind).is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported extrusion semantics",
                        feature.id
                    )));
                }
                let profile_source = profile_source(profile, &record_sources, &sketch_sources)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing extrusion profile",
                            feature.id
                        ))
                    })?;
                let mut parameters = record.parameters.clone();
                let mut properties = record.properties.clone();
                parameters.remove("Depth");
                parameters.remove("Depth2");
                parameters.remove("Draft");
                properties.remove("Direction");
                properties.remove("Face");
                match extent {
                    Extent::Blind { length } => {
                        properties.insert("EndCondition".into(), "Blind".into());
                        parameters.insert("Depth".into(), format_length_mm(length.0));
                    }
                    Extent::Symmetric { length } => {
                        properties.insert("EndCondition".into(), "Symmetric".into());
                        parameters.insert("Depth".into(), format_length_mm(length.0));
                    }
                    Extent::TwoSided { first, second } => {
                        properties.insert("EndCondition".into(), "TwoSided".into());
                        parameters.insert("Depth".into(), format_length_mm(first.0));
                        parameters.insert("Depth2".into(), format_length_mm(second.0));
                    }
                    Extent::ThroughAll => {
                        properties.insert("EndCondition".into(), "ThroughAll".into());
                    }
                    Extent::ToFace {
                        face: FaceSelection::Native(selection),
                    } if !selection.is_empty() => {
                        properties.insert("EndCondition".into(), "ToFace".into());
                        properties.insert("Face".into(), selection.clone());
                    }
                    Extent::ToFace { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion face selection",
                            feature.id
                        )));
                    }
                    Extent::Angle { .. }
                    | Extent::SymmetricAngle { .. }
                    | Extent::TwoSidedAngles { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion extent",
                            feature.id
                        )));
                    }
                }
                if let Some(direction) = direction {
                    require_direction(*direction, &feature.id, "extrusion direction")?;
                    properties.insert("Direction".into(), format_vector3(*direction));
                }
                if let Some(draft) = draft {
                    if !draft.0.is_finite() {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has a non-finite extrusion draft",
                            feature.id
                        )));
                    }
                    parameters.insert("Draft".into(), format_angle_rad(draft.0));
                }
                properties.insert("Operation".into(), format_boolean_op(*op).into());
                properties.insert("Profile".into(), profile_source);
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Fillet {
                edges: EdgeSelection::Native(selection),
                radius,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained fillet record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Fillet") || selection.trim().is_empty() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported fillet semantics",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                parameters.retain(|name, _| {
                    name != "Radius"
                        && !indexed_name(name, "Radius")
                        && !indexed_name(name, "Position")
                });
                match radius {
                    RadiusSpec::Constant {
                        radius: Length(radius),
                    } => {
                        parameters.insert("Radius".into(), format_length_mm(*radius));
                    }
                    RadiusSpec::Variable { points } => {
                        if points.len() < 2
                            || points.iter().any(|point| {
                                !point.parameter.is_finite()
                                    || !(0.0..=1.0).contains(&point.parameter)
                            })
                            || points
                                .windows(2)
                                .any(|pair| pair[0].parameter >= pair[1].parameter)
                        {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has an invalid variable-radius law",
                                feature.id
                            )));
                        }
                        for (index, point) in points.iter().enumerate() {
                            parameters
                                .insert(format!("Position{index}"), point.parameter.to_string());
                            parameters
                                .insert(format!("Radius{index}"), format_length_mm(point.radius.0));
                        }
                    }
                }
                let mut properties = record.properties.clone();
                write_native_selection(&mut properties, "Edges", selection, &record.id);
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Chamfer {
                edges: EdgeSelection::Native(selection),
                spec,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained chamfer record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Chamfer") || selection.trim().is_empty() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported chamfer semantics",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                match spec {
                    ChamferSpec::Distance { distance } => {
                        if parameters.contains_key("Distance1")
                            || parameters.contains_key("Distance2")
                            || parameters.contains_key("Angle")
                        {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} changes chamfer form",
                                feature.id
                            )));
                        }
                        parameters.insert("Distance".into(), format_length_mm(distance.0));
                    }
                    ChamferSpec::TwoDistances { first, second } => {
                        if !parameters.contains_key("Distance1")
                            || !parameters.contains_key("Distance2")
                            || parameters.contains_key("Angle")
                        {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} changes chamfer form",
                                feature.id
                            )));
                        }
                        parameters.insert("Distance1".into(), format_length_mm(first.0));
                        parameters.insert("Distance2".into(), format_length_mm(second.0));
                    }
                    ChamferSpec::DistanceAngle { distance, angle } => {
                        if !parameters.contains_key("Distance")
                            || !parameters.contains_key("Angle")
                            || parameters.contains_key("Distance1")
                            || parameters.contains_key("Distance2")
                        {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} changes chamfer form",
                                feature.id
                            )));
                        }
                        parameters.insert("Distance".into(), format_length_mm(distance.0));
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                }
                let mut properties = record.properties.clone();
                write_native_selection(&mut properties, "Edges", selection, &record.id);
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Shell {
                removed_faces: FaceSelection::Native(selection),
                thickness,
                outward,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained shell record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Shell") || selection.trim().is_empty() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported shell semantics",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                parameters.insert("Thickness".into(), format_length_mm(thickness.0));
                let mut properties = record.properties.clone();
                write_native_selection(&mut properties, "RemovedFaces", selection, &record.id);
                properties.insert("Outward".into(), outward.to_string());
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Draft {
                faces: FaceSelection::Native(faces),
                neutral_plane: FaceSelection::Native(neutral_plane),
                pull_direction,
                angle,
                outward,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained draft record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Draft")
                    || faces.trim().is_empty()
                    || neutral_plane.trim().is_empty()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported draft semantics",
                        feature.id
                    )));
                }
                require_direction(*pull_direction, &feature.id, "draft direction")?;
                if !angle.0.is_finite() {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a non-finite draft angle",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                parameters.insert("Angle".into(), format_angle_rad(angle.0));
                let mut properties = record.properties.clone();
                write_native_selection(&mut properties, "Faces", faces, &record.id);
                write_native_selection(&mut properties, "NeutralPlane", neutral_plane, &record.id);
                properties.insert("Direction".into(), format_vector3(*pull_direction));
                properties.insert("Outward".into(), outward.to_string());
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Combine {
                target: BodySelection::Native(target),
                tools: BodySelection::Native(tools),
                op,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained combine record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Combine") || *op == BooleanOp::NewBody {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported combine semantics",
                        feature.id
                    )));
                }
                if target.trim().is_empty() || tools.trim().is_empty() {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an empty combine selection",
                        feature.id
                    )));
                }
                let mut properties = record.properties.clone();
                properties.insert("Target".into(), target.clone());
                properties.insert("Tools".into(), tools.clone());
                properties.insert("Operation".into(), format_boolean_op(*op).into());
                (record.kind.clone(), record.parameters.clone(), properties)
            }
            FeatureDefinition::DeleteFace {
                faces: FaceSelection::Native(faces),
                heal,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained delete-face record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("DeleteFace") || faces.trim().is_empty() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported delete-face semantics",
                        feature.id
                    )));
                }
                let mut properties = record.properties.clone();
                properties.insert("Faces".into(), faces.clone());
                properties.insert("Heal".into(), heal.to_string());
                (record.kind.clone(), record.parameters.clone(), properties)
            }
            FeatureDefinition::MoveFace {
                faces: FaceSelection::Native(faces),
                motion,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained move-face record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("MoveFace") || faces.trim().is_empty() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported move-face semantics",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                let mut properties = record.properties.clone();
                properties.insert("Faces".into(), faces.clone());
                parameters.remove("Distance");
                parameters.remove("Angle");
                properties.remove("Direction");
                properties.remove("AxisOrigin");
                properties.remove("AxisDirection");
                match motion {
                    FaceMotion::Offset { distance } => {
                        properties.insert("Mode".into(), "Offset".into());
                        parameters.insert("Distance".into(), format_length_mm(distance.0));
                    }
                    FaceMotion::Translate {
                        direction,
                        distance,
                    } => {
                        require_direction(*direction, &feature.id, "face translation")?;
                        properties.insert("Mode".into(), "Translate".into());
                        properties.insert("Direction".into(), format_vector3(*direction));
                        parameters.insert("Distance".into(), format_length_mm(distance.0));
                    }
                    FaceMotion::Rotate {
                        axis_origin,
                        axis_dir,
                        angle,
                    } => {
                        require_direction(*axis_dir, &feature.id, "face rotation axis")?;
                        if !angle.0.is_finite() {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has a non-finite face rotation angle",
                                feature.id
                            )));
                        }
                        properties.insert("Mode".into(), "Rotate".into());
                        properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
                        properties.insert("AxisDirection".into(), format_vector3(*axis_dir));
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                }
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Dome {
                faces: FaceSelection::Native(faces),
                height,
                elliptical,
                reverse,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained dome record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Dome") || faces.trim().is_empty() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported dome semantics",
                        feature.id
                    )));
                }
                if !height.0.is_finite() {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a non-finite dome height",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                parameters.insert("Height".into(), format_length_mm(height.0));
                let mut properties = record.properties.clone();
                properties.insert("Faces".into(), faces.clone());
                properties.insert("Elliptical".into(), elliptical.to_string());
                properties.insert("Reverse".into(), reverse.to_string());
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Hole {
                face,
                position,
                direction,
                kind,
                diameter,
                extent,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained hole record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Hole") {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported hole semantics",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                parameters.insert("Diameter".into(), format_length_mm(diameter.0));
                parameters.remove("CounterboreDiameter");
                parameters.remove("CounterboreDepth");
                parameters.remove("CountersinkDiameter");
                parameters.remove("CountersinkAngle");
                match kind {
                    HoleKind::Simple => {}
                    HoleKind::Counterbore { diameter, depth } => {
                        parameters
                            .insert("CounterboreDiameter".into(), format_length_mm(diameter.0));
                        parameters.insert("CounterboreDepth".into(), format_length_mm(depth.0));
                    }
                    HoleKind::Countersink { diameter, angle } => {
                        parameters
                            .insert("CountersinkDiameter".into(), format_length_mm(diameter.0));
                        parameters.insert("CountersinkAngle".into(), format_angle_rad(angle.0));
                    }
                }
                let mut properties = record.properties.clone();
                match face {
                    Some(FaceSelection::Native(selection)) if !selection.is_empty() => {
                        properties.insert("Face".into(), selection.clone());
                    }
                    Some(FaceSelection::Native(_)) => {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has an empty hole face selection",
                            feature.id
                        )));
                    }
                    Some(FaceSelection::Faces(_)) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses a resolved hole face selection",
                            feature.id
                        )));
                    }
                    None => {
                        properties.remove("Face");
                    }
                }
                match position {
                    Some(position)
                        if position.x.is_finite()
                            && position.y.is_finite()
                            && position.z.is_finite() =>
                    {
                        properties.insert("Position".into(), format_point3_mm(*position));
                    }
                    Some(_) => {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has a non-finite hole position",
                            feature.id
                        )));
                    }
                    None => {
                        properties.remove("Position");
                    }
                }
                match direction {
                    Some(direction) => {
                        require_direction(*direction, &feature.id, "hole direction")?;
                        properties.insert("Direction".into(), format_vector3(*direction));
                    }
                    None => {
                        properties.remove("Direction");
                    }
                }
                match extent {
                    Extent::Blind {
                        length: Length(depth),
                    } => {
                        parameters.insert("Depth".into(), format_length_mm(*depth));
                        properties.insert("EndCondition".into(), "Blind".into());
                    }
                    Extent::ThroughAll => {
                        parameters.remove("Depth");
                        properties.insert("EndCondition".into(), "ThroughAll".into());
                    }
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} changes unsupported hole termination",
                            feature.id
                        )))
                    }
                }
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle,
                op,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained revolution record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Revolve") {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported revolution semantics",
                        feature.id
                    )));
                }
                if !valid_direction(*axis_dir) {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a degenerate revolution axis",
                        feature.id
                    )));
                }
                let profile_source = profile_source(profile, &record_sources, &sketch_sources)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing revolution profile",
                            feature.id
                        ))
                    })?;
                let mut parameters = record.parameters.clone();
                let mut properties = record.properties.clone();
                parameters.remove("Angle");
                parameters.remove("Angle2");
                match angle {
                    Extent::Angle { angle } => {
                        properties.insert("EndCondition".into(), "OneSided".into());
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                    Extent::SymmetricAngle { angle } => {
                        properties.insert("EndCondition".into(), "Symmetric".into());
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                    Extent::TwoSidedAngles { first, second } => {
                        properties.insert("EndCondition".into(), "TwoSided".into());
                        parameters.insert("Angle".into(), format_angle_rad(first.0));
                        parameters.insert("Angle2".into(), format_angle_rad(second.0));
                    }
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses a linear revolution extent",
                            feature.id
                        )));
                    }
                }
                properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
                properties.insert("AxisDirection".into(), format_vector3(*axis_dir));
                properties.insert("Operation".into(), format_boolean_op(*op).into());
                properties.insert("Profile".into(), profile_source);
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Sweep {
                profile,
                path,
                op,
                twist,
                scale,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained sweep record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Sweep") {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let profile_source = profile_source(profile, &record_sources, &sketch_sources)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing sweep profile",
                            feature.id
                        ))
                    })?;
                let path_source =
                    path_source(path, &record_sources, &sketch_sources).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing sweep path",
                            feature.id
                        ))
                    })?;
                let mut parameters = record.parameters.clone();
                match twist {
                    Some(twist) => {
                        parameters.insert("Twist".into(), format_angle_rad(twist.0));
                    }
                    None => {
                        parameters.remove("Twist");
                    }
                }
                match scale {
                    Some(scale) if scale.is_finite() && *scale > 0.0 => {
                        parameters.insert("Scale".into(), scale.to_string());
                    }
                    Some(_) => {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has an invalid sweep scale",
                            feature.id
                        )))
                    }
                    None => {
                        parameters.remove("Scale");
                    }
                }
                let mut properties = record.properties.clone();
                properties.insert("Profile".into(), profile_source.clone());
                properties.insert("Path".into(), path_source.clone());
                properties.insert("Operation".into(), format_boolean_op(*op).into());
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Loft {
                profiles,
                guides,
                op,
                closed,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained loft record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Loft") || profiles.len() < 2 {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported loft semantics",
                        feature.id
                    )));
                }
                let profile_sources = profiles
                    .iter()
                    .map(|profile| profile_source(profile, &record_sources, &sketch_sources))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing loft profile",
                            feature.id
                        ))
                    })?;
                let guide_sources = guides
                    .iter()
                    .map(|path| path_source(path, &record_sources, &sketch_sources))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing loft guide",
                            feature.id
                        ))
                    })?;
                let mut properties = record.properties.clone();
                properties.insert("Profiles".into(), profile_sources.join(","));
                if guide_sources.is_empty() {
                    properties.remove("Guides");
                } else {
                    properties.insert("Guides".into(), guide_sources.join(","));
                }
                properties.insert("Operation".into(), format_boolean_op(*op).into());
                properties.insert("Closed".into(), closed.to_string());
                (record.kind.clone(), record.parameters.clone(), properties)
            }
            FeatureDefinition::Rib {
                profile,
                direction,
                thickness,
                both_sides,
                draft,
                op,
            } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained rib record",
                        feature.id
                    )));
                };
                if !record.kind.eq_ignore_ascii_case("Rib") {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                require_direction(*direction, &feature.id, "rib direction")?;
                let profile_source = profile_source(profile, &record_sources, &sketch_sources)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing rib profile",
                            feature.id
                        ))
                    })?;
                let mut parameters = record.parameters.clone();
                parameters.insert("Thickness".into(), format_length_mm(thickness.0));
                match draft {
                    Some(draft) => {
                        parameters.insert("Draft".into(), format_angle_rad(draft.0));
                    }
                    None => {
                        parameters.remove("Draft");
                    }
                }
                let mut properties = record.properties.clone();
                properties.insert("Profile".into(), profile_source.clone());
                properties.insert("Direction".into(), format_vector3(*direction));
                properties.insert("BothSides".into(), both_sides.to_string());
                properties.insert("Operation".into(), format_boolean_op(*op).into());
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Pattern { seeds, pattern } => {
                let Some(record) = existing.as_deref() else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained pattern record",
                        feature.id
                    )));
                };
                let expected_form = match pattern {
                    PatternKind::Linear { .. } => PatternForm::Linear,
                    PatternKind::Circular { .. } => PatternForm::Circular,
                    PatternKind::Mirror { .. } => PatternForm::Mirror,
                };
                if pattern_form(record) != Some(expected_form) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes pattern form",
                        feature.id
                    )));
                }
                let seed_sources = seeds
                    .iter()
                    .map(|seed| parent_sources.get(seed).cloned())
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing pattern seed",
                            feature.id
                        ))
                    })?;
                if seed_sources.is_empty() {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has no pattern seeds",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                let mut properties = record.properties.clone();
                properties.insert("Seeds".into(), seed_sources.join(","));
                match pattern {
                    PatternKind::Linear {
                        direction,
                        spacing,
                        count,
                    } => {
                        require_direction(*direction, &feature.id, "pattern")?;
                        require_count(*count, &feature.id)?;
                        properties.insert("Direction".into(), format_vector3(*direction));
                        parameters.insert("Spacing".into(), format_length_mm(spacing.0));
                        parameters.insert("Count".into(), count.to_string());
                    }
                    PatternKind::Circular {
                        axis_origin,
                        axis_dir,
                        angle,
                        count,
                    } => {
                        require_direction(*axis_dir, &feature.id, "pattern axis")?;
                        require_count(*count, &feature.id)?;
                        properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
                        properties.insert("AxisDirection".into(), format_vector3(*axis_dir));
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                        parameters.insert("Count".into(), count.to_string());
                    }
                    PatternKind::Mirror {
                        plane_origin,
                        plane_normal,
                    } => {
                        require_direction(*plane_normal, &feature.id, "mirror plane normal")?;
                        properties.insert("PlaneOrigin".into(), format_point3_mm(*plane_origin));
                        properties.insert("PlaneNormal".into(), format_vector3(*plane_normal));
                    }
                }
                (record.kind.clone(), parameters, properties)
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires native operation serialization",
                    feature.id
                )))
            }
        };
        let ordinal = u32::try_from(feature.ordinal)
            .map_err(|_| CodecError::Malformed("feature ordinal exceeds u32".into()))?;
        let parent_source_id = feature
            .parent
            .as_ref()
            .and_then(|parent| structural_parent_sources.get(parent).cloned().flatten());
        let tree_parent = feature
            .parent
            .as_ref()
            .and_then(|parent| record_ids.get(parent).cloned());
        if let Some(existing) = existing.as_mut() {
            existing.ordinal = ordinal;
            existing.name = feature.name.clone().unwrap_or_default();
            existing.kind = kind;
            existing.suppressed = feature.suppressed;
            existing.parent_source_id = parent_source_id;
            existing.tree_parent = tree_parent;
            existing.parameters = parameters;
            existing.properties = properties;
        } else {
            let history = &mut native.feature_histories[0];
            history.features.push(Feature {
                id: record_ids[&feature.id].clone(),
                parent: history.id.clone(),
                xml_tag: feature_xml_tag(feature),
                tree_parent,
                source_id: Some(feature.id.0.clone()),
                parent_source_id,
                ordinal,
                name: feature.name.clone().unwrap_or_default(),
                kind,
                suppressed: feature.suppressed,
                parameters,
                properties,
            });
        }
    }
    Ok(())
}

fn profile_source(
    profile: &ProfileRef,
    native: &HashMap<String, String>,
    sketches: &HashMap<cadmpeg_ir::sketches::SketchId, String>,
) -> Option<String> {
    match profile {
        ProfileRef::Native(id) => native.get(id).cloned(),
        ProfileRef::Sketch(id) => sketches.get(id).cloned(),
        ProfileRef::Faces(_) => None,
    }
}

fn path_source(
    path: &PathRef,
    native: &HashMap<String, String>,
    sketches: &HashMap<cadmpeg_ir::sketches::SketchId, String>,
) -> Option<String> {
    match path {
        PathRef::Native(id) => native.get(id).cloned(),
        PathRef::Sketch(id) => sketches.get(id).cloned(),
        PathRef::Edges(_) | PathRef::Curves(_) => None,
    }
}

fn extrude_op(kind: &str) -> Option<BooleanOp> {
    match kind.to_ascii_lowercase().as_str() {
        "bossextrude" => Some(BooleanOp::Join),
        "cutextrude" => Some(BooleanOp::Cut),
        _ => None,
    }
}

fn require_direction(
    direction: Vector3,
    feature: &FeatureId,
    role: &str,
) -> Result<(), CodecError> {
    if direction.norm().is_finite() && direction.norm() > 0.0 {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "SLDPRT feature {feature} has a degenerate {role}"
        )))
    }
}

fn require_count(count: u32, feature: &FeatureId) -> Result<(), CodecError> {
    if count > 0 {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "SLDPRT feature {feature} has a zero pattern count"
        )))
    }
}

fn indexed_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn feature_xml_tag(feature: &cadmpeg_ir::features::Feature) -> String {
    let tag = match &feature.definition {
        FeatureDefinition::DatumPlane { .. } => "ReferencePlane",
        FeatureDefinition::DatumAxis { .. } => "ReferenceAxis",
        FeatureDefinition::DatumPoint { .. } => "ReferencePoint",
        FeatureDefinition::Sketch { .. } => "Sketch",
        FeatureDefinition::Extrude { .. } => "Extrusion",
        FeatureDefinition::Revolve { .. } => "Revolve",
        FeatureDefinition::Sweep { .. } => "Sweep",
        FeatureDefinition::Loft { .. } => "Loft",
        FeatureDefinition::Rib { .. } => "Rib",
        FeatureDefinition::Fillet { .. } => "Fillet",
        FeatureDefinition::Chamfer { .. } => "Chamfer",
        FeatureDefinition::Shell { .. } => "Shell",
        FeatureDefinition::Draft { .. } => "Draft",
        FeatureDefinition::Combine { .. } => "Combine",
        FeatureDefinition::DeleteFace { .. } => "DeleteFace",
        FeatureDefinition::MoveFace { .. } => "MoveFace",
        FeatureDefinition::Dome { .. } => "Dome",
        FeatureDefinition::Hole { .. } => "Hole",
        FeatureDefinition::Pattern {
            pattern: PatternKind::Mirror { .. },
            ..
        } => "Mirror",
        FeatureDefinition::Pattern { .. } => "Pattern",
        FeatureDefinition::Native { kind, .. } if extrude_op(kind).is_some() => "Extrusion",
        FeatureDefinition::Native { kind, .. } if valid_xml_name(kind) => kind,
        FeatureDefinition::Native { .. } => "Feature",
    };
    tag.into()
}

fn valid_xml_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':'))
        && bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-' | b'.'))
}

fn xml_text(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_prefix(&[0x86]).unwrap_or(bytes);
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        Some(String::from_utf16_lossy(&units))
    } else {
        std::str::from_utf8(bytes).ok().map(str::to_string)
    }
}
