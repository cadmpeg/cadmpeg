// SPDX-License-Identifier: Apache-2.0
//! Build IR and diagnostics from an NX SPLMSSTR container.
//!
//! [`scan`] parses the container and inflates its embedded streams. [`decode`]
//! converts supported analytic and NURBS carriers to millimetres, resolves
//! supported topology, preserves each Parasolid stream as an unknown record, and
//! returns a [`DecodeReport`] describing incomplete transfer. Partition and
//! deltas streams are both decoded; callers must use the report to account for
//! unresolved active-face selection and other loss.
//!
//! [`DecodeReport`]: cadmpeg_ir::report::DecodeReport

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::eval::{
    analytic_surface_parameters, curve_point, model_surface_point, nurbs_surface_partials,
    pcurve_uv, surface_point,
};
use cadmpeg_ir::features::{
    Angle, BodySelection, BodyTrimSide, BooleanOp, ChamferSpec, ConfigurationId,
    DesignConfiguration, DesignParameter, EdgeSelection, Extent, FaceSelection, Feature,
    FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole, HoleForm, HoleKind,
    Length, ParameterId, ParameterValue, PatternKind, ProfileRef, RadiusForm, RadiusSpec,
    RibConstruction, RibDraft, SketchSpace,
};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, IntcurveSupportContext,
    IntcurveSupportSide, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    AttributeId, BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId,
    ProceduralCurveId, ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::tessellation::{Tessellation, TessellationChannel};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::{self, Container};
use crate::geometry;
use crate::parasolid::{self, Stream, StreamKind};
use crate::topology::{Graph, Node};

pub(crate) const MISSING_TOLERANCE: f64 = -31_415_800_000_000.0;
const DISPLAY_JT_COLOR_CHANNEL: u32 = 0x4e58_0001;
const DISPLAY_JT_VERTEX_FLAG_CHANNEL: u32 = 0x4e58_0002;
const DISPLAY_JT_TEXTURE_CHANNEL_BASE: u32 = 0x4e58_0100;

#[derive(Clone, Copy)]
pub(crate) struct DisplayJtTessellationInputs<'a> {
    pub(crate) meshes: &'a [crate::native::DisplayJtPolygonMesh],
    pub(crate) coordinates: &'a [crate::native::DisplayJtVertexCoordinates],
    pub(crate) normals: &'a [crate::native::DisplayJtVertexNormals],
    pub(crate) colors: &'a [crate::native::DisplayJtVertexColors],
    pub(crate) texture_coordinates: &'a [crate::native::DisplayJtVertexTextureCoordinates],
    pub(crate) vertex_flags: &'a [crate::native::DisplayJtVertexFlags],
    pub(crate) vertex_headers: &'a [crate::native::DisplayJtCompressedVertexRecordsHeader],
    pub(crate) coordinate_headers: &'a [crate::native::DisplayJtVertexCoordinateArrayHeader],
    pub(crate) shape_elements: &'a [crate::native::DisplayJtShapeLodElement],
    pub(crate) bindings: &'a [crate::native::DisplayJtShapeLodBinding],
    pub(crate) shape_nodes: &'a [crate::native::DisplayJtTriStripShapeNode],
    pub(crate) base_nodes: &'a [crate::native::DisplayJtBaseNodeData],
    pub(crate) transforms: &'a [crate::native::DisplayJtGeometricTransformAttribute],
    pub(crate) partition_nodes: &'a [crate::native::DisplayJtPartitionNode],
    pub(crate) range_lod_nodes: &'a [crate::native::DisplayJtRangeLodNode],
    pub(crate) compressed_elements: &'a [crate::native::DisplayJtCompressedElement],
}

fn multiply_jt_matrices(left: [[f64; 4]; 4], right: [[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let mut product = [[0.0; 4]; 4];
    for (row, values) in product.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = (0..4)
                .map(|inner| left[row][inner] * right[inner][column])
                .sum();
            if !value.is_finite() {
                return None;
            }
        }
    }
    Some(product)
}

fn resolve_display_jt_node_transform(
    object_id: u32,
    by_object: &BTreeMap<u32, &crate::native::DisplayJtBaseNodeData>,
    parents: &BTreeMap<u32, Vec<u32>>,
    transforms: &[&crate::native::DisplayJtGeometricTransformAttribute],
    visiting: &mut BTreeSet<u32>,
) -> Option<([[f64; 4]; 4], bool)> {
    if !visiting.insert(object_id) {
        return None;
    }
    let parent_states = parents.get(&object_id).map_or_else(
        || {
            Some(vec![(
                [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                false,
            )])
        },
        |ids| {
            ids.iter()
                .map(|id| {
                    resolve_display_jt_node_transform(*id, by_object, parents, transforms, visiting)
                })
                .collect()
        },
    )?;
    visiting.remove(&object_id);
    let base = by_object.get(&object_id)?;
    let mut results = Vec::new();
    for (mut matrix, mut final_transform) in parent_states {
        for attribute_id in &base.attribute_object_ids {
            let mut matching = transforms
                .iter()
                .filter(|attribute| attribute.object_id == *attribute_id);
            let attribute = matching.next()?;
            if matching.next().is_some() {
                return None;
            }
            if attribute.state_flags & 0x04 != 0 {
                continue;
            }
            if final_transform && attribute.state_flags & 0x02 == 0 {
                continue;
            }
            let local = attribute.matrix.map(|row| row.map(f64::from));
            matrix = multiply_jt_matrices(local, matrix)?;
            final_transform |= attribute.state_flags & 0x01 != 0;
        }
        results.push((matrix, final_transform));
    }
    let first = results.first()?.0;
    results
        .iter()
        .all(|result| result.0 == first)
        .then_some(results[0])
}

fn display_jt_node_transform(
    scene_segment: &str,
    shape_object_id: u32,
    base_nodes: &[crate::native::DisplayJtBaseNodeData],
    transforms: &[crate::native::DisplayJtGeometricTransformAttribute],
    partition_nodes: &[crate::native::DisplayJtPartitionNode],
    range_lod_nodes: &[crate::native::DisplayJtRangeLodNode],
    compressed_elements: &[crate::native::DisplayJtCompressedElement],
) -> Option<[[f64; 4]; 4]> {
    let scoped = base_nodes
        .iter()
        .filter(|base| {
            compressed_elements
                .iter()
                .find(|element| element.id == base.element)
                .is_some_and(|element| element.segment == scene_segment)
        })
        .collect::<Vec<_>>();
    let mut by_object = BTreeMap::new();
    for base in &scoped {
        if by_object.insert(base.object_id, *base).is_some() {
            return None;
        }
    }
    by_object.get(&shape_object_id)?;
    let scoped_transforms = transforms
        .iter()
        .filter(|attribute| {
            compressed_elements
                .iter()
                .find(|element| element.id == attribute.element)
                .is_some_and(|element| element.segment == scene_segment)
        })
        .collect::<Vec<_>>();
    let mut parents = BTreeMap::<u32, Vec<u32>>::new();
    for (object_id, base) in &by_object {
        let children = partition_nodes
            .iter()
            .find(|node| node.base_node == base.id)
            .map(|node| node.child_object_ids.as_slice())
            .or_else(|| {
                range_lod_nodes
                    .iter()
                    .find(|node| node.base_node == base.id)
                    .map(|node| node.child_object_ids.as_slice())
            })
            .unwrap_or_default();
        for &child in children {
            by_object.get(&child)?;
            parents.entry(child).or_default().push(*object_id);
        }
    }
    resolve_display_jt_node_transform(
        shape_object_id,
        &by_object,
        &parents,
        &scoped_transforms,
        &mut BTreeSet::new(),
    )
    .map(|state| state.0)
}

fn transform_jt_point(matrix: [[f64; 4]; 4], point: [f32; 3]) -> Option<Point3> {
    let point = point.map(f64::from);
    let coordinate = |column| {
        (matrix[3][column]
            + (0..3)
                .map(|row| point[row] * matrix[row][column])
                .sum::<f64>())
            * 1000.0
    };
    let point = Point3::new(coordinate(0), coordinate(1), coordinate(2));
    [point.x, point.y, point.z]
        .iter()
        .all(|value| value.is_finite())
        .then_some(point)
}

fn transform_jt_normal(matrix: [[f64; 4]; 4], normal: [f32; 3]) -> Option<Vector3> {
    let a = matrix;
    let determinant = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !determinant.is_finite() || determinant == 0.0 {
        return None;
    }
    let inverse = [
        [
            (a[1][1] * a[2][2] - a[1][2] * a[2][1]) / determinant,
            (a[0][2] * a[2][1] - a[0][1] * a[2][2]) / determinant,
            (a[0][1] * a[1][2] - a[0][2] * a[1][1]) / determinant,
        ],
        [
            (a[1][2] * a[2][0] - a[1][0] * a[2][2]) / determinant,
            (a[0][0] * a[2][2] - a[0][2] * a[2][0]) / determinant,
            (a[0][2] * a[1][0] - a[0][0] * a[1][2]) / determinant,
        ],
        [
            (a[1][0] * a[2][1] - a[1][1] * a[2][0]) / determinant,
            (a[0][1] * a[2][0] - a[0][0] * a[2][1]) / determinant,
            (a[0][0] * a[1][1] - a[0][1] * a[1][0]) / determinant,
        ],
    ];
    let normal = normal.map(f64::from);
    let transformed = Vector3::new(
        (0..3).map(|index| normal[index] * inverse[0][index]).sum(),
        (0..3).map(|index| normal[index] * inverse[1][index]).sum(),
        (0..3).map(|index| normal[index] * inverse[2][index]).sum(),
    );
    let length = transformed.norm();
    (length.is_finite() && length > 0.0).then(|| {
        Vector3::new(
            transformed.x / length,
            transformed.y / length,
            transformed.z / length,
        )
    })
}

pub(crate) fn display_jt_tessellations(
    inputs: DisplayJtTessellationInputs<'_>,
) -> Option<Vec<(Tessellation, u64)>> {
    let DisplayJtTessellationInputs {
        meshes,
        coordinates,
        normals,
        colors,
        texture_coordinates,
        vertex_flags,
        vertex_headers,
        coordinate_headers,
        shape_elements,
        bindings,
        shape_nodes,
        base_nodes,
        transforms,
        partition_nodes,
        range_lod_nodes,
        compressed_elements,
    } = inputs;
    let mut tessellations = Vec::new();
    for mesh in meshes {
        let coordinate_header = coordinate_headers
            .iter()
            .find(|header| header.id == mesh.coordinate_header)?;
        let coordinates = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)?;
        let shape_element = shape_elements
            .iter()
            .find(|element| element.id == coordinate_header.element)?;
        let mut matching_bindings = bindings.iter().filter(|binding| {
            binding.shape_segment == shape_element.segment
                && binding.payload_object_id == shape_element.object_id
        });
        let binding = matching_bindings.next()?;
        if matching_bindings.next().is_some() {
            return None;
        }
        let mut matching_nodes = shape_nodes.iter().filter(|node| {
            if node.object_id != binding.shape_node_object_id {
                return false;
            }
            let Some(base) = base_nodes.iter().find(|base| base.id == node.base_node) else {
                return false;
            };
            compressed_elements
                .iter()
                .find(|element| element.id == base.element)
                .is_some_and(|element| element.segment == binding.scene_segment)
        });
        let shape_node = matching_nodes.next()?;
        if matching_nodes.next().is_some() {
            return None;
        }
        let transform = display_jt_node_transform(
            &binding.scene_segment,
            shape_node.object_id,
            base_nodes,
            transforms,
            partition_nodes,
            range_lod_nodes,
            compressed_elements,
        )?;
        let mut rendered = Vec::new();
        for ((polygon, attributes), &group) in mesh
            .polygons
            .iter()
            .zip(&mesh.vertex_attribute_indices)
            .zip(&mesh.polygon_groups)
        {
            if group < 0 {
                continue;
            }
            let triangle: [u32; 3] = polygon.as_slice().try_into().ok()?;
            let attributes: [Option<u32>; 3] = attributes.as_slice().try_into().ok()?;
            rendered.push((triangle, attributes));
        }
        if rendered.is_empty() {
            return None;
        }
        let vertex_header = vertex_headers
            .iter()
            .find(|header| header.element == shape_element.id)?;
        let normal_array = (vertex_header.vertex_bindings & 0x8 != 0)
            .then(|| {
                normals
                    .iter()
                    .find(|normals| normals.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x8 != 0 && normal_array.is_none() {
            return None;
        }
        let color_array = (vertex_header.vertex_bindings & 0x30 != 0)
            .then(|| {
                colors
                    .iter()
                    .find(|colors| colors.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x30 != 0 && color_array.is_none() {
            return None;
        }
        let texture_arrays = (0..8_u8)
            .filter(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
            .map(|channel| {
                texture_coordinates.iter().find(|coordinates| {
                    coordinates.vertex_records_header == vertex_header.id
                        && coordinates.channel == channel
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let vertex_flag_array = (vertex_header.vertex_bindings & 0x40 != 0)
            .then(|| {
                vertex_flags
                    .iter()
                    .find(|flags| flags.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x40 != 0 && vertex_flag_array.is_none() {
            return None;
        }
        let convert_point = |index: u32| {
            let point = coordinates.points_m.get(index as usize)?;
            transform_jt_point(transform, *point)
        };
        let has_vertex_attributes = normal_array.is_some()
            || color_array.is_some()
            || !texture_arrays.is_empty()
            || vertex_flag_array.is_some();
        let (vertices, triangles, normal_vectors, channels) = if has_vertex_attributes {
            let mut vertices = Vec::with_capacity(rendered.len() * 3);
            let mut triangles = Vec::with_capacity(rendered.len());
            let mut normal_vectors = normal_array
                .map(|_| Vec::with_capacity(rendered.len() * 3))
                .unwrap_or_default();
            let mut color_data = color_array
                .map(|_| Vec::with_capacity(rendered.len() * 3 * 16))
                .unwrap_or_default();
            let texture_component_counts = texture_arrays
                .iter()
                .map(|array| {
                    let count = array.values.first()?.len();
                    (1..=4)
                        .contains(&count)
                        .then_some(count)
                        .filter(|count| array.values.iter().all(|value| value.len() == *count))
                })
                .collect::<Option<Vec<_>>>()?;
            let mut texture_data = texture_component_counts
                .iter()
                .map(|count| Vec::with_capacity(rendered.len() * 3 * count * 4))
                .collect::<Vec<_>>();
            let mut vertex_flag_data = vertex_flag_array
                .map(|_| Vec::with_capacity(rendered.len() * 3 * 4))
                .unwrap_or_default();
            for (triangle, attributes) in rendered {
                let base = u32::try_from(vertices.len()).ok()?;
                for (coordinate, attribute) in triangle.into_iter().zip(attributes) {
                    vertices.push(convert_point(coordinate)?);
                    let attribute = attribute? as usize;
                    if let Some(normal_array) = normal_array {
                        let normal = normal_array.normals.get(attribute)?;
                        normal_vectors.push(transform_jt_normal(transform, *normal)?);
                    }
                    if let Some(color_array) = color_array {
                        for component in color_array.colors.get(attribute)? {
                            color_data.extend_from_slice(&component.to_le_bytes());
                        }
                    }
                    for (array, data) in texture_arrays.iter().zip(&mut texture_data) {
                        for component in array.values.get(attribute)? {
                            data.extend_from_slice(&component.to_le_bytes());
                        }
                    }
                    if let Some(array) = vertex_flag_array {
                        vertex_flag_data
                            .extend_from_slice(&array.values.get(attribute)?.to_le_bytes());
                    }
                }
                triangles.push([base, base.checked_add(1)?, base.checked_add(2)?]);
            }
            let count = u32::try_from(vertices.len()).ok()?;
            let mut channels = Vec::new();
            if color_array.is_some() {
                channels.push(TessellationChannel {
                    item_size: 16,
                    kind: DISPLAY_JT_COLOR_CHANNEL,
                    flags: ((vertex_header.vertex_bindings >> 4) & 0x3) as u32,
                    count,
                    data: color_data,
                });
            }
            for (((array, component_count), data), ordinal) in texture_arrays
                .iter()
                .zip(texture_component_counts)
                .zip(texture_data)
                .zip(0_u32..)
            {
                channels.push(TessellationChannel {
                    item_size: u32::try_from(component_count.checked_mul(4)?).ok()?,
                    kind: DISPLAY_JT_TEXTURE_CHANNEL_BASE.checked_add(ordinal)?,
                    flags: u32::from(array.channel)
                        | (((vertex_header.vertex_bindings >> (8 + 4 * array.channel)) & 0xf)
                            as u32)
                            << 8,
                    count,
                    data,
                });
            }
            if vertex_flag_array.is_some() {
                channels.push(TessellationChannel {
                    item_size: 4,
                    kind: DISPLAY_JT_VERTEX_FLAG_CHANNEL,
                    flags: 0,
                    count,
                    data: vertex_flag_data,
                });
            }
            (vertices, triangles, normal_vectors, channels)
        } else {
            let vertices = (0..coordinates.points_m.len())
                .map(|index| convert_point(index as u32))
                .collect::<Option<Vec<_>>>()?;
            let triangles = rendered.into_iter().map(|(triangle, _)| triangle).collect();
            (vertices, triangles, Vec::new(), Vec::new())
        };
        tessellations.push((
            Tessellation {
                id: format!(
                    "nx:display-jt:tessellation#{}-{}",
                    shape_element.source_offset, shape_element.object_id
                ),
                body: None,
                source_object: Some(SourceObjectAssociation {
                    format: "nx".to_string(),
                    object_id: shape_node.id.clone(),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
                vertices,
                triangles,
                strip_lengths: Vec::new(),
                normals: normal_vectors,
                channels,
            },
            shape_node.source_offset,
        ));
    }
    Some(tessellations)
}

/// Parsed container data shared by inspection and entity decoding.
pub struct Scan {
    /// Parsed SPLMSSTR container.
    pub container: Container,
    /// Located and inflated Parasolid or preview streams.
    pub streams: Vec<Stream>,
}

impl Scan {
    /// Count streams with the requested classification.
    pub fn count(&self, kind: StreamKind) -> usize {
        self.streams.iter().filter(|s| s.kind == kind).count()
    }

    /// Return whether the file contains an inline Parasolid stream.
    ///
    /// NX assemblies may contain only references to external child parts.
    pub fn has_parasolid(&self) -> bool {
        self.streams.iter().any(|s| s.kind.is_parasolid())
    }
}

/// Parse the SPLMSSTR container and inflate streams in its canonical part entry.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<Scan, CodecError> {
    let container = container::scan(reader)?;
    let streams = parasolid::extract_streams(&container.data);
    Ok(Scan { container, streams })
}

/// Decode an NX `.prt` into IR and a loss report.
///
/// When [`DecodeOptions::container_only`] is set, the returned IR contains source
/// metadata and preserved streams but no typed entities. Otherwise the decoder
/// emits supported geometry and resolvable topology. A valid container can
/// decode successfully with no geometry, including an assembly whose geometry
/// resides in external child parts.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = scan(reader)?;

    if options.container_only {
        let ir = build_metadata_ir(&scan)?;
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult::new(ir, report));
    }

    if let Some((ir, report)) = try_decode_geometry(&scan) {
        return Ok(DecodeResult::new(ir, report));
    }

    let ir = build_metadata_ir(&scan)?;
    let report = build_container_report(&scan, false);
    Ok(DecodeResult::new(ir, report))
}

/// Aggregate carrier counts across the decoded streams, for reporting.
#[derive(Debug, Default)]
struct Counts {
    points: usize,
    planes: usize,
    cylinders: usize,
    cones: usize,
    spheres: usize,
    tori: usize,
    nurbs_surfaces: usize,
    offset_surfaces: usize,
    blend_surfaces: usize,
    lines: usize,
    circles: usize,
    ellipses: usize,
    nurbs_curves: usize,
    intersection_curves: usize,
    intersection_rejections: crate::intersection::RejectionCounts,
}

impl Counts {
    fn surfaces(&self) -> usize {
        self.planes
            + self.cylinders
            + self.cones
            + self.spheres
            + self.tori
            + self.nurbs_surfaces
            + self.offset_surfaces
            + self.blend_surfaces
    }
    fn curves(&self) -> usize {
        self.lines + self.circles + self.ellipses + self.nurbs_curves + self.intersection_curves
    }
}

/// Decode analytic carriers from every Parasolid stream. Returns `None` when no
/// carrier of any kind passes its gate, so the caller falls back to metadata.
fn try_decode_geometry(scan: &Scan) -> Option<(CadIr, DecodeReport)> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    let mut counts = Counts::default();
    let mut body_node_ids = BTreeMap::new();
    let semantic_streams = semantic_streams(scan);
    let topology_streams = topology_streams(scan);

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let semantic = &semantic_streams[si];
        let stream_name = format!("parasolid#{si}:{}", stream.kind.label());
        let source_stream = annotations.stream(format!("nx:{stream_name}"));
        let graph = Graph::parse(&topology_streams[si]);
        body_node_ids.extend(topology_body_node_ids(si, &graph));
        let mut points_by_xmt = BTreeMap::new();
        let mut surfaces_by_xmt = BTreeMap::new();
        let mut curves_by_xmt = BTreeMap::new();
        let mut pcurves_by_xmt = BTreeMap::new();
        let mut pcurve_supports_by_xmt = BTreeMap::new();
        let mut trim_ranges = BTreeMap::new();
        let mut pending_blend_supports = Vec::new();
        let mut pending_blend_spines = Vec::new();
        let mut pending_ext11_support_uv = Vec::new();
        let first_surface = ir.model.surfaces.len();
        let first_curve = ir.model.curves.len();
        let mut point_ordinal = 0usize;
        for pt in geometry::points(semantic) {
            let pi = point_ordinal;
            point_ordinal += 1;
            let pid = PointId(format!("nx:s{si}:pt#{pi}"));
            annotations
                .note(&pid, source_stream, pt.pos as u64)
                .tag("POINT");
            annotations.derived(&pid, "position");
            ir.model.points.push(Point {
                id: pid.clone(),
                position: pt.position,
            });
            if let Some(node) = graph.at_pos(pt.pos) {
                if node.kind == 29 {
                    let point_id = ir
                        .model
                        .points
                        .last()
                        .expect("invariant: just pushed above")
                        .id
                        .clone();
                    points_by_xmt.insert(node.xmt, point_id);
                }
            }
            counts.points += 1;
        }
        for node in graph.of_kind(29) {
            if points_by_xmt.contains_key(&node.xmt) {
                continue;
            }
            let Some(position) = node.point_position() else {
                continue;
            };
            let pi = point_ordinal;
            point_ordinal += 1;
            let pid = PointId(format!("nx:s{si}:pt#{pi}"));
            annotate_node(&mut annotations, &pid, source_stream, node, "POINT");
            annotations.derived(&pid, "position");
            ir.model.points.push(Point {
                id: pid.clone(),
                position,
            });
            points_by_xmt.insert(node.xmt, pid);
            counts.points += 1;
        }

        for (fi, surf) in geometry::surfaces(semantic).into_iter().enumerate() {
            match &surf.geometry {
                SurfaceGeometry::Plane { .. } => counts.planes += 1,
                SurfaceGeometry::Cylinder { .. } => counts.cylinders += 1,
                SurfaceGeometry::Cone { .. } => counts.cones += 1,
                SurfaceGeometry::Sphere { .. } => counts.spheres += 1,
                SurfaceGeometry::Torus { .. } => counts.tori += 1,
                SurfaceGeometry::Nurbs(_)
                | SurfaceGeometry::Procedural { .. }
                | SurfaceGeometry::Unknown { .. } => {}
            }
            let id = SurfaceId(format!("nx:s{si}:surf#{fi}"));
            annotations
                .note(&id, source_stream, surf.pos as u64)
                .tag(surface_tag(&surf.geometry));
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                surfaces_by_xmt.insert(node.xmt, id);
            }
        }
        for node in (50..=54).flat_map(|kind| graph.of_kind(kind)) {
            if surfaces_by_xmt.contains_key(&node.xmt) {
                continue;
            }
            let Some(geometry) = node.surface_geometry() else {
                continue;
            };
            match &geometry {
                SurfaceGeometry::Plane { .. } => counts.planes += 1,
                SurfaceGeometry::Cylinder { .. } => counts.cylinders += 1,
                SurfaceGeometry::Cone { .. } => counts.cones += 1,
                SurfaceGeometry::Sphere { .. } => counts.spheres += 1,
                SurfaceGeometry::Torus { .. } => counts.tori += 1,
                _ => unreachable!("fixed analytic surface family"),
            }
            let id = SurfaceId(format!("nx:s{si}:graph-surf#{}", node.xmt));
            annotate_node(
                &mut annotations,
                &id,
                source_stream,
                node,
                surface_tag(&geometry),
            );
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            surfaces_by_xmt.insert(node.xmt, id);
        }

        for (fi, surf) in crate::nurbs::surfaces(semantic).into_iter().enumerate() {
            counts.nurbs_surfaces += 1;
            let id = SurfaceId(format!("nx:s{si}:nurbs-surf#{fi}"));
            annotations
                .note(&id, source_stream, surf.pos as u64)
                .tag("B_SPLINE_SURFACE");
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                surfaces_by_xmt.insert(node.xmt, id);
            }
        }

        for (oi, offset) in crate::topology::offset_surfaces(semantic)
            .into_iter()
            .enumerate()
        {
            let Some(support) = surfaces_by_xmt.get(&offset.support).cloned() else {
                continue;
            };
            let surface_id = SurfaceId(format!("nx:s{si}:offset-surf#{oi}"));
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:offset#{oi}"));
            annotations
                .note(&surface_id, source_stream, offset.pos as u64)
                .tag("OFFSET_SURF");
            annotations.derived(&surface_id, "geometry");
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                },
                source_object: Some(SourceObjectAssociation {
                    format: "nx".into(),
                    object_id: format!("nx:s{si}:offset-surface-record#{}", offset.xmt),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            annotations
                .note(&procedural_id, source_stream, offset.pos as u64)
                .tag("OFFSET_SURF");
            annotations.derived(&procedural_id, "definition");
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id.clone(),
                definition: ProceduralSurfaceDefinition::Offset {
                    support,
                    distance: offset.distance,
                    u_sense: 0,
                    v_sense: 0,
                    extension_flags: Vec::new(),
                },
                cache_fit_tolerance: None,
            });
            surfaces_by_xmt.insert(offset.xmt, surface_id);
            counts.offset_surfaces += 1;
        }

        for (bi, blend) in crate::topology::blend_surfaces(semantic)
            .into_iter()
            .enumerate()
        {
            let surface_id = SurfaceId(format!("nx:s{si}:blend-surf#{bi}"));
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:blend#{bi}"));
            annotations
                .note(&surface_id, source_stream, blend.pos as u64)
                .tag("BLEND_SURF");
            annotations.derived(&surface_id, "geometry");
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                },
                source_object: Some(SourceObjectAssociation {
                    format: "nx".to_string(),
                    object_id: format!("nx:s{si}:blend-surface-record#{}", blend.xmt),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            annotations
                .note(&procedural_id, source_stream, blend.pos as u64)
                .tag("BLEND_SURF");
            annotations.derived(&procedural_id, "definition");
            let procedural_index = ir.model.procedural_surfaces.len();
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id.clone(),
                definition: ProceduralSurfaceDefinition::Blend {
                    supports: [None, None],
                    spine: None,
                    radius: BlendRadiusLaw::Constant {
                        signed_radius: blend.offsets[0].abs(),
                    },
                    cross_section: BlendCrossSection::Circular,
                    native: None,
                },
                cache_fit_tolerance: None,
            });
            pending_blend_supports.push((procedural_index, blend.supports, blend.offsets));
            if blend.spine > 1 {
                pending_blend_spines.push((procedural_index, blend.spine));
            }
            surfaces_by_xmt.insert(blend.xmt, surface_id);
            counts.blend_surfaces += 1;
        }

        for (procedural_index, support_xmts, offsets) in pending_blend_supports {
            let supports = [0, 1].map(|side| {
                surfaces_by_xmt
                    .get(&support_xmts[side])
                    .cloned()
                    .map(|surface| BlendSupport {
                        surface,
                        reversed: offsets[side].is_sign_negative(),
                    })
            });
            let Some(ProceduralSurface {
                definition:
                    ProceduralSurfaceDefinition::Blend {
                        supports: slots, ..
                    },
                ..
            }) = ir.model.procedural_surfaces.get_mut(procedural_index)
            else {
                continue;
            };
            *slots = supports;
        }

        for (ci, crv) in geometry::curves(semantic).into_iter().enumerate() {
            match &crv.geometry {
                CurveGeometry::Line { .. } => counts.lines += 1,
                CurveGeometry::Circle { .. } => counts.circles += 1,
                CurveGeometry::Ellipse { .. } => counts.ellipses += 1,
                CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. }
                | CurveGeometry::Degenerate { .. }
                | CurveGeometry::Nurbs(_)
                | CurveGeometry::Procedural { .. }
                | CurveGeometry::Unknown { .. } => {}
            }
            let id = CurveId(format!("nx:s{si}:crv#{ci}"));
            annotations
                .note(&id, source_stream, crv.pos as u64)
                .tag(curve_tag(&crv.geometry));
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: crv.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                curves_by_xmt.insert(node.xmt, id);
            }
        }
        for node in (30..=32).flat_map(|kind| graph.of_kind(kind)) {
            if curves_by_xmt.contains_key(&node.xmt) {
                continue;
            }
            let Some(geometry) = node.curve_geometry() else {
                continue;
            };
            match &geometry {
                CurveGeometry::Line { .. } => counts.lines += 1,
                CurveGeometry::Circle { .. } => counts.circles += 1,
                CurveGeometry::Ellipse { .. } => counts.ellipses += 1,
                _ => unreachable!("fixed analytic curve family"),
            }
            let id = CurveId(format!("nx:s{si}:graph-crv#{}", node.xmt));
            annotate_node(
                &mut annotations,
                &id,
                source_stream,
                node,
                curve_tag(&geometry),
            );
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            curves_by_xmt.insert(node.xmt, id);
        }

        for (ci, crv) in crate::nurbs::curves(semantic).into_iter().enumerate() {
            counts.nurbs_curves += 1;
            let id = CurveId(format!("nx:s{si}:nurbs-crv#{ci}"));
            annotations
                .note(&id, source_stream, crv.pos as u64)
                .tag("B_SPLINE_CURVE");
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: crv.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                curves_by_xmt.insert(node.xmt, id);
            }
        }

        for (pi, pcurve) in crate::nurbs::pcurves(semantic).into_iter().enumerate() {
            let id = PcurveId(format!("nx:s{si}:pcurve#{pi}"));
            annotations
                .note(&id, source_stream, pcurve.pos as u64)
                .tag("B_CURVE_2D");
            annotations.derived(&id, "geometry");
            ir.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry: pcurve.geometry,
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: None,
                fit_tolerance: None,
            });
            if let Some(node) = graph.at_pos(pcurve.pos) {
                pcurves_by_xmt.insert(node.xmt, id);
            }
        }

        let intersection_scan = crate::intersection::scan(semantic);
        counts
            .intersection_rejections
            .extend(intersection_scan.rejected);
        let intersection_constructions = intersection_scan.constructions;
        let charted_intersections: BTreeMap<_, _> = intersection_scan
            .curves
            .into_iter()
            .map(|curve| (curve.xmt, curve))
            .collect();
        for (ci, construction) in intersection_constructions.into_iter().enumerate() {
            let curve_id = CurveId(format!("nx:s{si}:intersection-crv#{ci}"));
            let procedural_id = ProceduralCurveId(format!("nx:s{si}:intersection#{ci}"));
            let unknown_id = UnknownId(format!("nx:container:parasolid#{si}"));
            let charted = charted_intersections.get(&construction.xmt);
            if let Some(charted) = charted {
                pending_ext11_support_uv.push((
                    procedural_id.clone(),
                    charted.points.clone(),
                    charted.parameters.clone(),
                    charted.fit_tolerance,
                    charted.ext_support_uv.clone(),
                ));
            }
            annotations
                .note(&curve_id, source_stream, construction.pos as u64)
                .tag("INTERSECTION");
            if charted.is_some() {
                annotations.derived(&curve_id, "geometry");
            } else {
                annotations.exactness(&curve_id, Exactness::Unknown);
            }
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: charted.map_or_else(
                    || CurveGeometry::Unknown {
                        record: Some(unknown_id.clone()),
                    },
                    |charted| {
                        CurveGeometry::Nurbs(NurbsCurve {
                            degree: 1,
                            knots: linear_knots(&charted.parameters),
                            control_points: charted.points.clone(),
                            weights: None,
                            periodic: false,
                        })
                    },
                ),
                source_object: Some(SourceObjectAssociation {
                    format: "nx".into(),
                    object_id: format!("nx:s{si}:intersection-record#{}", construction.xmt),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            annotations
                .note(&procedural_id, source_stream, construction.pos as u64)
                .tag("INTERSECTION");
            if charted.is_some() {
                annotations.derived(&procedural_id, "definition");
            } else {
                annotations.exactness(&procedural_id, Exactness::Unknown);
            }
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id.clone(),
                definition: charted.map_or_else(
                    || ProceduralCurveDefinition::Unknown {
                        record: Some(unknown_id),
                    },
                    |charted| {
                        let mut support_uv = charted.support_uv.clone();
                        if let Some(ext_support_uv) = assign_ext11_support_uv(
                            &ir,
                            &surfaces_by_xmt,
                            charted.supports,
                            &charted.points,
                            charted.fit_tolerance,
                            &charted.ext_support_uv,
                        ) {
                            for side in 0..2 {
                                if support_uv[side].is_none() {
                                    support_uv[side].clone_from(&ext_support_uv[side]);
                                }
                            }
                        }
                        let first = intersection_side(
                            &ir,
                            &surfaces_by_xmt,
                            charted.supports[0],
                            support_uv[0]
                                .as_deref()
                                .filter(|uv| uv.len() == charted.parameters.len())
                                .map(|uv| (uv, charted.parameters.as_slice())),
                        );
                        let second = intersection_side(
                            &ir,
                            &surfaces_by_xmt,
                            charted.supports[1],
                            support_uv[1]
                                .as_deref()
                                .filter(|uv| uv.len() == charted.parameters.len())
                                .map(|uv| (uv, charted.parameters.as_slice())),
                        );
                        ProceduralCurveDefinition::Intersection {
                            context: IntcurveSupportContext {
                                sides: [first, second],
                                parameter_range: [
                                    charted.parameters[0],
                                    *charted
                                        .parameters
                                        .last()
                                        .expect("validated chart has points"),
                                ],
                                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                            },
                            discontinuity_flag: false,
                        }
                    },
                ),
                cache_fit_tolerance: charted.map(|charted| charted.fit_tolerance),
            });
            curves_by_xmt.insert(construction.xmt, curve_id);
            counts.intersection_curves += 1;
        }

        for (procedural_index, spine_xmt) in pending_blend_spines {
            let Some(spine) = curves_by_xmt.get(&spine_xmt).cloned() else {
                continue;
            };
            let Some(ProceduralSurface {
                definition: ProceduralSurfaceDefinition::Blend { spine: slot, .. },
                ..
            }) = ir.model.procedural_surfaces.get_mut(procedural_index)
            else {
                continue;
            };
            *slot = Some(spine);
        }

        let trimmed_curves = crate::topology::trimmed_curves(semantic);
        let mut normalized_pcurves = BTreeSet::new();
        let surface_curves = crate::topology::surface_curves(semantic);
        loop {
            let mapped = curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len();
            for trim in &trimmed_curves {
                if let Some(basis) = curves_by_xmt.get(&trim.basis).cloned() {
                    let parameters = canonical_trim_range(&ir, &basis, trim.parameters);
                    curves_by_xmt.insert(trim.xmt, basis);
                    if let Some(parameters) = parameters {
                        trim_ranges.insert(trim.xmt, parameters);
                    }
                }
                if let Some(pcurve) = pcurves_by_xmt.get(&trim.basis).cloned() {
                    if let Some(carrier) = ir.model.pcurves.iter_mut().find(|p| p.id == pcurve) {
                        carrier.parameter_range = Some(trim.parameters);
                    }
                    pcurves_by_xmt.insert(trim.xmt, pcurve);
                    if let Some(support) = pcurve_supports_by_xmt.get(&trim.basis).cloned() {
                        pcurve_supports_by_xmt.insert(trim.xmt, support);
                    }
                    trim_ranges.insert(trim.xmt, trim.parameters);
                }
            }
            for surface_curve in &surface_curves {
                if let Some(pcurve) = pcurves_by_xmt.get(&surface_curve.pcurve).cloned() {
                    if normalized_pcurves.insert(pcurve.clone()) {
                        let support = surfaces_by_xmt
                            .get(&surface_curve.surface)
                            .and_then(|id| {
                                ir.model.surfaces.iter().find(|surface| surface.id == *id)
                            })
                            .map(|surface| surface.geometry.clone());
                        if let (Some(support), Some(carrier)) = (
                            support,
                            ir.model
                                .pcurves
                                .iter_mut()
                                .find(|candidate| candidate.id == pcurve),
                        ) {
                            normalize_pcurve_parameters(&mut carrier.geometry, &support);
                        }
                    }
                    if let Some(carrier) = ir.model.pcurves.iter_mut().find(|p| p.id == pcurve) {
                        carrier.fit_tolerance = decoded_tolerance(surface_curve.tolerance);
                    }
                    pcurves_by_xmt.insert(surface_curve.xmt, pcurve);
                    if let Some(support) = surfaces_by_xmt.get(&surface_curve.surface).cloned() {
                        pcurve_supports_by_xmt.insert(surface_curve.xmt, support);
                    }
                }
                if let Some(original) = curves_by_xmt.get(&surface_curve.original).cloned() {
                    curves_by_xmt.insert(surface_curve.xmt, original);
                }
            }
            if curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len() == mapped {
                break;
            }
        }

        retain_unresolved_topology_carriers(
            &mut ir,
            si,
            &graph,
            &mut surfaces_by_xmt,
            &mut curves_by_xmt,
            &pcurves_by_xmt,
            source_stream,
            &mut annotations,
        );

        emit_topology(
            &mut ir,
            si,
            &graph,
            &points_by_xmt,
            &surfaces_by_xmt,
            &curves_by_xmt,
            &pcurves_by_xmt,
            &pcurve_supports_by_xmt,
            &trim_ranges,
            source_stream,
            &mut annotations,
        );
        complete_ext11_support_uv(&mut ir, &pending_ext11_support_uv);
        complete_parameterization_equivalent_support_uv(&mut ir);
        complete_support_uv(&mut ir, &pending_ext11_support_uv);
        attach_completed_intersection_pcurves(
            &mut ir,
            &graph,
            &format!("nx:s{si}"),
            source_stream,
            &mut annotations,
        );

        // Preserve the whole inflated stream verbatim so nothing is dropped.
        let mut unknown = unknown_stream(si, stream);
        unknown.links.extend(
            ir.model.surfaces[first_surface..]
                .iter()
                .map(|surface| surface.id.0.clone()),
        );
        unknown.links.extend(
            ir.model.curves[first_curve..]
                .iter()
                .map(|curve| curve.id.0.clone()),
        );
        let container_stream = annotations.stream("nx:container");
        annotations
            .note(&unknown.id, container_stream, stream.file_offset as u64)
            .tag(stream.kind.label());
        annotations.exactness(&unknown.id, Exactness::Derived);
        if !unknown.links.is_empty() {
            annotations.derived(&unknown.id, "links");
        }
        ir.push_native_unknown("nx", unknown).ok()?;
    }

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return None;
    }

    let mut active_body_selection = select_active_body(
        &mut ir,
        &body_node_ids,
        &scan.container.rmfastload_object_ids(),
    );
    if !active_body_selection {
        active_body_selection = select_terminal_feature_bodies(&mut ir, scan);
    }
    attach_native_object_model(&mut ir, scan, &mut annotations).ok()?;
    prune_unreferenced_unknown_carriers(&mut ir);
    classify_body_kinds(&mut ir);
    finalize_point_topology(&mut ir, &mut annotations);
    let referenced_pcurves: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .filter_map(|coedge| coedge.pcurve.clone())
        .collect();
    ir.model
        .pcurves
        .retain(|pcurve| referenced_pcurves.contains(&pcurve.id));
    ir.annotations = annotations.build();
    retain_live_annotations(&mut ir);
    retain_live_unknown_links(&mut ir);
    let report = build_geometry_report(
        scan,
        &counts,
        !ir.model.faces.is_empty(),
        ir.model.bodies.len() > 1 && !active_body_selection,
        ir.model.tessellations.len(),
    );
    Some((ir, report))
}

pub(crate) fn prune_unreferenced_unknown_carriers(ir: &mut CadIr) {
    let mut used_surfaces: BTreeSet<_> = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.clone())
        .collect();
    let mut used_curves: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect();
    loop {
        let previous = (used_surfaces.len(), used_curves.len());
        for procedural in &ir.model.procedural_surfaces {
            if !used_surfaces.contains(&procedural.surface) {
                continue;
            }
            match &procedural.definition {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    used_surfaces.insert(support.clone());
                }
                ProceduralSurfaceDefinition::Blend {
                    supports, spine, ..
                } => {
                    used_surfaces.extend(
                        supports
                            .iter()
                            .flatten()
                            .map(|support| support.surface.clone()),
                    );
                    used_curves.extend(spine.iter().cloned());
                }
                _ => {}
            }
        }
        for procedural in &ir.model.procedural_curves {
            if !used_curves.contains(&procedural.curve) {
                continue;
            }
            match &procedural.definition {
                ProceduralCurveDefinition::Intersection { context, .. }
                | ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                    used_surfaces
                        .extend(context.sides.iter().filter_map(|side| side.surface.clone()));
                }
                _ => {}
            }
        }
        if previous == (used_surfaces.len(), used_curves.len()) {
            break;
        }
    }
    ir.model.surfaces.retain(|surface| {
        !matches!(surface.geometry, SurfaceGeometry::Unknown { .. })
            || used_surfaces.contains(&surface.id)
    });
    ir.model.curves.retain(|curve| {
        !matches!(curve.geometry, CurveGeometry::Unknown { .. }) || used_curves.contains(&curve.id)
    });
}

pub(crate) fn semantic_streams(scan: &Scan) -> Vec<Vec<u8>> {
    let mut semantic = topology_streams(scan);
    for (partition, deltas) in paired_delta_streams(scan) {
        for delta in deltas {
            semantic[partition].extend_from_slice(&crate::deltas::procedural_residual(
                &scan.streams[delta].inflated,
            ));
            semantic[delta].clear();
        }
    }
    semantic
}

pub(crate) fn topology_streams(scan: &Scan) -> Vec<Vec<u8>> {
    let mut semantic = scan
        .streams
        .iter()
        .map(|stream| stream.inflated.clone())
        .collect::<Vec<_>>();
    for (partition, deltas) in paired_delta_streams(scan) {
        for delta in deltas {
            semantic[partition] =
                crate::deltas::merge_full_records(&semantic[partition], &semantic[delta]);
            semantic[delta].clear();
        }
    }
    semantic
}

fn paired_delta_streams(scan: &Scan) -> BTreeMap<usize, Vec<usize>> {
    let links = crate::native::segment_stream_links(&scan.container, &scan.streams);
    let linked_deltas = links
        .iter()
        .filter(|link| link.stream_kind == "deltas")
        .map(|link| link.stream_ordinal as usize)
        .collect::<BTreeSet<_>>();
    pair_stream_indices(&scan.streams, (!links.is_empty()).then_some(&linked_deltas))
}

pub(crate) fn pair_stream_indices(
    streams: &[Stream],
    eligible_deltas: Option<&BTreeSet<usize>>,
) -> BTreeMap<usize, Vec<usize>> {
    let mut pairs = BTreeMap::<usize, Vec<usize>>::new();
    for (delta, stream) in streams.iter().enumerate() {
        if stream.kind != StreamKind::Deltas
            || eligible_deltas.is_some_and(|eligible| !eligible.contains(&delta))
        {
            continue;
        }
        let partition = streams[..delta]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, candidate)| {
                candidate.kind == StreamKind::Partition && candidate.schema == stream.schema
            })
            .map(|(partition, _)| partition);
        if let Some(partition) = partition {
            pairs.entry(partition).or_default().push(delta);
        }
    }
    pairs
}

fn retain_live_annotations(ir: &mut CadIr) {
    let mut ids = BTreeSet::new();
    macro_rules! add_ids {
        ($($arena:expr),+ $(,)?) => {
            $(ids.extend($arena.iter().map(|entity| entity.id.to_string()));)+
        };
    }
    add_ids!(
        ir.model.bodies,
        ir.model.regions,
        ir.model.shells,
        ir.model.faces,
        ir.model.loops,
        ir.model.coedges,
        ir.model.edges,
        ir.model.vertices,
        ir.model.points,
        ir.model.surfaces,
        ir.model.curves,
        ir.model.pcurves,
        ir.model.procedural_surfaces,
        ir.model.procedural_curves,
        ir.model.features,
    );
    if let Ok(unknowns) = ir.native_unknowns("nx") {
        ids.extend(unknowns.iter().map(|unknown| unknown.id.to_string()));
    }
    ir.annotations.provenance.retain(|id, _| ids.contains(id));
    ir.annotations.exactness.retain(|id, _| ids.contains(id));
}

fn retain_live_unknown_links(ir: &mut CadIr) {
    let mut ids = BTreeSet::new();
    ids.extend(ir.model.surfaces.iter().map(|entity| entity.id.to_string()));
    ids.extend(ir.model.curves.iter().map(|entity| entity.id.to_string()));
    ids.extend(ir.model.pcurves.iter().map(|entity| entity.id.to_string()));
    ids.extend(
        ir.model
            .procedural_surfaces
            .iter()
            .map(|entity| entity.id.to_string()),
    );
    ids.extend(
        ir.model
            .procedural_curves
            .iter()
            .map(|entity| entity.id.to_string()),
    );
    let Ok(mut unknowns) = ir.native_unknowns("nx") else {
        return;
    };
    let mut empty_links = Vec::new();
    for unknown in &mut unknowns {
        unknown.links.retain(|link| ids.contains(link));
        if unknown.links.is_empty() {
            empty_links.push(unknown.id.to_string());
        }
    }
    let _ = ir.set_native_unknowns("nx", &unknowns);
    for id in empty_links {
        if let Some(note) = ir.annotations.exactness.get_mut(&id) {
            note.fields.remove("links");
        }
    }
}

fn topology_body_node_ids(stream_index: usize, graph: &Graph) -> BTreeMap<BodyId, BTreeSet<u32>> {
    let prefix = format!("nx:s{stream_index}");
    let body_xmts: BTreeSet<_> = graph
        .body_shape_shells()
        .into_iter()
        .filter_map(|shell| shell.shell_fields().map(|fields| fields.body))
        .collect();
    body_xmts
        .into_iter()
        .map(|body_xmt| {
            let shells: BTreeSet<_> = graph
                .of_kind(13)
                .filter(|shell| {
                    shell
                        .shell_fields()
                        .is_some_and(|fields| fields.body == body_xmt)
                })
                .map(|shell| shell.xmt)
                .collect();
            let faces: Vec<_> = graph
                .of_kind(14)
                .filter(|face| {
                    face.face_fields()
                        .is_some_and(|fields| shells.contains(&fields.shell))
                })
                .collect();
            let face_xmts: BTreeSet<_> = faces.iter().map(|face| face.xmt).collect();
            let loops: BTreeSet<_> = graph
                .of_kind(15)
                .filter(|loop_| {
                    loop_
                        .loop_fields()
                        .is_some_and(|fields| face_xmts.contains(&fields.face))
                })
                .map(|loop_| loop_.xmt)
                .collect();
            let fins: Vec<_> = graph
                .of_kind(17)
                .filter(|fin| {
                    fin.fin_fields()
                        .is_some_and(|fields| loops.contains(&fields.loop_xmt))
                })
                .collect();
            let edge_xmts: BTreeSet<_> = fins
                .iter()
                .filter_map(|fin| fin.fin_fields().map(|fields| fields.edge))
                .collect();
            let vertex_xmts: BTreeSet<_> = fins
                .iter()
                .filter_map(|fin| fin.fin_fields().map(|fields| fields.vertex))
                .collect();
            let ids = faces
                .into_iter()
                .filter_map(|face| face.u32_at(4))
                .chain(
                    graph
                        .of_kind(16)
                        .filter(|edge| edge_xmts.contains(&edge.xmt))
                        .filter_map(|edge| edge.u32_at(4)),
                )
                .chain(
                    graph
                        .of_kind(18)
                        .filter(|vertex| vertex_xmts.contains(&vertex.xmt))
                        .filter_map(|vertex| vertex.u32_at(4)),
                )
                .collect();
            (BodyId(format!("{prefix}:body#{body_xmt}")), ids)
        })
        .collect()
}

fn select_active_body(
    ir: &mut CadIr,
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
    rmfastload_ids: &[u32],
) -> bool {
    if rmfastload_ids.is_empty() || ir.model.bodies.len() <= 1 {
        return false;
    }
    let active: BTreeSet<_> = rmfastload_ids.iter().copied().collect();
    let mut scored: Vec<_> = ir
        .model
        .bodies
        .iter()
        .map(|body| {
            let ids = body_node_ids.get(&body.id);
            let count = ids.map_or(0, BTreeSet::len);
            let hits = ids.map_or(0, |ids| ids.intersection(&active).count());
            (hits, count, body.id.clone())
        })
        .collect();
    scored.sort_by(|first, second| second.0.cmp(&first.0).then(second.1.cmp(&first.1)));
    let Some(&(top_hits, top_count, ref top_body)) = scored.first() else {
        return false;
    };
    let next_hits = scored.get(1).map_or(0, |score| score.0);
    let mut selected: BTreeSet<_> = scored
        .iter()
        .filter(|(hits, count, _)| *hits > 0 && *count > 0 && (*hits as f64 / *count as f64) > 0.10)
        .map(|(_, _, body)| body.clone())
        .collect();
    let dominant = top_hits >= 5 * next_hits.max(1);
    if dominant {
        selected.retain(|body| body == top_body);
    }
    if top_count == 0
        || (top_hits as f64 / top_count as f64) <= 0.10
        || selected.is_empty()
        || (selected.len() == 1 && !dominant)
    {
        return false;
    }
    prune_inactive_topology(ir, &selected);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "active_body_selector".to_string(),
            "rmfastload_object_id_membership".to_string(),
        );
        source
            .attributes
            .insert("rmfastload_hits".to_string(), top_hits.to_string());
        source.attributes.insert(
            "rmfastload_active_body_count".to_string(),
            selected.len().to_string(),
        );
    }
    true
}

fn select_terminal_feature_bodies(ir: &mut CadIr, scan: &Scan) -> bool {
    if ir.model.bodies.len() <= 1 {
        return false;
    }
    let labels = crate::native::feature_operation_labels(&scan.container);
    let body_references = crate::native::feature_body_references(&scan.container);
    let booleans = crate::native::feature_boolean_operations(&scan.container);
    let bindings = crate::native::segment_body_bindings(&scan.container, &scan.streams);
    let body_reference_occurrences =
        crate::native::feature_body_reference_occurrences(&scan.container);
    let body_members = crate::native::feature_operation_body_members(&scan.container);
    let body_operands = crate::native::feature_operation_body_operands(
        &body_members,
        &body_reference_occurrences,
        &bindings,
    );
    if booleans.is_empty() && body_operands.is_empty() {
        return false;
    }
    let Some(statuses) = crate::native::segment_body_lineage_statuses(
        &labels,
        &body_references,
        &booleans,
        &body_operands,
        &bindings,
    ) else {
        return false;
    };
    let mut mapped = BTreeSet::new();
    let mut selected = BTreeSet::new();
    for (binding, status) in bindings
        .iter()
        .filter(|binding| binding.stream_kind == "partition")
        .filter_map(|binding| {
            statuses
                .iter()
                .find(|status| status.segment_body_binding == binding.id)
                .map(|status| (binding, status))
        })
    {
        let prefix = format!("nx:s{}:", binding.stream_ordinal);
        let stream_bodies = ir
            .model
            .bodies
            .iter()
            .filter(|body| body.id.0.starts_with(&prefix))
            .map(|body| body.id.clone())
            .collect::<Vec<_>>();
        if stream_bodies.is_empty() {
            continue;
        }
        mapped.extend(stream_bodies.iter().cloned());
        if status.terminal {
            selected.extend(stream_bodies);
        }
    }
    let emitted = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<BTreeSet<_>>();
    if mapped != emitted || selected.is_empty() || selected.len() == emitted.len() {
        return false;
    }

    prune_inactive_topology(ir, &selected);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "active_body_selector".to_string(),
            "terminal_feature_body_lineage".to_string(),
        );
        source.attributes.insert(
            "feature_terminal_body_count".to_string(),
            selected.len().to_string(),
        );
    }
    true
}

fn prune_inactive_topology(ir: &mut CadIr, selected: &BTreeSet<BodyId>) {
    ir.model.bodies.retain(|body| selected.contains(&body.id));
    ir.model
        .regions
        .retain(|region| selected.contains(&region.body));
    let regions: BTreeSet<_> = ir
        .model
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect();
    ir.model
        .shells
        .retain(|shell| regions.contains(&shell.region));
    let shells: BTreeSet<_> = ir
        .model
        .shells
        .iter()
        .map(|shell| shell.id.clone())
        .collect();
    ir.model.faces.retain(|face| shells.contains(&face.shell));
    let faces: BTreeSet<_> = ir.model.faces.iter().map(|face| face.id.clone()).collect();
    ir.model.loops.retain(|loop_| faces.contains(&loop_.face));
    let loops: BTreeSet<_> = ir
        .model
        .loops
        .iter()
        .map(|loop_| loop_.id.clone())
        .collect();
    ir.model
        .coedges
        .retain(|coedge| loops.contains(&coedge.owner_loop));
    let edges: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .chain(
            ir.model
                .shells
                .iter()
                .flat_map(|shell| shell.wire_edges.iter().cloned()),
        )
        .collect();
    ir.model.edges.retain(|edge| edges.contains(&edge.id));
    let vertices: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .chain(
            ir.model
                .shells
                .iter()
                .flat_map(|shell| shell.free_vertices.iter().cloned()),
        )
        .collect();
    ir.model
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let points: BTreeSet<_> = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect();
    ir.model.points.retain(|point| points.contains(&point.id));
    prune_inactive_geometry(ir);
}

fn prune_inactive_geometry(ir: &mut CadIr) {
    let mut surfaces: BTreeSet<_> = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.clone())
        .collect();
    let mut curves: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect();
    let pcurves: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .filter_map(|coedge| coedge.pcurve.clone())
        .collect();

    loop {
        let old_surface_count = surfaces.len();
        let old_curve_count = curves.len();
        for procedural in &ir.model.procedural_surfaces {
            if !surfaces.contains(&procedural.surface) {
                continue;
            }
            match &procedural.definition {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    surfaces.insert(support.clone());
                }
                ProceduralSurfaceDefinition::Blend {
                    supports, spine, ..
                } => {
                    surfaces.extend(
                        supports
                            .iter()
                            .flatten()
                            .map(|support| support.surface.clone()),
                    );
                    curves.extend(spine.iter().cloned());
                }
                _ => {}
            }
        }
        for procedural in &ir.model.procedural_curves {
            if !curves.contains(&procedural.curve) {
                continue;
            }
            match &procedural.definition {
                ProceduralCurveDefinition::Intersection { context, .. }
                | ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                    surfaces.extend(context.sides.iter().filter_map(|side| side.surface.clone()));
                }
                _ => {}
            }
        }
        if surfaces.len() == old_surface_count && curves.len() == old_curve_count {
            break;
        }
    }

    ir.model
        .procedural_surfaces
        .retain(|procedural| surfaces.contains(&procedural.surface));
    ir.model
        .procedural_curves
        .retain(|procedural| curves.contains(&procedural.curve));
    ir.model
        .surfaces
        .retain(|surface| surfaces.contains(&surface.id));
    ir.model.curves.retain(|curve| curves.contains(&curve.id));
    ir.model
        .pcurves
        .retain(|pcurve| pcurves.contains(&pcurve.id));
}

fn finalize_point_topology(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let referenced_points: BTreeSet<_> = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect();
    if !ir.model.bodies.is_empty() {
        ir.model
            .points
            .retain(|point| referenced_points.contains(&point.id));
        return;
    }

    if ir.model.points.is_empty() {
        return;
    }

    let body_id = BodyId("nx:derived:point-body#0".to_string());
    let region_id = RegionId("nx:derived:point-region#0".to_string());
    let shell_id = ShellId("nx:derived:point-shell#0".to_string());
    let stream = annotations.stream("nx:container");
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotations
            .note(id, stream, 0)
            .tag("derived_point_topology");
        annotations.exactness(id, Exactness::Inferred);
    }

    let mut free_vertices = Vec::with_capacity(ir.model.points.len());
    for (index, point) in ir.model.points.iter().enumerate() {
        let vertex_id = VertexId(format!("nx:derived:point-vertex#{index}"));
        annotations
            .note(&vertex_id, stream, 0)
            .tag("derived_point_topology");
        annotations.exactness(&vertex_id, Exactness::Inferred);
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point.id.clone(),
            tolerance: None,
        });
        free_vertices.push(vertex_id);
    }
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::General,
        regions: vec![region_id],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
}

fn classify_body_kinds(ir: &mut CadIr) {
    let region_bodies: BTreeMap<_, _> = ir
        .model
        .regions
        .iter()
        .map(|region| (region.id.clone(), region.body.clone()))
        .collect();
    let shell_bodies: BTreeMap<_, _> = ir
        .model
        .shells
        .iter()
        .filter_map(|shell| {
            region_bodies
                .get(&shell.region)
                .cloned()
                .map(|body| (shell.id.clone(), body))
        })
        .collect();
    let face_bodies: BTreeMap<_, _> = ir
        .model
        .faces
        .iter()
        .filter_map(|face| {
            shell_bodies
                .get(&face.shell)
                .cloned()
                .map(|body| (face.id.clone(), body))
        })
        .collect();
    let loop_bodies: BTreeMap<_, _> = ir
        .model
        .loops
        .iter()
        .filter_map(|loop_| {
            face_bodies
                .get(&loop_.face)
                .cloned()
                .map(|body| (loop_.id.clone(), body))
        })
        .collect();
    let coedge_bodies: BTreeMap<_, _> = ir
        .model
        .coedges
        .iter()
        .filter_map(|coedge| {
            loop_bodies
                .get(&coedge.owner_loop)
                .cloned()
                .map(|body| (coedge.id.clone(), body))
        })
        .collect();
    let mut edge_uses = BTreeMap::<BodyId, BTreeMap<EdgeId, usize>>::new();
    for coedge in &ir.model.coedges {
        let Some(body) = coedge_bodies.get(&coedge.id) else {
            continue;
        };
        *edge_uses
            .entry(body.clone())
            .or_default()
            .entry(coedge.edge.clone())
            .or_default() += 1;
    }
    for body in &mut ir.model.bodies {
        body.kind = if edge_uses
            .get(&body.id)
            .is_some_and(|uses| !uses.is_empty() && uses.values().all(|use_count| *use_count == 2))
        {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        };
    }
}

fn linear_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.push(parameters[0]);
    knots.extend_from_slice(parameters);
    knots.push(*parameters.last().expect("non-empty chart parameters"));
    knots
}

pub(crate) fn assign_ext11_support_uv(
    ir: &CadIr,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    supports: [u32; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let surface_ids = supports.map(|support| surfaces_by_xmt.get(&support).cloned());
    let [Some(first_surface), Some(second_surface)] = surface_ids else {
        return None;
    };
    assign_ext11_support_uv_to_surfaces(
        ir,
        [&first_surface, &second_surface],
        points,
        fit_tolerance,
        lanes,
    )
}

pub(crate) fn assign_ext11_support_uv_to_surfaces(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let lane_matches_surface = |surface: &SurfaceId, lane: usize| {
        let Some(values) = lanes[lane]
            .as_deref()
            .filter(|values| values.len() == points.len())
        else {
            return false;
        };
        let Some(geometry) = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface)
            .map(|surface| &surface.geometry)
        else {
            return false;
        };
        values.iter().zip(points).all(|(uv, point)| {
            let uv = surface_parameters(geometry, *uv);
            model_surface_point(ir, surface, uv.u, uv.v)
                .is_some_and(|candidate| point_distance(candidate, *point) <= fit_tolerance)
        })
    };
    let matches = [
        [
            lane_matches_surface(surfaces[0], 0),
            lane_matches_surface(surfaces[0], 1),
        ],
        [
            lane_matches_surface(surfaces[1], 0),
            lane_matches_surface(surfaces[1], 1),
        ],
    ];
    let mut assigned = [None, None];
    let mut assigned_lanes = [None, None];
    for lane in 0..2 {
        let support_matches = [matches[0][lane], matches[1][lane]];
        let Some(support) = support_matches
            .iter()
            .position(|matches| *matches)
            .filter(|_| support_matches.iter().filter(|matches| **matches).count() == 1)
        else {
            continue;
        };
        if assigned[support].is_some() {
            return None;
        }
        assigned[support].clone_from(&lanes[lane]);
        assigned_lanes[support] = Some(lane);
    }
    if surfaces[0] != surfaces[1] && assigned.iter().filter(|lane| lane.is_some()).count() == 1 {
        let assigned_support = assigned.iter().position(Option::is_some)?;
        let assigned_lane = assigned_lanes[assigned_support]?;
        let other_support = 1 - assigned_support;
        let other_lane = 1 - assigned_lane;
        if lane_matches_surface(surfaces[other_support], other_lane) {
            assigned[other_support].clone_from(&lanes[other_lane]);
        }
    }
    assigned.iter().any(Option::is_some).then_some(assigned)
}

pub(crate) type PendingExt11SupportUv = (
    ProceduralCurveId,
    Vec<Point3>,
    Vec<f64>,
    f64,
    [Option<Vec<[f64; 2]>>; 2],
);

fn missing_support_parameter(value: f64) -> bool {
    value.to_bits() == MISSING_TOLERANCE.to_bits()
}

fn pcurve_requires_completion(pcurve: Option<&PcurveGeometry>) -> bool {
    match pcurve {
        None => true,
        Some(PcurveGeometry::Nurbs { control_points, .. }) => control_points.iter().any(|point| {
            !point.u.is_finite()
                || !point.v.is_finite()
                || missing_support_parameter(point.u)
                || missing_support_parameter(point.v)
        }),
        Some(PcurveGeometry::Line { origin, direction }) => [origin, direction]
            .into_iter()
            .any(|point| !point.u.is_finite() || !point.v.is_finite()),
    }
}

fn pcurve_control_point_seed(pcurve: Option<&PcurveGeometry>, index: usize) -> Option<Point2> {
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve? else {
        return None;
    };
    control_points.get(index).copied().filter(|point| {
        point.u.is_finite()
            && point.v.is_finite()
            && !missing_support_parameter(point.u)
            && !missing_support_parameter(point.v)
    })
}

pub(crate) fn complete_ext11_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    for (procedural_id, points, parameters, fit_tolerance, lanes) in pending {
        let Some(procedural_index) = ir
            .model
            .procedural_curves
            .iter()
            .position(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let (surfaces, missing) = match &ir.model.procedural_curves[procedural_index].definition {
            ProceduralCurveDefinition::Intersection { context, .. } => {
                let [Some(first), Some(second)] = &context.sides.clone().map(|side| side.surface)
                else {
                    continue;
                };
                (
                    [first.clone(), second.clone()],
                    context
                        .sides
                        .each_ref()
                        .map(|side| pcurve_requires_completion(side.pcurve.as_ref())),
                )
            }
            _ => continue,
        };
        if !missing.into_iter().any(|missing| missing) {
            continue;
        }
        let Some(assigned) = assign_ext11_support_uv_to_surfaces(
            ir,
            [&surfaces[0], &surfaces[1]],
            points,
            *fit_tolerance,
            lanes,
        ) else {
            continue;
        };
        let replacements: [Option<PcurveGeometry>; 2] = std::array::from_fn(|side| {
            if !missing[side] {
                return None;
            }
            let surface_geometry = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == surfaces[side])
                .map(|surface| &surface.geometry)?;
            let values = assigned[side].as_ref()?;
            if values
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || missing_support_parameter(*value))
            {
                return None;
            }
            Some(PcurveGeometry::Nurbs {
                degree: 1,
                knots: linear_knots(parameters),
                control_points: values
                    .iter()
                    .map(|uv| surface_parameters(surface_geometry, *uv))
                    .collect(),
                weights: None,
                periodic: false,
            })
        });
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition checked above");
        };
        for (side, replacement) in replacements.into_iter().enumerate() {
            if let Some(replacement) = replacement {
                context.sides[side].pcurve = Some(replacement);
            }
        }
    }
}

pub(crate) fn complete_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    loop {
        let before = pending_support_lanes_requiring_completion(ir, pending);
        complete_support_uv_wave(ir, pending);
        let after = pending_support_lanes_requiring_completion(ir, pending);
        if after >= before {
            break;
        }
    }
}

fn pending_support_lanes_requiring_completion(
    ir: &CadIr,
    pending: &[PendingExt11SupportUv],
) -> usize {
    pending
        .iter()
        .filter_map(|(procedural_id, ..)| {
            ir.model
                .procedural_curves
                .iter()
                .find(|procedural| &procedural.id == procedural_id)
        })
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            Some(
                context
                    .sides
                    .iter()
                    .filter(|side| pcurve_requires_completion(side.pcurve.as_ref()))
                    .count(),
            )
        })
        .sum()
}

fn complete_support_uv_wave(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    let mut replacements = Vec::new();
    for (procedural_id, points, parameters, fit_tolerance, _) in pending {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter()
            .find(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        for side in 0..2 {
            if !pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
                continue;
            }
            let Some(surface_id) = &context.sides[side].surface else {
                continue;
            };
            let Some(surface) = ir
                .model
                .surfaces
                .iter()
                .find(|surface| &surface.id == surface_id)
            else {
                continue;
            };
            let effective_fit_tolerance =
                blend_spine_cache_fit_tolerance(ir, surface_id, *fit_tolerance);
            let mut blend_grid = None;
            let mut blend_grid_initialized = false;
            let mut uv = Vec::with_capacity(points.len());
            for (point_index, point) in points.iter().enumerate() {
                let seed =
                    pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), point_index)
                        .or_else(|| uv.last().copied());
                let parameters = match &surface.geometry {
                    SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, *point, seed),
                    SurfaceGeometry::Procedural { .. } => {
                        let other_side = &context.sides[1 - side];
                        other_side
                            .surface
                            .as_ref()
                            .zip(other_side.pcurve.as_ref())
                            .and_then(|(other_surface, other_pcurve)| {
                                blend_boundary_parameter_from_support_pcurve(
                                    ir,
                                    surface_id,
                                    other_surface,
                                    other_pcurve,
                                    parameters[point_index],
                                    *point,
                                    effective_fit_tolerance,
                                )
                            })
                            .or_else(|| {
                                offset_surface_parameters_with_tolerance(
                                    ir,
                                    surface_id,
                                    *point,
                                    seed,
                                    Some(effective_fit_tolerance),
                                )
                            })
                            .or_else(|| {
                                blend_surface_parameters_for_fit_with_grid(
                                    ir,
                                    surface_id,
                                    *point,
                                    seed,
                                    effective_fit_tolerance,
                                    BlendParameterGrid::Disabled,
                                )
                            })
                            .or_else(|| {
                                if !blend_grid_initialized {
                                    blend_grid = blend_surface_parameter_grid(ir, surface_id, 0);
                                    blend_grid_initialized = true;
                                }
                                blend_surface_parameters_for_fit_with_grid(
                                    ir,
                                    surface_id,
                                    *point,
                                    seed,
                                    effective_fit_tolerance,
                                    blend_grid.as_deref().map_or(
                                        BlendParameterGrid::Disabled,
                                        BlendParameterGrid::Cached,
                                    ),
                                )
                            })
                    }
                    geometry => analytic_surface_parameters(geometry, *point),
                };
                let Some(parameters) = parameters else {
                    uv.clear();
                    break;
                };
                uv.push(parameters);
            }
            if uv.len() != points.len() {
                continue;
            }
            if matches!(
                surface.geometry,
                SurfaceGeometry::Cylinder { .. }
                    | SurfaceGeometry::Cone { .. }
                    | SurfaceGeometry::Sphere { .. }
                    | SurfaceGeometry::Torus { .. }
            ) {
                for index in 1..uv.len() {
                    let turns = ((uv[index - 1].u - uv[index].u) / std::f64::consts::TAU).round();
                    uv[index].u += turns * std::f64::consts::TAU;
                }
            }
            let reproduces_chart = uv.iter().zip(points).all(|(uv, point)| {
                decoded_surface_point(ir, surface_id, uv.u, uv.v)
                    .is_some_and(|actual| point_distance(actual, *point) <= effective_fit_tolerance)
            });
            if reproduces_chart {
                replacements.push((
                    procedural_id.clone(),
                    side,
                    PcurveGeometry::Nurbs {
                        degree: 1,
                        knots: linear_knots(parameters),
                        control_points: uv,
                        weights: None,
                        periodic: false,
                    },
                    effective_fit_tolerance,
                ));
            }
        }
    }
    for (procedural_id, side, pcurve, effective_fit_tolerance) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
            procedural.cache_fit_tolerance = Some(
                procedural
                    .cache_fit_tolerance
                    .unwrap_or(0.0)
                    .max(effective_fit_tolerance),
            );
        }
    }
    complete_coupled_support_uv(ir, pending);
}

pub(crate) fn blend_spine_cache_fit_tolerance(
    ir: &CadIr,
    surface: &SurfaceId,
    fit_tolerance: f64,
) -> f64 {
    blend_surface_definition(ir, surface)
        .and_then(|(_, spine, _, _)| {
            ir.model
                .procedural_curves
                .iter()
                .find(|procedural| procedural.curve == spine)
                .and_then(|procedural| procedural.cache_fit_tolerance)
        })
        .filter(|tolerance| tolerance.is_finite() && *tolerance > 0.0)
        .map_or(fit_tolerance, |tolerance| fit_tolerance + tolerance)
}

fn complete_coupled_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    let mut replacements = Vec::new();
    for (procedural_id, points, parameters, fit_tolerance, _) in pending {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter()
            .find(|procedural| &procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        let missing = context
            .sides
            .each_ref()
            .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
        let [Some(first_surface), Some(second_surface)] =
            context.sides.each_ref().map(|side| side.surface.as_ref())
        else {
            continue;
        };
        let surfaces = [first_surface, second_surface];
        let unresolved_procedural_support = (0..2).any(|side| {
            missing[side]
                && pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), 0).is_some()
                && ir.model.surfaces.iter().any(|surface| {
                    &surface.id == surfaces[side]
                        && matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                })
        });
        if !unresolved_procedural_support {
            continue;
        }
        let seeds = context
            .sides
            .each_ref()
            .map(|side| pcurve_control_point_seed(side.pcurve.as_ref(), 0));
        let Some(lanes) = continue_surface_intersection_parameters_with_seeds(
            ir,
            surfaces,
            points,
            *fit_tolerance,
            seeds,
        ) else {
            continue;
        };
        for side in 0..2 {
            if missing[side] {
                replacements.push((
                    procedural_id.clone(),
                    side,
                    PcurveGeometry::Nurbs {
                        degree: 1,
                        knots: linear_knots(parameters),
                        control_points: lanes[side].clone(),
                        weights: None,
                        periodic: false,
                    },
                ));
            }
        }
    }
    for (procedural_id, side, pcurve) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
        }
    }
}

pub(crate) fn complete_parameterization_equivalent_support_uv(ir: &mut CadIr) {
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .enumerate()
        .filter_map(|(procedural_index, procedural)| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            let missing = context
                .sides
                .each_ref()
                .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
            let target = match missing {
                [true, false] => 0,
                [false, true] => 1,
                _ => return None,
            };
            let source = 1 - target;
            let (Some(target_surface), Some(source_surface), Some(source_pcurve)) = (
                context.sides[target].surface.as_ref(),
                context.sides[source].surface.as_ref(),
                context.sides[source].pcurve.as_ref(),
            ) else {
                return None;
            };
            parameterization_equivalent_surfaces(ir, target_surface, source_surface)
                .then(|| (procedural_index, target, source_pcurve.clone()))
        })
        .collect::<Vec<_>>();
    for (procedural_index, side, pcurve) in replacements {
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition selected above");
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
        }
    }
}

pub(crate) fn parameterization_equivalent_surfaces(
    ir: &CadIr,
    first: &SurfaceId,
    second: &SurfaceId,
) -> bool {
    fn equivalent(
        ir: &CadIr,
        first: &SurfaceId,
        second: &SurfaceId,
        visited: &mut BTreeSet<(SurfaceId, SurfaceId)>,
    ) -> bool {
        if first == second {
            return true;
        }
        if !visited.insert((first.clone(), second.clone())) {
            return false;
        }
        let geometry = |id: &SurfaceId| {
            ir.model
                .surfaces
                .iter()
                .find(|surface| &surface.id == id)
                .map(|surface| &surface.geometry)
        };
        let (Some(first_geometry), Some(second_geometry)) = (geometry(first), geometry(second))
        else {
            return false;
        };
        if first_geometry == second_geometry {
            return true;
        }
        let construction = |geometry: &SurfaceGeometry| {
            let SurfaceGeometry::Procedural { construction } = geometry else {
                return None;
            };
            ir.model
                .procedural_surfaces
                .iter()
                .find(|procedural| &procedural.id == construction)
                .map(|procedural| &procedural.definition)
        };
        let (
            Some(ProceduralSurfaceDefinition::Offset {
                support: first_support,
                distance: first_distance,
                u_sense: first_u_sense,
                v_sense: first_v_sense,
                extension_flags: first_extensions,
            }),
            Some(ProceduralSurfaceDefinition::Offset {
                support: second_support,
                distance: second_distance,
                u_sense: second_u_sense,
                v_sense: second_v_sense,
                extension_flags: second_extensions,
            }),
        ) = (construction(first_geometry), construction(second_geometry))
        else {
            return false;
        };
        first_distance.to_bits() == second_distance.to_bits()
            && first_u_sense == second_u_sense
            && first_v_sense == second_v_sense
            && first_extensions == second_extensions
            && equivalent(ir, first_support, second_support, visited)
    }

    equivalent(ir, first, second, &mut BTreeSet::new())
}

pub(crate) fn attach_completed_intersection_pcurves(
    ir: &mut CadIr,
    graph: &Graph,
    prefix: &str,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (&loop_.id, &loop_.face))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (&face.id, &face.surface))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((&edge.id, edge.curve.as_ref()?)))
        .collect::<BTreeMap<_, _>>();
    let mut candidates =
        BTreeMap::<(CurveId, SurfaceId), Vec<(PcurveGeometry, [f64; 2], Option<f64>)>>::new();
    for procedural in &ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        for side in &context.sides {
            let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            let values = candidates
                .entry((procedural.curve.clone(), surface.clone()))
                .or_default();
            let candidate = (
                pcurve.clone(),
                context.parameter_range,
                procedural.cache_fit_tolerance,
            );
            if !values.contains(&candidate) {
                values.push(candidate);
            }
        }
    }

    let replacements = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.pcurve.is_none() && coedge.id.0.starts_with(prefix))
        .filter_map(|coedge| {
            let surface = loop_faces
                .get(&coedge.owner_loop)
                .and_then(|face| face_surfaces.get(*face))?;
            let curve = edge_curves.get(&coedge.edge)?;
            let [candidate] = candidates
                .get(&((*curve).clone(), (*surface).clone()))?
                .as_slice()
            else {
                return None;
            };
            pcurve_matches_edge(ir, &coedge.edge, surface, &candidate.0, candidate.2)
                .then(|| (coedge.id.clone(), candidate.clone()))
        })
        .collect::<Vec<_>>();
    for (coedge_id, (geometry, parameter_range, fit_tolerance)) in replacements {
        let Some(fin_xmt) = coedge_id
            .0
            .rsplit_once('#')
            .and_then(|(_, value)| value.parse::<u32>().ok())
        else {
            continue;
        };
        let pcurve_id = PcurveId(format!("{prefix}:intersection-pcurve-completed#{fin_xmt}"));
        if ir.model.pcurves.iter().any(|pcurve| pcurve.id == pcurve_id) {
            continue;
        }
        let source_offset = graph.get(17, fin_xmt).map_or(0, |node| node.pos as u64);
        annotations
            .note(&pcurve_id, source_stream, source_offset)
            .tag("INTERSECTION_PCURVE");
        annotations.derived(&pcurve_id, "geometry");
        annotations.derived(&pcurve_id, "parameter_range");
        if fit_tolerance.is_some() {
            annotations.derived(&pcurve_id, "fit_tolerance");
        }
        ir.model.pcurves.push(Pcurve {
            id: pcurve_id.clone(),
            geometry,
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some(parameter_range),
            fit_tolerance,
        });
        if let Some(coedge) = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_id && coedge.pcurve.is_none())
        {
            coedge.pcurve = Some(pcurve_id);
        }
    }
}

fn decoded_surface_point(ir: &CadIr, surface: &SurfaceId, u: f64, v: f64) -> Option<Point3> {
    decoded_surface_point_inner(ir, surface, u, v, 0)
}

fn decoded_surface_point_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    model_surface_point(ir, surface, u, v)
        .or_else(|| blend_surface_point_inner(ir, surface, u, v, depth + 1))
}

#[cfg(test)]
pub(crate) fn blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    blend_surface_parameters_inner(ir, surface, point, seed, None, BlendParameterGrid::Build, 0)
}

pub(crate) fn blend_surface_parameters_for_fit(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
) -> Option<Point2> {
    blend_surface_parameters_for_fit_with_grid(
        ir,
        surface,
        point,
        seed,
        fit_tolerance,
        BlendParameterGrid::Build,
    )
}

#[derive(Clone, Copy)]
enum BlendParameterGrid<'a> {
    Build,
    Cached(&'a [(Point2, Point3)]),
    Disabled,
}

fn blend_surface_parameters_for_fit_with_grid(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
    grid: BlendParameterGrid<'_>,
) -> Option<Point2> {
    blend_surface_parameters_inner(ir, surface, point, seed, Some(fit_tolerance), grid, 0)
}

fn blend_surface_parameters_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
    grid: BlendParameterGrid<'_>,
    depth: usize,
) -> Option<Point2> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition(ir, surface)?;
    if let (Some(seed), Some(fit_tolerance)) = (seed, fit_tolerance) {
        if let Some(parameters) =
            refine_blend_surface_parameters(ir, surface, point, seed, depth + 1).filter(
                |parameters| {
                    blend_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)
                        .is_some_and(|candidate| point_distance(candidate, point) <= fit_tolerance)
                },
            )
        {
            return Some(parameters);
        }
    }
    if let Some(fit_tolerance) = fit_tolerance {
        let boundary_parameters = [0usize, 1usize].map(|boundary| {
            blend_boundary_parameter(ir, surface, point, boundary, depth + 1).filter(|parameter| {
                blend_boundary_point(ir, surface, *parameter, boundary, depth + 1)
                    .is_some_and(|candidate| point_distance(candidate, point) <= fit_tolerance)
            })
        });
        if let Some((parameter, boundary)) = match boundary_parameters {
            [Some(parameter), None] => Some((parameter, 0usize)),
            [None, Some(parameter)] => Some((parameter, 1usize)),
            _ => None,
        } {
            return Some(Point2::new(parameter, boundary as f64));
        }
    }
    let angular =
        closest_spine_parameter(ir, &spine, point, seed.map(|seed| seed.u)).and_then(|u| {
            let (center, tangent, first, second, _) =
                blend_surface_frame(ir, surface, u, depth + 1)?;
            let radial = unit_vector(Vector3::new(
                point.x - center.x,
                point.y - center.y,
                point.z - center.z,
            ))?;
            let alpha = signed_angle(first, second, tangent);
            if !alpha.is_finite() || alpha.abs() <= 1.0e-12 {
                return None;
            }
            let theta = signed_angle(first, radial, tangent);
            (-2..=2)
                .filter_map(|turn| {
                    let v = (theta + f64::from(turn) * std::f64::consts::TAU) / alpha;
                    let candidate = blend_surface_point_inner(ir, surface, u, v, depth + 1)?;
                    let branch_distance = seed.map_or(v.abs(), |seed| (v - seed.v).abs());
                    Some((
                        Point2::new(u, v),
                        point_distance(candidate, point),
                        branch_distance,
                    ))
                })
                .min_by(|first, second| {
                    if (first.1 - second.1).abs() <= 1.0e-12 {
                        first.2.total_cmp(&second.2)
                    } else {
                        first.1.total_cmp(&second.1)
                    }
                })
                .map(|(parameters, _, _)| parameters)
        });
    if let Some(initial) = angular {
        let parameters = refine_blend_surface_parameters(ir, surface, point, initial, depth + 1)
            .unwrap_or(initial);
        if let Some(candidate) =
            blend_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)
        {
            let distance = point_distance(candidate, point);
            if fit_tolerance.is_none_or(|tolerance| distance <= tolerance) {
                return Some(parameters);
            }
        }
    }
    let initial = match grid {
        BlendParameterGrid::Build => coarse_blend_surface_parameters(ir, surface, point, depth + 1),
        BlendParameterGrid::Cached(grid) => closest_blend_surface_grid_parameters(grid, point),
        BlendParameterGrid::Disabled => None,
    }?;
    let parameters =
        refine_blend_surface_parameters(ir, surface, point, initial, depth + 1).unwrap_or(initial);
    if !(0.0..=1.0).contains(&parameters.v) {
        return None;
    }
    let candidate = blend_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)?;
    let distance = point_distance(candidate, point);
    fit_tolerance
        .is_none_or(|tolerance| distance <= tolerance)
        .then_some(parameters)
}

pub(crate) fn coarse_blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
) -> Option<Point2> {
    let grid = blend_surface_parameter_grid(ir, surface, depth)?;
    closest_blend_surface_grid_parameters(&grid, point)
}

fn blend_surface_parameter_grid(
    ir: &CadIr,
    surface: &SurfaceId,
    depth: usize,
) -> Option<Vec<(Point2, Point3)>> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition(ir, surface)?;
    let curve = ir.model.curves.iter().find(|curve| curve.id == spine)?;
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree).ok()?;
    let count = nurbs.control_points.len();
    let domain = [*nurbs.knots.get(degree)?, *nurbs.knots.get(count)?];
    if !domain.into_iter().all(f64::is_finite) || domain[0] >= domain[1] {
        return None;
    }
    let mut grid = Vec::with_capacity(9 * 5);
    for u_index in 0..=8 {
        let u = domain[0] + (domain[1] - domain[0]) * f64::from(u_index) / 8.0;
        let frame = blend_surface_frame(ir, surface, u, depth + 1);
        for v_index in 0..=4 {
            let parameters = Point2::new(u, f64::from(v_index) / 4.0);
            let point = match v_index {
                0 => blend_boundary_point(ir, surface, u, 0, depth + 1),
                4 => blend_boundary_point(ir, surface, u, 1, depth + 1),
                _ => frame.map(|frame| blend_surface_point_from_frame(frame, parameters.v)),
            };
            let Some(point) = point else {
                continue;
            };
            grid.push((parameters, point));
        }
    }
    (!grid.is_empty()).then_some(grid)
}

fn closest_blend_surface_grid_parameters(
    grid: &[(Point2, Point3)],
    point: Point3,
) -> Option<Point2> {
    grid.iter()
        .min_by(|(_, first), (_, second)| {
            point_distance(*first, point).total_cmp(&point_distance(*second, point))
        })
        .map(|(parameters, _)| *parameters)
}

pub(crate) fn refine_blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    mut parameters: Point2,
    depth: usize,
) -> Option<Point2> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition(ir, surface)?;
    let u_domain = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == spine)
        .and_then(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => {
                let degree = usize::try_from(nurbs.degree).ok()?;
                let count = nurbs.control_points.len();
                Some([*nurbs.knots.get(degree)?, *nurbs.knots.get(count)?])
            }
            _ => None,
        });
    if let Some(domain) = u_domain {
        parameters.u = parameters.u.clamp(domain[0], domain[1]);
    }
    let squared_distance = |candidate: Point3| {
        (candidate.x - point.x).powi(2)
            + (candidate.y - point.y).powi(2)
            + (candidate.z - point.z).powi(2)
    };
    for _ in 0..16 {
        let position =
            blend_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let current_distance = squared_distance(position);
        let u_step = parameter_derivative_step(parameters.u, u_domain);
        let v_step = parameter_derivative_step(parameters.v, None);
        let derivative = |along_u: bool, step: f64| {
            let mut before = parameters;
            let mut after = parameters;
            if along_u {
                before.u -= step;
                after.u += step;
                if let Some(domain) = u_domain {
                    before.u = before.u.clamp(domain[0], domain[1]);
                    after.u = after.u.clamp(domain[0], domain[1]);
                }
            } else {
                before.v -= step;
                after.v += step;
            }
            let width = if along_u {
                after.u - before.u
            } else {
                after.v - before.v
            };
            if !width.is_finite() || width == 0.0 {
                return None;
            }
            let first = blend_surface_point_inner(ir, surface, before.u, before.v, depth + 1)?;
            let second = blend_surface_point_inner(ir, surface, after.u, after.v, depth + 1)?;
            Some(Vector3::new(
                (second.x - first.x) / width,
                (second.y - first.y) / width,
                (second.z - first.z) / width,
            ))
        };
        let du = derivative(true, u_step)?;
        let dv = derivative(false, v_step)?;
        let Some((step_u, step_v)) = least_squares_step(du, dv, residual) else {
            break;
        };
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..8 {
            let mut candidate =
                Point2::new(parameters.u - scale * step_u, parameters.v - scale * step_v);
            if let Some(domain) = u_domain {
                candidate.u = candidate.u.clamp(domain[0], domain[1]);
            }
            if let Some(position) =
                blend_surface_point_inner(ir, surface, candidate.u, candidate.v, depth + 1)
            {
                if squared_distance(position) < current_distance {
                    accepted = Some(candidate);
                    break;
                }
            }
            scale *= 0.5;
        }
        let Some(candidate) = accepted else {
            break;
        };
        let converged = (candidate.u - parameters.u).abs() <= 1.0e-12 * (1.0 + parameters.u.abs())
            && (candidate.v - parameters.v).abs() <= 1.0e-12 * (1.0 + parameters.v.abs());
        parameters = candidate;
        if converged {
            break;
        }
    }
    Some(parameters)
}

#[cfg(test)]
pub(crate) fn blend_surface_point(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
) -> Option<Point3> {
    blend_surface_point_inner(ir, surface, u, v, 0)
}

fn blend_surface_point_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    if v.to_bits() == 0.0f64.to_bits() {
        return blend_boundary_point(ir, surface, u, 0, depth + 1);
    }
    if v.to_bits() == 1.0f64.to_bits() {
        return blend_boundary_point(ir, surface, u, 1, depth + 1);
    }
    let frame = blend_surface_frame(ir, surface, u, depth + 1)?;
    Some(blend_surface_point_from_frame(frame, v))
}

type BlendSurfaceFrame = (Point3, Vector3, Vector3, Vector3, f64);

fn blend_surface_point_from_frame(
    (center, tangent, first, second, radius): BlendSurfaceFrame,
    v: f64,
) -> Point3 {
    let alpha = signed_angle(first, second, tangent);
    let radial = rodrigues_rotate(first, tangent, v * alpha);
    Point3::new(
        center.x + radius * radial.x,
        center.y + radius * radial.y,
        center.z + radius * radial.z,
    )
}

fn blend_surface_frame(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    depth: usize,
) -> Option<BlendSurfaceFrame> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, reversed) = blend_surface_definition(ir, surface)?;
    let center = model_curve_point(ir, &spine, u)?;
    let tangent = model_curve_tangent(ir, &spine, u)?;
    let first = spine_contact_direction(
        ir,
        &supports[0],
        &spine,
        u,
        center,
        (radius, reversed[0]),
        depth + 1,
    )
    .or_else(|| surface_contact_direction(ir, &supports[0], center, depth + 1))?;
    let second = spine_contact_direction(
        ir,
        &supports[1],
        &spine,
        u,
        center,
        (radius, reversed[1]),
        depth + 1,
    )
    .or_else(|| surface_contact_direction(ir, &supports[1], center, depth + 1))?;
    Some((center, tangent, first, second, radius))
}

fn spine_contact_direction(
    ir: &CadIr,
    support: &SurfaceId,
    spine: &CurveId,
    parameter: f64,
    center: Point3,
    contact: (f64, bool),
    depth: usize,
) -> Option<Vector3> {
    let contact = spine_contact_point(
        ir,
        support,
        spine,
        parameter,
        contact.0,
        contact.1,
        depth + 1,
    )?;
    unit_vector(Vector3::new(
        contact.x - center.x,
        contact.y - center.y,
        contact.z - center.z,
    ))
}

fn blend_boundary_point(
    ir: &CadIr,
    surface: &SurfaceId,
    parameter: f64,
    boundary: usize,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, reversed) = blend_surface_definition(ir, surface)?;
    spine_contact_point(
        ir,
        supports.get(boundary)?,
        &spine,
        parameter,
        radius,
        *reversed.get(boundary)?,
        depth + 1,
    )
}

fn blend_boundary_parameter(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    boundary: usize,
    depth: usize,
) -> Option<f64> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, reversed) = blend_surface_definition(ir, surface)?;
    let support = supports.get(boundary)?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == support)?;
    let uv = match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, point, None),
        SurfaceGeometry::Procedural { .. } => offset_surface_parameters(ir, support, point, None),
        geometry => analytic_surface_parameters(geometry, point),
    }?;
    let pcurve = spine_contact_pcurve(
        ir,
        support,
        &spine,
        radius,
        *reversed.get(boundary)?,
        depth + 1,
    )?;
    closest_pcurve_parameter(pcurve, uv)
}

fn blend_boundary_parameter_from_support_pcurve(
    ir: &CadIr,
    blend: &SurfaceId,
    support: &SurfaceId,
    support_pcurve: &PcurveGeometry,
    curve_parameter: f64,
    point: Point3,
    fit_tolerance: f64,
) -> Option<Point2> {
    let (supports, spine, radius, reversed) = blend_surface_definition(ir, blend)?;
    let boundary = supports
        .iter()
        .position(|candidate| parameterization_equivalent_surfaces(ir, candidate, support))?;
    if supports
        .iter()
        .filter(|candidate| parameterization_equivalent_surfaces(ir, candidate, support))
        .count()
        != 1
    {
        return None;
    }
    let support_uv = pcurve_uv(support_pcurve, curve_parameter)?;
    let contact_pcurve = spine_contact_pcurve(ir, support, &spine, radius, reversed[boundary], 0)?;
    let parameter = closest_pcurve_parameter(contact_pcurve, support_uv)?;
    blend_boundary_point(ir, blend, parameter, boundary, 0)
        .filter(|candidate| point_distance(*candidate, point) <= fit_tolerance)
        .map(|_| Point2::new(parameter, boundary as f64))
}

pub(crate) fn closest_pcurve_parameter(pcurve: &PcurveGeometry, point: Point2) -> Option<f64> {
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        ..
    } = pcurve
    else {
        return None;
    };
    let degree = usize::try_from(*degree).ok()?;
    let count = control_points.len();
    if count <= degree || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    if !domain[0].is_finite() || !domain[1].is_finite() || domain[0] >= domain[1] {
        return None;
    }
    if degree != 1 || weights.is_some() {
        let squared_distance = |parameter| {
            let position = pcurve_uv(pcurve, parameter)?;
            Some((position.u - point.u).powi(2) + (position.v - point.v).powi(2))
        };
        let samples = knot_domain_samples(knots, degree, domain);
        let distances = samples
            .iter()
            .map(|parameter| squared_distance(*parameter))
            .collect::<Option<Vec<_>>>()?;
        let mut best = samples[0];
        let mut best_distance = distances[0];
        for (index, &distance) in distances.iter().enumerate() {
            if distance < best_distance {
                best = samples[index];
                best_distance = distance;
            }
            if index > 0
                && index + 1 < samples.len()
                && distance <= distances[index - 1]
                && distance <= distances[index + 1]
            {
                let (parameter, distance) = golden_section_minimum(
                    samples[index - 1],
                    samples[index + 1],
                    &squared_distance,
                )?;
                if distance < best_distance {
                    best = parameter;
                    best_distance = distance;
                }
            }
        }
        return Some(best);
    }
    let mut candidates = control_points
        .windows(2)
        .enumerate()
        .filter_map(|(index, segment)| {
            let start = segment[0];
            let end = segment[1];
            let direction = Point2::new(end.u - start.u, end.v - start.v);
            let squared_length = direction.u * direction.u + direction.v * direction.v;
            if !squared_length.is_finite() || squared_length == 0.0 {
                return None;
            }
            let fraction = (((point.u - start.u) * direction.u
                + (point.v - start.v) * direction.v)
                / squared_length)
                .clamp(0.0, 1.0);
            let span_start = *knots.get(index + 1)?;
            let span_end = *knots.get(index + 2)?;
            if !span_start.is_finite() || !span_end.is_finite() || span_start >= span_end {
                return None;
            }
            let projected = Point2::new(
                start.u + fraction * direction.u,
                start.v + fraction * direction.v,
            );
            let squared_distance =
                (projected.u - point.u).powi(2) + (projected.v - point.v).powi(2);
            Some((
                span_start + fraction * (span_end - span_start),
                squared_distance,
            ))
        });
    let first = candidates.next()?;
    let best = candidates.fold(first, |best, candidate| {
        if candidate.1 < best.1 {
            candidate
        } else {
            best
        }
    });
    Some(best.0)
}

fn spine_contact_point(
    ir: &CadIr,
    support: &SurfaceId,
    spine: &CurveId,
    parameter: f64,
    radius: f64,
    reversed: bool,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    let pcurve = spine_contact_pcurve(ir, support, spine, radius, reversed, depth + 1)?;
    let uv = pcurve_uv(pcurve, parameter)?;
    decoded_surface_point_inner(ir, support, uv.u, uv.v, depth + 1)
}

fn spine_contact_pcurve<'a>(
    ir: &'a CadIr,
    support: &SurfaceId,
    spine: &CurveId,
    radius: f64,
    reversed: bool,
    depth: usize,
) -> Option<&'a PcurveGeometry> {
    (depth < 32).then_some(())?;
    let procedural = ir.model.procedural_curves.iter().find(|candidate| {
        candidate.curve == *spine
            && matches!(
                candidate.definition,
                ProceduralCurveDefinition::Intersection { .. }
            )
    })?;
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        unreachable!("definition selected above");
    };
    let candidates = context.sides.iter().filter_map(|side| {
        let side_surface = side.surface.as_ref()?;
        let pcurve = side.pcurve.as_ref()?;
        let offset = constant_surface_offset_between(ir, support, side_surface, depth + 1)?;
        if !blend_contact_offset_matches(0.0, offset, radius, reversed) {
            return None;
        }
        Some(pcurve)
    });
    let candidates = candidates.collect::<Vec<_>>();
    let [pcurve] = candidates.as_slice() else {
        return None;
    };
    Some(*pcurve)
}

pub(crate) fn constant_surface_offset_between(
    ir: &CadIr,
    support: &SurfaceId,
    offset_surface: &SurfaceId,
    depth: usize,
) -> Option<f64> {
    let (support_base, support_offset) = surface_offset_lineage(ir, support, depth + 1)?;
    let (offset_base, offset_distance) = surface_offset_lineage(ir, offset_surface, depth + 1)?;
    if support_base == offset_base {
        return Some(offset_distance - support_offset);
    }
    let support_geometry = &ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == support_base)?
        .geometry;
    let offset_geometry = &ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == offset_base)?
        .geometry;
    let base_offset = analytic_surface_offset(support_geometry, offset_geometry)
        .or_else(|| blend_surface_offset(ir, &support_base, &offset_base, depth + 1))?;
    Some(base_offset + offset_distance - support_offset)
}

fn blend_surface_offset(
    ir: &CadIr,
    support: &SurfaceId,
    offset: &SurfaceId,
    depth: usize,
) -> Option<f64> {
    (depth < 32).then_some(())?;
    let (support_carriers, support_spine, support_radius, support_reversed) =
        blend_surface_definition(ir, support)?;
    let (offset_carriers, offset_spine, offset_radius, offset_reversed) =
        blend_surface_definition(ir, offset)?;
    (support_spine == offset_spine).then_some(())?;

    let distance = offset_radius - support_radius;
    let magnitude = distance.abs();
    let matches = [[0usize, 1usize], [1usize, 0usize]]
        .into_iter()
        .filter(|permutation| {
            permutation
                .iter()
                .enumerate()
                .all(|(support_index, &offset_index)| {
                    support_reversed[support_index] == offset_reversed[offset_index]
                        && constant_surface_offset_between(
                            ir,
                            &support_carriers[support_index],
                            &offset_carriers[offset_index],
                            depth + 1,
                        )
                        .is_some_and(|carrier_distance| {
                            blend_contact_offset_matches(0.0, carrier_distance, magnitude, false)
                        })
                })
        })
        .count();
    (matches == 1).then_some(distance)
}

fn analytic_surface_offset(support: &SurfaceGeometry, offset: &SurfaceGeometry) -> Option<f64> {
    match (support, offset) {
        (
            SurfaceGeometry::Plane {
                origin: support_origin,
                normal: support_normal,
                u_axis: support_u,
            },
            SurfaceGeometry::Plane {
                origin: offset_origin,
                normal: offset_normal,
                u_axis: offset_u,
            },
        ) if support_normal == offset_normal && support_u == offset_u => {
            let delta = Vector3::new(
                offset_origin.x - support_origin.x,
                offset_origin.y - support_origin.y,
                offset_origin.z - support_origin.z,
            );
            let distance = dot_vector(delta, *support_normal);
            let residual = Vector3::new(
                delta.x - distance * support_normal.x,
                delta.y - distance * support_normal.y,
                delta.z - distance * support_normal.z,
            );
            let scale = [
                support_origin.x,
                support_origin.y,
                support_origin.z,
                offset_origin.x,
                offset_origin.y,
                offset_origin.z,
                distance,
            ]
            .into_iter()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
            let tolerance = 64.0 * f64::EPSILON * scale;
            (dot_vector(residual, residual) <= tolerance * tolerance).then_some(distance)
        }
        (
            SurfaceGeometry::Cylinder {
                origin: support_origin,
                axis: support_axis,
                ref_direction: support_ref,
                radius: support_radius,
            },
            SurfaceGeometry::Cylinder {
                origin: offset_origin,
                axis: offset_axis,
                ref_direction: offset_ref,
                radius: offset_radius,
            },
        ) if support_origin == offset_origin
            && support_axis == offset_axis
            && support_ref == offset_ref =>
        {
            Some(offset_radius - support_radius)
        }
        (
            SurfaceGeometry::Sphere {
                center: support_center,
                axis: support_axis,
                ref_direction: support_ref,
                radius: support_radius,
            },
            SurfaceGeometry::Sphere {
                center: offset_center,
                axis: offset_axis,
                ref_direction: offset_ref,
                radius: offset_radius,
            },
        ) if support_center == offset_center
            && support_axis == offset_axis
            && support_ref == offset_ref =>
        {
            Some(offset_radius - support_radius)
        }
        (
            SurfaceGeometry::Torus {
                center: support_center,
                axis: support_axis,
                ref_direction: support_ref,
                major_radius: support_major,
                minor_radius: support_minor,
            },
            SurfaceGeometry::Torus {
                center: offset_center,
                axis: offset_axis,
                ref_direction: offset_ref,
                major_radius: offset_major,
                minor_radius: offset_minor,
            },
        ) if support_center == offset_center
            && support_axis == offset_axis
            && support_ref == offset_ref
            && support_major.to_bits() == offset_major.to_bits() =>
        {
            Some(offset_minor - support_minor)
        }
        _ => None,
    }
}

pub(crate) fn blend_contact_offset_matches(
    support_offset: f64,
    spine_side_offset: f64,
    radius: f64,
    _reversed: bool,
) -> bool {
    let actual = (spine_side_offset - support_offset).abs();
    let expected = radius.abs();
    let scale = actual.max(expected).max(1.0);
    actual.is_finite()
        && expected.is_finite()
        && (actual - expected).abs() <= 64.0 * f64::EPSILON * scale
}

fn surface_offset_lineage(
    ir: &CadIr,
    surface: &SurfaceId,
    depth: usize,
) -> Option<(SurfaceId, f64)> {
    (depth < 32).then_some(())?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Procedural { construction } = &carrier.geometry else {
        return Some((surface.clone(), 0.0));
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| candidate.id == *construction && candidate.surface == *surface)?;
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        return Some((surface.clone(), 0.0));
    };
    let (base, accumulated) = surface_offset_lineage(ir, support, depth + 1)?;
    Some((base, accumulated + distance))
}

fn blend_surface_definition(
    ir: &CadIr,
    surface: &SurfaceId,
) -> Option<([SurfaceId; 2], CurveId, f64, [bool; 2])> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Procedural { construction } = &carrier.geometry else {
        return None;
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| &candidate.id == construction && &candidate.surface == surface)?;
    let ProceduralSurfaceDefinition::Blend {
        supports: [Some(first), Some(second)],
        spine: Some(spine),
        radius: BlendRadiusLaw::Constant { signed_radius },
        cross_section: BlendCrossSection::Circular,
        ..
    } = &procedural.definition
    else {
        return None;
    };
    let radius = signed_radius.abs();
    (radius.is_finite() && radius > 0.0).then(|| {
        (
            [first.surface.clone(), second.surface.clone()],
            spine.clone(),
            radius,
            [first.reversed, second.reversed],
        )
    })
}

fn surface_contact_direction(
    ir: &CadIr,
    surface: &SurfaceId,
    center: Point3,
    depth: usize,
) -> Option<Vector3> {
    (depth < 32).then_some(())?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let parameters = match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, center, None),
        SurfaceGeometry::Procedural { .. } => offset_surface_parameters(ir, surface, center, None)
            .or_else(|| {
                blend_surface_parameters_inner(
                    ir,
                    surface,
                    center,
                    None,
                    None,
                    BlendParameterGrid::Build,
                    depth + 1,
                )
            }),
        geometry => analytic_surface_parameters(geometry, center),
    }?;
    let contact = decoded_surface_point_inner(ir, surface, parameters.u, parameters.v, depth + 1)?;
    unit_vector(Vector3::new(
        contact.x - center.x,
        contact.y - center.y,
        contact.z - center.z,
    ))
}

fn model_curve_point(ir: &CadIr, curve: &CurveId, parameter: f64) -> Option<Point3> {
    let carrier = ir
        .model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)?;
    curve_point(&carrier.geometry, parameter)
}

fn model_curve_tangent(ir: &CadIr, curve: &CurveId, parameter: f64) -> Option<Vector3> {
    let step = 1.0e-6 * (1.0 + parameter.abs());
    let before = model_curve_point(ir, curve, parameter - step)?;
    let after = model_curve_point(ir, curve, parameter + step)?;
    unit_vector(Vector3::new(
        after.x - before.x,
        after.y - before.y,
        after.z - before.z,
    ))
}

pub(crate) fn closest_spine_parameter(
    ir: &CadIr,
    curve: &CurveId,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let carrier = ir
        .model
        .curves
        .iter()
        .find(|candidate| &candidate.id == curve)?;
    match &carrier.geometry {
        CurveGeometry::Line { origin, direction } => Some(
            (point.x - origin.x) * direction.x
                + (point.y - origin.y) * direction.y
                + (point.z - origin.z) * direction.z,
        ),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            ..
        }
        | CurveGeometry::Ellipse {
            center,
            axis,
            major_direction: ref_direction,
            ..
        } => closest_periodic_analytic_curve_parameter(
            &carrier.geometry,
            *center,
            *axis,
            *ref_direction,
            point,
            seed,
        ),
        CurveGeometry::Nurbs(nurbs) => {
            let degree = usize::try_from(nurbs.degree).ok()?;
            let count = nurbs.control_points.len();
            let domain = [*nurbs.knots.get(degree)?, *nurbs.knots.get(count)?];
            if domain[0] >= domain[1] {
                return None;
            }
            closest_nurbs_curve_parameter(
                &carrier.geometry,
                &nurbs.knots,
                degree,
                domain,
                point,
                seed,
            )
        }
        _ => None,
    }
}

fn closest_periodic_analytic_curve_parameter(
    geometry: &CurveGeometry,
    center: Point3,
    axis: Vector3,
    reference: Vector3,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let transverse = cross_vector(axis, reference);
    let delta = Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let phase = dot_vector(delta, transverse).atan2(dot_vector(delta, reference));
    phase.is_finite().then_some(())?;
    let anchor = seed.map_or(phase, |seed| {
        phase + ((seed - phase) / std::f64::consts::TAU).round() * std::f64::consts::TAU
    });
    let lower = anchor - std::f64::consts::PI;
    let step = std::f64::consts::TAU / 64.0;
    let squared_distance = |parameter| {
        let position = curve_point(geometry, parameter)?;
        Some(
            (position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2),
        )
    };
    let samples = (0..=64)
        .map(|index| lower + f64::from(index) * step)
        .collect::<Vec<_>>();
    let distances = samples
        .iter()
        .map(|parameter| squared_distance(*parameter))
        .collect::<Option<Vec<_>>>()?;
    let mut best_index = 0;
    for index in 1..distances.len() {
        if distances[index] < distances[best_index]
            || distances[index] == distances[best_index]
                && (samples[index] - anchor).abs() < (samples[best_index] - anchor).abs()
        {
            best_index = index;
        }
    }
    let bracket_center = match best_index {
        0 => samples[0] + std::f64::consts::TAU,
        64 => samples[64] - std::f64::consts::TAU,
        _ => samples[best_index],
    };
    let (parameter, _) = golden_section_minimum(
        bracket_center - step,
        bracket_center + step,
        &squared_distance,
    )?;
    Some(parameter + ((anchor - parameter) / std::f64::consts::TAU).round() * std::f64::consts::TAU)
}

fn closest_nurbs_curve_parameter(
    geometry: &CurveGeometry,
    knots: &[f64],
    degree: usize,
    domain: [f64; 2],
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let squared_distance = |parameter| {
        let position = curve_point(geometry, parameter)?;
        Some(
            (position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2),
        )
    };
    let samples = knot_domain_samples(knots, degree, domain);
    let distances = samples
        .iter()
        .map(|parameter| squared_distance(*parameter))
        .collect::<Option<Vec<_>>>()?;
    let mut best = samples[0];
    let mut best_distance = distances[0];
    let mut best_seed_distance = seed.map_or(best.abs(), |seed| (best - seed).abs());
    let mut consider = |parameter: f64, distance: f64| {
        let seed_distance = seed.map_or(parameter.abs(), |seed| (parameter - seed).abs());
        let same_point = (distance - best_distance).abs()
            <= f64::EPSILON * 64.0 * distance.abs().max(best_distance.abs()).max(1.0);
        if distance < best_distance && !same_point
            || same_point && seed_distance < best_seed_distance
        {
            best = parameter;
            best_distance = distance;
            best_seed_distance = seed_distance;
        }
    };
    for (index, &distance) in distances.iter().enumerate() {
        consider(samples[index], distance);
        if index > 0
            && index + 1 < samples.len()
            && distance <= distances[index - 1]
            && distance <= distances[index + 1]
        {
            let (parameter, distance) =
                golden_section_minimum(samples[index - 1], samples[index + 1], &squared_distance)?;
            consider(parameter, distance);
        }
    }
    if let Some(seed) = seed {
        let seed = seed.clamp(domain[0], domain[1]);
        let insertion = samples.partition_point(|parameter| *parameter < seed);
        let lower = samples[insertion.saturating_sub(1)];
        let upper = samples[insertion.min(samples.len() - 1)];
        if lower < upper {
            let (parameter, distance) = golden_section_minimum(lower, upper, &squared_distance)?;
            consider(parameter, distance);
        } else {
            consider(seed, squared_distance(seed)?);
        }
    }
    Some(best)
}

fn knot_domain_samples(knots: &[f64], degree: usize, domain: [f64; 2]) -> Vec<f64> {
    let subdivisions = 2 * (degree + 1).max(2);
    let mut samples = vec![domain[0]];
    for span in knots[degree..].windows(2) {
        let start = span[0].max(domain[0]);
        let end = span[1].min(domain[1]);
        if start >= end {
            continue;
        }
        for index in 1..=subdivisions {
            samples.push(start + (end - start) * index as f64 / subdivisions as f64);
        }
        if end >= domain[1] {
            break;
        }
    }
    samples.sort_by(f64::total_cmp);
    samples.dedup_by(|left, right| *left == *right);
    samples
}

fn golden_section_minimum(
    mut lower: f64,
    mut upper: f64,
    value: &impl Fn(f64) -> Option<f64>,
) -> Option<(f64, f64)> {
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut left = upper - ratio * (upper - lower);
    let mut right = lower + ratio * (upper - lower);
    let mut left_value = value(left)?;
    let mut right_value = value(right)?;
    for _ in 0..64 {
        if left_value <= right_value {
            upper = right;
            right = left;
            right_value = left_value;
            left = upper - ratio * (upper - lower);
            left_value = value(left)?;
        } else {
            lower = left;
            left = right;
            left_value = right_value;
            right = lower + ratio * (upper - lower);
            right_value = value(right)?;
        }
    }
    if left_value <= right_value {
        Some((left, left_value))
    } else {
        Some((right, right_value))
    }
}

fn signed_angle(first: Vector3, second: Vector3, axis: Vector3) -> f64 {
    dot_vector(cross_vector(first, second), axis).atan2(dot_vector(first, second))
}

fn rodrigues_rotate(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cross = cross_vector(axis, vector);
    let dot = dot_vector(axis, vector);
    Vector3::new(
        vector.x * angle.cos() + cross.x * angle.sin() + axis.x * dot * (1.0 - angle.cos()),
        vector.y * angle.cos() + cross.y * angle.sin() + axis.y * dot * (1.0 - angle.cos()),
        vector.z * angle.cos() + cross.z * angle.sin() + axis.z * dot * (1.0 - angle.cos()),
    )
}

fn cross_vector(first: Vector3, second: Vector3) -> Vector3 {
    Vector3::new(
        first.y * second.z - first.z * second.y,
        first.z * second.x - first.x * second.z,
        first.x * second.y - first.y * second.x,
    )
}

fn dot_vector(first: Vector3, second: Vector3) -> f64 {
    first.x * second.x + first.y * second.y + first.z * second.z
}

fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = dot_vector(vector, vector).sqrt();
    (norm.is_finite() && norm > 0.0)
        .then(|| Vector3::new(vector.x / norm, vector.y / norm, vector.z / norm))
}

pub(crate) fn offset_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    offset_surface_parameters_with_tolerance(ir, surface, point, seed, None)
}

pub(crate) fn offset_surface_parameters_with_tolerance(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<Point2> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Procedural { construction } = &carrier.geometry else {
        return None;
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|candidate| &candidate.id == construction && &candidate.surface == surface)?;
    let ProceduralSurfaceDefinition::Offset { support, .. } = &procedural.definition else {
        return None;
    };
    let domain = surface_parameter_domain(ir, support);
    let mut parameters = seed
        .or_else(|| initial_surface_parameters(ir, support, point, None))
        .or_else(|| {
            domain.and_then(|domain| coarse_model_surface_parameters(ir, surface, point, domain))
        })?;
    clamp_surface_parameters(&mut parameters, domain);
    for _ in 0..32 {
        let position = model_surface_point(ir, surface, parameters.u, parameters.v)?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        if fit_tolerance.is_some_and(|tolerance| {
            tolerance.is_finite()
                && tolerance >= 0.0
                && dot_vector(residual, residual) <= tolerance * tolerance
        }) {
            break;
        }
        let u_step = parameter_derivative_step(parameters.u, domain.map(|domain| domain.0));
        let v_step = parameter_derivative_step(parameters.v, domain.map(|domain| domain.1));
        let du =
            model_surface_derivative(ir, surface, parameters, u_step, true, domain, [None, None])?;
        let dv =
            model_surface_derivative(ir, surface, parameters, v_step, false, domain, [None, None])?;
        let Some((step_u, step_v)) = least_squares_step(du, dv, residual) else {
            break;
        };
        parameters.u -= step_u;
        parameters.v -= step_v;
        clamp_surface_parameters(&mut parameters, domain);
        if step_u.abs() <= 1.0e-12 * (1.0 + parameters.u.abs())
            && step_v.abs() <= 1.0e-12 * (1.0 + parameters.v.abs())
        {
            break;
        }
    }
    Some(parameters)
}

fn coarse_model_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    domain: ([f64; 2], [f64; 2]),
) -> Option<Point2> {
    let (u_domain, v_domain) = domain;
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for ui in 0..=8 {
        for vi in 0..=8 {
            let parameters = Point2::new(
                u_domain[0] + (u_domain[1] - u_domain[0]) * f64::from(ui) / 8.0,
                v_domain[0] + (v_domain[1] - v_domain[0]) * f64::from(vi) / 8.0,
            );
            let Some(candidate) = model_surface_point(ir, surface, parameters.u, parameters.v)
            else {
                continue;
            };
            let distance = (candidate.x - point.x).powi(2)
                + (candidate.y - point.y).powi(2)
                + (candidate.z - point.z).powi(2);
            if distance < best_distance {
                best = Some(parameters);
                best_distance = distance;
            }
        }
    }
    best
}

fn initial_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, point, seed),
        SurfaceGeometry::Procedural { construction } => {
            let procedural =
                ir.model.procedural_surfaces.iter().find(|candidate| {
                    &candidate.id == construction && &candidate.surface == surface
                })?;
            let ProceduralSurfaceDefinition::Offset { support, .. } = &procedural.definition else {
                return None;
            };
            initial_surface_parameters(ir, support, point, seed)
        }
        geometry => analytic_surface_parameters(geometry, point),
    }
}

fn surface_parameter_domain(ir: &CadIr, surface: &SurfaceId) -> Option<([f64; 2], [f64; 2])> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => {
            let u_degree = usize::try_from(nurbs.u_degree).ok()?;
            let v_degree = usize::try_from(nurbs.v_degree).ok()?;
            let u_count = usize::try_from(nurbs.u_count).ok()?;
            let v_count = usize::try_from(nurbs.v_count).ok()?;
            Some((
                [*nurbs.u_knots.get(u_degree)?, *nurbs.u_knots.get(u_count)?],
                [*nurbs.v_knots.get(v_degree)?, *nurbs.v_knots.get(v_count)?],
            ))
        }
        SurfaceGeometry::Procedural { construction } => {
            let procedural =
                ir.model.procedural_surfaces.iter().find(|candidate| {
                    &candidate.id == construction && &candidate.surface == surface
                })?;
            let ProceduralSurfaceDefinition::Offset { support, .. } = &procedural.definition else {
                return None;
            };
            surface_parameter_domain(ir, support)
        }
        _ => None,
    }
}

fn clamp_surface_parameters(parameters: &mut Point2, domain: Option<([f64; 2], [f64; 2])>) {
    if let Some((u_domain, v_domain)) = domain {
        parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
        parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
    }
}

fn parameter_derivative_step(parameter: f64, domain: Option<[f64; 2]>) -> f64 {
    domain.map_or_else(
        || 1.0e-6 * (1.0 + parameter.abs()),
        |domain| 1.0e-6 * (domain[1] - domain[0]).abs().max(1.0),
    )
}

fn model_surface_derivative(
    ir: &CadIr,
    surface: &SurfaceId,
    parameters: Point2,
    step: f64,
    along_u: bool,
    domain: Option<([f64; 2], [f64; 2])>,
    periods: [Option<f64>; 2],
) -> Option<Vector3> {
    let mut before = parameters;
    let mut after = parameters;
    if along_u {
        before.u -= step;
        after.u += step;
    } else {
        before.v -= step;
        after.v += step;
    }
    clamp_surface_parameters_with_periods(&mut before, domain, periods);
    clamp_surface_parameters_with_periods(&mut after, domain, periods);
    let width = if along_u {
        after.u - before.u
    } else {
        after.v - before.v
    };
    if !width.is_finite() || width == 0.0 {
        return None;
    }
    let first = model_surface_point(ir, surface, before.u, before.v)?;
    let second = model_surface_point(ir, surface, after.u, after.v)?;
    Some(Vector3::new(
        (second.x - first.x) / width,
        (second.y - first.y) / width,
        (second.z - first.z) / width,
    ))
}

/// Continue one chart-selected surface-intersection branch in both support
/// parameter spaces. The chart seeds and orders the branch; corrected points
/// satisfy the two support surfaces rather than interpolating chart samples.
#[cfg(test)]
pub(crate) fn continue_surface_intersection_parameters(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    chart: &[Point3],
    fit_tolerance: f64,
) -> Option<[Vec<Point2>; 2]> {
    continue_surface_intersection_parameters_with_seeds(
        ir,
        surfaces,
        chart,
        fit_tolerance,
        [None, None],
    )
}

fn continue_surface_intersection_parameters_with_seeds(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    chart: &[Point3],
    fit_tolerance: f64,
    seeds: [Option<Point2>; 2],
) -> Option<[Vec<Point2>; 2]> {
    if chart.len() < 2
        || surfaces[0] == surfaces[1]
        || !fit_tolerance.is_finite()
        || fit_tolerance <= 0.0
    {
        return None;
    }
    let fit_parameters = |surface: &SurfaceId, point: Point3, seed: Option<Point2>| {
        let geometry = &ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface)?
            .geometry;
        match geometry {
            SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, point, seed),
            SurfaceGeometry::Procedural { .. } => offset_surface_parameters_with_tolerance(
                ir,
                surface,
                point,
                seed,
                Some(fit_tolerance),
            )
            .or_else(|| blend_surface_parameters_for_fit(ir, surface, point, seed, fit_tolerance)),
            geometry => analytic_surface_parameters(geometry, point),
        }
    };
    let first = [
        fit_parameters(surfaces[0], chart[0], seeds[0])?,
        fit_parameters(surfaces[1], chart[0], seeds[1])?,
    ];
    let space = IntersectionParameterSpace {
        domains: surfaces.map(|surface| surface_parameter_domain(ir, surface)),
        periods: surfaces.map(|surface| surface_parameter_periods(ir, surface)),
    };
    let seed = [first[0].u, first[0].v, first[1].u, first[1].v];
    let first_chord = Vector3::new(
        chart[1].x - chart[0].x,
        chart[1].y - chart[0].y,
        chart[1].z - chart[0].z,
    );
    let seed_tangent = intersection_parameter_tangent(ir, surfaces, seed, space, first_chord)?;
    let mut current = correct_intersection_parameters(
        ir,
        surfaces,
        seed,
        seed_tangent,
        space,
        fit_tolerance,
        1.0,
    )?;
    let first_point = model_surface_point(ir, surfaces[0], current[0], current[1])?;
    if point_distance(first_point, chart[0]) > fit_tolerance {
        return None;
    }
    let mut lanes = [
        vec![Point2::new(current[0], current[1])],
        vec![Point2::new(current[2], current[3])],
    ];

    for chart_pair in chart.windows(2) {
        let jacobian = intersection_parameter_jacobian(ir, surfaces, current, space)?;
        let chord = Vector3::new(
            chart_pair[1].x - chart_pair[0].x,
            chart_pair[1].y - chart_pair[0].y,
            chart_pair[1].z - chart_pair[0].z,
        );
        let tangent = intersection_parameter_tangent(ir, surfaces, current, space, chord)?;
        let spatial_tangent = Vector3::new(
            jacobian[0][0] * tangent[0] + jacobian[0][1] * tangent[1],
            jacobian[1][0] * tangent[0] + jacobian[1][1] * tangent[1],
            jacobian[2][0] * tangent[0] + jacobian[2][1] * tangent[1],
        );
        let target = [
            fit_parameters(
                surfaces[0],
                chart_pair[1],
                Some(Point2::new(current[0], current[1])),
            )?,
            fit_parameters(
                surfaces[1],
                chart_pair[1],
                Some(Point2::new(current[2], current[3])),
            )?,
        ];
        let mut predictor = [target[0].u, target[0].v, target[1].u, target[1].v];
        for (side, surface_periods) in space.periods.into_iter().enumerate() {
            for (coordinate, period) in surface_periods.into_iter().enumerate() {
                let index = side * 2 + coordinate;
                if let Some(period) = period {
                    predictor[index] =
                        lift_periodic_parameter(predictor[index], current[index], period);
                }
            }
        }
        let scale = (0..4)
            .map(|index| (predictor[index] - current[index]) * tangent[index])
            .sum::<f64>();
        if !scale.is_finite() || scale == 0.0 || dot_vector(spatial_tangent, chord) * scale <= 0.0 {
            return None;
        }
        let corrected = correct_intersection_parameters(
            ir,
            surfaces,
            predictor,
            tangent,
            space,
            fit_tolerance,
            scale,
        )?;
        let point = model_surface_point(ir, surfaces[0], corrected[0], corrected[1])?;
        if point_distance(point, chart_pair[1]) > fit_tolerance {
            return None;
        }
        current = corrected;
        lanes[0].push(Point2::new(current[0], current[1]));
        lanes[1].push(Point2::new(current[2], current[3]));
    }
    Some(lanes)
}

fn lift_periodic_parameter(value: f64, reference: f64, period: f64) -> f64 {
    value + ((reference - value) / period).round() * period
}

/// Return supported parameter periods while rejecting cyclic procedural support graphs.
pub(crate) fn surface_parameter_periods(ir: &CadIr, surface: &SurfaceId) -> [Option<f64>; 2] {
    surface_parameter_periods_inner(ir, surface, &mut BTreeSet::new())
}

fn surface_parameter_periods_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    visiting: &mut BTreeSet<SurfaceId>,
) -> [Option<f64>; 2] {
    if !visiting.insert(surface.clone()) {
        return [None, None];
    }
    let Some(carrier) = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)
    else {
        visiting.remove(surface);
        return [None, None];
    };
    let periods = match &carrier.geometry {
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. } => [Some(std::f64::consts::TAU), None],
        SurfaceGeometry::Torus { .. } => [Some(std::f64::consts::TAU), Some(std::f64::consts::TAU)],
        SurfaceGeometry::Nurbs(nurbs) => {
            let period = |periodic: bool, knots: &[f64], degree: u32, count: u32| {
                periodic.then(|| {
                    let degree = usize::try_from(degree).ok()?;
                    let count = usize::try_from(count).ok()?;
                    let period = knots.get(count)? - knots.get(degree)?;
                    (period.is_finite() && period > 0.0).then_some(period)
                })?
            };
            [
                period(
                    nurbs.u_periodic,
                    &nurbs.u_knots,
                    nurbs.u_degree,
                    nurbs.u_count,
                ),
                period(
                    nurbs.v_periodic,
                    &nurbs.v_knots,
                    nurbs.v_degree,
                    nurbs.v_count,
                ),
            ]
        }
        SurfaceGeometry::Procedural { construction } => ir
            .model
            .procedural_surfaces
            .iter()
            .find(|candidate| &candidate.id == construction && &candidate.surface == surface)
            .and_then(|procedural| match &procedural.definition {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    Some(surface_parameter_periods_inner(ir, support, visiting))
                }
                _ => None,
            })
            .unwrap_or([None, None]),
        _ => [None, None],
    };
    visiting.remove(surface);
    periods
}

fn correct_intersection_parameters(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    predictor: [f64; 4],
    tangent: [f64; 4],
    space: IntersectionParameterSpace,
    fit_tolerance: f64,
    scale: f64,
) -> Option<[f64; 4]> {
    let mut corrected = predictor;
    clamp_intersection_parameters(&mut corrected, space);
    for _ in 0..32 {
        let first = model_surface_point(ir, surfaces[0], corrected[0], corrected[1])?;
        let second = model_surface_point(ir, surfaces[1], corrected[2], corrected[3])?;
        let residual = [
            first.x - second.x,
            first.y - second.y,
            first.z - second.z,
            (0..4)
                .map(|index| (corrected[index] - predictor[index]) * tangent[index])
                .sum(),
        ];
        let equality_error = residual[..3]
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if equality_error <= fit_tolerance * 1.0e-6
            && residual[3].abs() <= 1.0e-11 * (1.0 + scale.abs())
        {
            return Some(corrected);
        }
        let jacobian = intersection_parameter_jacobian(ir, surfaces, corrected, space)?;
        let matrix = [jacobian[0], jacobian[1], jacobian[2], tangent];
        let step = solve_4x4(matrix, residual.map(|value| -value))?;
        for index in 0..4 {
            corrected[index] += step[index];
        }
        clamp_intersection_parameters(&mut corrected, space);
    }
    None
}

#[derive(Clone, Copy)]
struct IntersectionParameterSpace {
    domains: [Option<([f64; 2], [f64; 2])>; 2],
    periods: [[Option<f64>; 2]; 2],
}

fn intersection_parameter_tangent(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    parameters: [f64; 4],
    space: IntersectionParameterSpace,
    chord: Vector3,
) -> Option<[f64; 4]> {
    let jacobian = intersection_parameter_jacobian(ir, surfaces, parameters, space)?;
    if let Some(tangent) = null_vector_3x4(jacobian) {
        return Some(tangent);
    }
    let chord = unit_vector(chord)?;
    let derivatives = [
        [
            Vector3::new(jacobian[0][0], jacobian[1][0], jacobian[2][0]),
            Vector3::new(jacobian[0][1], jacobian[1][1], jacobian[2][1]),
        ],
        [
            Vector3::new(-jacobian[0][2], -jacobian[1][2], -jacobian[2][2]),
            Vector3::new(-jacobian[0][3], -jacobian[1][3], -jacobian[2][3]),
        ],
    ];
    let mut tangent = [0.0; 4];
    for side in 0..2 {
        let (u, v) = least_squares_step(derivatives[side][0], derivatives[side][1], chord)?;
        let mapped = unit_vector(Vector3::new(
            derivatives[side][0].x * u + derivatives[side][1].x * v,
            derivatives[side][0].y * u + derivatives[side][1].y * v,
            derivatives[side][0].z * u + derivatives[side][1].z * v,
        ))?;
        if dot_vector(mapped, chord) < 1.0 - 1.0e-8 {
            return None;
        }
        tangent[side * 2] = u;
        tangent[side * 2 + 1] = v;
    }
    let norm = tangent
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    (norm.is_finite() && norm > 1.0e-14).then(|| tangent.map(|value| value / norm))
}

fn intersection_parameter_jacobian(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    parameters: [f64; 4],
    space: IntersectionParameterSpace,
) -> Option<[[f64; 4]; 3]> {
    let pairs = [
        Point2::new(parameters[0], parameters[1]),
        Point2::new(parameters[2], parameters[3]),
    ];
    let derivatives = std::array::from_fn(|side| {
        let u_step =
            parameter_derivative_step(pairs[side].u, space.domains[side].map(|value| value.0));
        let v_step =
            parameter_derivative_step(pairs[side].v, space.domains[side].map(|value| value.1));
        Some([
            model_surface_derivative(
                ir,
                surfaces[side],
                pairs[side],
                u_step,
                true,
                space.domains[side],
                space.periods[side],
            )?,
            model_surface_derivative(
                ir,
                surfaces[side],
                pairs[side],
                v_step,
                false,
                space.domains[side],
                space.periods[side],
            )?,
        ])
    });
    let [Some(first), Some(second)] = derivatives else {
        return None;
    };
    Some([
        [first[0].x, first[1].x, -second[0].x, -second[1].x],
        [first[0].y, first[1].y, -second[0].y, -second[1].y],
        [first[0].z, first[1].z, -second[0].z, -second[1].z],
    ])
}

fn clamp_intersection_parameters(parameters: &mut [f64; 4], space: IntersectionParameterSpace) {
    for side in 0..2 {
        let mut pair = Point2::new(parameters[side * 2], parameters[side * 2 + 1]);
        clamp_surface_parameters_with_periods(&mut pair, space.domains[side], space.periods[side]);
        parameters[side * 2] = pair.u;
        parameters[side * 2 + 1] = pair.v;
    }
}

fn clamp_surface_parameters_with_periods(
    parameters: &mut Point2,
    domain: Option<([f64; 2], [f64; 2])>,
    periods: [Option<f64>; 2],
) {
    if let Some((u_domain, v_domain)) = domain {
        if periods[0].is_none() {
            parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
        }
        if periods[1].is_none() {
            parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
        }
    }
}

fn determinant_3x3(matrix: [[f64; 3]; 3]) -> f64 {
    matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
        - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
        + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0])
}

fn null_vector_3x4(matrix: [[f64; 4]; 3]) -> Option<[f64; 4]> {
    let mut vector = [0.0; 4];
    for (omitted, component) in vector.iter_mut().enumerate() {
        let minor = std::array::from_fn(|row| {
            let mut column = 0;
            std::array::from_fn(|_| {
                while column == omitted {
                    column += 1;
                }
                let value = matrix[row][column];
                column += 1;
                value
            })
        });
        *component = if omitted % 2 == 0 { 1.0 } else { -1.0 } * determinant_3x3(minor);
    }
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    (norm.is_finite() && norm > 1.0e-14).then(|| vector.map(|value| value / norm))
}

fn solve_4x4(mut matrix: [[f64; 4]; 4], mut rhs: [f64; 4]) -> Option<[f64; 4]> {
    for pivot in 0..4 {
        let row = (pivot..4).max_by(|first, second| {
            matrix[*first][pivot]
                .abs()
                .total_cmp(&matrix[*second][pivot].abs())
        })?;
        if !matrix[row][pivot].is_finite() || matrix[row][pivot].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(pivot, row);
        rhs.swap(pivot, row);
        let pivot_row = matrix[pivot];
        for row in pivot + 1..4 {
            let factor = matrix[row][pivot] / matrix[pivot][pivot];
            for (value, pivot_value) in matrix[row][pivot..].iter_mut().zip(&pivot_row[pivot..]) {
                *value -= factor * pivot_value;
            }
            rhs[row] -= factor * rhs[pivot];
        }
    }
    let mut solution = [0.0; 4];
    for row in (0..4).rev() {
        let known = (row + 1..4)
            .map(|column| matrix[row][column] * solution[column])
            .sum::<f64>();
        solution[row] = (rhs[row] - known) / matrix[row][row];
    }
    solution
        .iter()
        .all(|value| value.is_finite())
        .then_some(solution)
}

fn least_squares_step(du: Vector3, dv: Vector3, residual: Vector3) -> Option<(f64, f64)> {
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let du_squared = dot(du, du);
    let mixed = dot(du, dv);
    let dv_squared = dot(dv, dv);
    let determinant = du_squared * dv_squared - mixed * mixed;
    if !determinant.is_finite()
        || determinant.abs() <= f64::EPSILON * du_squared.max(dv_squared).powi(2)
    {
        return None;
    }
    let du_residual = dot(du, residual);
    let dv_residual = dot(dv, residual);
    Some((
        (dv_squared * du_residual - mixed * dv_residual) / determinant,
        (du_squared * dv_residual - mixed * du_residual) / determinant,
    ))
}

pub(crate) fn nurbs_parameters(
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    let seed = seed.filter(|seed| seed.u.is_finite() && seed.v.is_finite());
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let u_domain = [
        *surface.u_knots.get(u_degree)?,
        *surface.u_knots.get(u_count)?,
    ];
    let v_domain = [
        *surface.v_knots.get(v_degree)?,
        *surface.v_knots.get(v_count)?,
    ];
    if u_domain[0] >= u_domain[1] || v_domain[0] >= v_domain[1] {
        return None;
    }
    let squared_distance = |candidate: Point3| point_distance(candidate, point).powi(2);
    let mut coarse = vec![None; 81];
    for ui in 0..=8 {
        for vi in 0..=8 {
            let ui_value = f64::from(u32::try_from(ui).ok()?);
            let vi_value = f64::from(u32::try_from(vi).ok()?);
            let parameters = Point2::new(
                u_domain[0] + (u_domain[1] - u_domain[0]) * ui_value / 8.0,
                v_domain[0] + (v_domain[1] - v_domain[0]) * vi_value / 8.0,
            );
            let Some(position) =
                cadmpeg_ir::eval::nurbs_surface_point(surface, parameters.u, parameters.v)
            else {
                continue;
            };
            coarse[ui * 9 + vi] = Some((parameters, squared_distance(position)));
        }
    }
    let mut starts = Vec::new();
    if let Some(seed) = seed {
        starts.push(seed);
    }
    for ui in 0..=8 {
        for vi in 0..=8 {
            let index = ui * 9 + vi;
            let Some((parameters, distance)) = coarse[index] else {
                continue;
            };
            let local_minimum = ui.saturating_sub(1)..=(ui + 1).min(8);
            if local_minimum
                .flat_map(|neighbor_u| {
                    (vi.saturating_sub(1)..=(vi + 1).min(8))
                        .map(move |neighbor_v| neighbor_u * 9 + neighbor_v)
                })
                .all(|neighbor| coarse[neighbor].is_none_or(|(_, value)| distance <= value))
            {
                starts.push(parameters);
            }
        }
    }
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    let mut best_seed_distance = f64::INFINITY;
    for start in starts {
        let Some(parameters) = refine_nurbs_surface_parameters(
            surface,
            point,
            start,
            u_domain,
            v_domain,
            &squared_distance,
        ) else {
            continue;
        };
        let Some(position) =
            cadmpeg_ir::eval::nurbs_surface_point(surface, parameters.u, parameters.v)
        else {
            continue;
        };
        let distance = squared_distance(position);
        let seed_distance = seed.map_or(parameters.u.abs() + parameters.v.abs(), |seed| {
            (parameters.u - seed.u).hypot(parameters.v - seed.v)
        });
        let same_point = (distance - best_distance).abs()
            <= f64::EPSILON * 64.0 * distance.abs().max(best_distance.abs()).max(1.0);
        if distance < best_distance && !same_point
            || same_point && seed_distance < best_seed_distance
        {
            best = Some(parameters);
            best_distance = distance;
            best_seed_distance = seed_distance;
        }
    }
    best
}

fn refine_nurbs_surface_parameters(
    surface: &NurbsSurface,
    point: Point3,
    mut parameters: Point2,
    u_domain: [f64; 2],
    v_domain: [f64; 2],
    squared_distance: &impl Fn(Point3) -> f64,
) -> Option<Point2> {
    parameters.u = parameters.u.clamp(u_domain[0], u_domain[1]);
    parameters.v = parameters.v.clamp(v_domain[0], v_domain[1]);
    for _ in 0..32 {
        let position = cadmpeg_ir::eval::nurbs_surface_point(surface, parameters.u, parameters.v)?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let (du, dv) = nurbs_surface_partials(surface, parameters.u, parameters.v)?;
        let dot =
            |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
        let du_squared = dot(du, du);
        let mixed = dot(du, dv);
        let dv_squared = dot(dv, dv);
        let determinant = du_squared * dv_squared - mixed * mixed;
        if !determinant.is_finite()
            || determinant.abs() <= f64::EPSILON * du_squared.max(dv_squared).powi(2)
        {
            break;
        }
        let du_residual = dot(du, residual);
        let dv_residual = dot(dv, residual);
        let step = Point2::new(
            (dv_squared * du_residual - mixed * dv_residual) / determinant,
            (du_squared * dv_residual - mixed * du_residual) / determinant,
        );
        let current_distance = squared_distance(position);
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..16 {
            let candidate = Point2::new(
                (parameters.u - scale * step.u).clamp(u_domain[0], u_domain[1]),
                (parameters.v - scale * step.v).clamp(v_domain[0], v_domain[1]),
            );
            let candidate_position =
                cadmpeg_ir::eval::nurbs_surface_point(surface, candidate.u, candidate.v)?;
            if squared_distance(candidate_position) <= current_distance {
                accepted = Some(candidate);
                break;
            }
            scale *= 0.5;
        }
        let Some(candidate) = accepted else {
            break;
        };
        parameters = candidate;
        if scale * step.u.abs() <= 1.0e-12 * (1.0 + parameters.u.abs())
            && scale * step.v.abs() <= 1.0e-12 * (1.0 + parameters.v.abs())
        {
            break;
        }
    }
    Some(parameters)
}

fn point_distance(first: Point3, second: Point3) -> f64 {
    ((first.x - second.x).powi(2) + (first.y - second.y).powi(2) + (first.z - second.z).powi(2))
        .sqrt()
}

fn intersection_side(
    ir: &CadIr,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    surface_xmt: u32,
    uv: Option<(&[[f64; 2]], &[f64])>,
) -> IntcurveSupportSide {
    let surface = surfaces_by_xmt.get(&surface_xmt).cloned();
    let pcurve = surface.as_ref().and_then(|surface_id| {
        let geometry = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| &candidate.id == surface_id)
            .map(|surface| &surface.geometry)?;
        let (uv, parameters) = uv?;
        Some(PcurveGeometry::Nurbs {
            degree: 1,
            knots: linear_knots(parameters),
            control_points: uv
                .iter()
                .map(|pair| surface_parameters(geometry, *pair))
                .collect(),
            weights: None,
            periodic: false,
        })
    });
    IntcurveSupportSide { surface, pcurve }
}

fn surface_parameters(surface: &SurfaceGeometry, uv: [f64; 2]) -> Point2 {
    match surface {
        SurfaceGeometry::Plane { .. } => Point2::new(uv[0] * 1000.0, uv[1] * 1000.0),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
            Point2::new(uv[0], uv[1] * 1000.0)
        }
        SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Torus { .. }
        | SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => Point2::new(uv[0], uv[1]),
    }
}

fn normalize_pcurve_parameters(pcurve: &mut PcurveGeometry, surface: &SurfaceGeometry) {
    match pcurve {
        PcurveGeometry::Line { origin, direction } => {
            let end = Point2::new(origin.u + direction.u, origin.v + direction.v);
            let converted_origin = surface_parameters(surface, [origin.u, origin.v]);
            let converted_end = surface_parameters(surface, [end.u, end.v]);
            *origin = converted_origin;
            *direction = Point2::new(
                converted_end.u - converted_origin.u,
                converted_end.v - converted_origin.v,
            );
        }
        PcurveGeometry::Nurbs { control_points, .. } => {
            for point in control_points {
                *point = surface_parameters(surface, [point.u, point.v]);
            }
        }
    }
}

// The parameters are the per-stream lookup tables produced by the decode pass;
// bundling them into a struct would only rename the same lookup tables.
#[allow(clippy::too_many_arguments)]
fn emit_topology(
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    points: &BTreeMap<u32, PointId>,
    surfaces: &BTreeMap<u32, SurfaceId>,
    curves: &BTreeMap<u32, CurveId>,
    pcurves: &BTreeMap<u32, PcurveId>,
    pcurve_supports: &BTreeMap<u32, SurfaceId>,
    trim_ranges: &BTreeMap<u32, [f64; 2]>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let prefix = format!("nx:s{stream_index}");
    let body_shape_shells = graph.body_shape_shells();
    let valid_face_xmts: BTreeSet<u32> = body_shape_shells
        .iter()
        .filter_map(|shell| graph.shell_face_xmts(shell))
        .flatten()
        .collect();
    let valid_loop_rings: BTreeMap<u32, Vec<u32>> = valid_face_xmts
        .iter()
        .filter_map(|face_xmt| graph.face_loop_rings(*face_xmt))
        .flatten()
        .collect();
    let valid_fin_xmts: BTreeSet<u32> = valid_loop_rings
        .values()
        .flat_map(|ring| ring.iter().copied())
        .collect();
    let valid_edge_xmts: BTreeSet<u32> = valid_fin_xmts
        .iter()
        .filter_map(|xmt| graph.get(17, *xmt)?.fin_fields().map(|fields| fields.edge))
        .collect();
    let valid_vertex_xmts: BTreeSet<u32> = valid_fin_xmts
        .iter()
        .flat_map(|xmt| {
            let fields = graph.get(17, *xmt).and_then(Node::fin_fields);
            let partner_vertex = fields
                .filter(|fields| fields.other > 1)
                .and_then(|fields| graph.get(17, fields.other))
                .and_then(Node::fin_fields)
                .map(|fields| fields.vertex);
            [fields.map(|fields| fields.vertex), partner_vertex]
                .into_iter()
                .flatten()
        })
        .filter(|xmt| *xmt > 1)
        .collect();
    let body_xmts: BTreeSet<_> = body_shape_shells
        .iter()
        .filter_map(|shell| shell.shell_fields().map(|fields| fields.body))
        .collect();
    let mut bodies = BTreeMap::new();
    for body_xmt in body_xmts {
        let id = BodyId(format!("{prefix}:body#{body_xmt}"));
        if let Some(node) = graph.get(12, body_xmt) {
            annotate_node(annotations, &id, source_stream, node, "BODY");
        } else if let Some(shell) = body_shape_shells.iter().find(|shell| {
            shell
                .shell_fields()
                .is_some_and(|fields| fields.body == body_xmt)
        }) {
            annotations
                .note(&id, source_stream, shell.pos as u64)
                .tag("UNRESOLVED_BODY_REFERENCE");
            annotations.exactness(&id, Exactness::Unknown);
        }
        bodies.insert(body_xmt, id.clone());
        ir.model.bodies.push(Body {
            id,
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }

    let mut regions: BTreeMap<u32, (RegionId, BodyId)> = BTreeMap::new();
    let mut shells = BTreeMap::new();
    for node in body_shape_shells {
        let Some(fields) = node.shell_fields() else {
            continue;
        };
        let Some(body) = bodies.get(&fields.body).cloned() else {
            continue;
        };
        let region_id = if let Some((region, owner)) = regions.get(&fields.region) {
            if owner != &body {
                continue;
            }
            region.clone()
        } else {
            let region = RegionId(format!("{prefix}:region#{}", fields.region));
            if let Some(region_node) = graph.get(19, fields.region) {
                annotate_node(annotations, &region, source_stream, region_node, "REGION");
            } else {
                annotations
                    .note(&region, source_stream, node.pos as u64)
                    .tag("UNRESOLVED_REGION_REFERENCE");
                annotations.exactness(&region, Exactness::Unknown);
            }
            annotations.derived(&region, "body");
            ir.model.regions.push(Region {
                id: region.clone(),
                body: body.clone(),
                shells: Vec::new(),
            });
            if let Some(parent) = ir
                .model
                .bodies
                .iter_mut()
                .find(|candidate| candidate.id == body)
            {
                parent.regions.push(region.clone());
            }
            regions.insert(fields.region, (region.clone(), body.clone()));
            region
        };
        let shell_id = ShellId(format!("{prefix}:shell#{}", node.xmt));
        annotate_node(annotations, &shell_id, source_stream, node, "SHELL");
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        if let Some(parent) = ir
            .model
            .regions
            .iter_mut()
            .find(|candidate| candidate.id == region_id)
        {
            parent.shells.push(shell_id.clone());
        }
        shells.insert(node.xmt, shell_id);
    }

    let mut vertices = BTreeMap::new();
    for node in graph
        .of_kind(18)
        .filter(|node| valid_vertex_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.vertex_fields() else {
            continue;
        };
        let Some(point) = points.get(&fields.point).cloned() else {
            continue;
        };
        let tolerance = decoded_tolerance(fields.tolerance);
        let vertex = VertexId(format!("{prefix}:vertex#{}", node.xmt));
        annotate_node(annotations, &vertex, source_stream, node, "VERTEX");
        if tolerance.is_some() {
            annotations.derived(&vertex, "tolerance");
        }
        ir.model.vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance,
        });
        vertices.insert(node.xmt, vertex.clone());
    }

    let mut edges = BTreeMap::new();
    for node in graph
        .of_kind(16)
        .filter(|node| valid_edge_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.edge_fields() else {
            continue;
        };
        let Some(fin) = graph.get(17, fields.fin) else {
            continue;
        };
        let Some(fin_fields) = fin.fin_fields() else {
            continue;
        };
        let curve_xmt = [fields.curve, fin_fields.curve_xmt]
            .into_iter()
            .find(|xmt| *xmt > 1);
        let mut curve = curve_xmt.and_then(|xmt| curves.get(&xmt)).cloned();
        let mut param_range = curve_xmt.and_then(|xmt| trim_ranges.get(&xmt)).copied();
        if curve.is_none() {
            let lifted = curve_xmt
                .and_then(|xmt| pcurves.get(&xmt))
                .and_then(|pcurve_id| {
                    let pcurve = ir
                        .model
                        .pcurves
                        .iter()
                        .find(|pcurve| &pcurve.id == pcurve_id)?;
                    let surface = pcurve_supports.get(&curve_xmt?)?.clone();
                    let parameter_range = pcurve
                        .parameter_range
                        .or(param_range)
                        .or_else(|| pcurve_parameter_range(&pcurve.geometry))?;
                    let parameter_range = ordered_parameter_range(parameter_range)?;
                    Some((
                        surface,
                        pcurve.geometry.clone(),
                        parameter_range,
                        pcurve.fit_tolerance,
                    ))
                });
            if let Some((surface, pcurve, parameter_range, fit_tolerance)) = lifted {
                let carrier = CurveId(format!("{prefix}:edge-parametric-curve#{}", node.xmt));
                let construction = ProceduralCurveId(format!(
                    "{prefix}:edge-parametric-construction#{}",
                    node.xmt
                ));
                annotations
                    .note(&carrier, source_stream, node.pos as u64)
                    .tag("PARAMETRIC_SURFACE_CURVE");
                annotations.derived(&carrier, "geometry");
                ir.model.curves.push(Curve {
                    id: carrier.clone(),
                    geometry: CurveGeometry::Procedural {
                        construction: construction.clone(),
                    },
                    source_object: None,
                });
                ir.model.procedural_curves.push(ProceduralCurve {
                    id: construction,
                    curve: carrier.clone(),
                    definition: ProceduralCurveDefinition::SurfaceCurve {
                        family: SurfaceCurveFamily::Parametric,
                        context: IntcurveSupportContext {
                            sides: [
                                IntcurveSupportSide {
                                    surface: Some(surface),
                                    pcurve: Some(pcurve),
                                },
                                IntcurveSupportSide {
                                    surface: None,
                                    pcurve: None,
                                },
                            ],
                            parameter_range,
                            discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                        },
                    },
                    cache_fit_tolerance: fit_tolerance,
                });
                curve = Some(carrier);
                param_range = None;
            }
        }
        let start = vertices.get(&fin_fields.vertex).cloned().or_else(|| {
            (fin_fields.vertex == 1
                && fin_fields.forward == fin.xmt
                && fin_fields.backward == fin.xmt)
                .then(|| {
                    synthesize_closed_edge_vertex(
                        ir,
                        annotations,
                        &prefix,
                        node,
                        curve.as_ref()?,
                        param_range,
                        source_stream,
                        decoded_tolerance(fields.tolerance),
                    )
                })
                .flatten()
        });
        let Some(start) = start else {
            continue;
        };
        let end_fin = if fin_fields.other > 1 {
            fin_fields.other
        } else {
            fin_fields.forward
        };
        let end = graph
            .get(17, end_fin)
            .and_then(Node::fin_fields)
            .and_then(|next| vertices.get(&next.vertex))
            .cloned()
            .unwrap_or_else(|| start.clone());
        let (mut start, mut end) = (start, end);
        let id = EdgeId(format!("{prefix}:edge#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "EDGE");
        if decoded_tolerance(fields.tolerance).is_some() {
            annotations.derived(&id, "tolerance");
        }
        if let (Some(carrier), Some(range)) = (&curve, param_range) {
            match orient_edge_range(
                ir,
                carrier,
                range,
                &start,
                &end,
                decoded_tolerance(fields.tolerance),
            ) {
                Some((oriented, reverse_edge)) => {
                    param_range = Some(oriented);
                    if reverse_edge {
                        std::mem::swap(&mut start, &mut end);
                    }
                }
                None => {
                    param_range = None;
                }
            }
        }
        ir.model.edges.push(Edge {
            id: id.clone(),
            curve,
            start,
            end,
            param_range,
            tolerance: decoded_tolerance(fields.tolerance),
        });
        edges.insert(node.xmt, id);
    }

    let mut faces = BTreeMap::new();
    for node in graph
        .of_kind(14)
        .filter(|node| valid_face_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.face_fields() else {
            continue;
        };
        let Some(shell) = shells.get(&fields.shell).cloned() else {
            continue;
        };
        let Some(surface) = surfaces.get(&fields.surface).cloned() else {
            continue;
        };
        let id = FaceId(format!("{prefix}:face#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "FACE");
        if decoded_tolerance(fields.tolerance).is_some() {
            annotations.derived(&id, "tolerance");
        }
        ir.model.faces.push(Face {
            id: id.clone(),
            shell: shell.clone(),
            surface,
            sense: sense(Some(fields.sense)),
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: decoded_tolerance(fields.tolerance),
        });
        if let Some(parent) = ir
            .model
            .shells
            .iter_mut()
            .find(|candidate| candidate.id == shell)
        {
            parent.faces.push(id.clone());
        }
        faces.insert(node.xmt, id);
    }

    let mut loops = BTreeMap::new();
    for &loop_xmt in valid_loop_rings.keys() {
        let ring_resolves = valid_loop_rings[&loop_xmt].iter().all(|fin_xmt| {
            graph
                .get(17, *fin_xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| edges.contains_key(&fields.edge))
        });
        if !ring_resolves {
            continue;
        }
        let Some(node) = graph.get(15, loop_xmt) else {
            continue;
        };
        let Some(fields) = node.loop_fields() else {
            continue;
        };
        let Some(face) = faces.get(&fields.face).cloned() else {
            continue;
        };
        let id = LoopId(format!("{prefix}:loop#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "LOOP");
        ir.model.loops.push(Loop {
            id: id.clone(),
            face: face.clone(),
            coedges: Vec::new(),
        });
        if let Some(parent) = ir
            .model
            .faces
            .iter_mut()
            .find(|candidate| candidate.id == face)
        {
            parent.loops.push(id.clone());
        }
        loops.insert(node.xmt, id);
    }

    let fin_ids: BTreeMap<u32, CoedgeId> = valid_fin_xmts
        .iter()
        .filter(|xmt| {
            graph
                .get(17, **xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| loops.contains_key(&fields.loop_xmt))
        })
        .map(|xmt| (*xmt, CoedgeId(format!("{prefix}:fin#{xmt}"))))
        .collect();
    let intersection_pcurves: BTreeMap<_, _> = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            Some(context.sides.iter().filter_map(move |side| {
                Some((
                    (procedural.curve.clone(), side.surface.clone()?),
                    (
                        side.pcurve.clone()?,
                        context.parameter_range,
                        procedural.cache_fit_tolerance,
                    ),
                ))
            }))
        })
        .flatten()
        .collect();
    for &fin_xmt in fin_ids.keys() {
        let Some(node) = graph.get(17, fin_xmt) else {
            continue;
        };
        let Some(fields) = node.fin_fields() else {
            continue;
        };
        let Some(loop_id) = loops.get(&fields.loop_xmt).cloned() else {
            continue;
        };
        let Some(edge) = edges.get(&fields.edge).cloned() else {
            continue;
        };
        let id = fin_ids.get(&node.xmt).cloned().expect("filtered above");
        annotate_node(annotations, &id, source_stream, node, "FIN");
        let next = fin_ids
            .get(&fields.forward)
            .cloned()
            .expect("validated FIN ring resolves forward link");
        let previous = fin_ids
            .get(&fields.backward)
            .cloned()
            .expect("validated FIN ring resolves backward link");
        let partner = fin_ids.get(&fields.other).cloned();
        let radial_next = partner.clone().unwrap_or_else(|| id.clone());
        let support = graph
            .get(15, fields.loop_xmt)
            .and_then(Node::loop_fields)
            .and_then(|loop_| graph.get(14, loop_.face))
            .and_then(Node::face_fields)
            .and_then(|face| surfaces.get(&face.surface))
            .cloned();
        let mut pcurve = pcurves.get(&fields.curve_xmt).cloned().filter(|id| {
            let Some((carrier, support)) = ir
                .model
                .pcurves
                .iter()
                .find(|carrier| &carrier.id == id)
                .zip(support.as_ref())
            else {
                return false;
            };
            pcurve_matches_edge_range(
                ir,
                &edge,
                support,
                &carrier.geometry,
                carrier.parameter_range,
                carrier.fit_tolerance,
            )
        });
        if pcurve.is_none() {
            let carrier = ir
                .model
                .edges
                .iter()
                .find(|candidate| candidate.id == edge)
                .and_then(|edge| edge.curve.clone());
            if let Some((_support, geometry, parameter_range, fit_tolerance)) = carrier
                .zip(support)
                .and_then(|key| {
                    intersection_pcurves
                        .get(&key)
                        .cloned()
                        .map(|value| (key.1, value.0, value.1, value.2))
                })
                .filter(|(support, geometry, _, fit_tolerance)| {
                    pcurve_matches_edge(ir, &edge, support, geometry, *fit_tolerance)
                })
            {
                let pcurve_id = PcurveId(format!("{prefix}:intersection-pcurve#{fin_xmt}"));
                annotations
                    .note(&pcurve_id, source_stream, node.pos as u64)
                    .tag("INTERSECTION_PCURVE");
                annotations.derived(&pcurve_id, "geometry");
                annotations.derived(&pcurve_id, "parameter_range");
                if fit_tolerance.is_some() {
                    annotations.derived(&pcurve_id, "fit_tolerance");
                }
                ir.model.pcurves.push(Pcurve {
                    id: pcurve_id.clone(),
                    geometry,
                    wrapper_reversed: None,
                    native_tail_flags: None,
                    parameter_range: Some(parameter_range),
                    fit_tolerance,
                });
                pcurve = Some(pcurve_id);
            }
        }
        ir.model.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: loop_id.clone(),
            edge,
            next,
            previous,
            radial_next,
            sense: sense(Some(fields.sense)),
            pcurve,
        });
        if let Some(parent) = ir
            .model
            .loops
            .iter_mut()
            .find(|candidate| candidate.id == loop_id)
        {
            parent.coedges.push(id);
        }
    }

    attach_tolerant_edge_intersections(ir, graph, &edges, &prefix, source_stream, annotations);
    complete_intersection_supports_from_edge_incidence(ir);
    complete_intersection_pcurves_from_coedge_incidence(ir);
    complete_isoparametric_intersection_pcurves(ir);
    complete_intersection_pcurves_from_opposite_charts(ir);

    let owned_edges: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect();
    let candidate_edges: BTreeSet<_> = edges.into_values().collect();
    ir.model
        .edges
        .retain(|edge| !candidate_edges.contains(&edge.id) || owned_edges.contains(&edge.id));
    let retained_vertices: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .collect();
    ir.model.vertices.retain(|vertex| {
        !vertex.id.0.starts_with(&prefix) || retained_vertices.contains(&vertex.id)
    });
}

fn pcurve_parameter_range(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    let PcurveGeometry::Nurbs { knots, .. } = geometry else {
        return None;
    };
    ordered_parameter_range([*knots.first()?, *knots.last()?])
}

fn ordered_parameter_range(mut range: [f64; 2]) -> Option<[f64; 2]> {
    if !range.iter().all(|value| value.is_finite()) || range[0] == range[1] {
        return None;
    }
    if range[0] > range[1] {
        range.swap(0, 1);
    }
    Some(range)
}

pub(crate) fn complete_intersection_supports_from_edge_incidence(ir: &mut CadIr) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.clone(), loop_.face.clone()))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.surface.clone()))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((edge.id.clone(), edge.curve.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let mut incident_surfaces = BTreeMap::<CurveId, Vec<SurfaceId>>::new();
    for coedge in &ir.model.coedges {
        let Some(curve) = edge_curves.get(&coedge.edge) else {
            continue;
        };
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face| face_surfaces.get(face))
        else {
            continue;
        };
        let surfaces = incident_surfaces.entry(curve.clone()).or_default();
        if !surfaces.contains(surface) {
            surfaces.push(surface.clone());
        }
    }

    for procedural in &mut ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        let missing = context
            .sides
            .iter()
            .enumerate()
            .filter_map(|(index, side)| side.surface.is_none().then_some(index))
            .collect::<Vec<_>>();
        if missing.len() != 1 {
            continue;
        }
        let Some(incident) = incident_surfaces.get(&procedural.curve) else {
            continue;
        };
        let candidates = incident
            .iter()
            .filter(|surface| {
                !context
                    .sides
                    .iter()
                    .any(|side| side.surface.as_ref() == Some(surface))
            })
            .collect::<Vec<_>>();
        let [surface] = candidates.as_slice() else {
            continue;
        };
        context.sides[missing[0]].surface = Some((*surface).clone());
    }
}

pub(crate) fn complete_intersection_pcurves_from_coedge_incidence(ir: &mut CadIr) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.clone(), loop_.face.clone()))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.surface.clone()))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((edge.id.clone(), edge.curve.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let mut incident_pcurves = BTreeMap::<(CurveId, SurfaceId), Vec<PcurveId>>::new();
    for coedge in &ir.model.coedges {
        let Some(curve) = edge_curves.get(&coedge.edge) else {
            continue;
        };
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face| face_surfaces.get(face))
        else {
            continue;
        };
        let Some(pcurve) = &coedge.pcurve else {
            continue;
        };
        let pcurves = incident_pcurves
            .entry((curve.clone(), surface.clone()))
            .or_default();
        if !pcurves.contains(pcurve) {
            pcurves.push(pcurve.clone());
        }
    }

    for procedural in &mut ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        for side in &mut context.sides {
            if side.pcurve.is_some() {
                continue;
            }
            let Some(surface) = &side.surface else {
                continue;
            };
            let Some([pcurve]) = incident_pcurves
                .get(&(procedural.curve.clone(), surface.clone()))
                .map(Vec::as_slice)
            else {
                continue;
            };
            let Some(carrier) = ir
                .model
                .pcurves
                .iter()
                .find(|carrier| &carrier.id == pcurve)
            else {
                continue;
            };
            side.pcurve = Some(carrier.geometry.clone());
        }
    }
}

pub(crate) fn complete_intersection_pcurves_from_opposite_charts(ir: &mut CadIr) {
    let edge_tolerances = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| {
            Some((
                edge.curve.clone()?,
                edge.tolerance
                    .filter(|value| value.is_finite() && *value >= 0.0)?,
            ))
        })
        .fold(
            BTreeMap::<CurveId, f64>::new(),
            |mut values, (curve, tolerance)| {
                values
                    .entry(curve)
                    .and_modify(|current| *current = current.min(tolerance))
                    .or_insert(tolerance);
                values
            },
        );
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            let missing = context
                .sides
                .each_ref()
                .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
            let target = match missing {
                [true, false] => 0,
                [false, true] => 1,
                _ => return None,
            };
            let source = 1 - target;
            let source_surface = context.sides[source].surface.as_ref()?;
            let source_pcurve = context.sides[source].pcurve.as_ref()?;
            let target_surface = context.sides[target].surface.as_ref()?;
            let tolerance = procedural
                .cache_fit_tolerance
                .or_else(|| edge_tolerances.get(&procedural.curve).copied())?;
            let tolerance = blend_spine_cache_fit_tolerance(ir, target_surface, tolerance);
            let pcurve = transfer_intersection_pcurve(
                ir,
                &procedural.curve,
                source_surface,
                source_pcurve,
                target_surface,
                context.parameter_range,
                tolerance,
            )?;
            Some((procedural.id.clone(), target, pcurve, tolerance))
        })
        .collect::<Vec<_>>();
    for (procedural_id, side, pcurve, tolerance) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
            procedural.cache_fit_tolerance =
                Some(procedural.cache_fit_tolerance.unwrap_or(0.0).max(tolerance));
        }
    }
}

pub(crate) fn complete_isoparametric_intersection_pcurves(ir: &mut CadIr) {
    let vertex_points = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| {
            let point = ir
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)?;
            Some((vertex.id.clone(), point.position))
        })
        .collect::<BTreeMap<_, _>>();
    let replacements = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            if !context
                .sides
                .iter()
                .all(|side| pcurve_requires_completion(side.pcurve.as_ref()))
            {
                return None;
            }
            let [Some(first_surface), Some(second_surface)] =
                context.sides.each_ref().map(|side| side.surface.as_ref())
            else {
                return None;
            };
            let edges = ir
                .model
                .edges
                .iter()
                .filter(|edge| edge.curve.as_ref() == Some(&procedural.curve))
                .collect::<Vec<_>>();
            let [edge] = edges.as_slice() else {
                return None;
            };
            let tolerance = edge
                .tolerance
                .filter(|value| value.is_finite() && *value >= 0.0)?;
            let endpoints = [
                *vertex_points.get(&edge.start)?,
                *vertex_points.get(&edge.end)?,
            ];
            let candidates = [first_surface, second_surface].map(|surface| {
                isoparametric_boundary_pcurve(
                    ir,
                    surface,
                    endpoints,
                    context.parameter_range,
                    tolerance,
                )
            });
            let pcurves = match candidates {
                [Some(first), Some(second)] => coincident_pcurve_pair(
                    ir,
                    [first_surface, second_surface],
                    [&first, &second],
                    context.parameter_range,
                    tolerance,
                )
                .then_some([first, second])?,
                [Some(first), None] => [
                    first.clone(),
                    transfer_intersection_pcurve(
                        ir,
                        &procedural.curve,
                        first_surface,
                        &first,
                        second_surface,
                        context.parameter_range,
                        tolerance,
                    )?,
                ],
                [None, Some(second)] => [
                    transfer_intersection_pcurve(
                        ir,
                        &procedural.curve,
                        second_surface,
                        &second,
                        first_surface,
                        context.parameter_range,
                        tolerance,
                    )?,
                    second,
                ],
                [None, None] => return None,
            };
            Some((procedural.id.clone(), pcurves, tolerance))
        })
        .collect::<Vec<_>>();
    for (procedural_id, pcurves, tolerance) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if context
            .sides
            .iter()
            .all(|side| pcurve_requires_completion(side.pcurve.as_ref()))
        {
            for (side, pcurve) in context.sides.iter_mut().zip(pcurves) {
                side.pcurve = Some(pcurve);
            }
            procedural.cache_fit_tolerance = Some(tolerance);
        }
    }
}

fn isoparametric_boundary_pcurve(
    ir: &CadIr,
    surface: &SurfaceId,
    endpoints: [Point3; 2],
    range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    (range[0].is_finite() && range[1].is_finite() && range[0] < range[1]).then_some(())?;
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let SurfaceGeometry::Nurbs(nurbs) = &carrier.geometry else {
        return None;
    };
    let domain = surface_parameter_domain(ir, surface)?;
    let parameters = [
        nurbs_parameters(nurbs, endpoints[0], None)?,
        nurbs_parameters(nurbs, endpoints[1], None)?,
    ];
    for index in 0..2 {
        let point =
            cadmpeg_ir::eval::nurbs_surface_point(nurbs, parameters[index].u, parameters[index].v)?;
        if point_distance(point, endpoints[index]) > tolerance {
            return None;
        }
    }
    let axes = [
        ([parameters[0].u, parameters[1].u], domain.0),
        ([parameters[0].v, parameters[1].v], domain.1),
    ];
    let candidates = axes
        .into_iter()
        .enumerate()
        .filter_map(|(constant_axis, (values, axis_domain))| {
            let scale = (axis_domain[1] - axis_domain[0]).abs().max(1.0);
            let parameter_tolerance = 1.0e-8 * scale;
            let boundary = axis_domain.into_iter().find(|boundary| {
                values
                    .iter()
                    .all(|value| (*value - *boundary).abs() <= parameter_tolerance)
            })?;
            let varying = if constant_axis == 0 {
                [parameters[0].v, parameters[1].v]
            } else {
                [parameters[0].u, parameters[1].u]
            };
            ((varying[1] - varying[0]).abs() > parameter_tolerance).then(|| {
                let delta = (varying[1] - varying[0]) / (range[1] - range[0]);
                let (origin, direction) = if constant_axis == 0 {
                    (
                        Point2::new(boundary, varying[0] - delta * range[0]),
                        Point2::new(0.0, delta),
                    )
                } else {
                    (
                        Point2::new(varying[0] - delta * range[0], boundary),
                        Point2::new(delta, 0.0),
                    )
                };
                PcurveGeometry::Line { origin, direction }
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn coincident_pcurve_pair(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    pcurves: [&PcurveGeometry; 2],
    range: [f64; 2],
    tolerance: f64,
) -> bool {
    (0..=32).all(|index| {
        let fraction = f64::from(index) / 32.0;
        let parameter = range[0] + fraction * (range[1] - range[0]);
        let points = [0usize, 1usize].map(|side| {
            let uv = pcurve_uv(pcurves[side], parameter)?;
            decoded_surface_point(ir, surfaces[side], uv.u, uv.v)
        });
        matches!(points, [Some(first), Some(second)] if point_distance(first, second) <= tolerance)
    })
}

fn transfer_intersection_pcurve(
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    parameter_range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    (parameter_range[0].is_finite()
        && parameter_range[1].is_finite()
        && parameter_range[0] < parameter_range[1]
        && tolerance.is_finite()
        && tolerance >= 0.0)
        .then_some(())?;
    let first = transferred_pcurve_sample(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        parameter_range[0],
        None,
        tolerance,
    )?;
    let last = transferred_pcurve_sample(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        parameter_range[1],
        Some(first.1),
        tolerance,
    )?;
    let mut samples = vec![first];
    append_transferred_pcurve_segment(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        first,
        last,
        tolerance,
        0,
        &mut samples,
    )?;
    Some(PcurveGeometry::Nurbs {
        degree: 1,
        knots: linear_knots(&samples.iter().map(|sample| sample.0).collect::<Vec<_>>()),
        control_points: samples.iter().map(|sample| sample.1).collect(),
        weights: None,
        periodic: false,
    })
}

type TransferredPcurveSample = (f64, Point2, Point3);

#[allow(clippy::too_many_arguments)]
fn transferred_pcurve_sample(
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    parameter: f64,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<TransferredPcurveSample> {
    let source_uv = pcurve_uv(source_pcurve, parameter)?;
    let point = decoded_surface_point(ir, source_surface, source_uv.u, source_uv.v)
        .or_else(|| model_curve_point(ir, curve, parameter))?;
    let target_uv = blend_boundary_parameter_from_support_pcurve(
        ir,
        target_surface,
        source_surface,
        source_pcurve,
        parameter,
        point,
        tolerance,
    )
    .or_else(|| {
        blend_boundary_parameter_from_support_spine(
            ir,
            target_surface,
            source_surface,
            point,
            seed,
            tolerance,
        )
    })
    .or_else(|| surface_parameters_for_fit(ir, target_surface, point, seed, tolerance))?;
    (decoded_surface_point(ir, target_surface, target_uv.u, target_uv.v)
        .is_some_and(|candidate| point_distance(candidate, point) <= tolerance)
        || blend_boundary_spine_geometry_matches(ir, target_surface, target_uv, point, tolerance))
    .then_some((parameter, target_uv, point))
}

pub(crate) fn blend_boundary_parameter_from_support_spine(
    ir: &CadIr,
    blend: &SurfaceId,
    support: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<Point2> {
    let (supports, spine, _, _) = blend_surface_definition(ir, blend)?;
    let matches = supports
        .iter()
        .enumerate()
        .filter(|(_, candidate)| parameterization_equivalent_surfaces(ir, candidate, support))
        .map(|(boundary, _)| boundary)
        .collect::<Vec<_>>();
    let [boundary] = matches.as_slice() else {
        return None;
    };
    let parameter = closest_spine_parameter(ir, &spine, point, seed.map(|seed| seed.u))?;
    let parameters = Point2::new(parameter, *boundary as f64);
    (blend_surface_point_inner(ir, blend, parameters.u, parameters.v, 0)
        .is_some_and(|candidate| point_distance(candidate, point) <= tolerance)
        || blend_boundary_spine_geometry_matches(ir, blend, parameters, point, tolerance))
    .then_some(parameters)
}

fn blend_boundary_spine_geometry_matches(
    ir: &CadIr,
    blend: &SurfaceId,
    parameters: Point2,
    point: Point3,
    tolerance: f64,
) -> bool {
    if parameters.v.to_bits() != 0.0f64.to_bits() && parameters.v.to_bits() != 1.0f64.to_bits() {
        return false;
    }
    let Some((_, spine, radius, _)) = blend_surface_definition(ir, blend) else {
        return false;
    };
    let Some(center) = model_curve_point(ir, &spine, parameters.u) else {
        return false;
    };
    let radial = Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let distance = (radial.x * radial.x + radial.y * radial.y + radial.z * radial.z).sqrt();
    if !distance.is_finite() || (distance - radius).abs() > tolerance {
        return false;
    }
    let Some(radial) = unit_vector(radial) else {
        return false;
    };
    let Some(tangent) = model_curve_tangent(ir, &spine, parameters.u) else {
        return false;
    };
    let angular_tolerance = (tolerance / radius).max(1.0e-8);
    (radial.x * tangent.x + radial.y * tangent.y + radial.z * tangent.z).abs() <= angular_tolerance
}

#[allow(clippy::too_many_arguments)]
fn append_transferred_pcurve_segment(
    ir: &CadIr,
    curve: &CurveId,
    source_surface: &SurfaceId,
    source_pcurve: &PcurveGeometry,
    target_surface: &SurfaceId,
    first: TransferredPcurveSample,
    last: TransferredPcurveSample,
    tolerance: f64,
    depth: usize,
    samples: &mut Vec<TransferredPcurveSample>,
) -> Option<()> {
    let midpoint_parameter = f64::midpoint(first.0, last.0);
    let midpoint_seed = Point2::new(
        f64::midpoint(first.1.u, last.1.u),
        f64::midpoint(first.1.v, last.1.v),
    );
    let midpoint = transferred_pcurve_sample(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        midpoint_parameter,
        Some(midpoint_seed),
        tolerance,
    )?;
    let fits = [0.25, 0.5, 0.75].into_iter().all(|fraction| {
        let parameter = first.0 + fraction * (last.0 - first.0);
        let uv = Point2::new(
            first.1.u + fraction * (last.1.u - first.1.u),
            first.1.v + fraction * (last.1.v - first.1.v),
        );
        let Some(source_uv) = pcurve_uv(source_pcurve, parameter) else {
            return false;
        };
        let Some(source_point) =
            decoded_surface_point(ir, source_surface, source_uv.u, source_uv.v)
                .or_else(|| model_curve_point(ir, curve, parameter))
        else {
            return false;
        };
        decoded_surface_point(ir, target_surface, uv.u, uv.v)
            .is_some_and(|target_point| point_distance(source_point, target_point) <= tolerance)
            || blend_boundary_spine_geometry_matches(
                ir,
                target_surface,
                uv,
                source_point,
                tolerance,
            )
    });
    if fits {
        samples.push(last);
        return Some(());
    }
    (depth < 16).then_some(())?;
    append_transferred_pcurve_segment(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        first,
        midpoint,
        tolerance,
        depth + 1,
        samples,
    )?;
    append_transferred_pcurve_segment(
        ir,
        curve,
        source_surface,
        source_pcurve,
        target_surface,
        midpoint,
        last,
        tolerance,
        depth + 1,
        samples,
    )
}

fn surface_parameters_for_fit(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
) -> Option<Point2> {
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    match &carrier.geometry {
        SurfaceGeometry::Nurbs(nurbs) => nurbs_parameters(nurbs, point, seed),
        SurfaceGeometry::Procedural { .. } => {
            offset_surface_parameters_with_tolerance(ir, surface, point, seed, Some(tolerance))
                .or_else(|| blend_surface_parameters_for_fit(ir, surface, point, seed, tolerance))
        }
        geometry => analytic_surface_parameters(geometry, point),
    }
}

pub(crate) fn attach_tolerant_edge_intersections(
    ir: &mut CadIr,
    graph: &Graph,
    edges: &BTreeMap<u32, EdgeId>,
    prefix: &str,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let mut candidates = Vec::new();
    for (&xmt, edge_id) in edges {
        let Some(edge) = ir
            .model
            .edges
            .iter()
            .find(|candidate| &candidate.id == edge_id)
        else {
            continue;
        };
        if edge.curve.is_some() || edge.tolerance.is_none() {
            continue;
        }
        let mut supports = ir
            .model
            .coedges
            .iter()
            .filter(|coedge| &coedge.edge == edge_id)
            .filter_map(|coedge| {
                let face = ir
                    .model
                    .loops
                    .iter()
                    .find(|loop_| loop_.id == coedge.owner_loop)?
                    .face
                    .clone();
                ir.model
                    .faces
                    .iter()
                    .find(|candidate| candidate.id == face)
                    .map(|face| face.surface.clone())
            })
            .collect::<BTreeSet<_>>();
        if supports.len() != 2 {
            continue;
        }
        let second = supports.pop_last().expect("two supports");
        let first = supports.pop_first().expect("two supports");
        candidates.push((xmt, edge_id.clone(), [first, second]));
    }

    for (xmt, edge_id, supports) in candidates {
        let curve_id = CurveId(format!("{prefix}:tolerant-curve#{xmt}"));
        let procedural_id = ProceduralCurveId(format!("{prefix}:tolerant-intersection#{xmt}"));
        let Some(edge) = ir
            .model
            .edges
            .iter_mut()
            .find(|candidate| candidate.id == edge_id)
        else {
            continue;
        };
        edge.curve = Some(curve_id.clone());
        edge.param_range = Some([0.0, 1.0]);
        annotations.derived(&edge_id, "curve");
        annotations.derived(&edge_id, "param_range");
        if let Some(node) = graph.get(16, xmt) {
            annotations
                .note(&curve_id, source_stream, node.pos as u64)
                .tag("TOLERANT_EDGE_INTERSECTION");
            annotations
                .note(&procedural_id, source_stream, node.pos as u64)
                .tag("TOLERANT_EDGE_INTERSECTION");
        }
        annotations.derived(&curve_id, "geometry");
        annotations.derived(&procedural_id, "definition");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Procedural {
                construction: procedural_id.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: procedural_id,
            curve: curve_id,
            definition: ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: supports.map(|surface| IntcurveSupportSide {
                        surface: Some(surface),
                        pcurve: None,
                    }),
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });
    }
}

pub(crate) fn pcurve_matches_edge(
    ir: &CadIr,
    edge_id: &EdgeId,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    fit_tolerance: Option<f64>,
) -> bool {
    pcurve_matches_edge_range(ir, edge_id, surface_id, geometry, None, fit_tolerance)
}

fn pcurve_matches_edge_range(
    ir: &CadIr,
    edge_id: &EdgeId,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    parameter_range: Option<[f64; 2]>,
    fit_tolerance: Option<f64>,
) -> bool {
    let Some(edge) = ir.model.edges.iter().find(|edge| &edge.id == edge_id) else {
        return false;
    };
    let Some(coincident_surface) = ir
        .model
        .surfaces
        .iter()
        .find(|surface| &surface.id == surface_id)
        .and_then(|surface| {
            let [t0, t1] = parameter_range.or_else(|| pcurve_parameter_range(geometry))?;
            let uv = [pcurve_uv(geometry, t0)?, pcurve_uv(geometry, t1)?];
            Some([
                surface_point(&surface.geometry, uv[0].u, uv[0].v)?,
                surface_point(&surface.geometry, uv[1].u, uv[1].v)?,
            ])
        })
    else {
        return false;
    };
    let vertex = |id: &VertexId| {
        let vertex = ir.model.vertices.iter().find(|vertex| &vertex.id == id)?;
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)?;
        Some((point.position, vertex.tolerance))
    };
    let (Some((start, start_tolerance)), Some((end, end_tolerance))) =
        (vertex(&edge.start), vertex(&edge.end))
    else {
        return false;
    };
    let allowance = [
        edge.tolerance,
        start_tolerance,
        end_tolerance,
        fit_tolerance,
    ]
    .into_iter()
    .flatten()
    .fold(0.01_f64, f64::max);
    let distance = |a: cadmpeg_ir::math::Point3, b: cadmpeg_ir::math::Point3| {
        ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
    };
    (distance(coincident_surface[0], start) <= allowance
        && distance(coincident_surface[1], end) <= allowance)
        || (distance(coincident_surface[0], end) <= allowance
            && distance(coincident_surface[1], start) <= allowance)
}

#[allow(clippy::too_many_arguments)]
fn retain_unresolved_topology_carriers(
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    surfaces: &mut BTreeMap<u32, SurfaceId>,
    curves: &mut BTreeMap<u32, CurveId>,
    pcurves: &BTreeMap<u32, PcurveId>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let unknown = UnknownId(format!("nx:container:parasolid#{stream_index}"));
    for face in graph.of_kind(14) {
        let Some(surface_xmt) = face.face_fields().map(|fields| fields.surface) else {
            continue;
        };
        if surface_xmt <= 1 || surfaces.contains_key(&surface_xmt) {
            continue;
        }
        let id = SurfaceId(format!("nx:s{stream_index}:surface#unknown-{surface_xmt}"));
        annotations
            .note(&id, source_stream, face.pos as u64)
            .tag("UNRESOLVED_SURFACE_REFERENCE");
        annotations.exactness(&id, Exactness::Unknown);
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Unknown {
                record: Some(unknown.clone()),
            },
            source_object: None,
        });
        surfaces.insert(surface_xmt, id);
    }

    for edge in graph.of_kind(16) {
        let Some(curve_xmt) = edge.edge_fields().map(|fields| fields.curve) else {
            continue;
        };
        if curve_xmt <= 1 || curves.contains_key(&curve_xmt) || pcurves.contains_key(&curve_xmt) {
            continue;
        }
        let id = CurveId(format!("nx:s{stream_index}:curve#unknown-{curve_xmt}"));
        annotations
            .note(&id, source_stream, edge.pos as u64)
            .tag("UNRESOLVED_CURVE_REFERENCE");
        annotations.exactness(&id, Exactness::Unknown);
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Unknown {
                record: Some(unknown.clone()),
            },
            source_object: None,
        });
        curves.insert(curve_xmt, id);
    }
}

fn annotate_node(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    stream: cadmpeg_ir::annotations::StreamHandle,
    node: &Node,
    tag: &str,
) {
    annotations.note(id, stream, node.pos as u64).tag(tag);
}

fn surface_tag(geometry: &SurfaceGeometry) -> &'static str {
    match geometry {
        SurfaceGeometry::Plane { .. } => "PLANE",
        SurfaceGeometry::Cylinder { .. } => "CYLINDER",
        SurfaceGeometry::Cone { .. } => "CONE",
        SurfaceGeometry::Sphere { .. } => "SPHERE",
        SurfaceGeometry::Torus { .. } => "TORUS",
        SurfaceGeometry::Nurbs(_) => "B_SPLINE_SURFACE",
        SurfaceGeometry::Procedural { .. } => "PROCEDURAL_SURFACE",
        SurfaceGeometry::Unknown { .. } => "UNKNOWN_SURFACE",
    }
}

fn curve_tag(geometry: &CurveGeometry) -> &'static str {
    match geometry {
        CurveGeometry::Line { .. } => "LINE",
        CurveGeometry::Circle { .. } => "CIRCLE",
        CurveGeometry::Ellipse { .. } => "ELLIPSE",
        CurveGeometry::Parabola { .. } => "PARABOLA",
        CurveGeometry::Hyperbola { .. } => "HYPERBOLA",
        CurveGeometry::Degenerate { .. } => "DEGENERATE_CURVE",
        CurveGeometry::Nurbs(_) => "B_SPLINE_CURVE",
        CurveGeometry::Procedural { .. } => "PROCEDURAL_CURVE",
        CurveGeometry::Unknown { .. } => "UNKNOWN_CURVE",
    }
}

fn decoded_tolerance(value: f64) -> Option<f64> {
    match value {
        MISSING_TOLERANCE => None,
        value if value.is_finite() && value > 0.0 && value < 1.0e3 => Some(value * 1000.0),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn synthesize_closed_edge_vertex(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    prefix: &str,
    edge: &Node,
    curve: &CurveId,
    range: Option<[f64; 2]>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    tolerance: Option<f64>,
) -> Option<VertexId> {
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve)?
        .geometry;
    let parameter = range.map_or_else(
        || match geometry {
            CurveGeometry::Nurbs(nurbs) => nurbs.knots.first().copied().unwrap_or(0.0),
            _ => 0.0,
        },
        |range| range[0],
    );
    let position = curve_point(geometry, parameter)?;
    let point = PointId(format!("{prefix}:point#closed-edge-{}", edge.xmt));
    let vertex = VertexId(format!("{prefix}:vertex#closed-edge-{}", edge.xmt));
    annotations
        .note(&point, source_stream, edge.pos as u64)
        .tag("CLOSED_EDGE_POINT");
    annotations.exactness(&point, Exactness::Inferred);
    annotations
        .note(&vertex, source_stream, edge.pos as u64)
        .tag("CLOSED_EDGE_VERTEX");
    annotations.exactness(&vertex, Exactness::Inferred);
    ir.model.points.push(Point {
        id: point.clone(),
        position,
    });
    ir.model.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance,
    });
    Some(vertex)
}

fn canonical_trim_range(ir: &CadIr, basis: &CurveId, raw: [f64; 2]) -> Option<[f64; 2]> {
    let curve = ir.model.curves.iter().find(|curve| curve.id == *basis)?;
    match &curve.geometry {
        CurveGeometry::Line { .. } => Some([raw[0] * 1000.0, raw[1] * 1000.0]),
        CurveGeometry::Nurbs(nurbs) => {
            let domain = [*nurbs.knots.first()?, *nurbs.knots.last()?];
            let epsilon = 1.0e-6 * (1.0 + domain[0].abs().max(domain[1].abs()));
            if raw
                .iter()
                .any(|value| *value < domain[0] - epsilon || *value > domain[1] + epsilon)
            {
                None
            } else {
                Some([
                    raw[0].clamp(domain[0], domain[1]),
                    raw[1].clamp(domain[0], domain[1]),
                ])
            }
        }
        _ => Some(raw),
    }
}

fn orient_edge_range(
    ir: &CadIr,
    curve: &CurveId,
    range: [f64; 2],
    start: &VertexId,
    end: &VertexId,
    edge_tolerance: Option<f64>,
) -> Option<([f64; 2], bool)> {
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve)?
        .geometry;
    let range = if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    };
    let range = match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            let sweep = range[1] - range[0];
            (0.0..=std::f64::consts::TAU)
                .contains(&sweep)
                .then_some(())?;
            let start = range[0].rem_euclid(std::f64::consts::TAU);
            [start, start + sweep]
        }
        _ => range,
    };
    let at = match (
        curve_point(geometry, range[0]),
        curve_point(geometry, range[1]),
    ) {
        (Some(start), Some(end)) => [start, end],
        _ if ir
            .model
            .procedural_curves
            .iter()
            .any(|procedural| procedural.curve == *curve) =>
        {
            return Some((range, false));
        }
        _ => return None,
    };
    let vertex_position = |vertex: &VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|candidate| candidate.id == *vertex)?;
        let point = ir
            .model
            .points
            .iter()
            .find(|candidate| candidate.id == vertex.point)?;
        Some((point.position, vertex.tolerance))
    };
    let (start_position, start_tolerance) = vertex_position(start)?;
    let (end_position, end_tolerance) = vertex_position(end)?;
    let cache_tolerance = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == *curve)
        .and_then(|procedural| procedural.cache_fit_tolerance);
    let allowance = [
        edge_tolerance,
        start_tolerance,
        end_tolerance,
        cache_tolerance,
    ]
    .into_iter()
    .flatten()
    .fold(0.01_f64, f64::max);
    let distance = |a: cadmpeg_ir::math::Point3, b: cadmpeg_ir::math::Point3| {
        ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
    };
    if distance(at[0], start_position) <= allowance && distance(at[1], end_position) <= allowance {
        Some((range, false))
    } else if distance(at[1], start_position) <= allowance
        && distance(at[0], end_position) <= allowance
    {
        Some((range, true))
    } else {
        None
    }
}

fn sense(byte: Option<u8>) -> Sense {
    if byte == Some(b'-') {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn unknown_stream(si: usize, stream: &Stream) -> UnknownRecord {
    UnknownRecord {
        id: UnknownId(format!("nx:container:parasolid#{si}")),
        offset: stream.file_offset as u64,
        byte_len: stream.inflated.len() as u64,
        sha256: sha256_hex(&stream.inflated),
        data: Some(stream.inflated.clone()),
        links: Vec::new(),
    }
}

fn source_meta(scan: &Scan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "file_size".to_string(),
        scan.container.data.len().to_string(),
    );
    attributes.insert(
        "footer_offset".to_string(),
        scan.container.footer_offset.to_string(),
    );
    attributes.insert(
        "directory_entries".to_string(),
        scan.container.entries.len().to_string(),
    );
    attributes.insert(
        "partition_streams".to_string(),
        scan.count(StreamKind::Partition).to_string(),
    );
    attributes.insert(
        "deltas_streams".to_string(),
        scan.count(StreamKind::Deltas).to_string(),
    );
    attributes.insert(
        "plain_streams".to_string(),
        scan.count(StreamKind::Plain).to_string(),
    );
    if let Some(schema) = scan.streams.iter().find_map(|s| s.schema.as_deref()) {
        attributes.insert("parasolid_schema".to_string(), schema.to_string());
    }
    for (index, path) in scan
        .container
        .external_reference_paths()
        .into_iter()
        .enumerate()
    {
        attributes.insert(format!("external_reference.{index}"), path);
    }
    let active_ids = scan.container.rmfastload_object_ids();
    if !active_ids.is_empty() {
        attributes.insert(
            "rmfastload_active_object_count".to_string(),
            active_ids.len().to_string(),
        );
    }
    let mut preview_count = 0usize;
    for entry in scan
        .container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/images/preview")
    {
        let Some((offset, size)) = entry.file_span else {
            continue;
        };
        let (Ok(start), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
            continue;
        };
        let Some(payload) = scan.container.data.get(start..start.saturating_add(size)) else {
            continue;
        };
        let Some((width, height, precision, components)) = jpeg_dimensions(payload) else {
            continue;
        };
        let prefix = format!("jpeg_preview_{preview_count}");
        attributes.insert(format!("{prefix}_width"), width.to_string());
        attributes.insert(format!("{prefix}_height"), height.to_string());
        attributes.insert(format!("{prefix}_precision"), precision.to_string());
        attributes.insert(format!("{prefix}_components"), components.to_string());
        attributes.insert(format!("{prefix}_byte_len"), payload.len().to_string());
        attributes.insert(format!("{prefix}_sha256"), sha256_hex(payload));
        preview_count += 1;
    }
    attributes.insert("jpeg_preview_count".to_string(), preview_count.to_string());
    for (index, stream) in scan
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Deltas)
        .enumerate()
    {
        let census = crate::deltas::walk(&stream.inflated);
        attributes.insert(
            format!("deltas.{index}.grammar"),
            "status_byte_framed_topology".to_string(),
        );
        attributes.insert(
            format!("deltas.{index}.bytes_decoded"),
            census.bytes_decoded.to_string(),
        );
        for (name, count) in census.full_counts {
            attributes.insert(format!("deltas.{index}.full.{name}"), count.to_string());
        }
        for (name, count) in census.tombstone_counts {
            attributes.insert(
                format!("deltas.{index}.tombstone.{name}"),
                count.to_string(),
            );
        }
    }
    SourceMeta {
        format: "nx".to_string(),
        attributes,
    }
}

pub(crate) fn jpeg_dimensions(payload: &[u8]) -> Option<(u16, u16, u8, u8)> {
    if payload.get(..2)? != [0xff, 0xd8] {
        return None;
    }
    let mut offset = 2usize;
    while offset < payload.len() {
        while payload.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *payload.get(offset)?;
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            *payload.get(offset)?,
            *payload.get(offset + 1)?,
        ]));
        if length < 2 {
            return None;
        }
        let segment_start = offset + 2;
        let segment_end = offset.checked_add(length)?;
        let segment = payload.get(segment_start..segment_end)?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let precision = *segment.first()?;
            let height = u16::from_be_bytes([*segment.get(1)?, *segment.get(2)?]);
            let width = u16::from_be_bytes([*segment.get(3)?, *segment.get(4)?]);
            let components = *segment.get(5)?;
            if width == 0
                || height == 0
                || components == 0
                || segment.len() != 6 + 3 * usize::from(components)
            {
                return None;
            }
            return Some((width, height, precision, components));
        }
        offset = segment_end;
    }
    None
}

fn build_geometry_report(
    scan: &Scan,
    counts: &Counts,
    has_topology: bool,
    has_unresolved_sub_bodies: bool,
    tessellation_count: usize,
) -> DecodeReport {
    let mut losses = Vec::new();

    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "Decoded {} POINT carrier(s) verbatim from Parasolid POINT records (3×f64 big-endian, \
             metres → millimetres), {} analytic surface carrier(s) ({} plane, {} cylinder, {} \
             cone, {} sphere, {} torus), and {} analytic curve carrier(s) ({} line, {} circle, {} \
             ellipse). All parameters are byte-exact at the document's millimetre scale.",
            counts.points,
            counts.surfaces(),
            counts.planes,
            counts.cylinders,
            counts.cones,
            counts.spheres,
            counts.tori,
            counts.curves(),
            counts.lines,
            counts.circles,
            counts.ellipses,
        ),
        provenance: None,
    });

    if tessellation_count != 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Decoded {tessellation_count} embedded JT display tessellation(s) with scene-node ownership, model-space coordinates, topological triangle connectivity, and corner normals when bound."
            ),
            provenance: None,
        });
    }

    if !has_topology {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "The B-rep topology graph (body→shell→face→loop→fin→edge→vertex) was not \
                      reconstructed because the surviving typed records did not form a complete \
                      connected ownership graph. Exact-key supported partition↔deltas replacements \
                      and deletions were applied before graph construction. Required unresolved \
                      records prevent their dependent incidence from being emitted; decoded geometry \
                      then remains unattached."
                .to_string(),
            provenance: None,
        });
    }

    if counts.intersection_rejections.total() > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} surface-intersection record(s) without a complete validated CHART_s and \
                 term-endpoint witness remain opaque constructions. Support-UV values govern \
                 optional pcurve attachment and do not invalidate a witnessed 3D carrier. Each \
                 Parasolid stream is preserved verbatim as an unknown passthrough record so the \
                 unresolved source bytes remain available. Rejections: {} missing chart, {} missing \
                 start term, {} missing end term, {} endpoint mismatch.",
                counts.intersection_rejections.total(),
                counts.intersection_rejections.missing_chart,
                counts.intersection_rejections.missing_start_term,
                counts.intersection_rejections.missing_end_term,
                counts.intersection_rejections.endpoint_mismatch,
            ),
            provenance: None,
        });
    }

    if scan.count(StreamKind::Deltas) > 0 {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "{} Parasolid deltas stream(s) were paired with the preceding equal-schema partition \
                 in validated UG_PART segment order. Exact-key \
                 BODY, SHELL, FACE, LOOP, FIN, EDGE, VERTEX, REGION, POINT, LINE, CIRCLE, ELLIPSE, PLANE, CYLINDER, CONE, SPHERE, TORUS, B_SURFACE, and B_CURVE full records and compact \
                 non-topology replacements and tombstones were applied using the last event for \
                 each key. Validated partition topology remained authoritative, including any \
                 point, curve, or surface carrier still referenced by surviving topology. \
                 Tombstones without an exact partition key remain unresolved.",
                scan.count(StreamKind::Deltas)
            ),
            provenance: None,
        });
    }

    if has_unresolved_sub_bodies {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "This part is composed of {} sub-body partition(s); its decoded feature-history \
                 Booleans do not resolve every intermediate body object to a partition image. \
                 Carriers from all sub-bodies are emitted without the unresolved composition that \
                 would remove interior/construction faces.",
                scan.count(StreamKind::Partition)
            ),
            provenance: None,
        });
    }

    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Materials, appearances, unresolved entity-owned attribute fields, complete feature parameters, \
                  sketch geometry, constraints, and assembly occurrence placements were not transferred: \
                  their remaining NX object-model and Parasolid field serialization is not decoded."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "nx".to_string(),
        container_only: false,
        geometry_transferred: true,
        losses,
        notes: summary_notes(scan),
    }
}

fn build_metadata_ir(scan: &Scan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind.is_parasolid() {
            let unknown = unknown_stream(si, stream);
            let source_stream = annotations.stream("nx:container");
            annotations
                .note(&unknown.id, source_stream, stream.file_offset as u64)
                .tag(stream.kind.label());
            annotations.exactness(&unknown.id, Exactness::Derived);
            ir.push_native_unknown("nx", unknown)?;
        }
    }
    attach_native_object_model(&mut ir, scan, &mut annotations)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    ir.annotations = annotations.build();
    Ok(ir)
}

fn attach_native_object_model(
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let segment_index_rows = crate::native::segment_index_rows(&scan.container);
    let segment_om_links = crate::native::segment_om_links(&scan.container);
    let segment_stream_links = crate::native::segment_stream_links(&scan.container, &scan.streams);
    let segment_body_bindings =
        crate::native::segment_body_bindings(&scan.container, &scan.streams);
    let parasolid_blend_surface_records =
        crate::native::parasolid_blend_surface_records(&scan.streams);
    let parasolid_blend_bound_records = crate::native::parasolid_blend_bound_records(&scan.streams);
    let parasolid_offset_surface_records =
        crate::native::parasolid_offset_surface_records(&scan.streams);
    let parasolid_trimmed_curve_records =
        crate::native::parasolid_trimmed_curve_records(&scan.streams);
    let parasolid_surface_curve_records =
        crate::native::parasolid_surface_curve_records(&scan.streams);
    let parasolid_intersection_records =
        crate::native::parasolid_intersection_records(&scan.streams);
    let parasolid_term_use_records = crate::native::parasolid_term_use_records(&scan.streams);
    let parasolid_support_uv_records = crate::native::parasolid_support_uv_records(&scan.streams);
    let parasolid_chart_records = crate::native::parasolid_chart_records(&scan.streams);
    let parasolid_attribute_definitions =
        crate::native::parasolid_attribute_definitions(&scan.streams);
    let parasolid_entity_51_records = crate::native::parasolid_entity_51_records(&scan.streams);
    let parasolid_entity_52_integer_records =
        crate::native::parasolid_entity_52_integer_records(&scan.streams);
    let parasolid_entity_53_double_records =
        crate::native::parasolid_entity_53_double_records(&scan.streams);
    let parasolid_entity_54_string_records =
        crate::native::parasolid_entity_54_string_records(&scan.streams);
    let parasolid_entity_51_numeric_uses = crate::native::parasolid_entity_51_numeric_uses(
        &parasolid_entity_51_records,
        &parasolid_entity_52_integer_records,
        &parasolid_entity_53_double_records,
    );
    let parasolid_entity_51_string_uses = crate::native::parasolid_entity_51_string_uses(
        &parasolid_entity_51_records,
        &parasolid_entity_54_string_records,
    );
    let parasolid_topology_attribute_list_references =
        crate::native::parasolid_topology_attribute_list_references(
            &scan.streams,
            &parasolid_entity_51_records,
        );
    let parasolid_topology_attribute_class_uses =
        crate::native::parasolid_topology_attribute_class_uses(
            &parasolid_topology_attribute_list_references,
            &parasolid_entity_51_records,
            &parasolid_attribute_definitions,
        );
    let om_record_areas = crate::native::om_record_areas(&scan.container);
    let feature_operation_labels = crate::native::feature_operation_labels(&scan.container);
    let feature_operation_records = crate::native::feature_operation_records(&scan.container);
    let feature_payload_strings = crate::native::feature_payload_strings(&scan.container);
    let feature_simple_hole_templates = crate::native::feature_simple_hole_templates(
        &feature_operation_labels,
        &feature_operation_records,
        &feature_payload_strings,
    );
    let feature_simple_hole_repeated_scalar_lanes =
        crate::native::feature_simple_hole_repeated_scalar_lanes(&scan.container);
    let feature_simple_hole_repeated_scalar_lane_block_references =
        crate::native::feature_simple_hole_repeated_scalar_lane_block_references(&scan.container);
    let feature_simple_hole_construction_groups =
        crate::native::feature_simple_hole_construction_groups(
            &feature_simple_hole_repeated_scalar_lanes,
            &feature_simple_hole_repeated_scalar_lane_block_references,
        );
    let feature_body_references = crate::native::feature_body_references(&scan.container);
    let feature_body_reference_occurrences =
        crate::native::feature_body_reference_occurrences(&scan.container);
    let feature_input_blocks = crate::native::feature_input_blocks(&scan.container);
    let feature_input_block_identity_groups =
        crate::native::feature_input_block_identity_groups(&feature_input_blocks);
    let display_jt_indices = crate::native::display_jt_indices(&scan.container);
    let display_jt_documents =
        crate::native::display_jt_documents(&scan.container, &display_jt_indices);
    let display_jt_segments =
        crate::native::display_jt_segments(&scan.container, &display_jt_documents);
    let display_jt_shape_lod_elements =
        crate::native::display_jt_shape_lod_elements(&scan.container, &display_jt_segments);
    let display_jt_tri_strip_lod_headers = crate::native::display_jt_tri_strip_lod_headers(
        &scan.container,
        &display_jt_shape_lod_elements,
    );
    let display_jt_initial_face_degree_symbols =
        crate::native::display_jt_initial_face_degree_symbols(
            &scan.container,
            &display_jt_shape_lod_elements,
        );
    let (
        display_jt_topology_packet_sequences,
        display_jt_vertex_records_headers,
        display_jt_coordinate_array_headers,
    ) = crate::native::display_jt_topology_packet_sequences(
        &scan.container,
        &display_jt_shape_lod_elements,
    );
    let display_jt_vertex_coordinates = crate::native::display_jt_vertex_coordinates(
        &scan.container,
        &display_jt_coordinate_array_headers,
    );
    let display_jt_vertex_normals = crate::native::display_jt_vertex_normals(
        &scan.container,
        &display_jt_vertex_records_headers,
        &display_jt_coordinate_array_headers,
        &display_jt_vertex_coordinates,
    );
    let display_jt_vertex_colors = crate::native::display_jt_vertex_colors(
        &scan.container,
        &display_jt_vertex_records_headers,
        &display_jt_coordinate_array_headers,
        &display_jt_vertex_coordinates,
        &display_jt_vertex_normals,
    );
    let display_jt_vertex_texture_coordinates =
        crate::native::display_jt_vertex_texture_coordinates(
            &scan.container,
            &display_jt_vertex_records_headers,
            &display_jt_coordinate_array_headers,
            &display_jt_vertex_coordinates,
            &display_jt_vertex_normals,
            &display_jt_vertex_colors,
        );
    let display_jt_vertex_flags = crate::native::display_jt_vertex_flags(
        &scan.container,
        &display_jt_vertex_records_headers,
        &display_jt_coordinate_array_headers,
        &display_jt_vertex_coordinates,
        &display_jt_vertex_normals,
        &display_jt_vertex_colors,
        &display_jt_vertex_texture_coordinates,
    );
    let display_jt_polygon_meshes = crate::native::display_jt_polygon_meshes(
        &display_jt_topology_packet_sequences,
        &display_jt_coordinate_array_headers,
    );
    let (display_jt_compressed_elements, display_jt_compressed_element_sequences) =
        crate::native::display_jt_compressed_element_sequences(
            &scan.container,
            &display_jt_segments,
        );
    let display_jt_string_property_atoms =
        crate::native::display_jt_string_property_atoms(&scan.container, &display_jt_segments);
    let display_jt_shape_lod_bindings =
        crate::native::display_jt_shape_lod_bindings(&scan.container, &display_jt_segments);
    let display_jt_base_node_data = crate::native::display_jt_base_node_data(
        &scan.container,
        &display_jt_segments,
        &display_jt_documents,
    );
    let display_jt_geometric_transform_attributes =
        crate::native::display_jt_geometric_transform_attributes(
            &scan.container,
            &display_jt_segments,
            &display_jt_documents,
        );
    let display_jt_partition_nodes = crate::native::display_jt_partition_nodes(
        &scan.container,
        &display_jt_segments,
        &display_jt_documents,
    );
    let display_jt_range_lod_nodes = crate::native::display_jt_range_lod_nodes(
        &scan.container,
        &display_jt_segments,
        &display_jt_documents,
    );
    let display_jt_tri_strip_shape_nodes = crate::native::display_jt_tri_strip_shape_nodes(
        &scan.container,
        &display_jt_segments,
        &display_jt_documents,
    );
    let display_jt_tessellations = display_jt_tessellations(DisplayJtTessellationInputs {
        meshes: &display_jt_polygon_meshes,
        coordinates: &display_jt_vertex_coordinates,
        normals: &display_jt_vertex_normals,
        colors: &display_jt_vertex_colors,
        texture_coordinates: &display_jt_vertex_texture_coordinates,
        vertex_flags: &display_jt_vertex_flags,
        vertex_headers: &display_jt_vertex_records_headers,
        coordinate_headers: &display_jt_coordinate_array_headers,
        shape_elements: &display_jt_shape_lod_elements,
        bindings: &display_jt_shape_lod_bindings,
        shape_nodes: &display_jt_tri_strip_shape_nodes,
        base_nodes: &display_jt_base_node_data,
        transforms: &display_jt_geometric_transform_attributes,
        partition_nodes: &display_jt_partition_nodes,
        range_lod_nodes: &display_jt_range_lod_nodes,
        compressed_elements: &display_jt_compressed_elements,
    })
    .unwrap_or_default();
    let feature_datum_csys_constructions =
        crate::native::feature_datum_csys_constructions(&scan.container);
    let feature_datum_csys_payloads = crate::native::feature_datum_csys_payloads(
        &scan.container,
        &feature_datum_csys_constructions,
    );
    let feature_datum_csys_payload_scalar_pairs =
        crate::native::feature_datum_csys_payload_scalar_pairs(
            &scan.container,
            &feature_datum_csys_payloads,
        );
    let feature_datum_csys_payload_fixed_pairs =
        crate::native::feature_datum_csys_payload_fixed_pairs(
            &scan.container,
            &feature_datum_csys_payloads,
        );
    let feature_datum_csys_payload_scalars = crate::native::feature_datum_csys_payload_scalars(
        &scan.container,
        &feature_datum_csys_payloads,
    );
    let feature_datum_csys_descriptors = crate::native::feature_datum_csys_descriptors(
        &scan.container,
        &feature_datum_csys_constructions,
    );
    let feature_datum_plane_headers = crate::native::feature_datum_plane_headers(&scan.container);
    let feature_datum_plane_block_uses = crate::native::feature_datum_plane_block_uses(
        &feature_datum_plane_headers,
        &feature_input_blocks,
    );
    let feature_datum_plane_payloads =
        crate::native::feature_datum_plane_payloads(&scan.container, &feature_datum_plane_headers);
    let feature_datum_plane_payload_scalar_pairs =
        crate::native::feature_datum_plane_payload_scalar_pairs(
            &scan.container,
            &feature_datum_plane_payloads,
        );
    let feature_datum_plane_descriptors = crate::native::feature_datum_plane_descriptors(
        &scan.container,
        &feature_datum_plane_headers,
    );
    let feature_datum_plane_csys_identity_uses =
        crate::native::feature_datum_plane_csys_identity_uses(
            &feature_datum_plane_descriptors,
            &feature_datum_csys_descriptors,
        );
    let feature_datum_csys_block_uses = crate::native::feature_datum_csys_block_uses(
        &feature_datum_csys_constructions,
        &feature_input_blocks,
    );
    let feature_sketch_references = crate::native::feature_sketch_references(&scan.container);
    let feature_extrude_profile_references =
        crate::native::feature_extrude_profile_references(&scan.container);
    let feature_extrude_payload_headers =
        crate::native::feature_extrude_payload_headers(&scan.container);
    let feature_extrude_payload_footers =
        crate::native::feature_extrude_payload_footers(&scan.container);
    let feature_extrude_payload_scalar_triples =
        crate::native::feature_extrude_payload_scalar_triples(&scan.container);
    let feature_operation_body_scalar_triples =
        crate::native::feature_operation_body_scalar_triples(&scan.container);
    let feature_operation_body_members =
        crate::native::feature_operation_body_members(&scan.container);
    let feature_operation_body_operands = crate::native::feature_operation_body_operands(
        &feature_operation_body_members,
        &feature_body_reference_occurrences,
        &segment_body_bindings,
    );
    let feature_operation_body_11_continuations =
        crate::native::feature_operation_body_11_continuations(&scan.container);
    let feature_operation_body_reference_lanes =
        crate::native::feature_operation_body_reference_lanes(&scan.container);
    let feature_extrude_construction_profiles =
        crate::native::feature_extrude_construction_profiles(
            &feature_extrude_profile_references,
            &feature_operation_body_reference_lanes,
        );
    let feature_extrude_payload_32_branches =
        crate::native::feature_extrude_payload_32_branches(&scan.container);
    let feature_extrude_32_constructions = crate::native::feature_extrude_32_constructions(
        &feature_extrude_profile_references,
        &feature_extrude_payload_32_branches,
    );
    let feature_block_construction_references =
        crate::native::feature_block_construction_references(&scan.container);
    let feature_block_constructions =
        crate::native::feature_block_constructions(&feature_block_construction_references);
    let feature_block_construction_payloads = crate::native::feature_block_construction_payloads(
        &scan.container,
        &feature_block_constructions,
    );
    let feature_block_payload_scalars = crate::native::feature_block_payload_scalars(
        &scan.container,
        &feature_block_construction_payloads,
    );
    let feature_block_payload_names = crate::native::feature_block_payload_names(
        &scan.container,
        &feature_block_construction_payloads,
    );
    let feature_block_payload_named_records = crate::native::feature_block_payload_named_records(
        &feature_block_construction_payloads,
        &feature_block_payload_names,
        &feature_block_payload_scalars,
    );
    let feature_block_payload_points = crate::native::feature_block_payload_points(
        &feature_block_payload_named_records,
        &feature_block_payload_names,
        &feature_block_payload_scalars,
    );
    let feature_block_payload_point_groups =
        crate::native::feature_block_payload_point_groups(&feature_block_payload_points);
    let feature_sketch_records = crate::native::feature_sketch_records(
        &feature_operation_labels,
        &feature_operation_records,
        &feature_input_blocks,
        &feature_sketch_references,
    );
    let feature_sketch_construction_inputs = crate::native::feature_sketch_construction_inputs(
        &feature_sketch_records,
        &feature_sketch_references,
    );
    let feature_sketch_construction_payloads = crate::native::feature_sketch_construction_payloads(
        &scan.container,
        &feature_sketch_construction_inputs,
    );
    let feature_sketch_payload_coordinate_pairs =
        crate::native::feature_sketch_payload_coordinate_pairs(
            &scan.container,
            &feature_sketch_construction_payloads,
        );
    let feature_sketch_payload_fixed_pairs = crate::native::feature_sketch_payload_fixed_pairs(
        &scan.container,
        &feature_sketch_construction_payloads,
    );
    let feature_sketch_payload_scalars = crate::native::feature_sketch_payload_scalars(
        &scan.container,
        &feature_sketch_construction_inputs,
    );
    let feature_sketch_payload_names = crate::native::feature_sketch_payload_names(
        &scan.container,
        &feature_sketch_construction_inputs,
    );
    let feature_sketch_payload_named_records = crate::native::feature_sketch_payload_named_records(
        &feature_sketch_construction_payloads,
        &feature_sketch_payload_names,
        &feature_sketch_payload_scalars,
        &feature_sketch_payload_fixed_pairs,
    );
    let feature_sketch_fixed_points = crate::native::feature_sketch_fixed_points(
        &feature_sketch_payload_named_records,
        &feature_sketch_payload_names,
        &feature_sketch_payload_fixed_pairs,
    );
    let feature_sketch_points = crate::native::feature_sketch_points(
        &feature_sketch_payload_named_records,
        &feature_sketch_payload_names,
        &feature_sketch_payload_scalars,
    );
    let feature_sketch_point_groups =
        crate::native::feature_sketch_point_groups(&feature_sketch_points);
    let offset_store_named_points = crate::native::offset_store_named_points(&scan.container);
    let feature_sketch_named_point_block_uses =
        crate::native::feature_sketch_named_point_block_uses(
            &feature_sketch_references,
            &offset_store_named_points,
        );
    let feature_sketch_preceding_named_point_uses =
        crate::native::feature_sketch_preceding_named_point_uses(
            &feature_sketch_references,
            &offset_store_named_points,
        );
    let feature_sketch_point_uses = crate::native::feature_sketch_point_uses(
        &feature_sketch_point_groups,
        &offset_store_named_points,
        &feature_sketch_named_point_block_uses,
    );
    let feature_sketch_datum_csys_dependencies =
        crate::native::feature_sketch_datum_csys_dependencies(
            &feature_operation_labels,
            &offset_store_named_points,
            &feature_sketch_point_uses,
            &feature_datum_csys_constructions,
        );
    let feature_boolean_operations = crate::native::feature_boolean_operations(&scan.container);
    let segment_body_lineage_statuses = crate::native::segment_body_lineage_statuses(
        &feature_operation_labels,
        &feature_body_references,
        &feature_boolean_operations,
        &feature_operation_body_operands,
        &segment_body_bindings,
    )
    .unwrap_or_default();
    let expression_declarations = crate::native::expression_declarations(&scan.container);
    let data_block_object_frames = crate::native::data_block_object_frames(&scan.container);
    let expressions = crate::native::expressions(&scan.container);
    let classes = crate::native::class_definitions(&scan.container);
    let fields = crate::native::field_definitions(&scan.container);
    let object_records = crate::native::object_records(&scan.container);
    let data_blocks = crate::native::data_blocks(&scan.container);
    let data_block_control_values = crate::native::data_block_control_values(&scan.container);
    let data_block_control_class_references =
        crate::native::data_block_control_class_references(&scan.container);
    let data_block_control_index_values =
        crate::native::data_block_control_index_values(&scan.container);
    let data_block_control_references =
        crate::native::data_block_control_references(&scan.container);
    let data_block_control_handle_pairs =
        crate::native::data_block_control_handle_pairs(&data_block_control_references);
    let data_block_references = crate::native::data_block_references(&scan.container);
    let data_block_counted_index_lanes =
        crate::native::data_block_counted_index_lanes(&scan.container);
    let data_block_abr_reference_lanes =
        crate::native::data_block_abr_reference_lanes(&scan.container);
    let feature_parameter_bindings = crate::native::feature_parameter_bindings(
        &feature_input_blocks,
        &data_block_references,
        &expressions,
    );
    let feature_parameter_uses = crate::native::feature_parameter_uses(&feature_parameter_bindings);
    let feature_block_dimensions = crate::native::feature_block_dimensions(
        &feature_block_constructions,
        &feature_parameter_bindings,
        &expression_declarations,
        &expressions,
    );
    let store_headers = crate::native::store_headers(&scan.container);
    let string_values = crate::native::string_values(&scan.container);
    let object_references = crate::native::object_references(&scan.container);
    let configurations = crate::native::configurations(&scan.container);
    let part_attributes = crate::native::part_attributes(&scan.container);
    let configuration_attribute_uses =
        crate::native::configuration_attribute_uses(&configurations, &part_attributes);
    let external_references = crate::native::external_references(&scan.container);
    let external_reference_records = crate::native::external_reference_records(&scan.container);
    let material_texture_assets = crate::native::material_texture_assets(&scan.container);
    let material_texture_catalog_entries =
        crate::native::material_texture_catalog_entries(&scan.container, &material_texture_assets);
    let persistent_handles = crate::native::persistent_handles(
        &object_references,
        &data_block_control_references,
        &external_reference_records,
    );
    let object_sections = scan.container.indexed_om_sections();
    if segment_index_rows.is_empty()
        && segment_om_links.is_empty()
        && segment_stream_links.is_empty()
        && segment_body_bindings.is_empty()
        && segment_body_lineage_statuses.is_empty()
        && parasolid_blend_surface_records.is_empty()
        && parasolid_blend_bound_records.is_empty()
        && parasolid_offset_surface_records.is_empty()
        && parasolid_trimmed_curve_records.is_empty()
        && parasolid_surface_curve_records.is_empty()
        && parasolid_intersection_records.is_empty()
        && parasolid_term_use_records.is_empty()
        && parasolid_support_uv_records.is_empty()
        && parasolid_chart_records.is_empty()
        && parasolid_attribute_definitions.is_empty()
        && parasolid_entity_51_records.is_empty()
        && parasolid_entity_52_integer_records.is_empty()
        && parasolid_entity_53_double_records.is_empty()
        && parasolid_entity_54_string_records.is_empty()
        && parasolid_entity_51_numeric_uses.is_empty()
        && parasolid_entity_51_string_uses.is_empty()
        && parasolid_topology_attribute_list_references.is_empty()
        && om_record_areas.is_empty()
        && feature_operation_labels.is_empty()
        && feature_operation_records.is_empty()
        && feature_payload_strings.is_empty()
        && feature_simple_hole_templates.is_empty()
        && feature_simple_hole_repeated_scalar_lanes.is_empty()
        && feature_simple_hole_repeated_scalar_lane_block_references.is_empty()
        && feature_simple_hole_construction_groups.is_empty()
        && feature_body_references.is_empty()
        && feature_input_blocks.is_empty()
        && feature_input_block_identity_groups.is_empty()
        && feature_datum_csys_constructions.is_empty()
        && feature_datum_plane_headers.is_empty()
        && feature_datum_plane_block_uses.is_empty()
        && feature_datum_plane_payloads.is_empty()
        && feature_datum_csys_block_uses.is_empty()
        && feature_sketch_references.is_empty()
        && feature_extrude_profile_references.is_empty()
        && feature_extrude_payload_headers.is_empty()
        && feature_extrude_payload_footers.is_empty()
        && feature_extrude_payload_scalar_triples.is_empty()
        && feature_operation_body_scalar_triples.is_empty()
        && feature_operation_body_members.is_empty()
        && feature_operation_body_operands.is_empty()
        && feature_operation_body_11_continuations.is_empty()
        && feature_operation_body_reference_lanes.is_empty()
        && feature_extrude_construction_profiles.is_empty()
        && feature_extrude_payload_32_branches.is_empty()
        && feature_extrude_32_constructions.is_empty()
        && feature_block_construction_references.is_empty()
        && feature_block_constructions.is_empty()
        && feature_block_construction_payloads.is_empty()
        && feature_block_payload_scalars.is_empty()
        && feature_block_payload_names.is_empty()
        && feature_block_payload_named_records.is_empty()
        && feature_block_payload_points.is_empty()
        && feature_block_payload_point_groups.is_empty()
        && feature_sketch_records.is_empty()
        && feature_sketch_construction_inputs.is_empty()
        && feature_sketch_construction_payloads.is_empty()
        && feature_sketch_payload_scalars.is_empty()
        && feature_sketch_payload_names.is_empty()
        && feature_sketch_payload_fixed_pairs.is_empty()
        && feature_sketch_payload_named_records.is_empty()
        && feature_sketch_points.is_empty()
        && feature_sketch_fixed_points.is_empty()
        && feature_sketch_point_groups.is_empty()
        && offset_store_named_points.is_empty()
        && feature_sketch_named_point_block_uses.is_empty()
        && feature_sketch_point_uses.is_empty()
        && feature_sketch_datum_csys_dependencies.is_empty()
        && feature_boolean_operations.is_empty()
        && expression_declarations.is_empty()
        && data_block_object_frames.is_empty()
        && expressions.is_empty()
        && classes.is_empty()
        && fields.is_empty()
        && object_records.is_empty()
        && data_blocks.is_empty()
        && data_block_control_values.is_empty()
        && data_block_control_class_references.is_empty()
        && data_block_control_index_values.is_empty()
        && data_block_control_references.is_empty()
        && data_block_control_handle_pairs.is_empty()
        && data_block_references.is_empty()
        && data_block_counted_index_lanes.is_empty()
        && data_block_abr_reference_lanes.is_empty()
        && feature_parameter_bindings.is_empty()
        && feature_parameter_uses.is_empty()
        && store_headers.is_empty()
        && string_values.is_empty()
        && object_references.is_empty()
        && persistent_handles.is_empty()
        && configurations.is_empty()
        && part_attributes.is_empty()
        && external_references.is_empty()
        && external_reference_records.is_empty()
        && material_texture_assets.is_empty()
        && material_texture_catalog_entries.is_empty()
        && object_sections.is_empty()
        && display_jt_indices.is_empty()
    {
        return Ok(());
    }
    let annotation_stream = annotations.stream("nx:container");
    for (tessellation, source_offset) in display_jt_tessellations {
        annotations
            .note(&tessellation.id, annotation_stream, source_offset)
            .tag("DISPLAY_JT_TESSELLATION");
        annotations.exactness(&tessellation.id, Exactness::Derived);
        ir.model.tessellations.push(tessellation);
    }
    for index in &display_jt_indices {
        annotations
            .note(&index.id, annotation_stream, index.source_offset)
            .tag("DISPLAY_JT_INDEX");
        annotations.exactness(&index.id, Exactness::ByteExact);
        for row in &index.rows {
            annotations
                .note(&row.id, annotation_stream, row.source_offset)
                .tag("DISPLAY_JT_INDEX_ROW");
            annotations.exactness(&row.id, Exactness::ByteExact);
        }
    }
    for document in &display_jt_documents {
        annotations
            .note(&document.id, annotation_stream, document.source_offset)
            .tag("DISPLAY_JT_DOCUMENT");
        annotations.exactness(&document.id, Exactness::ByteExact);
        for entry in &document.toc_entries {
            annotations
                .note(&entry.id, annotation_stream, entry.source_offset)
                .tag("DISPLAY_JT_TOC_ENTRY");
            annotations.exactness(&entry.id, Exactness::ByteExact);
        }
    }
    for segment in &display_jt_segments {
        annotations
            .note(&segment.id, annotation_stream, segment.source_offset)
            .tag("DISPLAY_JT_SEGMENT");
        annotations.exactness(&segment.id, Exactness::ByteExact);
    }
    for element in &display_jt_shape_lod_elements {
        annotations
            .note(&element.id, annotation_stream, element.source_offset)
            .tag("DISPLAY_JT_SHAPE_LOD_ELEMENT");
        annotations.exactness(&element.id, Exactness::ByteExact);
    }
    for header in &display_jt_tri_strip_lod_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("DISPLAY_JT_TRI_STRIP_LOD_HEADER");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for symbols in &display_jt_initial_face_degree_symbols {
        annotations
            .note(&symbols.id, annotation_stream, symbols.source_offset)
            .tag("DISPLAY_JT_INITIAL_FACE_DEGREE_SYMBOLS");
        annotations.exactness(&symbols.id, Exactness::ByteExact);
    }
    for sequence in &display_jt_topology_packet_sequences {
        annotations
            .note(&sequence.id, annotation_stream, sequence.source_offset)
            .tag("DISPLAY_JT_TOPOLOGY_PACKET_SEQUENCE");
        annotations.exactness(&sequence.id, Exactness::ByteExact);
    }
    for header in &display_jt_vertex_records_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("DISPLAY_JT_VERTEX_RECORDS_HEADER");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for header in &display_jt_coordinate_array_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("DISPLAY_JT_COORDINATE_ARRAY_HEADER");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for coordinates in &display_jt_vertex_coordinates {
        annotations
            .note(
                &coordinates.id,
                annotation_stream,
                coordinates.source_offset,
            )
            .tag("DISPLAY_JT_VERTEX_COORDINATES");
        annotations.exactness(&coordinates.id, Exactness::Derived);
    }
    for normals in &display_jt_vertex_normals {
        annotations
            .note(&normals.id, annotation_stream, normals.source_offset)
            .tag("DISPLAY_JT_VERTEX_NORMALS");
        annotations.exactness(&normals.id, Exactness::Derived);
    }
    for colors in &display_jt_vertex_colors {
        annotations
            .note(&colors.id, annotation_stream, colors.source_offset)
            .tag("DISPLAY_JT_VERTEX_COLORS");
        annotations.exactness(&colors.id, Exactness::Derived);
    }
    for texture_coordinates in &display_jt_vertex_texture_coordinates {
        annotations
            .note(
                &texture_coordinates.id,
                annotation_stream,
                texture_coordinates.source_offset,
            )
            .tag("DISPLAY_JT_VERTEX_TEXTURE_COORDINATES");
        annotations.exactness(&texture_coordinates.id, Exactness::Derived);
    }
    for flags in &display_jt_vertex_flags {
        annotations
            .note(&flags.id, annotation_stream, flags.source_offset)
            .tag("DISPLAY_JT_VERTEX_FLAGS");
        annotations.exactness(&flags.id, Exactness::Derived);
    }
    for transform in &display_jt_geometric_transform_attributes {
        annotations
            .note(&transform.id, annotation_stream, transform.source_offset)
            .tag("DISPLAY_JT_GEOMETRIC_TRANSFORM");
        annotations.exactness(&transform.id, Exactness::Derived);
    }
    for mesh in &display_jt_polygon_meshes {
        annotations
            .note(&mesh.id, annotation_stream, mesh.source_offset)
            .tag("DISPLAY_JT_POLYGON_MESH");
        annotations.exactness(&mesh.id, Exactness::Derived);
    }
    for sequence in &display_jt_compressed_element_sequences {
        annotations
            .note(&sequence.id, annotation_stream, sequence.source_offset)
            .tag("DISPLAY_JT_COMPRESSED_ELEMENT_SEQUENCE");
        annotations.exactness(&sequence.id, Exactness::ByteExact);
    }
    for element in &display_jt_compressed_elements {
        annotations
            .note(&element.id, annotation_stream, element.source_offset)
            .tag("DISPLAY_JT_COMPRESSED_ELEMENT");
        annotations.exactness(&element.id, Exactness::ByteExact);
    }
    for atom in &display_jt_string_property_atoms {
        annotations
            .note(&atom.id, annotation_stream, atom.source_offset)
            .tag("DISPLAY_JT_STRING_PROPERTY_ATOM");
        annotations.exactness(&atom.id, Exactness::ByteExact);
    }
    for binding in &display_jt_shape_lod_bindings {
        annotations
            .note(&binding.id, annotation_stream, binding.source_offset)
            .tag("DISPLAY_JT_SHAPE_LOD_BINDING");
        annotations.exactness(&binding.id, Exactness::ByteExact);
    }
    for node in &display_jt_base_node_data {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_BASE_NODE_DATA");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for node in &display_jt_partition_nodes {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_PARTITION_NODE");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for node in &display_jt_range_lod_nodes {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_RANGE_LOD_NODE");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for node in &display_jt_tri_strip_shape_nodes {
        annotations
            .note(&node.id, annotation_stream, node.source_offset)
            .tag("DISPLAY_JT_TRI_STRIP_SHAPE_NODE");
        annotations.exactness(&node.id, Exactness::ByteExact);
    }
    for row in &segment_index_rows {
        annotations
            .note(&row.id, annotation_stream, row.source_offset)
            .tag("UG_PART_SEGMENT_INDEX_ROW");
        annotations.exactness(&row.id, Exactness::ByteExact);
    }
    for link in &segment_stream_links {
        annotations
            .note(&link.id, annotation_stream, link.source_offset)
            .tag("UG_PART_SEGMENT_STREAM_LINK");
        annotations.exactness(&link.id, Exactness::ByteExact);
    }
    for binding in &segment_body_bindings {
        annotations
            .note(&binding.id, annotation_stream, binding.source_offset)
            .tag("UG_PART_SEGMENT_BODY_BINDING");
        annotations.exactness(&binding.id, Exactness::ByteExact);
    }
    for status in &segment_body_lineage_statuses {
        annotations
            .note(&status.id, annotation_stream, status.source_offset)
            .tag("SEGMENT_BODY_LINEAGE_STATUS");
        annotations.exactness(&status.id, Exactness::Derived);
    }
    for record in &parasolid_blend_surface_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("BLEND_SURF");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_blend_bound_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("BLEND_BOUND");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_offset_surface_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("OFFSET_SURF");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_trimmed_curve_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("TRIMMED_CURVE");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_surface_curve_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("SP_CURVE");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_intersection_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag(if record.delta_twin {
                "INTERSECTION_DATA"
            } else {
                "INTERSECTION"
            });
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_term_use_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("term_use");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_support_uv_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("values");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_chart_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("CHART_s");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for definition in &parasolid_attribute_definitions {
        let source_stream = annotations.stream(format!("nx:s{}", definition.stream_ordinal));
        annotations
            .note(&definition.id, source_stream, definition.inflated_offset)
            .tag("ATTRIBUTE_DEFINITION");
        annotations.exactness(&definition.id, Exactness::ByteExact);
    }
    for record in &parasolid_entity_51_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("ENTITY_51");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_entity_52_integer_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("ENTITY_52_INTEGERS");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_entity_53_double_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("ENTITY_53_DOUBLES");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for record in &parasolid_entity_54_string_records {
        let source_stream = annotations.stream(format!("nx:s{}", record.stream_ordinal));
        annotations
            .note(&record.id, source_stream, record.inflated_offset)
            .tag("ENTITY_54_STRING");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for block_use in &parasolid_entity_51_string_uses {
        let source_stream = annotations.stream(format!("nx:s{}", block_use.stream_ordinal));
        annotations
            .note(&block_use.id, source_stream, block_use.inflated_offset)
            .tag("ENTITY_51_STRING_USE");
        annotations.exactness(&block_use.id, Exactness::ByteExact);
    }
    for value_use in &parasolid_entity_51_numeric_uses {
        let source_stream = annotations.stream(format!("nx:s{}", value_use.stream_ordinal));
        annotations
            .note(&value_use.id, source_stream, value_use.inflated_offset)
            .tag("ENTITY_51_NUMERIC_USE");
        annotations.exactness(&value_use.id, Exactness::ByteExact);
    }
    for reference in &parasolid_topology_attribute_list_references {
        let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
        annotations
            .note(&reference.id, source_stream, reference.inflated_offset)
            .tag("TOPOLOGY_ATTRIBUTE_LIST_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for class_use in &parasolid_topology_attribute_class_uses {
        let reference = parasolid_topology_attribute_list_references
            .iter()
            .find(|reference| reference.id == class_use.topology_attribute_reference)
            .expect("class use owns a topology attribute reference");
        let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
        let entity = parasolid_entity_51_records
            .iter()
            .find(|entity| entity.id == class_use.entity_51_record)
            .expect("class use owns a type-81 entity");
        annotations
            .note(&class_use.id, source_stream, entity.inflated_offset)
            .tag("TOPOLOGY_ATTRIBUTE_CLASS_USE");
        annotations.exactness(&class_use.id, Exactness::Derived);
    }
    for frame in &data_block_object_frames {
        annotations
            .note(&frame.id, annotation_stream, frame.source_offset)
            .tag("OFFSET_STORE_OBJECT_FRAME");
        annotations.exactness(&frame.id, Exactness::ByteExact);
    }
    for point in &offset_store_named_points {
        annotations
            .note(&point.id, annotation_stream, point.source_offset)
            .tag("OFFSET_STORE_NAMED_POINT");
        annotations.exactness(&point.id, Exactness::ByteExact);
    }
    for block_use in &feature_sketch_named_point_block_uses {
        annotations
            .note(&block_use.id, annotation_stream, block_use.source_offset)
            .tag("SKETCH_NAMED_POINT_BLOCK_USE");
        annotations.exactness(&block_use.id, Exactness::ByteExact);
    }
    for point_use in &feature_sketch_preceding_named_point_uses {
        annotations
            .note(&point_use.id, annotation_stream, point_use.source_offset)
            .tag("SKETCH_PRECEDING_NAMED_POINT_USE");
        annotations.exactness(&point_use.id, Exactness::ByteExact);
    }
    for point_use in &feature_sketch_point_uses {
        annotations
            .note(
                &point_use.id,
                annotation_stream,
                point_use.source_offsets[0],
            )
            .tag("SKETCH_POINT_USE");
        annotations.exactness(&point_use.id, Exactness::Derived);
    }
    for dependency in &feature_sketch_datum_csys_dependencies {
        annotations
            .note(&dependency.id, annotation_stream, dependency.source_offset)
            .tag("SKETCH_DATUM_CSYS_DEPENDENCY");
        annotations.exactness(&dependency.id, Exactness::Derived);
    }
    for group in &feature_input_block_identity_groups {
        annotations
            .note(&group.id, annotation_stream, group.source_offsets[0])
            .tag("FEATURE_INPUT_BLOCK_IDENTITY_GROUP");
        annotations.exactness(&group.id, Exactness::ByteExact);
    }
    for lane in &data_block_abr_reference_lanes {
        annotations
            .note(&lane.id, annotation_stream, lane.source_offset)
            .tag("OFFSET_STORE_ABR_REFERENCE_LANE");
        annotations.exactness(&lane.id, Exactness::ByteExact);
    }
    for link in &segment_om_links {
        annotations
            .note(&link.id, annotation_stream, link.source_offset)
            .tag("UG_PART_SEGMENT_OM_LINK");
        annotations.exactness(&link.id, Exactness::ByteExact);
    }
    for area in &om_record_areas {
        annotations
            .note(&area.id, annotation_stream, area.source_offset)
            .tag("OM_RECORD_AREA");
        annotations.exactness(&area.id, Exactness::ByteExact);
    }
    for label in &feature_operation_labels {
        annotations
            .note(&label.id, annotation_stream, label.source_offset)
            .tag("FEATURE_OPERATION_LABEL");
        annotations.exactness(&label.id, Exactness::ByteExact);
    }
    for sketch in &feature_sketch_records {
        annotations
            .note(&sketch.id, annotation_stream, sketch.source_offset)
            .tag("FEATURE_SKETCH_RECORD");
        annotations.exactness(&sketch.id, Exactness::Derived);
    }
    for pair in &feature_sketch_payload_fixed_pairs {
        annotations
            .note(&pair.id, annotation_stream, pair.source_offset)
            .tag("FEATURE_SKETCH_FIXED_PAIR");
        annotations.exactness(&pair.id, Exactness::ByteExact);
    }
    for point in &feature_sketch_fixed_points {
        annotations
            .note(&point.id, annotation_stream, point.source_offset)
            .tag("FEATURE_SKETCH_FIXED_POINT");
        annotations.exactness(&point.id, Exactness::Derived);
    }
    for record in &feature_operation_records {
        annotations
            .note(&record.id, annotation_stream, record.source_offset)
            .tag("FEATURE_OPERATION_RECORD");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for value in &feature_payload_strings {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("FEATURE_PAYLOAD_STRING");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for reference in &feature_body_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("FEATURE_BODY_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for reference in &feature_body_reference_occurrences {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("FEATURE_BODY_REFERENCE_OCCURRENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for input in &feature_input_blocks {
        annotations
            .note(&input.id, annotation_stream, input.source_offset)
            .tag("FEATURE_INPUT_BLOCK");
        annotations.exactness(&input.id, Exactness::ByteExact);
    }
    for operation in &feature_boolean_operations {
        annotations
            .note(&operation.id, annotation_stream, operation.source_offset)
            .tag("FEATURE_BOOLEAN_OPERATION");
        annotations.exactness(&operation.id, Exactness::ByteExact);
    }
    for declaration in &expression_declarations {
        annotations
            .note(
                &declaration.id,
                annotation_stream,
                declaration.source_offset,
            )
            .tag("EXPRESSION_DECLARATION");
        annotations.exactness(&declaration.id, Exactness::ByteExact);
    }
    for value in &data_block_control_values {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_VALUE");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for reference in &data_block_control_class_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_CLASS_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for value in &data_block_control_index_values {
        annotations
            .note(&value.id, annotation_stream, value.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_INDEX_VALUE");
        annotations.exactness(&value.id, Exactness::ByteExact);
    }
    for reference in &data_block_control_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for pair in &data_block_control_handle_pairs {
        annotations
            .note(&pair.id, annotation_stream, pair.source_offset)
            .tag("OM_DATA_BLOCK_CONTROL_HANDLE_PAIR");
        annotations.exactness(&pair.id, Exactness::ByteExact);
    }
    for reference in &data_block_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("OM_DATA_BLOCK_REFERENCE");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for binding in &feature_parameter_bindings {
        annotations
            .note(&binding.id, annotation_stream, binding.source_offset)
            .tag("FEATURE_PARAMETER_BINDING");
        annotations.exactness(&binding.id, Exactness::Derived);
    }
    for parameter_use in &feature_parameter_uses {
        annotations
            .note(
                &parameter_use.id,
                annotation_stream,
                parameter_use.source_offsets[0],
            )
            .tag("FEATURE_PARAMETER_USE");
        annotations.exactness(&parameter_use.id, Exactness::Derived);
    }
    for header in &store_headers {
        annotations
            .note(&header.id, annotation_stream, header.source_offset)
            .tag("OM_STORE_VERSION");
        annotations.exactness(&header.id, Exactness::ByteExact);
    }
    for reference in &external_references {
        annotations
            .note(&reference.id, annotation_stream, reference.source_offset)
            .tag("EXTREFSTREAM_STRING");
        annotations.exactness(&reference.id, Exactness::ByteExact);
    }
    for attribute in &part_attributes {
        annotations
            .note(&attribute.id, annotation_stream, attribute.source_offset)
            .tag("Attribute");
        annotations.exactness(&attribute.id, Exactness::ByteExact);
        let id = AttributeId(format!("{}:neutral", attribute.id));
        annotations
            .note(&id.0, annotation_stream, attribute.source_offset)
            .tag("Attribute");
        annotations.derived(&id.0, "target");
        annotations.derived(&id.0, "name");
        annotations.derived(&id.0, "values");
        ir.model.attributes.push(SourceAttribute {
            id,
            target: AttributeTarget::Document,
            name: attribute.title.clone(),
            values: vec![AttributeValue::String(attribute.value.clone())],
        });
    }
    attach_parasolid_topology_string_attributes(
        ir,
        &parasolid_topology_attribute_list_references,
        &parasolid_topology_attribute_class_uses,
        &parasolid_attribute_definitions,
        &parasolid_entity_51_string_uses,
        &parasolid_entity_54_string_records,
        annotations,
    );
    attach_parasolid_topology_numeric_attributes(
        ir,
        &ParasolidNumericAttributeSources {
            topology_references: &parasolid_topology_attribute_list_references,
            class_uses: &parasolid_topology_attribute_class_uses,
            definitions: &parasolid_attribute_definitions,
            numeric_uses: &parasolid_entity_51_numeric_uses,
            integers: &parasolid_entity_52_integer_records,
            doubles: &parasolid_entity_53_double_records,
        },
        annotations,
    );
    for record in &external_reference_records {
        annotations
            .note(&record.id, annotation_stream, record.source_offset)
            .tag("EXTREFSTREAM_RECORD");
        annotations.exactness(&record.id, Exactness::ByteExact);
    }
    for asset in &material_texture_assets {
        annotations
            .note(&asset.id, annotation_stream, asset.source_offset)
            .tag("TIFF_MATERIAL_TEXTURE");
        annotations.exactness(&asset.id, Exactness::ByteExact);
    }
    for entry in &material_texture_catalog_entries {
        annotations
            .note(&entry.id, annotation_stream, entry.source_offset)
            .tag("QAF_MATERIAL_TEXTURE_CATALOG_ENTRY");
        annotations.exactness(&entry.id, Exactness::Derived);
    }
    let mut unknowns = ir.native_unknowns("nx")?;
    for (section_index, (entry, section)) in object_sections.iter().enumerate() {
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (record_index, record) in section
            .control
            .iter()
            .chain(section.records.iter())
            .enumerate()
        {
            let kind = if record.object_id.is_some() {
                "record"
            } else {
                "block"
            };
            let id = UnknownId(format!(
                "nx:om-section-{section_index}:{kind}#{record_index}"
            ));
            let offset = entry_offset + record.offset as u64;
            annotations
                .note(&id, annotation_stream, offset)
                .tag(if record.object_id.is_some() {
                    "OM_ENTITY_RECORD"
                } else {
                    "OM_DATA_BLOCK"
                });
            annotations.exactness(&id, Exactness::ByteExact);
            unknowns.push(UnknownRecord {
                id,
                offset,
                byte_len: record.bytes.len() as u64,
                sha256: sha256_hex(record.bytes),
                data: Some(record.bytes.to_vec()),
                links: Vec::new(),
            });
        }
    }
    ir.set_native_unknowns("nx", &unknowns)?;
    if !configurations.is_empty() {
        for (ordinal, configuration) in configurations.iter().enumerate() {
            let id = ConfigurationId(format!("nx:arrangements:configuration#{ordinal}"));
            let active_attribute_use = configuration_attribute_uses
                .iter()
                .find(|relation| relation.configuration == configuration.id);
            let bodies: Vec<BodyId> = if active_attribute_use.is_some() {
                ir.model.bodies.iter().map(|body| body.id.clone()).collect()
            } else {
                Vec::new()
            };
            annotations
                .note(&id.0, annotation_stream, configuration.source_offset)
                .tag("Arrangement");
            annotations.derived(&id.0, "ordinal");
            annotations.derived(&id.0, "active");
            annotations.derived(&id.0, "source_index");
            annotations.derived(&id.0, "name");
            annotations.derived(&id.0, "native_ref");
            if !bodies.is_empty() {
                annotations.derived(&id.0, "bodies");
            }
            ir.model.configurations.push(DesignConfiguration {
                id,
                ordinal: ordinal as u32,
                active: configuration.active,
                source_index: Some(ordinal as u32),
                name: configuration.name.clone(),
                material: None,
                properties: active_attribute_use
                    .map(|relation| {
                        BTreeMap::from([("active_attribute_use".to_string(), relation.id.clone())])
                    })
                    .unwrap_or_default(),
                bodies,
                native_ref: Some(configuration.id.clone()),
            });
        }
    }
    attach_feature_operations(
        ir,
        &FeatureOperationSources {
            labels: &feature_operation_labels,
            booleans: &feature_boolean_operations,
            body_references: &feature_body_references,
            body_reference_occurrences: &feature_body_reference_occurrences,
            input_blocks: &feature_input_blocks,
            input_block_identity_groups: &feature_input_block_identity_groups,
            datum_csys_constructions: &feature_datum_csys_constructions,
            datum_plane_headers: &feature_datum_plane_headers,
            datum_plane_block_uses: &feature_datum_plane_block_uses,
            datum_plane_payloads: &feature_datum_plane_payloads,
            datum_plane_csys_identity_uses: &feature_datum_plane_csys_identity_uses,
            sketch_datum_csys_dependencies: &feature_sketch_datum_csys_dependencies,
            sketch_references: &feature_sketch_references,
            sketch_named_point_block_uses: &feature_sketch_named_point_block_uses,
            sketch_preceding_named_point_uses: &feature_sketch_preceding_named_point_uses,
            sketch_point_uses: &feature_sketch_point_uses,
            sketch_point_groups: &feature_sketch_point_groups,
            extrude_profile_references: &feature_extrude_profile_references,
            extrude_construction_profiles: &feature_extrude_construction_profiles,
            operation_body_operands: &feature_operation_body_operands,
            sketch_construction_inputs: &feature_sketch_construction_inputs,
            sketch_coordinate_pairs: &feature_sketch_payload_coordinate_pairs,
            sketch_fixed_pairs: &feature_sketch_payload_fixed_pairs,
            sketch_fixed_points: &feature_sketch_fixed_points,
            block_constructions: &feature_block_constructions,
            block_dimensions: &feature_block_dimensions,
            block_payload_points: &feature_block_payload_points,
            block_payload_point_groups: &feature_block_payload_point_groups,
            extrude_32_constructions: &feature_extrude_32_constructions,
            extrude_payload_headers: &feature_extrude_payload_headers,
            extrude_payload_footers: &feature_extrude_payload_footers,
            operation_body_scalar_triples: &feature_operation_body_scalar_triples,
            parameter_bindings: &feature_parameter_bindings,
            parameter_uses: &feature_parameter_uses,
            expressions: &expressions,
            operation_records: &feature_operation_records,
            payload_strings: &feature_payload_strings,
            simple_hole_templates: &feature_simple_hole_templates,
            simple_hole_repeated_scalar_lanes: &feature_simple_hole_repeated_scalar_lanes,
            simple_hole_repeated_scalar_lane_block_references:
                &feature_simple_hole_repeated_scalar_lane_block_references,
            simple_hole_construction_groups: &feature_simple_hole_construction_groups,
            body_bindings: &segment_body_bindings,
        },
        annotations,
    );
    attach_expression_parameters(
        ir,
        &expressions,
        &expression_declarations,
        &feature_parameter_uses,
        annotations,
    );
    attach_block_dimension_parameter_consumers(ir, &feature_block_dimensions, annotations);
    ir.model
        .features
        .sort_by(|first, second| first.id.cmp(&second.id));
    let namespace = ir.native.namespace_mut("nx");
    namespace.version = namespace.version.max(145);
    if !segment_index_rows.is_empty() {
        namespace.set_arena("segment_index_rows", &segment_index_rows)?;
    }
    if !segment_stream_links.is_empty() {
        namespace.set_arena("segment_stream_links", &segment_stream_links)?;
    }
    if !segment_body_bindings.is_empty() {
        namespace.set_arena("segment_body_bindings", &segment_body_bindings)?;
    }
    if !segment_body_lineage_statuses.is_empty() {
        namespace.set_arena(
            "segment_body_lineage_statuses",
            &segment_body_lineage_statuses,
        )?;
    }
    if !parasolid_blend_surface_records.is_empty() {
        namespace.set_arena(
            "parasolid_blend_surface_records",
            &parasolid_blend_surface_records,
        )?;
    }
    if !parasolid_blend_bound_records.is_empty() {
        namespace.set_arena(
            "parasolid_blend_bound_records",
            &parasolid_blend_bound_records,
        )?;
    }
    if !parasolid_offset_surface_records.is_empty() {
        namespace.set_arena(
            "parasolid_offset_surface_records",
            &parasolid_offset_surface_records,
        )?;
    }
    if !parasolid_trimmed_curve_records.is_empty() {
        namespace.set_arena(
            "parasolid_trimmed_curve_records",
            &parasolid_trimmed_curve_records,
        )?;
    }
    if !parasolid_surface_curve_records.is_empty() {
        namespace.set_arena(
            "parasolid_surface_curve_records",
            &parasolid_surface_curve_records,
        )?;
    }
    if !parasolid_intersection_records.is_empty() {
        namespace.set_arena(
            "parasolid_intersection_records",
            &parasolid_intersection_records,
        )?;
    }
    if !parasolid_term_use_records.is_empty() {
        namespace.set_arena("parasolid_term_use_records", &parasolid_term_use_records)?;
    }
    if !parasolid_support_uv_records.is_empty() {
        namespace.set_arena(
            "parasolid_support_uv_records",
            &parasolid_support_uv_records,
        )?;
    }
    if !parasolid_chart_records.is_empty() {
        namespace.set_arena("parasolid_chart_records", &parasolid_chart_records)?;
    }
    if !parasolid_attribute_definitions.is_empty() {
        namespace.set_arena(
            "parasolid_attribute_definitions",
            &parasolid_attribute_definitions,
        )?;
    }
    if !parasolid_entity_51_records.is_empty() {
        namespace.set_arena("parasolid_entity_51_records", &parasolid_entity_51_records)?;
    }
    if !parasolid_entity_52_integer_records.is_empty() {
        namespace.set_arena(
            "parasolid_entity_52_integer_records",
            &parasolid_entity_52_integer_records,
        )?;
    }
    if !parasolid_entity_53_double_records.is_empty() {
        namespace.set_arena(
            "parasolid_entity_53_double_records",
            &parasolid_entity_53_double_records,
        )?;
    }
    if !parasolid_entity_54_string_records.is_empty() {
        namespace.set_arena(
            "parasolid_entity_54_string_records",
            &parasolid_entity_54_string_records,
        )?;
    }
    if !parasolid_entity_51_string_uses.is_empty() {
        namespace.set_arena(
            "parasolid_entity_51_string_uses",
            &parasolid_entity_51_string_uses,
        )?;
    }
    if !parasolid_entity_51_numeric_uses.is_empty() {
        namespace.set_arena(
            "parasolid_entity_51_numeric_uses",
            &parasolid_entity_51_numeric_uses,
        )?;
    }
    if !parasolid_topology_attribute_list_references.is_empty() {
        namespace.set_arena(
            "parasolid_topology_attribute_list_references",
            &parasolid_topology_attribute_list_references,
        )?;
    }
    if !parasolid_topology_attribute_class_uses.is_empty() {
        namespace.set_arena(
            "parasolid_topology_attribute_class_uses",
            &parasolid_topology_attribute_class_uses,
        )?;
    }
    if !segment_om_links.is_empty() {
        namespace.set_arena("segment_om_links", &segment_om_links)?;
    }
    if !om_record_areas.is_empty() {
        namespace.set_arena("om_record_areas", &om_record_areas)?;
    }
    if !feature_operation_labels.is_empty() {
        namespace.set_arena("feature_operation_labels", &feature_operation_labels)?;
    }
    if !feature_operation_records.is_empty() {
        namespace.set_arena("feature_operation_records", &feature_operation_records)?;
    }
    if !feature_payload_strings.is_empty() {
        namespace.set_arena("feature_payload_strings", &feature_payload_strings)?;
    }
    if !feature_simple_hole_templates.is_empty() {
        namespace.set_arena(
            "feature_simple_hole_templates",
            &feature_simple_hole_templates,
        )?;
    }
    if !feature_simple_hole_repeated_scalar_lanes.is_empty() {
        namespace.set_arena(
            "feature_simple_hole_repeated_scalar_lanes",
            &feature_simple_hole_repeated_scalar_lanes,
        )?;
    }
    if !feature_simple_hole_repeated_scalar_lane_block_references.is_empty() {
        namespace.set_arena(
            "feature_simple_hole_repeated_scalar_lane_block_references",
            &feature_simple_hole_repeated_scalar_lane_block_references,
        )?;
    }
    if !feature_simple_hole_construction_groups.is_empty() {
        namespace.set_arena(
            "feature_simple_hole_construction_groups",
            &feature_simple_hole_construction_groups,
        )?;
    }
    if !feature_body_references.is_empty() {
        namespace.set_arena("feature_body_references", &feature_body_references)?;
    }
    if !feature_body_reference_occurrences.is_empty() {
        namespace.set_arena(
            "feature_body_reference_occurrences",
            &feature_body_reference_occurrences,
        )?;
    }
    if !feature_input_blocks.is_empty() {
        namespace.set_arena("feature_input_blocks", &feature_input_blocks)?;
    }
    if !feature_input_block_identity_groups.is_empty() {
        namespace.set_arena(
            "feature_input_block_identity_groups",
            &feature_input_block_identity_groups,
        )?;
    }
    if !display_jt_indices.is_empty() {
        namespace.set_arena("display_jt_indices", &display_jt_indices)?;
    }
    if !display_jt_documents.is_empty() {
        namespace.set_arena("display_jt_documents", &display_jt_documents)?;
    }
    if !display_jt_segments.is_empty() {
        namespace.set_arena("display_jt_segments", &display_jt_segments)?;
    }
    if !display_jt_shape_lod_elements.is_empty() {
        namespace.set_arena(
            "display_jt_shape_lod_elements",
            &display_jt_shape_lod_elements,
        )?;
    }
    if !display_jt_tri_strip_lod_headers.is_empty() {
        namespace.set_arena(
            "display_jt_tri_strip_lod_headers",
            &display_jt_tri_strip_lod_headers,
        )?;
    }
    if !display_jt_initial_face_degree_symbols.is_empty() {
        namespace.set_arena(
            "display_jt_initial_face_degree_symbols",
            &display_jt_initial_face_degree_symbols,
        )?;
    }
    if !display_jt_topology_packet_sequences.is_empty() {
        namespace.set_arena(
            "display_jt_topology_packet_sequences",
            &display_jt_topology_packet_sequences,
        )?;
    }
    if !display_jt_vertex_records_headers.is_empty() {
        namespace.set_arena(
            "display_jt_vertex_records_headers",
            &display_jt_vertex_records_headers,
        )?;
    }
    if !display_jt_coordinate_array_headers.is_empty() {
        namespace.set_arena(
            "display_jt_coordinate_array_headers",
            &display_jt_coordinate_array_headers,
        )?;
    }
    if !display_jt_vertex_coordinates.is_empty() {
        namespace.set_arena(
            "display_jt_vertex_coordinates",
            &display_jt_vertex_coordinates,
        )?;
    }
    if !display_jt_vertex_normals.is_empty() {
        namespace.set_arena("display_jt_vertex_normals", &display_jt_vertex_normals)?;
    }
    if !display_jt_vertex_colors.is_empty() {
        namespace.set_arena("display_jt_vertex_colors", &display_jt_vertex_colors)?;
    }
    if !display_jt_vertex_texture_coordinates.is_empty() {
        namespace.set_arena(
            "display_jt_vertex_texture_coordinates",
            &display_jt_vertex_texture_coordinates,
        )?;
    }
    if !display_jt_vertex_flags.is_empty() {
        namespace.set_arena("display_jt_vertex_flags", &display_jt_vertex_flags)?;
    }
    if !display_jt_geometric_transform_attributes.is_empty() {
        namespace.set_arena(
            "display_jt_geometric_transform_attributes",
            &display_jt_geometric_transform_attributes,
        )?;
    }
    if !display_jt_polygon_meshes.is_empty() {
        namespace.set_arena("display_jt_polygon_meshes", &display_jt_polygon_meshes)?;
    }
    if !display_jt_compressed_element_sequences.is_empty() {
        namespace.set_arena(
            "display_jt_compressed_element_sequences",
            &display_jt_compressed_element_sequences,
        )?;
    }
    if !display_jt_compressed_elements.is_empty() {
        namespace.set_arena(
            "display_jt_compressed_elements",
            &display_jt_compressed_elements,
        )?;
    }
    if !display_jt_string_property_atoms.is_empty() {
        namespace.set_arena(
            "display_jt_string_property_atoms",
            &display_jt_string_property_atoms,
        )?;
    }
    if !display_jt_shape_lod_bindings.is_empty() {
        namespace.set_arena(
            "display_jt_shape_lod_bindings",
            &display_jt_shape_lod_bindings,
        )?;
    }
    if !display_jt_base_node_data.is_empty() {
        namespace.set_arena("display_jt_base_node_data", &display_jt_base_node_data)?;
    }
    if !display_jt_partition_nodes.is_empty() {
        namespace.set_arena("display_jt_partition_nodes", &display_jt_partition_nodes)?;
    }
    if !display_jt_range_lod_nodes.is_empty() {
        namespace.set_arena("display_jt_range_lod_nodes", &display_jt_range_lod_nodes)?;
    }
    if !display_jt_tri_strip_shape_nodes.is_empty() {
        namespace.set_arena(
            "display_jt_tri_strip_shape_nodes",
            &display_jt_tri_strip_shape_nodes,
        )?;
    }
    if !feature_datum_csys_constructions.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_constructions",
            &feature_datum_csys_constructions,
        )?;
    }
    if !feature_datum_csys_payloads.is_empty() {
        namespace.set_arena("feature_datum_csys_payloads", &feature_datum_csys_payloads)?;
    }
    if !feature_datum_csys_payload_scalar_pairs.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_payload_scalar_pairs",
            &feature_datum_csys_payload_scalar_pairs,
        )?;
    }
    if !feature_datum_csys_payload_fixed_pairs.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_payload_fixed_pairs",
            &feature_datum_csys_payload_fixed_pairs,
        )?;
    }
    if !feature_datum_csys_payload_scalars.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_payload_scalars",
            &feature_datum_csys_payload_scalars,
        )?;
    }
    if !feature_datum_csys_descriptors.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_descriptors",
            &feature_datum_csys_descriptors,
        )?;
    }
    if !feature_datum_csys_block_uses.is_empty() {
        namespace.set_arena(
            "feature_datum_csys_block_uses",
            &feature_datum_csys_block_uses,
        )?;
    }
    if !feature_datum_plane_headers.is_empty() {
        namespace.set_arena("feature_datum_plane_headers", &feature_datum_plane_headers)?;
    }
    if !feature_datum_plane_block_uses.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_block_uses",
            &feature_datum_plane_block_uses,
        )?;
    }
    if !feature_datum_plane_payloads.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_payloads",
            &feature_datum_plane_payloads,
        )?;
    }
    if !feature_datum_plane_payload_scalar_pairs.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_payload_scalar_pairs",
            &feature_datum_plane_payload_scalar_pairs,
        )?;
    }
    if !feature_datum_plane_descriptors.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_descriptors",
            &feature_datum_plane_descriptors,
        )?;
    }
    if !feature_datum_plane_csys_identity_uses.is_empty() {
        namespace.set_arena(
            "feature_datum_plane_csys_identity_uses",
            &feature_datum_plane_csys_identity_uses,
        )?;
    }
    if !feature_sketch_references.is_empty() {
        namespace.set_arena("feature_sketch_references", &feature_sketch_references)?;
    }
    if !feature_extrude_profile_references.is_empty() {
        namespace.set_arena(
            "feature_extrude_profile_references",
            &feature_extrude_profile_references,
        )?;
    }
    if !feature_extrude_payload_headers.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_headers",
            &feature_extrude_payload_headers,
        )?;
    }
    if !feature_extrude_payload_footers.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_footers",
            &feature_extrude_payload_footers,
        )?;
    }
    if !feature_extrude_payload_scalar_triples.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_scalar_triples",
            &feature_extrude_payload_scalar_triples,
        )?;
    }
    if !feature_operation_body_scalar_triples.is_empty() {
        namespace.set_arena(
            "feature_operation_body_scalar_triples",
            &feature_operation_body_scalar_triples,
        )?;
    }
    if !feature_operation_body_members.is_empty() {
        namespace.set_arena(
            "feature_operation_body_members",
            &feature_operation_body_members,
        )?;
    }
    if !feature_operation_body_operands.is_empty() {
        namespace.set_arena(
            "feature_operation_body_operands",
            &feature_operation_body_operands,
        )?;
    }
    if !feature_operation_body_11_continuations.is_empty() {
        namespace.set_arena(
            "feature_operation_body_11_continuations",
            &feature_operation_body_11_continuations,
        )?;
    }
    if !feature_operation_body_reference_lanes.is_empty() {
        namespace.set_arena(
            "feature_operation_body_reference_lanes",
            &feature_operation_body_reference_lanes,
        )?;
    }
    if !feature_extrude_construction_profiles.is_empty() {
        namespace.set_arena(
            "feature_extrude_construction_profiles",
            &feature_extrude_construction_profiles,
        )?;
    }
    if !feature_extrude_payload_32_branches.is_empty() {
        namespace.set_arena(
            "feature_extrude_payload_32_branches",
            &feature_extrude_payload_32_branches,
        )?;
    }
    if !feature_extrude_32_constructions.is_empty() {
        namespace.set_arena(
            "feature_extrude_32_constructions",
            &feature_extrude_32_constructions,
        )?;
    }
    if !feature_block_construction_references.is_empty() {
        namespace.set_arena(
            "feature_block_construction_references",
            &feature_block_construction_references,
        )?;
    }
    if !feature_block_constructions.is_empty() {
        namespace.set_arena("feature_block_constructions", &feature_block_constructions)?;
    }
    if !feature_block_construction_payloads.is_empty() {
        namespace.set_arena(
            "feature_block_construction_payloads",
            &feature_block_construction_payloads,
        )?;
    }
    if !feature_block_payload_scalars.is_empty() {
        namespace.set_arena(
            "feature_block_payload_scalars",
            &feature_block_payload_scalars,
        )?;
    }
    if !feature_block_payload_names.is_empty() {
        namespace.set_arena("feature_block_payload_names", &feature_block_payload_names)?;
    }
    if !feature_block_payload_named_records.is_empty() {
        namespace.set_arena(
            "feature_block_payload_named_records",
            &feature_block_payload_named_records,
        )?;
    }
    if !feature_block_payload_points.is_empty() {
        namespace.set_arena(
            "feature_block_payload_points",
            &feature_block_payload_points,
        )?;
    }
    if !feature_block_payload_point_groups.is_empty() {
        namespace.set_arena(
            "feature_block_payload_point_groups",
            &feature_block_payload_point_groups,
        )?;
    }
    if !feature_block_dimensions.is_empty() {
        namespace.set_arena("feature_block_dimensions", &feature_block_dimensions)?;
    }
    if !feature_sketch_records.is_empty() {
        namespace.set_arena("feature_sketch_records", &feature_sketch_records)?;
    }
    if !feature_sketch_construction_inputs.is_empty() {
        namespace.set_arena(
            "feature_sketch_construction_inputs",
            &feature_sketch_construction_inputs,
        )?;
    }
    if !feature_sketch_construction_payloads.is_empty() {
        namespace.set_arena(
            "feature_sketch_construction_payloads",
            &feature_sketch_construction_payloads,
        )?;
    }
    if !feature_sketch_payload_coordinate_pairs.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_coordinate_pairs",
            &feature_sketch_payload_coordinate_pairs,
        )?;
    }
    if !feature_sketch_payload_fixed_pairs.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_fixed_pairs",
            &feature_sketch_payload_fixed_pairs,
        )?;
    }
    if !feature_sketch_payload_scalars.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_scalars",
            &feature_sketch_payload_scalars,
        )?;
    }
    if !feature_sketch_payload_names.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_names",
            &feature_sketch_payload_names,
        )?;
    }
    if !feature_sketch_payload_named_records.is_empty() {
        namespace.set_arena(
            "feature_sketch_payload_named_records",
            &feature_sketch_payload_named_records,
        )?;
    }
    if !feature_sketch_points.is_empty() {
        namespace.set_arena("feature_sketch_points", &feature_sketch_points)?;
    }
    if !feature_sketch_fixed_points.is_empty() {
        namespace.set_arena("feature_sketch_fixed_points", &feature_sketch_fixed_points)?;
    }
    if !feature_sketch_point_groups.is_empty() {
        namespace.set_arena("feature_sketch_point_groups", &feature_sketch_point_groups)?;
    }
    if !offset_store_named_points.is_empty() {
        namespace.set_arena("offset_store_named_points", &offset_store_named_points)?;
    }
    if !feature_sketch_named_point_block_uses.is_empty() {
        namespace.set_arena(
            "feature_sketch_named_point_block_uses",
            &feature_sketch_named_point_block_uses,
        )?;
    }
    if !feature_sketch_preceding_named_point_uses.is_empty() {
        namespace.set_arena(
            "feature_sketch_preceding_named_point_uses",
            &feature_sketch_preceding_named_point_uses,
        )?;
    }
    if !feature_sketch_point_uses.is_empty() {
        namespace.set_arena("feature_sketch_point_uses", &feature_sketch_point_uses)?;
    }
    if !feature_sketch_datum_csys_dependencies.is_empty() {
        namespace.set_arena(
            "feature_sketch_datum_csys_dependencies",
            &feature_sketch_datum_csys_dependencies,
        )?;
    }
    if !feature_boolean_operations.is_empty() {
        namespace.set_arena("feature_boolean_operations", &feature_boolean_operations)?;
    }
    if !expression_declarations.is_empty() {
        namespace.set_arena("expression_declarations", &expression_declarations)?;
    }
    if !data_block_object_frames.is_empty() {
        namespace.set_arena("data_block_object_frames", &data_block_object_frames)?;
    }
    if !expressions.is_empty() {
        namespace.set_arena("expressions", &expressions)?;
    }
    if !classes.is_empty() {
        namespace.set_arena("class_definitions", &classes)?;
    }
    if !fields.is_empty() {
        namespace.set_arena("field_definitions", &fields)?;
    }
    if !object_records.is_empty() {
        namespace.set_arena("object_records", &object_records)?;
    }
    if !data_blocks.is_empty() {
        namespace.set_arena("data_blocks", &data_blocks)?;
    }
    if !data_block_control_values.is_empty() {
        namespace.set_arena("data_block_control_values", &data_block_control_values)?;
    }
    if !data_block_control_class_references.is_empty() {
        namespace.set_arena(
            "data_block_control_class_references",
            &data_block_control_class_references,
        )?;
    }
    if !data_block_control_index_values.is_empty() {
        namespace.set_arena(
            "data_block_control_index_values",
            &data_block_control_index_values,
        )?;
    }
    if !data_block_control_references.is_empty() {
        namespace.set_arena(
            "data_block_control_references",
            &data_block_control_references,
        )?;
    }
    if !data_block_control_handle_pairs.is_empty() {
        namespace.set_arena(
            "data_block_control_handle_pairs",
            &data_block_control_handle_pairs,
        )?;
    }
    if !data_block_references.is_empty() {
        namespace.set_arena("data_block_references", &data_block_references)?;
    }
    if !data_block_counted_index_lanes.is_empty() {
        namespace.set_arena(
            "data_block_counted_index_lanes",
            &data_block_counted_index_lanes,
        )?;
    }
    if !data_block_abr_reference_lanes.is_empty() {
        namespace.set_arena(
            "data_block_abr_reference_lanes",
            &data_block_abr_reference_lanes,
        )?;
    }
    if !feature_parameter_bindings.is_empty() {
        namespace.set_arena("feature_parameter_bindings", &feature_parameter_bindings)?;
    }
    if !feature_parameter_uses.is_empty() {
        namespace.set_arena("feature_parameter_uses", &feature_parameter_uses)?;
    }
    if !store_headers.is_empty() {
        namespace.set_arena("store_headers", &store_headers)?;
    }
    if !string_values.is_empty() {
        namespace.set_arena("string_values", &string_values)?;
    }
    if !object_references.is_empty() {
        namespace.set_arena("object_references", &object_references)?;
    }
    if !persistent_handles.is_empty() {
        namespace.set_arena("persistent_handles", &persistent_handles)?;
    }
    if !configurations.is_empty() {
        namespace.set_arena("configurations", &configurations)?;
    }
    if !configuration_attribute_uses.is_empty() {
        namespace.set_arena(
            "configuration_attribute_uses",
            &configuration_attribute_uses,
        )?;
    }
    if !part_attributes.is_empty() {
        namespace.set_arena("part_attributes", &part_attributes)?;
    }
    if !external_references.is_empty() {
        namespace.set_arena("external_references", &external_references)?;
    }
    if !external_reference_records.is_empty() {
        namespace.set_arena("external_reference_records", &external_reference_records)?;
    }
    if !material_texture_assets.is_empty() {
        namespace.set_arena("material_texture_assets", &material_texture_assets)?;
    }
    if !material_texture_catalog_entries.is_empty() {
        namespace.set_arena(
            "material_texture_catalog_entries",
            &material_texture_catalog_entries,
        )?;
    }
    Ok(())
}

fn attach_parasolid_topology_string_attributes(
    ir: &mut CadIr,
    topology_references: &[crate::native::ParasolidTopologyAttributeListReference],
    class_uses: &[crate::native::ParasolidTopologyAttributeClassUse],
    definitions: &[crate::native::ParasolidAttributeDefinition],
    string_uses: &[crate::native::ParasolidEntity51StringUse],
    strings: &[crate::native::ParasolidEntity54StringRecord],
    annotations: &mut AnnotationBuilder,
) {
    let class_names = parasolid_topology_attribute_class_names(class_uses, definitions);
    let strings_by_id = strings
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut uses_by_entity =
        BTreeMap::<&str, Vec<&crate::native::ParasolidEntity51StringUse>>::new();
    for string_use in string_uses {
        uses_by_entity
            .entry(string_use.entity_51_record.as_str())
            .or_default()
            .push(string_use);
    }
    for uses in uses_by_entity.values_mut() {
        uses.sort_by_key(|string_use| string_use.reference_ordinal);
    }
    let mut references_by_target =
        BTreeMap::<String, Vec<&crate::native::ParasolidTopologyAttributeListReference>>::new();
    for reference in topology_references {
        let Some(kind) = parasolid_topology_kind(reference.topology_type) else {
            continue;
        };
        references_by_target
            .entry(format!(
                "nx:s{}:{kind}#{}",
                reference.stream_ordinal, reference.topology_xmt
            ))
            .or_default()
            .push(reference);
    }
    let emitted_targets = parasolid_topology_attribute_targets(ir);
    for (target_key, references) in references_by_target {
        let [reference] = references.as_slice() else {
            continue;
        };
        let Some(target) = emitted_targets.get(target_key.as_str()) else {
            continue;
        };
        let Some(entity) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        for string_use in uses_by_entity.get(entity).into_iter().flatten() {
            let Some(string) = strings_by_id.get(string_use.string_record.as_str()) else {
                continue;
            };
            let id = AttributeId(format!(
                "nx:s{}:topology-string-attribute#{}-{}-{}",
                reference.stream_ordinal,
                reference.topology_type,
                reference.topology_xmt,
                string_use.reference_ordinal
            ));
            let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
            annotations
                .note(&id.0, source_stream, string.inflated_offset)
                .tag("ENTITY_54_STRING_ATTRIBUTE");
            annotations.derived(&id.0, "target");
            annotations.derived(&id.0, "name");
            let generic_name = format!(
                "parasolid_type_84_reference_{}",
                string_use.reference_ordinal
            );
            ir.model.attributes.push(SourceAttribute {
                id,
                target: target.clone(),
                name: class_names
                    .get(reference.id.as_str())
                    .map_or(generic_name.clone(), |class_name| {
                        format!("{class_name}.{generic_name}")
                    }),
                values: vec![AttributeValue::String(string.value.clone())],
            });
        }
    }
    ir.model
        .attributes
        .sort_by(|first, second| first.id.0.cmp(&second.id.0));
}

pub(crate) struct ParasolidNumericAttributeSources<'a> {
    pub(crate) topology_references: &'a [crate::native::ParasolidTopologyAttributeListReference],
    pub(crate) class_uses: &'a [crate::native::ParasolidTopologyAttributeClassUse],
    pub(crate) definitions: &'a [crate::native::ParasolidAttributeDefinition],
    pub(crate) numeric_uses: &'a [crate::native::ParasolidEntity51NumericUse],
    pub(crate) integers: &'a [crate::native::ParasolidEntity52IntegerRecord],
    pub(crate) doubles: &'a [crate::native::ParasolidEntity53DoubleRecord],
}

fn parasolid_topology_attribute_class_names<'a>(
    class_uses: &'a [crate::native::ParasolidTopologyAttributeClassUse],
    definitions: &'a [crate::native::ParasolidAttributeDefinition],
) -> BTreeMap<&'a str, &'a str> {
    let definitions = definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    class_uses
        .iter()
        .filter_map(|class_use| {
            Some((
                class_use.topology_attribute_reference.as_str(),
                *definitions.get(class_use.attribute_definition.as_str())?,
            ))
        })
        .collect()
}

fn parasolid_topology_kind(topology_type: u8) -> Option<&'static str> {
    match topology_type {
        13 => Some("shell"),
        14 => Some("face"),
        15 => Some("loop"),
        16 => Some("edge"),
        17 => Some("fin"),
        18 => Some("vertex"),
        _ => None,
    }
}

fn parasolid_topology_attribute_targets(ir: &CadIr) -> BTreeMap<String, AttributeTarget> {
    ir.model
        .shells
        .iter()
        .map(|shell| (shell.id.0.clone(), AttributeTarget::Shell(shell.id.clone())))
        .chain(
            ir.model
                .faces
                .iter()
                .map(|face| (face.id.0.clone(), AttributeTarget::Face(face.id.clone()))),
        )
        .chain(
            ir.model
                .loops
                .iter()
                .map(|loop_| (loop_.id.0.clone(), AttributeTarget::Loop(loop_.id.clone()))),
        )
        .chain(
            ir.model
                .edges
                .iter()
                .map(|edge| (edge.id.0.clone(), AttributeTarget::Edge(edge.id.clone()))),
        )
        .chain(ir.model.coedges.iter().map(|coedge| {
            (
                coedge.id.0.clone(),
                AttributeTarget::Coedge(coedge.id.clone()),
            )
        }))
        .chain(ir.model.vertices.iter().map(|vertex| {
            (
                vertex.id.0.clone(),
                AttributeTarget::Vertex(vertex.id.clone()),
            )
        }))
        .collect()
}

pub(crate) fn attach_parasolid_topology_numeric_attributes(
    ir: &mut CadIr,
    sources: &ParasolidNumericAttributeSources<'_>,
    annotations: &mut AnnotationBuilder,
) {
    let class_names =
        parasolid_topology_attribute_class_names(sources.class_uses, sources.definitions);
    let integers_by_id = sources
        .integers
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let doubles_by_id = sources
        .doubles
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut uses_by_entity =
        BTreeMap::<&str, Vec<&crate::native::ParasolidEntity51NumericUse>>::new();
    for numeric_use in sources.numeric_uses {
        uses_by_entity
            .entry(numeric_use.entity_51_record.as_str())
            .or_default()
            .push(numeric_use);
    }
    for uses in uses_by_entity.values_mut() {
        uses.sort_by_key(|numeric_use| numeric_use.reference_ordinal);
    }
    let mut references_by_target =
        BTreeMap::<String, Vec<&crate::native::ParasolidTopologyAttributeListReference>>::new();
    for reference in sources.topology_references {
        let Some(kind) = parasolid_topology_kind(reference.topology_type) else {
            continue;
        };
        references_by_target
            .entry(format!(
                "nx:s{}:{kind}#{}",
                reference.stream_ordinal, reference.topology_xmt
            ))
            .or_default()
            .push(reference);
    }
    let emitted_targets = parasolid_topology_attribute_targets(ir);

    for (target_key, references) in references_by_target {
        let [reference] = references.as_slice() else {
            continue;
        };
        let Some(target) = emitted_targets.get(target_key.as_str()) else {
            continue;
        };
        let Some(entity) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        for numeric_use in uses_by_entity.get(entity).into_iter().flatten() {
            let (values, source_offset, tag, lane) = match numeric_use.kind {
                crate::native::ParasolidEntity51NumericKind::UnsignedIntegers => {
                    let Some(record) = integers_by_id.get(numeric_use.value_record.as_str()) else {
                        continue;
                    };
                    (
                        record
                            .values
                            .iter()
                            .map(|value| AttributeValue::Integer(i64::from(*value)))
                            .collect(),
                        record.inflated_offset,
                        "ENTITY_52_INTEGER_ATTRIBUTE",
                        "integer",
                    )
                }
                crate::native::ParasolidEntity51NumericKind::Doubles => {
                    let Some(record) = doubles_by_id.get(numeric_use.value_record.as_str()) else {
                        continue;
                    };
                    (
                        record
                            .values
                            .iter()
                            .copied()
                            .map(AttributeValue::Float)
                            .collect(),
                        record.inflated_offset,
                        "ENTITY_53_DOUBLE_ATTRIBUTE",
                        "double",
                    )
                }
            };
            let id = AttributeId(format!(
                "nx:s{}:topology-numeric-attribute#{}-{}-{}",
                reference.stream_ordinal,
                reference.topology_type,
                reference.topology_xmt,
                numeric_use.reference_ordinal
            ));
            let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
            annotations
                .note(&id.0, source_stream, source_offset)
                .tag(tag);
            annotations.derived(&id.0, "target");
            annotations.derived(&id.0, "name");
            let generic_name = format!(
                "parasolid_type_{lane}_reference_{}",
                numeric_use.reference_ordinal
            );
            ir.model.attributes.push(SourceAttribute {
                id,
                target: target.clone(),
                name: class_names
                    .get(reference.id.as_str())
                    .map_or(generic_name.clone(), |class_name| {
                        format!("{class_name}.{generic_name}")
                    }),
                values,
            });
        }
    }
    ir.model
        .attributes
        .sort_by(|first, second| first.id.0.cmp(&second.id.0));
}

#[derive(Clone, Copy)]
struct FeatureOperationSources<'a> {
    labels: &'a [crate::native::FeatureOperationLabel],
    booleans: &'a [crate::native::FeatureBooleanOperation],
    body_references: &'a [crate::native::FeatureBodyReference],
    body_reference_occurrences: &'a [crate::native::FeatureBodyReferenceOccurrence],
    input_blocks: &'a [crate::native::FeatureInputBlock],
    input_block_identity_groups: &'a [crate::native::FeatureInputBlockIdentityGroup],
    datum_csys_constructions: &'a [crate::native::FeatureDatumCsysConstruction],
    datum_plane_headers: &'a [crate::native::FeatureDatumPlaneHeader],
    datum_plane_block_uses: &'a [crate::native::FeatureDatumPlaneBlockUse],
    datum_plane_payloads: &'a [crate::native::FeatureDatumPlanePayload],
    datum_plane_csys_identity_uses: &'a [crate::native::FeatureDatumPlaneCsysIdentityUse],
    sketch_datum_csys_dependencies: &'a [crate::native::FeatureSketchDatumCsysDependency],
    sketch_references: &'a [crate::native::FeatureSketchReference],
    sketch_named_point_block_uses: &'a [crate::native::FeatureSketchNamedPointBlockUse],
    sketch_preceding_named_point_uses: &'a [crate::native::FeatureSketchPrecedingNamedPointUse],
    sketch_point_uses: &'a [crate::native::FeatureSketchPointUse],
    sketch_point_groups: &'a [crate::native::FeatureSketchPointGroup],
    extrude_profile_references: &'a [crate::native::FeatureExtrudeProfileReference],
    extrude_construction_profiles: &'a [crate::native::FeatureExtrudeConstructionProfile],
    operation_body_operands: &'a [crate::native::FeatureOperationBodyOperand],
    sketch_construction_inputs: &'a [crate::native::FeatureSketchConstructionInputs],
    sketch_coordinate_pairs: &'a [crate::native::FeatureSketchPayloadCoordinatePair],
    sketch_fixed_pairs: &'a [crate::native::FeatureSketchPayloadFixedPair],
    sketch_fixed_points: &'a [crate::native::FeatureSketchFixedPoint],
    block_constructions: &'a [crate::native::FeatureBlockConstruction],
    block_dimensions: &'a [crate::native::FeatureBlockDimensions],
    block_payload_points: &'a [crate::native::FeatureBlockPayloadPoint],
    block_payload_point_groups: &'a [crate::native::FeatureBlockPayloadPointGroup],
    extrude_32_constructions: &'a [crate::native::FeatureExtrude32Construction],
    extrude_payload_headers: &'a [crate::native::FeatureExtrudePayloadHeader],
    extrude_payload_footers: &'a [crate::native::FeatureExtrudePayloadFooter],
    operation_body_scalar_triples: &'a [crate::native::FeatureOperationBodyScalarTriple],
    parameter_bindings: &'a [crate::native::FeatureParameterBinding],
    parameter_uses: &'a [crate::native::FeatureParameterUse],
    expressions: &'a [crate::native::Expression],
    operation_records: &'a [crate::native::FeatureOperationRecord],
    payload_strings: &'a [crate::native::FeaturePayloadString],
    simple_hole_templates: &'a [crate::native::FeatureSimpleHoleTemplate],
    simple_hole_repeated_scalar_lanes: &'a [crate::native::FeatureSimpleHoleRepeatedScalarLane],
    simple_hole_repeated_scalar_lane_block_references:
        &'a [crate::native::FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
    simple_hole_construction_groups: &'a [crate::native::FeatureSimpleHoleConstructionGroup],
    body_bindings: &'a [crate::native::SegmentBodyBinding],
}

fn attach_feature_operations(
    ir: &mut CadIr,
    sources: &FeatureOperationSources<'_>,
    annotations: &mut AnnotationBuilder,
) {
    let FeatureOperationSources {
        labels,
        booleans,
        body_references,
        body_reference_occurrences,
        input_blocks,
        input_block_identity_groups,
        datum_csys_constructions,
        datum_plane_headers,
        datum_plane_block_uses,
        datum_plane_payloads,
        datum_plane_csys_identity_uses,
        sketch_datum_csys_dependencies,
        sketch_references,
        sketch_named_point_block_uses,
        sketch_preceding_named_point_uses,
        sketch_point_uses,
        sketch_point_groups,
        extrude_profile_references,
        extrude_construction_profiles,
        operation_body_operands,
        sketch_construction_inputs,
        sketch_coordinate_pairs,
        sketch_fixed_pairs,
        sketch_fixed_points,
        block_constructions,
        block_dimensions,
        block_payload_points,
        block_payload_point_groups,
        extrude_32_constructions,
        extrude_payload_headers,
        extrude_payload_footers,
        operation_body_scalar_triples,
        parameter_bindings,
        parameter_uses,
        expressions,
        operation_records,
        payload_strings,
        simple_hole_templates,
        simple_hole_repeated_scalar_lanes,
        simple_hole_repeated_scalar_lane_block_references,
        simple_hole_construction_groups,
        body_bindings,
    } = *sources;
    let stream = annotations.stream("nx:container");
    let base_ordinal = ir.model.features.len() as u64;
    let booleans = booleans
        .iter()
        .map(|operation| (operation.operation_label.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let body_references = body_references
        .iter()
        .map(|reference| {
            (
                reference.operation_label.as_str(),
                reference.body_object_index,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut body_reference_occurrences_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBodyReferenceOccurrence>>::new();
    for reference in body_reference_occurrences {
        body_reference_occurrences_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut last_body_writer = BTreeMap::<u32, FeatureId>::new();
    let body_alias_roots = crate::native::body_alias_roots(body_bindings).unwrap_or_default();
    let canonical_body =
        |identity: u32| body_alias_roots.get(&identity).copied().unwrap_or(identity);
    let mut input_blocks_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureInputBlock>>::new();
    for input in input_blocks {
        input_blocks_by_operation
            .entry(input.operation_label.as_str())
            .or_default()
            .push(input);
    }
    let input_block_identity_group_by_input = input_block_identity_groups
        .iter()
        .flat_map(|group| {
            group
                .input_blocks
                .iter()
                .map(move |input| (input.as_str(), group.id.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let datum_csys_constructions_by_operation = datum_csys_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let datum_plane_headers_by_operation = datum_plane_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let datum_plane_payloads_by_operation = datum_plane_payloads
        .iter()
        .map(|payload| (payload.operation_label.as_str(), payload))
        .collect::<BTreeMap<_, _>>();
    let mut datum_plane_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureDatumPlaneBlockUse>>::new();
    for block_use in datum_plane_block_uses {
        datum_plane_uses_by_input_operation
            .entry(block_use.input_operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let operation_positions = labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let sketch_datum_csys_dependencies = sketch_datum_csys_dependencies
        .iter()
        .map(|dependency| (dependency.datum_csys_operation_label.as_str(), dependency))
        .collect::<BTreeMap<_, _>>();
    let mut datum_identity_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureDatumPlaneCsysIdentityUse>>::new();
    for identity_use in datum_plane_csys_identity_uses {
        datum_identity_uses_by_operation
            .entry(identity_use.datum_plane_operation_label.as_str())
            .or_default()
            .push(identity_use);
        datum_identity_uses_by_operation
            .entry(identity_use.datum_csys_operation_label.as_str())
            .or_default()
            .push(identity_use);
    }
    let mut sketch_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchReference>>::new();
    for reference in sketch_references {
        sketch_references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut sketch_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchNamedPointBlockUse>>::new();
    for block_use in sketch_named_point_block_uses {
        sketch_named_point_uses_by_operation
            .entry(block_use.operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let mut sketch_preceding_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPrecedingNamedPointUse>>::new();
    for point_use in sketch_preceding_named_point_uses {
        sketch_preceding_named_point_uses_by_operation
            .entry(point_use.operation_label.as_str())
            .or_default()
            .push(point_use);
    }
    let mut sketch_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPointUse>>::new();
    for point_use in sketch_point_uses {
        sketch_point_uses_by_operation
            .entry(point_use.operation_label.as_str())
            .or_default()
            .push(point_use);
    }
    let mut sketch_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPointGroup>>::new();
    for group in sketch_point_groups {
        sketch_point_groups_by_operation
            .entry(group.operation_label.as_str())
            .or_default()
            .push(group);
    }
    let mut extrude_profile_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureExtrudeProfileReference>>::new();
    for reference in extrude_profile_references {
        extrude_profile_references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let extrude_construction_profiles_by_operation = extrude_construction_profiles
        .iter()
        .map(|profile| (profile.operation_label.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let mut operation_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands {
        operation_body_operands_by_operation
            .entry(operand.operation_label.as_str())
            .or_default()
            .push(operand);
    }
    let sketch_construction_inputs_by_operation = sketch_construction_inputs
        .iter()
        .map(|inputs| (inputs.operation_label.as_str(), inputs))
        .collect::<BTreeMap<_, _>>();
    let mut sketch_coordinate_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPayloadCoordinatePair>>::new();
    for pair in sketch_coordinate_pairs {
        sketch_coordinate_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_fixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPayloadFixedPair>>::new();
    for pair in sketch_fixed_pairs {
        sketch_fixed_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_fixed_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchFixedPoint>>::new();
    for point in sketch_fixed_points {
        sketch_fixed_points_by_operation
            .entry(point.operation_label.as_str())
            .or_default()
            .push(point);
    }
    let block_constructions_by_operation = block_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let block_dimensions_by_operation = block_dimensions
        .iter()
        .map(|dimensions| (dimensions.operation_label.as_str(), dimensions))
        .collect::<BTreeMap<_, _>>();
    let mut block_payload_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBlockPayloadPoint>>::new();
    for point in block_payload_points {
        block_payload_points_by_operation
            .entry(point.operation_label.as_str())
            .or_default()
            .push(point);
    }
    let mut block_payload_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBlockPayloadPointGroup>>::new();
    for group in block_payload_point_groups {
        block_payload_point_groups_by_operation
            .entry(group.operation_label.as_str())
            .or_default()
            .push(group);
    }
    let extrude_32_constructions_by_operation = extrude_32_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let extrude_payload_headers_by_operation = extrude_payload_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let extrude_payload_footers_by_operation = extrude_payload_footers
        .iter()
        .map(|footer| (footer.operation_label.as_str(), footer))
        .collect::<BTreeMap<_, _>>();
    let mut operation_body_scalar_triples_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyScalarTriple>>::new();
    for triple in operation_body_scalar_triples {
        operation_body_scalar_triples_by_operation
            .entry(triple.operation_label.as_str())
            .or_default()
            .push(triple);
    }
    for triples in operation_body_scalar_triples_by_operation.values_mut() {
        triples.sort_by_key(|triple| triple.body_reference_ordinal);
    }
    let simple_hole_diameters =
        simple_hole_diameters(ir, simple_hole_templates, simple_hole_construction_groups);
    let simple_hole_chamfers = simple_hole_chamfers(ir, simple_hole_templates);
    let mut parameter_bindings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureParameterBinding>>::new();
    for binding in parameter_bindings {
        parameter_bindings_by_operation
            .entry(binding.operation_label.as_str())
            .or_default()
            .push(binding);
    }
    let mut parameter_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        parameter_uses_by_operation
            .entry(parameter_use.operation_label.as_str())
            .or_default()
            .push(parameter_use);
    }
    let operation_labels_by_record = operation_records
        .iter()
        .map(|record| (record.id.as_str(), record.operation_label.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut payload_strings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeaturePayloadString>>::new();
    for value in payload_strings {
        let Some(operation) = operation_labels_by_record.get(value.operation_record.as_str())
        else {
            continue;
        };
        payload_strings_by_operation
            .entry(operation)
            .or_default()
            .push(value);
    }
    let mut bodies_by_object_index = BTreeMap::<u32, Vec<BodyId>>::new();
    for binding in body_bindings {
        let prefix = format!("nx:s{}:", binding.stream_ordinal);
        let mut stream_bodies = Vec::new();
        for body in ir
            .model
            .bodies
            .iter()
            .filter(|body| body.id.0.starts_with(&prefix))
        {
            if !stream_bodies.contains(&body.id) {
                stream_bodies.push(body.id.clone());
            }
        }
        for identity in [binding.body_object_index, binding.body_alias_object_index] {
            let bodies = bodies_by_object_index.entry(identity).or_default();
            for body in &stream_bodies {
                if !bodies.contains(body) {
                    bodies.push(body.clone());
                }
            }
        }
    }
    for (ordinal, label) in labels.iter().enumerate() {
        let key = label.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let id = FeatureId(format!("nx:feature-history:feature#{key}"));
        let mut dependencies = Vec::new();
        if let Some(body) = body_references.get(label.id.as_str()) {
            if let Some(writer) = last_body_writer.get(&canonical_body(*body)) {
                dependencies.push(writer.clone());
            }
        }
        if let Some(operation) = booleans.get(label.id.as_str()) {
            for body in &operation.tool_object_indices {
                if let Some(writer) = last_body_writer.get(&canonical_body(*body)) {
                    if !dependencies.contains(writer) {
                        dependencies.push(writer.clone());
                    }
                }
            }
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            if let Some(writer) =
                last_body_writer.get(&canonical_body(operand.operand_object_index))
            {
                if !dependencies.contains(writer) {
                    dependencies.push(writer.clone());
                }
            }
        }
        for block_use in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let Some(construction_position) =
                operation_positions.get(block_use.construction_operation_label.as_str())
            else {
                continue;
            };
            if *construction_position >= ordinal {
                continue;
            }
            let construction_key = block_use
                .construction_operation_label
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            let dependency = FeatureId(format!("nx:feature-history:feature#{construction_key}"));
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        for identity_use in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let other = if identity_use.datum_plane_operation_label == label.id {
                identity_use.datum_csys_operation_label.as_str()
            } else {
                identity_use.datum_plane_operation_label.as_str()
            };
            let Some(other_position) = operation_positions.get(other) else {
                continue;
            };
            if *other_position >= ordinal {
                continue;
            }
            let other_key = other.rsplit_once('#').map_or("unknown", |(_, key)| key);
            let dependency = FeatureId(format!("nx:feature-history:feature#{other_key}"));
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            let sketch_key = dependency
                .sketch_operation_label
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            let dependency = FeatureId(format!("nx:feature-history:feature#{sketch_key}"));
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        let mut source_properties = BTreeMap::new();
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            source_properties.insert(
                "sketch_point_dependency_use".to_string(),
                dependency.sketch_point_use.clone(),
            );
            match &dependency.block_relation {
                crate::native::FeatureSketchDatumCsysBlockRelation::Shared { data_block } => {
                    source_properties.insert(
                        "sketch_point_dependency_shared_block".to_string(),
                        data_block.clone(),
                    );
                }
                crate::native::FeatureSketchDatumCsysBlockRelation::Consecutive {
                    point_data_block,
                    construction_data_block,
                } => {
                    source_properties.insert(
                        "sketch_point_dependency_point_block".to_string(),
                        point_data_block.clone(),
                    );
                    source_properties.insert(
                        "sketch_point_dependency_construction_block".to_string(),
                        construction_data_block.clone(),
                    );
                }
            }
            source_properties.insert(
                "sketch_datum_csys_dependency".to_string(),
                dependency.id.clone(),
            );
        }
        let outputs = body_references
            .get(label.id.as_str())
            .map_or_else(Vec::new, |body| {
                feature_body_outputs(*body, &bodies_by_object_index)
            });
        if let Some(body) = body_references.get(label.id.as_str()) {
            source_properties.insert("primary_body_object_index".to_string(), body.to_string());
        }
        for reference in body_reference_occurrences_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("body_reference.{}", reference.ordinal),
                reference.body_object_index.to_string(),
            );
        }
        if let Some(inputs) = sketch_construction_inputs_by_operation.get(label.id.as_str()) {
            source_properties.insert("sketch_construction_inputs".to_string(), inputs.id.clone());
        }
        for pair in sketch_coordinate_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_coordinate_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for pair in sketch_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_fixed_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for (ordinal, point) in sketch_fixed_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_fixed_point.{ordinal}"), point.id.clone());
        }
        if let Some(construction) = block_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert("block_construction".to_string(), construction.id.clone());
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            source_properties.insert("block_dimensions".to_string(), dimensions.id.clone());
            for (dimension_ordinal, (declaration, expression)) in dimensions
                .declarations
                .iter()
                .zip(&dimensions.expressions)
                .enumerate()
            {
                source_properties.insert(
                    format!("block_dimension_declaration.{dimension_ordinal}"),
                    declaration.clone(),
                );
                source_properties.insert(
                    format!("block_dimension_expression.{dimension_ordinal}"),
                    expression.clone(),
                );
            }
        }
        for (ordinal, point) in block_payload_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("block_payload_point.{ordinal}"), point.id.clone());
        }
        for (ordinal, group) in block_payload_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("block_payload_point_group.{ordinal}"),
                group.id.clone(),
            );
        }
        if let Some(construction) = extrude_32_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "extrude_32_construction".to_string(),
                construction.id.clone(),
            );
        }
        if let Some(header) = extrude_payload_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("extrude_payload_header".to_string(), header.id.clone());
        }
        if let Some(footer) = extrude_payload_footers_by_operation.get(label.id.as_str()) {
            source_properties.insert("extrude_payload_footer".to_string(), footer.id.clone());
        }
        for triple in operation_body_scalar_triples_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_scalar_triple.{}",
                    triple.body_reference_ordinal
                ),
                triple.id.clone(),
            );
        }
        if let Some(construction) = datum_csys_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "datum_csys_construction".to_string(),
                construction.id.clone(),
            );
        }
        if let Some(header) = datum_plane_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("datum_plane_header".to_string(), header.id.clone());
        }
        if let Some(payload) = datum_plane_payloads_by_operation.get(label.id.as_str()) {
            source_properties.insert("datum_plane_payload".to_string(), payload.id.clone());
        }
        for (ordinal, identity_use) in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_identity_use.{ordinal}"),
                identity_use.id.clone(),
            );
        }
        for block_use in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "datum_plane_input.{}.{}",
                    block_use.input_slot, block_use.reference_ordinal
                ),
                block_use.datum_plane_header.clone(),
            );
        }
        source_properties.extend(simple_hole_native_properties(
            &label.id,
            simple_hole_templates,
            simple_hole_repeated_scalar_lanes,
            simple_hole_repeated_scalar_lane_block_references,
            simple_hole_construction_groups,
        ));
        for (slot, value) in label.object_indices.iter().enumerate() {
            source_properties.insert(
                format!("object_index.{slot}"),
                value.map_or_else(|| "null".to_string(), |value| value.to_string()),
            );
        }
        for input in input_blocks_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("input_block.{}", input.input_slot),
                input.data_block.clone(),
            );
            if let Some(group) = input_block_identity_group_by_input.get(input.id.as_str()) {
                source_properties.insert(
                    format!("input_block_identity_group.{}", input.input_slot),
                    (*group).to_string(),
                );
            }
        }
        for reference in sketch_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for (ordinal, block_use) in sketch_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("sketch_named_point_block_use.{ordinal}"),
                block_use.id.clone(),
            );
        }
        for (ordinal, point_use) in sketch_preceding_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("sketch_preceding_named_point_use.{ordinal}"),
                point_use.id.clone(),
            );
        }
        for (ordinal, point_use) in sketch_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_point_use.{ordinal}"), point_use.id.clone());
        }
        for (ordinal, group) in sketch_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_point_group.{ordinal}"), group.id.clone());
        }
        for reference in extrude_profile_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("extrude_profile_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        if let Some(profile) = extrude_construction_profiles_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "extrude_construction_profile".to_string(),
                profile.id.clone(),
            );
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("operation_body_operand.{}", operand.ordinal),
                operand.operand_object_index.to_string(),
            );
        }
        for binding in parameter_bindings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "input_parameter_declaration.{}.{}",
                    binding.input_slot, binding.reference_ordinal
                ),
                binding.expression_declaration.clone(),
            );
            if let Some(expression) = &binding.expression {
                source_properties.insert(
                    format!(
                        "input_parameter_expression.{}.{}",
                        binding.input_slot, binding.reference_ordinal
                    ),
                    expression.clone(),
                );
            }
        }
        for (ordinal, parameter_use) in parameter_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("parameter_use.{ordinal}"), parameter_use.id.clone());
        }
        let operation_payload_string_records = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let operation_payload_strings = operation_payload_string_records
            .iter()
            .map(|value| value.value.as_str())
            .collect::<Vec<_>>();
        let block_dimension_values = block_dimensions_by_operation
            .get(label.id.as_str())
            .map(|dimensions| dimensions.values);
        let block_placement =
            block_dimension_values.and_then(|dimensions| block_placement(ir, dimensions));
        let sew_projection = (label.value == "SEW")
            .then(|| {
                sew_body_feature_definition(
                    operation_body_operands_by_operation
                        .get(label.id.as_str())?
                        .as_slice(),
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let trim_body_projection = (label.value == "TRIM BODY")
            .then(|| {
                trim_body_feature_definition(
                    *body_references.get(label.id.as_str())?,
                    operation_body_operands_by_operation
                        .get(label.id.as_str())?
                        .as_slice(),
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let offset_projection = (label.value == "OFFSET")
            .then(|| offset_surface_feature_definition(ir, &outputs))
            .flatten();
        if let Some((_, supports)) = &offset_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                source_properties.insert(
                    format!("offset_support_surface.{support_ordinal}"),
                    support.0.clone(),
                );
            }
        }
        let blend_projection = (label.value == "BLEND")
            .then(|| blend_feature_definition(ir, &outputs))
            .flatten();
        if let Some((_, surfaces)) = &blend_projection {
            for (surface_ordinal, surface) in surfaces.iter().enumerate() {
                source_properties.insert(
                    format!("blend_result_surface.{surface_ordinal}"),
                    surface.0.clone(),
                );
            }
        }
        let extrude_projection = (label.value == "EXTRUDE")
            .then(|| {
                let body = body_references.get(label.id.as_str())?;
                let output_kinds = outputs
                    .iter()
                    .filter_map(|output| {
                        ir.model
                            .bodies
                            .iter()
                            .find(|body| body.id == *output)
                            .map(|body| body.kind)
                    })
                    .collect::<Vec<_>>();
                let op = extrude_boolean_op(
                    last_body_writer.contains_key(&canonical_body(*body)),
                    &output_kinds,
                );
                extrude_feature_definition(
                    extrude_construction_profiles_by_operation
                        .get(label.id.as_str())
                        .map(|profile| profile.id.as_str()),
                    extrude_32_constructions_by_operation
                        .get(label.id.as_str())
                        .map(|construction| construction.id.as_str()),
                    op,
                )
            })
            .flatten();
        let operation_parameter_uses = parameter_uses_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let native_parameters = native_feature_parameters(operation_parameter_uses, expressions);
        let definition = booleans.get(label.id.as_str()).map_or_else(
            || {
                trim_body_projection
                    .or(sew_projection)
                    .or(extrude_projection)
                    .or_else(|| blend_projection.map(|(definition, _)| definition))
                    .or_else(|| offset_projection.map(|(definition, _)| definition))
                    .unwrap_or_else(|| {
                        non_boolean_feature_definition_with_parameters(
                            &label.value,
                            &operation_payload_strings,
                            block_dimension_values,
                            block_placement,
                            simple_hole_diameters.get(label.id.as_str()).copied(),
                            simple_hole_chamfers.get(label.id.as_str()).copied(),
                            native_parameters,
                        )
                    })
            },
            |operation| FeatureDefinition::Combine {
                target: feature_body_selection(
                    &[operation.target_object_index],
                    &bodies_by_object_index,
                    format!("nx:om-object-index#{}", operation.target_object_index),
                ),
                tools: feature_body_selection(
                    &operation.tool_object_indices,
                    &bodies_by_object_index,
                    format!(
                        "nx:om-object-indices#{}",
                        operation
                            .tool_object_indices
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                ),
                op: match operation.kind {
                    crate::native::FeatureBooleanKind::Unite => BooleanOp::Join,
                    crate::native::FeatureBooleanKind::Subtract => BooleanOp::Cut,
                    crate::native::FeatureBooleanKind::Intersect => BooleanOp::Intersect,
                },
            },
        );
        annotations
            .note(&id, stream, label.source_offset)
            .tag("FEATURE_OPERATION");
        annotations.exactness(&id, Exactness::Derived);
        let mut source_content =
            feature_source_content(operation_payload_string_records, operation_parameter_uses);
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            append_feature_expression_content(&mut source_content, &dimensions.expressions);
        }
        if !source_content.is_empty() {
            annotations.derived(&id, "source_content");
        }
        ir.model.features.push(Feature {
            id: id.clone(),
            ordinal: base_ordinal + ordinal as u64,
            name: Some(label.value.clone()),
            suppressed: false,
            parent: None,
            dependencies,
            source_properties,
            source_tag: Some(label.value.clone()),
            source_text: None,
            source_content,
            outputs,
            definition,
            native_ref: Some(label.id.clone()),
        });
        if let Some(body) = body_references.get(label.id.as_str()) {
            last_body_writer.insert(canonical_body(*body), id);
        }
    }
}

pub(crate) fn extrude_feature_definition(
    construction_profile: Option<&str>,
    structured_construction: Option<&str>,
    op: BooleanOp,
) -> Option<FeatureDefinition> {
    let constructions = [construction_profile, structured_construction]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let [construction] = constructions.as_slice() else {
        return None;
    };
    Some(FeatureDefinition::Extrude {
        profile: ProfileRef::Native((*construction).to_string()),
        direction: None,
        extent: Extent::Unresolved,
        op,
        draft: None,
    })
}

pub(crate) fn extrude_boolean_op(
    has_previous_writer: bool,
    output_kinds: &[cadmpeg_ir::topology::BodyKind],
) -> BooleanOp {
    if !has_previous_writer && output_kinds == [cadmpeg_ir::topology::BodyKind::Solid] {
        BooleanOp::NewBody
    } else {
        BooleanOp::Unresolved
    }
}

pub(crate) fn blend_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let [body] = outputs else {
        return None;
    };
    let (prefix, _) = body.0.rsplit_once("body#")?;
    let mut surfaces = Vec::new();
    let mut laws = Vec::new();
    for procedural in &ir.model.procedural_surfaces {
        if !procedural.surface.0.starts_with(prefix) {
            continue;
        }
        let ProceduralSurfaceDefinition::Blend {
            radius,
            cross_section,
            ..
        } = &procedural.definition
        else {
            continue;
        };
        if *cross_section != BlendCrossSection::Circular {
            return None;
        }
        surfaces.push(procedural.surface.clone());
        laws.push(radius);
    }
    if laws.is_empty() {
        return None;
    }
    surfaces.sort();
    let constant_radii = laws
        .iter()
        .map(|law| match law {
            BlendRadiusLaw::Constant { signed_radius }
                if signed_radius.is_finite() && *signed_radius != 0.0 =>
            {
                Some(signed_radius.abs())
            }
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    let radius = constant_radii
        .as_ref()
        .filter(|radii| {
            radii
                .iter()
                .all(|radius| radius.to_bits() == radii[0].to_bits())
        })
        .map_or_else(
            || RadiusSpec::Unresolved {
                form: if constant_radii.is_some() {
                    Some(RadiusForm::Constant)
                } else if laws.iter().all(|law| {
                    matches!(
                        law,
                        BlendRadiusLaw::Linear { .. } | BlendRadiusLaw::Law { .. }
                    )
                }) {
                    Some(RadiusForm::Variable)
                } else {
                    None
                },
            },
            |radii| RadiusSpec::Constant {
                radius: Length(radii[0]),
            },
        );
    Some((
        FeatureDefinition::Fillet {
            edges: EdgeSelection::Unresolved,
            radius,
        },
        surfaces,
    ))
}

pub(crate) fn offset_surface_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let [body] = outputs else {
        return None;
    };
    let (prefix, _) = body.0.rsplit_once("body#")?;
    let mut distance = None::<f64>;
    let mut supports = Vec::new();
    for procedural in &ir.model.procedural_surfaces {
        if !procedural.surface.0.starts_with(prefix) {
            continue;
        }
        let ProceduralSurfaceDefinition::Offset {
            support,
            distance: candidate,
            ..
        } = &procedural.definition
        else {
            continue;
        };
        if distance.is_some_and(|distance| distance.to_bits() != candidate.to_bits()) {
            return None;
        }
        distance = Some(*candidate);
        if !supports.contains(support) {
            supports.push(support.clone());
        }
    }
    let distance = distance?;
    supports.sort();
    Some((
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Native(format!("{prefix}offset-support-surfaces")),
            distance: Length(distance),
        },
        supports,
    ))
}

pub(crate) fn feature_source_content(
    payload_strings: &[&crate::native::FeaturePayloadString],
    parameter_uses: &[&crate::native::FeatureParameterUse],
) -> Vec<FeatureSourceContent> {
    let mut content = payload_strings
        .iter()
        .map(|value| {
            (
                value.source_offset,
                FeatureSourceContent::Text(value.value.clone()),
            )
        })
        .collect::<Vec<_>>();
    for parameter_use in parameter_uses {
        let Some(parameter) = expression_parameter_id(&parameter_use.expression) else {
            continue;
        };
        content.extend(
            parameter_use
                .source_offsets
                .iter()
                .map(|offset| (*offset, FeatureSourceContent::Parameter(parameter.clone()))),
        );
    }
    content.sort_by_key(|(offset, _)| *offset);
    content.into_iter().map(|(_, content)| content).collect()
}

pub(crate) fn append_feature_expression_content<const N: usize>(
    content: &mut Vec<FeatureSourceContent>,
    expressions: &[String; N],
) {
    for expression in expressions {
        let Some(parameter) = expression_parameter_id(expression) else {
            continue;
        };
        let item = FeatureSourceContent::Parameter(parameter);
        if !content.contains(&item) {
            content.push(item);
        }
    }
}

pub(crate) fn simple_hole_native_properties(
    operation_label: &str,
    templates: &[crate::native::FeatureSimpleHoleTemplate],
    repeated_lanes: &[crate::native::FeatureSimpleHoleRepeatedScalarLane],
    block_references: &[crate::native::FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
    construction_groups: &[crate::native::FeatureSimpleHoleConstructionGroup],
) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    if let Some(template) = templates
        .iter()
        .find(|template| template.operation_label == operation_label)
    {
        properties.insert("simple_hole_template".to_string(), template.id.clone());
    }
    if let Some(pair) = repeated_lanes
        .iter()
        .find(|pair| pair.operation_label == operation_label)
    {
        properties.insert(
            "simple_hole_repeated_scalar_lane".to_string(),
            pair.id.clone(),
        );
    }
    if let Some(references) = block_references
        .iter()
        .find(|references| references.operation_label == operation_label)
    {
        properties.insert(
            "simple_hole_repeated_scalar_lane_block_references".to_string(),
            references.id.clone(),
        );
    }
    if let Some(group) = construction_groups.iter().find(|group| {
        group
            .operation_labels
            .iter()
            .any(|label| label == operation_label)
    }) {
        properties.insert(
            "simple_hole_construction_group".to_string(),
            group.id.clone(),
        );
    }
    properties
}

pub(crate) fn block_placement(ir: &CadIr, dimensions: [f64; 3]) -> Option<Transform> {
    #[derive(Clone, Copy)]
    struct PlaneBand {
        normal: Vector3,
        minimum: f64,
        maximum: f64,
    }

    fn canonical_normal(mut normal: Vector3, angular_tolerance: f64) -> Option<Vector3> {
        normal = unit_vector(normal)?;
        let leading = [normal.x, normal.y, normal.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)?;
        if leading < 0.0 {
            normal = Vector3::new(-normal.x, -normal.y, -normal.z);
        }
        Some(normal)
    }

    let linear_tolerance = ir.tolerances.linear;
    let angular_tolerance = ir.tolerances.angular;
    if dimensions
        .iter()
        .any(|dimension| !dimension.is_finite() || *dimension <= linear_tolerance)
    {
        return None;
    }
    let region_bodies = ir
        .model
        .regions
        .iter()
        .map(|region| (&region.id, &region.body))
        .collect::<BTreeMap<_, _>>();
    let shell_bodies = ir
        .model
        .shells
        .iter()
        .filter_map(|shell| Some((&shell.id, *region_bodies.get(&shell.region)?)))
        .collect::<BTreeMap<_, _>>();
    let surface_geometry = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let mut candidates = Vec::new();
    for body in &ir.model.bodies {
        let mut bands = Vec::<PlaneBand>::new();
        for face in ir.model.faces.iter().filter(|face| {
            shell_bodies
                .get(&face.shell)
                .is_some_and(|owner| **owner == body.id)
        }) {
            let Some(SurfaceGeometry::Plane { origin, normal, .. }) =
                surface_geometry.get(&face.surface).copied()
            else {
                continue;
            };
            let Some(normal) = canonical_normal(*normal, angular_tolerance) else {
                continue;
            };
            let offset = normal.x * origin.x + normal.y * origin.y + normal.z * origin.z;
            let existing = bands
                .iter_mut()
                .find(|band| (1.0 - dot_vector(band.normal, normal)).abs() <= angular_tolerance);
            if let Some(band) = existing {
                band.minimum = band.minimum.min(offset);
                band.maximum = band.maximum.max(offset);
            } else {
                bands.push(PlaneBand {
                    normal,
                    minimum: offset,
                    maximum: offset,
                });
            }
        }
        if bands.len() != 3
            || bands.iter().any(|band| {
                !band.minimum.is_finite()
                    || !band.maximum.is_finite()
                    || band.maximum - band.minimum <= linear_tolerance
            })
            || (0..3).any(|first| {
                (first + 1..3).any(|second| {
                    dot_vector(bands[first].normal, bands[second].normal).abs() > angular_tolerance
                })
            })
        {
            continue;
        }
        let permutations = [
            [0usize, 1usize, 2usize],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let matches = permutations
            .into_iter()
            .filter(|permutation| {
                (0..3).all(|axis| {
                    let band = bands[permutation[axis]];
                    ((band.maximum - band.minimum) - dimensions[axis]).abs() <= linear_tolerance
                })
            })
            .collect::<Vec<_>>();
        let [permutation] = matches.as_slice() else {
            continue;
        };
        let ordered = permutation.map(|index| bands[index]);
        let origin = Point3::new(
            ordered
                .iter()
                .map(|band| band.minimum * band.normal.x)
                .sum(),
            ordered
                .iter()
                .map(|band| band.minimum * band.normal.y)
                .sum(),
            ordered
                .iter()
                .map(|band| band.minimum * band.normal.z)
                .sum(),
        );
        let [x_axis, y_axis, z_axis] = ordered.map(|band| band.normal);
        candidates.push(Transform {
            rows: [
                [x_axis.x, y_axis.x, z_axis.x, origin.x],
                [x_axis.y, y_axis.y, z_axis.y, origin.y],
                [x_axis.z, y_axis.z, z_axis.z, origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        });
    }
    let [placement] = candidates.as_slice() else {
        return None;
    };
    Some(*placement)
}

#[cfg(test)]
pub(crate) fn non_boolean_feature_definition(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole_diameter: Option<Length>,
) -> FeatureDefinition {
    non_boolean_feature_definition_with_parameters(
        kind,
        payload_strings,
        block_dimensions,
        block_placement,
        hole_diameter,
        None,
        BTreeMap::new(),
    )
}

pub(crate) fn non_boolean_feature_definition_with_parameters(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole_diameter: Option<Length>,
    hole_chamfer: Option<HoleKind>,
    native_parameters: BTreeMap<String, String>,
) -> FeatureDefinition {
    if let ("BLOCK", Some(dimensions)) = (kind, block_dimensions) {
        return FeatureDefinition::Block {
            dimensions: dimensions.map(Length),
            placement: block_placement,
        };
    }
    match kind {
        "SKETCH" => FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: None,
        },
        "SIMPLE HOLE" => FeatureDefinition::Hole {
            face: None,
            position: None,
            direction: None,
            kind: hole_chamfer.unwrap_or_else(|| simple_hole_kind(payload_strings)),
            exit_kind: hole_chamfer.or_else(|| simple_hole_exit_kind(payload_strings)),
            diameter: hole_diameter,
            extent: simple_hole_extent(payload_strings),
        },
        "HOLE PACKAGE" => FeatureDefinition::Hole {
            face: None,
            position: None,
            direction: None,
            kind: HoleKind::Unresolved {
                form: None,
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            },
            exit_kind: None,
            diameter: None,
            extent: None,
        },
        "RIB" => FeatureDefinition::Rib {
            construction: RibConstruction {
                profile: None,
                direction: None,
                thickness: None,
                side: None,
                draft: RibDraft::Unresolved,
            },
            op: BooleanOp::Unresolved,
        },
        "CHAMFER" => FeatureDefinition::Chamfer {
            edges: EdgeSelection::Unresolved,
            spec: ChamferSpec::Unresolved { form: None },
        },
        "THICKEN_SHEET" => FeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        },
        "Pattern Feature" | "Pattern Geometry" => FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved { form: None },
        },
        _ => FeatureDefinition::Native {
            kind: kind.to_string(),
            parameters: native_parameters,
            properties: BTreeMap::new(),
        },
    }
}

pub(crate) fn native_feature_parameters(
    uses: &[&crate::native::FeatureParameterUse],
    expressions: &[crate::native::Expression],
) -> BTreeMap<String, String> {
    let by_id = expressions
        .iter()
        .map(|expression| (expression.id.as_str(), expression))
        .collect::<BTreeMap<_, _>>();
    let mut parameters = BTreeMap::new();
    for parameter_use in uses {
        let Some(expression) = by_id.get(parameter_use.expression.as_str()) else {
            return BTreeMap::new();
        };
        if parameters
            .insert(expression.name.clone(), expression.expression.clone())
            .is_some()
        {
            return BTreeMap::new();
        }
    }
    parameters
}

/// Derive a shared simple-hole diameter only when the active B-rep supplies a
/// complete bijection between simple through-hole operations and through-bore
/// cylinder walls. A native construction group establishes the operation set
/// when present. Without a group, a uniform equal-cardinality bore set makes
/// every possible bijection yield the same diameter. Differing radii or any
/// unmatched operation or bore wall reject the projection atomically.
pub(crate) fn simple_hole_diameters(
    ir: &CadIr,
    templates: &[crate::native::FeatureSimpleHoleTemplate],
    groups: &[crate::native::FeatureSimpleHoleConstructionGroup],
) -> BTreeMap<String, Length> {
    let template_operations = templates
        .iter()
        .filter(|template| {
            template.form == crate::native::SimpleHoleForm::Simple
                && template.extent == crate::native::SimpleHoleExtent::Through
        })
        .map(|template| template.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    if template_operations.len() != templates.len() || template_operations.is_empty() {
        return BTreeMap::new();
    }
    let operations = match groups {
        [] => templates
            .iter()
            .map(|template| template.operation_label.clone())
            .collect::<Vec<_>>(),
        [group] => {
            let group_operations = group
                .operation_labels
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            if group_operations.len() != group.operation_labels.len()
                || template_operations != group_operations
            {
                return BTreeMap::new();
            }
            group.operation_labels.clone()
        }
        _ => return BTreeMap::new(),
    };

    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let radii = ir
        .model
        .faces
        .iter()
        .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
        .filter_map(|face| match surfaces.get(&face.surface)? {
            SurfaceGeometry::Cylinder { radius, .. } if radius.is_finite() && *radius > 0.0 => {
                Some(*radius)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let Some(radius) = radii.first().copied() else {
        return BTreeMap::new();
    };
    if radii.len() != operations.len()
        || radii
            .iter()
            .any(|candidate| candidate.to_bits() != radius.to_bits())
    {
        return BTreeMap::new();
    }
    operations
        .iter()
        .cloned()
        .map(|operation| (operation, Length(radius * 2.0)))
        .collect()
}

/// Derive identical entry and exit chamfer treatments only when every simple
/// through-hole bore has exactly two coaxial conical faces and every cone is
/// bounded by the bore circle and one equal larger circle.
pub(crate) fn simple_hole_chamfers(
    ir: &CadIr,
    templates: &[crate::native::FeatureSimpleHoleTemplate],
) -> BTreeMap<String, HoleKind> {
    let operations = templates
        .iter()
        .filter(|template| {
            template.form == crate::native::SimpleHoleForm::Simple
                && template.extent == crate::native::SimpleHoleExtent::Through
                && template.start_treatment == crate::native::SimpleHoleEndTreatment::Chamfer
                && template.end_treatment == crate::native::SimpleHoleEndTreatment::Chamfer
        })
        .map(|template| template.operation_label.clone())
        .collect::<BTreeSet<_>>();
    if operations.len() != templates.len() || operations.is_empty() {
        return BTreeMap::new();
    }

    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let bores = ir
        .model
        .faces
        .iter()
        .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
        .filter_map(|face| match surfaces.get(&face.surface)? {
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                radius,
                ..
            } if radius.is_finite() && *radius > 0.0 => Some((*origin, *axis, *radius)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [(_, _, bore_radius), ..] = bores.as_slice() else {
        return BTreeMap::new();
    };
    if bores.len() != operations.len()
        || bores
            .iter()
            .any(|(_, _, radius)| radius.to_bits() != bore_radius.to_bits())
    {
        return BTreeMap::new();
    }

    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (&edge.id, edge.curve.as_ref()))
        .collect::<BTreeMap<_, _>>();
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let mut coedges_by_loop = BTreeMap::<&LoopId, Vec<&Coedge>>::new();
    for coedge in &ir.model.coedges {
        coedges_by_loop
            .entry(&coedge.owner_loop)
            .or_default()
            .push(coedge);
    }

    let linear_tolerance = ir.tolerances.linear.max(1e-9);
    let angular_tolerance = ir.tolerances.angular.max(1e-12);
    let mut cone_counts = vec![0usize; bores.len()];
    let mut outer_radii = Vec::new();
    let mut included_angles = Vec::new();
    for face in ir
        .model
        .faces
        .iter()
        .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
    {
        let Some(SurfaceGeometry::Cone {
            origin,
            axis,
            half_angle,
            ..
        }) = surfaces.get(&face.surface).copied()
        else {
            continue;
        };
        if !half_angle.is_finite()
            || *half_angle <= 0.0
            || *half_angle >= std::f64::consts::FRAC_PI_2
        {
            return BTreeMap::new();
        }
        let matching_bores = bores
            .iter()
            .enumerate()
            .filter_map(|(ordinal, (bore_origin, bore_axis, _))| {
                let dot = axis.x * bore_axis.x + axis.y * bore_axis.y + axis.z * bore_axis.z;
                if (1.0 - dot.abs()) > angular_tolerance {
                    return None;
                }
                let delta = Vector3::new(
                    origin.x - bore_origin.x,
                    origin.y - bore_origin.y,
                    origin.z - bore_origin.z,
                );
                let cross = Vector3::new(
                    delta.y * bore_axis.z - delta.z * bore_axis.y,
                    delta.z * bore_axis.x - delta.x * bore_axis.z,
                    delta.x * bore_axis.y - delta.y * bore_axis.x,
                );
                (cross.norm() <= linear_tolerance).then_some(ordinal)
            })
            .collect::<Vec<_>>();
        let [bore_ordinal] = matching_bores.as_slice() else {
            return BTreeMap::new();
        };
        cone_counts[*bore_ordinal] += 1;

        let mut radii = face
            .loops
            .iter()
            .flat_map(|loop_id| coedges_by_loop.get(loop_id).into_iter().flatten())
            .filter_map(|coedge| edges.get(&coedge.edge).copied().flatten())
            .filter_map(|curve_id| match curves.get(curve_id)? {
                CurveGeometry::Circle { radius, .. } if radius.is_finite() && *radius > 0.0 => {
                    Some(*radius)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        radii.sort_by(f64::total_cmp);
        let [inner, outer] = radii.as_slice() else {
            return BTreeMap::new();
        };
        if inner.to_bits() != bore_radius.to_bits() || outer <= inner {
            return BTreeMap::new();
        }
        outer_radii.push(*outer);
        included_angles.push(half_angle * 2.0);
    }
    if cone_counts.iter().any(|count| *count != 2)
        || outer_radii.len() != bores.len() * 2
        || included_angles.len() != outer_radii.len()
    {
        return BTreeMap::new();
    }
    outer_radii.sort_by(f64::total_cmp);
    included_angles.sort_by(f64::total_cmp);
    if outer_radii.last().expect("nonempty") - outer_radii[0] > linear_tolerance
        || included_angles.last().expect("nonempty") - included_angles[0] > angular_tolerance
    {
        return BTreeMap::new();
    }
    let treatment = HoleKind::Chamfer {
        diameter: Length(2.0 * outer_radii.iter().sum::<f64>() / outer_radii.len() as f64),
        angle: Angle(included_angles.iter().sum::<f64>() / included_angles.len() as f64),
    };
    operations
        .into_iter()
        .map(|operation| (operation, treatment))
        .collect()
}

fn simple_hole_extent(payload_strings: &[&str]) -> Option<cadmpeg_ir::features::Extent> {
    payload_strings
        .iter()
        .find_map(|value| crate::native::parse_simple_hole_template(value))
        .map(|_| cadmpeg_ir::features::Extent::ThroughAll)
}

fn simple_hole_kind(payload_strings: &[&str]) -> HoleKind {
    if payload_strings
        .iter()
        .any(|value| crate::native::parse_simple_hole_template(value).is_some())
    {
        HoleKind::Unresolved {
            form: Some(HoleForm::Chamfer),
            counterbore_diameter: None,
            counterbore_depth: None,
            countersink_diameter: None,
            countersink_angle: None,
        }
    } else {
        HoleKind::Simple
    }
}

fn simple_hole_exit_kind(payload_strings: &[&str]) -> Option<HoleKind> {
    payload_strings
        .iter()
        .any(|value| crate::native::parse_simple_hole_template(value).is_some())
        .then_some(HoleKind::Unresolved {
            form: Some(HoleForm::Chamfer),
            counterbore_diameter: None,
            counterbore_depth: None,
            countersink_diameter: None,
            countersink_angle: None,
        })
}

pub(crate) fn feature_body_selection(
    object_indices: &[u32],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> BodySelection {
    let mut bodies = Vec::new();
    for index in object_indices {
        let Some(bound) = bodies_by_object_index
            .get(index)
            .filter(|bound| !bound.is_empty())
        else {
            return BodySelection::Native(native);
        };
        for body in bound {
            if !bodies.contains(body) {
                bodies.push(body.clone());
            }
        }
    }
    BodySelection::Resolved { bodies, native }
}

pub(crate) fn sew_body_feature_definition(
    operands: &[&crate::native::FeatureOperationBodyOperand],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Option<FeatureDefinition> {
    let object_indices = operands
        .iter()
        .map(|operand| operand.operand_object_index)
        .collect::<Vec<_>>();
    (!object_indices.is_empty()).then(|| FeatureDefinition::SewBodies {
        bodies: feature_body_selection(
            &object_indices,
            bodies_by_object_index,
            format!(
                "nx:om-object-indices#{}",
                object_indices
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
        gap_tolerance: None,
    })
}

pub(crate) fn trim_body_feature_definition(
    target_object_index: u32,
    operands: &[&crate::native::FeatureOperationBodyOperand],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Option<FeatureDefinition> {
    let tool_object_indices = operands
        .iter()
        .map(|operand| operand.operand_object_index)
        .collect::<Vec<_>>();
    (!tool_object_indices.is_empty()).then(|| FeatureDefinition::TrimBodies {
        targets: feature_body_selection(
            &[target_object_index],
            bodies_by_object_index,
            format!("nx:om-object-index#{target_object_index}"),
        ),
        tools: feature_body_selection(
            &tool_object_indices,
            bodies_by_object_index,
            format!(
                "nx:om-object-indices#{}",
                tool_object_indices
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
        keep: BodyTrimSide::Unresolved,
    })
}

pub(crate) fn feature_body_outputs(
    object_index: u32,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Vec<BodyId> {
    bodies_by_object_index
        .get(&object_index)
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn attach_expression_parameters(
    ir: &mut CadIr,
    expressions: &[crate::native::Expression],
    declarations: &[crate::native::ExpressionDeclaration],
    parameter_uses: &[crate::native::FeatureParameterUse],
    annotations: &mut AnnotationBuilder,
) {
    let declarations = declarations
        .iter()
        .map(|declaration| (declaration.id.as_str(), declaration))
        .collect::<BTreeMap<_, _>>();
    let mut tables = BTreeMap::<String, Vec<&crate::native::Expression>>::new();
    for expression in expressions {
        let table = if expression.source_table.is_empty() {
            let Some((section, _)) = expression.id.split_once(":expression#") else {
                continue;
            };
            section
        } else {
            expression.source_table.as_str()
        };
        tables
            .entry(table.to_string())
            .or_default()
            .push(expression);
    }
    let stream = annotations.stream("nx:container");
    let mut uses_by_expression = BTreeMap::<&str, Vec<&crate::native::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        uses_by_expression
            .entry(parameter_use.expression.as_str())
            .or_default()
            .push(parameter_use);
    }
    for uses in uses_by_expression.values_mut() {
        uses.sort_by_key(|parameter_use| {
            parameter_use
                .operation_label
                .rsplit_once('-')
                .and_then(|(_, ordinal)| ordinal.parse::<u64>().ok())
                .unwrap_or(u64::MAX)
        });
    }
    let base_ordinal = ir.model.features.len() as u64;
    for (table_ordinal, (table, expressions)) in tables.into_iter().enumerate() {
        let feature_id = FeatureId(table.split_once(":expression-table#").map_or_else(
            || format!("{table}:feature#equations"),
            |(scope, key)| format!("{scope}:feature#equations-{key}"),
        ));
        let first_offset = expressions
            .iter()
            .map(|expression| expression.source_offset)
            .min()
            .unwrap_or(0);
        annotations
            .note(&feature_id, stream, first_offset)
            .tag("hostglobalvariables");
        annotations.exactness(&feature_id, Exactness::Derived);
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: base_ordinal + table_ordinal as u64,
            name: Some("NX expressions".to_string()),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("hostglobalvariables".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::Equations,
            },
            native_ref: None,
        });
        let mut parameter_ids = BTreeMap::<String, Vec<ParameterId>>::new();
        for expression in &expressions {
            parameter_ids
                .entry(expression.name.clone())
                .or_default()
                .push(
                    expression_parameter_id(&expression.id)
                        .expect("sectioned expressions have parameter identities"),
                );
        }
        for (ordinal, expression) in expressions.into_iter().enumerate() {
            let id = expression_parameter_id(&expression.id)
                .expect("sectioned expressions have parameter identities");
            annotations
                .note(&id.0, stream, expression.source_offset)
                .tag("Number");
            annotations.derived(&id.0, "owner");
            annotations.derived(&id.0, "ordinal");
            annotations.derived(&id.0, "value");
            annotations.derived(&id.0, "native_ref");
            let mut seen_dependencies = BTreeSet::new();
            let dependencies = crate::native::expression_parameter_names(&expression.expression)
                .into_iter()
                .filter_map(|name| {
                    let candidates = parameter_ids.get(name)?;
                    (candidates.len() == 1).then(|| candidates[0].clone())
                })
                .filter(|dependency| seen_dependencies.insert(dependency.clone()))
                .collect::<Vec<_>>();
            if !dependencies.is_empty() {
                annotations.derived(&id.0, "dependencies");
            }
            let value = expression.value.map(|value| match expression.unit {
                crate::native::ExpressionUnit::Millimeter => ParameterValue::Length(Length(value)),
                crate::native::ExpressionUnit::Degree => {
                    ParameterValue::Angle(Angle(value.to_radians()))
                }
            });
            let mut properties = BTreeMap::new();
            if let Some(declaration) = expression
                .declaration
                .as_deref()
                .and_then(|id| declarations.get(id))
            {
                properties.insert("declaration".to_string(), declaration.id.clone());
                properties.insert(
                    "declaration_object_id".to_string(),
                    declaration.object_id.to_string(),
                );
                annotations.derived(&id.0, "properties");
            }
            for (consumer_ordinal, parameter_use) in uses_by_expression
                .get(expression.id.as_str())
                .into_iter()
                .flatten()
                .enumerate()
            {
                properties.insert(
                    format!("consumer.{consumer_ordinal}"),
                    parameter_use
                        .operation_label
                        .replacen("operation-label", "feature", 1),
                );
                properties.insert(
                    format!("parameter_use.{consumer_ordinal}"),
                    parameter_use.id.clone(),
                );
                annotations.derived(&id.0, "properties");
            }
            ir.model.parameters.push(DesignParameter {
                id,
                owner: feature_id.clone(),
                ordinal: ordinal as u32,
                name: expression.name.clone(),
                expression: expression.expression.clone(),
                display: None,
                value,
                dependencies,
                properties,
                pmi: None,
                native_ref: Some(expression.id.clone()),
            });
        }
    }
}

pub(crate) fn attach_block_dimension_parameter_consumers(
    ir: &mut CadIr,
    dimensions: &[crate::native::FeatureBlockDimensions],
    annotations: &mut AnnotationBuilder,
) {
    let mut parameters = ir
        .model
        .parameters
        .iter_mut()
        .map(|parameter| (parameter.id.clone(), parameter))
        .collect::<BTreeMap<_, _>>();
    for dimension_set in dimensions {
        let consumer = dimension_set
            .operation_label
            .replacen("operation-label", "feature", 1);
        for (ordinal, expression) in dimension_set.expressions.iter().enumerate() {
            let Some(parameter_id) = expression_parameter_id(expression) else {
                continue;
            };
            let Some(parameter) = parameters.get_mut(&parameter_id) else {
                continue;
            };
            parameter.properties.insert(
                format!("block_dimension.{ordinal}"),
                dimension_set.id.clone(),
            );
            if !parameter
                .properties
                .values()
                .any(|value| value == &consumer)
            {
                let consumer_ordinal = (0..=parameter.properties.len())
                    .find(|candidate| {
                        !parameter
                            .properties
                            .contains_key(&format!("consumer.{candidate}"))
                    })
                    .expect("finite parameter properties have a free consumer ordinal");
                parameter
                    .properties
                    .insert(format!("consumer.{consumer_ordinal}"), consumer.clone());
            }
            annotations.derived(&parameter.id.0, "properties");
        }
    }
}

fn expression_parameter_id(expression_id: &str) -> Option<ParameterId> {
    let (section, key) = expression_id.split_once(":expression#")?;
    Some(ParameterId(format!("{section}:parameter#{key}")))
}

fn build_container_report(scan: &Scan, container_only: bool) -> DecodeReport {
    let mut losses = Vec::new();

    let assembly = scan
        .container
        .entries
        .iter()
        .any(|e| e.name.contains("ExternalReferences"))
        && !scan.has_parasolid();

    if assembly {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: "No inline Parasolid geometry: this is an assembly .prt. Component geometry \
                      lives in external child .prt files named in EXTREFSTREAM, and the assembled \
                      solid's inputs (child partitions + constraint solve) are absent from this \
                      file. This is an external-dependency boundary, not a decode gap."
                .to_string(),
            provenance: None,
        });
    } else {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: "No B-rep geometry was transferred: no gate-passing analytic carrier was found \
                      in the embedded Parasolid streams (they may hold only B-spline/procedural \
                      geometry this codec does not yet type). The streams are preserved verbatim as \
                      unknown passthrough records."
                .to_string(),
            provenance: None,
        });
    }

    if container_only {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; entity decode was not attempted."
                .to_string(),
            provenance: None,
        });
    }

    DecodeReport {
        format: "nx".to_string(),
        container_only,
        geometry_transferred: false,
        losses,
        notes: summary_notes(scan),
    }
}

/// Build container and embedded-stream notes for inspection and decode reports.
pub fn summary_notes(scan: &Scan) -> Vec<String> {
    let c = &scan.container;
    let mut notes = vec![format!(
        "SPLMSSTR container: version {:#04x}, file tag {}, footer offset {}, {} directory entry/ies",
        c.version,
        c.file_tag,
        c.footer_offset,
        c.entries.len()
    )];
    notes.push(format!(
        "embedded streams: {} partition, {} deltas, {} plain (cached body), {} preview/non-Parasolid",
        scan.count(StreamKind::Partition),
        scan.count(StreamKind::Deltas),
        scan.count(StreamKind::Plain),
        scan.count(StreamKind::Preview),
    ));
    if let Some(schema) = scan.streams.iter().find_map(|s| s.schema.as_deref()) {
        notes.push(format!("Parasolid schema: {schema}"));
    }
    let framed_om_sections = c.om_sections();
    if !framed_om_sections.is_empty() {
        let declarations = framed_om_sections
            .iter()
            .map(|(_, section)| section.types.len())
            .sum::<usize>();
        let fields = framed_om_sections
            .iter()
            .map(|(_, section)| section.fields.len())
            .sum::<usize>();
        notes.push(format!(
            "NX object model: {} size-framed section(s), {} class declaration(s), {} field declaration(s)",
            framed_om_sections.len(),
            declarations,
            fields
        ));
    }
    let om_sections = c.indexed_om_sections();
    if !om_sections.is_empty() {
        let entities = om_sections
            .iter()
            .filter(|(_, section)| {
                section
                    .records
                    .first()
                    .is_some_and(|record| record.object_id.is_some())
            })
            .map(|(_, section)| section.records.len())
            .sum::<usize>();
        let blocks = om_sections
            .iter()
            .filter(|(_, section)| {
                section
                    .records
                    .first()
                    .is_some_and(|record| record.object_id.is_none())
            })
            .map(|(_, section)| section.records.len() + usize::from(section.control.is_some()))
            .sum::<usize>();
        if blocks == 0 {
            notes.push(format!(
                "NX object model: {} indexed section(s), {} bounded entity record(s)",
                om_sections.len(),
                entities
            ));
        } else {
            notes.push(format!(
                "NX object model: {} indexed section(s), {} ID-bounded entity record(s), {} offset-only data block(s)",
                om_sections.len(),
                entities,
                blocks
            ));
        }
    }
    if !scan.has_parasolid()
        && c.entries
            .iter()
            .any(|e| e.name.contains("ExternalReferences"))
    {
        notes.push(
            "no inline Parasolid geometry (assembly .prt: geometry in external child parts)"
                .to_string(),
        );
    }
    notes
}
