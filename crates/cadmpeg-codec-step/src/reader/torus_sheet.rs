// SPDX-License-Identifier: Apache-2.0
//! Sheet selection for spindle `TOROIDAL_SURFACE` carriers from the bounding
//! edges of the faces on them.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::{model_curve_point_by_id, EvaluationFailure};
use cadmpeg_ir::geometry::analytic::TorusSurface;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::{ProceduralSurface, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::LoopBoundary;

use super::step_instance_id;
use crate::ids::{self, kind};
use crate::loss::StepLossCode;

/// Interior parameters of an edge's range, as fractions of the range, at which
/// the edge carrier is sampled.
const SHEET_SAMPLE_FRACTIONS: [f64; 3] = [0.25, 0.5, 0.75];

/// The factor by which a sample's distance from one sheet must exceed its
/// distance from the other before a sample outside its tolerance of both
/// sheets selects the nearer sheet. An order of magnitude separates an
/// exporter's placement error on one sheet from a point between the sheets.
const SHEET_SEPARATION_RATIO: f64 = 10.0;

/// The two sheets of a spindle torus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sheet {
    /// `(ρ − R)² + z² = r²`.
    Outer,
    /// `(ρ + R)² + z² = r²`.
    Inner,
}

impl Sheet {
    const fn select_outer(self) -> bool {
        matches!(self, Self::Outer)
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Outer => "outer",
            Self::Inner => "inner",
        }
    }
}

/// A spindle torus in its own frame.
struct Spindle {
    center: Point3,
    axis: [f64; 3],
    major: f64,
    minor: f64,
}

impl Spindle {
    fn new(torus: &TorusSurface) -> Option<Self> {
        let major = torus.major_radius().get();
        let minor = torus.minor_radius().get().abs();
        (major < minor).then(|| {
            let axis = cadmpeg_ir::math::Vector3::from(*torus.frame().axis());
            Self {
                center: torus.center().get(),
                axis: [axis.x, axis.y, axis.z],
                major,
                minor,
            }
        })
    }

    /// Distances of `point` from the outer and the inner sheet.
    fn residuals(&self, point: Point3) -> [f64; 2] {
        let offset = [
            point.x - self.center.x,
            point.y - self.center.y,
            point.z - self.center.z,
        ];
        let height = offset[0] * self.axis[0] + offset[1] * self.axis[1] + offset[2] * self.axis[2];
        let squared = offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2];
        let rho = (squared - height * height).max(0.0).sqrt();
        [
            ((rho - self.major).hypot(height) - self.minor).abs(),
            ((rho + self.major).hypot(height) - self.minor).abs(),
        ]
    }
}

/// The sheet one face states, or why it states none.
enum FaceSheet {
    Selected(Sheet),
    NoOffPolePoint,
    FitsNeither { point: Point3, residuals: [f64; 2] },
    Mixed,
}

/// A point of a face boundary with the tolerance that bounds its distance from
/// the surface.
struct Sample {
    point: Point3,
    tolerance: f64,
}

/// Attach a degenerate-torus sheet selection to each spindle
/// `TOROIDAL_SURFACE` whose faces all lie on one sheet, and report each
/// spindle surface whose faces select none.
pub(super) fn select_spindle_sheets(
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut spindles = BTreeMap::new();
    for surface in &ir.geometry.surfaces {
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus)) = &surface.geometry {
            if let Some(spindle) = Spindle::new(torus) {
                spindles.insert(surface.id.as_str(), (surface.id.clone(), spindle));
            }
        }
    }
    if spindles.is_empty() {
        return Ok(());
    }
    let index = ModelIndex::build(ir);
    let linear = ir.tolerances.linear.get();
    let mut faces: BTreeMap<&str, Vec<(&FaceId, FaceSheet)>> = BTreeMap::new();
    for face in &ir.geometry.faces {
        let Some((_, spindle)) = spindles.get(face.surface.as_str()) else {
            continue;
        };
        let face_tolerance = face
            .tolerance
            .map_or(linear, |bound| linear.max(bound.get()));
        let samples = face_samples(ctx, &index, face, face_tolerance)?;
        faces
            .entry(face.surface.as_str())
            .or_default()
            .push((&face.id, classify(spindle, &samples)));
    }
    let mut selections = Vec::new();
    for (surface, (surface_id, spindle)) in &spindles {
        let Some(on_surface) = faces.get(surface) else {
            continue;
        };
        match surface_sheet(on_surface) {
            Ok(sheet) => selections.push((surface_id.clone(), sheet)),
            Err(reason) => losses.push(StepLossCode::TorusSheetUnresolved.note(format!(
                "TOROIDAL_SURFACE {surface_id} (major radius {:.6e}, minor radius {:.6e}) \
                 is a spindle torus and carries no sheet selection: {reason}",
                spindle.major, spindle.minor
            ))),
        }
    }
    for (surface, sheet) in selections {
        attach(ir, &surface, sheet, losses)?;
    }
    Ok(())
}

fn attach(
    ir: &mut CadIr,
    surface: &SurfaceId,
    sheet: Sheet,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(step_id) = step_instance_id(surface.as_str()) else {
        return Ok(());
    };
    let procedural = ProceduralSurface::new(
        ProceduralSurfaceId::from(ids::construction(kind!("degenerate_torus"), step_id)),
        ProceduralSurfaceDefinition::DegenerateTorus {
            select_outer: sheet.select_outer(),
        },
        None,
    );
    if let Err(error) = ir
        .geometry
        .add_procedural_surfaces(vec![(surface.clone(), procedural)])?
    {
        losses.push(StepLossCode::TorusSheetUnresolved.note(format!(
            "TOROIDAL_SURFACE {surface}: the {} sheet selection was refused: {error}",
            sheet.name()
        )));
    }
    Ok(())
}

