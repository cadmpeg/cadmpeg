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
    /// A solid body whose faces use an edge of extent once, where the
    /// format bounds a solid by closed shells.
    OpenSolid,
    /// A body whose kind differs from the kind its shapes state, where the
    /// format derives a body's kind from its shapes: a solid from closed
    /// shells, a sheet from faces, a wire from edges and a general body from
    /// vertices or mixed members.
    DerivedBodyKind,
    /// A primitive feature with a literal dimension, where the format holds
    /// each primitive dimension as a property parameter.
    LiteralPrimitiveDimension,
    /// A part definition that holds bodies itself, where the format holds a
    /// body in an object inside the part.
    PartOwnedBodies,
    /// A product definition with no source name or one that is no
    /// identifier, where the format names each product object by an
    /// identifier.
    NonIdentifierProductName,
    /// A surface whose solved geometry a construction also states, where
    /// the format holds the construction alone.
    SolvedSurfaceBesideConstruction,
    /// A feature of a native kind that is no `Module::Class` type name, with
    /// the parameters it owns, where the format holds a feature by its type.
    ForeignNativeFeature,
    /// A parameter with a text value, where the format holds text in no
    /// parameter.
    TextParameter,
    /// A surface of unknown geometry with no construction, or stated only by
    /// a construction other than an extrusion, revolution, offset or
    /// parameter subset, with the faces on it and the shells and bodies that
    /// hold those faces, where the format writes a surface from its geometry
    /// or one of those constructions.
    UnencodedSurface,
    /// A shell with faces beside wire edges or free vertices, with its
    /// region and body, where the format's shell holds faces only.
    MixedShell,
    /// An appearance that binds no body, face, edge, vertex or occurrence,
    /// where the format holds an appearance on the view provider of the
    /// object it colours.
    UnboundAppearance,
    /// A pcurve that no coedge uses, where the format holds a pcurve on the
    /// edge whose coedge uses it.
    OrphanPcurve,
    /// A pcurve whose stated parameter range descends, where the format's
    /// pcurve range ascends.
    DescendingPcurveRange,
    /// A feature whose definition references a record outside the
    /// document's model, where the format's features reference objects it
    /// writes.
    FeatureOutsideModel,
    /// A feature whose definition leaves a value unresolved, where the
    /// format's features state each operand.
    UnresolvedFeature,
    /// A pcurve on a face bounded by an edge with a curve and no parameter
    /// range, where the format holds an edge's range with its curve and so
    /// writes neither the edge nor the face.
    RangelessEdgePcurve,
    /// A surface constructed by a construction kind the format re-reads as
    /// an extrusion, revolution or offset of another kind.
    ReinterpretedConstructionSurface,
    /// A pcurve that a coedge uses after its first, where the format holds
    /// one pcurve of a coedge on its face's surface.
    SecondCoedgePcurve,
    /// A parameter whose literal expression differs from the canonical
    /// spelling of its value, where the format stores a literal as its
    /// value.
    UncanonicalLiteralExpression,
    /// A sketch feature whose name differs from its sketch's name, where the
    /// format holds one label for both.
    DistinctSketchFeatureName,
    /// A sketch constraint of a native kind of another format, where the
    /// format holds its own constraint kinds.
    ForeignNativeConstraint,
    /// A parameter that a sketch constraint binds, where the format names a
    /// constraint's parameter by its position.
    ConstraintParameter,
    /// An attribute whose values no single property type holds, where the
    /// format stores an attribute as one typed property.
    UnencodedAttribute,
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
            Self::OpenSolid => return open_solids(document),
            Self::DerivedBodyKind => return bodies_off_their_shapes(document),
            Self::UnencodedSurface => return unencoded_surfaces(document),
            Self::MixedShell => return mixed_shells(document),
            Self::UnboundAppearance => return unbound_appearances(document),
            Self::OrphanPcurve => return orphan_pcurves(document),
            Self::DescendingPcurveRange => return descending_pcurves(document),
            Self::FeatureOutsideModel => return features_outside_model(document),
            Self::DistinctSketchFeatureName => return distinct_sketch_feature_names(document),
            Self::ConstraintParameter => return constraint_parameters(document),
            Self::RangelessEdgePcurve => return rangeless_edge_pcurves(document),
            Self::SecondCoedgePcurve => return second_coedge_pcurves(document),
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
            | Self::UnstatedTolerance
            | Self::OpenSolid
            | Self::DerivedBodyKind
            | Self::UnencodedSurface
            | Self::MixedShell
            | Self::UnboundAppearance
            | Self::OrphanPcurve
            | Self::DescendingPcurveRange
            | Self::FeatureOutsideModel
            | Self::DistinctSketchFeatureName
            | Self::ConstraintParameter
            | Self::RangelessEdgePcurve
            | Self::SecondCoedgePcurve => record
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
            Self::LiteralPrimitiveDimension => {
                arena == "features"
                    && matches!(
                        record
                            .pointer("/definition/definition")
                            .and_then(Value::as_str),
                        Some("primitive" | "block")
                    )
                    && record.get("definition").is_some_and(holds_literal)
            }
            Self::PartOwnedBodies => {
                arena == "product_definitions"
                    && record.get("kind").and_then(Value::as_str) == Some("part")
                    && record
                        .get("bodies")
                        .and_then(Value::as_array)
                        .is_some_and(|bodies| !bodies.is_empty())
            }
            Self::SolvedSurfaceBesideConstruction => {
                arena == "surfaces"
                    && record
                        .pointer("/geometry/kind")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| !matches!(kind, "procedural" | "unknown"))
                    && record
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| constructed_surface(document, id))
            }
            Self::ForeignNativeFeature => match arena {
                "features" => foreign_native_feature(record),
                "parameters" => record
                    .get("owner")
                    .and_then(Value::as_str)
                    .is_some_and(|owner| {
                        model_rows(document, "features").into_iter().any(|feature| {
                            feature.get("id").and_then(Value::as_str) == Some(owner)
                                && foreign_native_feature(feature)
                        })
                    }),
                _ => false,
            },
            Self::TextParameter => {
                arena == "parameters"
                    && record.pointer("/value/kind").and_then(Value::as_str) == Some("string")
            }
            Self::UncanonicalLiteralExpression => {
                arena == "parameters" && uncanonical_literal(record)
            }
            Self::ReinterpretedConstructionSurface => {
                arena == "surfaces"
                    && record.get("id").and_then(Value::as_str).is_some_and(|id| {
                        model_rows(document, "procedural_surfaces")
                            .into_iter()
                            .any(|row| {
                                row.get("surface").and_then(Value::as_str) == Some(id)
                                    && matches!(
                                        row.pointer("/definition/kind").and_then(Value::as_str),
                                        Some(
                                            "parallel_offset" | "axis_revolution" | "linear_sweep"
                                        )
                                    )
                            })
                    })
            }
            Self::UnresolvedFeature => {
                arena == "features" && record.get("definition").is_some_and(holds_unresolved)
            }
            Self::ForeignNativeConstraint => {
                arena == "sketch_constraints"
                    && record.pointer("/definition/kind").and_then(Value::as_str) == Some("native")
            }
            Self::UnencodedAttribute => {
                arena == "attributes"
                    && !record
                        .get("values")
                        .and_then(Value::as_array)
                        .is_some_and(|values| encodable_attribute(values))
            }
            Self::NonIdentifierProductName => {
                arena == "product_definitions"
                    && !record
                        .get("source_name")
                        .and_then(Value::as_str)
                        .is_some_and(is_identifier)
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

/// Whether `value` holds a literal dimension.
fn holds_literal(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            fields.get("kind").and_then(Value::as_str) == Some("literal")
                || fields.values().any(holds_literal)
        }
        Value::Array(items) => items.iter().any(holds_literal),
        _ => false,
    }
}

