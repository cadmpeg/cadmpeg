// SPDX-License-Identifier: Apache-2.0
//! Stored-body composites: paired stored source and current result body sets.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, FeatureResultMembers, FeatureResultTopology,
};
use cadmpeg_ir::ids::{BodyId, FeatureResultTopologyId, IdentityKey};
use cadmpeg_ir::report::loss::LossNote;
use serde::{Deserialize, Serialize};

use super::body_selection::Scope;
use super::{
    ClassId, FeatureInventory, PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcKernelEntityLink,
};
use crate::loss::InventorLossCode;
use crate::native::ActiveCarrierRecord;
use crate::pmdc::{type_id_string, PmDcReference};
use crate::rse::RseInventory;

pub(crate) const CLASS_ID: ClassId = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
    0x08, 0x43, 0xc7, 0xc2, 0x24, 0x4c, 0x2d, 0xee, 0x62, 0xb9, 0x5a, 0xb0, 0x11, 0x18, 0xd1, 0x08,
]));

/// The kernel-entity link state of a body selection in a body set.
const SET_SELECTION_STATE: u8 = 3;

pub(crate) const SOURCE_SET_PROPERTY: &str = "source_body_set";
pub(crate) const RESULT_SET_PROPERTY: &str = "result_body_set";
pub(crate) const CONTROL_PROPERTY: &str = "control";
pub(crate) const SOURCE_TAG: &str = "composite";

/// The external-reference join of one result-set member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MemberLink {
    pub(crate) id: String,
    pub(crate) set: String,
    pub(crate) index: u32,
    pub(crate) link: String,
    pub(crate) reference: PmDcReference,
}

/// The native definition of a composite whose body join is open.
pub(crate) fn definition(unique_label: bool) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Native {
        kind: super::native_kind(unique_label.then_some(CLASS_ID)),
        parameters: BTreeMap::new(),
    })
}

/// The three typed slot records of one composite.
pub(crate) struct Operands<'a> {
    pub(crate) source: &'a PmDcFeatureProperty,
    pub(crate) result: &'a PmDcFeatureProperty,
    pub(crate) control: &'a PmDcFeatureProperty,
}

fn local(reference: PmDcReference) -> bool {
    reference.qualified() && reference.index() != 0
}

fn external(reference: PmDcReference) -> bool {
    !reference.qualified() && reference.index() != 0
}

fn set_members(property: &PmDcFeatureProperty) -> Option<(u32, &[PmDcReference])> {
    match &property.kind {
        PmDcFeaturePropertyKind::BodySet { value, items } if items.marker() == 2 => {
            Some((*value, items.references()))
        }
        _ => None,
    }
}

impl Operands<'_> {
    fn source_members(&self) -> &[PmDcReference] {
        set_members(self.source).map_or(&[], |(_, members)| members)
    }

    pub(crate) fn result_members(&self) -> &[PmDcReference] {
        set_members(self.result).map_or(&[], |(_, members)| members)
    }
}

/// Check the property vector and the typed slot records. The first failed
/// check is the refusal.
pub(crate) fn operands<'a>(
    feature: &PmDcFeature,
    mut property: impl FnMut(u32) -> Option<&'a PmDcFeatureProperty>,
) -> Result<Operands<'a>, &'static str> {
    if feature.properties.marker() != 2 {
        return Err("property-vector marker requires the type-2 form");
    }
    let [source, result, control] = feature.properties.references() else {
        return Err("property vector requires three slots");
    };
    for (reference, reason) in [
        (
            source,
            "slot 0 requires a qualified stored-body-set reference",
        ),
        (
            result,
            "slot 1 requires a qualified result-body-set reference",
        ),
        (control, "slot 2 requires a qualified Boolean reference"),
    ] {
        if !local(*reference) {
            return Err(reason);
        }
    }
    let mut typed = |reference: PmDcReference, type_id: [u8; 16]| {
        let ordinal = reference.index() - 1;
        property(ordinal).filter(|record| {
            record.identity.segment_token == feature.identity.segment_token
                && record.identity.record_ordinal == ordinal
                && record.save_version_major == feature.save_version_major
                && record.identity.type_id.as_str() == type_id_string(type_id)
        })
    };
    let Some(source_set) = typed(*source, super::BODY_SET_TYPE) else {
        return Err("slot 0 requires one stored-body-set record");
    };
    let Some(result_set) = typed(*result, super::BODY_SET_TYPE) else {
        return Err("slot 1 requires one result-body-set record");
    };
    let Some(control) = typed(*control, super::BOOLEAN_TYPE) else {
        return Err("slot 2 requires one Boolean record");
    };
    let Some((source_value, sources)) = set_members(source_set) else {
        return Err("slot 0 requires the type-2 body-set form");
    };
    let Some((result_value, results)) = set_members(result_set) else {
        return Err("slot 1 requires the type-2 body-set form");
    };
    if source_value != 0 {
        return Err("slot 0 body-set value requires its construction meaning");
    }
    if result_value != 0 {
        return Err("slot 1 body-set value requires its construction meaning");
    }
    if !matches!(
        &control.kind,
        PmDcFeaturePropertyKind::Boolean { name, name_value: 0, .. } if name.is_empty()
    ) {
        return Err("slot 2 requires an unnamed Boolean");
    }
    if sources.is_empty() || sources.len() != results.len() {
        return Err("slot 0 and slot 1 require equal nonempty member counts");
    }
    if !sources.iter().all(|member| local(*member)) {
        return Err("slot 0 members require qualified references");
    }
    if !results.iter().all(|member| external(*member)) {
        return Err("slot 1 members require unqualified external references");
    }
    Ok(Operands {
        source: source_set,
        result: result_set,
        control,
    })
}

