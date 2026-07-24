// SPDX-License-Identifier: Apache-2.0
//! Occurrence-aware transfer of exact-shape topology into neutral CADIR.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::provenance::SourceObjectAssociation;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::builder::{BodySpec, CoedgeSpec, EdgeSpec, FaceSpec, TopologyBuilder};
use cadmpeg_ir::topology::{BodyKind, Coedge, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::wire::hash::sha256_hex;

use crate::brep::{
    ShapePayloadRecord, TextCurve2d, TextEdgeRepresentation, TextLocation, TextOrientation,
    TextPolygon3d, TextPolygonOnTriangulation, TextShapeKind, TextShapeUse, TextSurface,
    TextTShape, TextTShapeGeometry, TextTriangulation,
};
use crate::native::PropertyRecord;

type IndexedPolygon = (Vec<Point3>, Option<Vec<f64>>, f64);

/// Transfer text or binary shape-set topology with placements applied once.
pub(crate) fn transfer(
    ir: &mut CadIr,
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
) -> Result<(), CodecError> {
    for payload in payloads {
        let Some(tables) = Tables::from_payload(payload) else {
            continue;
        };
        let source_object = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map_or_else(
                || payload.property.clone(),
                |property| property.owner.clone(),
            );
        let mut builder = Builder::new(payload, tables, source_object);
        builder.emit_pcurves(ir);
        for root in builder.body_roots()? {
            builder.append_body(ir, root)?;
        }
        builder.emit_unowned_triangulations(ir);
    }
    close_radial_rings(&mut ir.model.coedges);
    let referenced_pcurves = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .map(|use_| &use_.pcurve)
        .collect::<HashSet<_>>();
    ir.model
        .pcurves
        .retain(|pcurve| referenced_pcurves.contains(&pcurve.id));
    Ok(())
}

#[derive(Clone, Copy)]
struct Tables<'a> {
    locations: &'a [TextLocation],
    curve2ds: &'a [TextCurve2d],
    surfaces: &'a [TextSurface],
    polygons3d: &'a [TextPolygon3d],
    polygons_on_triangulations: &'a [TextPolygonOnTriangulation],
    tshapes: &'a [TextTShape],
    triangulations: &'a [TextTriangulation],
    roots: &'a [TextShapeUse],
}

impl<'a> Tables<'a> {
    fn from_payload(payload: &'a ShapePayloadRecord) -> Option<Self> {
        payload
            .text
            .as_ref()
            .map(|text| Self {
                locations: &text.locations,
                curve2ds: &text.curve2ds,
                surfaces: &text.surfaces,
                polygons3d: &text.polygons3d,
                polygons_on_triangulations: &text.polygons_on_triangulations,
                tshapes: &text.tshapes,
                triangulations: &text.triangulations,
                roots: &text.roots,
            })
            .or_else(|| {
                payload.binary.as_ref().map(|binary| Self {
                    locations: &binary.locations,
                    curve2ds: &binary.curve2ds,
                    surfaces: &binary.surfaces,
                    polygons3d: &binary.polygons3d,
                    polygons_on_triangulations: &binary.polygons_on_triangulations,
                    tshapes: &binary.tshapes,
                    triangulations: &binary.triangulations,
                    roots: &binary.roots,
                })
            })
    }

    fn location(&self, index: usize) -> Transform {
        if index == 0 {
            Transform::identity()
        } else {
            self.locations[index - 1].transform
        }
    }
}

#[derive(Clone, Copy)]
struct BodyRoot {
    shape: usize,
    transform: Transform,
    reversed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OccurrenceKey {
    shape: usize,
    transform: [u64; 16],
}

impl OccurrenceKey {
    fn new(shape: usize, transform: Transform) -> Self {
        Self {
            shape,
            transform: transform_bits(transform),
        }
    }
}

struct Builder<'a> {
    payload: &'a ShapePayloadRecord,
    tables: Tables<'a>,
    vertices: HashMap<OccurrenceKey, VertexId>,
    edges: HashMap<OccurrenceKey, EdgeId>,
    emitted_curves: HashSet<CurveId>,
    emitted_surfaces: HashSet<SurfaceId>,
    emitted_triangulations: HashSet<usize>,
    body_scope: Transform,
    current_body: Option<BodyId>,
    source_object: String,
}

impl<'a> Builder<'a> {
    fn new(payload: &'a ShapePayloadRecord, tables: Tables<'a>, source_object: String) -> Self {
        Self {
            payload,
            tables,
            vertices: HashMap::new(),
            edges: HashMap::new(),
            emitted_curves: HashSet::new(),
            emitted_surfaces: HashSet::new(),
            emitted_triangulations: HashSet::new(),
            body_scope: Transform::identity(),
            current_body: None,
            source_object,
        }
    }

    fn source_association(&self) -> SourceObjectAssociation {
        SourceObjectAssociation {
            format: "fcstd".into(),
            object_id: self.source_object.clone(),
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }
    }