/// Whether `text` is an ASCII identifier: a letter or underscore, then
/// letters, digits and underscores.
fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// The bodies whose stated kind differs from the kind their shells state.
/// A shell closes when each edge with a curve that its faces use is used
/// equally often in each sense.
fn bodies_off_their_shapes(document: &Value) -> BTreeSet<String> {
    let model = document.get("model");
    let rows = |arena: &str| -> BTreeMap<String, &Value> {
        model
            .and_then(|model| model.get(arena))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|row| Some((row.get("id")?.as_str()?.to_owned(), row)))
            .collect()
    };
    let (regions, shells, faces, loops, coedges, edges) = (
        rows("regions"),
        rows("shells"),
        rows("faces"),
        rows("loops"),
        rows("coedges"),
        rows("edges"),
    );
    let ids = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect()
    };
    let closed = |shell: &Value| -> bool {
        let mut uses = BTreeMap::<&str, [usize; 2]>::new();
        for member in shell
            .get("members")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if member.get("kind").and_then(Value::as_str) != Some("face") {
                continue;
            }
            let Some(face) = member
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| faces.get(id))
            else {
                return false;
            };
            let wires = face.pointer("/loops/loops").or_else(|| face.get("loops"));
            for wire in ids(wires) {
                let Some(wire) = loops.get(&wire) else {
                    return false;
                };
                for coedge in ids(wire.pointer("/boundary/coedges")) {
                    let Some(coedge) = coedges.get(&coedge) else {
                        return false;
                    };
                    let Some(edge) = coedge.get("edge").and_then(Value::as_str) else {
                        return false;
                    };
                    let curved = edges
                        .get(edge)
                        .and_then(|edge| edge.pointer("/carrier/curve"))
                        .is_some_and(|curve| !curve.is_null());
                    if curved {
                        let reversed =
                            coedge.get("sense").and_then(Value::as_str) == Some("reversed");
                        uses.entry(edge).or_default()[usize::from(reversed)] += 1;
                    }
                }
            }
        }
        !uses.is_empty()
            && uses
                .values()
                .all(|&[forward, reversed]| forward > 0 && forward == reversed)
    };
    let mut members = BTreeSet::new();
    for (id, body) in rows("bodies") {
        let mut kinds = BTreeSet::new();
        let mut all_closed = true;
        for region in ids(body.get("regions")) {
            let Some(region) = regions.get(&region) else {
                continue;
            };
            for shell in ids(region.get("shells")) {
                let Some(shell) = shells.get(&shell) else {
                    continue;
                };
                let mut faced = false;
                for member in shell
                    .get("members")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    match member.get("kind").and_then(Value::as_str) {
                        Some("face") => faced = true,
                        Some("wire_edge") => {
                            kinds.insert("wire");
                        }
                        Some("free_vertex") => {
                            kinds.insert("general");
                        }
                        _ => {}
                    }
                }
                if faced {
                    kinds.insert("faces");
                    all_closed &= closed(shell);
                }
            }
        }
        let derived = match kinds.iter().copied().collect::<Vec<_>>().as_slice() {
            [] => continue,
            ["faces"] if all_closed => "solid",
            ["faces"] => "sheet",
            [single] => *single,
            _ => "general",
        };
        if body.get("kind").and_then(Value::as_str) != Some(derived) {
            members.insert(id);
        }
    }
    members
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
            // A solved carrier with no point at a range end holds that
            // range outside its domain.
            Err(_) => Ok(index
                .curves(curve.as_str(), cadmpeg_ir::index::StandardIndex)
                .and_then(|curve| curve.geometry.solved())
                .is_some_and(|geometry| {
                    !matches!(
                        geometry,
                        cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { .. }
                    )
                })),
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

