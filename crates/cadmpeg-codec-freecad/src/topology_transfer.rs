// SPDX-License-Identifier: Apache-2.0
//! Occurrence-aware transfer of exact-shape topology into neutral CADIR.

mod body_names;
pub(crate) mod canonical_indices;
mod pcurve_source;
pub(crate) use body_names::apply as apply_body_names;

use std::collections::{HashMap, HashSet};

mod locations;
mod ring_order;
use crate::brep::location_identity::LocationIdentityError;
use locations::{Located, Locations};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::nurbs::NurbsError;
use cadmpeg_ir::geometry::pcurve::{PcurveMetadata, PcurveNurbsPoles, WeightedPole2};
use cadmpeg_ir::geometry::{
    pcurve::{Pcurve, PcurveGeometry, PcurveNurbs},
    sampled::{PolygonalSurface, PolylineCurve, PolylineSamples, PolylineVertex},
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::{Transform, Transform2};
use cadmpeg_ir::units::NonzeroPoint2;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::SourceObjectAssociation;

use crate::brep::{
    surface_parameter_affine, ShapePayloadRecord, SurfaceParameterAffine, Tables, TextCurve2d,
    TextEdgeRepresentation, TextOrientation, TextShapeKind, TextShapeUse, TextSurface, TextTShape,
    TextTShapeGeometry,
};
use crate::loss::FreecadLossCode;
use cadmpeg_ir::report::loss::LossNote;

const EPS_TOPOLOGY_TRANSFER_GEOMETRY: f64 = 1.0e-9;
const EPS_TOPOLOGY_TRANSFER_DEGENERATE: f64 = 1.0e-10;
const EPS_TOPOLOGY_TRANSFER_EXACT_GEOMETRY: f64 = 1.0e-12;

struct IndexedPolygon {
    samples: PolylineSamples<FiniteReal, FinitePoint3>,
    deflection: cadmpeg_ir::scalar::NonNegativeReal,
}

impl IndexedPolygon {
    /// Pair the polygon's two native lanes into sample rows.
    ///
    /// `FreeCAD` states nodes and parameters as two properties, so the codec
    /// pairs them here and refuses a polygon whose lanes disagree. The IR
    /// carries the rows only.
    fn try_new(
        nodes: Vec<FinitePoint3>,
        parameters: Option<Vec<FiniteReal>>,
        deflection: cadmpeg_ir::scalar::NonNegativeReal,
    ) -> Result<Self, CodecError> {
        let samples = match parameters {
            None => PolylineSamples::Unparameterized {
                points: nodes.try_into().map_err(|error| {
                    CodecError::malformed(format_args!("polygon states no node: {error}"))
                })?,
            },
            Some(parameters) => {
                if parameters.len() != nodes.len() {
                    return Err(CodecError::Malformed(
                        "polygon parameters length must equal nodes length".into(),
                    ));
                }
                let mut vertices = Vec::new();
                for (point, parameter) in nodes.into_iter().zip(parameters) {
                    vertices.push(PolylineVertex { parameter, point });
                }
                PolylineSamples::Parameterized {
                    vertices: vertices.try_into().map_err(|error| {
                        CodecError::malformed(format_args!("polygon states no node: {error}"))
                    })?,
                }
            }
        };
        Ok(Self {
            samples,
            deflection,
        })
    }
}
type FacePcurve = (PcurveId, Option<[FiniteReal; 2]>);

pub(crate) struct TopologyOccurrence {
    pub(crate) property: String,
    pub(crate) indexed_name: &'static str,
    pub(crate) source_index: usize,
    pub(crate) topology_id: String,
}

/// Transfer text or binary shape-set topology with placements applied once.
pub(crate) fn transfer<'owner>(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    payloads: &[ShapePayloadRecord],
    properties: impl Into<crate::brep::owners::Properties<'owner>>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<TopologyOccurrence>, CodecError> {
    transfer_with_sources(ctx, ir, payloads, properties, losses, None)
}

pub(crate) fn transfer_with_sources<'owner>(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    payloads: &[ShapePayloadRecord],
    properties: impl Into<crate::brep::owners::Properties<'owner>>,
    losses: &mut Vec<LossNote>,
    sources: Option<&crate::brep::source::projection::ProjectionSources<'_>>,
) -> Result<Vec<TopologyOccurrence>, CodecError> {
    let existing_pcurves = ir
        .model
        .pcurves
        .iter()
        .map(|curve| curve.id.clone())
        .collect::<HashSet<_>>();
    let properties = properties.into();
    let mut occurrences = Vec::new();
    for payload in payloads {
        let Some(tables) = Tables::from_payload(payload) else {
            continue;
        };
        let source_object = properties.owner(ctx, &payload.property)?;
        let mut builder = match Builder::new(ctx, payload, tables, source_object) {
            Ok(builder) => builder,
            Err(LocationIdentityError::Decode(error)) => return Err(error),
            Err(LocationIdentityError::Bounded) => {
                losses.push(FreecadLossCode::TopologyLocationIdentityBounded.note(format!(
                    "shape payload {} retains native topology because location identity expansion reached its local work bound",
                    payload.id,
                )));
                continue;
            }
        };
        builder.sources = sources;
        builder.shape_indices = (1..=tables.tshapes.len())
            .map(|index| payload.source_index(7, index))
            .collect();
        builder.emit_pcurves(ir, sources)?;
        for root in builder.body_roots()? {
            builder.append_body(ctx, ir, root)?;
        }
        builder.emit_unowned_triangulations(ir)?;
        occurrences.extend(builder.occurrences);
        losses.extend(builder.losses);
    }
    close_radial_rings(&mut ir.model.coedges);
    let referenced_pcurves = referenced_pcurve_ids(&ir.model.coedges);
    if sources.is_some() {
        for curve in ir.model.pcurves.iter().filter(|curve| {
            !existing_pcurves.contains(&curve.id) && !referenced_pcurves.contains(&curve.id)
        }) {
            if let Some(authority) = ctx.record_capture_authority(curve.id.as_str())? {
                ctx.retire_record_capture(&authority)?;
            }
        }
    }
    ir.model
        .pcurves
        .retain(|pcurve| referenced_pcurves.contains(&pcurve.id));
    Ok(occurrences)
}

fn referenced_pcurve_ids(coedges: &[Coedge]) -> HashSet<&PcurveId> {
    let mut referenced = HashSet::new();
    for coedge in coedges {
        for pcurve in &coedge.pcurves {
            referenced.insert(&pcurve.pcurve);
        }
    }
    referenced
}

impl Tables<'_> {
    fn location(
        &self,
        index: impl Into<crate::brep::LocationRef>,
    ) -> Result<Transform, CodecError> {
        index.into().resolve(self.locations)
    }
}

#[derive(Clone, Copy)]
struct BodyRoot {
    shape: usize,
    transform: Located,
    reversed: bool,
    root_ordinal: Option<usize>,
}

#[derive(Clone, Copy)]
struct RegionTraversal {
    shape_index: usize,
    transform: Located,
    reversed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OccurrenceKey(String);

impl OccurrenceKey {
    fn new(shape: usize, transform: Located) -> Self {
        Self(occurrence_label(shape, transform))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SourceOccurrenceKey(String);

impl SourceOccurrenceKey {
    fn new(shape: usize, transform: Located) -> Self {
        Self(format!("{}@{}", shape, transform.identity))
    }
}

/// An edge occurrence and its start and end vertex occurrences.
#[derive(Clone)]
struct PlacedEdge {
    id: EdgeId,
    start: VertexId,
    end: VertexId,
}

impl PlacedEdge {
    /// The vertices a use of this edge runs from and to.
    fn oriented_ends(&self, sense: Sense) -> (&VertexId, &VertexId) {
        match sense {
            Sense::Forward => (&self.start, &self.end),
            Sense::Reversed => (&self.end, &self.start),
        }
    }
}

struct Builder<'a, 'c, 'r> {
    procedural_admission: Option<cadmpeg_ir::document::procedural::ProceduralAdmission>,
    ctx: &'c DecodeContext<'r>,
    payload: &'a ShapePayloadRecord,
    tables: Tables<'a>,
    sources: Option<&'a crate::brep::source::projection::ProjectionSources<'a>>,
    vertices: HashMap<OccurrenceKey, VertexId>,
    edges: HashMap<OccurrenceKey, PlacedEdge>,
    emitted_curves: HashSet<CurveId>,
    emitted_surfaces: HashSet<SurfaceId>,
    emitted_triangulations: HashSet<usize>,
    body_scope: Located,
    locations: Locations,
    root_discriminator: Option<usize>,
    current_body: Option<BodyId>,
    source_object: Option<cadmpeg_core::text::NonBlankString>,
    source_indices: HashMap<(TextShapeKind, SourceOccurrenceKey), usize>,
    shape_indices: Vec<usize>,
    occurrences: Vec<TopologyOccurrence>,
    losses: Vec<LossNote>,
}

impl<'a, 'c, 'r> Builder<'a, 'c, 'r> {
    fn new(
        ctx: &'c DecodeContext<'r>,
        payload: &'a ShapePayloadRecord,
        tables: Tables<'a>,
        source_object: Option<cadmpeg_core::text::NonBlankString>,
    ) -> Result<Self, LocationIdentityError> {
        let mut locations = Locations::new(ctx, tables)?;
        admit_topology_locations(ctx, tables, &mut locations)?;
        let source_indices = source_topology_indices(ctx, tables, &mut locations)?;
        Ok(Self {
            ctx,
            procedural_admission: None,
            payload,
            tables,
            sources: None,
            vertices: HashMap::new(),
            edges: HashMap::new(),
            emitted_curves: HashSet::new(),
            emitted_surfaces: HashSet::new(),
            emitted_triangulations: HashSet::new(),
            body_scope: Located::identity(),
            root_discriminator: None,
            current_body: None,
            source_object,
            source_indices,
            shape_indices: Vec::new(),
            locations,
            occurrences: Vec::new(),
            losses: Vec::new(),
        })
    }

    fn location(&self, index: impl Into<crate::brep::LocationRef>) -> Result<Located, CodecError> {
        self.locations.source(self.tables, index)
    }

    fn compose(&self, left: Located, right: Located) -> Result<Located, CodecError> {
        self.locations.compose(left, right)
    }

    fn source_association(&self) -> Result<Option<SourceObjectAssociation>, CodecError> {
        Ok(self
            .source_object
            .as_ref()
            .map(|object_id| SourceObjectAssociation {
                geometry_role: None,
                format: cadmpeg_ir::CodecFormat::Fcstd,
                object_id: object_id.clone(),
                instance_path: Vec::new(),
            }))
    }

    fn bind_topology(
        &mut self,
        kind: TextShapeKind,
        shape: usize,
        local: Located,
        topology_id: &str,
    ) -> Result<(), CodecError> {
        let key = SourceOccurrenceKey::new(shape, self.compose(self.body_scope, local)?);
        let Some(source_index) = self.source_indices.get(&(kind, key)).copied() else {
            return Ok(());
        };
        self.occurrences.push(TopologyOccurrence {
            property: self.payload.property.clone(),
            indexed_name: indexed_name(kind),
            source_index,
            topology_id: topology_id.to_owned(),
        });
        Ok(())
    }

    fn emit_pcurves(
        &mut self,
        ir: &mut CadIr,
        sources: Option<&crate::brep::source::projection::ProjectionSources<'_>>,
    ) -> Result<(), CodecError> {
        use crate::brep::source::projection::{PcurveInputs, PcurveUse};
        for (position, shape) in self.tables.tshapes.iter().enumerate() {
            let TextTShapeGeometry::Edge {
                representations, ..
            } = &shape.geometry
            else {
                continue;
            };
            for (representation_index, representation) in representations.iter().enumerate() {
                let (primary, secondary, surface, parameter_range) = match representation {
                    TextEdgeRepresentation::Pcurve {
                        curve,
                        surface,
                        parameter_range,
                        ..
                    } => (*curve, None, *surface, *parameter_range),
                    TextEdgeRepresentation::PcurvePair {
                        curves,
                        surface,
                        parameter_range,
                        ..
                    } => (curves[0], Some(curves[1]), *surface, *parameter_range),
                    _ => continue,
                };
                for (curve, secondary) in
                    std::iter::once((primary, false)).chain(secondary.map(|curve| (curve, true)))
                {
                    let id = self.pcurve_id(position + 1, representation_index, secondary)?;
                    if ir.model.pcurves.iter().any(|current| current.id == id) {
                        continue;
                    }
                    let request = pcurve_source::Projection {
                        id: id.clone(),
                        native_ref: self.payload.row_identity(1, curve).map(str::to_owned),
                        payload: &self.payload.id,
                        curve,
                    };
                    let pcurve = if let Some(sources) = sources {
                        sources.produce(
                            self.ctx,
                            id.as_str(),
                            self.payload,
                            PcurveUse {
                                shape: position + 1,
                                representation: representation_index,
                                curve,
                                secondary,
                            },
                            |inputs| {
                                pcurve_source::project(self.ctx, request, inputs, &mut self.losses)
                            },
                        )?
                    } else {
                        let inputs = PcurveInputs {
                            curve: self.tables.curve2ds[curve - 1].clone(),
                            parameter_range,
                            affine: self.tables.surfaces.get(surface - 1).map(|surface| {
                                surface_parameter_affine(face_support_surface(surface))
                            }),
                        };
                        pcurve_source::project(self.ctx, request, inputs, &mut self.losses)?
                    };
                    if let Some(pcurve) = pcurve {
                        ir.model.pcurves.push(pcurve);
                    }
                }
            }
        }
        Ok(())
    }

