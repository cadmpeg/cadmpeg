// SPDX-License-Identifier: Apache-2.0
//! Source lanes whose projection equals the current neutral content (W2).

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::CadIr;

use crate::native::SldprtNative;
use crate::records::FeatureInputLane;
use crate::writer::write_error::WriteError;

/// The ids of the source lanes the writer re-emits as the document holds
/// them (#209 §2.3 W2).
///
/// A lane qualifies when its typed records agree with its bytes, none of
/// its records has an owning feature or a neutral counterpart, and the
/// sketches its nested streams project equal the document's sketches of that
/// lane, which no neutral feature references.
pub(crate) fn held_lanes(
    ctx: &DecodeContext<'_>,
    document: &CadIr,
    source: &SldprtNative,
) -> Result<BTreeSet<String>, WriteError> {
    let mut held = BTreeSet::new();
    for lane in &source.feature_input_lanes {
        if lane_is_held(ctx, document, source, lane)? {
            held.insert(lane.id.clone());
        }
    }
    Ok(held)
}

fn lane_is_held(
    ctx: &DecodeContext<'_>,
    document: &CadIr,
    source: &SldprtNative,
    lane: &FeatureInputLane,
) -> Result<bool, WriteError> {
    if lane.native_payload.is_empty()
        || crate::resolved_features::write_prepare::validate_lane_against_payload(
            &source.feature_histories,
            lane,
            true,
            crate::writer::SourceTrust::Trustworthy,
        )
        .is_err()
    {
        return Ok(false);
    }
    let owned = lane
        .classes
        .iter()
        .any(|record| record.feature_ref.is_some())
        || lane.names.iter().any(|record| record.feature_ref.is_some())
        || lane
            .scalars
            .iter()
            .any(|record| record.feature_ref.is_some())
        || lane
            .relation_bindings
            .iter()
            .any(|record| record.feature_ref.is_some())
        || lane
            .references
            .iter()
            .any(|record| record.feature_ref.is_some())
        || lane
            .sketch_entities
            .iter()
            .any(|record| record.feature_ref.is_some())
        || !lane.relation_instances.is_empty()
        || !lane.body_selections.is_empty()
        || !lane.edge_selections.is_empty()
        || !lane.surface_selections.is_empty()
        || !lane.generated_surface_identities.is_empty();
    if owned {
        return Ok(false);
    }
    let children = lane_record_ids(lane);
    let model = &document.model;
    let counterpart = model
        .parameters
        .iter()
        .filter_map(|record| record.native_ref.as_deref())
        .chain(
            model
                .sketch_entities
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            model
                .sketch_constraints
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            model
                .spatial_sketch_entities
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            model
                .spatial_sketch_constraints
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            model
                .spatial_sketches
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .any(|id| children.contains(id) || id == lane.id);
    if counterpart {
        return Ok(false);
    }
    let sketches = model
        .sketches
        .iter()
        .filter(|sketch| sketch.native_ref.as_deref() == Some(lane.id.as_str()))
        .collect::<Vec<_>>();
    let sketch_ids = sketches
        .iter()
        .map(|sketch| sketch.id.as_str())
        .collect::<BTreeSet<_>>();
    for feature in &model.features {
        let definition = serde_json::to_string(feature.evaluation.definition())
            .map_err(crate::writer::write_error::WriteError::unwritable)?;
        if sketch_ids
            .iter()
            .any(|id| definition.contains(&format!("\"{id}\"")))
        {
            return Ok(false);
        }
    }
    let projected = project(ctx, lane)?;
    let Some(projected) = projected else {
        return Ok(false);
    };
    let entities = model
        .sketch_entities
        .iter()
        .filter(|entity| sketch_ids.contains(entity.sketch.as_str()))
        .collect::<Vec<_>>();
    let constraints = model
        .sketch_constraints
        .iter()
        .filter(|constraint| sketch_ids.contains(constraint.sketch.as_str()))
        .collect::<Vec<_>>();
    Ok(same(&sketches, &projected.sketches)?
        && same(&entities, &projected.entities)?
        && same(&constraints, &projected.constraints)?
        && same(
            &source
                .nested_sketch_carriers
                .iter()
                .filter(|carriers| carriers.parent == lane.id)
                .collect::<Vec<_>>(),
            &projected.carriers,
        )?)
}

/// Every child record identity a lane holds.
fn lane_record_ids(lane: &FeatureInputLane) -> BTreeSet<&str> {
    lane.classes
        .iter()
        .map(|record| record.id.as_str())
        .chain(lane.names.iter().map(|record| record.id.as_str()))
        .chain(lane.scalars.iter().map(|record| record.id.as_str()))
        .chain(
            lane.relation_bindings
                .iter()
                .map(|record| record.id.as_str()),
        )
        .chain(lane.references.iter().map(|record| record.id.as_str()))
        .chain(
            lane.sketch_entities
                .iter()
                .map(crate::records::SketchInputEntity::id),
        )
        .collect()
}

/// The sketches the decoder projects from one lane's nested streams.
fn project(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
) -> Result<Option<crate::resolved_features::sketch_projection::ProjectedSketches>, WriteError> {
    let Some(key) = lane
        .id
        .strip_prefix("sldprt:feature-input:resolved-features#")
    else {
        return Ok(None);
    };
    let Some(configuration) = lane.configuration.as_deref() else {
        return Ok(None);
    };
    // A configuration-keyed lane projects from a section of that name; an
    // offset-keyed one from a section at that offset.
    let ordinal = if key == format!("config-{configuration}") {
        0
    } else if let Ok(ordinal) = key.parse::<usize>() {
        ordinal
    } else {
        return Ok(None);
    };
    let Ok(section) = cadmpeg_ir::StreamName::try_from(format!(
        "Contents/Config-{configuration}-ResolvedFeatures"
    )) else {
        return Ok(None);
    };
    let payload = lane.native_payload.to_vec();
    let extracted = crate::parasolid::extract_streams_with_offsets(&payload, ctx)?;
    let scan = crate::container::ContainerScan {
        source_image: &[],
        version: 0,
        blocks: vec![crate::container::Block {
            offset: ordinal,
            type_id: 0,
            comp_sz: 0,
            section: crate::container::BlockName::Named(section),
            family: crate::container::PayloadFamily::Unknown,
            payload,
            ps_streams: extracted.streams,
            ps_probe_capped: extracted.probe_capped,
        }],
        block_views: std::collections::BTreeMap::new(),
        directory: Vec::new(),
        cache_cells: Vec::new(),
        truncated_blocks: Vec::new(),
        compound_streams: Vec::new(),
        compound_views: std::collections::BTreeMap::new(),
        solidworks: crate::container::SolidWorksEnvelopeScan::default(),
    };
    let projected = crate::resolved_features::sketch_projection::sketches(
        ctx,
        &scan,
        &mut cadmpeg_ir::annotations::Annotations::default(),
        &mut Vec::new(),
    )?;
    Ok((projected.untyped_lanes.is_empty()).then_some(projected))
}

/// Whether two record sets serialize alike, in identity order.
fn same<A: serde::Serialize, B: serde::Serialize>(
    left: &[A],
    right: &[B],
) -> Result<bool, WriteError> {
    let ordered = |rows: Vec<serde_json::Value>| {
        let mut rows = rows;
        rows.sort_by(|left, right| {
            left.get("id")
                .and_then(serde_json::Value::as_str)
                .cmp(&right.get("id").and_then(serde_json::Value::as_str))
        });
        rows
    };
    let left = left
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::writer::write_error::WriteError::unwritable)?;
    let right = right
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::writer::write_error::WriteError::unwritable)?;
    Ok(ordered(left) == ordered(right))
}

/// Declare a held lane and its child records written as the document holds
/// them.
pub(crate) fn declare_written(
    lane: &FeatureInputLane,
    dispositions: &mut cadmpeg_ir::codec::write::NativeDispositions,
) {
    use cadmpeg_ir::codec::write::NativeDisposition;
    let mut declare = |arena: &str, id: &str| {
        dispositions.set_record("sldprt", arena, id, NativeDisposition::Written);
    };
    declare("feature_input_lanes", &lane.id);
    for record in &lane.classes {
        declare("feature_input_classes", &record.id);
    }
    for record in &lane.names {
        declare("feature_input_names", &record.id);
    }
    for record in &lane.scalars {
        declare("feature_input_scalars", &record.id);
    }
    for record in &lane.references {
        declare("feature_input_references", &record.id);
    }
    for record in &lane.relation_bindings {
        declare("feature_input_relation_bindings", &record.id);
    }
    for record in &lane.sketch_entities {
        declare("sketch_input_entities", record.id());
    }
}
