// SPDX-License-Identifier: Apache-2.0
//! The instance space of one occurrence: its chain from the root, the
//! composed placement of the chain, and the bodies of its subtree.
//!
//! The chain climbs from the occurrence to the definition that owns it, then
//! to the one occurrence that places that definition, until it reaches the
//! root definition. Each occurrence's transform applies inside the one above
//! it, so the placement is `T(top) ∘ … ∘ T(occurrence)`. The subtree composes
//! the same way downward: an occurrence `C` that the prototype of `O` owns is
//! placed by `placement(O) ∘ T(C)`. `T` of an instance below the chain's
//! top-level occurrence is the transform an override of that occurrence
//! states for its path, where one does (§8.1). A body that the prototype of a
//! subtree member lists is placed by its own transform first, then by that
//! member's placement.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use crate::{fast_map_with_capacity, FastMap};

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, ProductDefinition, Storage};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::reason::UnavailableReason;
use crate::report::{DescendantMeasures, OccurrenceMeasures};
use crate::tolerance::COMPARE_OCCURRENCE_SCALE_DEVIATION;
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

/// The occurrences from the root to `occurrence` through `above`, or `None`
/// when a listed occurrence does not place the owner of the next, an owning
/// definition above the first is placed by no occurrence or by several, or
/// the chain repeats.
fn chain<'ir>(
    ir: &'ir CadIr,
    above: &[&'ir Occurrence],
    occurrence: &'ir Occurrence,
) -> Option<Vec<&'ir Occurrence>> {
    let model = &ir.model;
    let mut listed: Vec<&'ir Occurrence> = above.to_vec();
    listed.push(occurrence);
    for (index, entry) in listed.iter().enumerate() {
        if listed[..index].iter().any(|earlier| earlier.id == entry.id) {
            return None;
        }
    }
    for pair in listed.windows(2) {
        let owner = cadmpeg_ir::products::owner(&model.product_definitions, &pair[1].id)?;
        if owner.id != pair[0].prototype {
            return None;
        }
    }
    let first = listed[0];
    let mut chain = vec![first];
    let mut current = first;
    loop {
        let owner = cadmpeg_ir::products::owner(&model.product_definitions, &current.id)?;
        if owner.is_root() {
            break;
        }
        if chain.len() > model.occurrences.len() {
            return None;
        }
        current = cadmpeg_ir::products::sole_placement(&model.occurrences, &owner.id)?;
        chain.push(current);
    }
    chain.reverse();
    chain.extend(listed.into_iter().skip(1));
    Some(chain)
}

/// The product definition an occurrence places, where the document holds it.
fn definition<'ir>(
    definitions: &'ir [ProductDefinition],
    occurrence: &Occurrence,
) -> Option<&'ir ProductDefinition> {
    definitions.iter().find(|candidate| {
        candidate.id == occurrence.prototype && !matches!(candidate.storage, Storage::External(_))
    })
}

/// The transform of an occurrence; one that states none places it as is.
fn transform(occurrence: &Occurrence) -> Transform {
    occurrence.transform.unwrap_or_default()
}

/// The base transform of the instance of `occurrence` at `path` below the
/// top-level occurrence `top`, or the one an override of `top` states for
/// that path (§8.1).
fn instance_transform(
    top: Option<&Occurrence>,
    path: &[&str],
    occurrence: &Occurrence,
) -> Transform {
    use cadmpeg_ir::design::selection::StepRef;
    top.filter(|_| !path.is_empty())
        .and_then(|top| {
            top.overrides.iter().find(|entry| {
                entry.path.0.len() == path.len()
                    && entry.path.0.iter().zip(path).all(|(step, id)| {
                        matches!(&step.occurrence, StepRef::Occurrence(step) if step.as_str() == *id)
                    })
            })
        })
        .and_then(|entry| entry.transform)
        .unwrap_or_else(|| transform(occurrence))
}