    fn emit_unowned_triangulations(&self, ir: &mut CadIr) -> Result<(), CodecError> {
        for (offset, triangulation) in self.tables.triangulations.iter().enumerate() {
            let index = offset + 1;
            if self.emitted_triangulations.contains(&index) {
                continue;
            }
            let id = cadmpeg_ir::tessellation::TessellationId::mint(crate::native::model_id(
                "tessellation",
                &self.payload.id,
                index.to_string(),
            ))
            .map_err(CodecError::malformed)?;
            let build = |triangulation: crate::brep::triangulation::TextTriangulation,
                         matrix: Transform| {
                self.triangulation_value(
                    id.clone(),
                    &triangulation,
                    index,
                    matrix,
                    Vec::new(),
                    None,
                )
            };
            let mesh = if let Some(sources) = self.sources {
                sources.unowned_triangulation(self.ctx, id.as_str(), self.payload, index, build)?
            } else if triangulation.nodes().is_empty() && triangulation.triangles().is_empty() {
                None
            } else {
                Some(build(triangulation.clone(), Transform::identity())?)
            };
            if let Some(mesh) = mesh {
                ir.model.tessellations.push(mesh);
            }
        }
        Ok(())
    }

    fn triangulation_value(
        &self,
        id: cadmpeg_ir::tessellation::TessellationId,
        triangulation: &crate::brep::triangulation::TextTriangulation,
        index: usize,
        transform: Transform,
        faces: Vec<FaceId>,
        body: Option<BodyId>,
    ) -> Result<Tessellation, CodecError> {
        let vertices = triangulation
            .nodes()
            .iter()
            .map(|point| {
                transform.apply_point(point.get()).ok_or_else(|| {
                    CodecError::malformed("placed triangulation contains a non-finite point")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let normals = triangulation
            .normals()
            .map(|values| {
                if faces.is_empty() {
                    return Ok(values.to_vec());
                }
                values
                    .iter()
                    .map(|normal| {
                        transform_normalized_vector(transform, normal.get()).ok_or_else(|| {
                            CodecError::malformed(
                                "placed triangulation contains a non-finite normal",
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        Ok(Tessellation::from_parts(
            id,
            cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
                vertices,
                triangulation.triangles().to_vec(),
                normals,
            )?,
            Vec::new(),
        )
        .map_err(CodecError::malformed)?
        .with_body(body)
        .with_faces(faces)
        .with_native_ref(self.payload.row_identity(6, index).map(str::to_owned))
        .with_admitted_chordal_deflection(Some(
            triangulation
                .deflection
                .scaled(uniform_scale(transform)?)
                .ok_or_else(|| {
                    CodecError::malformed("placed triangulation deflection is not finite")
                })?,
        ))
        .with_source_object(self.source_association()?))
    }

    fn pcurve_id(
        &self,
        edge: usize,
        representation: usize,
        secondary: bool,
    ) -> Result<PcurveId, CodecError> {
        let TextTShapeGeometry::Edge {
            representations, ..
        } = &self.tables.tshapes[edge - 1].geometry
        else {
            return Err(CodecError::malformed("pcurve owner is not an edge"));
        };
        let (curve, surface, range) = match &representations[representation] {
            TextEdgeRepresentation::Pcurve {
                curve,
                surface,
                parameter_range,
                ..
            } => (*curve, *surface, *parameter_range),
            TextEdgeRepresentation::PcurvePair {
                curves,
                surface,
                parameter_range,
                ..
            } => (curves[usize::from(secondary)], *surface, *parameter_range),
            _ => {
                return Err(CodecError::malformed(
                    "pcurve identity has no parameter-space representation",
                ))
            }
        };
        // Curve2d rows are content shared by edge occurrences. Include their
        // UV chart and domain, which can give one row distinct neutral forms.
        let chart = self
            .tables
            .surfaces
            .get(surface - 1)
            .map(|surface| surface_parameter_affine(face_support_surface(surface)));
        let curve = self.payload.source_index(1, curve);
        let key = format!("{curve}:{chart:?}:{range:?}");
        PcurveId::mint(crate::native::model_id("pcurve", &self.payload.id, &key))
            .map_err(CodecError::malformed)
    }

    fn shell_component_id(&self, key: &str, component_index: usize) -> Result<ShellId, CodecError> {
        let child = format!("{key}:component:{}", component_index + 1);
        ShellId::mint(crate::native::model_id("shell", &self.payload.id, &child))
            .map_err(CodecError::malformed)
    }

    fn body_roots(&self) -> Result<Vec<BodyRoot>, CodecError> {
        let has_multiple_roots = self.tables.roots.len() > 1;
        let mut roots = Vec::new();
        for (index, root) in self.tables.roots.iter().enumerate() {
            self.shape(root.shape)?;
            let transform = self.location(root.location)?;
            roots.push(BodyRoot {
                shape: root.shape,
                transform,
                reversed: is_reversed(root.orientation),
                root_ordinal: has_multiple_roots.then_some(index + 1),
            });
        }
        Ok(roots)
    }

    fn append_body(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        root: BodyRoot,
    ) -> Result<(), CodecError> {
        self.body_scope = root.transform;
        self.root_discriminator = root.root_ordinal;
        self.vertices.clear();
        self.edges.clear();
        let root_shape = self.shape(root.shape)?;
        let root_kind = root_shape.kind();
        if let TextTShapeGeometry::Edge {
            degenerated,
            representations,
            ..
        } = &root_shape.geometry
        {
            if root_shape.children.is_empty() {
                if !degenerated
                    && !representations.iter().any(|representation| {
                        matches!(representation, TextEdgeRepresentation::Curve3d { .. })
                    })
                {
                    let source = self
                        .payload
                        .row_identity(7, root.shape)
                        .unwrap_or(&self.payload.id);
                    self.losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                        "B-rep row {source} is retained natively; its unbounded edge has no surviving exact curve for a neutral body"
                    )));
                }
                return Ok(());
            }
        }
        let mut pending = vec![root.shape];
        let mut visited = HashSet::new();
        while let Some(index) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            let shape = self.shape(index)?;
            if matches!(shape.geometry, TextTShapeGeometry::Edge { .. })
                && (edge_endpoint_uses(index, &shape.children).is_err()
                    || shape.children.iter().any(|child| {
                        self.tables.tshapes[child.shape - 1].kind() != TextShapeKind::Vertex
                    }))
            {
                let source = self
                    .payload
                    .row_identity(7, index)
                    .unwrap_or(&self.payload.id);
                self.losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!("B-rep row {source} is retained natively; its surviving endpoint uses cannot form a neutral body")));
                return Ok(());
            }
            pending.extend(shape.children.iter().map(|child| child.shape));
        }
        let body_key = self.topology_label(root.shape, Located::identity())?;
        let body_id = BodyId::mint(crate::native::model_id("body", &self.payload.id, &body_key))
            .map_err(CodecError::malformed)?;
        self.current_body = Some(body_id.clone());
        let tessellation_start = ir.model.tessellations.len();
        let mut regions = Vec::new();
        self.append_shape_regions(
            ctx,
            ir,
            &body_id,
            RegionTraversal {
                shape_index: root.shape,
                transform: Located::identity(),
                reversed: root.reversed,
            },
            &mut regions,
        )?;
        if regions.is_empty() {
            ir.model.tessellations.truncate(tessellation_start);
            return Ok(());
        }
        let build = |source_kind: TextShapeKind, matrix: Transform| {
            let kind = match source_kind {
                TextShapeKind::Solid | TextShapeKind::Compound | TextShapeKind::CompSolid => {
                    self.body_kind(root.shape)?.unwrap_or(BodyKind::General)
                }
                TextShapeKind::Wire | TextShapeKind::Edge => BodyKind::Wire,
                TextShapeKind::Shell | TextShapeKind::Face => BodyKind::Sheet,
                _ => BodyKind::General,
            };
            Ok(Body {
                native_ref: Some(self.payload.id.clone()),
                id: body_id.clone(),
                kind,
                regions,
                transform: (!is_identity(matrix)).then_some(matrix),
                name: None,
                color: None,
                visible: None,
            })
        };
        let body = if let Some(sources) = self.sources {
            sources.body(
                ctx,
                body_id.as_str(),
                self.payload,
                root.shape,
                root.root_ordinal,
                build,
            )?
        } else {
            Some(build(
                self.shape(root.shape)?.kind(),
                root.transform.transform,
            )?)
        };
        let Some(body) = body else {
            return Ok(());
        };
        ir.model.bodies.push(body);
        if matches!(
            root_kind,
            TextShapeKind::Compound | TextShapeKind::CompSolid
        ) {
            self.bind_topology(root_kind, root.shape, Located::identity(), body_id.as_str())?;
        }
        Ok(())
    }

    fn append_shape_regions(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        body: &BodyId,
        traversal: RegionTraversal,
        output: &mut Vec<RegionId>,
    ) -> Result<(), CodecError> {
        let RegionTraversal {
            shape_index,
            transform,
            reversed,
        } = traversal;
        let _depth = ctx.enter_nested("transfer FCStd topology nesting")?;
        let shape = self.shape(shape_index)?.clone();
        if matches!(
            shape.kind(),
            TextShapeKind::Compound | TextShapeKind::CompSolid
        ) {
            for child in &shape.children {
                self.append_shape_regions(
                    ctx,
                    ir,
                    body,
                    RegionTraversal {
                        shape_index: child.shape,
                        transform: self.compose(transform, self.location(child.location)?)?,
                        reversed: reversed ^ is_reversed(child.orientation),
                    },
                    output,
                )?;
            }
            return Ok(());
        }
        let key = self.topology_label(shape_index, transform)?;
        let region_id = RegionId::mint(crate::native::model_id("region", &self.payload.id, &key))
            .map_err(CodecError::malformed)?;
        let mut shells = Vec::new();
        if shape.kind() == TextShapeKind::Solid {
            for child in shape
                .children
                .iter()
                .filter(|child| self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Shell)
            {
                let added = self.append_shell(ctx, ir, &region_id, child, transform, reversed)?;
                shells.extend(added);
            }
        } else {
            let added =
                self.append_shell_shape(ctx, ir, &region_id, shape_index, transform, reversed)?;
            shells.extend(added);
        }
        if !shells.is_empty() {
            let region = self.component(region_id.as_str(), shape_index, || {
                Ok(Region {
                    native_ref: self.payload.row_identity(7, shape_index).map(str::to_owned),
                    id: region_id.clone(),
                    body: body.clone(),
                    shells,
                })
            })?;
            let Some(region) = region else {
                return Ok(());
            };
            ir.model.regions.push(region);
            if shape.kind() == TextShapeKind::Solid {
                self.bind_topology(
                    TextShapeKind::Solid,
                    shape_index,
                    transform,
                    region_id.as_str(),
                )?;
            }
            output.push(region_id);
        }
        Ok(())
    }

    fn component<T>(
        &self,
        id: &str,
        shape: usize,
        build: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let expected = self.shape(shape)?.kind();
        if let Some(sources) = self.sources {
            sources.component(self.ctx, id, self.payload, shape, |value| {
                if value != expected {
                    return Err(CodecError::malformed(
                        "component source changed its topology family",
                    ));
                }
                build()
            })
        } else {
            build().map(Some)
        }
    }

    fn append_shell(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        region: &RegionId,
        shell_use: &TextShapeUse,
        parent: Located,
        reversed: bool,
    ) -> Result<Vec<ShellId>, CodecError> {
        let transform = self.compose(parent, self.location(shell_use.location)?)?;
        self.append_shell_shape(
            ctx,
            ir,
            region,
            shell_use.shape,
            transform,
            reversed ^ is_reversed(shell_use.orientation),
        )
    }

    fn append_shell_shape(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        region: &RegionId,
        shape_index: usize,
        transform: Located,
        reversed: bool,
    ) -> Result<Vec<ShellId>, CodecError> {
        let shape = self.shape(shape_index)?.clone();
        let key = self.topology_label(shape_index, transform)?;
        let shell_id = ShellId::mint(crate::native::model_id("shell", &self.payload.id, &key))
            .map_err(CodecError::malformed)?;
        if shape.kind() == TextShapeKind::Shell {
            let mut face_uses = Vec::new();
            face_uses.extend(shape.children.iter().filter(|child| {
                self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Face
            }));
            let components = self.face_components(ctx, &face_uses, transform)?;
            let mut shell_ids = Vec::new();
            for (component_index, component) in components.iter().enumerate() {
                let component_id = if component_index == 0 {
                    shell_id.clone()
                } else {
                    self.shell_component_id(&key, component_index)?
                };
                let mut faces = Vec::new();
                for &face_index in component {
                    if let Some(face) = self.append_face(
                        ir,
                        &component_id,
                        face_uses[face_index],
                        transform,
                        reversed,
                    )? {
                        faces.push(face);
                    }
                }
                if faces.is_empty() {
                    continue;
                }
                let shell = self.component(component_id.as_str(), shape_index, || {
                    Shell::new(
                        component_id.clone(),
                        region.clone(),
                        faces,
                        Vec::new(),
                        Vec::new(),
                    )
                    .map(|mut shell| {
                        shell.native_ref =
                            self.payload.row_identity(7, shape_index).map(str::to_owned);
                        shell
                    })
                    .map_err(|message| cadmpeg_core::CodecError::Malformed(message.to_string()))
                })?;
                let Some(shell) = shell else {
                    continue;
                };
                ir.model.shells.push(shell);
                self.bind_topology(
                    TextShapeKind::Shell,
                    shape_index,
                    transform,
                    component_id.as_str(),
                )?;
                shell_ids.push(component_id);
            }
            return Ok(shell_ids);
        }
        let mut faces = Vec::new();
        let mut wire_edges = Vec::new();
        match shape.kind() {
            TextShapeKind::Face => {
                let shape_use = TextShapeUse {
                    shape: shape_index,
                    orientation: TextOrientation::Forward,
                    location: 0.into(),
                };
                if let Some(face) =
                    self.append_face(ir, &shell_id, &shape_use, transform, reversed)?
                {
                    faces.push(face);
                }
            }
            TextShapeKind::Wire => {
                for child in &shape.children {
                    if self.shape(child.shape)?.kind() == TextShapeKind::Edge {
                        let edge = self.ensure_edge(ir, child, transform)?;
                        wire_edges.extend(edge.map(|edge| edge.id));
                    }
                }
            }
            TextShapeKind::Edge => {
                let edge_use = TextShapeUse {
                    shape: shape_index,
                    orientation: if reversed {
                        TextOrientation::Reversed
                    } else {
                        TextOrientation::Forward
                    },
                    location: 0.into(),
                };
                let edge = self.ensure_edge(ir, &edge_use, transform)?;
                wire_edges.extend(edge.map(|edge| edge.id));
            }
            TextShapeKind::Vertex => {
                let vertex_use = TextShapeUse {
                    shape: shape_index,
                    orientation: TextOrientation::Forward,
                    location: 0.into(),
                };
                let vertex = self.ensure_vertex(ir, &vertex_use, transform, None)?;
                let shell = self.component(shell_id.as_str(), shape_index, || {
                    Shell::new(
                        shell_id.clone(),
                        region.clone(),
                        faces,
                        wire_edges,
                        vec![vertex],
                    )
                    .map(|mut shell| {
                        shell.native_ref =
                            self.payload.row_identity(7, shape_index).map(str::to_owned);
                        shell
                    })
                    .map_err(|message| cadmpeg_core::CodecError::Malformed(message.to_string()))
                })?;
                let Some(shell) = shell else {
                    return Ok(Vec::new());
                };
                ir.model.shells.push(shell);
                return Ok(vec![shell_id]);
            }
            _ => {}
        }
        if faces.is_empty() && wire_edges.is_empty() {
            return Ok(Vec::new());
        }
        let shell = self.component(shell_id.as_str(), shape_index, || {
            Shell::new(
                shell_id.clone(),
                region.clone(),
                faces,
                wire_edges,
                Vec::new(),
            )
            .map(|mut shell| {
                shell.native_ref = self.payload.row_identity(7, shape_index).map(str::to_owned);
                shell
            })
            .map_err(|message| cadmpeg_core::CodecError::Malformed(message.to_string()))
        })?;
        let Some(shell) = shell else {
            return Ok(Vec::new());
        };
        ir.model.shells.push(shell);
        if shape.kind() == TextShapeKind::Wire {
            self.bind_topology(shape.kind(), shape_index, transform, shell_id.as_str())?;
        }
        Ok(vec![shell_id])
    }

    fn face_components(
        &self,
        ctx: &DecodeContext<'_>,
        face_uses: &[&TextShapeUse],
        parent: Located,
    ) -> Result<Vec<Vec<usize>>, CodecError> {
        if face_uses.is_empty() {
            return Ok(vec![Vec::new()]);
        }
        let mut connectivity = Vec::new();
        for face_use in face_uses {
            let face_transform = self.compose(parent, self.location(face_use.location)?)?;
            let face = self.shape(face_use.shape)?;
            let mut keys = HashSet::new();
            for wire_use in face
                .children
                .iter()
                .filter(|child| self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Wire)
            {
                let wire_transform =
                    self.compose(face_transform, self.location(wire_use.location)?)?;
                let wire = self.shape(wire_use.shape)?;
                for edge_use in wire.children.iter().filter(|child| {
                    self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Edge
                }) {
                    let edge_transform =
                        self.compose(wire_transform, self.location(edge_use.location)?)?;
                    let edge_key = OccurrenceKey::new(
                        edge_use.shape,
                        self.compose(self.body_scope, edge_transform)?,
                    );
                    let key = format!("edge:{}", edge_key.0);
                    keys.insert(key);
                    let edge = self.shape(edge_use.shape)?;
                    for vertex_use in edge.children.iter().filter(|child| {
                        self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Vertex
                    }) {
                        let vertex_transform =
                            self.compose(edge_transform, self.location(vertex_use.location)?)?;
                        let vertex_key = OccurrenceKey::new(
                            vertex_use.shape,
                            self.compose(self.body_scope, vertex_transform)?,
                        );
                        let key = format!("vertex:{}", vertex_key.0);
                        keys.insert(key);
                    }
                }
            }
            connectivity.push(keys);
        }

        connected_components(ctx, &connectivity)
    }

    fn append_face(
        &mut self,
        ir: &mut CadIr,
        shell: &ShellId,
        face_use: &TextShapeUse,
        parent: Located,
        reversed: bool,
    ) -> Result<Option<FaceId>, CodecError> {
        let face_transform = self.compose(parent, self.location(face_use.location)?)?;
        let face_reversed = reversed ^ is_reversed(face_use.orientation);
        let shape = self.shape(face_use.shape)?.clone();
        let TextTShapeGeometry::Face {
            surface,
            location,
            triangulation,
            ..
        } = shape.geometry
        else {
            return Ok(None);
        };
        let surface_transform = self.compose(face_transform, self.location(location)?)?;
        let face_key = self.topology_label(face_use.shape, face_transform)?;
        let face_id = FaceId::mint(crate::native::model_id("face", &self.payload.id, &face_key))
            .map_err(CodecError::malformed)?;
        // OCCT triangulation nodes are already expressed in the face's surface-location frame.
        // Only the owning topological face placement remains to be applied here.
        let located_triangulation = triangulation
            .map(|index| {
                let triangulation = index.resolve(self.tables.triangulations)?;
                let index = index.index();
                Ok::<_, CodecError>((index, triangulation))
            })
            .transpose()?;
        // A left-handed OCCT frame turns the neutral surface normal around, so
        // the face's sense against it turns too; its wires keep their sense.
        let mut normal_reversed = false;
        let surface_id = if let Some(surface) = surface {
            normal_reversed = surface.resolve(self.tables.surfaces)?.normal_reversed();
            let mut support = surface.resolve(self.tables.surfaces)?;
            let mut trimmed_layers = 0;
            while let TextSurface::Trimmed { basis, .. } = support {
                trimmed_layers += 1;
                support = basis.surface();
            }
            if trimmed_layers == 0 {
                self.located_surface(
                    ir,
                    surface.index(),
                    face_use.shape,
                    surface_transform.transform,
                )?
            } else {
                self.located_surface_layers(
                    ir,
                    surface.index(),
                    face_use.shape,
                    trimmed_layers,
                    surface_transform.transform,
                )?
            }
        } else if let Some((index, triangulation)) = &located_triangulation {
            let key = format!("triangulation:{index}@{face_key}");
            let id = SurfaceId::mint(crate::native::model_id("surface", &self.payload.id, &key))
                .map_err(CodecError::malformed)?;
            let new_surface = !self.emitted_surfaces.contains(&id);
            if new_surface {
                self.emitted_surfaces.insert(
                    SurfaceId::mint(id.as_str().to_owned()).map_err(CodecError::malformed)?,
                );

                let build = |value: crate::brep::triangulation::TextTriangulation,
                             matrix: Transform| {
                    let vertices = value
                        .nodes()
                        .iter()
                        .map(|point| {
                            matrix.apply_point(point.get()).ok_or_else(|| {
                                CodecError::malformed(
                                    "placed triangulation contains a non-finite point",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(Surface {
                        native_ref: self.payload.row_identity(6, *index).map(str::to_owned),
                        id: id.clone(),
                        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(
                            PolygonalSurface::from_admitted_scaled_deflection(
                                vertices,
                                value.triangles().to_vec(),
                                value.deflection,
                                uniform_scale(matrix)?,
                            )
                            .map_err(CodecError::malformed)?,
                        )),
                        source_object: self.source_association()?.map(|mut source| {
                            source.geometry_role = Some(cadmpeg_ir::SourceGeometryRole::Support);
                            source
                        }),
                    })
                };
                let surface = if let Some(sources) = self.sources {
                    sources.triangulation(
                        self.ctx,
                        id.as_str(),
                        self.payload,
                        *index,
                        Some((
                            face_use.shape,
                            face_transform.transform,
                            self.root_discriminator,
                        )),
                        build,
                    )?
                } else {
                    build((*triangulation).clone(), face_transform.transform)?
                };
                ir.model.surfaces.push(surface);
            }
            id
        } else {
            return Ok(None);
        };
        if let Some((index, triangulation)) = located_triangulation {
            self.emitted_triangulations.insert(index);
            let tessellation_key = format!("{index}@{face_key}");
            let id = cadmpeg_ir::tessellation::TessellationId::mint(crate::native::model_id(
                "tessellation",
                &self.payload.id,
                &tessellation_key,
            ))
            .map_err(CodecError::malformed)?;
            let build = |value: crate::brep::triangulation::TextTriangulation,
                         matrix: Transform| {
                self.triangulation_value(
                    id.clone(),
                    &value,
                    index,
                    matrix,
                    vec![face_id.clone()],
                    self.current_body.clone(),
                )
            };
            let mesh = if let Some(sources) = self.sources {
                sources.triangulation(
                    self.ctx,
                    id.as_str(),
                    self.payload,
                    index,
                    Some((
                        face_use.shape,
                        face_transform.transform,
                        self.root_discriminator,
                    )),
                    build,
                )?
            } else {
                build(triangulation.clone(), face_transform.transform)?
            };
            ir.model.tessellations.push(mesh);
        }
        let mut loops = Vec::new();
        for (loop_index, wire_use) in shape
            .children
            .iter()
            .filter(|child| self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Wire)
            .enumerate()
        {
            let loop_key = format!("{}:{}", face_key, loop_index + 1);
            let loop_id =
                LoopId::mint(crate::native::model_id("loop", &self.payload.id, &loop_key))
                    .map_err(CodecError::malformed)?;
            let expected = self.compose(face_transform, self.location(wire_use.location)?)?;
            let ordinal = self.root_discriminator;
            let sources = self.sources;
            let ctx = self.ctx;
            let payload = self.payload;
            let cached = if sources.is_none() {
                Some(self.shape(wire_use.shape)?.clone())
            } else {
                None
            };
            let mut build = |wire: crate::brep::TextTShape, matrix: Transform| {
                self.append_wire_loop(
                    ir,
                    &face_id,
                    &face_key,
                    loop_index,
                    loop_id.clone(),
                    wire_use,
                    Located {
                        transform: matrix,
                        ..expected
                    },
                    face_reversed,
                    surface,
                    surface_transform,
                    wire,
                )
            };
            let value = if let Some(sources) = sources {
                sources.topology_placed(
                    ctx,
                    loop_id.as_str(),
                    payload,
                    wire_use.shape,
                    expected.transform,
                    ordinal,
                    build,
                )?
            } else {
                build(
                    cached.ok_or_else(|| {
                        CodecError::malformed("wire producer has no typed source")
                    })?,
                    expected.transform,
                )?
            };
            if let Some(loop_id) = value {
                loops.push(loop_id);
            } else if let Some(authority) = ctx.record_capture_authority(loop_id.as_str())? {
                ctx.retire_record_capture(&authority)?;
            }
        }
        let build = |shape: crate::brep::TextTShape| {
            let TextTShapeGeometry::Face { tolerance, .. } = shape.geometry else {
                return Err(CodecError::malformed(
                    "face producer requires a face source",
                ));
            };
            Ok(Face {
                native_ref: Some(self.payload.id.clone()),
                id: face_id.clone(),
                shell: shell.clone(),
                surface: surface_id,
                sense: sense(face_reversed ^ normal_reversed),
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
                name: None,
                color: None,
                tolerance: PositiveReal::from_finite(tolerance),
            })
        };
        let face = if let Some(sources) = self.sources {
            sources.topology(
                self.ctx,
                face_id.as_str(),
                self.payload,
                face_use.shape,
                build,
            )?
        } else {
            build(self.shape(face_use.shape)?.clone())?
        };
        ir.model.faces.push(face);
        self.bind_topology(
            TextShapeKind::Face,
            face_use.shape,
            face_transform,
            face_id.as_str(),
        )?;
        Ok(Some(face_id))
    }

    fn append_wire_loop(
        &mut self,
        ir: &mut CadIr,
        face_id: &FaceId,
        face_key: &str,
        loop_index: usize,
        loop_id: LoopId,
        wire_use: &TextShapeUse,
        wire_transform: Located,
        face_reversed: bool,
        surface: Option<crate::brep::TableRef<crate::brep::TextSurface>>,
        surface_transform: Located,
        wire: crate::brep::TextTShape,
    ) -> Result<Option<LoopId>, CodecError> {
        let mut edge_uses = Vec::new();
        edge_uses.extend(
            wire.children
                .iter()
                .filter(|child| self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Edge)
                .cloned(),
        );
        let wire_reversed = face_reversed ^ is_reversed(wire_use.orientation);
        if wire_reversed {
            edge_uses.reverse();
        }
        if edge_uses.is_empty() {
            return Ok(None);
        }
        let mut admitted_edges = Vec::new();
        for (index, edge_use) in edge_uses.iter().enumerate() {
            let expected = self.compose(wire_transform, self.location(edge_use.location)?)?;
            let available = self
                .sources
                .map(|sources| {
                    sources.edge_carrier_available(
                        self.payload,
                        edge_use.shape,
                        expected.transform,
                        self.root_discriminator,
                    )
                })
                .transpose()?
                .unwrap_or(true);
            let edge = self.ensure_edge(ir, edge_use, wire_transform)?;
            if let (true, Some(edge)) = (available, edge) {
                admitted_edges.push((index, edge));
            }
        }
        if admitted_edges.is_empty() {
            return Ok(None);
        }
        let mut coedge_ids = Vec::new();
        for (index, _) in &admitted_edges {
            let key = format!("{}:{}:{}", face_key, loop_index + 1, index + 1);
            coedge_ids.push(
                CoedgeId::mint(crate::native::model_id("coedge", &self.payload.id, &key))
                    .map_err(CodecError::malformed)?,
            );
        }
        let mut placed_uses = Vec::new();
        for ((index, edge), id) in admitted_edges.into_iter().zip(coedge_ids.iter()) {
            let edge_use = &edge_uses[index];
            let edge_transform = self.compose(wire_transform, self.location(edge_use.location)?)?;
            let pcurve = self.face_pcurve(edge_use, edge_transform, surface, surface_transform)?;
            let id = id.clone();
            let build = |wire: crate::brep::TextTShape| {
                let mut uses = wire
                    .children
                    .into_iter()
                    .filter(|child| {
                        self.tables.tshapes[child.shape - 1].kind() == TextShapeKind::Edge
                    })
                    .collect::<Vec<_>>();
                if wire_reversed {
                    uses.reverse();
                }
                let actual = uses
                    .get(index)
                    .ok_or_else(|| CodecError::malformed("coedge source use is absent"))?;
                Ok(Coedge { native_ref: self.payload.row_identity(7,wire_use.shape).map(str::to_owned),
                    id: id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge.id.clone(),
                    radial_next: id.clone(),
                    sense: sense(is_reversed(actual.orientation) ^ wire_reversed),
                    use_curve: None,
                    pcurves: pcurve
                        .into_iter()
                        .map(|(pcurve, parameter_range)| {
                            Ok(cadmpeg_ir::topology::PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range: (parameter_range)
                                    .map(cadmpeg_ir::geometry::DirectedParameterRange::from_finite_endpoints)
                                    .transpose()
                                    .map_err(CodecError::malformed)?,
                            })
                        })
                        .collect::<Result<Vec<_>, CodecError>>()?,
                    })
            };
            let coedge = if let Some(sources) = self.sources {
                sources.topology(self.ctx, id.as_str(), self.payload, wire_use.shape, build)?
            } else {
                build(self.shape(wire_use.shape)?.clone())?
            };
            let use_sense = coedge.sense;
            ir.model.coedges.push(coedge);
            placed_uses.push((edge, use_sense));
        }
        let ends = placed_uses
            .iter()
            .map(|(edge, use_sense)| edge.oriented_ends(*use_sense))
            .collect::<Vec<_>>();
        let ring = ring_order::traversal_order(&ends)
            .into_iter()
            .map(|position| coedge_ids[position].clone())
            .collect();
        ir.model.loops.push(Loop {
            native_ref: self
                .payload
                .row_identity(7, wire_use.shape)
                .map(str::to_owned),
            id: loop_id.clone(),
            face: face_id.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(ring, Vec::new()).map_err(|error| {
                    CodecError::malformed(format_args!(
                        "FCStd face {} loop {} has invalid ring: {error}",
                        face_id,
                        loop_index + 1,
                    ))
                })?,
            ),
        });
        self.bind_topology(
            TextShapeKind::Wire,
            wire_use.shape,
            wire_transform,
            loop_id.as_str(),
        )?;
        Ok(Some(loop_id))
    }

    fn ensure_edge(
        &mut self,
        ir: &mut CadIr,
        edge_use: &TextShapeUse,
        parent: Located,
    ) -> Result<Option<PlacedEdge>, CodecError> {
        let transform = self.compose(parent, self.location(edge_use.location)?)?;
        let key = OccurrenceKey::new(edge_use.shape, self.compose(self.body_scope, transform)?);
        if let Some(placed) = self.edges.get(&key) {
            let placed = placed.clone();
            self.bind_topology(
                TextShapeKind::Edge,
                edge_use.shape,
                transform,
                placed.id.as_str(),
            )?;
            return Ok(Some(placed));
        }
        let label = self.topology_label(edge_use.shape, transform)?;
        let id = EdgeId::mint(crate::native::model_id("edge", &self.payload.id, &label))
            .map_err(CodecError::malformed)?;
        if let Some(sources) = self.sources {
            sources.edge(
                self.ctx,
                id.as_str(),
                self.payload,
                edge_use.shape,
                transform.transform,
                self.root_discriminator,
                |input| {
                    self.append_edge_value(ir, edge_use, key, id.clone(), transform.identity, input)
                },
            )
        } else {
            let shape = self.shape(edge_use.shape)?.clone();
            self.append_edge_value(
                ir,
                edge_use,
                key,
                id,
                transform.identity,
                crate::brep::source::projection::EdgeInputs {
                    shape,
                    transform: transform.transform,
                    curve: None,
                    carrier_available: true,
                },
            )
        }
    }

    fn append_edge_value(
        &mut self,
        ir: &mut CadIr,
        edge_use: &TextShapeUse,
        key: OccurrenceKey,
        id: EdgeId,
        location_identity: usize,
        input: crate::brep::source::projection::EdgeInputs,
    ) -> Result<Option<PlacedEdge>, CodecError> {
        let crate::brep::source::projection::EdgeInputs {
            shape,
            transform,
            curve: original_curve,
            carrier_available,
        } = input;
        let transform = Located {
            transform,
            identity: location_identity,
        };
        let TextTShapeGeometry::Edge {
            tolerance,
            degenerated,
            representations,
            ..
        } = shape.geometry
        else {
            return Err(CodecError::malformed(format_args!(
                "TShape {} is not an edge",
                edge_use.shape
            )));
        };
        let (start_use, end_use) = edge_endpoint_uses(edge_use.shape, &shape.children)?;
        if !carrier_available {
            for vertex_use in [start_use, end_use] {
                let matrix = self.compose(transform, self.location(vertex_use.location)?)?;
                self.ensure_point(ir, vertex_use, matrix)?;
            }
            return Ok(None);
        }
        let start = self.ensure_vertex(ir, start_use, transform, Some(edge_use.shape))?;
        let end = self.ensure_vertex(ir, end_use, transform, Some(edge_use.shape))?;
        let curve_representation =
            select_exact_curve_representation(edge_use.shape, &representations, &self.tables)?;
        let polygon_representation = if curve_representation.is_none() {
            unique_fallback_polygon_representation(edge_use.shape, &representations)?
        } else {
            None
        };
        let curve = if degenerated {
            None
        } else if let Some((
            _,
            TextEdgeRepresentation::Curve3d {
                curve, location, ..
            },
        )) = curve_representation
        {
            let carrier_transform = self.compose(transform, self.location(*location)?)?;
            Some(self.located_curve(ir, *curve, edge_use.shape, carrier_transform.transform)?)
        } else if let Some((ordinal, representation)) = polygon_representation {
            Some(self.polygon_curve(ir, &id, edge_use.shape, ordinal, representation, transform)?)
        } else {
            None
        };
        let param_range = curve_representation
            .and_then(|(_, representation)| representation.parameter_range())
            .or_else(|| {
                polygon_representation.and_then(|(_, representation)| {
                    self.polygon_parameters(representation)
                        .and_then(|parameters| Some([*parameters.first()?, *parameters.last()?]))
                })
            });
        let original_geometry = original_curve
            .as_ref()
            .map(|value| crate::brep::curve_geometry(self.ctx, value))
            .transpose()?;
        let geometry = original_geometry.as_ref().or_else(|| {
            curve
                .as_ref()
                .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id))
                .map(|curve| &curve.geometry)
        });
        let subset_range = curve_representation
            .and_then(|(_, representation)| match representation {
                TextEdgeRepresentation::Curve3d { curve, .. } => Some(*curve),
                _ => None,
            })
            .and_then(|source| {
                let base = CurveId::mint(crate::native::model_id(
                    "curve",
                    &self.payload.id,
                    self.payload.source_index(2, source).to_string(),
                ))
                .ok()?;
                occt_subset_local_range(ir, &base, param_range?)
            });
        let param_range = subset_range.or_else(|| {
            geometry.map_or(param_range, |geometry| {
                normalize_occt_curve_range(geometry.solved()?, param_range)
            })
        });
        let placed = PlacedEdge {
            id: id.clone(),
            start: start.clone(),
            end: end.clone(),
        };
        ir.model.edges.push(Edge {
            native_ref: self
                .payload
                .row_identity(7, edge_use.shape)
                .map(str::to_owned),
            id: id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::from_finite_parts(curve, param_range)
                .map_err(CodecError::malformed)?,
            start,
            end,
            tolerance: PositiveReal::from_finite(tolerance),
        });
        self.bind_topology(TextShapeKind::Edge, edge_use.shape, transform, id.as_str())?;
        self.edges.insert(key, placed.clone());
        Ok(Some(placed))
    }

    fn polygon_curve(
        &mut self,
        ir: &mut CadIr,
        edge: &EdgeId,
        shape: usize,
        ordinal: usize,
        representation: &TextEdgeRepresentation,
        transform: Located,
    ) -> Result<CurveId, CodecError> {
        let id = Self::polygon_curve_id(edge, ordinal, false)?;
        let secondary = matches!(representation, TextEdgeRepresentation::PolygonPair { .. });
        for second in [false, true]
            .into_iter()
            .take(if secondary { 2 } else { 1 })
        {
            let id = Self::polygon_curve_id(edge, ordinal, second)?;
            let (table, polygon) = match representation {
                TextEdgeRepresentation::Polygon3d { polygon, .. } => (3, *polygon),
                TextEdgeRepresentation::PolygonOnTriangulation { polygon, .. } => (4, *polygon),
                TextEdgeRepresentation::PolygonPair { polygons, .. } => {
                    (4, polygons[usize::from(second)])
                }
                _ => {
                    return Err(CodecError::malformed(
                        "non-polygon representation reached polygon transfer",
                    ))
                }
            };
            let build = |input: crate::brep::source::projection::polygon::PolygonInputs| {
                let IndexedPolygon {
                    mut samples,
                    deflection,
                } = IndexedPolygon::try_new(input.nodes, input.parameters, input.deflection)?;
                place_polyline_samples(&mut samples, input.transform)?;
                Ok(Curve {
                    native_ref: self.payload.row_identity(table, polygon).map(str::to_owned),
                    parameter_range: None,
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
                        PolylineCurve::from_scaled_deflection(
                            samples,
                            deflection,
                            uniform_scale(input.transform)?,
                        )
                        .map_err(CodecError::malformed)?,
                    )),
                    source_object: self.source_association()?,
                })
            };
            let curve = if let Some(sources) = self.sources {
                sources.polygon(
                    self.ctx,
                    id.as_str(),
                    self.payload,
                    crate::brep::source::projection::polygon::PolygonUse {
                        shape,
                        representation: ordinal,
                        table,
                        polygon,
                        secondary: second,
                        expected: transform.transform,
                        root: self.root_discriminator,
                    },
                    build,
                )?
            } else {
                let (nodes, parameters, deflection) = if table == 3 {
                    let value = &self.tables.polygons3d[polygon - 1];
                    (
                        value.nodes.clone(),
                        value.parameters.clone(),
                        value.deflection,
                    )
                } else {
                    let index = match representation {
                        TextEdgeRepresentation::PolygonOnTriangulation {
                            triangulation, ..
                        }
                        | TextEdgeRepresentation::PolygonPair { triangulation, .. } => {
                            *triangulation
                        }
                        _ => {
                            return Err(CodecError::malformed("indexed polygon has no mesh source"))
                        }
                    };
                    let value = &self.tables.polygons_on_triangulations[polygon - 1];
                    let mesh = &self.tables.triangulations[index - 1];
                    let nodes = value
                        .nodes
                        .iter()
                        .map(|index| {
                            let offset = usize::try_from(*index)
                                .ok()
                                .and_then(|index| index.checked_sub(1))
                                .ok_or_else(|| {
                                    CodecError::malformed("indexed polygon node is invalid")
                                })?;
                            mesh.nodes().get(offset).copied().ok_or_else(|| {
                                CodecError::malformed(
                                    "indexed polygon node is outside its triangulation",
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    (nodes, value.parameters.clone(), value.deflection)
                };
                build(crate::brep::source::projection::polygon::PolygonInputs {
                    nodes,
                    parameters,
                    deflection,
                    transform: self
                        .compose(transform, self.location(representation.location())?)?
                        .transform,
                })?
            };
            ir.model.curves.push(curve);
        }
        Ok(id)
    }

    fn polygon_curve_id(
        edge: &EdgeId,
        ordinal: usize,
        secondary: bool,
    ) -> Result<CurveId, CodecError> {
        let id = if secondary {
            format!("{}:polygon:{}:secondary", edge.as_str(), ordinal + 1)
        } else {
            format!("{}:polygon:{}", edge.as_str(), ordinal + 1)
        };
        CurveId::mint(id).map_err(CodecError::malformed)
    }

    fn polygon_parameters(&self, representation: &TextEdgeRepresentation) -> Option<&[FiniteReal]> {
        match representation {
            TextEdgeRepresentation::Polygon3d { polygon, .. } => {
                self.tables.polygons3d[polygon - 1].parameters.as_deref()
            }
            TextEdgeRepresentation::PolygonOnTriangulation { polygon, .. } => {
                self.tables.polygons_on_triangulations[polygon - 1]
                    .parameters
                    .as_deref()
            }
            TextEdgeRepresentation::PolygonPair { polygons, .. } => {
                self.tables.polygons_on_triangulations[polygons[0] - 1]
                    .parameters
                    .as_deref()
            }
            _ => None,
        }
    }

    fn ensure_vertex(
        &mut self,
        ir: &mut CadIr,
        vertex_use: &TextShapeUse,
        parent: Located,
        parent_edge: Option<usize>,
    ) -> Result<VertexId, CodecError> {
        let transform = self.compose(parent, self.location(vertex_use.location)?)?;
        let key = OccurrenceKey::new(vertex_use.shape, self.compose(self.body_scope, transform)?);
        if let Some(id) = self.vertices.get(&key) {
            let id: VertexId = id.clone();
            self.bind_topology(
                TextShapeKind::Vertex,
                vertex_use.shape,
                transform,
                id.as_str(),
            )?;
            return Ok(id);
        }
        let label = self.topology_label(vertex_use.shape, transform)?;
        let vertex_id = VertexId::mint(crate::native::model_id("vertex", &self.payload.id, &label))
            .map_err(CodecError::malformed)?;
        if let Some(sources) = self.sources {
            sources.topology_placed(
                self.ctx,
                vertex_id.as_str(),
                self.payload,
                vertex_use.shape,
                transform.transform,
                self.root_discriminator,
                |shape, matrix| {
                    if let Some(edge) = parent_edge {
                        if !sources.edge_carrier_available(
                            self.payload,
                            edge,
                            parent.transform,
                            self.root_discriminator,
                        )? {
                            return Err(CodecError::malformed(
                                "vertex requires an admitted edge use",
                            ));
                        }
                    }
                    self.append_vertex_value(
                        ir,
                        vertex_use,
                        Located {
                            transform: matrix,
                            ..transform
                        },
                        key,
                        vertex_id.clone(),
                        shape,
                    )
                },
            )
        } else {
            let shape = self.shape(vertex_use.shape)?.clone();
            self.append_vertex_value(ir, vertex_use, transform, key, vertex_id, shape)
        }
    }

    fn append_vertex_value(
        &mut self,
        ir: &mut CadIr,
        vertex_use: &TextShapeUse,
        transform: Located,
        key: OccurrenceKey,
        vertex_id: VertexId,
        shape: crate::brep::TextTShape,
    ) -> Result<VertexId, CodecError> {
        let TextTShapeGeometry::Vertex { tolerance, .. } = shape.geometry else {
            return Err(CodecError::malformed(format_args!(
                "TShape {} is not a vertex",
                vertex_use.shape
            )));
        };
        let point_id = self.ensure_point(ir, vertex_use, transform)?;
        ir.model.vertices.push(Vertex {
            native_ref: self
                .payload
                .row_identity(7, vertex_use.shape)
                .map(str::to_owned),
            id: vertex_id.clone(),
            point: point_id,
            tolerance: positive_tolerance(
                tolerance.get() * uniform_scale(transform.transform)?.get(),
            ),
        });
        self.bind_topology(
            TextShapeKind::Vertex,
            vertex_use.shape,
            transform,
            vertex_id.as_str(),
        )?;
        let cached_id =
            VertexId::mint(vertex_id.as_str().to_owned()).map_err(CodecError::malformed)?;
        self.vertices.insert(key, cached_id);
        Ok(vertex_id)
    }

    fn ensure_point(
        &self,
        ir: &mut CadIr,
        vertex_use: &TextShapeUse,
        transform: Located,
    ) -> Result<PointId, CodecError> {
        let label = self.topology_label(vertex_use.shape, transform)?;
        let point_id = PointId::mint(crate::native::model_id("point", &self.payload.id, &label))
            .map_err(CodecError::malformed)?;
        if ir.model.points.iter().any(|point| point.id == point_id) {
            return Ok(point_id);
        }
        let source = self.source_association()?;
        let build = |value: crate::brep::TextTShape, matrix: Transform| {
            let TextTShapeGeometry::Vertex { point, .. } = value.geometry else {
                return Err(CodecError::malformed(
                    "point producer requires a vertex source",
                ));
            };
            let position = matrix.apply_point(point.get()).ok_or_else(|| {
                CodecError::malformed("placed vertex position contains a non-finite coordinate")
            })?;
            Ok(
                Point::new(point_id.clone(), position, source).with_native_ref(
                    self.payload
                        .row_identity(7, vertex_use.shape)
                        .map(str::to_owned),
                ),
            )
        };
        let point = if let Some(sources) = self.sources {
            sources.topology_placed(
                self.ctx,
                point_id.as_str(),
                self.payload,
                vertex_use.shape,
                transform.transform,
                self.root_discriminator,
                build,
            )?
        } else {
            build(self.shape(vertex_use.shape)?.clone(), transform.transform)?
        };
        ir.model.points.push(point);
        Ok(point_id)
    }

    fn located_curve(
        &mut self,
        ir: &mut CadIr,
        source: usize,
        topology: usize,
        transform: Transform,
    ) -> Result<CurveId, CodecError> {
        let native_ref = self.payload.row_identity(2, source).map(str::to_owned);
        let dense = source;
        let source = self.payload.source_index(2, source);
        let base_key = format!("{source}");
        let base_id = CurveId::mint(crate::native::model_id(
            "curve",
            &self.payload.id,
            &base_key,
        ))
        .map_err(CodecError::malformed)?;
        if is_identity(transform) {
            return Ok(base_id);
        }
        let key = format!("{}@{}", source, transform_digest(transform));
        let id = CurveId::mint(crate::native::model_id("curve", &self.payload.id, &key))
            .map_err(CodecError::malformed)?;
        let cached_id = CurveId::mint(id.as_str().to_owned()).map_err(CodecError::malformed)?;
        if self.emitted_curves.insert(cached_id) {
            let base = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == base_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("missing curve table entry {source}"))
                })?;
            let geometry = if let Some(sources) = self.sources {
                sources.placed_curve(
                    self.ctx,
                    id.as_str(),
                    self.payload,
                    dense,
                    topology,
                    transform,
                    self.root_discriminator,
                    |value, matrix| {
                        transform_curve(
                            &crate::brep::local_curve_geometry(self.ctx, &value, &base_id)?,
                            matrix,
                        )
                    },
                )?
            } else {
                transform_curve(&base.geometry, transform)?
            };
            let mut source_object = base.source_object.clone();
            if let Some(source) = &mut source_object {
                source.geometry_role = Some(cadmpeg_ir::SourceGeometryRole::Support);
            }
            ir.model.curves.push(Curve {
                native_ref,
                parameter_range: None,
                id: id.clone(),
                geometry,
                source_object,
            });
        }
        Ok(id)
    }

    fn located_surface(
        &mut self,
        ir: &mut CadIr,
        source: usize,
        topology: usize,
        transform: Transform,
    ) -> Result<SurfaceId, CodecError> {
        self.located_surface_layers(ir, source, topology, 0, transform)
    }

    fn located_surface_layers(
        &mut self,
        ir: &mut CadIr,
        source: usize,
        topology: usize,
        trimmed_layers: usize,
        transform: Transform,
    ) -> Result<SurfaceId, CodecError> {
        let native_ref = self.payload.row_identity(5, source).map(str::to_owned);
        let dense = source;
        let source = self.payload.source_index(5, source);
        let mut base_key = format!("{source}");
        let mut base_id = SurfaceId::mint(crate::native::model_id(
            "surface",
            &self.payload.id,
            &base_key,
        ))
        .map_err(CodecError::malformed)?;
        for _ in 0..trimmed_layers {
            let wrapper = ir
                .model
                .surfaces
                .iter_mut()
                .find(|surface| surface.id == base_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("missing trimmed surface {base_id}"))
                })?;
            if let Some(association) = wrapper.source_object.as_mut() {
                association.geometry_role = Some(cadmpeg_ir::SourceGeometryRole::Support);
            }
            base_key.push_str(":basis");
            base_id = SurfaceId::mint(crate::native::model_id(
                "surface",
                base_id.as_str(),
                "basis",
            ))
            .map_err(CodecError::malformed)?;
        }
        if trimmed_layers > 0 {
            let support = ir
                .model
                .surfaces
                .iter_mut()
                .find(|surface| surface.id == base_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("missing face support surface {base_id}"))
                })?;
            if let Some(association) = support.source_object.as_mut() {
                association.geometry_role = None;
            }
        }
        if is_identity(transform) {
            return Ok(base_id);
        }
        let key = format!("{}@{}", base_key, transform_digest(transform));
        let id = SurfaceId::mint(crate::native::model_id("surface", &self.payload.id, &key))
            .map_err(CodecError::malformed)?;
        let cached_id = SurfaceId::mint(id.as_str().to_owned()).map_err(CodecError::malformed)?;
        if self.emitted_surfaces.insert(cached_id) {
            let base = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == base_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("missing surface table entry {source}"))
                })?;
            let geometry = if let Some(sources) = self.sources {
                sources.placed_surface(
                    self.ctx,
                    id.as_str(),
                    self.payload,
                    dense,
                    topology,
                    transform,
                    self.root_discriminator,
                    |value, matrix| {
                        let mut support = &value;
                        for _ in 0..trimmed_layers {
                            let TextSurface::Trimmed { basis, .. } = support else {
                                return Err(CodecError::malformed(
                                    "face support source lacks its trimmed layer",
                                ));
                            };
                            support = basis.surface();
                        }
                        transform_surface(
                            &crate::brep::surface_geometry(self.ctx, support)?,
                            matrix,
                        )
                    },
                )?
            } else {
                transform_surface(&base.geometry, transform)?
            };
            let mut source_object = base.source_object.clone();
            if let Some(source) = &mut source_object {
                source.geometry_role = if trimmed_layers > 0 {
                    None
                } else {
                    Some(cadmpeg_ir::SourceGeometryRole::Support)
                };
            }
            let has_procedural_construction = base.geometry.procedural_construction().is_some();
            ir.model.surfaces.push(Surface {
                native_ref,
                id: id.clone(),
                geometry,
                source_object,
            });
            if has_procedural_construction {
                let construction_id = ProceduralSurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("fcstd", "model", "surface"),
                    id.key().colon(cadmpeg_ir::identity_key!("construction")),
                );
                let build = |matrix: Transform| {
                    let mut record = ProceduralSurface::new(
                        construction_id.clone(),
                        ProceduralSurfaceDefinition::Replica {
                            source: base_id.clone(),
                            transform: matrix,
                        },
                        None,
                    );
                    record.native_ref = self.sources.map(|_| self.payload.id.clone());
                    Ok(record)
                };
                let construction = if let Some(sources) = self.sources {
                    sources.placed_surface(
                        self.ctx,
                        construction_id.as_str(),
                        self.payload,
                        dense,
                        topology,
                        transform,
                        self.root_discriminator,
                        |value, matrix| {
                            if !matches!(
                                value,
                                TextSurface::Trimmed { .. }
                                    | TextSurface::Offset { .. }
                                    | TextSurface::Extrusion { .. }
                                    | TextSurface::Revolution { .. }
                            ) {
                                return Err(CodecError::malformed(
                                    "placed surface construction requires its procedural source",
                                ));
                            }
                            build(matrix)
                        },
                    )?
                } else {
                    build(transform)?
                };
                let admission = match self.procedural_admission.take() {
                    Some(admission) => admission,
                    None => cadmpeg_ir::document::procedural::ProceduralAdmission::new(&ir.model),
                };
                self.procedural_admission
                    .insert(admission)
                    .add_surface(&mut ir.model, &id.clone(), construction)?
                    .map_err(|error| CodecError::malformed(error.to_string()))?;
            }
        }
        Ok(id)
    }

    fn face_pcurve(
        &mut self,
        edge_use: &TextShapeUse,
        edge_transform: Located,
        surface: Option<crate::brep::TableRef<TextSurface>>,
        surface_transform: Located,
    ) -> Result<Option<FacePcurve>, CodecError> {
        let Some(surface) = surface else {
            return Ok(None);
        };
        let TextTShapeGeometry::Edge {
            degenerated,
            representations,
            ..
        } = &self.tables.tshapes[edge_use.shape - 1].geometry
        else {
            return Ok(None);
        };
        let selection = select_pcurve_representation(
            representations,
            &self.tables,
            &self.locations,
            edge_transform,
            surface.index(),
            surface_transform,
        )?;
        let (index, representation) = match selection {
            PcurveSelection::Missing => return Ok(None),
            PcurveSelection::Selected(index, representation) => (index, representation),
            PcurveSelection::AmbiguousLocation => {
                self.losses
                    .push(FreecadLossCode::PcurveLocationAmbiguous.note(format!(
                    "edge TShape {} has equal-matrix pcurves but no matching native location chain",
                    edge_use.shape,
                )));
                return Ok(None);
            }
        };
        let reversed = is_reversed(edge_use.orientation);
        let (curve_index, parameter_range, secondary) = match representation {
            TextEdgeRepresentation::Pcurve {
                curve,
                parameter_range,
                ..
            } => (*curve, *parameter_range, false),
            TextEdgeRepresentation::PcurvePair {
                curves,
                parameter_range,
                ..
            } => {
                let secondary = reversed;
                (
                    if secondary { curves[1] } else { curves[0] },
                    *parameter_range,
                    secondary,
                )
            }
            _ => return Ok(None),
        };
        let read = match pcurve_geometry(self.ctx, &self.tables.curve2ds[curve_index - 1]) {
            Ok(geometry) => geometry,
            Err(PcurveGeometryError::Resource(error)) => return Err(error),
            Err(error) => {
                self.losses
                    .push(pcurve_loss(&self.payload.id, curve_index, Some(&error)));
                return Ok(None);
            }
        };
        let Some(geometry) = read else {
            self.losses
                .push(pcurve_loss(&self.payload.id, curve_index, None));
            return Ok(None);
        };
        let parameter_range = normalize_pcurve_parameter_range(&geometry, Some(parameter_range));
        Ok(Some((
            self.pcurve_id(edge_use.shape, index, secondary)?,
            bounded_pcurve_range(*degenerated, parameter_range),
        )))
    }

    /// The kind of the topology under `index`: a solid whose shells are all
    /// closed is `Solid`, a solid with an open shell is `Sheet`, and a solid
    /// with no shell or a non-shell child is `General`. A compound or compsolid
    /// has the kind its member shapes share, or `General` when they differ.
    /// `None` for a container with no members. Each distinct shape is visited
    /// once.
    fn body_kind(&self, index: usize) -> Result<Option<BodyKind>, CodecError> {
        let mut shared = None;
        let mut pending = vec![(index, Located::identity())];
        let mut visited = std::collections::HashSet::new();
        while let Some((index, local)) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            let shape = self.shape(index)?;
            let kind = match shape.kind() {
                // A solid bounds a volume only when every shell it owns is
                // closed; an open one leaves a sheet.
                TextShapeKind::Solid => {
                    let mut kind = (!shape.children.is_empty()).then_some(BodyKind::Solid);
                    for child in &shape.children {
                        if self.shape(child.shape)?.kind() != TextShapeKind::Shell {
                            kind = None;
                            break;
                        }
                        let shell = self.compose(local, self.location(child.location)?)?;
                        if !self.shell_closed(child.shape, shell)? {
                            kind = Some(BodyKind::Sheet);
                        }
                    }
                    kind.unwrap_or(BodyKind::General)
                }
                TextShapeKind::Wire | TextShapeKind::Edge => BodyKind::Wire,
                TextShapeKind::Shell | TextShapeKind::Face => BodyKind::Sheet,
                TextShapeKind::Vertex => BodyKind::General,
                TextShapeKind::Compound | TextShapeKind::CompSolid => {
                    for child in &shape.children {
                        pending.push((
                            child.shape,
                            self.compose(local, self.location(child.location)?)?,
                        ));
                    }
                    continue;
                }
            };
            match shared {
                None => shared = Some(kind),
                Some(existing) if existing == kind => {}
                Some(_) => return Ok(Some(BodyKind::General)),
            }
        }
        Ok(shared)
    }

    /// Whether the shell under `index` is closed: every non-degenerate edge
    /// occurrence of its faces' wires is used exactly twice within the shell.
    /// An occurrence is an edge shape at its composed face, wire, and edge
    /// location, so one shape placed at two locations is two edges.
    fn shell_closed(&self, index: usize, shell: Located) -> Result<bool, CodecError> {
        let mut uses = HashMap::<OccurrenceKey, u32>::new();
        for face_use in &self.shape(index)?.children {
            let face_transform = self.compose(shell, self.location(face_use.location)?)?;
            for wire_use in &self.shape(face_use.shape)?.children {
                let wire = self.shape(wire_use.shape)?;
                if wire.kind() != TextShapeKind::Wire {
                    continue;
                }
                let wire_transform =
                    self.compose(face_transform, self.location(wire_use.location)?)?;
                for edge_use in &wire.children {
                    if let TextTShapeGeometry::Edge {
                        degenerated: false, ..
                    } = self.shape(edge_use.shape)?.geometry
                    {
                        let edge_transform =
                            self.compose(wire_transform, self.location(edge_use.location)?)?;
                        let count = uses
                            .entry(OccurrenceKey::new(edge_use.shape, edge_transform))
                            .or_default();
                        *count = count.checked_add(1).ok_or_else(|| {
                            CodecError::malformed("shell edge use count overflows u32")
                        })?;
                    }
                }
            }
        }
        Ok(!uses.is_empty() && uses.values().all(|&count| count == 2))
    }

    fn shape(&self, index: usize) -> Result<&TextTShape, CodecError> {
        self.tables.tshapes.resolve(index)
    }

    fn topology_label(&self, shape: usize, local: Located) -> Result<String, CodecError> {
        let original_shape = shape
            .checked_sub(1)
            .and_then(|index| self.shape_indices.get(index))
            .copied()
            .unwrap_or(shape);
        let label = occurrence_label(original_shape, self.compose(self.body_scope, local)?);
        Ok(match self.root_discriminator {
            Some(ordinal) => format!("{label}~root{ordinal}"),
            None => label,
        })
    }
}

