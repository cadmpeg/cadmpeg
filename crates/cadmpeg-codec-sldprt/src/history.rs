// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

use crate::classification::{classify, native_object_class, FeatureClass, NativeClassKind};
use crate::container::ContainerScan;
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::features::{
    Angle, AxisAngle, BodyRetentionMode, BodySelection, BooleanOp, ChamferForm, ChamferSpec,
    ConfigurationBodies, ConfigurationId, CurveProjectionDirection, CurveProjectionDirectionState,
    DesignConfiguration, DesignParameter, DimensionDisplay, EdgeSelection, Extent, FaceMotion,
    FaceSelection, FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole,
    FlexForm, FlexMode, HoleForm, HoleKind, Length, ParameterId, ParameterValue, PathRef,
    PatternForm, PatternKind, ProfileRef, RadiusForm, RadiusSpec, RevolutionAxis,
    RevolutionConstruction, RibConstruction, RibDraft, RibSide, RuledSurfaceMode, ScaleCenter,
    ScaleFactors, SketchSpace, SurfaceContinuity, SurfaceExtension, SweepMode, TrimRegion,
    VariableRadius, WrapMode,
};
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
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
                        ordinal: ordinal as u32,
                        source_index: None,
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
                            .find(|ancestor| feature_ids.contains_key(&ancestor.range().start))
                            .and_then(|parent| parent.attribute("id"))
                            .map(str::to_string),
                        ordinal: ordinal as u32,
                        name: node.attribute("Name").unwrap_or("").into(),
                        kind: node
                            .attribute("Type")
                            .unwrap_or_else(|| node.tag_name().name())
                            .into(),
                        input_class: None,
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
                                    dimension.text().unwrap_or_default().trim().into(),
                                ))
                            })
                            .collect::<BTreeMap<_, _>>(),
                        dimension_properties: node
                            .children()
                            .filter(|child| {
                                child.is_element() && child.tag_name().name() == "Dimension"
                            })
                            .filter_map(|dimension| {
                                let name = dimension.attribute("Name")?;
                                let properties = dimension
                                    .attributes()
                                    .filter(|attribute| attribute.name() != "Name")
                                    .map(|attribute| {
                                        (
                                            attribute.name().to_string(),
                                            attribute.value().to_string(),
                                        )
                                    })
                                    .collect::<BTreeMap<_, _>>();
                                (!properties.is_empty()).then(|| (name.into(), properties))
                            })
                            .collect(),
                        properties: node
                            .attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "id" | "Name" | "Type" | "Suppressed")
                            })
                            .map(|attribute| {
                                (attribute.name().to_string(), attribute.value().to_string())
                            })
                            .collect(),
                        text: (!node.children().any(|child| child.is_element()))
                            .then(|| node.text().map(str::trim).unwrap_or_default().to_string())
                            .filter(|value| !value.is_empty()),
                        content: node
                            .children()
                            .filter_map(|child| {
                                if child.is_text() {
                                    let value = child.text()?.trim();
                                    return (!value.is_empty())
                                        .then(|| FeatureContent::Text(value.into()));
                                }
                                if !child.is_element() {
                                    return None;
                                }
                                if child.tag_name().name() == "Dimension" {
                                    return child
                                        .attribute("Name")
                                        .map(|name| FeatureContent::Dimension(name.into()));
                                }
                                feature_ids
                                    .get(&child.range().start)
                                    .cloned()
                                    .map(FeatureContent::Feature)
                            })
                            .collect(),
                    }
                })
                .collect::<Vec<_>>();
            let configuration_ids = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .map(|(ordinal, node)| {
                    (
                        node.range().start,
                        format!("sldprt:history:configuration#{}:{ordinal}", block.offset),
                    )
                })
                .collect::<HashMap<_, _>>();
            let content = root
                .children()
                .filter_map(|child| {
                    if child.is_text() {
                        let value = child.text()?.trim();
                        return (!value.is_empty()).then(|| HistoryContent::Text(value.into()));
                    }
                    if !child.is_element() {
                        return None;
                    }
                    configuration_ids
                        .get(&child.range().start)
                        .cloned()
                        .map(HistoryContent::Configuration)
                        .or_else(|| {
                            feature_ids
                                .get(&child.range().start)
                                .cloned()
                                .map(HistoryContent::Feature)
                        })
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
                properties: root
                    .attributes()
                    .filter(|attribute| attribute.name() != "Name")
                    .map(|attribute| (attribute.name().to_string(), attribute.value().to_string()))
                    .collect(),
                content,
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
                    suppressed: Some(feature.suppressed),
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
                    dependencies: project_feature_dependencies(feature, &by_source),
                    source_properties: feature.properties.clone(),
                    source_tag: Some(feature.xml_tag.clone()),
                    source_text: feature.text.clone(),
                    source_content: project_feature_content(feature, &by_native),
                    outputs: Vec::new(),
                    definition: project_definition(feature, &by_source, &native_by_source),
                    native_ref: Some(feature.id.clone()),
                })
        })
        .collect()
}

fn project_feature_content(
    feature: &Feature,
    by_native: &HashMap<&str, FeatureId>,
) -> Vec<FeatureSourceContent> {
    if feature.text.is_some() {
        return Vec::new();
    }
    let parameters = parameter_names(feature)
        .into_iter()
        .enumerate()
        .map(|(ordinal, name)| (name, neutral_parameter_id(feature, ordinal)))
        .collect::<HashMap<_, _>>();
    feature
        .content
        .iter()
        .filter_map(|content| match content {
            FeatureContent::Text(text) => Some(FeatureSourceContent::Text(text.clone())),
            FeatureContent::Dimension(name) => parameters
                .get(name)
                .cloned()
                .map(FeatureSourceContent::Parameter),
            FeatureContent::Feature(id) => by_native
                .get(id.as_str())
                .cloned()
                .map(FeatureSourceContent::Feature),
        })
        .collect()
}

fn parameter_names(feature: &Feature) -> Vec<String> {
    let mut names = feature
        .content
        .iter()
        .filter_map(|content| match content {
            FeatureContent::Dimension(name) if feature.parameters.contains_key(name) => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let missing = feature
        .parameters
        .keys()
        .filter(|name| !names.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    names.extend(missing);
    names
}

fn neutral_parameter_id(feature: &Feature, ordinal: usize) -> ParameterId {
    let key = feature
        .id
        .strip_prefix("sldprt:history:feature#")
        .unwrap_or(&feature.id);
    ParameterId(format!("sldprt:model:parameter#{key}:{ordinal}"))
}

fn project_feature_dependencies(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
) -> Vec<FeatureId> {
    const REFERENCE_PROPERTIES: &[&str] = &[
        "Profile",
        "Path",
        "Profiles",
        "Guides",
        "Seeds",
        "Dependency",
        "Dependencies",
        "ParentFeatures",
    ];
    let owner = neutral_feature_id(&feature.id);
    let mut seen = std::collections::HashSet::new();
    REFERENCE_PROPERTIES
        .iter()
        .filter_map(|name| feature.properties.get(*name))
        .flat_map(|value| {
            value
                .split(|character: char| {
                    character == ',' || character == ';' || character.is_whitespace()
                })
                .filter(|reference| !reference.is_empty())
        })
        .filter_map(|reference| by_source.get(reference).cloned())
        .filter(|dependency| dependency != &owner)
        .filter(|dependency| seen.insert(dependency.clone()))
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
            ordinal: configuration.ordinal,
            active: false,
            source_index: configuration.source_index,
            name: configuration.name.clone(),
            material: configuration.material.clone(),
            properties: configuration.properties.clone(),
            bodies: ConfigurationBodies::Unresolved,
            native_ref: Some(configuration.id.clone()),
        })
        .collect()
}

/// Project every native feature dimension into the neutral parameter arena.
pub fn project_parameters(histories: &[FeatureHistory]) -> Vec<DesignParameter> {
    let feature_names = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| !feature.name.is_empty())
        .map(|feature| (neutral_feature_id(&feature.id), feature.name.clone()))
        .collect::<HashMap<_, _>>();
    let mut parameters = histories
        .iter()
        .flat_map(|history| &history.features)
        .flat_map(|feature| {
            parameter_names(feature)
                .into_iter()
                .enumerate()
                .map(move |(ordinal, name)| {
                    let expression = &feature.parameters[&name];
                    let display = dimension_display(expression);
                    let properties = feature
                        .dimension_properties
                        .get(&name)
                        .cloned()
                        .unwrap_or_default();
                    let parse_value = |value: &str| match display {
                        Some(DimensionDisplay::Diameter | DimensionDisplay::Radius) => {
                            parse_dimension_display_length(value)
                                .map(|value| ParameterValue::Length(Length(value)))
                        }
                        None => parse_native_parameter_literal(feature, &name, value),
                    };
                    let value = properties
                        .get("Value")
                        .and_then(|value| parse_value(value))
                        .or_else(|| parse_value(expression));
                    DesignParameter {
                        id: neutral_parameter_id(feature, ordinal),
                        owner: neutral_feature_id(&feature.id),
                        ordinal: ordinal as u32,
                        properties,
                        name,
                        expression: expression.clone(),
                        display,
                        value,
                        dependencies: Vec::new(),
                        native_ref: None,
                        pmi: None,
                    }
                })
        })
        .collect::<Vec<_>>();
    populate_parameter_dependencies(&mut parameters, &feature_names);
    parameters
}

fn parse_native_parameter_literal(
    feature: &Feature,
    name: &str,
    expression: &str,
) -> Option<ParameterValue> {
    if native_parameter_is_length(feature, name, Some(expression)) {
        return parse_positive_dimension_length_mm(expression)
            .map(|value| ParameterValue::Length(Length(value)));
    }
    parse_parameter_literal(expression)
}

fn native_parameter_is_length(feature: &Feature, name: &str, expression: Option<&str>) -> bool {
    match name {
        "D1" => {
            is_extrude(feature)
                || is_fillet(feature)
                || is_chamfer(feature)
                || feature_family(feature, "Shell")
                || feature_family(feature, "Thicken")
                || feature_family(feature, "Thickness")
                || feature_input_class(feature, NativeClassKind::Thicken)
                || is_offset_plane(feature)
        }
        "D2" if is_chamfer(feature) => {
            expression.is_none_or(|value| parse_angle_rad(value).is_none())
        }
        "D3" if matches!(
            pattern_form(feature),
            Some(PatternForm::Linear | PatternForm::CurveDriven)
        ) =>
        {
            true
        }
        _ => false,
    }
}

pub(crate) fn format_native_scalar(feature: &Feature, name: &str, value: f64) -> String {
    if native_parameter_is_length(feature, name, None) {
        format_length_mm(value * 1000.0)
    } else {
        value.to_string()
    }
}

fn populate_parameter_dependencies(
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
) {
    let mut aliases = HashMap::<String, Option<ParameterId>>::new();
    for parameter in parameters.iter() {
        let mut parameter_aliases = vec![parameter.id.0.clone(), parameter.name.clone()];
        if let Some(equation_id) = parameter.properties.get("EquationId") {
            parameter_aliases.push(equation_id.clone());
        }
        if let Some(owner_name) = feature_names.get(&parameter.owner) {
            parameter_aliases.push(format!("{}@{owner_name}", parameter.name));
            if let Some(equation_id) = parameter.properties.get("EquationId") {
                parameter_aliases.push(format!("{equation_id}@{owner_name}"));
            }
        }
        for alias in parameter_aliases {
            aliases
                .entry(alias)
                .and_modify(|candidate| *candidate = None)
                .or_insert_with(|| Some(parameter.id.clone()));
        }
    }
    for parameter in parameters.iter_mut() {
        let mut seen = std::collections::HashSet::new();
        parameter.dependencies = expression_identifiers(&parameter.expression)
            .filter_map(|identifier| aliases.get(&identifier).and_then(Clone::clone))
            .filter(|dependency| dependency != &parameter.id && seen.insert(dependency.clone()))
            .collect();
    }
}

fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> + '_ {
    expression_identifier_tokens(expression)
        .into_iter()
        .map(|token| token.value)
}

struct ExpressionIdentifier {
    start: usize,
    end: usize,
    value: String,
    quoted: bool,
}

fn expression_identifier_tokens(expression: &str) -> Vec<ExpressionIdentifier> {
    let mut identifiers = Vec::new();
    let mut at = 0;
    while at < expression.len() {
        let rest = &expression[at..];
        if rest.starts_with('"') {
            let mut value = String::new();
            let mut cursor = at + 1;
            let mut closed = false;
            while cursor < expression.len() {
                let quoted = &expression[cursor..];
                if quoted.starts_with("\"\"") {
                    value.push('"');
                    cursor += 2;
                } else if quoted.starts_with('"') {
                    cursor += 1;
                    closed = true;
                    break;
                } else {
                    let character = quoted.chars().next().expect("nonempty suffix");
                    value.push(character);
                    cursor += character.len_utf8();
                }
            }
            if closed && !value.is_empty() {
                identifiers.push(ExpressionIdentifier {
                    start: at,
                    end: cursor,
                    value,
                    quoted: true,
                });
                at = cursor;
                continue;
            }
        }

        let Some(character) = rest.chars().next() else {
            break;
        };
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.' | '-') {
            let end = rest
                .find(|candidate: char| {
                    !(candidate.is_ascii_alphanumeric()
                        || matches!(candidate, '_' | '@' | '$' | '.' | '-'))
                })
                .unwrap_or(rest.len());
            identifiers.push(ExpressionIdentifier {
                start: at,
                end: at + end,
                value: rest[..end].to_string(),
                quoted: false,
            });
            at += end;
        } else {
            at += character.len_utf8();
        }
    }
    identifiers
}

/// Bind a uniquely identified native sketch history node to solved sketch geometry.
pub fn bind_unique_sketch_feature(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
) {
    let feature_indices = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| {
            matches!(
                feature.definition,
                FeatureDefinition::Sketch {
                    space: SketchSpace::Planar,
                    ..
                }
            )
        })
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
            bindings.push((
                *index,
                features[*index].id.clone(),
                native_ref,
                sketch.id.clone(),
            ));
        }
    }
    if bindings.is_empty() {
        if let ([index], [sketch]) = (feature_indices.as_slice(), sketches) {
            if let Some(native_ref) = features[*index].native_ref.clone() {
                bindings.push((
                    *index,
                    features[*index].id.clone(),
                    native_ref,
                    sketch.id.clone(),
                ));
            }
        }
    }
    for (index, _, _, sketch) in &bindings {
        features[*index].definition = FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: Some(sketch.clone()),
        };
    }
    for feature in features {
        for (_, dependency, native_ref, sketch) in &bindings {
            if bind_definition_sketch(&mut feature.definition, native_ref, sketch)
                && !feature.dependencies.contains(dependency)
            {
                feature.dependencies.push(dependency.clone());
            }
        }
    }
}

/// Resolve native topology selections against decoded B-rep identities.
pub fn bind_topology_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    bodies: &[Body],
    faces: &[Face],
    edges: &[Edge],
    curves: &[Curve],
) {
    let body_ids = selection_ids(
        bodies
            .iter()
            .map(|body| (body.id.0.as_str(), body.name.as_deref(), body.id.clone())),
    );
    let face_ids = selection_ids(
        faces
            .iter()
            .map(|face| (face.id.0.as_str(), face.name.as_deref(), face.id.clone())),
    );
    let edge_ids = selection_ids(
        edges
            .iter()
            .map(|edge| (edge.id.0.as_str(), None, edge.id.clone())),
    );
    let curve_ids = selection_ids(
        curves
            .iter()
            .map(|curve| (curve.id.0.as_str(), None, curve.id.clone())),
    );
    for feature in features {
        if let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| {
                histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|record| record.id == native_ref)
            })
            .and_then(|record| record.properties.get("Scope"))
        {
            if let Some(outputs) = resolve_ids(scope, &body_ids) {
                feature.outputs = outputs;
            }
        }
        match &mut feature.definition {
            FeatureDefinition::ExtractBody { source } => {
                resolve_body_selection(source, &body_ids);
            }
            FeatureDefinition::Extrude {
                profile, extent, ..
            } => {
                resolve_profile_ref(profile, &face_ids);
                if let Extent::ToFace { face } = extent {
                    resolve_face_selection(face, &face_ids);
                }
            }
            FeatureDefinition::Revolve { construction, .. } => {
                if let Some(profile) = &mut construction.profile {
                    resolve_profile_ref(profile, &face_ids);
                }
            }
            FeatureDefinition::Rib { construction, .. } => {
                if let Some(profile) = &mut construction.profile {
                    resolve_profile_ref(profile, &face_ids);
                }
            }
            FeatureDefinition::Sweep { profile, path, .. } => {
                if let Some(profile) = profile {
                    resolve_profile_ref(profile, &face_ids);
                }
                if let Some(path) = path {
                    resolve_path_ref(path, &edge_ids, &curve_ids);
                }
            }
            FeatureDefinition::Loft {
                profiles, guides, ..
            } => {
                for profile in profiles {
                    resolve_profile_ref(profile, &face_ids);
                }
                for path in guides {
                    resolve_path_ref(path, &edge_ids, &curve_ids);
                }
            }
            FeatureDefinition::Fillet { edges, .. } | FeatureDefinition::Chamfer { edges, .. } => {
                resolve_edge_selection(edges, &edge_ids);
            }
            FeatureDefinition::Shell { removed_faces, .. } => {
                resolve_face_selection(removed_faces, &face_ids);
            }
            FeatureDefinition::Thicken { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::OffsetSurface { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::KnitSurface { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                ..
            } => {
                resolve_edge_selection(boundary, &edge_ids);
                resolve_face_selection(support_faces, &face_ids);
            }
            FeatureDefinition::TrimSurface { faces, tool, .. } => {
                resolve_face_selection(faces, &face_ids);
                resolve_path_ref(tool, &edge_ids, &curve_ids);
            }
            FeatureDefinition::ExtendSurface { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                ..
            } => {
                resolve_edge_selection(edges, &edge_ids);
                resolve_face_selection(support_faces, &face_ids);
            }
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                ..
            } => {
                resolve_face_selection(faces, &face_ids);
                resolve_face_selection(neutral_plane, &face_ids);
            }
            FeatureDefinition::Combine { target, tools, .. } => {
                resolve_body_selection(target, &body_ids);
                resolve_body_selection(tools, &body_ids);
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                resolve_body_selection(targets, &body_ids);
                resolve_face_selection(tools, &face_ids);
            }
            FeatureDefinition::DeleteBody { bodies, .. } => {
                resolve_body_selection(bodies, &body_ids);
            }
            FeatureDefinition::Pattern {
                pattern:
                    PatternKind::CurveDriven {
                        path: Some(path), ..
                    },
                ..
            } => resolve_path_ref(path, &edge_ids, &curve_ids),
            FeatureDefinition::Scale { bodies, .. } => {
                resolve_body_selection(bodies, &body_ids);
            }
            FeatureDefinition::MoveBody { bodies, .. } => {
                resolve_body_selection(bodies, &body_ids);
            }
            FeatureDefinition::DeleteFace { faces, .. }
            | FeatureDefinition::MoveFace { faces, .. }
            | FeatureDefinition::Dome { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => {
                resolve_face_selection(targets, &face_ids);
                resolve_face_selection(replacements, &face_ids);
            }
            FeatureDefinition::Hole {
                face: Some(face), ..
            } => {
                resolve_face_selection(face, &face_ids);
            }
            FeatureDefinition::Wrap { profile, face, .. } => {
                resolve_profile_ref(profile, &face_ids);
                resolve_face_selection(face, &face_ids);
            }
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                ..
            } => {
                resolve_path_ref(source, &edge_ids, &curve_ids);
                resolve_face_selection(target_faces, &face_ids);
            }
            FeatureDefinition::CompositeCurve { segments, .. } => {
                for segment in segments {
                    resolve_path_ref(segment, &edge_ids, &curve_ids);
                }
            }
            _ => {}
        }
    }
}

