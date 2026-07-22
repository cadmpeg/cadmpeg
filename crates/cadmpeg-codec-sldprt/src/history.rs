// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

use crate::classification::{
    classify, native_object_class, principal_plane, FeatureClass, NativeClassKind,
};
use crate::container::ContainerScan;
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::features::{
    Angle, AxisAngle, BodyRetentionMode, BodySelection, BooleanOp, ChamferForm, ChamferSpec,
    ConfigurationId, CosmeticThreadExtent, DesignConfiguration, DesignParameter, DimensionDisplay,
    EdgeSelection, Extent, FaceMotion, FaceSelection, FeatureDefinition, FeatureId,
    FeatureSourceContent, FeatureTreeNodeRole, FlexForm, FlexMode, HoleForm, HoleKind, Length,
    ParameterId, ParameterValue, PathRef, PatternForm, PatternKind, PatternSeed, ProfileRef,
    RadiusForm, RadiusSpec, RevolutionAxis, RevolutionConstruction, RibConstruction, RibDraft,
    RibSide, RuledSurfaceMode, ScaleCenter, ScaleFactors, SketchSpace, SweepMode, VariableRadius,
    VertexSelection, WrapMode,
};
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

const FEATURE_REFERENCE_PROPERTIES: &[&str] = &[
    "Profile",
    "Path",
    "Profiles",
    "Guides",
    "Seeds",
    "Dependency",
    "Dependencies",
    "ParentFeatures",
    "Planes",
    "DissectableChildren",
    "BlockDefinition",
];

pub fn histories(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<FeatureHistory> {
    scan.sections()
        .filter_map(|section| {
            let source = section.ordinal();
            let text = xml_text(section.payload())?;
            let doc = roxmltree::Document::parse(&text).ok()?;
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                return None;
            }
            let stream = section.display_name();
            let parent = format!("sldprt:history:feature-history#{source}");
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .map(|(ordinal, node)| {
                    let id = format!("sldprt:history:configuration#{source}:{ordinal}");
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
                        format!("sldprt:history:feature#{source}:{ordinal}"),
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
                        format!("sldprt:history:configuration#{source}:{ordinal}"),
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

pub(crate) fn enrich_scene_classes(
    histories: &mut [FeatureHistory],
    scene_classes: &crate::tessellation::SceneFeatureClasses,
) {
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let Some(source) = feature.source_id.as_deref() else {
            continue;
        };
        if feature.input_class.is_none() && classless_builtin_node(feature) {
            feature.input_class = scene_classes.by_source.get(source).cloned();
        }
    }
    for history in histories {
        let mut groups = HashMap::<&str, Vec<usize>>::new();
        for (index, feature) in history.features.iter().enumerate() {
            if classless_builtin_node(feature) {
                groups.entry(feature.kind.as_str()).or_default().push(index);
            }
        }
        let proposals = scene_classes
            .anonymous_counts
            .iter()
            .filter_map(|(class, count)| {
                let candidates = groups
                    .values()
                    .filter(|indices| indices.len() == *count)
                    .collect::<Vec<_>>();
                let [indices] = candidates.as_slice() else {
                    return None;
                };
                Some(((*indices).clone(), class))
            })
            .collect::<Vec<_>>();
        for (indices, class) in &proposals {
            if proposals
                .iter()
                .filter(|(candidate, _)| candidate == indices)
                .count()
                != 1
            {
                continue;
            }
            for &index in indices {
                history.features[index].input_class = Some((*class).clone());
            }
        }
    }
}

/// Project native Keywords records into the neutral feature arena.
pub fn project_features(histories: &[FeatureHistory]) -> Vec<cadmpeg_ir::features::Feature> {
    let mut features = histories
        .iter()
        .flat_map(|history| {
            let source_bindings = unique_source_bindings(history);
            let by_source = source_bindings
                .iter()
                .filter_map(|(source, binding)| {
                    binding
                        .as_ref()
                        .map(|(_, neutral)| (*source, neutral.clone()))
                })
                .collect::<HashMap<_, _>>();
            let by_native = history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
                .map(|feature| (feature.id.as_str(), neutral_feature_id(&feature.id)))
                .collect::<HashMap<_, _>>();
            let native_by_source = source_bindings
                .iter()
                .filter_map(|(source, binding)| {
                    binding.as_ref().map(|(native, _)| (*source, *native))
                })
                .collect::<HashMap<_, _>>();
            let features_by_source = history
                .features
                .iter()
                .filter_map(|feature| Some((feature.source_id.as_deref()?, feature)))
                .collect::<HashMap<_, _>>();
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
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
                    source_content: project_feature_content(feature, &by_native),
                    outputs: Vec::new(),
                    definition: project_definition(
                        feature,
                        &by_source,
                        &native_by_source,
                        &features_by_source,
                        &history.features,
                    ),
                    native_ref: Some(feature.id.clone()),
                })
        })
        .collect::<Vec<_>>();
    bind_native_profile_features(&mut features, histories);
    features
}

fn bind_native_profile_features(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
) {
    let construction_native_refs = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| {
            matches!(
                classify(feature),
                Some(
                    FeatureClass::Sketch
                        | FeatureClass::SketchBlockInstance
                        | FeatureClass::EquationCurve
                        | FeatureClass::ProjectedCurve
                        | FeatureClass::CompositeCurve
                )
            )
        })
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| {
            let native = feature.native_ref.as_deref()?;
            construction_native_refs
                .contains(native)
                .then_some((native.to_string(), feature.id.clone()))
        })
        .collect::<HashMap<_, _>>();

    for feature in features {
        let mut dependencies = Vec::new();
        let mut bind = |profile: &mut ProfileRef| {
            let ProfileRef::Native(native) = profile else {
                return;
            };
            let Some(target) = feature_ids_by_native.get(native.as_str()) else {
                return;
            };
            *profile = ProfileRef::Feature(target.clone());
            dependencies.push(target.clone());
        };
        match &mut feature.definition {
            FeatureDefinition::Extrude { profile, .. }
            | FeatureDefinition::Wrap { profile, .. } => bind(profile),
            FeatureDefinition::Revolve { construction, .. } => {
                if let Some(profile) = &mut construction.profile {
                    bind(profile);
                }
            }
            FeatureDefinition::Rib { construction, .. } => {
                if let Some(profile) = &mut construction.profile {
                    bind(profile);
                }
            }
            FeatureDefinition::Sweep { profile, .. } => {
                if let Some(profile) = profile {
                    bind(profile);
                }
            }
            FeatureDefinition::Loft { profiles, .. } => {
                for profile in profiles {
                    bind(profile);
                }
            }
            _ => {}
        }
        for dependency in dependencies {
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                feature.dependencies.push(dependency);
            }
        }
    }
}

/// Project Keywords custom-property records into document-owned attributes.
pub(crate) fn custom_property_attributes(histories: &[FeatureHistory]) -> Vec<SourceAttribute> {
    histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| is_custom_property(feature))
        .map(|feature| {
            let key = feature
                .id
                .strip_prefix("sldprt:history:feature#")
                .unwrap_or(&feature.id);
            SourceAttribute {
                id: AttributeId(format!("sldprt:history:custom-property#{key}")),
                target: AttributeTarget::Document,
                name: feature.name.clone(),
                values: feature
                    .text
                    .iter()
                    .cloned()
                    .map(AttributeValue::String)
                    .collect(),
            }
        })
        .collect()
}

fn is_custom_property(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("CustomProperty")
}

fn is_history_metadata_record(feature: &Feature, features: &[Feature]) -> bool {
    if is_custom_property(feature)
        || matches!(
            feature.input_class.as_deref(),
            Some("moAlignGroup_c" | "moAttribute_c" | "moConfigCommentsFolder_c")
        )
    {
        return true;
    }
    feature.input_class.is_none()
        && feature.source_id.as_deref() == Some("-1")
        && !feature.name.is_empty()
        && features.iter().any(|candidate| {
            candidate.input_class.as_deref() == Some("moAttribute_c")
                && candidate.name.starts_with(&feature.name)
        })
}

fn unique_source_bindings(history: &FeatureHistory) -> HashMap<&str, Option<(&str, FeatureId)>> {
    let mut bindings = HashMap::new();
    for feature in &history.features {
        if is_history_metadata_record(feature, &history.features) {
            continue;
        }
        let Some(source) = feature.source_id.as_deref() else {
            continue;
        };
        let binding = (feature.id.as_str(), neutral_feature_id(&feature.id));
        bindings
            .entry(source)
            .and_modify(|existing| *existing = None)
            .or_insert(Some(binding));
    }
    bindings
}

pub(crate) fn incomplete_history_reference_features(histories: &[FeatureHistory]) -> usize {
    histories
        .iter()
        .map(|history| {
            let sources = unique_source_bindings(history);
            let native_ids = history
                .features
                .iter()
                .map(|feature| feature.id.as_str())
                .collect::<HashSet<_>>();
            history
                .features
                .iter()
                .filter(|feature| {
                    let duplicate_source = feature
                        .source_id
                        .as_deref()
                        .is_some_and(|source| sources.get(source).is_some_and(Option::is_none));
                    let parent_requested =
                        feature.tree_parent.is_some() || feature.parent_source_id.is_some();
                    let parent_resolved = feature
                        .tree_parent
                        .as_deref()
                        .is_some_and(|parent| native_ids.contains(parent))
                        || feature
                            .parent_source_id
                            .as_deref()
                            .is_some_and(|source| sources.get(source).is_some_and(Option::is_some));
                    let incomplete_content = feature.content.iter().any(|item| match item {
                        FeatureContent::Feature(child) => !native_ids.contains(child.as_str()),
                        FeatureContent::Dimension(name) => !feature.parameters.contains_key(name),
                        FeatureContent::Text(_) => false,
                    });
                    let unresolved_dependency = FEATURE_REFERENCE_PROPERTIES
                        .iter()
                        .filter_map(|name| feature.properties.get(*name))
                        .flat_map(|value| {
                            value.split(|character: char| {
                                character == ',' || character == ';' || character.is_whitespace()
                            })
                        })
                        .filter(|reference| !reference.is_empty())
                        .any(|reference| {
                            sources.get(reference).and_then(Option::as_ref).is_none_or(
                                |(_, dependency)| dependency == &neutral_feature_id(&feature.id),
                            )
                        });
                    duplicate_source
                        || (parent_requested && !parent_resolved)
                        || incomplete_content
                        || unresolved_dependency
                })
                .count()
        })
        .sum()
}

fn project_feature_content(
    feature: &Feature,
    by_native: &HashMap<&str, FeatureId>,
) -> Vec<FeatureSourceContent> {
    if feature.text.is_some() {
        return Vec::new();
    }
    let parameters = projected_parameter_names(feature)
        .into_iter()
        .enumerate()
        .map(|(ordinal, name)| (name, neutral_parameter_id(feature, ordinal)))
        .collect::<HashMap<_, _>>();
    let mut emitted_parameters = HashSet::new();
    feature
        .content
        .iter()
        .filter_map(|content| match content {
            FeatureContent::Text(text) => Some(FeatureSourceContent::Text(text.clone())),
            FeatureContent::Dimension(name) => parameters
                .get(name)
                .filter(|parameter| emitted_parameters.insert((*parameter).clone()))
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

fn projected_parameter_names(feature: &Feature) -> Vec<String> {
    let mut seen = HashSet::new();
    parameter_names(feature)
        .into_iter()
        .filter(|name| seen.insert(name.clone()))
        .collect()
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
    let owner = neutral_feature_id(&feature.id);
    let mut seen = std::collections::HashSet::new();
    FEATURE_REFERENCE_PROPERTIES
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
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(configuration.id.clone()),
        })
        .collect()
}

/// Project every native feature dimension into the neutral parameter arena.
pub fn project_parameters(histories: &[FeatureHistory]) -> Vec<DesignParameter> {
    let feature_names = histories
        .iter()
        .flat_map(|history| {
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
        })
        .filter(|feature| !feature.name.is_empty())
        .map(|feature| (neutral_feature_id(&feature.id), feature.name.clone()))
        .collect::<HashMap<_, _>>();
    let global_owners = histories
        .iter()
        .flat_map(|history| {
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
        })
        .filter(|feature| feature.kind.eq_ignore_ascii_case("EquationDriven"))
        .map(|feature| neutral_feature_id(&feature.id))
        .collect::<HashSet<_>>();
    let mut parameters = histories
        .iter()
        .flat_map(|history| {
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
        })
        .flat_map(|feature| {
            projected_parameter_names(feature)
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
    populate_parameter_dependencies(&mut parameters, &feature_names, &global_owners);
    order_parameters_by_dependencies(&mut parameters);
    evaluate_parameter_expressions(&mut parameters, &feature_names, &global_owners);
    parameters
}

pub(crate) fn global_parameter_owners(
    features: &[cadmpeg_ir::features::Feature],
) -> HashSet<FeatureId> {
    features
        .iter()
        .filter(|feature| {
            matches!(
                &feature.definition,
                FeatureDefinition::Native { kind, .. }
                    if kind.eq_ignore_ascii_case("EquationDriven")
            )
        })
        .map(|feature| feature.id.clone())
        .collect()
}

/// Replace evaluable expressions with canonical literals in a temporary history projection.
///
/// Retained native histories keep their source expressions. This normalization exists only so
/// typed feature projectors consume the same evaluated values exposed in the neutral parameter
/// arena.
pub(crate) fn apply_evaluated_parameters(histories: &mut [FeatureHistory]) {
    let evaluated = project_parameters(histories)
        .into_iter()
        .filter_map(|parameter| {
            parameter
                .value
                .map(|value| ((parameter.owner, parameter.name), value))
        })
        .collect::<HashMap<_, _>>();
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let owner = neutral_feature_id(&feature.id);
        let replacements = feature
            .parameters
            .iter()
            .filter(|(name, expression)| {
                parse_native_parameter_literal(feature, name, expression).is_none()
            })
            .filter_map(|(name, _)| {
                evaluated
                    .get(&(owner.clone(), name.clone()))
                    .map(|value| (name.clone(), format_parameter_value(value)))
            })
            .collect::<Vec<_>>();
        for (name, value) in replacements {
            feature.parameters.insert(name, value);
        }
    }
}

pub(crate) fn parse_native_parameter_literal(
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
    let cosmetic_thread = classify(feature) == Some(FeatureClass::CosmeticThread);
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
                || cosmetic_thread
        }
        "D2" if cosmetic_thread => true,
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
        _ => {
            is_extrude(feature)
                && matches!(
                    feature.properties.get("EndCondition").map(String::as_str),
                    Some("Blind" | "Symmetric")
                )
                && feature.parameters.len() == 1
                && feature.parameters.contains_key(name)
        }
    }
}

pub(crate) fn format_native_scalar(
    feature: &Feature,
    name: &str,
    value: f64,
    expression: Option<&str>,
) -> String {
    if let Some(display) = expression.and_then(dimension_display) {
        let prefix = match display {
            DimensionDisplay::Diameter => expression
                .filter(|value| value.trim().starts_with("&lt;MOD-DIAM&gt;"))
                .map_or("<MOD-DIAM>", |_| "&lt;MOD-DIAM&gt;"),
            DimensionDisplay::Radius => expression
                .filter(|value| value.trim().starts_with("&lt;MOD-RHO&gt;"))
                .map_or("<MOD-RHO>", |_| "&lt;MOD-RHO&gt;"),
        };
        format!("{prefix}{}", format_f64_literal(value * 1000.0))
    } else if native_parameter_is_length(feature, name, expression) {
        format_length_mm(value * 1000.0)
    } else if expression.and_then(parse_angle_rad).is_some() {
        format_angle_rad(value)
    } else {
        format_f64_literal(value)
    }
}

fn populate_parameter_dependencies(
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    for parameter in parameters.iter_mut() {
        let aliases = &aliases[&parameter.owner];
        let mut seen = std::collections::HashSet::new();
        parameter.dependencies = expression_identifiers(&parameter.expression)
            .filter_map(|identifier| aliases.get(&identifier).and_then(Clone::clone))
            .filter(|dependency| dependency != &parameter.id && seen.insert(dependency.clone()))
            .collect();
    }
}

fn order_parameters_by_dependencies(parameters: &mut [DesignParameter]) {
    let mut seen_owners = std::collections::HashSet::new();
    let owner_order = parameters
        .iter()
        .map(|parameter| parameter.owner.clone())
        .filter(|owner| seen_owners.insert(owner.clone()))
        .collect::<Vec<_>>();
    let parameter_owners = parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect::<HashMap<_, _>>();
    for owner in owner_order {
        let mut remaining = parameters
            .iter()
            .enumerate()
            .filter(|(_, parameter)| parameter.owner == owner)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut ordered = Vec::<usize>::with_capacity(remaining.len());
        let mut ordered_ids = std::collections::HashSet::new();
        while !remaining.is_empty() {
            let Some(position) = remaining.iter().position(|index| {
                parameters[*index].dependencies.iter().all(|dependency| {
                    parameter_owners
                        .get(dependency)
                        .is_none_or(|dependency_owner| dependency_owner != &owner)
                        || ordered_ids.contains(dependency)
                })
            }) else {
                ordered.clear();
                break;
            };
            let index = remaining.remove(position);
            ordered_ids.insert(parameters[index].id.clone());
            ordered.push(index);
        }
        for (ordinal, index) in ordered.into_iter().enumerate() {
            parameters[index].ordinal = ordinal as u32;
        }
    }
}

fn parameter_aliases(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    expression_owner: &FeatureId,
) -> HashMap<String, Option<ParameterId>> {
    fn insert(
        aliases: &mut HashMap<String, Option<ParameterId>>,
        alias: String,
        parameter: &ParameterId,
    ) {
        aliases
            .entry(alias)
            .and_modify(|candidate| {
                if candidate
                    .as_ref()
                    .is_some_and(|existing| existing != parameter)
                {
                    *candidate = None;
                }
            })
            .or_insert_with(|| Some(parameter.clone()));
    }

    let mut global = HashMap::new();
    let mut local = HashMap::new();
    let mut exact = HashMap::new();
    for parameter in parameters {
        insert(&mut exact, parameter.id.0.clone(), &parameter.id);
        let mut unqualified = vec![parameter.name.clone()];
        if let Some(equation_id) = parameter
            .properties
            .get("EquationId")
            .filter(|equation_id| !equation_id.contains('@'))
        {
            unqualified.push(equation_id.clone());
        }
        if let Some(owner_name) = feature_names.get(&parameter.owner) {
            insert(
                &mut exact,
                format!("{}@{owner_name}", parameter.name),
                &parameter.id,
            );
            if let Some(equation_id) = parameter.properties.get("EquationId") {
                let qualified = if equation_id.contains('@') {
                    equation_id.clone()
                } else {
                    format!("{equation_id}@{owner_name}")
                };
                insert(&mut exact, qualified, &parameter.id);
            }
        }
        if global_owners.contains(&parameter.owner) {
            for alias in &unqualified {
                insert(&mut global, alias.clone(), &parameter.id);
            }
        }
        if parameter.owner == *expression_owner {
            for alias in unqualified {
                insert(&mut local, alias, &parameter.id);
            }
        }
    }
    global.extend(local);
    global.extend(exact);
    global
}

