// SPDX-License-Identifier: Apache-2.0
//! Typed IGES entity accessors and neutral projection.

use std::collections::BTreeSet;

pub(crate) fn directed_cycle(
    sequence: u32,
    visited: &mut BTreeSet<u32>,
    successors: impl Fn(u32) -> Vec<u32>,
) -> bool {
    if visited.contains(&sequence) {
        return false;
    }
    let mut active = BTreeSet::new();
    let mut stack = vec![(sequence, false)];
    while let Some((current, expanded)) = stack.pop() {
        if expanded {
            active.remove(&current);
            visited.insert(current);
            continue;
        }
        if visited.contains(&current) {
            continue;
        }
        if !active.insert(current) {
            return true;
        }
        stack.push((current, true));
        for target in successors(current).into_iter().rev() {
            if active.contains(&target) {
                return true;
            }
            if !visited.contains(&target) {
                stack.push((target, false));
            }
        }
    }
    false
}

pub(crate) mod analytic_surfaces;
pub(crate) mod annotation;
pub(crate) mod brep;
pub(crate) mod composite;
pub(crate) mod conics;
pub(crate) mod copious;
pub(crate) mod csg;
pub(crate) mod curve_conversion;
pub(crate) mod drawing;
pub(crate) mod evaluation;
pub(crate) mod geometry;
pub(crate) mod offsets;
pub(crate) mod presentation;
pub(crate) mod splines;
pub(crate) mod structure;
pub(crate) mod surfaces;
pub(crate) mod trimming;
