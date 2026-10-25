// SPDX-License-Identifier: Apache-2.0
//! Bounded framing for text and binary exact-shape side entries.

pub(crate) mod location_identity;
pub(crate) mod owners;
pub(crate) mod records;
mod table;
use table::ReadTable;
pub(crate) mod source;
mod source_rows;
pub(crate) mod triangulation;
#[cfg(test)]
use source::{parse_binary_prefix, parse_text};

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
    /// Canonical polygon identities, in source table order.
    pub(crate) rows: Vec<String>,
    /// Canonical nonpolygon tables and transient polygon values.
    pub(crate) payload: ShapePayload,
}

impl ShapePayloadRecord {
    pub(crate) fn declared_text_counts(&self) -> Option<&BTreeMap<String, usize>> {
        match &self.payload {
            ShapePayload::Text { facts, .. } => Some(&facts.declared_counts),
            ShapePayload::Empty | ShapePayload::Binary { .. } => None,
        }
    }
    pub(crate) fn has_unread_tables(&self) -> bool {
        self.payload.shape_set().is_some_and(|facts| {
            facts.locations.as_read().is_none()
                || facts.curve2ds.as_read().is_none()
                || facts.curves.as_read().is_none()
                || facts.polygon3d_refs.as_read().is_none()
                || facts.indexed_polygon_refs.as_read().is_none()
                || facts.surfaces.as_read().is_none()
                || facts.triangulations.as_read().is_none()
                || facts.tshapes.0.as_read().is_none()
                || facts.roots.as_read().is_none()
        })
    }
    pub(crate) fn polygon_references(&self) -> impl Iterator<Item = &str> {
        self.payload.shape_set().into_iter().flat_map(|facts| {
            facts
                .polygon3d_refs
                .iter()
                .chain(&facts.indexed_polygon_refs)
                .map(String::as_str)
        })
    }
    pub(crate) fn source_index(&self, table: u8, index: usize) -> usize {
        index
            .checked_sub(1)
            .and_then(|offset| self.payload.shape_set()?.indices.get(&table)?.get(offset))
            .copied()
            .unwrap_or(index)
    }
    pub(crate) fn row_identity(&self, table: u8, index: usize) -> Option<&str> {
        let facts = self.payload.shape_set()?;
        if table != 3 && table != 4 {
            return Some(self.id.as_str());
        }
        let source_index = self.source_index(table, index);
        facts
            .row_ids
            .get(&(table, source_index))
            .map(String::as_str)
    }
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
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ShapeSet {
    /// Source indices retained only in this transient projection cache.
    indices: BTreeMap<u8, Vec<usize>>,
    /// Original readable text table counts survive loss of individual rows.
    declared_counts: BTreeMap<String, usize>,
    row_ids: BTreeMap<(u8, usize), String>,
    polygon3d_refs: ReadTable<String>,
    indexed_polygon_refs: ReadTable<String>,
    /// Ordered location table with resolved transforms.
    locations: ReadTable<TextLocation>,
    /// Ordered parameter-space curve table.
    curve2ds: ReadTable<TextCurve2d>,
    /// Ordered 3D curve table.
    curves: ReadTable<TextCurve>,
    /// Ordered standalone 3D polygons.
    polygons3d: ReadTable<TextPolygon3d>,
    /// Ordered polygons indexing triangulation nodes.
    polygons_on_triangulations: ReadTable<TextPolygonOnTriangulation>,
    /// Ordered exact surface table.
    surfaces: ReadTable<TextSurface>,
    /// Ordered display triangulation table.
    triangulations: ReadTable<TextTriangulation>,
    /// Ordered subshape-first topology records.
    tshapes: TextTShapes,
    /// Root shape uses stored after the shape set.
    roots: ReadTable<TextShapeUse>,
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
    fn admit_table(&mut self, table: usize) {
        match table {
            0 => self.locations.admit_empty(),
            1 => self.curve2ds.admit_empty(),
            2 => self.curves.admit_empty(),
            3 => {
                self.polygons3d.admit_empty();
                self.polygon3d_refs.admit_empty();
            }
            4 => {
                self.polygons_on_triangulations.admit_empty();
                self.indexed_polygon_refs.admit_empty();
            }
            5 => self.surfaces.admit_empty(),
            6 => self.triangulations.admit_empty(),
            7 => self.tshapes.0.admit_empty(),
            _ => {}
        }
    }

    fn section_counts(&self) -> BTreeMap<String, usize> {
        self.declared_counts.clone()
    }

    fn wire_indices(&self) -> BTreeMap<u8, &[usize]> {
        self.indices
            .iter()
            // Polygon slots have one canonical home: their own row.index.
            .filter(|(table, _)| !matches!(**table, 3 | 4))
            .map(|(table, indices)| (*table, indices.as_slice()))
            .collect()
    }

    fn validate_source_counts(&self) -> Result<(), String> {
        for (table, name) in source::TABLES.iter().enumerate() {
            let Some(count) = self.declared_counts.get(*name) else {
                continue;
            };
            let actual = match table {
                0 => self.locations.len(),
                1 => self.curve2ds.len(),
                2 => self.curves.len(),
                3 => self.polygon3d_refs.len().max(self.polygons3d.len()),
                4 => self
                    .indexed_polygon_refs
                    .len()
                    .max(self.polygons_on_triangulations.len()),
                5 => self.surfaces.len(),
                6 => self.triangulations.len(),
                7 => self.tshapes.len(),
                _ => {
                    return Err(format!(
                        "unknown B-rep source table {name} at index {table}"
                    ));
                }
            };
            if *count > 1_000_000 || actual > *count {
                return Err(format!("text shape-set source count for {name} is invalid"));
            }
            let table = u8::try_from(table).map_err(|_| "invalid B-rep table index")?;
            if self
                .indices
                .get(&table)
                .is_some_and(|indices| indices.iter().any(|index| *index == 0 || index > count))
            {
                return Err(format!("text shape-set source index exceeds {name} count"));
            }
        }
        Ok(())
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
        self.validate_source_counts()?;
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
    #[serde(default)]
    source_indices: BTreeMap<u8, Vec<usize>>,
    topology_version: u8,
    section_counts: BTreeMap<String, usize>,
    shape_types: Option<BTreeMap<String, usize>>,
    locations: Option<Vec<TextLocation>>,
    curve2ds: Option<Vec<TextCurve2d>>,
    curves: Option<Vec<TextCurve>>,
    surfaces: Option<Vec<TextSurface>>,
    polygons3d: Option<Vec<String>>,
    polygons_on_triangulations: Option<Vec<String>>,
    triangulations: Option<Vec<TextTriangulation>>,
    tshapes: Option<TextTShapes>,
    roots: Option<Vec<TextShapeUse>>,
}

#[derive(Deserialize)]
struct BinaryFactsWire {
    #[serde(default)]
    source_indices: BTreeMap<u8, Vec<usize>>,
    topology_version: u8,
    locations: Option<Vec<TextLocation>>,
    curve2ds: Option<Vec<TextCurve2d>>,
    curves: Option<Vec<TextCurve>>,
    polygons3d: Option<Vec<String>>,
    polygons_on_triangulations: Option<Vec<String>>,
    surfaces: Option<Vec<TextSurface>>,
    triangulations: Option<Vec<TextTriangulation>>,
    tshapes: Option<TextTShapes>,
    roots: Option<Vec<TextShapeUse>>,
}

/// The retained wire shape, borrowed from the record it states.
///
/// Reading owns the tables it builds; writing needs no copy of them. The text
/// form retains source table counts and derives its admitted shape-kind census.
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
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    source_indices: BTreeMap<u8, &'a [usize]>,
    topology_version: u8,
    section_counts: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shape_types: Option<BTreeMap<String, usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locations: Option<&'a [TextLocation]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    curve2ds: Option<&'a [TextCurve2d]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    curves: Option<&'a [TextCurve]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    surfaces: Option<&'a [TextSurface]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    polygons3d: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    polygons_on_triangulations: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    triangulations: Option<&'a [TextTriangulation]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tshapes: Option<&'a TextTShapes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    roots: Option<&'a [TextShapeUse]>,
}

#[derive(Serialize)]
struct BinaryFactsOut<'a> {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    source_indices: BTreeMap<u8, &'a [usize]>,
    topology_version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    locations: Option<&'a [TextLocation]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    curve2ds: Option<&'a [TextCurve2d]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    curves: Option<&'a [TextCurve]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    polygons3d: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    polygons_on_triangulations: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    surfaces: Option<&'a [TextSurface]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    triangulations: Option<&'a [TextTriangulation]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tshapes: Option<&'a TextTShapes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    roots: Option<&'a [TextShapeUse]>,
}

