// SPDX-License-Identifier: Apache-2.0
//! Triangle mesh to faceted B-rep.
//!
//! [`add_mesh_body`] turns one closed (or nearly closed) triangle mesh into
//! one [`Body`](cadmpeg_ir::topology::Body) of a [`CadIr`]: planar faces with
//! straight edges, built as follows.
//!
//! 1. Vertices within the weld tolerance weld through a deterministic grid
//!    (neighbour cells included). The tolerance is
//!    [`MeshBrepOptions::weld_tolerance_mm`], raised to a few single-precision
//!    steps of the largest coordinate when that is larger.
//! 2. Triangles with a repeated vertex or an area below
//!    `MIN_TRIANGLE_AREA_MM2` are dropped and counted.
//! 3. T-junctions close: a vertex of an unpaired edge that lies within
//!    `T_JUNCTION_TOLERANCE_MM` of another unpaired edge, strictly between its
//!    end points, splits that edge and its triangle. Work is bounded by
//!    `MAX_REPAIR_ROUNDS` and a probe budget.
//! 4. Cracks close: end points of unpaired edges within `CRACK_TOLERANCE_MM`
//!    of each other merge, and step 3 runs again.
//! 5. Half-edges pair up. Edges used once are open. At an edge used more than
//!    twice, the triangles pair up around the edge when they alternate in
//!    direction (solids that touch along an edge). Edges used more than twice
//!    otherwise, or twice in the same direction, are non-manifold. Open and
//!    non-manifold half-edges stay unpaired, so each becomes an edge of its
//!    own with a single coedge, and the body is a sheet.
//! 6. Triangles join into components through paired edges. A closed component
//!    inside no other closed component bounds a solid and has a positive
//!    signed volume; one inside another bounds a void and has a negative
//!    volume. Components of the wrong sign are reversed, and closed components
//!    without a volume are dropped. An open component with a surface area
//!    below `MIN_SHEET_AREA_MM2` is a sliver and is dropped too, unless every
//!    component of the mesh is one.
//! 7. Triangles grow into planar regions against the region's own plane
//!    (`PLANE_DISTANCE_TOLERANCE_MM`, `PLANE_ANGLE_TOLERANCE_RAD`).
//! 8. Region boundaries chain into loops by walking triangle fans, which keeps
//!    loops of a pinched region apart. The only counter-clockwise loop is the
//!    outer loop; a region with no such unique loop lists its loops
//!    unclassified. A region with several or no counter-clockwise loops (a
//!    planar triangulation that covers part of its plane twice) splits into
//!    one region per triangle.
//! 9. Boundary vertices where exactly two edges meet are dropped when they lie
//!    within `COLLINEAR_TOLERANCE_MM` of the merged straight edge. If that
//!    produces an inconsistent pairing the module falls back to keeping every
//!    boundary vertex.
//! 10. The faces of one component form one shell. Each outer component bounds
//!     its own region, and a void component is a further shell of the region
//!     of the component that contains it. Disjoint solids are therefore
//!     several regions of a body.
//! 11. A face whose only loop shrank to two coedges has no area and is
//!     removed, its two neighbours sharing the edge instead. A closed shell of
//!     fewer than four faces encloses no volume and is dropped. Importers
//!     that triangulate a face from its loop mis-measure a two-coedge face,
//!     so none is emitted.
//!
//! Faces whose triangles are not planar (curved surfaces) come out as many
//! small planar faces; no attempt is made to recover analytic surfaces.
//!
//! Ids are `edrawings:model:<kind>#<scope>:<n>`, with `scope` the encoded
//! [`MeshInput::scope`] and `<n>` a deterministic counter. Distinct bodies of
//! one model need distinct scopes. Entities are appended to the arenas in
//! creation order; the caller runs [`CadIr::finalize`] afterwards.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, IdentityKey};
use cadmpeg_ir::topology::Color;

mod adjacency;
mod emit;
mod geom;
mod loops;
mod mesh;
mod nesting;
mod regions;
mod repair;
mod runs;
mod simplify;
#[cfg(test)]
mod tests;
mod weld;

/// Default vertex weld distance in millimetres.
pub(crate) const WELD_TOLERANCE_MM: f64 = 0.001;

/// Weld distance of the second build of a body that is not watertight, in
/// millimetres. The weld raises it to a few single-precision steps of the
/// largest coordinate.
pub(crate) const FINE_WELD_TOLERANCE_MM: f64 = 1.0e-6;

/// Largest accepted triangle count.
pub(crate) const MAX_TRIANGLES: usize = 5_000_000;

