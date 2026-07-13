// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

use crate::container::ContainerScan;
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::features::{
    Angle, BodySelection, BooleanOp, ChamferSpec, ConfigurationId, DesignConfiguration,
    DesignParameter, EdgeSelection, Extent, FaceMotion, FaceSelection, FeatureDefinition,
    FeatureId, FlexMode, HoleKind, Length, ParameterId, ParameterValue, PathRef, PatternKind,
    ProfileRef, RadiusSpec, VariableRadius,
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
                    dependencies: project_feature_dependencies(feature, &by_source),
                    source_properties: feature.properties.clone(),
                    source_tag: Some(feature.xml_tag.clone()),
                    source_text: feature.text.clone(),
                    outputs: Vec::new(),
                    definition: project_definition(feature, &by_source, &native_by_source),
                    native_ref: Some(feature.id.clone()),
                })
        })
        .collect()
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
            bodies: Vec::new(),
            native_ref: Some(configuration.id.clone()),
        })
        .collect()
}

/// Project every native feature dimension into the neutral parameter arena.
pub fn project_parameters(histories: &[FeatureHistory]) -> Vec<DesignParameter> {
    let mut parameters = histories
        .iter()
        .flat_map(|history| &history.features)
        .flat_map(|feature| {
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
            names.into_iter().enumerate().map(move |(ordinal, name)| {
                let expression = &feature.parameters[&name];
                let key = feature
                    .id
                    .strip_prefix("sldprt:history:feature#")
                    .unwrap_or(&feature.id);
                let properties = feature
                    .dimension_properties
                    .get(&name)
                    .cloned()
                    .unwrap_or_default();
                let value = properties
                    .get("Value")
                    .and_then(|value| parse_parameter_literal(value))
                    .or_else(|| parse_parameter_literal(expression));
                DesignParameter {
                    id: ParameterId(format!("sldprt:model:parameter#{key}:{ordinal}")),
                    owner: neutral_feature_id(&feature.id),
                    ordinal: ordinal as u32,
                    properties,
                    name,
                    expression: expression.clone(),
                    value,
                    dependencies: Vec::new(),
                }
            })
        })
        .collect::<Vec<_>>();
    populate_parameter_dependencies(&mut parameters);
    parameters
}