impl<'a> BinaryFactsOut<'a> {
    fn new(facts: &'a ShapeSet, version: BinaryTopologyVersion) -> Self {
        Self {
            topology_version: version.number(),
            source_indices: facts.wire_indices(),
            locations: facts.locations.as_read(),
            curve2ds: facts.curve2ds.as_read(),
            curves: facts.curves.as_read(),
            polygons3d: facts.polygon3d_refs.as_read(),
            polygons_on_triangulations: facts.indexed_polygon_refs.as_read(),
            surfaces: facts.surfaces.as_read(),
            triangulations: facts.triangulations.as_read(),
            tshapes: facts.tshapes.0.as_read().map(|_| &facts.tshapes),
            roots: facts.roots.as_read(),
        }
    }
}

impl<'a> TextFactsOut<'a> {
    fn new(facts: &'a ShapeSet, version: TextTopologyVersion) -> Self {
        Self {
            topology_version: version.number(),
            source_indices: facts.wire_indices(),
            section_counts: facts.section_counts(),
            shape_types: facts.tshapes.0.as_read().map(|_| facts.shape_type_counts()),
            locations: facts.locations.as_read(),
            curve2ds: facts.curve2ds.as_read(),
            curves: facts.curves.as_read(),
            surfaces: facts.surfaces.as_read(),
            polygons3d: facts.polygon3d_refs.as_read(),
            polygons_on_triangulations: facts.indexed_polygon_refs.as_read(),
            triangulations: facts.triangulations.as_read(),
            tshapes: facts.tshapes.0.as_read().map(|_| &facts.tshapes),
            roots: facts.roots.as_read(),
        }
    }
}

impl TryFrom<BinaryFactsWire> for ShapeSet {
    type Error = String;

    fn try_from(value: BinaryFactsWire) -> Result<Self, Self::Error> {
        let facts = Self {
            indices: value.source_indices,
            declared_counts: BTreeMap::new(),
            row_ids: BTreeMap::new(),
            locations: value.locations.into(),
            curve2ds: value.curve2ds.into(),
            curves: value.curves.into(),
            polygons3d: value
                .polygons3d
                .as_ref()
                .filter(|rows| rows.is_empty())
                .map(|_| Vec::new())
                .into(),
            polygon3d_refs: value.polygons3d.into(),
            polygons_on_triangulations: value
                .polygons_on_triangulations
                .as_ref()
                .filter(|rows| rows.is_empty())
                .map(|_| Vec::new())
                .into(),
            indexed_polygon_refs: value.polygons_on_triangulations.into(),
            surfaces: value.surfaces.into(),
            triangulations: value.triangulations.into(),
            tshapes: value.tshapes.unwrap_or_default(),
            roots: value.roots.into(),
        };
        if facts.polygon3d_refs.is_empty() && facts.indexed_polygon_refs.is_empty() {
            facts.validate()?;
        }
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
                    indices: text.source_indices,
                    declared_counts: text.section_counts,
                    row_ids: BTreeMap::new(),
                    locations: text.locations.into(),
                    curve2ds: text.curve2ds.into(),
                    curves: text.curves.into(),
                    polygons3d: text
                        .polygons3d
                        .as_ref()
                        .filter(|rows| rows.is_empty())
                        .map(|_| Vec::new())
                        .into(),
                    polygon3d_refs: text.polygons3d.into(),
                    polygons_on_triangulations: text
                        .polygons_on_triangulations
                        .as_ref()
                        .filter(|rows| rows.is_empty())
                        .map(|_| Vec::new())
                        .into(),
                    indexed_polygon_refs: text.polygons_on_triangulations.into(),
                    surfaces: text.surfaces.into(),
                    triangulations: text.triangulations.into(),
                    tshapes: text.tshapes.unwrap_or_default(),
                    roots: text.roots.into(),
                };

