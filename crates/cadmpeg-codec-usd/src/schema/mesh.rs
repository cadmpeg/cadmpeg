// SPDX-License-Identifier: Apache-2.0
//! The `Mesh` schema (spec §12) as a triangle list.
//!
//! Each face of three or more vertices becomes a fan of triangles from its
//! first vertex. A `leftHanded` mesh reverses each triangle, so every
//! triangle runs counterclockwise about its front side.

use std::collections::BTreeSet;

use crate::schema::{attribute, index, integers, reals, token, vectors3, Issues};
use crate::stage::{Prim, Stage};

/// Shading normals of a triangle list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Normals {
    None,
    /// One normal per point.
    Vertex(Vec<[f64; 3]>),
    /// One normal per triangle corner.
    Corner(Vec<[f64; 3]>),
}

/// One named set of texture coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextureCoordinates {
    /// The primvar name without the `primvars:` namespace.
    pub(crate) name: String,
    /// `true` for one value per point, `false` for one per triangle corner.
    pub(crate) per_vertex: bool,
    pub(crate) values: Vec<[f64; 2]>,
}

/// One face geometry subset in triangle terms.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Subset {
    pub(crate) path: String,
    pub(crate) family: Option<String>,
    /// Strictly increasing triangle ordinals.
    pub(crate) triangles: Vec<u32>,
}

/// A mesh read as triangles, in scene units.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MeshData {
    pub(crate) points: Vec<[f64; 3]>,
    pub(crate) triangles: Vec<[u32; 3]>,
    pub(crate) normals: Normals,
    /// The authored subdivision scheme, when it is not `none`.
    pub(crate) subdivision: Option<String>,
    /// Faces that are not part of the surface: below three vertices, or
    /// indexing no point.
    pub(crate) rejected_faces: usize,
    pub(crate) subsets: Vec<Subset>,
    /// Subsets that overlap another subset or name no face.
    pub(crate) dropped_subsets: Vec<String>,
    /// Normals with an interpolation or a count that does not map to the
    /// triangles.
    pub(crate) dropped_normals: bool,
    pub(crate) texture_coordinates: Vec<TextureCoordinates>,
    /// Texture-coordinate primvars with an interpolation or a count that
    /// does not map to the triangles, by primvar name.
    pub(crate) dropped_texture_coordinates: Vec<String>,
}

/// Interpolation of a primvar (§11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interpolation {
    Constant,
    Uniform,
    Vertex,
    FaceVarying,
}

fn interpolation(prim: &Prim<'_>, name: &str, fallback: Interpolation) -> Option<Interpolation> {
    match prim.attribute_metadata_text(name, "interpolation") {
        None => Some(fallback),
        Some("constant") => Some(Interpolation::Constant),
        Some("uniform") => Some(Interpolation::Uniform),
        Some("vertex" | "varying") => Some(Interpolation::Vertex),
        Some("faceVarying") => Some(Interpolation::FaceVarying),
        Some(_) => None,
    }
}

/// One triangle with the face it came from and the face-vertex index of
/// each corner.
struct Corner {
    face: usize,
    face_vertices: [usize; 3],
}