/// The vertices of a face's boundary and points of its edge carriers at
/// interior parameters.
fn face_samples(
    ctx: &DecodeContext<'_>,
    index: &ModelIndex<'_>,
    face: &cadmpeg_ir::topology::Face,
    face_tolerance: f64,
) -> Result<Vec<Sample>, CodecError> {
    let mut samples = Vec::new();
    let vertex_sample = |vertex: &str, tolerance: f64| {
        let vertex = index.vertices(vertex, StandardIndex)?;
        let point = index.points(vertex.point.as_str(), StandardIndex)?;
        Some(Sample {
            point: point.position().get(),
            tolerance: vertex
                .tolerance
                .map_or(tolerance, |bound| tolerance.max(bound.get())),
        })
    };
    for loop_id in face.loops.stated() {
        let Some(boundary) = index.loops(loop_id.as_str(), StandardIndex) else {
            continue;
        };
        let ring = match &boundary.boundary {
            LoopBoundary::Vertex { vertex, .. } => {
                samples.extend(vertex_sample(vertex.as_str(), face_tolerance));
                continue;
            }
            LoopBoundary::Ring(ring) => ring,
        };
        for coedge in ring.coedges() {
            let Some(edge) = index
                .coedges(coedge.as_str(), StandardIndex)
                .and_then(|coedge| index.edges(coedge.edge.as_str(), StandardIndex))
            else {
                continue;
            };
            let tolerance = edge
                .tolerance
                .map_or(face_tolerance, |bound| face_tolerance.max(bound.get()));
            if let Some(ends) = &edge.ends {
                samples.extend(vertex_sample(ends.start.as_str(), tolerance));
                samples.extend(vertex_sample(ends.end.as_str(), tolerance));
            }
            let (Some(curve), Some(range)) = (edge.curve(), edge.param_range()) else {
                continue;
            };
            let [start, end] = range.get();
            for fraction in SHEET_SAMPLE_FRACTIONS {
                let parameter = start + (end - start) * fraction;
                match model_curve_point_by_id(
                    EvaluationAdmission::Decode(ctx),
                    index,
                    curve,
                    parameter,
                ) {
                    Ok(point) => samples.push(Sample {
                        point: point.get(),
                        tolerance,
                    }),
                    Err(EvaluationFailure::ResourceLimit(limit)) => return Err(limit.into()),
                    Err(_) => {}
                }
            }
        }
    }
    Ok(samples)
}

/// The sheet the off-pole samples of one face fit. A sample within its
/// tolerance of both sheets lies at a pole. A sample outside its tolerance of
/// both sheets selects the nearer sheet when the other lies
/// `SHEET_SEPARATION_RATIO` times farther.
fn classify(spindle: &Spindle, samples: &[Sample]) -> FaceSheet {
    let mut found = None;
    for sample in samples {
        let residuals = spindle.residuals(sample.point);
        let sheet = match residuals.map(|residual| residual <= sample.tolerance) {
            [true, true] => continue,
            [true, false] => Sheet::Outer,
            [false, true] => Sheet::Inner,
            [false, false] if residuals[1] >= SHEET_SEPARATION_RATIO * residuals[0] => Sheet::Outer,
            [false, false] if residuals[0] >= SHEET_SEPARATION_RATIO * residuals[1] => Sheet::Inner,
            [false, false] => {
                return FaceSheet::FitsNeither {
                    point: sample.point,
                    residuals,
                }
            }
        };
        match found {
            None => found = Some(sheet),
            Some(previous) if previous != sheet => return FaceSheet::Mixed,
            Some(_) => {}
        }
    }
    found.map_or(FaceSheet::NoOffPolePoint, FaceSheet::Selected)
}

/// The sheet every face on one surface selects, or the reason for the loss.
fn surface_sheet(faces: &[(&FaceId, FaceSheet)]) -> Result<Sheet, String> {
    let mut selected: Option<(&FaceId, Sheet)> = None;
    for (face, sheet) in faces {
        let sheet = match sheet {
            FaceSheet::Selected(sheet) => *sheet,
            FaceSheet::NoOffPolePoint => {
                return Err(format!(
                    "face {face} has no boundary point away from the poles"
                ))
            }
            FaceSheet::FitsNeither { point, residuals } => {
                return Err(format!(
                    "a boundary point ({:.6e}, {:.6e}, {:.6e}) of face {face} lies \
                     {:.6e} from the outer sheet and {:.6e} from the inner sheet",
                    point.x, point.y, point.z, residuals[0], residuals[1]
                ))
            }
            FaceSheet::Mixed => {
                return Err(format!(
                    "the boundary points of face {face} fit both sheets away from the poles"
                ))
            }
        };
        match selected {
            None => selected = Some((face, sheet)),
            Some((first, previous)) if previous != sheet => {
                return Err(format!(
                    "face {first} lies on the {} sheet and face {face} on the {} sheet",
                    previous.name(),
                    sheet.name()
                ))
            }
            Some(_) => {}
        }
    }
    selected
        .map(|(_, sheet)| sheet)
        .ok_or_else(|| "no face lies on the surface".to_owned())
}
