// SPDX-License-Identifier: Apache-2.0
//! Current-model identities of persistent surface-body wrappers.

use std::collections::{HashMap, HashSet};

use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{
    patterns::PatternSeed, BodySelection, CombineOperands, FeatureDefinition, FeatureOperation,
    TrimBodyOperands,
};
use cadmpeg_ir::ids::BodyId;
use serde::{Deserialize, Serialize};

use super::{base, FeatureInventory, PmDcFeatureProperty, PmDcKernelEntityLink};
use crate::design::DesignInventory;
use crate::kernel::ActiveCarrierState;
use crate::native::ActiveCarrierRecord;
use crate::rse::{RseInventory, SegmentDescriptor, SegmentKind, SegmentMetaState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WrapperLink {
    pub(crate) id: String,
    pub(crate) wrapper: String,
    pub(crate) link: String,
    pub(crate) reference: crate::pmdc::PmDcReference,
}

/// The current-carrier segment and the document-scoped segments that hold
/// resolvable external body references.
pub(crate) struct Scoped<'a, 'b> {
    pub(crate) target: &'b SegmentDescriptor<'a>,
    pub(crate) sources: Vec<(&'b SegmentDescriptor<'a>, Vec<u32>)>,
}

pub(crate) fn scoped<'a, 'b>(
    ctx: &DecodeContext<'_>,
    document: &'b RseInventory<'a>,
    design: &DesignInventory,
) -> Result<Option<Scoped<'a, 'b>>, CodecError> {
    let ActiveCarrierState::Selected(carrier) = &document.active_carrier else {
        return Ok(None);
    };
    let mut targets = document.segments.iter().filter(|segment| {
        segment.kind == SegmentKind::PmBRep && segment.pair.token.key() == &carrier.segment_token
    });
    let Some(target) = targets.next() else {
        return Ok(None);
    };
    let SegmentMetaState::Parsed(target_meta) = &target.meta else {
        return Ok(None);
    };
    if targets.next().is_some()
        || document
            .segments
            .iter()
            .filter(|segment| {
                matches!(&segment.meta, SegmentMetaState::Parsed(meta)
                if meta.segment_id == target_meta.segment_id)
            })
            .count()
            != 1
    {
        return Ok(None);
    }
    let mut sources = Vec::new();
    for source in &document.segments {
        let token = source.pair.token.key().as_str();
        let mut roots = design.model_roots.iter().filter(|root| {
            root.identity.segment_token.as_str() == token
                && crate::design::scope::root_role(root, &design.brep_segments)
                    == Some(crate::design::scope::BrepSegmentRole::Document)
        });
        let Some(root) = roots.next() else { continue };
        if roots.next().is_some() || root.brep_segment != target_meta.display_name {
            continue;
        }
        let SegmentMetaState::Parsed(meta) = &source.meta else {
            continue;
        };
        let Some(counts) =
            base::reference_counts(ctx, &meta.tables.sections[7], &meta.tables.sections[9])?
        else {
            continue;
        };
        sources.push((source, counts));
    }
    Ok(Some(Scoped { target, sources }))
}

pub(crate) fn links(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
    inventory: &FeatureInventory,
    design: &DesignInventory,
) -> Result<Vec<WrapperLink>, CodecError> {
    let mut joined = Vec::new();
    let Some(scope) = scoped(ctx, document, design)? else {
        return Ok(joined);
    };
    for (source, counts) in &scope.sources {
        let token = source.pair.token.key().as_str();
        for wrapper in inventory
            .properties
            .iter()
            .filter(|wrapper| wrapper.identity.segment_token.as_str() == token)
        {
            let Some(reference) = base::surface_reference(wrapper) else {
                continue;
            };
            if reference.index() == 0 || reference.qualified() {
                continue;
            }
            if let Some(link) = base::external_link(
                source,
                scope.target,
                reference,
                counts,
                &inventory.kernel_entity_links,
            )? {
                joined.push(WrapperLink {
                    id: format!(
                        "inventor:pmdc:body-wrapper-link#{}-{}",
                        wrapper.identity.segment_token, wrapper.identity.record_ordinal
                    ),
                    wrapper: wrapper.id(),
                    link: link.id(),
                    reference,
                });
            }
        }
    }
    Ok(joined)
}

pub(crate) struct Scope<'a> {
    pub(crate) metadata: &'a [crate::native::SegmentMetaRecord],
    pub(crate) roots: &'a [crate::design::scope::PmDcModelRoot],
}

