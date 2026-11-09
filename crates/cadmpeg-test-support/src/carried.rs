// SPDX-License-Identifier: Apache-2.0
//! Carried sets for the W-closure-cross harness (§5.2, #232).
//!
//! For a writer X:
//! - `A_X`: the arenas X's decoder populates;
//! - `V_X`: the enum variants it constructs, as `Enum::Variant` strings;
//! - `P_X`: the JSON key paths it fills with non-null values, array indices
//!   collapsed.
//!
//! Variants are read syntactically from CADIR JSON through the generated
//! enum registry (`data/variant_registry.json`): externally-tagged values
//! contribute their key or bare spelling, internally and adjacently tagged
//! values contribute their tag value. A bare string attributes only a unit
//! variant, and a single-key object only a variant with a payload: a unit
//! variant never serializes as `{"Variant": ...}`, so a structural map such
//! as an edge carrier `{"curve": <id>}` attributes nothing. The same reading
//! serves observation (what X's decodes hold) and containment (what a record
//! holds), so the two sides can never disagree about a spelling. A tag value
//! claimed by several enums resolves to all of them; the ambiguity is
//! measured by [`ambiguity_sample`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use cadmpeg_core::decode::ResourceLimit;

use serde_json::Value;

const REGISTRY_JSON: &str = include_str!("data/variant_registry.json");
const TABLE_JSON: &str = include_str!("data/constructor_table.json");

fn registry() -> &'static Value {
    static REGISTRY: OnceLock<Value> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        serde_json::from_str(REGISTRY_JSON).expect("checked-in variant registry parses")
    })
}

pub(crate) fn typed_variant(enum_name: &str, value: &Value) -> Option<(String, Option<String>)> {
    let definition = registry().get("enums")?.get(enum_name)?;
    let style = definition
        .get("style")
        .and_then(Value::as_str)
        .unwrap_or("external");
    let (spelling, tag) = match style {
        "external" => match value {
            Value::String(text) => (text.as_str(), None),
            Value::Object(fields) if fields.len() == 1 => (fields.keys().next()?.as_str(), None),
            _ => return None,
        },
        "internal" | "adjacent" => {
            let tag = definition.get("tag")?.as_str()?;
            (value.get(tag)?.as_str()?, Some(tag.to_owned()))
        }
        _ => return None,
    };
    let variants = definition.get("variants")?.as_object()?;
    let variant = variants
        .iter()
        .find_map(|(name, wire)| (wire.as_str() == Some(spelling)).then_some(name))?;
    Some((format!("{enum_name}::{variant}"), tag))
}

pub(crate) fn variant_domain(enum_name: &str) -> Option<BTreeSet<String>> {
    Some(
        registry()
            .get("enums")?
            .get(enum_name)?
            .get("variants")?
            .as_object()?
            .values()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
    )
}

/// What a writer carries across formats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Carried {
    /// Registry field names of populated model arenas, plus
    /// `native.<namespace>.<arena>` for populated native arenas.
    pub arenas: BTreeSet<String>,
    /// `Enum::Variant` strings.
    pub variants: BTreeSet<String>,
    /// Collapsed JSON key paths with non-null values (`a.b.[].c`).
    pub paths: BTreeSet<String>,
    /// Supported edge carrier variants, when the writer declares them.
    pub edge_carriers: Option<BTreeSet<EdgeCarrierVariant>>,
    /// Record classes the writer declares its format cannot hold.
    pub unheld: BTreeSet<UnheldClass>,
}

/// A record class inside a carried variant that the target format cannot
/// hold, so a record of the class need not reappear (#209 §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnheldClass {
    /// A surface on a cone with an elliptical section and a non-zero half
    /// angle.
    EllipticalCone,
    /// A binding of a translucent appearance to a curve, edge, point or
    /// vertex, whose style holds no transparency.
    TranslucentLineBinding,
    /// A solid body with more than one region: a manifold solid bounds one
    /// connected volume.
    MultiRegionSolid,
    /// A product definition whose source name differs from its label, where
    /// the format holds one product name.
    DistinctProductNames,
    /// A coedge with more than one pcurve, where the format associates one
    /// pcurve with a coedge on its face's surface.
    MultiPcurveCoedge,
    /// A curve construction beside the solved geometry of the curve it
    /// constructs, where the format holds the solved carrier.
    ConstructionBesideSolvedCurve,
    /// An edge whose curve, at the ends of its stated parameter range, lies
    /// off its vertices, where the format derives the range from the
    /// vertices.
    RangeOffVertices,
    /// A pcurve that coedges use on more than one surface, where a pcurve
    /// lies on one surface.
    PcurveOnSeveralSurfaces,
    /// A pcurve whose image on its coedge's face surface misses a vertex of
    /// the coedge's edge, where the format's pcurve is the edge curve itself.
    PcurveOffItsEdge,
    /// A ring loop whose oriented coedges do not close into a path, where
    /// the format's loop is a closed path of edges.
    OpenRing,
    /// A pcurve that only coedges of edges with no 3D curve use, where the
    /// format attaches a pcurve to an edge through the edge's 3D curve.
    CurvelessEdgePcurve,
    /// An external occurrence that names its document by a persisted
    /// document identity, where the format names an external source by one
    /// identifier that a location also fills.
    DocumentIdentityReference,
    /// An appearance with no colour, where the format's surface and curve
    /// styles state their colour.
    ColourlessAppearance,
    /// A presentation annotation with no placement, where the format places
    /// the text of every annotation.
    UnplacedPresentation,
    /// A face tolerance, or a vertex or edge tolerance on a solid body or
    /// other than the document's linear tolerance, where the format states
    /// one document resolution and its solid topology states no tolerance.
    UnstatedTolerance,
}