                if facts.polygon3d_refs.is_empty() && facts.indexed_polygon_refs.is_empty() {
                    facts.validate()?;
                }
                if facts
                    .declared_counts
                    .keys()
                    .any(|name| !source::TABLES.contains(&name.as_str()))
                {
                    return Err("text shape-set section_counts has invalid table names".to_owned());
                }
                facts.validate_source_counts()?;
                if text.shape_types != facts.tshapes.0.as_read().map(|_| facts.shape_type_counts())
                {
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
            rows: Vec::new(),
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
pub(crate) struct TextTShapes(ReadTable<TextTShape>);

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
        Self(shapes.into())
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
            .map(|rows: Vec<TextTShape>| Self(rows.into()))
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
    pub(crate) location: usize,
    /// Signed composition power.
    pub(crate) power: i64,
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
    /// Whether the surface's OCCT frame is left-handed. The neutral carrier
    /// keeps the right-handed frame and a reversed parameter, so its normal is
    /// opposite the OCCT normal; a trimmed or offset surface inherits it from
    /// its basis.
    pub(crate) fn normal_reversed(&self) -> bool {
        match self {
            Self::Plane { v_reversed, .. } => *v_reversed,
            Self::Cylinder { u_reversed, .. }
            | Self::Cone { u_reversed, .. }
            | Self::Sphere { u_reversed, .. }
            | Self::Torus { u_reversed, .. } => *u_reversed,
            Self::Trimmed { basis, .. } | Self::Offset { basis, .. } => {
                basis.surface().normal_reversed()
            }
            Self::Nurbs(_) | Self::Extrusion { .. } | Self::Revolution { .. } => false,
        }
    }

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

/// Reuse the admitted surface grammar for a row generated by the writer.
/// The inverse chart maps neutral UV coordinates back to OCCT coordinates.
pub(crate) fn generated_surface_parameters(
    row: &str,
) -> Result<SurfaceParameterAffine, CodecError> {
    let tokens = row.split_whitespace().collect::<Vec<_>>();
    let mut cursor = TokenCursor::new(&tokens);
    let surface = parse_surface(&mut cursor, 0, 0)?;
    if !cursor.is_empty() {
        return Err(CodecError::malformed(
            "generated surface has trailing fields",
        ));
    }
    Ok(surface_parameter_affine(&surface))
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

/// Derive an exhaustive family census from successfully parsed exact-shape payloads.
pub(crate) fn carrier_census(
    ctx: &DecodeContext<'_>,
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<crate::native::CarrierCensusRecord>, CodecError> {
    let mut census = Vec::new();
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
        let polygons3d = facts.polygon3d_refs.len().max(facts.polygons3d.len());
        let indexed = facts
            .indexed_polygon_refs
            .len()
            .max(facts.polygons_on_triangulations.len());
        let triangulations = facts.triangulations.len();
        let tshapes = &facts.tshapes;
        let mut record = crate::native::CarrierCensusRecord {
            id: crate::native::native_child_id("carrier-census", &payload.id, "families"),
            payload: payload.id.clone(),
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
            polygons_3d: cadmpeg_core::decode::u64_from_index(polygons3d),
            polygons_on_triangulations: cadmpeg_core::decode::u64_from_index(indexed),
            triangulations: cadmpeg_core::decode::u64_from_index(triangulations),
        };
        for curve in curve2ds {
            census_curve(CensusCurve::Parameter(curve), &mut record.curves_2d)?;
        }
        for curve in curves {
            census_curve(CensusCurve::Model(curve), &mut record.curves_3d)?;
        }
        for surface in surfaces {
            census_surface(ctx, surface, &mut record.surfaces, &mut record.curves_3d)?;
        }
        for shape in tshapes.iter() {
            increment(
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
            );
        }
        census.push(record);
    }
    census.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(census)
}

fn increment(counts: &mut BTreeMap<String, u64>, family: &str) {
    if let Some(count) = counts.get_mut(family) {
        *count += 1;
    } else {
        counts.insert(family.to_owned(), 1);
    }
}

enum CensusCurve<'a> {
    Model(&'a TextCurve),
    Parameter(&'a TextCurve2d),
}

fn census_curve(
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
        increment(counts, family);
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
            increment(counts, "extrusion");
            census_curve(CensusCurve::Model(directrix.curve()), curves)?;
            return Ok(());
        }
        TextSurface::Revolution { directrix, .. } => {
            increment(counts, "revolution");
            census_curve(CensusCurve::Model(directrix.curve()), curves)?;
            return Ok(());
        }
        TextSurface::Trimmed { basis, .. } => {
            increment(counts, "trimmed");
            return census_surface(ctx, basis.surface(), counts, curves);
        }
        TextSurface::Offset { basis, .. } => {
            increment(counts, "offset");
            return census_surface(ctx, basis.surface(), counts, curves);
        }
    };
    increment(counts, family);
    Ok(())
}

#[derive(Clone, Copy)]
struct BinaryGeometryCounts {
    curves: usize,
    curves2d: usize,
    surfaces: usize,
    locations: usize,
    polygons3d: usize,
    indexed_polygons: usize,
    triangulations: usize,
}

fn parse_binary_tshape(
    cursor: &mut BinaryCursor<'_>,
    version: u8,
    index: usize,
    tshape_count: usize,
    counts: BinaryGeometryCounts,
) -> Result<TextTShape, CodecError> {
    let BinaryGeometryCounts {
        curves: curve_count,
        curves2d: curve2d_count,
        surfaces: surface_count,
        locations: location_count,
        triangulations: triangulation_count,
        ..
    } = counts;
    cursor.mark_census(source::census::Lane::Shape, 1)?;
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
                let representation_start = cursor.view.position();
                let representation_kind = cursor.u8("binary edge representation kind")?;
                if representation_kind == 0 {
                    break;
                }
                if representations.len() >= 1_000_000 {
                    return Err(CodecError::Malformed(
                        "binary edge representation-count limit exceeded".into(),
                    ));
                }
                let representation =
                    parse_binary_edge_representation(cursor, version, representation_kind, counts)?;
                if matches!(representation_kind, 2 | 3) {
                    cursor.representation_marks.push((
                        representations.len(),
                        representation_start..cursor.view.position(),
                    ));
                }
                representations.push(representation);
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

fn parse_binary_edge_representation(
    cursor: &mut BinaryCursor<'_>,
    version: u8,
    kind: u8,
    counts: BinaryGeometryCounts,
) -> Result<TextEdgeRepresentation, CodecError> {
    let BinaryGeometryCounts {
        curves: curve_count,
        curves2d: curve2d_count,
        surfaces: surface_count,
        locations: location_count,
        polygons3d: polygon3d_count,
        indexed_polygons: indexed_polygon_count,
        triangulations: triangulation_count,
    } = counts;
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
    cursor: &mut BinaryCursor<'_>,
    depth: usize,
) -> Result<TextSurface, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "binary surface nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    cursor.mark_census(source::census::Lane::Surface, 1)?;
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
            cursor.bounded(pole_count, 24, "binary Bezier surface pole")?;
            let mut control_points = Vec::new();
            let mut weights = (rational).then(Vec::new);
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
                        clamped_bezier_knots(u_degree)?,
                        false,
                    ),
                    NurbsSurfaceAxis::new(
                        u32::try_from(v_degree).map_err(|_| {
                            CodecError::Malformed("binary Bezier v degree exceeds u32".into())
                        })?,
                        clamped_bezier_knots(v_degree)?,
                        false,
                    ),
                    NurbsSurfaceLanes::new(
                        grid_rows(control_points, v_count)?,
                        weights
                            .map(|values| grid_rows(values, v_count))
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
            cursor.bounded(pole_count, 24, "binary B-spline surface pole")?;
            let mut control_points = Vec::new();
            let mut weights = (rational).then(Vec::new);
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point3("binary B-spline surface pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary B-spline surface weight")?);
                }
            }
            TextSurface::Nurbs(normalize_periodic_surface(
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
    cursor: &mut BinaryCursor<'_>,
    depth: usize,
) -> Result<TextCurve, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "binary 3D curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    cursor.mark_census(source::census::Lane::Curve, 1)?;
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
            cursor.bounded(pole_count, 24, "binary Bezier pole")?;
            let mut control_points = Vec::new();
            let mut weights = (rational).then(Vec::new);
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
                    clamped_bezier_knots(degree)?,
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
            cursor.bounded(pole_count, 24, "binary B-spline pole")?;
            let mut control_points = Vec::new();
            let mut weights = (rational).then(Vec::new);
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point3("binary B-spline pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary B-spline weight")?);
                }
            }
            let knots = cursor.expanded_knots(knot_count, "binary B-spline")?;
            let (knots, padding) = normalize_periodic_knots(knots, degree, periodic)?;
            append_periodic_curve_poles(&mut control_points, weights.as_mut(), padding)?;
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
    cursor: &mut BinaryCursor<'_>,
    depth: usize,
) -> Result<TextCurve2d, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "binary parameter-curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    cursor.mark_census(source::census::Lane::Curve2d, 1)?;
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
            cursor.bounded(pole_count, 16, "binary Bezier parameter pole")?;
            let mut control_points = Vec::new();
            let mut weights = (rational).then(Vec::new);
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
                knots: clamped_bezier_knots(degree)?,
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
            cursor.bounded(pole_count, 16, "binary B-spline parameter pole")?;
            let mut control_points = Vec::new();
            let mut weights = (rational).then(Vec::new);
            for _ in 0..pole_count {
                control_points.push(cursor.finite_point2("binary B-spline pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.finite_f64("binary B-spline weight")?);
                }
            }
            let knots = cursor.expanded_knots(knot_count, "binary B-spline")?;
            let (knots, padding) = normalize_periodic_knots(knots, degree, periodic)?;
            append_periodic_curve_poles(&mut control_points, weights.as_mut(), padding)?;
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

struct BinaryCursor<'a> {
    view: View<'a>,
    census_marks: Vec<(View<'a>, source::census::Lane)>,
    representation_marks: Vec<(usize, std::ops::Range<usize>)>,
}

impl<'a> BinaryCursor<'a> {
    #[cfg(test)]
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            view: View::over_retained(bytes),
            census_marks: Vec::new(),
            representation_marks: Vec::new(),
        }
    }

    fn remaining(&self) -> usize {
        self.view.remaining()
    }

    fn mark_census(&mut self, lane: source::census::Lane, width: usize) -> Result<(), CodecError> {
        let start = self.view.position();
        let view = self
            .view
            .child(start, start + width)
            .ok_or_else(|| CodecError::malformed("cut-short census discriminator"))?;
        self.census_marks.push((view, lane));
        Ok(())
    }

    fn read_counted<T>(
        &mut self,
        count: usize,
        mut read: impl FnMut(&mut Self) -> Result<T, CodecError>,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        for _ in 0..count {
            values.push(read(self)?);
        }
        Ok(values)
    }

    /// Clamps a declared element count to what the unread bytes can hold.
    ///
    /// `element_size` is the minimum encoded bytes of one element; a count that
    /// could not physically fit in the remaining input is rejected.
    fn bounded(&self, count: usize, element_size: usize, label: &str) -> Result<usize, CodecError> {
        bounded_len(
            cadmpeg_core::decode::u64_from_index(count),
            element_size,
            self.remaining(),
        )
        .ok_or_else(|| CodecError::malformed(format_args!("{label} count exceeds remaining input")))
    }

    fn take(&mut self, count: usize, _label: &str) -> Result<&'a [u8], CodecError> {
        Ok(self.view.req_take(count)?)
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
            return Err(CodecError::malformed(format_args!(
                "binary B-rep expected {name} section, found {line:?}"
            )));
        }
        tokens
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| CodecError::malformed(format_args!("invalid binary {name} count")))
    }

    fn u8(&mut self, _label: &str) -> Result<u8, CodecError> {
        Ok(self.view.req_u8()?)
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

    fn u16(&mut self, _label: &str) -> Result<u16, CodecError> {
        Ok(self.view.req_u16_le()?)
    }

    fn i32(&mut self, _label: &str) -> Result<i32, CodecError> {
        Ok(self.view.req_i32_le()?)
    }

    fn count(&mut self, label: &str) -> Result<usize, CodecError> {
        let value = self.i32(label)?;
        usize::try_from(value)
            .ok()
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| CodecError::malformed(format_args!("invalid {label}")))
    }

    fn finite_f64(&mut self, label: &str) -> Result<FiniteReal, CodecError> {
        let value = self.view.req_f64_le()?;
        FiniteReal::new(value)
            .ok_or_else(|| CodecError::malformed(format_args!("non-finite {label}")))
    }

    fn f32(&mut self, label: &str) -> Result<FiniteBinary32, CodecError> {
        let value = self.view.req_f32_le()?;
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
            knots.extend(std::iter::repeat_with(|| knot).take(multiplicity));
        }
        Ok(knots)
    }
}

