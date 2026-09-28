// SPDX-License-Identifier: Apache-2.0
//! Bounded framing for text and binary exact-shape side entries.

pub(crate) mod triangulation;

use triangulation::TextTriangulation;

use std::collections::BTreeMap;

use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::scalar::{
    Angle, FiniteBinary32, FiniteReal, NonNegativeLength, NonNegativeReal, NonZeroLength,
    PositiveLength, PositiveReal,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{FinitePoint2, OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::SourceObjectAssociation;
use serde::{Deserialize, Serialize};

use crate::native::{self, EntryRecord, PropertyRecord};
use crate::resource::{
    collection_vec, optional_collection_vec, reserve_vec_items, retained_format, retained_string,
    retained_strings,
};

/// Exact-shape side-entry form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ShapePayloadForm {
    /// Explicit zero-byte null shape.
    Empty,
    /// Compact text shape-set grammar.
    Text,
    /// Binary shape-set grammar.
    Binary,
}

/// One exact-shape property bound to its side entry.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "ShapePayloadRecordWire")]
pub(crate) struct ShapePayloadRecord {
    /// Stable payload identity.
    pub(crate) id: String,
    /// Owning property identity.
    pub(crate) property: String,
    /// Side-entry identity.
    pub(crate) entry: String,
    /// Carrier payload.
    pub(crate) payload: ShapePayload,
}

/// Supported text topology grammar versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTopologyVersion {
    /// Version 1.
    V1,
    /// Version 2.
    V2,
    /// Version 3.
    V3,
}

impl TextTopologyVersion {
    /// Returns the wire version number.
    const fn number(self) -> u8 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
            Self::V3 => 3,
        }
    }
}

impl TryFrom<u8> for TextTopologyVersion {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::V1),
            2 => Ok(Self::V2),
            3 => Ok(Self::V3),
            _ => Err("text topology_version must be in 1..=3".to_owned()),
        }
    }
}

/// Supported binary topology grammar versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryTopologyVersion {
    /// Version 1.
    V1,
    /// Version 2.
    V2,
    /// Version 3.
    V3,
    /// Version 4.
    V4,
}

impl BinaryTopologyVersion {
    /// Returns the wire version number.
    const fn number(self) -> u8 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
            Self::V3 => 3,
            Self::V4 => 4,
        }
    }
}

impl TryFrom<u8> for BinaryTopologyVersion {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::V1),
            2 => Ok(Self::V2),
            3 => Ok(Self::V3),
            4 => Ok(Self::V4),
            _ => Err("binary topology_version must be in 1..=4".to_owned()),
        }
    }
}

/// Parsed exact-shape carrier.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShapePayload {
    /// Explicit zero-byte null shape.
    Empty,
    /// Compact text shape-set grammar.
    Text {
        /// Text grammar version.
        version: TextTopologyVersion,
        /// Shared table contents.
        facts: ShapeSet,
    },
    /// Binary shape-set grammar.
    Binary {
        /// Binary grammar version.
        version: BinaryTopologyVersion,
        /// Shared table contents.
        facts: ShapeSet,
    },
}

impl ShapePayload {
    /// Carrier form retained on the CADIR wire.
    const fn form(&self) -> ShapePayloadForm {
        match self {
            Self::Empty => ShapePayloadForm::Empty,
            Self::Text { .. } => ShapePayloadForm::Text,
            Self::Binary { .. } => ShapePayloadForm::Binary,
        }
    }

    /// Returns the grammar version for a nonempty carrier.
    pub(crate) const fn topology_version(&self) -> Option<u8> {
        match self {
            Self::Empty => None,
            Self::Text { version, .. } => Some(version.number()),
            Self::Binary { version, .. } => Some(version.number()),
        }
    }

    /// Shared table contents when the carrier is text or binary.
    pub(crate) const fn shape_set(&self) -> Option<&ShapeSet> {
        match self {
            Self::Empty => None,
            Self::Text { facts, .. } | Self::Binary { facts, .. } => Some(facts),
        }
    }
}

/// Versioned prefix tables shared by text and binary shape sets.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapeSet {
    /// Ordered location table with resolved transforms.
    locations: Vec<TextLocation>,
    /// Ordered parameter-space curve table.
    curve2ds: Vec<TextCurve2d>,
    /// Ordered 3D curve table.
    curves: Vec<TextCurve>,
    /// Ordered standalone 3D polygons.
    polygons3d: Vec<TextPolygon3d>,
    /// Ordered polygons indexing triangulation nodes.
    polygons_on_triangulations: Vec<TextPolygonOnTriangulation>,
    /// Ordered exact surface table.
    surfaces: Vec<TextSurface>,
    /// Ordered display triangulation table.
    triangulations: Vec<TextTriangulation>,
    /// Ordered subshape-first topology records.
    tshapes: TextTShapes,
    /// Root shape uses stored after the shape set.
    roots: Vec<TextShapeUse>,
}

/// Immutable tables from one validated shape set.
#[derive(Clone, Copy)]
pub(crate) struct Tables<'a> {
    pub(crate) locations: &'a [TextLocation],
    pub(crate) curve2ds: &'a [TextCurve2d],
    pub(crate) curves: &'a [TextCurve],
    pub(crate) surfaces: &'a [TextSurface],
    pub(crate) polygons3d: &'a [TextPolygon3d],
    pub(crate) polygons_on_triangulations: &'a [TextPolygonOnTriangulation],
    pub(crate) tshapes: &'a TextTShapes,
    pub(crate) triangulations: &'a [TextTriangulation],
    pub(crate) roots: &'a [TextShapeUse],
}

impl<'a> Tables<'a> {
    pub(crate) fn from_payload(payload: &'a ShapePayloadRecord) -> Option<Self> {
        let set = payload.payload.shape_set()?;
        Some(Self {
            locations: &set.locations,
            curve2ds: &set.curve2ds,
            curves: &set.curves,
            surfaces: &set.surfaces,
            polygons3d: &set.polygons3d,
            polygons_on_triangulations: &set.polygons_on_triangulations,
            tshapes: &set.tshapes,
            triangulations: &set.triangulations,
            roots: &set.roots,
        })
    }
}

impl ShapeSet {
    fn section_counts(&self) -> BTreeMap<String, usize> {
        [
            ("Locations", self.locations.len()),
            ("Curve2ds", self.curve2ds.len()),
            ("Curves", self.curves.len()),
            ("Polygon3D", self.polygons3d.len()),
            (
                "PolygonOnTriangulations",
                self.polygons_on_triangulations.len(),
            ),
            ("Surfaces", self.surfaces.len()),
            ("Triangulations", self.triangulations.len()),
            ("TShapes", self.tshapes.len()),
        ]
        .into_iter()
        .map(|(name, count)| (name.to_owned(), count))
        .collect()
    }

    fn shape_type_counts(&self) -> BTreeMap<String, usize> {
        const KINDS: [&str; 8] = [
            "vertex",
            "edge",
            "wire",
            "face",
            "shell",
            "solid",
            "compsolid",
            "compound",
        ];
        let mut counts = [0_usize; KINDS.len()];
        for shape in self.tshapes.iter() {
            let index = match shape.kind() {
                TextShapeKind::Vertex => 0,
                TextShapeKind::Edge => 1,
                TextShapeKind::Wire => 2,
                TextShapeKind::Face => 3,
                TextShapeKind::Shell => 4,
                TextShapeKind::Solid => 5,
                TextShapeKind::CompSolid => 6,
                TextShapeKind::Compound => 7,
            };
            counts[index] += 1;
        }
        KINDS
            .into_iter()
            .zip(counts)
            .filter(|(_, count)| *count != 0)
            .map(|(name, count)| (name.to_owned(), count))
            .collect()
    }

    /// Validate all cross-table references before a shape set enters CADIR.
    ///
    /// The binary and text readers perform these checks while parsing source
    /// bytes. CADIR can also deserialize a retained shape payload directly,
    /// so the same table bounds and subshape-first relation must be checked at
    /// that admission boundary.
    fn validate(&self) -> Result<(), String> {
        for (position, location) in self.locations.iter().enumerate() {
            for factor in &location.factors {
                validate_one_based(factor.location, position, "location factor reference")?;
            }
        }
        for (position, polygon) in self.polygons3d.iter().enumerate() {
            if polygon
                .parameters
                .as_ref()
                .is_some_and(|parameters| parameters.len() != polygon.nodes.len())
            {
                return Err(format!(
                    "Polygon3D[{position}] parameters length must equal nodes length"
                ));
            }
        }
        for (position, polygon) in self.polygons_on_triangulations.iter().enumerate() {
            if polygon.nodes.contains(&0) {
                return Err(format!(
                    "PolygonOnTriangulations[{position}] node indices must be one-based"
                ));
            }
            if polygon
                .parameters
                .as_ref()
                .is_some_and(|parameters| parameters.len() != polygon.nodes.len())
            {
                return Err(format!(
                    "PolygonOnTriangulations[{position}] parameters length must equal nodes length"
                ));
            }
        }
        for (position, shape) in self.tshapes.iter().enumerate() {
            let shape_index = position + 1;
            for child in &shape.children {
                validate_one_based(child.shape, self.tshapes.len(), "TShape child")?;
                if child.shape >= shape_index {
                    return Err(format!(
                        "TShape {shape_index} references non-prior child {}",
                        child.shape
                    ));
                }
                validate_location_ref(child.location, self.locations.len(), "TShape child")?;
            }
            match &shape.geometry {
                TextTShapeGeometry::Vertex {
                    representations, ..
                } => {
                    for representation in representations {
                        match representation {
                            TextPointRepresentation::Curve3d {
                                curve, location, ..
                            } => {
                                validate_one_based(*curve, self.curves.len(), "vertex curve")?;
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "vertex location",
                                )?;
                            }
                            TextPointRepresentation::Pcurve {
                                curve,
                                surface,
                                location,
                                ..
                            } => {
                                validate_one_based(
                                    *curve,
                                    self.curve2ds.len(),
                                    "vertex parameter curve",
                                )?;
                                validate_one_based(
                                    *surface,
                                    self.surfaces.len(),
                                    "vertex surface",
                                )?;
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "vertex location",
                                )?;
                            }
                            TextPointRepresentation::Surface {
                                surface, location, ..
                            } => {
                                validate_one_based(
                                    *surface,
                                    self.surfaces.len(),
                                    "vertex surface",
                                )?;
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "vertex location",
                                )?;
                            }
                        }
                    }
                }
                TextTShapeGeometry::Edge {
                    representations, ..
                } => {
                    for representation in representations {
                        match representation {
                            TextEdgeRepresentation::Curve3d {
                                curve, location, ..
                            } => {
                                validate_one_based(*curve, self.curves.len(), "edge curve")?;
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "edge curve location",
                                )?;
                            }
                            TextEdgeRepresentation::Pcurve {
                                curve,
                                surface,
                                location,
                                ..
                            } => {
                                validate_one_based(
                                    *curve,
                                    self.curve2ds.len(),
                                    "edge parameter curve",
                                )?;
                                validate_one_based(*surface, self.surfaces.len(), "edge surface")?;
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "edge surface location",
                                )?;
                            }
                            TextEdgeRepresentation::PcurvePair {
                                curves,
                                surface,
                                location,
                                ..
                            } => {
                                for curve in curves {
                                    validate_one_based(
                                        *curve,
                                        self.curve2ds.len(),
                                        "edge parameter curve",
                                    )?;
                                }
                                validate_one_based(*surface, self.surfaces.len(), "edge surface")?;
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "edge surface location",
                                )?;
                            }
                            TextEdgeRepresentation::Regularity {
                                surfaces,
                                locations,
                                ..
                            } => {
                                for surface in surfaces {
                                    validate_one_based(
                                        *surface,
                                        self.surfaces.len(),
                                        "edge regularity surface",
                                    )?;
                                }
                                for location in locations {
                                    validate_optional_index(
                                        *location,
                                        self.locations.len(),
                                        "edge regularity location",
                                    )?;
                                }
                            }
                            TextEdgeRepresentation::Polygon3d { polygon, location } => {
                                validate_one_based(
                                    *polygon,
                                    self.polygons3d.len(),
                                    "edge 3D polygon",
                                )?;
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "edge polygon location",
                                )?;
                            }
                            TextEdgeRepresentation::PolygonOnTriangulation {
                                polygon,
                                triangulation,
                                location,
                            } => {
                                self.validate_indexed_polygon(
                                    *polygon,
                                    *triangulation,
                                    "edge indexed polygon",
                                )?;
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "edge indexed polygon location",
                                )?;
                            }
                            TextEdgeRepresentation::PolygonPair {
                                polygons,
                                triangulation,
                                location,
                            } => {
                                for polygon in polygons {
                                    self.validate_indexed_polygon(
                                        *polygon,
                                        *triangulation,
                                        "edge indexed polygon",
                                    )?;
                                }
                                validate_optional_index(
                                    *location,
                                    self.locations.len(),
                                    "edge indexed polygon location",
                                )?;
                            }
                        }
                    }
                }
                TextTShapeGeometry::Face {
                    surface,
                    location,
                    triangulation,
                    ..
                } => {
                    if let Some(surface) = surface {
                        validate_one_based(surface.index(), self.surfaces.len(), "face surface")?;
                    }
                    validate_location_ref(*location, self.locations.len(), "face location")?;
                    if let Some(triangulation) = triangulation {
                        validate_one_based(
                            triangulation.index(),
                            self.triangulations.len(),
                            "face triangulation",
                        )?;
                    }
                }
                TextTShapeGeometry::Wire
                | TextTShapeGeometry::Shell
                | TextTShapeGeometry::Solid
                | TextTShapeGeometry::CompSolid
                | TextTShapeGeometry::Compound => {}
            }
        }
        for root in &self.roots {
            validate_one_based(root.shape, self.tshapes.len(), "root TShape")?;
            validate_location_ref(root.location, self.locations.len(), "root shape")?;
        }
        Ok(())
    }

    fn validate_indexed_polygon(
        &self,
        polygon: usize,
        triangulation: usize,
        label: &str,
    ) -> Result<(), String> {
        let polygon_record = polygon
            .checked_sub(1)
            .and_then(|index| self.polygons_on_triangulations.get(index))
            .ok_or_else(|| {
                format!(
                    "{label} polygon index {polygon} is out of range 1..={}",
                    self.polygons_on_triangulations.len()
                )
            })?;
        let node_count = triangulation
            .checked_sub(1)
            .and_then(|index| self.triangulations.get(index))
            .ok_or_else(|| {
                format!(
                    "{label} triangulation index {triangulation} is out of range 1..={}",
                    self.triangulations.len()
                )
            })?
            .nodes()
            .len();
        for node in &polygon_record.nodes {
            let node = usize::try_from(*node)
                .map_err(|_| format!("{label} node index does not fit usize"))?;
            validate_one_based(node, node_count, label)?;
        }
        Ok(())
    }
}

fn validate_one_based(index: usize, length: usize, label: &str) -> Result<(), String> {
    if index == 0 || index > length {
        return Err(format!(
            "{label} index {index} is out of range 1..={length}"
        ));
    }
    Ok(())
}

fn validate_optional_index(index: usize, length: usize, label: &str) -> Result<(), String> {
    if index > length {
        return Err(format!(
            "{label} index {index} is out of range 0..={length}"
        ));
    }
    Ok(())
}

fn validate_location_ref(location: LocationRef, length: usize, label: &str) -> Result<(), String> {
    validate_optional_index(location.index(), length, label)
}

#[derive(Deserialize)]
struct ShapePayloadRecordWire {
    id: String,
    property: String,
    entry: String,
    form: ShapePayloadForm,
    text: Option<TextFactsWire>,
    binary: Option<BinaryFactsWire>,
}

#[derive(Deserialize)]
struct TextFactsWire {
    topology_version: u8,
    section_counts: BTreeMap<String, usize>,
    shape_types: BTreeMap<String, usize>,
    locations: Vec<TextLocation>,
    curve2ds: Vec<TextCurve2d>,
    curves: Vec<TextCurve>,
    surfaces: Vec<TextSurface>,
    polygons3d: Vec<TextPolygon3d>,
    polygons_on_triangulations: Vec<TextPolygonOnTriangulation>,
    triangulations: Vec<TextTriangulation>,
    tshapes: TextTShapes,
    roots: Vec<TextShapeUse>,
}

#[derive(Deserialize)]
struct BinaryFactsWire {
    topology_version: u8,
    locations: Vec<TextLocation>,
    curve2ds: Vec<TextCurve2d>,
    curves: Vec<TextCurve>,
    polygons3d: Vec<TextPolygon3d>,
    polygons_on_triangulations: Vec<TextPolygonOnTriangulation>,
    surfaces: Vec<TextSurface>,
    triangulations: Vec<TextTriangulation>,
    tshapes: TextTShapes,
    roots: Vec<TextShapeUse>,
}

/// The retained wire shape, borrowed from the record it states.
///
/// Reading owns the tables it builds; writing needs no copy of them. The text
/// form states two censuses beside its tables, and they are derived, so only
/// they are owned here.
#[derive(Serialize)]
struct ShapePayloadRecordOut<'a> {
    id: &'a str,
    property: &'a str,
    entry: &'a str,
    form: ShapePayloadForm,
    text: Option<TextFactsOut<'a>>,
    binary: Option<BinaryFactsOut<'a>>,
}

#[derive(Serialize)]
struct TextFactsOut<'a> {
    topology_version: u8,
    section_counts: BTreeMap<String, usize>,
    shape_types: BTreeMap<String, usize>,
    locations: &'a [TextLocation],
    curve2ds: &'a [TextCurve2d],
    curves: &'a [TextCurve],
    surfaces: &'a [TextSurface],
    polygons3d: &'a [TextPolygon3d],
    polygons_on_triangulations: &'a [TextPolygonOnTriangulation],
    triangulations: &'a [TextTriangulation],
    tshapes: &'a TextTShapes,
    roots: &'a [TextShapeUse],
}

#[derive(Serialize)]
struct BinaryFactsOut<'a> {
    topology_version: u8,
    locations: &'a [TextLocation],
    curve2ds: &'a [TextCurve2d],
    curves: &'a [TextCurve],
    polygons3d: &'a [TextPolygon3d],
    polygons_on_triangulations: &'a [TextPolygonOnTriangulation],
    surfaces: &'a [TextSurface],
    triangulations: &'a [TextTriangulation],
    tshapes: &'a TextTShapes,
    roots: &'a [TextShapeUse],
}

impl<'a> BinaryFactsOut<'a> {
    fn new(facts: &'a ShapeSet, version: BinaryTopologyVersion) -> Self {
        Self {
            topology_version: version.number(),
            locations: &facts.locations,
            curve2ds: &facts.curve2ds,
            curves: &facts.curves,
            polygons3d: &facts.polygons3d,
            polygons_on_triangulations: &facts.polygons_on_triangulations,
            surfaces: &facts.surfaces,
            triangulations: &facts.triangulations,
            tshapes: &facts.tshapes,
            roots: &facts.roots,
        }
    }
}

impl<'a> TextFactsOut<'a> {
    fn new(facts: &'a ShapeSet, version: TextTopologyVersion) -> Self {
        Self {
            topology_version: version.number(),
            section_counts: facts.section_counts(),
            shape_types: facts.shape_type_counts(),
            locations: &facts.locations,
            curve2ds: &facts.curve2ds,
            curves: &facts.curves,
            surfaces: &facts.surfaces,
            polygons3d: &facts.polygons3d,
            polygons_on_triangulations: &facts.polygons_on_triangulations,
            triangulations: &facts.triangulations,
            tshapes: &facts.tshapes,
            roots: &facts.roots,
        }
    }
}

impl TryFrom<BinaryFactsWire> for ShapeSet {
    type Error = String;

    fn try_from(value: BinaryFactsWire) -> Result<Self, Self::Error> {
        let facts = Self {
            locations: value.locations,
            curve2ds: value.curve2ds,
            curves: value.curves,
            polygons3d: value.polygons3d,
            polygons_on_triangulations: value.polygons_on_triangulations,
            surfaces: value.surfaces,
            triangulations: value.triangulations,
            tshapes: value.tshapes,
            roots: value.roots,
        };
        facts.validate()?;
        Ok(facts)
    }
}

impl Serialize for ShapePayloadRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (text, binary) = match &self.payload {
            ShapePayload::Empty => (None, None),
            ShapePayload::Text { facts, version } => {
                (Some(TextFactsOut::new(facts, *version)), None)
            }
            ShapePayload::Binary { facts, version } => {
                (None, Some(BinaryFactsOut::new(facts, *version)))
            }
        };
        ShapePayloadRecordOut {
            id: &self.id,
            property: &self.property,
            entry: &self.entry,
            form: self.payload.form(),
            text,
            binary,
        }
        .serialize(serializer)
    }
}

impl TryFrom<ShapePayloadRecordWire> for ShapePayloadRecord {
    type Error = String;

    fn try_from(wire: ShapePayloadRecordWire) -> Result<Self, Self::Error> {
        let payload = match (wire.form, wire.text, wire.binary) {
            (ShapePayloadForm::Empty, None, None) => ShapePayload::Empty,
            (ShapePayloadForm::Text, Some(text), None) => {
                let version = TextTopologyVersion::try_from(text.topology_version)?;
                let facts = ShapeSet {
                    locations: text.locations,
                    curve2ds: text.curve2ds,
                    curves: text.curves,
                    polygons3d: text.polygons3d,
                    polygons_on_triangulations: text.polygons_on_triangulations,
                    surfaces: text.surfaces,
                    triangulations: text.triangulations,
                    tshapes: text.tshapes,
                    roots: text.roots,
                };
                facts.validate()?;
                if text.section_counts != facts.section_counts() {
                    return Err(
                        "text shape-set section_counts disagrees with table lengths".to_owned()
                    );
                }
                if text.shape_types != facts.shape_type_counts() {
                    return Err("text shape-set shape_types disagrees with TShape kinds".to_owned());
                }
                ShapePayload::Text { version, facts }
            }
            (ShapePayloadForm::Binary, None, Some(binary)) => {
                let version = BinaryTopologyVersion::try_from(binary.topology_version)?;
                ShapePayload::Binary {
                    version,
                    facts: binary.try_into()?,
                }
            }
            _ => return Err("shape payload form disagrees with text and binary facts".to_owned()),
        };
        Ok(Self {
            id: wire.id,
            property: wire.property,
            entry: wire.entry,
            payload,
        })
    }
}

/// Topological shape family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextShapeKind {
    Vertex,
    Edge,
    Wire,
    Face,
    Shell,
    Solid,
    CompSolid,
    Compound,
}

/// Orientation of one shape use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextOrientation {
    Forward,
    Reversed,
    Internal,
    External,
}

/// One oriented, located use of a topology record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TextShapeUse {
    /// One-based `tshapes` index.
    pub(crate) shape: usize,
    /// Use orientation.
    pub(crate) orientation: TextOrientation,
    /// One-based location index, or zero for identity.
    pub(crate) location: LocationRef,
}

/// One vertex point representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "TextPointRepresentationWire",
    into = "TextPointRepresentationWire"
)]
pub(crate) enum TextPointRepresentation {
    /// Kind 1: point on a 3D curve.
    Curve3d {
        /// Curve parameter.
        parameter: FiniteReal,
        /// One-based 3D curve index.
        curve: usize,
        /// Location index, or zero for identity.
        location: usize,
    },
    /// Kind 2: point on a parameter-space curve of a surface.
    Pcurve {
        /// Parameter-curve parameter.
        parameter: FiniteReal,
        /// One-based 2D curve index.
        curve: usize,
        /// One-based surface index.
        surface: usize,
        /// Location index, or zero for identity.
        location: usize,
    },
    /// Kind 3: point on a surface.
    Surface {
        /// First surface parameter.
        parameter: FiniteReal,
        /// Second surface parameter.
        second_parameter: FiniteReal,
        /// One-based surface index.
        surface: usize,
        /// Location index, or zero for identity.
        location: usize,
    },
}