impl Scope<'_> {
    pub(crate) fn admits(&self, wrapper: &PmDcFeatureProperty, carrier_token: &str) -> bool {
        let mut targets = self
            .metadata
            .iter()
            .filter(|meta| meta.token == carrier_token && meta.kind == "pm_brep");
        let Some(target) = targets.next() else {
            return false;
        };
        if targets.next().is_some()
            || self
                .metadata
                .iter()
                .filter(|meta| meta.segment_id == target.segment_id)
                .count()
                != 1
            || self
                .metadata
                .iter()
                .filter(|meta| {
                    matches!(meta.kind.as_str(), "pm_brep" | "mb_brep")
                        && meta.display_name == target.display_name
                })
                .count()
                != 1
        {
            return false;
        }
        let mut roots = self.roots.iter().filter(|root| {
            root.identity.segment_token == wrapper.identity.segment_token
                && self
                    .metadata
                    .iter()
                    .any(|meta| meta.kind == "pm_brep" && meta.display_name == root.brep_segment)
        });
        roots
            .next()
            .is_some_and(|root| root.brep_segment == target.display_name)
            && roots.next().is_none()
    }
}

pub(crate) fn body_map(
    properties: &[PmDcFeatureProperty],
    links: &[PmDcKernelEntityLink],
    joins: &[WrapperLink],
    carrier: &ActiveCarrierRecord,
    keys: &[BodyNativeKey],
    model: &Model,
    scope: &Scope<'_>,
) -> HashMap<String, BodyId> {
    let mut bodies = HashMap::new();
    let ActiveCarrierRecord::Selected {
        segment_token,
        record_ordinal,
        selected_key,
        ..
    } = carrier
    else {
        return bodies;
    };
    let properties = cadmpeg_core::decode::unique_index(
        properties.iter().map(|wrapper| (wrapper.id(), wrapper)),
    );
    let link_ids = cadmpeg_core::decode::unique_index(links.iter().map(|link| (link.id(), link)));
    let join_ids =
        cadmpeg_core::decode::unique_index(joins.iter().map(|join| (join.wrapper.as_str(), join)));
    let model_bodies =
        cadmpeg_core::decode::unique_index(model.bodies.iter().map(|body| (&body.id, body)));
    for join in joins {
        let Some(wrapper) = properties.get(&join.wrapper).and_then(Option::as_ref) else {
            continue;
        };
        let Some(link) = link_ids.get(&join.link).and_then(Option::as_ref) else {
            continue;
        };
        if base::surface_reference(wrapper) != Some(join.reference)
            || wrapper.identity.type_id.as_str()
                != crate::pmdc::type_id_string(super::SURFACE_BODY_TYPE)
            || link.identity.type_id.as_str()
                != crate::pmdc::type_id_string(super::KERNEL_ENTITY_LINK_TYPE)
            || !scope.admits(wrapper, segment_token)
            || join_ids
                .get(join.wrapper.as_str())
                .is_none_or(Option::is_none)
            || !base::unique_link_header(link, links)
            || crate::pmdc::PmDcReference::from_packed(*selected_key) != link.body
        {
            continue;
        }
        let Some(key) = base::current_body(wrapper, link, segment_token, *record_ordinal, keys)
        else {
            continue;
        };
        if model_bodies.get(&key.body).is_some_and(Option::is_some) {
            bodies.insert(join.wrapper.clone(), key.body.clone());
        }
    }
    bodies
}