fn parse_curve2d(
    cursor: &mut TokenCursor<'_>,
    depth: usize,
    table_index: usize,
) -> Result<TextCurve2d, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "text B-rep 2D curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    cursor.mark_census(source::census::Lane::Curve2d)?;
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

fn parse_bezier_curve2d(cursor: &mut TokenCursor<'_>) -> Result<NurbsCurve2d, CodecError> {
    let rational = cursor.boolean("2D Bezier rational flag")?;
    let degree = cursor.count("2D Bezier degree", 64)?;
    let pole_count = degree + 1;
    let mut control_points = Vec::new();
    let mut weights = (rational).then(Vec::new);
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point2("2D Bezier pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("2D Bezier weight")?);
        }
    }
    Ok(NurbsCurve2d {
        degree: u32::try_from(degree)
            .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
        knots: clamped_bezier_knots(degree)?,
        control_points,
        weights,
        periodic: false,
    })
}

fn parse_nurbs_curve2d(cursor: &mut TokenCursor<'_>) -> Result<NurbsCurve2d, CodecError> {
    let rational = cursor.boolean("2D B-spline rational flag")?;
    let periodic = cursor.boolean("2D B-spline periodic flag")?;
    let degree = cursor.count("2D B-spline degree", 64)?;
    let pole_count = cursor.count("2D B-spline pole count", 1_000_000)?;
    let knot_count = cursor.count("2D B-spline knot count", 1_000_000)?;
    // Each pole consumes at least its two point2 tokens.
    cursor.bounded(pole_count, 2, "2D B-spline pole")?;
    let mut control_points = Vec::new();
    let mut weights = (rational).then(Vec::new);
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point2("2D B-spline pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("2D B-spline weight")?);
        }
    }
    let knots = parse_knots(cursor, knot_count, degree, "2D B-spline")?;
    let (knots, padding) = normalize_periodic_knots(
        knots,
        u32::try_from(degree)
            .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
        periodic,
    )?;
    append_periodic_curve_poles(&mut control_points, weights.as_mut(), padding)?;
    Ok(NurbsCurve2d {
        degree: u32::try_from(degree)
            .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
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

fn parse_shape_kind(token: &str) -> Result<TextShapeKind, CodecError> {
    match token {
        "Ve" => Ok(TextShapeKind::Vertex),
        "Ed" => Ok(TextShapeKind::Edge),
        "Wi" => Ok(TextShapeKind::Wire),
        "Fa" => Ok(TextShapeKind::Face),
        "Sh" => Ok(TextShapeKind::Shell),
        "So" => Ok(TextShapeKind::Solid),
        "CS" => Ok(TextShapeKind::CompSolid),
        "Co" => Ok(TextShapeKind::Compound),
        _ => Err(CodecError::malformed(format_args!(
            "invalid TShape kind {token:?}"
        ))),
    }
}

fn parse_tshape_geometry(
    kind: TextShapeKind,
    cursor: &mut TokenCursor<'_>,
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
    cursor: &mut TokenCursor<'_>,
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
        let location_of = |cursor: &mut TokenCursor<'_>| {
            parse_reference(
                cursor,
                "vertex location",
                counts.get("Locations").copied(),
                true,
            )
        };
        let representation = match kind {
            1 => TextPointRepresentation::Curve3d {
                parameter,
                curve: parse_reference(
                    cursor,
                    "vertex curve",
                    counts.get("Curves").copied(),
                    false,
                )?,
                location: location_of(cursor)?,
            },
            2 => TextPointRepresentation::Pcurve {
                parameter,
                curve: parse_reference(
                    cursor,
                    "vertex parameter curve",
                    counts.get("Curve2ds").copied(),
                    false,
                )?,
                surface: parse_reference(
                    cursor,
                    "vertex surface",
                    counts.get("Surfaces").copied(),
                    false,
                )?,
                location: location_of(cursor)?,
            },
            3 => TextPointRepresentation::Surface {
                parameter,
                second_parameter: cursor.finite_real("vertex second surface parameter")?,
                surface: parse_reference(
                    cursor,
                    "vertex surface",
                    counts.get("Surfaces").copied(),
                    false,
                )?,
                location: location_of(cursor)?,
            },
            other => {
                return Err(CodecError::malformed(format_args!(
                    "invalid vertex representation kind {other}"
                )));
            }
        };
        representations.push(representation);
    }
    Ok(TextTShapeGeometry::Vertex {
        tolerance,
        point,
        representations,
    })
}