fn face_support_surface(mut surface: &TextSurface) -> &TextSurface {
    while let TextSurface::Trimmed { basis, .. } = surface {
        surface = basis.surface();
    }
    surface
}

fn bounded_pcurve_range(
    degenerated: bool,
    range: Option<[FiniteReal; 2]>,
) -> Option<[FiniteReal; 2]> {
    (!degenerated)
        .then_some(range)
        .flatten()
        .filter(|range| range[0] < range[1])
}

fn normalize_pcurve_parameter_range(
    geometry: &PcurveGeometry,
    range: Option<[FiniteReal; 2]>,
) -> Option<[FiniteReal; 2]> {
    let mut range = range?;
    let domain = match geometry {
        PcurveGeometry::Nurbs { nurbs } => {
            let degree = usize::try_from(nurbs.degree()).ok()?;
            [
                *nurbs.knots().get(degree)?,
                *nurbs
                    .knots()
                    .get(nurbs.knots().len().checked_sub(degree + 1)?)?,
            ]
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            parameter_range.endpoints()
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            return normalize_pcurve_parameter_range(basis, Some(range));
        }
        PcurveGeometry::Transformed(placed) => {
            return normalize_pcurve_parameter_range(placed.basis(), Some(range));
        }
        _ => return Some(range),
    };
    let scale = range
        .into_iter()
        .map(FiniteReal::get)
        .chain(domain)
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    // Snap neighborhoods must not overlap, even on a small or translated domain.
    let tolerance =
        (scale * EPS_TOPOLOGY_TRANSFER_GEOMETRY).min((0.25 * domain[1] - 0.25 * domain[0]).abs());
    for value in &mut range {
        if (domain[0]..=domain[1]).contains(&value.get()) {
            continue;
        }
        let lower_distance = (value.get() - domain[0]).abs();
        let upper_distance = (value.get() - domain[1]).abs();
        if lower_distance < upper_distance && lower_distance <= tolerance {
            *value = FiniteReal::new(domain[0])?;
        } else if upper_distance < lower_distance && upper_distance <= tolerance {
            *value = FiniteReal::new(domain[1])?;
        }
    }
    Some(range)
}

