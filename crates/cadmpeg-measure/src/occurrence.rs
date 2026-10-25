// SPDX-License-Identifier: Apache-2.0
//! The instance space of one occurrence: its chain from the root, the
//! composed placement of the chain, and the bodies of its subtree.
//!
//! The chain composes as the assembly graph does: each occurrence's
//! effective transform applies inside its parent's, so the placement is
//! `E(root) ∘ … ∘ E(occurrence)`. The subtree composes the same way
//! downward: a child `C` of `O` is placed by `placement(O) ∘ E(C)`. A body
//! that the prototype of a subtree member lists is placed by its own
//! transform first, then by that member's placement.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, ProductDefinition, PrototypeReference};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::canonical::placement::Placement;
use crate::reason::UnavailableReason;
use crate::report::PlacementClass;
use crate::report::{DescendantMeasures, OccurrenceMeasures};
use crate::value::Unavailable;

/// The instance space of an occurrence.
pub(crate) struct Space<'ir> {
    /// The occurrence block of the report.
    pub(crate) report: OccurrenceMeasures,
    /// The composed placement of each body of the subtree; empty when the
    /// space has no rows.
    pub(crate) placements: BTreeMap<&'ir str, Transform>,
    /// The bodies of the subtree.
    pub(crate) bodies: BTreeSet<&'ir str>,
}

impl Space<'_> {
    /// Whether the space places its bodies, so that it has rows.
    pub(crate) const fn placed(&self) -> bool {
        self.report.unavailable.is_none()
    }

    /// Why the space has no rows.
    pub(crate) const fn unavailable(&self) -> Option<&Unavailable<()>> {
        self.report.unavailable.as_ref()
    }
}

/// The occurrences from the root to `occurrence`, or `None` when a parent
/// is missing or the chain repeats.
fn chain<'ir>(
    by_id: &HashMap<&str, &'ir Occurrence>,
    occurrence: &'ir Occurrence,
) -> Option<Vec<&'ir Occurrence>> {
    let mut chain = vec![occurrence];
    let mut current = occurrence;
    while let OccurrenceParent::Occurrence { occurrence: parent } = &current.parent {
        if chain.len() > by_id.len() {
            return None;
        }
        current = by_id.get(parent.as_str())?;
        chain.push(current);
    }
    chain.reverse();
    Some(chain)
}

/// The local product definition an occurrence places.
fn definition<'ir>(
    definitions: &'ir [ProductDefinition],
    occurrence: &Occurrence,
) -> Option<&'ir ProductDefinition> {
    match &occurrence.prototype {
        PrototypeReference::Local { definition } => definitions
            .iter()
            .find(|candidate| candidate.id == *definition),
        PrototypeReference::External { .. } | PrototypeReference::Unresolved {} => None,
    }
}

fn scaled(occurrence: &Occurrence) -> bool {
    occurrence.transform.is_some_and(|transform| {
        Placement::of(transform).is_ok_and(|placement| {
            matches!(
                placement.class,
                PlacementClass::Similarity | PlacementClass::Affine
            )
        })
    })
}

/// The first reason a space has no rows; later failures keep it.
#[derive(Default)]
struct FirstFailure(Option<Unavailable<()>>);

impl FirstFailure {
    fn note(&mut self, reason: UnavailableReason, entity: &str, message: &str) {
        if self.0.is_none() {
            self.0 = Some(Unavailable::new(reason, Some(entity), message));
        }
    }
}

/// One member of the subtree below the occurrence.
struct Member<'ir> {
    occurrence: &'ir Occurrence,
    parent: &'ir str,
    placement: Option<Transform>,
}