fn resolve_profile_ref(
    profile: &mut ProfileRef,
    faces: &HashMap<String, Option<cadmpeg_ir::ids::FaceId>>,
) {
    if let ProfileRef::Native(native) = profile {
        if let Some(ids) = resolve_ids(native, faces) {
            *profile = ProfileRef::Faces(ids);
        }
    }
}

fn resolve_path_ref(
    path: &mut PathRef,
    edges: &HashMap<String, Option<cadmpeg_ir::ids::EdgeId>>,
    curves: &HashMap<String, Option<cadmpeg_ir::ids::CurveId>>,
) {
    if let PathRef::Native(native) = path {
        if let Some(ids) = resolve_ids(native, edges) {
            *path = PathRef::Edges(ids);
        } else if let Some(ids) = resolve_ids(native, curves) {
            *path = PathRef::Curves(ids);
        }
    }
}

fn selection_ids<'a, Id: Clone + 'a>(
    values: impl Iterator<Item = (&'a str, Option<&'a str>, Id)>,
) -> HashMap<String, Option<Id>> {
    let mut ids = HashMap::new();
    for (id, name, value) in values {
        ids.insert(id.to_string(), Some(value.clone()));
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            ids.entry(name.to_string())
                .and_modify(|candidate| *candidate = None)
                .or_insert(Some(value));
        }
    }
    ids
}

fn resolve_ids<Id: Clone>(native: &str, ids: &HashMap<String, Option<Id>>) -> Option<Vec<Id>> {
    let resolved = native
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| ids.get(token).and_then(Clone::clone))
        .collect::<Option<Vec<_>>>()?;
    (!resolved.is_empty()).then_some(resolved)
}

fn resolve_face_selection(
    selection: &mut FaceSelection,
    ids: &HashMap<String, Option<cadmpeg_ir::ids::FaceId>>,
) {
    if let FaceSelection::Native(native) = selection {
        if let Some(faces) = resolve_ids(native, ids) {
            *selection = FaceSelection::Resolved {
                faces,
                native: native.clone(),
            };
        }
    }
}

fn resolve_edge_selection(
    selection: &mut EdgeSelection,
    ids: &HashMap<String, Option<cadmpeg_ir::ids::EdgeId>>,
) {
    if let EdgeSelection::Native(native) = selection {
        if let Some(edges) = resolve_ids(native, ids) {
            *selection = EdgeSelection::Resolved {
                edges,
                native: native.clone(),
            };
        }
    }
}

fn resolve_body_selection(
    selection: &mut BodySelection,
    ids: &HashMap<String, Option<cadmpeg_ir::ids::BodyId>>,
) {
    if let BodySelection::Native(native) = selection {
        if let Some(bodies) = resolve_ids(native, ids) {
            *selection = BodySelection::Resolved {
                bodies,
                native: native.clone(),
            };
        }
    }
}

fn bind_definition_sketch(
    definition: &mut FeatureDefinition,
    native_ref: &str,
    sketch: &cadmpeg_ir::sketches::SketchId,
) -> bool {
    let bind_profile = |profile: &mut ProfileRef| {
        if matches!(profile, ProfileRef::Native(value) if value == native_ref) {
            *profile = ProfileRef::Sketch(sketch.clone());
            true
        } else {
            false
        }
    };
    let bind_path = |path: &mut PathRef| {
        if matches!(path, PathRef::Native(value) if value == native_ref) {
            *path = PathRef::Sketch(sketch.clone());
            true
        } else {
            false
        }
    };
    match definition {
        FeatureDefinition::Extrude { profile, .. } | FeatureDefinition::Wrap { profile, .. } => {
            bind_profile(profile)
        }
        FeatureDefinition::Rib { construction, .. } => {
            construction.profile.as_mut().is_some_and(bind_profile)
        }
        FeatureDefinition::Revolve { construction, .. } => {
            construction.profile.as_mut().is_some_and(bind_profile)
        }
        FeatureDefinition::Sweep { profile, path, .. } => {
            profile.as_mut().is_some_and(bind_profile) | path.as_mut().is_some_and(bind_path)
        }
        FeatureDefinition::TrimSurface { tool, .. } => bind_path(tool),
        FeatureDefinition::ProjectedCurve { source, .. } => bind_path(source),
        FeatureDefinition::CompositeCurve { segments, .. } => segments.iter_mut().any(bind_path),
        FeatureDefinition::Loft {
            profiles, guides, ..
        } => {
            let mut profile_bound = false;
            for profile in profiles {
                profile_bound |= bind_profile(profile);
            }
            let mut guide_bound = false;
            for path in guides {
                guide_bound |= bind_path(path);
            }
            profile_bound || guide_bound
        }
        _ => false,
    }
}

fn project_definition(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
    native_by_source: &HashMap<&str, &str>,
) -> FeatureDefinition {
    if let Some(role) = feature_tree_node_role(feature) {
        return FeatureDefinition::TreeNode {
            role,
            children: Vec::new(),
            active_child: None,
        };
    }
    let class = classify(feature);
    if class == Some(FeatureClass::Sketch) {
        return FeatureDefinition::Sketch {
            space: if feature.kind.eq_ignore_ascii_case("3DSketch") {
                SketchSpace::Spatial
            } else {
                SketchSpace::Planar
            },
            sketch: None,
        };
    }
    if class == Some(FeatureClass::ReferencePlane) && is_offset_plane(feature) {
        return project_offset_plane(feature, by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if let Some(plane) = principal_plane(feature) {
        return FeatureDefinition::DatumPrincipalPlane { plane };
    }
    if class == Some(FeatureClass::ReferencePlane) {
        return project_datum_plane(feature).unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::ReferenceAxis) {
        return project_datum_axis(feature).unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::ReferencePoint) {
        return project_datum_point(feature).unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::CoordinateSystem) {
        return project_datum_coordinate_system(feature)
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::EquationCurve) {
        return project_equation_curve(feature).unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::ProjectedCurve) {
        return project_projected_curve(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::CompositeCurve) {
        return project_composite_curve(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::Helix) {
        return project_helix(feature)
            .or_else(|| project_native_axis_helix(feature))
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::Wrap) {
        return project_wrap(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::Extrude) {
        project_extrude(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Fillet) {
        project_fillet(feature)
    } else if class == Some(FeatureClass::Chamfer) {
        project_chamfer(feature)
    } else if class == Some(FeatureClass::Shell) {
        project_shell(feature)
    } else if class == Some(FeatureClass::Thicken) {
        project_thicken(feature)
    } else if class == Some(FeatureClass::OffsetSurface) {
        project_offset_surface(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::KnitSurface) {
        project_knit_surface(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::FilledSurface) {
        project_filled_surface(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::TrimSurface) {
        project_trim_surface(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::ExtendSurface) {
        project_extend_surface(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::RuledSurface) {
        project_ruled_surface(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Draft) {
        project_draft(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Combine) {
        project_combine(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::CutWithSurface) {
        project_cut_with_surface(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::DeleteBody) {
        project_delete_body(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::DeleteFace) {
        project_delete_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::ReplaceFace) {
        project_replace_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::MoveFace) {
        project_move_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::MoveBody) {
        project_move_body(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Dome) {
        project_dome(feature)
    } else if class == Some(FeatureClass::Flex) {
        project_flex(feature)
    } else if class == Some(FeatureClass::Scale) {
        project_scale(feature)
    } else if class == Some(FeatureClass::Hole) {
        project_hole(feature)
    } else if class == Some(FeatureClass::Revolve) {
        project_revolve(feature, native_by_source)
    } else if class == Some(FeatureClass::Pattern) {
        project_pattern(feature, by_source, native_by_source)
    } else if class == Some(FeatureClass::Sweep) {
        project_sweep(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Loft) {
        project_loft(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Rib) {
        project_rib(feature, native_by_source)
    } else {
        native_definition(feature)
    }
}

fn feature_tree_node_role(feature: &Feature) -> Option<FeatureTreeNodeRole> {
    native_object_class(feature.input_class.as_deref()?).tree_node
}

fn feature_tree_node_kind(role: FeatureTreeNodeRole) -> &'static str {
    match role {
        FeatureTreeNodeRole::Annotations => "Annotations",
        FeatureTreeNodeRole::AmbientLight => "Ambient",
        FeatureTreeNodeRole::Comments => "Comments",
        FeatureTreeNodeRole::DesignBinder => "Design Binder",
        FeatureTreeNodeRole::DirectionalLight => "Directional",
        FeatureTreeNodeRole::Equations => "Equations",
        FeatureTreeNodeRole::ExplodedViews => "Exploded Views",
        FeatureTreeNodeRole::Favorites => "Favorites",
        FeatureTreeNodeRole::History => "History",
        FeatureTreeNodeRole::LightsAndCameras => "Lights and Cameras",
        FeatureTreeNodeRole::Markups => "Markups",
        FeatureTreeNodeRole::Materials => "SOLIDWORKS Materials",
        FeatureTreeNodeRole::Notes => "Notes",
        FeatureTreeNodeRole::SelectionSets => "Selection Sets",
        FeatureTreeNodeRole::Sensors => "Sensors",
        FeatureTreeNodeRole::SolidBodies => "Solid Bodies",
        FeatureTreeNodeRole::SurfaceBodies => "Surface Bodies",
    }
}

fn feature_family(feature: &Feature, family: &str) -> bool {
    feature.xml_tag.eq_ignore_ascii_case(family)
}

fn feature_input_class(feature: &Feature, class: NativeClassKind) -> bool {
    feature
        .input_class
        .as_deref()
        .map(native_object_class)
        .map(|class| class.kind)
        == Some(class)
}

fn is_fillet(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Fillet)
}

fn is_chamfer(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Chamfer)
}

fn is_extrude(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Extrude)
}

fn extrude_feature_op(feature: &Feature) -> Option<BooleanOp> {
    extrude_op(&feature.kind)
}

fn is_revolve(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Revolve)
}

fn is_loft(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Loft)
}

fn is_sweep(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Sweep)
}

fn is_helix(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Helix)
}

fn is_offset_plane(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::ReferencePlane) && feature.parameters.contains_key("D1")
}

fn principal_plane(feature: &Feature) -> Option<cadmpeg_ir::features::PrincipalPlane> {
    use cadmpeg_ir::features::PrincipalPlane;

    if !feature.parameters.is_empty() || !feature.properties.is_empty() {
        return None;
    }
    match feature.source_id.as_deref()? {
        "2" => Some(PrincipalPlane::Front),
        "3" => Some(PrincipalPlane::Top),
        "4" => Some(PrincipalPlane::Right),
        _ => None,
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
        .or_else(|| extrude_feature_op(feature))
        .unwrap_or(BooleanOp::Unresolved);
    let length = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_positive_length_mm(value))
            .or_else(|| {
                (name == "Depth")
                    .then(|| feature.parameters.get("D1"))
                    .flatten()
                    .and_then(|value| parse_positive_dimension_length_mm(value))
            })
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
            Some(
                native_by_source
                    .get(source.as_str())
                    .map_or_else(|| source.clone(), |id| (*id).to_string()),
            )
        },
    )?;
    Some(FeatureDefinition::Extrude {
        profile: ProfileRef::Native(profile),
        direction,
        extent,
        op,
        draft,
        reverse_draft: None,
        direction_source: None,
        solid: Some(true),
        face_maker: None,
        inner_wire_taper: None,
        first_offset: None,
        second_offset: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
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

fn project_offset_plane(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    let reference = feature
        .properties
        .get("Reference")
        .or_else(|| feature.properties.get("Plane"))
        .and_then(|source| by_source.get(source.as_str()).cloned());
    Some(FeatureDefinition::DatumOffsetPlane {
        reference,
        distance: Length(parse_dimension_length_mm(feature.parameters.get("D1")?)?),
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

fn project_datum_coordinate_system(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let x_axis = parse_vector3(feature.properties.get("XAxis")?)?;
    let y_axis = parse_vector3(feature.properties.get("YAxis")?)?;
    let z_axis = parse_vector3(feature.properties.get("ZAxis")?)?;
    valid_coordinate_frame(origin, x_axis, y_axis, z_axis).then_some(
        FeatureDefinition::DatumCoordinateSystem {
            origin,
            x_axis,
            y_axis,
            z_axis,
        },
    )
}

fn project_equation_curve(feature: &Feature) -> Option<FeatureDefinition> {
    let parameter = feature.properties.get("Parameter")?.trim().to_string();
    let x_expression = feature.properties.get("XEquation")?.trim().to_string();
    let y_expression = feature.properties.get("YEquation")?.trim().to_string();
    let z_expression = feature.properties.get("ZEquation")?.trim().to_string();
    let start = feature
        .properties
        .get("Start")?
        .trim()
        .parse::<f64>()
        .ok()?;
    let end = feature.properties.get("End")?.trim().parse::<f64>().ok()?;
    (!parameter.is_empty()
        && !x_expression.is_empty()
        && !y_expression.is_empty()
        && !z_expression.is_empty()
        && start.is_finite()
        && end.is_finite()
        && start < end)
        .then_some(FeatureDefinition::EquationCurve {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start,
            end,
        })
}

fn project_projected_curve(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let source = feature.properties.get("Source")?;
    let source = native_by_source
        .get(source.as_str())
        .map_or_else(|| source.clone(), |id| (*id).to_string());
    let direction = match feature.properties.get("Direction") {
        Some(value) => CurveProjectionDirection::Vector(parse_valid_direction(value)?),
        None => CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal),
    };
    Some(FeatureDefinition::ProjectedCurve {
        source: PathRef::Native(source),
        target_faces: FaceSelection::Native(feature.properties.get("TargetFaces")?.clone()),
        direction,
        bidirectional: Some(
            feature
                .properties
                .get("Bidirectional")
                .and_then(|value| parse_bool(value))
                .unwrap_or(false),
        ),
    })
}

fn project_composite_curve(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let segments = feature
        .properties
        .get("Segments")?
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|source| {
            PathRef::Native(
                native_by_source
                    .get(source)
                    .map_or_else(|| source.to_string(), |id| (*id).to_string()),
            )
        })
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }
    Some(FeatureDefinition::CompositeCurve {
        segments,
        closed: feature
            .properties
            .get("Closed")
            .map_or(Some(false), |value| parse_bool(value))?,
    })
}

fn project_helix(feature: &Feature) -> Option<FeatureDefinition> {
    let axis_origin = parse_point3_mm(feature.properties.get("AxisOrigin")?)?;
    let axis_direction = parse_valid_direction(feature.properties.get("AxisDirection")?)?;
    let radius = parse_positive_length_mm(feature.parameters.get("Radius")?)?;
    let pitch = parse_length_mm(feature.parameters.get("Pitch")?)?;
    let revolutions = feature
        .parameters
        .get("Revolutions")?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)?;
    let clockwise = feature
        .properties
        .get("Clockwise")
        .and_then(|value| parse_bool(value))
        .unwrap_or(false);
    Some(FeatureDefinition::Helix {
        axis_origin,
        axis_direction,
        radius: Length(radius),
        pitch: Length(pitch),
        revolutions,
        clockwise,
        radial_growth: None,
        cone_angle: None,
        segment_turns: None,
        construction_style: None,
    })
}

fn project_native_axis_helix(feature: &Feature) -> Option<FeatureDefinition> {
    let radius = parse_positive_dimension_length_mm(feature.parameters.get("D3")?)?;
    let height = parse_dimension_length_mm(feature.parameters.get("D4")?)?;
    let revolutions = feature
        .parameters
        .get("D5")?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)?;
    let start_angle = Angle(parse_angle_rad(feature.parameters.get("D7")?)?);
    let clockwise = feature
        .properties
        .get("Clockwise")
        .and_then(|value| parse_bool(value))
        .unwrap_or(false);
    Some(FeatureDefinition::HelixNativeAxis {
        axis_native_ref: feature.id.clone(),
        radius: Length(radius),
        height: Length(height),
        revolutions,
        start_angle,
        clockwise,
    })
}

fn project_wrap(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let profile = feature.properties.get("Profile")?;
    let profile = native_by_source
        .get(profile.as_str())
        .map_or_else(|| profile.clone(), |id| (*id).to_string());
    let face = FaceSelection::Native(feature.properties.get("Face")?.clone());
    let mode = match feature
        .properties
        .get("Mode")?
        .to_ascii_lowercase()
        .as_str()
    {
        "emboss" => WrapMode::Emboss,
        "deboss" => WrapMode::Deboss,
        "scribe" => WrapMode::Scribe,
        _ => return None,
    };
    let depth = match mode {
        WrapMode::Emboss | WrapMode::Deboss => Some(Length(parse_positive_length_mm(
            feature.parameters.get("Depth")?,
        )?)),
        WrapMode::Scribe => None,
    };
    Some(FeatureDefinition::Wrap {
        profile: ProfileRef::Native(profile),
        face,
        mode,
        depth,
    })
}

fn valid_coordinate_frame(
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
) -> bool {
    let finite_origin = [origin.x, origin.y, origin.z]
        .into_iter()
        .all(f64::is_finite);
    let unit = |axis: Vector3| (axis.norm() - 1.0).abs() <= 1.0e-9;
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let cross = Vector3::new(
        x_axis.y * y_axis.z - x_axis.z * y_axis.y,
        x_axis.z * y_axis.x - x_axis.x * y_axis.z,
        x_axis.x * y_axis.y - x_axis.y * y_axis.x,
    );
    finite_origin
        && unit(x_axis)
        && unit(y_axis)
        && unit(z_axis)
        && dot(x_axis, y_axis).abs() <= 1.0e-9
        && dot(x_axis, z_axis).abs() <= 1.0e-9
        && dot(y_axis, z_axis).abs() <= 1.0e-9
        && dot(cross, z_axis) >= 1.0 - 1.0e-9
}

fn valid_direction(direction: Vector3) -> bool {
    direction.norm().is_finite() && direction.norm() > f64::EPSILON
}

fn project_fillet(feature: &Feature) -> FeatureDefinition {
    let radius = if let Some(radius) = feature
        .parameters
        .get("Radius")
        .and_then(|value| parse_positive_length_mm(value))
        .or_else(|| {
            feature
                .parameters
                .get("D1")
                .and_then(|value| parse_positive_dimension_length_mm(value))
        }) {
        RadiusSpec::Constant {
            radius: Length(radius),
        }
    } else {
        let points = feature
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
                let radius = parse_positive_length_mm(radius)?;
                (parameter.is_finite() && (0.0..=1.0).contains(&parameter)).then_some((
                    index,
                    VariableRadius {
                        parameter,
                        radius: Length(radius),
                    },
                ))
            })
            .collect::<Option<Vec<_>>>();
        points
            .and_then(|mut points| {
                points.sort_by_key(|(index, _)| *index);
                (points.len() >= 2
                    && points
                        .iter()
                        .enumerate()
                        .all(|(expected, (actual, _))| expected == *actual))
                .then_some(points)
            })
            .map_or_else(
                || RadiusSpec::Unresolved {
                    form: feature
                        .parameters
                        .keys()
                        .any(|name| indexed_name(name, "Radius"))
                        .then_some(RadiusForm::Variable)
                        .or_else(|| {
                            feature
                                .parameters
                                .keys()
                                .any(|name| matches!(name.as_str(), "Radius" | "D1"))
                                .then_some(RadiusForm::Constant)
                        }),
                },
                |points| RadiusSpec::Variable {
                    points: points.into_iter().map(|(_, point)| point).collect(),
                },
            )
    };
    FeatureDefinition::Fillet {
        edges: feature
            .properties
            .get("Edges")
            .cloned()
            .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
        radius,
    }
}

fn project_rib(feature: &Feature, native_by_source: &HashMap<&str, &str>) -> FeatureDefinition {
    let profile = feature.properties.get("Profile").map(|profile| {
        ProfileRef::Native(
            native_by_source
                .get(profile.as_str())
                .map_or_else(|| profile.clone(), |id| (*id).to_string()),
        )
    });
    let direction = feature
        .properties
        .get("Direction")
        .and_then(|value| parse_valid_direction(value));
    let draft = match feature.parameters.get("Draft") {
        Some(value) => parse_angle_rad(value)
            .map(Angle)
            .map_or(RibDraft::Unresolved, RibDraft::Angle),
        None => RibDraft::None,
    };
    FeatureDefinition::Rib {
        construction: RibConstruction {
            profile,
            direction,
            thickness: feature
                .parameters
                .get("Thickness")
                .and_then(|value| parse_positive_length_mm(value))
                .map(Length),
            side: feature
                .properties
                .get("BothSides")
                .and_then(|value| parse_bool(value))
                .map(|both_sides| {
                    if both_sides {
                        RibSide::Centered
                    } else {
                        RibSide::OneSided
                    }
                }),
            draft,
        },
        op: feature
            .properties
            .get("Operation")
            .and_then(|value| parse_boolean_op(value))
            .unwrap_or(BooleanOp::Unresolved),
    }
}

fn project_loft(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let profiles = feature.properties.get("Profiles").map_or_else(
        || Some(Vec::new()),
        |value| {
            Some(
                resolve_native_refs(value, native_by_source)?
                    .into_iter()
                    .map(ProfileRef::Native)
                    .collect::<Vec<_>>(),
            )
        },
    )?;
    let guides = feature.properties.get("Guides").map_or_else(
        || Some(Vec::new()),
        |value| resolve_native_refs(value, native_by_source),
    )?;
    Some(FeatureDefinition::Loft {
        profiles,
        guides: guides.into_iter().map(PathRef::Native).collect(),
        op: feature
            .properties
            .get("Operation")
            .and_then(|operation| parse_boolean_op(operation))
            .or_else(|| loft_op(&feature.kind))
            .unwrap_or(BooleanOp::Unresolved),
        closed: feature
            .properties
            .get("Closed")
            .map_or(Some(false), |closed| parse_bool(closed))?,
        solid: true,
        ruled: false,
        max_degree: None,
        check_compatibility: None,
        allow_multi_profile_faces: None,
    })
}

fn resolve_native_refs(value: &str, native_by_source: &HashMap<&str, &str>) -> Option<Vec<String>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(|source| {
            Some(
                native_by_source
                    .get(source)
                    .map_or_else(|| source.to_string(), |id| (*id).to_string()),
            )
        })
        .collect()
}

fn project_sweep(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let native_ref = |source: &String| {
        native_by_source
            .get(source.as_str())
            .map_or_else(|| source.clone(), |id| (*id).to_string())
    };
    let profile = feature
        .properties
        .get("Profile")
        .map(|source| ProfileRef::Native(native_ref(source)));
    let path = feature
        .properties
        .get("Path")
        .map(|source| PathRef::Native(native_ref(source)));
    let mode = if feature.xml_tag == "Surface-Sweep" || feature.kind == "Surface-Sweep" {
        SweepMode::Surface
    } else if let Some(op) = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
    {
        SweepMode::Solid { op }
    } else {
        SweepMode::Unresolved
    };
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
                .filter(|value| value.is_finite() && *value > 0.0)?,
        ),
        None => None,
    };
    Some(FeatureDefinition::Sweep {
        profile,
        sections: Vec::new(),
        path,
        mode,
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist,
        scale,
        allow_multi_profile_faces: None,
    })
}

fn pattern_form(feature: &Feature) -> Option<PatternForm> {
    let parse = |form: &str| match form.to_ascii_lowercase().as_str() {
        "linear" | "linearpattern" => Some(PatternForm::Linear),
        "circular" | "circularpattern" => Some(PatternForm::Circular),
        "crvpattern" | "curvepattern" | "curvedrivenpattern" => Some(PatternForm::CurveDriven),
        "mirror" => Some(PatternForm::Mirror),
        _ => None,
    };
    if feature_input_class(feature, NativeClassKind::LinearPattern) {
        return Some(PatternForm::Linear);
    }
    if feature_input_class(feature, NativeClassKind::CurvePattern) {
        return Some(PatternForm::CurveDriven);
    }
    if let Some(form) = parse(&feature.kind) {
        return Some(form);
    }
    if feature.xml_tag.eq_ignore_ascii_case("Mirror") {
        return Some(PatternForm::Mirror);
    }
    feature
        .xml_tag
        .eq_ignore_ascii_case("Pattern")
        .then(|| feature.properties.get("PatternType"))
        .flatten()
        .and_then(|form| parse(form))
}

fn project_pattern(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
    native_by_source: &HashMap<&str, &str>,
) -> FeatureDefinition {
    let form = pattern_form(feature);
    let seeds = match feature.properties.get("Seeds") {
        Some(seeds) => seeds
            .split(',')
            .map(str::trim)
            .map(|source| by_source.get(source).cloned())
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let resolved = form.and_then(|form| {
        Some(match form {
            PatternForm::Linear => PatternKind::Linear {
                direction: match feature.properties.get("Direction") {
                    Some(value) => Some(parse_valid_direction(value)?),
                    None => None,
                },
                spacing: Length(parse_positive_dimension_length_mm(
                    feature
                        .parameters
                        .get("Spacing")
                        .or_else(|| feature.parameters.get("D3"))?,
                )?),
                count: parse_count(
                    feature
                        .parameters
                        .get("Count")
                        .or_else(|| feature.parameters.get("D1"))?,
                )?,
            },
            PatternForm::Circular => PatternKind::Circular {
                axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
                axis_dir: parse_valid_direction(feature.properties.get("AxisDirection")?)?,
                angle: Angle(
                    feature
                        .parameters
                        .get("Angle")
                        .and_then(|value| parse_positive_angle_rad(value))?,
                ),
                count: parse_count(feature.parameters.get("Count")?)?,
            },
            PatternForm::CurveDriven => PatternKind::CurveDriven {
                path: feature.properties.get("Path").map(|source| {
                    PathRef::Native(
                        native_by_source
                            .get(source.as_str())
                            .map_or_else(|| source.clone(), |id| (*id).to_string()),
                    )
                }),
                spacing: Length(parse_positive_dimension_length_mm(
                    feature
                        .parameters
                        .get("Spacing")
                        .or_else(|| feature.parameters.get("D3"))?,
                )?),
                count: parse_count(
                    feature
                        .parameters
                        .get("Count")
                        .or_else(|| feature.parameters.get("D1"))?,
                )?,
            },
            PatternForm::Mirror => PatternKind::Mirror {
                plane_origin: parse_point3_mm(feature.properties.get("PlaneOrigin")?)?,
                plane_normal: parse_valid_direction(feature.properties.get("PlaneNormal")?)?,
            },
            PatternForm::Scale | PatternForm::Composite => return None,
        })
    });
    let seeds_required = !matches!(form, Some(PatternForm::Linear | PatternForm::CurveDriven));
    let pattern = resolved
        .filter(|_| !seeds_required || !seeds.is_empty())
        .unwrap_or(PatternKind::Unresolved { form });
    FeatureDefinition::Pattern { seeds, pattern }
}

fn parse_count(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|count| *count > 0)
}

fn project_revolve(feature: &Feature, native_by_source: &HashMap<&str, &str>) -> FeatureDefinition {
    let angle = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_positive_angle_rad(value))
            .map(Angle)
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("OneSided") => angle("Angle").map(|angle| Extent::Angle { angle }),
        Some("Symmetric") => angle("Angle").map(|angle| Extent::SymmetricAngle { angle }),
        Some("TwoSided") => angle("Angle")
            .zip(angle("Angle2"))
            .map(|(first, second)| Extent::TwoSidedAngles { first, second }),
        Some(_) => None,
    };
    let profile = feature.properties.get("Profile").and_then(|source| {
        native_by_source
            .get(source.as_str())
            .map(|id| ProfileRef::Native((*id).to_string()))
    });
    let axis = feature
        .properties
        .get("AxisOrigin")
        .and_then(|value| parse_point3_mm(value))
        .zip(
            feature
                .properties
                .get("AxisDirection")
                .and_then(|value| parse_valid_direction(value)),
        )
        .map(|(origin, direction)| RevolutionAxis { origin, direction });
    let op = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
        .unwrap_or(BooleanOp::Unresolved);
    FeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile,
            axis,
            extent,
            axis_reference: None,
            solid: Some(true),
            face_maker_class: None,
            fuse_order: None,
            allow_multi_profile_faces: None,
        },
        op,
    }
}