/// The cross-record inputs of the body join.
pub(crate) struct Joins<'a> {
    links: HashMap<(&'a str, u32), Option<&'a PmDcKernelEntityLink>>,
    by_id: HashMap<String, Option<&'a PmDcKernelEntityLink>>,
    all: &'a [PmDcKernelEntityLink],
    members: HashMap<(&'a str, u32), Option<&'a MemberLink>>,
    keys: &'a [BodyNativeKey],
    carrier: &'a ActiveCarrierRecord,
    scope: &'a Scope<'a>,
}

impl<'a> Joins<'a> {
    pub(crate) fn new(
        links: &'a [PmDcKernelEntityLink],
        members: &'a [MemberLink],
        keys: &'a [BodyNativeKey],
        carrier: &'a ActiveCarrierRecord,
        scope: &'a Scope<'a>,
    ) -> Self {
        Self {
            links: unique_index(links.iter().map(|link| {
                (
                    (
                        link.identity.segment_token.as_str(),
                        link.identity.record_ordinal,
                    ),
                    link,
                )
            })),
            by_id: unique_index(links.iter().map(|link| (link.id(), link))),
            all: links,
            members: unique_index(
                members
                    .iter()
                    .map(|member| ((member.set.as_str(), member.index), member)),
            ),
            keys,
            carrier,
            scope,
        }
    }
}

fn current_body<'a>(
    link: &PmDcKernelEntityLink,
    carrier_token: &str,
    carrier_ordinal: u32,
    keys: &'a [BodyNativeKey],
) -> Option<&'a BodyNativeKey> {
    if link.header_value != 0
        || link.state != SET_SELECTION_STATE
        || link.identity.segment_token.as_str() != carrier_token
        || !link.body.qualified()
        || link.body.record_ordinal()? != carrier_ordinal
    {
        return None;
    }
    let source = format!("B{carrier_token}:record:{carrier_ordinal}");
    let mut matches = keys.iter().filter(|key| {
        key.source_brep.as_deref() == Some(source.as_str())
            && key.asm_body_key == Some(u64::from(link.entity_id))
    });
    let key = matches.next()?;
    matches.next().is_none().then_some(key)
}