fn open_solids(document: &Value) -> BTreeSet<String> {
    fn text<'v>(row: &'v Value, key: &str) -> Option<&'v str> {
        row.get(key).and_then(Value::as_str)
    }
    let rows = |arena: &str| {
        document
            .get("model")
            .and_then(|model| model.get(arena))
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let owner = |arena: &str, key: &str| -> BTreeMap<&str, &str> {
        rows(arena)
            .iter()
            .filter_map(|row| Some((text(row, "id")?, text(row, key)?)))
            .collect()
    };
    let region_body = owner("regions", "body");
    let shell_region = owner("shells", "region");
    let face_shell = owner("faces", "shell");
    let loop_face = owner("loops", "face");
    let degenerate_curves: BTreeSet<&str> = rows("curves")
        .iter()
        .filter(|curve| {
            curve.pointer("/geometry/kind").and_then(Value::as_str) == Some("degenerate")
        })
        .filter_map(|curve| text(curve, "id"))
        .collect();
    // An edge collapsed to one vertex on no carrier, or on a degenerate one,
    // closes a loop at a singular point and has no second use.
    let collapsed: BTreeSet<&str> = rows("edges")
        .iter()
        .filter(|edge| {
            text(edge, "start") == text(edge, "end")
                && edge
                    .pointer("/carrier/curve")
                    .and_then(Value::as_str)
                    .is_none_or(|curve| degenerate_curves.contains(curve))
        })
        .filter_map(|edge| text(edge, "id"))
        .collect();
    let mut uses = BTreeMap::<(&str, &str), usize>::new();
    for coedge in rows("coedges") {
        let body = text(coedge, "owner_loop")
            .and_then(|lp| loop_face.get(lp))
            .and_then(|face| face_shell.get(face))
            .and_then(|shell| shell_region.get(shell))
            .and_then(|region| region_body.get(region));
        if let (Some(body), Some(edge)) = (body, text(coedge, "edge")) {
            if !collapsed.contains(edge) {
                *uses.entry((body, edge)).or_default() += 1;
            }
        }
    }
    let solids: BTreeSet<&str> = rows("bodies")
        .iter()
        .filter(|body| text(body, "kind") == Some("solid"))
        .filter_map(|body| text(body, "id"))
        .collect();
    uses.into_iter()
        .filter(|((body, _), count)| *count == 1 && solids.contains(body))
        .map(|((body, _), _)| body.to_owned())
        .collect()
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

/// The shells holding faces beside other members, with their regions and
/// bodies.
fn mixed_shells(document: &Value) -> BTreeSet<String> {
    let mut members = BTreeSet::new();
    for shell in model_rows(document, "shells") {
        let kinds: BTreeSet<&str> = shell
            .get("members")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|member| member.get("kind").and_then(Value::as_str))
            .collect();
        if !(kinds.contains("face") && kinds.len() > 1) {
            continue;
        }
        let Some(id) = row_id(shell) else {
            continue;
        };
        let region = shell
            .get("region")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let body = region.as_deref().and_then(|region| {
            model_rows(document, "regions")
                .into_iter()
                .find(|row| row.get("id").and_then(Value::as_str) == Some(region))
                .and_then(|row| row.get("body").and_then(Value::as_str))
                .map(str::to_owned)
        });
        members.insert(id);
        members.extend(region);
        members.extend(body);
    }
    members
}