impl UnheldClass {
    /// The ids of the records of `document` in the class.
    #[must_use]
    pub fn members(self, document: &Value) -> BTreeSet<String> {
        match self {
            Self::RangeOffVertices => {
                return evaluated(edges_with_ranges_off_their_vertices(document))
            }
            Self::PcurveOnSeveralSurfaces => return pcurves_on_several_surfaces(document),
            Self::PcurveOffItsEdge => return evaluated(pcurves_off_their_edges(document)),
            Self::OpenRing => return open_rings(document),
            Self::CurvelessEdgePcurve => return curveless_edge_pcurves(document),
            Self::UnstatedTolerance => return unstated_tolerances(document),
            _ => {}
        }
        let mut members = BTreeSet::new();
        for (arena, rows) in document
            .get("model")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            for row in rows.as_array().into_iter().flatten() {
                if let Some(id) = row.get("id").and_then(Value::as_str) {
                    if self.contains(arena, row, document) {
                        members.insert(id.to_owned());
                    }
                }
            }
        }
        members
    }

    /// Whether a serialized record of `arena` in `document` belongs to the
    /// class.
    #[must_use]
    pub fn contains(self, arena: &str, record: &Value, document: &Value) -> bool {
        match self {
            Self::RangeOffVertices
            | Self::PcurveOnSeveralSurfaces
            | Self::PcurveOffItsEdge
            | Self::OpenRing
            | Self::CurvelessEdgePcurve
            | Self::UnstatedTolerance => record
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| self.members(document).contains(id)),
            Self::EllipticalCone => {
                arena == "surfaces" && record.get("geometry").is_some_and(contains_elliptical_cone)
            }
            Self::TranslucentLineBinding => {
                arena == "appearance_bindings"
                    && matches!(
                        record
                            .get("target")
                            .and_then(|target| target.get("kind"))
                            .and_then(Value::as_str),
                        Some("curve" | "edge" | "point" | "vertex")
                    )
                    && record
                        .get("appearance")
                        .and_then(Value::as_str)
                        .is_some_and(|appearance| translucent(document, appearance))
            }
            Self::ConstructionBesideSolvedCurve => {
                arena == "procedural_curves"
                    && record
                        .get("curve")
                        .and_then(Value::as_str)
                        .is_some_and(|curve| solved_curve(document, curve))
            }
            Self::MultiPcurveCoedge => {
                arena == "coedges"
                    && record
                        .get("pcurves")
                        .and_then(Value::as_array)
                        .is_some_and(|uses| uses.len() > 1)
            }
            Self::UnplacedPresentation => {
                arena == "pmi"
                    && record.pointer("/definition/kind").and_then(Value::as_str)
                        == Some("presentation")
                    && record
                        .pointer("/definition/placement")
                        .is_none_or(Value::is_null)
            }
            Self::ColourlessAppearance => {
                arena == "appearances" && record.get("base_color").is_none_or(Value::is_null)
            }
            Self::DocumentIdentityReference => {
                arena == "occurrences"
                    && record
                        .pointer("/prototype/document/resolution")
                        .and_then(Value::as_str)
                        == Some("document_id")
            }
            Self::DistinctProductNames => {
                let name = |key| record.get(key).and_then(Value::as_str);
                arena == "product_definitions"
                    && name("source_name")
                        .zip(name("label"))
                        .is_some_and(|(source, label)| source != label)
            }
            Self::MultiRegionSolid => {
                arena == "bodies"
                    && record.get("kind").and_then(Value::as_str) == Some("solid")
                    && record
                        .get("regions")
                        .and_then(Value::as_array)
                        .is_some_and(|regions| regions.len() > 1)
            }
        }
    }
}

/// The members of a class whose query evaluates geometry. The harness
/// evaluates within the default limits, so a refusal is a harness failure.
fn evaluated(members: Result<BTreeSet<String>, ResourceLimit>) -> BTreeSet<String> {
    members.unwrap_or_else(|limit| panic!("a class membership evaluation was refused: {limit}"))
}

