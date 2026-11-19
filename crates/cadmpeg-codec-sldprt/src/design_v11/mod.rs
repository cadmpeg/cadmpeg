// SPDX-License-Identifier: Apache-2.0
//! The CADIR v11 design layer of a part (part 5 of the CADIR v11
//! specification), projected from the native Keywords feature history beside
//! the v10 design records.
//!
//! The part is one design scope over its root definition, which owns the
//! bodies of the current geometry. The feature tree is one ordered timeline
//! whose current state, at its rollback position (`docs/formats/sldprt.md`
//! §3.4), holds those bodies; the scope holds the part's parameters and
//! configurations. A part whose history holds no feature, or that has no
//! history the decoder reads, states no design.

mod configurations;
mod operation;
mod parameters;
#[cfg(test)]
mod tests;
mod tree;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;
use crate::records::{FeatureHistory, FeatureInputLane};
use crate::rollback::{Reading, Unread};

/// Project the design of a part document into `ir.design`, beside its v10
/// records, and state the part's root definition.
///
/// `lanes` are the `ResolvedFeatures` lanes whose scalars type the v10
/// parameters. `bodies` are the bodies of the current geometry: those of the
/// active configuration. `read_rollback` reads the stored marks; a reading that
/// does not fix the position adds its reason to `losses`. Returns the number
/// of records added.
pub(crate) fn project(
    ir: &mut CadIr,
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    bodies: Vec<BodyId>,
    read_rollback: impl FnOnce() -> Result<Reading, CodecError>,
    losses: &mut Vec<LossNote>,
) -> Result<usize, CodecError> {
    let timeline = tree::timeline(histories)?;
    if timeline.features.is_empty() {
        return Ok(0);
    }
    let definition = root_definition();
    let decoded = parameters::Decoded::new(&ir.model.parameters, lanes);
    let parameters = parameters::parameters(histories, &timeline, &definition, &decoded)?;
    let configurations =
        configurations::configurations(histories, ir.source.as_ref(), &definition)?;
    let id = container_id(cadmpeg_ir::identity_key!("timeline"));
    let members = timeline.members();
    let built = match stated(&timeline.features, read_rollback()?) {
        Ok(built) => built,
        Err(unread) => {
            losses.push(SldprtLossCode::HistoryRollbackUnread.note(format!(
                "The rollback position of the feature tree is not read ({unread}); the timeline \
                 is stated built through its last member."
            )));
            members.len()
        }
    };
    let (rollback, at) = built_through(&id, &members, built);
    let current = (!bodies.is_empty())
        .then(|| current_state(at, bodies.clone()))
        .into_iter()
        .collect();
    let container = Container::Ordered {
        id,
        kind: OrderedKind::Timeline,
        members,
        folders: timeline.folders,
        rollback,
        placement: None,
    };
    let features = timeline.features;
    let added = 3 + features.len() + parameters.len() + configurations.records.len();
    let mut root = ProductDefinition::root(definition.clone(), ExternalDocument::default());
    root.kind = Some(ProductDefinitionKind::Part);
    root.bodies = bodies;
    ir.model.product_definitions.push(root);
    ir.design.scopes.push(DesignScope {
        definition,
        roots: vec![container.id().clone()],
        active_configuration: configurations.active,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current,
    });
    ir.design.containers.push(container);
    ir.design.features.extend(features);
    ir.design.parameters.extend(parameters);
    ir.design.configurations.extend(configurations.records);
    Ok(added)
}

/// The number of leading timeline features the stored marks state built:
/// all of them at `End`, or through the rolled-back feature.
fn stated(features: &[Feature], reading: Reading) -> Result<usize, Unread> {
    let (after, later) = match reading {
        Reading::End => return Ok(features.len()),
        Reading::Unread(unread) => return Err(unread),
        Reading::RolledBack { after, later } => (after, later),
    };
    let index = |name: String| {
        let found = {
            let mut named = features
                .iter()
                .enumerate()
                .filter(|(_, feature)| feature.name.as_deref() == Some(name.as_str()));
            match (named.next(), named.next()) {
                (Some((index, _)), None) => Some(index),
                _ => None,
            }
        };
        found.ok_or(Unread::FeatureUnresolved { name })
    };
    let rolled_back = index(after)?;
    let mut built = features.len();
    for name in later {
        let position = index(name.clone())?;
        if position <= rolled_back {
            return Err(Unread::FeatureOutOfOrder { name });
        }
        if !matches!(
            features[position].suppressed,
            Some(Dimension::Literal(true))
        ) {
            built = rolled_back + 1;
        }
    }
    Ok(built)
}

/// The rollback of a timeline `id` whose first `built` members are built, and
/// the position of its current state.
fn built_through(id: &ContainerId, members: &[Member], built: usize) -> (Rollback, Position) {
    let at = built
        .checked_sub(1)
        .and_then(|last| members.get(last))
        .cloned()
        .map_or_else(|| Position::Start(id.clone()), Position::After);
    let rollback = members
        .get(built)
        .cloned()
        .map_or(Rollback::End, Rollback::Before);
    (rollback, at)
}

/// The root definition: the part file itself, which no occurrence places.
fn root_definition() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    )
}

fn container_id(key: impl Into<cadmpeg_ir::ids::IdentityKey>) -> ContainerId {
    ContainerId::from(cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "model", "container"),
        key,
    ))
}

fn current_state(at: Position, bodies: Vec<BodyId>) -> CurrentState {
    CurrentState {
        at,
        bodies,
        poses: BTreeMap::new(),
        status: EvaluationStatus::Current,
        native_ref: None,
    }
}
