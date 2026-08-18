// SPDX-License-Identifier: Apache-2.0
//! Occurrence-aware transfer of exact-shape topology into neutral CADIR.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, PcurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::{Transform, Transform2};
use cadmpeg_ir::SourceObjectAssociation;

use crate::brep::{
    surface_parameter_affine, ShapePayloadRecord, SurfaceParameterAffine, TextCurve, TextCurve2d,
    TextEdgeRepresentation, TextLocation, TextOrientation, TextPolygon3d,
    TextPolygonOnTriangulation, TextShapeKind, TextShapeUse, TextSurface, TextTShape,
    TextTShapeGeometry, TextTriangulation,
};
use crate::native::PropertyRecord;

type IndexedPolygon = (Vec<Point3>, Option<Vec<f64>>, f64);
type FacePcurve = (PcurveId, Option<[f64; 2]>);

pub(crate) struct TopologyOccurrence {
    pub(crate) property: String,
    pub(crate) indexed_name: &'static str,
    pub(crate) source_index: usize,
    pub(crate) topology_id: String,
}

/// Transfer text or binary shape-set topology with placements applied once.
pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<TopologyOccurrence>, CodecError> {
    let mut occurrences = Vec::new();
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
            builder.append_body(ctx, ir, root)?;
        }
        builder.emit_unowned_triangulations(ir);
        occurrences.extend(builder.occurrences);
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
    Ok(occurrences)
}

#[derive(Clone, Copy)]
struct Tables<'a> {
    locations: &'a [TextLocation],
    curve2ds: &'a [TextCurve2d],
    curves: &'a [TextCurve],
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
                curves: &text.curves,
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
                    curves: &binary.curves,
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
    root_ordinal: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OccurrenceKey(String);