fn parse_edge_geometry(
    cursor: &mut TokenCursor<'_>,
    counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<TextTShapeGeometry, CodecError> {
    let tolerance = cursor.finite_real("edge tolerance")?;
    let same_parameter = cursor.boolean("edge same-parameter flag")?;
    let same_range = cursor.boolean("edge same-range flag")?;
    let degenerated = cursor.boolean("edge degenerated flag")?;
    let mut representations = Vec::new();
    loop {
        let representation_start = cursor.index;
        let kind = cursor.integer("edge representation kind")?;
        if kind == 0 {
            break;
        }
        if representations.len() >= 1_000_000 {
            return Err(CodecError::Malformed(
                "edge representation-count limit exceeded".into(),
            ));
        }
        let representation = parse_edge_representation(kind, cursor, counts, topology_version)?;
        if matches!(kind, 2 | 3) {
            if let Some(ranges) = cursor.ranges {
                let first = ranges
                    .get(representation_start)
                    .ok_or_else(|| CodecError::malformed("pcurve source start is absent"))?;
                let last = cursor
                    .index
                    .checked_sub(1)
                    .and_then(|index| ranges.get(index))
                    .ok_or_else(|| CodecError::malformed("pcurve source end is absent"))?;
                cursor
                    .representation_marks
                    .push((representations.len(), first.start..last.end));
            }
        }
        representations.push(representation);
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
    cursor: &mut TokenCursor<'_>,
    counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<TextEdgeRepresentation, CodecError> {
    u8::try_from(kind)
        .map_err(|_| CodecError::Malformed("invalid edge representation kind".into()))?;
    match kind {
        1 => {
            let curve = parse_reference(
                cursor,
                "edge 3D curve",
                counts.get("Curves").copied(),
                false,
            )?;
            let location = parse_reference(
                cursor,
                "edge curve location",
                counts.get("Locations").copied(),
                true,
            )?;
            let parameter_range = parse_range(cursor, "edge curve")?;
            Ok(TextEdgeRepresentation::Curve3d {
                curve,
                location,
                parameter_range,
            })
        }
        2 | 3 => {
            let curve = parse_reference(
                cursor,
                "edge parameter curve",
                counts.get("Curve2ds").copied(),
                false,
            )?;
            let secondary = if kind == 3 {
                let (secondary, joined_continuity) = parse_reference_suffix(
                    cursor,
                    "edge secondary parameter curve",
                    counts.get("Curve2ds").copied(),
                )?;
                let continuity = match joined_continuity {
                    Some(continuity) => continuity,
                    None => cursor.next("edge continuity")?.to_owned(),
                };
                Some((secondary, continuity))
            } else {
                None
            };
            let surface = parse_reference(
                cursor,
                "edge surface",
                counts.get("Surfaces").copied(),
                false,
            )?;
            let location = parse_reference(
                cursor,
                "edge surface location",
                counts.get("Locations").copied(),
                true,
            )?;
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
            let continuity = continuity_token.to_owned();
            let first_surface = parse_reference(
                cursor,
                "edge regularity surface",
                counts.get("Surfaces").copied(),
                false,
            )?;
            let location = parse_reference(
                cursor,
                "edge regularity location",
                counts.get("Locations").copied(),
                true,
            )?;
            let second_surface = parse_reference(
                cursor,
                "edge second regularity surface",
                counts.get("Surfaces").copied(),
                false,
            )?;
            let second_location = parse_reference(
                cursor,
                "edge second regularity location",
                counts.get("Locations").copied(),
                true,
            )?;
            Ok(TextEdgeRepresentation::Regularity {
                continuity,
                surfaces: [first_surface, second_surface],
                locations: [location, second_location],
            })
        }
        5 => {
            let polygon = parse_reference(
                cursor,
                "edge 3D polygon",
                counts.get("Polygon3D").copied(),
                false,
            )?;
            let location = parse_reference(
                cursor,
                "edge polygon location",
                counts.get("Locations").copied(),
                true,
            )?;
            Ok(TextEdgeRepresentation::Polygon3d { polygon, location })
        }
        6 | 7 => {
            let polygon = parse_reference(
                cursor,
                "edge polygon on triangulation",
                counts.get("PolygonOnTriangulations").copied(),
                false,
            )?;
            let secondary = if kind == 7 {
                Some(parse_reference(
                    cursor,
                    "edge second polygon on triangulation",
                    counts.get("PolygonOnTriangulations").copied(),
                    false,
                )?)
            } else {
                None
            };
            let triangulation = parse_reference(
                cursor,
                "edge triangulation",
                counts.get("Triangulations").copied(),
                false,
            )?;
            let location = parse_reference(
                cursor,
                "edge triangulation location",
                counts.get("Locations").copied(),
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
    cursor: &mut TokenCursor<'_>,
    counts: &BTreeMap<String, usize>,
) -> Result<TextTShapeGeometry, CodecError> {
    let natural_restriction = cursor.boolean("face natural-restriction flag")?;
    let tolerance = cursor.finite_real("face tolerance")?;
    let surface = parse_reference(
        cursor,
        "face surface",
        counts.get("Surfaces").copied(),
        true,
    )?;
    let location = parse_reference(
        cursor,
        "face location",
        counts.get("Locations").copied(),
        true,
    )?;
    let triangulation = if cursor.peek()? == Some("2") {
        cursor.next("face triangulation marker")?;
        Some(parse_reference(
            cursor,
            "face triangulation",
            counts.get("Triangulations").copied(),
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

fn parse_shape_flags(token: &str, topology_version: u8) -> Result<[bool; 7], CodecError> {
    if token.len() != 7 || !token.bytes().all(|byte| matches!(byte, b'0' | b'1')) {
        return Err(CodecError::malformed(format_args!(
            "invalid TShape flags {token:?}"
        )));
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
    cursor: &mut TokenCursor<'_>,
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
            return Err(CodecError::malformed(format_args!(
                "invalid shape use {token:?}"
            )));
        }
    };
    let encoded = encoded
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("invalid shape use {token:?}")))?;
    if encoded == 0 || encoded > shape_count {
        return Err(CodecError::malformed(format_args!(
            "shape use index {encoded} is out of range"
        )));
    }
    let shape = shape_count - encoded + 1;
    let location = parse_reference(
        cursor,
        "shape use location",
        counts.get("Locations").copied(),
        true,
    )?;
    Ok(TextShapeUse {
        shape,
        orientation,
        location: location.into(),
    })
}

fn parse_reference(
    cursor: &mut TokenCursor<'_>,
    label: &str,
    maximum: Option<usize>,
    allow_zero: bool,
) -> Result<usize, CodecError> {
    let value = cursor
        .next(label)?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("invalid {label}")))?;
    if maximum.is_some_and(|maximum| value > maximum) {
        return Err(CodecError::malformed(format_args!(
            "{label} limit exceeded"
        )));
    }
    if value == 0 && !allow_zero {
        return Err(CodecError::malformed(format_args!("{label} index is zero")));
    }
    Ok(value)
}

fn parse_reference_suffix(
    cursor: &mut TokenCursor<'_>,
    label: &str,
    maximum: Option<usize>,
) -> Result<(usize, Option<String>), CodecError> {
    let token = cursor.next(label)?;
    let split = token
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(token.len());
    let (reference, suffix) = token.split_at(split);
    let value = reference
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("invalid {label}")))?;
    if value == 0 || maximum.is_some_and(|maximum| value > maximum) {
        return Err(CodecError::malformed(format_args!(
            "{label} limit exceeded"
        )));
    }
    Ok((value, (!suffix.is_empty()).then(|| suffix.to_owned())))
}

fn parse_range(cursor: &mut TokenCursor<'_>, label: &str) -> Result<[FiniteReal; 2], CodecError> {
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
    cursor: &mut TokenCursor<'_>,
    depth: usize,
    table_index: usize,
) -> Result<TextSurface, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "text B-rep surface nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    cursor.mark_census(source::census::Lane::Surface)?;
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
    cursor: &mut TokenCursor<'_>,
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

fn parse_nurbs_surface(cursor: &mut TokenCursor<'_>) -> Result<NurbsSurface, CodecError> {
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
    cursor.bounded(pole_count, 3, "B-spline surface pole")?;
    let mut control_points = Vec::new();
    let mut weights = (rational).then(Vec::new);
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point("B-spline surface pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("B-spline surface weight")?);
        }
    }
    let u_knots = parse_knots(cursor, u_knot_count, u_degree, "B-spline u")?;
    let v_knots = parse_knots(cursor, v_knot_count, v_degree, "B-spline v")?;
    normalize_periodic_surface(
        [
            u32::try_from(u_degree)
                .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
            u32::try_from(v_degree)
                .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
        ],
        [u_knots, v_knots],
        [u_count, v_count],
        control_points,
        weights,
        [u_periodic, v_periodic],
    )
}

fn parse_bezier_surface(cursor: &mut TokenCursor<'_>) -> Result<NurbsSurface, CodecError> {
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
    let mut control_points = Vec::new();
    let mut weights = (rational).then(Vec::new);
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point("Bezier surface pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("Bezier surface weight")?);
        }
    }
    NurbsSurface::from_finite_lanes(
        NurbsSurfaceAxis::new(
            u32::try_from(u_degree)
                .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
            clamped_bezier_knots(u_degree)?,
            false,
        ),
        NurbsSurfaceAxis::new(
            u32::try_from(v_degree)
                .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
            clamped_bezier_knots(v_degree)?,
            false,
        ),
        NurbsSurfaceLanes::new(
            grid_rows(control_points, v_count)?,
            weights
                .map(|values| grid_rows(values, v_count))
                .transpose()?,
        ),
        false,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn parse_knots(
    cursor: &mut TokenCursor<'_>,
    knot_count: usize,
    degree: usize,
    label: &str,
) -> Result<Vec<FiniteReal>, CodecError> {
    let mut knots = Vec::new();
    for _ in 0..knot_count {
        let knot = cursor.finite_real(&format!("{label} knot"))?;
        let multiplicity = cursor.count(&format!("{label} knot multiplicity"), degree + 1)?;
        if knots
            .len()
            .checked_add(multiplicity)
            .is_none_or(|count| count > 2_000_000)
        {
            return Err(CodecError::malformed(format_args!(
                "expanded {label} knot limit exceeded"
            )));
        }
        knots.extend(std::iter::repeat_with(|| knot).take(multiplicity));
    }
    Ok(knots)
}

fn normalize_periodic_knots(
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
    knots
        .len()
        .checked_add(2 * padding)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline knot limit exceeded".into()))?;
    let mut normalized = Vec::new();
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
    control_points.extend_from_within(..padding);
    if let Some(weights) = weights {
        if weights.len() < padding {
            return Err(CodecError::Malformed(
                "periodic B-spline has insufficient weights".into(),
            ));
        }
        weights.extend_from_within(..padding);
    }
    Ok(())
}

fn normalize_periodic_surface(
    degrees: [u32; 2],
    knots: [Vec<FiniteReal>; 2],
    counts: [usize; 2],
    control_points: Vec<FinitePoint3>,
    weights: Option<Vec<FiniteReal>>,
    periodic: [bool; 2],
) -> Result<NurbsSurface, CodecError> {
    let [u_source_knots, v_source_knots] = knots;
    let (u_knots, u_padding) = normalize_periodic_knots(u_source_knots, degrees[0], periodic[0])?;
    let (v_knots, v_padding) = normalize_periodic_knots(v_source_knots, degrees[1], periodic[1])?;
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
    new_u
        .checked_mul(new_v)
        .filter(|count| *count <= 2_000_000)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline pole limit exceeded".into()))?;
    let (control_points, weights) = if u_padding != 0 || v_padding != 0 {
        let old_points = &control_points;
        let old_weights = weights.as_deref();
        let mut points = Vec::new();
        let mut weights = (old_weights.is_some()).then(Vec::new);
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
            grid_rows(
                control_points,
                usize::try_from(v_count).map_err(|_| {
                    CodecError::Malformed(
                        "periodic B-spline v pole count exceeds address space".into(),
                    )
                })?,
            )?,
            weights
                .map(|values| {
                    grid_rows(
                        values,
                        usize::try_from(v_count).map_err(|_| {
                            CodecError::Malformed(
                                "periodic B-spline v pole count exceeds address space".into(),
                            )
                        })?,
                    )
                })
                .transpose()?,
        ),
        false,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn parse_curve(
    cursor: &mut TokenCursor<'_>,
    depth: usize,
    table_index: usize,
) -> Result<TextCurve, CodecError> {
    if depth > MAX_GEOMETRY_NESTING_DEPTH {
        return Err(CodecError::malformed(format_args!(
            "text B-rep 3D curve nesting exceeds {MAX_GEOMETRY_NESTING_DEPTH}"
        )));
    }
    cursor.mark_census(source::census::Lane::Curve)?;
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

fn parse_nurbs_curve(cursor: &mut TokenCursor<'_>) -> Result<NurbsCurve, CodecError> {
    let rational = cursor.boolean("B-spline rational flag")?;
    let periodic = cursor.boolean("B-spline periodic flag")?;
    let degree = cursor.count("B-spline degree", 64)?;
    let pole_count = cursor.count("B-spline pole count", 1_000_000)?;
    let knot_count = cursor.count("B-spline knot count", 1_000_000)?;
    // Each pole consumes its three point tokens.
    cursor.bounded(pole_count, 3, "B-spline pole")?;
    let mut control_points = Vec::new();
    let mut weights = (rational).then(Vec::new);
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point("B-spline pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("B-spline weight")?);
        }
    }
    let knots = parse_knots(cursor, knot_count, degree, "B-spline")?;
    let (knots, padding) = normalize_periodic_knots(
        knots,
        u32::try_from(degree)
            .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
        periodic,
    )?;
    append_periodic_curve_poles(&mut control_points, weights.as_mut(), padding)?;
    NurbsCurve::from_finite_lanes(
        u32::try_from(degree)
            .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
        knots,
        control_points,
        weights,
        periodic,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn parse_bezier_curve(cursor: &mut TokenCursor<'_>) -> Result<NurbsCurve, CodecError> {
    let rational = cursor.boolean("Bezier rational flag")?;
    let degree = cursor.count("Bezier degree", 64)?;
    let pole_count = degree + 1;
    let mut control_points = Vec::new();
    let mut weights = (rational).then(Vec::new);
    for _ in 0..pole_count {
        control_points.push(cursor.finite_point("Bezier pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.finite_real("Bezier weight")?);
        }
    }
    NurbsCurve::from_finite_lanes(
        u32::try_from(degree)
            .map_err(|_| CodecError::Malformed("B-rep integer exceeds u32".into()))?,
        clamped_bezier_knots(degree)?,
        control_points,
        weights,
        false,
    )
    .map_err(|error| CodecError::Malformed(error.to_string()))
}

fn clamped_bezier_knots(degree: usize) -> Result<Vec<FiniteReal>, CodecError> {
    let half = degree
        .checked_add(1)
        .ok_or_else(|| CodecError::malformed("FreeCAD Bezier degree overflows"))?;
    half.checked_mul(2)
        .ok_or_else(|| CodecError::malformed("FreeCAD Bezier knot count overflows"))?;
    let mut knots = Vec::new();
    knots.extend(std::iter::repeat_with(|| FiniteReal::ZERO).take(half));
    knots.extend(std::iter::repeat_with(|| FiniteReal::ONE).take(half));
    Ok(knots)
}

fn grid_rows<T>(values: Vec<T>, width: usize) -> Result<Vec<Vec<T>>, CodecError> {
    if width == 0 || !values.len().is_multiple_of(width) {
        return Err(CodecError::malformed(
            "surface grid dimensions do not match pole count",
        ));
    }
    let mut rows = Vec::new();
    let mut values = values.into_iter();
    while values.len() != 0 {
        let mut row = Vec::new();
        row.extend(values.by_ref().take(width));
        rows.push(row);
    }
    Ok(rows)
}

struct TokenCursor<'a> {
    tokens: &'a [&'a str],
    index: usize,
    source: Option<View<'a>>,
    ranges: Option<&'a [std::ops::Range<usize>]>,
    discriminator: Option<&'a str>,
    census_marks: Vec<(View<'a>, source::census::Lane)>,
    representation_marks: Vec<(usize, std::ops::Range<usize>)>,
}

impl<'a> TokenCursor<'a> {
    fn new(tokens: &'a [&'a str]) -> Self {
        Self {
            tokens,
            index: 0,
            source: None,
            ranges: None,
            discriminator: None,
            census_marks: Vec::new(),
            representation_marks: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn mark_census(&mut self, lane: source::census::Lane) -> Result<(), CodecError> {
        if let Some(source) = self.source {
            let range = self
                .ranges
                .and_then(|ranges| ranges.get(self.index))
                .ok_or_else(|| CodecError::malformed("cut-short census discriminator"))?;
            let view = source
                .child(range.start, range.end)
                .ok_or_else(|| CodecError::malformed("census discriminator exceeds source"))?;
            self.census_marks.push((view, lane));
        }
        Ok(())
    }

    fn remaining(&self) -> usize {
        self.tokens.get(self.index..).map_or(0, <[_]>::len)
    }

    /// Clamps a declared element count to the unread token count.
    ///
    /// `element_size` is the minimum tokens one element consumes; a count that
    /// could not fit in the remaining tokens is rejected.
    fn bounded(&self, count: usize, element_size: usize, label: &str) -> Result<usize, CodecError> {
        bounded_len(
            cadmpeg_core::decode::u64_from_index(count),
            element_size,
            self.remaining(),
        )
        .ok_or_else(|| {
            CodecError::malformed(format_args!("{label} count exceeds available tokens"))
        })
    }

    fn peek(&mut self) -> Result<Option<&'a str>, CodecError> {
        if let Some(token) = self.discriminator {
            return Ok(Some(token));
        }
        if self.is_empty() {
            return Ok(None);
        }
        let token = self.next("B-rep lookahead token")?;
        self.index -= 1;
        self.discriminator = Some(token);
        Ok(Some(token))
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
        if let Some(token) = self.discriminator.take() {
            self.index += 1;
            return Ok(token);
        }
        let token = self.tokens.get(self.index).copied().ok_or_else(|| {
            CodecError::malformed(format_args!("truncated {label} in text B-rep Curves table"))
        })?;
        let token = if let Some(source) = self.source {
            let range = self
                .ranges
                .and_then(|ranges| ranges.get(self.index))
                .ok_or_else(|| CodecError::malformed("B-rep token has no source range"))?;
            let mut view = source
                .child(range.start, range.end)
                .ok_or_else(|| CodecError::malformed("B-rep token exceeds its source"))?;
            let bytes = view.req_take(range.len())?;
            std::str::from_utf8(bytes)
                .map_err(|_| CodecError::malformed("B-rep token is not UTF-8"))?
        } else {
            token
        };
        self.index += 1;
        Ok(token)
    }
}

#[derive(Default)]
pub(crate) struct CurveTransfer {
    pub(crate) curves: Vec<Curve>,
    pub(crate) procedural: Vec<(CurveId, ProceduralCurve)>,
}

fn model_identity<I>(kind: &str, parent: &str, child: &str) -> Result<I, CodecError>
where
    I: TryFrom<String>,
    I::Error: std::fmt::Display,
{
    I::try_from(native::model_id(kind, parent, child)).map_err(CodecError::malformed)
}

pub(crate) fn transfer_text_curves<'owner>(
    ctx: &DecodeContext<'_>,
    payloads: &[ShapePayloadRecord],
    properties: impl Into<owners::Properties<'owner>>,
    sources: Option<&source::projection::ProjectionSources<'_>>,
) -> Result<CurveTransfer, CodecError> {
    let properties = properties.into();
    let mut transfer = CurveTransfer::default();
    for payload in payloads {
        let Some(curves) = payload.payload.shape_set().map(|set| &set.curves) else {
            continue;
        };
        let association =
            properties
                .owner(ctx, &payload.property)?
                .map(|object_id| SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Fcstd,
                    object_id,
                    instance_path: Vec::new(),
                });
        for (index, curve) in curves.iter().enumerate() {
            let id: CurveId = model_identity(
                "curve",
                &payload.id,
                &payload.source_index(2, index + 1).to_string(),
            )?;
            let start = transfer.curves.len();
            let procedural_start = transfer.procedural.len();
            let original = sources
                .map(|sources| sources.curve(payload, index + 1))
                .transpose()?;
            let principal = append_text_curve(
                ctx,
                curve,
                id,
                association.as_ref(),
                &mut transfer,
                original,
            )?;
            for record in &mut transfer.curves[start..] {
                record.native_ref = payload.row_identity(2, index + 1).map(str::to_owned);
                if let Some(source) = &mut record.source_object {
                    source.geometry_role = Some(cadmpeg_ir::SourceGeometryRole::Support);
                }
            }
            if let Some(source) = &mut transfer.curves[principal].source_object {
                source.geometry_role = None;
            }
            for (_, record) in &mut transfer.procedural[procedural_start..] {
                record.native_ref = payload.row_identity(2, index + 1).map(str::to_owned);
            }
        }
    }
    Ok(transfer)
}

pub(crate) fn curve_geometry(
    ctx: &DecodeContext<'_>,
    curve: &TextCurve,
) -> Result<CurveGeometry, CodecError> {
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
        TextCurve::Nurbs(nurbs) => CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs.clone())),
        TextCurve::Trimmed { basis, .. } => {
            let _depth = ctx.enter_nested("FreeCAD curve geometry nesting")?;
            curve_geometry(ctx, basis.curve())?
        }
        TextCurve::Offset { .. } => {
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None })
        }
    };
    Ok(geometry)
}

