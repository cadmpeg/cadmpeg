// SPDX-License-Identifier: Apache-2.0
//! The world instances of a document's bodies and their comparison.
//!
//! An instance is a path of occurrences from a root definition. Its
//! placement is `T(o₁) ∘ … ∘ T(oₙ)`. `T(oⱼ)` is the transform of `oⱼ`, the
//! identity where it states none, replaced by the transform a nested
//! override of an occurrence above it states for the path from that
//! occurrence to `oⱼ`; where two occurrences above it state one, the outer
//! one holds (`docs/cadir-v11.md` §8.1). Suppression resolves the same way,
//! and a suppressed instance places nothing below it. An instance shows the
//! bodies of its prototype, or the bodies that a definition above it lists
//! for its path in `instance_bodies`, the outermost such definition first. A
//! root definition's bodies, and a body that no definition lists, are placed
//! once at the identity.

use crate::{fast_map_with_capacity, FastMap};
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::selection::{OccurrencePath, StepRef};
use cadmpeg_ir::products::{Occurrence, ProductDefinition, Storage};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::{Operand, Row, Rule};
use crate::value::Unit;

/// Largest number of instances one document's comparison enumerates.
const MAX_COMPARE_INSTANCES: usize = 1 << 20;

/// The placements of each body of a document, by body ID, or `None` when an
/// occurrence chain repeats, a placement does not compose to a finite
/// transform, or the instances exceed [`MAX_COMPARE_INSTANCES`].
pub(super) fn world_instances(ir: &CadIr) -> Option<BTreeMap<&str, Vec<Transform>>> {
    let model = &ir.model;
    let mut definitions: FastMap<&str, &ProductDefinition> =
        fast_map_with_capacity(model.product_definitions.len());
    for definition in model
        .product_definitions
        .iter()
        .filter(|definition| !matches!(definition.storage, Storage::External(_)))
    {
        definitions.insert(definition.id.as_str(), definition);
    }
    let mut occurrences: FastMap<&str, &Occurrence> =
        fast_map_with_capacity(model.occurrences.len());
    for occurrence in &model.occurrences {
        occurrences.insert(occurrence.id.as_str(), occurrence);
    }
    let bodies: BTreeSet<&str> = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    let mut placed: BTreeMap<&str, Vec<Transform>> = BTreeMap::new();
    let mut count = 0_usize;
    // A body the geometry does not hold places nothing.
    let mut place = |body: &'_ str, transform: Transform| -> Option<()> {
        let Some(&key) = bodies.get(body) else {
            return Some(());
        };
        count += 1;
        (count <= MAX_COMPARE_INSTANCES).then_some(())?;
        placed.entry(key).or_default().push(transform);
        Some(())
    };
    let listed: BTreeSet<&str> = model
        .product_definitions
        .iter()
        .flat_map(|definition| {
            definition.bodies.iter().chain(
                definition
                    .instance_bodies
                    .iter()
                    .flat_map(|instance| &instance.bodies),
            )
        })
        .map(cadmpeg_ir::ids::BodyId::as_str)
        .collect();
    for body in &ir.geometry.bodies {
        if !listed.contains(body.id.as_str()) {
            place(body.id.as_str(), Transform::identity())?;
        }
    }
    for root in model
        .product_definitions
        .iter()
        .filter(|definition| definition.is_root())
    {
        for body in &root.bodies {
            place(body.as_str(), Transform::identity())?;
        }
        // Each entry: the path of occurrences from `root`, and the composed
        // placement of the last one.
        let mut pending: Vec<(Vec<&Occurrence>, Transform)> = owned(root, &occurrences)
            .into_iter()
            .map(|occurrence| (vec![occurrence], Transform::identity()))
            .collect();
        while let Some((path, above)) = pending.pop() {
            if path.len() > model.occurrences.len() {
                return None;
            }
            if suppressed(&path) {
                continue;
            }
            let last = path[path.len() - 1];
            let placement = above.compose(transform(&path)).ok()?;
            let Some(prototype) = definitions.get(last.prototype.as_str()) else {
                continue;
            };
            let shown = shown_bodies(root, &path, &definitions).unwrap_or(&prototype.bodies);
            for body in shown {
                place(body.as_str(), placement)?;
            }
            for child in owned(prototype, &occurrences) {
                let mut child_path = path.clone();
                child_path.push(child);
                pending.push((child_path, placement));
            }
        }
    }
    Some(placed)
}

/// The occurrences `definition` owns, in its order.
fn owned<'ir>(
    definition: &'ir ProductDefinition,
    occurrences: &FastMap<&str, &'ir Occurrence>,
) -> Vec<&'ir Occurrence> {
    definition
        .occurrences
        .iter()
        .filter_map(|id| occurrences.get(id.as_str()).copied())
        .collect()
}