fn parameter_aliases_by_owner(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> HashMap<FeatureId, HashMap<String, Option<ParameterId>>> {
    parameters
        .iter()
        .map(|parameter| parameter.owner.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|owner| {
            let aliases = parameter_aliases(parameters, feature_names, global_owners, &owner);
            (owner, aliases)
        })
        .collect()
}

fn evaluate_parameter_expressions(
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    let mut values = parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .value
                .clone()
                .map(|value| (parameter.id.clone(), value))
        })
        .collect::<HashMap<_, _>>();
    loop {
        let mut changed = false;
        for parameter in parameters
            .iter_mut()
            .filter(|parameter| parameter.value.is_none())
        {
            let aliases = &aliases[&parameter.owner];
            let Some(value) =
                ParameterExpressionParser::new(&parameter.expression, aliases, &values).parse()
            else {
                continue;
            };
            if !parameter_value_is_finite(&value) {
                continue;
            }
            values.insert(parameter.id.clone(), value.clone());
            parameter.value = Some(value);
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

struct ParameterExpressionParser<'a> {
    input: &'a str,
    offset: usize,
    aliases: &'a HashMap<String, Option<ParameterId>>,
    values: &'a HashMap<ParameterId, ParameterValue>,
}

impl<'a> ParameterExpressionParser<'a> {
    fn new(
        input: &'a str,
        aliases: &'a HashMap<String, Option<ParameterId>>,
        values: &'a HashMap<ParameterId, ParameterValue>,
    ) -> Self {
        Self {
            input,
            offset: 0,
            aliases,
            values,
        }
    }

    fn parse(mut self) -> Option<ParameterValue> {
        self.skip_space();
        self.take('=');
        self.skip_space();
        if let Some(value) = parse_parameter_literal(&self.input[self.offset..]) {
            return Some(value);
        }
        let value = self.comparison()?;
        self.skip_space();
        (self.offset == self.input.len()).then_some(value)
    }

    fn comparison(&mut self) -> Option<ParameterValue> {
        let left = self.sum()?;
        self.skip_space();
        let operator = ["<=", ">=", "<>", "=", "<", ">"]
            .into_iter()
            .find(|operator| self.input[self.offset..].starts_with(operator));
        let Some(operator) = operator else {
            return Some(left);
        };
        self.offset += operator.len();
        compare_parameter_values(&left, &self.sum()?, operator).map(ParameterValue::Boolean)
    }

    fn sum(&mut self) -> Option<ParameterValue> {
        let mut value = self.product()?;
        loop {
            self.skip_space();
            let op = self.take_one(&['+', '-']);
            let Some(op) = op else { return Some(value) };
            value = add_parameter_values(value, self.product()?, op == '-')?;
        }
    }

    fn product(&mut self) -> Option<ParameterValue> {
        let mut value = self.unary()?;
        loop {
            self.skip_space();
            let op = self.take_one(&['*', '/']);
            let Some(op) = op else { return Some(value) };
            value = multiply_parameter_values(value, self.unary()?, op == '/')?;
        }
    }

    fn unary(&mut self) -> Option<ParameterValue> {
        self.skip_space();
        if self.take('-') {
            negate_parameter_value(&self.unary()?)
        } else if self.take('+') {
            self.unary()
        } else {
            self.power()
        }
    }

    fn power(&mut self) -> Option<ParameterValue> {
        let base = self.primary()?;
        self.skip_space();
        if self.take('^') {
            exponentiate_parameter_value(&base, &self.unary()?)
        } else {
            Some(base)
        }
    }

    fn primary(&mut self) -> Option<ParameterValue> {
        self.skip_space();
        if self.take('(') {
            let value = self.comparison()?;
            self.skip_space();
            return self.take(')').then_some(value);
        }
        let (token, quoted) = self.token()?;
        if !quoted {
            self.skip_space();
            if self.take('(') {
                if token.eq_ignore_ascii_case("iif") {
                    let condition = self.comparison()?;
                    self.skip_space();
                    if !self.take(',') {
                        return None;
                    }
                    let when_true = self.comparison()?;
                    self.skip_space();
                    if !self.take(',') {
                        return None;
                    }
                    let when_false = self.comparison()?;
                    self.skip_space();
                    if !self.take(')') {
                        return None;
                    }
                    return conditional_parameter_value(&condition, when_true, when_false);
                }
                let argument = self.comparison()?;
                self.skip_space();
                if !self.take(')') {
                    return None;
                }
                return apply_parameter_function(&token, &argument)
                    .filter(parameter_value_is_finite);
            }
            if token.eq_ignore_ascii_case("pi") {
                return Some(ParameterValue::Real(std::f64::consts::PI));
            }
        }
        let referenced = || {
            self.aliases
                .get(&token)
                .and_then(Clone::clone)
                .and_then(|id| self.values.get(&id).cloned())
        };
        if quoted {
            referenced()
        } else {
            parse_parameter_literal(&token).or_else(referenced)
        }
    }

    fn token(&mut self) -> Option<(String, bool)> {
        let rest = &self.input[self.offset..];
        if let Some((marker, prefix)) = [
            ("<MOD-DIAM>", "<MOD-DIAM>"),
            ("&lt;MOD-DIAM&gt;", "<MOD-DIAM>"),
            ("<MOD-RHO>", "R"),
            ("&lt;MOD-RHO&gt;", "R"),
        ]
        .into_iter()
        .find(|(marker, _)| rest.starts_with(marker))
        {
            self.offset += marker.len();
            let (value, quoted) = self.token()?;
            return (!quoted).then(|| (format!("{prefix}{value}"), false));
        }
        if rest.starts_with('"') {
            self.offset += 1;
            let mut value = String::new();
            while self.offset < self.input.len() {
                let rest = &self.input[self.offset..];
                if rest.starts_with("\"\"") {
                    value.push('"');
                    self.offset += 2;
                } else if rest.starts_with('"') {
                    self.offset += 1;
                    return Some((value, true));
                } else {
                    let character = rest.chars().next()?;
                    value.push(character);
                    self.offset += character.len_utf8();
                }
            }
            return None;
        }
        let start = self.offset;
        let numeric = self.input[start..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit() || character == '.');
        while self.offset < self.input.len() {
            let character = self.input[self.offset..].chars().next()?;
            let exponent_sign = numeric
                && matches!(character, '+' | '-')
                && self.input[start..self.offset].ends_with(['e', 'E']);
            if character.is_whitespace() || (!exponent_sign && "+-*/^(),=<>".contains(character)) {
                break;
            }
            self.offset += character.len_utf8();
        }
        (self.offset > start).then(|| (self.input[start..self.offset].to_string(), false))
    }

    fn skip_space(&mut self) {
        while let Some(character) = self.input[self.offset..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            self.offset += character.len_utf8();
        }
    }

    fn take(&mut self, expected: char) -> bool {
        if self.input[self.offset..].starts_with(expected) {
            self.offset += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn take_one(&mut self, expected: &[char]) -> Option<char> {
        let character = self.input[self.offset..].chars().next()?;
        expected.contains(&character).then(|| {
            self.offset += character.len_utf8();
            character
        })
    }
}

fn negate_parameter_value(value: &ParameterValue) -> Option<ParameterValue> {
    Some(match value {
        ParameterValue::Length(Length(value)) => ParameterValue::Length(Length(-*value)),
        ParameterValue::Angle(Angle(value)) => ParameterValue::Angle(Angle(-*value)),
        ParameterValue::Real(value) => ParameterValue::Real(-*value),
        ParameterValue::Integer(value) => ParameterValue::Integer(value.checked_neg()?),
        ParameterValue::Boolean(_) => return None,
    })
}

fn add_parameter_values(
    left: ParameterValue,
    right: ParameterValue,
    subtract: bool,
) -> Option<ParameterValue> {
    let sign = if subtract { -1.0 } else { 1.0 };
    Some(match (left, right) {
        (ParameterValue::Length(Length(left)), ParameterValue::Length(Length(right))) => {
            ParameterValue::Length(Length(left + sign * right))
        }
        (ParameterValue::Angle(Angle(left)), ParameterValue::Angle(Angle(right))) => {
            ParameterValue::Angle(Angle(left + sign * right))
        }
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => {
            let right = if subtract {
                right.checked_neg()?
            } else {
                right
            };
            ParameterValue::Integer(left.checked_add(right)?)
        }
        (left, right) => ParameterValue::Real(
            real_parameter_value(&left)? + sign * real_parameter_value(&right)?,
        ),
    })
}

fn compare_parameter_values(
    left: &ParameterValue,
    right: &ParameterValue,
    operator: &str,
) -> Option<bool> {
    if matches!(
        (left, right),
        (ParameterValue::Boolean(_), ParameterValue::Boolean(_))
    ) && !matches!(operator, "=" | "<>")
    {
        return None;
    }
    let ordering = match (left, right) {
        (ParameterValue::Length(Length(left)), ParameterValue::Length(Length(right)))
        | (ParameterValue::Angle(Angle(left)), ParameterValue::Angle(Angle(right)))
        | (ParameterValue::Real(left), ParameterValue::Real(right)) => left.partial_cmp(right)?,
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => left.cmp(right),
        (ParameterValue::Real(left), ParameterValue::Integer(right)) => {
            compare_integer_real(*right, *left)?.reverse()
        }
        (ParameterValue::Integer(left), ParameterValue::Real(right)) => {
            compare_integer_real(*left, *right)?
        }
        (ParameterValue::Boolean(left), ParameterValue::Boolean(right)) => left.cmp(right),
        _ => return None,
    };
    Some(match operator {
        "=" => ordering.is_eq(),
        "<>" => !ordering.is_eq(),
        "<" => ordering.is_lt(),
        ">" => ordering.is_gt(),
        "<=" => !ordering.is_gt(),
        ">=" => !ordering.is_lt(),
        _ => return None,
    })
}

fn compare_integer_real(integer: i64, real: f64) -> Option<std::cmp::Ordering> {
    if real.is_nan() {
        return None;
    }
    if real < i64::MIN as f64 {
        return Some(std::cmp::Ordering::Greater);
    }
    if real >= -(i64::MIN as f64) {
        return Some(std::cmp::Ordering::Less);
    }

    let truncated = real as i64;
    match integer.cmp(&truncated) {
        std::cmp::Ordering::Equal => 0.0f64.partial_cmp(&real.fract()),
        ordering => Some(ordering),
    }
}

fn conditional_parameter_value(
    condition: &ParameterValue,
    when_true: ParameterValue,
    when_false: ParameterValue,
) -> Option<ParameterValue> {
    let ParameterValue::Boolean(condition) = condition else {
        return None;
    };
    match (&when_true, &when_false) {
        (ParameterValue::Length(_), ParameterValue::Length(_))
        | (ParameterValue::Angle(_), ParameterValue::Angle(_))
        | (ParameterValue::Real(_), ParameterValue::Real(_))
        | (ParameterValue::Integer(_), ParameterValue::Integer(_))
        | (ParameterValue::Boolean(_), ParameterValue::Boolean(_)) => {
            Some(if *condition { when_true } else { when_false })
        }
        (ParameterValue::Real(_), ParameterValue::Integer(_))
        | (ParameterValue::Integer(_), ParameterValue::Real(_)) => {
            Some(ParameterValue::Real(real_parameter_value(if *condition {
                &when_true
            } else {
                &when_false
            })?))
        }
        _ => None,
    }
}

fn multiply_parameter_values(
    left: ParameterValue,
    right: ParameterValue,
    divide: bool,
) -> Option<ParameterValue> {
    if divide && parameter_numeric_value(&right)? == 0.0 {
        return None;
    }
    match (left, right) {
        (ParameterValue::Length(Length(left)), ParameterValue::Length(Length(right))) if divide => {
            Some(ParameterValue::Real(left / right))
        }
        (ParameterValue::Angle(Angle(left)), ParameterValue::Angle(Angle(right))) if divide => {
            Some(ParameterValue::Real(left / right))
        }
        (ParameterValue::Length(Length(left)), right) => {
            Some(ParameterValue::Length(Length(if divide {
                left / real_parameter_value(&right)?
            } else {
                left * real_parameter_value(&right)?
            })))
        }
        (ParameterValue::Angle(Angle(left)), right) => {
            Some(ParameterValue::Angle(Angle(if divide {
                left / real_parameter_value(&right)?
            } else {
                left * real_parameter_value(&right)?
            })))
        }
        (left, ParameterValue::Length(Length(right))) if !divide => Some(ParameterValue::Length(
            Length(real_parameter_value(&left)? * right),
        )),
        (left, ParameterValue::Angle(Angle(right))) if !divide => Some(ParameterValue::Angle(
            Angle(real_parameter_value(&left)? * right),
        )),
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) if !divide => {
            Some(ParameterValue::Integer(left.checked_mul(right)?))
        }
        (left, right) => Some(ParameterValue::Real(if divide {
            real_parameter_value(&left)? / real_parameter_value(&right)?
        } else {
            real_parameter_value(&left)? * real_parameter_value(&right)?
        })),
    }
}

fn exponentiate_parameter_value(
    base: &ParameterValue,
    exponent: &ParameterValue,
) -> Option<ParameterValue> {
    if let (ParameterValue::Integer(base), ParameterValue::Integer(exponent)) = (base, exponent) {
        if let Ok(exponent) = u32::try_from(*exponent) {
            return base.checked_pow(exponent).map(ParameterValue::Integer);
        }
        if *exponent >= 0 {
            return match base {
                0 => Some(ParameterValue::Integer(0)),
                1 => Some(ParameterValue::Integer(1)),
                -1 => Some(ParameterValue::Integer(if exponent % 2 == 0 {
                    1
                } else {
                    -1
                })),
                _ => None,
            };
        }
        return Some(ParameterValue::Real(integer_power_real(*base, *exponent)));
    }

    let exponent = real_parameter_value(exponent)?;
    Some(match base {
        ParameterValue::Length(value) if exponent == 1.0 => ParameterValue::Length(*value),
        ParameterValue::Angle(value) if exponent == 1.0 => ParameterValue::Angle(*value),
        ParameterValue::Length(_) | ParameterValue::Angle(_) if exponent == 0.0 => {
            ParameterValue::Real(1.0)
        }
        ParameterValue::Real(base) => ParameterValue::Real(base.powf(exponent)),
        ParameterValue::Integer(base) => {
            if exponent.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&exponent) {
                ParameterValue::Integer(base.checked_pow(exponent as u32)?)
            } else {
                ParameterValue::Real((*base as f64).powf(exponent))
            }
        }
        ParameterValue::Length(_) | ParameterValue::Angle(_) | ParameterValue::Boolean(_) => {
            return None;
        }
    })
}

fn integer_power_real(base: i64, exponent: i64) -> f64 {
    let mut exponent = exponent.unsigned_abs();
    let mut factor = base as f64;
    let mut value = 1.0;
    while exponent != 0 {
        if exponent & 1 != 0 {
            value *= factor;
        }
        exponent >>= 1;
        factor *= factor;
    }
    value.recip()
}

fn apply_parameter_function(name: &str, argument: &ParameterValue) -> Option<ParameterValue> {
    let name = name.to_ascii_lowercase();
    Some(match name.as_str() {
        "abs" => match argument {
            ParameterValue::Length(Length(value)) => ParameterValue::Length(Length(value.abs())),
            ParameterValue::Angle(Angle(value)) => ParameterValue::Angle(Angle(value.abs())),
            ParameterValue::Real(value) => ParameterValue::Real(value.abs()),
            ParameterValue::Integer(value) => ParameterValue::Integer(value.checked_abs()?),
            ParameterValue::Boolean(_) => return None,
        },
        "sin" | "cos" | "tan" | "sec" | "cosec" | "cotan" => {
            let ParameterValue::Angle(Angle(angle)) = argument else {
                return None;
            };
            ParameterValue::Real(match name.as_str() {
                "sin" => angle.sin(),
                "cos" => angle.cos(),
                "tan" => angle.tan(),
                "sec" => angle.cos().recip(),
                "cosec" => angle.sin().recip(),
                "cotan" => angle.tan().recip(),
                _ => unreachable!(),
            })
        }
        "arcsin" | "arccos" | "atn" | "arcsec" | "arccosec" | "arccotan" => {
            let value = real_parameter_value(argument)?;
            ParameterValue::Angle(Angle(match name.as_str() {
                "arcsin" => value.asin(),
                "arccos" => value.acos(),
                "atn" => value.atan(),
                "arcsec" => value.recip().acos(),
                "arccosec" => value.recip().asin(),
                "arccotan" => value.recip().atan(),
                _ => unreachable!(),
            }))
        }
        "exp" => ParameterValue::Real(real_parameter_value(argument)?.exp()),
        "log" => ParameterValue::Real(real_parameter_value(argument)?.ln()),
        "sqr" => ParameterValue::Real(real_parameter_value(argument)?.sqrt()),
        "int" => match argument {
            ParameterValue::Integer(value) => ParameterValue::Integer(*value),
            ParameterValue::Real(value) => {
                let value = value.trunc();
                if value < i64::MIN as f64 || value >= -(i64::MIN as f64) {
                    return None;
                }
                ParameterValue::Integer(value as i64)
            }
            ParameterValue::Length(_) | ParameterValue::Angle(_) | ParameterValue::Boolean(_) => {
                return None;
            }
        },
        "sgn" => {
            let value = parameter_numeric_value(argument)?;
            if !value.is_finite() {
                return None;
            }
            ParameterValue::Integer(match value.partial_cmp(&0.0)? {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        _ => return None,
    })
}

fn real_parameter_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Real(value) => Some(*value),
        ParameterValue::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

fn parameter_numeric_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Length(Length(value))
        | ParameterValue::Angle(Angle(value))
        | ParameterValue::Real(value) => Some(*value),
        ParameterValue::Integer(value) => Some(*value as f64),
        ParameterValue::Boolean(_) => None,
    }
}

/// Convert a discrete integer to a native scalar without changing its value.
pub(crate) fn exact_integer_f64(value: i64) -> Option<f64> {
    let encoded = value as f64;
    ((encoded as i128) == i128::from(value)).then_some(encoded)
}

fn parameter_value_is_finite(value: &ParameterValue) -> bool {
    parameter_numeric_value(value).is_none_or(f64::is_finite)
}

pub(crate) fn parameters_with_unresolved_references(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> usize {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    parameters
        .iter()
        .filter(|parameter| {
            let aliases = &aliases[&parameter.owner];
            let parsed = expression_identifier_tokens(&parameter.expression);
            parsed.unclosed_quote
                || parsed
                    .identifiers
                    .into_iter()
                    .filter(|identifier| {
                        !expression_identifier_is_syntax(&parameter.expression, identifier)
                    })
                    .filter(definite_parameter_reference)
                    .any(|identifier| {
                        aliases
                            .get(&identifier.value)
                            .and_then(Clone::clone)
                            .is_none_or(|dependency| dependency == parameter.id)
                    })
        })
        .count()
}

pub(crate) fn parameters_with_unevaluable_expressions(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    let mut states = parameter_value_states(parameters, configurations, false);
    parameters
        .iter()
        .filter(|parameter| {
            let aliases = &aliases[&parameter.owner];
            states.iter_mut().any(|values| {
                let own = values.remove(&parameter.id);
                let evaluated =
                    ParameterExpressionParser::new(&parameter.expression, aliases, values)
                        .parse()
                        .filter(parameter_value_is_finite);
                if let Some(value) = own {
                    values.insert(parameter.id.clone(), value);
                }
                evaluated.is_none()
            })
        })
        .count()
}

pub(crate) fn parameters_with_incoherent_dependencies(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> usize {
    let mut projected = parameters.to_vec();
    populate_parameter_dependencies(&mut projected, feature_names, global_owners);
    parameters
        .iter()
        .zip(projected)
        .filter(|(actual, projected)| actual.dependencies != projected.dependencies)
        .count()
}

pub(crate) fn parameters_with_incoherent_evaluated_values(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    let mut states = parameter_value_states(parameters, configurations, true);
    parameters
        .iter()
        .filter(|parameter| !parameter.dependencies.is_empty())
        .filter(|parameter| {
            let aliases = &aliases[&parameter.owner];
            states.iter_mut().any(|values| {
                let actual = values.remove(&parameter.id);
                let evaluated =
                    ParameterExpressionParser::new(&parameter.expression, aliases, values)
                        .parse()
                        .filter(parameter_value_is_finite);
                if let Some(value) = actual.clone() {
                    values.insert(parameter.id.clone(), value);
                }
                actual.zip(evaluated).is_some_and(|(actual, evaluated)| {
                    !equivalent_parameter_values(&actual, &evaluated)
                })
            })
        })
        .count()
}

fn parameter_value_states(
    parameters: &[DesignParameter],
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
    include_global: bool,
) -> Vec<HashMap<ParameterId, ParameterValue>> {
    let global_values = parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .value
                .clone()
                .map(|value| (parameter.id.clone(), value))
        })
        .collect::<HashMap<_, _>>();
    let mut states = include_global
        .then(|| global_values.clone())
        .into_iter()
        .collect::<Vec<_>>();
    states.extend(configurations.iter().map(|configuration| {
        let mut values = global_values.clone();
        values.extend(configuration.parameter_values.clone());
        values
    }));
    if states.is_empty() {
        states.push(global_values);
    }
    states
}

fn equivalent_parameter_values(left: &ParameterValue, right: &ParameterValue) -> bool {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * (1.0 + left.abs().max(right.abs()))
    };
    match (left, right) {
        (ParameterValue::Length(Length(left)), ParameterValue::Length(Length(right)))
        | (ParameterValue::Angle(Angle(left)), ParameterValue::Angle(Angle(right)))
        | (ParameterValue::Real(left), ParameterValue::Real(right)) => close(*left, *right),
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => left == right,
        (ParameterValue::Boolean(left), ParameterValue::Boolean(right)) => left == right,
        (ParameterValue::Integer(integer), ParameterValue::Real(real))
        | (ParameterValue::Real(real), ParameterValue::Integer(integer)) => {
            exact_integer_f64(*integer) == Some(*real)
        }
        _ => false,
    }
}

fn definite_parameter_reference(identifier: &ExpressionIdentifier) -> bool {
    identifier.quoted
        || identifier.value.contains('@')
        || identifier.value.strip_prefix('D').is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> + '_ {
    expression_identifier_tokens(expression)
        .identifiers
        .into_iter()
        .filter(|token| !expression_identifier_is_syntax(expression, token))
        .map(|token| token.value)
}

fn expression_identifier_is_syntax(expression: &str, identifier: &ExpressionIdentifier) -> bool {
    if identifier.quoted {
        return false;
    }
    if identifier
        .value
        .starts_with(|character: char| character.is_ascii_digit() || character == '.')
    {
        return true;
    }
    if identifier.value.eq_ignore_ascii_case("pi")
        || identifier.value.eq_ignore_ascii_case("true")
        || identifier.value.eq_ignore_ascii_case("false")
    {
        return true;
    }
    let is_function = matches!(
        identifier.value.to_ascii_lowercase().as_str(),
        "iif"
            | "abs"
            | "sin"
            | "cos"
            | "tan"
            | "sec"
            | "cosec"
            | "cotan"
            | "arcsin"
            | "arccos"
            | "atn"
            | "arcsec"
            | "arccosec"
            | "arccotan"
            | "exp"
            | "log"
            | "sqr"
            | "int"
            | "sgn"
    );
    is_function && expression[identifier.end..].trim_start().starts_with('(')
}

struct ParsedExpressionIdentifiers {
    identifiers: Vec<ExpressionIdentifier>,
    unclosed_quote: bool,
}

struct ExpressionIdentifier {
    start: usize,
    end: usize,
    value: String,
    quoted: bool,
}

fn expression_identifier_tokens(expression: &str) -> ParsedExpressionIdentifiers {
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
            if closed {
                if !value.is_empty() {
                    identifiers.push(ExpressionIdentifier {
                        start: at,
                        end: cursor,
                        value,
                        quoted: true,
                    });
                }
                at = cursor;
                continue;
            }
            return ParsedExpressionIdentifiers {
                identifiers,
                unclosed_quote: true,
            };
        }

        let Some(character) = rest.chars().next() else {
            break;
        };
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.') {
            let end = rest
                .find(|candidate: char| {
                    !(candidate.is_ascii_alphanumeric()
                        || matches!(candidate, '_' | '@' | '$' | '.'))
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
    ParsedExpressionIdentifiers {
        identifiers,
        unclosed_quote: false,
    }
}

#[cfg(test)]
mod history_reference_tests {
    use super::*;

    fn feature(id: &str, source_id: Option<&str>, ordinal: u32) -> Feature {
        Feature {
            id: id.into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: source_id.map(str::to_string),
            parent_source_id: None,
            ordinal,
            name: id.into(),
            kind: "Custom".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }
    }

    fn feature_input_lane(
        id: &str,
        configuration: Option<&str>,
    ) -> crate::records::FeatureInputLane {
        crate::records::FeatureInputLane {
            id: id.into(),
            configuration: configuration.map(str::to_string),
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        }
    }

    #[test]
    fn blind_extrusion_uses_its_sole_dimension_as_depth() {
        let mut feature = feature("sldprt:history:feature#1:2", Some("12"), 2);
        feature.xml_tag = "Extrusion".into();
        feature.input_class = Some("moExtrusion_c".into());
        feature.parameters.insert("s".into(), "2.1".into());
        feature
            .properties
            .insert("EndCondition".into(), "Blind".into());

        assert!(native_parameter_is_length(&feature, "s", Some("2.1")));
        assert!(matches!(
            project_extrude(&feature, &HashMap::new()),
            Some(FeatureDefinition::Extrude {
                extent: Extent::Blind {
                    length: Length(2.1)
                },
                ..
            })
        ));
    }

    #[test]
    fn repeated_dimension_content_projects_one_owned_parameter() {
        let mut feature = feature("sldprt:history:feature#1:2", None, 2);
        feature.parameters.insert("D1".into(), "2".into());
        feature.content = vec![
            FeatureContent::Dimension("D1".into()),
            FeatureContent::Dimension("D1".into()),
        ];

        assert_eq!(parameter_names(&feature), vec!["D1", "D1"]);
        assert_eq!(projected_parameter_names(&feature), vec!["D1"]);
        assert_eq!(
            project_feature_content(&feature, &HashMap::new()),
            vec![FeatureSourceContent::Parameter(ParameterId(
                "sldprt:model:parameter#1:2:0".into()
            ))]
        );
    }

    #[test]
    fn spatial_profile_class_projects_a_spatial_sketch() {
        let mut spatial = feature("spatial", Some("7"), 0);
        spatial.xml_tag = "Sketch".into();
        spatial.kind = "Sketch".into();
        spatial.input_class = Some("mo3DProfileFeature_c".into());

        assert_eq!(
            project_definition(
                &spatial,
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                std::slice::from_ref(&spatial),
            ),
            FeatureDefinition::SpatialSketch { sketch: None }
        );
    }

    #[test]
    fn hole_profile_dimension_order_distinguishes_counterbore_and_thread() {
        let profile = |roles: &[(&str, &str)]| {
            let mut profile = feature("profile", Some("7"), 0);
            profile.kind = "Sketch".into();
            profile.input_class = Some("moProfileFeature_c".into());
            for (name, expression) in roles {
                profile
                    .parameters
                    .insert((*name).into(), (*expression).into());
                profile
                    .content
                    .push(FeatureContent::Dimension((*name).into()));
            }
            profile
        };

        let counterbore = profile(&[
            ("a", "118°"),
            ("b", "5.7"),
            ("c", "<MOD-DIAM>9"),
            ("d", "12"),
            ("e", "<MOD-DIAM>5.5"),
        ]);
        let construction = hole_sketch_construction(&counterbore).unwrap();
        assert_eq!(construction.diameter, Length(5.5));
        assert_eq!(construction.depth, Some(Length(12.0)));
        assert!(matches!(
            construction.kind,
            HoleKind::CounterboreDrilled {
                diameter: Length(9.0),
                depth: Length(5.7),
                ..
            }
        ));

        let threaded = profile(&[
            ("a", "<MOD-DIAM>4.2"),
            ("b", "12.4"),
            ("c", "<MOD-DIAM>5"),
            ("d", "10"),
            ("e", "118°"),
        ]);
        let construction = hole_sketch_construction(&threaded).unwrap();
        assert_eq!(construction.diameter, Length(4.2));
        assert_eq!(construction.depth, Some(Length(12.4)));
        assert!(matches!(
            construction.kind,
            HoleKind::Threaded {
                major_diameter: Length(5.0),
                thread_depth: Length(10.0),
                pitch: None,
                ..
            }
        ));

        let mut canonical = feature("hole", Some("8"), 0);
        canonical.parameters = [
            ("Diameter".into(), "4.2mm".into()),
            ("Depth".into(), "12.4mm".into()),
            ("ThreadMajorDiameter".into(), "5mm".into()),
            ("ThreadDepth".into(), "10mm".into()),
            ("DrillPointAngle".into(), "118°".into()),
        ]
        .into();
        let projected = project_hole(&canonical, &HashMap::new());
        let FeatureDefinition::Hole {
            kind:
                HoleKind::Threaded {
                    major_diameter,
                    thread_depth,
                    ..
                },
            diameter: Some(diameter),
            extent: Some(Extent::Blind { length }),
            ..
        } = projected
        else {
            panic!("expected canonical threaded hole: {projected:?}");
        };
        assert!((diameter.0 - 4.2).abs() < 1.0e-12);
        assert!((major_diameter.0 - 5.0).abs() < 1.0e-12);
        assert!((thread_depth.0 - 10.0).abs() < 1.0e-12);
        assert!((length.0 - 12.4).abs() < 1.0e-12);
    }

    #[test]
    fn anonymous_scene_class_binds_only_a_unique_matching_kind_group() {
        let mut first = feature("first", Some("153"), 0);
        first.kind = "localized light".into();
        let mut second = feature("second", Some("155"), 1);
        second.kind = first.kind.clone();
        let mut singleton = feature("singleton", Some("200"), 2);
        singleton.kind = "unrelated".into();
        let mut histories = vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![first, second, singleton],
        }];
        let scene = crate::tessellation::SceneFeatureClasses {
            by_source: HashMap::new(),
            anonymous_counts: HashMap::from([("moDirectionLight_c".into(), 2)]),
        };

        enrich_scene_classes(&mut histories, &scene);

        assert_eq!(
            histories[0].features[0].input_class.as_deref(),
            Some("moDirectionLight_c")
        );
        assert_eq!(
            histories[0].features[1].input_class.as_deref(),
            Some("moDirectionLight_c")
        );
        assert_eq!(histories[0].features[2].input_class, None);
    }

    #[test]
    fn structurally_stable_feature_manager_nodes_use_source_identity() {
        let roster = |node: &Feature| {
            let mut roster = vec![node.clone()];
            for (source, class) in [
                ("7", "moDocsFolder_c"),
                ("8", "moCommentsFolder_c"),
                ("9", "moSolidBodyFolder_c"),
                ("10", "moSurfaceBodyFolder_c"),
            ] {
                let mut sentinel = feature("sentinel", Some(source), roster.len() as u32);
                sentinel.input_class = Some(class.into());
                roster.push(sentinel);
            }
            roster
        };
        let cases = [
            ("1", FeatureTreeNodeRole::Annotations),
            ("5", FeatureTreeNodeRole::ModelOrigin),
            ("6", FeatureTreeNodeRole::LightsAndCameras),
            ("12", FeatureTreeNodeRole::AmbientLight),
            ("13", FeatureTreeNodeRole::DirectionalLight),
            ("14", FeatureTreeNodeRole::DirectionalLight),
            ("15", FeatureTreeNodeRole::DirectionalLight),
        ];

        for (source_id, expected) in cases {
            let mut node = feature("node", Some(source_id), 0);
            node.kind = "任意本地化標籤".into();
            if source_id == "5" {
                node.xml_tag = "Sketch".into();
            }
            assert_eq!(
                feature_tree_node_role(&node, &roster(&node)),
                Some(expected)
            );
        }

        let mut fourth_light = feature("fourth", Some("70"), 0);
        fourth_light.kind = "本地化方向光".into();
        let mut directional_roster = roster(&fourth_light);
        let mut first_light = feature("light", Some("13"), 13);
        first_light.kind = fourth_light.kind.clone();
        directional_roster.push(first_light);
        assert_eq!(
            feature_tree_node_role(&fourth_light, &directional_roster),
            Some(FeatureTreeNodeRole::DirectionalLight)
        );

        let mut additional_ambient = feature("additional ambient", Some("16"), 0);
        additional_ambient.kind = "本地化环境光".into();
        let mut ambient_roster = roster(&additional_ambient);
        let mut reserved_ambient = feature("ambient", Some("12"), 12);
        reserved_ambient.kind = additional_ambient.kind.clone();
        ambient_roster.push(reserved_ambient);
        assert_eq!(
            feature_tree_node_role(&additional_ambient, &ambient_roster),
            Some(FeatureTreeNodeRole::AmbientLight)
        );

        let legacy_roster = |node: &Feature| {
            let mut roster = vec![node.clone()];
            for (source, class) in [
                ("6", "moOriginProfileFeature_c"),
                ("9", "moSurfaceBodyFolder_c"),
                ("10", "moSolidBodyFolder_c"),
                ("12", "moDocsFolder_c"),
                ("13", "moCommentsFolder_c"),
            ] {
                let mut sentinel = feature("sentinel", Some(source), roster.len() as u32);
                sentinel.input_class = Some(class.into());
                roster.push(sentinel);
            }
            roster
        };
        for (source, expected) in [
            ("2", FeatureTreeNodeRole::LightsAndCameras),
            ("7", FeatureTreeNodeRole::AmbientLight),
            ("8", FeatureTreeNodeRole::DirectionalLight),
        ] {
            let node = feature("legacy", Some(source), 0);
            assert_eq!(
                feature_tree_node_role(&node, &legacy_roster(&node)),
                Some(expected)
            );
        }
        let legacy_lights = feature("legacy lights", Some("2"), 0);
        let mut complete_legacy_roster = legacy_roster(&legacy_lights);
        for (source, class) in [
            ("1", "moDetailCabinet_c"),
            ("3", "moRefPlane_c"),
            ("4", "moRefPlane_c"),
            ("5", "moRefPlane_c"),
        ] {
            let mut sentinel = feature(
                "legacy frame",
                Some(source),
                complete_legacy_roster.len() as u32,
            );
            sentinel.input_class = Some(class.into());
            complete_legacy_roster.push(sentinel);
        }
        for source in ["7", "8"] {
            complete_legacy_roster.push(feature(
                "legacy light",
                Some(source),
                complete_legacy_roster.len() as u32,
            ));
        }
        assert_eq!(
            feature_tree_node_role(&legacy_lights, &complete_legacy_roster),
            Some(FeatureTreeNodeRole::LightsAndCameras)
        );

        let roster_from = |node: &Feature, classes: &[(&str, &str)], classless_sources: &[&str]| {
            let mut features = vec![node.clone()];
            for (source, class) in classes {
                let mut sentinel = feature("sentinel", Some(source), features.len() as u32);
                sentinel.input_class = Some((*class).into());
                features.push(sentinel);
            }
            for source in classless_sources {
                features.push(feature("reserved", Some(source), features.len() as u32));
            }
            features
        };
        let default_frame = [
            ("1", "moDetailCabinet_c"),
            ("2", "moRefPlane_c"),
            ("3", "moRefPlane_c"),
            ("4", "moRefPlane_c"),
            ("5", "moOriginProfileFeature_c"),
        ];
        let lights = feature("lights", Some("6"), 0);
        assert_eq!(
            feature_tree_node_role(&lights, &roster_from(&lights, &default_frame, &["7", "8"])),
            Some(FeatureTreeNodeRole::LightsAndCameras)
        );

        let ambient = feature("ambient", Some("10"), 0);
        let mut folders_at_seven = default_frame.to_vec();
        folders_at_seven.extend([("7", "moSolidBodyFolder_c"), ("8", "moSurfaceBodyFolder_c")]);
        assert_eq!(
            feature_tree_node_role(
                &ambient,
                &roster_from(&ambient, &folders_at_seven, &["6", "11", "12"]),
            ),
            Some(FeatureTreeNodeRole::AmbientLight)
        );

        let early_lights = feature("lights", Some("2"), 0);
        let origin_at_six = [
            ("1", "moDetailCabinet_c"),
            ("3", "moRefPlane_c"),
            ("4", "moRefPlane_c"),
            ("5", "moRefPlane_c"),
            ("6", "moOriginProfileFeature_c"),
        ];
        assert_eq!(
            feature_tree_node_role(
                &early_lights,
                &roster_from(&early_lights, &origin_at_six, &["7", "8"]),
            ),
            Some(FeatureTreeNodeRole::LightsAndCameras)
        );

        let ambiguous = feature("node", Some("99"), 0);
        assert_eq!(feature_tree_node_role(&ambiguous, &[]), None);

        let mut exploded_views = ambiguous.clone();
        exploded_views.name.clear();
        assert_eq!(
            feature_tree_node_role(&exploded_views, &roster(&exploded_views)),
            Some(FeatureTreeNodeRole::ExplodedViews)
        );

        let mut reference_plane = feature("node", Some("5"), 0);
        reference_plane.input_class = Some("moRefPlane_c".into());
        assert_eq!(feature_tree_node_role(&reference_plane, &[]), None);

        let mut sheet_metal = feature("node", Some("-1"), 0);
        sheet_metal.name.clear();
        assert_eq!(
            feature_tree_node_role(&sheet_metal, &roster(&sheet_metal)),
            Some(FeatureTreeNodeRole::SheetMetal)
        );
        sheet_metal.name = "任意本地化鈑金根節點".into();
        assert_eq!(
            feature_tree_node_role(&sheet_metal, &roster(&sheet_metal)),
            Some(FeatureTreeNodeRole::SheetMetal)
        );
        assert_eq!(feature_tree_node_role(&sheet_metal, &[]), None);
    }

    #[test]
    fn sketch_block_instances_bind_to_adjacent_typed_definition_objects() {
        let mut instance = feature("instance", Some("25"), 1);
        instance.input_class = Some("moSketchBlockInst_c".into());
        let mut compact_instance = feature("compact instance", Some("34"), 2);
        compact_instance.input_class = Some("moSketchBlockInst_c".into());
        let mut definition = feature("definition", Some("23"), 0);
        definition.input_class = Some("moSketchBlockDef_c".into());
        let mut histories = vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![definition, instance, compact_instance],
        }];
        let mut lane = feature_input_lane("lane", None);
        lane.native_payload.resize(500, 0);
        let write_local_id = |payload: &mut [u8], offset: usize, token: [u8; 4], local_id: u16| {
            payload[offset..offset + 4].copy_from_slice(&[0xff; 4]);
            payload[offset + 4..offset + 8].copy_from_slice(&token);
            payload[offset + 12..offset + 18].copy_from_slice(&[0x02, 0, 0, 0, 0, 0]);
            payload[offset + 18..offset + 20].copy_from_slice(&local_id.to_le_bytes());
            payload[offset + 40..offset + 44].copy_from_slice(&[0, 0, 1, 0]);
        };
        write_local_id(&mut lane.native_payload, 180, [0x11, 0x22, 0x33, 0x01], 0);
        write_local_id(
            &mut lane.native_payload,
            250,
            [0x11, 0x22, 0x33, 0x01],
            0x0115,
        );
        lane.native_payload[294..296].copy_from_slice(&[0x26, 0x81]);
        for (index, value) in [0.00575_f64, -0.169, 0.0].into_iter().enumerate() {
            let start = 296 + index * 8;
            lane.native_payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
        lane.native_payload[388..390].copy_from_slice(&0x0115_u16.to_le_bytes());
        write_local_id(
            &mut lane.native_payload,
            420,
            [0x44, 0x55, 0x66, 0x01],
            0x0115,
        );
        lane.native_payload[464..466].copy_from_slice(&[0x73, 0x81]);
        for (index, value) in [0.01075_f64, -0.132, 0.0].into_iter().enumerate() {
            let start = 466 + index * 8;
            lane.native_payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
        lane.names = vec![
            crate::records::FeatureInputName {
                id: "instance-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 100,
                object_id: Some(25),
                value: "instance".into(),
            },
            crate::records::FeatureInputName {
                id: "definition-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 140,
                object_id: Some(23),
                value: "definition".into(),
            },
            crate::records::FeatureInputName {
                id: "compact-instance-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 340,
                object_id: Some(34),
                value: "compact".into(),
            },
        ];

        crate::resolved_features::enrich_history_sketch_block_references(&mut histories, &[lane]);

        assert_eq!(
            histories[0].features[1]
                .properties
                .get("BlockDefinition")
                .map(String::as_str),
            Some("23")
        );
        assert_eq!(
            histories[0].features[1]
                .properties
                .get("BlockOrigin")
                .map(String::as_str),
            Some("5.75mm,-169mm,0mm")
        );
        assert_eq!(
            histories[0].features[2]
                .properties
                .get("BlockOrigin")
                .map(String::as_str),
            Some("10.75mm,-132mm,0mm")
        );
        assert_eq!(
            histories[0].features[2]
                .properties
                .get("BlockDefinition")
                .map(String::as_str),
            Some("23")
        );
    }

    #[test]
    fn principal_plane_requires_the_reference_plane_native_class() {
        let mut plane = feature("plane", Some("2"), 0);
        assert_eq!(principal_plane(&plane), None);
        plane.input_class = Some("moRefPlane_c".into());
        assert_eq!(
            principal_plane(&plane),
            Some(cadmpeg_ir::features::PrincipalPlane::Front)
        );
    }

    #[test]
    fn angular_plane_parameter_does_not_claim_offset_semantics() {
        let mut plane = feature("plane", Some("90"), 0);
        plane.input_class = Some("moRefPlane_c".into());
        plane.parameters.insert("D1".into(), "0rad".into());
        plane
            .properties
            .insert("Origin".into(), "0mm,70mm,0mm".into());
        plane.properties.insert("Normal".into(), "0,1,0".into());
        plane.properties.insert("UAxis".into(), "-1,0,0".into());

        assert!(!is_offset_plane(&plane));
        assert_eq!(
            project_definition(
                &plane,
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                std::slice::from_ref(&plane),
            ),
            FeatureDefinition::DatumPlane {
                origin: Point3::new(0.0, 70.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(-1.0, 0.0, 0.0),
            }
        );
    }

    #[test]
    fn length_plane_parameter_claims_offset_semantics() {
        let mut plane = feature("plane", Some("90"), 0);
        plane.input_class = Some("moRefPlane_c".into());
        plane.parameters.insert("D1".into(), "70mm".into());

        assert!(is_offset_plane(&plane));
        assert_eq!(
            project_definition(
                &plane,
                &HashMap::new(),
                &HashMap::new(),
                &HashMap::new(),
                std::slice::from_ref(&plane),
            ),
            FeatureDefinition::DatumOffsetPlane {
                reference: None,
                distance: Length(70.0),
            }
        );
    }

    #[test]
    fn legacy_principal_plane_requires_a_complete_matching_triplet() {
        let front = feature("front", Some("2"), 0);
        let top = feature("top", Some("3"), 1);
        let right = feature("right", Some("4"), 2);
        let features = [&front, &top, &right]
            .into_iter()
            .map(|feature| (feature.source_id.as_deref().unwrap(), feature))
            .collect::<HashMap<_, _>>();
        assert_eq!(
            principal_plane_in_history(&front, &features, &[]),
            Some(cadmpeg_ir::features::PrincipalPlane::Front)
        );

        let mut mismatched = right.clone();
        mismatched.kind = "Different".into();
        let features = [&front, &top, &mismatched]
            .into_iter()
            .map(|feature| (feature.source_id.as_deref().unwrap(), feature))
            .collect::<HashMap<_, _>>();
        assert_eq!(principal_plane_in_history(&front, &features, &[]), None);
    }

    #[test]
    fn idless_legacy_principal_planes_require_an_exact_bounded_triplet() {
        let front = feature("front", None, 10);
        let top = feature("top", None, 11);
        let right = feature("right", None, 12);
        let mut successor = feature("origin", None, 13);
        successor.kind = "Other".into();
        let records = [front.clone(), top, right, successor];

        assert_eq!(
            principal_plane_in_history(&front, &HashMap::new(), &records),
            Some(cadmpeg_ir::features::PrincipalPlane::Front)
        );

        let mut unbounded = records.clone();
        unbounded[3].kind = unbounded[0].kind.clone();
        assert_eq!(
            principal_plane_in_history(&front, &HashMap::new(), &unbounded),
            None
        );
    }

    #[test]
    fn custom_properties_are_document_attributes_not_model_features() {
        let mut property = feature("property", None, 0);
        property.xml_tag = "CustomProperty".into();
        property.name = "PartNumber".into();
        property.text = Some("A-123".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![property],
        };

        assert!(project_features(std::slice::from_ref(&history)).is_empty());
        let attributes = custom_property_attributes(std::slice::from_ref(&history));
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes[0].name, "PartNumber");
        assert_eq!(
            attributes[0].values,
            vec![AttributeValue::String("A-123".into())]
        );

        let mut native = Some(crate::native::SldprtNative {
            version: crate::native::SLDPRT_NATIVE_VERSION,
            feature_histories: vec![history],
            feature_input_lanes: Vec::new(),
            pmi_dimensions: Vec::new(),
        });
        sync_neutral_features(&[], &[], &[], &mut native).unwrap();
        assert_eq!(native.unwrap().feature_histories[0].features.len(), 1);
    }

    #[test]
    fn native_attribute_records_are_metadata_not_model_features() {
        let mut definition = feature("definition", Some("-1"), 0);
        definition.name = "VendorSettings.1".into();
        definition
            .parameters
            .insert("VendorSettings.1".into(), "0".into());
        let mut attribute = feature("attribute", Some("27"), 1);
        attribute.name = "VendorSettings.14236".into();
        attribute.input_class = Some("moAttribute_c".into());
        let mut comments = feature("comments", Some("28"), 2);
        comments.input_class = Some("moConfigCommentsFolder_c".into());
        let mut alignment = feature("alignment", Some("29"), 3);
        alignment.input_class = Some("moAlignGroup_c".into());
        let mut model = feature("model", Some("30"), 4);
        model.xml_tag = "Sketch".into();
        model.kind = "Sketch".into();
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![definition, attribute, comments, alignment, model],
        };

        let projected = project_features(std::slice::from_ref(&history));
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].native_ref.as_deref(), Some("model"));
        assert!(project_parameters(&[history]).is_empty());
    }

    #[test]
    fn configuration_snapshots_preserve_base_tree_node_roles() {
        let light = feature("light", Some("30"), 0);
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![light],
        };
        let mut configured = project_features(std::slice::from_ref(&history));
        assert!(matches!(
            configured[0].definition,
            FeatureDefinition::Native { .. }
        ));
        let mut base = configured.clone();
        base[0].definition = FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::DirectionalLight,
        };

        restore_configuration_tree_node_definitions(&mut configured, &base);
        assert!(matches!(
            configured[0].definition,
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::DirectionalLight
            }
        ));
    }

    #[test]
    fn simple_hole_uses_its_profile_dimension_roles() {
        let mut hole = feature("hole", Some("214"), 0);
        hole.xml_tag = "HoleWizard".into();
        hole.properties
            .insert("DissectableChildren".into(), "213,212".into());
        let mut position = feature("position", Some("213"), 1);
        position.xml_tag = "Sketch".into();
        position.kind = "Sketch".into();
        let mut profile = feature("profile", Some("212"), 1);
        profile.xml_tag = "Sketch".into();
        profile.kind = "Sketch".into();
        profile
            .parameters
            .insert("localized diameter".into(), "<MOD-DIAM>4.5".into());
        profile
            .parameters
            .insert("localized depth".into(), "13.2".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![hole, position, profile],
        };

        let projected = project_features(std::slice::from_ref(&history));
        let FeatureDefinition::Hole {
            diameter, extent, ..
        } = &projected[0].definition
        else {
            panic!("expected a hole definition");
        };
        assert_eq!(*diameter, Some(Length(4.5)));
        assert_eq!(
            *extent,
            Some(Extent::Blind {
                length: Length(13.2)
            })
        );

        let mut ambiguous = history;
        ambiguous.features[2]
            .parameters
            .insert("another length".into(), "2".into());
        let ambiguous = project_features(&[ambiguous]);
        let FeatureDefinition::Hole {
            diameter, extent, ..
        } = &ambiguous[0].definition
        else {
            panic!("expected a hole definition");
        };
        assert_eq!(*diameter, Some(Length(4.5)));
        assert_eq!(*extent, None);
    }

    #[test]
    fn hole_wizard_uses_the_unique_countersink_child_schema() {
        let mut hole = feature("hole", Some("214"), 0);
        hole.xml_tag = "HoleWizard".into();
        hole.properties
            .insert("DissectableChildren".into(), "213,212".into());
        let mut position = feature("position", Some("213"), 1);
        position.xml_tag = "Sketch".into();
        position.kind = "Sketch".into();
        position.parameters.insert("D1".into(), "11".into());
        let mut profile = feature("profile", Some("212"), 2);
        profile.xml_tag = "Sketch".into();
        profile.kind = "Sketch".into();
        profile
            .parameters
            .insert("localized bore".into(), "<MOD-DIAM>3.4".into());
        profile
            .parameters
            .insert("localized depth".into(), "3".into());
        profile
            .parameters
            .insert("localized entry".into(), "<MOD-DIAM>6.6".into());
        profile
            .parameters
            .insert("localized angle".into(), "90°".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![hole, position, profile],
        };

        let projected = project_features(&[history]);
        assert!(matches!(
            projected[0].definition,
            FeatureDefinition::Hole {
                kind: HoleKind::Countersink {
                    diameter: Length(6.6),
                    angle: Angle(angle),
                },
                diameter: Some(Length(3.4)),
                extent: Some(Extent::Blind {
                    length: Length(3.0),
                }),
                ..
            } if (angle - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
        ));
    }

    #[test]
    fn hole_wizard_drill_point_profile_retains_bore_and_blind_depth() {
        let mut hole = feature("hole", Some("214"), 0);
        hole.xml_tag = "HoleWizard".into();
        hole.properties
            .insert("DissectableChildren".into(), "212".into());
        let mut profile = feature("profile", Some("212"), 1);
        profile.xml_tag = "Sketch".into();
        profile.kind = "Sketch".into();
        profile.input_class = Some("moProfileFeature_c".into());
        profile
            .parameters
            .insert("螺纹孔钻头直径".into(), "<MOD-DIAM>4.2".into());
        profile
            .parameters
            .insert("螺纹孔钻头深度".into(), "10".into());
        profile.parameters.insert("导头角度".into(), "118°".into());
        profile.content.extend([
            FeatureContent::Dimension("导头角度".into()),
            FeatureContent::Dimension("螺纹孔钻头深度".into()),
            FeatureContent::Dimension("螺纹孔钻头直径".into()),
        ]);
        profile
            .parameters
            .insert("derived native scalar".into(), "937.25".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![hole, profile],
        };

        let projected = project_features(&[history]);
        assert!(matches!(
            projected[0].definition,
            FeatureDefinition::Hole {
                kind: HoleKind::SimpleDrilled {
                    drill_point_angle: Angle(drill_point_angle),
                },
                diameter: Some(Length(4.2)),
                extent: Some(Extent::Blind {
                    length: Length(10.0),
                }),
                ..
            } if (drill_point_angle - 118.0_f64.to_radians()).abs() < 1.0e-12
        ));
    }

    #[test]
    fn native_scalar_refresh_preserves_radial_dimension_semantics() {
        let profile = feature("profile", Some("212"), 1);

        assert_eq!(
            format_native_scalar(&profile, "bore", 0.0042, Some("<MOD-DIAM>4.2")),
            "<MOD-DIAM>4.2"
        );
        assert_eq!(
            format_native_scalar(&profile, "radius", 0.003, Some("&lt;MOD-RHO&gt;3")),
            "&lt;MOD-RHO&gt;3"
        );
    }

    #[test]
    fn legacy_revolve_uses_d1_angle_and_cut_class_operation() {
        let mut revolve = feature("revolve", Some("42"), 0);
        revolve.input_class = Some("moRevCut_c".into());
        revolve.parameters.insert("D1".into(), "360°".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![revolve],
        };

        let projected = project_features(&[history]);
        assert!(matches!(
            projected[0].definition,
            FeatureDefinition::Revolve {
                construction: RevolutionConstruction {
                    extent: Some(Extent::Angle { angle: Angle(value) }),
                    ..
                },
                op: BooleanOp::Cut,
            } if (value - std::f64::consts::TAU).abs() < 1.0e-12
        ));
    }

    #[test]
    fn localized_cut_extrusion_uses_its_native_class_operation() {
        let mut cut = feature("cut", Some("43"), 0);
        cut.input_class = Some("moCut_c".into());
        cut.parameters.insert("D1".into(), "45".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![cut],
        };

        let projected = project_features(&[history]);
        assert!(matches!(
            projected[0].definition,
            FeatureDefinition::Extrude {
                op: BooleanOp::Cut,
                ..
            }
        ));
    }

    #[test]
    fn revolve_uses_its_ordered_angle_dimension_name() {
        let mut revolve = feature("revolve", Some("42"), 0);
        revolve.input_class = Some("moRevolution_c".into());
        revolve.parameters.insert("FIX_1".into(), "360°".into());
        revolve
            .content
            .push(FeatureContent::Dimension("FIX_1".into()));
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![revolve],
        };

        let projected = project_features(&[history]);
        assert!(matches!(
            projected[0].definition,
            FeatureDefinition::Revolve {
                construction: RevolutionConstruction {
                    extent: Some(Extent::Angle { angle: Angle(value) }),
                    ..
                },
                ..
            } if (value - std::f64::consts::TAU).abs() < 1.0e-12
        ));
    }

    #[test]
    fn cosmetic_thread_retains_nominal_diameter_and_blind_length() {
        let mut thread = feature("thread", Some("42"), 0);
        thread.input_class = Some("moCosmeticThread_c".into());
        thread.parameters.insert("D1".into(), "16".into());
        thread.parameters.insert("D2".into(), "<MOD-DIAM>8".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![thread],
        };

        let projected = project_features(&[history]);
        assert_eq!(
            projected[0].definition,
            FeatureDefinition::CosmeticThread {
                face: FaceSelection::Unresolved,
                diameter: Some(Length(8.0)),
                extent: Some(CosmeticThreadExtent::Blind {
                    length: Length(16.0),
                }),
            }
        );
    }

    #[test]
    fn cosmetic_thread_without_blind_length_is_through() {
        let mut thread = feature("thread", Some("42"), 0);
        thread.input_class = Some("moCosmeticThread_c".into());
        thread.parameters.insert("D2".into(), "<MOD-DIAM>8".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![thread],
        };

        let projected = project_features(&[history]);
        assert_eq!(
            projected[0].definition,
            FeatureDefinition::CosmeticThread {
                face: FaceSelection::Unresolved,
                diameter: Some(Length(8.0)),
                extent: Some(CosmeticThreadExtent::Through),
            }
        );
    }

    #[test]
    fn profile_consumers_require_a_regeneration_profile() {
        let mut definition = FeatureDefinition::Extrude {
            profile: ProfileRef::Native("sketch-native".into()),
            direction: None,
            extent: Extent::Unresolved,
            op: BooleanOp::Unresolved,
            draft: None,
        };
        let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());

        assert!(!bind_definition_sketch(
            &mut definition,
            "sketch-native",
            &FeatureId("sketch-feature".into()),
            &sketch,
            false,
        ));
        assert!(matches!(
            definition,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Native(_),
                ..
            }
        ));
        assert!(bind_definition_sketch(
            &mut definition,
            "sketch-native",
            &FeatureId("sketch-feature".into()),
            &sketch,
            true,
        ));
        assert!(matches!(
            definition,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Sketch(ref bound),
                ..
            } if bound == &sketch
        ));
    }

    #[test]
    fn exact_native_profile_source_projects_a_feature_dependency() {
        let mut sketch = feature("sketch", Some("42"), 0);
        sketch.kind = "Sketch".into();
        sketch.input_class = Some("moProfileFeature_c".into());
        let mut extrusion = feature("extrusion", Some("43"), 1);
        extrusion.kind = "Extrusion".into();
        extrusion.input_class = Some("moExtrusion_c".into());
        extrusion.properties.insert("Profile".into(), "42".into());
        extrusion
            .properties
            .insert("Operation".into(), "Join".into());
        extrusion.parameters.insert("D1".into(), "5".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![sketch, extrusion],
        };

        let projected = project_features(&[history]);
        let sketch_id = neutral_feature_id("sketch");
        assert!(matches!(
            &projected[1].definition,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Feature(feature),
                ..
            } if feature == &sketch_id
        ));
        assert_eq!(projected[1].dependencies, [sketch_id]);
    }

    fn design_configuration(
        id: &str,
        ordinal: u32,
        source_index: Option<u32>,
        native_ref: Option<&str>,
    ) -> DesignConfiguration {
        DesignConfiguration {
            id: ConfigurationId(id.into()),
            ordinal,
            active: false,
            source_index,
            name: id.into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: Vec::new(),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: native_ref.map(str::to_string),
        }
    }

    fn native_configuration(id: &str, ordinal: u32, source_index: Option<u32>) -> Configuration {
        Configuration {
            id: id.into(),
            parent: "history".into(),
            ordinal,
            source_index,
            name: id.into(),
            material: None,
            properties: BTreeMap::new(),
        }
    }

    fn native_with_configuration_lanes(
        configurations: Vec<Configuration>,
        lanes: Vec<crate::records::FeatureInputLane>,
    ) -> Option<crate::native::SldprtNative> {
        Some(crate::native::SldprtNative {
            feature_histories: vec![FeatureHistory {
                id: "history".into(),
                part_name: None,
                properties: BTreeMap::new(),
                content: Vec::new(),
                configurations,
                features: Vec::new(),
            }],
            feature_input_lanes: lanes,
            ..crate::native::SldprtNative::default()
        })
    }
    #[test]
    fn repeated_aliases_from_one_parameter_remain_unambiguous() {
        let mut owner = feature("owner", Some("1"), 0);
        owner.parameters.insert("Width".into(), "4mm".into());
        owner.dimension_properties.insert(
            "Width".into(),
            BTreeMap::from([("EquationId".into(), "Width".into())]),
        );
        let parameters = project_parameters(&[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }]);

        let aliases = parameter_aliases(
            &parameters,
            &HashMap::new(),
            &HashSet::new(),
            &parameters[0].owner,
        );

        assert_eq!(aliases.get("Width"), Some(&Some(parameters[0].id.clone())));
    }

    #[test]
    fn subtraction_separates_unquoted_parameter_references() {
        assert_eq!(
            expression_identifiers("D1@Sketch1-D2@Sketch1").collect::<Vec<_>>(),
            ["D1@Sketch1", "D2@Sketch1"]
        );
    }

    #[test]
    fn numeric_literals_do_not_bind_numeric_parameter_names() {
        let mut owner = feature("owner", Some("1"), 0);
        owner.parameters = BTreeMap::from([
            ("4".into(), "3mm".into()),
            ("Literal".into(), "4".into()),
            ("Reference".into(), "\"4\" * 2".into()),
        ]);
        let parameters = project_parameters(&[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }]);
        let by_name = parameters
            .iter()
            .map(|parameter| (parameter.name.as_str(), parameter))
            .collect::<HashMap<_, _>>();

        assert!(by_name["Literal"].dependencies.is_empty());
        assert_eq!(by_name["Reference"].dependencies, [by_name["4"].id.clone()]);
        assert_eq!(
            by_name["Reference"].value,
            Some(ParameterValue::Length(Length(6.0)))
        );
        assert!(!unquoted_expression_identifier("4"));
        assert_eq!(
            rewrite_parameter_expression(
                "Width * 2",
                &HashMap::from([("Width".into(), "4".into())]),
            )
            .as_deref(),
            Some("\"4\" * 2")
        );
    }

    #[test]
    fn subtraction_projects_both_parameter_dependencies() {
        let mut owner = feature("owner", Some("1"), 0);
        owner.parameters = BTreeMap::from([
            ("A".into(), "7".into()),
            ("B".into(), "2".into()),
            ("C".into(), "A-B".into()),
        ]);
        let parameters = project_parameters(&[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }]);

        assert_eq!(
            parameters[2].dependencies,
            [parameters[0].id.clone(), parameters[1].id.clone()]
        );
        assert_eq!(parameters[2].value, Some(ParameterValue::Integer(5)));
    }

    #[test]
    fn unqualified_aliases_are_local_to_the_expression_owner() {
        let mut first = feature("first", Some("1"), 0);
        first.parameters.insert("Width".into(), "4mm".into());
        let mut second = feature("second", Some("2"), 1);
        second.parameters.insert("Width".into(), "5mm".into());
        let parameters = project_parameters(&[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![first, second],
        }]);

        let first_aliases = parameter_aliases(
            &parameters,
            &HashMap::new(),
            &HashSet::new(),
            &parameters[0].owner,
        );
        let second_aliases = parameter_aliases(
            &parameters,
            &HashMap::new(),
            &HashSet::new(),
            &parameters[1].owner,
        );
        let unrelated_aliases = parameter_aliases(
            &parameters,
            &HashMap::new(),
            &HashSet::new(),
            &FeatureId("unrelated".into()),
        );

        assert_eq!(
            first_aliases.get("Width"),
            Some(&Some(parameters[0].id.clone()))
        );
        assert_eq!(
            second_aliases.get("Width"),
            Some(&Some(parameters[1].id.clone()))
        );
        assert_eq!(unrelated_aliases.get("Width"), None);
    }

    #[test]
    fn equation_driven_parameters_are_global() {
        let mut equations = feature("equations", Some("1"), 0);
        equations.kind = "EquationDriven".into();
        equations.parameters.insert("Width".into(), "4mm".into());
        let mut consumer = feature("consumer", Some("2"), 1);
        consumer
            .parameters
            .insert("Result".into(), "Width * 2".into());

        let parameters = project_parameters(&[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![equations, consumer],
        }]);

        assert_eq!(parameters[1].dependencies, [parameters[0].id.clone()]);
        assert_eq!(
            parameters[1].value,
            Some(ParameterValue::Length(Length(8.0)))
        );
    }

    #[test]
    fn ordinary_feature_parameters_do_not_leak_globally() {
        let mut source = feature("source", Some("1"), 0);
        source.parameters.insert("Width".into(), "4mm".into());
        let mut consumer = feature("consumer", Some("2"), 1);
        consumer
            .parameters
            .insert("Result".into(), "Width * 2".into());

        let parameters = project_parameters(&[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![source, consumer],
        }]);

        assert!(parameters[1].dependencies.is_empty());
        assert_eq!(parameters[1].value, None);
    }

    #[test]
    fn local_parameter_precedes_same_named_global() {
        let mut equations = feature("equations", Some("1"), 0);
        equations.kind = "EquationDriven".into();
        equations.parameters.insert("Width".into(), "4mm".into());
        let mut consumer = feature("consumer", Some("2"), 1);
        consumer.parameters = BTreeMap::from([
            ("Width".into(), "5mm".into()),
            ("Result".into(), "Width * 2".into()),
        ]);

        let parameters = project_parameters(&[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![equations, consumer],
        }]);

        assert_eq!(parameters[1].dependencies, [parameters[2].id.clone()]);
        assert_eq!(
            parameters[1].value,
            Some(ParameterValue::Length(Length(10.0)))
        );
    }

    #[test]
    fn ambiguous_and_missing_history_references_do_not_bind_arbitrarily() {
        let first = feature("first", Some("1"), 0);
        let second = feature("second", Some("1"), 1);
        let mut dependent = feature("dependent", Some("2"), 2);
        dependent.properties.insert("Dependency".into(), "1".into());
        let mut malformed = feature("malformed", Some("3"), 3);
        malformed.parent_source_id = Some("missing".into());
        malformed
            .content
            .push(FeatureContent::Feature("missing-child".into()));
        malformed
            .content
            .push(FeatureContent::Dimension("D1".into()));
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![first, second, dependent, malformed],
        };

        let projected = project_features(std::slice::from_ref(&history));

        assert!(projected[2].dependencies.is_empty());
        assert_eq!(incomplete_history_reference_features(&[history]), 4);
    }

    #[test]
    fn assigning_configuration_index_does_not_capture_global_input_lane() {
        let mut native = native_with_configuration_lanes(
            vec![native_configuration("native-configuration", 0, None)],
            vec![feature_input_lane("global-lane", None)],
        );
        let mut configuration =
            design_configuration("configuration", 0, Some(0), Some("native-configuration"));
        configuration.active = true;
        sync_neutral_configurations(&[configuration], &mut native);

        let native = native.unwrap();
        assert_eq!(
            native.feature_histories[0].configurations[0].source_index,
            Some(0)
        );
        assert_eq!(native.feature_input_lanes[0].configuration, None);
    }

    #[test]
    fn explicit_configuration_index_precedes_ordinal_fallback() {
        let configurations = [
            design_configuration("explicit", 0, Some(1), None),
            design_configuration("fallback", 1, None, None),
        ];
        let lanes = [feature_input_lane("lane", Some("1"))];

        assert_eq!(
            configuration_lane_assignments(&configurations, &lanes),
            [(0, 0)]
        );
    }

    #[test]
    fn changing_shadowed_ordinal_does_not_steal_explicit_lane() {
        let native_configurations = vec![
            native_configuration("explicit-native", 0, Some(1)),
            native_configuration("fallback-native", 1, None),
        ];
        let mut native = native_with_configuration_lanes(
            native_configurations,
            vec![feature_input_lane("explicit-lane", Some("1"))],
        );
        let configurations = [
            design_configuration("explicit", 0, Some(1), Some("explicit-native")),
            design_configuration("fallback", 2, None, Some("fallback-native")),
        ];

        sync_neutral_configurations(&configurations, &mut native);

        assert_eq!(
            native.unwrap().feature_input_lanes[0]
                .configuration
                .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn configuration_lane_index_swaps_are_simultaneous() {
        let mut native = native_with_configuration_lanes(
            vec![
                native_configuration("first-native", 0, Some(1)),
                native_configuration("second-native", 1, Some(2)),
            ],
            vec![
                feature_input_lane("first-lane", Some("1")),
                feature_input_lane("second-lane", Some("2")),
            ],
        );
        let configurations = [
            design_configuration("first", 0, Some(2), Some("first-native")),
            design_configuration("second", 1, Some(1), Some("second-native")),
        ];

        sync_neutral_configurations(&configurations, &mut native);

        assert_eq!(
            native
                .unwrap()
                .feature_input_lanes
                .into_iter()
                .map(|lane| lane.configuration)
                .collect::<Vec<_>>(),
            [Some("2".into()), Some("1".into())]
        );
    }

    #[test]
    fn deleting_configuration_removes_its_uniquely_owned_lane() {
        let mut native = native_with_configuration_lanes(
            vec![
                native_configuration("kept-native", 0, Some(1)),
                native_configuration("deleted-native", 1, Some(2)),
            ],
            vec![
                feature_input_lane("kept-lane", Some("1")),
                feature_input_lane("deleted-lane", Some("2")),
            ],
        );

        sync_neutral_configurations(
            &[design_configuration(
                "kept",
                0,
                Some(1),
                Some("kept-native"),
            )],
            &mut native,
        );

        let native = native.unwrap();
        assert_eq!(native.feature_input_lanes.len(), 1);
        assert_eq!(native.feature_input_lanes[0].id, "kept-lane");

        let mut native = native_with_configuration_lanes(
            vec![native_configuration("deleted-native", 0, Some(1))],
            vec![
                feature_input_lane("global-lane", None),
                feature_input_lane("deleted-lane", Some("1")),
            ],
        );
        sync_neutral_configurations(&[], &mut native);
        let native = native.unwrap();
        assert!(native.feature_histories[0].configurations.is_empty());
        assert_eq!(native.feature_input_lanes.len(), 1);
        assert_eq!(native.feature_input_lanes[0].id, "global-lane");
    }

    #[test]
    fn configuration_lane_follows_effective_index_changes() {
        for (previous_ordinal, previous_source, previous_lane, ordinal, source, expected) in [
            (2, Some(7), "7", 3, None, "3"),
            (2, None, "2", 4, None, "4"),
        ] {
            let mut native = native_with_configuration_lanes(
                vec![native_configuration(
                    "native-configuration",
                    previous_ordinal,
                    previous_source,
                )],
                vec![feature_input_lane("lane", Some(previous_lane))],
            );
            let mut configuration = design_configuration(
                "configuration",
                ordinal,
                source,
                Some("native-configuration"),
            );
            configuration.active = true;
            sync_neutral_configurations(&[configuration], &mut native);

            assert_eq!(
                native.unwrap().feature_input_lanes[0]
                    .configuration
                    .as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn configuration_sketch_state_reuses_projected_neutral_sketch() {
        use cadmpeg_ir::features::{
            ConfigurationFeatureState, DesignConfiguration, Feature as NeutralFeature,
            FeatureDefinition, SketchSpace,
        };
        use cadmpeg_ir::sketches::{Sketch, SketchId, SpatialSketch, SpatialSketchId};

        let native_feature = feature("sketch-native", Some("7"), 0);
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![native_feature],
        };
        let feature_id = cadmpeg_ir::features::FeatureId("sketch".into());
        let unresolved = FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: None,
        };
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.features.push(NeutralFeature {
            id: feature_id.clone(),
            ordinal: 0,
            name: Some("sketch-native".into()),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: unresolved.clone(),
            native_ref: Some("sketch-native".into()),
        });
        let spatial_feature_id =
            cadmpeg_ir::features::FeatureId("sldprt:model:feature#spatial".into());
        let spatial_sketch_id = SpatialSketchId("sldprt:model:spatial-sketch#spatial".into());
        ir.model.features.push(NeutralFeature {
            id: spatial_feature_id.clone(),
            ordinal: 1,
            name: Some("spatial-native".into()),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::SpatialSketch {
                sketch: Some(spatial_sketch_id.clone()),
            },
            native_ref: Some("spatial-native".into()),
        });
        let sketch_id = SketchId("projected-sketch".into());
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: Some("sketch-native".into()),
            configuration: Some("0".into()),
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            profiles: Vec::new(),
            native_ref: Some("lane".into()),
        });
        ir.model.spatial_sketches.push(SpatialSketch {
            id: spatial_sketch_id.clone(),
            name: Some("spatial-native".into()),
            configuration: Some("0".into()),
            entities: Vec::new(),
            native_ref: Some("lane".into()),
        });
        ir.model.configurations.push(DesignConfiguration {
            id: cadmpeg_ir::features::ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Default".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: Vec::new(),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([
                (
                    feature_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: Vec::new(),
                        outputs: Vec::new(),
                        definition: unresolved,
                    },
                ),
                (
                    spatial_feature_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: Vec::new(),
                        outputs: Vec::new(),
                        definition: FeatureDefinition::SpatialSketch { sketch: None },
                    },
                ),
            ]),
            native_ref: None,
        });
        let lane = feature_input_lane("lane", Some("0"));

        project_configuration_sketch_states(&mut ir, &[history], &[lane]);

        assert_eq!(ir.model.sketches.len(), 1);
        assert!(matches!(
            &ir.model.configurations[0].feature_states[&feature_id].definition,
            FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } if sketch == &sketch_id
        ));
        assert!(matches!(
            &ir.model.configurations[0].feature_states[&spatial_feature_id].definition,
            FeatureDefinition::SpatialSketch {
                sketch: Some(sketch),
            } if sketch == &spatial_sketch_id
        ));
    }

    #[test]
    fn configuration_hole_inherits_shared_construction_and_placement() {
        use cadmpeg_ir::features::{
            Extent, FeatureDefinition, FeatureId, HoleKind, HolePlacement, Length,
        };

        let id = FeatureId("test:model:feature#hole".into());
        let base = cadmpeg_ir::features::Feature {
            id: id.clone(),
            ordinal: 0,
            name: Some("Hole".into()),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Hole {
                face: None,
                placements: vec![HolePlacement::Axis {
                    origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                    axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                }],
                kind: HoleKind::Counterbore {
                    diameter: Length(8.0),
                    depth: Length(4.0),
                },
                diameter: Some(Length(5.0)),
                extent: Some(Extent::Blind {
                    length: Length(12.0),
                }),
            },
            native_ref: None,
        };
        let mut configured = base.clone();
        configured.definition = FeatureDefinition::Hole {
            face: None,
            placements: Vec::new(),
            kind: HoleKind::Simple,
            diameter: None,
            extent: None,
        };

        inherit_configuration_hole_semantics(&mut configured.definition, &base.definition);

        assert_eq!(configured.definition, base.definition);
    }

    #[test]
    fn configuration_numeric_override_inherits_parameter_dimension() {
        use cadmpeg_ir::features::{
            ConfigurationId, DesignConfiguration, DesignParameter, FeatureId, ParameterId,
            ParameterValue,
        };

        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let parameter_id = ParameterId("test:model:parameter#depth".into());
        ir.model.parameters.push(DesignParameter {
            id: parameter_id.clone(),
            owner: FeatureId("test:model:feature#extrude".into()),
            ordinal: 0,
            name: "Depth".into(),
            expression: "7mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(7.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("test:model:configuration#default".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Default".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: Vec::new(),
            parameter_values: BTreeMap::from([(parameter_id.clone(), ParameterValue::Integer(7))]),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });

        align_configuration_parameter_kinds(&mut ir);

        assert_eq!(
            ir.model.configurations[0].parameter_values[&parameter_id],
            ParameterValue::Length(Length(7.0))
        );
    }
}

