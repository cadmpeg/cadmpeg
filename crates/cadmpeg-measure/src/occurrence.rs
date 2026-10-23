// SPDX-License-Identifier: Apache-2.0
//! The instance space of one occurrence: its chain from the root, the
//! composed placement of the chain, and the bodies of its prototype.
//!
//! The chain composes as the assembly graph does: each occurrence's
//! effective transform applies inside its parent's, so the placement is
//! `E(root) ∘ … ∘ E(occurrence)`. A body of the prototype is placed by its
//! own transform first, then by that placement.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::reason::UnavailableReason;
use crate::report::OccurrenceMeasures;
use crate::value::Unavailable;

/// The instance space of an occurrence.
pub(crate) struct Space<'ir> {
    /// The occurrence block of the report.
    pub(crate) report: OccurrenceMeasures,
    /// The composed placement of the chain; absent when the space has no
    /// rows.
    pub(crate) world: Option<Transform>,
    /// The bodies of the prototype.
    pub(crate) bodies: BTreeSet<&'ir str>,
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

/// The instance space of `id`, or `None` when the document has no such
/// occurrence.
pub(crate) fn space<'ir>(ir: &'ir CadIr, id: &OccurrenceId) -> Option<Space<'ir>> {
    let model = &ir.model;
    let by_id: HashMap<&str, &'ir Occurrence> = model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.as_str(), occurrence))
        .collect();
    let occurrence = *by_id.get(id.as_str())?;
    let name = occurrence.id.as_str();
    let definition = match &occurrence.prototype {
        PrototypeReference::Local { definition } => model
            .product_definitions
            .iter()
            .find(|candidate| candidate.id == *definition),
        PrototypeReference::External { .. } | PrototypeReference::Unresolved {} => None,
    };
    let bodies: BTreeSet<&'ir str> = definition
        .map(|definition| {
            definition
                .bodies
                .iter()
                .map(cadmpeg_ir::ids::BodyId::as_str)
                .collect()
        })
        .unwrap_or_default();
    let chain = chain(&by_id, occurrence);
    let missing = |reason: UnavailableReason, entity: &str, message: &str| {
        Some(Unavailable::new(reason, Some(entity), message))
    };
    let mut world = None;
    let unavailable = match &chain {
        None => missing(
            UnavailableReason::PlacementOccurrenceChainBroken,
            name,
            "the occurrence's parent chain does not reach a root occurrence",
        ),
        Some(chain) => {
            let scaled = chain
                .iter()
                .find(|member| member.scale.iter().any(|factor| factor.get() != 1.0));
            if let Some(scaled) = scaled {
                missing(
                    UnavailableReason::PlacementOccurrenceScale,
                    scaled.id.as_str(),
                    "an occurrence of the chain has a scale other than (1, 1, 1)",
                )
            } else if definition.is_none() {
                missing(
                    UnavailableReason::PlacementPrototypeUnresolved,
                    name,
                    "the occurrence's prototype is external or does not resolve",
                )
            } else {
                let composed = chain
                    .iter()
                    .try_fold(Transform::identity(), |placed, member| {
                        member
                            .effective_transform()
                            .and_then(|local| placed.compose(local))
                    });
                match composed {
                    Ok(composed) => {
                        world = Some(composed);
                        None
                    }
                    Err(_) => missing(
                        UnavailableReason::PlacementNonFinite,
                        name,
                        "the occurrence chain does not compose to a finite placement",
                    ),
                }
            }
        }
    };
    Some(Space {
        report: OccurrenceMeasures {
            occurrence: name.to_owned(),
            prototype: definition.map(|definition| definition.id.as_str().to_owned()),
            chain: chain
                .unwrap_or_default()
                .iter()
                .map(|member| member.id.as_str().to_owned())
                .collect(),
            transform: world.map(Transform::affine_rows),
            bodies: bodies.iter().map(|body| (*body).to_owned()).collect(),
            unavailable,
        },
        world,
        bodies,
    })
}