fn selection(selection: &mut BodySelection, bodies: &HashMap<String, BodyId>, group: Option<&str>) {
    let (wrappers, native) = match selection {
        BodySelection::Native(native) => (vec![native.clone()], native.clone()),
        BodySelection::NativeSet(members) => {
            let Some(group) = group else { return };
            (
                members.iter().map(|id| id.as_str().to_owned()).collect(),
                group.to_owned(),
            )
        }
        BodySelection::Local { bodies, native } => (
            bodies.iter().map(|id| id.as_str().to_owned()).collect(),
            native.as_str().to_owned(),
        ),
        _ => return,
    };
    let Some(resolved) = wrappers
        .iter()
        .map(|id| bodies.get(id).cloned())
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let Ok(resolved) = resolved.try_into() else {
        return;
    };
    *selection = BodySelection::Resolved {
        bodies: resolved,
        native,
    };
}

pub(crate) fn definition(
    definition: &mut FeatureDefinition,
    bodies: &HashMap<String, BodyId>,
    tools_group: Option<&str>,
) {
    match definition {
        FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) => {
            let (mut target, mut tools) = operands.clone().into_parts();
            selection(&mut target, bodies, None);
            selection(&mut tools, bodies, tools_group);
            if let Ok(resolved) = CombineOperands::new(target, tools) {
                *operands = resolved;
            }
        }
        FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, .. }) => {
            let mut targets = operands.targets().clone();
            let mut tools = operands.tools().clone();
            selection(&mut targets, bodies, None);
            selection(&mut tools, bodies, tools_group);
            if let Ok(resolved) = TrimBodyOperands::new(targets, tools) {
                *operands = resolved;
            }
        }
        FeatureDefinition::Operation(FeatureOperation::MirrorShape { source, .. }) => {
            selection(source, bodies, None);
        }
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
            for seed in seeds {
                if let PatternSeed::Bodies(source) = seed {
                    selection(source, bodies, None);
                }
            }
        }
        FeatureDefinition::Operation(
            FeatureOperation::MoveBody {
                bodies: selected, ..
            }
            | FeatureOperation::SplitBody {
                targets: selected, ..
            }
            | FeatureOperation::TrimBodiesAtPlane {
                targets: selected, ..
            },
        ) => {
            selection(selected, bodies, None);
        }
        _ => {}
    }
}

pub(crate) fn matches(
    mut expected: FeatureDefinition,
    actual: &FeatureDefinition,
    bodies: &HashMap<String, BodyId>,
    tools_group: Option<&str>,
) -> bool {
    definition(&mut expected, bodies, tools_group);
    &expected == actual
}

pub(crate) fn combine_matches(
    mut expected: FeatureDefinition,
    actual: &FeatureDefinition,
    bodies: &HashMap<String, BodyId>,
    tools_group: Option<&str>,
    earlier: &HashSet<String>,
) -> bool {
    definition(&mut expected, bodies, tools_group);
    combine_local(&mut expected, earlier, tools_group);
    &expected == actual
}

pub(crate) fn split_matches(
    mut expected: FeatureDefinition,
    actual: &FeatureDefinition,
    bodies: &HashMap<String, BodyId>,
    earlier: &HashSet<String>,
    qualified_target: bool,
) -> bool {
    definition(&mut expected, bodies, None);
    split_local(&mut expected, earlier, qualified_target);
    &expected == actual
}

pub(crate) struct ProducerRecords<'a> {
    pub(crate) features: &'a [super::PmDcFeature],
    pub(crate) labels: &'a [super::PmDcFeatureLabel],
    pub(crate) properties: &'a [PmDcFeatureProperty],
}