impl TextPointRepresentation {
    /// Representation family code 1 through 3 retained on the CADIR wire.
    const fn kind(&self) -> u8 {
        match self {
            Self::Curve3d { .. } => 1,
            Self::Pcurve { .. } => 2,
            Self::Surface { .. } => 3,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TextPointRepresentationWire {
    parameter: FiniteReal,
    second_parameter: Option<FiniteReal>,
    kind: u8,
    curve: Option<usize>,
    surface: Option<usize>,
    location: usize,
}

impl From<TextPointRepresentation> for TextPointRepresentationWire {
    fn from(value: TextPointRepresentation) -> Self {
        let kind = value.kind();
        match value {
            TextPointRepresentation::Curve3d {
                parameter,
                curve,
                location,
            } => Self {
                parameter,
                second_parameter: None,
                kind,
                curve: Some(curve),
                surface: None,
                location,
            },
            TextPointRepresentation::Pcurve {
                parameter,
                curve,
                surface,
                location,
            } => Self {
                parameter,
                second_parameter: None,
                kind,
                curve: Some(curve),
                surface: Some(surface),
                location,
            },
            TextPointRepresentation::Surface {
                parameter,
                second_parameter,
                surface,
                location,
            } => Self {
                parameter,
                second_parameter: Some(second_parameter),
                kind,
                curve: None,
                surface: Some(surface),
                location,
            },
        }
    }
}

impl TryFrom<TextPointRepresentationWire> for TextPointRepresentation {
    type Error = String;

    fn try_from(wire: TextPointRepresentationWire) -> Result<Self, Self::Error> {
        match (
            wire.kind,
            wire.parameter,
            wire.second_parameter,
            wire.curve,
            wire.surface,
            wire.location,
        ) {
            (1, parameter, None, Some(curve), None, location) => Ok(Self::Curve3d {
                parameter,
                curve,
                location,
            }),
            (2, parameter, None, Some(curve), Some(surface), location) => Ok(Self::Pcurve {
                parameter,
                curve,
                surface,
                location,
            }),
            (3, parameter, Some(second_parameter), None, Some(surface), location) => {
                Ok(Self::Surface {
                    parameter,
                    second_parameter,
                    surface,
                    location,
                })
            }
            _ => Err("vertex representation kind disagrees with payload fields".to_owned()),
        }
    }
}

/// One edge representation record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "TextEdgeRepresentationWire")]
pub(crate) enum TextEdgeRepresentation {
    /// Kind 1: exact 3D curve.
    Curve3d {
        /// One-based 3D curve index.
        curve: usize,
        /// Location index, or zero for identity.
        location: usize,
        /// Curve parameter range.
        parameter_range: [FiniteReal; 2],
    },
    /// Kind 2: one parameter-space curve on a surface.
    Pcurve {
        /// One-based 2D curve index.
        curve: usize,
        /// One-based surface index.
        surface: usize,
        /// Surface location index, or zero for identity.
        location: usize,
        /// Parameter-curve range.
        parameter_range: [FiniteReal; 2],
        /// Optional V2 cached UV endpoints.
        uv_endpoints: Option<[FinitePoint2; 2]>,
    },
    /// Kind 3: a pair of parameter-space curves on one surface.
    PcurvePair {
        /// One-based primary and secondary 2D curve indices.
        curves: [usize; 2],
        /// Continuity token joining the pair.
        continuity: String,
        /// One-based surface index.
        surface: usize,
        /// Surface location index, or zero for identity.
        location: usize,
        /// Parameter-curve range.
        parameter_range: [FiniteReal; 2],
        /// Optional V2 cached UV endpoints.
        uv_endpoints: Option<[FinitePoint2; 2]>,
    },
    /// Kind 4: regularity between two surfaces.
    Regularity {
        /// Continuity token.
        continuity: String,
        /// One-based first and second surface indices.
        surfaces: [usize; 2],
        /// First and second location indices.
        locations: [usize; 2],
    },
    /// Kind 5: standalone 3D polygon.
    Polygon3d {
        /// One-based 3D polygon index.
        polygon: usize,
        /// Location index, or zero for identity.
        location: usize,
    },
    /// Kind 6: polygon on one triangulation.
    PolygonOnTriangulation {
        /// One-based polygon-on-triangulation index.
        polygon: usize,
        /// One-based triangulation index.
        triangulation: usize,
        /// Location index, or zero for identity.
        location: usize,
    },
    /// Kind 7: a pair of polygons on one triangulation.
    PolygonPair {
        /// One-based primary and secondary polygon-on-triangulation indices.
        polygons: [usize; 2],
        /// One-based triangulation index.
        triangulation: usize,
        /// Location index, or zero for identity.
        location: usize,
    },
}

impl TextEdgeRepresentation {
    /// Representation code 1 through 7 retained on the CADIR wire.
    const fn kind(&self) -> u8 {
        match self {
            Self::Curve3d { .. } => 1,
            Self::Pcurve { .. } => 2,
            Self::PcurvePair { .. } => 3,
            Self::Regularity { .. } => 4,
            Self::Polygon3d { .. } => 5,
            Self::PolygonOnTriangulation { .. } => 6,
            Self::PolygonPair { .. } => 7,
        }
    }

    /// Primary location index, or zero for identity.
    pub(crate) const fn location(&self) -> usize {
        match *self {
            Self::Curve3d { location, .. }
            | Self::Pcurve { location, .. }
            | Self::PcurvePair { location, .. }
            | Self::Regularity {
                locations: [location, _],
                ..
            }
            | Self::Polygon3d { location, .. }
            | Self::PolygonOnTriangulation { location, .. }
            | Self::PolygonPair { location, .. } => location,
        }
    }

    /// Parameter range when the representation carries one.
    pub(crate) const fn parameter_range(&self) -> Option<[FiniteReal; 2]> {
        match *self {
            Self::Curve3d {
                parameter_range, ..
            }
            | Self::Pcurve {
                parameter_range, ..
            }
            | Self::PcurvePair {
                parameter_range, ..
            } => Some(parameter_range),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct TextEdgeRepresentationWire {
    kind: u8,
    primary: usize,
    secondary: Option<usize>,
    surface: Option<usize>,
    second_surface: Option<usize>,
    location: usize,
    second_location: Option<usize>,
    parameter_range: Option<[FiniteReal; 2]>,
    continuity: Option<String>,
    uv_endpoints: Option<[FinitePoint2; 2]>,
}

/// The retained wire shape, borrowed from the representation it states.
///
/// Every other member is a scalar; the continuity token is the one value the
/// record owns, and writing it needs no copy.
#[derive(Serialize)]
struct TextEdgeRepresentationOut<'a> {
    kind: u8,
    primary: usize,
    secondary: Option<usize>,
    surface: Option<usize>,
    second_surface: Option<usize>,
    location: usize,
    second_location: Option<usize>,
    parameter_range: Option<[f64; 2]>,
    continuity: Option<&'a str>,
    uv_endpoints: Option<[Point2; 2]>,
}

impl Serialize for TextEdgeRepresentation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TextEdgeRepresentationOut::new(self).serialize(serializer)
    }
}

impl<'a> TextEdgeRepresentationOut<'a> {
    fn new(value: &'a TextEdgeRepresentation) -> Self {
        let kind = value.kind();
        match *value {
            TextEdgeRepresentation::Curve3d {
                curve,
                location,
                parameter_range,
            } => Self {
                kind,
                primary: curve,
                secondary: None,
                surface: None,
                second_surface: None,
                location,
                second_location: None,
                parameter_range: Some(parameter_range.map(FiniteReal::get)),
                continuity: None,
                uv_endpoints: None,
            },
            TextEdgeRepresentation::Pcurve {
                curve,
                surface,
                location,
                parameter_range,
                uv_endpoints,
            } => Self {
                kind,
                primary: curve,
                secondary: None,
                surface: Some(surface),
                second_surface: None,
                location,
                second_location: None,
                parameter_range: Some(parameter_range.map(FiniteReal::get)),
                continuity: None,
                uv_endpoints: uv_endpoints.map(|points| points.map(FinitePoint2::get)),
            },
            TextEdgeRepresentation::PcurvePair {
                curves,
                ref continuity,
                surface,
                location,
                parameter_range,
                uv_endpoints,
            } => Self {
                kind,
                primary: curves[0],
                secondary: Some(curves[1]),
                surface: Some(surface),
                second_surface: None,
                location,
                second_location: None,
                parameter_range: Some(parameter_range.map(FiniteReal::get)),
                continuity: Some(continuity),
                uv_endpoints: uv_endpoints.map(|points| points.map(FinitePoint2::get)),
            },
            TextEdgeRepresentation::Regularity {
                ref continuity,
                surfaces,
                locations,
            } => Self {
                kind,
                primary: 0,
                secondary: None,
                surface: Some(surfaces[0]),
                second_surface: Some(surfaces[1]),
                location: locations[0],
                second_location: Some(locations[1]),
                parameter_range: None,
                continuity: Some(continuity),
                uv_endpoints: None,
            },
            TextEdgeRepresentation::Polygon3d { polygon, location } => Self {
                kind,
                primary: polygon,
                secondary: None,
                surface: None,
                second_surface: None,
                location,
                second_location: None,
                parameter_range: None,
                continuity: None,
                uv_endpoints: None,
            },
            TextEdgeRepresentation::PolygonOnTriangulation {
                polygon,
                triangulation,
                location,
            } => Self {
                kind,
                primary: polygon,
                secondary: None,
                surface: Some(triangulation),
                second_surface: None,
                location,
                second_location: None,
                parameter_range: None,
                continuity: None,
                uv_endpoints: None,
            },
            TextEdgeRepresentation::PolygonPair {
                polygons,
                triangulation,
                location,
            } => Self {
                kind,
                primary: polygons[0],
                secondary: Some(polygons[1]),
                surface: Some(triangulation),
                second_surface: None,
                location,
                second_location: None,
                parameter_range: None,
                continuity: None,
                uv_endpoints: None,
            },
        }
    }
}

impl TryFrom<TextEdgeRepresentationWire> for TextEdgeRepresentation {
    type Error = String;

    fn try_from(wire: TextEdgeRepresentationWire) -> Result<Self, Self::Error> {
        match (
            wire.kind,
            wire.primary,
            wire.secondary,
            wire.surface,
            wire.second_surface,
            wire.location,
            wire.second_location,
            wire.parameter_range,
            wire.continuity,
            wire.uv_endpoints,
        ) {
            (1, curve, None, None, None, location, None, Some(parameter_range), None, None) => {
                Ok(Self::Curve3d {
                    curve,
                    location,
                    parameter_range,
                })
            }
            (
                2,
                curve,
                None,
                Some(surface),
                None,
                location,
                None,
                Some(parameter_range),
                None,
                uv_endpoints,
            ) => Ok(Self::Pcurve {
                curve,
                surface,
                location,
                parameter_range,
                uv_endpoints,
            }),
            (
                3,
                primary,
                Some(secondary),
                Some(surface),
                None,
                location,
                None,
                Some(parameter_range),
                Some(continuity),
                uv_endpoints,
            ) => Ok(Self::PcurvePair {
                curves: [primary, secondary],
                continuity,
                surface,
                location,
                parameter_range,
                uv_endpoints,
            }),
            (
                4,
                0,
                None,
                Some(first_surface),
                Some(second_surface),
                location,
                Some(second_location),
                None,
                Some(continuity),
                None,
            ) => Ok(Self::Regularity {
                continuity,
                surfaces: [first_surface, second_surface],
                locations: [location, second_location],
            }),
            (5, polygon, None, None, None, location, None, None, None, None) => {
                Ok(Self::Polygon3d { polygon, location })
            }
            (6, polygon, None, Some(triangulation), None, location, None, None, None, None) => {
                Ok(Self::PolygonOnTriangulation {
                    polygon,
                    triangulation,
                    location,
                })
            }
            (
                7,
                primary,
                Some(secondary),
                Some(triangulation),
                None,
                location,
                None,
                None,
                None,
                None,
            ) => Ok(Self::PolygonPair {
                polygons: [primary, secondary],
                triangulation,
                location,
            }),
            _ => Err("edge representation kind disagrees with payload fields".to_owned()),
        }
    }
}

/// Geometry and flags specific to a topology record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextTShapeGeometry {
    Vertex {
        tolerance: FiniteReal,
        point: FinitePoint3,
        representations: Vec<TextPointRepresentation>,
    },
    Edge {
        tolerance: FiniteReal,
        same_parameter: bool,
        same_range: bool,
        degenerated: bool,
        representations: Vec<TextEdgeRepresentation>,
    },
    Face {
        natural_restriction: bool,
        tolerance: FiniteReal,
        surface: Option<TableRef<TextSurface>>,
        location: LocationRef,
        triangulation: Option<TableRef<TextTriangulation>>,
    },
    Wire,
    Shell,
    Solid,
    CompSolid,
    Compound,
}

impl TextTShapeGeometry {
    /// Shape family retained as `kind` on the CADIR wire.
    const fn kind(&self) -> TextShapeKind {
        match self {
            Self::Vertex { .. } => TextShapeKind::Vertex,
            Self::Edge { .. } => TextShapeKind::Edge,
            Self::Face { .. } => TextShapeKind::Face,
            Self::Wire => TextShapeKind::Wire,
            Self::Shell => TextShapeKind::Shell,
            Self::Solid => TextShapeKind::Solid,
            Self::CompSolid => TextShapeKind::CompSolid,
            Self::Compound => TextShapeKind::Compound,
        }
    }
}

/// An identity placement or a nonzero location-table reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "usize", into = "usize")]
pub(crate) enum LocationRef {
    /// The identity placement.
    Identity,
    /// A stored placement.
    Table(TableRef<TextLocation>),
}

impl From<usize> for LocationRef {
    fn from(index: usize) -> Self {
        TableRef::optional(index).map_or(Self::Identity, Self::Table)
    }
}

impl From<LocationRef> for usize {
    fn from(value: LocationRef) -> Self {
        value.index()
    }
}

impl LocationRef {
    /// Returns the location wire index.
    fn index(self) -> usize {
        match self {
            Self::Identity => 0,
            Self::Table(index) => index.index(),
        }
    }

    /// Resolves a placement against the location table.
    pub(crate) fn resolve(self, table: &[TextLocation]) -> Result<Transform, CodecError> {
        match self {
            Self::Identity => Ok(Transform::identity()),
            Self::Table(index) => Ok(index.resolve(table)?.transform),
        }
    }
}

/// A nonzero one-based reference to an owned table.
#[derive(Debug)]
pub(crate) struct TableRef<T> {
    index: std::num::NonZeroUsize,
    table: std::marker::PhantomData<fn() -> T>,
}

impl<T> PartialEq for TableRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T> Eq for TableRef<T> {}

impl<T> Copy for TableRef<T> {}

impl<T> Clone for TableRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> TableRef<T> {
    /// Admits a nonzero table index.
    fn new(index: usize) -> Result<Self, String> {
        Self::optional(index).ok_or_else(|| "table reference must be nonzero".to_owned())
    }

    /// Admits an optional one-based table index.
    fn optional(index: usize) -> Option<Self> {
        std::num::NonZeroUsize::new(index).map(|index| Self {
            index,
            table: std::marker::PhantomData,
        })
    }

    /// Returns the one-based wire index.
    pub(crate) fn index(self) -> usize {
        self.index.get()
    }

    /// Resolves the reference against its table.
    pub(crate) fn resolve(self, table: &[T]) -> Result<&T, CodecError> {
        table.get(self.index.get() - 1).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "table reference {} is out of range",
                self.index
            ))
        })
    }
}

/// One subshape-first topology record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextTShape {
    /// Family-specific geometry, including geometry-less families.
    pub(crate) geometry: TextTShapeGeometry,
    /// Free, modified, checked, orientable, closed, infinite, convex flags.
    pub(crate) flags: [bool; 7],
    /// Ordered child uses.
    pub(crate) children: Vec<TextShapeUse>,
}

impl TextTShape {
    /// Shape family retained as `kind` on the CADIR wire.
    pub(crate) const fn kind(&self) -> TextShapeKind {
        self.geometry.kind()
    }
}

#[derive(Deserialize)]
struct TextTShapeWire {
    index: usize,
    kind: TextShapeKind,
    geometry: TextTShapeGeometryWire,
    flags: [bool; 7],
    children: Vec<TextShapeUse>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TextTShapeGeometryWire {
    Vertex {
        tolerance: FiniteReal,
        point: FinitePoint3,
        representations: Vec<TextPointRepresentation>,
    },
    Edge {
        tolerance: FiniteReal,
        same_parameter: bool,
        same_range: bool,
        degenerated: bool,
        representations: Vec<TextEdgeRepresentation>,
    },
    Face {
        natural_restriction: bool,
        tolerance: FiniteReal,
        surface: usize,
        location: usize,
        triangulation: Option<usize>,
    },
    Empty,
}

/// The retained wire shape of one topology record, borrowed from the record.
///
/// Reading owns the tables it builds; writing states the one-based position and
/// the family, which are derived, and borrows everything else.
#[derive(Serialize)]
struct TextTShapeOut<'a> {
    index: usize,
    kind: TextShapeKind,
    geometry: TextTShapeGeometryOut<'a>,
    flags: [bool; 7],
    children: &'a [TextShapeUse],
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TextTShapeGeometryOut<'a> {
    Vertex {
        tolerance: f64,
        point: FinitePoint3,
        representations: &'a [TextPointRepresentation],
    },
    Edge {
        tolerance: f64,
        same_parameter: bool,
        same_range: bool,
        degenerated: bool,
        representations: &'a [TextEdgeRepresentation],
    },
    Face {
        natural_restriction: bool,
        tolerance: f64,
        surface: usize,
        location: usize,
        triangulation: Option<usize>,
    },
    Empty,
}

impl<'a> TextTShapeOut<'a> {
    fn new(position: usize, value: &'a TextTShape) -> Self {
        let geometry = match &value.geometry {
            TextTShapeGeometry::Vertex {
                tolerance,
                point,
                representations,
            } => TextTShapeGeometryOut::Vertex {
                tolerance: tolerance.get(),
                point: *point,
                representations,
            },
            TextTShapeGeometry::Edge {
                tolerance,
                same_parameter,
                same_range,
                degenerated,
                representations,
            } => TextTShapeGeometryOut::Edge {
                tolerance: tolerance.get(),
                same_parameter: *same_parameter,
                same_range: *same_range,
                degenerated: *degenerated,
                representations,
            },
            TextTShapeGeometry::Face {
                natural_restriction,
                tolerance,
                surface,
                location,
                triangulation,
            } => TextTShapeGeometryOut::Face {
                natural_restriction: *natural_restriction,
                tolerance: tolerance.get(),
                surface: surface.map_or(0, TableRef::index),
                location: location.index(),
                triangulation: triangulation.map(TableRef::index),
            },
            TextTShapeGeometry::Wire
            | TextTShapeGeometry::Shell
            | TextTShapeGeometry::Solid
            | TextTShapeGeometry::CompSolid
            | TextTShapeGeometry::Compound => TextTShapeGeometryOut::Empty,
        };
        Self {
            index: position + 1,
            kind: value.kind(),
            geometry,
            flags: value.flags,
            children: &value.children,
        }
    }
}

/// Topology records whose one-based identity is their collection position.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(try_from = "Vec<TextTShapeWire>")]
pub(crate) struct TextTShapes(Vec<TextTShape>);

impl Serialize for TextTShapes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .iter()
                .enumerate()
                .map(|(position, value)| TextTShapeOut::new(position, value)),
        )
    }
}

impl TextTShapes {
    /// Resolve a one-based topology identity without unchecked subtraction.
    pub(crate) fn resolve(&self, index: usize) -> Result<&TextTShape, CodecError> {
        index
            .checked_sub(1)
            .and_then(|position| self.0.get(position))
            .ok_or_else(|| CodecError::malformed(format_args!("missing TShape {index}")))
    }
}

impl std::ops::Deref for TextTShapes {
    type Target = [TextTShape];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Vec<TextTShape>> for TextTShapes {
    fn from(shapes: Vec<TextTShape>) -> Self {
        // Native records have no independent index; position defines identity.
        Self(shapes)
    }
}

impl TryFrom<Vec<TextTShapeWire>> for TextTShapes {
    type Error = String;

    fn try_from(shapes: Vec<TextTShapeWire>) -> Result<Self, Self::Error> {
        shapes
            .into_iter()
            .enumerate()
            .map(|(position, wire)| {
                if wire.index != position + 1 {
                    return Err(format!(
                        "tshapes[{position}].index must equal {}",
                        position + 1
                    ));
                }
                wire.try_into()
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
}

impl TryFrom<TextTShapeWire> for TextTShape {
    type Error = String;

    fn try_from(wire: TextTShapeWire) -> Result<Self, Self::Error> {
        let geometry = match (wire.kind, wire.geometry) {
            (
                TextShapeKind::Vertex,
                TextTShapeGeometryWire::Vertex {
                    tolerance,
                    point,
                    representations,
                },
            ) => TextTShapeGeometry::Vertex {
                tolerance,
                point,
                representations,
            },
            (
                TextShapeKind::Edge,
                TextTShapeGeometryWire::Edge {
                    tolerance,
                    same_parameter,
                    same_range,
                    degenerated,
                    representations,
                },
            ) => TextTShapeGeometry::Edge {
                tolerance,
                same_parameter,
                same_range,
                degenerated,
                representations,
            },
            (
                TextShapeKind::Face,
                TextTShapeGeometryWire::Face {
                    natural_restriction,
                    tolerance,
                    surface,
                    location,
                    triangulation,
                },
            ) => TextTShapeGeometry::Face {
                natural_restriction,
                tolerance,
                surface: TableRef::optional(surface),
                location: location.into(),
                triangulation: triangulation
                    .map(TableRef::new)
                    .transpose()
                    .map_err(|error| format!("triangulation: {error}"))?,
            },
            (TextShapeKind::Wire, TextTShapeGeometryWire::Empty) => TextTShapeGeometry::Wire,
            (TextShapeKind::Shell, TextTShapeGeometryWire::Empty) => TextTShapeGeometry::Shell,
            (TextShapeKind::Solid, TextTShapeGeometryWire::Empty) => TextTShapeGeometry::Solid,
            (TextShapeKind::CompSolid, TextTShapeGeometryWire::Empty) => {
                TextTShapeGeometry::CompSolid
            }
            (TextShapeKind::Compound, TextTShapeGeometryWire::Empty) => {
                TextTShapeGeometry::Compound
            }
            _ => return Err("TShape kind disagrees with geometry".to_owned()),
        };
        Ok(Self {
            geometry,
            flags: wire.flags,
            children: wire.children,
        })
    }
}

/// One standalone 3D polygon carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TextPolygon3d {
    /// Chordal deflection.
    pub(crate) deflection: NonNegativeReal,
    /// Ordered model-space nodes.
    pub(crate) nodes: Vec<FinitePoint3>,
    /// Optional per-node curve parameters.
    pub(crate) parameters: Option<Vec<FiniteReal>>,
}

/// One polygon whose indices address a triangulation node table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TextPolygonOnTriangulation {
    /// One-based source node indices.
    pub(crate) nodes: Vec<u32>,
    /// Chordal deflection.
    pub(crate) deflection: NonNegativeReal,
    /// Optional per-node curve parameters.
    pub(crate) parameters: Option<Vec<FiniteReal>>,
}

fn admit_polygon_deflection(value: FiniteReal) -> Result<NonNegativeReal, CodecError> {
    NonNegativeReal::from_finite(value).ok_or_else(|| {
        CodecError::Malformed("polygon deflection must be finite and non-negative".into())
    })
}

/// A rational or non-rational 2D B-spline curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct NurbsCurve2d {
    /// Curve degree.
    pub(crate) degree: u32,
    /// Full knot vector.
    pub(crate) knots: Vec<FiniteReal>,
    /// Ordered parameter-space poles.
    pub(crate) control_points: Vec<FinitePoint2>,
    /// Optional rational weights.
    pub(crate) weights: Option<Vec<FiniteReal>>,
    /// Periodicity flag.
    pub(crate) periodic: bool,
}

/// Admitted count of inline geometry records over one leaf record.
///
/// A geometry record states its basis or directrix inline, so one input token
/// or byte adds a record and an input of ordinary size can nest deeper than the
/// stack holds. `NestedCurve2d`, `NestedCurve` and `NestedSurface` own this
/// count: every recursive field of `TextCurve2d`, `TextCurve` and `TextSurface`
/// is one of them, and each refuses a record whose own count is already the
/// bound. No deeper tree exists to parse, deserialize or walk, so
/// `census_surface`, `append_text_surface`, `append_text_curve`,
/// `surface_parameter_affine` and `pcurve_geometry` hold no budget of their
/// own. An extrusion or revolution directrix spends the same count as its
/// surface, so the count is the records in one table entry, not per type.
///
/// The six parsers restate the number because their recursion descends before
/// any record is constructed: the parser guard bounds the parse stack, these
/// carriers bound the value. One table entry therefore costs at most
/// `MAX_GEOMETRY_NESTING_DEPTH + 1` parse frames on either route.
///
/// The native trees are not neutral carriers. `MAX_GEOMETRY_NESTING` bounds the
/// neutral chain separately, in the carrier constructors `topology_transfer`
/// calls.
const MAX_GEOMETRY_NESTING_DEPTH: usize = 64;

/// One exact parameter-space curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TextCurve2d {
    /// Infinite line.
    Line {
        origin: FinitePoint2,
        direction: FinitePoint2,
    },
    /// Full circle with its oriented parameter frame.
    Circle {
        center: FinitePoint2,
        x_axis: FinitePoint2,
        y_axis: FinitePoint2,
        radius: FiniteReal,
    },
    /// Full ellipse.
    Ellipse {
        center: FinitePoint2,
        x_axis: FinitePoint2,
        y_axis: FinitePoint2,
        major_radius: FiniteReal,
        minor_radius: FiniteReal,
    },
    /// Parabola.
    Parabola {
        vertex: FinitePoint2,
        x_axis: FinitePoint2,
        y_axis: FinitePoint2,
        focal_distance: FiniteReal,
    },
    /// Hyperbola.
    Hyperbola {
        center: FinitePoint2,
        x_axis: FinitePoint2,
        y_axis: FinitePoint2,
        major_radius: FiniteReal,
        minor_radius: FiniteReal,
    },
    /// Rational or non-rational B-spline.
    Nurbs(NurbsCurve2d),
    /// Parameter restriction of an inline basis curve.
    Trimmed {
        parameter_range: [FiniteReal; 2],
        basis: NestedCurve2d,
    },
    /// Signed planar offset of an inline basis curve.
    Offset {
        distance: FiniteReal,
        basis: NestedCurve2d,
    },
}

impl TextCurve2d {
    /// Inline records this curve carries over its leaf record.
    const fn nesting_depth(&self) -> usize {
        match self {
            Self::Line { .. }
            | Self::Circle { .. }
            | Self::Ellipse { .. }
            | Self::Parabola { .. }
            | Self::Hyperbola { .. }
            | Self::Nurbs(_) => 0,
            Self::Trimmed { basis, .. } | Self::Offset { basis, .. } => basis.depth,
        }
    }
}