    fn emit_pcurves(&self, ir: &mut CadIr) {
        for shape in self.tables.tshapes {
            let TextTShapeGeometry::Edge {
                representations, ..
            } = &shape.geometry
            else {
                continue;
            };
            for (representation_index, representation) in representations.iter().enumerate() {
                if !matches!(representation.kind, 2 | 3) {
                    continue;
                }
                let (u_scale, v_scale) = representation
                    .surface
                    .and_then(|surface| self.tables.surfaces.get(surface - 1))
                    .map_or((None, None), |surface| match surface {
                        TextSurface::Plane {
                            v_reversed: true, ..
                        } => (None, Some(-1.0)),
                        TextSurface::Cylinder {
                            u_reversed: true, ..
                        } => (Some(-1.0), None),
                        TextSurface::Cone { half_angle, .. } => (None, Some(half_angle.cos())),
                        _ => (None, None),
                    });
                let mut primary_geometry =
                    pcurve_geometry(&self.tables.curve2ds[representation.primary - 1]);
                if let Some(v_scale) = v_scale {
                    scale_pcurve_v(&mut primary_geometry, v_scale);
                }
                if let Some(u_scale) = u_scale {
                    scale_pcurve_u(&mut primary_geometry, u_scale);
                }
                ir.model.pcurves.push(Pcurve {
                    id: self.pcurve_id(shape.index, representation_index, false),
                    geometry: primary_geometry,
                    wrapper_reversed: None,
                    native_tail_flags: None,
                    parameter_range: representation.parameter_range,
                    fit_tolerance: None,
                });
                if let Some(secondary) = representation.secondary {
                    let mut secondary_geometry =
                        pcurve_geometry(&self.tables.curve2ds[secondary - 1]);
                    if let Some(v_scale) = v_scale {
                        scale_pcurve_v(&mut secondary_geometry, v_scale);
                    }
                    if let Some(u_scale) = u_scale {
                        scale_pcurve_u(&mut secondary_geometry, u_scale);
                    }
                    ir.model.pcurves.push(Pcurve {
                        id: self.pcurve_id(shape.index, representation_index, true),
                        geometry: secondary_geometry,
                        wrapper_reversed: None,
                        native_tail_flags: None,
                        parameter_range: representation.parameter_range,
                        fit_tolerance: None,
                    });
                }
            }
        }
    }