pub(crate) fn produced_before(
    features: &[cadmpeg_ir::features::Feature],
    results: &[cadmpeg_ir::features::FeatureResultTopology],
    feature: &cadmpeg_ir::features::Feature,
    records: &ProducerRecords<'_>,
) -> HashSet<String> {
    if feature.suppressed.is_none() {
        return HashSet::new();
    }
    let producers = features
        .iter()
        .filter(|other| other.ordinal < feature.ordinal && other.suppressed == Some(false))
        .collect::<Vec<_>>();
    let ids = producers
        .iter()
        .map(|other| &other.id)
        .collect::<HashSet<_>>();
    let mut wrappers = results
        .iter()
        .filter(|result| ids.contains(&result.output_of))
        .flat_map(cadmpeg_ir::features::FeatureResultTopology::bodies)
        .map(|wrapper| wrapper.as_str().to_owned())
        .collect::<HashSet<_>>();
    let sources = cadmpeg_core::decode::unique_index(
        records.features.iter().map(|source| (source.id(), source)),
    );
    let properties =
        cadmpeg_core::decode::unique_index(records.properties.iter().map(|property| {
            (
                (
                    property.identity.segment_token.as_str(),
                    property.identity.record_ordinal,
                ),
                property,
            )
        }));
    let labels = cadmpeg_core::decode::unique_index(records.labels.iter().map(|label| {
        (
            (
                label.identity.segment_token.as_str(),
                label.header.owner.record_ordinal(),
            ),
            label,
        )
    }));
    for producer in producers {
        if !matches!(
            producer.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        let Some(source) = producer
            .native_ref
            .as_deref()
            .and_then(|id| sources.get(id))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let token = source.identity.segment_token.as_str();
        let Some(label) = labels
            .get(&(token, Some(source.identity.record_ordinal)))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        if !super::FeatureFamily::Base
            .class_ids()
            .contains(&label.class_id())
        {
            continue;
        }
        if let Some((_, output)) = base::wrappers(source, label.class_id(), |ordinal| {
            properties
                .get(&(token, ordinal))
                .and_then(Option::as_ref)
                .copied()
        }) {
            wrappers.insert(output.id());
        }
    }
    wrappers
}

fn combine_local(
    definition: &mut FeatureDefinition,
    earlier: &HashSet<String>,
    tools_group: Option<&str>,
) {
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = definition
    else {
        return;
    };
    let local = |selection: &mut BodySelection, group: Option<&str>| {
        let (wrappers, native) = match selection {
            BodySelection::Native(wrapper) => (vec![wrapper.clone()], wrapper.clone()),
            BodySelection::NativeSet(members) => {
                let Some(group) = group else { return };
                (
                    members.iter().map(|id| id.as_str().to_owned()).collect(),
                    group.to_owned(),
                )
            }
            _ => return,
        };
        if wrappers.iter().all(|wrapper| earlier.contains(wrapper)) {
            if let Ok(local) = BodySelection::local(wrappers, native) {
                *selection = local;
            }
        }
    };
    let (mut target, mut tools) = operands.clone().into_parts();
    local(&mut target, None);
    local(&mut tools, tools_group);
    if let Ok(retained) = CombineOperands::new(target, tools) {
        *operands = retained;
    }
}

pub(crate) fn qualified_split_target(
    source: &super::PmDcFeature,
    properties: &[PmDcFeatureProperty],
) -> bool {
    let resolve = |reference: crate::pmdc::PmDcReference| {
        let ordinal = reference.record_ordinal()?;
        let mut matches = properties.iter().filter(|property| {
            property.identity.segment_token == source.identity.segment_token
                && property.identity.record_ordinal == ordinal
        });
        let property = matches.next()?;
        matches.next().is_none().then_some(property)
    };
    let Some(reference) = source.properties.references().get(7) else {
        return false;
    };
    if !reference.qualified() {
        return false;
    }
    let Some(selection) = resolve(*reference) else {
        return false;
    };
    let super::PmDcFeaturePropertyKind::References {
        family: super::PmDcFeatureReferenceFamily::BodySelection,
        items,
    } = &selection.kind
    else {
        return false;
    };
    let [wrapper] = items.references() else {
        return false;
    };
    wrapper.qualified()
        && resolve(*wrapper).is_some_and(|property| {
            property.identity.type_id
                == crate::record_identity::RecordTypeId::from_bytes(super::SURFACE_BODY_TYPE)
                && matches!(property.kind, super::PmDcFeaturePropertyKind::SurfaceBody { body }
                    if body.index() != 0 && !body.qualified())
        })
}

fn split_local(
    definition: &mut FeatureDefinition,
    earlier: &HashSet<String>,
    qualified_target: bool,
) {
    let targets = match definition {
        FeatureDefinition::Operation(FeatureOperation::TrimBodiesAtPlane { targets, .. }) => {
            targets
        }
        FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, .. })
            if qualified_target =>
        {
            targets
        }
        FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, .. }) => {
            let mut targets = operands.targets().clone();
            local_split_selection(&mut targets, earlier);
            if let Ok(retained) = TrimBodyOperands::new(targets, operands.tools().clone()) {
                *operands = retained;
            }
            return;
        }
        _ => return,
    };
    local_split_selection(targets, earlier);
}