/// Partition faces into the classes of the transitive "shares a key" relation.
///
/// Components are ordered by their lowest face index, and each lists its faces
/// in ascending order.
fn connected_components(
    ctx: &DecodeContext<'_>,
    connectivity: &[HashSet<String>],
) -> Result<Vec<Vec<usize>>, CodecError> {
    let mut parent = ctx.alloc_filled(
        connectivity.len(),
        0_usize,
        "freecad connected-component parent",
    )?;
    for (face, slot) in parent.iter_mut().enumerate() {
        *slot = face;
    }
    let mut first_face_by_key = HashMap::<&str, usize>::new();
    for (face, keys) in connectivity.iter().enumerate() {
        for key in keys {
            match first_face_by_key.entry(key.as_str()) {
                std::collections::hash_map::Entry::Occupied(first) => {
                    let left = component_root(&mut parent, *first.get());
                    let right = component_root(&mut parent, face);
                    parent[left.max(right)] = left.min(right);
                }
                std::collections::hash_map::Entry::Vacant(first) => {
                    first.insert(face);
                }
            }
        }
    }
    let mut component_by_root = ctx.alloc_filled(
        connectivity.len(),
        None::<usize>,
        "freecad connected-component index",
    )?;
    let mut components = Vec::<Vec<usize>>::new();
    for face in 0..connectivity.len() {
        let root = component_root(&mut parent, face);
        let component = if let Some(component) = component_by_root[root] {
            component
        } else {
            let component = components.len();
            components.push(Vec::new());
            component_by_root[root] = Some(component);
            component
        };
        components[component].push(face);
    }
    Ok(components)
}