    fn emit_unowned_triangulations(&self, ir: &mut CadIr) {
        for (offset, triangulation) in self.tables.triangulations.iter().enumerate() {
            let index = offset + 1;
            if self.emitted_triangulations.contains(&index) {
                continue;
            }
            ir.model.tessellations.push(Tessellation {
                id: crate::native::model_id("tessellation", &self.payload.id, index.to_string()),
                body: None,
                faces: Vec::new(),
                chordal_deflection: Some(triangulation.deflection),
                source_object: Some(SourceObjectAssociation {
                    format: "fcstd".into(),
                    object_id: self.source_object.clone(),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
                vertices: triangulation.nodes.clone(),
                triangles: triangulation
                    .triangles
                    .iter()
                    .map(|triangle| [triangle[0] - 1, triangle[1] - 1, triangle[2] - 1])
                    .collect(),
                strip_lengths: Vec::new(),
                normals: triangulation.normals.clone().unwrap_or_default(),
                channels: Vec::new(),
            });
        }
    }

    fn pcurve_id(&self, edge: usize, representation: usize, secondary: bool) -> PcurveId {
        PcurveId(crate::native::model_id(
            "pcurve",
            &self.payload.id,
            format!(
                "{}:{}:{}",
                edge,
                representation + 1,
                usize::from(secondary) + 1
            ),
        ))
    }

    fn body_roots(&self) -> Result<Vec<BodyRoot>, CodecError> {
        self.tables
            .roots
            .iter()
            .map(|root| {
                self.shape(root.shape)?;
                Ok(BodyRoot {
                    shape: root.shape,
                    transform: self.tables.location(root.location),
                    reversed: is_reversed(root.orientation),
                })
            })
            .collect()
    }

    fn append_body(&mut self, ir: &mut CadIr, root: BodyRoot) -> Result<(), CodecError> {
        self.body_scope = root.transform;
        let root_shape = self.shape(root.shape)?;
        let root_kind = root_shape.kind;
        if root_kind == TextShapeKind::Edge && root_shape.children.is_empty() {
            let TextTShapeGeometry::Edge {
                degenerated,
                representations,
                ..
            } = &root_shape.geometry
            else {
                unreachable!("edge kind and geometry must agree")
            };
            if !degenerated
                && !representations
                    .iter()
                    .any(|representation| representation.kind == 1)
            {
                return Err(CodecError::Malformed(format!(
                    "unbounded edge TShape {} has no exact curve",
                    root.shape
                )));
            }
            return Ok(());
        }
        let body_key = occurrence_label(root.shape, root.transform);
        let body_id = BodyId(crate::native::model_id("body", &self.payload.id, &body_key));
        self.current_body = Some(body_id.clone());
        let kind = match root_kind {
            TextShapeKind::Solid => BodyKind::Solid,
            TextShapeKind::Wire | TextShapeKind::Edge => BodyKind::Wire,
            TextShapeKind::Shell | TextShapeKind::Face => BodyKind::Sheet,
            _ => BodyKind::General,
        };
        let tessellation_start = ir.model.tessellations.len();
        // Stage this body's whole hierarchy in a private builder. A body that turns
        // out to own no region is dropped by discarding the builder unfinished,
        // preserving the source's "empty body is not emitted" tolerance without any
        // arena retraction.
        let mut topology = TopologyBuilder::new();
        topology
            .body(
                body_id.clone(),
                BodySpec {
                    kind,
                    transform: (!is_identity(root.transform)).then_some(root.transform),
                    name: None,
                    color: None,
                    visible: None,
                },
            )
            .expect("first registration in a fresh builder cannot collide");
        let mut regions = Vec::new();
        self.append_shape_regions(
            &mut topology,
            ir,
            &body_id,
            root.shape,
            Transform::identity(),
            root.reversed,
            &mut regions,
            0,
        )?;
        if regions.is_empty() {
            ir.model.tessellations.truncate(tessellation_start);
            return Ok(());
        }
        topology
            .finish(&mut ir.model)
            .expect("staged body topology appends without id or owner conflicts");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Depth is explicit to bound hostile topology nesting.
    fn append_shape_regions(
        &mut self,
        topology: &mut TopologyBuilder,
        ir: &mut CadIr,
        body: &BodyId,
        shape_index: usize,
        transform: Transform,
        reversed: bool,
        output: &mut Vec<RegionId>,
        depth: usize,
    ) -> Result<(), CodecError> {
        if depth > 256 {
            return Err(CodecError::Malformed(
                "topology nesting limit exceeded".into(),
            ));
        }
        let shape = self.shape(shape_index)?.clone();
        if matches!(
            shape.kind,
            TextShapeKind::Compound | TextShapeKind::CompSolid
        ) {
            for child in &shape.children {
                self.append_shape_regions(
                    topology,
                    ir,
                    body,
                    child.shape,
                    multiply(transform, self.tables.location(child.location)),
                    reversed ^ is_reversed(child.orientation),
                    output,
                    depth + 1,
                )?;
            }
            return Ok(());
        }
        // Every non-solid shape yields exactly one shell; a solid yields one per
        // shell child. The builder needs the region registered before its shells,
        // so decide emptiness up front rather than retracting a childless region —
        // matching the source's "region with no shell is not emitted" tolerance.
        let has_shell = shape.kind != TextShapeKind::Solid
            || shape
                .children
                .iter()
                .any(|child| self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Shell);
        if !has_shell {
            return Ok(());
        }
        let key = self.topology_label(shape_index, transform);
        let region_id = RegionId(crate::native::model_id("region", &self.payload.id, &key));
        topology
            .region(region_id.clone(), body)
            .expect("region registers under the body staged by `append_body`");
        if shape.kind == TextShapeKind::Solid {
            for child in shape
                .children
                .iter()
                .filter(|child| self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Shell)
            {
                self.append_shell(topology, ir, &region_id, child, transform, reversed)?;
            }
        } else {
            self.append_shell_shape(topology, ir, &region_id, shape_index, transform, reversed)?;
        }
        output.push(region_id);
        Ok(())
    }

    fn append_shell(
        &mut self,
        topology: &mut TopologyBuilder,
        ir: &mut CadIr,
        region: &RegionId,
        shell_use: &TextShapeUse,
        parent: Transform,
        reversed: bool,
    ) -> Result<(), CodecError> {
        let transform = multiply(parent, self.tables.location(shell_use.location));
        self.append_shell_shape(
            topology,
            ir,
            region,
            shell_use.shape,
            transform,
            reversed ^ is_reversed(shell_use.orientation),
        )
    }

    fn append_shell_shape(
        &mut self,
        topology: &mut TopologyBuilder,
        ir: &mut CadIr,
        region: &RegionId,
        shape_index: usize,
        transform: Transform,
        reversed: bool,
    ) -> Result<(), CodecError> {
        let shape = self.shape(shape_index)?.clone();
        let key = self.topology_label(shape_index, transform);
        let shell_id = ShellId(crate::native::model_id("shell", &self.payload.id, &key));
        topology
            .shell(shell_id.clone(), region)
            .expect("shell registers under the region staged by `append_shape_regions`");
        match shape.kind {
            TextShapeKind::Shell => {
                for child in &shape.children {
                    if self.shape(child.shape)?.kind == TextShapeKind::Face {
                        self.append_face(topology, ir, &shell_id, child, transform, reversed)?;
                    }
                }
            }
            TextShapeKind::Face => {
                let shape_use = TextShapeUse {
                    shape: shape_index,
                    orientation: TextOrientation::Forward,
                    location: 0,
                };
                self.append_face(topology, ir, &shell_id, &shape_use, transform, reversed)?;
            }
            TextShapeKind::Wire => {
                for child in &shape.children {
                    if self.shape(child.shape)?.kind == TextShapeKind::Edge {
                        let edge = self.ensure_edge(topology, ir, child, transform)?;
                        topology
                            .wire_edge(&shell_id, edge)
                            .expect("wire edge registers under the staged shell");
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
                    location: 0,
                };
                let edge = self.ensure_edge(topology, ir, &edge_use, transform)?;
                topology
                    .wire_edge(&shell_id, edge)
                    .expect("wire edge registers under the staged shell");
            }
            TextShapeKind::Vertex => {
                let vertex_use = TextShapeUse {
                    shape: shape_index,
                    orientation: TextOrientation::Forward,
                    location: 0,
                };
                let vertex = self.ensure_vertex(topology, &vertex_use, transform)?;
                topology
                    .free_vertex(&shell_id, vertex)
                    .expect("free vertex registers under the staged shell");
            }
            _ => {}
        }
        Ok(())
    }

    fn append_face(
        &mut self,
        topology: &mut TopologyBuilder,
        ir: &mut CadIr,
        shell: &ShellId,
        face_use: &TextShapeUse,
        parent: Transform,
        reversed: bool,
    ) -> Result<(), CodecError> {
        let face_transform = multiply(parent, self.tables.location(face_use.location));
        let face_reversed = reversed ^ is_reversed(face_use.orientation);
        let shape = self.shape(face_use.shape)?.clone();
        let TextTShapeGeometry::Face {
            tolerance,
            surface,
            location,
            triangulation,
            ..
        } = shape.geometry
        else {
            return Ok(());
        };
        let surface_transform = multiply(face_transform, self.tables.location(location));
        let face_key = self.topology_label(face_use.shape, face_transform);
        let face_id = FaceId(crate::native::model_id("face", &self.payload.id, &face_key));
        // OCCT triangulation nodes are already expressed in the face's surface-location frame.
        // Only the owning topological face placement remains to be applied here.
        let located_triangulation = triangulation.map(|index| {
            let triangulation = &self.tables.triangulations[index - 1];
            let vertices = triangulation
                .nodes
                .iter()
                .map(|point| transform_point(face_transform, *point))
                .collect::<Vec<_>>();
            let triangles = triangulation
                .triangles
                .iter()
                .map(|triangle| [triangle[0] - 1, triangle[1] - 1, triangle[2] - 1])
                .collect::<Vec<_>>();
            (index, triangulation, vertices, triangles)
        });
        let triangulation_scale = located_triangulation
            .as_ref()
            .map(|_| similarity(face_transform).map(|similarity| similarity.scale))
            .transpose()?;
        let surface_id = if surface != 0 {
            self.located_surface(ir, surface, surface_transform)?
        } else if let Some((index, triangulation, vertices, triangles)) = &located_triangulation {
            let id = SurfaceId(crate::native::model_id(
                "surface",
                &self.payload.id,
                format!("triangulation:{index}@{face_key}"),
            ));
            let deflection_scale = triangulation_scale.expect("triangulation scale");
            if self.emitted_surfaces.insert(id.clone()) {
                ir.model.surfaces.push(Surface {
                    id: id.clone(),
                    geometry: SurfaceGeometry::Polygonal {
                        vertices: vertices.clone(),
                        triangles: triangles.clone(),
                        chordal_deflection: triangulation.deflection * deflection_scale,
                    },
                    source_object: Some(self.source_association()),
                });
            }
            id
        } else {
            return Ok(());
        };
        if let Some((index, triangulation, vertices, triangles)) = located_triangulation {
            self.emitted_triangulations.insert(index);
            let deflection_scale = triangulation_scale.expect("triangulation scale");
            let normals = triangulation
                .normals
                .as_ref()
                .map(|normals| {
                    normals
                        .iter()
                        .map(|normal| transform_vector(face_transform, *normal))
                        .collect()
                })
                .unwrap_or_default();
            ir.model.tessellations.push(Tessellation {
                id: crate::native::model_id(
                    "tessellation",
                    &self.payload.id,
                    format!("{index}@{face_key}"),
                ),
                body: self.current_body.clone(),
                faces: vec![face_id.clone()],
                chordal_deflection: Some(triangulation.deflection * deflection_scale),
                source_object: Some(self.source_association()),
                vertices,
                triangles,
                strip_lengths: Vec::new(),
                normals,
                channels: Vec::new(),
            });
        }
        // The surface resolved, so this face is emitted. Register it before its
        // rings so the builder aggregates `face.loops` from the `ring` calls and
        // `shell.faces` from this registration.
        topology
            .face(
                face_id.clone(),
                shell,
                FaceSpec {
                    surface: surface_id,
                    sense: sense(face_reversed),
                    name: None,
                    color: None,
                    tolerance: positive_tolerance(tolerance),
                },
            )
            .expect("face registers under the shell staged by `append_shell_shape`");
        for (loop_index, wire_use) in shape
            .children
            .iter()
            .filter(|child| self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Wire)
            .enumerate()
        {
            let wire_transform = multiply(face_transform, self.tables.location(wire_use.location));
            let wire = self.shape(wire_use.shape)?.clone();
            let mut edge_uses = wire
                .children
                .iter()
                .filter(|child| self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Edge)
                .cloned()
                .collect::<Vec<_>>();
            let wire_reversed = face_reversed ^ is_reversed(wire_use.orientation);
            if wire_reversed {
                edge_uses.reverse();
            }
            if edge_uses.is_empty() {
                continue;
            }
            let loop_id = LoopId(crate::native::model_id(
                "loop",
                &self.payload.id,
                format!("{}:{}", face_key, loop_index + 1),
            ));
            let mut coedges = Vec::with_capacity(edge_uses.len());
            for (index, edge_use) in edge_uses.iter().enumerate() {
                let edge_transform =
                    multiply(wire_transform, self.tables.location(edge_use.location));
                let edge = self.ensure_edge(topology, ir, edge_use, wire_transform)?;
                let pcurve = self.face_pcurve(edge_use, edge_transform, surface, surface_transform);
                coedges.push(CoedgeSpec {
                    id: CoedgeId(crate::native::model_id(
                        "coedge",
                        &self.payload.id,
                        format!("{}:{}:{}", face_key, loop_index + 1, index + 1),
                    )),
                    edge,
                    sense: sense(is_reversed(edge_use.orientation) ^ wire_reversed),
                    pcurves: pcurve
                        .into_iter()
                        .map(
                            |(pcurve, parameter_range)| cadmpeg_ir::topology::PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range,
                            },
                        )
                        .collect(),
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
            // `ring` derives each coedge's next/previous from this slice order and
            // defaults radial_next to self, reproducing the former modulo wiring;
            // `close_radial_rings` still resolves the shared-edge radials afterward.
            topology
                .ring(
                    loop_id,
                    &face_id,
                    cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                    coedges,
                    Vec::new(),
                )
                .expect("loop ring registers under the staged face");
        }
        Ok(())
    }

    fn ensure_edge(
        &mut self,
        topology: &mut TopologyBuilder,
        ir: &mut CadIr,
        edge_use: &TextShapeUse,
        parent: Transform,
    ) -> Result<EdgeId, CodecError> {
        let transform = multiply(parent, self.tables.location(edge_use.location));
        let key = OccurrenceKey::new(edge_use.shape, multiply(self.body_scope, transform));
        if let Some(id) = self.edges.get(&key) {
            return Ok(id.clone());
        }
        let shape = self.shape(edge_use.shape)?.clone();
        let TextTShapeGeometry::Edge {
            tolerance,
            degenerated,
            representations,
            ..
        } = shape.geometry
        else {
            return Err(CodecError::Malformed(format!(
                "TShape {} is not an edge",
                edge_use.shape
            )));
        };
        let endpoint_use = |orientation| {
            shape
                .children
                .iter()
                .find(|child| child.orientation == orientation)
                .or_else(|| shape.children.first())
                .cloned()
        };
        let Some(start_use) = endpoint_use(TextOrientation::Forward) else {
            return Err(CodecError::Malformed(format!(
                "edge TShape {} has no vertex",
                edge_use.shape
            )));
        };
        let end_use = endpoint_use(TextOrientation::Reversed).unwrap_or_else(|| start_use.clone());
        let start = self.ensure_vertex(topology, &start_use, transform)?;
        let end = self.ensure_vertex(topology, &end_use, transform)?;
        let id = EdgeId(crate::native::model_id(
            "edge",
            &self.payload.id,
            self.topology_label(edge_use.shape, transform),
        ));
        let curve_representation = representations
            .iter()
            .find(|representation| representation.kind == 1);
        let polygon_representation = representations
            .iter()
            .enumerate()
            .find(|(_, representation)| matches!(representation.kind, 5..=7));
        let curve = if degenerated {
            None
        } else if let Some(representation) = curve_representation {
            let carrier_transform =
                multiply(transform, self.tables.location(representation.location));
            Some(self.located_curve(ir, representation.primary, carrier_transform)?)
        } else if let Some((ordinal, representation)) = polygon_representation {
            Some(self.polygon_curve(ir, &id, ordinal, representation, transform)?)
        } else {
            None
        };
        let param_range = curve_representation
            .and_then(|representation| representation.parameter_range)
            .or_else(|| {
                polygon_representation.and_then(|(_, representation)| {
                    self.polygon_parameters(representation)
                        .and_then(|parameters| Some([*parameters.first()?, *parameters.last()?]))
                })
            });
        topology
            .edge(
                id.clone(),
                EdgeSpec {
                    curve,
                    start,
                    end,
                    param_range,
                    tolerance: positive_tolerance(tolerance),
                },
            )
            .expect("edge id is minted once per occurrence, so registration cannot collide");
        self.edges.insert(key, id.clone());
        Ok(id)
    }

    fn polygon_curve(
        &mut self,
        ir: &mut CadIr,
        edge: &EdgeId,
        ordinal: usize,
        representation: &TextEdgeRepresentation,
        transform: Transform,
    ) -> Result<CurveId, CodecError> {
        let carrier_transform = multiply(transform, self.tables.location(representation.location));
        let scale = similarity(carrier_transform)?.scale;
        let (points, parameters, deflection) = match representation.kind {
            5 => {
                let polygon = &self.tables.polygons3d[representation.primary - 1];
                (
                    polygon.nodes.clone(),
                    polygon.parameters.clone(),
                    polygon.deflection,
                )
            }
            6 | 7 => self.indexed_polygon(representation.primary, representation)?,
            _ => {
                return Err(CodecError::Malformed(
                    "non-polygon edge representation reached polygon transfer".into(),
                ))
            }
        };
        let id = CurveId(format!("{}:polygon:{}", edge.0, ordinal + 1));
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Polyline {
                points: points
                    .iter()
                    .map(|point| transform_point(carrier_transform, *point))
                    .collect(),
                parameters,
                chordal_deflection: deflection * scale,
            },
            source_object: Some(self.source_association()),
        });
        if representation.kind == 7 {
            if let Some(secondary) = representation.secondary {
                let (points, parameters, deflection) =
                    self.indexed_polygon(secondary, representation)?;
                ir.model.curves.push(Curve {
                    id: CurveId(format!("{}:polygon:{}:secondary", edge.0, ordinal + 1)),
                    geometry: CurveGeometry::Polyline {
                        points: points
                            .iter()
                            .map(|point| transform_point(carrier_transform, *point))
                            .collect(),
                        parameters,
                        chordal_deflection: deflection * scale,
                    },
                    source_object: Some(self.source_association()),
                });
            }
        }
        Ok(id)
    }

    fn indexed_polygon(
        &self,
        index: usize,
        representation: &TextEdgeRepresentation,
    ) -> Result<IndexedPolygon, CodecError> {
        let polygon = &self.tables.polygons_on_triangulations[index - 1];
        let triangulation_index = representation.surface.ok_or_else(|| {
            CodecError::Malformed("indexed polygon has no triangulation reference".into())
        })?;
        let triangulation = &self.tables.triangulations[triangulation_index - 1];
        let points = polygon
            .nodes
            .iter()
            .map(|node| {
                usize::try_from(*node)
                    .ok()
                    .and_then(|node| node.checked_sub(1))
                    .and_then(|node| triangulation.nodes.get(node).copied())
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "polygon-on-triangulation node is out of bounds".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((points, polygon.parameters.clone(), polygon.deflection))
    }

    fn polygon_parameters(&self, representation: &TextEdgeRepresentation) -> Option<&[f64]> {
        match representation.kind {
            5 => self.tables.polygons3d[representation.primary - 1]
                .parameters
                .as_deref(),
            6 | 7 => self.tables.polygons_on_triangulations[representation.primary - 1]
                .parameters
                .as_deref(),
            _ => None,
        }
    }

    fn ensure_vertex(
        &mut self,
        topology: &mut TopologyBuilder,
        vertex_use: &TextShapeUse,
        parent: Transform,
    ) -> Result<VertexId, CodecError> {
        let transform = multiply(parent, self.tables.location(vertex_use.location));
        let key = OccurrenceKey::new(vertex_use.shape, multiply(self.body_scope, transform));
        if let Some(id) = self.vertices.get(&key) {
            return Ok(id.clone());
        }
        let shape = self.shape(vertex_use.shape)?;
        let TextTShapeGeometry::Vertex {
            tolerance, point, ..
        } = shape.geometry
        else {
            return Err(CodecError::Malformed(format!(
                "TShape {} is not a vertex",
                vertex_use.shape
            )));
        };
        let label = self.topology_label(vertex_use.shape, transform);
        let point_id = PointId(crate::native::model_id("point", &self.payload.id, &label));
        let vertex_id = VertexId(crate::native::model_id("vertex", &self.payload.id, &label));
        topology
            .point(
                point_id.clone(),
                transform_point(transform, point),
                Some(SourceObjectAssociation {
                    format: "fcstd".into(),
                    object_id: self.source_object.clone(),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            )
            .expect("point id is minted once per occurrence, so registration cannot collide");
        let vertex_tolerance = positive_tolerance(tolerance * similarity(transform)?.scale);
        topology
            .vertex(vertex_id.clone(), point_id, vertex_tolerance)
            .expect("vertex id is minted once per occurrence, so registration cannot collide");
        self.vertices.insert(key, vertex_id.clone());
        Ok(vertex_id)
    }

    fn located_curve(
        &mut self,
        ir: &mut CadIr,
        source: usize,
        transform: Transform,
    ) -> Result<CurveId, CodecError> {
        let base_id = CurveId(crate::native::model_id(
            "curve",
            &self.payload.id,
            source.to_string(),
        ));
        if is_identity(transform) {
            return Ok(base_id);
        }
        let id = CurveId(crate::native::model_id(
            "curve",
            &self.payload.id,
            format!("{}@{}", source, transform_digest(transform)),
        ));
        if self.emitted_curves.insert(id.clone()) {
            let base = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == base_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("missing curve table entry {source}"))
                })?
                .clone();
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: transform_curve(&base.geometry, transform)?,
                source_object: base.source_object,
            });
        }
        Ok(id)
    }

    fn located_surface(
        &mut self,
        ir: &mut CadIr,
        source: usize,
        transform: Transform,
    ) -> Result<SurfaceId, CodecError> {
        let base_id = SurfaceId(crate::native::model_id(
            "surface",
            &self.payload.id,
            source.to_string(),
        ));
        if is_identity(transform) {
            return Ok(base_id);
        }
        let id = SurfaceId(crate::native::model_id(
            "surface",
            &self.payload.id,
            format!("{}@{}", source, transform_digest(transform)),
        ));
        if self.emitted_surfaces.insert(id.clone()) {
            let base = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == base_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("missing surface table entry {source}"))
                })?
                .clone();
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: transform_surface(&base.geometry, transform)?,
                source_object: base.source_object,
            });
        }
        Ok(id)
    }

    fn face_pcurve(
        &self,
        edge_use: &TextShapeUse,
        edge_transform: Transform,
        surface: usize,
        surface_transform: Transform,
    ) -> Option<(PcurveId, Option<[f64; 2]>)> {
        let TextTShapeGeometry::Edge {
            representations, ..
        } = &self.tables.tshapes[edge_use.shape - 1].geometry
        else {
            return None;
        };
        representations
            .iter()
            .enumerate()
            .find(|(_, representation)| {
                matches!(representation.kind, 2 | 3)
                    && representation.surface == Some(surface)
                    && transforms_equal(
                        multiply(
                            edge_transform,
                            self.tables.location(representation.location),
                        ),
                        surface_transform,
                    )
            })
            .map(|(index, representation)| {
                let reversed = is_reversed(edge_use.orientation);
                (
                    self.pcurve_id(
                        edge_use.shape,
                        index,
                        representation.secondary.is_some() && reversed,
                    ),
                    representation.parameter_range,
                )
            })
    }

    fn shape(&self, index: usize) -> Result<&TextTShape, CodecError> {
        self.tables
            .tshapes
            .get(index - 1)
            .ok_or_else(|| CodecError::Malformed(format!("missing TShape {index}")))
    }

    fn topology_label(&self, shape: usize, local: Transform) -> String {
        occurrence_label(shape, multiply(self.body_scope, local))
    }
}

