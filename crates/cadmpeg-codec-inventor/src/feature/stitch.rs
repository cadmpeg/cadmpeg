// SPDX-License-Identifier: Apache-2.0
//! Stitch Surface features
//! ([spec §13.12](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1312-stitch-surface)).
//!
//! Slot 0 of the six-slot record is the object collection of the surface-body
//! wrappers that the stitch joins, and slot 2 is the wrapper of the result.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::evaluation::{
    FeatureEvaluation, FeatureResultMembers, FeatureResultTopology,
};
use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers, NativeSelections};
use cadmpeg_ir::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation, SewBodySelection,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::scalar::{Length, PositiveLength};

use super::{
    ClassId, PmDcFeature, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily, ProjectionIndex,
};

/// Feature-label class of a Stitch Surface.
pub(super) const CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x60, 0x8b, 0xa3, 0x9e, 0xd6, 0x11, 0x74, 0x73, 0x27, 0x99, 0x43, 0xad, 0x7f, 0xbe, 0x0b, 0x63,
]));

/// Source tag of the projected feature.
pub(crate) const TAG: &str = "stitch";

/// Slot of the result surface-body wrapper.
pub(super) const RESULT_SLOT: usize = 2;

/// Count of property slots of the record.
const SLOT_COUNT: usize = 6;

/// Slots of the two Boolean controls.
const CONTROL_SLOTS: [usize; 2] = [3, 5];

/// Slot of the tolerance parameter.
const TOLERANCE_SLOT: usize = 4;

/// The neutral operation and the result wrapper that the PmDc slots give.
/// `property` resolves a record reference index to a feature property, and
/// `length` resolves one to the value of a length parameter.
pub(crate) fn operation<'a>(
    source: &PmDcFeature,
    property: impl Fn(u32) -> Option<&'a PmDcFeatureProperty>,
    length: impl Fn(u32) -> Option<Length>,
) -> Option<(FeatureOperation, &'a PmDcFeatureProperty)> {
    let slots = source.properties.references();
    if slots.len() != SLOT_COUNT || slots[1].index() != 0 {
        return None;
    }
    let slot = |slot: usize| property(slots[slot].index());
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &slot(0)?.kind
    else {
        return None;
    };
    let wrappers = items
        .references()
        .iter()
        .map(|reference| {
            let wrapper = property(reference.index())?;
            matches!(wrapper.kind, PmDcFeaturePropertyKind::SurfaceBody { .. })
                .then(|| wrapper.id())
        })
        .collect::<Option<Vec<_>>>()?;
    let distinct = wrappers.iter().collect::<std::collections::HashSet<_>>();
    if wrappers.is_empty() || distinct.len() != wrappers.len() {
        return None;
    }
    if !CONTROL_SLOTS.into_iter().all(|control| {
        matches!(
            slot(control).map(|property| &property.kind),
            Some(PmDcFeaturePropertyKind::Boolean { .. })
        )
    }) {
        return None;
    }
    let result = slot(RESULT_SLOT)?;
    if !matches!(result.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
        return None;
    }
    let gap_tolerance = PositiveLength::new(length(slots[TOLERANCE_SLOT].index())?.get())?;
    let bodies = match wrappers.as_slice() {
        [one] => BodySelection::Native(one.clone()),
        _ => BodySelection::NativeSet(NativeSelections::try_from(wrappers).ok()?),
    };
    Some((
        FeatureOperation::SewBodies {
            bodies: SewBodySelection::try_from(bodies).ok()?,
            gap_tolerance: Some(gap_tolerance),
        },
        result,
    ))
}

/// Project a Stitch Surface with its operand wrappers and its result body.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let token = source.identity.segment_token.as_str();
    let (operation, wrapper) = operation(
        source,
        |reference| super::resolve_property(token, reference, index),
        |reference| super::length_reference(token, reference, index),
    )?;
    let members = FeatureResultMembers::new(
        vec![cadmpeg_core::text::NonBlankString::new(wrapper.id())?],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .ok()?;
    if let Err(error) = ctx.charge_entities(2, "project Inventor Stitch Surface") {
        return Some(Err(error));
    }
    let key = match source.identity.key() {
        Ok(key) => key,
        Err(error) => return Some(Err(error)),
    };
    let id = FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        key.clone(),
    );
    let result = FeatureResultTopology::new(
        FeatureResultTopologyId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
            key,
        ),
        id.clone(),
        members,
        Some(wrapper.id()),
    );
    Some(Ok((
        Feature {
            product_definition: None,
            id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: super::boolean_properties(source, &CONTROL_SLOTS, index),
            source_tag: Some(TAG.into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

/// The loss of the projection, if any Stitch Surface feature exists.
pub(crate) fn losses(projected: usize) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    if projected == 0 {
        return Vec::new();
    }
    vec![crate::loss::InventorLossCode::FeatureStitchControlsNative.note(format!(
        "Typed {projected} Stitch Surface feature(s) as knits of their operand collection; the two Boolean controls stay source properties, and the role of slot 4 as the tolerance is assumed from its position."
    ))]
}

#[cfg(test)]
mod tests;
