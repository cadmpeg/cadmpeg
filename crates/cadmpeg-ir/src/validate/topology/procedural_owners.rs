// SPDX-License-Identifier: Apache-2.0
//! Each procedural surface of the geometry is the `construction` of exactly
//! one of its surfaces, and each procedural curve of exactly one of its
//! curves (part 10, Topology and model records).

use std::collections::BTreeMap;

use crate::document::CadIr;
use crate::report::{check::Check, check::Finding, Severity};
use crate::validate::record_finding;

pub(in crate::validate) fn check_procedural_owners(ir: &CadIr, findings: &mut Vec<Finding>) {
    let geometry = &ir.geometry;
    let mut surfaces: BTreeMap<&str, usize> = geometry
        .procedural_surfaces
        .iter()
        .map(|procedural| (procedural.id.as_str(), 0))
        .collect();
    for surface in &geometry.surfaces {
        if let Some(construction) = surface.geometry.procedural_construction() {
            if let Some(count) = surfaces.get_mut(construction.as_str()) {
                *count += 1;
            }
        }
    }
    let mut curves: BTreeMap<&str, usize> = geometry
        .procedural_curves
        .iter()
        .map(|procedural| (procedural.id.as_str(), 0))
        .collect();
    for curve in &geometry.curves {
        if let Some(construction) = curve.geometry.procedural_construction() {
            if let Some(count) = curves.get_mut(construction.as_str()) {
                *count += 1;
            }
        }
    }
    let counted = surfaces
        .into_iter()
        .map(|(id, count)| (id, count, "surface"))
        .chain(curves.into_iter().map(|(id, count)| (id, count, "curve")));
    for (id, count, kind) in counted {
        if count != 1 {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(id),
                format_args!(
                    "procedural {kind} is the construction of {count} {kind}s; it is the construction of exactly one"
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