fn project_hole(feature: &Feature) -> FeatureDefinition {
    let diameter = feature
        .parameters
        .get("Diameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length);
    let has_counterbore = feature.parameters.contains_key("CounterboreDiameter")
        || feature.parameters.contains_key("CounterboreDepth");
    let has_countersink = feature.parameters.contains_key("CountersinkDiameter")
        || feature.parameters.contains_key("CountersinkAngle");
    let counterbore_diameter = feature
        .parameters
        .get("CounterboreDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length);
    let counterbore_depth = feature
        .parameters
        .get("CounterboreDepth")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length);
    let countersink_diameter = feature
        .parameters
        .get("CountersinkDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length);
    let countersink_angle = feature
        .parameters
        .get("CountersinkAngle")
        .and_then(|value| parse_bounded_angle_rad(value))
        .map(Angle);
    let kind = if has_counterbore && has_countersink {
        HoleKind::Unresolved {
            form: None,
            counterbore_diameter,
            counterbore_depth,
            countersink_diameter,
            countersink_angle,
        }
    } else if has_counterbore {
        match (counterbore_diameter, counterbore_depth) {
            (Some(diameter), Some(depth)) => HoleKind::Counterbore { diameter, depth },
            (diameter, depth) => HoleKind::Unresolved {
                form: Some(HoleForm::Counterbore),
                counterbore_diameter: diameter,
                counterbore_depth: depth,
                countersink_diameter: None,
                countersink_angle: None,
            },
        }
    } else if has_countersink {
        match (countersink_diameter, countersink_angle) {
            (Some(diameter), Some(angle)) => HoleKind::Countersink { diameter, angle },
            (diameter, angle) => HoleKind::Unresolved {
                form: Some(HoleForm::Countersink),
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: diameter,
                countersink_angle: angle,
            },
        }
    } else {
        HoleKind::Simple
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("Blind") => feature
            .parameters
            .get("Depth")
            .and_then(|value| parse_positive_length_mm(value))
            .map(|length| Extent::Blind {
                length: Length(length),
            }),
        Some("ThroughAll") => Some(Extent::ThroughAll),
        Some(_) => None,
    };
    FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: feature
            .properties
            .get("Face")
            .cloned()
            .map(FaceSelection::Native),
        position: match feature.properties.get("Position") {
            Some(value) => parse_point3_mm(value),
            None => None,
        },
        direction: match feature.properties.get("Direction") {
            Some(value) => parse_vector3(value).filter(|direction| valid_direction(*direction)),
            None => None,
        },
        kind,
        exit_kind: None,
        diameter,
        extent,
        bottom: None,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    }
}

fn project_shell(feature: &Feature) -> FeatureDefinition {
    let thickness = feature
        .parameters
        .get("Thickness")
        .and_then(|value| parse_positive_length_mm(value))
        .or_else(|| {
            feature
                .parameters
                .get("D1")
                .and_then(|value| parse_positive_dimension_length_mm(value))
        });
    let outward = feature
        .properties
        .get("Outward")
        .and_then(|value| parse_bool(value));
    FeatureDefinition::Shell {
        removed_faces: feature
            .properties
            .get("RemovedFaces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness: thickness.map(Length),
        outward,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    }
}

fn project_thicken(feature: &Feature) -> FeatureDefinition {
    use cadmpeg_ir::features::ThickenSide;

    let thickness = feature
        .parameters
        .get("Thickness")
        .and_then(|value| parse_positive_length_mm(value))
        .or_else(|| {
            feature
                .parameters
                .get("D1")
                .and_then(|value| parse_positive_dimension_length_mm(value))
        });
    let both_sides = feature
        .properties
        .get("BothSides")
        .map(|value| parse_bool(value));
    let reverse = feature
        .properties
        .get("Reverse")
        .map(|value| parse_bool(value));
    let side = match (both_sides, reverse) {
        (Some(Some(true)), Some(Some(true))) | (Some(None), _) | (_, Some(None)) => None,
        (Some(Some(true)), _) => Some(ThickenSide::Both),
        (_, Some(Some(true))) => Some(ThickenSide::Reverse),
        (Some(Some(false)), _) | (_, Some(Some(false))) => Some(ThickenSide::Forward),
        (None, None) => Some(ThickenSide::Forward),
    };
    FeatureDefinition::Thicken {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness: thickness.map(Length),
        side,
    }
}

fn project_offset_surface(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::OffsetSurface {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        distance: Some(Length(parse_length_mm(
            feature.parameters.get("Distance")?,
        )?)),
    })
}

fn project_knit_surface(feature: &Feature) -> Option<FeatureDefinition> {
    let gap_tolerance = match feature.parameters.get("GapTolerance") {
        Some(value) => Some(parse_length_mm(value).filter(|value| *value >= 0.0)?),
        None => None,
    };
    Some(FeatureDefinition::KnitSurface {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        merge_entities: feature
            .properties
            .get("MergeEntities")
            .and_then(|value| parse_bool(value))
            .unwrap_or(true),
        create_solid: feature
            .properties
            .get("CreateSolid")
            .and_then(|value| parse_bool(value))
            .unwrap_or(false),
        gap_tolerance: gap_tolerance.map(Length),
    })
}

fn project_filled_surface(feature: &Feature) -> Option<FeatureDefinition> {
    let continuity = match feature
        .properties
        .get("Continuity")?
        .to_ascii_lowercase()
        .as_str()
    {
        "contact" => SurfaceContinuity::Contact,
        "tangent" => SurfaceContinuity::Tangent,
        "curvature" => SurfaceContinuity::Curvature,
        _ => return None,
    };
    Some(FeatureDefinition::FilledSurface {
        boundary: EdgeSelection::Native(feature.properties.get("Boundary")?.clone()),
        support_faces: FaceSelection::Native(feature.properties.get("SupportFaces")?.clone()),
        continuity,
        merge_result: feature
            .properties
            .get("MergeResult")
            .and_then(|value| parse_bool(value))
            .unwrap_or(false),
    })
}

fn project_trim_surface(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let tool = feature.properties.get("Tool")?;
    let tool = native_by_source
        .get(tool.as_str())
        .map_or_else(|| tool.clone(), |id| (*id).to_string());
    let keep = match feature
        .properties
        .get("Keep")?
        .to_ascii_lowercase()
        .as_str()
    {
        "inside" => TrimRegion::Inside,
        "outside" => TrimRegion::Outside,
        _ => return None,
    };
    Some(FeatureDefinition::TrimSurface {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        tool: PathRef::Native(tool),
        keep,
    })
}

fn project_extend_surface(feature: &Feature) -> Option<FeatureDefinition> {
    let method = match feature
        .properties
        .get("Method")?
        .to_ascii_lowercase()
        .as_str()
    {
        "natural" => SurfaceExtension::Natural,
        "linear" => SurfaceExtension::Linear,
        _ => return None,
    };
    Some(FeatureDefinition::ExtendSurface {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        distance: Some(Length(parse_positive_length_mm(
            feature.parameters.get("Distance")?,
        )?)),
        method,
    })
}

fn project_ruled_surface(feature: &Feature) -> Option<FeatureDefinition> {
    let distance = Length(parse_positive_length_mm(
        feature.parameters.get("Distance")?,
    )?);
    let mode = match feature
        .properties
        .get("Mode")?
        .to_ascii_lowercase()
        .as_str()
    {
        "normal" => RuledSurfaceMode::Normal { distance },
        "tangent" => RuledSurfaceMode::Tangent { distance },
        "direction" => RuledSurfaceMode::Direction {
            direction: parse_valid_direction(feature.properties.get("Direction")?)?,
            distance,
        },
        _ => return None,
    };
    Some(FeatureDefinition::RuledSurface {
        edges: EdgeSelection::Native(feature.properties.get("Edges")?.clone()),
        support_faces: FaceSelection::Native(feature.properties.get("SupportFaces")?.clone()),
        mode,
    })
}

fn project_draft(feature: &Feature) -> Option<FeatureDefinition> {
    let pull_direction = parse_vector3(feature.properties.get("Direction")?)?;
    if !(pull_direction.norm().is_finite() && pull_direction.norm() > 0.0) {
        return None;
    }
    Some(FeatureDefinition::Draft {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        neutral_plane: feature
            .properties
            .get("NeutralPlane")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
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

fn body_retention_mode(feature: &Feature) -> Option<BodyRetentionMode> {
    let value = feature
        .properties
        .get("Mode")
        .map_or(feature.kind.as_str(), String::as_str);
    match value.to_ascii_lowercase().as_str() {
        "delete" | "deletebody" => Some(BodyRetentionMode::DeleteSelected),
        "keep" | "keepbody" => Some(BodyRetentionMode::KeepSelected),
        _ if feature.xml_tag.eq_ignore_ascii_case("DeleteBody") => {
            Some(BodyRetentionMode::DeleteSelected)
        }
        _ if feature.xml_tag.eq_ignore_ascii_case("KeepBody") => {
            Some(BodyRetentionMode::KeepSelected)
        }
        _ if feature.kind.trim().eq_ignore_ascii_case("Body-Delete/Keep") => {
            Some(BodyRetentionMode::Unresolved)
        }
        _ => None,
    }
}

fn project_cut_with_surface(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::CutWithSurface {
        targets: BodySelection::Native(feature.properties.get("Targets")?.clone()),
        tools: FaceSelection::Native(feature.properties.get("Tools")?.clone()),
        reverse: feature
            .properties
            .get("Reverse")
            .and_then(|value| parse_bool(value))
            .unwrap_or(false),
    })
}

fn project_delete_body(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::DeleteBody {
        bodies: feature
            .properties
            .get("Bodies")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        mode: body_retention_mode(feature)?,
    })
}

fn project_delete_face(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::DeleteFace {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        heal: parse_bool(feature.properties.get("Heal")?)?,
    })
}

fn project_replace_face(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::ReplaceFace {
        targets: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        replacements: FaceSelection::Native(feature.properties.get("ReplacementFaces")?.clone()),
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
            direction: parse_valid_direction(feature.properties.get("Direction")?)?,
            distance: Length(
                feature
                    .parameters
                    .get("Distance")
                    .and_then(|value| parse_length_mm(value))?,
            ),
        },
        "rotate" => FaceMotion::Rotate {
            axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
            axis_dir: parse_valid_direction(feature.properties.get("AxisDirection")?)?,
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

fn project_move_body(feature: &Feature) -> Option<FeatureDefinition> {
    let bodies = BodySelection::Native(feature.properties.get("Bodies")?.clone());
    let translation = parse_point3_mm(feature.properties.get("Translation")?)?;
    let translation = Vector3::new(translation.x, translation.y, translation.z);
    let rotation = match feature.parameters.get("Rotation") {
        Some(angle) => Some(AxisAngle {
            origin: parse_point3_mm(feature.properties.get("RotationOrigin")?)?,
            direction: parse_valid_direction(feature.properties.get("RotationAxis")?)?,
            angle: Angle(parse_angle_rad(angle)?),
        }),
        None => None,
    };
    let copies = feature
        .properties
        .get("Copies")
        .map_or(Some(0), |value| value.trim().parse::<u32>().ok())?;
    Some(FeatureDefinition::MoveBody {
        bodies,
        translation,
        rotation,
        copies,
    })
}

fn project_dome(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Dome {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        height: feature
            .parameters
            .get("Height")
            .and_then(|value| parse_positive_length_mm(value))
            .map(Length),
        elliptical: feature
            .properties
            .get("Elliptical")
            .and_then(|value| parse_bool(value)),
        reverse: feature
            .properties
            .get("Reverse")
            .and_then(|value| parse_bool(value)),
    }
}

fn project_flex(feature: &Feature) -> FeatureDefinition {
    let axis = feature
        .properties
        .get("Axis")
        .or_else(|| feature.properties.get("AxisDirection"))
        .and_then(|value| parse_valid_direction(value));
    let angle = feature
        .parameters
        .get("Angle")
        .and_then(|value| parse_angle_rad(value))
        .filter(|value| value.is_finite())
        .map(Angle);
    let factor = feature
        .parameters
        .get("Factor")
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0);
    let distance = feature
        .parameters
        .get("Distance")
        .and_then(|value| parse_length_mm(value))
        .filter(|value| value.is_finite())
        .map(Length);
    let form = feature.properties.get("Mode").and_then(|value| {
        match value.to_ascii_lowercase().as_str() {
            "bending" | "bend" => Some(FlexForm::Bending),
            "twisting" | "twist" => Some(FlexForm::Twisting),
            "tapering" | "taper" => Some(FlexForm::Tapering),
            "stretching" | "stretch" => Some(FlexForm::Stretching),
            _ => None,
        }
    });
    let mode = match form {
        Some(FlexForm::Bending) if angle.is_some() => FlexMode::Bending {
            angle: angle.expect("guarded above"),
        },
        Some(FlexForm::Twisting) if angle.is_some() => FlexMode::Twisting {
            angle: angle.expect("guarded above"),
        },
        Some(FlexForm::Tapering) if factor.is_some() => FlexMode::Tapering {
            factor: factor.expect("guarded above"),
        },
        Some(FlexForm::Stretching) if distance.is_some() => FlexMode::Stretching {
            distance: distance.expect("guarded above"),
        },
        _ => FlexMode::Unresolved {
            form,
            angle,
            factor,
            distance,
        },
    };
    FeatureDefinition::Flex { axis, mode }
}

fn project_scale(feature: &Feature) -> FeatureDefinition {
    let center = match feature.properties.get("CenterType").map(String::as_str) {
        None | Some("Point") => feature
            .properties
            .get("Center")
            .and_then(|value| parse_point3_mm(value))
            .map(ScaleCenter::Point),
        Some("Centroid") => Some(ScaleCenter::Centroid),
        Some("Origin" | "ModelOrigin") => Some(ScaleCenter::ModelOrigin),
        Some("Reference" | "CoordinateSystem") => feature
            .properties
            .get("CenterRef")
            .filter(|value| !value.is_empty())
            .cloned()
            .map(ScaleCenter::Native),
        Some(_) => None,
    };
    let factor = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value != 0.0)
    };
    FeatureDefinition::Scale {
        bodies: feature
            .properties
            .get("Bodies")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        center,
        factors: ScaleFactors {
            uniform: factor("Factor"),
            x: factor("ScaleX"),
            y: factor("ScaleY"),
            z: factor("ScaleZ"),
        },
    }
}

