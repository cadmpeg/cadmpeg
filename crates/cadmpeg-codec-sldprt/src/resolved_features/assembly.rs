//! Feature-input lane assembly from container streams.

use super::markers::{admit_sketch_input_entities, reference_cells, relation_bindings};
use super::names::{class_declarations, configuration, object_names};
use super::scalars::named_scalars;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use crate::container::ContainerScan;
use crate::records::{FeatureInputClassRole, FeatureInputLane};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::Exactness;

pub(crate) fn is_supplemental_config_lane(lane: &FeatureInputLane) -> bool {
    lane.id.contains(":config-objects#")
}

pub(crate) fn lanes(
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> Result<Vec<FeatureInputLane>, cadmpeg_core::CodecError> {
    let sections = scan.sections().collect::<Vec<_>>();
    let has_explicit_lanes = sections.iter().any(|source| {
        source
            .name()
            .is_some_and(|name| name.to_ascii_lowercase().contains("resolvedfeatures"))
    });
    let mut result = Vec::new();
    for source in sections {
        let Some(section) = source.name() else {
            continue;
        };
        if if has_explicit_lanes {
            !section.to_ascii_lowercase().contains("resolvedfeatures")
        } else {
            !legacy_feature_input_section(section)
        } {
            continue;
        }
        result.push(feature_input_lane(
            source,
            section,
            "resolved-features",
            annotations,
        )?);
    }
    Ok(result)
}

pub(crate) fn supplemental_config_lanes(
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> Result<Vec<FeatureInputLane>, cadmpeg_core::CodecError> {
    let has_explicit_lanes = scan.sections().any(|source| {
        source
            .name()
            .is_some_and(|name| name.to_ascii_lowercase().contains("resolvedfeatures"))
    });
    if !has_explicit_lanes {
        return Ok(Vec::new());
    }
    scan.sections()
        .filter_map(|source| {
            let section = source.name()?;
            (legacy_feature_input_section(section) && legacy_sketch_object_stream(source.payload()))
                .then_some((source, section))
        })
        .map(|(source, section)| feature_input_lane(source, section, "config-objects", annotations))
        .collect()
}

fn feature_input_lane(
    source: crate::container::Section<'_>,
    section: &str,
    family: &str,
    annotations: &mut Annotations,
) -> Result<FeatureInputLane, cadmpeg_core::CodecError> {
    let parent = format!("sldprt:feature-input:{family}#{}", source.ordinal());
    let payload = source.payload();
    let classes = class_declarations(payload, &parent);
    let names = object_names(payload, &parent);
    let scalars = named_scalars(payload, &parent, &names);
    let relation_bindings = relation_bindings(&parent, &classes, &scalars);
    let references = reference_cells(&scalars, &classes);
    let sketch_entities = admit_sketch_input_entities(payload, &parent)?;
    for entity in &sketch_entities {
        let signature = usize::try_from(entity.offset())
            .ok()
            .and_then(|offset| payload.get(offset..offset + SKETCH_MARKER.len()))
            .map_or("sketch-marker", |prefix| {
                if prefix == LEGACY_SKETCH_MARKER {
                    "ff_ff_07_00_01"
                } else if prefix == LEGACY_EXTENDED_SKETCH_MARKER {
                    "ff_ff_1f_00_01"
                } else {
                    "ff_ff_1f_00_03"
                }
            });
        crate::annotations::note(
            annotations,
            entity.id(),
            source.source_stream(),
            entity.offset(),
            signature,
            Exactness::ByteExact,
        );
    }
    crate::annotations::note(
        annotations,
        parent.clone(),
        source.source_stream(),
        0,
        if family == "config-objects" {
            "ConfigObjects"
        } else {
            "ResolvedFeatures"
        },
        Exactness::ByteExact,
    );
    Ok(FeatureInputLane {
        id: parent,
        configuration: configuration(section),
        native_payload: payload.to_vec(),
        classes,
        names,
        scalars,
        relation_bindings,
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references,
        sketch_entities,
    })
}

fn legacy_feature_input_section(section: &str) -> bool {
    let normalized = section.replace('\\', "/");
    let Some(configuration) = normalized
        .strip_prefix("Contents/Config-")
        .or_else(|| normalized.strip_prefix("contents/config-"))
    else {
        return false;
    };
    !configuration.is_empty() && configuration.bytes().all(|byte| byte.is_ascii_digit())
}

fn legacy_sketch_object_stream(payload: &[u8]) -> bool {
    let classes = class_declarations(payload, "legacy-sketch-probe");
    classes.iter().any(|class| class.name == "sgSketch")
        && classes
            .iter()
            .any(|class| class.role() == FeatureInputClassRole::SketchEntity)
}

#[cfg(test)]
mod assembly_tests;