/// One inline parameter-space basis curve, with the records it carries.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Box<TextCurve2d>")]
pub(crate) struct NestedCurve2d {
    curve: Box<TextCurve2d>,
    depth: usize,
}

impl NestedCurve2d {
    /// Admits `curve` as the basis of one more inline record.
    pub(crate) fn try_new(curve: TextCurve2d) -> Result<Self, String> {
        let depth = curve.nesting_depth() + 1;
        if depth > MAX_GEOMETRY_NESTING_DEPTH {
            return Err(format!(
                "parameter-curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
            ));
        }
        Ok(Self {
            curve: Box::new(curve),
            depth,
        })
    }

    /// The carried record.
    pub(crate) fn curve(&self) -> &TextCurve2d {
        &self.curve
    }
}

impl TryFrom<Box<TextCurve2d>> for NestedCurve2d {
    type Error = String;

    fn try_from(curve: Box<TextCurve2d>) -> Result<Self, Self::Error> {
        Self::try_new(*curve)
    }
}

impl Serialize for NestedCurve2d {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.curve.serialize(serializer)
    }
}

/// One factor in a compound location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LocationFactor {
    /// One-based index of an earlier location.
    location: usize,
    /// Signed composition power.
    power: i64,
}

/// One text B-rep location record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TextLocation {
    /// Ordered source factors; empty for an elementary transform.
    pub(crate) factors: Vec<LocationFactor>,
    /// Fully composed affine transform.
    pub(crate) transform: Transform,
}

/// Supported byte-exact 3D curve records from the text carrier table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TextCurve {
    /// Infinite line.
    Line {
        origin: FinitePoint3,
        direction: FiniteVector3,
    },
    /// Full circle.
    Circle {
        center: FinitePoint3,
        axis: FiniteVector3,
        ref_direction: FiniteVector3,
        radius: FiniteReal,
    },
    /// Full ellipse.
    Ellipse {
        center: FinitePoint3,
        axis: FiniteVector3,
        major_direction: FiniteVector3,
        major_radius: FiniteReal,
        minor_radius: FiniteReal,
    },
    /// Parabola.
    Parabola {
        vertex: FinitePoint3,
        axis: FiniteVector3,
        major_direction: FiniteVector3,
        focal_distance: FiniteReal,
    },
    /// Hyperbola.
    Hyperbola {
        center: FinitePoint3,
        axis: FiniteVector3,
        major_direction: FiniteVector3,
        major_radius: FiniteReal,
        minor_radius: FiniteReal,
    },
    /// Rational or non-rational B-spline curve.
    Nurbs(NurbsCurve),
    /// A parameter sub-range of an inline basis curve.
    Trimmed {
        parameter_range: [FiniteReal; 2],
        basis: NestedCurve,
    },
    /// A signed offset from an inline basis curve in a fixed direction.
    Offset {
        distance: FiniteReal,
        direction: FiniteVector3,
        basis: NestedCurve,
    },
}

impl TextCurve {
    /// Inline records this curve carries over its leaf record.
    const fn nesting_depth(&self) -> usize {
        match self {
            Self::Line { .. }
            | Self::Circle { .. }
            | Self::Ellipse { .. }
            | Self::Parabola { .. }
            | Self::Hyperbola { .. }
            | Self::Nurbs(_) => 0,
            Self::Trimmed { basis, .. } | Self::Offset { basis, .. } => basis.depth,
        }
    }
}

/// One inline 3D basis or directrix curve, with the records it carries.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Box<TextCurve>")]
pub(crate) struct NestedCurve {
    curve: Box<TextCurve>,
    depth: usize,
}

impl NestedCurve {
    /// Admits `curve` as the basis or directrix of one more inline record.
    pub(crate) fn try_new(curve: TextCurve) -> Result<Self, String> {
        let depth = curve.nesting_depth() + 1;
        if depth > MAX_GEOMETRY_NESTING_DEPTH {
            return Err(format!(
                "3D curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
            ));
        }
        Ok(Self {
            curve: Box::new(curve),
            depth,
        })
    }

    /// The carried record.
    pub(crate) fn curve(&self) -> &TextCurve {
        &self.curve
    }
}

impl TryFrom<Box<TextCurve>> for NestedCurve {
    type Error = String;

    fn try_from(curve: Box<TextCurve>) -> Result<Self, Self::Error> {
        Self::try_new(*curve)
    }
}

impl Serialize for NestedCurve {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.curve.serialize(serializer)
    }
}

/// Supported byte-exact surface records from the text carrier table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TextSurface {
    /// Infinite plane.
    Plane {
        origin: FinitePoint3,
        axis: FiniteVector3,
        u_axis: FiniteVector3,
        v_reversed: bool,
    },
    /// Circular cylinder.
    Cylinder {
        origin: FinitePoint3,
        axis: FiniteVector3,
        ref_direction: FiniteVector3,
        radius: FiniteReal,
        u_reversed: bool,
    },
    /// Circular cone.
    Cone {
        origin: FinitePoint3,
        axis: FiniteVector3,
        ref_direction: FiniteVector3,
        radius: FiniteReal,
        half_angle: FiniteReal,
        u_reversed: bool,
    },
    /// Sphere.
    Sphere {
        center: FinitePoint3,
        axis: FiniteVector3,
        ref_direction: FiniteVector3,
        radius: FiniteReal,
        u_reversed: bool,
    },
    /// Torus.
    Torus {
        center: FinitePoint3,
        axis: FiniteVector3,
        ref_direction: FiniteVector3,
        major_radius: FiniteReal,
        minor_radius: FiniteReal,
        u_reversed: bool,
    },
    /// Rational or non-rational tensor-product B-spline surface.
    Nurbs(NurbsSurface),
    /// Translation of an inline directrix curve.
    Extrusion {
        direction: FiniteVector3,
        directrix: NestedCurve,
    },
    /// Revolution of an inline directrix around an axis.
    Revolution {
        axis_origin: FinitePoint3,
        axis_direction: FiniteVector3,
        directrix: NestedCurve,
    },
    /// Rectangular parameter sub-range of an inline basis surface.
    Trimmed {
        parameter_ranges: [[FiniteReal; 2]; 2],
        basis: NestedSurface,
    },
    /// Signed normal offset from an inline basis surface.
    Offset {
        distance: FiniteReal,
        basis: NestedSurface,
    },
}

impl TextSurface {
    /// Inline records this surface carries over its leaf record.
    const fn nesting_depth(&self) -> usize {
        match self {
            Self::Plane { .. }
            | Self::Cylinder { .. }
            | Self::Cone { .. }
            | Self::Sphere { .. }
            | Self::Torus { .. }
            | Self::Nurbs(_) => 0,
            Self::Extrusion { directrix, .. } | Self::Revolution { directrix, .. } => {
                directrix.depth
            }
            Self::Trimmed { basis, .. } | Self::Offset { basis, .. } => basis.depth,
        }
    }
}

/// One inline basis surface, with the records it carries.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Box<TextSurface>")]
pub(crate) struct NestedSurface {
    surface: Box<TextSurface>,
    depth: usize,
}

impl NestedSurface {
    /// Admits `surface` as the basis of one more inline record.
    pub(crate) fn try_new(surface: TextSurface) -> Result<Self, String> {
        let depth = surface.nesting_depth() + 1;
        if depth > MAX_GEOMETRY_NESTING_DEPTH {
            return Err(format!(
                "surface nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
            ));
        }
        Ok(Self {
            surface: Box::new(surface),
            depth,
        })
    }

    /// The carried record.
    pub(crate) fn surface(&self) -> &TextSurface {
        &self.surface
    }
}

impl TryFrom<Box<TextSurface>> for NestedSurface {
    type Error = String;

    fn try_from(surface: Box<TextSurface>) -> Result<Self, Self::Error> {
        Self::try_new(*surface)
    }
}

impl Serialize for NestedSurface {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.surface.serialize(serializer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceParameterAffine {
    pub(crate) u_scale: f64,
    pub(crate) u_offset: f64,
    pub(crate) v_scale: f64,
    pub(crate) v_offset: f64,
}

pub(crate) fn surface_parameter_affine(surface: &TextSurface) -> SurfaceParameterAffine {
    let identity = SurfaceParameterAffine {
        u_scale: 1.0,
        u_offset: 0.0,
        v_scale: 1.0,
        v_offset: 0.0,
    };
    match surface {
        TextSurface::Plane {
            v_reversed: true, ..
        } => SurfaceParameterAffine {
            v_scale: -1.0,
            ..identity
        },
        TextSurface::Cylinder {
            u_reversed: true, ..
        }
        | TextSurface::Sphere {
            u_reversed: true, ..
        }
        | TextSurface::Torus {
            u_reversed: true, ..
        } => SurfaceParameterAffine {
            u_scale: -1.0,
            ..identity
        },
        TextSurface::Cone {
            half_angle,
            u_reversed,
            ..
        } => SurfaceParameterAffine {
            u_scale: if *u_reversed { -1.0 } else { 1.0 },
            v_scale: half_angle.get().cos(),
            ..identity
        },
        TextSurface::Trimmed {
            parameter_ranges,
            basis,
        } => {
            let basis = surface_parameter_affine(basis.surface());
            let u_scale = basis.u_scale.abs();
            let v_scale = basis.v_scale.abs();
            SurfaceParameterAffine {
                u_scale,
                u_offset: -parameter_ranges[0][0].get() * u_scale,
                v_scale,
                v_offset: -parameter_ranges[1][0].get() * v_scale,
            }
        }
        TextSurface::Offset { basis, .. } => surface_parameter_affine(basis.surface()),
        _ => identity,
    }
}

/// Bind every exact-shape property to and frame its payload.
pub(crate) fn parse_payloads(
    ctx: &DecodeContext<'_>,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<Vec<ShapePayloadRecord>, CodecError> {
    let mut entries_by_name = BTreeMap::new();
    for entry in entries {
        if !entries_by_name.contains_key(entry.name.as_str()) {
            ctx.charge_collection_items(1, "FreeCAD shape entry index")?;
        }
        entries_by_name.insert(entry.name.as_str(), entry);
    }
    let mut payloads = Vec::new();
    for property in properties
        .iter()
        .filter(|property| property.type_name == "Part::PropertyPartShape")
    {
        let Some(name) = direct_shape_entry(ctx, property)? else {
            continue;
        };
        let Some(entry) = entries_by_name.get(name.as_str()) else {
            return Err(CodecError::Malformed(retained_format(
                ctx,
                format_args!("missing exact-shape entry {name}"),
                "FreeCAD missing shape entry",
            )?));
        };
        let payload = if entry.data.is_empty() {
            ShapePayload::Empty
        } else if name
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("bin"))
        {
            let (facts, version) = parse_binary_prefix(ctx, &entry.data)?;
            ShapePayload::Binary { facts, version }
        } else {
            let (facts, version) = parse_text(ctx, &entry.data)?;
            ShapePayload::Text { facts, version }
        };
        reserve_vec_items(ctx, &mut payloads, 1, "FreeCAD shape payload records")?;
        payloads.push(ShapePayloadRecord {
            id: crate::native::native_child_id_charged(ctx, "shape-payload", &property.id, &name)?,
            property: retained_string(ctx, &property.id, "FreeCAD shape payload property")?,
            entry: retained_string(ctx, &entry.id, "FreeCAD shape payload entry")?,
            payload,
        });
    }
    Ok(payloads)
}

fn direct_shape_entry(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Option<String>, CodecError> {
    let document = roxmltree::Document::parse(property.xml.text()).or_else(|error| {
        Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("invalid exact-shape property XML {}: {error}", property.id),
            "FreeCAD shape property XML diagnostic",
        )?))
    })?;
    let root = document.root_element();
    if !matches!(root.tag_name().name(), "Property" | "_Property") {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "exact-shape property {} has no property record root",
                property.id
            ),
            "FreeCAD shape property root diagnostic",
        )?));
    }
    let mut parts = root.children().filter(|node| node.has_tag_name("Part"));
    let Some(part) = parts.next() else {
        return Ok(None);
    };
    if parts.next().is_some() {
        return Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!(
                "exact-shape property {} has multiple direct Part carriers",
                property.id
            ),
            "FreeCAD shape property carrier diagnostic",
        )?));
    }
    part.attribute("file")
        .filter(|file| !file.is_empty())
        .map(|file| retained_string(ctx, file, "FreeCAD shape entry name"))
        .transpose()
}

/// Derive an exhaustive family census from successfully parsed exact-shape payloads.
pub(crate) fn carrier_census(
    ctx: &DecodeContext<'_>,
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<crate::native::CarrierCensusRecord>, CodecError> {
    let count = payloads
        .iter()
        .filter(|payload| payload.payload.shape_set().is_some())
        .count();
    let mut census = collection_vec(ctx, count, "FreeCAD carrier census records")?;
    for payload in payloads {
        let Some(facts) = payload.payload.shape_set() else {
            continue;
        };
        let Some(version) = payload.payload.topology_version() else {
            continue;
        };
        let curve2ds = &facts.curve2ds;
        let curves = &facts.curves;
        let surfaces = &facts.surfaces;
        let polygons3d = facts.polygons3d.len();
        let indexed = facts.polygons_on_triangulations.len();
        let triangulations = facts.triangulations.len();
        let tshapes = &facts.tshapes;
        let mut record = crate::native::CarrierCensusRecord {
            id: crate::native::native_child_id_charged(
                ctx,
                "carrier-census",
                &payload.id,
                "families",
            )?,
            payload: retained_string(ctx, &payload.id, "FreeCAD carrier census payload")?,
            form: match payload.payload.form() {
                ShapePayloadForm::Text => crate::native::CarrierCensusForm::Text,
                ShapePayloadForm::Binary => crate::native::CarrierCensusForm::Binary,
                ShapePayloadForm::Empty => continue,
            },
            topology_version: version,
            curves_2d: BTreeMap::new(),
            curves_3d: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            topology: BTreeMap::new(),
            polygons_3d: polygons3d as u64,
            polygons_on_triangulations: indexed as u64,
            triangulations: triangulations as u64,
        };
        for curve in curve2ds {
            census_curve(ctx, CensusCurve::Parameter(curve), &mut record.curves_2d)?;
        }
        for curve in curves {
            census_curve(ctx, CensusCurve::Model(curve), &mut record.curves_3d)?;
        }
        for surface in surfaces {
            census_surface(ctx, surface, &mut record.surfaces, &mut record.curves_3d)?;
        }
        for shape in tshapes.iter() {
            increment(
                ctx,
                &mut record.topology,
                match shape.kind() {
                    TextShapeKind::Vertex => "vertex",
                    TextShapeKind::Edge => "edge",
                    TextShapeKind::Wire => "wire",
                    TextShapeKind::Face => "face",
                    TextShapeKind::Shell => "shell",
                    TextShapeKind::Solid => "solid",
                    TextShapeKind::CompSolid => "compsolid",
                    TextShapeKind::Compound => "compound",
                },
            )?;
        }
        census.push(record);
    }
    census.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(census)
}

fn increment(
    ctx: &DecodeContext<'_>,
    counts: &mut BTreeMap<String, u64>,
    family: &str,
) -> Result<(), CodecError> {
    if let Some(count) = counts.get_mut(family) {
        *count += 1;
    } else {
        ctx.charge_collection_items(1, "FreeCAD carrier census families")?;
        counts.insert(family.to_owned(), 1);
    }
    Ok(())
}

enum CensusCurve<'a> {
    Model(&'a TextCurve),
    Parameter(&'a TextCurve2d),
}

fn census_curve(
    ctx: &DecodeContext<'_>,
    mut curve: CensusCurve<'_>,
    counts: &mut BTreeMap<String, u64>,
) -> Result<(), CodecError> {
    use CensusCurve::{Model, Parameter};
    loop {
        let (family, basis) = match curve {
            Model(TextCurve::Line { .. }) | Parameter(TextCurve2d::Line { .. }) => ("line", None),
            Model(TextCurve::Circle { .. }) | Parameter(TextCurve2d::Circle { .. }) => {
                ("circle", None)
            }
            Model(TextCurve::Ellipse { .. }) | Parameter(TextCurve2d::Ellipse { .. }) => {
                ("ellipse", None)
            }
            Model(TextCurve::Parabola { .. }) | Parameter(TextCurve2d::Parabola { .. }) => {
                ("parabola", None)
            }
            Model(TextCurve::Hyperbola { .. }) | Parameter(TextCurve2d::Hyperbola { .. }) => {
                ("hyperbola", None)
            }
            Model(TextCurve::Nurbs(_)) | Parameter(TextCurve2d::Nurbs(_)) => ("nurbs", None),
            Model(TextCurve::Trimmed { basis, .. }) => ("trimmed", Some(Model(basis.curve()))),
            Parameter(TextCurve2d::Trimmed { basis, .. }) => {
                ("trimmed", Some(Parameter(basis.curve())))
            }
            Model(TextCurve::Offset { basis, .. }) => ("offset", Some(Model(basis.curve()))),
            Parameter(TextCurve2d::Offset { basis, .. }) => {
                ("offset", Some(Parameter(basis.curve())))
            }
        };
        ctx.charge_work(1, "FreeCAD carrier curve census")?;
        increment(ctx, counts, family)?;
        let Some(basis) = basis else { return Ok(()) };
        curve = basis;
    }
}

fn census_surface(
    ctx: &DecodeContext<'_>,
    surface: &TextSurface,
    counts: &mut BTreeMap<String, u64>,
    curves: &mut BTreeMap<String, u64>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("FreeCAD carrier surface census")?;
    let family = match surface {
        TextSurface::Plane { .. } => "plane",
        TextSurface::Cylinder { .. } => "cylinder",
        TextSurface::Cone { .. } => "cone",
        TextSurface::Sphere { .. } => "sphere",
        TextSurface::Torus { .. } => "torus",
        TextSurface::Nurbs(_) => "nurbs",
        TextSurface::Extrusion { directrix, .. } => {
            increment(ctx, counts, "extrusion")?;
            census_curve(ctx, CensusCurve::Model(directrix.curve()), curves)?;
            return Ok(());
        }
        TextSurface::Revolution { directrix, .. } => {
            increment(ctx, counts, "revolution")?;
            census_curve(ctx, CensusCurve::Model(directrix.curve()), curves)?;
            return Ok(());
        }
        TextSurface::Trimmed { basis, .. } => {
            increment(ctx, counts, "trimmed")?;
            return census_surface(ctx, basis.surface(), counts, curves);
        }
        TextSurface::Offset { basis, .. } => {
            increment(ctx, counts, "offset")?;
            return census_surface(ctx, basis.surface(), counts, curves);
        }
    };
    increment(ctx, counts, family)
}

fn parse_text(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(ShapeSet, TextTopologyVersion), CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("text B-rep is not UTF-8".into()))?;
    let headers = [
        ("CASCADE Topology V1, (c) Matra-Datavision", 1),
        ("CASCADE Topology V2, (c) Matra-Datavision", 2),
        ("CASCADE Topology V3, (c) Open Cascade", 3),
    ];
    let mut topology_version = None;
    for (header, version) in headers {
        let count = text.matches(header).count();
        if count > 1 {
            return Err(CodecError::Malformed(
                "text B-rep has duplicate topology headers".into(),
            ));
        }
        if count == 1 && topology_version.replace(version).is_some() {
            return Err(CodecError::Malformed(
                "text B-rep has multiple topology headers".into(),
            ));
        }
    }
    let topology_version = topology_version.ok_or_else(|| {
        CodecError::Malformed("text B-rep has no supported topology header".into())
    })?;
    let token_count = text.split_ascii_whitespace().count();
    let (mut tokens, _token_reservation) =
        crate::resource::materialized_vec::<&str>(ctx, token_count, "FreeCAD text B-rep tokens")?;
    tokens.extend(text.split_ascii_whitespace());
    let mut section_counts = BTreeMap::new();
    let mut previous_section = None;
    for section in [
        "Locations",
        "Curve2ds",
        "Curves",
        "Polygon3D",
        "PolygonOnTriangulations",
        "Surfaces",
        "Triangulations",
    ] {
        let (index, count) = text_brep_section(&tokens, section, previous_section)?;
        previous_section = Some(index);
        section_counts.insert(section.to_owned(), count);
    }
    let (tshapes, declared_shapes) = text_brep_section(&tokens, "TShapes", previous_section)?;
    section_counts.insert("TShapes".to_owned(), declared_shapes);
    let mut shape_types = BTreeMap::new();
    for token in &tokens[tshapes + 2..] {
        let name = match *token {
            "Ve" => "vertex",
            "Ed" => "edge",
            "Wi" => "wire",
            "Fa" => "face",
            "Sh" => "shell",
            "So" => "solid",
            "CS" => "compsolid",
            "Co" => "compound",
            _ => continue,
        };
        if let Some(count) = shape_types.get_mut(name) {
            *count += 1;
        } else {
            shape_types.insert(name.to_owned(), 1);
        }
    }
    if shape_types.values().sum::<usize>() != declared_shapes {
        return Err(CodecError::malformed(format_args!(
            "TShapes declares {declared_shapes} records but the shape-type census found {}",
            shape_types.values().sum::<usize>()
        )));
    }
    let locations = parse_locations(ctx, &tokens, &section_counts)?;
    let curve2ds = parse_geometry_table(
        ctx,
        &tokens,
        &section_counts,
        "Curve2ds",
        "Curves",
        parse_curve2d,
    )?;
    let curves = parse_geometry_table(
        ctx,
        &tokens,
        &section_counts,
        "Curves",
        "Polygon3D",
        parse_curve,
    )?;
    let surfaces = parse_geometry_table(
        ctx,
        &tokens,
        &section_counts,
        "Surfaces",
        "Triangulations",
        parse_surface,
    )?;
    let polygons3d = parse_polygons3d(ctx, &tokens, &section_counts)?;
    let polygons_on_triangulations =
        parse_polygons_on_triangulations(ctx, &tokens, &section_counts)?;
    let triangulations = parse_triangulations(ctx, &tokens, &section_counts, topology_version)?;
    let (tshapes, roots) = parse_tshapes(ctx, &tokens, &section_counts, topology_version)?;
    let facts = ShapeSet {
        locations,
        curve2ds,
        curves,
        polygons3d,
        polygons_on_triangulations,
        surfaces,
        triangulations,
        tshapes: tshapes.into(),
        roots,
    };
    if shape_types != facts.shape_type_counts() {
        return Err(CodecError::Malformed(
            "TShapes shape-type census disagrees with parsed records".into(),
        ));
    }
    facts.validate().map_err(CodecError::Malformed)?;
    Ok((
        facts,
        TextTopologyVersion::try_from(topology_version).map_err(CodecError::Malformed)?,
    ))
}

/// The token position of one text B-rep section marker and its declared count.
fn text_brep_section(
    tokens: &[&str],
    section: &str,
    previous_section: Option<usize>,
) -> Result<(usize, usize), CodecError> {
    let mut section_tokens = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| **token == section);
    let Some((index, _)) = section_tokens.next() else {
        return Err(CodecError::malformed(format_args!(
            "text B-rep has no {section} table"
        )));
    };
    if section_tokens.next().is_some() {
        return Err(CodecError::Malformed(
            "text B-rep has duplicate section markers".into(),
        ));
    }
    let count = tokens
        .get(index + 1)
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::malformed(format_args!("invalid {section} count")))?;
    if count > 1_000_000 {
        return Err(CodecError::malformed(format_args!(
            "{section} count limit exceeded"
        )));
    }
    if previous_section.is_some_and(|previous| index <= previous) {
        return Err(CodecError::malformed(format_args!(
            "text B-rep {section} table is out of order"
        )));
    }
    Ok((index, count))
}