/// Reads `prim` as a mesh, or returns `None` when it has no points or
/// faces.
pub(crate) fn read(stage: &Stage<'_>, prim: &Prim<'_>, issues: &mut Issues) -> Option<MeshData> {
    let points = vectors3(prim, "points", issues)?;
    let counts = integers(prim, "faceVertexCounts", issues)?;
    let indices = integers(prim, "faceVertexIndices", issues)?;
    let holes: BTreeSet<usize> = integers(prim, "holeIndices", issues)
        .unwrap_or_default()
        .into_iter()
        .filter_map(index)
        .collect();
    let left_handed = token(prim, "orientation", issues).as_deref() == Some("leftHanded");
    let subdivision = match token(prim, "subdivisionScheme", issues) {
        Some(scheme) if scheme == "none" => None,
        Some(scheme) => Some(scheme),
        None => Some("catmullClark".to_owned()),
    };
    let mut triangles = Vec::new();
    let mut corners = Vec::new();
    let mut rejected_faces = 0;
    let mut start = 0_usize;
    let mut face_triangles: Vec<Vec<u32>> = Vec::new();
    for (face, count) in counts.iter().enumerate() {
        face_triangles.push(Vec::new());
        let Some(count) = index(*count) else {
            rejected_faces += 1;
            continue;
        };
        let Some(end) = start.checked_add(count).filter(|end| *end <= indices.len()) else {
            rejected_faces += counts.len() - face;
            break;
        };
        let vertices: Option<Vec<u32>> = indices
            .get(start..end)
            .unwrap_or_default()
            .iter()
            .map(|value| {
                index(*value)
                    .filter(|vertex| *vertex < points.len())
                    .and_then(|vertex| u32::try_from(vertex).ok())
            })
            .collect();
        let face_start = start;
        start = end;
        if holes.contains(&face) {
            continue;
        }
        let Some(vertices) = vertices.filter(|vertices| vertices.len() >= 3) else {
            rejected_faces += 1;
            continue;
        };
        for corner in 1..vertices.len() - 1 {
            let (second, third) = if left_handed {
                (corner + 1, corner)
            } else {
                (corner, corner + 1)
            };
            let Ok(ordinal) = u32::try_from(triangles.len()) else {
                rejected_faces += 1;
                continue;
            };
            if let Some(list) = face_triangles.last_mut() {
                list.push(ordinal);
            }
            triangles.push([vertices[0], vertices[second], vertices[third]]);
            corners.push(Corner {
                face,
                face_vertices: [face_start, face_start + second, face_start + third],
            });
        }
    }
    if points.is_empty() || triangles.is_empty() {
        return None;
    }
    let (normals, dropped_normals) = normals(prim, &points, &counts, &corners, issues);
    let (subsets, dropped_subsets) = subsets(stage, prim, &face_triangles, issues);
    let mut texture_coordinates = Vec::new();
    let mut dropped_texture_coordinates = Vec::new();
    for name in texture_coordinate_primvars(prim) {
        let mapped = reals(prim, name, 2, issues).and_then(|flat| {
            let values = flat
                .chunks_exact(2)
                .map(|chunk| [chunk[0], chunk[1]])
                .collect();
            mapped(
                prim,
                name,
                values,
                Interpolation::Constant,
                (points.len(), &counts, &corners),
                issues,
            )
        });
        let short = name.strip_prefix("primvars:").unwrap_or(name).to_owned();
        match mapped {
            Some(Mapped::Vertex(values)) => texture_coordinates.push(TextureCoordinates {
                name: short,
                per_vertex: true,
                values,
            }),
            Some(Mapped::Corner(values)) => texture_coordinates.push(TextureCoordinates {
                name: short,
                per_vertex: false,
                values,
            }),
            None => dropped_texture_coordinates.push(short),
        }
    }
    Some(MeshData {
        points,
        triangles,
        normals,
        subdivision,
        rejected_faces,
        subsets,
        dropped_subsets,
        dropped_normals,
        texture_coordinates,
        dropped_texture_coordinates,
    })
}

/// The texture-coordinate primvars of `prim` (§11.1): the `texCoord2`
/// primvars, and the two-component real primvars named `st` or `st`
/// followed by digits.
pub(crate) fn texture_coordinate_primvars<'p>(prim: &Prim<'p>) -> Vec<&'p str> {
    prim.property_names()
        .into_iter()
        .filter(|name| {
            let Some(short) = name.strip_prefix("primvars:") else {
                return false;
            };
            if short.contains(':') {
                return false;
            }
            let Some(declared) = prim.attribute_type(name) else {
                return false;
            };
            declared.starts_with("texCoord2")
                || (matches!(declared, "float2" | "double2" | "half2")
                    && short
                        .strip_prefix("st")
                        .is_some_and(|digits| digits.chars().all(|c| c.is_ascii_digit())))
        })
        .collect()
}

/// Primvar values mapped to the triangles.
enum Mapped<const N: usize> {
    /// One value per point.
    Vertex(Vec<[f64; N]>),
    /// One value per triangle corner.
    Corner(Vec<[f64; N]>),
}

/// The values of primvar `name` mapped to the triangle list, after its
/// `:indices` selectors, or `None` when its interpolation or count does not
/// map. `topology` is the point count, the face vertex counts, and the
/// triangle corners.
fn mapped<const N: usize>(
    prim: &Prim<'_>,
    name: &str,
    values: Vec<[f64; N]>,
    fallback: Interpolation,
    (point_count, counts, corners): (usize, &[i128], &[Corner]),
    issues: &mut Issues,
) -> Option<Mapped<N>> {
    let values = match integers(prim, &format!("{name}:indices"), issues) {
        Some(selectors) => selectors
            .iter()
            .map(|selector| index(*selector).and_then(|selector| values.get(selector).copied()))
            .collect::<Option<Vec<_>>>()?,
        None => values,
    };
    match interpolation(prim, name, fallback)? {
        Interpolation::Vertex => (values.len() == point_count).then_some(Mapped::Vertex(values)),
        Interpolation::Constant => values
            .first()
            .map(|value| Mapped::Corner(corners.iter().flat_map(|_| [*value; 3]).collect())),
        Interpolation::Uniform => (values.len() == counts.len()).then(|| {
            Mapped::Corner(
                corners
                    .iter()
                    .flat_map(|corner| [values[corner.face]; 3])
                    .collect(),
            )
        }),
        Interpolation::FaceVarying => corners
            .iter()
            .flat_map(|corner| {
                corner
                    .face_vertices
                    .map(|vertex| values.get(vertex).copied())
            })
            .collect::<Option<Vec<_>>>()
            .map(Mapped::Corner),
    }
}