/// Bind a uniquely identified native sketch history node to solved sketch geometry.
pub fn bind_unique_sketch_feature(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    histories: &[FeatureHistory],
) {
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
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
                !sketch.profiles.is_empty(),
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
                    !sketch.profiles.is_empty(),
                ));
            }
        }
    }
    for (index, _, _, sketch, _) in &bindings {
        features[*index].definition = FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: Some(sketch.clone()),
        };
    }
    let mut aliases = Vec::new();
    for index in &feature_indices {
        let FeatureDefinition::Sketch { sketch: None, .. } = &features[*index].definition else {
            continue;
        };
        let Some(base_name) = features[*index]
            .name
            .as_deref()
            .and_then(sketch_alias_base_name)
        else {
            continue;
        };
        let candidates = bindings
            .iter()
            .filter(|(base_index, _, base_native_ref, _, _)| {
                let alias_native = features[*index]
                    .native_ref
                    .as_deref()
                    .and_then(|native_ref| native_features.get(native_ref));
                let base_native = native_features.get(base_native_ref.as_str());
                features[*base_index].name.as_deref() == Some(base_name)
                    && alias_native.zip(base_native).is_some_and(|(alias, base)| {
                        alias.xml_tag == base.xml_tag
                            && alias.input_class == base.input_class
                            && alias.parameters == base.parameters
                            && alias.content == base.content
                    })
            })
            .collect::<Vec<_>>();
        let [(base_index, base_dependency, _, sketch, has_profile)] = candidates.as_slice() else {
            continue;
        };
        let Some(native_ref) = features[*index].native_ref.clone() else {
            continue;
        };
        if !features[*index].dependencies.contains(base_dependency) {
            features[*index]
                .dependencies
                .push((*base_dependency).clone());
        }
        aliases.push((
            *base_index,
            (*base_dependency).clone(),
            native_ref,
            (*sketch).clone(),
            *has_profile,
        ));
    }
    bindings.extend(aliases);
    for feature in features {
        for (_, dependency, native_ref, sketch, has_profile) in &bindings {
            if bind_definition_sketch(
                &mut feature.definition,
                native_ref,
                dependency,
                sketch,
                *has_profile,
            ) && !feature.dependencies.contains(dependency)
            {
                feature.dependencies.push(dependency.clone());
            }
        }
    }
}