pub(crate) fn local_curve_geometry(
    ctx: &DecodeContext<'_>,
    curve: &TextCurve,
    id: &CurveId,
) -> Result<CurveGeometry, CodecError> {
    let TextCurve::Trimmed {
        basis,
        parameter_range,
    } = curve
    else {
        return curve_geometry(ctx, curve);
    };
    let _depth = ctx.enter_nested("FreeCAD subset local geometry nesting")?;
    let basis_id: CurveId = model_identity("curve", id.as_str(), "basis")?;
    let basis_geometry = local_curve_geometry(ctx, basis.curve(), &basis_id)?;
    let solved = basis_geometry
        .solved()
        .ok_or_else(|| CodecError::NotImplemented("carrier has no solved geometry".into()))?;
    let parameter_range =
        crate::topology_transfer::normalize_occt_curve_range(solved, Some(*parameter_range))
            .unwrap_or(*parameter_range);
    let subset = cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction::from_finite_parts(
        basis_id,
        parameter_range,
        true,
        None,
    )
    .map_err(CodecError::malformed)?;
    Ok(match subset.local_cache(solved) {
        Some(cache) => CurveGeometry::Solved(cache),
        None => CurveGeometry::Procedural {
            construction: model_identity("curve", id.as_str(), "construction")?,
            cache: None,
        },
    })
}