fn parse_binary_prefix(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(ShapeSet, BinaryTopologyVersion), CodecError> {
    let mut cursor = BinaryCursor::new(ctx, bytes);
    let version = loop {
        let line = cursor.line("binary B-rep version")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let version = line
            .strip_prefix("Open CASCADE Topology V")
            .and_then(|tail| tail.as_bytes().first())
            .and_then(|byte| byte.checked_sub(b'0'))
            .filter(|version| (1..=4).contains(version))
            .ok_or_else(|| CodecError::Malformed("unsupported binary B-rep header".into()))?;
        break version;
    };
    let location_count = cursor.section_count("Locations")?;
    // Each location consumes at least its 1-byte kind discriminant.
    let mut locations: Vec<TextLocation> = collection_vec(
        cursor.ctx,
        cursor.bounded(location_count, 1, "binary Locations")?,
        "FreeCAD B-rep parse_binary_prefix",
    )?;
    for index in 0..location_count {
        let kind = cursor.u8("binary location kind")?;
        let location = match kind {
            1 => {
                let mut rows = [[FiniteReal::ZERO; 4]; 3];
                for row in &mut rows {
                    for value in row {
                        *value = cursor.finite_f64("binary location transform")?;
                    }
                }
                let transform = Transform::from_finite_rows(rows);
                invert_affine(transform)?;
                TextLocation {
                    factors: Vec::new(),
                    transform,
                }
            }
            2 => {
                let mut factors = Vec::new();
                let mut transform = Transform::identity();
                loop {
                    if factors.len() >= 1_000_000 {
                        return Err(CodecError::Malformed(
                            "binary location factor-count limit exceeded".into(),
                        ));
                    }
                    let referenced = cursor.i32("binary location factor")?;
                    if referenced == 0 {
                        break;
                    }
                    let referenced = usize::try_from(referenced).map_err(|_| {
                        CodecError::Malformed("negative binary location factor".into())
                    })?;
                    if referenced == 0 || referenced > locations.len() {
                        return Err(CodecError::malformed(format_args!(
                            "binary location {} references unavailable location {referenced}",
                            index + 1
                        )));
                    }
                    let power = cursor.i32("binary location power")?;
                    let powered =
                        transform_power(locations[referenced - 1].transform, i64::from(power))?;
                    transform = powered
                        .compose(transform)
                        .map_err(location_transform_error)?;
                    reserve_vec_items(
                        cursor.ctx,
                        &mut factors,
                        1,
                        "FreeCAD binary location factors",
                    )?;
                    factors.push(LocationFactor {
                        location: referenced,
                        power: i64::from(power),
                    });
                }
                TextLocation { factors, transform }
            }
            other => {
                return Err(CodecError::malformed(format_args!(
                    "invalid binary location type {other}"
                )));
            }
        };
        locations.push(location);
    }
    let curve_count = cursor.section_count("Curve2ds")?;
    // Each parameter curve consumes at least its 1-byte kind discriminant.
    let mut curve2ds = collection_vec(
        cursor.ctx,
        cursor.bounded(curve_count, 1, "binary Curve2ds")?,
        "FreeCAD B-rep parse_binary_prefix",
    )?;
    for _ in 0..curve_count {
        curve2ds.push(parse_binary_curve2d(&mut cursor, 0)?);
    }
    let curve_count = cursor.section_count("Curves")?;
    // Each 3D curve consumes at least its 1-byte kind discriminant.
    let mut curves = collection_vec(
        cursor.ctx,
        cursor.bounded(curve_count, 1, "binary Curves")?,
        "FreeCAD B-rep parse_binary_prefix",
    )?;
    for _ in 0..curve_count {
        curves.push(parse_binary_curve(&mut cursor, 0)?);
    }
    let polygon_count = cursor.section_count("Polygon3D")?;
    // Each 3D polygon consumes at least a 4-byte node count, a 1-byte flag, and an 8-byte deflection.
    let mut polygons3d = collection_vec(
        cursor.ctx,
        cursor.bounded(polygon_count, 13, "binary Polygon3D")?,
        "FreeCAD B-rep parse_binary_prefix",
    )?;
    for _ in 0..polygon_count {
        let node_count = cursor.count("binary 3D polygon node count")?;
        let has_parameters = cursor.bool("binary 3D polygon parameter flag")?;
        let deflection = cursor.finite_f64("binary 3D polygon deflection")?;
        let nodes = cursor.read_counted(node_count, "FreeCAD binary polygon nodes", |cursor| {
            cursor.finite_point3("binary 3D polygon node")
        })?;
        let parameters = has_parameters
            .then(|| {
                cursor.read_counted(node_count, "FreeCAD binary polygon parameters", |cursor| {
                    cursor.finite_f64("binary 3D polygon parameter")
                })
            })
            .transpose()?;
        polygons3d.push(TextPolygon3d {
            deflection: admit_polygon_deflection(deflection)?,
            nodes,
            parameters,
        });
    }
    let indexed_polygon_count = cursor.section_count("PolygonOnTriangulations")?;
    // Each indexed polygon consumes at least a 4-byte node count, an 8-byte deflection, and a 1-byte flag.
    let mut polygons_on_triangulations = collection_vec(
        cursor.ctx,
        cursor.bounded(indexed_polygon_count, 13, "binary PolygonOnTriangulations")?,
        "FreeCAD B-rep parse_binary_prefix",
    )?;
    for _ in 0..indexed_polygon_count {
        let node_count = cursor.count("binary indexed polygon node count")?;
        let nodes = cursor.read_counted(
            node_count,
            "FreeCAD binary indexed polygon nodes",
            |cursor| {
                let node = cursor.i32("binary indexed polygon node")?;
                u32::try_from(node).map_err(|_| {
                    CodecError::Malformed("non-positive binary indexed polygon node".into())
                })
            },
        )?;
        if nodes.contains(&0) {
            return Err(CodecError::Malformed(
                "binary indexed polygon node indices are one-based".into(),
            ));
        }
        let deflection = cursor.finite_f64("binary indexed polygon deflection")?;
        let has_parameters = cursor.bool("binary indexed polygon parameter flag")?;
        let parameters = has_parameters
            .then(|| {
                cursor.read_counted(
                    node_count,
                    "FreeCAD binary indexed polygon parameters",
                    |cursor| cursor.finite_f64("binary indexed polygon parameter"),
                )
            })
            .transpose()?;
        polygons_on_triangulations.push(TextPolygonOnTriangulation {
            nodes,
            deflection: admit_polygon_deflection(deflection)?,
            parameters,
        });
    }
    let surface_count = cursor.section_count("Surfaces")?;
    // Each surface consumes at least its 1-byte kind discriminant.
    let mut surfaces = collection_vec(
        cursor.ctx,
        cursor.bounded(surface_count, 1, "binary Surfaces")?,
        "FreeCAD B-rep parse_binary_prefix",
    )?;
    for _ in 0..surface_count {
        surfaces.push(parse_binary_surface(&mut cursor, 0)?);
    }
    let triangulation_count = cursor.section_count("Triangulations")?;
    // Each triangulation consumes at least two 4-byte counts, a 1-byte flag, and an 8-byte deflection.
    let mut triangulations = collection_vec(
        cursor.ctx,
        cursor.bounded(triangulation_count, 17, "binary Triangulations")?,
        "FreeCAD B-rep parse_binary_prefix",
    )?;
    for _ in 0..triangulation_count {
        let node_count = cursor.count("binary triangulation node count")?;
        let triangle_count = cursor.count("binary triangulation triangle count")?;
        let has_uv = cursor.bool("binary triangulation UV flag")?;
        let has_normals = version >= 4 && cursor.bool("binary triangulation normal flag")?;
        let deflection = cursor.finite_f64("binary triangulation deflection")?;
        let nodes =
            cursor.read_counted(node_count, "FreeCAD binary triangulation nodes", |cursor| {
                cursor.finite_point3("binary triangulation node")
            })?;
        let uv_nodes = has_uv
            .then(|| {
                cursor.read_counted(
                    node_count,
                    "FreeCAD binary triangulation UV nodes",
                    |cursor| cursor.finite_point2("binary triangulation UV node"),
                )
            })
            .transpose()?;
        let triangles = cursor.read_counted(
            triangle_count,
            "FreeCAD binary triangulation triangles",
            |cursor| {
                let mut triangle = [0_u32; 3];
                for node in &mut triangle {
                    let value = cursor.i32("binary triangulation triangle node")?;
                    *node = u32::try_from(value).map_err(|_| {
                        CodecError::Malformed("negative binary triangle node".into())
                    })?;
                }
                Ok(triangle)
            },
        )?;
        let normals = has_normals
            .then(|| {
                cursor.read_counted(
                    node_count,
                    "FreeCAD binary triangulation normals",
                    |cursor| cursor.finite_vector3_f32("binary triangulation normal"),
                )
            })
            .transpose()?;
        triangulations.push(
            TextTriangulation::from_admitted_parts(deflection, nodes, uv_nodes, triangles, normals)
                .map_err(CodecError::Malformed)?,
        );
    }
    let tshape_count = cursor.section_count("TShapes")?;
    // Each TShape consumes at least its 1-byte kind discriminant.
    let mut tshapes = collection_vec(
        cursor.ctx,
        cursor.bounded(tshape_count, 1, "binary TShapes")?,
        "FreeCAD B-rep parse_binary_prefix",
    )?;
    for index in 0..tshape_count {
        tshapes.push(parse_binary_tshape(
            &mut cursor,
            version,
            index + 1,
            tshape_count,
            curve_count,
            curve2ds.len(),
            surfaces.len(),
            locations.len(),
            polygons3d.len(),
            polygons_on_triangulations.len(),
            triangulations.len(),
        )?);
    }
    let roots = if cursor.remaining() == 0 {
        Vec::new()
    } else {
        if cursor.remaining() != 12 {
            return Err(CodecError::Malformed(
                "binary B-rep root record has invalid length".into(),
            ));
        }
        let shape = cursor.i32("binary root shape")?;
        let location = cursor.i32("binary root location")?;
        let orientation = cursor.i32("binary root orientation")?;
        if shape == -1 && location == -1 && orientation == -1 {
            Vec::new()
        } else {
            vec![TextShapeUse {
                shape: checked_binary_reference(shape, tshape_count, false, "root shape")?,
                location: checked_binary_reference(
                    location,
                    locations.len(),
                    true,
                    "root location",
                )?
                .into(),
                orientation: binary_orientation(orientation)?,
            }]
        }
    };
    let facts = ShapeSet {
        locations,
        curve2ds,
        curves,
        polygons3d,
        polygons_on_triangulations,
        surfaces,
        triangulations,
        tshapes: tshapes.into(),
        roots,
    };
    facts.validate().map_err(CodecError::Malformed)?;
    Ok((
        facts,
        BinaryTopologyVersion::try_from(version).map_err(CodecError::Malformed)?,
    ))
}

#[allow(clippy::too_many_arguments)]
fn parse_binary_tshape(
    cursor: &mut BinaryCursor<'_, '_, '_>,
    version: u8,
    index: usize,
    tshape_count: usize,
    curve_count: usize,
    curve2d_count: usize,
    surface_count: usize,
    location_count: usize,
    polygon3d_count: usize,
    indexed_polygon_count: usize,
    triangulation_count: usize,
) -> Result<TextTShape, CodecError> {
    let kind = match cursor.u8("binary TShape kind")? {
        0 => TextShapeKind::Compound,
        1 => TextShapeKind::CompSolid,
        2 => TextShapeKind::Solid,
        3 => TextShapeKind::Shell,
        4 => TextShapeKind::Face,
        5 => TextShapeKind::Wire,
        6 => TextShapeKind::Edge,
        7 => TextShapeKind::Vertex,
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid binary TShape kind {other}"
            )));
        }
    };
    let geometry = match kind {
        TextShapeKind::Vertex => {
            let tolerance = cursor.finite_f64("binary vertex tolerance")?;
            let point = cursor.finite_point3("binary vertex point")?;
            let mut representations = Vec::new();
            loop {
                let representation_kind = cursor.u8("binary vertex representation kind")?;
                if representation_kind == 0 {
                    break;
                }
                if representations.len() >= 1_000_000 {
                    return Err(CodecError::Malformed(
                        "binary vertex representation-count limit exceeded".into(),
                    ));
                }
                let parameter = cursor.finite_f64("binary vertex parameter")?;
                let representation = match representation_kind {
                    1 => TextPointRepresentation::Curve3d {
                        parameter,
                        curve: checked_binary_reference(
                            cursor.i32("binary vertex curve")?,
                            curve_count,
                            false,
                            "vertex curve",
                        )?,
                        location: checked_binary_reference(
                            cursor.i32("binary vertex location")?,
                            location_count,
                            true,
                            "vertex location",
                        )?,
                    },
                    2 => TextPointRepresentation::Pcurve {
                        parameter,
                        curve: checked_binary_reference(
                            cursor.i32("binary vertex pcurve")?,
                            curve2d_count,
                            false,
                            "vertex pcurve",
                        )?,
                        surface: checked_binary_reference(
                            cursor.i32("binary vertex surface")?,
                            surface_count,
                            false,
                            "vertex surface",
                        )?,
                        location: checked_binary_reference(
                            cursor.i32("binary vertex location")?,
                            location_count,
                            true,
                            "vertex location",
                        )?,
                    },
                    3 => TextPointRepresentation::Surface {
                        parameter,
                        second_parameter: cursor
                            .finite_f64("binary vertex second surface parameter")?,
                        surface: checked_binary_reference(
                            cursor.i32("binary vertex surface")?,
                            surface_count,
                            false,
                            "vertex surface",
                        )?,
                        location: checked_binary_reference(
                            cursor.i32("binary vertex location")?,
                            location_count,
                            true,
                            "vertex location",
                        )?,
                    },
                    other => {
                        return Err(CodecError::malformed(format_args!(
                            "invalid binary vertex representation kind {other}"
                        )));
                    }
                };
                reserve_vec_items(
                    cursor.ctx,
                    &mut representations,
                    1,
                    "FreeCAD binary vertex representations",
                )?;
                representations.push(representation);
            }
            TextTShapeGeometry::Vertex {
                tolerance,
                point,
                representations,
            }
        }
        TextShapeKind::Edge => {
            let tolerance = cursor.finite_f64("binary edge tolerance")?;
            let same_parameter = cursor.bool("binary edge same-parameter flag")?;
            let same_range = cursor.bool("binary edge same-range flag")?;
            let degenerated = cursor.bool("binary edge degenerated flag")?;
            let mut representations = Vec::new();
            loop {
                let representation_kind = cursor.u8("binary edge representation kind")?;
                if representation_kind == 0 {
                    break;
                }
                if representations.len() >= 1_000_000 {
                    return Err(CodecError::Malformed(
                        "binary edge representation-count limit exceeded".into(),
                    ));
                }
                reserve_vec_items(
                    cursor.ctx,
                    &mut representations,
                    1,
                    "FreeCAD binary edge representations",
                )?;
                representations.push(parse_binary_edge_representation(
                    cursor,
                    version,
                    representation_kind,
                    curve_count,
                    curve2d_count,
                    surface_count,
                    location_count,
                    polygon3d_count,
                    indexed_polygon_count,
                    triangulation_count,
                )?);
            }
            TextTShapeGeometry::Edge {
                tolerance,
                same_parameter,
                same_range,
                degenerated,
                representations,
            }
        }
        TextShapeKind::Face => {
            let natural_restriction = cursor.bool("binary face natural-restriction flag")?;
            let tolerance = cursor.finite_f64("binary face tolerance")?;
            let surface = checked_binary_reference(
                cursor.i32("binary face surface")?,
                surface_count,
                true,
                "face surface",
            )?;
            let location = checked_binary_reference(
                cursor.i32("binary face location")?,
                location_count,
                true,
                "face location",
            )?;
            let triangulation = match cursor.u8("binary face triangulation marker")? {
                0 | 1 => None,
                2 => Some(checked_binary_reference(
                    cursor.i32("binary face triangulation")?,
                    triangulation_count,
                    false,
                    "face triangulation",
                )?),
                other => {
                    return Err(CodecError::malformed(format_args!(
                        "invalid binary face triangulation marker {other}"
                    )));
                }
            };
            TextTShapeGeometry::Face {
                natural_restriction,
                tolerance,
                surface: TableRef::optional(surface),
                location: location.into(),
                triangulation: triangulation
                    .map(TableRef::new)
                    .transpose()
                    .map_err(CodecError::Malformed)?,
            }
        }
        TextShapeKind::Wire => TextTShapeGeometry::Wire,
        TextShapeKind::Shell => TextTShapeGeometry::Shell,
        TextShapeKind::Solid => TextTShapeGeometry::Solid,
        TextShapeKind::CompSolid => TextTShapeGeometry::CompSolid,
        TextShapeKind::Compound => TextTShapeGeometry::Compound,
    };
    let mut flags = [false; 7];
    for flag in &mut flags {
        *flag = cursor.bool("binary TShape flag")?;
    }
    let mut children = Vec::new();
    loop {
        let orientation = cursor.u8("binary child orientation")?;
        if orientation == b'*' {
            break;
        }
        let reverse_index = checked_binary_reference(
            cursor.i32("binary child reverse index")?,
            tshape_count,
            false,
            "child reverse index",
        )?;
        let shape = tshape_count - reverse_index + 1;
        if shape >= index {
            return Err(CodecError::malformed(format_args!(
                "binary TShape {index} references non-prior child {shape}"
            )));
        }
        reserve_vec_items(
            cursor.ctx,
            &mut children,
            1,
            "FreeCAD binary shape children",
        )?;
        children.push(TextShapeUse {
            shape,
            orientation: binary_orientation(i32::from(orientation))?,
            location: checked_binary_reference(
                cursor.i32("binary child location")?,
                location_count,
                true,
                "child location",
            )?
            .into(),
        });
    }
    Ok(TextTShape {
        geometry,
        flags,
        children,
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_binary_edge_representation(
    cursor: &mut BinaryCursor<'_, '_, '_>,
    version: u8,
    kind: u8,
    curve_count: usize,
    curve2d_count: usize,
    surface_count: usize,
    location_count: usize,
    polygon3d_count: usize,
    indexed_polygon_count: usize,
    triangulation_count: usize,
) -> Result<TextEdgeRepresentation, CodecError> {
    match kind {
        1 => {
            let curve = checked_binary_reference(
                cursor.i32("binary edge curve")?,
                curve_count,
                false,
                "edge curve",
            )?;
            let location = checked_binary_reference(
                cursor.i32("binary edge curve location")?,
                location_count,
                true,
                "edge curve location",
            )?;
            let parameter_range = [
                cursor.finite_f64("binary edge curve start")?,
                cursor.finite_f64("binary edge curve end")?,
            ];
            Ok(TextEdgeRepresentation::Curve3d {
                curve,
                location,
                parameter_range,
            })
        }
        2 | 3 => {
            let curve = checked_binary_reference(
                cursor.i32("binary edge pcurve")?,
                curve2d_count,
                false,
                "edge pcurve",
            )?;
            let secondary = if kind == 3 {
                Some((
                    checked_binary_reference(
                        cursor.i32("binary edge secondary pcurve")?,
                        curve2d_count,
                        false,
                        "edge secondary pcurve",
                    )?,
                    cursor.u8("binary edge continuity")?.to_string(),
                ))
            } else {
                None
            };
            let surface = checked_binary_reference(
                cursor.i32("binary edge surface")?,
                surface_count,
                false,
                "edge surface",
            )?;
            let location = checked_binary_reference(
                cursor.i32("binary edge surface location")?,
                location_count,
                true,
                "edge surface location",
            )?;
            let parameter_range = [
                cursor.finite_f64("binary edge pcurve start")?,
                cursor.finite_f64("binary edge pcurve end")?,
            ];
            let uv_endpoints = if matches!(version, 2 | 3) {
                Some([
                    cursor.finite_point2("binary edge first UV endpoint")?,
                    cursor.finite_point2("binary edge last UV endpoint")?,
                ])
            } else {
                None
            };
            Ok(if let Some((secondary, continuity)) = secondary {
                TextEdgeRepresentation::PcurvePair {
                    curves: [curve, secondary],
                    continuity,
                    surface,
                    location,
                    parameter_range,
                    uv_endpoints,
                }
            } else {
                TextEdgeRepresentation::Pcurve {
                    curve,
                    surface,
                    location,
                    parameter_range,
                    uv_endpoints,
                }
            })
        }
        4 => {
            let continuity = cursor.u8("binary edge continuity")?.to_string();
            let first_surface = checked_binary_reference(
                cursor.i32("binary edge regularity surface")?,
                surface_count,
                false,
                "edge regularity surface",
            )?;
            let location = checked_binary_reference(
                cursor.i32("binary edge regularity location")?,
                location_count,
                true,
                "edge regularity location",
            )?;
            let second_surface = checked_binary_reference(
                cursor.i32("binary edge second regularity surface")?,
                surface_count,
                false,
                "edge second regularity surface",
            )?;
            let second_location = checked_binary_reference(
                cursor.i32("binary edge second regularity location")?,
                location_count,
                true,
                "edge second regularity location",
            )?;
            Ok(TextEdgeRepresentation::Regularity {
                continuity,
                surfaces: [first_surface, second_surface],
                locations: [location, second_location],
            })
        }
        5 => {
            let polygon = checked_binary_reference(
                cursor.i32("binary edge 3D polygon")?,
                polygon3d_count,
                false,
                "edge 3D polygon",
            )?;
            let location = checked_binary_reference(
                cursor.i32("binary edge polygon location")?,
                location_count,
                true,
                "edge polygon location",
            )?;
            Ok(TextEdgeRepresentation::Polygon3d { polygon, location })
        }
        6 | 7 => {
            let polygon = checked_binary_reference(
                cursor.i32("binary edge indexed polygon")?,
                indexed_polygon_count,
                false,
                "edge indexed polygon",
            )?;
            let secondary = if kind == 7 {
                Some(checked_binary_reference(
                    cursor.i32("binary edge secondary indexed polygon")?,
                    indexed_polygon_count,
                    false,
                    "edge secondary indexed polygon",
                )?)
            } else {
                None
            };
            let triangulation = checked_binary_reference(
                cursor.i32("binary edge triangulation")?,
                triangulation_count,
                false,
                "edge triangulation",
            )?;
            let location = checked_binary_reference(
                cursor.i32("binary edge triangulation location")?,
                location_count,
                true,
                "edge triangulation location",
            )?;
            Ok(if let Some(secondary) = secondary {
                TextEdgeRepresentation::PolygonPair {
                    polygons: [polygon, secondary],
                    triangulation,
                    location,
                }
            } else {
                TextEdgeRepresentation::PolygonOnTriangulation {
                    polygon,
                    triangulation,
                    location,
                }
            })
        }
        other => Err(CodecError::malformed(format_args!(
            "invalid binary edge representation kind {other}"
        ))),
    }
}

fn checked_binary_reference(
    value: i32,
    count: usize,
    allow_zero: bool,
    label: &str,
) -> Result<usize, CodecError> {
    let value = usize::try_from(value)
        .map_err(|_| CodecError::malformed(format_args!("negative binary {label}")))?;
    if value > count || (!allow_zero && value == 0) {
        return Err(CodecError::malformed(format_args!(
            "binary {label} index {value} exceeds table count {count}"
        )));
    }
    Ok(value)
}

fn binary_orientation(value: i32) -> Result<TextOrientation, CodecError> {
    match value {
        0 => Ok(TextOrientation::Forward),
        1 => Ok(TextOrientation::Reversed),
        2 => Ok(TextOrientation::Internal),
        3 => Ok(TextOrientation::External),
        other => Err(CodecError::malformed(format_args!(
            "invalid binary orientation {other}"
        ))),
    }
}

fn parse_binary_surface(
    cursor: &mut BinaryCursor<'_, '_, '_>,
    depth: usize,
) -> Result<TextSurface, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "binary surface nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    Ok(match cursor.u8("binary surface kind")? {
        1 => {
            let origin = cursor.finite_point3("binary plane origin")?;
            let axis = cursor.finite_vector3("binary plane axis")?;
            let u_axis = cursor.finite_vector3("binary plane u axis")?;
            let v_axis = cursor.finite_vector3("binary plane v axis")?;
            TextSurface::Plane {
                origin,
                axis,
                u_axis,
                v_reversed: frame_v_reversed(axis.get(), u_axis.get(), v_axis.get()),
            }
        }
        2 => {
            let origin = cursor.finite_point3("binary cylinder origin")?;
            let axis = cursor.finite_vector3("binary cylinder axis")?;
            let ref_direction = cursor.finite_vector3("binary cylinder reference direction")?;
            let y_direction = cursor.finite_vector3("binary cylinder v direction")?;
            TextSurface::Cylinder {
                origin,
                axis,
                ref_direction,
                radius: cursor.finite_f64("binary cylinder radius")?,
                u_reversed: frame_v_reversed(axis.get(), ref_direction.get(), y_direction.get()),
            }
        }
        3 => {
            let origin = cursor.finite_point3("binary cone origin")?;
            let axis = cursor.finite_vector3("binary cone axis")?;
            let ref_direction = cursor.finite_vector3("binary cone reference direction")?;
            let y_direction = cursor.finite_vector3("binary cone v direction")?;
            TextSurface::Cone {
                origin,
                axis,
                ref_direction,
                radius: cursor.finite_f64("binary cone reference radius")?,
                half_angle: cursor.finite_f64("binary cone half angle")?,
                u_reversed: frame_v_reversed(axis.get(), ref_direction.get(), y_direction.get()),
            }
        }
        4 => {
            let center = cursor.finite_point3("binary sphere center")?;
            let axis = cursor.finite_vector3("binary sphere axis")?;
            let ref_direction = cursor.finite_vector3("binary sphere reference direction")?;
            let y_direction = cursor.finite_vector3("binary sphere v direction")?;
            TextSurface::Sphere {
                center,
                axis,
                ref_direction,
                radius: cursor.finite_f64("binary sphere radius")?,
                u_reversed: frame_v_reversed(axis.get(), ref_direction.get(), y_direction.get()),
            }
        }
        5 => {
            let center = cursor.finite_point3("binary torus center")?;
            let axis = cursor.finite_vector3("binary torus axis")?;
            let ref_direction = cursor.finite_vector3("binary torus reference direction")?;
            let y_direction = cursor.finite_vector3("binary torus v direction")?;
            TextSurface::Torus {
                center,
                axis,
                ref_direction,
                major_radius: cursor.finite_f64("binary torus major radius")?,
                minor_radius: cursor.finite_f64("binary torus minor radius")?,
                u_reversed: frame_v_reversed(axis.get(), ref_direction.get(), y_direction.get()),
            }
        }
        6 => TextSurface::Extrusion {
            direction: cursor.finite_vector3("binary extrusion direction")?,
            directrix: NestedCurve::try_new(parse_binary_curve(cursor, depth + 1)?)
                .map_err(CodecError::malformed)?,
        },
        7 => TextSurface::Revolution {
            axis_origin: cursor.finite_point3("binary revolution axis origin")?,
            axis_direction: cursor.finite_vector3("binary revolution axis direction")?,
            directrix: NestedCurve::try_new(parse_binary_curve(cursor, depth + 1)?)
                .map_err(CodecError::malformed)?,
        },
        8 => {
            let u_rational = cursor.bool("binary Bezier u-rational flag")?;
            let v_rational = cursor.bool("binary Bezier v-rational flag")?;
            let u_degree = usize::from(cursor.u16("binary Bezier u degree")?);
            let v_degree = usize::from(cursor.u16("binary Bezier v degree")?);
            let u_count = u_degree.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("binary Bezier u pole count overflow".into())
            })?;
            let v_count = v_degree.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("binary Bezier v pole count overflow".into())
            })?;
            let pole_count = checked_grid_count(u_count, v_count, "binary Bezier")?;
            let rational = u_rational || v_rational;
            // Each pole consumes at least a 24-byte point3.
            let capacity = cursor.bounded(pole_count, 24, "binary Bezier surface pole")?;
            let mut control_points =
                collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_binary_surface")?;
            let mut weights =
                optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point3("binary Bezier surface pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary Bezier surface weight")?);
                }
            }
            TextSurface::Nurbs(
                NurbsSurface::from_finite_lanes(
                    NurbsSurfaceAxis::new(
                        u32::try_from(u_degree).map_err(|_| {
                            CodecError::Malformed("binary Bezier u degree exceeds u32".into())
                        })?,
                        clamped_bezier_knots(cursor.ctx, u_degree)?,
                        false,
                    ),
                    NurbsSurfaceAxis::new(
                        u32::try_from(v_degree).map_err(|_| {
                            CodecError::Malformed("binary Bezier v degree exceeds u32".into())
                        })?,
                        clamped_bezier_knots(cursor.ctx, v_degree)?,
                        false,
                    ),
                    NurbsSurfaceLanes::new(
                        grid_rows(cursor.ctx, control_points, v_count)?,
                        weights
                            .map(|values| grid_rows(cursor.ctx, values, v_count))
                            .transpose()?,
                    ),
                    false,
                )
                .map_err(|error| CodecError::Malformed(error.to_string()))?,
            )
        }
        9 => {
            let u_rational = cursor.bool("binary B-spline u-rational flag")?;
            let v_rational = cursor.bool("binary B-spline v-rational flag")?;
            let u_periodic = cursor.bool("binary B-spline u-periodic flag")?;
            let v_periodic = cursor.bool("binary B-spline v-periodic flag")?;
            let u_degree = u32::from(cursor.u16("binary B-spline u degree")?);
            let v_degree = u32::from(cursor.u16("binary B-spline v degree")?);
            let u_count = cursor.count("binary B-spline u pole count")?;
            let v_count = cursor.count("binary B-spline v pole count")?;
            let u_knot_count = cursor.count("binary B-spline u knot count")?;
            let v_knot_count = cursor.count("binary B-spline v knot count")?;
            let pole_count = checked_grid_count(u_count, v_count, "binary B-spline")?;
            let rational = u_rational || v_rational;
            // Each pole consumes at least a 24-byte point3.
            let capacity = cursor.bounded(pole_count, 24, "binary B-spline surface pole")?;
            let mut control_points =
                collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_binary_surface")?;
            let mut weights =
                optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point3("binary B-spline surface pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary B-spline surface weight")?);
                }
            }
            TextSurface::Nurbs(normalize_periodic_surface(
                cursor.ctx,
                [u_degree, v_degree],
                [
                    cursor.expanded_knots(u_knot_count, "binary B-spline u knots")?,
                    cursor.expanded_knots(v_knot_count, "binary B-spline v knots")?,
                ],
                [u_count, v_count],
                control_points,
                weights,
                [u_periodic, v_periodic],
            )?)
        }
        10 => TextSurface::Trimmed {
            parameter_ranges: [
                [
                    cursor.finite_f64("binary surface u trim start")?,
                    cursor.finite_f64("binary surface u trim end")?,
                ],
                [
                    cursor.finite_f64("binary surface v trim start")?,
                    cursor.finite_f64("binary surface v trim end")?,
                ],
            ],
            basis: NestedSurface::try_new(parse_binary_surface(cursor, depth + 1)?)
                .map_err(CodecError::malformed)?,
        },
        11 => TextSurface::Offset {
            distance: cursor.finite_f64("binary surface offset")?,
            basis: NestedSurface::try_new(parse_binary_surface(cursor, depth + 1)?)
                .map_err(CodecError::malformed)?,
        },
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid binary surface kind {other}"
            )));
        }
    })
}