fn sketch_alias_base_name(name: &str) -> Option<&str> {
    let (base, suffix) = name.rsplit_once('<')?;
    let ordinal = suffix.strip_suffix('>')?;
    (!base.is_empty() && !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(base)
}

/// Assign stable neutral regeneration ordinals with every structural parent and
/// explicit dependency before its consumer. Native history ordinals retain the
/// independent Keywords serialization order.
pub fn order_features_for_regeneration(features: &mut [cadmpeg_ir::features::Feature]) -> bool {
    let by_id = features
        .iter()
        .enumerate()
        .map(|(index, feature)| (feature.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut outgoing = vec![Vec::<usize>::new(); features.len()];
    let mut indegree = vec![0usize; features.len()];
    for (consumer, feature) in features.iter().enumerate() {
        let mut predecessors = feature
            .dependencies
            .iter()
            .collect::<std::collections::HashSet<_>>();
        if let Some(parent) = &feature.parent {
            predecessors.insert(parent);
        }
        for predecessor in predecessors {
            let Some(&source) = by_id.get(predecessor) else {
                continue;
            };
            outgoing[source].push(consumer);
            indegree[consumer] += 1;
        }
    }
    let mut ready = std::collections::BTreeSet::new();
    for (index, feature) in features.iter().enumerate() {
        if indegree[index] == 0 {
            ready.insert((feature.ordinal, feature.id.clone(), index));
        }
    }
    let mut order = Vec::with_capacity(features.len());
    while let Some(item) = ready.pop_first() {
        let index = item.2;
        order.push(index);
        for &consumer in &outgoing[index] {
            indegree[consumer] -= 1;
            if indegree[consumer] == 0 {
                let feature = &features[consumer];
                ready.insert((feature.ordinal, feature.id.clone(), consumer));
            }
        }
    }
    if order.len() != features.len() {
        return false;
    }
    for (ordinal, index) in order.into_iter().enumerate() {
        features[index].ordinal = ordinal as u64;
    }
    true
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
                if let Extent::ToFace { face } | Extent::OffsetFromFace { face, .. } = extent {
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
    feature_ref: &FeatureId,
    sketch: &cadmpeg_ir::sketches::SketchId,
    has_profile: bool,
) -> bool {
    let bind_profile = |profile: &mut ProfileRef| {
        if has_profile
            && (matches!(profile, ProfileRef::Unresolved(owner) if owner == native_ref)
                || matches!(profile, ProfileRef::Native(value) if value == native_ref)
                || matches!(profile, ProfileRef::Feature(value) if value == feature_ref))
        {
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
    features_by_source: &HashMap<&str, &Feature>,
    history_features: &[Feature],
) -> FeatureDefinition {
    if let Some(role) = feature_tree_node_role(feature, history_features) {
        return FeatureDefinition::TreeNode { role };
    }
    let class = classify(feature);
    if class == Some(FeatureClass::CosmeticThread) {
        return project_cosmetic_thread(feature);
    }
    if class == Some(FeatureClass::Sketch) {
        return if feature.kind.eq_ignore_ascii_case("3DSketch")
            || feature.input_class.as_deref() == Some("mo3DProfileFeature_c")
        {
            FeatureDefinition::SpatialSketch { sketch: None }
        } else {
            FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: None,
            }
        };
    }
    if class == Some(FeatureClass::SketchBlockDefinition) {
        return FeatureDefinition::SketchBlockDefinition { sketch: None };
    }
    if class == Some(FeatureClass::SketchBlockInstance) {
        return FeatureDefinition::SketchBlockInstance {
            block: feature
                .properties
                .get("BlockDefinition")
                .and_then(|source| by_source.get(source.as_str()).cloned()),
            placement: sketch_block_placement(feature),
        };
    }
    if class == Some(FeatureClass::ReferencePlane) && is_offset_plane(feature) {
        return project_offset_plane(feature, by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if let Some(plane) = principal_plane_in_history(feature, features_by_source, history_features) {
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
        project_hole(feature, features_by_source)
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

fn project_cosmetic_thread(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::CosmeticThread {
        face: feature
            .properties
            .get("Face")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        diameter: feature
            .parameters
            .get("D2")
            .and_then(|value| parse_dimension_display_length(value))
            .filter(|value| *value > 0.0)
            .map(Length),
        extent: match feature.parameters.get("D1") {
            Some(value) => parse_positive_dimension_length_mm(value).map(|length| {
                CosmeticThreadExtent::Blind {
                    length: Length(length),
                }
            }),
            None => Some(CosmeticThreadExtent::Through),
        },
    }
}

fn sketch_block_placement(feature: &Feature) -> Option<Transform> {
    let origin = parse_point3_mm(feature.properties.get("BlockOrigin")?)?;
    let mut placement = Transform::identity();
    placement.rows[0][3] = origin.x;
    placement.rows[1][3] = origin.y;
    placement.rows[2][3] = origin.z;
    Some(placement)
}

fn feature_tree_node_role(
    feature: &Feature,
    history_features: &[Feature],
) -> Option<FeatureTreeNodeRole> {
    reserved_feature_tree_node_role(feature, history_features)
        .or_else(|| native_object_class(feature.input_class.as_deref()?).tree_node)
}

fn reserved_feature_tree_node_role(
    feature: &Feature,
    history_features: &[Feature],
) -> Option<FeatureTreeNodeRole> {
    let layout = feature_manager_layout(history_features)?;
    if !classless_builtin_node(feature) {
        return None;
    }
    let source = feature.source_id.as_deref()?;
    match (layout, feature.xml_tag.as_str(), source) {
        (FeatureManagerLayout::Current, tag, "1") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::Annotations)
        }
        (FeatureManagerLayout::Current, tag, "5") if tag.eq_ignore_ascii_case("Sketch") => {
            Some(FeatureTreeNodeRole::ModelOrigin)
        }
        (FeatureManagerLayout::Current, tag, "6") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::Current, tag, "12") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::Current, tag, "13" | "14" | "15")
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::Legacy, tag, "2") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::Legacy, tag, "7") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::Legacy, tag, "8") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::LightsAtSix, tag, "6")
        | (FeatureManagerLayout::FoldersAtSeven, tag, "6")
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::LightsAtSix, tag, "7") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::LightsAtSix, tag, "8") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::FoldersAtSeven, tag, "10")
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::FoldersAtSeven, tag, "11" | "12")
            if tag.eq_ignore_ascii_case("Feature") =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (FeatureManagerLayout::OriginAtSix, tag, "2") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::LightsAndCameras)
        }
        (FeatureManagerLayout::OriginAtSix, tag, "7") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (FeatureManagerLayout::OriginAtSix, tag, "8") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (_, tag, _)
            if tag.eq_ignore_ascii_case("Feature")
                && repeated_builtin_node_kind(
                    feature,
                    history_features,
                    layout,
                    FeatureTreeNodeRole::AmbientLight,
                ) =>
        {
            Some(FeatureTreeNodeRole::AmbientLight)
        }
        (_, tag, _)
            if tag.eq_ignore_ascii_case("Feature")
                && repeated_builtin_node_kind(
                    feature,
                    history_features,
                    layout,
                    FeatureTreeNodeRole::DirectionalLight,
                ) =>
        {
            Some(FeatureTreeNodeRole::DirectionalLight)
        }
        (_, tag, "-1") if tag.eq_ignore_ascii_case("Feature") => {
            Some(FeatureTreeNodeRole::SheetMetal)
        }
        (_, _, _) if empty_feature_tree_node(feature) => Some(FeatureTreeNodeRole::ExplodedViews),
        _ => None,
    }
}

fn classless_builtin_node(feature: &Feature) -> bool {
    feature.input_class.is_none() && builtin_node_payload(feature)
}

fn builtin_node_payload(feature: &Feature) -> bool {
    feature.parameters.is_empty()
        && feature.dimension_properties.is_empty()
        && feature.properties.is_empty()
        && feature.text.is_none()
        && feature.content.is_empty()
}