fn project_chamfer(feature: &Feature) -> FeatureDefinition {
    let length = |name, positional| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_positive_length_mm(value))
            .or_else(|| {
                feature
                    .parameters
                    .get(positional)
                    .and_then(|value| parse_positive_dimension_length_mm(value))
            })
            .map(Length)
    };
    let positional_angle = feature
        .parameters
        .get("D2")
        .filter(|value| parse_bounded_angle_rad(value).is_some());
    let spec = (|| {
        Some(
            if let Some(value) = feature.parameters.get("Angle").or(positional_angle) {
                ChamferSpec::DistanceAngle {
                    distance: length("Distance", "D1")?,
                    angle: Angle(parse_bounded_angle_rad(value)?),
                }
            } else if let (Some(first), Some(second)) =
                (length("Distance1", "D1"), length("Distance2", "D2"))
            {
                ChamferSpec::TwoDistances { first, second }
            } else {
                ChamferSpec::Distance {
                    distance: length("Distance", "D1")?,
                }
            },
        )
    })()
    .unwrap_or_else(|| ChamferSpec::Unresolved {
        form: if feature.parameters.contains_key("Angle") {
            Some(ChamferForm::DistanceAngle)
        } else if feature.parameters.contains_key("Distance1")
            || feature.parameters.contains_key("Distance2")
        {
            Some(ChamferForm::TwoDistances)
        } else if feature.parameters.contains_key("Distance")
            || (feature.parameters.contains_key("D1") && !feature.parameters.contains_key("D2"))
        {
            Some(ChamferForm::Distance)
        } else {
            None
        },
    });
    FeatureDefinition::Chamfer {
        edges: feature
            .properties
            .get("Edges")
            .cloned()
            .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
        spec,
        flip_direction: Some(false),
    }
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
    let (value, display_length) = value
        .strip_prefix(['R', 'r', '\u{2300}', '\u{00d8}'])
        .map_or((value, false), |value| (value.trim(), true));
    for (suffix, scale) in [("mm", 1.0), ("cm", 10.0), ("in", 25.4), ("m", 1000.0)] {
        if let Some(number) = value.strip_suffix(suffix) {
            return number
                .trim()
                .parse::<f64>()
                .ok()
                .map(|value| value * scale)
                .filter(|value| value.is_finite());
        }
    }
    display_length
        .then(|| value.parse::<f64>().ok())
        .flatten()
        .filter(|value| value.is_finite())
}

fn parse_positive_length_mm(value: &str) -> Option<f64> {
    parse_length_mm(value).filter(|value| *value > 0.0)
}

fn parse_positive_dimension_length_mm(value: &str) -> Option<f64> {
    parse_positive_length_mm(value).or_else(|| {
        value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
    })
}

fn parse_dimension_length_mm(value: &str) -> Option<f64> {
    parse_length_mm(value).or_else(|| {
        value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    })
}

fn format_length_like(value: f64, previous: Option<&str>) -> String {
    let previous = previous.map(str::trim).unwrap_or_default();
    if previous.starts_with(['R', 'r']) {
        format!("R{value}")
    } else if previous.starts_with(['\u{2300}', '\u{00d8}']) {
        format!("\u{2300}{value}")
    } else if previous.parse::<f64>().is_ok() {
        value.to_string()
    } else {
        format_length_mm(value)
    }
}

fn format_angle_like(value: f64, previous: Option<&str>) -> String {
    if previous
        .map(str::trim)
        .is_some_and(|value| value.ends_with('\u{00b0}'))
    {
        let degrees = value.to_degrees();
        let rounded = degrees.round();
        let degrees = if (degrees - rounded).abs() <= 1.0e-12 {
            rounded
        } else {
            degrees
        };
        format!("{degrees}\u{00b0}")
    } else {
        format_angle_rad(value)
    }
}

pub(crate) fn format_length_mm(value: f64) -> String {
    format!("{value}mm")
}

fn parse_angle_rad(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Some(number) = value
        .strip_suffix("deg")
        .or_else(|| value.strip_suffix('\u{00b0}'))
    {
        return number
            .trim()
            .parse::<f64>()
            .ok()
            .map(f64::to_radians)
            .filter(|value| value.is_finite());
    }
    value
        .strip_suffix("rad")
        .and_then(|number| number.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn parse_positive_angle_rad(value: &str) -> Option<f64> {
    parse_angle_rad(value).filter(|value| *value > 0.0)
}

fn parse_bounded_angle_rad(value: &str) -> Option<f64> {
    parse_positive_angle_rad(value).filter(|value| *value < std::f64::consts::PI)
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

fn parse_valid_direction(value: &str) -> Option<Vector3> {
    parse_vector3(value).filter(|value| valid_direction(*value))
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

fn face_selection_value(selection: &FaceSelection) -> Option<String> {
    match selection {
        FaceSelection::Native(native) | FaceSelection::Resolved { native, .. }
            if !native.trim().is_empty() =>
        {
            Some(native.clone())
        }
        FaceSelection::Faces(faces) if !faces.is_empty() => Some(
            faces
                .iter()
                .map(|face| face.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => None,
    }
}

fn edge_selection_value(selection: &EdgeSelection) -> Option<String> {
    match selection {
        EdgeSelection::Native(native) | EdgeSelection::Resolved { native, .. }
            if !native.trim().is_empty() =>
        {
            Some(native.clone())
        }
        EdgeSelection::Edges(edges) if !edges.is_empty() => Some(
            edges
                .iter()
                .map(|edge| edge.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => None,
    }
}

fn body_selection_value(selection: &BodySelection) -> Option<String> {
    match selection {
        BodySelection::Native(native) | BodySelection::Resolved { native, .. }
            if !native.trim().is_empty() =>
        {
            Some(native.clone())
        }
        BodySelection::Bodies(bodies) if !bodies.is_empty() => Some(
            bodies
                .iter()
                .map(|body| body.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => None,
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

fn format_boolean_op(value: BooleanOp) -> Option<&'static str> {
    Some(match value {
        BooleanOp::Unresolved => return None,
        BooleanOp::Join => "Join",
        BooleanOp::Cut => "Cut",
        BooleanOp::Intersect => "Intersect",
        BooleanOp::NewBody => "NewBody",
    })
}

fn resolved_boolean_op(value: BooleanOp, feature: &FeatureId) -> Result<&'static str, CodecError> {
    format_boolean_op(value).ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "SLDPRT feature {feature} has an unresolved boolean operation"
        ))
    })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "True" => Some(true),
        "0" | "false" | "False" => Some(false),
        _ => None,
    }
}

fn parse_parameter_literal(expression: &str) -> Option<ParameterValue> {
    if dimension_display(expression).is_some() {
        return parse_dimension_display_length(expression)
            .map(|value| ParameterValue::Length(Length(value)));
    }
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

fn dimension_display(expression: &str) -> Option<DimensionDisplay> {
    let expression = expression.trim();
    if expression.starts_with("<MOD-DIAM>")
        || (expression.starts_with(['⌀', 'Ø']) && parse_length_mm(expression).is_some())
    {
        Some(DimensionDisplay::Diameter)
    } else if expression.starts_with(['R', 'r']) && parse_length_mm(expression).is_some() {
        Some(DimensionDisplay::Radius)
    } else {
        None
    }
}

fn parse_dimension_display_length(expression: &str) -> Option<f64> {
    let value = expression
        .trim()
        .strip_prefix("<MOD-DIAM>")
        .unwrap_or(expression)
        .trim();
    parse_dimension_length_mm(value).or_else(|| parse_length_mm(expression))
}

fn parse_neutral_parameter_literal(
    feature: &cadmpeg_ir::features::Feature,
    name: &str,
    expression: &str,
) -> Option<ParameterValue> {
    let positional_length = match name {
        "D1" => matches!(
            feature.definition,
            FeatureDefinition::Extrude { .. }
                | FeatureDefinition::Fillet { .. }
                | FeatureDefinition::Chamfer { .. }
                | FeatureDefinition::Shell { .. }
                | FeatureDefinition::Thicken { .. }
                | FeatureDefinition::DatumOffsetPlane { .. }
        ),
        "D2" => matches!(
            feature.definition,
            FeatureDefinition::Chamfer {
                spec: ChamferSpec::TwoDistances { .. },
                ..
            }
        ),
        "D3" => matches!(
            feature.definition,
            FeatureDefinition::Pattern {
                pattern: PatternKind::Linear { .. } | PatternKind::CurveDriven { .. },
                ..
            }
        ),
        _ => false,
    };
    if positional_length {
        return parse_positive_dimension_length_mm(expression)
            .map(|value| ParameterValue::Length(Length(value)));
    }
    parse_parameter_literal(expression)
}

fn format_parameter_value(value: &ParameterValue) -> String {
    match value {
        ParameterValue::Length(Length(value)) => format_length_mm(*value),
        ParameterValue::Angle(Angle(value)) => format_angle_rad(*value),
        ParameterValue::Real(value) => value.to_string(),
        ParameterValue::Integer(value) => value.to_string(),
        ParameterValue::Boolean(value) => value.to_string(),
    }
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

/// Stable hash of native feature parameters, properties, and ordering.
pub fn native_parameter_hash(histories: &[FeatureHistory]) -> String {
    let mut parameters = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| {
            (
                feature.id.clone(),
                feature.parameters.clone(),
                feature.dimension_properties.clone(),
                feature
                    .content
                    .iter()
                    .filter_map(|item| match item {
                        FeatureContent::Dimension(name) => Some(name.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            )
        })
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
        return sync_neutral_features(
            &ir.model.features,
            &ir.model.parameters,
            &ir.model.bodies,
            native,
        );
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
        (true, false) => sync_neutral_features(
            &ir.model.features,
            &ir.model.parameters,
            &ir.model.bodies,
            native,
        ),
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
    let mut parameters = ir.model.parameters.clone();
    let feature_names = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .name
                .as_ref()
                .map(|name| (feature.id.clone(), name.clone()))
        })
        .collect::<HashMap<_, _>>();
    if let Some(native) = native.as_ref() {
        let original = project_parameters(&native.feature_histories);
        rewrite_renamed_parameter_references(&mut parameters, &original, &feature_names);
    }
    let mut projected_dependencies = parameters.clone();
    populate_parameter_dependencies(&mut projected_dependencies, &feature_names);
    if ir
        .model
        .parameters
        .iter()
        .zip(&projected_dependencies)
        .any(|(actual, projected)| actual.dependencies != projected.dependencies)
    {
        return Err(CodecError::Malformed(
            "SLDPRT parameter dependencies are inconsistent with their expressions".into(),
        ));
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    let mut desired = HashMap::<FeatureId, Vec<&DesignParameter>>::new();
    for parameter in &parameters {
        let Some(owner) = features.get(&parameter.owner) else {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} references a missing feature",
                parameter.id.0
            )));
        };
        if parameter.display != dimension_display(&parameter.expression) {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} has display semantics inconsistent with its expression",
                parameter.id.0
            )));
        }
        if parse_neutral_parameter_literal(owner, &parameter.name, &parameter.expression)
            .is_some_and(|literal| parameter.value.as_ref() != Some(&literal))
        {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} has a value inconsistent with its expression",
                parameter.id.0
            )));
        }
        let owner_parameters = desired.entry(parameter.owner.clone()).or_default();
        if owner_parameters
            .iter()
            .any(|candidate| candidate.name == parameter.name)
        {
            return Err(CodecError::Malformed(format!(
                "duplicate SLDPRT parameter {} on feature {}",
                parameter.name, parameter.owner
            )));
        }
        if owner_parameters
            .iter()
            .any(|candidate| candidate.ordinal == parameter.ordinal)
        {
            return Err(CodecError::Malformed(format!(
                "duplicate SLDPRT parameter ordinal {} on feature {}",
                parameter.ordinal, parameter.owner
            )));
        }
        owner_parameters.push(parameter);
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
        let mut parameters = desired.remove(feature_id).unwrap_or_default();
        parameters.sort_by_key(|parameter| parameter.ordinal);
        record.parameters = parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.expression.clone()))
            .collect();
        record.dimension_properties = parameters
            .iter()
            .map(|parameter| {
                let mut properties = parameter.properties.clone();
                if parse_parameter_literal(&parameter.expression).is_none() {
                    if let Some(value) = &parameter.value {
                        properties.insert("Value".into(), format_parameter_value(value));
                    } else {
                        properties.remove("Value");
                    }
                }
                (parameter.name.clone(), properties)
            })
            .collect();
        let mut names = parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect::<Vec<_>>()
            .into_iter();
        let mut content = record
            .content
            .iter()
            .filter_map(|item| match item {
                FeatureContent::Dimension(_) => names.next().map(FeatureContent::Dimension),
                other => Some(other.clone()),
            })
            .collect::<Vec<_>>();
        content.extend(names.map(FeatureContent::Dimension));
        record.content = content;
    }
    for parameter in &parameters {
        let Some(native_ref) = parameter.native_ref.as_deref() else {
            continue;
        };
        let location = native
            .feature_input_lanes
            .iter()
            .enumerate()
            .find_map(|(lane_index, lane)| {
                lane.scalars
                    .iter()
                    .position(|scalar| scalar.id == native_ref)
                    .map(|scalar_index| (lane_index, scalar_index))
            })
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT parameter {} references missing scalar {native_ref}",
                    parameter.id.0
                ))
            })?;
        let lane = &mut native.feature_input_lanes[location.0];
        let scalar = &mut lane.scalars[location.1];
        if scalar.role == crate::records::FeatureInputScalarRole::Display {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} references a display scalar",
                parameter.id.0
            )));
        }
        let value = match parameter.value {
            Some(ParameterValue::Length(length)) => length.0 / 1000.0,
            Some(ParameterValue::Angle(angle)) => angle.0,
            Some(ParameterValue::Real(value)) => value,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT scalar {} requires a real-valued parameter",
                    scalar.id
                )));
            }
        };
        let offset = usize::try_from(scalar.offset).map_err(|_| {
            CodecError::Malformed("SLDPRT scalar offset exceeds address space".into())
        })?;
        let bytes = lane
            .native_payload
            .get_mut(offset..offset + 8)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT scalar {} lies outside its payload",
                    scalar.id
                ))
            })?;
        bytes.copy_from_slice(&value.to_le_bytes());
        scalar.value = value;
    }
    Ok(())
}

fn rewrite_renamed_parameter_references(
    parameters: &mut [DesignParameter],
    original: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
) {
    let original = original
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let desired = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let mut replacements = HashMap::<ParameterId, HashMap<String, String>>::new();
    for (id, parameter) in &desired {
        let Some(previous) = original.get(id) else {
            continue;
        };
        let mut aliases = HashMap::new();
        if previous.name != parameter.name {
            aliases.insert(previous.name.clone(), parameter.name.clone());
            if let Some(owner_name) = feature_names.get(&parameter.owner) {
                aliases.insert(
                    format!("{}@{owner_name}", previous.name),
                    format!("{}@{owner_name}", parameter.name),
                );
            }
        }
        if let Some(previous_id) = previous.properties.get("EquationId") {
            let replacement = parameter
                .properties
                .get("EquationId")
                .unwrap_or(&parameter.name);
            if previous_id != replacement {
                aliases.insert(previous_id.clone(), replacement.clone());
            }
        }
        if !aliases.is_empty() {
            replacements.insert((*id).clone(), aliases);
        }
    }
    for parameter in parameters {
        let aliases = parameter
            .dependencies
            .iter()
            .filter_map(|dependency| replacements.get(dependency))
            .flat_map(|aliases| aliases.iter())
            .map(|(alias, replacement)| (alias.as_str(), replacement.as_str()))
            .collect::<HashMap<_, _>>();
        if aliases.is_empty() {
            continue;
        }
        let tokens = expression_identifier_tokens(&parameter.expression);
        let mut rewritten = String::with_capacity(parameter.expression.len());
        let mut copied = 0;
        for token in tokens {
            let Some(replacement) = aliases.get(token.value.as_str()) else {
                continue;
            };
            rewritten.push_str(&parameter.expression[copied..token.start]);
            if token.quoted || !unquoted_expression_identifier(replacement) {
                rewritten.push('"');
                rewritten.push_str(&replacement.replace('"', "\"\""));
                rewritten.push('"');
            } else {
                rewritten.push_str(replacement);
            }
            copied = token.end;
        }
        if copied != 0 {
            rewritten.push_str(&parameter.expression[copied..]);
            parameter.expression = rewritten;
        }
    }
}

fn unquoted_expression_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.' | '-')
        })
}

fn sync_neutral_configurations(
    configurations: &[DesignConfiguration],
    native: &mut Option<crate::native::SldprtNative>,
) {
    if configurations.is_empty() {
        if let Some(native) = native {
            for history in &mut native.feature_histories {
                history.configurations.clear();
            }
        }
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
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: Vec::new(),
        });
    }
    let mut configurations = configurations.iter().collect::<Vec<_>>();
    configurations.sort_by_key(|configuration| configuration.ordinal);
    let desired_ids = configurations
        .iter()
        .map(|configuration| {
            configuration
                .native_ref
                .clone()
                .unwrap_or_else(|| format!("sldprt:generated:configuration#{}", configuration.id.0))
        })
        .collect::<std::collections::HashSet<_>>();
    for history in &mut native.feature_histories {
        history
            .configurations
            .retain(|configuration| desired_ids.contains(&configuration.id));
    }
    for configuration in configurations {
        let existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.configurations)
            .find(|candidate| configuration.native_ref.as_deref() == Some(candidate.id.as_str()));
        if let Some(existing) = existing {
            let previous_index = existing.source_index;
            existing.ordinal = configuration.ordinal;
            existing.source_index = configuration.source_index;
            existing.name.clone_from(&configuration.name);
            existing.material.clone_from(&configuration.material);
            existing.properties.clone_from(&configuration.properties);
            if previous_index != configuration.source_index {
                for lane in &mut native.feature_input_lanes {
                    if lane.configuration.as_deref()
                        == previous_index.as_ref().map(ToString::to_string).as_deref()
                    {
                        lane.configuration =
                            configuration.source_index.map(|index| index.to_string());
                    }
                }
            }
        } else {
            let parent = native.feature_histories[0].id.clone();
            native.feature_histories[0]
                .configurations
                .push(Configuration {
                    id: configuration.native_ref.clone().unwrap_or_else(|| {
                        format!("sldprt:generated:configuration#{}", configuration.id.0)
                    }),
                    parent,
                    ordinal: configuration.ordinal,
                    source_index: configuration.source_index,
                    name: configuration.name.clone(),
                    material: configuration.material.clone(),
                    properties: configuration.properties.clone(),
                });
        }
    }
    for history in &mut native.feature_histories {
        history
            .configurations
            .sort_by_key(|configuration| configuration.ordinal);
    }
    synchronize_history_content_order(native);
}