fn checked_grid_count(u_count: usize, v_count: usize, label: &str) -> Result<usize, CodecError> {
    u_count
        .checked_mul(v_count)
        .filter(|count| *count <= 1_000_000)
        .ok_or_else(|| CodecError::malformed(format_args!("{label} pole-count limit exceeded")))
}

fn parse_binary_curve(
    cursor: &mut BinaryCursor<'_, '_, '_>,
    depth: usize,
) -> Result<TextCurve, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "binary 3D curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    Ok(match cursor.u8("binary 3D curve kind")? {
        1 => TextCurve::Line {
            origin: cursor.finite_point3("binary line origin")?,
            direction: cursor.finite_vector3("binary line direction")?,
        },
        2 => {
            let center = cursor.finite_point3("binary circle center")?;
            let axis = cursor.finite_vector3("binary circle axis")?;
            let ref_direction = cursor.finite_vector3("binary circle reference direction")?;
            cursor.finite_vector3("binary circle y axis")?;
            TextCurve::Circle {
                center,
                axis,
                ref_direction,
                radius: cursor.finite_f64("binary circle radius")?,
            }
        }
        3 => {
            let center = cursor.finite_point3("binary ellipse center")?;
            let axis = cursor.finite_vector3("binary ellipse axis")?;
            let major_direction = cursor.finite_vector3("binary ellipse major direction")?;
            cursor.finite_vector3("binary ellipse minor direction")?;
            TextCurve::Ellipse {
                center,
                axis,
                major_direction,
                major_radius: cursor.finite_f64("binary ellipse major radius")?,
                minor_radius: cursor.finite_f64("binary ellipse minor radius")?,
            }
        }
        4 => {
            let vertex = cursor.finite_point3("binary parabola vertex")?;
            let axis = cursor.finite_vector3("binary parabola axis")?;
            let major_direction = cursor.finite_vector3("binary parabola major direction")?;
            cursor.finite_vector3("binary parabola minor direction")?;
            TextCurve::Parabola {
                vertex,
                axis,
                major_direction,
                focal_distance: cursor.finite_f64("binary parabola focal distance")?,
            }
        }
        5 => {
            let center = cursor.finite_point3("binary hyperbola center")?;
            let axis = cursor.finite_vector3("binary hyperbola axis")?;
            let major_direction = cursor.finite_vector3("binary hyperbola major direction")?;
            cursor.finite_vector3("binary hyperbola minor direction")?;
            TextCurve::Hyperbola {
                center,
                axis,
                major_direction,
                major_radius: cursor.finite_f64("binary hyperbola major radius")?,
                minor_radius: cursor.finite_f64("binary hyperbola minor radius")?,
            }
        }
        6 => {
            let rational = cursor.bool("binary Bezier rational flag")?;
            let degree = usize::from(cursor.u16("binary Bezier degree")?);
            let pole_count = degree
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("binary Bezier pole count overflow".into()))?;
            // Each pole consumes at least a 24-byte point3.
            let capacity = cursor.bounded(pole_count, 24, "binary Bezier pole")?;
            let mut control_points =
                collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_binary_curve")?;
            let mut weights =
                optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point3("binary Bezier pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary Bezier weight")?);
                }
            }
            TextCurve::Nurbs(
                NurbsCurve::from_finite_lanes(
                    u32::try_from(degree).map_err(|_| {
                        CodecError::Malformed("binary Bezier degree exceeds u32".into())
                    })?,
                    clamped_bezier_knots(cursor.ctx, degree)?,
                    control_points,
                    weights,
                    false,
                )
                .map_err(|error| CodecError::Malformed(error.to_string()))?,
            )
        }
        7 => {
            let rational = cursor.bool("binary B-spline rational flag")?;
            let periodic = cursor.bool("binary B-spline periodic flag")?;
            let degree = u32::from(cursor.u16("binary B-spline degree")?);
            let pole_count = cursor.count("binary B-spline pole count")?;
            let knot_count = cursor.count("binary B-spline knot count")?;
            // Each pole consumes at least a 24-byte point3.
            let capacity = cursor.bounded(pole_count, 24, "binary B-spline pole")?;
            let mut control_points =
                collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_binary_curve")?;
            let mut weights =
                optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point3("binary B-spline pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary B-spline weight")?);
                }
            }
            let knots = cursor.expanded_knots(knot_count, "binary B-spline")?;
            let (knots, padding) = normalize_periodic_knots(cursor.ctx, knots, degree, periodic)?;
            append_periodic_curve_poles(
                cursor.ctx,
                &mut control_points,
                weights.as_mut(),
                padding,
            )?;
            TextCurve::Nurbs(
                NurbsCurve::from_finite_lanes(degree, knots, control_points, weights, periodic)
                    .map_err(|error| CodecError::Malformed(error.to_string()))?,
            )
        }
        8 => TextCurve::Trimmed {
            parameter_range: [
                cursor.finite_f64("binary trim start")?,
                cursor.finite_f64("binary trim end")?,
            ],
            basis: NestedCurve::try_new(parse_binary_curve(cursor, depth + 1)?)
                .map_err(CodecError::malformed)?,
        },
        9 => TextCurve::Offset {
            distance: cursor.finite_f64("binary offset distance")?,
            direction: cursor.finite_vector3("binary offset direction")?,
            basis: NestedCurve::try_new(parse_binary_curve(cursor, depth + 1)?)
                .map_err(CodecError::malformed)?,
        },
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid binary 3D curve kind {other}"
            )));
        }
    })
}

fn parse_binary_curve2d(
    cursor: &mut BinaryCursor<'_, '_, '_>,
    depth: usize,
) -> Result<TextCurve2d, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "binary parameter-curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    Ok(match cursor.u8("binary parameter-curve kind")? {
        1 => TextCurve2d::Line {
            origin: cursor.finite_point2("binary line origin")?,
            direction: cursor.finite_point2("binary line direction")?,
        },
        2 => TextCurve2d::Circle {
            center: cursor.finite_point2("binary circle center")?,
            x_axis: cursor.finite_point2("binary circle x axis")?,
            y_axis: cursor.finite_point2("binary circle y axis")?,
            radius: cursor.finite_f64("binary circle radius")?,
        },
        3 => TextCurve2d::Ellipse {
            center: cursor.finite_point2("binary ellipse center")?,
            x_axis: cursor.finite_point2("binary ellipse x axis")?,
            y_axis: cursor.finite_point2("binary ellipse y axis")?,
            major_radius: cursor.finite_f64("binary ellipse major radius")?,
            minor_radius: cursor.finite_f64("binary ellipse minor radius")?,
        },
        4 => TextCurve2d::Parabola {
            vertex: cursor.finite_point2("binary parabola vertex")?,
            x_axis: cursor.finite_point2("binary parabola x axis")?,
            y_axis: cursor.finite_point2("binary parabola y axis")?,
            focal_distance: cursor.finite_f64("binary parabola focal distance")?,
        },
        5 => TextCurve2d::Hyperbola {
            center: cursor.finite_point2("binary hyperbola center")?,
            x_axis: cursor.finite_point2("binary hyperbola x axis")?,
            y_axis: cursor.finite_point2("binary hyperbola y axis")?,
            major_radius: cursor.finite_f64("binary hyperbola major radius")?,
            minor_radius: cursor.finite_f64("binary hyperbola minor radius")?,
        },
        6 => {
            let rational = cursor.bool("binary Bezier rational flag")?;
            let degree = usize::from(cursor.u16("binary Bezier degree")?);
            let pole_count = degree
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("binary Bezier pole count overflow".into()))?;
            // Each pole consumes at least a 16-byte point2.
            let capacity = cursor.bounded(pole_count, 16, "binary Bezier parameter pole")?;
            let mut control_points =
                collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_binary_curve2d")?;
            let mut weights =
                optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point2("binary Bezier pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary Bezier weight")?);
                }
            }
            TextCurve2d::Nurbs(NurbsCurve2d {
                degree: u32::try_from(degree).map_err(|_| {
                    CodecError::Malformed("binary Bezier degree exceeds u32".into())
                })?,
                knots: clamped_bezier_knots(cursor.ctx, degree)?,
                control_points,
                weights,
                periodic: false,
            })
        }
        7 => {
            let rational = cursor.bool("binary B-spline rational flag")?;
            let periodic = cursor.bool("binary B-spline periodic flag")?;
            let degree = u32::from(cursor.u16("binary B-spline degree")?);
            let pole_count = cursor.count("binary B-spline pole count")?;
            let knot_count = cursor.count("binary B-spline knot count")?;
            // Each pole consumes at least a 16-byte point2.
            let capacity = cursor.bounded(pole_count, 16, "binary B-spline parameter pole")?;
            let mut control_points =
                collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_binary_curve2d")?;
            let mut weights =
                optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point2("binary B-spline pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary B-spline weight")?);
                }
            }
            let knots = cursor.expanded_knots(knot_count, "binary B-spline")?;
            let (knots, padding) = normalize_periodic_knots(cursor.ctx, knots, degree, periodic)?;
            append_periodic_curve_poles(
                cursor.ctx,
                &mut control_points,
                weights.as_mut(),
                padding,
            )?;
            TextCurve2d::Nurbs(NurbsCurve2d {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            })
        }
        8 => TextCurve2d::Trimmed {
            parameter_range: [
                cursor.finite_f64("binary trim start")?,
                cursor.finite_f64("binary trim end")?,
            ],
            basis: NestedCurve2d::try_new(parse_binary_curve2d(cursor, depth + 1)?)
                .map_err(CodecError::malformed)?,
        },
        9 => TextCurve2d::Offset {
            distance: cursor.finite_f64("binary offset distance")?,
            basis: NestedCurve2d::try_new(parse_binary_curve2d(cursor, depth + 1)?)
                .map_err(CodecError::malformed)?,
        },
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid binary parameter-curve kind {other}"
            )));
        }
    })
}

struct BinaryCursor<'a, 'c, 'r> {
    view: View<'a>,
    ctx: &'c DecodeContext<'r>,
}

impl<'a, 'c, 'r> BinaryCursor<'a, 'c, 'r> {
    fn new(ctx: &'c DecodeContext<'r>, bytes: &'a [u8]) -> Self {
        Self {
            view: View::over_retained(bytes),
            ctx,
        }
    }

    fn remaining(&self) -> usize {
        self.view.remaining()
    }

    fn read_counted<T>(
        &mut self,
        count: usize,
        operation: &'static str,
        mut read: impl FnMut(&mut Self) -> Result<T, CodecError>,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = collection_vec(self.ctx, count, operation)?;
        for _ in 0..count {
            values.push(read(self)?);
        }
        Ok(values)
    }

    fn truncated(label: &str) -> CodecError {
        CodecError::malformed(format_args!("truncated {label}"))
    }

    /// Clamps a declared element count to what the unread bytes can hold.
    ///
    /// `element_size` is the minimum encoded bytes of one element; a count that
    /// could not physically fit in the remaining input is rejected.
    fn bounded(&self, count: usize, element_size: usize, label: &str) -> Result<usize, CodecError> {
        bounded_len(count as u64, element_size, self.remaining()).ok_or_else(|| {
            CodecError::malformed(format_args!("{label} count exceeds remaining input"))
        })
    }

    fn take(&mut self, count: usize, label: &str) -> Result<&'a [u8], CodecError> {
        self.view.take(count).ok_or_else(|| Self::truncated(label))
    }

    fn line(&mut self, label: &str) -> Result<&'a str, CodecError> {
        let tail = self.view.unread();
        let length = tail
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| CodecError::malformed(format_args!("unterminated {label}")))?;
        let line = self.take(length + 1, label)?;
        std::str::from_utf8(&line[..length])
            .map_err(|_| CodecError::malformed(format_args!("non-UTF-8 {label}")))
    }

    fn section_count(&mut self, name: &str) -> Result<usize, CodecError> {
        let line = loop {
            let line = self.line(name)?;
            if !line.trim().is_empty() {
                break line;
            }
        };
        let mut tokens = line.split_ascii_whitespace();
        if tokens.next() != Some(name) || tokens.clone().count() != 1 {
            return Err(crate::resource::malformed_charged(
                self.ctx,
                format_args!("binary B-rep expected {name} section, found {line:?}"),
                "FreeCAD binary section diagnostic",
            ));
        }
        tokens
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| CodecError::malformed(format_args!("invalid binary {name} count")))
    }

    fn u8(&mut self, label: &str) -> Result<u8, CodecError> {
        self.view.u8().ok_or_else(|| Self::truncated(label))
    }

    fn bool(&mut self, label: &str) -> Result<bool, CodecError> {
        match self.u8(label)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(CodecError::malformed(format_args!(
                "invalid {label} byte {other}"
            ))),
        }
    }

    fn u16(&mut self, label: &str) -> Result<u16, CodecError> {
        self.view.u16_le().ok_or_else(|| Self::truncated(label))
    }

    fn i32(&mut self, label: &str) -> Result<i32, CodecError> {
        self.view.i32_le().ok_or_else(|| Self::truncated(label))
    }

    fn count(&mut self, label: &str) -> Result<usize, CodecError> {
        let value = self.i32(label)?;
        usize::try_from(value)
            .ok()
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| CodecError::malformed(format_args!("invalid {label}")))
    }

    fn finite_f64(&mut self, label: &str) -> Result<FiniteReal, CodecError> {
        let value = self.view.f64_le().ok_or_else(|| Self::truncated(label))?;
        FiniteReal::new(value)
            .ok_or_else(|| CodecError::malformed(format_args!("non-finite {label}")))
    }

    fn f32(&mut self, label: &str) -> Result<FiniteBinary32, CodecError> {
        let value = self.view.f32_le().ok_or_else(|| Self::truncated(label))?;
        FiniteBinary32::new(value)
            .ok_or_else(|| CodecError::malformed(format_args!("non-finite {label}")))
    }

    fn finite_point2(&mut self, label: &str) -> Result<FinitePoint2, CodecError> {
        Ok(FinitePoint2::from_coordinates(
            self.finite_f64(label)?,
            self.finite_f64(label)?,
        ))
    }

    fn finite_point3(&mut self, label: &str) -> Result<FinitePoint3, CodecError> {
        Ok(FinitePoint3::from_coordinates(
            self.finite_f64(label)?,
            self.finite_f64(label)?,
            self.finite_f64(label)?,
        ))
    }

    fn finite_vector3(&mut self, label: &str) -> Result<FiniteVector3, CodecError> {
        Ok(FiniteVector3::from_components(
            self.finite_f64(label)?,
            self.finite_f64(label)?,
            self.finite_f64(label)?,
        ))
    }

    fn finite_vector3_f32(&mut self, label: &str) -> Result<FiniteVector3, CodecError> {
        Ok(FiniteVector3::from_components(
            self.f32(label)?.into(),
            self.f32(label)?.into(),
            self.f32(label)?.into(),
        ))
    }

    fn expanded_knots(&mut self, count: usize, label: &str) -> Result<Vec<FiniteReal>, CodecError> {
        let mut knots = Vec::new();
        for _ in 0..count {
            let knot = self.finite_f64(label)?;
            let multiplicity = self.count(label)?;
            if knots
                .len()
                .checked_add(multiplicity)
                .is_none_or(|len| len > 1_000_000)
            {
                return Err(CodecError::malformed(format_args!(
                    "{label} expanded knot-count limit exceeded"
                )));
            }
            reserve_vec_items(
                self.ctx,
                &mut knots,
                multiplicity,
                "FreeCAD binary expanded knots",
            )?;
            knots.extend(std::iter::repeat_n(knot, multiplicity));
        }
        Ok(knots)
    }
}

fn parse_locations(
    ctx: &DecodeContext<'_>,
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextLocation>, CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == "Locations")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Locations table".into()))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == "Curve2ds")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Curve2ds table".into()))?;
    let count = section_counts.get("Locations").copied().unwrap_or(0);
    let mut cursor = TokenCursor::new(ctx, &tokens[start..end]);
    // Each location consumes at least its one type token.
    let mut locations: Vec<TextLocation> = collection_vec(
        cursor.ctx,
        cursor.bounded(count, 1, "text Locations")?,
        "FreeCAD B-rep parse_locations",
    )?;
    for index in 0..count {
        let kind = cursor.integer("location type")?;
        let location = match kind {
            1 => {
                let mut rows = [[FiniteReal::ZERO; 4]; 3];
                for row in &mut rows {
                    for value in row {
                        *value = cursor.finite_real("location transform value")?;
                    }
                }
                let transform = Transform::from_finite_rows(rows);
                invert_affine(transform)?;
                TextLocation {
                    factors: Vec::new(),
                    transform,
                }
            }
            2 => {
                let mut factors = Vec::new();
                let mut transform = Transform::identity();
                loop {
                    let referenced = cursor.integer("location factor index")?;
                    if referenced == 0 {
                        break;
                    }
                    let referenced = usize::try_from(referenced).map_err(|_| {
                        CodecError::Malformed("negative location factor index".into())
                    })?;
                    if referenced == 0 || referenced > locations.len() {
                        return Err(CodecError::malformed(format_args!(
                            "location {} references unavailable location {referenced}",
                            index + 1
                        )));
                    }
                    if factors.len() >= 1_000_000 {
                        return Err(CodecError::Malformed(
                            "location factor-count limit exceeded".into(),
                        ));
                    }
                    let power = cursor.integer("location factor power")?;
                    let powered = transform_power(locations[referenced - 1].transform, power)?;
                    transform = powered
                        .compose(transform)
                        .map_err(location_transform_error)?;
                    reserve_vec_items(
                        cursor.ctx,
                        &mut factors,
                        1,
                        "FreeCAD text location factors",
                    )?;
                    factors.push(LocationFactor {
                        location: referenced,
                        power,
                    });
                }
                TextLocation { factors, transform }
            }
            other => {
                return Err(CodecError::malformed(format_args!(
                    "invalid location type {other} at table index {}",
                    index + 1
                )));
            }
        };
        locations.push(location);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep Locations table contains trailing tokens".into(),
        ));
    }
    Ok(locations)
}

fn parse_geometry_table<T>(
    ctx: &DecodeContext<'_>,
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
    table: &str,
    next_table: &str,
    mut parse: impl FnMut(&mut TokenCursor<'_, '_, '_>, usize, usize) -> Result<T, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == table)
        .ok_or_else(|| CodecError::malformed(format_args!("text B-rep has no {table} table")))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == next_table)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("text B-rep has no {next_table} table"))
        })?;
    let count = section_counts.get(table).copied().unwrap_or(0);
    let mut cursor = TokenCursor::new(
        ctx,
        tokens.get(start..end).ok_or_else(|| {
            CodecError::malformed(format_args!("text B-rep {table} table has invalid bounds"))
        })?,
    );
    // Every row consumes at least its type token.
    let mut curves = collection_vec(
        cursor.ctx,
        cursor.bounded(count, 1, &format!("text {table}"))?,
        "FreeCAD B-rep parse_geometry_table",
    )?;
    for index in 0..count {
        curves.push(parse(&mut cursor, 0, index + 1)?);
    }
    if !cursor.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "text B-rep {table} table contains trailing tokens"
        )));
    }
    Ok(curves)
}

fn parse_curve2d(
    cursor: &mut TokenCursor<'_, '_, '_>,
    depth: usize,
    table_index: usize,
) -> Result<TextCurve2d, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "text B-rep 2D curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    let kind = cursor.integer("2D curve type")?;
    Ok(match kind {
        1 => TextCurve2d::Line {
            origin: cursor.finite_point2("2D line origin")?,
            direction: cursor.finite_point2("2D line direction")?,
        },
        2 => TextCurve2d::Circle {
            center: cursor.finite_point2("2D circle center")?,
            x_axis: cursor.finite_point2("2D circle x axis")?,
            y_axis: cursor.finite_point2("2D circle y axis")?,
            radius: cursor.finite_real("2D circle radius")?,
        },
        3 => TextCurve2d::Ellipse {
            center: cursor.finite_point2("2D ellipse center")?,
            x_axis: cursor.finite_point2("2D ellipse x axis")?,
            y_axis: cursor.finite_point2("2D ellipse y axis")?,
            major_radius: cursor.finite_real("2D ellipse major radius")?,
            minor_radius: cursor.finite_real("2D ellipse minor radius")?,
        },
        4 => TextCurve2d::Parabola {
            vertex: cursor.finite_point2("2D parabola vertex")?,
            x_axis: cursor.finite_point2("2D parabola x axis")?,
            y_axis: cursor.finite_point2("2D parabola y axis")?,
            focal_distance: cursor.finite_real("2D parabola focal distance")?,
        },
        5 => TextCurve2d::Hyperbola {
            center: cursor.finite_point2("2D hyperbola center")?,
            x_axis: cursor.finite_point2("2D hyperbola x axis")?,
            y_axis: cursor.finite_point2("2D hyperbola y axis")?,
            major_radius: cursor.finite_real("2D hyperbola major radius")?,
            minor_radius: cursor.finite_real("2D hyperbola minor radius")?,
        },
        6 => TextCurve2d::Nurbs(parse_bezier_curve2d(cursor)?),
        7 => TextCurve2d::Nurbs(parse_nurbs_curve2d(cursor)?),
        8 => {
            let first = cursor.finite_real("trimmed 2D curve first parameter")?;
            let last = cursor.finite_real("trimmed 2D curve last parameter")?;
            if first > last {
                return Err(CodecError::Malformed(
                    "trimmed 2D curve parameter range is reversed".into(),
                ));
            }
            TextCurve2d::Trimmed {
                parameter_range: [first, last],
                basis: NestedCurve2d::try_new(parse_curve2d(cursor, depth + 1, table_index)?)
                    .map_err(CodecError::malformed)?,
            }
        }
        9 => TextCurve2d::Offset {
            distance: cursor.finite_real("offset 2D curve distance")?,
            basis: NestedCurve2d::try_new(parse_curve2d(cursor, depth + 1, table_index)?)
                .map_err(CodecError::malformed)?,
        },
        other => {
            return Err(CodecError::NotImplemented(format!(
                "text B-rep 2D curve family {other} at table index {table_index}"
            )));
        }
    })
}

