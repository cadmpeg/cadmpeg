// SPDX-License-Identifier: Apache-2.0
//! The part's bodies, its sets, its own group, and their member features
//! (§5.2).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::FrameValue;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::ops::sketch::{Planar, Sketch, SketchConstraint};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::Anchor;
use cadmpeg_ir::features::{Feature as LegacyFeature, FeatureId, FeatureOperation as Legacy};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{SketchConstraint as LegacyConstraint, SketchId};
use cadmpeg_ir::CadIr;

use super::tree::{Item, Set, SetKind, Tree};
use super::{classes, datums, sketches};
use crate::ids::neutral_history_id;
use crate::loss::{identity_statement, CatiaLossCode};

/// Push the part's containers and their member features, and return the
/// scope's roots: the part's group first, then its sets in list order.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    tree: Option<&Tree<'_>>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<ContainerId>, CodecError> {
    let legacy = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<BTreeMap<_, _>>();
    let model_sketches = ir
        .model
        .sketches
        .iter()
        .map(|sketch| (&sketch.id, (&sketch.native_ref, sketches::frame(sketch))))
        .collect::<BTreeMap<_, _>>();
    let mut stated = Statements {
        ctx,
        legacy: &legacy,
        model_sketches: &model_sketches,
        model_constraints: &ir.model.sketch_constraints,
        current: tree.and_then(|tree| tree.current),
        rollback_stated: false,
        restated: BTreeSet::new(),
        features: Vec::new(),
        containers: Vec::new(),
        sketches: Vec::new(),
        constraints: Vec::new(),
        charges: Charges::default(),
    };
    let mut group = BTreeSet::new();
    let mut sets = Vec::new();
    if let Some(tree) = tree {
        for item in &tree.group {
            group.insert(stated.item(tree.graph_offset, item, Anchor::Current)?);
        }
        for set in &tree.sets {
            sets.push(stated.set(tree.graph_offset, set)?);
        }
        if !tree.unstated.is_empty() {
            let records = tree
                .unstated
                .iter()
                .map(|record| format!("{} ({})", record.record, record.reason))
                .collect::<Vec<_>>();
            losses.push(CatiaLossCode::DesignTreeRecordsUnstated.note(format!(
                "{} record(s) of the part's feature tree, and the members they list, are not stated: {}",
                records.len(),
                identity_statement(&records),
            )));
        }
        if let (Some(current), false) = (tree.current, stated.rollback_stated) {
            stated.charges.rollbacks.push(format!(
                "the in-work object, entity {current}, is no member of a stated body and is no stated body"
            ));
        }
    }
    let mut ungrouped = Vec::new();
    for feature in legacy.values() {
        if stated.restated.contains(&feature.id) {
            continue;
        }
        ungrouped.push(feature.id.as_str().to_owned());
        group.insert(stated.legacy_feature(feature, Anchor::Current)?);
    }
    if !ungrouped.is_empty() {
        losses.push(CatiaLossCode::DesignFeaturesUngrouped.note(format!(
            "{} feature(s) that no stated set lists are members of the part's group: {}",
            ungrouped.len(),
            identity_statement(&ungrouped),
        )));
    }
    stated.charges.push_to(losses);
    let mut roots = Vec::new();
    if !group.is_empty() {
        ctx.charge_entities(1, "project the CATIA part group")?;
        let key = tree.map_or_else(
            || "root".to_owned(),
            |tree| format!("{:010}-{:010}", tree.graph_offset, tree.root.entity_id),
        );
        let id = ContainerId::mint(format!("catia:outer:feature-group#{key}"))
            .map_err(CodecError::malformed)?;
        roots.push(id.clone());
        stated.containers.push(Container::Unordered {
            id,
            kind: UnorderedKind::Group,
            members: group,
        });
    }
    roots.extend(sets);
    let Statements {
        features,
        containers,
        sketches,
        constraints,
        ..
    } = stated;
    ir.design.features.extend(features);
    ir.design.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.containers.extend(containers);
    ir.design.containers.sort_by(|a, b| a.id().cmp(b.id()));
    ir.design.sketches.extend(sketches);
    ir.design.sketches.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketch_constraints.extend(constraints);
    ir.design.sketch_constraints.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(roots)
}

/// What the statements leave unread, one loss each.
#[derive(Default)]
struct Charges {
    /// Members kept in place whose record is unread.
    members: Vec<String>,
    /// Sets stated unordered because their kind is unread.
    sets: Vec<String>,
    /// In-work objects no stated rollback holds, and stated rollbacks
    /// before an unread member.
    rollbacks: Vec<String>,
    /// Sketches stated without their entities.
    sketches: Vec<String>,
    /// Sketch constraints of a kind the projection does not restate.
    constraints: Vec<String>,
}

