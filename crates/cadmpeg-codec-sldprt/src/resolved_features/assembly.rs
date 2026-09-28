//! Feature-input lane assembly from container streams.

use super::markers::{
    admit_sketch_input_entities, reference_cells_charged, relation_bindings_charged,
};
use super::names::{class_declarations, configuration, object_names};
use super::scalars::named_scalars_charged;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use crate::container::ContainerScan;
use crate::records::{FeatureInputClassRole, FeatureInputLane};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::Exactness;

pub(crate) fn is_supplemental_config_lane(lane: &FeatureInputLane) -> bool {
    lane.id.contains(":config-objects#")
}

pub(crate) fn lanes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
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
        let lane = feature_input_lane(
            ctx,
            source,
            section,
            "resolved-features",
            annotations,
        )?;
        ctx.reserve_collection_vec(&mut result, 1, "collect SLDPRT feature input lanes")?;
        result.push(lane);
    }
    Ok(result)
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
        let Some(section) = source.name() else { continue; };
        if legacy_feature_input_section(section) && legacy_sketch_object_stream(ctx, source.payload())? {
            let lane = feature_input_lane(ctx, source, section, "config-objects", annotations)?;
            ctx.reserve_collection_vec(&mut lanes, 1, "collect SLDPRT supplemental feature lanes")?;
            lanes.push(lane);
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
    let classes = class_declarations(ctx, payload, &parent)?;
    let names = object_names(ctx, payload, &parent)?;
    let scalars = named_scalars_charged(ctx, payload, &parent, &names)?;
    let relation_bindings = relation_bindings_charged(ctx, &parent, &classes, &scalars)?;
    let references = reference_cells_charged(ctx, &scalars, &classes)?;
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
        configuration: configuration(ctx, section)?,
        native_payload: ctx.copy_retained(payload, "retain SLDPRT feature input payload")?,
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

fn legacy_sketch_object_stream(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<bool, cadmpeg_core::CodecError> {
    let classes = class_declarations(ctx, payload, "legacy-sketch-probe")?;
    Ok(classes.iter().any(|class| class.name == "sgSketch")
        && classes
            .iter()
            .any(|class| class.role() == FeatureInputClassRole::SketchEntity))
}

#[cfg(test)]
mod assembly_tests;