/// Resolve one composite to its ordered result links and current bodies. The
/// source selections must name stored kernel carriers, the selections must pair
/// by `selection_source`, and every result must join one distinct current body.
pub(crate) fn resolve(
    operands: &Operands<'_>,
    joins: &Joins<'_>,
    stored_carrier: impl Fn(&IdentityKey, u32) -> bool,
    bodies: &[cadmpeg_ir::topology::Body],
    has_body: impl Fn(&BodyId) -> bool,
) -> Result<Vec<(String, BodyId)>, &'static str> {
    let ActiveCarrierRecord::Selected {
        segment_token,
        record_ordinal,
        selected_key,
        ..
    } = joins.carrier
    else {
        return Err("the active carrier requires one selected record");
    };
    let token = &operands.source.identity.segment_token;
    if !joins.scope.admits(operands.result, segment_token) {
        return Err("the current carrier requires one document segment");
    }
    let mut source_keys = Vec::new();
    for member in operands.source_members() {
        let Some(link) = member
            .record_ordinal()
            .and_then(|ordinal| joins.links.get(&(token.as_str(), ordinal)))
            .and_then(|link| *link)
        else {
            return Err("slot 0 member requires one kernel-entity link");
        };
        if link.state != SET_SELECTION_STATE
            || !link.body.qualified()
            || !link
                .body
                .record_ordinal()
                .is_some_and(|ordinal| stored_carrier(token, ordinal))
        {
            return Err("slot 0 member requires a state-3 selection of a stored kernel carrier");
        }
        source_keys.push(link.selection_source);
    }
    let result_id = operands.result.id();
    let mut resolved = Vec::new();
    let mut result_keys = Vec::new();
    for (index, member) in operands.result_members().iter().enumerate() {
        let Ok(index) = u32::try_from(index) else {
            return Err("slot 1 member index exceeds the u32 range");
        };
        let Some(joined) = joins
            .members
            .get(&(result_id.as_str(), index))
            .and_then(|joined| *joined)
            .filter(|joined| joined.reference == *member)
        else {
            return Err("slot 1 member requires one external-reference join");
        };
        let Some(link) = joins.by_id.get(&joined.link).and_then(|link| *link) else {
            return Err("slot 1 member requires one kernel-entity link");
        };
        if !super::base::unique_link_header(link, joins.all)
            || PmDcReference::from_packed(*selected_key) != link.body
        {
            return Err("slot 1 member requires a unique link on the current carrier");
        }
        let Some(key) = current_body(link, segment_token, *record_ordinal, joins.keys) else {
            return Err("slot 1 member requires one state-3 current body key");
        };
        let Some(body) = bodies
            .iter()
            .find(|candidate| key.owns_native_ref(candidate.native_ref.as_deref()))
        else {
            return Err("slot 1 member requires a current model body");
        };
        if !has_body(&body.id) {
            return Err("slot 1 member requires a current model body");
        }
        result_keys.push(link.selection_source);
        resolved.push((link.id(), body.id.clone()));
    }
    let distinct = |values: &[u32]| values.iter().collect::<HashSet<_>>().len() == values.len();
    if !distinct(&source_keys) || !distinct(&result_keys) || source_keys != result_keys {
        return Err("slot 0 and slot 1 members require equal distinct selection sources");
    }
    if resolved
        .iter()
        .map(|(_, body)| body)
        .collect::<HashSet<_>>()
        .len()
        != resolved.len()
    {
        return Err("slot 1 members require distinct current bodies");
    }
    Ok(resolved)
}

pub(crate) fn unique_owner(labels: &[super::PmDcFeatureLabel], feature: &PmDcFeature) -> bool {
    labels
        .iter()
        .filter(|label| {
            label.identity.segment_token == feature.identity.segment_token
                && label.header.owner.record_ordinal() == Some(feature.identity.record_ordinal)
        })
        .take(2)
        .count()
        == 1
}