impl Charges {
    fn push_to(self, losses: &mut Vec<LossNote>) {
        for (records, code, statement) in [
            (
                self.members,
                CatiaLossCode::DesignMembersUnread,
                "member(s) of the part's feature tree whose record is unread are stated as native placeholder features",
            ),
            (
                self.sets,
                CatiaLossCode::DesignSetKindUnread,
                "set(s) whose kind (body, ordered geometrical set, or geometrical set) is unread are stated as unordered geometrical sets",
            ),
            (
                self.rollbacks,
                CatiaLossCode::DesignRollbackUnread,
                "in-work object position(s) are not stated by a rollback on a read member",
            ),
            (
                self.sketches,
                CatiaLossCode::DesignSketchEntitiesUnread,
                "sketch(es) are stated without their entities, which the decoder does not read",
            ),
            (
                self.constraints,
                CatiaLossCode::DesignSketchConstraintsUnstated,
                "sketch constraint(s) of a kind other than native are not restated",
            ),
        ] {
            if !records.is_empty() {
                losses.push(code.note(format!(
                    "{} {statement}: {}",
                    records.len(),
                    identity_statement(&records),
                )));
            }
        }
    }
}

struct Statements<'c, 'l> {
    ctx: &'c DecodeContext<'c>,
    legacy: &'l BTreeMap<&'l FeatureId, &'l LegacyFeature>,
    /// The native reference and the resolved frame of each sketch the
    /// decoder reads.
    model_sketches: &'l BTreeMap<&'l SketchId, (&'l Option<String>, Option<FrameValue>)>,
    model_constraints: &'l [LegacyConstraint],
    /// The in-work object's entity.
    current: Option<u32>,
    /// Whether a stated body's rollback holds the in-work object.
    rollback_stated: bool,
    restated: BTreeSet<FeatureId>,
    features: Vec<Feature>,
    containers: Vec<Container>,
    sketches: Vec<Sketch>,
    constraints: Vec<SketchConstraint<Planar>>,
    charges: Charges,
}

impl Statements<'_, '_> {
    fn set(&mut self, graph_offset: u64, set: &Set<'_>) -> Result<ContainerId, CodecError> {
        let id = ContainerId::mint(set.container.id.clone()).map_err(CodecError::malformed)?;
        if set.kind == SetKind::KindUnread {
            let mut members = BTreeSet::new();
            for item in &set.members {
                members.insert(self.item(graph_offset, item, Anchor::Current)?);
            }
            self.charges.sets.push(format!(
                "{} ({})",
                set.container.object_record, set.container.class.name
            ));
            self.ctx
                .charge_entities(1, "project a CATIA geometrical set")?;
            self.containers.push(Container::Unordered {
                id: id.clone(),
                kind: UnorderedKind::GeometricalSet,
                members,
            });
            return Ok(id);
        }
        let mut members = Vec::with_capacity(set.members.len());
        let mut in_work = None;
        for item in &set.members {
            if Some(item.entity_id()) == self.current {
                in_work = Some(members.len());
            }
            members.push(self.item(graph_offset, item, Anchor::Input)?);
        }
        let rollback = if Some(set.container.entity_id) == self.current {
            Some(Rollback::End)
        } else {
            in_work.map(
                |index| match (set.members.get(index + 1), members.get(index + 1)) {
                    (Some(next), Some(member)) => {
                        if let Item::Unread { member: listed, .. } = next {
                            self.charges.rollbacks.push(format!(
                                "{} is before entity {}, whose record is unread",
                                set.container.object_record, listed.entity_id
                            ));
                        }
                        Rollback::Before(member.clone())
                    }
                    _ => Rollback::End,
                },
            )
        };
        self.rollback_stated |= rollback.is_some();
        self.ctx.charge_entities(1, "project a CATIA body")?;
        self.containers.push(Container::Ordered {
            id: id.clone(),
            kind: OrderedKind::Body,
            members,
            folders: Vec::new(),
            rollback,
            placement: None,
        });
        Ok(id)
    }