fn unstated_tolerances(document: &Value) -> BTreeSet<String> {
    let Ok(ir) = serde_json::from_value::<cadmpeg_ir::CadIr>(document.clone()) else {
        return BTreeSet::new();
    };
    let resolution = ir
        .tolerances
        .linear
        .map(cadmpeg_ir::scalar::PositiveLength::get);
    let regions = ir
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid)
        .flat_map(|body| body.regions.iter())
        .collect::<BTreeSet<_>>();
    let shells = ir
        .model
        .shells
        .iter()
        .filter(|shell| regions.contains(&shell.region))
        .map(|shell| &shell.id)
        .collect::<BTreeSet<_>>();
    let faces = ir
        .model
        .faces
        .iter()
        .filter(|face| shells.contains(&face.shell))
        .map(|face| &face.id)
        .collect::<BTreeSet<_>>();
    let loops = ir
        .model
        .loops
        .iter()
        .filter(|boundary| faces.contains(&boundary.face))
        .map(|boundary| &boundary.id)
        .collect::<BTreeSet<_>>();
    let solid_edges = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| loops.contains(&coedge.owner_loop))
        .map(|coedge| &coedge.edge)
        .collect::<BTreeSet<_>>();
    let mut solid = BTreeSet::new();
    for edge in ir
        .model
        .edges
        .iter()
        .filter(|edge| solid_edges.contains(&edge.id))
    {
        solid.insert(edge.id.as_str());
        solid.insert(edge.start.as_str());
        solid.insert(edge.end.as_str());
    }
    let unstated = |id: &str, tolerance: Option<cadmpeg_ir::scalar::PositiveReal>| {
        tolerance.is_some_and(|tolerance| solid.contains(id) || resolution != Some(tolerance.get()))
    };
    let vertices = ir
        .model
        .vertices
        .iter()
        .filter(|vertex| unstated(vertex.id.as_str(), vertex.tolerance))
        .map(|vertex| vertex.id.to_string());
    let edges = ir
        .model
        .edges
        .iter()
        .filter(|edge| unstated(edge.id.as_str(), edge.tolerance))
        .map(|edge| edge.id.to_string());
    let faces = ir
        .model
        .faces
        .iter()
        .filter(|face| face.tolerance.is_some())
        .map(|face| face.id.to_string());
    vertices.chain(edges).chain(faces).collect()
}

fn edges_with_ranges_off_their_vertices(
    document: &Value,
) -> Result<BTreeSet<String>, ResourceLimit> {
    let Ok(ir) = serde_json::from_value::<cadmpeg_ir::CadIr>(document.clone()) else {
        return Ok(BTreeSet::new());
    };
    let index = cadmpeg_ir::index::ModelIndex::build(&ir);
    let tolerance = ir.tolerances.linear_or_default().get();
    let position = |vertex: &cadmpeg_ir::ids::VertexId| {
        let vertex = index.vertices(vertex.as_str(), cadmpeg_ir::index::StandardIndex)?;
        let point = index.points(vertex.point.as_str(), cadmpeg_ir::index::StandardIndex)?;
        Some(point.position().get())
    };
    let off = |curve: &cadmpeg_ir::ids::CurveId, parameter: f64, vertex| {
        let Some(vertex) = position(vertex) else {
            return Ok(false);
        };
        match cadmpeg_ir::eval::model_curve_point_by_id(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index,
            curve,
            parameter,
        ) {
            Ok(point) => Ok(point.get().distance(vertex) > tolerance),
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => Err(limit),
            Err(_) => Ok(false),
        }
    };
    let mut members = BTreeSet::new();
    for edge in &ir.model.edges {
        let Some((curve, range)) = edge.curve().zip(edge.param_range()) else {
            continue;
        };
        if off(curve, range[0], &edge.start)? || off(curve, range[1], &edge.end)? {
            members.insert(edge.id.as_str().to_owned());
        }
    }
    Ok(members)
}

fn curveless_edge_pcurves(document: &Value) -> BTreeSet<String> {
    let rows = |arena: &str| {
        document
            .get("model")
            .and_then(|model| model.get(arena))
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let curveless: BTreeSet<&str> = rows("edges")
        .iter()
        .filter(|edge| {
            edge.get("carrier")
                .and_then(|carrier| carrier.get("curve"))
                .is_none_or(Value::is_null)
        })
        .filter_map(|edge| edge.get("id")?.as_str())
        .collect();
    let mut carried_by = BTreeMap::<&str, bool>::new();
    for coedge in rows("coedges") {
        let on_curveless = coedge
            .get("edge")
            .and_then(Value::as_str)
            .is_some_and(|edge| curveless.contains(edge));
        for use_ in coedge
            .get("pcurves")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(pcurve) = use_.get("pcurve").and_then(Value::as_str) {
                *carried_by.entry(pcurve).or_insert(true) &= on_curveless;
            }
        }
    }
    carried_by
        .into_iter()
        .filter(|(_, only_curveless)| *only_curveless)
        .map(|(pcurve, _)| pcurve.to_owned())
        .collect()
}