fn positive_tolerance(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

pub(crate) fn pcurve_geometry(curve: &TextCurve2d) -> PcurveGeometry {
    match curve {
        TextCurve2d::Line { origin, direction } => PcurveGeometry::Line {
            origin: *origin,
            direction: *direction,
        },
        TextCurve2d::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => PcurveGeometry::Circle {
            center: *center,
            x_axis: *x_axis,
            y_axis: *y_axis,
            radius: *radius,
        },
        TextCurve2d::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => PcurveGeometry::Ellipse {
            center: *center,
            x_axis: *x_axis,
            y_axis: *y_axis,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        TextCurve2d::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } => PcurveGeometry::Parabola {
            vertex: *vertex,
            x_axis: *x_axis,
            y_axis: *y_axis,
            focal_distance: *focal_distance,
        },
        TextCurve2d::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => PcurveGeometry::Hyperbola {
            center: *center,
            x_axis: *x_axis,
            y_axis: *y_axis,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        TextCurve2d::Nurbs(nurbs) => PcurveGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots.clone(),
            control_points: nurbs.control_points.clone(),
            weights: nurbs.weights.clone(),
            periodic: nurbs.periodic,
        },
        TextCurve2d::Trimmed {
            parameter_range,
            basis,
        } => PcurveGeometry::Trimmed {
            parameter_range: *parameter_range,
            basis: Box::new(pcurve_geometry(basis)),
        },
        TextCurve2d::Offset { distance, basis } => PcurveGeometry::Offset {
            distance: *distance,
            basis: Box::new(pcurve_geometry(basis)),
        },
    }
}

fn scale_pcurve_v(geometry: &mut PcurveGeometry, scale: f64) {
    let scale_point = |point: &mut Point2| point.v *= scale;
    match geometry {
        PcurveGeometry::Line { origin, direction } => {
            scale_point(origin);
            scale_point(direction);
        }
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            ..
        }
        | PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            ..
        }
        | PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            ..
        } => {
            scale_point(center);
            scale_point(x_axis);
            scale_point(y_axis);
        }
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            ..
        } => {
            scale_point(vertex);
            scale_point(x_axis);
            scale_point(y_axis);
        }
        PcurveGeometry::Nurbs { control_points, .. } => {
            control_points.iter_mut().for_each(scale_point);
        }
        PcurveGeometry::PolarHarmonic {
            axial_origin,
            axial_cos,
            axial_sin,
            ..
        } => {
            *axial_origin *= scale;
            *axial_cos *= scale;
            *axial_sin *= scale;
        }
        PcurveGeometry::PolarNurbs {
            axial_control_points,
            ..
        } => {
            for value in axial_control_points {
                *value *= scale;
            }
        }
        PcurveGeometry::Trimmed { basis, .. } | PcurveGeometry::Offset { basis, .. } => {
            scale_pcurve_v(basis, scale);
        }
    }
}