fn classless_or_scene_builtin_node(feature: &Feature) -> bool {
    builtin_node_payload(feature)
        && feature.input_class.as_deref().is_none_or(|class| {
            matches!(
                native_object_class(class).tree_node,
                Some(
                    FeatureTreeNodeRole::AmbientLight
                        | FeatureTreeNodeRole::DirectionalLight
                        | FeatureTreeNodeRole::PointLight
                        | FeatureTreeNodeRole::SpotLight
                )
            )
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureManagerLayout {
    OriginAtSix,
    LightsAtSix,
    FoldersAtSeven,
    Legacy,
    Current,
}

fn feature_manager_layout(features: &[Feature]) -> Option<FeatureManagerLayout> {
    let matches_roster = |roster: &[(&str, &str)]| {
        roster.iter().all(|(source, class)| {
            let mut matches = features.iter().filter(|feature| {
                feature.source_id.as_deref() == Some(*source)
                    && feature.input_class.as_deref() == Some(*class)
            });
            matches.next().is_some() && matches.next().is_none()
        })
    };
    let matches_builtin_sources = |sources: &[&str]| {
        sources.iter().all(|source| {
            let mut matches = features.iter().filter(|feature| {
                feature.source_id.as_deref() == Some(*source)
                    && classless_or_scene_builtin_node(feature)
            });
            matches.next().is_some() && matches.next().is_none()
        })
    };
    let legacy = matches_roster(&[
        ("6", "moOriginProfileFeature_c"),
        ("9", "moSurfaceBodyFolder_c"),
        ("10", "moSolidBodyFolder_c"),
        ("12", "moDocsFolder_c"),
        ("13", "moCommentsFolder_c"),
    ]);
    let current = matches_roster(&[
        ("7", "moDocsFolder_c"),
        ("8", "moCommentsFolder_c"),
        ("9", "moSolidBodyFolder_c"),
        ("10", "moSurfaceBodyFolder_c"),
    ]);
    let default_frame = matches_roster(&[
        ("1", "moDetailCabinet_c"),
        ("2", "moRefPlane_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moOriginProfileFeature_c"),
    ]);
    let origin_at_six = matches_roster(&[
        ("1", "moDetailCabinet_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moRefPlane_c"),
        ("6", "moOriginProfileFeature_c"),
    ]) && matches_builtin_sources(&["2", "7", "8"])
        && !legacy;
    let lights_at_six = default_frame && matches_builtin_sources(&["6", "7", "8"]);
    let folders_at_seven = default_frame
        && matches_roster(&[("7", "moSolidBodyFolder_c"), ("8", "moSurfaceBodyFolder_c")])
        && matches_builtin_sources(&["6", "10", "11", "12"]);
    let mut layouts = [
        (origin_at_six, FeatureManagerLayout::OriginAtSix),
        (lights_at_six, FeatureManagerLayout::LightsAtSix),
        (folders_at_seven, FeatureManagerLayout::FoldersAtSeven),
        (legacy, FeatureManagerLayout::Legacy),
        (current, FeatureManagerLayout::Current),
    ]
    .into_iter()
    .filter_map(|(matches, layout)| matches.then_some(layout));
    let layout = layouts.next()?;
    layouts.next().is_none().then_some(layout)
}

fn repeated_builtin_node_kind(
    feature: &Feature,
    features: &[Feature],
    layout: FeatureManagerLayout,
    role: FeatureTreeNodeRole,
) -> bool {
    let reserved_source = match (layout, role) {
        (FeatureManagerLayout::OriginAtSix, FeatureTreeNodeRole::AmbientLight)
        | (FeatureManagerLayout::LightsAtSix, FeatureTreeNodeRole::AmbientLight) => "7",
        (FeatureManagerLayout::OriginAtSix, FeatureTreeNodeRole::DirectionalLight)
        | (FeatureManagerLayout::LightsAtSix, FeatureTreeNodeRole::DirectionalLight) => "8",
        (FeatureManagerLayout::FoldersAtSeven, FeatureTreeNodeRole::AmbientLight) => "10",
        (FeatureManagerLayout::FoldersAtSeven, FeatureTreeNodeRole::DirectionalLight) => "11",
        (FeatureManagerLayout::Legacy, FeatureTreeNodeRole::AmbientLight) => "7",
        (FeatureManagerLayout::Legacy, FeatureTreeNodeRole::DirectionalLight) => "8",
        (FeatureManagerLayout::Current, FeatureTreeNodeRole::AmbientLight) => "12",
        (FeatureManagerLayout::Current, FeatureTreeNodeRole::DirectionalLight) => "13",
        _ => return false,
    };
    let mut anchors = features.iter().filter(|candidate| {
        candidate.source_id.as_deref() == Some(reserved_source) && classless_builtin_node(candidate)
    });
    let Some(anchor) = anchors.next() else {
        return false;
    };
    anchors.next().is_none() && !anchor.kind.is_empty() && feature.kind == anchor.kind
}

fn empty_feature_tree_node(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.name.is_empty()
        && feature.dimension_properties.is_empty()
        && feature.text.is_none()
        && feature.content.is_empty()
}

fn feature_tree_node_kind(role: FeatureTreeNodeRole) -> &'static str {
    match role {
        FeatureTreeNodeRole::Annotations => "Annotations",
        FeatureTreeNodeRole::AmbientLight => "Ambient",
        FeatureTreeNodeRole::Comments => "Comments",
        FeatureTreeNodeRole::DesignBinder => "Design Binder",
        FeatureTreeNodeRole::Details => "Details",
        FeatureTreeNodeRole::DissectedProfile => "Profile Selection",
        FeatureTreeNodeRole::DirectionalLight => "Directional",
        FeatureTreeNodeRole::Equations => "Equations",
        FeatureTreeNodeRole::ExplodedViews => "Exploded Views",
        FeatureTreeNodeRole::Favorites => "Favorites",
        FeatureTreeNodeRole::FeatureFolder => "Folder",
        FeatureTreeNodeRole::History => "History",
        FeatureTreeNodeRole::LightsAndCameras => "Lights and Cameras",
        FeatureTreeNodeRole::Markups => "Markups",
        FeatureTreeNodeRole::ModelOrigin => "Origin",
        FeatureTreeNodeRole::PointLight => "Point Light",
        FeatureTreeNodeRole::Materials => "SOLIDWORKS Materials",
        FeatureTreeNodeRole::Notes => "Notes",
        FeatureTreeNodeRole::SelectionSets => "Selection Sets",
        FeatureTreeNodeRole::Sensors => "Sensors",
        FeatureTreeNodeRole::SheetMetal => "Sheet Metal",
        FeatureTreeNodeRole::SolidBodies => "Solid Bodies",
        FeatureTreeNodeRole::SpotLight => "Spot Light",
        FeatureTreeNodeRole::SurfaceBodies => "Surface Bodies",
        FeatureTreeNodeRole::Tables => "Tables",
    }
}

fn feature_family(feature: &Feature, family: &str) -> bool {
    feature.xml_tag.eq_ignore_ascii_case(family)
}

/// Reject a neutral edit that retargets an existing native record to an
/// operation family it did not originate in. A missing record (a freshly
/// synthesized feature) always passes. The accepted native families are part of
/// each feature type's write schema; centralizing them keeps the read
/// classification and the write guard from drifting apart. The emitted
/// `NotImplemented` message is byte-identical to the historical per-arm guard.
fn require_same_family(
    existing: Option<&Feature>,
    feature_id: &FeatureId,
    families: &[&str],
) -> Result<(), CodecError> {
    if existing.is_some_and(|record| !families.iter().any(|family| feature_family(record, family)))
    {
        return Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {feature_id} changes operation family"
        )));
    }
    Ok(())
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
        .or_else(|| (feature.input_class.as_deref() == Some("moCut_c")).then_some(BooleanOp::Cut))
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
    classify(feature) == Some(FeatureClass::ReferencePlane)
        && feature
            .parameters
            .get("D1")
            .and_then(|value| parse_dimension_length_mm(value))
            .is_some()
}

fn principal_plane_in_history(
    feature: &Feature,
    features_by_source: &HashMap<&str, &Feature>,
    history_features: &[Feature],
) -> Option<cadmpeg_ir::features::PrincipalPlane> {
    use cadmpeg_ir::features::PrincipalPlane;

    if let Some(plane) = principal_plane(feature) {
        return Some(plane);
    }
    let legacy_shape = |record: &Feature| {
        record.input_class.is_none()
            && record.xml_tag.eq_ignore_ascii_case("Feature")
            && record.parameters.is_empty()
            && record.properties.is_empty()
            && !record.kind.is_empty()
    };
    let source_triplet = ["2", "3", "4"].map(|source| features_by_source.get(source).copied());
    if let [Some(front), Some(top), Some(right)] = source_triplet {
        if [front, top, right].into_iter().all(legacy_shape)
            && front.kind == top.kind
            && front.kind == right.kind
        {
            return match feature.source_id.as_deref() {
                Some("2") => Some(PrincipalPlane::Front),
                Some("3") => Some(PrincipalPlane::Top),
                Some("4") => Some(PrincipalPlane::Right),
                _ => None,
            };
        }
    }

    history_features.windows(4).find_map(|records| {
        let [front, top, right, successor] = records else {
            return None;
        };
        let triplet = [front, top, right];
        if !triplet.into_iter().all(|record| {
            record.xml_tag.eq_ignore_ascii_case("Feature")
                && record.parameters.is_empty()
                && !record.kind.is_empty()
                && match record.input_class.as_deref() {
                    Some(class) => {
                        native_object_class(class).kind == NativeClassKind::ReferencePlane
                    }
                    None => record.properties.is_empty(),
                }
                && record.source_id.is_none()
                && record.tree_parent.is_none()
                && record.parent_source_id.is_none()
        }) || front.kind != top.kind
            || front.kind != right.kind
            || top.ordinal != front.ordinal + 1
            || right.ordinal != top.ordinal + 1
            || !successor.xml_tag.eq_ignore_ascii_case("Feature")
            || !successor.parameters.is_empty()
            || !successor.properties.is_empty()
            || successor.kind.is_empty()
            || successor.input_class.as_deref().is_some_and(|class| {
                native_object_class(class).kind != NativeClassKind::OriginProfileFeature
            })
            || successor.source_id.is_some()
            || successor.tree_parent.is_some()
            || successor.parent_source_id.is_some()
            || successor.ordinal != right.ordinal + 1
            || successor.kind == front.kind
        {
            return None;
        }
        match feature.id.as_str() {
            id if id == front.id => Some(PrincipalPlane::Front),
            id if id == top.id => Some(PrincipalPlane::Top),
            id if id == right.id => Some(PrincipalPlane::Right),
            _ => None,
        }
    })
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
    let sole_length = || {
        let mut values = feature.parameters.values();
        let sole = values.next().filter(|_| values.next().is_none())?;
        parse_positive_length_mm(sole)
            .or_else(|| parse_positive_dimension_length_mm(sole))
            .map(Length)
    };
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
        None if !feature.parameters.contains_key("Depth")
            && !feature.parameters.contains_key("D1") =>
        {
            Extent::Unresolved
        }
        None | Some("Blind") => Extent::Blind {
            length: length("Depth").or_else(sole_length)?,
        },
        Some("Symmetric") => match length("Depth").or_else(sole_length) {
            Some(length) => Extent::Symmetric { length },
            None => Extent::Unresolved,
        },
        Some("TwoSided") => Extent::TwoSided {
            first: length("Depth")?,
            second: length("Depth2")?,
        },
        Some("ThroughAll") => Extent::ThroughAll,
        Some("ThroughAllBoth") => Extent::ThroughAllBoth,
        Some("ThroughNext") => Extent::ThroughNext,
        Some("ToFace") => Extent::ToFace {
            face: FaceSelection::Native(feature.properties.get("Face")?.clone()),
        },
        Some("ToVertex") => Extent::ToVertex {
            vertex: VertexSelection::Native(feature.properties.get("Vertex")?.clone()),
        },
        Some("OffsetFromFace") => match length("Depth").or_else(sole_length) {
            Some(offset) => Extent::OffsetFromFace {
                face: FaceSelection::Native(feature.properties.get("Face")?.clone()),
                offset,
            },
            None => Extent::Unresolved,
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
    let profile = if let Some(source) = feature.properties.get("Profile") {
        ProfileRef::Native(
            native_by_source
                .get(source.as_str())
                .map_or_else(|| source.clone(), |id| (*id).to_string()),
        )
    } else if let Some(children) = feature.properties.get("DissectableChildren") {
        let profiles = resolve_native_refs(children, native_by_source)?;
        match profiles.as_slice() {
            [profile] => ProfileRef::Native(profile.clone()),
            _ => ProfileRef::Unresolved(feature.id.clone()),
        }
    } else {
        ProfileRef::Unresolved(feature.id.clone())
    };
    Some(FeatureDefinition::Extrude {
        profile,
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
        Some(value) => Some(parse_valid_direction(value)?),
        None => None,
    };
    Some(FeatureDefinition::ProjectedCurve {
        source: PathRef::Native(source),
        target_faces: FaceSelection::Native(feature.properties.get("TargetFaces")?.clone()),
        direction,
        bidirectional: feature
            .properties
            .get("Bidirectional")
            .and_then(|value| parse_bool(value))
            .unwrap_or(false),
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
    let start_angle = match feature.parameters.get("StartAngle") {
        Some(value) => parse_angle_rad(value)?,
        None => 0.0,
    };
    Some(FeatureDefinition::Helix {
        axis_origin,
        axis_direction,
        radius: Length(radius),
        pitch: Length(pitch),
        revolutions,
        start_angle: Angle(start_angle),
        clockwise,
    })
}

fn project_native_axis_helix(feature: &Feature) -> Option<FeatureDefinition> {
    let axial_rise = parse_dimension_length_mm(feature.parameters.get("D3")?)?;
    let pitch = parse_dimension_length_mm(feature.parameters.get("D4")?)?;
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
        axial_rise: Length(axial_rise),
        pitch: Length(pitch),
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
    let mode = if feature_input_class(feature, NativeClassKind::SweepReferenceSurface)
        || feature.xml_tag == "Surface-Sweep"
        || feature.kind == "Surface-Sweep"
    {
        SweepMode::Surface
    } else if feature_input_class(feature, NativeClassKind::Sweep) {
        SweepMode::Solid {
            op: feature
                .properties
                .get("Operation")
                .and_then(|value| parse_boolean_op(value))
                .unwrap_or(BooleanOp::Unresolved),
        }
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
        path,
        mode,
        twist,
        scale,
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
            .map(|source| by_source.get(source).cloned().map(PatternSeed::Feature))
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
                second: match (
                    feature.properties.get("Direction2"),
                    feature.parameters.get("D4"),
                    feature.parameters.get("D2"),
                ) {
                    (Some(direction), Some(spacing), Some(count)) => {
                        Some(cadmpeg_ir::features::LinearPatternDirection {
                            direction: parse_valid_direction(direction)?,
                            spacing: Length(parse_positive_dimension_length_mm(spacing)?),
                            count: parse_count(count)?,
                        })
                    }
                    _ => None,
                },
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
    let ordered_angle = |ordinal| {
        feature
            .content
            .iter()
            .filter_map(|content| match content {
                FeatureContent::Dimension(name) => feature.parameters.get(name),
                FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
            })
            .filter_map(|value| parse_positive_angle_rad(value))
            .nth(ordinal)
    };
    let angle = |name, ordinal| {
        feature
            .parameters
            .get(name)
            .or_else(|| match name {
                "Angle" => feature.parameters.get("D1"),
                "Angle2" => feature.parameters.get("D2"),
                _ => None,
            })
            .and_then(|value| parse_positive_angle_rad(value))
            .or_else(|| ordered_angle(ordinal))
            .map(Angle)
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("OneSided") => angle("Angle", 0).map(|angle| Extent::Angle { angle }),
        Some("Symmetric") => angle("Angle", 0).map(|angle| Extent::SymmetricAngle { angle }),
        Some("TwoSided") => angle("Angle", 0)
            .zip(angle("Angle2", 1))
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
        .or_else(|| {
            (feature.input_class.as_deref() == Some("moRevCut_c")).then_some(BooleanOp::Cut)
        })
        .unwrap_or(BooleanOp::Unresolved);
    FeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile,
            axis,
            extent,
        },
        op,
    }
}

fn project_hole(
    feature: &Feature,
    features_by_source: &HashMap<&str, &Feature>,
) -> FeatureDefinition {
    let profile = hole_profile_construction(feature, features_by_source);
    let diameter = feature
        .parameters
        .get("Diameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length)
        .or_else(|| profile.as_ref().map(|profile| profile.diameter));
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
    let drill_point_angle = feature
        .parameters
        .get("DrillPointAngle")
        .and_then(|value| parse_bounded_angle_rad(value))
        .map(Angle);
    let thread = feature
        .parameters
        .get("ThreadMajorDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length)
        .zip(
            feature
                .parameters
                .get("ThreadDepth")
                .and_then(|value| parse_positive_length_mm(value))
                .map(Length),
        )
        .zip(drill_point_angle)
        .map(
            |((major_diameter, thread_depth), drill_point_angle)| HoleKind::Threaded {
                major_diameter,
                thread_depth,
                pitch: feature
                    .parameters
                    .get("ThreadPitch")
                    .and_then(|value| parse_positive_length_mm(value))
                    .map(Length),
                drill_point_angle,
            },
        );
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
            (Some(diameter), Some(depth)) => drill_point_angle.map_or(
                HoleKind::Counterbore { diameter, depth },
                |drill_point_angle| HoleKind::CounterboreDrilled {
                    diameter,
                    depth,
                    drill_point_angle,
                },
            ),
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
    } else if let Some(thread) = thread {
        thread
    } else if let Some(drill_point_angle) = drill_point_angle {
        HoleKind::SimpleDrilled { drill_point_angle }
    } else {
        profile
            .as_ref()
            .map_or(HoleKind::Simple, |profile| profile.kind.clone())
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("Blind") => feature
            .parameters
            .get("Depth")
            .and_then(|value| parse_positive_length_mm(value))
            .map(Length)
            .or_else(|| profile.as_ref().and_then(|profile| profile.depth))
            .map(|length| Extent::Blind { length }),
        Some("ThroughAll") => Some(Extent::ThroughAll),
        Some(_) => None,
    };
    FeatureDefinition::Hole {
        face: feature
            .properties
            .get("Face")
            .cloned()
            .map(FaceSelection::Native),
        placements: feature
            .properties
            .get("Position")
            .and_then(|value| parse_point3_mm(value))
            .zip(
                feature
                    .properties
                    .get("Direction")
                    .and_then(|value| parse_vector3(value))
                    .filter(|direction| valid_direction(*direction)),
            )
            .map(|(position, direction)| {
                vec![cadmpeg_ir::features::HolePlacement::Directed {
                    position,
                    direction,
                }]
            })
            .unwrap_or_default(),
        kind,
        diameter,
        extent,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct HoleProfileConstruction {
    diameter: Length,
    depth: Option<Length>,
    kind: HoleKind,
}

fn hole_profile_construction(
    feature: &Feature,
    features_by_source: &HashMap<&str, &Feature>,
) -> Option<HoleProfileConstruction> {
    let children = feature.properties.get("DissectableChildren")?;
    let mut constructions = children
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .filter_map(|source| features_by_source.get(source).copied())
        .filter(|profile| classify(profile) == Some(FeatureClass::Sketch))
        .filter_map(hole_sketch_construction);
    let construction = constructions.next()?;
    constructions.next().is_none().then_some(construction)
}

fn hole_sketch_construction(profile: &Feature) -> Option<HoleProfileConstruction> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum DimensionRole {
        Diameter,
        Length,
        Angle,
    }

    let mut diameters = Vec::new();
    let mut lengths = Vec::new();
    let mut angles = Vec::new();
    let mut roles = Vec::new();
    let source_dimensions = profile
        .content
        .iter()
        .filter_map(|content| match content {
            crate::records::FeatureContent::Dimension(name) => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let expressions = if source_dimensions.is_empty() {
        profile.parameters.values().collect::<Vec<_>>()
    } else {
        source_dimensions
            .into_iter()
            .filter_map(|name| profile.parameters.get(name))
            .collect::<Vec<_>>()
    };
    for expression in expressions {
        if strip_diameter_modifier(expression).is_some() {
            if let Some(value) = parse_dimension_display_length(expression)
                .filter(|value| *value > 0.0)
                .map(Length)
            {
                diameters.push(value);
                roles.push(DimensionRole::Diameter);
            }
        } else if let Some(value) = parse_bounded_angle_rad(expression).map(Angle) {
            angles.push(value);
            roles.push(DimensionRole::Angle);
        } else {
            if let Some(value) = parse_positive_dimension_length_mm(expression).map(Length) {
                lengths.push(value);
                roles.push(DimensionRole::Length);
            }
        }
    }
    diameters.sort_by(|left, right| left.0.total_cmp(&right.0));
    lengths.sort_by(|left, right| left.0.total_cmp(&right.0));
    match (diameters.as_slice(), lengths.as_slice(), angles.as_slice()) {
        ([diameter], depths, []) => Some(HoleProfileConstruction {
            diameter: *diameter,
            depth: match depths {
                [depth] => Some(*depth),
                _ => None,
            },
            kind: HoleKind::Simple,
        }),
        ([diameter], [depth], [drill_point_angle]) => Some(HoleProfileConstruction {
            diameter: *diameter,
            depth: Some(*depth),
            kind: HoleKind::SimpleDrilled {
                drill_point_angle: *drill_point_angle,
            },
        }),
        ([diameter, entry_diameter], [depth], [angle]) if diameter.0 < entry_diameter.0 => {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*depth),
                kind: HoleKind::Countersink {
                    diameter: *entry_diameter,
                    angle: *angle,
                },
            })
        }
        ([diameter, major_diameter], [thread_depth, drill_depth], [drill_point_angle])
            if roles
                == [
                    DimensionRole::Diameter,
                    DimensionRole::Length,
                    DimensionRole::Diameter,
                    DimensionRole::Length,
                    DimensionRole::Angle,
                ]
                && diameter.0 < major_diameter.0
                && thread_depth.0 < drill_depth.0 =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*drill_depth),
                kind: HoleKind::Threaded {
                    major_diameter: *major_diameter,
                    thread_depth: *thread_depth,
                    pitch: None,
                    drill_point_angle: *drill_point_angle,
                },
            })
        }
        ([diameter, entry_diameter], [entry_depth, depth], [drill_point_angle])
            if roles.last() == Some(&DimensionRole::Diameter)
                && diameter.0 < entry_diameter.0
                && entry_depth.0 < depth.0 =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*depth),
                kind: HoleKind::CounterboreDrilled {
                    diameter: *entry_diameter,
                    depth: *entry_depth,
                    drill_point_angle: *drill_point_angle,
                },
            })
        }
        ([diameter, entry_diameter], [entry_depth, depth], [])
            if diameter.0 < entry_diameter.0 && entry_depth.0 < depth.0 =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*depth),
                kind: HoleKind::Counterbore {
                    diameter: *entry_diameter,
                    depth: *entry_depth,
                },
            })
        }
        _ => None,
    }
}

pub(crate) fn is_hole_profile_construction(feature: &Feature) -> bool {
    hole_sketch_construction(feature).is_some()
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
        distance: Length(parse_length_mm(feature.parameters.get("Distance")?)?),
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
    let continuity =
        crate::feature_schema::parse_surface_continuity(feature.properties.get("Continuity")?)?;
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
    let keep = crate::feature_schema::parse_trim_region(feature.properties.get("Keep")?)?;
    Some(FeatureDefinition::TrimSurface {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        tool: PathRef::Native(tool),
        keep,
    })
}

fn project_extend_surface(feature: &Feature) -> Option<FeatureDefinition> {
    let method = crate::feature_schema::parse_surface_extension(feature.properties.get("Method")?)?;
    Some(FeatureDefinition::ExtendSurface {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        distance: Length(parse_positive_length_mm(
            feature.parameters.get("Distance")?,
        )?),
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
    let op = feature
        .properties
        .get("Operation")
        .map_or(Some(BooleanOp::Unresolved), |value| parse_boolean_op(value))?;
    if op == BooleanOp::NewBody {
        return None;
    }
    Some(FeatureDefinition::Combine {
        target: feature
            .properties
            .get("Target")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        tools: feature
            .properties
            .get("Tools")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
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
    for (suffix, scale) in [
        ("uin", 25.4e-6),
        ("mil", 0.0254),
        ("mm", 1.0),
        ("cm", 10.0),
        ("in", 25.4),
        ("ft", 304.8),
        ("nm", 1.0e-6),
        ("um", 1.0e-3),
        ("µm", 1.0e-3),
        ("μm", 1.0e-3),
        ("Å", 1.0e-7),
        ("A", 1.0e-7),
        ("m", 1000.0),
    ] {
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

pub(crate) fn parse_positive_dimension_length_mm(value: &str) -> Option<f64> {
    parse_positive_length_mm(value).or_else(|| {
        value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
    })
}

pub(crate) fn parse_dimension_length_mm(value: &str) -> Option<f64> {
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
        format_f64_literal(value)
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
    format!("{}mm", format_f64_literal(value))
}

pub(crate) fn parse_angle_rad(value: &str) -> Option<f64> {
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

pub(crate) fn format_angle_rad(value: f64) -> String {
    format!("{}rad", format_f64_literal(value))
}

fn format_f64_literal(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1.0e-6..1.0e15).contains(&magnitude) {
        format!("{value:e}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod literal_tests {
    use super::{
        apply_parameter_function, compare_parameter_values, dimension_display, exact_integer_f64,
        exponentiate_parameter_value, format_f64_literal, parse_length_mm, parse_parameter_literal,
        rewrite_parameter_expression, DimensionDisplay, ParameterExpressionParser, ParameterValue,
    };

    #[test]
    fn native_scalar_literals_are_compact_and_bit_exact() {
        for value in [
            0.0,
            -0.0,
            0.125,
            -42.5,
            7.745_183_829_698_638e-127,
            -5.486_124_068_793_69e307,
        ] {
            let literal = format_f64_literal(value);
            let parsed = literal.parse::<f64>().unwrap();
            assert_eq!(parsed.to_bits(), value.to_bits(), "{literal}");
        }
        assert_eq!(format_f64_literal(0.125), "0.125");
        assert_eq!(
            format_f64_literal(7.745_183_829_698_638e-127),
            "7.745183829698638e-127"
        );
    }

    #[test]
    fn solidworks_length_units_convert_to_millimeters() {
        for (literal, expected) in [
            ("1A", 1.0e-7),
            ("1Å", 1.0e-7),
            ("1nm", 1.0e-6),
            ("1um", 1.0e-3),
            ("1µm", 1.0e-3),
            ("1μm", 1.0e-3),
            ("1mm", 1.0),
            ("1cm", 10.0),
            ("1m", 1000.0),
            ("1uin", 25.4e-6),
            ("1mil", 0.0254),
            ("1in", 25.4),
            ("1ft", 304.8),
        ] {
            assert_eq!(parse_length_mm(literal), Some(expected), "{literal}");
        }
    }

    #[test]
    fn diameter_display_literals_participate_in_expressions() {
        let aliases = std::collections::HashMap::new();
        let values = std::collections::HashMap::new();
        assert_eq!(
            ParameterExpressionParser::new("<MOD-DIAM>4mm / 2", &aliases, &values).parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(2.0)))
        );
        assert_eq!(
            ParameterExpressionParser::new("<MOD-DIAM>4 + 1mm", &aliases, &values).parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(5.0)))
        );
        assert_eq!(
            ParameterExpressionParser::new("&lt;MOD-DIAM&gt;4mm / 2", &aliases, &values).parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(2.0)))
        );
        assert_eq!(
            parse_parameter_literal("&lt;MOD-DIAM&gt;4.917"),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(4.917)))
        );
    }

    #[test]
    fn radius_display_literals_participate_in_expressions() {
        let aliases = std::collections::HashMap::new();
        let values = std::collections::HashMap::new();
        assert_eq!(
            ParameterExpressionParser::new("<MOD-RHO>4mm / 2", &aliases, &values).parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(2.0)))
        );
        assert_eq!(
            ParameterExpressionParser::new("&lt;MOD-RHO&gt;4 + 1mm", &aliases, &values).parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(5.0)))
        );
        assert_eq!(
            parse_parameter_literal("<MOD-RHO>0.5"),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(0.5)))
        );
        assert_eq!(
            dimension_display("&lt;MOD-RHO&gt;0.5"),
            Some(DimensionDisplay::Radius)
        );
    }

    #[test]
    fn dimension_decorations_preserve_the_nominal_scalar() {
        let aliases = std::collections::HashMap::new();
        let values = std::collections::HashMap::new();
        for (expression, expected, display) in [
            ("2X<MOD-DIAM>1.2", 1.2, DimensionDisplay::Diameter),
            ("6XR2", 2.0, DimensionDisplay::Radius),
            ("<MOD-DIAM>15H7", 15.0, DimensionDisplay::Diameter),
            ("3x &lt;MOD-RHO&gt;0.5", 0.5, DimensionDisplay::Radius),
        ] {
            assert_eq!(
                parse_parameter_literal(expression),
                Some(ParameterValue::Length(cadmpeg_ir::features::Length(
                    expected
                ))),
                "{expression}"
            );
            assert_eq!(dimension_display(expression), Some(display), "{expression}");
            assert_eq!(
                ParameterExpressionParser::new(expression, &aliases, &values).parse(),
                Some(ParameterValue::Length(cadmpeg_ir::features::Length(
                    expected
                ))),
                "{expression}"
            );
        }
        assert_eq!(parse_parameter_literal("x2"), None);
        assert_eq!(parse_parameter_literal("15mmH7"), None);
        assert_eq!(parse_parameter_literal("<MOD-DIAM>15H"), None);
    }

    #[test]
    fn solidworks_sign_function_is_three_way() {
        for (argument, expected) in [(-2, -1), (0, 0), (2, 1)] {
            assert_eq!(
                apply_parameter_function("sgn", &ParameterValue::Integer(argument)),
                Some(ParameterValue::Integer(expected))
            );
        }
    }

    #[test]
    fn integer_function_preserves_discrete_integer_values() {
        for value in [i64::MIN, -(1_i64 << 53) - 1, (1_i64 << 53) + 1, i64::MAX] {
            assert_eq!(
                apply_parameter_function("int", &ParameterValue::Integer(value)),
                Some(ParameterValue::Integer(value))
            );
        }
        assert_eq!(
            apply_parameter_function("int", &ParameterValue::Real(-3.75)),
            Some(ParameterValue::Integer(-3))
        );
    }

    #[test]
    fn integer_powers_preserve_exact_exponent_parity() {
        let odd = ParameterValue::Integer((1_i64 << 53) + 1);
        assert_eq!(
            exponentiate_parameter_value(&ParameterValue::Integer(-1), &odd),
            Some(ParameterValue::Integer(-1))
        );
        assert_eq!(
            exponentiate_parameter_value(
                &ParameterValue::Integer(-1),
                &ParameterValue::Integer(-((1_i64 << 53) + 1)),
            ),
            Some(ParameterValue::Real(-1.0))
        );
        assert_eq!(
            exponentiate_parameter_value(&ParameterValue::Integer(2), &ParameterValue::Integer(-3),),
            Some(ParameterValue::Real(0.125))
        );
    }

    #[test]
    fn non_finite_parameter_literals_have_no_evaluated_value() {
        for literal in ["NaN", "inf", "-inf"] {
            assert_eq!(parse_parameter_literal(literal), None, "{literal}");
        }
    }

    #[test]
    fn bare_binary_digits_are_integer_parameters() {
        assert_eq!(
            parse_parameter_literal("0"),
            Some(ParameterValue::Integer(0))
        );
        assert_eq!(
            parse_parameter_literal("1"),
            Some(ParameterValue::Integer(1))
        );
        assert_eq!(
            parse_parameter_literal("true"),
            Some(ParameterValue::Boolean(true))
        );
        assert_eq!(
            parse_parameter_literal("false"),
            Some(ParameterValue::Boolean(false))
        );
    }

    #[test]
    fn native_scalars_accept_only_exact_integer_values() {
        let largest_consecutive = 1_i64 << 53;
        assert_eq!(exact_integer_f64(largest_consecutive), Some(2_f64.powi(53)));
        assert_eq!(exact_integer_f64(largest_consecutive + 1), None);
        assert_eq!(exact_integer_f64(i64::MIN), Some(i64::MIN as f64));
        assert_eq!(exact_integer_f64(i64::MAX), None);
    }

    #[test]
    fn mixed_numeric_comparisons_preserve_integer_identity() {
        let integer = ParameterValue::Integer((1_i64 << 53) + 1);
        let rounded_real = ParameterValue::Real(2_f64.powi(53));
        assert_eq!(
            compare_parameter_values(&integer, &rounded_real, "="),
            Some(false)
        );
        assert_eq!(
            compare_parameter_values(&integer, &rounded_real, ">"),
            Some(true)
        );
        assert_eq!(
            compare_parameter_values(&rounded_real, &integer, "<"),
            Some(true)
        );

        assert_eq!(
            compare_parameter_values(
                &ParameterValue::Integer(-3),
                &ParameterValue::Real(-3.5),
                ">",
            ),
            Some(true)
        );
        assert_eq!(
            compare_parameter_values(
                &ParameterValue::Integer(i64::MAX),
                &ParameterValue::Real(-(i64::MIN as f64)),
                "<",
            ),
            Some(true)
        );
    }

    #[test]
    fn expression_rewrite_quotes_hyphenated_identifiers() {
        let aliases = std::collections::HashMap::from([("Width".into(), "Wall-Gauge".into())]);
        assert_eq!(
            rewrite_parameter_expression("Width * 2", &aliases).as_deref(),
            Some("\"Wall-Gauge\" * 2")
        );
    }
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
        FaceSelection::Native(native)
        | FaceSelection::Resolved { native, .. }
        | FaceSelection::Generated { native, .. }
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