    /// The member for one listed item; `at` anchors the selections of a
    /// feature in the container that lists it (§5.8).
    fn item(
        &mut self,
        graph_offset: u64,
        item: &Item<'_>,
        at: Anchor,
    ) -> Result<Member, CodecError> {
        let (member, class) = match item {
            Item::Set(set) => return Ok(Member::Container(self.set(graph_offset, set)?)),
            Item::Feature { member, class } => (member, Some(*class)),
            Item::Unread { member, .. } => (member, None),
        };
        let id = member_feature_id(graph_offset, member.entity_id)?;
        if let Some(legacy) = self.legacy.get(&id) {
            return self.legacy_feature(legacy, at);
        }
        let native_ref = member
            .target
            .as_ref()
            .map(|target| target.object_record.clone());
        let operation = match class.map(|class| (class, classes::family(class))) {
            Some((_, Some(family))) => FeatureOperation::Unresolved { family },
            Some((class, None)) => native_operation(class)?,
            None => {
                if let Item::Unread { reason, .. } = item {
                    self.charges.members.push(format!(
                        "{} ({reason})",
                        native_ref
                            .clone()
                            .unwrap_or_else(|| format!("entity {}", member.entity_id))
                    ));
                }
                native_operation(PLACEHOLDER_CLASS)?
            }
        };
        self.ctx.charge_entities(1, "project a CATIA feature")?;
        self.restated.insert(id.clone());
        self.features
            .push(feature(id.clone(), operation, native_ref));
        Ok(Member::Feature(id))
    }

    fn legacy_feature(&mut self, legacy: &LegacyFeature, at: Anchor) -> Result<Member, CodecError> {
        self.ctx.charge_entities(1, "project a CATIA feature")?;
        self.restated.insert(legacy.id.clone());
        let operation = self.legacy_operation(legacy, at)?;
        self.features.push(feature(
            legacy.id.clone(),
            operation,
            legacy.native_ref.clone(),
        ));
        Ok(Member::Feature(legacy.id.clone()))
    }

    /// The design operation of a feature the decoder types, and the sketch
    /// and constraints a sketch feature owns.
    fn legacy_operation(
        &mut self,
        feature: &LegacyFeature,
        at: Anchor,
    ) -> Result<FeatureOperation, CodecError> {
        let family = match feature.evaluation.definition().operation() {
            Legacy::Sketch { sketch } => {
                if let Some((id, (native_ref, frame))) = sketch
                    .id()
                    .and_then(|id| self.model_sketches.get_key_value(id))
                {
                    let id = (*id).clone();
                    self.ctx.charge_entities(1, "project a CATIA sketch")?;
                    self.sketches.push(sketches::sketch(
                        id.clone(),
                        feature.id.clone(),
                        (*native_ref).clone(),
                        frame.clone(),
                        at,
                    ));
                    self.charges.sketches.push(id.as_str().to_owned());
                    for legacy in self
                        .model_constraints
                        .iter()
                        .filter(|item| item.sketch == id)
                    {
                        match sketches::constraint(legacy)? {
                            Some(constraint) => {
                                self.ctx
                                    .charge_entities(1, "project a CATIA sketch constraint")?;
                                self.constraints.push(constraint);
                            }
                            None => self.charges.constraints.push(legacy.id.as_str().to_owned()),
                        }
                    }
                    return Ok(FeatureOperation::Sketch(id));
                }
                OperationFamily::Sketch
            }
            Legacy::DatumPrincipalPlane { plane } => {
                if let Some(operation) = datums::principal(*plane, at) {
                    return Ok(operation);
                }
                OperationFamily::Datum
            }
            _ => OperationFamily::Datum,
        };
        Ok(FeatureOperation::Unresolved { family })
    }
}

/// The class of the native placeholder for a member whose record is unread.
const PLACEHOLDER_CLASS: &str = "feature";

fn native_operation(class: &str) -> Result<FeatureOperation, CodecError> {
    let kind = CodecName::new(&cadmpeg_ir::codec_id!("catia"), class)
        .ok_or_else(|| CodecError::malformed("CATIA compact class name is blank"))?;
    Ok(FeatureOperation::Native {
        kind,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })
}

fn feature(id: FeatureId, operation: FeatureOperation, native_ref: Option<String>) -> Feature {
    Feature {
        id,
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation,
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref,
    }
}

/// The feature identity of a listed record: the identity the decoder gives
/// the design object of the record's entity.
pub(super) fn member_feature_id(
    graph_offset: u64,
    entity_id: u32,
) -> Result<FeatureId, CodecError> {
    neutral_history_id(
        &crate::native::design_object_id(graph_offset, entity_id),
        &cadmpeg_ir::identity_component!("feature"),
    )
    .map(FeatureId::from)
}