/// Root of `face` in a union-find forest, halving the path on the way.
fn component_root(parent: &mut [usize], mut face: usize) -> usize {
    while parent[face] != face {
        parent[face] = parent[parent[face]];
        face = parent[face];
    }
    face
}

/// The pcurve placed onto its support's parameter frame.
///
/// The affine coefficients come off the document, so a scale or offset that
/// drives one non-finite is a source the transform carrier refuses; `None`
/// then states that the placement is not representable, rather than panicking.
fn transformed_pcurve_geometry(
    geometry: PcurveGeometry,
    affine: Option<SurfaceParameterAffine>,
) -> Option<PcurveGeometry> {
    let Some(affine) = affine else {
        return Some(geometry);
    };
    if affine
        == (SurfaceParameterAffine {
            u_scale: 1.0,
            u_offset: 0.0,
            v_scale: 1.0,
            v_offset: 0.0,
        })
    {
        return Some(geometry);
    }
    // Reflections and translations have an exact native line form. Normalize
    // those chart changes rather than making source coordinate conventions
    // part of neutral content. Non-isometric charts retain their affine form.
    if affine.u_scale.abs() == 1.0 && affine.v_scale.abs() == 1.0 {
        use cadmpeg_ir::geometry::pcurve::{
            CirclePcurve, EllipsePcurve, HyperbolaPcurve, ParabolaPcurve,
        };
        let point = |point: cadmpeg_ir::math::Point2| {
            cadmpeg_ir::math::Point2::new(
                affine.u_scale * point.u + affine.u_offset,
                affine.v_scale * point.v + affine.v_offset,
            )
        };
        let vector = |point: cadmpeg_ir::math::Point2| {
            cadmpeg_ir::math::Point2::new(affine.u_scale * point.u, affine.v_scale * point.v)
        };
        match &geometry {
            PcurveGeometry::Circle(value) => {
                return CirclePcurve::try_new(
                    point(value.center().get()),
                    vector(value.x_axis().get()),
                    vector(value.y_axis().get()),
                    value.radius().get(),
                )
                .ok()
                .map(PcurveGeometry::Circle)
            }
            PcurveGeometry::Ellipse(value) => {
                return EllipsePcurve::try_new(
                    point(value.center().get()),
                    vector(value.x_axis().get()),
                    vector(value.y_axis().get()),
                    value.major_radius().get(),
                    value.minor_radius().get(),
                )
                .ok()
                .map(PcurveGeometry::Ellipse)
            }
            PcurveGeometry::Parabola(value) => {
                return ParabolaPcurve::try_new(
                    point(value.vertex().get()),
                    vector(value.x_axis().get()),
                    vector(value.y_axis().get()),
                    value.focal_distance().get(),
                )
                .ok()
                .map(PcurveGeometry::Parabola)
            }
            PcurveGeometry::Hyperbola(value) => {
                return HyperbolaPcurve::try_new(
                    point(value.center().get()),
                    vector(value.x_axis().get()),
                    vector(value.y_axis().get()),
                    value.major_radius().get(),
                    value.minor_radius().get(),
                )
                .ok()
                .map(PcurveGeometry::Hyperbola)
            }
            _ => {}
        }
        if let PcurveGeometry::Line(line) = &geometry {
            let origin = line.origin().get();
            let direction = line.direction().as_raw();
            return cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(
                    affine.u_scale * origin.u + affine.u_offset,
                    affine.v_scale * origin.v + affine.v_offset,
                ),
                cadmpeg_ir::math::Point2::new(
                    affine.u_scale * direction.u,
                    affine.v_scale * direction.v,
                ),
            )
            .ok()
            .map(PcurveGeometry::Line);
        }
    }
    Some(PcurveGeometry::Transformed(
        cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
            Box::new(geometry),
            Transform2::affine([
                [affine.u_scale, 0.0, affine.u_offset],
                [0.0, affine.v_scale, affine.v_offset],
            ])?,
        )
        .ok()?,
    ))
}