/// Largest accepted absolute coordinate in millimetres.
const MAX_COORDINATE_MM: f64 = 1.0e9;

/// Largest accepted point count.
pub(crate) const MAX_POSITIONS: usize = 15_000_000;

/// A vertex may lie this far from its region plane.
const PLANE_DISTANCE_TOLERANCE_MM: f64 = 0.001;

/// A triangle normal may deviate this far from its region normal.
const PLANE_ANGLE_TOLERANCE_RAD: f64 = 2.0e-3;

/// Upper bound of the loosened angular tolerance granted to thin triangles.
const SLIVER_ANGLE_LIMIT_RAD: f64 = 0.35;

/// Triangles smaller than this area are degenerate.
const MIN_TRIANGLE_AREA_MM2: f64 = 1.0e-10;

/// A dropped boundary vertex may lie this far from the merged edge.
const COLLINEAR_TOLERANCE_MM: f64 = 0.001;

/// An unpaired-edge vertex splits another unpaired edge when it lies this far
/// from it.
const T_JUNCTION_TOLERANCE_MM: f64 = 0.01;

/// The weld tolerance is at least this many single-precision steps of the
/// largest coordinate.
const WELD_ULP_FACTOR: f64 = 8.0;

/// Repair rounds per body; a round splits one edge of each triangle.
const MAX_REPAIR_ROUNDS: usize = 8;

/// Grid cells one repair round may probe per triangle.
const REPAIR_PROBES_PER_TRIANGLE: usize = 64;

/// Grid cells one repair round may probe at least, whatever the triangle count.
const MAX_REPAIR_PROBES: usize = 1 << 16;

/// Repair grid cells are at least this many tolerances wide.
const REPAIR_SAMPLE_CELL_FACTOR: f64 = 4.0;

/// A closed component is reversed when its volume has the wrong sign for its
/// nesting depth and exceeds this value in magnitude. A closed component with
/// a smaller volume has no orientation and is dropped.
const MIN_REVERSAL_VOLUME_MM3: f64 = 1.0e-9;

/// An open component with a smaller surface area than this is a sliver and is
/// dropped when the mesh has any other component.
const MIN_SHEET_AREA_MM2: f64 = 1.0e-4;

/// A void is dropped when its volume is below this many plane tolerances
/// times its area.
const THIN_COMPONENT_FACTOR: f64 = 2.0;

/// A component lies within another when its box exceeds the other's box by at
/// most this distance.
const NESTING_BBOX_SLACK_MM: f64 = 0.001;

/// Triangle-ray tests one body may spend on containment. Past the allowance a
/// component lies within another when its box does and its volume is smaller.
const MAX_NESTING_RAY_TESTS: usize = 1 << 27;

/// Unpaired end points closer than this merge, closing a crack the weld left.
const CRACK_TOLERANCE_MM: f64 = 0.025;

/// Two triangles around a non-manifold edge pair up only when their angles
/// about it differ by more than this.
const MIN_WEDGE_ANGLE_RAD: f64 = 1.0e-6;

/// Failure to build a B-rep from a mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum MeshBrepError {
    /// The mesh has no triangles.
    Empty,
    /// Every triangle is degenerate.
    AllDegenerate,
    /// The mesh exceeds [`MAX_TRIANGLES`] or [`MAX_POSITIONS`].
    TooLarge,
    /// The weld tolerance is not a positive finite number.
    InvalidTolerance,
    /// A triangle references a point that does not exist.
    IndexOutOfRange {
        /// Triangle position in the input.
        triangle: usize,
        /// The offending index.
        index: u32,
    },
    /// A referenced point has a NaN or infinite coordinate.
    NonFinitePosition {
        /// Point position in the input.
        index: usize,
    },
    /// A coordinate is too large for the weld grid.
    CoordinateTooLarge,
    /// A CADIR constructor refused an entity.
    Model(String),
    /// An internal invariant failed; the input is not at fault by itself.
    Internal(&'static str),
}

impl std::fmt::Display for MeshBrepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("mesh has no triangles"),
            Self::AllDegenerate => f.write_str("every triangle of the mesh is degenerate"),
            Self::TooLarge => write!(
                f,
                "mesh exceeds {MAX_TRIANGLES} triangles or {MAX_POSITIONS} points"
            ),
            Self::InvalidTolerance => f.write_str("weld tolerance must be positive and finite"),
            Self::IndexOutOfRange { triangle, index } => {
                write!(f, "triangle {triangle} references missing point {index}")
            }
            Self::NonFinitePosition { index } => {
                write!(f, "point {index} has a non-finite coordinate")
            }
            Self::CoordinateTooLarge => f.write_str("coordinate too large to weld"),
            Self::Model(message) => write!(f, "invalid B-rep entity: {message}"),
            Self::Internal(message) => write!(f, "internal mesh B-rep error: {message}"),
        }
    }
}