/// The normals of the triangle list and whether authored normals were
/// dropped. `primvars:normals` overrides `normals` (§11.3).
fn normals(
    prim: &Prim<'_>,
    points: &[[f64; 3]],
    counts: &[i128],
    corners: &[Corner],
    issues: &mut Issues,
) -> (Normals, bool) {
    let (name, fallback) = if prim.has_attribute("primvars:normals") {
        ("primvars:normals", Interpolation::Constant)
    } else if prim.has_attribute("normals") {
        ("normals", Interpolation::Vertex)
    } else {
        return (Normals::None, false);
    };
    let Some(values) = vectors3(prim, name, issues) else {
        return (Normals::None, true);
    };
    match mapped(
        prim,
        name,
        values,
        fallback,
        (points.len(), counts, corners),
        issues,
    ) {
        Some(Mapped::Vertex(values)) => (Normals::Vertex(values), false),
        Some(Mapped::Corner(values)) => (Normals::Corner(values), false),
        None => (Normals::None, true),
    }
}

/// The face subsets of a mesh in triangle terms, and the subsets that could
/// not be kept (§12.1).
fn subsets(
    stage: &Stage<'_>,
    prim: &Prim<'_>,
    face_triangles: &[Vec<u32>],
    issues: &mut Issues,
) -> (Vec<Subset>, Vec<String>) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    let mut claimed = BTreeSet::new();
    for child in &prim.children {
        let Some(subset) = stage.prims.get(*child) else {
            continue;
        };
        if subset.type_name() != Some("GeomSubset") {
            continue;
        }
        let element = token(subset, "elementType", issues).unwrap_or_else(|| "face".to_owned());
        if element != "face" {
            dropped.push(subset.path.clone());
            continue;
        }
        let faces = integers(subset, "indices", issues).unwrap_or_default();
        let mut triangles: Vec<u32> = faces
            .iter()
            .filter_map(|face| index(*face))
            .filter_map(|face| face_triangles.get(face))
            .flatten()
            .copied()
            .collect();
        triangles.sort_unstable();
        triangles.dedup();
        if triangles.is_empty() || triangles.iter().any(|triangle| claimed.contains(triangle)) {
            dropped.push(subset.path.clone());
            continue;
        }
        claimed.extend(triangles.iter().copied());
        kept.push(Subset {
            path: subset.path.clone(),
            family: token(subset, "familyName", issues),
            triangles,
        });
    }
    (kept, dropped)
}

/// The constant display color and opacity of a gprim, when constant.
/// Returns the color and whether a non-constant one was dropped.
pub(crate) fn display_color(prim: &Prim<'_>, issues: &mut Issues) -> (Option<[f64; 4]>, bool) {
    let mut dropped = false;
    let mut constant = |name: &str, width: usize, issues: &mut Issues| -> Option<Vec<f64>> {
        let value = attribute(prim, name, issues)?;
        let reals = value.reals()?.to_vec();
        let interpolation = interpolation(prim, name, Interpolation::Constant);
        if reals.len() == width
            || (interpolation == Some(Interpolation::Constant) && reals.len() > width)
        {
            reals.get(..width).map(<[f64]>::to_vec)
        } else {
            dropped |= !reals.is_empty();
            None
        }
    };
    let color = constant("primvars:displayColor", 3, issues);
    let opacity = constant("primvars:displayOpacity", 1, issues);
    let color = match (color, opacity) {
        (None, None) => None,
        (color, opacity) => {
            let [r, g, b] = match color.as_deref() {
                Some([r, g, b]) => [*r, *g, *b],
                _ => [0.5, 0.5, 0.5],
            };
            let a = opacity
                .as_deref()
                .and_then(<[f64]>::first)
                .copied()
                .unwrap_or(1.0);
            Some([r, g, b, a])
        }
    };
    (color, dropped)
}

#[cfg(test)]
mod tests;