fn scale_pcurve_u(geometry: &mut PcurveGeometry, scale: f64) {
    let scale_point = |point: &mut Point2| point.u *= scale;
    match geometry {
        PcurveGeometry::Line { origin, direction } => {
            scale_point(origin);
            scale_point(direction);
        }
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            ..
        }
        | PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            ..
        }
        | PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            ..
        } => {
            scale_point(center);
            scale_point(x_axis);
            scale_point(y_axis);
        }
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            ..
        } => {
            scale_point(vertex);
            scale_point(x_axis);
            scale_point(y_axis);
        }
        PcurveGeometry::Nurbs { control_points, .. } => {
            control_points.iter_mut().for_each(scale_point);
        }
        PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            ..
        } => {
            debug_assert_eq!(scale, -1.0);
            radial_center.v = -radial_center.v;
            radial_cos.v = -radial_cos.v;
            radial_sin.v = -radial_sin.v;
        }
        PcurveGeometry::PolarNurbs {
            radial_control_points,
            ..
        } => {
            debug_assert_eq!(scale, -1.0);
            for point in radial_control_points {
                point.v = -point.v;
            }
        }
        PcurveGeometry::Trimmed { basis, .. } | PcurveGeometry::Offset { basis, .. } => {
            scale_pcurve_u(basis, scale);
        }
    }
}