impl std::error::Error for MeshBrepError {}

impl From<MeshBrepError> for CodecError {
    fn from(error: MeshBrepError) -> Self {
        Self::malformed(error)
    }
}

/// One mesh body to convert.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshInput<'a> {
    /// Id scope of the body; encoded into every id. Distinct bodies of one
    /// model need distinct scopes.
    pub(crate) scope: &'a str,
    /// Body name.
    pub(crate) name: Option<&'a str>,
    /// Body and face colour.
    pub(crate) color: Option<Color>,
    /// Point positions in millimetres, already in the final frame.
    pub(crate) positions: &'a [[f64; 3]],
    /// Triangles, counter-clockwise seen from outside.
    pub(crate) triangles: &'a [[u32; 3]],
}

/// Conversion settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MeshBrepOptions {
    /// Points closer than this weld into one vertex.
    pub(crate) weld_tolerance_mm: f64,
}

impl Default for MeshBrepOptions {
    fn default() -> Self {
        Self {
            weld_tolerance_mm: WELD_TOLERANCE_MM,
        }
    }
}

/// What the conversion produced, for loss notes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MeshBrepSummary {
    /// The new body.
    pub(crate) body: BodyId,
    /// Faces emitted.
    pub(crate) faces: usize,
    /// Edges emitted.
    pub(crate) edges: usize,
    /// Vertices emitted.
    pub(crate) vertices: usize,
    /// Shells (and regions) emitted.
    pub(crate) shells: usize,
    /// Undirected edges used by one triangle only.
    pub(crate) open_edges: usize,
    /// Undirected edges used by more than two triangles or by two triangles of
    /// equal direction.
    pub(crate) non_manifold_edges: usize,
    /// Triangles dropped as degenerate.
    pub(crate) dropped_degenerate_triangles: usize,
    /// Closed components dropped for a zero volume.
    pub(crate) dropped_zero_components: usize,
    /// Voids dropped for a volume below the plane tolerance.
    pub(crate) dropped_thin_components: usize,
    /// Open slivers dropped for a surface area below `MIN_SHEET_AREA_MM2`.
    pub(crate) dropped_sliver_components: usize,
    /// Triangles of the dropped components.
    pub(crate) dropped_component_triangles: usize,
    /// Faces with a two-coedge loop (no area) removed; their neighbours share
    /// the edge instead.
    pub(crate) collapsed_faces: usize,
    /// Closed shells of fewer than four faces (no volume) dropped.
    pub(crate) dropped_flat_shells: usize,
    /// Edge splits that closed T-junctions.
    pub(crate) t_junction_splits: usize,
    /// Boundary points merged into another one to close a crack.
    pub(crate) crack_merged_points: usize,
    /// Whether any closed component had the wrong orientation and was reversed.
    pub(crate) reversed: bool,
    /// Closed components that were reversed.
    pub(crate) reversed_components: usize,
    /// Input points absorbed into another point by welding.
    pub(crate) welded_points: usize,
    /// Triangles kept.
    pub(crate) triangles: usize,
    /// Whether every edge is shared by exactly two oppositely oriented
    /// triangles; the body is then a solid, otherwise a sheet.
    pub(crate) closed: bool,
    /// Signed volume of the kept triangles; negative for an inside-out mesh.
    pub(crate) signed_volume_mm3: f64,
    /// Whether the body was built at the fine weld tolerance because the
    /// requested one left it open.
    pub(crate) fine_weld: bool,
}

/// The requested weld tolerance, raised to [`WELD_ULP_FACTOR`] single-precision
/// steps of the largest coordinate: the source stores single-precision
/// coordinates, so copies of one point differ by about that much.
fn effective_weld_tolerance(input: &MeshInput<'_>, options: MeshBrepOptions) -> f64 {
    let extent = input
        .triangles
        .iter()
        .flatten()
        .filter_map(|&i| usize::try_from(i).ok().and_then(|i| input.positions.get(i)))
        .flatten()
        .fold(0.0_f64, |m, c| m.max(c.abs()));
    options
        .weld_tolerance_mm
        .max(WELD_ULP_FACTOR * f64::from(f32::EPSILON) * extent)
}