fn populate_parameter_dependencies(parameters: &mut [DesignParameter]) {
    let mut aliases = HashMap::<String, Option<ParameterId>>::new();
    for parameter in parameters.iter() {
        for alias in std::iter::once(parameter.id.0.as_str())
            .chain(std::iter::once(parameter.name.as_str()))
            .chain(parameter.properties.get("EquationId").map(String::as_str))
        {
            aliases
                .entry(alias.to_string())
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
            FeatureDefinition::Extrude {
                profile, extent, ..
            } => {
                resolve_profile_ref(profile, &face_ids);
                if let Extent::ToFace { face } = extent {
                    resolve_face_selection(face, &face_ids);
                }
            }
            FeatureDefinition::Revolve { profile, .. } | FeatureDefinition::Rib { profile, .. } => {
                resolve_profile_ref(profile, &face_ids);
            }
            FeatureDefinition::Sweep { profile, path, .. } => {
                resolve_profile_ref(profile, &face_ids);
                resolve_path_ref(path, &edge_ids, &curve_ids);
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
            FeatureDefinition::DeleteFace { faces, .. }
            | FeatureDefinition::MoveFace { faces, .. }
            | FeatureDefinition::Dome { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::Hole {
                face: Some(face), ..
            } => {
                resolve_face_selection(face, &face_ids);
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
        FeatureDefinition::Extrude { profile, .. }
        | FeatureDefinition::Revolve { profile, .. }
        | FeatureDefinition::Rib { profile, .. } => bind_profile(profile),
        FeatureDefinition::Sweep { profile, path, .. } => bind_profile(profile) | bind_path(path),
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
    if feature_family(feature, "Sketch") {
        return FeatureDefinition::Sketch { sketch: None };
    }
    if feature_family(feature, "ReferencePlane") {
        return project_datum_plane(feature).unwrap_or_else(|| native_definition(feature));
    }
    if feature_family(feature, "ReferenceAxis") {
        return project_datum_axis(feature).unwrap_or_else(|| native_definition(feature));
    }
    if feature_family(feature, "ReferencePoint") {
        return project_datum_point(feature).unwrap_or_else(|| native_definition(feature));
    }
    if is_extrude(feature) {
        project_extrude(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Fillet") {
        project_fillet(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Chamfer") {
        project_chamfer(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Shell") {
        project_shell(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Draft") {
        project_draft(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Combine") {
        project_combine(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "DeleteFace") {
        project_delete_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "MoveFace") {
        project_move_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Dome") {
        project_dome(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Flex") {
        project_flex(feature).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Hole") {
        project_hole(feature).unwrap_or_else(|| native_definition(feature))
    } else if is_revolve(feature) {
        project_revolve(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if pattern_form(feature).is_some() {
        project_pattern(feature, by_source).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Sweep") {
        project_sweep(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Loft") {
        project_loft(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if feature_family(feature, "Rib") {
        project_rib(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else {
        native_definition(feature)
    }
}

fn feature_family(feature: &Feature, family: &str) -> bool {
    feature.kind.eq_ignore_ascii_case(family) || feature.xml_tag.eq_ignore_ascii_case(family)
}

fn is_extrude(feature: &Feature) -> bool {
    extrude_op(&feature.kind).is_some()
        || feature.xml_tag.eq_ignore_ascii_case("Extrusion")
            && feature
                .properties
                .get("Operation")
                .and_then(|operation| parse_boolean_op(operation))
                .is_some()
}

fn is_revolve(feature: &Feature) -> bool {
    (feature.xml_tag.eq_ignore_ascii_case("Revolve")
        || feature.xml_tag.eq_ignore_ascii_case("Revolution"))
        && feature
            .properties
            .get("Operation")
            .and_then(|operation| parse_boolean_op(operation))
            .is_some()
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
            .and_then(|value| parse_positive_length_mm(value))
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
        .and_then(|value| parse_positive_length_mm(value))
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
                let radius = parse_positive_length_mm(radius)?;
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
        edges: feature
            .properties
            .get("Edges")
            .cloned()
            .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
        radius,
    })
}

fn project_rib(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let profile = feature.properties.get("Profile")?;
    let profile = native_by_source
        .get(profile.as_str())
        .map_or_else(|| profile.clone(), |id| (*id).to_string());
    let direction = parse_vector3(feature.properties.get("Direction")?)?;
    if !valid_direction(direction) {
        return None;
    }
    let draft = match feature.parameters.get("Draft") {
        Some(value) => Some(Angle(parse_angle_rad(value)?)),
        None => None,
    };
    Some(FeatureDefinition::Rib {
        profile: ProfileRef::Native(profile),
        direction,
        thickness: Length(
            feature
                .parameters
                .get("Thickness")
                .and_then(|value| parse_positive_length_mm(value))?,
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
    let profile = feature.properties.get("Profile")?;
    let profile = native_by_source
        .get(profile.as_str())
        .map_or_else(|| profile.clone(), |id| (*id).to_string());
    let path = feature.properties.get("Path")?;
    let path = native_by_source
        .get(path.as_str())
        .map_or_else(|| path.clone(), |id| (*id).to_string());
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
                .filter(|value| value.is_finite() && *value > 0.0)?,
        ),
        None => None,
    };
    Some(FeatureDefinition::Sweep {
        profile: ProfileRef::Native(profile),
        path: PathRef::Native(path),
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
    let parse = |form: &str| match form.to_ascii_lowercase().as_str() {
        "linear" | "linearpattern" => Some(PatternForm::Linear),
        "circular" | "circularpattern" => Some(PatternForm::Circular),
        "mirror" => Some(PatternForm::Mirror),
        _ => None,
    };
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
            direction: parse_valid_direction(feature.properties.get("Direction")?)?,
            spacing: Length(
                feature
                    .parameters
                    .get("Spacing")
                    .and_then(|value| parse_positive_length_mm(value))?,
            ),
            count: parse_count(feature.parameters.get("Count")?)?,
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
        PatternForm::Mirror => PatternKind::Mirror {
            plane_origin: parse_point3_mm(feature.properties.get("PlaneOrigin")?)?,
            plane_normal: parse_valid_direction(feature.properties.get("PlaneNormal")?)?,
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
            .and_then(|value| parse_positive_angle_rad(value))
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
        .and_then(|value| parse_positive_length_mm(value))?;
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
                    .and_then(|value| parse_positive_length_mm(value))?,
            ),
            depth: Length(
                feature
                    .parameters
                    .get("CounterboreDepth")
                    .and_then(|value| parse_positive_length_mm(value))?,
            ),
        }
    } else if has_countersink {
        HoleKind::Countersink {
            diameter: Length(
                feature
                    .parameters
                    .get("CountersinkDiameter")
                    .and_then(|value| parse_positive_length_mm(value))?,
            ),
            angle: Angle(
                feature
                    .parameters
                    .get("CountersinkAngle")
                    .and_then(|value| parse_bounded_angle_rad(value))?,
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
                    .and_then(|value| parse_positive_length_mm(value))?,
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
            .map(FaceSelection::Native),
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
        .and_then(|value| parse_positive_length_mm(value))?;
    let outward = parse_bool(feature.properties.get("Outward")?)?;
    Some(FeatureDefinition::Shell {
        removed_faces: feature
            .properties
            .get("RemovedFaces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
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

fn project_dome(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::Dome {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        height: Length(
            feature
                .parameters
                .get("Height")
                .and_then(|value| parse_positive_length_mm(value))?,
        ),
        elliptical: parse_bool(feature.properties.get("Elliptical")?)?,
        reverse: parse_bool(feature.properties.get("Reverse")?)?,
    })
}

fn project_flex(feature: &Feature) -> Option<FeatureDefinition> {
    let axis = parse_vector3(
        feature
            .properties
            .get("Axis")
            .or_else(|| feature.properties.get("AxisDirection"))?,
    )?;
    if !(axis.norm().is_finite() && axis.norm() > 0.0) {
        return None;
    }
    let mode = match feature
        .properties
        .get("Mode")?
        .to_ascii_lowercase()
        .as_str()
    {
        "bending" | "bend" => FlexMode::Bending {
            angle: Angle(
                feature
                    .parameters
                    .get("Angle")
                    .and_then(|value| parse_angle_rad(value))?,
            ),
        },
        "twisting" | "twist" => FlexMode::Twisting {
            angle: Angle(
                feature
                    .parameters
                    .get("Angle")
                    .and_then(|value| parse_angle_rad(value))?,
            ),
        },
        "tapering" | "taper" => FlexMode::Tapering {
            factor: feature.parameters.get("Factor")?.trim().parse().ok()?,
        },
        "stretching" | "stretch" => FlexMode::Stretching {
            distance: Length(
                feature
                    .parameters
                    .get("Distance")
                    .and_then(|value| parse_length_mm(value))?,
            ),
        },
        _ => return None,
    };
    let valid = match mode {
        FlexMode::Bending { angle } | FlexMode::Twisting { angle } => angle.0.is_finite(),
        FlexMode::Tapering { factor } => factor.is_finite() && factor > 0.0,
        FlexMode::Stretching { distance } => distance.0.is_finite(),
    };
    if !valid {
        return None;
    }
    Some(FeatureDefinition::Flex { axis, mode })
}

fn project_chamfer(feature: &Feature) -> Option<FeatureDefinition> {
    let length = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_positive_length_mm(value))
            .map(Length)
    };
    let spec = if let Some(value) = feature.parameters.get("Angle") {
        ChamferSpec::DistanceAngle {
            distance: length("Distance")?,
            angle: Angle(parse_bounded_angle_rad(value)?),
        }
    } else if let (Some(first), Some(second)) = (length("Distance1"), length("Distance2")) {
        ChamferSpec::TwoDistances { first, second }
    } else {
        ChamferSpec::Distance {
            distance: length("Distance")?,
        }
    };
    Some(FeatureDefinition::Chamfer {
        edges: feature
            .properties
            .get("Edges")
            .cloned()
            .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
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
            return number
                .trim()
                .parse::<f64>()
                .ok()
                .map(|value| value * scale)
                .filter(|value| value.is_finite());
        }
    }
    None
}

fn parse_positive_length_mm(value: &str) -> Option<f64> {
    parse_length_mm(value).filter(|value| *value > 0.0)
}

fn format_length_mm(value: f64) -> String {
    format!("{value}mm")
}

fn parse_angle_rad(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix("deg") {
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
        return sync_neutral_features(&ir.model.features, &ir.model.bodies, native);
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
        (true, false) => sync_neutral_features(&ir.model.features, &ir.model.bodies, native),
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
    if let Some(native) = native.as_ref() {
        let original = project_parameters(&native.feature_histories);
        rewrite_renamed_parameter_references(&mut parameters, &original);
    }
    let mut projected_dependencies = parameters.clone();
    populate_parameter_dependencies(&mut projected_dependencies);
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
        if parse_parameter_literal(&parameter.expression)
            .is_some_and(|literal| parameter.value.as_ref() != Some(&literal))
        {
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
    Ok(())
}

fn rewrite_renamed_parameter_references(
    parameters: &mut [DesignParameter],
    original: &[DesignParameter],
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

/// Apply neutral native-feature edits to the `SolidWorks` history used for writing.
pub fn sync_neutral_features(
    features: &[cadmpeg_ir::features::Feature],
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
        let (kind, parameters, mut properties) = match &feature.definition {
            FeatureDefinition::Native {
                kind,
                parameters,
                properties,
            } => {
                let mut merged = feature.source_properties.clone();
                merged.extend(properties.clone());
                (kind.clone(), parameters.clone(), merged)
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
            FeatureDefinition::Sketch { .. } => {
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
                    existing
                        .as_deref()
                        .map_or_else(|| "Sketch".into(), |record| record.kind.clone()),
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
            } => {
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
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let mut properties = feature.source_properties.clone();
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
                let kind = existing.as_deref().map_or_else(
                    || match op {
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
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Fillet"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported fillet semantics",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
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
            FeatureDefinition::Chamfer { edges, spec } => {
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
                    .is_some_and(|record| !feature_family(record, "Chamfer"))
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
                match spec {
                    ChamferSpec::Distance { distance } => {
                        if existing.is_some()
                            && (parameters.contains_key("Distance1")
                                || parameters.contains_key("Distance2")
                                || parameters.contains_key("Angle"))
                        {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} changes chamfer form",
                                feature.id
                            )));
                        }
                        parameters.insert("Distance".into(), format_length_mm(distance.0));
                    }
                    ChamferSpec::TwoDistances { first, second } => {
                        if existing.is_some()
                            && (!parameters.contains_key("Distance1")
                                || !parameters.contains_key("Distance2")
                                || parameters.contains_key("Angle"))
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
                        if existing.is_some()
                            && (!parameters.contains_key("Distance")
                                || !parameters.contains_key("Angle")
                                || parameters.contains_key("Distance1")
                                || parameters.contains_key("Distance2"))
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
            FeatureDefinition::Shell {
                removed_faces,
                thickness,
                outward,
            } => {
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
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert("Thickness".into(), format_length_mm(thickness.0));
                let mut properties = feature.source_properties.clone();
                if let Some(selection) = selection {
                    write_native_selection(
                        &mut properties,
                        "RemovedFaces",
                        &selection,
                        existing.as_deref().map_or("", |record| record.id.as_str()),
                    );
                }
                properties.insert("Outward".into(), outward.to_string());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Shell".into(), |record| record.kind.clone()),
                    parameters,
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
                properties.insert("Operation".into(), format_boolean_op(*op).into());
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
                    || faces.is_none()
                {
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
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert("Height".into(), format_length_mm(height.0));
                let mut properties = feature.source_properties.clone();
                properties.insert("Faces".into(), faces.expect("checked above"));
                properties.insert("Elliptical".into(), elliptical.to_string());
                properties.insert("Reverse".into(), reverse.to_string());
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
                require_direction(*axis, &feature.id, "flex axis")?;
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.remove("Angle");
                parameters.remove("Factor");
                parameters.remove("Distance");
                let mut properties = feature.source_properties.clone();
                properties.insert("Axis".into(), format_vector3(*axis));
                properties.remove("AxisDirection");
                match mode {
                    FlexMode::Bending { angle } => {
                        if !angle.0.is_finite() {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has a non-finite flex angle",
                                feature.id
                            )));
                        }
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
            FeatureDefinition::Hole {
                face,
                position,
                direction,
                kind,
                diameter,
                extent,
            } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Hole"))
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported hole semantics",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
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
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Hole".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle,
                op,
            } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !is_revolve(record))
                {
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
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let mut properties = feature.source_properties.clone();
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
                path,
                op,
                twist,
                scale,
            } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Sweep"))
                {
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
                properties.insert("Profile".into(), profile_source.clone());
                properties.insert("Path".into(), path_source.clone());
                properties.insert("Operation".into(), format_boolean_op(*op).into());
                (
                    existing
                        .as_deref()
                        .map_or_else(|| "Sweep".into(), |record| record.kind.clone()),
                    parameters,
                    properties,
                )
            }
            FeatureDefinition::Loft {
                profiles,
                guides,
                op,
                closed,
            } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Loft"))
                    || profiles.len() < 2
                {
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
                let mut properties = feature.source_properties.clone();
                properties.insert("Profiles".into(), profile_sources.join(","));
                if guide_sources.is_empty() {
                    properties.remove("Guides");
                } else {
                    properties.insert("Guides".into(), guide_sources.join(","));
                }
                properties.insert("Operation".into(), format_boolean_op(*op).into());
                properties.insert("Closed".into(), closed.to_string());
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
            FeatureDefinition::Rib {
                profile,
                direction,
                thickness,
                both_sides,
                draft,
                op,
            } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| !feature_family(record, "Rib"))
                {
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
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert("Thickness".into(), format_length_mm(thickness.0));
                match draft {
                    Some(draft) => {
                        parameters.insert("Draft".into(), format_angle_rad(draft.0));
                    }
                    None => {
                        parameters.remove("Draft");
                    }
                }
                let mut properties = feature.source_properties.clone();
                properties.insert("Profile".into(), profile_source.clone());
                properties.insert("Direction".into(), format_vector3(*direction));
                properties.insert("BothSides".into(), both_sides.to_string());
                properties.insert("Operation".into(), format_boolean_op(*op).into());
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
                    PatternKind::Linear { .. } => PatternForm::Linear,
                    PatternKind::Circular { .. } => PatternForm::Circular,
                    PatternKind::Mirror { .. } => PatternForm::Mirror,
                };
                if existing
                    .as_deref()
                    .is_some_and(|record| pattern_form(record) != Some(expected_form))
                {
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
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                let mut properties = feature.source_properties.clone();
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
                let kind = existing.as_deref().map_or_else(
                    || match expected_form {
                        PatternForm::Linear => "LinearPattern".into(),
                        PatternForm::Circular => "CircularPattern".into(),
                        PatternForm::Mirror => "Mirror".into(),
                    },
                    |record| record.kind.clone(),
                );
                (kind, parameters, properties)
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
            existing.suppressed = feature.suppressed;
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
                suppressed: feature.suppressed,
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

fn dependency_residual(
    feature: &cadmpeg_ir::features::Feature,
    dependencies: Vec<FeatureId>,
    sketch_features: &std::collections::HashSet<FeatureId>,
) -> Vec<FeatureId> {
    match feature.definition {
        FeatureDefinition::Extrude { .. }
        | FeatureDefinition::Revolve { .. }
        | FeatureDefinition::Sweep { .. }
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
    if let Some(tag) = feature
        .source_tag
        .as_ref()
        .filter(|tag| valid_xml_name(tag))
    {
        return tag.clone();
    }
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
        FeatureDefinition::Flex { .. } => "Flex",
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