#[derive(Clone, Copy)]
struct Similarity {
    scale: f64,
}

fn similarity(transform: Transform) -> Result<Similarity, CodecError> {
    let columns = [
        Vector3::new(
            transform.rows[0][0],
            transform.rows[1][0],
            transform.rows[2][0],
        ),
        Vector3::new(
            transform.rows[0][1],
            transform.rows[1][1],
            transform.rows[2][1],
        ),
        Vector3::new(
            transform.rows[0][2],
            transform.rows[1][2],
            transform.rows[2][2],
        ),
    ];
    let scale = columns[0].norm();
    let tolerance = 1.0e-10 * scale.max(1.0);
    if !scale.is_finite()
        || scale <= 0.0
        || columns
            .iter()
            .any(|column| (column.norm() - scale).abs() > tolerance)
        || dot(columns[0], columns[1]).abs() > tolerance
        || dot(columns[0], columns[2]).abs() > tolerance
        || dot(columns[1], columns[2]).abs() > tolerance
    {
        return Err(CodecError::Malformed(
            "B-rep location is not a finite similarity transform".into(),
        ));
    }
    Ok(Similarity { scale })
}

fn transform_curve(
    geometry: &CurveGeometry,
    transform: Transform,
) -> Result<CurveGeometry, CodecError> {
    similarity(transform)?;
    Ok(CurveGeometry::Transformed {
        basis: Box::new(geometry.clone()),
        transform,
    })
}