/// Join the external result references of every composite with their links.
pub(crate) fn links(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
    inventory: &FeatureInventory,
    design: &crate::design::DesignInventory,
) -> Result<Vec<MemberLink>, CodecError> {
    let mut joined = Vec::new();
    if !inventory
        .labels
        .iter()
        .any(|label| label.class_id() == CLASS_ID)
    {
        return Ok(joined);
    }
    let Some(scope) = super::body_selection::scoped(ctx, document, design)? else {
        return Ok(joined);
    };
    let properties = unique_index(inventory.properties.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    for label in inventory
        .labels
        .iter()
        .filter(|label| label.class_id() == CLASS_ID)
    {
        let token = label.identity.segment_token.as_str();
        let Some(feature) = label.header.owner.record_ordinal().and_then(|ordinal| {
            let mut matches = inventory.features.iter().filter(|feature| {
                feature.identity.segment_token.as_str() == token
                    && feature.identity.record_ordinal == ordinal
            });
            let feature = matches.next()?;
            matches.next().is_none().then_some(feature)
        }) else {
            continue;
        };
        if !unique_owner(&inventory.labels, feature) {
            continue;
        }
        let Ok(operands) = operands(feature, |ordinal| {
            properties.get(&(token, ordinal)).and_then(|record| *record)
        }) else {
            continue;
        };
        let Some((source, counts)) = scope
            .sources
            .iter()
            .find(|(source, _)| source.pair.token.key().as_str() == token)
        else {
            continue;
        };
        for (index, reference) in operands.result_members().iter().enumerate() {
            let index = u32::try_from(index)
                .map_err(|_| CodecError::malformed("Inventor composite member index overflow"))?;
            if let Some(link) = super::base::external_link(
                source,
                scope.target,
                *reference,
                counts,
                &inventory.kernel_entity_links,
            )? {
                joined.push(MemberLink {
                    id: format!(
                        "inventor:pmdc:composite-member#{}-{}-{index}",
                        operands.result.identity.segment_token,
                        operands.result.identity.record_ordinal
                    ),
                    set: operands.result.id(),
                    index,
                    link: link.id(),
                    reference: *reference,
                });
            }
        }
    }
    Ok(joined)
}

/// The result of the composite projection.
pub(crate) struct Projection {
    pub(crate) transferred: usize,
    pub(crate) losses: Vec<LossNote>,
    /// Result link identifiers with the current bodies they join.
    pub(crate) bodies: HashMap<String, BodyId>,
}

/// Project every composite whose body join resolves as a stored-geometry
/// feature with its result bodies. Any other composite stays native with the
/// first refused check in its loss.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &FeatureInventory,
    members: &[MemberLink],
    carrier: &ActiveCarrierRecord,
    scope: &Scope<'_>,
    keys: &[BodyNativeKey],
    model: &mut Model,
) -> Result<Projection, CodecError> {
    let mut projection = Projection {
        transferred: 0,
        losses: Vec::new(),
        bodies: HashMap::new(),
    };
    if !inventory
        .labels
        .iter()
        .any(|label| label.class_id() == CLASS_ID)
    {
        return Ok(projection);
    }
    let features = unique_index(inventory.features.iter().map(|feature| {
        (
            (
                feature.identity.segment_token.as_str(),
                feature.identity.record_ordinal,
            ),
            feature,
        )
    }));
    let properties = unique_index(inventory.properties.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let joins = Joins::new(
        &inventory.kernel_entity_links,
        members,
        keys,
        carrier,
        scope,
    );
    let carrier_type = type_id_string(crate::kernel::KERNEL_RECORD_TYPE_ID);
    let positions = model
        .features
        .iter()
        .enumerate()
        .filter_map(|(position, feature)| Some((feature.native_ref.clone()?, position)))
        .collect::<HashMap<_, _>>();
    let model_bodies = model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<HashSet<_>>();
    let mut owners = HashSet::new();
    let mut counts = BTreeMap::new();
    for label in inventory
        .labels
        .iter()
        .filter(|label| label.class_id() == CLASS_ID)
    {
        let token = label.identity.segment_token.as_str();
        let owner = label
            .header
            .owner
            .record_ordinal()
            .ok_or(label.identity.record_ordinal);
        if !owners.insert((token, owner)) {
            continue;
        }
        let Some(feature) = label
            .header
            .owner
            .record_ordinal()
            .and_then(|ordinal| features.get(&(token, ordinal)))
            .and_then(Option::as_ref)
            .copied()
        else {
            *counts
                .entry("label owner requires one typed feature record")
                .or_insert(0_usize) += 1;
            continue;
        };
        let resolved = if unique_owner(&inventory.labels, feature) {
            operands(feature, |ordinal| {
                properties.get(&(token, ordinal)).and_then(|record| *record)
            })
            .and_then(|operands| {
                resolve(
                    &operands,
                    &joins,
                    |segment, ordinal| {
                        inventory
                            .record_types
                            .get(segment)
                            .and_then(|types| types.get(&ordinal))
                            .and_then(Option::as_deref)
                            == Some(carrier_type.as_str())
                    },
                    &model.bodies,
                    |body| model_bodies.contains(body),
                )
                .map(|bodies| (operands, bodies))
            })
        } else {
            Err("label owner requires one feature label")
        };
        let (operands, bodies) = match resolved {
            Ok(resolved) => resolved,
            Err(reason) => {
                *counts.entry(reason).or_insert(0_usize) += 1;
                continue;
            }
        };
        let Some(&position) = positions.get(&feature.id()) else {
            continue;
        };
        let target = &mut model.features[position];
        if !matches!(
            target.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        let ids = bodies
            .iter()
            .map(|(link, _)| cadmpeg_core::text::NonBlankString::new(link.clone()))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| CodecError::malformed("Inventor composite result link is blank"))?;
        let result_members = FeatureResultMembers::new(ids, Vec::new(), Vec::new(), Vec::new())
            .map_err(CodecError::malformed)?;
        ctx.charge_entities(1, "project Inventor composite result identity")?;
        let owner_id = target.id.clone();
        for (key, value) in [
            (
                cadmpeg_core::nonblank_literal!("source_body_set"),
                operands.source.id(),
            ),
            (
                cadmpeg_core::nonblank_literal!("result_body_set"),
                operands.result.id(),
            ),
            (
                cadmpeg_core::nonblank_literal!("control"),
                operands.control.id(),
            ),
        ] {
            target.source_properties.insert(key, value);
        }
        target.source_tag = Some(SOURCE_TAG.into());
        target
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            ));
        model
            .feature_result_topologies
            .push(FeatureResultTopology::new(
                FeatureResultTopologyId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                    feature.identity.key()?,
                ),
                owner_id,
                result_members,
                Some(operands.result.id()),
            ));
        projection.bodies.extend(bodies);
        projection.transferred += 1;
    }
    projection.losses = counts
        .into_iter()
        .map(|(reason, count)| {
            InventorLossCode::FeatureCompositeOperandsNative.note(format!(
                "{count} Composite feature(s) remain native: {reason}."
            ))
        })
        .collect();
    Ok(projection)
}

#[cfg(test)]
pub(crate) mod tests;
