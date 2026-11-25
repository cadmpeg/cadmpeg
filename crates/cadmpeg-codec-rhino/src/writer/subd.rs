// SPDX-License-Identifier: Apache-2.0
//! `ON_SubD` object payloads for subdivision cages (§5.6).
//!
//! The payload is one level-zero `ON_SubDimple` of version 1.0. Component
//! archive ids run over vertices, then edges, then faces, from 1; each
//! component's `m_id` is its one-based index among its own type.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteBinary32;
use cadmpeg_ir::subd::{SubdCage, SubdEdgeTag, SubdScheme, SubdSymmetryKind, SubdVertexTag};
use cadmpeg_ir::transform::Transform;

use crate::RhinoArchiveVersion;

/// `ON_SubD` class UUID in wire byte order.
pub(super) const SUBD_CLASS: [u8; 16] = [
    0xd9, 0xa4, 0x9b, 0xf0, 0x5b, 0x45, 0xc3, 0x42, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85, 0x3b,
];

/// `ON::object_type::subd_object`.
pub(super) const SUBD_OBJECT_TYPE: i64 = 0x40000;

const ANONYMOUS: u32 = 0x4000_8000;

const CLASS_USERDATA: u32 = 0x0002_7ffd;
const CLASS_USERDATA_HEADER: u32 = 0x0002_fff9;

/// Class-userdata wrapper version 2.2, as `major << 4 | minor`.
const CLASS_USERDATA_VERSION: u8 = 0x22;

/// `ON_Symmetry::ReflectId`.
const REFLECT_ID: crate::wire::Uuid = crate::wire::Uuid::from_canonical([
    0x3c, 0x6c, 0x7a, 0xbd, 0xf3, 0xd5, 0x41, 0xc2, 0x96, 0xc9, 0xdb, 0x9a, 0xea, 0xf0, 0x6e, 0x90,
]);

/// The empty bounding box `ON_BoundingBox::EmptyBoundingBox`.
const EMPTY_BOX: [f64; 6] = [1.0, 0.0, 0.0, -1.0, 0.0, 0.0];

/// How a 3DM archive holds a cage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CageForm {
    /// An `ON_SubD` object.
    Subd,
    /// An `ON_Mesh` of the control net that carries the cage as
    /// `ON_SubDMeshProxyUserData` (§7.2.6).
    MeshProxy,
    /// An `ON_Mesh` of the control net only.
    ControlMesh,
}

/// How `version` holds `cage` of `scheme`. Archive version 50 holds no
/// `ON_SubD` object, and holds the cage as a mesh of its control net.
pub(super) fn cage_form(
    scheme: SubdScheme,
    cage: &SubdCage,
    version: RhinoArchiveVersion,
) -> Result<CageForm, CodecError> {
    if version != RhinoArchiveVersion::V5 {
        Ok(CageForm::Subd)
    } else if cage.faces().is_empty() {
        Err(CodecError::NotImplemented(
            "archive version 50 holds a cage only as a mesh of its control net, and a 3DM mesh \
             holds at least one face"
                .into(),
        ))
    } else if scheme == SubdScheme::TSpline {
        Ok(CageForm::ControlMesh)
    } else {
        Ok(CageForm::MeshProxy)
    }
}

/// A cage's control net as mesh rows: its vertices, and its faces as four
/// vertex indices. A face of three sides repeats its third corner; a face of
/// more than four sides is a fan of triangles from its first corner.
pub(super) struct ControlMesh {
    pub(super) vertices: Vec<FinitePoint3>,
    pub(super) faces: Vec<[u32; 4]>,
    pub(super) fanned: bool,
}