/// The surfaces of unknown geometry or stated only by a construction the
/// format cannot hold, the faces on them, and the shells and bodies holding
/// those faces.
fn unencoded_surfaces(document: &Value) -> BTreeSet<String> {
    let rows = |arena: &str| -> Vec<&Value> {
        document
            .get("model")
            .and_then(|model| model.get(arena))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect()
    };
    let id = |row: &Value| row.get("id").and_then(Value::as_str).map(str::to_owned);
    let held: BTreeSet<String> = rows("procedural_surfaces")
        .into_iter()
        .filter(|row| {
            matches!(
                row.pointer("/definition/kind").and_then(Value::as_str),
                Some(
                    "extrusion"
                        | "revolution"
                        | "axis_revolution"
                        | "offset"
                        | "parallel_offset"
                        | "subset"
                )
            )
        })
        .filter_map(id)
        .collect();
    let surfaces: BTreeSet<String> = rows("surfaces")
        .into_iter()
        .filter(|row| {
            let kind = row.pointer("/geometry/kind").and_then(Value::as_str);
            let unknown = kind == Some("unknown")
                && !row_id(row).is_some_and(|id| constructed_surface(document, &id));
            unknown
                || (kind == Some("procedural")
                    && row.pointer("/geometry/cache").is_none_or(Value::is_null)
                    && !row
                        .pointer("/geometry/construction")
                        .and_then(Value::as_str)
                        .is_some_and(|construction| held.contains(construction)))
        })
        .filter_map(id)
        .collect();
    let faces: BTreeSet<String> = rows("faces")
        .into_iter()
        .filter(|row| {
            row.get("surface")
                .and_then(Value::as_str)
                .is_some_and(|surface| surfaces.contains(surface))
        })
        .filter_map(id)
        .collect();
    let refers = |row: &Value, key: &str, targets: &BTreeSet<String>| {
        row.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|item| {
                item.as_str()
                    .or_else(|| item.get("id").and_then(Value::as_str))
                    .is_some_and(|target| targets.contains(target))
            })
    };
    let shells: BTreeSet<String> = rows("shells")
        .into_iter()
        .filter(|row| refers(row, "members", &faces))
        .filter_map(id)
        .collect();
    let regions: BTreeSet<String> = rows("regions")
        .into_iter()
        .filter(|row| refers(row, "shells", &shells))
        .filter_map(id)
        .collect();
    let bodies: BTreeSet<String> = rows("bodies")
        .into_iter()
        .filter(|row| refers(row, "regions", &regions))
        .filter_map(id)
        .collect();
    surfaces
        .into_iter()
        .chain(faces)
        .chain(shells)
        .chain(bodies)
        .collect()
}