fn synchronize_feature_input_names(
    features: &[cadmpeg_ir::features::Feature],
    native: &mut crate::native::SldprtNative,
) -> Result<(), CodecError> {
    let renames = features
        .iter()
        .filter_map(|feature| {
            let record = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|record| feature.native_ref.as_deref() == Some(record.id.as_str()))?;
            let new_name = feature.name.as_deref().unwrap_or_default();
            if new_name == record.name {
                return None;
            }
            Some((
                record.name.clone(),
                new_name.to_string(),
                record.input_class.clone()?,
            ))
        })
        .collect::<Vec<_>>();

    for (old_name, new_name, input_class) in renames {
        let mut matches = Vec::<(usize, usize)>::new();
        for (lane_index, lane) in native.feature_input_lanes.iter().enumerate() {
            for class in lane
                .classes
                .iter()
                .filter(|class| class.name == input_class)
            {
                let name_offset = class.offset + 6 + class.name.len() as u64;
                if let Some((name_index, _)) = lane
                    .names
                    .iter()
                    .enumerate()
                    .find(|(_, name)| name.offset == name_offset && name.value == old_name)
                {
                    matches.push((lane_index, name_index));
                }
            }
        }
        let [(lane_index, name_index)] = matches.as_slice() else {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature-input name for {old_name:?} is not uniquely linked"
            )));
        };
        native.feature_input_lanes[*lane_index].names[*name_index].value = new_name;
    }
    Ok(())
}