/// Whether a transform scales an axis.
fn scaled(transform: Transform) -> bool {
    let rows = transform.affine_rows();
    (0..3).any(|column| {
        let length = rows
            .iter()
            .map(|row| row[column] * row[column])
            .sum::<f64>()
            .sqrt();
        (length - 1.0).abs() > COMPARE_OCCURRENCE_SCALE_DEVIATION
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

/// The instance space of `id` reached through `above` (outermost first), or
/// `None` when the document has no such occurrence or holds no occurrence
/// of `above`.
pub(crate) fn space<'ir>(
    ir: &'ir CadIr,
    above: &[OccurrenceId],
    id: &OccurrenceId,
) -> Option<Space<'ir>> {
    let model = &ir.model;
    let definitions = model.product_definitions.as_slice();
    let mut by_id: FastMap<&str, &'ir Occurrence> =
        fast_map_with_capacity(model.occurrences.len());
    for occurrence in &model.occurrences {
        by_id.insert(occurrence.id.as_str(), occurrence);
    }
    // The occurrences each definition owns, in its order.
    let mut owned: FastMap<&str, Vec<&'ir Occurrence>> =
        fast_map_with_capacity(definitions.len());
    for definition in definitions {
        owned.insert(
            definition.id.as_str(),
            definition
                .occurrences
                .iter()
                .filter_map(|child| by_id.get(child.as_str()).copied())
                .collect(),
        );
    }
    let occurrence = *by_id.get(id.as_str())?;
    let above = above
        .iter()
        .map(|id| by_id.get(id.as_str()).copied())
        .collect::<Option<Vec<_>>>()?;
    let name = occurrence.id.as_str();
    let prototype = definition(definitions, occurrence);
    let chain = chain(ir, &above, occurrence);
    let mut failure = FirstFailure::default();
    let mut world = None;
    let top = chain.as_ref().and_then(|chain| chain.first().copied());
    let below_top = chain
        .as_ref()
        .map(|chain| {
            chain
                .iter()
                .skip(1)
                .map(|member| member.id.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let chain_transforms = chain
        .as_ref()
        .map(|chain| {
            chain
                .iter()
                .enumerate()
                .map(|(at, member)| instance_transform(top, &below_top[..at], member))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match &chain {
        None => failure.note(
            UnavailableReason::PlacementOccurrenceChainBroken,
            name,
            "the occurrence's chain does not reach the root definition",
        ),
        Some(chain) => {
            if let Some((scaled, _)) = chain
                .iter()
                .zip(&chain_transforms)
                .find(|(_, transform)| scaled(**transform))
            {
                failure.note(
                    UnavailableReason::PlacementOccurrenceScale,
                    scaled.id.as_str(),
                    "an occurrence of the chain has a transform that scales",
                );
            } else if prototype.is_none() {
                failure.note(
                    UnavailableReason::PlacementPrototypeUnresolved,
                    name,
                    "the occurrence's prototype is external or does not resolve",
                );
            }
            let composed = chain_transforms
                .iter()
                .try_fold(Transform::identity(), |placed, member| {
                    placed.compose(*member)
                });
            match composed {
                Ok(composed) => world = Some(composed),
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
    let mut pending: VecDeque<(&'ir Occurrence, Option<Transform>, usize, Vec<&'ir str>)> =
        VecDeque::from([(occurrence, world, 0, below_top)]);
    while let Some((parent_occurrence, placement, depth, path)) = pending.pop_front() {
        let parent = parent_occurrence.id.as_str();
        let children = owned
            .get(parent_occurrence.prototype.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        if depth > model.occurrences.len() && !children.is_empty() {
            failure.note(
                UnavailableReason::PlacementOccurrenceChainBroken,
                parent,
                "the subtree's definitions place one another",
            );
            break;
        }
        for &child in children {
            let child_name = child.id.as_str();
            let mut child_path = path.clone();
            child_path.push(child_name);
            let child_transform = instance_transform(top, &child_path, child);
            if scaled(child_transform) {
                failure.note(
                    UnavailableReason::PlacementOccurrenceScale,
                    child_name,
                    "an occurrence of the subtree has a transform that scales",
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
                placement
                    .compose(child_transform)
                    .map_err(|_| {
                        failure.note(
                            UnavailableReason::PlacementNonFinite,
                            child_name,
                            "the subtree placement does not compose to a finite placement",
                        );
                    })
                    .ok()
            });
            place(child_prototype, child_placement, &mut failure);
            members.push(Member {
                occurrence: child,
                parent,
                placement: child_placement,
            });
            pending.push_back((child, child_placement, depth + 1, child_path));
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