fn positive_tolerance(value: f64) -> Option<cadmpeg_ir::scalar::PositiveReal> {
    cadmpeg_ir::scalar::PositiveReal::new(value)
}

#[derive(Debug)]
pub(crate) enum PcurveGeometryError {
    Nurbs(NurbsError),
    Resource(CodecError),
}

fn pcurve_loss(
    payload_id: &str,
    curve_index: usize,
    error: Option<&PcurveGeometryError>,
) -> LossNote {
    let message = match error {
        Some(error) => format!("payload {payload_id} curve2ds index {curve_index} could not enter neutral geometry: {error}"),
        None => format!("payload {payload_id} curve2ds index {curve_index} could not enter neutral geometry"),
    };
    FreecadLossCode::PcurveNotTransferred.note(message)
}

impl std::fmt::Display for PcurveGeometryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nurbs(error) => error.fmt(formatter),
            Self::Resource(error) => error.fmt(formatter),
        }
    }
}

impl From<NurbsError> for PcurveGeometryError {
    fn from(error: NurbsError) -> Self {
        Self::Nurbs(error)
    }
}

impl From<CodecError> for PcurveGeometryError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl From<PcurveGeometryError> for CodecError {
    fn from(error: PcurveGeometryError) -> Self {
        match error {
            PcurveGeometryError::Nurbs(error) => error.into(),
            PcurveGeometryError::Resource(error) => error,
        }
    }
}