fn transform_surface(
    geometry: &SurfaceGeometry,
    transform: Transform,
) -> Result<SurfaceGeometry, CodecError> {
    similarity(transform)?;
    Ok(SurfaceGeometry::Transformed {
        basis: Box::new(geometry.clone()),
        transform,
    })
}

fn transform_point(transform: Transform, point: Point3) -> Point3 {
    Point3::new(
        transform.rows[0][0] * point.x
            + transform.rows[0][1] * point.y
            + transform.rows[0][2] * point.z
            + transform.rows[0][3],
        transform.rows[1][0] * point.x
            + transform.rows[1][1] * point.y
            + transform.rows[1][2] * point.z
            + transform.rows[1][3],
        transform.rows[2][0] * point.x
            + transform.rows[2][1] * point.y
            + transform.rows[2][2] * point.z
            + transform.rows[2][3],
    )
}

fn transform_vector(transform: Transform, vector: Vector3) -> Vector3 {
    let transformed = Vector3::new(
        transform.rows[0][0] * vector.x
            + transform.rows[0][1] * vector.y
            + transform.rows[0][2] * vector.z,
        transform.rows[1][0] * vector.x
            + transform.rows[1][1] * vector.y
            + transform.rows[1][2] * vector.z,
        transform.rows[2][0] * vector.x
            + transform.rows[2][1] * vector.y
            + transform.rows[2][2] * vector.z,
    );
    let magnitude = (transformed.x * transformed.x
        + transformed.y * transformed.y
        + transformed.z * transformed.z)
        .sqrt();
    if magnitude > 0.0 && magnitude.is_finite() {
        Vector3::new(
            transformed.x / magnitude,
            transformed.y / magnitude,
            transformed.z / magnitude,
        )
    } else {
        transformed
    }
}

