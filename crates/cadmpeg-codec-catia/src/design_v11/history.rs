// SPDX-License-Identifier: Apache-2.0
//! The part's bodies, its own group, and their member features (§5.2).

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
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureId, FeatureOperation as Legacy, UnresolvedFamily,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{SketchConstraint as LegacyConstraint, SketchId};
use cadmpeg_ir::CadIr;

use super::tree::{Body, Item, Tree};
use super::{datums, sketches};
use crate::ids::neutral_history_id;
use crate::loss::{identity_statement, CatiaLossCode};
use crate::native::CatiaNative;

/// Push the part's containers and their member features, and return the
/// scope's roots: the part's group first, then its bodies in list order.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    tree: Option<&Tree<'_>>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<ContainerId>, CodecError> {
    let legacy = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.clone(), feature.clone()))
        .collect::<BTreeMap<_, _>>();
    let model_sketches = ir
        .model
        .sketches
        .iter()
        .map(|sketch| {
            (
                sketch.id.clone(),
                (sketch.native_ref.clone(), sketches::frame(sketch)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let model_constraints = ir.model.sketch_constraints.clone();
    let mut stated = Statements {
        ctx,
        legacy: &legacy,
        model_sketches: &model_sketches,
        model_constraints: &model_constraints,
        current: tree.and_then(|tree| tree.current),
        restated: BTreeSet::new(),
        reclassified: Vec::new(),
        features: Vec::new(),
        containers: Vec::new(),
        sketches: Vec::new(),
        constraints: Vec::new(),
    };
    let mut group = BTreeSet::new();
    let mut bodies = Vec::new();
    if let Some(tree) = tree {
        for item in &tree.group {
            group.insert(stated.item(tree.graph_offset, item, Anchor::Current)?);
        }
        for body in &tree.bodies {
            bodies.push(stated.body(tree.graph_offset, body)?);
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
    }
    let mut contradicted = std::mem::take(&mut stated.reclassified);
    let mut ungrouped = Vec::new();
    for feature in legacy.values() {
        if stated.restated.contains(&feature.id) {
            continue;
        }
        if let Some(class) = contradicting_class(native, feature) {
            contradicted.push(format!(
                "{} (record class {class}, not restated)",
                feature.id.as_str()
            ));
            continue;
        }
        ungrouped.push(feature.id.as_str().to_owned());
        group.insert(stated.legacy_feature(feature, Anchor::Current)?);
    }
    if !contradicted.is_empty() {
        losses.push(CatiaLossCode::DesignFeaturesContradicted.note(format!(
            "{} feature(s) read from a record whose compact class contradicts the reading; the reading is not restated: {}",
            contradicted.len(),
            identity_statement(&contradicted),
        )));
    }
    if !ungrouped.is_empty() {
        losses.push(CatiaLossCode::DesignFeaturesUngrouped.note(format!(
            "{} feature(s) that no stated set lists are members of the part's group: {}",
            ungrouped.len(),
            identity_statement(&ungrouped),
        )));
    }
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
    roots.extend(bodies);
    ir.design.features.extend(stated.features);
    ir.design.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.containers.extend(stated.containers);
    ir.design.containers.sort_by(|a, b| a.id().cmp(b.id()));
    ir.design.sketches.extend(stated.sketches);
    ir.design.sketches.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketch_constraints.extend(stated.constraints);
    ir.design.sketch_constraints.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(roots)
}

struct Statements<'c, 'l> {
    ctx: &'c DecodeContext<'c>,
    legacy: &'l BTreeMap<FeatureId, LegacyFeature>,
    /// The native reference and the resolved frame of each sketch the
    /// decoder reads.
    model_sketches: &'l BTreeMap<SketchId, (Option<String>, Option<FrameValue>)>,
    model_constraints: &'l [LegacyConstraint],
    /// The in-work object's entity.
    current: Option<u32>,
    restated: BTreeSet<FeatureId>,
    reclassified: Vec<String>,
    features: Vec<Feature>,
    containers: Vec<Container>,
    sketches: Vec<Sketch>,
    constraints: Vec<SketchConstraint<Planar>>,
}

impl Statements<'_, '_> {
    fn body(&mut self, graph_offset: u64, body: &Body<'_>) -> Result<ContainerId, CodecError> {
        let mut members = Vec::with_capacity(body.members.len());
        let mut in_work = None;
        for item in &body.members {
            if Some(item.entity_id()) == self.current {
                in_work = Some(members.len());
            }
            members.push(self.item(graph_offset, item, Anchor::Input)?);
        }
        let rollback = if Some(body.container.entity_id) == self.current {
            Some(Rollback::End)
        } else {
            in_work.map(|index| {
                members
                    .get(index + 1)
                    .map_or(Rollback::End, |next| Rollback::Before(next.clone()))
            })
        };
        self.ctx.charge_entities(1, "project a CATIA body")?;
        let id = ContainerId::mint(body.container.id.clone()).map_err(CodecError::malformed)?;
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
            Item::Body(body) => return Ok(Member::Container(self.body(graph_offset, body)?)),
            Item::Feature { member, class } => (member, *class),
        };
        let id = member_feature_id(graph_offset, member.entity_id)?;
        if let Some(legacy) = self.legacy.get(&id) {
            if legacy_agrees(legacy, class) {
                return self.legacy_feature(legacy, at);
            }
            self.reclassified.push(format!(
                "{} (record class {class}, restated by it)",
                id.as_str()
            ));
        }
        let native_ref = member
            .target
            .as_ref()
            .map(|target| target.object_record.clone());
        let operation = match datums::by_class(class) {
            Some(operation) => operation,
            None if class == SKETCH_CLASS => {
                let sketch = SketchId::from(neutral_history_id(
                    &crate::native::design_object_id(graph_offset, member.entity_id),
                    &cadmpeg_ir::identity_component!("sketch"),
                )?);
                self.ctx.charge_entities(1, "project a CATIA sketch")?;
                self.sketches.push(sketches::sketch(
                    sketch.clone(),
                    id.clone(),
                    native_ref.clone(),
                    None,
                    at,
                ));
                FeatureOperation::Sketch(sketch)
            }
            None => {
                let kind = CodecName::new(&cadmpeg_ir::codec_id!("catia"), class)
                    .ok_or_else(|| CodecError::malformed("CATIA compact class name is blank"))?;
                FeatureOperation::Native {
                    kind,
                    native_options: BTreeMap::new(),
                    source_settings: BTreeMap::new(),
                }
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
                    self.ctx.charge_entities(1, "project a CATIA sketch")?;
                    self.sketches.push(sketches::sketch(
                        id.clone(),
                        feature.id.clone(),
                        native_ref.clone(),
                        frame.clone(),
                        at,
                    ));
                    for legacy in self
                        .model_constraints
                        .iter()
                        .filter(|item| &item.sketch == id)
                    {
                        if let Some(constraint) = sketches::constraint(legacy)? {
                            self.ctx
                                .charge_entities(1, "project a CATIA sketch constraint")?;
                            self.constraints.push(constraint);
                        }
                    }
                    return Ok(FeatureOperation::Sketch(id.clone()));
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

const SKETCH_CLASS: &str = "PRTSketch";

/// The compact class a legacy reading needs of its record.
fn legacy_class(feature: &LegacyFeature) -> Option<&'static str> {
    match feature.evaluation.definition().operation() {
        Legacy::DatumPrincipalPlane { .. }
        | Legacy::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        } => Some("GSMPlane"),
        Legacy::Sketch { .. } => Some(SKETCH_CLASS),
        _ => None,
    }
}

fn legacy_agrees(feature: &LegacyFeature, class: &str) -> bool {
    legacy_class(feature) == Some(class)
}

/// The compact class of the record a legacy feature reads, where it
/// contradicts the reading.
fn contradicting_class<'n>(native: &'n CatiaNative, feature: &LegacyFeature) -> Option<&'n str> {
    let object = native
        .design_objects
        .iter()
        .find(|object| Some(object.id.as_str()) == feature.native_ref.as_deref())?;
    let class = native
        .entity_records
        .iter()
        .find(|entity| {
            entity.object_graph == object.parent && entity.entity_id == object.owner_entity_id
        })?
        .compact_class
        .as_ref()?;
    (!legacy_agrees(feature, &class.name)).then_some(class.name.as_str())
}