fn parse_bezier_curve2d(cursor: &mut TokenCursor<'_, '_, '_>) -> Result<NurbsCurve2d, CodecError> {
    let rational = cursor.boolean("2D Bezier rational flag")?;
    let degree = cursor.count("2D Bezier degree", 64)?;
    let pole_count = degree + 1;
    let mut control_points =
        collection_vec(cursor.ctx, pole_count, "FreeCAD B-rep parse_bezier_curve2d")?;
    let mut weights =
        optional_collection_vec(cursor.ctx, rational, pole_count, "FreeCAD B-rep weights")?;
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point2("2D Bezier pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("2D Bezier weight")?);
        }
    }
    Ok(NurbsCurve2d {
        degree: degree as u32,
        knots: clamped_bezier_knots(cursor.ctx, degree)?,
        control_points,
        weights,
        periodic: false,
    })
}

fn parse_nurbs_curve2d(cursor: &mut TokenCursor<'_, '_, '_>) -> Result<NurbsCurve2d, CodecError> {
    let rational = cursor.boolean("2D B-spline rational flag")?;
    let periodic = cursor.boolean("2D B-spline periodic flag")?;
    let degree = cursor.count("2D B-spline degree", 64)?;
    let pole_count = cursor.count("2D B-spline pole count", 1_000_000)?;
    let knot_count = cursor.count("2D B-spline knot count", 1_000_000)?;
    // Each pole consumes at least its two point2 tokens.
    let capacity = cursor.bounded(pole_count, 2, "2D B-spline pole")?;
    let mut control_points =
        collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_nurbs_curve2d")?;
    let mut weights =
        optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point2("2D B-spline pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("2D B-spline weight")?);
        }
    }
    let knots = parse_knots(cursor, knot_count, degree, "2D B-spline")?;
    let (knots, padding) = normalize_periodic_knots(cursor.ctx, knots, degree as u32, periodic)?;
    append_periodic_curve_poles(cursor.ctx, &mut control_points, weights.as_mut(), padding)?;
    Ok(NurbsCurve2d {
        degree: degree as u32,
        knots,
        control_points,
        weights,
        periodic,
    })
}

fn transform_power(transform: Transform, power: i64) -> Result<Transform, CodecError> {
    let mut base = if power < 0 {
        invert_affine(transform)?
    } else {
        transform
    };
    let mut exponent = power.unsigned_abs();
    let mut result = Transform::identity();
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result.compose(base).map_err(location_transform_error)?;
        }
        exponent >>= 1;
        if exponent > 0 {
            base = base.compose(base).map_err(location_transform_error)?;
        }
    }
    Ok(result)
}

/// Converts location arithmetic failure to the shape decoder error.
pub(crate) fn location_transform_error(error: cadmpeg_ir::transform::TransformError) -> CodecError {
    CodecError::malformed(format_args!("invalid location transform: {error}"))
}

fn invert_affine(transform: Transform) -> Result<Transform, CodecError> {
    transform
        .try_inverse_affine()
        .map_err(location_transform_error)
}

fn parse_polygons3d(
    ctx: &DecodeContext<'_>,
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextPolygon3d>, CodecError> {
    let mut cursor = section_cursor(ctx, tokens, "Polygon3D", "PolygonOnTriangulations")?;
    let count = section_counts.get("Polygon3D").copied().unwrap_or(0);
    // Each polygon consumes at least a node-count, flag, and deflection token.
    let mut polygons = collection_vec(
        cursor.ctx,
        cursor.bounded(count, 3, "text Polygon3D")?,
        "FreeCAD B-rep parse_polygons3d",
    )?;
    for _ in 0..count {
        let node_count = cursor.count("3D polygon node count", 1_000_000)?;
        let has_parameters = cursor.boolean("3D polygon parameter flag")?;
        let deflection = cursor.finite_real("3D polygon deflection")?;
        // Each node consumes its three point tokens.
        let mut nodes = collection_vec(
            cursor.ctx,
            cursor.bounded(node_count, 3, "3D polygon node")?,
            "FreeCAD B-rep parse_polygons3d",
        )?;
        for _ in 0..node_count {
            nodes.push(cursor.finite_point("3D polygon node")?);
        }
        let parameters = if has_parameters {
            // Each parameter consumes its one token.
            let mut parameters = collection_vec(
                cursor.ctx,
                cursor.bounded(node_count, 1, "3D polygon parameter")?,
                "FreeCAD B-rep parse_polygons3d",
            )?;
            for _ in 0..node_count {
                parameters.push(cursor.finite_real("3D polygon parameter")?);
            }
            Some(parameters)
        } else {
            None
        };
        polygons.push(TextPolygon3d {
            deflection: admit_polygon_deflection(deflection)?,
            nodes,
            parameters,
        });
    }
    ensure_section_consumed(&cursor, "Polygon3D")?;
    Ok(polygons)
}

fn parse_polygons_on_triangulations(
    ctx: &DecodeContext<'_>,
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextPolygonOnTriangulation>, CodecError> {
    let mut cursor = section_cursor(ctx, tokens, "PolygonOnTriangulations", "Surfaces")?;
    let count = section_counts
        .get("PolygonOnTriangulations")
        .copied()
        .unwrap_or(0);
    // Each polygon consumes at least a node-count, marker, deflection, and flag token.
    let mut polygons = collection_vec(
        cursor.ctx,
        cursor.bounded(count, 4, "text PolygonOnTriangulations")?,
        "FreeCAD B-rep parse_polygons_on_triangulations",
    )?;
    for _ in 0..count {
        let node_count = cursor.count("polygon-on-triangulation node count", 1_000_000)?;
        // Each node index consumes its one token.
        let mut nodes = collection_vec(
            cursor.ctx,
            cursor.bounded(node_count, 1, "polygon-on-triangulation node")?,
            "FreeCAD B-rep parse_polygons_on_triangulations",
        )?;
        for _ in 0..node_count {
            let node = cursor.count("polygon-on-triangulation node index", u32::MAX as usize)?;
            if node == 0 {
                return Err(CodecError::Malformed(
                    "polygon-on-triangulation node index is zero".into(),
                ));
            }
            nodes.push(node as u32);
        }
        if cursor.next("polygon-on-triangulation parameter marker")? != "p" {
            return Err(CodecError::Malformed(
                "polygon-on-triangulation has no parameter marker".into(),
            ));
        }
        let deflection = cursor.finite_real("polygon-on-triangulation deflection")?;
        let has_parameters = cursor.boolean("polygon-on-triangulation parameter flag")?;
        let parameters = if has_parameters {
            // Each parameter consumes its one token.
            let mut parameters = collection_vec(
                cursor.ctx,
                cursor.bounded(node_count, 1, "polygon-on-triangulation parameter")?,
                "FreeCAD B-rep parse_polygons_on_triangulations",
            )?;
            for _ in 0..node_count {
                parameters.push(cursor.finite_real("polygon-on-triangulation parameter")?);
            }
            Some(parameters)
        } else {
            None
        };
        polygons.push(TextPolygonOnTriangulation {
            nodes,
            deflection: admit_polygon_deflection(deflection)?,
            parameters,
        });
    }
    ensure_section_consumed(&cursor, "PolygonOnTriangulations")?;
    Ok(polygons)
}

fn parse_triangulations(
    ctx: &DecodeContext<'_>,
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<Vec<TextTriangulation>, CodecError> {
    let mut cursor = section_cursor(ctx, tokens, "Triangulations", "TShapes")?;
    let count = section_counts.get("Triangulations").copied().unwrap_or(0);
    // Each triangulation consumes at least two counts, a flag, and a deflection token.
    let mut triangulations = collection_vec(
        cursor.ctx,
        cursor.bounded(count, 4, "text Triangulations")?,
        "FreeCAD B-rep parse_triangulations",
    )?;
    for _ in 0..count {
        let node_count = cursor.count("triangulation node count", 1_000_000)?;
        let triangle_count = cursor.count("triangulation triangle count", 1_000_000)?;
        let has_uv = cursor.boolean("triangulation UV flag")?;
        let has_normals = topology_version >= 3 && cursor.boolean("triangulation normal flag")?;
        let deflection = cursor.finite_real("triangulation deflection")?;
        // Each node consumes its three point tokens.
        let mut nodes = collection_vec(
            cursor.ctx,
            cursor.bounded(node_count, 3, "triangulation node")?,
            "FreeCAD B-rep parse_triangulations",
        )?;
        for _ in 0..node_count {
            nodes.push(cursor.finite_point("triangulation node")?);
        }
        let uv_nodes = if has_uv {
            // Each UV node consumes its two point2 tokens.
            let mut uv_nodes = collection_vec(
                cursor.ctx,
                cursor.bounded(node_count, 2, "triangulation UV node")?,
                "FreeCAD B-rep parse_triangulations",
            )?;
            for _ in 0..node_count {
                uv_nodes.push(cursor.finite_point2("triangulation UV node")?);
            }
            Some(uv_nodes)
        } else {
            None
        };
        // Each triangle consumes its three index tokens.
        let mut triangles = collection_vec(
            cursor.ctx,
            cursor.bounded(triangle_count, 3, "triangulation triangle")?,
            "FreeCAD B-rep parse_triangulations",
        )?;
        for _ in 0..triangle_count {
            let mut triangle = [0_u32; 3];
            for node in &mut triangle {
                let index = cursor.count("triangulation node index", node_count)?;
                if index == 0 {
                    return Err(CodecError::Malformed(
                        "triangulation node index is zero".into(),
                    ));
                }
                *node = index as u32;
            }
            triangles.push(triangle);
        }
        let normals = if has_normals {
            // Each normal consumes its three vector tokens.
            let mut normals = collection_vec(
                cursor.ctx,
                cursor.bounded(node_count, 3, "triangulation normal")?,
                "FreeCAD B-rep parse_triangulations",
            )?;
            for _ in 0..node_count {
                normals.push(cursor.finite_vector("triangulation normal")?);
            }
            Some(normals)
        } else {
            None
        };
        triangulations.push(
            TextTriangulation::from_admitted_parts(deflection, nodes, uv_nodes, triangles, normals)
                .map_err(CodecError::Malformed)?,
        );
    }
    ensure_section_consumed(&cursor, "Triangulations")?;
    Ok(triangulations)
}

fn section_cursor<'a, 'c, 'r>(
    ctx: &'c DecodeContext<'r>,
    tokens: &'a [&'a str],
    section: &str,
    following: &str,
) -> Result<TokenCursor<'a, 'c, 'r>, CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == section)
        .ok_or_else(|| CodecError::malformed(format_args!("text B-rep has no {section} table")))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == following)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("text B-rep has no {following} table"))
        })?;
    Ok(TokenCursor::new(ctx, &tokens[start..end]))
}

fn ensure_section_consumed(
    cursor: &TokenCursor<'_, '_, '_>,
    section: &str,
) -> Result<(), CodecError> {
    if cursor.is_empty() {
        Ok(())
    } else {
        Err(CodecError::malformed(format_args!(
            "text B-rep {section} table contains trailing tokens"
        )))
    }
}

fn parse_tshapes(
    ctx: &DecodeContext<'_>,
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<(Vec<TextTShape>, Vec<TextShapeUse>), CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == "TShapes")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no TShapes table".into()))?
        + 2;
    let count = section_counts.get("TShapes").copied().unwrap_or(0);
    let mut cursor = TokenCursor::new(ctx, &tokens[start..]);
    // Each TShape consumes at least its one kind token.
    let mut shapes = collection_vec(
        cursor.ctx,
        cursor.bounded(count, 1, "text TShapes")?,
        "FreeCAD B-rep parse_tshapes",
    )?;
    for index in 1..=count {
        let token = cursor.next("TShape kind")?;
        let kind = parse_shape_kind(cursor.ctx, token)?;
        let geometry = parse_tshape_geometry(kind, &mut cursor, section_counts, topology_version)?;
        let flags_token = cursor.next("TShape flags")?;
        let flags = parse_shape_flags(cursor.ctx, flags_token, topology_version)?;
        let mut children = Vec::new();
        loop {
            if cursor.peek() == Some("*") {
                cursor.next("TShape child terminator")?;
                break;
            }
            let child = parse_shape_use(&mut cursor, count, section_counts)?;
            if child.shape >= index {
                return Err(CodecError::malformed(format_args!(
                    "TShape {index} references non-prior child {}",
                    child.shape
                )));
            }
            reserve_vec_items(cursor.ctx, &mut children, 1, "FreeCAD text shape children")?;
            children.push(child);
        }
        shapes.push(TextTShape {
            geometry,
            flags,
            children,
        });
    }
    let mut roots = Vec::new();
    while !cursor.is_empty() {
        if cursor.peek() == Some("*") {
            cursor.next("root shape terminator")?;
            break;
        }
        reserve_vec_items(cursor.ctx, &mut roots, 1, "FreeCAD text shape roots")?;
        roots.push(parse_shape_use(&mut cursor, count, section_counts)?);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep contains tokens after root shape terminator".into(),
        ));
    }
    Ok((shapes, roots))
}

fn parse_shape_kind(ctx: &DecodeContext<'_>, token: &str) -> Result<TextShapeKind, CodecError> {
    match token {
        "Ve" => Ok(TextShapeKind::Vertex),
        "Ed" => Ok(TextShapeKind::Edge),
        "Wi" => Ok(TextShapeKind::Wire),
        "Fa" => Ok(TextShapeKind::Face),
        "Sh" => Ok(TextShapeKind::Shell),
        "So" => Ok(TextShapeKind::Solid),
        "CS" => Ok(TextShapeKind::CompSolid),
        "Co" => Ok(TextShapeKind::Compound),
        _ => Err(CodecError::Malformed(retained_format(
            ctx,
            format_args!("invalid TShape kind {token:?}"),
            "FreeCAD invalid shape kind",
        )?)),
    }
}

fn parse_tshape_geometry(
    kind: TextShapeKind,
    cursor: &mut TokenCursor<'_, '_, '_>,
    counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<TextTShapeGeometry, CodecError> {
    match kind {
        TextShapeKind::Vertex => parse_vertex_geometry(cursor, counts),
        TextShapeKind::Edge => parse_edge_geometry(cursor, counts, topology_version),
        TextShapeKind::Face => parse_face_geometry(cursor, counts),
        TextShapeKind::Wire => Ok(TextTShapeGeometry::Wire),
        TextShapeKind::Shell => Ok(TextTShapeGeometry::Shell),
        TextShapeKind::Solid => Ok(TextTShapeGeometry::Solid),
        TextShapeKind::CompSolid => Ok(TextTShapeGeometry::CompSolid),
        TextShapeKind::Compound => Ok(TextTShapeGeometry::Compound),
    }
}

fn parse_vertex_geometry(
    cursor: &mut TokenCursor<'_, '_, '_>,
    counts: &BTreeMap<String, usize>,
) -> Result<TextTShapeGeometry, CodecError> {
    let tolerance = cursor.finite_real("vertex tolerance")?;
    let point = cursor.finite_point("vertex point")?;
    let mut representations = Vec::new();
    loop {
        let parameter = cursor.finite_real("vertex representation parameter")?;
        let kind = cursor.integer("vertex representation kind")?;
        if kind == 0 {
            break;
        }
        if representations.len() >= 1_000_000 {
            return Err(CodecError::Malformed(
                "vertex representation-count limit exceeded".into(),
            ));
        }
        let location_of = |cursor: &mut TokenCursor<'_, '_, '_>| {
            parse_reference(cursor, "vertex location", counts["Locations"], true)
        };
        let representation = match kind {
            1 => TextPointRepresentation::Curve3d {
                parameter,
                curve: parse_reference(cursor, "vertex curve", counts["Curves"], false)?,
                location: location_of(cursor)?,
            },
            2 => TextPointRepresentation::Pcurve {
                parameter,
                curve: parse_reference(
                    cursor,
                    "vertex parameter curve",
                    counts["Curve2ds"],
                    false,
                )?,
                surface: parse_reference(cursor, "vertex surface", counts["Surfaces"], false)?,
                location: location_of(cursor)?,
            },
            3 => TextPointRepresentation::Surface {
                parameter,
                second_parameter: cursor.finite_real("vertex second surface parameter")?,
                surface: parse_reference(cursor, "vertex surface", counts["Surfaces"], false)?,
                location: location_of(cursor)?,
            },
            other => {
                return Err(CodecError::malformed(format_args!(
                    "invalid vertex representation kind {other}"
                )));
            }
        };
        reserve_vec_items(
            cursor.ctx,
            &mut representations,
            1,
            "FreeCAD text vertex representations",
        )?;
        representations.push(representation);
    }
    Ok(TextTShapeGeometry::Vertex {
        tolerance,
        point,
        representations,
    })
}

fn parse_edge_geometry(
    cursor: &mut TokenCursor<'_, '_, '_>,
    counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<TextTShapeGeometry, CodecError> {
    let tolerance = cursor.finite_real("edge tolerance")?;
    let same_parameter = cursor.boolean("edge same-parameter flag")?;
    let same_range = cursor.boolean("edge same-range flag")?;
    let degenerated = cursor.boolean("edge degenerated flag")?;
    let mut representations = Vec::new();
    loop {
        let kind = cursor.integer("edge representation kind")?;
        if kind == 0 {
            break;
        }
        if representations.len() >= 1_000_000 {
            return Err(CodecError::Malformed(
                "edge representation-count limit exceeded".into(),
            ));
        }
        reserve_vec_items(
            cursor.ctx,
            &mut representations,
            1,
            "FreeCAD text edge representations",
        )?;
        representations.push(parse_edge_representation(
            kind,
            cursor,
            counts,
            topology_version,
        )?);
    }
    Ok(TextTShapeGeometry::Edge {
        tolerance,
        same_parameter,
        same_range,
        degenerated,
        representations,
    })
}

fn parse_edge_representation(
    kind: i64,
    cursor: &mut TokenCursor<'_, '_, '_>,
    counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<TextEdgeRepresentation, CodecError> {
    u8::try_from(kind)
        .map_err(|_| CodecError::Malformed("invalid edge representation kind".into()))?;
    match kind {
        1 => {
            let curve = parse_reference(cursor, "edge 3D curve", counts["Curves"], false)?;
            let location =
                parse_reference(cursor, "edge curve location", counts["Locations"], true)?;
            let parameter_range = parse_range(cursor, "edge curve")?;
            Ok(TextEdgeRepresentation::Curve3d {
                curve,
                location,
                parameter_range,
            })
        }
        2 | 3 => {
            let curve = parse_reference(cursor, "edge parameter curve", counts["Curve2ds"], false)?;
            let secondary = if kind == 3 {
                let (secondary, joined_continuity) = parse_reference_suffix(
                    cursor,
                    "edge secondary parameter curve",
                    counts["Curve2ds"],
                )?;
                let continuity = joined_continuity.map_or_else(
                    || {
                        let token = cursor.next("edge continuity")?;
                        retained_string(cursor.ctx, token, "FreeCAD B-rep edge continuity")
                    },
                    Ok,
                )?;
                Some((secondary, continuity))
            } else {
                None
            };
            let surface = parse_reference(cursor, "edge surface", counts["Surfaces"], false)?;
            let location =
                parse_reference(cursor, "edge surface location", counts["Locations"], true)?;
            let parameter_range = parse_range(cursor, "edge parameter curve")?;
            let uv_endpoints = if topology_version == 2 {
                Some([
                    cursor.finite_point2("edge first UV endpoint")?,
                    cursor.finite_point2("edge last UV endpoint")?,
                ])
            } else {
                None
            };
            Ok(if let Some((secondary, continuity)) = secondary {
                TextEdgeRepresentation::PcurvePair {
                    curves: [curve, secondary],
                    continuity,
                    surface,
                    location,
                    parameter_range,
                    uv_endpoints,
                }
            } else {
                TextEdgeRepresentation::Pcurve {
                    curve,
                    surface,
                    location,
                    parameter_range,
                    uv_endpoints,
                }
            })
        }
        4 => {
            let continuity_token = cursor.next("edge continuity")?;
            let continuity = retained_string(
                cursor.ctx,
                continuity_token,
                "FreeCAD B-rep edge continuity",
            )?;
            let first_surface =
                parse_reference(cursor, "edge regularity surface", counts["Surfaces"], false)?;
            let location = parse_reference(
                cursor,
                "edge regularity location",
                counts["Locations"],
                true,
            )?;
            let second_surface = parse_reference(
                cursor,
                "edge second regularity surface",
                counts["Surfaces"],
                false,
            )?;
            let second_location = parse_reference(
                cursor,
                "edge second regularity location",
                counts["Locations"],
                true,
            )?;
            Ok(TextEdgeRepresentation::Regularity {
                continuity,
                surfaces: [first_surface, second_surface],
                locations: [location, second_location],
            })
        }
        5 => {
            let polygon = parse_reference(cursor, "edge 3D polygon", counts["Polygon3D"], false)?;
            let location =
                parse_reference(cursor, "edge polygon location", counts["Locations"], true)?;
            Ok(TextEdgeRepresentation::Polygon3d { polygon, location })
        }
        6 | 7 => {
            let polygon = parse_reference(
                cursor,
                "edge polygon on triangulation",
                counts["PolygonOnTriangulations"],
                false,
            )?;
            let secondary = if kind == 7 {
                Some(parse_reference(
                    cursor,
                    "edge second polygon on triangulation",
                    counts["PolygonOnTriangulations"],
                    false,
                )?)
            } else {
                None
            };
            let triangulation = parse_reference(
                cursor,
                "edge triangulation",
                counts["Triangulations"],
                false,
            )?;
            let location = parse_reference(
                cursor,
                "edge triangulation location",
                counts["Locations"],
                true,
            )?;
            Ok(if let Some(secondary) = secondary {
                TextEdgeRepresentation::PolygonPair {
                    polygons: [polygon, secondary],
                    triangulation,
                    location,
                }
            } else {
                TextEdgeRepresentation::PolygonOnTriangulation {
                    polygon,
                    triangulation,
                    location,
                }
            })
        }
        other => Err(CodecError::malformed(format_args!(
            "invalid edge representation kind {other}"
        ))),
    }
}

fn parse_face_geometry(
    cursor: &mut TokenCursor<'_, '_, '_>,
    counts: &BTreeMap<String, usize>,
) -> Result<TextTShapeGeometry, CodecError> {
    let natural_restriction = cursor.boolean("face natural-restriction flag")?;
    let tolerance = cursor.finite_real("face tolerance")?;
    let surface = parse_reference(cursor, "face surface", counts["Surfaces"], true)?;
    let location = parse_reference(cursor, "face location", counts["Locations"], true)?;
    let triangulation = if cursor.peek() == Some("2") {
        cursor.next("face triangulation marker")?;
        Some(parse_reference(
            cursor,
            "face triangulation",
            counts["Triangulations"],
            false,
        )?)
    } else {
        None
    };
    Ok(TextTShapeGeometry::Face {
        natural_restriction,
        tolerance,
        surface: TableRef::optional(surface),
        location: location.into(),
        triangulation: triangulation
            .map(TableRef::new)
            .transpose()
            .map_err(CodecError::Malformed)?,
    })
}

fn parse_shape_flags(
    ctx: &DecodeContext<'_>,
    token: &str,
    topology_version: u8,
) -> Result<[bool; 7], CodecError> {
    if token.len() != 7 || !token.bytes().all(|byte| matches!(byte, b'0' | b'1')) {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!("invalid TShape flags {token:?}"),
            "FreeCAD TShape flag diagnostic",
        ));
    }
    let mut flags = [false; 7];
    for (index, byte) in token.bytes().enumerate() {
        flags[index] = byte == b'1';
    }
    if topology_version == 1 {
        flags[2] = false;
    }
    Ok(flags)
}

fn parse_shape_use(
    cursor: &mut TokenCursor<'_, '_, '_>,
    shape_count: usize,
    counts: &BTreeMap<String, usize>,
) -> Result<TextShapeUse, CodecError> {
    let token = cursor.next("shape use")?;
    let (orientation, encoded) = match token.as_bytes().first() {
        Some(b'+') => (TextOrientation::Forward, &token[1..]),
        Some(b'-') => (TextOrientation::Reversed, &token[1..]),
        Some(b'i') => (TextOrientation::Internal, &token[1..]),
        Some(b'e') => (TextOrientation::External, &token[1..]),
        _ => {
            return Err(CodecError::Malformed(retained_format(
                cursor.ctx,
                format_args!("invalid shape use {token:?}"),
                "FreeCAD invalid shape use",
            )?));
        }
    };
    let encoded = encoded.parse::<usize>().or_else(|_| {
        Err(CodecError::Malformed(retained_format(
            cursor.ctx,
            format_args!("invalid shape use {token:?}"),
            "FreeCAD invalid shape use",
        )?))
    })?;
    if encoded == 0 || encoded > shape_count {
        return Err(CodecError::malformed(format_args!(
            "shape use index {encoded} is out of range"
        )));
    }
    let shape = shape_count - encoded + 1;
    let location = parse_reference(cursor, "shape use location", counts["Locations"], true)?;
    Ok(TextShapeUse {
        shape,
        orientation,
        location: location.into(),
    })
}