fn local_split_selection(targets: &mut BodySelection, earlier: &HashSet<String>) {
    let BodySelection::Native(wrapper) = targets else {
        return;
    };
    if earlier.contains(wrapper.as_str()) {
        if let Ok(local) = BodySelection::local(vec![wrapper.clone()], wrapper.clone()) {
            *targets = local;
        }
    }
}

pub(crate) fn tools_group(
    source: &super::PmDcFeature,
    properties: &[PmDcFeatureProperty],
) -> Option<String> {
    let ordinal = source.properties.references().get(1)?.record_ordinal()?;
    let mut matches = properties.iter().filter(|property| {
        property.identity.segment_token == source.identity.segment_token
            && property.identity.record_ordinal == ordinal
    });
    let property = matches.next()?;
    matches.next().is_none().then(|| property.id())
}

pub(crate) fn outputs(
    feature: &cadmpeg_ir::features::FeatureId,
    results: &[cadmpeg_ir::features::FeatureResultTopology],
    bodies: &HashMap<String, BodyId>,
) -> Option<cadmpeg_ir::features::DistinctMembers<BodyId>> {
    let mut matches = results.iter().filter(|result| &result.output_of == feature);
    let result = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(
        result
            .bodies()
            .iter()
            .filter_map(|wrapper| bodies.get(wrapper.as_str()).cloned())
            .collect(),
    )
}

pub(crate) fn outputs_match(
    feature: &cadmpeg_ir::features::Feature,
    results: &[cadmpeg_ir::features::FeatureResultTopology],
    bodies: &HashMap<String, BodyId>,
) -> bool {
    match outputs(&feature.id, results, bodies) {
        Some(expected) => feature.evaluation.outputs().as_slice() == &*expected,
        None => {
            feature.evaluation.outputs().is_empty()
                && !results.iter().any(|result| result.output_of == feature.id)
        }
    }
}

pub(crate) fn project(
    model: &mut Model,
    bodies: &HashMap<String, BodyId>,
    inventory: &FeatureInventory,
) {
    let sources = cadmpeg_core::decode::unique_index(
        inventory
            .features
            .iter()
            .map(|source| (source.id(), source)),
    );
    let features = model.features.clone();
    for feature in &mut model.features {
        let group = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Combine { .. })
        )
        .then(|| {
            feature
                .native_ref
                .as_deref()
                .and_then(|native| sources.get(native))
                .and_then(Option::as_ref)
                .and_then(|source| tools_group(source, &inventory.properties))
        })
        .flatten();
        let earlier = produced_before(
            &features,
            &model.feature_result_topologies,
            feature,
            &ProducerRecords {
                features: &inventory.features,
                labels: &inventory.labels,
                properties: &inventory.properties,
            },
        );
        let qualified_target = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::SplitBody { .. })
        ) && feature
            .native_ref
            .as_deref()
            .and_then(|native| sources.get(native))
            .and_then(Option::as_ref)
            .is_some_and(|source| qualified_split_target(source, &inventory.properties));
        feature.evaluation.edit(|value, _| {
            definition(value, bodies, group.as_deref());
            combine_local(value, &earlier, group.as_deref());
            split_local(value, &earlier, qualified_target);
        });
        if let Some(outputs) = outputs(&feature.id, &model.feature_result_topologies, bodies) {
            feature.evaluation.set_outputs(outputs);
        }
    }
    name_bodies(model, bodies, inventory);
}