fn curve_input<'a>(
    ctx: &DecodeContext<'_>,
    source: Option<&source::projection::geometry::GeometrySource<'_>>,
    id: &str,
    fallback: &'a TextCurve,
) -> Result<std::borrow::Cow<'a, TextCurve>, CodecError> {
    source
        .map(|source| source.read_curve(ctx, id).map(std::borrow::Cow::Owned))
        .unwrap_or_else(|| Ok(std::borrow::Cow::Borrowed(fallback)))
}

fn append_text_curve<'owner>(
    ctx: &DecodeContext<'_>,
    curve: &TextCurve,
    id: CurveId,
    association: impl Into<Option<&'owner SourceObjectAssociation>>,
    transfer: &mut CurveTransfer,
    source: Option<source::projection::geometry::GeometrySource<'_>>,
) -> Result<usize, CodecError> {
    let association = association.into();
    let _depth = ctx.enter_nested("FreeCAD curve transfer nesting")?;
    let input = curve_input(ctx, source.as_ref(), id.as_str(), curve)?;
    let curve = input.as_ref();
    let mut geometry = curve_geometry(ctx, curve)?;
    match curve {
        TextCurve::Trimmed { basis, .. } => {
            let basis_id: CurveId = model_identity("curve", id.as_str(), "basis")?;
            let basis_source = source.as_ref().map(|source| source.basis()).transpose()?;
            let basis_index = append_text_curve(
                ctx,
                basis.curve(),
                basis_id.clone(),
                association,
                transfer,
                basis_source,
            )?;
            let construction_id: ProceduralCurveId =
                model_identity("curve", id.as_str(), "construction")?;
            let construction = curve_input(ctx, source.as_ref(), construction_id.as_str(), curve)?;
            let TextCurve::Trimmed {
                parameter_range,
                basis,
            } = construction.as_ref()
            else {
                return Err(CodecError::malformed(
                    "trimmed curve construction source differs",
                ));
            };
            let basis_geometry = curve_geometry(ctx, basis.curve())?;
            let parameter_range = crate::topology_transfer::normalize_occt_curve_range(
                basis_geometry.solved().ok_or_else(|| {
                    CodecError::NotImplemented("carrier has no solved geometry".into())
                })?,
                Some(*parameter_range),
            )
            .unwrap_or(*parameter_range);
            let admitted =
                cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction::from_finite_parts(
                    basis_id,
                    parameter_range,
                    true,
                    None,
                )
                .map_err(CodecError::malformed)?;
            geometry = match transfer.curves[basis_index]
                .geometry
                .solved()
                .and_then(|basis| admitted.local_cache(basis))
            {
                Some(cache) => CurveGeometry::Solved(cache),
                None => CurveGeometry::Procedural {
                    construction: construction_id.clone(),
                    cache: None,
                },
            };
            transfer.procedural.push((
                id.clone(),
                ProceduralCurve::new(construction_id, ProceduralCurveDefinition::Subset(admitted)),
            ));
        }
        TextCurve::Offset { basis, .. } => {
            let basis_id: CurveId = model_identity("curve", id.as_str(), "basis")?;
            let basis_source = source.as_ref().map(|source| source.basis()).transpose()?;
            append_text_curve(
                ctx,
                basis.curve(),
                basis_id.clone(),
                association,
                transfer,
                basis_source,
            )?;
            let construction_id: ProceduralCurveId =
                model_identity("curve", id.as_str(), "construction")?;
            let construction = curve_input(ctx, source.as_ref(), construction_id.as_str(), curve)?;
            let TextCurve::Offset {
                distance,
                direction,
                ..
            } = construction.as_ref()
            else {
                return Err(CodecError::malformed(
                    "offset curve construction source differs",
                ));
            };
            let admitted = cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::from_admitted_direction(basis_id,*distance,*direction).map_err(CodecError::malformed)?;
            transfer.procedural.push((
                id.clone(),
                ProceduralCurve::new(construction_id, ProceduralCurveDefinition::Offset(admitted)),
            ));
        }
        _ => {}
    }
    let index = transfer.curves.len();
    transfer.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id,
        geometry,
        source_object: association.cloned(),
    });
    Ok(index)
}

#[derive(Default)]
pub(crate) struct SurfaceTransfer {
    pub(crate) surfaces: Vec<Surface>,
    pub(crate) procedural: Vec<(SurfaceId, ProceduralSurface)>,
}