fn open_rings(document: &Value) -> BTreeSet<String> {
    let rows = |arena: &str| {
        document
            .get("model")
            .and_then(|model| model.get(arena))
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let by_id = |arena: &str| -> BTreeMap<&str, &Value> {
        rows(arena)
            .iter()
            .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
            .collect()
    };
    let coedges = by_id("coedges");
    let edges = by_id("edges");
    let text = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
    let mut members = BTreeSet::new();
    for lp in rows("loops") {
        let Some(boundary) = lp
            .get("boundary")
            .filter(|boundary| boundary.get("kind").and_then(Value::as_str) == Some("ring"))
        else {
            continue;
        };
        let mut balance = BTreeMap::<String, i64>::new();
        let mut complete = true;
        for coedge in boundary
            .get("coedges")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let ends = coedge
                .as_str()
                .and_then(|id| coedges.get(id))
                .and_then(|coedge| {
                    let edge = edges.get(text(coedge, "edge")?.as_str())?;
                    let (start, end) = (text(edge, "start")?, text(edge, "end")?);
                    Some(match coedge.get("sense").and_then(Value::as_str) {
                        Some("reversed") => (end, start),
                        _ => (start, end),
                    })
                });
            let Some((start, end)) = ends else {
                complete = false;
                break;
            };
            *balance.entry(start).or_default() += 1;
            *balance.entry(end).or_default() -= 1;
        }
        if complete && balance.values().any(|count| *count != 0) {
            if let Some(id) = text(lp, "id") {
                members.insert(id);
            }
        }
    }
    members
}

fn pcurves_off_their_edges(document: &Value) -> Result<BTreeSet<String>, ResourceLimit> {
    use cadmpeg_ir::eval::admission::EvaluationAdmission;
    use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::index::StandardIndex;
    let Ok(ir) = serde_json::from_value::<cadmpeg_ir::CadIr>(document.clone()) else {
        return Ok(BTreeSet::new());
    };
    let index = cadmpeg_ir::index::ModelIndex::build(&ir);
    let tolerance = ir.tolerances.linear_or_default().get();
    let position = |vertex: &cadmpeg_ir::ids::VertexId| {
        let vertex = index.vertices(vertex.as_str(), StandardIndex)?;
        let point = index.points(vertex.point.as_str(), StandardIndex)?;
        Some(point.position().get())
    };
    let lifted = |surface: &SurfaceGeometry, pcurve: &PcurveGeometry, parameter: f64| {
        let uv = match cadmpeg_ir::eval::decode::pcurve_uv(
            EvaluationAdmission::Standard,
            pcurve,
            parameter,
        ) {
            Ok(uv) => uv.get(),
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => return Err(limit),
            Err(_) => return Ok(None),
        };
        match cadmpeg_ir::eval::model_surface_point(
            EvaluationAdmission::Standard,
            &ir,
            surface,
            uv.u,
            uv.v,
        ) {
            Ok(point) => Ok(Some(point.get())),
            Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => Err(limit),
            Err(_) => Ok(None),
        }
    };
    // The nearest point of a line pcurve to a vertex, found in the surface
    // chart where the analytic surface inverts the vertex.
    let line_foot = |solved: &SolvedSurfaceGeometry, pcurve: &PcurveGeometry, vertex| {
        let mut basis = pcurve;
        let mut transforms = Vec::new();
        while let PcurveGeometry::Transformed(placed) = basis {
            transforms.push(*placed.transform());
            basis = placed.basis();
        }
        let PcurveGeometry::Line(line) = basis else {
            return None;
        };
        let mut origin = line.origin().get();
        let mut direction = *line.direction().as_raw();
        for transform in transforms.into_iter().rev() {
            origin = transform.apply_point(origin);
            direction = transform.apply_vector(direction);
        }
        let length_squared = direction.u * direction.u + direction.v * direction.v;
        let uv = cadmpeg_ir::eval::analytic_surface_parameters_solved(solved, vertex)?.get();
        let parameter =
            ((uv.u - origin.u) * direction.u + (uv.v - origin.v) * direction.v) / length_squared;
        parameter.is_finite().then_some(parameter)
    };
    let mut members = BTreeSet::new();
    for coedge in &ir.model.coedges {
        let Some(edge) = index
            .edges(coedge.edge.as_str(), StandardIndex)
            .filter(|edge| edge.curve().is_some())
        else {
            continue;
        };
        let Some(surface) = index
            .loops(coedge.owner_loop.as_str(), StandardIndex)
            .and_then(|lp| index.faces(lp.face.as_str(), StandardIndex))
            .and_then(|face| index.surfaces(face.surface.as_str(), StandardIndex))
        else {
            continue;
        };
        let SurfaceGeometry::Solved(
            solved @ (SolvedSurfaceGeometry::Plane(_)
            | SolvedSurfaceGeometry::Cylinder(_)
            | SolvedSurfaceGeometry::Cone(_)
            | SolvedSurfaceGeometry::Sphere(_)
            | SolvedSurfaceGeometry::Torus(_)),
        ) = &surface.geometry
        else {
            continue;
        };
        let (Some(start), Some(end)) = (position(&edge.start), position(&edge.end)) else {
            continue;
        };
        for use_ in &coedge.pcurves {
            let Some(pcurve) = index.pcurves(use_.pcurve.as_str(), StandardIndex) else {
                continue;
            };
            let geometry = &pcurve.geometry;
            let misses = |vertex: cadmpeg_ir::math::Point3| -> Result<bool, ResourceLimit> {
                let feet = match use_.parameter_range {
                    Some(range) => range.endpoints().to_vec(),
                    None => line_foot(solved, geometry, vertex).into_iter().collect(),
                };
                if feet.is_empty() {
                    return Ok(false);
                }
                for parameter in feet {
                    match lifted(&surface.geometry, geometry, parameter)? {
                        Some(point) if point.distance(vertex) > tolerance => {}
                        _ => return Ok(false),
                    }
                }
                Ok(true)
            };
            if misses(start)? || misses(end)? {
                members.insert(use_.pcurve.as_str().to_owned());
            }
        }
    }
    Ok(members)
}

