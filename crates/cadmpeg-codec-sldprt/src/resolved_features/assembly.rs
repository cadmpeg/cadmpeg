//! Feature-input lane assembly from container streams.

use super::markers::{
    admit_sketch_input_entities, reference_cells_charged, relation_bindings_charged,
};
use super::names::{class_declarations, configuration, next_payload_class, object_names};
use super::scalars::named_scalars_charged;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use crate::classification::native_object_class;
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
    let has_explicit_lanes = has_resolved_features_sections(ctx, scan)?;
    let mut result = Vec::new();
    for source in scan.sections(ctx)? {
        let Some(section) = source.name() else {
            continue;
        };
        let lane_section = if has_explicit_lanes {
            names_resolved_features(ctx, section)?
        } else {
            legacy_feature_input_section(ctx, section)?
        };
        if !lane_section {
            continue;
        }
        let lane = feature_input_lane(ctx, source, section, "resolved-features", annotations)?;
        ctx.push_vec(&mut result, lane, "collect SLDPRT feature input lanes")?;
    }
    Ok(result)
}

pub(crate) fn supplemental_config_lanes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> Result<Vec<FeatureInputLane>, cadmpeg_core::CodecError> {
    if !has_resolved_features_sections(ctx, scan)? {
        return Ok(Vec::new());
    }
    let mut lanes = Vec::new();
    for source in scan.sections(ctx)? {
        let Some(section) = source.name() else {
            continue;
        };
        if legacy_feature_input_section(ctx, section)?
            && legacy_sketch_object_stream(ctx, source.payload())?
        {
            let lane = feature_input_lane(ctx, source, section, "config-objects", annotations)?;
            ctx.push_vec(
                &mut lanes,
                lane,
                "collect SLDPRT supplemental feature lanes",
            )?;
        }
    }
    Ok(lanes)
}

fn has_resolved_features_sections(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.any_by(
        scan.section_steps(),
        |source| match source.name() {
            Some(name) => names_resolved_features(ctx, name),
            None => Ok(false),
        },
        "find SLDPRT resolved-features sections",
    )
}

/// Whether a section name contains `resolvedfeatures` in any ASCII case.
fn names_resolved_features(
    ctx: &DecodeContext<'_>,
    section: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    const NEEDLE: &[u8] = b"resolvedfeatures";
    ctx.any_by(
        section.as_bytes().windows(NEEDLE.len()),
        |window| Ok(window.eq_ignore_ascii_case(NEEDLE)),
        "find SLDPRT resolved-features section name",
    )
}

fn feature_input_lane(
    ctx: &DecodeContext<'_>,
    source: crate::container::Section<'_>,
    section: &str,
    family: &str,
    annotations: &mut Annotations,
) -> Result<FeatureInputLane, cadmpeg_core::CodecError> {
    let parent = ctx.format_retained(
        format_args!("sldprt:feature-input:{family}#{}", source.ordinal()),
        "format SLDPRT supplemental feature-input identity",
    )?;
    let payload = source.payload();
    let classes = class_declarations(ctx, payload, &parent)?;
    let names = object_names(ctx, payload, &parent)?;
    let scalars = named_scalars_charged(ctx, payload, &parent, &names)?;
    let relation_bindings = relation_bindings_charged(ctx, &parent, &classes, &scalars)?;
    let references = reference_cells_charged(ctx, &scalars, &classes)?;
    let sketch_entities = admit_sketch_input_entities(ctx, payload, &parent)?;
    for entity in ctx.admit_iter(&sketch_entities, "annotate SLDPRT sketch input entities")? {
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
            ctx,
            annotations,
            entity.id(),
            source.source_stream(),
            entity.offset(),
            signature,
            Exactness::ByteExact,
        )?;
    }
    crate::annotations::note(
        ctx,
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

fn legacy_feature_input_section(
    ctx: &DecodeContext<'_>,
    section: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let bytes = section.as_bytes();
    if bytes.len() < "Contents/Config-".len()
        || !matches!(bytes[8], b'/' | b'\\')
        || !((bytes[..8] == *b"Contents" && bytes[9..16] == *b"Config-")
            || (bytes[..8] == *b"contents" && bytes[9..16] == *b"config-"))
    {
        return Ok(false);
    }
    let configuration = &section[16..];
    Ok(!configuration.is_empty()
        && ctx.all_by(
            configuration.bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            "check SLDPRT legacy configuration section",
        )?)
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
    let mut sketch = false;
    let mut sketch_entity = false;
    let mut offsets = 0..payload.len().saturating_sub(3);
    while let Some((_, name)) = next_payload_class(ctx, payload, &mut offsets)? {
        sketch |= name == "sgSketch";
        sketch_entity |= native_object_class(name).role() == FeatureInputClassRole::SketchEntity;
        if sketch && sketch_entity {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod assembly_tests;
