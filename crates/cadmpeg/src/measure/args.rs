// SPDX-License-Identifier: Apache-2.0
//! `measure` selectors and their resolution against a loaded document.

use anyhow::anyhow;
use cadmpeg_ir::CadIr;
use cadmpeg_measure::report::MeasureReport;
use cadmpeg_measure::{FaceFilter, RadiusRange, Selection, SurfaceKind};
use clap::{Args, ValueEnum};

use crate::application::refusal::ApplicationError;
use crate::selection::{ambiguous_message, miss_id_message, resolve_one, ResolveError};

/// A canonical surface kind on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum KindArg {
    /// A plane.
    Plane,
    /// A circular cylinder.
    Cylinder,
    /// A cone.
    Cone,
    /// A sphere.
    Sphere,
    /// A torus.
    Torus,
    /// A NURBS surface.
    Nurbs,
    /// A polygonal surface.
    Polygonal,
    /// A procedural surface.
    Procedural,
    /// An unknown surface.
    Unknown,
}

impl From<KindArg> for SurfaceKind {
    fn from(kind: KindArg) -> Self {
        match kind {
            KindArg::Plane => Self::Plane,
            KindArg::Cylinder => Self::Cylinder,
            KindArg::Cone => Self::Cone,
            KindArg::Sphere => Self::Sphere,
            KindArg::Torus => Self::Torus,
            KindArg::Nurbs => Self::Nurbs,
            KindArg::Polygonal => Self::Polygonal,
            KindArg::Procedural => Self::Procedural,
            KindArg::Unknown => Self::Unknown,
        }
    }
}

/// A radius range: `MIN..MAX`, `MIN..`, `..MAX`, or `R`.
#[derive(Debug, Clone)]
pub(crate) struct RadiusArg {
    text: String,
    range: RadiusRange,
}

fn radius_bound(text: &str) -> Result<Option<cadmpeg_ir::scalar::NonNegativeLength>, String> {
    if text.is_empty() {
        return Ok(None);
    }
    let value: f64 = text
        .parse()
        .map_err(|_| format!("{text:?} is not a number"))?;
    cadmpeg_ir::scalar::NonNegativeLength::new(value)
        .map(Some)
        .ok_or_else(|| format!("{text:?} is not a finite non-negative length"))
}

fn parse_radius(text: &str) -> Result<RadiusArg, String> {
    let (min, max) = match text.split_once("..") {
        Some((min, max)) => (radius_bound(min)?, radius_bound(max)?),
        None => {
            let value = radius_bound(text)?;
            if value.is_none() {
                return Err("a radius range needs a bound".to_owned());
            }
            (value, value)
        }
    };
    if min.is_none() && max.is_none() {
        return Err("a radius range needs a bound".to_owned());
    }
    let range = RadiusRange::new(min, max).ok_or("MIN must not exceed MAX")?;
    Ok(RadiusArg {
        text: text.to_owned(),
        range,
    })
}

/// What `measure` measures.
#[derive(Debug, Args)]
pub(crate) struct SelectionArgs {
    /// Measure only these bodies: an exact ID or a unique ID suffix. Repeatable.
    #[arg(long = "body", value_name = "ID")]
    bodies: Vec<String>,
    /// Print a row for this face: an exact ID or a unique ID suffix. Repeatable.
    #[arg(long = "face", value_name = "ID")]
    faces: Vec<String>,
    /// Print a row for this edge: an exact ID or a unique ID suffix. Repeatable.
    #[arg(long = "edge", value_name = "ID")]
    edges: Vec<String>,
    /// Print a row for every face in scope.
    #[arg(long = "faces")]
    all_faces: bool,
    /// Print a row for every edge in scope.
    #[arg(long = "edges")]
    all_edges: bool,
    /// Keep face rows of these canonical surface kinds.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "KIND")]
    kind: Vec<KindArg>,
    /// Keep face rows whose radius lies in RANGE: `MIN..MAX`, `MIN..`,
    /// `..MAX`, or `R`. The radius is a cylinder's radius, a sphere's radius
    /// or a torus's tube radius; other faces never match.
    #[arg(long, value_parser = parse_radius, value_name = "RANGE")]
    radius: Option<RadiusArg>,
}

/// The one table `--tsv` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Level {
    Bodies,
    Faces,
    Edges,
}

impl SelectionArgs {
    /// Whether a face selector or filter is given.
    pub(super) fn face_level(&self) -> bool {
        !self.faces.is_empty() || self.all_faces || !self.kind.is_empty() || self.radius.is_some()
    }