fn check_input(input: &MeshInput<'_>, options: MeshBrepOptions) -> Result<(), MeshBrepError> {
    if !(options.weld_tolerance_mm.is_finite() && options.weld_tolerance_mm > 0.0) {
        return Err(MeshBrepError::InvalidTolerance);
    }
    if input.triangles.is_empty() {
        return Err(MeshBrepError::Empty);
    }
    if input.triangles.len() > MAX_TRIANGLES || input.positions.len() > MAX_POSITIONS {
        return Err(MeshBrepError::TooLarge);
    }
    for (triangle, corners) in input.triangles.iter().enumerate() {
        for &index in corners {
            let position = usize::try_from(index)
                .ok()
                .and_then(|i| input.positions.get(i).map(|p| (i, p)));
            let Some((i, p)) = position else {
                return Err(MeshBrepError::IndexOutOfRange { triangle, index });
            };
            if !p.iter().all(|c| c.is_finite()) {
                return Err(MeshBrepError::NonFinitePosition { index: i });
            }
            if p.iter().any(|c| c.abs() > MAX_COORDINATE_MM) {
                return Err(MeshBrepError::CoordinateTooLarge);
            }
        }
    }
    Ok(())
}

/// Convert `input` into one body and append its entities to `ir.model`.
///
/// A body that is not watertight at the requested weld tolerance is built a
/// second time at [`FINE_WELD_TOLERANCE_MM`]: a feature as thin as the weld
/// tolerance (a plate a micrometre thick) welds flat at the first try. The
/// second build replaces the first only when it is watertight.
///
/// On error nothing is added. The caller should run [`CadIr::finalize`] after
/// the last body.
pub(crate) fn add_mesh_body(
    ir: &mut CadIr,
    input: &MeshInput<'_>,
    options: MeshBrepOptions,
) -> Result<MeshBrepSummary, MeshBrepError> {
    let (mut entities, mut summary) = build(input, options)?;
    if !summary.closed && FINE_WELD_TOLERANCE_MM < options.weld_tolerance_mm {
        let fine = MeshBrepOptions {
            weld_tolerance_mm: FINE_WELD_TOLERANCE_MM,
        };
        if let Ok((fine_entities, fine_summary)) = build(input, fine) {
            if fine_summary.closed {
                entities = fine_entities;
                summary = MeshBrepSummary {
                    fine_weld: true,
                    ..fine_summary
                };
            }
        }
    }
    let model = &mut ir.model;
    model.points.extend(entities.points);
    model.vertices.extend(entities.vertices);
    model.curves.extend(entities.curves);
    model.edges.extend(entities.edges);
    model.surfaces.extend(entities.surfaces);
    model.coedges.extend(entities.coedges);
    model.loops.extend(entities.loops);
    model.faces.extend(entities.faces);
    model.shells.extend(entities.shells);
    model.regions.extend(entities.regions);
    model.bodies.extend(entities.bodies);
    Ok(summary)
}

