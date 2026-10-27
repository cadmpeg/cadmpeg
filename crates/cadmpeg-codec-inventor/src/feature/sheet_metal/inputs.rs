// SPDX-License-Identifier: Apache-2.0
//! Historical attachment edges selected through exact stored revisions.

use cadmpeg_core::{
    decode::{unique_index, DecodeContext},
    CodecError,
};
use cadmpeg_ir::{
    features::{
        DistinctMembers, EdgeSelection, Feature, FeatureDefinition, FeatureInputTopology,
        FeatureOperation, HistoricalEdgeGeometry,
    },
    ids::{FeatureInputTopologyId, HistoricalEdgeId},
};

use super::operands;
use crate::feature::{FeatureInventory, PmDcFeature};

fn state_id(source: &PmDcFeature) -> Result<FeatureInputTopologyId, CodecError> {
    Ok(FeatureInputTopologyId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-input"),
        source.identity.key()?,
    ))
}

pub(in crate::feature) fn project(
    ctx: &DecodeContext<'_>,
    features: &mut [Feature],
    inventory: &FeatureInventory,
) -> Result<Vec<FeatureInputTopology>, CodecError> {
    let sources = unique_index(
        inventory
            .features
            .iter()
            .map(|source| (source.id(), source)),
    );
    let properties = unique_index(inventory.properties.iter().map(|property| {
        (
            (
                property.identity.segment_token.as_str(),
                property.identity.record_ordinal,
            ),
            property,
        )
    }));
    let mut states = Vec::new();
    for position in 0..features.len() {
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
            attachment,
            ..
        }) = features[position].evaluation.definition()
        else {
            continue;
        };
        let EdgeSelection::Generated { native, .. } = attachment else {
            return Err(CodecError::malformed(
                "Inventor attached plate lost its proven input edges",
            ));
        };
        let source = features[position]
            .native_ref
            .as_deref()
            .and_then(|id| sources.get(id))
            .and_then(Option::as_ref)
            .ok_or_else(|| CodecError::malformed("Inventor attached plate has no source"))?;
        let token = source.identity.segment_token.as_str();
        let selected = operands::selection_sources(source, |reference| {
            properties
                .get(&(token, reference.record_ordinal()?))?
                .as_ref()
                .copied()
        })
        .ok_or_else(|| CodecError::malformed("Inventor attached plate lost its input selection"))?;
        let mut edges = Vec::new();
        let mut geometry = Vec::new();
        for source in selected {
            let sample = inventory
                .edge_inputs
                .geometry
                .get(&(token.to_owned(), source))
                .ok_or_else(|| {
                    CodecError::malformed("Inventor attachment has no stored edge geometry")
                })?;
            let edge = HistoricalEdgeId::mint(format!("inventor:input-edge#{token}-{source}"))
                .map_err(CodecError::malformed)?;
            geometry.push(HistoricalEdgeGeometry {
                edge: edge.clone(),
                start: Some(sample.start),
                end: Some(sample.end),
                interior: sample.interior,
                tangent: Some(sample.tangent),
                faces: DistinctMembers::default(),
            });
            edges.push(edge);
        }
        let native = native.as_str().to_owned();
        let plate_id = features[position].id.clone();
        let targets = std::iter::once(position).chain(features.iter().enumerate().filter_map(|(index,feature)| {
            matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::SheetMetalBend { plate,.. }) if plate==&plate_id).then_some(index)
        })).collect::<Vec<_>>();
        for target in targets {
            let source = features[target]
                .native_ref
                .as_deref()
                .and_then(|id| sources.get(id))
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    CodecError::malformed("Inventor sheet-metal feature has no source")
                })?;
            let id = state_id(source)?;
            let selection = EdgeSelection::historical(id.clone(), edges.clone(), native.clone())
                .map_err(CodecError::malformed)?;
            features[target].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(
                    FeatureOperation::SheetMetalAttachedPlate { attachment, .. }
                    | FeatureOperation::SheetMetalBend { attachment, .. },
                ) = definition
                {
                    *attachment = selection;
                }
            });
            ctx.charge_entities(
                1 + cadmpeg_core::decode::u64_from_index(geometry.len()),
                "project Inventor historical attachment edges",
            )?;
            states.push(FeatureInputTopology {
                id,
                input_of: features[target].id.clone(),
                bodies: DistinctMembers::default(),
                faces: DistinctMembers::default(),
                edges: edges.clone().try_into().map_err(CodecError::malformed)?,
                vertices: DistinctMembers::default(),
                edge_geometry: geometry.clone(),
                native_ref: Some(native.clone()),
            });
        }
    }
    Ok(states)
}

#[cfg(test)]
mod tests;