    /// Whether an edge selector is given.
    pub(super) fn edge_level(&self) -> bool {
        !self.edges.is_empty() || self.all_edges
    }

    pub(super) fn level(&self) -> Level {
        if self.face_level() {
            Level::Faces
        } else if self.edge_level() {
            Level::Edges
        } else {
            Level::Bodies
        }
    }

    /// Resolve every selector against `ir`. Every unresolved or ambiguous
    /// selector is reported before the command exits.
    pub(super) fn resolve(&self, ir: &CadIr) -> Result<Selection, ApplicationError> {
        let mut errors = Vec::new();
        let bodies = resolve_all(
            &self.bodies,
            "model.bodies",
            &ir.model.bodies.iter().map(|body| &body.id),
            cadmpeg_ir::ids::BodyId::as_str,
            &mut errors,
        );
        let faces = resolve_all(
            &self.faces,
            "model.faces",
            &ir.model.faces.iter().map(|face| &face.id),
            cadmpeg_ir::ids::FaceId::as_str,
            &mut errors,
        );
        let edges = resolve_all(
            &self.edges,
            "model.edges",
            &ir.model.edges.iter().map(|edge| &edge.id),
            cadmpeg_ir::ids::EdgeId::as_str,
            &mut errors,
        );
        if !errors.is_empty() {
            return Err(anyhow!("{}", errors.join("\n")).into());
        }
        Ok(Selection {
            bodies,
            faces,
            edges,
            all_faces: self.all_faces,
            all_edges: self.all_edges,
            face_filter: FaceFilter {
                kinds: self.kind.iter().copied().map(SurfaceKind::from).collect(),
                radius: self.radius.as_ref().map(|radius| radius.range),
            },
        })
    }

    /// The note printed when a face or edge selection matches no row.
    pub(super) fn empty_note(&self, report: &MeasureReport) -> Option<String> {
        if self.face_level() && report.faces().is_empty() {
            let mut filters = Vec::new();
            if !self.kind.is_empty() {
                let kinds: Vec<&str> = self
                    .kind
                    .iter()
                    .map(|kind| SurfaceKind::from(*kind).as_str())
                    .collect();
                filters.push(format!("--kind {}", kinds.join(",")));
            }
            if let Some(radius) = &self.radius {
                filters.push(format!("--radius {}", radius.text));
            }
            return Some(if filters.is_empty() {
                "note: no face matches the selection".to_owned()
            } else {
                format!("note: no face matches {}", filters.join(" "))
            });
        }
        if self.edge_level() && report.edges().is_empty() {
            return Some("note: no edge matches the selection".to_owned());
        }
        None
    }
}

/// Resolve each request against the arena's IDs, recording each failure.
fn resolve_all<'a, T: Clone + 'a>(
    requests: &[String],
    arena: &str,
    ids: &(impl Iterator<Item = &'a T> + Clone),
    name: fn(&T) -> &str,
    errors: &mut Vec<String>,
) -> Vec<T> {
    let mut resolved = Vec::new();
    for request in requests {
        let indexed = ids
            .clone()
            .enumerate()
            .map(|(index, id)| (Some(name(id)), index));
        match resolve_one(request, indexed) {
            Ok(index) => {
                if let Some(id) = ids.clone().nth(index) {
                    if !resolved
                        .iter()
                        .any(|existing: &T| name(existing) == name(id))
                    {
                        resolved.push(id.clone());
                    }
                }
            }
            Err(ResolveError::Ambiguous(matches)) => {
                errors.push(ambiguous_message(request, arena, &matches));
            }
            Err(ResolveError::Missing) => {
                let all: Vec<String> = ids.clone().map(|id| name(id).to_owned()).collect();
                errors.push(miss_id_message(
                    arena,
                    request,
                    cadmpeg_core::decode::u64_from_index(all.len()),
                    &all,
                ));
            }
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::parse_radius;

    #[test]
    fn radius_ranges_parse_open_and_closed_forms() {
        for (text, inside, outside) in [
            ("9..11", 10.0, 12.0),
            ("20..", 25.0, 10.0),
            ("..5", 4.0, 6.0),
            ("10", 10.0, 10.5),
        ] {
            let range = parse_radius(text).unwrap().range;
            assert!(range.contains(inside, 0.0), "{text}");
            assert!(!range.contains(outside, 0.0), "{text}");
        }
        for text in ["", "..", "11..9", "-1", "abc", "inf"] {
            assert!(parse_radius(text).is_err(), "{text}");
        }
    }
}