fn pcurves_on_several_surfaces(document: &Value) -> BTreeSet<String> {
    let rows = |arena: &str| {
        document
            .get("model")
            .and_then(|model| model.get(arena))
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let field = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
    let index = |arena: &str, key: &str| -> BTreeMap<String, String> {
        rows(arena)
            .iter()
            .filter_map(|row| Some((field(row, "id")?, field(row, key)?)))
            .collect()
    };
    let loop_faces = index("loops", "face");
    let face_surfaces = index("faces", "surface");
    let mut surfaces = BTreeMap::<String, BTreeSet<String>>::new();
    for coedge in rows("coedges") {
        let Some(surface) = field(coedge, "owner_loop")
            .and_then(|lp| loop_faces.get(&lp))
            .and_then(|face| face_surfaces.get(face))
        else {
            continue;
        };
        for use_ in coedge
            .get("pcurves")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(pcurve) = field(use_, "pcurve") {
                surfaces.entry(pcurve).or_default().insert(surface.clone());
            }
        }
    }
    surfaces
        .into_iter()
        .filter(|(_, surfaces)| surfaces.len() > 1)
        .map(|(pcurve, _)| pcurve)
        .collect()
}

fn solved_curve(document: &Value, curve: &str) -> bool {
    document
        .get("model")
        .and_then(|model| model.get("curves"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| row.get("id").and_then(Value::as_str) == Some(curve))
        .filter_map(|row| row.get("geometry")?.get("kind")?.as_str())
        .any(|kind| kind != "procedural")
}

fn translucent(document: &Value, appearance: &str) -> bool {
    document
        .get("model")
        .and_then(|model| model.get("appearances"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| row.get("id").and_then(Value::as_str) == Some(appearance))
        .filter_map(|row| row.get("base_color")?.get("a")?.as_f64())
        .any(|alpha| alpha < 1.0)
}

fn contains_elliptical_cone(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            let elliptical = fields.get("kind").and_then(Value::as_str) == Some("cone")
                && fields
                    .get("ratio")
                    .and_then(Value::as_f64)
                    .is_some_and(|ratio| ratio != 1.0)
                && fields
                    .get("half_angle")
                    .and_then(Value::as_f64)
                    .is_some_and(|angle| angle != 0.0);
            elliptical || fields.values().any(contains_elliptical_cone)
        }
        Value::Array(items) => items.iter().any(contains_elliptical_cone),
        _ => false,
    }
}

/// The variants of the edge carrier's untagged wire representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeCarrierVariant {
    /// No curve or endpoints.
    Free,
    /// Endpoints without a curve.
    Endpoints,
    /// A curve without endpoints.
    Curve,
    /// A curve with endpoints.
    Bounded,
}

impl EdgeCarrierVariant {
    /// Identify the variant in an edge record's serialized carrier.
    #[must_use]
    pub fn from_record(record: &Value) -> Option<Self> {
        let carrier = record.get("carrier")?.as_object()?;
        let curve = carrier.get("curve").is_some_and(|value| !value.is_null());
        let range = carrier
            .get("param_range")
            .is_some_and(|value| !value.is_null());
        Some(match (curve, range) {
            (false, false) => Self::Free,
            (false, true) => Self::Endpoints,
            (true, false) => Self::Curve,
            (true, true) => Self::Bounded,
        })
    }
}