/// The control net of `cage` as mesh rows.
pub(super) fn control_mesh(cage: &SubdCage) -> Result<ControlMesh, CodecError> {
    let edges = cage.edges();
    let mut faces = Vec::new();
    let mut fanned = false;
    for face in cage.faces() {
        let corners = face
            .edge_uses()
            .map(|edge_use| {
                let edge = position(edge_use.edge)
                    .ok()
                    .and_then(|index| edges.get(index))
                    .ok_or_else(|| {
                        CodecError::NotImplemented(format!(
                            "the cage names edge {}, which it does not hold",
                            edge_use.edge
                        ))
                    })?;
                Ok(edge.vertices()[usize::from(edge_use.reversed)])
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        match corners[..] {
            [] | [_] | [_, _] => {}
            [a, b, c] => faces.push([a, b, c, c]),
            [a, b, c, d] => faces.push([a, b, c, d]),
            [first, ref rest @ ..] => {
                fanned = true;
                faces.extend(
                    rest.windows(2)
                        .map(|pair| [first, pair[0], pair[1], pair[1]]),
                );
            }
        }
    }
    let vertices = cage
        .vertices()
        .iter()
        .map(cadmpeg_ir::subd::SubdVertex::point)
        .collect::<Vec<_>>();
    let limit = crate::mesh::MAX_MESH_COUNT;
    if vertices.len() > limit || faces.len() > limit {
        return Err(CodecError::NotImplemented(
            "the cage's control net has more faces or vertices than the 2^24 a 3DM mesh holds"
                .into(),
        ));
    }
    Ok(ControlMesh {
        vertices,
        faces,
        fanned,
    })
}

/// The body that holds a cage (§5.6), the region and shell that hold it
/// there, and whether the body holds other cages or other shells too, which
/// the archive holds as other objects.
pub(super) struct CageOwner<'ir> {
    pub(super) body: &'ir cadmpeg_ir::topology::Body,
    pub(super) region: &'ir str,
    pub(super) shell: &'ir str,
    pub(super) shared: bool,
}

/// The owner of each cage that a shell of cages only, which no face names,
/// lists, by cage id.
pub(super) fn cage_owners(ir: &CadIr) -> HashMap<&str, CageOwner<'_>> {
    use cadmpeg_ir::topology::ShellMember;

    let geometry = &ir.geometry;
    let face_shells = geometry
        .faces
        .iter()
        .map(|face| face.shell.as_str())
        .collect::<HashSet<_>>();
    let shells = geometry
        .shells
        .iter()
        .map(|shell| (shell.id.as_str(), shell))
        .collect::<HashMap<_, _>>();
    let bodies = geometry
        .bodies
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<HashMap<_, _>>();
    let mut owners = HashMap::new();
    let mut cages_by_body = HashMap::<&str, usize>::new();
    let mut mixed_bodies = HashSet::new();
    for region in &geometry.regions {
        let Some(body) = bodies.get(region.body.as_str()).copied() else {
            continue;
        };
        for shell in region.stated_shells() {
            let Some(record) = shells.get(shell.as_str()) else {
                continue;
            };
            let cages = record
                .members
                .iter()
                .filter_map(|member| match member {
                    ShellMember::Subd { id } => Some(id.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if cages.is_empty()
                || cages.len() != record.members.len()
                || face_shells.contains(shell.as_str())
            {
                mixed_bodies.insert(body.id.as_str());
                continue;
            }
            for cage in cages {
                if owners.contains_key(cage) {
                    continue;
                }
                *cages_by_body.entry(body.id.as_str()).or_default() += 1;
                owners.insert(
                    cage,
                    CageOwner {
                        body,
                        region: region.id.as_str(),
                        shell: shell.as_str(),
                        shared: false,
                    },
                );
            }
        }
    }
    for owner in owners.values_mut() {
        let body = owner.body.id.as_str();
        owner.shared = cages_by_body.get(body) > Some(&1) || mixed_bodies.contains(body);
    }
    owners
}

/// Cage content that an `ON_SubD` of the written archive version does not
/// hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Unheld {
    Symmetries,
    SymmetryCorrespondences,
    SecondaryGrips,
    KnotIntervals,
    EndSharpness,
}

impl Unheld {
    /// What the writer does with the content, as a loss states it.
    pub(super) const fn reason(self) -> &'static str {
        match self {
            Self::Symmetries => {
                "writes these cages without the symmetries their ON_SubD does not hold; an \
                 ON_SubD of archive version 70 or later holds the first symmetry of a cage where \
                 it is a reflection and the body placement is a rigid motion, and an earlier one \
                 holds no symmetry"
            }
            Self::SymmetryCorrespondences => {
                "writes the reflection symmetry of these cages without its face, edge, and \
                 vertex correspondences, which an ON_Symmetry does not hold"
            }
            Self::SecondaryGrips => "writes these cages without their vertices' secondary grips",
            Self::KnotIntervals => {
                "writes these cages without their edge knot intervals and user knots"
            }
            Self::EndSharpness => {
                "writes the start sharpness of each edge of these cages at both of its ends; \
                 archive versions before 80 hold one sharpness per edge"
            }
        }
    }
}

/// The unit plane equation `[x, y, z, d]`, with `x X + y Y + z Z + d = 0`, of
/// the reflection symmetry that an `ON_SubD` of `version` holds for `cage`
/// placed by `transform`: the first symmetry of the cage, where it is a
/// reflection, the archive version is 70 or later, and the placement is a
/// rigid motion.
pub(super) fn reflection(
    cage: &SubdCage,
    transform: Option<Transform>,
    version: RhinoArchiveVersion,
) -> Option<[f64; 4]> {
    let symmetry = cage.symmetries().first()?;
    if version.value() < 70 || !matches!(symmetry.kind(), SubdSymmetryKind::Correspondence {}) {
        return None;
    }
    let plane = &symmetry.plane;
    let origin = plane.origin().get();
    let normal = plane
        .first_axis()
        .as_raw()
        .cross(*plane.second_axis().as_raw());
    let (origin, normal) = match transform {
        None => (origin, normal),
        Some(transform) if transform.is_proper_rigid() => (
            transform.apply_point(origin)?.get(),
            transform.apply_vector(normal)?.get(),
        ),
        Some(_) => return None,
    };
    let normal = normal.unit()?;
    let offset = -normal.dot(Vector3::new(origin.x, origin.y, origin.z));
    offset
        .is_finite()
        .then_some([normal.x, normal.y, normal.z, offset])
}

/// The content of `cage` of `scheme` that an `ON_SubD` of `version` does not
/// hold, where `reflected` states that it holds the reflection symmetry of
/// [`reflection`]. The knot intervals of a T-spline cage belong to its
/// reduction to a Catmull-Clark cage.
pub(super) fn unheld(
    cage: &SubdCage,
    scheme: SubdScheme,
    reflected: bool,
    version: RhinoArchiveVersion,
) -> Vec<Unheld> {
    let symmetries = cage.symmetries();
    let edges = cage.edges();
    [
        (
            Unheld::Symmetries,
            symmetries.len() > usize::from(reflected),
        ),
        (
            Unheld::SymmetryCorrespondences,
            reflected
                && symmetries.first().is_some_and(|symmetry| {
                    !symmetry.face_pairs().is_empty()
                        || !symmetry.edge_pairs().is_empty()
                        || !symmetry.vertex_pairs().is_empty()
                }),
        ),
        (
            Unheld::SecondaryGrips,
            cage.vertices()
                .iter()
                .any(|vertex| vertex.secondary_grips().is_some()),
        ),
        (
            Unheld::KnotIntervals,
            scheme == SubdScheme::CatmullClark
                && edges
                    .iter()
                    .any(|edge| edge.knot_interval().is_some() || edge.user_knot.is_some()),
        ),
        (
            Unheld::EndSharpness,
            version.value() < 80
                && edges.iter().any(|edge| {
                    let [start, end] = edge.sharpness();
                    start != end
                }),
        ),
    ]
    .into_iter()
    .filter_map(|(content, present)| present.then_some(content))
    .collect()
}

/// The `ON_SubDMeshProxyUserData` item that carries the `ON_SubD` payload
/// `subd` on the mesh written from `mesh` (§7.2.6). Each chunk checksum
/// covers the bytes the chunk holds outside its child chunks.
pub(super) fn mesh_proxy(subd: &SubdPayload, mesh: &ControlMesh) -> Result<Vec<u8>, CodecError> {
    let unwritable = || CodecError::Malformed("mesh vertex exceeds f32 range".into());
    let vertices = mesh
        .vertices
        .iter()
        .map(|point| {
            [point.x, point.y, point.z].map(|value| {
                cadmpeg_core::convert::f32_from_f64(value).and_then(FiniteBinary32::new)
            })
        })
        .map(|point| match point {
            [Some(x), Some(y), Some(z)] => Ok([x, y, z]),
            _ => Err(unwritable()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fingerprint = crate::mesh::native_proxy_fingerprint(&mesh.faces, &vertices);
    let count = |value: usize| {
        i32::try_from(value).map_err(|_| {
            CodecError::Malformed("the control net counts exceed a 3DM mesh's range".into())
        })
    };

    let mut proxy = 1_i32.to_le_bytes().to_vec();
    proxy.extend(1_i32.to_le_bytes());
    proxy.push(1);
    let mut direct = proxy.clone();
    direct.extend(&subd.direct);
    proxy.extend(&subd.body);
    let counts = [fingerprint.face_count, fingerprint.vertex_count]
        .map(|value| count(value).map(i32::to_le_bytes))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .concat();
    proxy.extend(&counts);
    direct.extend(&counts);
    for digest in [fingerprint.face_sha1, fingerprint.vertex_sha1] {
        let mut record = 1_i32.to_le_bytes().to_vec();
        record.extend(0_i32.to_le_bytes());
        record.extend(digest);
        proxy.extend(super::crc_chunk(ANONYMOUS, &record)?);
    }
    let payload = super::crc_chunk_with_direct(
        ANONYMOUS,
        &super::crc_chunk_with_direct(ANONYMOUS, &proxy, &direct)?,
        &[],
    )?;

    let class = crate::subd::SUBD_MESH_PROXY_USERDATA.to_wire();
    let mut header = class.to_vec();
    header.extend(class);
    header.extend(1_i32.to_le_bytes());
    for index in 0..16 {
        let value: f64 = if index % 5 == 0 { 1.0 } else { 0.0 };
        header.extend(value.to_le_bytes());
    }
    header.extend(crate::presentation::OPENNURBS6_APPLICATION.to_wire());
    header.push(0);
    header.extend(
        i32::try_from(RhinoArchiveVersion::V5.value())
            .map_err(|_| CodecError::Malformed("the archive version exceeds i32".into()))?
            .to_le_bytes(),
    );
    header.extend(
        u32::try_from(super::OPENNURBS_WRITER_VERSION)
            .map_err(|_| CodecError::Malformed("the writer version exceeds u32".into()))?
            .to_le_bytes(),
    );

    let mut body = vec![CLASS_USERDATA_VERSION];
    body.extend(super::crc_chunk(CLASS_USERDATA_HEADER, &header)?);
    body.extend(payload);
    super::crc_chunk_with_direct(CLASS_USERDATA, &body, &[CLASS_USERDATA_VERSION])
}

/// A cage's `ON_SubD` class data, and the bytes its class-data checksum
/// covers.
pub(super) struct SubdPayload {
    pub(super) body: Vec<u8>,
    pub(super) direct: Vec<u8>,
}

/// The `ON_SubD` class data of `cage` for `version`, with the reflection
/// symmetry of the plane equation `reflection`, which needs archive version
/// 70 or later.
pub(super) fn payload(
    cage: &SubdCage,
    reflection: Option<[f64; 4]>,
    version: RhinoArchiveVersion,
) -> Result<SubdPayload, CodecError> {
    let archive = version.value();
    let vertices = cage.vertices();
    let edges = cage.edges();
    let faces = cage.faces();
    let vertex_count = count_u32(vertices.len())?;
    let edge_count = count_u32(edges.len())?;
    let face_count = count_u32(faces.len())?;
    let first_edge = component_id(1, vertices.len())?;
    let first_face = component_id(first_edge, edges.len())?;
    let end = component_id(first_face, faces.len())?;

    let mut vertex_edges: Vec<Vec<(usize, bool)>> = vec![Vec::new(); vertices.len()];
    let mut vertex_faces: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    let mut edge_faces: Vec<Vec<(usize, bool)>> = vec![Vec::new(); edges.len()];
    for (index, edge) in edges.iter().enumerate() {
        for (end_index, vertex) in edge.vertices().into_iter().enumerate() {
            slot(&mut vertex_edges, vertex, "vertex")?.push((index, end_index == 1));
        }
    }
    for (index, face) in faces.iter().enumerate() {
        for edge_use in face.edge_uses() {
            slot(&mut edge_faces, edge_use.edge, "edge")?.push((index, edge_use.reversed));
            let edge = &edges[position(edge_use.edge)?];
            for vertex in edge.vertices() {
                let faces = slot(&mut vertex_faces, vertex, "vertex")?;
                if faces.last() != Some(&index) {
                    faces.push(index);
                }
            }
        }
    }

    let base = |bytes: &mut Vec<u8>, archive_id: u32, id: u32| {
        bytes.extend(archive_id.to_le_bytes());
        bytes.extend(id.to_le_bytes());
        bytes.extend(0_u16.to_le_bytes());
        if archive < 70 {
            bytes.extend([0, 0]);
        } else {
            bytes.extend([0, 0, 0, 255]);
        }
    };
    let record_end = if archive < 70 { 0 } else { 255 };

    let mut level = Vec::new();
    level.extend(1_i32.to_le_bytes());
    level.extend(1_i32.to_le_bytes());
    level.extend(0_u16.to_le_bytes());
    level.extend([4, 4, 4]);
    for value in control_box(cage) {
        level.extend(value.to_le_bytes());
    }
    for partition in [1, first_edge, first_face, end] {
        level.extend(partition.to_le_bytes());
    }
    for (index, vertex) in vertices.iter().enumerate() {
        let id = component_id(1, index)?;
        base(&mut level, id, id);
        level.push(match vertex.tag {
            SubdVertexTag::Smooth => 1,
            SubdVertexTag::Crease => 2,
            SubdVertexTag::Corner => 3,
            SubdVertexTag::Dart => 4,
        });
        let point = vertex.point().get();
        for value in [point.x, point.y, point.z] {
            level.extend(value.to_le_bytes());
        }
        let edge_total = count_u16(vertex_edges[index].len())?;
        let face_total = count_u16(vertex_faces[index].len())?;
        level.extend(edge_total.to_le_bytes());
        level.extend(face_total.to_le_bytes());
        level.push(0);
        level.extend(edge_total.to_le_bytes());
        for &(edge, second) in &vertex_edges[index] {
            pointer(&mut level, component_id(first_edge, edge)?, second);
        }
        level.extend(face_total.to_le_bytes());
        for &face in &vertex_faces[index] {
            pointer(&mut level, component_id(first_face, face)?, false);
        }
        level.push(record_end);
    }
    for (index, edge) in edges.iter().enumerate() {
        base(
            &mut level,
            component_id(first_edge, index)?,
            component_id(1, index)?,
        );
        level.push(match edge.tag {
            SubdEdgeTag::Smooth => 1,
            SubdEdgeTag::Crease => 2,
            SubdEdgeTag::SmoothX => 4,
        });
        let face_total = count_u16(edge_faces[index].len())?;
        level.extend(face_total.to_le_bytes());
        for value in edge.sector_coefficients() {
            level.extend(value.get().to_le_bytes());
        }
        let [start, end] = edge
            .sharpness()
            .map(cadmpeg_ir::scalar::NonNegativeReal::get);
        level.extend(start.to_le_bytes());
        level.extend(2_u16.to_le_bytes());
        for vertex in edge.vertices() {
            pointer(&mut level, component_id(1, position(vertex)?)?, false);
        }
        level.extend(face_total.to_le_bytes());
        for &(face, reversed) in &edge_faces[index] {
            pointer(&mut level, component_id(first_face, face)?, reversed);
        }
        if archive >= 80 {
            level.push(8);
            level.extend(end.to_le_bytes());
        }
        level.push(record_end);
    }
    for (index, face) in faces.iter().enumerate() {
        let id = component_id(1, index)?;
        base(&mut level, component_id(first_face, index)?, id);
        level.extend(id.to_le_bytes());
        level.extend(0_u32.to_le_bytes());
        let edge_total = count_u16(face.edge_uses().count())?;
        level.extend(edge_total.to_le_bytes());
        level.extend(edge_total.to_le_bytes());
        for edge_use in face.edge_uses() {
            pointer(
                &mut level,
                component_id(first_edge, position(edge_use.edge)?)?,
                edge_use.reversed,
            );
        }
        if archive >= 70 {
            level.extend([0, 0, 0, 0, 0]);
        }
        level.push(record_end);
    }
    level.push(0);
    let level = super::crc_chunk(ANONYMOUS, &level)?;

    let mut subdimple = Vec::new();
    subdimple.extend(1_i32.to_le_bytes());
    subdimple.extend(if reflection.is_some() { 2_i32 } else { 0 }.to_le_bytes());
    for value in [1, vertex_count, edge_count, face_count] {
        subdimple.extend(value.to_le_bytes());
    }
    for value in EMPTY_BOX {
        subdimple.extend(value.to_le_bytes());
    }
    let mut direct = subdimple.clone();
    subdimple.extend(level);
    if let Some(plane) = reflection {
        subdimple.push(0);
        direct.push(0);
        subdimple.extend(identity_mapping_tag()?);
        subdimple.extend(reflect_symmetry(plane)?);
    }
    let mut body = vec![1];
    body.extend(super::crc_chunk_with_direct(
        ANONYMOUS, &subdimple, &direct,
    )?);
    Ok(SubdPayload {
        body,
        direct: vec![1],
    })
}

/// The `ON_MappingTag` 1.1 of no texture mapping: the nil mapping id, a zero
/// checksum, the identity transform, and the unset mapping type.
fn identity_mapping_tag() -> Result<Vec<u8>, CodecError> {
    let mut tag = 1_i32.to_le_bytes().to_vec();
    tag.extend(1_i32.to_le_bytes());
    tag.extend([0; 16]);
    tag.extend(0_i32.to_le_bytes());
    for index in 0..16 {
        let value: f64 = if index % 5 == 0 { 1.0 } else { 0.0 };
        tag.extend(value.to_le_bytes());
    }
    tag.extend(0_u32.to_le_bytes());
    super::crc_chunk(ANONYMOUS, &tag)
}

/// The `ON_Symmetry` 1.2 of the reflection across the plane of the unit
/// plane equation `plane`, in object coordinates. Its checksum covers the
/// bytes outside the plane child chunk.
fn reflect_symmetry(plane: [f64; 4]) -> Result<Vec<u8>, CodecError> {
    let mut inner = 1_i32.to_le_bytes().to_vec();
    inner.extend(3_i32.to_le_bytes());
    for value in plane {
        inner.extend(value.to_le_bytes());
    }
    let mut symmetry = 1_i32.to_le_bytes().to_vec();
    symmetry.extend(2_i32.to_le_bytes());
    symmetry.push(1);
    symmetry.extend(2_u32.to_le_bytes());
    symmetry.extend(1_u32.to_le_bytes());
    symmetry.extend(REFLECT_ID.to_wire());
    let mut direct = symmetry.clone();
    symmetry.extend(super::crc_chunk(ANONYMOUS, &inner)?);
    symmetry.push(1);
    direct.push(1);
    super::crc_chunk_with_direct(ANONYMOUS, &symmetry, &direct)
}

/// `cage` with its vertices placed by `transform`, the placement of the body
/// that holds it.
pub(super) fn placed(
    cage: &SubdCage,
    transform: Option<Transform>,
) -> Result<Cow<'_, SubdCage>, CodecError> {
    let Some(transform) = transform else {
        return Ok(Cow::Borrowed(cage));
    };
    let points = cage
        .vertices()
        .iter()
        .map(|vertex| {
            transform.apply_point(vertex.point().get()).ok_or_else(|| {
                CodecError::NotImplemented(
                    "a cage vertex leaves the finite range under the body placement".into(),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut placed = cage.clone();
    placed
        .edit_vertices(|vertices| {
            for (vertex, point) in vertices.iter_mut().zip(points) {
                vertex.set_point(point);
            }
            Ok(())
        })
        .map_err(|error| CodecError::NotImplemented(error.to_string()))?;
    Ok(Cow::Owned(placed))
}

/// The control-net bounding box as `ON_BoundingBox` writes it: the minimum
/// corner, then the maximum corner, or the empty box for a cage of no vertex.
fn control_box(cage: &SubdCage) -> [f64; 6] {
    let mut points = cage.vertices().iter().map(|vertex| vertex.point().get());
    let Some(first) = points.next() else {
        return EMPTY_BOX;
    };
    let mut bounds = [first.x, first.y, first.z, first.x, first.y, first.z];
    for point in points {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            bounds[axis] = bounds[axis].min(value);
            bounds[axis + 3] = bounds[axis + 3].max(value);
        }
    }
    bounds
}

fn pointer(bytes: &mut Vec<u8>, archive_id: u32, flag: bool) {
    bytes.extend(archive_id.to_le_bytes());
    bytes.push(u8::from(flag));
}

/// The incidence list of the component at cage index `index`.
fn slot<'list, T>(
    lists: &'list mut [Vec<T>],
    index: u32,
    kind: &str,
) -> Result<&'list mut Vec<T>, CodecError> {
    let missing = || {
        CodecError::NotImplemented(format!(
            "the cage names {kind} {index}, which it does not hold"
        ))
    };
    let index = position(index)?;
    lists.get_mut(index).ok_or_else(missing)
}

fn position(index: u32) -> Result<usize, CodecError> {
    usize::try_from(index).map_err(|_| too_many("components"))
}

/// The archive id of the component at `index` of the partition that starts
/// at `first`.
fn component_id(first: u32, index: usize) -> Result<u32, CodecError> {
    count_u32(index)?
        .checked_add(first)
        .ok_or_else(|| too_many("components"))
}

fn count_u32(count: usize) -> Result<u32, CodecError> {
    u32::try_from(count).map_err(|_| too_many("components"))
}

fn count_u16(count: usize) -> Result<u16, CodecError> {
    u16::try_from(count).map_err(|_| too_many("incident components"))
}

fn too_many(what: &str) -> CodecError {
    CodecError::NotImplemented(format!("the cage has more {what} than an ON_SubD counts"))
}

#[cfg(test)]
mod tests;
