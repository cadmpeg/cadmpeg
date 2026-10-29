// SPDX-License-Identifier: Apache-2.0
//! Body operands and retained target identity of a Combine feature.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, CombineOperands, DistinctMembers, Feature, FeatureContent,
    FeatureDefinition, FeatureEvaluation, FeatureOperation, FeatureResultTopology,
    NativeSelections,
};
use cadmpeg_ir::native::bytes::NativeBytes;

use super::{
    ClassId, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectionIndex,
};

pub(super) const CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x33, 0x67, 0x23, 0xc0, 0x7e, 0x40, 0xea, 0xcb, 0x2e, 0xff, 0x66, 0x9e, 0x9a, 0xdd, 0x87, 0xf8,
]));

pub(crate) fn definition<'a>(
    source: &PmDcFeature,
    resolve: impl Fn(u32) -> Option<&'a PmDcFeatureProperty>,
) -> Option<FeatureDefinition> {
    let [target, tools, operation, keep_tools] = source.properties.references() else {
        return None;
    };
    let target = resolve(target.index())?;
    let tools = resolve(tools.index())?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::BodySelection,
        items: targets,
    } = &target.kind
    else {
        return None;
    };
    let [target] = targets.references() else {
        return None;
    };
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items: tools,
    } = &tools.kind
    else {
        return None;
    };
    let body = |reference: crate::pmdc::PmDcReference| {
        let property = resolve(reference.index())?;
        matches!(property.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }).then(|| property.id())
    };
    let target = body(*target)?;
    let tools = tools
        .references()
        .iter()
        .copied()
        .map(body)
        .collect::<Option<Vec<_>>>()?;
    if tools.iter().any(|tool| tool == &target) {
        return None;
    }
    let tools = match tools.as_slice() {
        [one] => BodySelection::Native(one.clone()),
        _ => BodySelection::NativeSet(NativeSelections::try_from(tools).ok()?),
    };
    let operands = CombineOperands::new(BodySelection::Native(target), tools).ok()?;
    let PmDcFeaturePropertyKind::Enumeration {
        family: PmDcFeatureEnumFamily::PartOperation,
        type_value: 5,
        value,
    } = resolve(operation.index())?.kind
    else {
        return None;
    };
    let op = match value {
        2 => BooleanKind::Cut,
        3 => BooleanKind::Join,
        4 => BooleanKind::Intersect,
        _ => return None,
    };
    let PmDcFeaturePropertyKind::Boolean {
        value: keep_tools, ..
    } = resolve(keep_tools.index())?.kind
    else {
        return None;
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Combine {
        operands,
        op,
        keep_tools,
    }))
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let definition = definition(source, |reference| {
        super::resolve_property(source.identity.segment_token.as_str(), reference, index)
    })?;
    let selector = super::slot_property(source, 0, index)?;
    let PmDcFeaturePropertyKind::References { items, .. } = &selector.kind else {
        return None;
    };
    let (id, result) =
        match super::feature_result_members(ctx, &source.identity, selector, items, index)? {
            Ok(result) => result,
            Err(error) => return Some(Err(error)),
        };
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            product_definition: Default::default(),
            id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::default(),
            source_tag: Some("combine".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(definition),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

#[cfg(test)]
mod tests;
