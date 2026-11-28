// SPDX-License-Identifier: Apache-2.0
//! Contour attachment selections from stored input-edge identities.

use std::collections::BTreeMap;

use cadmpeg_core::decode::unique_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::sheet_metal::{BendSpec, SheetMetal, SheetMetalKind};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::selection::{
    Anchor, Members, PickTarget, Picked, Picks, Selection, Witness,
};
use cadmpeg_ir::features::evaluation::FeatureInputTopology;
use cadmpeg_ir::CadIr;

use crate::feature::sheet_metal::flange::{Flange, FlangeKind};
use crate::feature::{FeatureInventory, PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind};
use crate::pmdc::PmDcReference;

/// The contour flange of the attached plate `source` under `parent`.
/// `witness` gives the witness of the attachment edge with a selector
/// source.
pub(crate) fn definition<'a>(
    parent: &Flange,
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    witness: impl Fn(u32) -> Option<Witness>,
) -> Result<Option<FeatureOperation>, CodecError> {
    if FlangeKind::of(parent) != Some(FlangeKind::Contour)
        || parent.identity.segment_token != source.identity.segment_token
        || parent.children.references().len() != 3
        || parent.children.references()[0].record_ordinal() != Some(source.identity.record_ordinal)
    {
        return Ok(None);
    }
    let Some(attachment) = &parent.attachment_edges else {
        return Ok(None);
    };
    let slots = source.properties.references();
    if slots.len() != 17 || attachment.selection != slots[12] || attachment.edges.is_empty() {
        return Ok(None);
    }
    let Some(profile) = property(slots[0]).filter(|property| {
        matches!(
            property.kind,
            PmDcFeaturePropertyKind::ProfileSelection { value: 0, .. }
        )
    }) else {
        return Ok(None);
    };
    if !property(slots[16]).is_some_and(|property| {
        matches!(
            property.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: crate::feature::PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 1,
            }
        )
    }) {
        return Ok(None);
    }
    let Some(state) = attachment.state else {
        return Ok(None);
    };
    let Some(selection) = property(slots[12]).filter(|property| {
        matches!(
            property.kind,
            PmDcFeaturePropertyKind::References {
                family: crate::feature::PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections,
                ..
            }
        )
    }) else {
        return Ok(None);
    };
    let mut picks = Vec::new();
    for edge in &attachment.edges {
        let Some(revision) = edge.revision else {
            return Ok(None);
        };
        picks.push(Picked {
            target: PickTarget::Native { within: None },
            context: None,
            native: Some(NativeName {
                codec: cadmpeg_ir::codec_id!("inventor"),
                value: NativeNameValue::Integers(vec![
                    i64::from(state.ordinal),
                    state.state_id,
                    i64::from(edge.item.index()),
                    i64::from(edge.selector_source),
                    edge.entity_index,
                    revision,
                ]),
            }),
            domain: None,
            witness: witness(edge.selector_source),
            resolved: None,
        });
    }
    let native = |text: String| -> Result<_, CodecError> {
        Ok(Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: Some(
                vec![text]
                    .try_into()
                    .map_err(|_| CodecError::malformed("Inventor contour selection is blank"))?,
            ),
        })
    };
    let edges = Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(picks).map_err(CodecError::malformed)?),
        native: Some(
            vec![selection.id()]
                .try_into()
                .map_err(|_| CodecError::malformed("Inventor contour attachment is blank"))?,
        ),
    };
    let controls = source.id();
    let controls = cadmpeg_core::text::NonBlankString::new(controls)
        .ok_or_else(|| CodecError::malformed("Inventor contour controls are blank"))?;
    Ok(Some(FeatureOperation::SheetMetal(Box::new(SheetMetal {
        kind: SheetMetalKind::ContourFlange {
            profile: native(profile.id())?,
            edges: Some(edges),
            width: None,
            extent: None,
            thickness: None,
            bend: BendSpec {
                radius: None,
                radius_side: None,
                relief: None,
            },
            vertex_bends: Vec::new(),
            rule: None,
        },
        native_options: BTreeMap::from([(
            cadmpeg_ir::codec_name!("inventor:contour_controls"),
            NativeOption::Text(Dimension::Literal(controls)),
        )]),
        source_settings: BTreeMap::new(),
    }))))
}

pub(crate) fn project(ir: &mut CadIr, inventory: &FeatureInventory) -> Result<(), CodecError> {
    let sources = unique_index(inventory.features.iter().map(|source| {
        (
            (
                source.identity.segment_token.as_str(),
                source.identity.record_ordinal,
            ),
            source,
        )
    }));
    let properties = super::property_index(inventory);
    for parent in &inventory.sheet_metal_flanges {
        let token = parent.identity.segment_token.as_str();
        let Some(source) = parent
            .children
            .references()
            .first()
            .and_then(|reference| reference.record_ordinal())
            .and_then(|ordinal| sources.get(&(token, ordinal)))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        // Only a plate that the operation stages retained in its native
        // form takes the attached contour flange.
        let Some(index) = ir.design.features.iter().position(|feature| {
            feature.native_ref.as_deref() == Some(source.id().as_str())
                && matches!(
                    feature.definition.operation,
                    FeatureOperation::Native { .. }
                )
        }) else {
            continue;
        };
        let feature = &ir.design.features[index].id;
        let mut inputs = ir
            .model
            .feature_input_topologies
            .iter()
            .filter(|input| &input.input_of == feature);
        let input = match (inputs.next(), inputs.next()) {
            (Some(input), None) => Some(input),
            _ => None,
        };
        let Some(operation) = definition(
            parent,
            source,
            |reference| {
                properties
                    .get(&(token, reference.record_ordinal()?))
                    .and_then(Option::as_ref)
                    .copied()
            },
            |selector| witness(input?, token, selector),
        )?
        else {
            continue;
        };
        ir.design.features[index].definition.operation = operation;
    }
    Ok(())
}

/// The witness of the edge with selector source `selector` of the contour
/// plate's input state `input`, where `input` holds one sample of it.
fn witness(input: &FeatureInputTopology, token: &str, selector: u32) -> Option<Witness> {
    let edge = format!("inventor:input-edge#{token}-{selector}");
    let mut samples = input
        .edge_geometry
        .iter()
        .filter(|sample| sample.edge.as_str() == edge);
    match (samples.next(), samples.next()) {
        (Some(sample), None) => Some(Witness::edge_sample(sample)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