/// Apply neutral native-feature edits to the `SolidWorks` history used for writing.
pub fn sync_neutral_features(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[DesignParameter],
    bodies: &[Body],
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    if features.is_empty() {
        if let Some(native) = native {
            for history in &mut native.feature_histories {
                history.features.clear();
            }
        }
        return Ok(());
    }
    if native.is_none() {
        *native = Some(crate::native::SldprtNative {
            version: crate::native::SLDPRT_NATIVE_VERSION,
            feature_histories: vec![FeatureHistory {
                id: "sldprt:generated:feature-history#0".into(),
                part_name: None,
                properties: BTreeMap::new(),
                content: Vec::new(),
                configurations: Vec::new(),
                features: Vec::new(),
            }],
            feature_input_lanes: Vec::new(),
            pmi_dimensions: Vec::new(),
        });
    }
    let native = native.as_mut().expect("initialized above");
    if native.feature_histories.is_empty() {
        native.feature_histories.push(FeatureHistory {
            id: "sldprt:generated:feature-history#0".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: Vec::new(),
        });
    }

    synchronize_feature_input_names(features, native)?;

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
    let desired_record_ids = record_ids
        .values()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    for history in &mut native.feature_histories {
        history
            .features
            .retain(|feature| desired_record_ids.contains(&feature.id));
    }
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
                ..
            } => parent_sources
                .get(&feature.id)
                .map(|source| (sketch.clone(), source.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let body_sources = bodies
        .iter()
        .map(|body| (body.id.clone(), body.id.0.clone()))
        .collect::<HashMap<_, _>>();

    for feature in features {
        if feature
            .source_tag
            .as_deref()
            .is_some_and(|tag| !valid_xml_name(tag))
        {
            return Err(CodecError::Malformed(format!(
                "SLDPRT feature {} has an invalid source tag",
                feature.id
            )));
        }
        let mut existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()));
        let suppressed = feature
            .suppressed
            .or_else(|| existing.as_deref().map(|record| record.suppressed))
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT writing requires resolved suppression for feature {}",
                    feature.id
                ))
            })?;
        let (kind, parameters, mut properties) = match &feature.definition {
            FeatureDefinition::TreeNode {
                role,
                children,
                active_child,
            } => {
                if !children.is_empty() || active_child.is_some() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses explicit tree membership",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| feature_tree_node_role(record) != Some(*role))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes feature-tree node role",
                        feature.id
                    )));
                }
                (
                    existing.as_deref().map_or_else(
                        || feature_tree_node_kind(*role).into(),
                        |record| record.kind.clone(),
                    ),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    feature.source_properties.clone(),
                )
            }
            FeatureDefinition::Native {
                kind,
                parameters,
                properties,
            } => {
                let mut merged = feature.source_properties.clone();
                merged.extend(properties.clone());
                (kind.clone(), parameters.clone(), merged)
            }
            FeatureDefinition::Block { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} cannot encode a neutral block primitive",
                    feature.id
                )));
            }
            FeatureDefinition::StoredGeometry => (
                existing
                    .as_deref()
                    .map_or_else(|| "Feature".into(), |record| record.kind.clone()),
                existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                feature.source_properties.clone(),
            ),
            FeatureDefinition::ExtractBody { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported body-extraction semantics",
                    feature.id
                )));
            }
            FeatureDefinition::DerivedGeometry { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported copied-geometry semantics",
                    feature.id
                )));
            }
            FeatureDefinition::LoftUnresolved => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved loft semantics",
                    feature.id
                )));
            }
            FeatureDefinition::FreeformSurfaceUnresolved => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved freeform-surface semantics",
                    feature.id
                )));
            }
            FeatureDefinition::DraftUnresolved => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved draft semantics",
                    feature.id
                )));
            }
            FeatureDefinition::ImportedGeometry { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported external-import semantics",
                    feature.id
                )));
            }
            FeatureDefinition::Primitive { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported analytic-primitive semantics",
                    feature.id
                )));
            }
            FeatureDefinition::DatumPrincipalPlane { plane } => {
                let record = existing.as_deref().ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained principal-plane record",
                        feature.id
                    ))
                })?;
                if principal_plane(record) != Some(*plane) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes its principal-plane role",
                        feature.id
                    )));
                }
                (
                    record.kind.clone(),
                    record.parameters.clone(),
                    feature.source_properties.clone(),
                )
            }
            FeatureDefinition::SpatialSketch { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes unsupported spatial-sketch semantics",
                    feature.id
                )));
            }
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
                    .is_some_and(|record| !feature_family(record, "ReferencePlane"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
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
            FeatureDefinition::DatumPlaneUnresolved => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved reference-plane construction",
                    feature.id
                )));
            }
            FeatureDefinition::DatumPointUnresolved => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved reference-point construction",
                    feature.id
                )));
            }
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } => {
                if !distance.0.is_finite() {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a non-finite reference-plane offset",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !is_offset_plane(record))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                match reference {
                    Some(reference) => {
                        let source = parent_sources.get(reference).ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} references a missing datum plane",
                                feature.id
                            ))
                        })?;
                        let key = if properties.contains_key("Plane")
                            && !properties.contains_key("Reference")
                        {
                            "Plane"
                        } else {
                            "Reference"
                        };
                        properties.insert(key.into(), source.clone());
                    }
                    None if existing.is_some() => {}
                    None => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has an unresolved datum-plane reference",
                            feature.id
                        )));
                    }
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert(
                    "D1".into(),
                    format_length_like(
                        distance.0,
                        existing
                            .as_deref()
                            .and_then(|record| record.parameters.get("D1"))
                            .map(String::as_str),
                    ),
                );
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Plane".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::TrimSurface { faces, tool, keep } => {
                let resolved_faces = face_selection_value(faces);
                if resolved_faces.is_none() {
                    if matches!(faces, FaceSelection::Unresolved) {
                        if existing.is_none() {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has unresolved trim-surface input faces",
                                feature.id
                            )));
                        }
                    } else {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has no trim-surface input faces",
                            feature.id
                        )));
                    }
                }
                let resolved_tool = path_source(tool, &record_sources, &sketch_sources);
                if resolved_tool.is_none() {
                    if matches!(tool, PathRef::Unresolved) {
                        if existing.is_none() {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has an unresolved trim path",
                                feature.id
                            )));
                        }
                    } else {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing trim path",
                            feature.id
                        )));
                    }
                }
                if matches!(keep, TrimRegion::Unresolved) && existing.is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved retained trim region",
                        feature.id
                    )));
                }
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "TrimSurface") && !feature_family(record, "SurfaceTrim")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = existing
                    .as_deref()
                    .map(|record| record.properties.clone())
                    .unwrap_or_default();
                properties.extend(feature.source_properties.clone());
                if let Some(faces) = resolved_faces {
                    properties.insert("Faces".into(), faces);
                }
                if let Some(tool) = resolved_tool {
                    properties.insert("Tool".into(), tool);
                }
                if let Some(keep) = match keep {
                    TrimRegion::Unresolved => None,
                    TrimRegion::Inside => Some("Inside"),
                    TrimRegion::Outside => Some("Outside"),
                } {
                    properties.insert("Keep".into(), keep.into());
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "TrimSurface".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::ExtendSurface {
                faces,
                distance,
                method,
            } => {
                let resolved_faces = face_selection_value(faces);
                if resolved_faces.is_none() {
                    if matches!(faces, FaceSelection::Unresolved) {
                        if existing.is_none() {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has unresolved extend-surface input faces",
                                feature.id
                            )));
                        }
                    } else {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has no extend-surface input faces",
                            feature.id
                        )));
                    }
                }
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "ExtendSurface")
                        && !feature_family(record, "SurfaceExtend")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                if let Some(distance) = distance {
                    if !distance.0.is_finite() || distance.0 <= 0.0 {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has an invalid surface extension",
                            feature.id
                        )));
                    }
                } else if existing.is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved extension distance",
                        feature.id
                    )));
                }
                if matches!(method, SurfaceExtension::Unresolved) && existing.is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved extension method",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                if let Some(distance) = distance {
                    parameters.insert("Distance".into(), format_length_mm(distance.0));
                }
                let mut properties = existing
                    .as_deref()
                    .map(|record| record.properties.clone())
                    .unwrap_or_default();
                properties.extend(feature.source_properties.clone());
                if let Some(faces) = resolved_faces {
                    properties.insert("Faces".into(), faces);
                }
                if let Some(method) = match method {
                    SurfaceExtension::Unresolved => None,
                    SurfaceExtension::Natural => Some("Natural"),
                    SurfaceExtension::Linear => Some("Linear"),
                } {
                    properties.insert("Method".into(), method.into());
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "ExtendSurface".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                mode,
            } => {
                let edges = edge_selection_value(edges).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no ruled-surface boundary edges",
                        feature.id
                    ))
                })?;
                let support_faces = face_selection_value(support_faces).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no ruled-surface supports",
                        feature.id
                    ))
                })?;
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "RuledSurface")
                        && !feature_family(record, "SurfaceRuled")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let (mode_name, direction, distance) = match mode {
                    RuledSurfaceMode::Normal { distance } => ("Normal", None, *distance),
                    RuledSurfaceMode::Tangent { distance } => ("Tangent", None, *distance),
                    RuledSurfaceMode::Direction {
                        direction,
                        distance,
                    } => {
                        require_direction(*direction, &feature.id, "ruled-surface direction")?;
                        ("Direction", Some(*direction), *distance)
                    }
                };
                if !distance.0.is_finite() || distance.0 <= 0.0 {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an invalid ruled-surface distance",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert("Distance".into(), format_length_mm(distance.0));
                let mut properties = feature.source_properties.clone();
                properties.insert("Edges".into(), edges);
                properties.insert("SupportFaces".into(), support_faces);
                properties.insert("Mode".into(), mode_name.into());
                match direction {
                    Some(direction) => {
                        properties.insert("Direction".into(), format_vector3(direction));
                    }
                    None => {
                        properties.remove("Direction");
                    }
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "RuledSurface".into(), |record| record.kind.clone()),
                    parameters,
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
                    .is_some_and(|record| !feature_family(record, "ReferenceAxis"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
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
                    .is_some_and(|record| !feature_family(record, "ReferencePoint"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
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
            FeatureDefinition::DatumCoordinateSystem {
                origin,
                x_axis,
                y_axis,
                z_axis,
            } => {
                if !valid_coordinate_frame(*origin, *x_axis, *y_axis, *z_axis) {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an invalid coordinate-system frame",
                        feature.id
                    )));
                }
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "CoordinateSystem")
                        && !feature_family(record, "ReferenceCoordinateSystem")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Origin".into(), format_point3_mm(*origin));
                properties.insert("XAxis".into(), format_vector3(*x_axis));
                properties.insert("YAxis".into(), format_vector3(*y_axis));
                properties.insert("ZAxis".into(), format_vector3(*z_axis));
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "CoordinateSystem".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::DatumCoordinateSystemUnresolved => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} has unresolved coordinate-system construction",
                    feature.id
                )));
            }
            FeatureDefinition::EquationCurve {
                parameter,
                x_expression,
                y_expression,
                z_expression,
                start,
                end,
            } => {
                if parameter.trim().is_empty()
                    || x_expression.trim().is_empty()
                    || y_expression.trim().is_empty()
                    || z_expression.trim().is_empty()
                    || !start.is_finite()
                    || !end.is_finite()
                    || start >= end
                {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an invalid equation curve",
                        feature.id
                    )));
                }
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "EquationDrivenCurve")
                        && !feature_family(record, "EquationCurve")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Parameter".into(), parameter.clone());
                properties.insert("XEquation".into(), x_expression.clone());
                properties.insert("YEquation".into(), y_expression.clone());
                properties.insert("ZEquation".into(), z_expression.clone());
                properties.insert("Start".into(), start.to_string());
                properties.insert("End".into(), end.to_string());
                (
                    existing.as_deref().map_or_else(
                        || "EquationDrivenCurve".into(),
                        |record| record.kind.clone(),
                    ),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            } => {
                let source =
                    path_source(source, &record_sources, &sketch_sources).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing projection source",
                            feature.id
                        ))
                    })?;
                let target_faces = face_selection_value(target_faces).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no projection target faces",
                        feature.id
                    ))
                })?;
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "ProjectedCurve")
                        && !feature_family(record, "ProjectionCurve")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Source".into(), source);
                properties.insert("TargetFaces".into(), target_faces);
                if let Some(bidirectional) = bidirectional {
                    properties.insert("Bidirectional".into(), bidirectional.to_string());
                } else if existing.is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved projection directionality",
                        feature.id
                    )));
                }
                match direction {
                    CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved) => {
                        if existing.is_none() {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has an unresolved projection direction",
                                feature.id
                            )));
                        }
                    }
                    CurveProjectionDirection::State(
                        CurveProjectionDirectionState::TargetNormal,
                    ) => {
                        properties.remove("Direction");
                    }
                    CurveProjectionDirection::Vector(direction) => {
                        require_direction(*direction, &feature.id, "projection direction")?;
                        properties.insert("Direction".into(), format_vector3(*direction));
                    }
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "ProjectedCurve".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::CompositeCurve { segments, closed } => {
                if segments.is_empty() {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has no composite-curve segments",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "CompositeCurve"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let segments = segments
                    .iter()
                    .map(|segment| {
                        path_source(segment, &record_sources, &sketch_sources).ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} references a missing composite segment",
                                feature.id
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut properties = feature.source_properties.clone();
                properties.insert("Segments".into(), segments.join(";"));
                properties.insert("Closed".into(), closed.to_string());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "CompositeCurve".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::Helix {
                axis_origin,
                axis_direction,
                radius,
                pitch,
                revolutions,
                clockwise,
                radial_growth,
                cone_angle,
                segment_turns,
                construction_style,
            } => {
                if radial_growth.is_some()
                    || cone_angle.is_some()
                    || segment_turns.is_some()
                    || construction_style.is_some()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses unsupported helix construction controls",
                        feature.id
                    )));
                }
                if ![axis_origin.x, axis_origin.y, axis_origin.z, pitch.0]
                    .into_iter()
                    .all(f64::is_finite)
                    || !valid_direction(*axis_direction)
                    || !radius.0.is_finite()
                    || radius.0 <= 0.0
                    || !revolutions.is_finite()
                    || *revolutions <= 0.0
                {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has invalid helix geometry",
                        feature.id
                    )));
                }
                if existing.as_deref().is_some_and(|record| !is_helix(record)) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert("Radius".into(), format_length_mm(radius.0));
                parameters.insert("Pitch".into(), format_length_mm(pitch.0));
                parameters.insert("Revolutions".into(), revolutions.to_string());
                let mut properties = feature.source_properties.clone();
                properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
                properties.insert("AxisDirection".into(), format_vector3(*axis_direction));
                properties.insert("Clockwise".into(), clockwise.to_string());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Helix".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::HelixNativeAxis {
                axis_native_ref,
                radius,
                height,
                revolutions,
                start_angle,
                clockwise,
            } => {
                if axis_native_ref.is_empty()
                    || !radius.0.is_finite()
                    || radius.0 <= 0.0
                    || !height.0.is_finite()
                    || !revolutions.is_finite()
                    || *revolutions <= 0.0
                    || !start_angle.0.is_finite()
                {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has invalid native-axis helix geometry",
                        feature.id
                    )));
                }
                let record = existing.as_deref().ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained native helix axis",
                        feature.id
                    ))
                })?;
                if !is_helix(record) || axis_native_ref != &record.id {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes its native helix axis",
                        feature.id
                    )));
                }
                let mut parameters = record.parameters.clone();
                parameters.insert(
                    "D3".into(),
                    format_length_like(radius.0, record.parameters.get("D3").map(String::as_str)),
                );
                parameters.insert(
                    "D4".into(),
                    format_length_like(height.0, record.parameters.get("D4").map(String::as_str)),
                );
                parameters.insert("D5".into(), revolutions.to_string());
                parameters.insert(
                    "D7".into(),
                    format_angle_like(
                        start_angle.0,
                        record.parameters.get("D7").map(String::as_str),
                    ),
                );
                let mut properties = feature.source_properties.clone();
                if properties.contains_key("Clockwise") || *clockwise {
                    properties.insert("Clockwise".into(), clockwise.to_string());
                }
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::Wrap {
                profile,
                face,
                mode,
                depth,
            } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Wrap"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let profile = profile_source(profile, &record_sources, &sketch_sources)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing wrap profile",
                            feature.id
                        ))
                    })?;
                let face = face_selection_value(face).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no wrap target face",
                        feature.id
                    ))
                })?;
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                match mode {
                    WrapMode::Emboss | WrapMode::Deboss => {
                        let depth = depth
                            .filter(|value| value.0.is_finite() && value.0 > 0.0)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "SLDPRT feature {} has invalid wrap depth",
                                    feature.id
                                ))
                            })?;
                        parameters.insert("Depth".into(), format_length_mm(depth.0));
                    }
                    WrapMode::Scribe => {
                        if depth.is_some() {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} gives a scribe wrap a depth",
                                feature.id
                            )));
                        }
                        parameters.remove("Depth");
                    }
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Profile".into(), profile);
                properties.insert("Face".into(), face);
                properties.insert(
                    "Mode".into(),
                    match mode {
                        WrapMode::Emboss => "Emboss",
                        WrapMode::Deboss => "Deboss",
                        WrapMode::Scribe => "Scribe",
                    }
                    .into(),
                );
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Wrap".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Sketch { space, .. } => {
                let requested_kind = match space {
                    SketchSpace::Unresolved => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has unresolved sketch coordinate space",
                            feature.id
                        )));
                    }
                    SketchSpace::Planar => "Sketch",
                    SketchSpace::Spatial => "3DSketch",
                };
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Sketch"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                (
                    existing.as_deref().map_or_else(
                        || requested_kind.into(),
                        |record| {
                            let native_space = if record.kind.eq_ignore_ascii_case("3DSketch") {
                                SketchSpace::Spatial
                            } else {
                                SketchSpace::Planar
                            };
                            if native_space == *space {
                                record.kind.clone()
                            } else {
                                requested_kind.into()
                            }
                        },
                    ),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    feature.source_properties.clone(),
                )
            }
            FeatureDefinition::Extrude {
                profile,
                direction,
                extent,
                op,
                draft,
                reverse_draft,
                direction_source,
                solid,
                face_maker,
                inner_wire_taper,
                first_offset,
                second_offset,
                length_along_profile_normal,
                allow_multi_profile_faces,
            } => {
                if reverse_draft.is_some()
                    || direction_source.is_some()
                    || *solid == Some(false)
                    || face_maker.is_some()
                    || inner_wire_taper.is_some()
                    || first_offset.is_some()
                    || second_offset.is_some()
                    || length_along_profile_normal.is_some()
                    || allow_multi_profile_faces.is_some()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses unsupported extrusion construction controls",
                        feature.id
                    )));
                }
                if *op == BooleanOp::Unresolved && existing.is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires retained extrusion operation data",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !is_extrude(record))
                {
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
                if let Some(record) = existing.as_deref() {
                    if !record.properties.contains_key("Operation")
                        && *op != BooleanOp::Unresolved
                        && extrude_feature_op(record).is_some_and(|native_op| native_op != *op)
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} changes its inferred extrusion operation",
                            feature.id
                        )));
                    }
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let positional_depth =
                    parameters.contains_key("D1") && !parameters.contains_key("Depth");
                let mut properties = feature.source_properties.clone();
                parameters.remove("Depth");
                parameters.remove("Depth2");
                parameters.remove("Draft");
                properties.remove("Direction");
                properties.remove("Face");
                match extent {
                    Extent::Blind { length } => {
                        if properties.contains_key("EndCondition") || existing.is_none() {
                            properties.insert("EndCondition".into(), "Blind".into());
                        }
                        let key = if positional_depth { "D1" } else { "Depth" };
                        parameters.insert(
                            key.into(),
                            format_length_like(
                                length.0,
                                existing
                                    .as_deref()
                                    .and_then(|record| record.parameters.get(key))
                                    .map(String::as_str),
                            ),
                        );
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
                    Extent::ToFirst | Extent::ToLast | Extent::ToShape { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion termination",
                            feature.id
                        )));
                    }
                    Extent::ToFace { face } if face_selection_value(face).is_some() => {
                        let selection = face_selection_value(face).expect("guarded above");
                        properties.insert("EndCondition".into(), "ToFace".into());
                        properties.insert("Face".into(), selection);
                    }
                    Extent::ToFace { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion face selection",
                            feature.id
                        )));
                    }
                    Extent::Unresolved
                    | Extent::Angle { .. }
                    | Extent::SymmetricAngle { .. }
                    | Extent::TwoSidedAngles { .. }
                    | Extent::TwoSidedExtents { .. }
                    | Extent::SymmetricExtent { .. } => {
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
                if *op != BooleanOp::Unresolved
                    && (properties.contains_key("Operation")
                        || existing.as_deref().and_then(extrude_feature_op).is_none())
                {
                    properties.insert(
                        "Operation".into(),
                        resolved_boolean_op(*op, &feature.id)?.into(),
                    );
                }
                let implicit_profile = existing.as_deref().is_some_and(|record| {
                    !record.properties.contains_key("Profile")
                        && matches!(profile, ProfileRef::Native(native) if native == &record.id)
                });
                if !implicit_profile {
                    properties.insert("Profile".into(), profile_source);
                }
                let kind = existing.as_deref().map_or_else(
                    || match op {
                        BooleanOp::Unresolved => "Extrusion".into(),
                        BooleanOp::Join => "BossExtrude".into(),
                        BooleanOp::Cut => "CutExtrude".into(),
                        BooleanOp::NewBody | BooleanOp::Intersect => "Extrusion".into(),
                    },
                    |record| record.kind.clone(),
                );
                (kind, parameters, properties)
            }
            FeatureDefinition::Fillet { edges, radius } => {
                let selection = edge_selection_value(edges);
                if selection.is_none()
                    && !(matches!(edges, EdgeSelection::Unresolved) && existing.is_some())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported fillet semantics",
                        feature.id
                    )));
                }
                if existing.as_deref().is_some_and(|record| !is_fillet(record)) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported fillet semantics",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let positional_radius = parameters.contains_key("D1")
                    && !parameters.contains_key("Radius")
                    && !parameters.keys().any(|name| indexed_name(name, "Radius"));
                match radius {
                    RadiusSpec::Unresolved { .. } => {
                        if existing.is_none() {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has an unresolved fillet radius law",
                                feature.id
                            )));
                        }
                    }
                    RadiusSpec::Constant {
                        radius: Length(radius),
                    } => {
                        parameters.retain(|name, _| {
                            name != "Radius"
                                && !indexed_name(name, "Radius")
                                && !indexed_name(name, "Position")
                        });
                        let key = if positional_radius { "D1" } else { "Radius" };
                        let value = format_length_like(
                            *radius,
                            existing
                                .as_deref()
                                .and_then(|record| record.parameters.get(key))
                                .map(String::as_str),
                        );
                        parameters.insert(key.into(), value);
                    }
                    RadiusSpec::Variable { points } => {
                        parameters.retain(|name, _| {
                            name != "Radius"
                                && !indexed_name(name, "Radius")
                                && !indexed_name(name, "Position")
                        });
                        if positional_radius {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} changes positional fillet form",
                                feature.id
                            )));
                        }
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
                let mut properties = feature.source_properties.clone();
                if let Some(selection) = selection {
                    write_native_selection(
                        &mut properties,
                        "Edges",
                        &selection,
                        existing.as_deref().map_or("", |record| record.id.as_str()),
                    );
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Fillet".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::FaceBlend { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT writing does not support face-blend feature {}",
                    feature.id
                )));
            }
            FeatureDefinition::Chamfer {
                edges,
                spec,
                flip_direction,
            } => {
                if *flip_direction == Some(true) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses an unsupported reversed chamfer reference side",
                        feature.id
                    )));
                }
                let selection = edge_selection_value(edges);
                if selection.is_none()
                    && !(matches!(edges, EdgeSelection::Unresolved) && existing.is_some())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported chamfer semantics",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !is_chamfer(record))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported chamfer semantics",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let positional = parameters.contains_key("D1")
                    && !parameters.contains_key("Distance")
                    && !parameters.contains_key("Distance1");
                let positional_angle = positional
                    && parameters
                        .get("D2")
                        .is_some_and(|value| parse_bounded_angle_rad(value).is_some());
                match spec {
                    ChamferSpec::Unresolved { .. } => {
                        if existing.is_none() {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has unresolved chamfer dimensions",
                                feature.id
                            )));
                        }
                    }
                    ChamferSpec::Distance { distance } => {
                        if existing.is_some()
                            && if positional {
                                parameters.contains_key("D2")
                            } else {
                                parameters.contains_key("Distance1")
                                    || parameters.contains_key("Distance2")
                                    || parameters.contains_key("Angle")
                            }
                        {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} changes chamfer form",
                                feature.id
                            )));
                        }
                        let key = if positional { "D1" } else { "Distance" };
                        let value = format_length_like(
                            distance.0,
                            existing
                                .as_deref()
                                .and_then(|record| record.parameters.get(key))
                                .map(String::as_str),
                        );
                        parameters.insert(key.into(), value);
                    }
                    ChamferSpec::TwoDistances { first, second } => {
                        if existing.is_some()
                            && if positional {
                                !parameters.contains_key("D2") || positional_angle
                            } else {
                                !parameters.contains_key("Distance1")
                                    || !parameters.contains_key("Distance2")
                                    || parameters.contains_key("Angle")
                            }
                        {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} changes chamfer form",
                                feature.id
                            )));
                        }
                        let (first_key, second_key) = if positional {
                            ("D1", "D2")
                        } else {
                            ("Distance1", "Distance2")
                        };
                        parameters.insert(
                            first_key.into(),
                            format_length_like(
                                first.0,
                                existing
                                    .as_deref()
                                    .and_then(|record| record.parameters.get(first_key))
                                    .map(String::as_str),
                            ),
                        );
                        parameters.insert(
                            second_key.into(),
                            format_length_like(
                                second.0,
                                existing
                                    .as_deref()
                                    .and_then(|record| record.parameters.get(second_key))
                                    .map(String::as_str),
                            ),
                        );
                    }
                    ChamferSpec::DistanceAngle { distance, angle } => {
                        if existing.is_some()
                            && if positional {
                                !positional_angle
                            } else {
                                !parameters.contains_key("Distance")
                                    || !parameters.contains_key("Angle")
                                    || parameters.contains_key("Distance1")
                                    || parameters.contains_key("Distance2")
                            }
                        {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} changes chamfer form",
                                feature.id
                            )));
                        }
                        let (distance_key, angle_key) = if positional {
                            ("D1", "D2")
                        } else {
                            ("Distance", "Angle")
                        };
                        parameters.insert(
                            distance_key.into(),
                            format_length_like(
                                distance.0,
                                existing
                                    .as_deref()
                                    .and_then(|record| record.parameters.get(distance_key))
                                    .map(String::as_str),
                            ),
                        );
                        parameters.insert(
                            angle_key.into(),
                            format_angle_like(
                                angle.0,
                                existing
                                    .as_deref()
                                    .and_then(|record| record.parameters.get(angle_key))
                                    .map(String::as_str),
                            ),
                        );
                    }
                }
                let mut properties = feature.source_properties.clone();
                if let Some(selection) = selection {
                    write_native_selection(
                        &mut properties,
                        "Edges",
                        &selection,
                        existing.as_deref().map_or("", |record| record.id.as_str()),
                    );
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Chamfer".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::OffsetShape { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported whole-shape offset semantics",
                    feature.id
                )));
            }
            FeatureDefinition::PostProcess { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported topology post-processing semantics",
                    feature.id
                )));
            }
            FeatureDefinition::PointGeometry { .. }
            | FeatureDefinition::LineSegment { .. }
            | FeatureDefinition::CircularArc { .. }
            | FeatureDefinition::EllipticArc { .. }
            | FeatureDefinition::Polyline { .. }
            | FeatureDefinition::RegularPolygonCurve { .. }
            | FeatureDefinition::PlanarPatch { .. }
            | FeatureDefinition::FaceFromShapes { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported construction-geometry semantics",
                    feature.id
                )));
            }
            FeatureDefinition::Compound { .. }
            | FeatureDefinition::RefineShape { .. }
            | FeatureDefinition::ReverseShape { .. }
            | FeatureDefinition::RuledBetweenCurves { .. }
            | FeatureDefinition::SectionShape { .. }
            | FeatureDefinition::MirrorShape { .. }
            | FeatureDefinition::ProjectOnSurface { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported derived-shape semantics",
                    feature.id
                )));
            }
            FeatureDefinition::Shell {
                removed_faces,
                thickness,
                outward,
                mode,
                join,
                resolve_intersections,
                allow_self_intersections,
            } => {
                if mode.is_some()
                    || join.is_some()
                    || resolve_intersections.is_some()
                    || allow_self_intersections.is_some()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported shell construction semantics",
                        feature.id
                    )));
                }
                let selection = face_selection_value(removed_faces);
                if selection.is_none()
                    && !(matches!(removed_faces, FaceSelection::Unresolved) && existing.is_some())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported shell semantics",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Shell"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported shell semantics",
                        feature.id
                    )));
                }
                if existing.is_none() && (thickness.is_none() || outward.is_none()) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved shell construction",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let thickness_key =
                    if parameters.contains_key("D1") && !parameters.contains_key("Thickness") {
                        "D1"
                    } else {
                        "Thickness"
                    };
                if let Some(thickness) = thickness {
                    parameters.insert(
                        thickness_key.into(),
                        format_length_like(
                            thickness.0,
                            existing
                                .as_deref()
                                .and_then(|record| record.parameters.get(thickness_key))
                                .map(String::as_str),
                        ),
                    );
                }
                let mut properties = feature.source_properties.clone();
                if let Some(selection) = selection {
                    write_native_selection(
                        &mut properties,
                        "RemovedFaces",
                        &selection,
                        existing.as_deref().map_or("", |record| record.id.as_str()),
                    );
                }
                if let Some(outward) = outward {
                    properties.insert("Outward".into(), outward.to_string());
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Shell".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } => {
                use cadmpeg_ir::features::ThickenSide;

                let selection = face_selection_value(faces);
                if selection.is_none()
                    && !(matches!(faces, FaceSelection::Unresolved) && existing.is_some())
                    || existing.as_deref().is_some_and(|record| {
                        !feature_family(record, "Thicken")
                            && !feature_family(record, "Thickness")
                            && !feature_input_class(record, NativeClassKind::Thicken)
                    })
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported thicken semantics",
                        feature.id
                    )));
                }
                if existing.is_none() && (thickness.is_none() || side.is_none()) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved thicken construction",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let thickness_key =
                    if parameters.contains_key("D1") && !parameters.contains_key("Thickness") {
                        "D1"
                    } else {
                        "Thickness"
                    };
                if let Some(thickness) = thickness {
                    parameters.insert(
                        thickness_key.into(),
                        format_length_like(
                            thickness.0,
                            existing
                                .as_deref()
                                .and_then(|record| record.parameters.get(thickness_key))
                                .map(String::as_str),
                        ),
                    );
                }
                let mut properties = feature.source_properties.clone();
                if let Some(selection) = selection {
                    write_native_selection(
                        &mut properties,
                        "Faces",
                        &selection,
                        existing.as_deref().map_or("", |record| record.id.as_str()),
                    );
                }
                if let Some(side) = side {
                    let both_sides = matches!(side, ThickenSide::Both);
                    if both_sides || properties.contains_key("BothSides") {
                        properties.insert("BothSides".into(), both_sides.to_string());
                    }
                    let reverse = matches!(side, ThickenSide::Reverse);
                    if reverse || properties.contains_key("Reverse") {
                        properties.insert("Reverse".into(), reverse.to_string());
                    }
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Thicken".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::OffsetSurface { faces, distance } => {
                let distance = distance.ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no surface-offset distance",
                        feature.id
                    ))
                })?;
                let selection = face_selection_value(faces).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no offset-surface support faces",
                        feature.id
                    ))
                })?;
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "OffsetSurface"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                if !distance.0.is_finite() {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a non-finite surface offset",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert("Distance".into(), format_length_mm(distance.0));
                let mut properties = feature.source_properties.clone();
                properties.insert("Faces".into(), selection);
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "OffsetSurface".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::KnitSurface {
                faces,
                merge_entities,
                create_solid,
                gap_tolerance,
            } => {
                let selection = face_selection_value(faces).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no knit-surface input faces",
                        feature.id
                    ))
                })?;
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "KnitSurface") && !feature_family(record, "Knit")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                match gap_tolerance {
                    Some(value) if value.0.is_finite() && value.0 >= 0.0 => {
                        parameters.insert("GapTolerance".into(), format_length_mm(value.0));
                    }
                    Some(_) => {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has an invalid knit tolerance",
                            feature.id
                        )));
                    }
                    None => {
                        parameters.remove("GapTolerance");
                    }
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Faces".into(), selection);
                properties.insert("MergeEntities".into(), merge_entities.to_string());
                properties.insert("CreateSolid".into(), create_solid.to_string());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "KnitSurface".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::SewBodies { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses a body-level sew operation",
                    feature.id
                )));
            }
            FeatureDefinition::TrimBodies { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses a body-level trim operation",
                    feature.id
                )));
            }
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                continuity,
                merge_result,
            } => {
                let boundary = edge_selection_value(boundary).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no filled-surface boundary",
                        feature.id
                    ))
                })?;
                let support_faces = face_selection_value(support_faces).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no filled-surface supports",
                        feature.id
                    ))
                })?;
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "FilledSurface")
                        && !feature_family(record, "FillSurface")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Boundary".into(), boundary);
                properties.insert("SupportFaces".into(), support_faces);
                properties.insert(
                    "Continuity".into(),
                    match continuity {
                        SurfaceContinuity::Contact => "Contact",
                        SurfaceContinuity::Tangent => "Tangent",
                        SurfaceContinuity::Curvature => "Curvature",
                    }
                    .into(),
                );
                properties.insert("MergeResult".into(), merge_result.to_string());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "FilledSurface".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::Draft {
                faces: face_selection,
                neutral_plane: plane_selection,
                pull_direction,
                angle,
                outward,
            } => {
                let faces = face_selection_value(face_selection);
                let neutral_plane = face_selection_value(plane_selection);
                let operands_supported = |selection: &FaceSelection, native: Option<&String>| {
                    native.is_some()
                        || matches!(selection, FaceSelection::Unresolved) && existing.is_some()
                };
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Draft"))
                    || !operands_supported(face_selection, faces.as_ref())
                    || !operands_supported(plane_selection, neutral_plane.as_ref())
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
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert("Angle".into(), format_angle_rad(angle.0));
                let mut properties = feature.source_properties.clone();
                let fallback = existing.as_deref().map_or("", |record| record.id.as_str());
                if let Some(faces) = faces {
                    write_native_selection(&mut properties, "Faces", &faces, fallback);
                }
                if let Some(neutral_plane) = neutral_plane {
                    write_native_selection(
                        &mut properties,
                        "NeutralPlane",
                        &neutral_plane,
                        fallback,
                    );
                }
                properties.insert("Direction".into(), format_vector3(*pull_direction));
                properties.insert("Outward".into(), outward.to_string());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Draft".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Combine { target, tools, op } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Combine"))
                    || *op == BooleanOp::NewBody
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported combine semantics",
                        feature.id
                    )));
                }
                let target = body_selection_value(target);
                let tools = body_selection_value(tools);
                if target.is_none() || tools.is_none() {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an empty combine selection",
                        feature.id
                    )));
                }
                let target = target.expect("checked above");
                let tools = tools.expect("checked above");
                let mut properties = feature.source_properties.clone();
                properties.insert("Target".into(), target);
                properties.insert("Tools".into(), tools);
                properties.insert(
                    "Operation".into(),
                    resolved_boolean_op(*op, &feature.id)?.into(),
                );
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Combine".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::CutWithSurface {
                targets,
                tools,
                reverse,
            } => {
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "CutWithSurface")
                        && !feature_family(record, "SurfaceCut")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let targets = body_selection_value(targets).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no surface-cut target bodies",
                        feature.id
                    ))
                })?;
                let tools = face_selection_value(tools).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no surface-cut tools",
                        feature.id
                    ))
                })?;
                let mut properties = feature.source_properties.clone();
                properties.insert("Targets".into(), targets);
                properties.insert("Tools".into(), tools);
                properties.insert("Reverse".into(), reverse.to_string());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "CutWithSurface".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::DeleteBody { bodies, mode } => {
                let selection = body_selection_value(bodies);
                if existing
                    .as_deref()
                    .is_some_and(|record| body_retention_mode(record).is_none())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported delete-body semantics",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                if let Some(selection) = selection {
                    properties.insert("Bodies".into(), selection);
                } else if !matches!(mode, BodyRetentionMode::Unresolved) || existing.is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported delete-body semantics",
                        feature.id
                    )));
                }
                match mode {
                    BodyRetentionMode::Unresolved => {
                        let Some(record) = existing.as_deref() else {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} requires a retained unresolved body operation",
                                feature.id
                            )));
                        };
                        if body_retention_mode(record) != Some(BodyRetentionMode::Unresolved) {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} removes a resolved body-retention mode",
                                feature.id
                            )));
                        }
                        properties.remove("Mode");
                    }
                    BodyRetentionMode::DeleteSelected => {
                        properties.insert("Mode".into(), "Delete".into());
                    }
                    BodyRetentionMode::KeepSelected => {
                        properties.insert("Mode".into(), "Keep".into());
                    }
                }
                (
                    existing.as_deref().map_or_else(
                        || match mode {
                            BodyRetentionMode::Unresolved => "Feature".into(),
                            BodyRetentionMode::DeleteSelected => "DeleteBody".into(),
                            BodyRetentionMode::KeepSelected => "KeepBody".into(),
                        },
                        |record| record.kind.clone(),
                    ),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::DeleteFace { faces, heal } => {
                let faces = face_selection_value(faces);
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "DeleteFace"))
                    || faces.is_none()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported delete-face semantics",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Faces".into(), faces.expect("checked above"));
                properties.insert("Heal".into(), heal.to_string());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "DeleteFace".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => {
                let targets = face_selection_value(targets);
                let replacements = face_selection_value(replacements);
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "ReplaceFace"))
                    || targets.is_none()
                    || replacements.is_none()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported replace-face semantics",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Faces".into(), targets.expect("checked above"));
                properties.insert(
                    "ReplacementFaces".into(),
                    replacements.expect("checked above"),
                );
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "ReplaceFace".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::MoveFace { faces, motion } => {
                let faces = face_selection_value(faces);
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "MoveFace"))
                    || faces.is_none()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported move-face semantics",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let mut properties = feature.source_properties.clone();
                properties.insert("Faces".into(), faces.expect("checked above"));
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
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "MoveFace".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::MoveBody {
                bodies,
                translation,
                rotation,
                copies,
            } => {
                let bodies = body_selection_value(bodies).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no body-motion selection",
                        feature.id
                    ))
                })?;
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "MoveBody") && !feature_family(record, "MoveCopyBody")
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                if ![translation.x, translation.y, translation.z]
                    .into_iter()
                    .all(f64::is_finite)
                {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a non-finite body translation",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let mut properties = feature.source_properties.clone();
                properties.insert("Bodies".into(), bodies);
                properties.insert(
                    "Translation".into(),
                    format_point3_mm(Point3::new(translation.x, translation.y, translation.z)),
                );
                properties.insert("Copies".into(), copies.to_string());
                match rotation {
                    Some(rotation) => {
                        require_direction(rotation.direction, &feature.id, "body rotation axis")?;
                        if !rotation.angle.0.is_finite()
                            || ![rotation.origin.x, rotation.origin.y, rotation.origin.z]
                                .into_iter()
                                .all(f64::is_finite)
                        {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has invalid body rotation",
                                feature.id
                            )));
                        }
                        properties
                            .insert("RotationOrigin".into(), format_point3_mm(rotation.origin));
                        properties
                            .insert("RotationAxis".into(), format_vector3(rotation.direction));
                        parameters.insert("Rotation".into(), format_angle_rad(rotation.angle.0));
                    }
                    None => {
                        properties.remove("RotationOrigin");
                        properties.remove("RotationAxis");
                        parameters.remove("Rotation");
                    }
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "MoveBody".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Dome {
                faces,
                height,
                elliptical,
                reverse,
            } => {
                let faces = face_selection_value(faces);
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Dome"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported dome semantics",
                        feature.id
                    )));
                }
                if existing.is_none()
                    && (faces.is_none()
                        || height.is_none()
                        || elliptical.is_none()
                        || reverse.is_none())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved dome construction",
                        feature.id
                    )));
                }
                if height.is_some_and(|height| !height.0.is_finite()) {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has a non-finite dome height",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                if let Some(height) = height {
                    parameters.insert("Height".into(), format_length_mm(height.0));
                }
                let mut properties = feature.source_properties.clone();
                if let Some(faces) = faces {
                    properties.insert("Faces".into(), faces);
                }
                if let Some(elliptical) = elliptical {
                    properties.insert("Elliptical".into(), elliptical.to_string());
                }
                if let Some(reverse) = reverse {
                    properties.insert("Reverse".into(), reverse.to_string());
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Dome".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Flex { axis, mode } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Flex"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported flex semantics",
                        feature.id
                    )));
                }
                if existing.is_none()
                    && (axis.is_none() || matches!(mode, FlexMode::Unresolved { .. }))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved flex construction",
                        feature.id
                    )));
                }
                if let Some(axis) = axis {
                    require_direction(*axis, &feature.id, "flex axis")?;
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let mut properties = feature.source_properties.clone();
                if let Some(axis) = axis {
                    properties.insert("Axis".into(), format_vector3(*axis));
                    properties.remove("AxisDirection");
                }
                match mode {
                    FlexMode::Unresolved { .. } => {}
                    FlexMode::Bending { angle } => {
                        if !angle.0.is_finite() {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has a non-finite flex angle",
                                feature.id
                            )));
                        }
                        parameters.remove("Factor");
                        parameters.remove("Distance");
                        properties.insert("Mode".into(), "Bending".into());
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                    FlexMode::Twisting { angle } => {
                        if !angle.0.is_finite() {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has a non-finite flex angle",
                                feature.id
                            )));
                        }
                        parameters.remove("Factor");
                        parameters.remove("Distance");
                        properties.insert("Mode".into(), "Twisting".into());
                        parameters.insert("Angle".into(), format_angle_rad(angle.0));
                    }
                    FlexMode::Tapering { factor } => {
                        if !factor.is_finite() || *factor <= 0.0 {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has an invalid flex taper factor",
                                feature.id
                            )));
                        }
                        parameters.remove("Angle");
                        parameters.remove("Distance");
                        properties.insert("Mode".into(), "Tapering".into());
                        parameters.insert("Factor".into(), factor.to_string());
                    }
                    FlexMode::Stretching { distance } => {
                        if !distance.0.is_finite() {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has a non-finite flex distance",
                                feature.id
                            )));
                        }
                        parameters.remove("Angle");
                        parameters.remove("Factor");
                        properties.insert("Mode".into(), "Stretching".into());
                        parameters.insert("Distance".into(), format_length_mm(distance.0));
                    }
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Flex".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Scale {
                bodies,
                center,
                factors,
            } => {
                let selection = body_selection_value(bodies);
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Scale"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported scale semantics",
                        feature.id
                    )));
                }
                let center_valid = center.as_ref().is_none_or(|center| match center {
                    ScaleCenter::Point(point) => {
                        [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                    }
                    ScaleCenter::Native(reference) => !reference.is_empty(),
                    ScaleCenter::Centroid | ScaleCenter::ModelOrigin => true,
                });
                let resolved_factors = factors.resolved();
                if existing.is_none()
                    && (selection.is_none() || center.is_none() || resolved_factors.is_none())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved scale construction",
                        feature.id
                    )));
                }
                let factors_valid = [factors.uniform, factors.x, factors.y, factors.z]
                    .into_iter()
                    .flatten()
                    .all(|factor| factor.is_finite() && factor != 0.0);
                if !factors_valid || !center_valid {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an invalid scale transform",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                if let Some(factor) = factors.uniform {
                    parameters.insert("Factor".into(), factor.to_string());
                } else {
                    if [factors.x, factors.y, factors.z]
                        .into_iter()
                        .all(|factor| factor.is_some())
                    {
                        parameters.remove("Factor");
                    }
                    if let Some(factor) = factors.x {
                        parameters.insert("ScaleX".into(), factor.to_string());
                    }
                    if let Some(factor) = factors.y {
                        parameters.insert("ScaleY".into(), factor.to_string());
                    }
                    if let Some(factor) = factors.z {
                        parameters.insert("ScaleZ".into(), factor.to_string());
                    }
                }
                let mut properties = feature.source_properties.clone();
                if let Some(selection) = selection {
                    properties.insert("Bodies".into(), selection);
                }
                match center {
                    Some(ScaleCenter::Centroid) => {
                        properties.remove("Center");
                        properties.remove("CenterRef");
                        properties.insert("CenterType".into(), "Centroid".into());
                    }
                    Some(ScaleCenter::ModelOrigin) => {
                        properties.remove("Center");
                        properties.remove("CenterRef");
                        properties.insert("CenterType".into(), "ModelOrigin".into());
                    }
                    Some(ScaleCenter::Point(point)) => {
                        properties.remove("CenterRef");
                        properties.insert("CenterType".into(), "Point".into());
                        properties.insert("Center".into(), format_point3_mm(*point));
                    }
                    Some(ScaleCenter::Native(reference)) => {
                        properties.remove("Center");
                        properties.insert("CenterType".into(), "Reference".into());
                        properties.insert("CenterRef".into(), reference.clone());
                    }
                    None => {}
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Scale".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Hole {
                profile,
                profile_filter,
                face,
                position,
                direction,
                kind,
                exit_kind,
                diameter,
                extent,
                bottom,
                taper_angle,
                specification,
                allow_multi_profile_faces,
            } => {
                if profile.is_some()
                    || profile_filter.is_some()
                    || bottom.is_some()
                    || taper_angle.is_some()
                    || specification.is_some()
                    || allow_multi_profile_faces.is_some()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported hole construction semantics",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Hole"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported hole semantics",
                        feature.id
                    )));
                }
                if existing.is_none() && diameter.is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved hole diameter",
                        feature.id
                    )));
                }
                if exit_kind.is_some() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unsupported hole exit treatment",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                if let Some(diameter) = diameter {
                    parameters.insert("Diameter".into(), format_length_mm(diameter.0));
                }
                match kind {
                    HoleKind::Unresolved { .. } if existing.is_some() => {}
                    HoleKind::Unresolved { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has unresolved hole entry construction",
                            feature.id
                        )));
                    }
                    HoleKind::Simple => {
                        parameters.remove("CounterboreDiameter");
                        parameters.remove("CounterboreDepth");
                        parameters.remove("CountersinkDiameter");
                        parameters.remove("CountersinkAngle");
                    }
                    HoleKind::Counterbore { diameter, depth } => {
                        parameters.remove("CountersinkDiameter");
                        parameters.remove("CountersinkAngle");
                        parameters
                            .insert("CounterboreDiameter".into(), format_length_mm(diameter.0));
                        parameters.insert("CounterboreDepth".into(), format_length_mm(depth.0));
                    }
                    HoleKind::Chamfer { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has an unsupported chamfered hole treatment",
                            feature.id
                        )));
                    }
                    HoleKind::Countersink { diameter, angle } => {
                        parameters.remove("CounterboreDiameter");
                        parameters.remove("CounterboreDepth");
                        parameters
                            .insert("CountersinkDiameter".into(), format_length_mm(diameter.0));
                        parameters.insert("CountersinkAngle".into(), format_angle_rad(angle.0));
                    }
                    HoleKind::Counterdrill { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has unsupported counterdrill construction",
                            feature.id
                        )));
                    }
                }
                let mut properties = feature.source_properties.clone();
                match face {
                    Some(face) if face_selection_value(face).is_some() => {
                        properties.insert(
                            "Face".into(),
                            face_selection_value(face).expect("guarded above"),
                        );
                    }
                    Some(FaceSelection::Unresolved) if existing.is_some() => {}
                    Some(FaceSelection::Unresolved) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has an unresolved hole face selection",
                            feature.id
                        )));
                    }
                    Some(_) => {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has an empty hole face selection",
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
                    None if existing.is_none() => {
                        properties.remove("Position");
                    }
                    None => {}
                }
                match direction {
                    Some(direction) => {
                        require_direction(*direction, &feature.id, "hole direction")?;
                        properties.insert("Direction".into(), format_vector3(*direction));
                    }
                    None if existing.is_none() => {
                        properties.remove("Direction");
                    }
                    None => {}
                }
                match extent {
                    Some(Extent::Blind {
                        length: Length(depth),
                    }) => {
                        parameters.insert("Depth".into(), format_length_mm(*depth));
                        properties.insert("EndCondition".into(), "Blind".into());
                    }
                    Some(Extent::ThroughAll) => {
                        parameters.remove("Depth");
                        properties.insert("EndCondition".into(), "ThroughAll".into());
                    }
                    Some(_) => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} changes unsupported hole termination",
                            feature.id
                        )))
                    }
                    None if existing.is_some() => {}
                    None => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has unresolved hole termination",
                            feature.id
                        )))
                    }
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Hole".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Revolve { construction, op } => {
                if construction.axis_reference.is_some()
                    || construction.solid == Some(false)
                    || construction.face_maker_class.is_some()
                    || construction.fuse_order.is_some()
                    || construction.allow_multi_profile_faces.is_some()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} uses unsupported revolution construction controls",
                        feature.id
                    )));
                }
                if existing
                    .as_deref()
                    .is_some_and(|record| !is_revolve(record))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported revolution semantics",
                        feature.id
                    )));
                }
                if existing.is_none()
                    && (construction.profile.is_none()
                        || construction.axis.is_none()
                        || construction.extent.is_none()
                        || *op == BooleanOp::Unresolved)
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved revolution construction",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let mut properties = feature.source_properties.clone();
                if let Some(extent) = &construction.extent {
                    parameters.remove("Angle");
                    parameters.remove("Angle2");
                    match extent {
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
                }
                if let Some(axis) = construction.axis {
                    if !valid_direction(axis.direction) {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} has a degenerate revolution axis",
                            feature.id
                        )));
                    }
                    properties.insert("AxisOrigin".into(), format_point3_mm(axis.origin));
                    properties.insert("AxisDirection".into(), format_vector3(axis.direction));
                }
                if *op != BooleanOp::Unresolved {
                    properties.insert(
                        "Operation".into(),
                        resolved_boolean_op(*op, &feature.id)?.into(),
                    );
                }
                if let Some(profile) = &construction.profile {
                    let profile_source = profile_source(profile, &record_sources, &sketch_sources)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} references a missing revolution profile",
                                feature.id
                            ))
                        })?;
                    properties.insert("Profile".into(), profile_source);
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Revolve".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Sweep {
                profile,
                sections,
                path,
                mode,
                orientation,
                transition,
                transformation,
                path_tangent,
                linearize,
                twist,
                scale,
                allow_multi_profile_faces,
            } => {
                if !sections.is_empty()
                    || orientation.is_some()
                    || transition.is_some()
                    || transformation.is_some()
                    || *path_tangent
                    || *linearize
                    || allow_multi_profile_faces.is_some()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported sweep construction semantics",
                        feature.id
                    )));
                }
                if existing.as_deref().is_some_and(|record| !is_sweep(record)) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let profile_source = match profile {
                    Some(profile) => Some(
                        profile_source(profile, &record_sources, &sketch_sources).ok_or_else(
                            || {
                                CodecError::Malformed(format!(
                                    "SLDPRT feature {} references a missing sweep profile",
                                    feature.id
                                ))
                            },
                        )?,
                    ),
                    None => None,
                };
                let path_source = match path {
                    Some(path) => Some(
                        path_source(path, &record_sources, &sketch_sources).ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} references a missing sweep path",
                                feature.id
                            ))
                        })?,
                    ),
                    None => None,
                };
                if existing.is_none() && (profile_source.is_none() || path_source.is_none()) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved sweep operands",
                        feature.id
                    )));
                }
                if existing.is_none() && *mode == SweepMode::Unresolved {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved sweep result semantics",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
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
                let mut properties = feature.source_properties.clone();
                if let Some(profile) = profile_source {
                    properties.insert("Profile".into(), profile);
                }
                if let Some(path) = path_source {
                    properties.insert("Path".into(), path);
                }
                match mode {
                    SweepMode::Solid { op } => {
                        properties.insert(
                            "Operation".into(),
                            resolved_boolean_op(*op, &feature.id)?.into(),
                        );
                    }
                    SweepMode::Surface => {
                        properties.remove("Operation");
                    }
                    SweepMode::Unresolved => {}
                }
                (
                    existing.as_deref().map_or_else(
                        || {
                            match mode {
                                SweepMode::Surface => "Surface-Sweep",
                                SweepMode::Solid { .. } | SweepMode::Unresolved => "Sweep",
                            }
                            .into()
                        },
                        |record| record.kind.clone(),
                    ),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Loft {
                profiles,
                guides,
                op,
                closed,
                solid,
                ruled,
                max_degree,
                check_compatibility,
                allow_multi_profile_faces,
            } => {
                if !solid
                    || *ruled
                    || max_degree.is_some()
                    || check_compatibility.is_some()
                    || allow_multi_profile_faces.is_some()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported loft result semantics",
                        feature.id
                    )));
                }
                if existing.as_deref().is_some_and(|record| !is_loft(record)) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported loft semantics",
                        feature.id
                    )));
                }
                if existing.is_none() && (profiles.len() < 2 || *op == BooleanOp::Unresolved) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved loft construction semantics",
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
                let mut properties = feature.source_properties.clone();
                if !profile_sources.is_empty() || existing.is_none() {
                    properties.insert("Profiles".into(), profile_sources.join(","));
                }
                if guide_sources.is_empty() && existing.is_none() {
                    properties.remove("Guides");
                } else if !guide_sources.is_empty() {
                    properties.insert("Guides".into(), guide_sources.join(","));
                }
                if *op != BooleanOp::Unresolved {
                    properties.insert(
                        "Operation".into(),
                        resolved_boolean_op(*op, &feature.id)?.into(),
                    );
                }
                if *closed || existing.is_none() || properties.contains_key("Closed") {
                    properties.insert("Closed".into(), closed.to_string());
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Loft".into(), |record| record.kind.clone()),
                    existing
                        .as_deref()
                        .map(|record| record.parameters.clone())
                        .unwrap_or_default(),
                    properties,
                )
            }
            FeatureDefinition::Rib { construction, op } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Rib"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                if existing.is_none()
                    && (construction.profile.is_none()
                        || construction.direction.is_none()
                        || construction.thickness.is_none()
                        || construction.side.is_none()
                        || construction.draft == RibDraft::Unresolved
                        || *op == BooleanOp::Unresolved)
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved rib construction",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                if let Some(thickness) = construction.thickness {
                    parameters.insert("Thickness".into(), format_length_mm(thickness.0));
                }
                match construction.draft {
                    RibDraft::Angle(draft) => {
                        parameters.insert("Draft".into(), format_angle_rad(draft.0));
                    }
                    RibDraft::None => {
                        parameters.remove("Draft");
                    }
                    RibDraft::Unresolved => {}
                }
                let mut properties = feature.source_properties.clone();
                if let Some(profile) = &construction.profile {
                    let profile_source = profile_source(profile, &record_sources, &sketch_sources)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "SLDPRT feature {} references a missing rib profile",
                                feature.id
                            ))
                        })?;
                    properties.insert("Profile".into(), profile_source);
                }
                if let Some(direction) = construction.direction {
                    require_direction(direction, &feature.id, "rib direction")?;
                    properties.insert("Direction".into(), format_vector3(direction));
                }
                if let Some(side) = construction.side {
                    properties.insert("BothSides".into(), (side == RibSide::Centered).to_string());
                }
                if *op != BooleanOp::Unresolved {
                    properties.insert(
                        "Operation".into(),
                        resolved_boolean_op(*op, &feature.id)?.into(),
                    );
                }
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Rib".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Pattern { seeds, pattern } => {
                let expected_form = match pattern {
                    PatternKind::Unresolved { form } => *form,
                    PatternKind::Linear { .. } | PatternKind::LinearOffsets { .. } => {
                        Some(PatternForm::Linear)
                    }
                    PatternKind::Circular { .. } | PatternKind::CircularAngles { .. } => {
                        Some(PatternForm::Circular)
                    }
                    PatternKind::CurveDriven { .. } => Some(PatternForm::CurveDriven),
                    PatternKind::Mirror { .. } => Some(PatternForm::Mirror),
                    PatternKind::Scale { .. } => Some(PatternForm::Scale),
                    PatternKind::Composite { .. } => Some(PatternForm::Composite),
                };
                if existing.as_deref().is_some_and(|record| {
                    expected_form.is_some_and(|form| pattern_form(record) != Some(form))
                }) {
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
                if seed_sources.is_empty()
                    && (!matches!(
                        expected_form,
                        Some(PatternForm::Linear | PatternForm::CurveDriven)
                    ) || existing.is_none())
                    && !matches!(pattern, PatternKind::Unresolved { .. })
                {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has no pattern seeds",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let mut properties = feature.source_properties.clone();
                if !seed_sources.is_empty() {
                    properties.insert("Seeds".into(), seed_sources.join(","));
                }
                match pattern {
                    PatternKind::Unresolved { .. } => {
                        if existing.is_none() {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has unresolved pattern construction",
                                feature.id
                            )));
                        }
                    }
                    PatternKind::Linear {
                        direction,
                        spacing,
                        count,
                    } => {
                        match direction {
                            Some(direction) => {
                                require_direction(*direction, &feature.id, "pattern")?;
                                properties.insert("Direction".into(), format_vector3(*direction));
                            }
                            None if existing.is_some() => {}
                            None => {
                                return Err(CodecError::NotImplemented(format!(
                                    "SLDPRT feature {} has an unresolved pattern direction",
                                    feature.id
                                )));
                            }
                        }
                        require_count(*count, &feature.id)?;
                        if !spacing.0.is_finite() || spacing.0 <= 0.0 {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has invalid linear-pattern spacing",
                                feature.id
                            )));
                        }
                        let spacing_key = if parameters.contains_key("D3")
                            && !parameters.contains_key("Spacing")
                        {
                            "D3"
                        } else {
                            "Spacing"
                        };
                        let count_key =
                            if parameters.contains_key("D1") && !parameters.contains_key("Count") {
                                "D1"
                            } else {
                                "Count"
                            };
                        parameters.insert(
                            spacing_key.into(),
                            format_length_like(
                                spacing.0,
                                existing
                                    .as_deref()
                                    .and_then(|record| record.parameters.get(spacing_key))
                                    .map(String::as_str),
                            ),
                        );
                        parameters.insert(count_key.into(), count.to_string());
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
                    PatternKind::CurveDriven {
                        path,
                        spacing,
                        count,
                    } => {
                        require_count(*count, &feature.id)?;
                        if !spacing.0.is_finite() || spacing.0 <= 0.0 {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has invalid curve-pattern spacing",
                                feature.id
                            )));
                        }
                        match path {
                            Some(path) => {
                                let path = path_source(path, &record_sources, &sketch_sources)
                                    .ok_or_else(|| {
                                        CodecError::Malformed(format!(
                                            "SLDPRT feature {} references a missing pattern path",
                                            feature.id
                                        ))
                                    })?;
                                properties.insert("Path".into(), path);
                            }
                            None if existing.is_some() => {}
                            None => {
                                return Err(CodecError::NotImplemented(format!(
                                    "SLDPRT feature {} has an unresolved curve-pattern path",
                                    feature.id
                                )));
                            }
                        }
                        let spacing_key = if parameters.contains_key("D3")
                            && !parameters.contains_key("Spacing")
                        {
                            "D3"
                        } else {
                            "Spacing"
                        };
                        let count_key =
                            if parameters.contains_key("D1") && !parameters.contains_key("Count") {
                                "D1"
                            } else {
                                "Count"
                            };
                        parameters.insert(
                            spacing_key.into(),
                            format_length_like(
                                spacing.0,
                                existing
                                    .as_deref()
                                    .and_then(|record| record.parameters.get(spacing_key))
                                    .map(String::as_str),
                            ),
                        );
                        parameters.insert(count_key.into(), count.to_string());
                    }
                    PatternKind::Mirror {
                        plane_origin,
                        plane_normal,
                    } => {
                        require_direction(*plane_normal, &feature.id, "mirror plane normal")?;
                        properties.insert("PlaneOrigin".into(), format_point3_mm(*plane_origin));
                        properties.insert("PlaneNormal".into(), format_vector3(*plane_normal));
                    }
                    PatternKind::LinearOffsets { .. }
                    | PatternKind::CircularAngles { .. }
                    | PatternKind::Scale { .. }
                    | PatternKind::Composite { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses a pattern form that cannot be written",
                            feature.id
                        )));
                    }
                }
                let kind = existing.as_deref().map_or_else(
                    || match expected_form {
                        Some(PatternForm::Linear) => "LinearPattern".into(),
                        Some(PatternForm::Circular) => "CircularPattern".into(),
                        Some(PatternForm::CurveDriven) => "CrvPattern".into(),
                        Some(PatternForm::Mirror) => "Mirror".into(),
                        Some(PatternForm::Scale | PatternForm::Composite) => "Pattern".into(),
                        None => "Pattern".into(),
                    },
                    |record| record.kind.clone(),
                );
                (kind, parameters, properties)
            }
            FeatureDefinition::HelicalSweep { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses a helical sweep that cannot be written",
                    feature.id
                )));
            }
            FeatureDefinition::Binder { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses design-binder semantics that cannot be written",
                    feature.id
                )));
            }
        };
        if feature.outputs.is_empty() {
            if existing.is_none() {
                properties.remove("Scope");
            }
        } else {
            let scope = feature
                .outputs
                .iter()
                .map(|body| body_sources.get(body).cloned())
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} references a missing output body",
                        feature.id
                    ))
                })?;
            properties.insert("Scope".into(), scope.join(","));
        }
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
            if let Some(tag) = &feature.source_tag {
                existing.xml_tag.clone_from(tag);
            }
            existing.ordinal = ordinal;
            existing.name = feature.name.clone().unwrap_or_default();
            existing.kind = kind;
            existing.suppressed = suppressed;
            existing.parent_source_id = parent_source_id;
            existing.tree_parent = tree_parent;
            existing.parameters = parameters;
            existing.properties = properties;
            if existing
                .content
                .iter()
                .all(|item| matches!(item, FeatureContent::Text(_)))
            {
                existing.content = feature
                    .source_text
                    .iter()
                    .cloned()
                    .map(FeatureContent::Text)
                    .collect();
            }
            existing.text.clone_from(&feature.source_text);
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
                input_class: None,
                suppressed,
                parameters,
                dimension_properties: BTreeMap::new(),
                properties,
                text: feature.source_text.clone(),
                content: feature
                    .source_text
                    .iter()
                    .cloned()
                    .map(FeatureContent::Text)
                    .collect(),
            });
        }
    }
    synchronize_neutral_feature_content(features, parameters, &record_ids, native)?;
    let projected_features = project_features(&native.feature_histories);
    let desired_sketches = features
        .iter()
        .filter(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .map(|feature| feature.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let projected_sketches = projected_features
        .iter()
        .filter(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .map(|feature| feature.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let projected_features = projected_features
        .into_iter()
        .map(|feature| (feature.id.clone(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let projected_id = neutral_feature_id(&record_ids[&feature.id]);
        let expected = feature
            .dependencies
            .iter()
            .map(|dependency| {
                record_ids
                    .get(dependency)
                    .map_or_else(|| dependency.clone(), |record| neutral_feature_id(record))
            })
            .collect::<Vec<_>>();
        let expected = dependency_residual(feature, expected, &desired_sketches);
        let consistent = projected_features
            .get(&projected_id)
            .is_some_and(|projected| {
                let projected_dependencies = dependency_residual(
                    projected,
                    projected.dependencies.clone(),
                    &projected_sketches,
                );
                if feature.native_ref.is_some() {
                    projected_dependencies == expected
                } else {
                    expected
                        .iter()
                        .all(|dependency| projected_dependencies.contains(dependency))
                }
            });
        if !consistent {
            return Err(CodecError::Malformed(format!(
                "SLDPRT feature {} dependencies are inconsistent with its operands",
                feature.id
            )));
        }
    }
    synchronize_feature_content_order(native);
    synchronize_history_content_order(native);
    Ok(())
}

fn synchronize_neutral_feature_content(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[DesignParameter],
    record_ids: &HashMap<FeatureId, String>,
    native: &mut crate::native::SldprtNative,
) -> Result<(), CodecError> {
    let parameters = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    for feature in features {
        if feature.source_content.is_empty() {
            continue;
        }
        let content = feature
            .source_content
            .iter()
            .map(|item| match item {
                FeatureSourceContent::Text(text) => Ok(FeatureContent::Text(text.clone())),
                FeatureSourceContent::Parameter(id) => {
                    let parameter = parameters.get(id).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} content references missing parameter {}",
                            feature.id, id.0
                        ))
                    })?;
                    if parameter.owner != feature.id {
                        return Err(CodecError::Malformed(format!(
                            "SLDPRT feature {} content references parameter {} owned by another feature",
                            feature.id, id.0
                        )));
                    }
                    Ok(FeatureContent::Dimension(parameter.name.clone()))
                }
                FeatureSourceContent::Feature(id) => record_ids
                    .get(id)
                    .cloned()
                    .map(FeatureContent::Feature)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} content references missing feature {}",
                            feature.id, id
                        ))
                    }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let record_id = &record_ids[&feature.id];
        let record = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|record| &record.id == record_id)
            .ok_or_else(|| CodecError::Malformed("missing SLDPRT feature record".into()))?;
        record.content = content;
    }
    Ok(())
}

