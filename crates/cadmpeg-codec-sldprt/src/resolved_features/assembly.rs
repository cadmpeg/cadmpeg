//! Feature-input lane assembly from container streams.

use super::markers::{admit_sketch_input_entities, reference_cells, relation_bindings};
use super::names::{class_declarations, configuration, object_names};
use super::scalars::named_scalars;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use crate::container::ContainerScan;
use crate::loss::SldprtLossCode;
use crate::records::{ArchiveParameterClass, FeatureInputClassRole, FeatureInputLane};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::Exactness;
use std::collections::HashMap;

const RESOLVED_FEATURES_SUFFIX: &str = "-resolvedfeatures";

pub(crate) fn is_supplemental_config_lane(lane: &FeatureInputLane) -> bool {
    lane.id.contains(":config-objects#")
}

pub(crate) fn lanes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<FeatureInputLane>, cadmpeg_core::CodecError> {
    let has_explicit_lanes = scan.sections().any(|source| {
        source
            .name()
            .is_some_and(|name| contains_ascii_case_insensitive(name, "resolvedfeatures"))
    });
    let mut result = Vec::new();
    for source in scan.sections() {
        let Some(section) = source.name() else {
            continue;
        };
        if if has_explicit_lanes {
            !contains_ascii_case_insensitive(section, "resolvedfeatures")
        } else {
            !legacy_feature_input_section(section)
        } {
            continue;
        }
        let mut lane = feature_input_lane(ctx, source, section, "resolved-features", annotations)?;
        if has_explicit_lanes {
            type_scalars_from_archive(ctx, scan, section, source.payload(), &mut lane, losses)?;
        }
        result.push(lane);
    }
    Ok(result)
}

/// Give each scalar of a `Config-N-ResolvedFeatures` lane the class of the
/// parameter object whose value it is, from the exact walk of the stream.
/// Scalars the walk did not reach keep no class.
fn type_scalars_from_archive(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    section: &str,
    resolved: &[u8],
    lane: &mut FeatureInputLane,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(config_name) = configuration_name(section) else {
        return Ok(());
    };
    let Some(config) = scan.sections().find_map(|other| {
        other
            .name()
            .filter(|name| name.replace('\\', "/").eq_ignore_ascii_case(&config_name))
            .map(|_| other.payload())
    }) else {
        return Ok(());
    };
    let Some(version) =
        crate::carchive::walk_version(crate::container::declared_sw_version(scan), config)
    else {
        return Ok(());
    };
    let Some(walk) = crate::carchive::walk_configuration(ctx, config, resolved, Some(version))?
    else {
        return Ok(());
    };
    if let crate::carchive::Status::Stop(_) = walk.status {
        losses.push(SldprtLossCode::FeatureInputArchiveWalkStopped.note(format!(
            "the archive walk of {config_name} stopped before the end of the stream, so its \
             scalars keep no parameter class"
        )));
        return Ok(());
    }
    let classes = scalar_archive_classes(&walk);
    for scalar in &mut lane.scalars {
        scalar.archive_class = classes.get(&scalar.offset).copied();
    }
    Ok(())
}

/// The `Config-N` name a `Config-N-ResolvedFeatures` section belongs to.
fn configuration_name(section: &str) -> Option<String> {
    let normalized = section.replace('\\', "/");
    normalized
        .len()
        .checked_sub(RESOLVED_FEATURES_SUFFIX.len())
        .filter(|&length| {
            normalized.is_char_boundary(length)
                && normalized[length..].eq_ignore_ascii_case(RESOLVED_FEATURES_SUFFIX)
        })
        .map(|length| normalized[..length].to_owned())
}

/// The archive parameter class of each scalar value offset.
fn scalar_archive_classes(walk: &crate::carchive::Walk) -> HashMap<u64, ArchiveParameterClass> {
    walk.parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                u64_from_index(parameter.value_at),
                ArchiveParameterClass::from_class_name(&parameter.class)?,
            ))
        })
        .collect()
}

pub(crate) fn supplemental_config_lanes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> Result<Vec<FeatureInputLane>, cadmpeg_core::CodecError> {
    let has_explicit_lanes = scan.sections().any(|source| {
        source
            .name()
            .is_some_and(|name| contains_ascii_case_insensitive(name, "resolvedfeatures"))
    });
    if !has_explicit_lanes {
        return Ok(Vec::new());
    }
    let mut lanes = Vec::new();
    for source in scan.sections() {
        let Some(section) = source.name() else {
            continue;
        };
        if legacy_feature_input_section(section) && legacy_sketch_object_stream(source.payload())? {
            lanes.push(feature_input_lane(
                ctx,
                source,
                section,
                "config-objects",
                annotations,
            )?);
        }
    }
    Ok(lanes)
}

fn feature_input_lane(
    ctx: &DecodeContext<'_>,
    source: crate::container::Section<'_>,
    section: &str,
    family: &str,
    annotations: &mut Annotations,
) -> Result<FeatureInputLane, cadmpeg_core::CodecError> {
    let parent = format!("sldprt:feature-input:{family}#{}", source.ordinal());
    let payload = source.payload();
    let classes = class_declarations(payload, &parent)?;
    let names = object_names(ctx, payload, &parent)?;
    let scalars = named_scalars(payload, &parent, &names)?;
    let relation_bindings = relation_bindings(&parent, &classes, &scalars, &[])?;
    let references = reference_cells(&scalars, &classes)?;
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
        )?;
    }
    crate::annotations::note(
        annotations,
        parent.as_str(),
        source.source_stream(),
        0,
        if family == "config-objects" {
            "ConfigObjects"
        } else {
            "ResolvedFeatures"
        },
        Exactness::ByteExact,
    )?;
    ctx.retain_bytes(
        u64_from_index(payload.len()),
        "retain SLDPRT feature input payload",
    )?;
    Ok(FeatureInputLane {
        id: parent,
        configuration: configuration(section),
        native_payload: payload.to_vec().into(),
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
    let bytes = section.as_bytes();
    if bytes.len() < "Contents/Config-".len()
        || !matches!(bytes[8], b'/' | b'\\')
        || !((bytes[..8] == *b"Contents" && bytes[9..16] == *b"Config-")
            || (bytes[..8] == *b"contents" && bytes[9..16] == *b"config-"))
    {
        return false;
    }
    let configuration = &section[16..];
    !configuration.is_empty() && configuration.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn contains_ascii_case_insensitive(text: &str, needle: &str) -> bool {
    text.as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn legacy_sketch_object_stream(payload: &[u8]) -> Result<bool, cadmpeg_core::CodecError> {
    let classes = class_declarations(payload, "legacy-sketch-probe")?;
    Ok(classes.iter().any(|class| class.name == "sgSketch")
        && classes
            .iter()
            .any(|class| class.role() == FeatureInputClassRole::SketchEntity))
}

#[cfg(test)]
mod assembly_tests;