fn model_rows<'a>(document: &'a Value, arena: &str) -> Vec<&'a Value> {
    document
        .get("model")
        .and_then(|model| model.get(arena))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect()
}

fn row_id(row: &Value) -> Option<String> {
    row.get("id").and_then(Value::as_str).map(str::to_owned)
}

/// Every string at any depth of `value`.
fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

fn foreign_native_feature(record: &Value) -> bool {
    record
        .pointer("/definition/definition")
        .and_then(Value::as_str)
        == Some("native")
        && !record
            .pointer("/definition/kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.contains("::"))
}

/// Whether `value` holds an unresolved value.
fn holds_unresolved(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            fields.get("kind").and_then(Value::as_str) == Some("unresolved")
                || fields.get("definition").and_then(Value::as_str) == Some("unresolved")
                || fields.values().any(holds_unresolved)
        }
        Value::Array(items) => items.iter().any(holds_unresolved),
        _ => false,
    }
}

fn second_coedge_pcurves(document: &Value) -> BTreeSet<String> {
    model_rows(document, "coedges")
        .into_iter()
        .flat_map(|coedge| {
            coedge
                .get("pcurves")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .skip(1)
        })
        .filter_map(|usage| {
            usage
                .get("pcurve")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn rangeless_edge_pcurves(document: &Value) -> BTreeSet<String> {
    let rangeless: BTreeSet<String> = model_rows(document, "edges")
        .into_iter()
        .filter(|row| {
            row.pointer("/carrier/curve")
                .is_some_and(|curve| !curve.is_null())
                && row
                    .pointer("/carrier/param_range")
                    .is_none_or(Value::is_null)
        })
        .filter_map(row_id)
        .collect();
    let face_of: BTreeMap<String, String> = model_rows(document, "loops")
        .into_iter()
        .filter_map(|row| Some((row_id(row)?, row.get("face")?.as_str()?.to_owned())))
        .collect();
    let face = |coedge: &Value| {
        coedge
            .get("owner_loop")
            .and_then(Value::as_str)
            .and_then(|wire| face_of.get(wire))
            .cloned()
    };
    let unwritten: BTreeSet<String> = model_rows(document, "coedges")
        .into_iter()
        .filter(|coedge| {
            coedge
                .get("edge")
                .and_then(Value::as_str)
                .is_some_and(|edge| rangeless.contains(edge))
        })
        .filter_map(face)
        .collect();
    pcurve_uses(document)
        .into_iter()
        .filter(|(coedge, _)| face(coedge).is_some_and(|face| unwritten.contains(&face)))
        .filter_map(|(_, usage)| {
            usage
                .get("pcurve")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn unbound_appearances(document: &Value) -> BTreeSet<String> {
    let held = |target: &Value| match target.get("kind").and_then(Value::as_str) {
        Some("body" | "face" | "edge" | "vertex" | "occurrence") => true,
        Some("record") => matches!(
            target.get("arena").and_then(Value::as_str),
            Some("bodies" | "faces" | "edges" | "vertices" | "occurrences")
        ),
        _ => false,
    };
    let bound: BTreeSet<&str> = model_rows(document, "appearance_bindings")
        .into_iter()
        .filter(|row| row.get("target").is_some_and(held))
        .filter_map(|row| row.get("appearance").and_then(Value::as_str))
        .collect();
    model_rows(document, "appearances")
        .into_iter()
        .filter_map(row_id)
        .filter(|id| !bound.contains(id.as_str()))
        .collect()
}

/// The pcurve uses of every coedge: the coedge and the use's record.
fn pcurve_uses(document: &Value) -> Vec<(&Value, &Value)> {
    model_rows(document, "coedges")
        .into_iter()
        .flat_map(|coedge| {
            coedge
                .get("pcurves")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(move |usage| (coedge, usage))
        })
        .collect()
}

fn orphan_pcurves(document: &Value) -> BTreeSet<String> {
    let used: BTreeSet<&str> = pcurve_uses(document)
        .into_iter()
        .filter_map(|(_, usage)| usage.get("pcurve").and_then(Value::as_str))
        .collect();
    model_rows(document, "pcurves")
        .into_iter()
        .filter_map(row_id)
        .filter(|id| !used.contains(id.as_str()))
        .collect()
}

fn descending(range: Option<&Value>) -> bool {
    let bounds = range
        .and_then(Value::as_array)
        .map(|range| range.iter().filter_map(Value::as_f64).collect::<Vec<_>>());
    matches!(bounds.as_deref(), Some([start, end]) if start > end)
}

fn descending_pcurves(document: &Value) -> BTreeSet<String> {
    let mut members: BTreeSet<String> = model_rows(document, "pcurves")
        .into_iter()
        .filter(|row| descending(row.pointer("/metadata/form/parameter_range")))
        .filter_map(row_id)
        .collect();
    for (_, usage) in pcurve_uses(document) {
        if descending(usage.get("parameter_range")) {
            if let Some(pcurve) = usage.get("pcurve").and_then(Value::as_str) {
                members.insert(pcurve.to_owned());
            }
        }
    }
    members
}

fn features_outside_model(document: &Value) -> BTreeSet<String> {
    let model: BTreeSet<String> = document
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|model| model.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(row_id)
        .collect();
    // A reference to a feature's native record names that feature.
    let features: BTreeSet<&str> = model_rows(document, "features")
        .into_iter()
        .filter_map(|row| row.get("native_ref").and_then(Value::as_str))
        .collect();
    model_rows(document, "features")
        .into_iter()
        .filter(|row| {
            let mut texts = Vec::new();
            if let Some(definition) = row.get("definition") {
                strings(definition, &mut texts);
            }
            texts.iter().any(|text| {
                text.contains('#')
                    && text.contains(':')
                    && !model.contains(*text)
                    && !features.contains(text)
            })
        })
        .filter_map(row_id)
        .collect()
}

fn distinct_sketch_feature_names(document: &Value) -> BTreeSet<String> {
    let sketches: BTreeMap<String, Option<&str>> = model_rows(document, "sketches")
        .into_iter()
        .filter_map(|row| Some((row_id(row)?, row.get("name").and_then(Value::as_str))))
        .collect();
    model_rows(document, "features")
        .into_iter()
        .filter(|row| {
            let mut texts = Vec::new();
            if let Some(definition) = row.get("definition") {
                strings(definition, &mut texts);
            }
            let name = row.get("name").and_then(Value::as_str);
            texts
                .iter()
                .any(|text| sketches.get(*text).is_some_and(|sketch| *sketch != name))
        })
        .filter_map(row_id)
        .collect()
}

fn constraint_parameters(document: &Value) -> BTreeSet<String> {
    let parameters: BTreeSet<String> = model_rows(document, "parameters")
        .into_iter()
        .filter_map(row_id)
        .collect();
    let mut members = BTreeSet::new();
    for constraint in model_rows(document, "sketch_constraints") {
        let mut texts = Vec::new();
        if let Some(definition) = constraint.get("definition") {
            strings(definition, &mut texts);
        }
        members.extend(
            texts
                .into_iter()
                .filter(|text| parameters.contains(*text))
                .map(str::to_owned),
        );
    }
    members
}

/// Whether a parameter states a one-token literal expression that differs
/// from its value's canonical spelling only by spacing.
fn uncanonical_literal(record: &Value) -> bool {
    let Some([token]) = record
        .pointer("/expression/tokens")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    else {
        return false;
    };
    let Some(text) = token.get("value").and_then(Value::as_str) else {
        return false;
    };
    let Ok(value) =
        serde_json::from_value::<cadmpeg_ir::features::ParameterValue>(record["value"].clone())
    else {
        return false;
    };
    let canonical = serde_json::to_value(cadmpeg_ir::features::Expression::literal(&value))
        .ok()
        .and_then(|expression| {
            expression
                .pointer("/tokens/0/value")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    canonical.is_some_and(|canonical| {
        canonical != text && canonical.replace(' ', "") == text.replace(' ', "")
    })
}

/// Whether one property type holds `values`: one string, boolean, integer,
/// float, three-component vector or reference, or a list of strings.
fn encodable_attribute(values: &[Value]) -> bool {
    let kind = |value: &Value| value.get("kind").and_then(Value::as_str).map(str::to_owned);
    match values {
        [single] => match kind(single).as_deref() {
            Some("string" | "boolean" | "integer" | "float" | "reference") => true,
            Some("vector") => single
                .get("value")
                .and_then(Value::as_array)
                .is_some_and(|vector| vector.len() == 3),
            _ => false,
        },
        [] => false,
        values => values
            .iter()
            .all(|value| value.is_null() || kind(value).as_deref() == Some("string")),
    }
}

/// Whether a procedural surface of `document` constructs `surface`.
fn constructed_surface(document: &Value, surface: &str) -> bool {
    document
        .get("model")
        .and_then(|model| model.get("procedural_surfaces"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|row| row.get("surface").and_then(Value::as_str) == Some(surface))
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
    use super::{ambiguity_sample, json_variants, observe_arenas, observe_paths, UnheldClass};
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
    fn a_body_stated_as_a_solid_on_an_open_shell_holds_a_derived_kind() {
        let body = |kind: &str| {
            json!({"model": {
                "bodies": [{"id": "body", "kind": kind, "regions": ["region"]}],
                "regions": [{"id": "region", "body": "body", "shells": ["shell"]}],
                "shells": [{"id": "shell", "members": [{"kind": "face", "id": "face"}]}],
                "faces": [{"id": "face", "loops": {"loops": ["loop"]}}],
                "loops": [{"id": "loop", "face": "face",
                           "boundary": {"kind": "ring", "coedges": ["coedge"]}}],
                "coedges": [{"id": "coedge", "edge": "edge", "sense": "forward"}],
                "edges": [{"id": "edge", "carrier": {"curve": "curve"}}]
            }})
        };
        assert!(UnheldClass::DerivedBodyKind
            .members(&body("solid"))
            .contains("body"));
        assert!(UnheldClass::DerivedBodyKind
            .members(&body("sheet"))
            .is_empty());
    }

    #[test]
    fn freecad_unheld_classes_name_their_records() {
        let document = json!({"model": {
            "features": [
                {"id": "box", "definition": {"definition": "primitive",
                    "solid": {"kind": "box", "length": {"kind": "literal", "value": 2.0}}}},
                {"id": "native", "definition": {"definition": "native", "kind": "Decal"}},
                {"id": "own", "definition": {"definition": "native", "kind": "Part::Thing"}}
            ],
            "parameters": [
                {"id": "owned", "owner": "native", "value": {"kind": "length", "value": 1.0}},
                {"id": "text", "value": {"kind": "string", "value": "x"}}
            ],
            "product_definitions": [
                {"id": "part", "kind": "part", "bodies": ["body"], "source_name": "Part1"},
                {"id": "unnamed", "kind": "object"}
            ],
            "appearances": [{"id": "curve-colour"}, {"id": "face-colour"}],
            "appearance_bindings": [
                {"id": "b1", "appearance": "curve-colour", "target": {"kind": "curve", "id": "c"}},
                {"id": "b2", "appearance": "face-colour", "target": {"kind": "face", "id": "f"}}
            ],
            "attributes": [
                {"id": "vector4", "values": [{"kind": "vector", "value": [1.0, 2.0, 3.0, 4.0]}]},
                {"id": "string", "values": [{"kind": "string", "value": "a"}]}
            ]
        }});
        let members = |class: UnheldClass| class.members(&document);
        assert_eq!(
            members(UnheldClass::LiteralPrimitiveDimension),
            ["box".to_owned()].into()
        );
        assert_eq!(
            members(UnheldClass::ForeignNativeFeature),
            ["native".to_owned(), "owned".to_owned()].into()
        );
        assert_eq!(
            members(UnheldClass::TextParameter),
            ["text".to_owned()].into()
        );
        assert_eq!(
            members(UnheldClass::PartOwnedBodies),
            ["part".to_owned()].into()
        );
        assert_eq!(
            members(UnheldClass::NonIdentifierProductName),
            ["unnamed".to_owned()].into()
        );
        assert_eq!(
            members(UnheldClass::UnboundAppearance),
            ["curve-colour".to_owned()].into()
        );
        assert_eq!(
            members(UnheldClass::UnencodedAttribute),
            ["vector4".to_owned()].into()
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