fn vertex_selection_value(selection: &VertexSelection) -> Option<String> {
    match selection {
        VertexSelection::Native(native) | VertexSelection::Generated { native, .. }
            if !native.trim().is_empty() =>
        {
            Some(native.clone())
        }
        _ => None,
    }
}

fn edge_selection_value(selection: &EdgeSelection) -> Option<String> {
    match selection {
        EdgeSelection::Native(native)
        | EdgeSelection::Resolved { native, .. }
        | EdgeSelection::Generated { native, .. }
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
        BodySelection::Native(native)
        | BodySelection::Resolved { native, .. }
        | BodySelection::Generated { native, .. }
        | BodySelection::Local { native, .. }
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
    let expression = expression.trim();
    if expression.eq_ignore_ascii_case("true") {
        return Some(ParameterValue::Boolean(true));
    }
    if expression.eq_ignore_ascii_case("false") {
        return Some(ParameterValue::Boolean(false));
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
        .filter(|value| value.is_finite())
        .map(ParameterValue::Real)
}

fn dimension_display(expression: &str) -> Option<DimensionDisplay> {
    let expression = strip_dimension_count(expression.trim());
    if strip_diameter_modifier(expression).is_some()
        || (expression.starts_with(['⌀', 'Ø']) && parse_length_mm(expression).is_some())
    {
        Some(DimensionDisplay::Diameter)
    } else if strip_radius_modifier(expression).is_some()
        || (expression.starts_with(['R', 'r']) && parse_length_mm(expression).is_some())
    {
        Some(DimensionDisplay::Radius)
    } else {
        None
    }
}

fn parse_dimension_display_length(expression: &str) -> Option<f64> {
    let expression = strip_dimension_count(expression.trim());
    let value = strip_diameter_modifier(expression)
        .or_else(|| strip_radius_modifier(expression))
        .unwrap_or(expression)
        .trim();
    parse_dimension_length_mm(value)
        .or_else(|| strip_dimension_fit(value).and_then(parse_dimension_length_mm))
        .or_else(|| parse_length_mm(expression))
}

fn strip_dimension_count(expression: &str) -> &str {
    let digit_count = expression.bytes().take_while(u8::is_ascii_digit).count();
    let (count, rest) = expression.split_at(digit_count);
    if !count.is_empty()
        && count.parse::<u64>().is_ok_and(|count| count > 0)
        && rest.starts_with(['X', 'x'])
    {
        rest[1..].trim_start()
    } else {
        expression
    }
}

fn strip_dimension_fit(value: &str) -> Option<&str> {
    let fit_start = value
        .char_indices()
        .find_map(|(offset, character)| character.is_ascii_alphabetic().then_some(offset))?;
    let (nominal, fit) = value.split_at(fit_start);
    let grade_start = fit
        .char_indices()
        .find_map(|(offset, character)| character.is_ascii_digit().then_some(offset))?;
    let (position, grade) = fit.split_at(grade_start);
    (!nominal.is_empty()
        && !position.is_empty()
        && position.bytes().all(|byte| byte.is_ascii_alphabetic())
        && grade.bytes().all(|byte| byte.is_ascii_digit()))
    .then_some(nominal)
}

fn strip_diameter_modifier(expression: &str) -> Option<&str> {
    let expression = expression.trim();
    expression
        .strip_prefix("<MOD-DIAM>")
        .or_else(|| expression.strip_prefix("&lt;MOD-DIAM&gt;"))
}

fn strip_radius_modifier(expression: &str) -> Option<&str> {
    let expression = expression.trim();
    expression
        .strip_prefix("<MOD-RHO>")
        .or_else(|| expression.strip_prefix("&lt;MOD-RHO&gt;"))
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
        ParameterValue::Real(value) => format_f64_literal(*value),
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

/// Stable hash of configuration-local evaluated parameter state.
pub fn configuration_parameter_value_hash(configurations: &[DesignConfiguration]) -> String {
    let mut values = configurations
        .iter()
        .filter(|configuration| !configuration.parameter_values.is_empty())
        .map(|configuration| (&configuration.id, &configuration.parameter_values))
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(right.0));
    hash_debug(&values)
}

/// Stable hash of configuration-local evaluated feature state.
pub fn configuration_feature_state_hash(configurations: &[DesignConfiguration]) -> String {
    let mut states = configurations
        .iter()
        .filter(|configuration| !configuration.feature_states.is_empty())
        .map(|configuration| (&configuration.id, &configuration.feature_states))
        .collect::<Vec<_>>();
    states.sort_by(|left, right| left.0.cmp(right.0));
    hash_debug(&states)
}

/// Which side of the codec drives the history-enrichment prefix.
///
/// The read (decode) path and the write-side reprojections run the same ordered
/// choreography of native-lane enrichments, with one direction-specific step:
/// the read path applies hole-construction enrichment, and the write and
/// configuration-reprojection paths omit it. Any other divergence between the
/// directions lives in the callers, around the shared calls below, not inside
/// this prefix.
#[derive(Clone, Copy)]
pub(crate) enum HistoryEnrichment {
    /// Decode path: includes `enrich_history_hole_constructions`.
    Read,
    /// Write path and configuration reprojection: omits hole constructions.
    Write,
}

/// Semantic-projection mode of `resolved_features::enrich_history_parameters`
/// (the historical `true` argument): projects parameters together with their
/// downstream semantic feature inputs.
pub(crate) fn enrich_history_parameters_semantic(
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) {
    crate::resolved_features::enrich_history_parameters(histories, lanes, true);
}

/// Parameter-only mode of `resolved_features::enrich_history_parameters` (the
/// historical `false` argument): projects parameter values without the semantic
/// feature-input projection.
pub(crate) fn enrich_history_parameters_values_only(
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) {
    crate::resolved_features::enrich_history_parameters(histories, lanes, false);
}

/// The shared native-lane enrichment prefix, declared once for both codec
/// directions. Runs the ordered extrusion-termination, combine, sweep-path,
/// sketch-block, parameter, reference-plane, PMI, evaluated-parameter,
/// reference-axis, and revolution-input enrichments; the read path additionally
/// applies hole-construction enrichment (selected by `mode`).
pub(crate) fn enrich_history_semantic(
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
    mode: HistoryEnrichment,
) {
    crate::resolved_features::enrich_history_extrusion_terminations(histories, lanes);
    crate::resolved_features::enrich_history_combine_selections(histories, lanes);
    crate::resolved_features::enrich_history_sweep_paths(histories, lanes);
    crate::resolved_features::enrich_history_sketch_block_references(histories, lanes);
    enrich_history_parameters_semantic(histories, lanes);
    if matches!(mode, HistoryEnrichment::Read) {
        crate::resolved_features::enrich_history_hole_constructions(histories, lanes);
    }
    crate::resolved_features::enrich_history_reference_planes(histories, lanes);
    crate::pmi::enrich_history_parameters(histories, pmi_dimensions);
    apply_evaluated_parameters(histories);
    crate::resolved_features::enrich_history_reference_axes(histories, lanes);
    crate::resolved_features::enrich_history_revolution_inputs(histories, lanes);
}

/// The shared compact/generated projection block, declared once for both codec
/// directions. Applies the seven ordered projections that read, write, and
/// configuration reprojection all run against a freshly projected feature list.
/// Direction-specific operation and profile bindings (pattern inputs,
/// sweep/revolution/extrusion operations, spatial sketches, tree-node restore)
/// stay in each caller, around this block.
pub(crate) fn project_compact_and_generated(
    features: &mut [cadmpeg_ir::features::Feature],
    projection: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) {
    crate::resolved_features::project_compact_body_selections(features, lanes);
    crate::resolved_features::project_compact_combine_paths(features, projection, lanes);
    crate::resolved_features::project_compact_edge_selections(features, lanes);
    crate::resolved_features::project_compact_surface_selections(features, projection, lanes);
    crate::resolved_features::project_surface_sweep_profiles(features, projection, lanes);
    crate::resolved_features::project_helix_axes(features, projection, lanes);
    crate::resolved_features::project_adjacent_extrusion_profiles(features, projection, lanes);
}

/// Reproject configuration-local evaluated parameters and feature operations from native lanes.
pub(crate) fn project_configuration_design_states(
    ir: &mut cadmpeg_ir::CadIr,
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
) {
    for (configuration_index, lane_index) in
        configuration_lane_assignments(&ir.model.configurations, lanes)
    {
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let mut projection = histories.to_vec();
        enrich_history_parameters_semantic(&mut projection, scoped_lanes);
        crate::pmi::enrich_history_parameters(&mut projection, pmi_dimensions);
        ir.model.configurations[configuration_index].parameter_values =
            project_parameters(&projection)
                .into_iter()
                .filter_map(|parameter| parameter.value.map(|value| (parameter.id, value)))
                .collect();

        let mut projection = histories.to_vec();
        enrich_history_semantic(
            &mut projection,
            scoped_lanes,
            pmi_dimensions,
            HistoryEnrichment::Write,
        );
        let mut features = project_features(&projection);
        crate::resolved_features::bind_pattern_inputs(&mut features, &projection, scoped_lanes);
        project_compact_and_generated(&mut features, &projection, scoped_lanes);
        crate::resolved_features::bind_extrusion_operations(&mut features, histories, scoped_lanes);
        crate::resolved_features::bind_revolution_operations(
            &mut features,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::bind_sweep_operations(&mut features, histories, scoped_lanes);
        crate::resolved_features::bind_sweep_adjacent_profiles(
            &mut features,
            histories,
            scoped_lanes,
        );
        restore_configuration_tree_node_definitions(&mut features, &ir.model.features);
        ir.model.configurations[configuration_index].feature_states = features
            .into_iter()
            .map(|feature| {
                (
                    feature.id,
                    cadmpeg_ir::features::ConfigurationFeatureState {
                        suppressed: feature.suppressed,
                        dependencies: feature.dependencies,
                        outputs: feature.outputs,
                        definition: feature.definition,
                    },
                )
            })
            .collect();
    }
}

fn restore_configuration_tree_node_definitions(
    features: &mut [cadmpeg_ir::features::Feature],
    base_features: &[cadmpeg_ir::features::Feature],
) {
    let base = base_features
        .iter()
        .map(|feature| (&feature.id, &feature.definition))
        .collect::<HashMap<_, _>>();
    for feature in features {
        if !matches!(feature.definition, FeatureDefinition::Native { .. }) {
            continue;
        }
        let Some(FeatureDefinition::TreeNode { role }) = base.get(&feature.id).copied() else {
            continue;
        };
        feature.definition = FeatureDefinition::TreeNode { role: *role };
    }
}

/// Apply sketch ownership projection to configuration-local feature snapshots.
pub(crate) fn project_configuration_sketch_states(
    ir: &mut cadmpeg_ir::CadIr,
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) {
    let modeller_generation = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("parasolid_schema"))
        .and_then(|schema| crate::container::parasolid_modeler_generation(schema));
    for (configuration_index, lane_index) in
        configuration_lane_assignments(&ir.model.configurations, lanes)
    {
        let surfaces = configuration_surface_carriers(ir, configuration_index);
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let states = &ir.model.configurations[configuration_index].feature_states;
        let mut features = ir
            .model
            .features
            .iter()
            .filter_map(|feature| {
                let state = states.get(&feature.id)?;
                let mut feature = feature.clone();
                feature.suppressed = state.suppressed;
                feature.dependencies.clone_from(&state.dependencies);
                feature.outputs.clone_from(&state.outputs);
                feature.definition.clone_from(&state.definition);
                Some(feature)
            })
            .collect::<Vec<_>>();
        let scoped_spatial_sketches = ir
            .model
            .spatial_sketches
            .iter()
            .filter(|sketch| sketch.native_ref.as_deref() == Some(scoped_lanes[0].id.as_str()))
            .map(|sketch| &sketch.id)
            .collect::<HashSet<_>>();
        for feature in &mut features {
            let FeatureDefinition::SpatialSketch { sketch } = &mut feature.definition else {
                continue;
            };
            let expected = cadmpeg_ir::sketches::SpatialSketchId(feature.id.0.replacen(
                ":model:feature#",
                ":model:spatial-sketch#",
                1,
            ));
            if sketch.is_none() && scoped_spatial_sketches.contains(&expected) {
                *sketch = Some(expected);
            }
        }
        let mut parameters = ir.model.parameters.clone();
        for parameter in &mut parameters {
            if let Some(value) = ir.model.configurations[configuration_index]
                .parameter_values
                .get(&parameter.id)
            {
                parameter.value = Some(value.clone());
            }
        }
        crate::resolved_features::bind_sketch_profiles(
            &mut features,
            &mut ir.model.sketches,
            &ir.model.sketch_entities,
            &parameters,
            histories,
            scoped_lanes,
            &ir.annotations,
        );
        crate::resolved_features::project_compact_sketch_profiles(
            &mut features,
            &mut ir.model.sketches,
            &mut ir.model.sketch_entities,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::project_marker_backed_sketches(
            &mut features,
            &mut ir.model.sketches,
            &mut ir.model.sketch_entities,
            histories,
            scoped_lanes,
            modeller_generation,
        );
        bind_unique_sketch_feature(&mut features, &ir.model.sketches, histories);
        crate::resolved_features::project_dissected_sketches(
            &mut features,
            &ir.model.sketches,
            histories,
        );
        crate::resolved_features::bind_profile_revolution_axes(
            &mut features,
            histories,
            scoped_lanes,
            &ir.model.sketches,
            &surfaces,
        );
        crate::resolved_features::bind_pattern_inputs(&mut features, histories, scoped_lanes);
        crate::resolved_features::project_adjacent_extrusion_profiles(
            &mut features,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::bind_sweep_adjacent_profiles(
            &mut features,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::project_hole_position_sketches(
            &mut features,
            &ir.model.sketches,
            &ir.model.sketch_entities,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::project_spatial_hole_position_sketches(
            &mut features,
            &ir.model.spatial_sketches,
            &ir.model.spatial_sketch_entities,
            &surfaces,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::project_hole_axes(
            &mut features,
            &surfaces,
            histories,
            scoped_lanes,
        );
        for feature in features {
            let Some(state) = ir.model.configurations[configuration_index]
                .feature_states
                .get_mut(&feature.id)
            else {
                continue;
            };
            state.suppressed = feature.suppressed;
            state.dependencies = feature.dependencies;
            state.outputs = feature.outputs;
            state.definition = feature.definition;
        }
    }
    let base = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.clone(), feature.definition.clone()))
        .collect::<HashMap<_, _>>();
    for configuration in &mut ir.model.configurations {
        for (feature_id, state) in &mut configuration.feature_states {
            if let Some(base_definition) = base.get(feature_id) {
                inherit_configuration_hole_semantics(&mut state.definition, base_definition);
            }
        }
    }
}

fn inherit_configuration_hole_semantics(
    definition: &mut FeatureDefinition,
    base_definition: &FeatureDefinition,
) {
    let FeatureDefinition::Hole {
        face,
        placements,
        kind,
        diameter,
        extent,
    } = definition
    else {
        return;
    };
    let FeatureDefinition::Hole {
        face: base_face,
        placements: base_placements,
        kind: base_kind,
        diameter: base_diameter,
        extent: base_extent,
    } = base_definition
    else {
        return;
    };
    let missing_construction = diameter.is_none() && extent.is_none();
    if face.is_none() {
        face.clone_from(base_face);
    }
    if placements.is_empty() {
        placements.clone_from(base_placements);
    }
    if missing_construction {
        kind.clone_from(base_kind);
    }
    if diameter.is_none() {
        diameter.clone_from(base_diameter);
    }
    if extent.is_none() {
        extent.clone_from(base_extent);
    }
}

fn configuration_surface_carriers(
    ir: &cadmpeg_ir::CadIr,
    configuration_index: usize,
) -> Vec<cadmpeg_ir::geometry::Surface> {
    let body_ids = ir.model.configurations[configuration_index]
        .bodies
        .iter()
        .collect::<HashSet<_>>();
    let region_ids = ir
        .model
        .bodies
        .iter()
        .filter(|body| body_ids.contains(&body.id))
        .flat_map(|body| &body.regions)
        .collect::<HashSet<_>>();
    let shell_ids = ir
        .model
        .regions
        .iter()
        .filter(|region| region_ids.contains(&region.id))
        .flat_map(|region| &region.shells)
        .collect::<HashSet<_>>();
    let face_ids = ir
        .model
        .shells
        .iter()
        .filter(|shell| shell_ids.contains(&shell.id))
        .flat_map(|shell| &shell.faces)
        .collect::<HashSet<_>>();
    let surface_ids = ir
        .model
        .faces
        .iter()
        .filter(|face| face_ids.contains(&face.id))
        .map(|face| &face.surface)
        .collect::<HashSet<_>>();
    ir.model
        .surfaces
        .iter()
        .filter(|surface| surface_ids.contains(&surface.id))
        .cloned()
        .collect()
}

/// Give configuration-local numeric overrides the dimensional kind established
/// by their neutral parameter definition.
pub(crate) fn align_configuration_parameter_kinds(ir: &mut cadmpeg_ir::CadIr) {
    let parameter_kinds = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| Some((&parameter.id, parameter.value.as_ref()?)))
        .collect::<HashMap<_, _>>();
    for value in ir
        .model
        .configurations
        .iter_mut()
        .flat_map(|configuration| &mut configuration.parameter_values)
    {
        let (parameter, value) = value;
        let Some(canonical) = parameter_kinds.get(parameter) else {
            continue;
        };
        let aligned = match (&**canonical, &*value) {
            (ParameterValue::Length(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer).map(|value| ParameterValue::Length(Length(value)))
            }
            (ParameterValue::Length(_), ParameterValue::Real(real)) if real.is_finite() => {
                Some(ParameterValue::Length(Length(*real)))
            }
            (ParameterValue::Angle(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer).map(|value| ParameterValue::Angle(Angle(value)))
            }
            (ParameterValue::Angle(_), ParameterValue::Real(real)) if real.is_finite() => {
                Some(ParameterValue::Angle(Angle(*real)))
            }
            (ParameterValue::Real(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer).map(ParameterValue::Real)
            }
            _ => None,
        };
        if let Some(aligned) = aligned {
            *value = aligned;
        }
    }
}

pub(crate) fn configuration_lane_assignments(
    configurations: &[DesignConfiguration],
    lanes: &[crate::records::FeatureInputLane],
) -> Vec<(usize, usize)> {
    let mut lanes_by_configuration = BTreeMap::<u32, Vec<usize>>::new();
    for (lane_index, lane) in lanes.iter().enumerate() {
        let Some(source_index) = lane
            .configuration
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        lanes_by_configuration
            .entry(source_index)
            .or_default()
            .push(lane_index);
    }
    lanes_by_configuration
        .into_iter()
        .filter_map(|(source_index, lane_indices)| {
            let [lane_index] = lane_indices.as_slice() else {
                return None;
            };
            let explicit_candidates = configurations
                .iter()
                .enumerate()
                .filter(|(_, configuration)| configuration.source_index == Some(source_index))
                .map(|(position, _)| position)
                .collect::<Vec<_>>();
            let candidates = if explicit_candidates.is_empty() {
                configurations
                    .iter()
                    .enumerate()
                    .filter(|(_, configuration)| {
                        configuration.source_index.is_none()
                            && configuration.ordinal == source_index
                    })
                    .map(|(position, _)| position)
                    .collect::<Vec<_>>()
            } else {
                explicit_candidates
            };
            let [configuration_index] = candidates.as_slice() else {
                return None;
            };
            Some((*configuration_index, *lane_index))
        })
        .collect()
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

/// Collect retained feature names changed by the neutral model.
pub(crate) fn feature_name_changes(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
) -> HashMap<FeatureId, (String, String)> {
    native.map_or_else(HashMap::new, |native| {
        ir.model
            .features
            .iter()
            .filter_map(|feature| {
                let record = native
                    .feature_histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|record| feature.native_ref.as_deref() == Some(record.id.as_str()))?;
                let new_name = feature.name.as_deref().unwrap_or_default();
                (record.name != new_name).then(|| {
                    (
                        feature.id.clone(),
                        (record.name.clone(), new_name.to_string()),
                    )
                })
            })
            .collect()
    })
}

pub(crate) fn native_parameters_match_source(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
) -> bool {
    native
        .map(|native| native_parameter_hash(&native.feature_histories))
        .zip(
            ir.source
                .as_ref()
                .and_then(|source| source.attributes.get("sldprt_native_parameter_sha256")),
        )
        .is_some_and(|(current, baseline)| &current == baseline)
}

pub(crate) fn apply_feature_name_changes(
    parameters: &mut [DesignParameter],
    changes: &HashMap<FeatureId, (String, String)>,
) {
    let owners = parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect::<HashMap<_, _>>();
    for parameter in parameters {
        if let Some((old_owner, new_owner)) = changes.get(&parameter.owner) {
            if let Some(equation_id) = parameter.properties.get_mut("EquationId") {
                if let Some(base) = equation_id.strip_suffix(&format!("@{old_owner}")) {
                    *equation_id = format!("{base}@{new_owner}");
                }
            }
        }
        let dependency_changes = parameter
            .dependencies
            .iter()
            .filter_map(|dependency| owners.get(dependency))
            .filter_map(|owner| changes.get(owner))
            .collect::<Vec<_>>();
        let aliases = expression_identifier_tokens(&parameter.expression)
            .identifiers
            .into_iter()
            .filter_map(|token| {
                dependency_changes
                    .iter()
                    .find_map(|(old_owner, new_owner)| {
                        token
                            .value
                            .strip_suffix(&format!("@{old_owner}"))
                            .map(|base| (token.value.clone(), format!("{base}@{new_owner}")))
                    })
            })
            .collect::<HashMap<_, _>>();
        if let Some(rewritten) = rewrite_parameter_expression(&parameter.expression, &aliases) {
            parameter.expression = rewritten;
        }
    }
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
        validate_compact_body_selection_edits(&ir.model.features, native.as_ref())?;
        validate_compact_edge_selection_edits(&ir.model.features, native.as_ref())?;
        validate_compact_surface_selection_edits(&ir.model.features, native.as_ref())?;
        validate_surface_sweep_profile_edits(&ir.model.features, native.as_ref())?;
        validate_embedded_helix_edits(&ir.model.features, native.as_ref())?;
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
                .map(project_features_with_native_inputs)
                .unwrap_or_default();
            if feature_hash(&projected) == neutral_hash {
                Ok(())
            } else {
                Err(CodecError::Malformed(
                    "conflicting neutral and native SLDPRT feature edits".into(),
                ))
            }
        }
        (true, false) => {
            validate_compact_body_selection_edits(&ir.model.features, native.as_ref())?;
            validate_compact_edge_selection_edits(&ir.model.features, native.as_ref())?;
            validate_compact_surface_selection_edits(&ir.model.features, native.as_ref())?;
            validate_surface_sweep_profile_edits(&ir.model.features, native.as_ref())?;
            validate_embedded_helix_edits(&ir.model.features, native.as_ref())?;
            sync_neutral_features(
                &ir.model.features,
                &ir.model.parameters,
                &ir.model.bodies,
                native,
            )
        }
    }
}

fn validate_embedded_helix_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let embedded = project_features(&native.feature_histories)
        .into_iter()
        .filter_map(|feature| {
            matches!(
                feature.definition,
                FeatureDefinition::HelixNativeAxis { .. }
            )
            .then_some(feature.id)
        })
        .collect::<HashSet<_>>();
    let expected = project_features_with_native_inputs(native)
        .into_iter()
        .filter_map(|feature| {
            (embedded.contains(&feature.id)
                && matches!(feature.definition, FeatureDefinition::Helix { .. }))
            .then_some((feature.id, feature.definition))
        })
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(expected) = expected.get(&feature.id) else {
            continue;
        };
        if &feature.definition != expected {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes embedded helix geometry",
                feature.id
            )));
        }
    }
    Ok(())
}