/// Build the entities of one body at the weld tolerance of `options`.
fn build(
    input: &MeshInput<'_>,
    options: MeshBrepOptions,
) -> Result<(emit::Entities, MeshBrepSummary), MeshBrepError> {
    use geom::Slot;

    check_input(input, options)?;
    let tolerance = effective_weld_tolerance(input, options);
    let welded = weld::weld(input.positions, input.triangles, tolerance)?;
    let welded_points = {
        let referenced = welded.map.iter().filter(|&&m| m != geom::NONE).count();
        referenced - welded.positions.len()
    };
    let mut mesh = mesh::Mesh::build(welded, input.triangles)?;
    if mesh.tris.is_empty() {
        return Err(MeshBrepError::AllDegenerate);
    }
    let mut adjacency = adjacency::build(&mesh.tris)?;
    let mut t_junction_splits = 0;
    let mut crack_merged_points = 0;
    if adjacency.open_edges > 0 || adjacency.non_manifold_edges > 0 {
        let t_tolerance = T_JUNCTION_TOLERANCE_MM.max(tolerance);
        let repaired = repair::split_t_junctions(&mesh.pos, &mesh.tris, t_tolerance)?;
        let mut tris = repaired.tris;
        t_junction_splits += repaired.splits;
        let mut changed = repaired.splits > 0;
        if adjacency::build(&tris)?.open_edges > 0 {
            let crack_tolerance = CRACK_TOLERANCE_MM.max(tolerance);
            if let Some(merged) = repair::close_cracks(&mesh.pos, &tris, crack_tolerance)? {
                let again = repair::split_t_junctions(&mesh.pos, &merged.tris, t_tolerance)?;
                tris = again.tris;
                t_junction_splits += again.splits;
                crack_merged_points = merged.splits;
                changed = true;
            }
        }
        if changed {
            mesh = mesh.with_triangles(&tris)?;
            adjacency = adjacency::build(&mesh.tris)?;
        }
    }
    if adjacency.non_manifold_edges > 0 {
        adjacency::pair_around_edges(&mesh, &mut adjacency)?;
    }
    let mut before = nesting::components(&mesh, &adjacency.twin)?;
    before.nest(&mesh)?;
    let mut reversed_components = 0;
    let mut dropped_components = (0, 0, 0, 0);
    let mut old_of_new = None;
    if let Some(rewrite) = before.rewrite(&mesh)? {
        mesh = mesh.with_triangles(&rewrite.tris)?;
        if mesh.tris.len() != rewrite.tris.len() {
            return Err(MeshBrepError::Internal("component rewrite lost triangles"));
        }
        if mesh.tris.is_empty() {
            return Err(MeshBrepError::AllDegenerate);
        }
        adjacency = adjacency::build(&mesh.tris)?;
        if adjacency.non_manifold_edges > 0 {
            adjacency::pair_around_edges(&mesh, &mut adjacency)?;
        }
        reversed_components = rewrite.reversed_components;
        dropped_components = (
            rewrite.dropped_zero_components,
            rewrite.dropped_thin_components,
            rewrite.dropped_triangles,
            rewrite.dropped_sliver_components,
        );
        old_of_new = Some(rewrite.old_of_new);
    }
    let current = nesting::components(&mesh, &adjacency.twin)?;
    let void_parent = nesting::void_parents(&before, &current, old_of_new.as_deref())?;
    let reversed = reversed_components > 0;
    let mut regions = regions::build(&mesh, &adjacency.twin)?;
    let mut loops = loops::trace(&mesh, &adjacency.twin, &regions)?;
    let ambiguous = loops.ambiguous_regions();
    if !ambiguous.is_empty() {
        regions.split_into_triangles(&mesh, &ambiguous)?;
        loops = loops::trace(&mesh, &adjacency.twin, &regions)?;
    }
    let on_boundary = simplify::boundary_vertices(&mesh, &loops)?;
    let kept = simplify::select_kept(&mesh, &adjacency.twin, &loops, &on_boundary)?;
    let mut run_set = match runs::build(&mesh, &adjacency.twin, &loops, &kept)? {
        Some(run_set) => run_set,
        None => runs::build(&mesh, &adjacency.twin, &loops, &on_boundary)?
            .ok_or(MeshBrepError::Internal("inconsistent boundary pairing"))?,
    };
    let collapsed_faces = run_set.collapse_digons()?;
    let mut component_of_region = vec![geom::NONE; regions.list.len()];
    for (t, &region) in regions.region_of.iter().enumerate() {
        *component_of_region.slot_mut(region)? = *current.component_of.slot(t)?;
    }
    let dropped_flat_shells = run_set.drop_flat_shells(&component_of_region)?;
    if (collapsed_faces > 0 || dropped_flat_shells > 0) && run_set.rings.iter().all(Vec::is_empty) {
        return Err(MeshBrepError::AllDegenerate);
    }
    let closed = adjacency.open_edges == 0 && adjacency.non_manifold_edges == 0;
    let scope = IdentityKey::encode_segment(input.scope);
    let naming = emit::Naming {
        scope: &scope,
        name: input.name,
        color: input.color,
        closed,
    };
    let shells = emit::Shells {
        component_of: &current.component_of,
        void_parent: &void_parent,
    };
    let entities = emit::emit(&mesh, &regions, &loops, &run_set, &shells, &naming)?;
    let body = entities
        .bodies
        .slot(0)
        .map_err(|_| MeshBrepError::Internal("no body"))?
        .id
        .clone();
    let summary = MeshBrepSummary {
        body,
        faces: entities.faces.len(),
        edges: entities.edges.len(),
        vertices: entities.vertices.len(),
        shells: entities.shells.len(),
        open_edges: adjacency.open_edges,
        non_manifold_edges: adjacency.non_manifold_edges,
        dropped_degenerate_triangles: mesh.dropped,
        dropped_zero_components: dropped_components.0,
        dropped_thin_components: dropped_components.1,
        dropped_sliver_components: dropped_components.3,
        dropped_component_triangles: dropped_components.2,
        collapsed_faces,
        dropped_flat_shells,
        t_junction_splits,
        crack_merged_points,
        reversed,
        reversed_components,
        welded_points,
        triangles: mesh.tris.len(),
        closed,
        signed_volume_mm3: mesh.signed_volume,
        fine_weld: false,
    };
    Ok((entities, summary))
}
