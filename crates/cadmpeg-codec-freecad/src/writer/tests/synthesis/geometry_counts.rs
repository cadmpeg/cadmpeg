// SPDX-License-Identifier: Apache-2.0
//! Bounded evidence for extra geometry emitted by synthesis.
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[derive(Default, Debug, PartialEq)]
struct Counts {
    curves: BTreeMap<String, usize>,
    surfaces: BTreeMap<String, usize>,
    curve_bases: BTreeMap<usize, usize>,
    surface_bases: BTreeMap<usize, usize>,
}

fn tags(value: &serde_json::Value, output: &mut Vec<String>) {
    if let Some(fields) = value.as_object() {
        if let Some(kind) = fields.get("kind").and_then(serde_json::Value::as_str) {
            output.push(kind.to_owned());
        }
        for child in fields.values().filter(|child| child.is_object()) {
            tags(child, output);
        }
    }
}

fn kinds(value: &impl serde::Serialize) -> String {
    let mut output = Vec::new();
    tags(&serde_json::to_value(value).unwrap(), &mut output);
    output.join("/")
}

fn counts(ir: &CadIr) -> BTreeMap<String, Counts> {
    let native = ir.native.namespace("fcstd").unwrap();
    let payloads = crate::brep::records::payloads_untracked(native).unwrap();
    let properties = native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let objects = native
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let homes = payloads
        .iter()
        .filter_map(|payload| {
            let property = properties
                .iter()
                .find(|property| property.id == payload.property)?;
            let owner = objects
                .iter()
                .find(|object| object.id().as_str() == property.owner)?;
            Some((
                payload.id.as_str(),
                format!("{}:{}", owner.name(), property.name),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let group = |home: Option<&str>| {
        home.and_then(|id| homes.get(id))
            .cloned()
            .unwrap_or_else(|| "other".to_owned())
    };
    let mut output = BTreeMap::<String, Counts>::new();
    for curve in &ir.model.curves {
        let counts = output
            .entry(group(curve.native_ref.as_deref()))
            .or_default();
        *counts.curves.entry(kinds(&curve.geometry)).or_default() += 1;
        if let Some(SolvedCurveGeometry::Transformed(placed)) = curve.geometry.solved() {
            let matches = ir
                .model
                .curves
                .iter()
                .filter(|candidate| {
                    candidate.id != curve.id
                        && candidate.native_ref == curve.native_ref
                        && candidate.geometry.solved() == Some(placed.basis())
                })
                .count();
            *counts.curve_bases.entry(matches).or_default() += 1;
        }
    }
    for surface in &ir.model.surfaces {
        let counts = output
            .entry(group(surface.native_ref.as_deref()))
            .or_default();
        *counts.surfaces.entry(kinds(&surface.geometry)).or_default() += 1;
        if let Some(SolvedSurfaceGeometry::Transformed(placed)) = surface.geometry.solved() {
            let matches = ir
                .model
                .surfaces
                .iter()
                .filter(|candidate| {
                    candidate.id != surface.id
                        && candidate.native_ref == surface.native_ref
                        && candidate.geometry.solved() == Some(placed.basis())
                })
                .count();
            *counts.surface_bases.entry(matches).or_default() += 1;
        }
    }
    output
}

pub(super) fn diagnose(before: &CadIr, after: &CadIr) {
    if before.model.curves.len() == after.model.curves.len()
        && before.model.surfaces.len() == after.model.surfaces.len()
    {
        return;
    }
    let a = counts(before);
    let b = counts(after);
    for (home, counts) in a
        .iter()
        .filter(|(home, value)| b.get(*home) != Some(*value))
        .take(6)
    {
        eprintln!(
            "synthesis geometry {home}: before={counts:?}; after={:?}",
            b.get(home)
        );
    }
}