impl OccurrenceKey {
    fn new(shape: usize, transform: Transform) -> Self {
        Self(occurrence_label(shape, transform))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SourceOccurrenceKey(String);

impl SourceOccurrenceKey {
    fn new(shape: usize, transform: Transform) -> Self {
        Self(format!("{}@{}", shape, exact_transform_digest(transform)))
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
    root_discriminator: Option<usize>,
    current_body: Option<BodyId>,
    source_object: String,
    source_indices: HashMap<(TextShapeKind, SourceOccurrenceKey), usize>,
    occurrences: Vec<TopologyOccurrence>,
}

impl<'a> Builder<'a> {
    fn new(payload: &'a ShapePayloadRecord, tables: Tables<'a>, source_object: String) -> Self {
        let source_indices = source_topology_indices(tables);
        Self {
            payload,
            tables,
            vertices: HashMap::new(),
            edges: HashMap::new(),
            emitted_curves: HashSet::new(),
            emitted_surfaces: HashSet::new(),
            emitted_triangulations: HashSet::new(),
            body_scope: Transform::identity(),
            root_discriminator: None,
            current_body: None,
            source_object,
            source_indices,
            occurrences: Vec::new(),
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

    fn bind_topology(
        &mut self,
        kind: TextShapeKind,
        shape: usize,
        local: Transform,
        topology_id: String,
    ) {
        let key = SourceOccurrenceKey::new(shape, self.body_scope.compose(local));
        let Some(source_index) = self.source_indices.get(&(kind, key)).copied() else {
            return;
        };
        self.occurrences.push(TopologyOccurrence {
            property: self.payload.property.clone(),
            indexed_name: indexed_name(kind),
            source_index,
            topology_id,
        });
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
                let parameter_affine = representation
                    .surface
                    .and_then(|surface| self.tables.surfaces.get(surface - 1))
                    .map(surface_parameter_affine);
                let primary_geometry = transformed_pcurve_geometry(
                    pcurve_geometry(&self.tables.curve2ds[representation.primary - 1]),
                    parameter_affine,
                );
                let primary_range = normalize_pcurve_parameter_range(
                    &primary_geometry,
                    representation.parameter_range,
                );
                ir.model.pcurves.push(Pcurve {
                    id: self.pcurve_id(shape.index, representation_index, false),
                    geometry: primary_geometry,
                    wrapper_reversed: None,
                    native_tail_flags: None,
                    parameter_range: primary_range,
                    fit_tolerance: None,
                });
                if let Some(secondary) = representation.secondary {
                    let secondary_geometry = transformed_pcurve_geometry(
                        pcurve_geometry(&self.tables.curve2ds[secondary - 1]),
                        parameter_affine,
                    );
                    let secondary_range = normalize_pcurve_parameter_range(
                        &secondary_geometry,
                        representation.parameter_range,
                    );
                    ir.model.pcurves.push(Pcurve {
                        id: self.pcurve_id(shape.index, representation_index, true),
                        geometry: secondary_geometry,
                        wrapper_reversed: None,
                        native_tail_flags: None,
                        parameter_range: secondary_range,
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
                feature_edges: Vec::new(),
                strip_lengths: Vec::new(),
                normals: triangulation.normals.clone().unwrap_or_default(),
                corner_normals: Vec::new(),
                triangle_groups: Vec::new(),
                texture_assignments: Vec::new(),
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
        let has_multiple_roots = self.tables.roots.len() > 1;
        self.tables
            .roots
            .iter()
            .enumerate()
            .map(|(index, root)| {
                self.shape(root.shape)?;
                let transform = self.tables.location(root.location);
                Ok(BodyRoot {
                    shape: root.shape,
                    transform,
                    reversed: is_reversed(root.orientation),
                    root_ordinal: has_multiple_roots.then_some(index + 1),
                })
            })
            .collect()
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
        let body_key = self.topology_label(root.shape, Transform::identity());
        let body_id = BodyId(crate::native::model_id("body", &self.payload.id, &body_key));
        self.current_body = Some(body_id.clone());
        let kind = match root_kind {
            TextShapeKind::Solid => BodyKind::Solid,
            TextShapeKind::Wire | TextShapeKind::Edge => BodyKind::Wire,
            TextShapeKind::Shell | TextShapeKind::Face => BodyKind::Sheet,
            _ => BodyKind::General,
        };
        let tessellation_start = ir.model.tessellations.len();
        let mut regions = Vec::new();
        self.append_shape_regions(
            ctx,
            ir,
            &body_id,
            root.shape,
            Transform::identity(),
            root.reversed,
            &mut regions,
        )?;
        if regions.is_empty() {
            ir.model.tessellations.truncate(tessellation_start);
            return Ok(());
        }
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind,
            regions,
            transform: (!is_identity(root.transform)).then_some(root.transform),
            name: None,
            color: None,
            visible: None,
        });
        if matches!(
            root_kind,
            TextShapeKind::Compound | TextShapeKind::CompSolid
        ) {
            self.bind_topology(root_kind, root.shape, Transform::identity(), body_id.0);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn append_shape_regions(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        body: &BodyId,
        shape_index: usize,
        transform: Transform,
        reversed: bool,
        output: &mut Vec<RegionId>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("transfer FCStd topology nesting", None)?;
        let shape = self.shape(shape_index)?.clone();
        if matches!(
            shape.kind,
            TextShapeKind::Compound | TextShapeKind::CompSolid
        ) {
            for child in &shape.children {
                self.append_shape_regions(
                    ctx,
                    ir,
                    body,
                    child.shape,
                    transform.compose(self.tables.location(child.location)),
                    reversed ^ is_reversed(child.orientation),
                    output,
                )?;
            }
            return Ok(());
        }
        let key = self.topology_label(shape_index, transform);
        let region_id = RegionId(crate::native::model_id("region", &self.payload.id, &key));
        let mut shells = Vec::new();
        if shape.kind == TextShapeKind::Solid {
            for child in shape
                .children
                .iter()
                .filter(|child| self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Shell)
            {
                shells.extend(self.append_shell(ir, &region_id, child, transform, reversed)?);
            }
        } else {
            shells.extend(self.append_shell_shape(
                ir,
                &region_id,
                shape_index,
                transform,
                reversed,
            )?);
        }
        if !shells.is_empty() {
            ir.model.regions.push(Region {
                id: region_id.clone(),
                body: body.clone(),
                shells,
            });
            if shape.kind == TextShapeKind::Solid {
                self.bind_topology(
                    TextShapeKind::Solid,
                    shape_index,
                    transform,
                    region_id.0.clone(),
                );
            }
            output.push(region_id);
        }
        Ok(())
    }

    fn append_shell(
        &mut self,
        ir: &mut CadIr,
        region: &RegionId,
        shell_use: &TextShapeUse,
        parent: Transform,
        reversed: bool,
    ) -> Result<Vec<ShellId>, CodecError> {
        let transform = parent.compose(self.tables.location(shell_use.location));
        self.append_shell_shape(
            ir,
            region,
            shell_use.shape,
            transform,
            reversed ^ is_reversed(shell_use.orientation),
        )
    }

    fn append_shell_shape(
        &mut self,
        ir: &mut CadIr,
        region: &RegionId,
        shape_index: usize,
        transform: Transform,
        reversed: bool,
    ) -> Result<Vec<ShellId>, CodecError> {
        let shape = self.shape(shape_index)?.clone();
        let key = self.topology_label(shape_index, transform);
        let shell_id = ShellId(crate::native::model_id("shell", &self.payload.id, &key));
        if shape.kind == TextShapeKind::Shell {
            let face_uses = shape
                .children
                .iter()
                .filter(|child| self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Face)
                .collect::<Vec<_>>();
            let components = self.face_components(&face_uses, transform)?;
            let mut shell_ids = Vec::with_capacity(components.len());
            for (component_index, component) in components.iter().enumerate() {
                let component_id = if component_index == 0 {
                    shell_id.clone()
                } else {
                    ShellId(crate::native::model_id(
                        "shell",
                        &self.payload.id,
                        format!("{key}:component:{}", component_index + 1),
                    ))
                };
                let mut faces = Vec::with_capacity(component.len());
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
                ir.model.shells.push(Shell {
                    id: component_id.clone(),
                    region: region.clone(),
                    faces,
                    wire_edges: Vec::new(),
                    free_vertices: Vec::new(),
                });
                self.bind_topology(
                    TextShapeKind::Shell,
                    shape_index,
                    transform,
                    component_id.0.clone(),
                );
                shell_ids.push(component_id);
            }
            return Ok(shell_ids);
        }
        let mut faces = Vec::new();
        let mut wire_edges = Vec::new();
        match shape.kind {
            TextShapeKind::Face => {
                let shape_use = TextShapeUse {
                    shape: shape_index,
                    orientation: TextOrientation::Forward,
                    location: 0,
                };
                if let Some(face) =
                    self.append_face(ir, &shell_id, &shape_use, transform, reversed)?
                {
                    faces.push(face);
                }
            }
            TextShapeKind::Wire => {
                for child in &shape.children {
                    if self.shape(child.shape)?.kind == TextShapeKind::Edge {
                        wire_edges.push(self.ensure_edge(ir, child, transform)?);
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
                wire_edges.push(self.ensure_edge(ir, &edge_use, transform)?);
            }
            TextShapeKind::Vertex => {
                let vertex_use = TextShapeUse {
                    shape: shape_index,
                    orientation: TextOrientation::Forward,
                    location: 0,
                };
                let vertex = self.ensure_vertex(ir, &vertex_use, transform)?;
                ir.model.shells.push(Shell {
                    id: shell_id.clone(),
                    region: region.clone(),
                    faces,
                    wire_edges,
                    free_vertices: vec![vertex],
                });
                return Ok(vec![shell_id]);
            }
            _ => {}
        }
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region.clone(),
            faces,
            wire_edges,
            free_vertices: Vec::new(),
        });
        if shape.kind == TextShapeKind::Wire {
            self.bind_topology(shape.kind, shape_index, transform, shell_id.0.clone());
        }
        Ok(vec![shell_id])
    }

    fn face_components(
        &self,
        face_uses: &[&TextShapeUse],
        parent: Transform,
    ) -> Result<Vec<Vec<usize>>, CodecError> {
        if face_uses.is_empty() {
            return Ok(vec![Vec::new()]);
        }
        let mut connectivity = Vec::with_capacity(face_uses.len());
        for face_use in face_uses {
            let face_transform = parent.compose(self.tables.location(face_use.location));
            let face = self.shape(face_use.shape)?;
            let mut keys = HashSet::new();
            for wire_use in face
                .children
                .iter()
                .filter(|child| self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Wire)
            {
                let wire_transform =
                    face_transform.compose(self.tables.location(wire_use.location));
                let wire = self.shape(wire_use.shape)?;
                for edge_use in wire.children.iter().filter(|child| {
                    self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Edge
                }) {
                    let edge_transform =
                        wire_transform.compose(self.tables.location(edge_use.location));
                    let edge_key =
                        OccurrenceKey::new(edge_use.shape, self.body_scope.compose(edge_transform));
                    keys.insert(format!("edge:{}", edge_key.0));
                    let edge = self.shape(edge_use.shape)?;
                    for vertex_use in edge.children.iter().filter(|child| {
                        self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Vertex
                    }) {
                        let vertex_transform =
                            edge_transform.compose(self.tables.location(vertex_use.location));
                        let vertex_key = OccurrenceKey::new(
                            vertex_use.shape,
                            self.body_scope.compose(vertex_transform),
                        );
                        keys.insert(format!("vertex:{}", vertex_key.0));
                    }
                }
            }
            connectivity.push(keys);
        }

        Ok(connected_components(&connectivity))
    }

    fn append_face(
        &mut self,
        ir: &mut CadIr,
        shell: &ShellId,
        face_use: &TextShapeUse,
        parent: Transform,
        reversed: bool,
    ) -> Result<Option<FaceId>, CodecError> {
        let face_transform = parent.compose(self.tables.location(face_use.location));
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
            return Ok(None);
        };
        let surface_transform = face_transform.compose(self.tables.location(location));
        let face_key = self.topology_label(face_use.shape, face_transform);
        let face_id = FaceId(crate::native::model_id("face", &self.payload.id, &face_key));
        // OCCT triangulation nodes are already expressed in the face's surface-location frame.
        // Only the owning topological face placement remains to be applied here.
        let located_triangulation = triangulation.map(|index| {
            let triangulation = &self.tables.triangulations[index - 1];
            let vertices = triangulation
                .nodes
                .iter()
                .map(|point| face_transform.apply_point(*point))
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
            return Ok(None);
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
                        .map(|normal| transform_normalized_vector(face_transform, *normal))
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
                feature_edges: Vec::new(),
                strip_lengths: Vec::new(),
                normals,
                corner_normals: Vec::new(),
                triangle_groups: Vec::new(),
                texture_assignments: Vec::new(),
                channels: Vec::new(),
            });
        }
        let mut loops = Vec::new();
        for (loop_index, wire_use) in shape
            .children
            .iter()
            .filter(|child| self.tables.tshapes[child.shape - 1].kind == TextShapeKind::Wire)
            .enumerate()
        {
            let wire_transform = face_transform.compose(self.tables.location(wire_use.location));
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
            let coedge_ids = (0..edge_uses.len())
                .map(|index| {
                    CoedgeId(crate::native::model_id(
                        "coedge",
                        &self.payload.id,
                        format!("{}:{}:{}", face_key, loop_index + 1, index + 1),
                    ))
                })
                .collect::<Vec<_>>();
            for (index, edge_use) in edge_uses.iter().enumerate() {
                let edge_transform =
                    wire_transform.compose(self.tables.location(edge_use.location));
                let edge = self.ensure_edge(ir, edge_use, wire_transform)?;
                let pcurve = self.face_pcurve(edge_use, edge_transform, surface, surface_transform);
                let id = coedge_ids[index].clone();
                ir.model.coedges.push(Coedge {
                    id: id.clone(),
                    owner_loop: loop_id.clone(),
                    edge,
                    next: coedge_ids[(index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()].clone(),
                    radial_next: id,
                    sense: sense(is_reversed(edge_use.orientation) ^ wire_reversed),
                    use_curve: None,
                    use_curve_parameter_range: None,
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
                });
            }
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                coedges: coedge_ids,
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                vertex_uses: Vec::new(),
            });
            self.bind_topology(
                TextShapeKind::Wire,
                wire_use.shape,
                wire_transform,
                loop_id.0.clone(),
            );
            loops.push(loop_id);
        }
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: shell.clone(),
            surface: surface_id,
            sense: sense(face_reversed),
            loops,
            name: None,
            color: None,
            tolerance: positive_tolerance(tolerance),
        });
        self.bind_topology(
            TextShapeKind::Face,
            face_use.shape,
            face_transform,
            face_id.0.clone(),
        );
        Ok(Some(face_id))
    }

    fn ensure_edge(
        &mut self,
        ir: &mut CadIr,
        edge_use: &TextShapeUse,
        parent: Transform,
    ) -> Result<EdgeId, CodecError> {
        let transform = parent.compose(self.tables.location(edge_use.location));
        let key = OccurrenceKey::new(edge_use.shape, self.body_scope.compose(transform));
        if let Some(id) = self.edges.get(&key).cloned() {
            self.bind_topology(TextShapeKind::Edge, edge_use.shape, transform, id.0.clone());
            return Ok(id);
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
        let (start_use, end_use) = edge_endpoint_uses(edge_use.shape, &shape.children)?;
        let start = self.ensure_vertex(ir, start_use, transform)?;
        let end = self.ensure_vertex(ir, end_use, transform)?;
        let id = EdgeId(crate::native::model_id(
            "edge",
            &self.payload.id,
            self.topology_label(edge_use.shape, transform),
        ));
        let curve_representation =
            select_exact_curve_representation(edge_use.shape, &representations, &self.tables)?;
        let polygon_representation = if curve_representation.is_none() {
            unique_fallback_polygon_representation(edge_use.shape, &representations)?
        } else {
            None
        };
        let curve = if degenerated {
            None
        } else if let Some((_, representation)) = curve_representation {
            let carrier_transform =
                transform.compose(self.tables.location(representation.location));
            Some(self.located_curve(ir, representation.primary, carrier_transform)?)
        } else if let Some((ordinal, representation)) = polygon_representation {
            Some(self.polygon_curve(ir, &id, ordinal, representation, transform)?)
        } else {
            None
        };
        let param_range = curve_representation
            .and_then(|(_, representation)| representation.parameter_range)
            .or_else(|| {
                polygon_representation.and_then(|(_, representation)| {
                    self.polygon_parameters(representation)
                        .and_then(|parameters| Some([*parameters.first()?, *parameters.last()?]))
                })
            });
        let param_range = curve
            .as_ref()
            .and_then(|curve| ir.model.curves.iter().find(|item| item.id == *curve))
            .map_or(param_range, |curve| {
                normalize_occt_curve_range(&curve.geometry, param_range)
            });
        ir.model.edges.push(Edge {
            id: id.clone(),
            curve,
            start,
            end,
            param_range,
            tolerance: positive_tolerance(tolerance),
        });
        self.bind_topology(TextShapeKind::Edge, edge_use.shape, transform, id.0.clone());
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
        let carrier_transform = transform.compose(self.tables.location(representation.location));
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
                    .map(|point| carrier_transform.apply_point(*point))
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
                            .map(|point| carrier_transform.apply_point(*point))
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
        ir: &mut CadIr,
        vertex_use: &TextShapeUse,
        parent: Transform,
    ) -> Result<VertexId, CodecError> {
        let transform = parent.compose(self.tables.location(vertex_use.location));
        let key = OccurrenceKey::new(vertex_use.shape, self.body_scope.compose(transform));
        if let Some(id) = self.vertices.get(&key).cloned() {
            self.bind_topology(
                TextShapeKind::Vertex,
                vertex_use.shape,
                transform,
                id.0.clone(),
            );
            return Ok(id);
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
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: transform.apply_point(point),
            source_object: Some(SourceObjectAssociation {
                format: "fcstd".into(),
                object_id: self.source_object.clone(),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: positive_tolerance(tolerance * similarity(transform)?.scale),
        });
        self.bind_topology(
            TextShapeKind::Vertex,
            vertex_use.shape,
            transform,
            vertex_id.0.clone(),
        );
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
            let has_procedural_construction = ir
                .model
                .procedural_surfaces
                .iter()
                .any(|surface| surface.surface == base_id);
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: transform_surface(&base.geometry, transform)?,
                source_object: base.source_object,
            });
            if has_procedural_construction {
                ir.model.procedural_surfaces.push(ProceduralSurface {
                    id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                    surface: id.clone(),
                    definition: ProceduralSurfaceDefinition::Replica {
                        source: base_id,
                        transform,
                    },
                    record_bounds: None,
                    cache_fit_tolerance: None,
                });
            }
        }
        Ok(id)
    }

    fn face_pcurve(
        &self,
        edge_use: &TextShapeUse,
        edge_transform: Transform,
        surface: usize,
        surface_transform: Transform,
    ) -> Option<FacePcurve> {
        let TextTShapeGeometry::Edge {
            degenerated,
            representations,
            ..
        } = &self.tables.tshapes[edge_use.shape - 1].geometry
        else {
            return None;
        };
        let (index, representation) =
            first_edge_representation(representations, |representation| {
                matches!(representation.kind, 2 | 3)
                    && representation.surface == Some(surface)
                    && transforms_equal(
                        edge_transform.compose(self.tables.location(representation.location)),
                        surface_transform,
                    )
            })?;
        let reversed = is_reversed(edge_use.orientation);
        let secondary = representation.secondary.is_some() && reversed;
        let curve_index = if secondary {
            representation
                .secondary
                .expect("secondary representation exists")
        } else {
            representation.primary
        };
        let geometry = pcurve_geometry(&self.tables.curve2ds[curve_index - 1]);
        let parameter_range =
            normalize_pcurve_parameter_range(&geometry, representation.parameter_range);
        Some((
            self.pcurve_id(edge_use.shape, index, secondary),
            bounded_pcurve_range(*degenerated, parameter_range),
        ))
    }

    fn shape(&self, index: usize) -> Result<&TextTShape, CodecError> {
        self.tables
            .tshapes
            .get(index - 1)
            .ok_or_else(|| CodecError::Malformed(format!("missing TShape {index}")))
    }

    fn topology_label(&self, shape: usize, local: Transform) -> String {
        let label = occurrence_label(shape, self.body_scope.compose(local));
        self.root_discriminator
            .map_or(label.clone(), |ordinal| format!("{label}~root{ordinal}"))
    }
}

fn bounded_pcurve_range(degenerated: bool, range: Option<[f64; 2]>) -> Option<[f64; 2]> {
    (!degenerated)
        .then_some(range)
        .flatten()
        .filter(|range| range[0] < range[1])
}

fn normalize_pcurve_parameter_range(
    geometry: &PcurveGeometry,
    range: Option<[f64; 2]>,
) -> Option<[f64; 2]> {
    let mut range = range?;
    let domain = match geometry {
        PcurveGeometry::Nurbs { degree, knots, .. } => {
            let degree = usize::try_from(*degree).ok()?;
            [
                *knots.get(degree)?,
                *knots.get(knots.len().checked_sub(degree + 1)?)?,
            ]
        }
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => *parameter_range,
        PcurveGeometry::Offset { basis, .. } | PcurveGeometry::Transformed { basis, .. } => {
            return normalize_pcurve_parameter_range(basis, Some(range));
        }
        _ => return Some(range),
    };
    let scale = range
        .into_iter()
        .chain(domain)
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let tolerance = scale * 1.0e-9;
    for value in &mut range {
        if (*value - domain[0]).abs() <= tolerance {
            *value = domain[0];
        } else if (*value - domain[1]).abs() <= tolerance {
            *value = domain[1];
        }
    }
    Some(range)
}

fn connected_components(connectivity: &[HashSet<String>]) -> Vec<Vec<usize>> {
    let mut assigned = vec![false; connectivity.len()];
    let mut components = Vec::new();
    for seed in 0..connectivity.len() {
        if assigned[seed] {
            continue;
        }
        assigned[seed] = true;
        let mut component = Vec::new();
        let mut stack = vec![seed];
        while let Some(current) = stack.pop() {
            component.push(current);
            for candidate in 0..connectivity.len() {
                if !assigned[candidate]
                    && !connectivity[current].is_disjoint(&connectivity[candidate])
                {
                    assigned[candidate] = true;
                    stack.push(candidate);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components
}

fn transformed_pcurve_geometry(
    geometry: PcurveGeometry,
    affine: Option<SurfaceParameterAffine>,
) -> PcurveGeometry {
    let Some(affine) = affine else {
        return geometry;
    };
    if affine
        == (SurfaceParameterAffine {
            u_scale: 1.0,
            u_offset: 0.0,
            v_scale: 1.0,
            v_offset: 0.0,
        })
    {
        return geometry;
    }
    PcurveGeometry::Transformed {
        basis: Box::new(geometry),
        transform: Transform2 {
            rows: [
                [affine.u_scale, 0.0, affine.u_offset],
                [0.0, affine.v_scale, affine.v_offset],
                [0.0, 0.0, 1.0],
            ],
        },
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
            same_sense: true,
            basis: Box::new(pcurve_geometry(basis)),
        },
        TextCurve2d::Offset { distance, basis } => PcurveGeometry::Offset {
            distance: *distance,
            basis: Box::new(pcurve_geometry(basis)),
        },
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
        || columns[0].dot(columns[1]).abs() > tolerance
        || columns[0].dot(columns[2]).abs() > tolerance
        || columns[1].dot(columns[2]).abs() > tolerance
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

fn transform_normalized_vector(transform: Transform, vector: Vector3) -> Vector3 {
    let transformed = transform.apply_vector(vector);
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

fn occurrence_label(shape: usize, transform: Transform) -> String {
    if is_identity(transform) {
        shape.to_string()
    } else {
        format!("{}@{}", shape, transform_digest(transform))
    }
}

fn source_topology_indices(
    tables: Tables<'_>,
) -> HashMap<(TextShapeKind, SourceOccurrenceKey), usize> {
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
        for root in tables.roots {
            let mut stack = vec![(root.clone(), Transform::identity())];
            while let Some((shape_use, parent)) = stack.pop() {
                let transform = parent.compose(tables.location(shape_use.location));
                let shape = &tables.tshapes[shape_use.shape - 1];
                if shape.kind == target {
                    let key = SourceOccurrenceKey::new(shape_use.shape, transform);
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        indices.entry((target, key))
                    {
                        entry.insert(next_index);
                        next_index += 1;
                    }
                }
                if topology_rank(shape.kind) <= topology_rank(target) {
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
    indices
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
    let mut bytes = Vec::with_capacity(16 * 8);
    for row in transform.rows {
        for value in row {
            // TopLoc locations can encode the same placement through different
            // factor chains. Matrix composition then leaves sub-picometre
            // roundoff even though OCCT treats the occurrences as identical.
            // Canonicalize with one decimal digit of margin around the codec's
            // transform-equivalence tolerance so shared topology receives one
            // occurrence identity even when roundoff crosses zero.
            let rounded = (value * 1.0e11).round() / 1.0e11;
            let canonical = if rounded == 0.0 { 0.0 } else { rounded };
            bytes.extend_from_slice(&canonical.to_bits().to_le_bytes());
        }
    }
    sha256_hex(&bytes)[..16].to_owned()
}

fn exact_transform_digest(transform: Transform) -> String {
    let mut bytes = Vec::with_capacity(16 * 8);
    for row in transform.rows {
        for value in row {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }
    sha256_hex(&bytes)[..16].to_owned()
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
                    return Err(CodecError::Malformed(format!(
                        "edge TShape {edge} has multiple forward endpoint uses"
                    )));
                }
            }
            TextOrientation::Reversed => {
                if end.replace(child).is_some() {
                    return Err(CodecError::Malformed(format!(
                        "edge TShape {edge} has multiple reversed endpoint uses"
                    )));
                }
            }
            TextOrientation::Internal | TextOrientation::External => {}
        }
    }
    start.zip(end).ok_or_else(|| {
        CodecError::Malformed(format!(
            "edge TShape {edge} does not have both forward and reversed endpoint uses"
        ))
    })
}

fn first_edge_representation<Predicate>(
    representations: &[TextEdgeRepresentation],
    predicate: Predicate,
) -> Option<(usize, &TextEdgeRepresentation)>
where
    Predicate: Fn(&TextEdgeRepresentation) -> bool,
{
    representations
        .iter()
        .enumerate()
        .find(|(_, representation)| predicate(representation))
}

fn select_exact_curve_representation<'a>(
    edge: usize,
    representations: &'a [TextEdgeRepresentation],
    tables: &Tables<'_>,
) -> Result<Option<(usize, &'a TextEdgeRepresentation)>, CodecError> {
    let mut matches = representations
        .iter()
        .enumerate()
        .filter(|(_, representation)| representation.kind == 1);
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if matches.any(|(_, representation)| {
        !equivalent_exact_curve_representation(first.1, representation, tables)
    }) {
        return Err(CodecError::Malformed(format!(
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
    let Some(left_curve) = left.primary.checked_sub(1) else {
        return false;
    };
    let Some(right_curve) = right.primary.checked_sub(1) else {
        return false;
    };
    tables.curves.get(left_curve) == tables.curves.get(right_curve)
        && tables.location(left.location) == tables.location(right.location)
        && left.parameter_range == right.parameter_range
}

fn unique_fallback_polygon_representation(
    edge: usize,
    representations: &[TextEdgeRepresentation],
) -> Result<Option<(usize, &TextEdgeRepresentation)>, CodecError> {
    let mut matches = representations
        .iter()
        .enumerate()
        .filter(|(_, representation)| matches!(representation.kind, 5..=7));
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "edge TShape {edge} has multiple fallback polygon representations"
        )));
    }
    Ok(Some(first))
}

pub(crate) fn normalize_occt_curve_range(
    geometry: &CurveGeometry,
    range: Option<[f64; 2]>,
) -> Option<[f64; 2]> {
    match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            let [start, end] = range?;
            let sweep = end - start;
            let tau = std::f64::consts::TAU;
            if !start.is_finite() || !end.is_finite() || (sweep - tau).abs() <= 1.0e-9 {
                return Some([start, end]);
            }
            let canonical_start = start.rem_euclid(tau);
            let canonical_start = if (tau - canonical_start).abs() <= 1.0e-12 {
                0.0
            } else {
                canonical_start
            };
            Some([canonical_start, canonical_start + sweep])
        }
        CurveGeometry::Parabola { focal_distance, .. } => {
            if !focal_distance.is_finite() || *focal_distance <= 0.0 {
                return range;
            }
            range.map(|[start, end]| {
                let scale = 2.0 * focal_distance;
                [start / scale, end / scale]
            })
        }
        CurveGeometry::Transformed { basis, .. } => normalize_occt_curve_range(basis, range),
        _ => range,
    }
}

#[cfg(test)]
pub(crate) mod tests;