/// Whether `path` names the occurrences `members`, in order.
fn names(path: &OccurrencePath, members: &[&Occurrence]) -> bool {
    path.0.len() == members.len()
        && path.0.iter().zip(members).all(|(step, member)| {
            matches!(&step.occurrence, StepRef::Occurrence(id) if id == &member.id)
        })
}

/// The transform of the last occurrence of `path`, resolved through the
/// nested overrides of the occurrences above it.
fn transform(path: &[&Occurrence]) -> Transform {
    let last = path.len() - 1;
    (0..last)
        .find_map(|above| {
            path[above]
                .overrides
                .iter()
                .find(|entry| names(&entry.path, &path[above + 1..]))
                .and_then(|entry| entry.transform)
        })
        .or(path[last].transform)
        .unwrap_or_default()
}

/// Whether the last occurrence of `path` is suppressed, resolved through
/// the nested overrides of the occurrences above it.
fn suppressed(path: &[&Occurrence]) -> bool {
    let last = path.len() - 1;
    (0..last)
        .find_map(|above| {
            path[above]
                .overrides
                .iter()
                .find(|entry| names(&entry.path, &path[above + 1..]))
                .and_then(|entry| entry.suppressed)
        })
        .or(path[last].suppressed)
        .unwrap_or(false)
}

/// The bodies that `root` or a prototype above the last occurrence of
/// `path` lists for that instance, the outermost first.
fn shown_bodies<'ir>(
    root: &'ir ProductDefinition,
    path: &[&'ir Occurrence],
    definitions: &FastMap<&str, &'ir ProductDefinition>,
) -> Option<&'ir Vec<cadmpeg_ir::ids::BodyId>> {
    let owners = std::iter::once(Some(root)).chain(
        path[..path.len() - 1]
            .iter()
            .map(|occurrence| definitions.get(occurrence.prototype.as_str()).copied()),
    );
    owners.enumerate().find_map(|(depth, owner)| {
        owner?
            .instance_bodies
            .iter()
            .find(|instance| names(&instance.occurrence, &path[depth..]))
            .map(|instance| &instance.bodies)
    })
}

/// The distance by which `b` moves a point of a box of diagonal `scale`
/// away from where `a` places it, bounded by the translation difference
/// plus `scale` times the largest difference of a linear coefficient.
fn deviation(a: Transform, b: Transform, scale: f64) -> f64 {
    let (a, b) = (a.affine_rows(), b.affine_rows());
    let mut translation = 0.0_f64;
    let mut linear = 0.0_f64;
    for row in 0..3 {
        let shift = a[row][3] - b[row][3];
        translation += shift * shift;
        for column in 0..3 {
            linear = linear.max((a[row][column] - b[row][column]).abs());
        }
    }
    translation.sqrt() + scale * linear
}

/// The instance rows of two documents whose bodies `pairs` pairs: the
/// instance count of the paired bodies, and whether each pair's instances
/// in one document match its instances in the other one to one within
/// `tolerance`.
pub(super) fn instance_rows(
    a: &BTreeMap<&str, Vec<Transform>>,
    b: &BTreeMap<&str, Vec<Transform>>,
    pairs: &[(&str, &str)],
    scale: f64,
    tolerance: f64,
) -> Vec<Row> {
    let empty = Vec::new();
    let mut counts = [0_usize; 2];
    let mut matched = true;
    let mut largest = 0.0_f64;
    for (body_a, body_b) in pairs {
        let first = a.get(body_a).unwrap_or(&empty);
        let second = b.get(body_b).unwrap_or(&empty);
        counts[0] += first.len();
        counts[1] += second.len();
        if first.len() != second.len() {
            matched = false;
            continue;
        }
        let mut free: Vec<Transform> = second.clone();
        for placement in first {
            let nearest = free
                .iter()
                .enumerate()
                .map(|(at, other)| (at, deviation(*placement, *other, scale)))
                .min_by(|left, right| left.1.total_cmp(&right.1));
            if let Some((at, distance)) = nearest {
                largest = largest.max(distance);
                free.swap_remove(at);
            }
        }
    }
    let agrees = matched && largest <= tolerance;
    vec![
        Row {
            quantity: "instances".to_owned(),
            unit: None,
            a: Some(Operand::Count(counts[0])),
            b: Some(Operand::Count(counts[1])),
            difference: None,
            tolerance: None,
            rule: Rule::Exact,
            agrees: Some(counts[0] == counts[1]),
            cause: None,
        },
        Row {
            quantity: "instance_placement".to_owned(),
            unit: Some(Unit::Millimeter),
            a: None,
            b: None,
            difference: matched.then_some(largest),
            tolerance: Some(tolerance),
            rule: Rule::Equal,
            agrees: Some(agrees),
            cause: None,
        },
    ]
}

#[cfg(test)]
mod tests;