fn dependency_residual(
    feature: &cadmpeg_ir::features::Feature,
    dependencies: Vec<FeatureId>,
    sketch_features: &std::collections::HashSet<FeatureId>,
) -> Vec<FeatureId> {
    match feature.definition {
        FeatureDefinition::Extrude { .. }
        | FeatureDefinition::Revolve { .. }
        | FeatureDefinition::Sweep { .. }
        | FeatureDefinition::HelicalSweep { .. }
        | FeatureDefinition::Loft { .. }
        | FeatureDefinition::Rib { .. } => dependencies
            .into_iter()
            .filter(|dependency| !sketch_features.contains(dependency))
            .collect(),
        FeatureDefinition::Pattern { .. } => Vec::new(),
        _ => dependencies,
    }
}

fn synchronize_history_content_order(native: &mut crate::native::SldprtNative) {
    for history in &mut native.feature_histories {
        let configurations = history
            .configurations
            .iter()
            .map(|configuration| (configuration.ordinal, configuration.id.clone()))
            .collect::<Vec<_>>();
        let mut features = history
            .features
            .iter()
            .filter(|feature| feature.tree_parent.is_none() && feature.parent_source_id.is_none())
            .map(|feature| (feature.ordinal, feature.id.clone()))
            .collect::<Vec<_>>();
        let mut configurations = configurations;
        configurations.sort();
        features.sort();
        let mut configuration_index = 0;
        let mut feature_index = 0;
        for item in &mut history.content {
            match item {
                HistoryContent::Configuration(id) => {
                    *id = configurations
                        .get(configuration_index)
                        .map_or_else(String::new, |(_, id)| id.clone());
                    configuration_index += 1;
                }
                HistoryContent::Feature(id) => {
                    *id = features
                        .get(feature_index)
                        .map_or_else(String::new, |(_, id)| id.clone());
                    feature_index += 1;
                }
                HistoryContent::Text(_) => {}
            }
        }
        history.content.retain(|item| {
            !matches!(item, HistoryContent::Configuration(id) | HistoryContent::Feature(id) if id.is_empty())
        });
        history.content.extend(
            configurations
                .iter()
                .skip(configuration_index)
                .map(|(_, id)| HistoryContent::Configuration(id.clone())),
        );
        history.content.extend(
            features
                .iter()
                .skip(feature_index)
                .map(|(_, id)| HistoryContent::Feature(id.clone())),
        );
    }
}