fn validate_surface_sweep_profile_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let expected = project_features_with_native_inputs(native)
        .into_iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sweep {
                profile: Some(profile @ (ProfileRef::Feature(_) | ProfileRef::Generated { .. })),
                ..
            } = feature.definition
            else {
                return None;
            };
            (matches!(profile, ProfileRef::Generated { .. })
                || !feature.source_properties.contains_key("Profile"))
            .then_some((feature.id, profile))
        })
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(expected) = expected.get(&feature.id) else {
            continue;
        };
        let FeatureDefinition::Sweep {
            profile: Some(profile),
            ..
        } = &feature.definition
        else {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a reference-curve sweep profile",
                feature.id
            )));
        };
        if profile != expected {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a reference-curve sweep profile",
                feature.id
            )));
        }
    }
    Ok(())
}

fn project_features_with_native_inputs(
    native: &crate::native::SldprtNative,
) -> Vec<cadmpeg_ir::features::Feature> {
    let mut histories = native.feature_histories.clone();
    enrich_history_semantic(
        &mut histories,
        &native.feature_input_lanes,
        &native.pmi_dimensions,
        HistoryEnrichment::Write,
    );
    let mut features = project_features(&histories);
    crate::resolved_features::bind_pattern_inputs(
        &mut features,
        &histories,
        &native.feature_input_lanes,
    );
    crate::resolved_features::bind_sweep_operations(
        &mut features,
        &histories,
        &native.feature_input_lanes,
    );
    project_compact_and_generated(&mut features, &histories, &native.feature_input_lanes);
    crate::resolved_features::bind_revolution_operations(
        &mut features,
        &histories,
        &native.feature_input_lanes,
    );
    let _ = crate::resolved_features::spatial_sketches(
        &mut features,
        &histories,
        &native.feature_input_lanes,
    );
    features
}

fn validate_compact_body_selection_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let mut selections = HashMap::<&str, Vec<&crate::records::FeatureInputBodySelection>>::new();
    for selection in native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.body_selections)
    {
        selections
            .entry(selection.feature_ref.as_str())
            .or_default()
            .push(selection);
    }
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some([selection]) = selections.get(native_ref).map(Vec::as_slice) else {
            continue;
        };
        let FeatureDefinition::DeleteBody { bodies, mode } = &feature.definition else {
            continue;
        };
        let expected = BodySelection::Local {
            bodies: selection
                .local_body_ids
                .iter()
                .map(u32::to_string)
                .collect(),
            native: crate::resolved_features::compact_body_selection_value(
                &selection.local_body_ids,
            ),
        };
        if bodies != &expected {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a compact body selection",
                feature.id
            )));
        }
        if selection
            .mode
            .as_ref()
            .is_some_and(|expected| mode != expected)
        {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a compact body retention mode",
                feature.id
            )));
        }
    }
    Ok(())
}

fn validate_compact_edge_selection_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let Some(native) = native else {
        return Ok(());
    };
    let mut selections = HashMap::<&str, Vec<&crate::records::FeatureInputEdgeSelection>>::new();
    for selection in native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.edge_selections)
    {
        selections
            .entry(selection.feature_ref.as_str())
            .or_default()
            .push(selection);
    }
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(edge_selections) = selections
            .get(native_ref)
            .filter(|selections| !selections.is_empty())
        else {
            continue;
        };
        let (FeatureDefinition::Fillet { edges, .. } | FeatureDefinition::Chamfer { edges, .. }) =
            &feature.definition
        else {
            continue;
        };
        let native = crate::resolved_features::compact_edge_selection_set_value(edge_selections);
        let generated = edge_selections
            .iter()
            .map(|selection| {
                let native_feature = selection.terminal_feature_ref.as_deref()?;
                let feature = feature_ids_by_native.get(native_feature)?.clone();
                let local_id = selection
                    .local_edge_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                Some(cadmpeg_ir::features::GeneratedEdgeRef { feature, local_id })
            })
            .collect::<Option<Vec<_>>>();
        let expected = match generated.filter(|edges| !edges.is_empty()) {
            Some(edges) => EdgeSelection::Generated { edges, native },
            None => EdgeSelection::Native(native),
        };
        if edges != &expected {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a compact edge selection",
                feature.id
            )));
        }
    }
    Ok(())
}

fn validate_compact_surface_selection_edits(
    features: &[cadmpeg_ir::features::Feature],
    native: Option<&crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    enum SelectionSlot<'a> {
        Face(&'a FaceSelection),
        Vertex(&'a VertexSelection),
    }
    let Some(native) = native else { return Ok(()) };
    let mut selections = HashMap::<&str, Vec<&crate::records::FeatureInputSurfaceSelection>>::new();
    for selection in native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.surface_selections)
    {
        selections
            .entry(selection.feature_ref.as_str())
            .or_default()
            .push(selection);
    }
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some([selection]) = selections.get(native_ref).map(Vec::as_slice) else {
            continue;
        };
        let first_component =
            matches!(feature.definition, FeatureDefinition::CosmeticThread { .. });
        let slot = match &feature.definition {
            FeatureDefinition::Thicken { faces, .. } => SelectionSlot::Face(faces),
            FeatureDefinition::CosmeticThread { face, .. } => SelectionSlot::Face(face),
            FeatureDefinition::Extrude {
                extent:
                    cadmpeg_ir::features::Extent::ToFace { face }
                    | cadmpeg_ir::features::Extent::OffsetFromFace { face, .. },
                ..
            } => SelectionSlot::Face(face),
            FeatureDefinition::Extrude {
                extent: cadmpeg_ir::features::Extent::ToVertex { vertex },
                ..
            } => SelectionSlot::Vertex(vertex),
            _ => continue,
        };
        let native =
            crate::resolved_features::compact_surface_selection_value(&selection.components);
        let producer = if first_component {
            selection.producer_feature_refs.first().map(String::as_str)
        } else {
            selection.terminal_feature_ref.as_deref()
        };
        let component = if first_component {
            selection.components.first()
        } else {
            selection.components.last()
        };
        let generated = producer
            .and_then(|producer| feature_ids_by_native.get(producer))
            .zip(component)
            .and_then(|(feature, component)| Some((feature, component.local_id?)));
        let changed = match slot {
            SelectionSlot::Face(faces) => {
                let expected = match generated {
                    Some((feature, local_id)) => FaceSelection::Generated {
                        faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
                            feature: feature.clone(),
                            local_id: local_id.to_string(),
                        }],
                        native,
                    },
                    None => FaceSelection::Native(native),
                };
                faces != &expected
            }
            // Edge-endpoint references keep the endpoint selector native.
            SelectionSlot::Vertex(VertexSelection::Native(value))
                if value.starts_with("sldprt:feature-input:edge-endpoint-ref:") =>
            {
                false
            }
            SelectionSlot::Vertex(vertex) => {
                let expected = match generated {
                    Some((feature, local_id)) => VertexSelection::Generated {
                        vertex: cadmpeg_ir::features::GeneratedVertexRef {
                            feature: feature.clone(),
                            local_id: local_id.to_string(),
                        },
                        native,
                    },
                    None => VertexSelection::Native(native),
                };
                vertex != &expected
            }
        };
        if changed {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature {} changes a compact surface selection",
                feature.id
            )));
        }
    }
    Ok(())
}

/// Resolve neutral/native configuration edit authority before writing.
pub fn prepare_configurations_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let feature_state_hash = configuration_feature_state_hash(&ir.model.configurations);
    let baseline_feature_states = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_configuration_feature_states_sha256")
    });
    let feature_states_changed = baseline_feature_states
        .is_some_and(|baseline| baseline != &feature_state_hash)
        || baseline_feature_states.is_none()
            && ir
                .model
                .configurations
                .iter()
                .any(|configuration| !configuration.feature_states.is_empty());
    let parameter_value_hash = configuration_parameter_value_hash(&ir.model.configurations);
    let baseline_parameter_values = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_configuration_parameter_values_sha256")
    });
    let parameter_values_changed = baseline_parameter_values
        .is_some_and(|baseline| baseline != &parameter_value_hash)
        || baseline_parameter_values.is_none()
            && ir
                .model
                .configurations
                .iter()
                .any(|configuration| !configuration.parameter_values.is_empty());
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
    } else {
        match (neutral_changed, native_changed) {
            (false, _) => {}
            (true, true) => {
                let projected = native
                    .as_ref()
                    .map(|value| project_configurations(&value.feature_histories))
                    .unwrap_or_default();
                if configuration_hash(&projected) != neutral_hash {
                    return Err(CodecError::Malformed(
                        "conflicting neutral and native SLDPRT configuration edits".into(),
                    ));
                }
            }
            (true, false) => {
                sync_neutral_configurations(&ir.model.configurations, native);
            }
        }
    }
    if feature_states_changed || parameter_values_changed {
        sync_configuration_design_state(ir, native)?;
    }
    Ok(())
}

fn sync_configuration_design_state(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
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
    let global_owners = global_parameter_owners(&ir.model.features);
    if parameters_with_incoherent_evaluated_values(
        &ir.model.parameters,
        &feature_names,
        &global_owners,
        &ir.model.configurations,
    ) > 0
    {
        return Err(CodecError::Malformed(
            "SLDPRT configuration parameter values are inconsistent with their expressions".into(),
        ));
    }
    let Some(native) = native.as_mut() else {
        return Err(CodecError::NotImplemented(
            "SLDPRT configuration design state requires retained feature-input lanes".into(),
        ));
    };
    let mut current_projection = ir.clone();
    project_configuration_design_states(
        &mut current_projection,
        &native.feature_histories,
        &native.feature_input_lanes,
        &native.pmi_dimensions,
    );
    align_configuration_parameter_kinds(&mut current_projection);
    project_configuration_sketch_states(
        &mut current_projection,
        &native.feature_histories,
        &native.feature_input_lanes,
    );
    let current_parameter_hash =
        configuration_parameter_value_hash(&current_projection.model.configurations);
    let current_feature_hash =
        configuration_feature_state_hash(&current_projection.model.configurations);
    let current_matches = current_parameter_hash
        == configuration_parameter_value_hash(&ir.model.configurations)
        && current_feature_hash == configuration_feature_state_hash(&ir.model.configurations);
    if current_matches {
        return Ok(());
    }
    let native_design_state_changed = ir.source.as_ref().is_some_and(|source| {
        source
            .attributes
            .get("sldprt_configuration_parameter_values_sha256")
            .is_some_and(|baseline| baseline != &current_parameter_hash)
            || source
                .attributes
                .get("sldprt_configuration_feature_states_sha256")
                .is_some_and(|baseline| baseline != &current_feature_hash)
    });
    if native_design_state_changed {
        return Err(CodecError::Malformed(
            "conflicting neutral and native SLDPRT configuration design-state edits".into(),
        ));
    }
    patch_configuration_parameter_scalars(ir, native)?;

    let mut projected = ir.clone();
    project_configuration_design_states(
        &mut projected,
        &native.feature_histories,
        &native.feature_input_lanes,
        &native.pmi_dimensions,
    );
    align_configuration_parameter_kinds(&mut projected);
    project_configuration_sketch_states(
        &mut projected,
        &native.feature_histories,
        &native.feature_input_lanes,
    );
    if configuration_parameter_value_hash(&projected.model.configurations)
        != configuration_parameter_value_hash(&ir.model.configurations)
        || configuration_feature_state_hash(&projected.model.configurations)
            != configuration_feature_state_hash(&ir.model.configurations)
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT configuration design-state edit has no complete native lane encoding".into(),
        ));
    }
    Ok(())
}

fn patch_configuration_parameter_scalars(
    ir: &cadmpeg_ir::CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), CodecError> {
    let parameters = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    for (configuration_index, lane_index) in
        configuration_lane_assignments(&ir.model.configurations, &native.feature_input_lanes)
    {
        let configuration = &ir.model.configurations[configuration_index];
        let lane = &mut native.feature_input_lanes[lane_index];
        let names = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name.value.as_str()))
            .collect::<HashMap<_, _>>();
        let mut starts = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|record| {
                crate::resolved_features::feature_object_name(record, lane)
                    .map(|name| (name.offset, record))
            })
            .collect::<Vec<_>>();
        starts.sort_by_key(|(offset, _)| *offset);
        for (parameter_id, value) in &configuration.parameter_values {
            let Some(parameter) = parameters.get(parameter_id) else {
                continue;
            };
            let Some(feature) = features.get(&parameter.owner) else {
                continue;
            };
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let Some((position, (start, _))) = starts
                .iter()
                .enumerate()
                .find(|(_, (_, record))| record.id == native_ref)
            else {
                continue;
            };
            let end = starts
                .get(position + 1)
                .map_or(u64::MAX, |(offset, _)| *offset);
            let candidates = lane
                .scalars
                .iter()
                .enumerate()
                .filter(|(_, scalar)| scalar.offset > *start && scalar.offset < end)
                .filter(|(_, scalar)| {
                    names.get(scalar.name.as_str()) == Some(&parameter.name.as_str())
                })
                .collect::<Vec<_>>();
            let driving = candidates
                .iter()
                .filter(|(_, scalar)| {
                    scalar.role == crate::records::FeatureInputScalarRole::Driving
                })
                .map(|(index, _)| *index)
                .collect::<Vec<_>>();
            let candidates = if driving.is_empty() {
                candidates
                    .into_iter()
                    .filter(|(_, scalar)| {
                        scalar.role == crate::records::FeatureInputScalarRole::Native
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>()
            } else {
                driving
            };
            let [scalar_index] = candidates.as_slice() else {
                continue;
            };
            let encoded = match value {
                ParameterValue::Length(value) => value.0 / 1000.0,
                ParameterValue::Angle(value) => value.0,
                ParameterValue::Real(value) => *value,
                ParameterValue::Integer(value) => exact_integer_f64(*value).ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "SLDPRT configuration parameter {} cannot be represented by a native scalar",
                        parameter.id.0
                    ))
                })?,
                ParameterValue::Boolean(value) => f64::from(*value),
            };
            let scalar = &mut lane.scalars[*scalar_index];
            let offset = usize::try_from(scalar.offset).map_err(|_| {
                CodecError::Malformed("SLDPRT scalar offset exceeds address space".into())
            })?;
            lane.native_payload
                .get_mut(offset..offset + 8)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT scalar {} lies outside its payload",
                        scalar.id
                    ))
                })?
                .copy_from_slice(&encoded.to_le_bytes());
            scalar.value = encoded;
        }
    }
    Ok(())
}

/// Resolve neutral/native parameter edit authority before writing.
pub fn prepare_parameters_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
    feature_parameter_changes_authorized: bool,
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
        (false, true) if feature_parameter_changes_authorized => Ok(()),
        (false, _) => Ok(()),
        (true, true) => {
            if feature_parameter_changes_authorized {
                return sync_neutral_parameters(ir, native);
            }
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
    let global_owners = global_parameter_owners(&ir.model.features);
    if let Some(native) = native.as_ref() {
        let original = project_parameters(&native.feature_histories);
        let original_feature_names = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .map(|feature| (neutral_feature_id(&feature.id), feature.name.clone()))
            .collect::<HashMap<_, _>>();
        rewrite_renamed_parameter_references(
            &mut parameters,
            &original,
            &original_feature_names,
            &feature_names,
        );
    }
    if parameters_with_incoherent_dependencies(&parameters, &feature_names, &global_owners) > 0 {
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
                    || (feature.native_ref.is_none()
                        && record.id == generated_feature_record_id(feature_id))
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
    original_feature_names: &HashMap<FeatureId, String>,
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
        let previous_owner_name = original_feature_names.get(&parameter.owner);
        let owner_name = feature_names.get(&parameter.owner);
        if previous.name != parameter.name {
            aliases.insert(previous.name.clone(), parameter.name.clone());
        }
        if previous.name != parameter.name || previous_owner_name != owner_name {
            if let (Some(previous_owner_name), Some(owner_name)) = (previous_owner_name, owner_name)
            {
                aliases.insert(
                    format!("{}@{previous_owner_name}", previous.name),
                    format!("{}@{owner_name}", parameter.name),
                );
            }
        }
        if let Some(previous_id) = previous.properties.get("EquationId") {
            let replacement = parameter
                .properties
                .get("EquationId")
                .unwrap_or(&parameter.name);
            if previous_id != replacement || previous_owner_name != owner_name {
                aliases.insert(previous_id.clone(), replacement.clone());
                if let (Some(previous_owner_name), Some(owner_name)) =
                    (previous_owner_name, owner_name)
                {
                    if !previous_id.contains('@') {
                        let qualified_replacement = if replacement.contains('@') {
                            replacement.clone()
                        } else {
                            format!("{replacement}@{owner_name}")
                        };
                        aliases.insert(
                            format!("{previous_id}@{previous_owner_name}"),
                            qualified_replacement,
                        );
                    }
                }
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
            .map(|(alias, replacement)| (alias.clone(), replacement.clone()))
            .collect::<HashMap<_, _>>();
        if aliases.is_empty() {
            continue;
        }
        if let Some(rewritten) = rewrite_parameter_expression(&parameter.expression, &aliases) {
            parameter.expression = rewritten;
        }
    }
}

fn rewrite_parameter_expression(
    expression: &str,
    aliases: &HashMap<String, String>,
) -> Option<String> {
    let tokens = expression_identifier_tokens(expression).identifiers;
    let mut rewritten = String::with_capacity(expression.len());
    let mut copied = 0;
    for token in tokens {
        if expression_identifier_is_syntax(expression, &token) {
            continue;
        }
        let Some(replacement) = aliases.get(&token.value) else {
            continue;
        };
        rewritten.push_str(&expression[copied..token.start]);
        if token.quoted || !unquoted_expression_identifier(replacement) {
            rewritten.push('"');
            rewritten.push_str(&replacement.replace('"', "\"\""));
            rewritten.push('"');
        } else {
            rewritten.push_str(replacement);
        }
        copied = token.end;
    }
    if copied == 0 {
        return None;
    }
    rewritten.push_str(&expression[copied..]);
    Some(rewritten)
}

fn unquoted_expression_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(|character| {
        !character.is_ascii_digit()
            && character != '.'
            && (character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$'))
    }) && characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.')
    })
}