impl Carried {
    /// CADIR carries everything (§5.2): every arena and variant counts as
    /// carried, and paths are not projected.
    #[must_use]
    pub fn all() -> Self {
        Self {
            arenas: BTreeSet::from(["*".to_owned()]),
            variants: BTreeSet::from(["*".to_owned()]),
            paths: BTreeSet::from(["*".to_owned()]),
            edge_carriers: None,
            unheld: BTreeSet::new(),
        }
    }

    /// Whether the wildcard stands in for every arena, variant or path.
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.arenas.contains("*")
            && self.variants.contains("*")
            && self.edge_carriers.is_none()
            && self.unheld.is_empty()
    }

    /// Whether an arena counts as carried: the wildcard carries all.
    #[must_use]
    pub fn carries_arena(&self, arena: &str) -> bool {
        self.arenas.contains("*") || self.arenas.contains(arena)
    }

    /// Whether a variant counts as carried: the wildcard carries all.
    #[must_use]
    pub fn carries_variant(&self, variant: &str) -> bool {
        self.variants.contains("*") || self.variants.contains(variant)
    }
}

#[derive(Debug, Clone)]
struct VariantMaps {
    /// Bare-string spelling to variants (unit variants).
    units: BTreeMap<String, Vec<String>>,
    /// Single-key object spelling to variants (externally tagged).
    keys: BTreeMap<String, Vec<String>>,
    /// (tag key, tag value) to variants (internally/adjacently tagged).
    tags: BTreeMap<(String, String), Vec<String>>,
}

fn variant_maps() -> &'static VariantMaps {
    static MAPS: OnceLock<VariantMaps> = OnceLock::new();
    MAPS.get_or_init(|| {
        let registry = registry();
        let mut maps = VariantMaps {
            units: BTreeMap::new(),
            keys: BTreeMap::new(),
            tags: BTreeMap::new(),
        };
        let enums = registry
            .get("enums")
            .and_then(Value::as_object)
            .expect("registry holds enums");
        for (name, definition) in enums {
            let style = definition
                .get("style")
                .and_then(Value::as_str)
                .unwrap_or("external");
            let tag = definition.get("tag").and_then(Value::as_str);
            let variants = definition
                .get("variants")
                .and_then(Value::as_object)
                .expect("enum holds variants");
            let objects: Option<BTreeSet<String>> = definition
                .get("object_variants")
                .and_then(Value::as_array)
                .map(|names| {
                    names
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                });
            for (variant, spelling) in variants {
                let spelling = spelling.as_str().unwrap_or(variant);
                let qualified = format!("{name}::{variant}");
                match style {
                    "untagged" => {}
                    "internal" | "adjacent" => {
                        if let Some(tag) = tag {
                            maps.tags
                                .entry((tag.to_owned(), spelling.to_owned()))
                                .or_default()
                                .push(qualified.clone());
                        }
                    }
                    _ => {
                        // Without the generated payload list (a stale
                        // registry), keep the legacy reading: every
                        // spelling attributes both forms.
                        let object = objects.as_ref().is_none_or(|set| set.contains(variant));
                        if objects.is_none() || !object {
                            maps.units
                                .entry(spelling.to_owned())
                                .or_default()
                                .push(qualified.clone());
                        }
                        if objects.is_none() || object {
                            maps.keys
                                .entry(spelling.to_owned())
                                .or_default()
                                .push(qualified);
                        }
                    }
                }
            }
        }
        maps
    })
}

/// Every `Enum::Variant` the JSON value exhibits through the registry.
#[must_use]
pub fn json_variants(value: &Value) -> BTreeSet<String> {
    let maps = variant_maps();
    let mut found = BTreeSet::new();
    walk_variants(value, maps, &mut found);
    found
}

fn walk_variants(value: &Value, maps: &VariantMaps, found: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            if let Some(variants) = maps.units.get(text) {
                found.extend(variants.iter().cloned());
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_variants(item, maps, found);
            }
        }
        Value::Object(fields) => {
            if fields.len() == 1 {
                if let Some((key, _)) = fields.iter().next() {
                    if let Some(variants) = maps.keys.get(key) {
                        found.extend(variants.iter().cloned());
                    }
                }
            }
            for (key, child) in fields {
                if let Value::String(text) = child {
                    if let Some(variants) = maps.tags.get(&(key.clone(), text.clone())) {
                        found.extend(variants.iter().cloned());
                    }
                }
                walk_variants(child, maps, found);
            }
        }
        _ => {}
    }
}