/// Read a 2D curve record into neutral geometry. `Ok(None)` states a record
/// the neutral model cannot carry; `Err` states a B-spline record whose lanes
/// the carrier refuses.
pub(crate) fn pcurve_geometry(
    ctx: &DecodeContext<'_>,
    curve: &TextCurve2d,
) -> Result<Option<PcurveGeometry>, PcurveGeometryError> {
    let _depth = ctx.enter_nested("FreeCAD pcurve geometry nesting")?;
    Ok(match curve {
        TextCurve2d::Line { origin, direction } => NonzeroPoint2::new(direction.get())
            .map(|direction| cadmpeg_ir::geometry::pcurve::LinePcurve::new(*origin, direction))
            .map(PcurveGeometry::Line),
        TextCurve2d::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => PositiveReal::from_finite(*radius)
            .and_then(|radius| {
                cadmpeg_ir::geometry::pcurve::CirclePcurve::from_parts(
                    *center, *x_axis, *y_axis, radius,
                )
            })
            .map(PcurveGeometry::Circle),
        TextCurve2d::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => PositiveReal::from_finite(*major_radius)
            .zip(PositiveReal::from_finite(*minor_radius))
            .and_then(|(major_radius, minor_radius)| {
                cadmpeg_ir::geometry::pcurve::EllipsePcurve::from_parts(
                    *center,
                    *x_axis,
                    *y_axis,
                    major_radius,
                    minor_radius,
                )
            })
            .map(PcurveGeometry::Ellipse),
        TextCurve2d::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } => PositiveReal::from_finite(*focal_distance)
            .and_then(|focal_distance| {
                cadmpeg_ir::geometry::pcurve::ParabolaPcurve::from_parts(
                    *vertex,
                    *x_axis,
                    *y_axis,
                    focal_distance,
                )
            })
            .map(PcurveGeometry::Parabola),
        TextCurve2d::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => PositiveReal::from_finite(*major_radius)
            .zip(PositiveReal::from_finite(*minor_radius))
            .and_then(|(major_radius, minor_radius)| {
                cadmpeg_ir::geometry::pcurve::HyperbolaPcurve::from_parts(
                    *center,
                    *x_axis,
                    *y_axis,
                    major_radius,
                    minor_radius,
                )
            })
            .map(PcurveGeometry::Hyperbola),
        TextCurve2d::Nurbs(nurbs) => {
            let poles = if let Some(weights) = &nurbs.weights {
                if weights.len() != nurbs.control_points.len() {
                    return Err(NurbsError::WeightLaneLength {
                        field: "pcurve poles".into(),
                        poles: nurbs.control_points.len(),
                        weights: weights.len(),
                    }
                    .into());
                }
                let mut rows = Vec::new();
                for (index, (point, weight)) in nurbs.control_points.iter().zip(weights).enumerate()
                {
                    let Some(weight) = NonZeroReal::from_finite(*weight) else {
                        return Err(NurbsError::UnusableWeight {
                            field: "pcurve poles".into(),
                            index,
                            weight: weight.get(),
                        }
                        .into());
                    };
                    rows.push(WeightedPole2 {
                        point: *point,
                        weight,
                    });
                }
                PcurveNurbsPoles::Rational { points: rows }
            } else {
                PcurveNurbsPoles::Polynomial {
                    points: nurbs.control_points.clone(),
                }
            };
            let mut knots = Vec::new();
            knots.extend(nurbs.knots.iter().map(|knot| knot.get()));
            Some(PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::new(
                    nurbs.degree,
                    cadmpeg_ir::geometry::nurbs::KnotVector::new(knots)?,
                    poles,
                    nurbs.periodic,
                )?,
            })
        }
        TextCurve2d::Trimmed {
            parameter_range,
            basis,
        } => {
            let Some(basis) = pcurve_geometry(ctx, basis.curve())? else {
                return Ok(None);
            };
            cadmpeg_ir::geometry::pcurve::TrimmedPcurve::from_finite_parts(
                *parameter_range,
                true,
                Box::new(basis),
            )
            .ok()
            .map(PcurveGeometry::Trimmed)
        }
        TextCurve2d::Offset { distance, basis } => {
            let Some(basis) = pcurve_geometry(ctx, basis.curve())? else {
                return Ok(None);
            };
            cadmpeg_ir::geometry::pcurve::OffsetPcurve::from_finite_parts(
                *distance,
                Box::new(basis),
            )
            .ok()
            .map(PcurveGeometry::Offset)
        }
    })
}

fn ensure_similarity(transform: Transform) -> Result<(), CodecError> {
    uniform_scale(transform).map(|_| ())
}

fn uniform_scale(transform: Transform) -> Result<cadmpeg_ir::scalar::PositiveReal, CodecError> {
    let columns = transform.linear_columns();
    let scale = cadmpeg_ir::scalar::PositiveReal::new(columns[0].norm()).ok_or_else(|| {
        CodecError::Malformed("B-rep location is not a finite similarity transform".into())
    })?;
    let lengths = columns.map(|column| column.norm());
    let units = columns.map(cadmpeg_ir::features::FiniteVector3::unit_nonzero);
    if lengths.iter().any(|length| {
        !length.is_finite()
            || (*length / scale.get() - 1.0).abs() > EPS_TOPOLOGY_TRANSFER_DEGENERATE
    }) || units.iter().any(Option::is_none)
        || units[0]
            .zip(units[1])
            .is_some_and(|(a, b)| a.dot(b).abs() > EPS_TOPOLOGY_TRANSFER_DEGENERATE)
        || units[0]
            .zip(units[2])
            .is_some_and(|(a, b)| a.dot(b).abs() > EPS_TOPOLOGY_TRANSFER_DEGENERATE)
        || units[1]
            .zip(units[2])
            .is_some_and(|(a, b)| a.dot(b).abs() > EPS_TOPOLOGY_TRANSFER_DEGENERATE)
    {
        return Err(CodecError::Malformed(
            "B-rep location is not a finite similarity transform".into(),
        ));
    }
    Ok(scale)
}

fn transform_curve(
    geometry: &CurveGeometry,
    transform: Transform,
) -> Result<CurveGeometry, CodecError> {
    ensure_similarity(transform)?;
    let solved = geometry.solved().ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
    })?;
    let basis = solved.clone();
    Ok(CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(Box::new(basis), transform)
            .map_err(cadmpeg_core::CodecError::malformed)?,
    )))
}

fn transform_surface(
    geometry: &SurfaceGeometry,
    transform: Transform,
) -> Result<SurfaceGeometry, CodecError> {
    ensure_similarity(transform)?;
    let solved = geometry.solved().ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
    })?;
    let basis = solved.clone();
    Ok(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(Box::new(basis), transform)
            .map_err(cadmpeg_core::CodecError::malformed)?,
    )))
}

/// Places every polyline sample, refusing a sample the transform sends out of
/// the finite range.
fn place_polyline_samples(
    samples: &mut PolylineSamples<FiniteReal, FinitePoint3>,
    transform: Transform,
) -> Result<(), CodecError> {
    samples.edit_admitted_points(|point| match transform.apply_point(point.get()) {
        Some(point) => Ok(point),
        None => Err(CodecError::Malformed(
            "placed polyline sample contains a non-finite coordinate".to_owned(),
        )),
    })?;
    Ok(())
}

fn transform_normalized_vector(transform: Transform, vector: Vector3) -> Option<FiniteVector3> {
    UnitVector3::normalized_nonzero(transform.apply_vector(vector)?).map(FiniteVector3::from)
}

fn occurrence_label(shape: usize, transform: Located) -> String {
    if transform.identity == 0 {
        shape.to_string()
    } else {
        format!("{}@location{}", shape, transform.identity)
    }
}

fn admit_topology_locations(
    ctx: &DecodeContext<'_>,
    tables: Tables<'_>,
    locations: &mut Locations,
) -> Result<(), LocationIdentityError> {
    for root in tables.roots {
        let root_location = locations.source(tables, root.location)?;
        let mut visited = HashSet::new();
        let mut stack = vec![(root.shape, Located::identity())];
        while let Some((shape_index, local)) = stack.pop() {
            if !locations.visit(ctx, &mut visited, shape_index, local)? {
                continue;
            }
            locations.admit(ctx, tables, root_location, local)?;
            let shape = tables.tshapes.resolve(shape_index)?;
            match &shape.geometry {
                TextTShapeGeometry::Face { location, .. } => {
                    let support = locations.source(tables, *location)?;
                    locations.admit(ctx, tables, local, support)?;
                }
                TextTShapeGeometry::Edge {
                    representations, ..
                } => {
                    for representation in representations {
                        let carrier = locations.source(tables, representation.location())?;
                        locations.admit(ctx, tables, local, carrier)?;
                    }
                }
                _ => {}
            }
            for child in &shape.children {
                let child_location = locations.source(tables, child.location)?;
                let child_location = locations.admit(ctx, tables, local, child_location)?;
                stack
                    .try_reserve(1)
                    .map_err(|_| locations::allocation_failed(ctx))?;
                stack.push((child.shape, child_location));
            }
        }
    }
    Ok(())
}