/// The instance space of `id`, or `None` when the document has no such
/// occurrence.
pub(crate) fn space<'ir>(ir: &'ir CadIr, id: &OccurrenceId) -> Option<Space<'ir>> {
    let model = &ir.model;
    let definitions = model.product_definitions.as_slice();
    let by_id: HashMap<&str, &'ir Occurrence> = model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.as_str(), occurrence))
        .collect();
    let mut children: HashMap<&str, Vec<&'ir Occurrence>> = HashMap::new();
    for occurrence in &model.occurrences {
        if let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent {
            children
                .entry(parent.as_str())
                .or_default()
                .push(occurrence);
        }
    }
    for siblings in children.values_mut() {
        siblings.sort_by(|left, right| {
            (left.ordinal, left.id.as_str()).cmp(&(right.ordinal, right.id.as_str()))
        });
    }
    let occurrence = *by_id.get(id.as_str())?;
    let name = occurrence.id.as_str();
    let prototype = definition(definitions, occurrence);
    let chain = chain(&by_id, occurrence);
    let mut failure = FirstFailure::default();
    let mut world = None;
    match &chain {
        None => failure.note(
            UnavailableReason::PlacementOccurrenceChainBroken,
            name,
            "the occurrence's parent chain does not reach a root occurrence",
        ),
        Some(chain) => {
            if let Some(scaled) = chain.iter().find(|member| scaled(member)) {
                failure.note(
                    UnavailableReason::PlacementOccurrenceScale,
                    scaled.id.as_str(),
                    "an occurrence of the chain has a scale other than (1, 1, 1)",
                );
            } else if prototype.is_none() {
                failure.note(
                    UnavailableReason::PlacementPrototypeUnresolved,
                    name,
                    "the occurrence's prototype is external or does not resolve",
                );
            }
            let composed = chain
                .iter()
                .try_fold(Some(Transform::identity()), |placed, member| {
                    member
                        .effective_transform()
                        .and_then(|local| match (placed, local) {
                            (Some(placed), Some(local)) => placed.compose(local).map(Some),
                            _ => Ok(None),
                        })
                });
            match composed {
                Ok(Some(composed)) => world = Some(composed),
                Ok(None) => failure.note(
                    UnavailableReason::PlacementOccurrenceUnplaced,
                    chain
                        .iter()
                        .find(|member| member.transform.is_none())
                        .map_or(name, |member| member.id.as_str()),
                    "an occurrence of the chain states no placement",
                ),
                Err(_) => failure.note(
                    UnavailableReason::PlacementNonFinite,
                    name,
                    "the occurrence chain does not compose to a finite placement",
                ),
            }
        }
    }

    let mut placements: BTreeMap<&'ir str, Transform> = BTreeMap::new();
    let mut bodies: BTreeSet<&'ir str> = BTreeSet::new();
    let mut place = |definition: Option<&'ir ProductDefinition>,
                     placement: Option<Transform>,
                     failure: &mut FirstFailure| {
        for body in definition
            .into_iter()
            .flat_map(|definition| &definition.bodies)
        {
            if !bodies.insert(body.as_str()) {
                failure.note(
                    UnavailableReason::PlacementOccurrenceBodyRepeated,
                    body.as_str(),
                    "two occurrences of the subtree place the same body",
                );
            }
            if let Some(placement) = placement {
                placements.insert(body.as_str(), placement);
            }
        }
    };
    place(prototype, world, &mut failure);
    let mut members: Vec<Member<'ir>> = Vec::new();
    let mut pending: VecDeque<(&'ir str, Option<Transform>)> = VecDeque::from([(name, world)]);
    while let Some((parent, placement)) = pending.pop_front() {
        for child in children.get(parent).into_iter().flatten() {
            let child_name = child.id.as_str();
            if scaled(child) {
                failure.note(
                    UnavailableReason::PlacementOccurrenceScale,
                    child_name,
                    "an occurrence of the subtree has a scale other than (1, 1, 1)",
                );
            }
            let child_prototype = definition(definitions, child);
            if child_prototype.is_none() {
                failure.note(
                    UnavailableReason::PlacementPrototypeUnresolved,
                    child_name,
                    "an occurrence of the subtree has an external or unresolved prototype",
                );
            }
            let child_placement = placement.and_then(|placement| {
                match child.effective_transform() {
                    Ok(Some(local)) => placement.compose(local).ok(),
                    Ok(None) => {
                        failure.note(
                            UnavailableReason::PlacementOccurrenceUnplaced,
                            child_name,
                            "an occurrence of the subtree states no placement",
                        );
                        return None;
                    }
                    Err(_) => None,
                }
                .or_else(|| {
                    failure.note(
                        UnavailableReason::PlacementNonFinite,
                        child_name,
                        "the subtree placement does not compose to a finite placement",
                    );
                    None
                })
            });
            place(child_prototype, child_placement, &mut failure);
            members.push(Member {
                occurrence: child,
                parent,
                placement: child_placement,
            });
            pending.push_back((child_name, child_placement));
        }
    }
    if bodies.is_empty() {
        failure.note(
            UnavailableReason::PlacementOccurrenceEmpty,
            name,
            "neither the prototype nor a prototype of the subtree lists a body",
        );
    }
    let unavailable = failure.0;
    let placed = unavailable.is_none();
    if !placed {
        placements.clear();
    }
    let rows =
        |placement: Option<Transform>| placement.filter(|_| placed).map(Transform::affine_rows);
    let descendants = members
        .iter()
        .map(|member| DescendantMeasures {
            occurrence: member.occurrence.id.as_str().to_owned(),
            parent: member.parent.to_owned(),
            prototype: definition(definitions, member.occurrence)
                .map(|definition| definition.id.as_str().to_owned()),
            transform: rows(member.placement),
            bodies: definition(definitions, member.occurrence)
                .map(|definition| {
                    definition
                        .bodies
                        .iter()
                        .map(|body| body.as_str().to_owned())
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    Some(Space {
        report: OccurrenceMeasures {
            occurrence: name.to_owned(),
            prototype: prototype.map(|definition| definition.id.as_str().to_owned()),
            chain: chain
                .unwrap_or_default()
                .iter()
                .map(|member| member.id.as_str().to_owned())
                .collect(),
            transform: rows(world),
            bodies: bodies.iter().map(|body| (*body).to_owned()).collect(),
            descendants,
            unavailable,
        },
        placements,
        bodies,
    })
}