fn sync_neutral_configurations(
    configurations: &[DesignConfiguration],
    native: &mut Option<crate::native::SldprtNative>,
) {
    if configurations.is_empty() && native.is_none() {
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
    let previous_index_owners = native_configuration_index_owners(&native.feature_histories);
    let deleted_ids = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.configurations)
        .filter(|configuration| !desired_ids.contains(&configuration.id))
        .map(|configuration| configuration.id.clone())
        .collect::<HashSet<_>>();
    native.feature_input_lanes.retain(|lane| {
        let Some(index) = lane
            .configuration
            .as_deref()
            .and_then(|configuration| configuration.parse::<u32>().ok())
        else {
            return true;
        };
        previous_index_owners
            .get(&index)
            .and_then(Option::as_deref)
            .is_none_or(|owner| !deleted_ids.contains(owner))
    });
    for history in &mut native.feature_histories {
        history
            .configurations
            .retain(|configuration| desired_ids.contains(&configuration.id));
    }
    let mut lane_configuration_remaps = HashMap::<String, String>::new();
    for configuration in configurations {
        let existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.configurations)
            .find(|candidate| configuration.native_ref.as_deref() == Some(candidate.id.as_str()));
        if let Some(existing) = existing {
            let existing_id = existing.id.clone();
            let previous_index = existing.source_index.unwrap_or(existing.ordinal);
            existing.ordinal = configuration.ordinal;
            existing.source_index = configuration.source_index;
            existing.name.clone_from(&configuration.name);
            existing.material.clone_from(&configuration.material);
            existing.properties.clone_from(&configuration.properties);
            let configuration_index = configuration.source_index.unwrap_or(configuration.ordinal);
            if previous_index != configuration_index
                && previous_index_owners
                    .get(&previous_index)
                    .and_then(Clone::clone)
                    == Some(existing_id)
            {
                lane_configuration_remaps
                    .insert(previous_index.to_string(), configuration_index.to_string());
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
    for lane in &mut native.feature_input_lanes {
        let Some(configuration) = lane.configuration.as_ref() else {
            continue;
        };
        if let Some(remapped) = lane_configuration_remaps.get(configuration) {
            lane.configuration = Some(remapped.clone());
        }
    }
    for history in &mut native.feature_histories {
        history
            .configurations
            .sort_by_key(|configuration| configuration.ordinal);
    }
    synchronize_history_content_order(native);
}

fn native_configuration_index_owners(
    histories: &[FeatureHistory],
) -> BTreeMap<u32, Option<String>> {
    let configurations = histories
        .iter()
        .flat_map(|history| &history.configurations)
        .collect::<Vec<_>>();
    let mut owners = BTreeMap::<u32, Option<String>>::new();
    for configuration in configurations
        .iter()
        .filter(|configuration| configuration.source_index.is_some())
    {
        let index = configuration.source_index.expect("filtered above");
        owners
            .entry(index)
            .and_modify(|owner| *owner = None)
            .or_insert_with(|| Some(configuration.id.clone()));
    }
    let explicit_indices = owners.keys().copied().collect::<HashSet<_>>();
    for configuration in configurations
        .into_iter()
        .filter(|configuration| configuration.source_index.is_none())
    {
        if explicit_indices.contains(&configuration.ordinal) {
            continue;
        }
        owners
            .entry(configuration.ordinal)
            .and_modify(|owner| *owner = None)
            .or_insert_with(|| Some(configuration.id.clone()));
    }
    owners
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

fn generated_feature_record_id(feature: &FeatureId) -> String {
    format!("sldprt:generated:feature#{}", feature.0)
}

fn generated_feature_source_ids(
    features: &[cadmpeg_ir::features::Feature],
    native: &crate::native::SldprtNative,
) -> Result<HashMap<FeatureId, String>, CodecError> {
    let mut used = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| feature.source_id.as_deref()?.parse::<u32>().ok())
        .collect::<HashSet<_>>();
    let existing = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| {
            Some((
                feature.id.as_str(),
                feature.source_id.as_deref()?.parse::<u32>().ok()?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut next = 1u32;
    let mut allocated = HashMap::new();
    for feature in features
        .iter()
        .filter(|feature| feature.native_ref.is_none())
    {
        let record_id = generated_feature_record_id(&feature.id);
        let source_id = if let Some(source_id) = existing.get(record_id.as_str()).copied() {
            source_id
        } else {
            while used.contains(&next) {
                next = next.checked_add(1).ok_or_else(|| {
                    CodecError::Malformed("SLDPRT feature source-id space is exhausted".into())
                })?;
            }
            let source_id = next;
            used.insert(source_id);
            next = next.checked_add(1).unwrap_or(next);
            source_id
        };
        allocated.insert(feature.id.clone(), source_id.to_string());
    }
    Ok(allocated)
}

fn restore_equivalent_parameter_expressions(
    feature: &Feature,
    original_parameters: &HashMap<String, BTreeMap<String, String>>,
    evaluated_parameters: &HashMap<String, BTreeMap<String, String>>,
    desired_parameters: &mut BTreeMap<String, String>,
) {
    let Some(original) = original_parameters.get(&feature.id) else {
        return;
    };
    let Some(evaluated) = evaluated_parameters.get(&feature.id) else {
        return;
    };
    for (name, desired) in desired_parameters {
        let Some(expression) = original.get(name) else {
            continue;
        };
        if parse_native_parameter_literal(feature, name, expression).is_some() {
            continue;
        }
        let Some(evaluated) = evaluated.get(name) else {
            continue;
        };
        let Some(desired_value) = parse_native_parameter_literal(feature, name, desired) else {
            continue;
        };
        let Some(evaluated_value) = parse_native_parameter_literal(feature, name, evaluated) else {
            continue;
        };
        if desired_value == evaluated_value {
            desired.clone_from(expression);
        }
    }
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
                history.features.retain(is_custom_property);
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
    let original_parameters = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.clone(), feature.parameters.clone()))
        .collect::<HashMap<_, _>>();
    let mut resolved_histories = native.feature_histories.clone();
    enrich_history_parameters_semantic(&mut resolved_histories, &native.feature_input_lanes);
    let resolved_parameter_names = resolved_histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| {
            (
                feature.id.clone(),
                feature.parameters.keys().cloned().collect::<HashSet<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    apply_evaluated_parameters(&mut resolved_histories);
    let evaluated_parameters = resolved_histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.clone(), feature.parameters.clone()))
        .collect::<HashMap<_, _>>();
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

    let generated_sources = generated_feature_source_ids(features, native)?;
    let parent_sources = features
        .iter()
        .map(|feature| {
            let source_id = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()))
                .and_then(|candidate| candidate.source_id.clone())
                .or_else(|| generated_sources.get(&feature.id).cloned())
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
                .or_else(|| generated_sources.get(&feature.id).cloned());
            (feature.id.clone(), source_id)
        })
        .collect::<HashMap<_, _>>();
    let record_ids = features
        .iter()
        .map(|feature| {
            let record_id = feature
                .native_ref
                .clone()
                .unwrap_or_else(|| generated_feature_record_id(&feature.id));
            (feature.id.clone(), record_id)
        })
        .collect::<HashMap<_, _>>();
    let desired_record_ids = record_ids
        .values()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    for history in &mut native.feature_histories {
        history.features.retain(|feature| {
            is_custom_property(feature) || desired_record_ids.contains(&feature.id)
        });
    }
    let principal_planes_by_record = native
        .feature_histories
        .iter()
        .flat_map(|history| {
            let by_source = history
                .features
                .iter()
                .filter_map(|feature| Some((feature.source_id.as_deref()?, feature)))
                .collect::<HashMap<_, _>>();
            history.features.iter().filter_map(move |feature| {
                Some((
                    feature.id.clone(),
                    principal_plane_in_history(feature, &by_source, &history.features)?,
                ))
            })
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
    let retained_tree_node_roles = native
        .feature_histories
        .iter()
        .flat_map(|history| {
            history.features.iter().filter_map(|feature| {
                Some((
                    feature.id.clone(),
                    feature_tree_node_role(feature, &history.features)?,
                ))
            })
        })
        .collect::<HashMap<_, _>>();
    let feature_sources = features
        .iter()
        .filter_map(|feature| {
            Some((
                &feature.id,
                record_sources.get(feature.native_ref.as_ref()?)?.as_str(),
            ))
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
        let (kind, mut parameters, mut properties) = match &feature.definition {
            FeatureDefinition::TreeNode { role } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| retained_tree_node_roles.get(&record.id) != Some(role))
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
            FeatureDefinition::CosmeticThread {
                face,
                diameter,
                extent,
            } => {
                let Some(record) = existing
                    .as_deref()
                    .filter(|record| classify(record) == Some(FeatureClass::CosmeticThread))
                else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} adds a cosmetic thread",
                        feature.id
                    )));
                };
                let mut parameters = record.parameters.clone();
                if let Some(diameter) = diameter {
                    let prefix = record
                        .parameters
                        .get("D2")
                        .filter(|value| value.trim().starts_with("&lt;MOD-DIAM&gt;"))
                        .map_or("<MOD-DIAM>", |_| "&lt;MOD-DIAM&gt;");
                    parameters.insert(
                        "D2".into(),
                        format!("{prefix}{}", format_f64_literal(diameter.0)),
                    );
                }
                match extent {
                    Some(CosmeticThreadExtent::Blind { length }) => {
                        parameters.insert(
                            "D1".into(),
                            format_length_like(
                                length.0,
                                record.parameters.get("D1").map(String::as_str),
                            ),
                        );
                    }
                    Some(CosmeticThreadExtent::Through) => {
                        parameters.remove("D1");
                    }
                    None => {}
                }
                let mut properties = feature.source_properties.clone();
                if let Some(value) = face_selection_value(face) {
                    properties.insert("Face".into(), value);
                } else if !matches!(face, FaceSelection::Unresolved) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes cosmetic-thread face selection",
                        feature.id
                    )));
                }
                (record.kind.clone(), parameters, properties)
            }
            FeatureDefinition::SketchBlockDefinition { sketch } => {
                if sketch.is_some() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes sketch-block geometry",
                        feature.id
                    )));
                }
                let record = existing.as_deref().filter(|record| {
                    feature_input_class(record, NativeClassKind::SketchBlockDefinition)
                });
                let Some(record) = record else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained sketch-block definition",
                        feature.id
                    )));
                };
                (
                    record.kind.clone(),
                    record.parameters.clone(),
                    record.properties.clone(),
                )
            }
            FeatureDefinition::SketchBlockInstance { block, placement } => {
                let retained_source = existing
                    .as_deref()
                    .and_then(|record| record.properties.get("BlockDefinition"))
                    .map(String::as_str);
                let block_source = block
                    .as_ref()
                    .and_then(|block| feature_sources.get(block).copied());
                let retained_placement = existing.as_deref().and_then(sketch_block_placement);
                if retained_source != block_source
                    || retained_placement.as_ref() != placement.as_ref()
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes sketch-block instance semantics",
                        feature.id
                    )));
                }
                let record = existing.as_deref().filter(|record| {
                    feature_input_class(record, NativeClassKind::SketchBlockInstance)
                });
                let Some(record) = record else {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained sketch-block instance",
                        feature.id
                    )));
                };
                (
                    record.kind.clone(),
                    record.parameters.clone(),
                    record.properties.clone(),
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
            FeatureDefinition::DatumPrincipalPlane { plane } => {
                let record = existing.as_deref().ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires a retained principal-plane record",
                        feature.id
                    ))
                })?;
                if principal_planes_by_record.get(&record.id) != Some(plane) {
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
                require_same_family(existing.as_deref(), &feature.id, &["ReferencePlane"])?;
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
                let faces = face_selection_value(faces).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no trim-surface input faces",
                        feature.id
                    ))
                })?;
                let tool =
                    path_source(tool, &record_sources, &sketch_sources).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing trim path",
                            feature.id
                        ))
                    })?;
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["TrimSurface", "SurfaceTrim"],
                )?;
                let mut properties = feature.source_properties.clone();
                properties.insert("Faces".into(), faces);
                properties.insert("Tool".into(), tool);
                properties.insert(
                    "Keep".into(),
                    crate::feature_schema::trim_region_token(*keep).into(),
                );
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
                let faces = face_selection_value(faces).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no extend-surface input faces",
                        feature.id
                    ))
                })?;
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["ExtendSurface", "SurfaceExtend"],
                )?;
                if !distance.0.is_finite() || distance.0 <= 0.0 {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has an invalid surface extension",
                        feature.id
                    )));
                }
                let mut parameters = existing
                    .as_deref()
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default();
                parameters.insert("Distance".into(), format_length_mm(distance.0));
                let mut properties = feature.source_properties.clone();
                properties.insert("Faces".into(), faces);
                properties.insert(
                    "Method".into(),
                    crate::feature_schema::surface_extension_token(*method).into(),
                );
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
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["RuledSurface", "SurfaceRuled"],
                )?;
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
                require_same_family(existing.as_deref(), &feature.id, &["ReferenceAxis"])?;
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
                require_same_family(existing.as_deref(), &feature.id, &["ReferencePoint"])?;
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
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["CoordinateSystem", "ReferenceCoordinateSystem"],
                )?;
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
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["EquationDrivenCurve", "EquationCurve"],
                )?;
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
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["ProjectedCurve", "ProjectionCurve"],
                )?;
                let mut properties = feature.source_properties.clone();
                properties.insert("Source".into(), source);
                properties.insert("TargetFaces".into(), target_faces);
                properties.insert("Bidirectional".into(), bidirectional.to_string());
                match direction {
                    Some(direction) => {
                        require_direction(*direction, &feature.id, "projection direction")?;
                        properties.insert("Direction".into(), format_vector3(*direction));
                    }
                    None => {
                        properties.remove("Direction");
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
                require_same_family(existing.as_deref(), &feature.id, &["CompositeCurve"])?;
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
                start_angle,
                clockwise,
            } => {
                if ![axis_origin.x, axis_origin.y, axis_origin.z, pitch.0]
                    .into_iter()
                    .all(f64::is_finite)
                    || !valid_direction(*axis_direction)
                    || !radius.0.is_finite()
                    || radius.0 <= 0.0
                    || !revolutions.is_finite()
                    || *revolutions <= 0.0
                    || !start_angle.0.is_finite()
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
                parameters.insert("StartAngle".into(), format_angle_rad(start_angle.0));
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
                axial_rise,
                pitch,
                revolutions,
                start_angle,
                clockwise,
            } => {
                if axis_native_ref.is_empty()
                    || !axial_rise.0.is_finite()
                    || !pitch.0.is_finite()
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
                    format_length_like(
                        axial_rise.0,
                        record.parameters.get("D3").map(String::as_str),
                    ),
                );
                parameters.insert(
                    "D4".into(),
                    format_length_like(pitch.0, record.parameters.get("D4").map(String::as_str)),
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
                require_same_family(existing.as_deref(), &feature.id, &["Wrap"])?;
                let profile =
                    profile_source(profile, &record_sources, &feature_sources, &sketch_sources)
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
                require_same_family(existing.as_deref(), &feature.id, &["Sketch"])?;
                (
                    existing.as_deref().map_or_else(
                        || match space {
                            SketchSpace::Planar => "Sketch".into(),
                            SketchSpace::Spatial => "3DSketch".into(),
                        },
                        |record| {
                            let native_space = if record.kind.eq_ignore_ascii_case("3DSketch") {
                                SketchSpace::Spatial
                            } else {
                                SketchSpace::Planar
                            };
                            if native_space == *space {
                                record.kind.clone()
                            } else {
                                match space {
                                    SketchSpace::Planar => "Sketch".into(),
                                    SketchSpace::Spatial => "3DSketch".into(),
                                }
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
            FeatureDefinition::SpatialSketch { .. } => {
                require_same_family(existing.as_deref(), &feature.id, &["Sketch"])?;
                (
                    "3DSketch".into(),
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
                if let ProfileRef::Unresolved(owner) = profile {
                    let retained = existing.as_deref().is_some_and(|record| {
                        record.id == *owner && !record.properties.contains_key("Profile")
                    });
                    if !retained {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} requires retained extrusion profile data",
                            feature.id
                        )));
                    }
                }
                let implicit_profile = existing.as_deref().is_some_and(|record| {
                    !record.properties.contains_key("Profile")
                        && (matches!(profile, ProfileRef::Unresolved(owner) if owner == &record.id)
                            || matches!(profile, ProfileRef::Native(native) if native == &record.id))
                });
                let profile_source = if implicit_profile {
                    None
                } else {
                    Some(
                        profile_source(profile, &record_sources, &feature_sources, &sketch_sources)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "SLDPRT feature {} references a missing extrusion profile",
                                    feature.id
                                ))
                            })?,
                    )
                };
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
                let positional_depth = (parameters.contains_key("D1")
                    || existing.as_deref().is_some_and(|record| {
                        resolved_parameter_names
                            .get(&record.id)
                            .is_some_and(|names| names.contains("D1"))
                    }))
                    && !parameters.contains_key("Depth");
                let mut properties = feature.source_properties.clone();
                if matches!(extent, Extent::Unresolved) && existing.is_none() {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} requires retained extrusion extent data",
                        feature.id
                    )));
                }
                if !matches!(extent, Extent::Unresolved) {
                    parameters.remove("Depth");
                    parameters.remove("Depth2");
                    parameters.remove("Draft");
                    properties.remove("Direction");
                    properties.remove("Face");
                    properties.remove("Vertex");
                }
                match extent {
                    Extent::Unresolved => {}
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
                    Extent::ThroughAllBoth => {
                        properties.insert("EndCondition".into(), "ThroughAllBoth".into());
                    }
                    Extent::ThroughNext => {
                        properties.insert("EndCondition".into(), "ThroughNext".into());
                    }
                    Extent::ToFace { face } if face_selection_value(face).is_some() => {
                        let selection = face_selection_value(face).expect("guarded above");
                        properties.insert("EndCondition".into(), "ToFace".into());
                        properties.insert("Face".into(), selection);
                    }
                    Extent::ToVertex { vertex } if vertex_selection_value(vertex).is_some() => {
                        let selection = vertex_selection_value(vertex).expect("guarded above");
                        properties.insert("EndCondition".into(), "ToVertex".into());
                        properties.insert("Vertex".into(), selection);
                    }
                    Extent::OffsetFromFace { face, offset }
                        if face_selection_value(face).is_some() =>
                    {
                        let selection = face_selection_value(face).expect("guarded above");
                        properties.insert("EndCondition".into(), "OffsetFromFace".into());
                        properties.insert("Face".into(), selection);
                        parameters.insert("Depth".into(), format_length_mm(offset.0));
                    }
                    Extent::ToFace { .. }
                    | Extent::ToVertex { .. }
                    | Extent::OffsetFromFace { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} uses an unsupported extrusion termination selection",
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
                if *op != BooleanOp::Unresolved
                    && (properties.contains_key("Operation")
                        || existing.as_deref().and_then(extrude_feature_op).is_none())
                {
                    properties.insert(
                        "Operation".into(),
                        resolved_boolean_op(*op, &feature.id)?.into(),
                    );
                }
                if !implicit_profile {
                    properties.insert(
                        "Profile".into(),
                        profile_source.expect("non-implicit profile was resolved"),
                    );
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
                let selection = face_selection_value(faces).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "SLDPRT feature {} has no offset-surface support faces",
                        feature.id
                    ))
                })?;
                require_same_family(existing.as_deref(), &feature.id, &["OffsetSurface"])?;
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
                require_same_family(existing.as_deref(), &feature.id, &["KnitSurface", "Knit"])?;
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
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["FilledSurface", "FillSurface"],
                )?;
                let mut properties = feature.source_properties.clone();
                properties.insert("Boundary".into(), boundary);
                properties.insert("SupportFaces".into(), support_faces);
                properties.insert(
                    "Continuity".into(),
                    crate::feature_schema::surface_continuity_token(*continuity).into(),
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
                if existing.as_deref().is_some_and(|record| {
                    !feature_family(record, "Combine")
                        && !feature_input_class(record, NativeClassKind::Combine)
                }) || *op == BooleanOp::NewBody
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes unsupported combine semantics",
                        feature.id
                    )));
                }
                if existing.is_none()
                    && (body_selection_value(target).is_none()
                        || body_selection_value(tools).is_none()
                        || *op == BooleanOp::Unresolved)
                {
                    return Err(CodecError::Malformed(format!(
                        "SLDPRT feature {} has unresolved combine semantics",
                        feature.id
                    )));
                }
                let mut properties = feature.source_properties.clone();
                if let Some(target) = body_selection_value(target) {
                    properties.insert("Target".into(), target);
                }
                if let Some(tools) = body_selection_value(tools) {
                    properties.insert("Tools".into(), tools);
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
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["CutWithSurface", "SurfaceCut"],
                )?;
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
                    if !crate::resolved_features::is_compact_body_selection_value(&selection) {
                        properties.insert("Bodies".into(), selection);
                    }
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
                require_same_family(
                    existing.as_deref(),
                    &feature.id,
                    &["MoveBody", "MoveCopyBody"],
                )?;
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
                face,
                placements,
                kind,
                diameter,
                extent,
            } => {
                if existing
                    .as_deref()
                    .is_some_and(|record| classify(record) != Some(FeatureClass::Hole))
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
                        parameters.remove("ThreadMajorDiameter");
                        parameters.remove("ThreadDepth");
                        parameters.remove("ThreadPitch");
                        parameters.remove("DrillPointAngle");
                    }
                    HoleKind::SimpleDrilled { drill_point_angle } => {
                        parameters.remove("CounterboreDiameter");
                        parameters.remove("CounterboreDepth");
                        parameters.remove("CountersinkDiameter");
                        parameters.remove("CountersinkAngle");
                        parameters.remove("ThreadMajorDiameter");
                        parameters.remove("ThreadDepth");
                        parameters.remove("ThreadPitch");
                        parameters.insert(
                            "DrillPointAngle".into(),
                            format_angle_rad(drill_point_angle.0),
                        );
                    }
                    HoleKind::Counterbore { diameter, depth } => {
                        parameters.remove("CountersinkDiameter");
                        parameters.remove("CountersinkAngle");
                        parameters.remove("ThreadMajorDiameter");
                        parameters.remove("ThreadDepth");
                        parameters.remove("ThreadPitch");
                        parameters
                            .insert("CounterboreDiameter".into(), format_length_mm(diameter.0));
                        parameters.insert("CounterboreDepth".into(), format_length_mm(depth.0));
                        parameters.remove("DrillPointAngle");
                    }
                    HoleKind::CounterboreDrilled {
                        diameter,
                        depth,
                        drill_point_angle,
                    } => {
                        parameters.remove("CountersinkDiameter");
                        parameters.remove("CountersinkAngle");
                        parameters.remove("ThreadMajorDiameter");
                        parameters.remove("ThreadDepth");
                        parameters.remove("ThreadPitch");
                        parameters
                            .insert("CounterboreDiameter".into(), format_length_mm(diameter.0));
                        parameters.insert("CounterboreDepth".into(), format_length_mm(depth.0));
                        parameters.insert(
                            "DrillPointAngle".into(),
                            format_angle_rad(drill_point_angle.0),
                        );
                    }
                    HoleKind::Countersink { diameter, angle } => {
                        parameters.remove("CounterboreDiameter");
                        parameters.remove("CounterboreDepth");
                        parameters.remove("ThreadMajorDiameter");
                        parameters.remove("ThreadDepth");
                        parameters.remove("ThreadPitch");
                        parameters.remove("DrillPointAngle");
                        parameters
                            .insert("CountersinkDiameter".into(), format_length_mm(diameter.0));
                        parameters.insert("CountersinkAngle".into(), format_angle_rad(angle.0));
                    }
                    HoleKind::Threaded {
                        major_diameter,
                        thread_depth,
                        pitch,
                        drill_point_angle,
                    } => {
                        parameters.remove("CounterboreDiameter");
                        parameters.remove("CounterboreDepth");
                        parameters.remove("CountersinkDiameter");
                        parameters.remove("CountersinkAngle");
                        parameters.insert(
                            "ThreadMajorDiameter".into(),
                            format_length_mm(major_diameter.0),
                        );
                        parameters.insert("ThreadDepth".into(), format_length_mm(thread_depth.0));
                        if let Some(pitch) = pitch {
                            parameters.insert("ThreadPitch".into(), format_length_mm(pitch.0));
                        } else {
                            parameters.remove("ThreadPitch");
                        }
                        parameters.insert(
                            "DrillPointAngle".into(),
                            format_angle_rad(drill_point_angle.0),
                        );
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
                match placements.as_slice() {
                    [cadmpeg_ir::features::HolePlacement::Directed {
                        position,
                        direction,
                    }] => {
                        if !position.x.is_finite()
                            || !position.y.is_finite()
                            || !position.z.is_finite()
                        {
                            return Err(CodecError::Malformed(format!(
                                "SLDPRT feature {} has a non-finite hole position",
                                feature.id
                            )));
                        }
                        require_direction(*direction, &feature.id, "hole direction")?;
                        properties.insert("Position".into(), format_point3_mm(*position));
                        properties.insert("Direction".into(), format_vector3(*direction));
                    }
                    [] if existing.is_none() => {
                        properties.remove("Position");
                        properties.remove("Direction");
                    }
                    [] => {}
                    placements
                        if existing.is_some()
                            && placements.iter().all(|placement| {
                                matches!(
                                    placement,
                                    cadmpeg_ir::features::HolePlacement::Axis { .. }
                                )
                            }) => {}
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "SLDPRT feature {} has placements that require native generated-surface identities",
                            feature.id
                        )));
                    }
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
                    let profile_source =
                        profile_source(profile, &record_sources, &feature_sources, &sketch_sources)
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
                path,
                mode,
                twist,
                scale,
            } => {
                if existing.as_deref().is_some_and(|record| !is_sweep(record)) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes operation family",
                        feature.id
                    )));
                }
                let profile_source = match profile {
                    Some(ProfileRef::Generated { .. }) if existing.is_some() => None,
                    Some(ProfileRef::Feature(_))
                        if existing
                            .as_deref()
                            .is_some_and(|record| !record.properties.contains_key("Profile")) =>
                    {
                        None
                    }
                    Some(profile) => Some(
                        profile_source(profile, &record_sources, &feature_sources, &sketch_sources)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "SLDPRT feature {} references a missing sweep profile",
                                    feature.id
                                ))
                            })?,
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
                if existing.is_none()
                    && matches!(
                        mode,
                        SweepMode::Solid {
                            op: BooleanOp::Unresolved
                        }
                    )
                {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has an unresolved boolean operation",
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
                    SweepMode::Solid { op } if *op != BooleanOp::Unresolved => {
                        properties.insert(
                            "Operation".into(),
                            resolved_boolean_op(*op, &feature.id)?.into(),
                        );
                    }
                    SweepMode::Solid { .. } => {}
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
            } => {
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
                    .map(|profile| {
                        profile_source(profile, &record_sources, &feature_sources, &sketch_sources)
                    })
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
                require_same_family(existing.as_deref(), &feature.id, &["Rib"])?;
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
                    let profile_source =
                        profile_source(profile, &record_sources, &feature_sources, &sketch_sources)
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
                    PatternKind::Linear { .. } => Some(PatternForm::Linear),
                    PatternKind::Circular { .. } => Some(PatternForm::Circular),
                    PatternKind::CurveDriven { .. } => Some(PatternForm::CurveDriven),
                    PatternKind::Mirror { .. } => Some(PatternForm::Mirror),
                };
                if existing.as_deref().is_some_and(|record| {
                    expected_form.is_some_and(|form| pattern_form(record) != Some(form))
                }) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} changes pattern form",
                        feature.id
                    )));
                }
                let mut seed_sources = Vec::new();
                for seed in seeds {
                    match seed {
                        PatternSeed::Feature(seed) => seed_sources.push(
                            parent_sources.get(seed).cloned().ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "SLDPRT feature {} references a missing pattern seed",
                                    feature.id
                                ))
                            })?,
                        ),
                        PatternSeed::Faces(_) | PatternSeed::Bodies(_) if existing.is_some() => {}
                        PatternSeed::Faces(_) | PatternSeed::Bodies(_) => {
                            return Err(CodecError::NotImplemented(format!(
                                "SLDPRT feature {} has a source-less topology pattern seed",
                                feature.id
                            )));
                        }
                    }
                }
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
                        second,
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
                        if let Some(second) = second {
                            require_direction(second.direction, &feature.id, "second pattern")?;
                            require_count(second.count, &feature.id)?;
                            if !second.spacing.0.is_finite() || second.spacing.0 <= 0.0 {
                                return Err(CodecError::Malformed(format!(
                                    "SLDPRT feature {} has invalid second linear-pattern spacing",
                                    feature.id
                                )));
                            }
                            properties
                                .insert("Direction2".into(), format_vector3(second.direction));
                            parameters
                                .insert("D4".into(), format_length_like(second.spacing.0, None));
                            parameters.insert("D2".into(), second.count.to_string());
                        }
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
                }
                let kind = existing.as_deref().map_or_else(
                    || match expected_form {
                        Some(PatternForm::Linear) => "LinearPattern".into(),
                        Some(PatternForm::Circular) => "CircularPattern".into(),
                        Some(PatternForm::CurveDriven) => "CrvPattern".into(),
                        Some(PatternForm::Mirror) => "Mirror".into(),
                        None => "Pattern".into(),
                    },
                    |record| record.kind.clone(),
                );
                (kind, parameters, properties)
            }
        };
        if let Some(record) = existing.as_deref() {
            restore_equivalent_parameter_expressions(
                record,
                &original_parameters,
                &evaluated_parameters,
                &mut parameters,
            );
        }
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
                source_id: generated_sources.get(&feature.id).cloned(),
                parent_source_id,
                ordinal,
                name: feature.name.clone().unwrap_or_default(),
                kind,
                input_class: None,
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
    synchronize_neutral_feature_content(features, parameters, &record_ids, native)?;
    let changed_parameters = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .flat_map(|feature| {
            let original = original_parameters.get(&feature.id);
            feature
                .parameters
                .iter()
                .filter(move |(name, expression)| {
                    original.and_then(|parameters| parameters.get(*name)) != Some(*expression)
                })
                .map(move |(name, _)| (feature.id.clone(), name.clone()))
        })
        .collect::<std::collections::HashSet<_>>();
    crate::resolved_features::sync_changed_feature_scalars(
        &native.feature_histories,
        &mut native.feature_input_lanes,
        &changed_parameters,
    )?;
    let projected_features = project_features_with_native_inputs(native);
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
    features: &HashMap<&FeatureId, &str>,
    sketches: &HashMap<cadmpeg_ir::sketches::SketchId, String>,
) -> Option<String> {
    match profile {
        ProfileRef::Unresolved(_) => None,
        ProfileRef::Native(id) => Some(native.get(id).cloned().unwrap_or_else(|| id.clone())),
        ProfileRef::Sketch(id) => sketches.get(id).cloned(),
        ProfileRef::Feature(id) => features.get(id).map(|source| (*source).to_string()),
        ProfileRef::Generated { .. } => None,
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
    let kind = kind
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    match kind.as_slice() {
        b"bossextrude" => Some(BooleanOp::Join),
        b"cutextrude" => Some(BooleanOp::Cut),
        _ => None,
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
        FeatureDefinition::CosmeticThread { .. } => "Feature",
        FeatureDefinition::DatumPrincipalPlane { .. } => "Feature",
        FeatureDefinition::DatumPlane { .. } => "ReferencePlane",
        FeatureDefinition::DatumOffsetPlane { .. } => "Feature",
        FeatureDefinition::DatumAxis { .. } => "ReferenceAxis",
        FeatureDefinition::DatumPoint { .. } => "ReferencePoint",
        FeatureDefinition::DatumCoordinateSystem { .. } => "CoordinateSystem",
        FeatureDefinition::EquationCurve { .. } => "EquationDrivenCurve",
        FeatureDefinition::ProjectedCurve { .. } => "ProjectedCurve",
        FeatureDefinition::CompositeCurve { .. } => "CompositeCurve",
        FeatureDefinition::Helix { .. } | FeatureDefinition::HelixNativeAxis { .. } => "Helix",
        FeatureDefinition::Wrap { .. } => "Wrap",
        FeatureDefinition::Sketch { .. } | FeatureDefinition::SpatialSketch { .. } => "Sketch",
        FeatureDefinition::SketchBlockDefinition { .. } => "Block",
        FeatureDefinition::SketchBlockInstance { .. } => "Feature",
        FeatureDefinition::Extrude { .. } => "Extrusion",
        FeatureDefinition::Revolve { .. } => "Revolve",
        FeatureDefinition::Sweep {
            mode: SweepMode::Surface,
            ..
        } => "Surface-Sweep",
        FeatureDefinition::Sweep { .. } => "Sweep",
        FeatureDefinition::Loft { .. } => "Loft",
        FeatureDefinition::Rib { .. } => "Rib",
        FeatureDefinition::Fillet { .. } => "Fillet",
        FeatureDefinition::Chamfer { .. } => "Chamfer",
        FeatureDefinition::Shell { .. } => "Shell",
        FeatureDefinition::Thicken { .. } => "Thicken",
        FeatureDefinition::OffsetSurface { .. } => "OffsetSurface",
        FeatureDefinition::KnitSurface { .. } => "KnitSurface",
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