fn source_topology_indices(
    ctx: &DecodeContext<'_>,
    tables: Tables<'_>,
    locations: &mut Locations,
) -> Result<HashMap<(TextShapeKind, SourceOccurrenceKey), usize>, LocationIdentityError> {
    let mut indices = HashMap::new();
    for target in [
        TextShapeKind::Vertex,
        TextShapeKind::Edge,
        TextShapeKind::Wire,
        TextShapeKind::Face,
        TextShapeKind::Shell,
        TextShapeKind::Solid,
        TextShapeKind::CompSolid,
        TextShapeKind::Compound,
    ] {
        let mut next_index = 1;
        let mut visited = HashSet::new();
        for root in tables.roots {
            let mut stack = Vec::new();
            stack.push((root.clone(), Located::identity()));
            while let Some((shape_use, parent)) = stack.pop() {
                let local = locations.source(tables, shape_use.location)?;
                let transform = locations.admit(ctx, tables, parent, local)?;
                if !locations.visit(ctx, &mut visited, shape_use.shape, transform)? {
                    continue;
                }
                let shape = &tables.tshapes[shape_use.shape - 1];
                if shape.kind() == target {
                    let key = (target, SourceOccurrenceKey::new(shape_use.shape, transform));
                    indices
                        .try_reserve(1)
                        .map_err(|_| locations::allocation_failed(ctx))?;
                    if let std::collections::hash_map::Entry::Vacant(e) = indices.entry(key) {
                        e.insert(next_index);
                        next_index += 1;
                    }
                    continue;
                }
                if topology_rank(shape.kind()) < topology_rank(target) {
                    stack
                        .try_reserve(shape.children.len())
                        .map_err(|_| locations::allocation_failed(ctx))?;
                    stack.extend(
                        shape
                            .children
                            .iter()
                            .rev()
                            .cloned()
                            .map(|child| (child, transform)),
                    );
                }
            }
        }
    }
    Ok(indices)
}

fn topology_rank(kind: TextShapeKind) -> u8 {
    match kind {
        TextShapeKind::Compound => 0,
        TextShapeKind::CompSolid => 1,
        TextShapeKind::Solid => 2,
        TextShapeKind::Shell => 3,
        TextShapeKind::Face => 4,
        TextShapeKind::Wire => 5,
        TextShapeKind::Edge => 6,
        TextShapeKind::Vertex => 7,
    }
}

fn indexed_name(kind: TextShapeKind) -> &'static str {
    match kind {
        TextShapeKind::Vertex => "Vertex",
        TextShapeKind::Edge => "Edge",
        TextShapeKind::Wire => "Wire",
        TextShapeKind::Face => "Face",
        TextShapeKind::Shell => "Shell",
        TextShapeKind::Solid => "Solid",
        TextShapeKind::CompSolid => "CompSolid",
        TextShapeKind::Compound => "Compound",
    }
}

fn transform_digest(transform: Transform) -> String {
    // TopLoc_Location equality is exact; neutral identities must not merge
    // source-distinct placements at a decoder tolerance boundary.
    let mut bytes = Vec::new();
    for row in transform.rows() {
        for value in row {
            let canonical = if value == 0.0 { 0.0 } else { value };
            bytes.extend_from_slice(&canonical.to_bits().to_le_bytes());
        }
    }
    sha256_hex(&bytes)[..16].to_owned()
}

fn is_identity(transform: Transform) -> bool {
    exact_transforms_equal(transform, Transform::identity())
}

fn exact_transforms_equal(left: Transform, right: Transform) -> bool {
    left.rows()
        .into_iter()
        .flatten()
        .zip(right.rows().into_iter().flatten())
        .all(|(left, right)| left == right)
}

fn is_reversed(orientation: TextOrientation) -> bool {
    orientation == TextOrientation::Reversed
}

fn sense(reversed: bool) -> Sense {
    if reversed {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn close_radial_rings(coedges: &mut [Coedge]) {
    let mut by_edge: HashMap<EdgeId, Vec<usize>> = HashMap::new();
    for (index, coedge) in coedges.iter().enumerate() {
        if !by_edge.contains_key(&coedge.edge) {
            let key = coedge.edge.clone();
            by_edge.insert(key, Vec::new());
        }
        if let Some(indices) = by_edge.get_mut(&coedge.edge) {
            indices.push(index);
        }
    }
    for indices in by_edge.values() {
        if let [first, second] = indices.as_slice() {
            coedges[*first].radial_next = coedges[*second].id.clone();
            coedges[*second].radial_next = coedges[*first].id.clone();
        }
    }
}

fn edge_endpoint_uses(
    edge: usize,
    children: &[TextShapeUse],
) -> Result<(&TextShapeUse, &TextShapeUse), CodecError> {
    let mut start = None;
    let mut end = None;
    for child in children {
        match child.orientation {
            TextOrientation::Forward => {
                if start.replace(child).is_some() {
                    return Err(CodecError::malformed(format_args!(
                        "edge TShape {edge} has multiple forward endpoint uses"
                    )));
                }
            }
            TextOrientation::Reversed => {
                if end.replace(child).is_some() {
                    return Err(CodecError::malformed(format_args!(
                        "edge TShape {edge} has multiple reversed endpoint uses"
                    )));
                }
            }
            TextOrientation::Internal | TextOrientation::External => {}
        }
    }
    start.zip(end).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "edge TShape {edge} does not have both forward and reversed endpoint uses"
        ))
    })
}

#[derive(Debug)]
enum PcurveSelection<'a> {
    Missing,
    Selected(usize, &'a TextEdgeRepresentation),
    AmbiguousLocation,
}

fn select_pcurve_representation<'a>(
    representations: &'a [TextEdgeRepresentation],
    tables: &Tables<'_>,
    locations: &Locations,
    edge_transform: Located,
    surface: usize,
    surface_transform: Located,
) -> Result<PcurveSelection<'a>, CodecError> {
    let mut matched = PcurveSelection::Missing;
    let mut matrix_match = false;
    for (index, representation) in representations.iter().enumerate() {
        let (candidate_surface, location) = match representation {
            TextEdgeRepresentation::Pcurve {
                surface, location, ..
            }
            | TextEdgeRepresentation::PcurvePair {
                surface, location, ..
            } => (*surface, *location),
            _ => continue,
        };
        if candidate_surface != surface {
            continue;
        }
        let local = locations.source(*tables, location)?;
        let candidate = locations.compose(edge_transform, local)?;
        if candidate.identity == surface_transform.identity {
            matched = PcurveSelection::Selected(index, representation);
        } else if exact_transforms_equal(candidate.transform, surface_transform.transform) {
            matrix_match = true;
        }
    }
    if matches!(matched, PcurveSelection::Missing) && matrix_match {
        return Ok(PcurveSelection::AmbiguousLocation);
    }
    Ok(matched)
}

fn select_exact_curve_representation<'a>(
    edge: usize,
    representations: &'a [TextEdgeRepresentation],
    tables: &Tables<'_>,
) -> Result<Option<(usize, &'a TextEdgeRepresentation)>, CodecError> {
    let mut matches = representations
        .iter()
        .enumerate()
        .filter(|(_, representation)| {
            matches!(representation, TextEdgeRepresentation::Curve3d { .. })
        });
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if matches.any(|(_, representation)| {
        !equivalent_exact_curve_representation(first.1, representation, tables)
    }) {
        return Err(CodecError::malformed(format_args!(
            "edge TShape {edge} has non-equivalent 3D curve representations"
        )));
    }
    Ok(Some(first))
}

fn equivalent_exact_curve_representation(
    left: &TextEdgeRepresentation,
    right: &TextEdgeRepresentation,
    tables: &Tables<'_>,
) -> bool {
    let (
        TextEdgeRepresentation::Curve3d {
            curve: left_curve,
            location: left_location,
            parameter_range: left_range,
        },
        TextEdgeRepresentation::Curve3d {
            curve: right_curve,
            location: right_location,
            parameter_range: right_range,
        },
    ) = (left, right)
    else {
        return false;
    };
    let Some(left_curve) = left_curve.checked_sub(1) else {
        return false;
    };
    let Some(right_curve) = right_curve.checked_sub(1) else {
        return false;
    };
    tables.curves.get(left_curve) == tables.curves.get(right_curve)
        && tables
            .location(*left_location)
            .ok()
            .zip(tables.location(*right_location).ok())
            .is_some_and(|(left, right)| left == right)
        && left_range == right_range
}

fn unique_fallback_polygon_representation(
    edge: usize,
    representations: &[TextEdgeRepresentation],
) -> Result<Option<(usize, &TextEdgeRepresentation)>, CodecError> {
    let mut matches = representations
        .iter()
        .enumerate()
        .filter(|(_, representation)| {
            matches!(
                representation,
                TextEdgeRepresentation::Polygon3d { .. }
                    | TextEdgeRepresentation::PolygonOnTriangulation { .. }
                    | TextEdgeRepresentation::PolygonPair { .. }
            )
        });
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "edge TShape {edge} has multiple fallback polygon representations"
        )));
    }
    Ok(Some(first))
}

/// An OCCT trimmed curve keeps its basis parameter, and a CADIR subset curve
/// runs on the local parameter from its first trim. The local interval of an
/// OCCT edge range on `curve` when `curve` is a subset; on a periodic basis an
/// endpoint moves by whole periods into the subset.
fn occt_subset_local_range(
    ir: &CadIr,
    curve: &CurveId,
    range: [FiniteReal; 2],
) -> Option<[FiniteReal; 2]> {
    let construction = ir
        .model
        .curves
        .iter()
        .find(|item| item.id == *curve)?
        .geometry
        .procedural_construction()?;
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.id == *construction)?;
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(subset) = procedural.definition()
    else {
        return None;
    };
    let [first, last] = subset.parameter_range().endpoints();
    let span = last - first;
    let period = ir
        .model
        .curves
        .iter()
        .find(|item| item.id == *subset.source())
        .and_then(|source| source.geometry.solved())
        .and_then(occt_curve_period);
    let local = |parameter: f64| {
        let local = if *subset.sense() {
            parameter - first
        } else {
            last - parameter
        };
        let local = match period {
            Some(period)
                if local < -EPS_TOPOLOGY_TRANSFER_GEOMETRY
                    || local > span + EPS_TOPOLOGY_TRANSFER_GEOMETRY =>
            {
                let wrapped = local.rem_euclid(period);
                if period - wrapped <= EPS_TOPOLOGY_TRANSFER_GEOMETRY {
                    0.0
                } else {
                    wrapped
                }
            }
            _ => local,
        };
        if local.abs() <= EPS_TOPOLOGY_TRANSFER_GEOMETRY {
            0.0
        } else if (local - span).abs() <= EPS_TOPOLOGY_TRANSFER_GEOMETRY {
            span
        } else {
            local
        }
    };
    let [start, end] = range.map(|parameter| local(parameter.get()));
    let (start, end) = if *subset.sense() {
        (start, end)
    } else {
        (end, start)
    };
    let end = match period {
        Some(period) if end <= start => end + period,
        _ => end,
    };
    Some([FiniteReal::new(start)?, FiniteReal::new(end)?])
}

fn occt_curve_period(geometry: &SolvedCurveGeometry) -> Option<f64> {
    match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            Some(std::f64::consts::TAU)
        }
        SolvedCurveGeometry::Nurbs(nurbs) if nurbs.periodic() => {
            let [lower, upper] = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)?.endpoints();
            Some(upper - lower).filter(|period| period.is_finite() && *period > 0.0)
        }
        SolvedCurveGeometry::Transformed(placed) => occt_curve_period(placed.basis()),
        _ => None,
    }
}

pub(crate) fn normalize_occt_curve_range(
    geometry: &SolvedCurveGeometry,
    range: Option<[FiniteReal; 2]>,
) -> Option<[FiniteReal; 2]> {
    match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            let [start, end] = range?;
            let sweep = end.get() - start.get();
            let tau = std::f64::consts::TAU;
            if !sweep.is_finite() || (sweep - tau).abs() <= EPS_TOPOLOGY_TRANSFER_GEOMETRY {
                return Some([start, end]);
            }
            let canonical_start = start.get().rem_euclid(tau);
            let canonical_start =
                if (tau - canonical_start).abs() <= EPS_TOPOLOGY_TRANSFER_EXACT_GEOMETRY {
                    0.0
                } else {
                    canonical_start
                };
            Some([
                FiniteReal::new(canonical_start)?,
                FiniteReal::new(canonical_start + sweep)?,
            ])
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let focal_distance = parabola_curve.focal_distance().magnitude();
            let [start, end] = range?;
            Some([
                cadmpeg_ir::math::multiply_divide(start, FiniteReal::HALF, focal_distance)?,
                cadmpeg_ir::math::multiply_divide(end, FiniteReal::HALF, focal_distance)?,
            ])
        }
        SolvedCurveGeometry::Transformed(placed) => {
            normalize_occt_curve_range(placed.basis(), range)
        }
        _ => range,
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod admission_tests;

#[cfg(test)]
mod numerical_range_tests;

#[cfg(test)]
mod located_geometry_tests;

#[cfg(test)]
mod pcurve_selection_tests;

#[cfg(test)]
mod trimmed_support_tests;