pub(crate) fn transfer_text_surfaces<'owner>(
    ctx: &DecodeContext<'_>,
    payloads: &[ShapePayloadRecord],
    properties: impl Into<owners::Properties<'owner>>,
    curve_transfer: &mut CurveTransfer,
    sources: Option<&source::projection::ProjectionSources<'_>>,
) -> Result<SurfaceTransfer, CodecError> {
    let properties = properties.into();
    let mut transfer = SurfaceTransfer::default();
    for payload in payloads {
        let Some(surfaces) = payload.payload.shape_set().map(|set| &set.surfaces) else {
            continue;
        };
        let association =
            properties
                .owner(ctx, &payload.property)?
                .map(|object_id| SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Fcstd,
                    object_id,
                    instance_path: Vec::new(),
                });
        for (index, surface) in surfaces.iter().enumerate() {
            let id: SurfaceId = model_identity(
                "surface",
                &payload.id,
                &payload.source_index(5, index + 1).to_string(),
            )?;
            let start = transfer.surfaces.len();
            let procedural_start = transfer.procedural.len();
            let curve_start = curve_transfer.curves.len();
            let curve_procedural_start = curve_transfer.procedural.len();
            let original = sources
                .map(|sources| sources.surface(payload, index + 1))
                .transpose()?;
            let principal = append_text_surface(
                ctx,
                surface,
                id,
                association.as_ref(),
                curve_transfer,
                &mut transfer,
                original,
            )?;
            for record in &mut transfer.surfaces[start..] {
                record.native_ref = payload.row_identity(5, index + 1).map(str::to_owned);
                if let Some(source) = &mut record.source_object {
                    source.geometry_role = Some(cadmpeg_ir::SourceGeometryRole::Support);
                }
            }
            if let Some(source) = &mut transfer.surfaces[principal].source_object {
                source.geometry_role = None;
            }
            for (_, record) in &mut transfer.procedural[procedural_start..] {
                record.native_ref = payload.row_identity(5, index + 1).map(str::to_owned);
            }
            for record in &mut curve_transfer.curves[curve_start..] {
                if let Some(source) = &mut record.source_object {
                    source.geometry_role = Some(cadmpeg_ir::SourceGeometryRole::Support);
                }
                record.native_ref = payload.row_identity(5, index + 1).map(str::to_owned);
            }
            for (_, record) in &mut curve_transfer.procedural[curve_procedural_start..] {
                record.native_ref = payload.row_identity(5, index + 1).map(str::to_owned);
            }
        }
    }
    Ok(transfer)
}

pub(crate) fn surface_geometry(
    ctx: &DecodeContext<'_>,
    surface: &TextSurface,
) -> Result<SurfaceGeometry, CodecError> {
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
        TextSurface::Nurbs(nurbs) => {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs.clone()))
        }
        TextSurface::Trimmed { basis, .. } => {
            let _depth = ctx.enter_nested("FreeCAD surface geometry nesting")?;
            surface_geometry(ctx, basis.surface())?
        }
        TextSurface::Extrusion { .. }
        | TextSurface::Revolution { .. }
        | TextSurface::Offset { .. } => {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })
        }
    };
    Ok(geometry)
}

fn surface_input<'a>(
    ctx: &DecodeContext<'_>,
    source: Option<&source::projection::geometry::GeometrySource<'_>>,
    id: &str,
    fallback: &'a TextSurface,
) -> Result<std::borrow::Cow<'a, TextSurface>, CodecError> {
    source
        .map(|source| source.read_surface(ctx, id).map(std::borrow::Cow::Owned))
        .unwrap_or_else(|| Ok(std::borrow::Cow::Borrowed(fallback)))
}

fn append_text_surface<'owner>(
    ctx: &DecodeContext<'_>,
    surface: &TextSurface,
    id: SurfaceId,
    association: impl Into<Option<&'owner SourceObjectAssociation>>,
    curve_transfer: &mut CurveTransfer,
    transfer: &mut SurfaceTransfer,
    source: Option<source::projection::geometry::GeometrySource<'_>>,
) -> Result<usize, CodecError> {
    let association = association.into();
    let _depth = ctx.enter_nested("FreeCAD surface transfer nesting")?;
    let input = surface_input(ctx, source.as_ref(), id.as_str(), surface)?;
    let surface = input.as_ref();
    let geometry = surface_geometry(ctx, surface)?;
    match surface {
        TextSurface::Extrusion { directrix, .. } | TextSurface::Revolution { directrix, .. } => {
            let directrix_id: CurveId = model_identity("surface", id.as_str(), "directrix")?;
            let directrix_source = source
                .as_ref()
                .map(|source| source.directrix())
                .transpose()?;
            append_text_curve(
                ctx,
                directrix.curve(),
                directrix_id.clone(),
                association,
                curve_transfer,
                directrix_source,
            )?;
            let construction_id: ProceduralSurfaceId =
                model_identity("surface", id.as_str(), "construction")?;
            let construction =
                surface_input(ctx, source.as_ref(), construction_id.as_str(), surface)?;
            let definition = match construction.as_ref() {
                TextSurface::Extrusion { direction, .. } => ProceduralSurfaceDefinition::Extrusion(
                    cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
                        directrix_id,
                        None,
                        *direction,
                        None,
                        None,
                    ),
                ),
                TextSurface::Revolution {
                    axis_origin,
                    axis_direction,
                    ..
                } => {
                    let admitted = cadmpeg_ir::geometry::surface_payloads::admit_revolution_axis_from_parts(*axis_origin,*axis_direction)
                        .and_then(|axis| cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(directrix_id,axis,[0.0,std::f64::consts::TAU],None,None,true,cadmpeg_ir::geometry::CacheContract::from_form(None)))
                        .map_err(CodecError::malformed)?;
                    ProceduralSurfaceDefinition::Revolution(admitted)
                }
                _ => {
                    return Err(CodecError::malformed(
                        "swept surface construction source differs",
                    ));
                }
            };
            transfer.procedural.push((
                id.clone(),
                ProceduralSurface::new(construction_id, definition, None),
            ));
        }
        TextSurface::Trimmed { basis, .. } | TextSurface::Offset { basis, .. } => {
            let basis_id: SurfaceId = model_identity("surface", id.as_str(), "basis")?;
            let basis_source = source.as_ref().map(|source| source.basis()).transpose()?;
            append_text_surface(
                ctx,
                basis.surface(),
                basis_id.clone(),
                association,
                curve_transfer,
                transfer,
                basis_source,
            )?;
            let construction_id: ProceduralSurfaceId =
                model_identity("surface", id.as_str(), "construction")?;
            let construction =
                surface_input(ctx, source.as_ref(), construction_id.as_str(), surface)?;
            let definition = match construction.as_ref() {
                TextSurface::Trimmed {
                    parameter_ranges,
                    basis,
                } => {
                    let affine = surface_parameter_affine(basis.surface());
                    let ranges = [
                        parameter_ranges[0]
                            .map(|value| value.get().mul_add(affine.u_scale, affine.u_offset)),
                        parameter_ranges[1]
                            .map(|value| value.get().mul_add(affine.v_scale, affine.v_offset)),
                    ];
                    let admitted =
                        cadmpeg_ir::geometry::surface_payloads::SubsetSurfaceConstruction::try_new(
                            basis_id, ranges, None, None, None,
                        )
                        .map_err(CodecError::malformed)?;
                    ProceduralSurfaceDefinition::Subset(admitted)
                }
                TextSurface::Offset { distance, .. } => ProceduralSurfaceDefinition::Offset(
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
                        basis_id,
                        *distance,
                        None,
                        None,
                        false,
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                        None,
                    ),
                ),
                _ => {
                    return Err(CodecError::malformed(
                        "derived surface construction source differs",
                    ));
                }
            };
            transfer.procedural.push((
                id.clone(),
                ProceduralSurface::new(construction_id, definition, None),
            ));
        }
        _ => {}
    }
    let index = transfer.surfaces.len();
    transfer.surfaces.push(Surface {
        native_ref: None,
        id,
        geometry,
        source_object: association.cloned(),
    });
    Ok(index)
}

#[cfg(test)]
pub(crate) mod tests;