fn synchronize_feature_content_order(native: &mut crate::native::SldprtNative) {
    for history in &mut native.feature_histories {
        let mut children = HashMap::<String, Vec<(u32, String)>>::new();
        for feature in &history.features {
            if let Some(parent) = &feature.tree_parent {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push((feature.ordinal, feature.id.clone()));
            }
        }
        for values in children.values_mut() {
            values.sort();
        }
        for feature in &mut history.features {
            let Some(children) = children.get(&feature.id) else {
                feature
                    .content
                    .retain(|item| !matches!(item, FeatureContent::Feature(_)));
                continue;
            };
            let mut index = 0;
            for item in &mut feature.content {
                if matches!(item, FeatureContent::Feature(_)) {
                    *item = FeatureContent::Feature(
                        children
                            .get(index)
                            .map_or_else(String::new, |(_, id)| id.clone()),
                    );
                    index += 1;
                }
            }
            feature
                .content
                .retain(|item| !matches!(item, FeatureContent::Feature(id) if id.is_empty()));
            feature.content.extend(
                children
                    .iter()
                    .skip(index)
                    .map(|(_, id)| FeatureContent::Feature(id.clone())),
            );
        }
    }
}

fn profile_source(
    profile: &ProfileRef,
    native: &HashMap<String, String>,
    sketches: &HashMap<cadmpeg_ir::sketches::SketchId, String>,
) -> Option<String> {
    match profile {
        ProfileRef::Unresolved => None,
        ProfileRef::Native(id) => Some(native.get(id).cloned().unwrap_or_else(|| id.clone())),
        ProfileRef::Sketch(id) => sketches.get(id).cloned(),
        ProfileRef::Faces(faces) if !faces.is_empty() => Some(
            faces
                .iter()
                .map(|face| face.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        ProfileRef::Faces(_) => None,
    }
}

fn path_source(
    path: &PathRef,
    native: &HashMap<String, String>,
    sketches: &HashMap<cadmpeg_ir::sketches::SketchId, String>,
) -> Option<String> {
    match path {
        PathRef::Unresolved => None,
        PathRef::Native(id) => Some(native.get(id).cloned().unwrap_or_else(|| id.clone())),
        PathRef::Sketch(id) => sketches.get(id).cloned(),
        PathRef::Edges(edges) if !edges.is_empty() => Some(
            edges
                .iter()
                .map(|edge| edge.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        PathRef::Curves(curves) if !curves.is_empty() => Some(
            curves
                .iter()
                .map(|curve| curve.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        PathRef::Edges(_) | PathRef::Curves(_) => None,
    }
}

fn extrude_op(kind: &str) -> Option<BooleanOp> {
    let kind = kind.trim().to_ascii_lowercase();
    if kind == "bossextrude" {
        Some(BooleanOp::Join)
    } else if kind == "cutextrude" {
        Some(BooleanOp::Cut)
    } else {
        None
    }
}

fn loft_op(kind: &str) -> Option<BooleanOp> {
    match kind.to_ascii_lowercase().as_str() {
        "bossloft" | "boundaryboss" => Some(BooleanOp::Join),
        "cutloft" | "boundarycut" => Some(BooleanOp::Cut),
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
    if let Some(tag) = feature
        .source_tag
        .as_ref()
        .filter(|tag| valid_xml_name(tag))
    {
        return tag.clone();
    }
    let tag = match &feature.definition {
        FeatureDefinition::TreeNode { .. } => "Feature",
        FeatureDefinition::DatumPrincipalPlane { .. } => "Feature",
        FeatureDefinition::DatumPlane { .. } => "ReferencePlane",
        FeatureDefinition::DatumPlaneUnresolved => "ReferencePlane",
        FeatureDefinition::SpatialSketch { .. } => "3DSketch",
        FeatureDefinition::DatumOffsetPlane { .. } => "Feature",
        FeatureDefinition::DatumAxis { .. } => "ReferenceAxis",
        FeatureDefinition::DatumPoint { .. } => "ReferencePoint",
        FeatureDefinition::DatumPointUnresolved => "ReferencePoint",
        FeatureDefinition::DatumCoordinateSystem { .. } => "CoordinateSystem",
        FeatureDefinition::DatumCoordinateSystemUnresolved => "CoordinateSystem",
        FeatureDefinition::Block { .. } => "Block",
        FeatureDefinition::EquationCurve { .. } => "EquationDrivenCurve",
        FeatureDefinition::ProjectedCurve { .. } => "ProjectedCurve",
        FeatureDefinition::CompositeCurve { .. } => "CompositeCurve",
        FeatureDefinition::Helix { .. } | FeatureDefinition::HelixNativeAxis { .. } => "Helix",
        FeatureDefinition::Wrap { .. } => "Wrap",
        FeatureDefinition::Sketch { .. } => "Sketch",
        FeatureDefinition::StoredGeometry => "Feature",
        FeatureDefinition::ExtractBody { .. } => "Feature",
        FeatureDefinition::DerivedGeometry { .. } => "Feature",
        FeatureDefinition::ImportedGeometry { .. } => "Feature",
        FeatureDefinition::Primitive { .. } => "Primitive",
        FeatureDefinition::Extrude { .. } => "Extrusion",
        FeatureDefinition::Revolve { .. } => "Revolve",
        FeatureDefinition::Sweep {
            mode: SweepMode::Surface,
            ..
        } => "Surface-Sweep",
        FeatureDefinition::Sweep { .. } => "Sweep",
        FeatureDefinition::HelicalSweep { .. } => "Helix",
        FeatureDefinition::Binder { .. } => "Feature",
        FeatureDefinition::LoftUnresolved => "Loft",
        FeatureDefinition::FreeformSurfaceUnresolved => "Feature",
        FeatureDefinition::DraftUnresolved => "Draft",
        FeatureDefinition::Loft { .. } => "Loft",
        FeatureDefinition::Rib { .. } => "Rib",
        FeatureDefinition::Fillet { .. } => "Fillet",
        FeatureDefinition::FaceBlend { .. } => "FaceBlend",
        FeatureDefinition::Chamfer { .. } => "Chamfer",
        FeatureDefinition::Shell { .. } => "Shell",
        FeatureDefinition::Thicken { .. } => "Thicken",
        FeatureDefinition::OffsetSurface { .. } => "OffsetSurface",
        FeatureDefinition::KnitSurface { .. } => "KnitSurface",
        FeatureDefinition::SewBodies { .. } => "Feature",
        FeatureDefinition::TrimBodies { .. } => "Feature",
        FeatureDefinition::FilledSurface { .. } => "FilledSurface",
        FeatureDefinition::TrimSurface { .. } => "TrimSurface",
        FeatureDefinition::ExtendSurface { .. } => "ExtendSurface",
        FeatureDefinition::RuledSurface { .. } => "RuledSurface",
        FeatureDefinition::Draft { .. } => "Draft",
        FeatureDefinition::Combine { .. } => "Combine",
        FeatureDefinition::CutWithSurface { .. } => "CutWithSurface",
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::Unresolved,
            ..
        } => "Feature",
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::DeleteSelected,
            ..
        } => "DeleteBody",
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::KeepSelected,
            ..
        } => "KeepBody",
        FeatureDefinition::DeleteFace { .. } => "DeleteFace",
        FeatureDefinition::ReplaceFace { .. } => "ReplaceFace",
        FeatureDefinition::MoveFace { .. } => "MoveFace",
        FeatureDefinition::MoveBody { .. } => "MoveBody",
        FeatureDefinition::Dome { .. } => "Dome",
        FeatureDefinition::Flex { .. } => "Flex",
        FeatureDefinition::Scale { .. } => "Scale",
        FeatureDefinition::OffsetShape { .. } => "Offset",
        FeatureDefinition::Compound { .. } => "Compound",
        FeatureDefinition::RefineShape { .. } => "Refine",
        FeatureDefinition::ReverseShape { .. } => "Reverse",
        FeatureDefinition::RuledBetweenCurves { .. } => "RuledSurface",
        FeatureDefinition::SectionShape { .. } => "Section",
        FeatureDefinition::MirrorShape { .. } => "Mirror",
        FeatureDefinition::ProjectOnSurface { .. } => "ProjectOnSurface",
        FeatureDefinition::Hole { .. } => "Hole",
        FeatureDefinition::Pattern {
            pattern: PatternKind::Mirror { .. },
            ..
        } => "Mirror",
        FeatureDefinition::Pattern { .. } => "Pattern",
        FeatureDefinition::PostProcess { .. } => "Feature",
        FeatureDefinition::PointGeometry { .. } => "Point",
        FeatureDefinition::LineSegment { .. } => "Line",
        FeatureDefinition::CircularArc { .. } => "Circle",
        FeatureDefinition::EllipticArc { .. } => "Ellipse",
        FeatureDefinition::Polyline { .. } => "Polyline",
        FeatureDefinition::RegularPolygonCurve { .. } => "Polygon",
        FeatureDefinition::PlanarPatch { .. } => "Plane",
        FeatureDefinition::FaceFromShapes { .. } => "Face",
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