/// Names each current body from the feature label that the body's wrapper
/// owns. A body keeps no name unless exactly one uniquely identified
/// surface-body wrapper joins it, exactly one label in the wrapper's
/// segment names the wrapper as its owner, and the stored name is faithful and
/// non-blank.
fn name_bodies(model: &mut Model, bodies: &HashMap<String, BodyId>, inventory: &FeatureInventory) {
    let properties = cadmpeg_core::decode::unique_index(
        inventory
            .properties
            .iter()
            .map(|property| (property.id(), property)),
    );
    let surface_body = crate::pmdc::type_id_string(super::SURFACE_BODY_TYPE);
    // Only uniquely identified surface-body wrappers count; other join keys,
    // such as composite member links, name no wrapper.
    let mut wrappers_of: HashMap<&BodyId, Vec<&super::PmDcFeatureProperty>> = HashMap::new();
    for (key, body) in bodies {
        if let Some(Some(property)) = properties.get(key.as_str()) {
            if property.identity.type_id.as_str() == surface_body {
                wrappers_of.entry(body).or_default().push(property);
            }
        }
    }
    let mut labels: HashMap<(&str, u32), Vec<&super::PmDcFeatureLabel>> = HashMap::new();
    for label in &inventory.labels {
        if let Some(owner) = label.header.owner.record_ordinal() {
            labels
                .entry((label.identity.segment_token.as_str(), owner))
                .or_default()
                .push(label);
        }
    }
    for body in &mut model.bodies {
        if body.name.is_some() {
            continue;
        }
        let Some([property]) = wrappers_of.get(&body.id).map(Vec::as_slice) else {
            continue;
        };
        let key = (
            property.identity.segment_token.as_str(),
            property.identity.record_ordinal,
        );
        let Some([label]) = labels.get(&key).map(Vec::as_slice) else {
            continue;
        };
        if label.name_replaced() {
            continue;
        }
        body.name = label.name().map(str::to_owned);
    }
}

pub(crate) fn native_losses(
    model: &Model,
    bodies: &HashMap<String, BodyId>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let result_count = model
        .feature_result_topologies
        .iter()
        .flat_map(cadmpeg_ir::features::FeatureResultTopology::bodies)
        .filter(|wrapper| !bodies.contains_key(wrapper.as_str()))
        .count();
    let selection_count = model
        .features
        .iter()
        .map(|feature| {
            let native = |selection: &BodySelection| {
                usize::from(!matches!(selection, BodySelection::Resolved { .. }))
            };
            match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) => {
                    let (target, tools) = operands.clone().into_parts();
                    native(&target) + native(&tools)
                }
                FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, .. }) => {
                    native(operands.targets()) + native(operands.tools())
                }
                FeatureDefinition::Operation(
                    FeatureOperation::MoveBody { bodies, .. }
                    | FeatureOperation::SplitBody {
                        targets: bodies, ..
                    }
                    | FeatureOperation::TrimBodiesAtPlane {
                        targets: bodies, ..
                    },
                ) => native(bodies),
                FeatureDefinition::Operation(FeatureOperation::MirrorShape { source, .. }) => {
                    native(source)
                }
                FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => seeds
                    .iter()
                    .map(|seed| match seed {
                        PatternSeed::Bodies(source) => native(source),
                        _ => 0,
                    })
                    .sum(),
                _ => 0,
            }
        })
        .sum::<usize>();
    let owners = cadmpeg_core::decode::unique_index(
        model
            .feature_result_topologies
            .iter()
            .map(|result| (&result.output_of, result)),
    );
    let ambiguous_results = owners.values().filter(|owner| owner.is_none()).count();
    let count = result_count + selection_count + ambiguous_results;
    if count == 0 {
        Vec::new()
    } else {
        vec![crate::loss::InventorLossCode::FeatureBodySelectionNative.note(format!(
            "Retained {count} feature result body wrapper(s) or selection(s) without a complete distinct current-model body join."
        ))]
    }
}

#[cfg(test)]
pub(crate) mod tests;