fn parse_reference(
    cursor: &mut TokenCursor<'_, '_, '_>,
    label: &str,
    maximum: usize,
    allow_zero: bool,
) -> Result<usize, CodecError> {
    let value = cursor.count(label, maximum)?;
    if value == 0 && !allow_zero {
        return Err(CodecError::malformed(format_args!("{label} index is zero")));
    }
    Ok(value)
}

fn parse_reference_suffix(
    cursor: &mut TokenCursor<'_, '_, '_>,
    label: &str,
    maximum: usize,
) -> Result<(usize, Option<String>), CodecError> {
    let token = cursor.next(label)?;
    let split = token
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(token.len());
    let (reference, suffix) = token.split_at(split);
    let value = reference
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("invalid {label}")))?;
    if value == 0 || value > maximum {
        return Err(CodecError::malformed(format_args!(
            "{label} limit exceeded"
        )));
    }
    Ok((
        value,
        (!suffix.is_empty())
            .then(|| retained_string(cursor.ctx, suffix, "FreeCAD B-rep reference suffix"))
            .transpose()?,
    ))
}

fn parse_range(
    cursor: &mut TokenCursor<'_, '_, '_>,
    label: &str,
) -> Result<[FiniteReal; 2], CodecError> {
    let range = [
        cursor.finite_real(&format!("{label} first parameter"))?,
        cursor.finite_real(&format!("{label} last parameter"))?,
    ];
    if range[0] > range[1] {
        return Err(CodecError::malformed(format_args!(
            "{label} parameter range is reversed"
        )));
    }
    Ok(range)
}

fn parse_surface(
    cursor: &mut TokenCursor<'_, '_, '_>,
    depth: usize,
    table_index: usize,
) -> Result<TextSurface, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "text B-rep surface nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    let kind = cursor.integer("surface type")?;
    Ok(match kind {
        1 => parse_analytic_surface(AnalyticSurfaceKind::Plane, cursor)?,
        2 => parse_analytic_surface(AnalyticSurfaceKind::Cylinder, cursor)?,
        3 => parse_analytic_surface(AnalyticSurfaceKind::Cone, cursor)?,
        4 => parse_analytic_surface(AnalyticSurfaceKind::Sphere, cursor)?,
        5 => parse_analytic_surface(AnalyticSurfaceKind::Torus, cursor)?,
        6 => {
            let direction = cursor.finite_vector("extrusion direction")?;
            if direction.get().norm() == 0.0 {
                return Err(CodecError::Malformed("extrusion direction is zero".into()));
            }
            TextSurface::Extrusion {
                direction,
                directrix: NestedCurve::try_new(parse_curve(cursor, depth + 1, table_index)?)
                    .map_err(CodecError::malformed)?,
            }
        }
        7 => {
            let axis_origin = cursor.finite_point("revolution axis origin")?;
            let axis_direction = cursor.finite_vector("revolution axis direction")?;
            if axis_direction.get().norm() == 0.0 {
                return Err(CodecError::Malformed(
                    "revolution axis direction is zero".into(),
                ));
            }
            TextSurface::Revolution {
                axis_origin,
                axis_direction,
                directrix: NestedCurve::try_new(parse_curve(cursor, depth + 1, table_index)?)
                    .map_err(CodecError::malformed)?,
            }
        }
        8 => TextSurface::Nurbs(parse_bezier_surface(cursor)?),
        9 => TextSurface::Nurbs(parse_nurbs_surface(cursor)?),
        10 => {
            let u_range = [
                cursor.finite_real("trimmed surface first u parameter")?,
                cursor.finite_real("trimmed surface last u parameter")?,
            ];
            let v_range = [
                cursor.finite_real("trimmed surface first v parameter")?,
                cursor.finite_real("trimmed surface last v parameter")?,
            ];
            if u_range[0] > u_range[1] || v_range[0] > v_range[1] {
                return Err(CodecError::Malformed(
                    "trimmed surface parameter range is reversed".into(),
                ));
            }
            TextSurface::Trimmed {
                parameter_ranges: [u_range, v_range],
                basis: NestedSurface::try_new(parse_surface(cursor, depth + 1, table_index)?)
                    .map_err(CodecError::malformed)?,
            }
        }
        11 => TextSurface::Offset {
            distance: cursor.finite_real("offset surface distance")?,
            basis: NestedSurface::try_new(parse_surface(cursor, depth + 1, table_index)?)
                .map_err(CodecError::malformed)?,
        },
        other => {
            return Err(CodecError::NotImplemented(format!(
                "text B-rep surface family {other} at table index {table_index}"
            )));
        }
    })
}

#[derive(Clone, Copy)]
enum AnalyticSurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
}

fn parse_analytic_surface(
    kind: AnalyticSurfaceKind,
    cursor: &mut TokenCursor<'_, '_, '_>,
) -> Result<TextSurface, CodecError> {
    let origin = cursor.finite_point("surface origin")?;
    let axis = cursor.finite_vector("surface axis")?;
    let ref_direction = cursor.finite_vector("surface reference direction")?;
    let y_direction = cursor.finite_vector("surface y direction")?;
    let reversed = frame_v_reversed(axis.get(), ref_direction.get(), y_direction.get());
    Ok(match kind {
        AnalyticSurfaceKind::Plane => TextSurface::Plane {
            origin,
            axis,
            u_axis: ref_direction,
            v_reversed: reversed,
        },
        AnalyticSurfaceKind::Cylinder => TextSurface::Cylinder {
            origin,
            axis,
            ref_direction,
            radius: cursor.finite_real("cylinder radius")?,
            u_reversed: reversed,
        },
        AnalyticSurfaceKind::Cone => TextSurface::Cone {
            origin,
            axis,
            ref_direction,
            radius: cursor.finite_real("cone radius")?,
            half_angle: cursor.finite_real("cone half angle")?,
            u_reversed: reversed,
        },
        AnalyticSurfaceKind::Sphere => TextSurface::Sphere {
            center: origin,
            axis,
            ref_direction,
            radius: cursor.finite_real("sphere radius")?,
            u_reversed: reversed,
        },
        AnalyticSurfaceKind::Torus => TextSurface::Torus {
            center: origin,
            axis,
            ref_direction,
            major_radius: cursor.finite_real("torus major radius")?,
            minor_radius: cursor.finite_real("torus minor radius")?,
            u_reversed: reversed,
        },
    })
}

fn frame_v_reversed(axis: Vector3, x_axis: Vector3, y_axis: Vector3) -> bool {
    let expected_y = Vector3::new(
        axis.y.mul_add(x_axis.z, -axis.z * x_axis.y),
        axis.z.mul_add(x_axis.x, -axis.x * x_axis.z),
        axis.x.mul_add(x_axis.y, -axis.y * x_axis.x),
    );
    expected_y.x.mul_add(
        y_axis.x,
        expected_y.y.mul_add(y_axis.y, expected_y.z * y_axis.z),
    ) < 0.0
}

fn parse_nurbs_surface(cursor: &mut TokenCursor<'_, '_, '_>) -> Result<NurbsSurface, CodecError> {
    let u_rational = cursor.boolean("B-spline u rational flag")?;
    let v_rational = cursor.boolean("B-spline v rational flag")?;
    let rational = u_rational || v_rational;
    let u_periodic = cursor.boolean("B-spline u periodic flag")?;
    let v_periodic = cursor.boolean("B-spline v periodic flag")?;
    let u_degree = cursor.count("B-spline u degree", 64)?;
    let v_degree = cursor.count("B-spline v degree", 64)?;
    let u_count = cursor.count("B-spline u pole count", 1_000_000)?;
    let v_count = cursor.count("B-spline v pole count", 1_000_000)?;
    let u_knot_count = cursor.count("B-spline u knot count", 1_000_000)?;
    let v_knot_count = cursor.count("B-spline v knot count", 1_000_000)?;
    let pole_count = u_count
        .checked_mul(v_count)
        .filter(|count| *count <= 1_000_000)
        .ok_or_else(|| CodecError::Malformed("B-spline surface pole limit exceeded".into()))?;
    // Each pole consumes its three point tokens.
    let capacity = cursor.bounded(pole_count, 3, "B-spline surface pole")?;
    let mut control_points =
        collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_nurbs_surface")?;
    let mut weights =
        optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point("B-spline surface pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("B-spline surface weight")?);
        }
    }
    let u_knots = parse_knots(cursor, u_knot_count, u_degree, "B-spline u")?;
    let v_knots = parse_knots(cursor, v_knot_count, v_degree, "B-spline v")?;
    normalize_periodic_surface(
        cursor.ctx,
        [u_degree as u32, v_degree as u32],
        [u_knots, v_knots],
        [u_count, v_count],
        control_points,
        weights,
        [u_periodic, v_periodic],
    )
}

fn parse_bezier_surface(cursor: &mut TokenCursor<'_, '_, '_>) -> Result<NurbsSurface, CodecError> {
    let u_rational = cursor.boolean("Bezier u rational flag")?;
    let v_rational = cursor.boolean("Bezier v rational flag")?;
    let rational = u_rational || v_rational;
    let u_degree = cursor.count("Bezier u degree", 64)?;
    let v_degree = cursor.count("Bezier v degree", 64)?;
    let u_count = u_degree + 1;
    let v_count = v_degree + 1;
    let pole_count = u_count
        .checked_mul(v_count)
        .ok_or_else(|| CodecError::Malformed("Bezier surface pole count overflow".into()))?;
    let mut control_points =
        collection_vec(cursor.ctx, pole_count, "FreeCAD B-rep parse_bezier_surface")?;
    let mut weights =
        optional_collection_vec(cursor.ctx, rational, pole_count, "FreeCAD B-rep weights")?;
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point("Bezier surface pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("Bezier surface weight")?);
        }
    }
    NurbsSurface::from_finite_lanes(
        NurbsSurfaceAxis::new(
            u_degree as u32,
            clamped_bezier_knots(cursor.ctx, u_degree)?,
            false,
        ),
        NurbsSurfaceAxis::new(
            v_degree as u32,
            clamped_bezier_knots(cursor.ctx, v_degree)?,
            false,
        ),
        NurbsSurfaceLanes::new(
            grid_rows(cursor.ctx, control_points, v_count)?,
            weights
                .map(|values| grid_rows(cursor.ctx, values, v_count))
                .transpose()?,
        ),
        false,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn parse_knots(
    cursor: &mut TokenCursor<'_, '_, '_>,
    knot_count: usize,
    degree: usize,
    label: &str,
) -> Result<Vec<FiniteReal>, CodecError> {
    let mut knots = Vec::new();
    for _ in 0..knot_count {
        let knot = cursor.finite_real(&format!("{label} knot"))?;
        let multiplicity = cursor.count(&format!("{label} knot multiplicity"), degree + 1)?;
        let expanded = knots
            .len()
            .checked_add(multiplicity)
            .filter(|count| *count <= 2_000_000)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("expanded {label} knot limit exceeded"))
            })?;
        reserve_vec_items(
            cursor.ctx,
            &mut knots,
            multiplicity,
            "FreeCAD text expanded knots",
        )?;
        knots.resize(expanded, knot);
    }
    Ok(knots)
}

fn normalize_periodic_knots(
    ctx: &DecodeContext<'_>,
    knots: Vec<FiniteReal>,
    degree: u32,
    periodic: bool,
) -> Result<(Vec<FiniteReal>, usize), CodecError> {
    if !periodic {
        return Ok((knots, 0));
    }
    let degree = usize::try_from(degree)
        .map_err(|_| CodecError::Malformed("periodic B-spline degree exceeds usize".into()))?;
    let Some(&first) = knots.first() else {
        return Err(CodecError::Malformed(
            "periodic B-spline has no knots".into(),
        ));
    };
    let Some(&last) = knots.last() else {
        return Err(CodecError::Malformed(
            "periodic B-spline has no knots".into(),
        ));
    };
    let first_multiplicity = knots.iter().take_while(|knot| **knot == first).count();
    let last_multiplicity = knots.iter().rev().take_while(|knot| **knot == last).count();
    if first_multiplicity == 0
        || first_multiplicity > degree
        || last_multiplicity != first_multiplicity
        || first >= last
    {
        return Err(CodecError::Malformed(
            "periodic B-spline endpoint knots are invalid".into(),
        ));
    }
    let padding = degree + 1 - first_multiplicity;
    let before_last = knots.len().checked_sub(last_multiplicity).ok_or_else(|| {
        CodecError::Malformed("periodic B-spline knot cardinality underflow".into())
    })?;
    if before_last < padding || knots.len() - first_multiplicity < padding {
        return Err(CodecError::Malformed(
            "periodic B-spline has insufficient interior knots".into(),
        ));
    }
    let normalized_count = knots
        .len()
        .checked_add(2 * padding)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline knot limit exceeded".into()))?;
    let mut normalized = collection_vec(ctx, normalized_count, "FreeCAD periodic B-rep knots")?;
    let overflow =
        || CodecError::Malformed("periodic B-spline extension exceeds finite knot range".into());
    for knot in &knots[before_last - padding..before_last] {
        normalized
            .push(FiniteReal::new(first.get() - (last.get() - knot.get())).ok_or_else(overflow)?);
    }
    normalized.extend_from_slice(&knots);
    for knot in &knots[first_multiplicity..first_multiplicity + padding] {
        normalized
            .push(FiniteReal::new(last.get() + (knot.get() - first.get())).ok_or_else(overflow)?);
    }
    Ok((normalized, padding))
}

fn append_periodic_curve_poles<T: Clone>(
    ctx: &DecodeContext<'_>,
    control_points: &mut Vec<T>,
    weights: Option<&mut Vec<FiniteReal>>,
    padding: usize,
) -> Result<(), CodecError> {
    if padding == 0 {
        return Ok(());
    }
    if control_points.len() < padding {
        return Err(CodecError::Malformed(
            "periodic B-spline has insufficient poles".into(),
        ));
    }
    reserve_vec_items(
        ctx,
        control_points,
        padding,
        "FreeCAD periodic B-rep curve poles",
    )?;
    control_points.extend_from_within(..padding);
    if let Some(weights) = weights {
        if weights.len() < padding {
            return Err(CodecError::Malformed(
                "periodic B-spline has insufficient weights".into(),
            ));
        }
        reserve_vec_items(
            ctx,
            weights,
            padding,
            "FreeCAD periodic B-rep curve weights",
        )?;
        weights.extend_from_within(..padding);
    }
    Ok(())
}

fn normalize_periodic_surface(
    ctx: &DecodeContext<'_>,
    degrees: [u32; 2],
    knots: [Vec<FiniteReal>; 2],
    counts: [usize; 2],
    control_points: Vec<FinitePoint3>,
    weights: Option<Vec<FiniteReal>>,
    periodic: [bool; 2],
) -> Result<NurbsSurface, CodecError> {
    let [u_source_knots, v_source_knots] = knots;
    let (u_knots, u_padding) =
        normalize_periodic_knots(ctx, u_source_knots, degrees[0], periodic[0])?;
    let (v_knots, v_padding) =
        normalize_periodic_knots(ctx, v_source_knots, degrees[1], periodic[1])?;
    let [old_u, old_v] = counts;
    let source_count = checked_grid_count(old_u, old_v, "B-spline")?;
    if control_points.len() != source_count
        || weights
            .as_ref()
            .is_some_and(|values| values.len() != source_count)
    {
        return Err(CodecError::Malformed(
            "B-spline pole grid cardinality mismatch".into(),
        ));
    }
    if old_u == 0 || old_v == 0 {
        return Err(CodecError::Malformed(
            "periodic B-spline pole grid is empty".into(),
        ));
    }
    let new_u = old_u
        .checked_add(u_padding)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline u pole limit exceeded".into()))?;
    let new_v = old_v
        .checked_add(v_padding)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline v pole limit exceeded".into()))?;
    let new_count = new_u
        .checked_mul(new_v)
        .filter(|count| *count <= 2_000_000)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline pole limit exceeded".into()))?;
    let (control_points, weights) = if u_padding != 0 || v_padding != 0 {
        let old_points = &control_points;
        let old_weights = weights.as_deref();
        let mut points = collection_vec(ctx, new_count, "FreeCAD periodic B-rep surface poles")?;
        let mut weights = optional_collection_vec(
            ctx,
            old_weights.is_some(),
            new_count,
            "FreeCAD periodic B-rep surface weights",
        )?;
        for u in 0..new_u {
            for v in 0..new_v {
                let source = (u % old_u) * old_v + v % old_v;
                points.push(old_points[source]);
                if let (Some(source_weights), Some(target_weights)) = (old_weights, &mut weights) {
                    target_weights.push(source_weights[source]);
                }
            }
        }
        (points, weights)
    } else {
        (control_points, weights)
    };
    let v_count = u32::try_from(new_v)
        .map_err(|_| CodecError::Malformed("periodic B-spline v pole count exceeds u32".into()))?;
    NurbsSurface::from_finite_lanes(
        NurbsSurfaceAxis::new(degrees[0], u_knots, periodic[0]),
        NurbsSurfaceAxis::new(degrees[1], v_knots, periodic[1]),
        NurbsSurfaceLanes::new(
            grid_rows(ctx, control_points, v_count as usize)?,
            weights
                .map(|values| grid_rows(ctx, values, v_count as usize))
                .transpose()?,
        ),
        false,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn parse_curve(
    cursor: &mut TokenCursor<'_, '_, '_>,
    depth: usize,
    table_index: usize,
) -> Result<TextCurve, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "text B-rep 3D curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    let kind = cursor.integer("curve type")?;
    Ok(match kind {
        1 => TextCurve::Line {
            origin: cursor.finite_point("line origin")?,
            direction: cursor.finite_vector("line direction")?,
        },
        2 => {
            let center = cursor.finite_point("circle center")?;
            let axis = cursor.finite_vector("circle axis")?;
            let ref_direction = cursor.finite_vector("circle reference direction")?;
            cursor.finite_vector("circle y direction")?;
            TextCurve::Circle {
                center,
                axis,
                ref_direction,
                radius: cursor.finite_real("circle radius")?,
            }
        }
        3 => {
            let center = cursor.finite_point("ellipse center")?;
            let axis = cursor.finite_vector("ellipse axis")?;
            let major_direction = cursor.finite_vector("ellipse major direction")?;
            cursor.finite_vector("ellipse y direction")?;
            TextCurve::Ellipse {
                center,
                axis,
                major_direction,
                major_radius: cursor.finite_real("ellipse major radius")?,
                minor_radius: cursor.finite_real("ellipse minor radius")?,
            }
        }
        4 => {
            let vertex = cursor.finite_point("parabola vertex")?;
            let axis = cursor.finite_vector("parabola axis")?;
            let major_direction = cursor.finite_vector("parabola major direction")?;
            cursor.finite_vector("parabola y direction")?;
            TextCurve::Parabola {
                vertex,
                axis,
                major_direction,
                focal_distance: cursor.finite_real("parabola focal distance")?,
            }
        }
        5 => {
            let center = cursor.finite_point("hyperbola center")?;
            let axis = cursor.finite_vector("hyperbola axis")?;
            let major_direction = cursor.finite_vector("hyperbola major direction")?;
            cursor.finite_vector("hyperbola y direction")?;
            TextCurve::Hyperbola {
                center,
                axis,
                major_direction,
                major_radius: cursor.finite_real("hyperbola major radius")?,
                minor_radius: cursor.finite_real("hyperbola minor radius")?,
            }
        }
        6 => TextCurve::Nurbs(parse_bezier_curve(cursor)?),
        7 => TextCurve::Nurbs(parse_nurbs_curve(cursor)?),
        8 => {
            let first = cursor.finite_real("trimmed curve first parameter")?;
            let last = cursor.finite_real("trimmed curve last parameter")?;
            if first > last {
                return Err(CodecError::Malformed(
                    "trimmed curve parameter range is reversed".into(),
                ));
            }
            TextCurve::Trimmed {
                parameter_range: [first, last],
                basis: NestedCurve::try_new(parse_curve(cursor, depth + 1, table_index)?)
                    .map_err(CodecError::malformed)?,
            }
        }
        9 => {
            let distance = cursor.finite_real("offset curve distance")?;
            let direction = cursor.finite_vector("offset curve direction")?;
            if direction.get().norm() == 0.0 {
                return Err(CodecError::Malformed(
                    "offset curve direction is zero".into(),
                ));
            }
            TextCurve::Offset {
                distance,
                direction,
                basis: NestedCurve::try_new(parse_curve(cursor, depth + 1, table_index)?)
                    .map_err(CodecError::malformed)?,
            }
        }
        other => {
            return Err(CodecError::NotImplemented(format!(
                "text B-rep 3D curve family {other} at table index {table_index}"
            )));
        }
    })
}

fn parse_nurbs_curve(cursor: &mut TokenCursor<'_, '_, '_>) -> Result<NurbsCurve, CodecError> {
    let rational = cursor.boolean("B-spline rational flag")?;
    let periodic = cursor.boolean("B-spline periodic flag")?;
    let degree = cursor.count("B-spline degree", 64)?;
    let pole_count = cursor.count("B-spline pole count", 1_000_000)?;
    let knot_count = cursor.count("B-spline knot count", 1_000_000)?;
    // Each pole consumes its three point tokens.
    let capacity = cursor.bounded(pole_count, 3, "B-spline pole")?;
    let mut control_points =
        collection_vec(cursor.ctx, capacity, "FreeCAD B-rep parse_nurbs_curve")?;
    let mut weights =
        optional_collection_vec(cursor.ctx, rational, capacity, "FreeCAD B-rep weights")?;
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point("B-spline pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("B-spline weight")?);
        }
    }
    let knots = parse_knots(cursor, knot_count, degree, "B-spline")?;
    let (knots, padding) = normalize_periodic_knots(cursor.ctx, knots, degree as u32, periodic)?;
    append_periodic_curve_poles(cursor.ctx, &mut control_points, weights.as_mut(), padding)?;
    NurbsCurve::from_finite_lanes(degree as u32, knots, control_points, weights, periodic)
        .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn parse_bezier_curve(cursor: &mut TokenCursor<'_, '_, '_>) -> Result<NurbsCurve, CodecError> {
    let rational = cursor.boolean("Bezier rational flag")?;
    let degree = cursor.count("Bezier degree", 64)?;
    let pole_count = degree + 1;
    let mut control_points =
        collection_vec(cursor.ctx, pole_count, "FreeCAD B-rep parse_bezier_curve")?;
    let mut weights =
        optional_collection_vec(cursor.ctx, rational, pole_count, "FreeCAD B-rep weights")?;
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point("Bezier pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("Bezier weight")?);
        }
    }
    NurbsCurve::from_finite_lanes(
        degree as u32,
        clamped_bezier_knots(cursor.ctx, degree)?,
        control_points,
        weights,
        false,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn clamped_bezier_knots(
    ctx: &DecodeContext<'_>,
    degree: usize,
) -> Result<Vec<FiniteReal>, CodecError> {
    let half = degree.checked_add(1).ok_or_else(|| {
        crate::resource::collection_allocation_failed(ctx, u64::MAX, "FreeCAD Bezier knots")
    })?;
    let count = half.checked_mul(2).ok_or_else(|| {
        crate::resource::collection_allocation_failed(ctx, u64::MAX, "FreeCAD Bezier knots")
    })?;
    let mut knots = collection_vec(ctx, count, "FreeCAD Bezier knots")?;
    knots.extend(std::iter::repeat_n(FiniteReal::ZERO, half));
    knots.extend(std::iter::repeat_n(FiniteReal::ONE, half));
    Ok(knots)
}

fn grid_rows<T>(
    ctx: &DecodeContext<'_>,
    values: Vec<T>,
    width: usize,
) -> Result<Vec<Vec<T>>, CodecError> {
    if width == 0 || !values.len().is_multiple_of(width) {
        return Err(CodecError::malformed(
            "surface grid dimensions do not match pole count",
        ));
    }
    let mut rows = collection_vec(ctx, values.len() / width, "FreeCAD B-rep surface rows")?;
    let mut values = values.into_iter();
    while values.len() != 0 {
        let mut row = collection_vec(ctx, width, "FreeCAD B-rep surface row values")?;
        row.extend(values.by_ref().take(width));
        rows.push(row);
    }
    Ok(rows)
}

struct TokenCursor<'a, 'c, 'r> {
    tokens: &'a [&'a str],
    index: usize,
    ctx: &'c DecodeContext<'r>,
}