/// Registry entries whose spelling resolves to several variants, over one
/// document: evidence for the documented over-approximation.
#[must_use]
pub fn ambiguity_sample(document: &Value) -> BTreeMap<String, usize> {
    fn walk(value: &Value, maps: &VariantMaps, check: &mut dyn FnMut(&str, usize)) {
        match value {
            Value::String(text) => {
                if let Some(variants) = maps.units.get(text) {
                    check(text, variants.len());
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, maps, check);
                }
            }
            Value::Object(fields) => {
                if fields.len() == 1 {
                    if let Some((key, _)) = fields.iter().next() {
                        if let Some(variants) = maps.keys.get(key) {
                            check(key, variants.len());
                        }
                    }
                }
                for (key, child) in fields {
                    if let Value::String(text) = child {
                        if let Some(variants) = maps.tags.get(&(key.clone(), text.clone())) {
                            check(&format!("{key}={text}"), variants.len());
                        }
                    }
                    walk(child, maps, check);
                }
            }
            _ => {}
        }
    }
    let maps = variant_maps();
    let mut ambiguous = BTreeMap::new();
    let mut check = |spelling: &str, count: usize| {
        if count > 1 {
            *ambiguous.entry(spelling.to_owned()).or_default() += 1;
        }
    };
    walk(document, maps, &mut check);
    ambiguous
}

/// Model arenas with at least one row, as registry field names, plus
/// `native.<namespace>.<arena>` for populated native arenas.
#[must_use]
pub fn observe_arenas(document: &Value) -> BTreeSet<String> {
    let mut arenas = BTreeSet::new();
    if let Some(model) = document.get("model").and_then(Value::as_object) {
        for (arena, rows) in model {
            if rows.as_array().is_some_and(|rows| !rows.is_empty()) {
                arenas.insert(arena.clone());
            }
        }
    }
    if let Some(native) = document.get("native").and_then(Value::as_object) {
        for (namespace, spaces) in native {
            for (arena, rows) in spaces.as_object().into_iter().flatten() {
                if rows.as_array().is_some_and(|rows| !rows.is_empty()) {
                    arenas.insert(format!("native.{namespace}.{arena}"));
                }
            }
        }
    }
    arenas
}

/// JSON key paths with non-null values, array indices collapsed to `[]`.
#[must_use]
pub fn observe_paths(document: &Value) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    walk_paths(document, &mut String::new(), &mut paths);
    paths
}

fn walk_paths(value: &Value, prefix: &mut String, paths: &mut BTreeSet<String>) {
    match value {
        Value::Null => {}
        Value::Array(items) => {
            let base = prefix.len();
            prefix.push_str("[]");
            for item in items {
                walk_paths(item, prefix, paths);
            }
            prefix.truncate(base);
        }
        Value::Object(fields) => {
            for (key, child) in fields {
                let base = prefix.len();
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(key);
                if !matches!(child, Value::Null) {
                    paths.insert(prefix.clone());
                }
                walk_paths(child, prefix, paths);
                prefix.truncate(base);
            }
        }
        _ => {}
    }
}

/// Observed carried sets of decoded documents: arenas and paths from JSON,
/// variants from JSON through the CADIR schema and registry.
#[must_use]
pub fn observe_carried(documents: &[Value]) -> Carried {
    let mut carried = Carried::default();
    for document in documents {
        carried.arenas.extend(observe_arenas(document));
        carried.paths.extend(observe_paths(document));
        carried
            .variants
            .extend(crate::cross_closure::typed_variants::document(document));
    }
    carried
}

/// The generated constructor table: constructor path to variants it builds.
#[must_use]
pub fn constructor_table() -> BTreeMap<String, Vec<String>> {
    serde_json::from_str(TABLE_JSON).expect("checked-in constructor table parses")
}

/// One codec crate's scan output, as checked in by the §5.2 scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrateScan {
    /// `Enum::Variant` strings the crate constructs outside the encode path.
    pub variants: BTreeSet<String>,
    /// Arena registry fields whose record types the crate constructs.
    pub arenas: BTreeSet<String>,
}