fn multiply(left: Transform, right: Transform) -> Transform {
    let mut rows = [[0.0; 4]; 4];
    for (row, values) in rows.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = (0..4)
                .map(|inner| left.rows[row][inner] * right.rows[inner][column])
                .sum();
        }
    }
    Transform { rows }
}

fn occurrence_label(shape: usize, transform: Transform) -> String {
    if is_identity(transform) {
        shape.to_string()
    } else {
        format!("{}@{}", shape, transform_digest(transform))
    }
}

fn transform_digest(transform: Transform) -> String {
    let mut bytes = Vec::with_capacity(16 * 8);
    for row in transform.rows {
        for value in row {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }
    sha256_hex(&bytes)[..16].to_owned()
}

fn transform_bits(transform: Transform) -> [u64; 16] {
    let mut output = [0; 16];
    for (target, value) in output.iter_mut().zip(transform.rows.into_iter().flatten()) {
        *target = value.to_bits();
    }
    output
}

fn is_identity(transform: Transform) -> bool {
    transforms_equal(transform, Transform::identity())
}

fn transforms_equal(left: Transform, right: Transform) -> bool {
    left.rows
        .into_iter()
        .flatten()
        .zip(right.rows.into_iter().flatten())
        .all(|(left, right)| left.to_bits() == right.to_bits() || (left - right).abs() <= 1.0e-12)
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
        by_edge.entry(coedge.edge.clone()).or_default().push(index);
    }
    for indices in by_edge.values() {
        for (position, index) in indices.iter().enumerate() {
            let next = indices[(position + 1) % indices.len()];
            coedges[*index].radial_next = coedges[next].id.clone();
        }
    }
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}