impl<'a, 'c, 'r> TokenCursor<'a, 'c, 'r> {
    fn new(ctx: &'c DecodeContext<'r>, tokens: &'a [&'a str]) -> Self {
        Self {
            tokens,
            index: 0,
            ctx,
        }
    }

    fn is_empty(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn remaining(&self) -> usize {
        self.tokens.get(self.index..).map_or(0, <[_]>::len)
    }

    /// Clamps a declared element count to the unread token count.
    ///
    /// `element_size` is the minimum tokens one element consumes; a count that
    /// could not fit in the remaining tokens is rejected.
    fn bounded(&self, count: usize, element_size: usize, label: &str) -> Result<usize, CodecError> {
        bounded_len(count as u64, element_size, self.remaining()).ok_or_else(|| {
            CodecError::malformed(format_args!("{label} count exceeds available tokens"))
        })
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.index).copied()
    }

    fn integer(&mut self, label: &str) -> Result<i64, CodecError> {
        self.next(label)?.parse().map_err(|_| {
            CodecError::malformed(format_args!("invalid {label} in text B-rep Curves table"))
        })
    }

    fn count(&mut self, label: &str, maximum: usize) -> Result<usize, CodecError> {
        let value = self.integer(label)?;
        let value = usize::try_from(value)
            .map_err(|_| CodecError::malformed(format_args!("negative {label}")))?;
        if value > maximum {
            return Err(CodecError::malformed(format_args!(
                "{label} limit exceeded"
            )));
        }
        Ok(value)
    }

    fn boolean(&mut self, label: &str) -> Result<bool, CodecError> {
        match self.integer(label)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::malformed(format_args!("invalid {label}"))),
        }
    }

    fn finite_real(&mut self, label: &str) -> Result<FiniteReal, CodecError> {
        let value = self.next(label)?.parse::<f64>().map_err(|_| {
            CodecError::malformed(format_args!("invalid {label} in text B-rep Curves table"))
        })?;
        FiniteReal::new(value).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "non-finite {label} in text B-rep Curves table"
            ))
        })
    }

    fn finite_point(&mut self, label: &str) -> Result<FinitePoint3, CodecError> {
        Ok(FinitePoint3::from_coordinates(
            self.finite_real(label)?,
            self.finite_real(label)?,
            self.finite_real(label)?,
        ))
    }

    fn finite_point2(&mut self, label: &str) -> Result<FinitePoint2, CodecError> {
        Ok(FinitePoint2::from_coordinates(
            self.finite_real(label)?,
            self.finite_real(label)?,
        ))
    }

    fn finite_vector(&mut self, label: &str) -> Result<FiniteVector3, CodecError> {
        Ok(FiniteVector3::from_components(
            self.finite_real(label)?,
            self.finite_real(label)?,
            self.finite_real(label)?,
        ))
    }

    fn next(&mut self, label: &str) -> Result<&'a str, CodecError> {
        let token = self.tokens.get(self.index).copied().ok_or_else(|| {
            CodecError::malformed(format_args!("truncated {label} in text B-rep Curves table"))
        })?;
        self.index += 1;
        Ok(token)
    }
}

#[derive(Default)]
pub(crate) struct CurveTransfer {
    pub(crate) curves: Vec<Curve>,
    pub(crate) procedural: Vec<(CurveId, ProceduralCurve)>,
}

pub(crate) fn clone_nurbs_curve(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsCurve,
) -> Result<NurbsCurve, CodecError> {
    let count = nurbs
        .knots()
        .len()
        .checked_add(nurbs.pole_count())
        .ok_or_else(|| {
            crate::resource::collection_allocation_failed(ctx, u64::MAX, "FreeCAD NURBS curve copy")
        })?;
    ctx.charge_collection_items(count as u64, "FreeCAD NURBS curve copy")?;
    nurbs.try_clone().map_err(|_| {
        crate::resource::collection_allocation_failed(ctx, count as u64, "FreeCAD NURBS curve copy")
    })
}

pub(crate) fn clone_nurbs_surface(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
) -> Result<NurbsSurface, CodecError> {
    let count = nurbs
        .u_count()
        .checked_mul(nurbs.v_count())
        .and_then(|count| count.checked_add(nurbs.u_count()))
        .and_then(|count| count.checked_add(nurbs.u_knots().len()))
        .and_then(|count| count.checked_add(nurbs.v_knots().len()))
        .ok_or_else(|| {
            crate::resource::collection_allocation_failed(
                ctx,
                u64::MAX,
                "FreeCAD NURBS surface copy",
            )
        })?;
    ctx.charge_collection_items(count as u64, "FreeCAD NURBS surface copy")?;
    nurbs.try_clone().map_err(|_| {
        crate::resource::collection_allocation_failed(
            ctx,
            count as u64,
            "FreeCAD NURBS surface copy",
        )
    })
}

fn clone_curve_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &CurveGeometry,
) -> Result<CurveGeometry, CodecError> {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => Ok(CurveGeometry::Solved(
            SolvedCurveGeometry::Nurbs(clone_nurbs_curve(ctx, nurbs)?),
        )),
        _ => Ok(geometry.clone()),
    }
}

fn clone_surface_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &SurfaceGeometry,
) -> Result<SurfaceGeometry, CodecError> {
    match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
            Ok(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                clone_nurbs_surface(ctx, nurbs)?,
            )))
        }
        _ => Ok(geometry.clone()),
    }
}

pub(crate) fn clone_source_association(
    ctx: &DecodeContext<'_>,
    source: &SourceObjectAssociation,
) -> Result<SourceObjectAssociation, CodecError> {
    Ok(SourceObjectAssociation {
        format: source.format,
        object_id: cadmpeg_core::text::NonBlankString::new(retained_string(
            ctx,
            source.object_id.as_str(),
            "FreeCAD geometry source association",
        )?)
        .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))?,
        name: source
            .name
            .as_deref()
            .map(|name| retained_string(ctx, name, "FreeCAD geometry source name"))
            .transpose()?,
        color: source.color,
        visible: source.visible,
        layer: source
            .layer
            .as_deref()
            .map(|layer| retained_string(ctx, layer, "FreeCAD geometry source layer"))
            .transpose()?,
        instance_path: retained_strings(
            ctx,
            &source.instance_path,
            "FreeCAD geometry source instance path",
        )?,
    })
}

fn model_identity<I>(
    ctx: &DecodeContext<'_>,
    kind: &str,
    parent: &str,
    child: &str,
    operation: &'static str,
) -> Result<I, CodecError>
where
    I: TryFrom<String>,
    I::Error: std::fmt::Display,
{
    I::try_from(native::model_id_charged_at(
        ctx, kind, parent, child, operation,
    )?)
    .map_err(CodecError::malformed)
}

pub(crate) fn transfer_text_curves(
    ctx: &DecodeContext<'_>,
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
) -> Result<CurveTransfer, CodecError> {
    let mut transfer = CurveTransfer::default();
    for payload in payloads {
        let Some(curves) = payload.payload.shape_set().map(|set| &set.curves) else {
            continue;
        };
        let object_id = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map_or(payload.property.as_str(), |property| {
                property.owner.as_str()
            });
        let association = SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Fcstd,
            object_id: cadmpeg_core::text::NonBlankString::new(retained_string(
                ctx,
                object_id,
                "FreeCAD curve source object",
            )?)
            .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))?,
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        for (index, curve) in curves.iter().enumerate() {
            let id: CurveId = model_identity(
                ctx,
                "curve",
                &payload.id,
                &(index + 1).to_string(),
                "FreeCAD transferred curve identity",
            )?;
            append_text_curve(ctx, curve, id, &association, &mut transfer)?;
        }
    }
    Ok(transfer)
}

fn append_text_curve(
    ctx: &DecodeContext<'_>,
    curve: &TextCurve,
    id: CurveId,
    association: &SourceObjectAssociation,
    transfer: &mut CurveTransfer,
) -> Result<CurveGeometry, CodecError> {
    let _depth = ctx.enter_nested("FreeCAD curve transfer nesting")?;
    let geometry = match curve {
        TextCurve::Line { origin, direction } => {
            let direction = UnitVector3::new(direction.get()).ok_or_else(|| {
                CodecError::malformed("LineCurve.direction must have unit length")
            })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(*origin, direction),
            ))
        }
        TextCurve::Circle {
            center,
            axis: _,
            ref_direction: _,
            radius,
        } if radius.get() == 0.0 => CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
            cadmpeg_ir::geometry::analytic::DegenerateCurve::new(*center),
        )),
        TextCurve::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let frame =
                OrthonormalFrame3::new(axis.get(), ref_direction.get()).ok_or_else(|| {
                    CodecError::malformed(
                        "CircleCurve.axis/ref_direction must form an orthonormal frame",
                    )
                })?;
            let radius = PositiveLength::from_assigned_real(*radius).ok_or_else(|| {
                CodecError::malformed("CircleCurve.radius must be positive and finite")
            })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(*center, frame, radius),
            ))
        }
        TextCurve::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let frame =
                OrthonormalFrame3::new(axis.get(), major_direction.get()).ok_or_else(|| {
                    CodecError::malformed(
                        "EllipseCurve.axis/major_direction must form an orthonormal frame",
                    )
                })?;
            let major_radius =
                PositiveLength::from_assigned_real(*major_radius).ok_or_else(|| {
                    CodecError::malformed("EllipseCurve.major_radius must be positive and finite")
                })?;
            let minor_radius =
                PositiveLength::from_assigned_real(*minor_radius).ok_or_else(|| {
                    CodecError::malformed("EllipseCurve.minor_radius must be positive and finite")
                })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                    *center,
                    frame,
                    major_radius,
                    minor_radius,
                )
                .map_err(CodecError::malformed)?,
            ))
        }
        TextCurve::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            let frame =
                OrthonormalFrame3::new(axis.get(), major_direction.get()).ok_or_else(|| {
                    CodecError::malformed(
                        "ParabolaCurve.axis/major_direction must form an orthonormal frame",
                    )
                })?;
            let focal_distance =
                PositiveLength::from_assigned_real(*focal_distance).ok_or_else(|| {
                    CodecError::malformed(
                        "ParabolaCurve.focal_distance must be positive and finite",
                    )
                })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
                cadmpeg_ir::geometry::analytic::ParabolaCurve::new(*vertex, frame, focal_distance),
            ))
        }
        TextCurve::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let frame =
                OrthonormalFrame3::new(axis.get(), major_direction.get()).ok_or_else(|| {
                    CodecError::malformed(
                        "HyperbolaCurve.axis/major_direction must form an orthonormal frame",
                    )
                })?;
            let major_radius =
                PositiveLength::from_assigned_real(*major_radius).ok_or_else(|| {
                    CodecError::malformed("HyperbolaCurve.major_radius must be positive and finite")
                })?;
            let minor_radius =
                PositiveLength::from_assigned_real(*minor_radius).ok_or_else(|| {
                    CodecError::malformed("HyperbolaCurve.minor_radius must be positive and finite")
                })?;
            CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
                cadmpeg_ir::geometry::analytic::HyperbolaCurve::new(
                    *center,
                    frame,
                    major_radius,
                    minor_radius,
                ),
            ))
        }
        TextCurve::Nurbs(nurbs) => {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(clone_nurbs_curve(ctx, nurbs)?))
        }
        TextCurve::Trimmed {
            parameter_range,
            basis,
        } => {
            let basis_id: CurveId = model_identity(
                ctx,
                "curve",
                id.as_str(),
                "basis",
                "FreeCAD curve basis identity",
            )?;
            let basis_geometry = append_text_curve(
                ctx,
                basis.curve(),
                crate::resource::copied_identity(
                    ctx,
                    basis_id.as_str(),
                    "FreeCAD curve basis identity copy",
                )?,
                association,
                transfer,
            )?;
            let parameter_range = crate::topology_transfer::normalize_occt_curve_range(
                basis_geometry.solved().ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(
                        "carrier has no solved geometry".into(),
                    )
                })?,
                Some(*parameter_range),
            )
            .unwrap_or(*parameter_range);
            reserve_vec_items(
                ctx,
                &mut transfer.procedural,
                1,
                "FreeCAD procedural curves",
            )?;
            let procedural_id: CurveId = crate::resource::copied_identity(
                ctx,
                id.as_str(),
                "FreeCAD procedural curve identity copy",
            )?;
            let admitted_payload =
                cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction::from_finite_parts(
                    basis_id,
                    parameter_range,
                    true,
                    None,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?;
            let construction_id: ProceduralCurveId = model_identity(
                ctx,
                "curve",
                id.as_str(),
                "construction",
                "FreeCAD curve construction identity",
            )?;
            transfer.procedural.push((
                procedural_id,
                ProceduralCurve::new(
                    construction_id,
                    ProceduralCurveDefinition::Subset(admitted_payload),
                ),
            ));
            basis_geometry
        }
        TextCurve::Offset {
            distance,
            direction,
            basis,
        } => {
            let basis_id: CurveId = model_identity(
                ctx,
                "curve",
                id.as_str(),
                "basis",
                "FreeCAD curve basis identity",
            )?;
            append_text_curve(
                ctx,
                basis.curve(),
                crate::resource::copied_identity(
                    ctx,
                    basis_id.as_str(),
                    "FreeCAD curve basis identity copy",
                )?,
                association,
                transfer,
            )?;
            reserve_vec_items(
                ctx,
                &mut transfer.procedural,
                1,
                "FreeCAD procedural curves",
            )?;
            let procedural_id: CurveId = crate::resource::copied_identity(
                ctx,
                id.as_str(),
                "FreeCAD procedural curve identity copy",
            )?;
            let admitted_payload = cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::from_admitted_direction(
                basis_id, *distance, *direction,
            ).map_err(cadmpeg_core::CodecError::malformed)?;
            let construction_id: ProceduralCurveId = model_identity(
                ctx,
                "curve",
                id.as_str(),
                "construction",
                "FreeCAD curve construction identity",
            )?;
            transfer.procedural.push((
                procedural_id,
                ProceduralCurve::new(
                    construction_id,
                    ProceduralCurveDefinition::Offset(admitted_payload),
                ),
            ));
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None })
        }
    };
    reserve_vec_items(ctx, &mut transfer.curves, 1, "FreeCAD transferred curves")?;
    transfer.curves.push(Curve {
        id,
        geometry: clone_curve_geometry(ctx, &geometry)?,
        source_object: Some(clone_source_association(ctx, association)?),
    });
    Ok(geometry)
}

#[derive(Default)]
pub(crate) struct SurfaceTransfer {
    pub(crate) surfaces: Vec<Surface>,
    pub(crate) procedural: Vec<(SurfaceId, ProceduralSurface)>,
}

pub(crate) fn transfer_text_surfaces(
    ctx: &DecodeContext<'_>,
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
    curve_transfer: &mut CurveTransfer,
) -> Result<SurfaceTransfer, CodecError> {
    let mut transfer = SurfaceTransfer::default();
    for payload in payloads {
        let Some(surfaces) = payload.payload.shape_set().map(|set| &set.surfaces) else {
            continue;
        };
        let object_id = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map_or(payload.property.as_str(), |property| {
                property.owner.as_str()
            });
        let association = SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Fcstd,
            object_id: cadmpeg_core::text::NonBlankString::new(retained_string(
                ctx,
                object_id,
                "FreeCAD surface source object",
            )?)
            .ok_or_else(|| CodecError::malformed("source object_id must not be empty"))?,
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        for (index, surface) in surfaces.iter().enumerate() {
            let id: SurfaceId = model_identity(
                ctx,
                "surface",
                &payload.id,
                &(index + 1).to_string(),
                "FreeCAD transferred surface identity",
            )?;
            append_text_surface(
                ctx,
                surface,
                id,
                &association,
                curve_transfer,
                &mut transfer,
            )?;
        }
    }
    Ok(transfer)
}

fn append_text_surface(
    ctx: &DecodeContext<'_>,
    surface: &TextSurface,
    id: SurfaceId,
    association: &SourceObjectAssociation,
    curve_transfer: &mut CurveTransfer,
    transfer: &mut SurfaceTransfer,
) -> Result<SurfaceGeometry, CodecError> {
    let _depth = ctx.enter_nested("FreeCAD surface transfer nesting")?;
    let geometry = match surface {
        TextSurface::Plane {
            origin,
            axis,
            u_axis,
            ..
        } => {
            let frame = OrthonormalFrame3::new(axis.get(), u_axis.get()).ok_or_else(|| {
                CodecError::malformed("PlaneSurface.normal/u_axis must form an orthonormal frame")
            })?;
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::new(*origin, frame),
            ))
        }
        TextSurface::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
            ..
        } => {
            let frame =
                OrthonormalFrame3::new(axis.get(), ref_direction.get()).ok_or_else(|| {
                    CodecError::malformed(
                        "CylinderSurface.axis/ref_direction must form an orthonormal frame",
                    )
                })?;
            let radius = PositiveLength::from_assigned_real(*radius).ok_or_else(|| {
                CodecError::malformed("CylinderSurface.radius must be positive and finite")
            })?;
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::new(*origin, frame, radius),
            ))
        }
        // The persisted b-rep cone holds a signed half angle in
        // `0 < |half_angle| < pi/2`, and the sign selects the direction the
        // cross-section grows along the frame axis. The reader keeps it: the
        // slant-to-axial conversion `surface_parameter_affine` applies to the
        // cone's pcurves is `cos(half_angle)`, which is even, so a negative
        // angle converts as consistently as a positive one. Both decode goldens
        // that hold cones hold negative half angles, so a positive interval here
        // would fail those decodes. Each arm of this match refuses what its IR
        // carrier refuses and nothing more.
        TextSurface::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            half_angle,
            ..
        } => {
            let frame =
                OrthonormalFrame3::new(axis.get(), ref_direction.get()).ok_or_else(|| {
                    CodecError::malformed(
                        "ConeSurface.axis/ref_direction must form an orthonormal frame",
                    )
                })?;
            let radius =
                NonNegativeLength::from_finite_assigned_real(*radius).ok_or_else(|| {
                    CodecError::malformed("ConeSurface.radius must be nonnegative and finite")
                })?;
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::new(
                    *origin,
                    frame,
                    radius,
                    PositiveReal::ONE,
                    Angle::from_assigned_real(*half_angle),
                ),
            ))
        }
        TextSurface::Sphere {
            center,
            axis,
            ref_direction,
            radius,
            ..
        } => {
            let frame =
                OrthonormalFrame3::new(axis.get(), ref_direction.get()).ok_or_else(|| {
                    CodecError::malformed(
                        "SphereSurface.axis/ref_direction must form an orthonormal frame",
                    )
                })?;
            let radius = NonZeroLength::from_assigned_real(*radius).ok_or_else(|| {
                CodecError::malformed("SphereSurface.radius must be finite and nonzero")
            })?;
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::analytic::SphereSurface::new(*center, frame, radius),
            ))
        }
        TextSurface::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
            ..
        } => {
            let frame =
                OrthonormalFrame3::new(axis.get(), ref_direction.get()).ok_or_else(|| {
                    CodecError::malformed(
                        "TorusSurface.axis/ref_direction must form an orthonormal frame",
                    )
                })?;
            let major_radius =
                PositiveLength::from_assigned_real(*major_radius).ok_or_else(|| {
                    CodecError::malformed("TorusSurface.major_radius must be positive and finite")
                })?;
            let minor_radius =
                NonZeroLength::from_assigned_real(*minor_radius).ok_or_else(|| {
                    CodecError::malformed("TorusSurface.minor_radius must be finite and nonzero")
                })?;
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                cadmpeg_ir::geometry::analytic::TorusSurface::new(
                    *center,
                    frame,
                    major_radius,
                    minor_radius,
                ),
            ))
        }
        TextSurface::Nurbs(nurbs) => SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            clone_nurbs_surface(ctx, nurbs)?,
        )),
        TextSurface::Extrusion {
            direction,
            directrix,
        } => {
            let directrix_id: CurveId = model_identity(
                ctx,
                "surface",
                id.as_str(),
                "directrix",
                "FreeCAD surface directrix identity",
            )?;
            append_text_curve(
                ctx,
                directrix.curve(),
                crate::resource::copied_identity(
                    ctx,
                    directrix_id.as_str(),
                    "FreeCAD surface directrix identity copy",
                )?,
                association,
                curve_transfer,
            )?;
            let admitted_payload =
                cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
                    directrix_id,
                    None,
                    *direction,
                    None,
                    None,
                );
            reserve_vec_items(
                ctx,
                &mut transfer.procedural,
                1,
                "FreeCAD procedural surfaces",
            )?;
            let procedural_id: SurfaceId = crate::resource::copied_identity(
                ctx,
                id.as_str(),
                "FreeCAD procedural surface identity copy",
            )?;
            let construction_id: ProceduralSurfaceId = model_identity(
                ctx,
                "surface",
                id.as_str(),
                "construction",
                "FreeCAD surface construction identity",
            )?;
            transfer.procedural.push((
                procedural_id,
                ProceduralSurface::new(
                    construction_id,
                    ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                    None,
                ),
            ));
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })
        }
        TextSurface::Revolution {
            axis_origin,
            axis_direction,
            directrix,
        } => {
            let directrix_id: CurveId = model_identity(
                ctx,
                "surface",
                id.as_str(),
                "directrix",
                "FreeCAD surface directrix identity",
            )?;
            append_text_curve(
                ctx,
                directrix.curve(),
                crate::resource::copied_identity(
                    ctx,
                    directrix_id.as_str(),
                    "FreeCAD surface directrix identity copy",
                )?,
                association,
                curve_transfer,
            )?;
            reserve_vec_items(
                ctx,
                &mut transfer.procedural,
                1,
                "FreeCAD procedural surfaces",
            )?;
            let procedural_id: SurfaceId = crate::resource::copied_identity(
                ctx,
                id.as_str(),
                "FreeCAD procedural surface identity copy",
            )?;
            let admitted_payload =
                cadmpeg_ir::geometry::surface_payloads::admit_revolution_axis_from_parts(
                    *axis_origin,
                    *axis_direction,
                )
                .and_then(|axis| {
                    cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                        directrix_id,
                        axis,
                        [0.0, std::f64::consts::TAU],
                        None,
                        None,
                        true,
                        cadmpeg_ir::geometry::CacheContract::from_form(None),
                    )
                })
                .map_err(cadmpeg_core::CodecError::malformed)?;
            let construction_id: ProceduralSurfaceId = model_identity(
                ctx,
                "surface",
                id.as_str(),
                "construction",
                "FreeCAD surface construction identity",
            )?;
            transfer.procedural.push((
                procedural_id,
                ProceduralSurface::new(
                    construction_id,
                    ProceduralSurfaceDefinition::Revolution(admitted_payload),
                    None,
                ),
            ));
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })
        }
        TextSurface::Trimmed {
            parameter_ranges,
            basis,
        } => {
            let basis_parameters = surface_parameter_affine(basis.surface());
            let parameter_ranges = [
                parameter_ranges[0].map(|value| {
                    value
                        .get()
                        .mul_add(basis_parameters.u_scale, basis_parameters.u_offset)
                }),
                parameter_ranges[1].map(|value| {
                    value
                        .get()
                        .mul_add(basis_parameters.v_scale, basis_parameters.v_offset)
                }),
            ];
            let basis_id: SurfaceId = model_identity(
                ctx,
                "surface",
                id.as_str(),
                "basis",
                "FreeCAD surface basis identity",
            )?;
            let basis_geometry = append_text_surface(
                ctx,
                basis.surface(),
                crate::resource::copied_identity(
                    ctx,
                    basis_id.as_str(),
                    "FreeCAD surface basis identity copy",
                )?,
                association,
                curve_transfer,
                transfer,
            )?;
            reserve_vec_items(
                ctx,
                &mut transfer.procedural,
                1,
                "FreeCAD procedural surfaces",
            )?;
            let procedural_id: SurfaceId = crate::resource::copied_identity(
                ctx,
                id.as_str(),
                "FreeCAD procedural surface identity copy",
            )?;
            let admitted_payload =
                cadmpeg_ir::geometry::surface_payloads::SubsetSurfaceConstruction::try_new(
                    basis_id,
                    parameter_ranges,
                    None,
                    None,
                    None,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?;
            let construction_id: ProceduralSurfaceId = model_identity(
                ctx,
                "surface",
                id.as_str(),
                "construction",
                "FreeCAD surface construction identity",
            )?;
            transfer.procedural.push((
                procedural_id,
                ProceduralSurface::new(
                    construction_id,
                    ProceduralSurfaceDefinition::Subset(admitted_payload),
                    None,
                ),
            ));
            basis_geometry
        }
        TextSurface::Offset { distance, basis } => {
            let basis_id: SurfaceId = model_identity(
                ctx,
                "surface",
                id.as_str(),
                "basis",
                "FreeCAD surface basis identity",
            )?;
            append_text_surface(
                ctx,
                basis.surface(),
                crate::resource::copied_identity(
                    ctx,
                    basis_id.as_str(),
                    "FreeCAD surface basis identity copy",
                )?,
                association,
                curve_transfer,
                transfer,
            )?;
            let admitted_payload =
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
                    basis_id,
                    *distance,
                    None,
                    None,
                    false,
                    cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                    None,
                );
            reserve_vec_items(
                ctx,
                &mut transfer.procedural,
                1,
                "FreeCAD procedural surfaces",
            )?;
            let procedural_id: SurfaceId = crate::resource::copied_identity(
                ctx,
                id.as_str(),
                "FreeCAD procedural surface identity copy",
            )?;
            let construction_id: ProceduralSurfaceId = model_identity(
                ctx,
                "surface",
                id.as_str(),
                "construction",
                "FreeCAD surface construction identity",
            )?;
            transfer.procedural.push((
                procedural_id,
                ProceduralSurface::new(
                    construction_id,
                    ProceduralSurfaceDefinition::Offset(admitted_payload),
                    None,
                ),
            ));
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })
        }
    };
    reserve_vec_items(
        ctx,
        &mut transfer.surfaces,
        1,
        "FreeCAD transferred surfaces",
    )?;
    transfer.surfaces.push(Surface {
        id,
        geometry: clone_surface_geometry(ctx, &geometry)?,
        source_object: Some(clone_source_association(ctx, association)?),
    });
    Ok(geometry)
}

#[cfg(test)]
pub(crate) mod tests;