/// Reads a `scan_<package>.json` artifact written by the §5.2 scan.
pub fn crate_scan(scan_json: &str) -> CrateScan {
    let parsed: Value = serde_json::from_str(scan_json).expect("crate scan parses");
    let strings = |key: &str| {
        parsed
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    CrateScan {
        variants: strings("variants"),
        arenas: strings("arenas"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ambiguity_sample, json_variants, observe_arenas, observe_paths};
    use serde_json::json;

    #[test]
    fn an_unstated_tolerance_differs_from_the_document_resolution() {
        let mut document = serde_json::to_value(cadmpeg_ir::CadIr::empty()).expect("document");
        document["tolerances"]["linear"] = json!(0.001);
        document["model"]["points"] = json!([
            {"id": "test:model:point#a", "position": {"x": 0.0, "y": 0.0, "z": 0.0}},
            {"id": "test:model:point#b", "position": {"x": 1.0, "y": 0.0, "z": 0.0}},
            {"id": "test:model:point#c", "position": {"x": 2.0, "y": 0.0, "z": 0.0}}
        ]);
        document["model"]["vertices"] = json!([
            {"id": "test:model:vertex#finer", "point": "test:model:point#a", "tolerance": 1.0e-7},
            {"id": "test:model:vertex#resolution", "point": "test:model:point#b", "tolerance": 0.001},
            {"id": "test:model:vertex#none", "point": "test:model:point#c"}
        ]);
        let members = super::UnheldClass::UnstatedTolerance.members(&document);
        assert_eq!(
            members,
            std::collections::BTreeSet::from(["test:model:vertex#finer".to_owned()])
        );
    }

    #[test]
    fn observation_attributes_a_tag_to_its_actual_enum() {
        let document = json!({"model": {"drawings": [
            {"id": "test:model:drawing#1", "kind": "dimension", "runtime_type": "view"}
        ]}});
        let observed = super::observe_carried(&[document]);
        assert!(
            observed.variants.contains("DrawingKind::Dimension"),
            "{:?}",
            observed.variants
        );
        assert!(
            !observed
                .variants
                .contains("SemanticAnnotationKind::Dimension"),
            "{:?}",
            observed.variants
        );
        assert!(
            !observed.variants.contains("DrawingKind::View"),
            "{:?}",
            observed.variants
        );
    }

    #[test]
    fn observation_recognizes_variants_in_a_complete_document() {
        let document =
            serde_json::to_value(cadmpeg_ir::examples::unit_cube().expect("build unit cube"))
                .expect("serialize unit cube");
        let observed = super::observe_carried(&[document]);
        assert!(
            observed.variants.contains("BodyKind::Solid"),
            "{:?}",
            observed.variants
        );
        assert!(
            observed.variants.contains("Sense::Forward"),
            "{:?}",
            observed.variants
        );
    }

    #[test]
    fn registry_reads_all_three_tag_styles() {
        let document = json!({
            "model": {
                "faces": [{"id": "a", "sense": "forward"}],
                "surfaces": [{"id": "b", "geometry": {"kind": "plane"}}],
                "bodies": [{"id": "c", "kind": {"solid": {}}}],
            }
        });
        let variants = json_variants(&document);
        assert!(
            variants.contains("Sense::Forward")
                || variants
                    .iter()
                    .any(|variant| variant.ends_with("::Forward")),
            "{variants:?}"
        );
        assert!(
            variants.contains("SolvedSurfaceGeometry::Plane"),
            "{variants:?}"
        );
    }

    #[test]
    fn arenas_and_paths_observe_populated_rows_only() {
        let document = json!({
            "model": {
                "faces": [{"id": "a", "loop": serde_json::Value::Null}],
                "edges": [],
            },
            "native": {"step": {"drawing_targets": [{"id": "b"}]}},
        });
        let arenas = observe_arenas(&document);
        assert!(arenas.contains("faces"), "{arenas:?}");
        assert!(arenas.contains("native.step.drawing_targets"), "{arenas:?}");
        assert!(!arenas.contains("edges"), "{arenas:?}");
        let paths = observe_paths(&document);
        assert!(paths.contains("model.faces[].id"), "{paths:?}");
        assert!(!paths.iter().any(|path| path.contains("loop")), "{paths:?}");
    }

    #[test]
    fn ambiguity_reports_shared_spellings() {
        drop(ambiguity_sample(&json!({"kind": "plane"})));
    }

    #[test]
    fn structural_single_key_maps_attribute_no_variant() {
        // An edge carrier is a struct, not an externally-tagged enum. The
        // release cross-closure misread `{"curve": <id>}` as
        // `VariableBlendSupportKind::Curve` (a unit variant, which never
        // serializes as a single-key object) and dropped every carried edge
        // from the required set: `step:data:edge#19` in ap203_sheet.p21,
        // `iges:model:edge#D55:D35:1` in explicit_tetrahedron_solid.igs, and
        // the coedges that need them.
        let carrier = json!({"carrier": {"curve": "step:data:curve#17"}});
        assert!(json_variants(&carrier).is_empty(), "{carrier:?}");
        let projected = json!({
            "carrier": {"curve": "iges:model:curve#D21"},
            "end": "iges:model:vertex#D55:D33:2",
            "id": "iges:model:edge#D55:D35:1",
            "start": "iges:model:vertex#D55:D33:1",
        });
        assert!(json_variants(&projected).is_empty(), "{projected:?}");
    }

    #[test]
    fn bare_unit_spellings_still_attribute() {
        assert!(json_variants(&json!("curve")).contains("VariableBlendSupportKind::Curve"));
        assert!(json_variants(&json!("forward")).contains("Sense::Forward"));
    }

    #[test]
    fn payload_single_key_objects_still_attribute() {
        // `ModelEvaluationIdentity::Curve` carries a payload, so its
        // single-key object form is genuine.
        let identity = json!({"Curve": {"lane": "model"}});
        assert!(
            json_variants(&identity).contains("ModelEvaluationIdentity::Curve"),
            "{identity:?}"
        );
    }
}
