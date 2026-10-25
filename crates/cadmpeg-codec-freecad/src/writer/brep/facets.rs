// SPDX-License-Identifier: Apache-2.0
//! Sampled geometry records and current face display-mesh associations.
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry},
    tessellation::Tessellation,
    units::FinitePoint2,
    CadIr,
};
use std::collections::{HashMap, HashSet};

pub(super) fn polyline(curve: &cadmpeg_ir::geometry::Curve) -> Option<String> {
    let SolvedCurveGeometry::Polyline(value) = curve.geometry.solved()? else {
        return None;
    };
    let parameters = value.parameters().map(|values| values.collect::<Vec<_>>());
    let mut text = format!(
        "{} {} {}",
        value.point_count(),
        u8::from(parameters.is_some()),
        value.chordal_deflection().get()
    );
    for point in value.points() {
        text.push_str(&format!(" {}", super::geometry::point(point.get())));
    }
    if let Some(parameters) = parameters {
        for value in parameters {
            text.push_str(&format!(" {}", value.get()));
        }
    }
    Some(text)
}
pub(super) fn mesh_record(
    mesh: &Tessellation,
    uv: Option<&[FinitePoint2]>,
) -> Result<Option<String>, CodecError> {
    let Some(deflection) = mesh.chordal_deflection() else {
        return Ok(None);
    };
    let nodes = mesh.vertices();
    let triangles = mesh.triangles();
    let normals = mesh.vertex_normals();
    let uv = uv.filter(|uv| uv.len() == nodes.len());
    let mut text = format!(
        "{} {} {} {} {}",
        nodes.len(),
        triangles.len(),
        u8::from(uv.is_some()),
        u8::from(!normals.is_empty()),
        deflection.get()
    );
    for point in nodes {
        text.push_str(&format!(" {}", super::geometry::point(point.get())));
    }
    if let Some(uv) = uv {
        for point in uv {
            text.push_str(&format!(" {} {}", point.get().u, point.get().v));
        }
    }
    for triangle in triangles {
        for index in triangle {
            let index = index.checked_add(1).ok_or_else(|| {
                CodecError::malformed("triangle index exceeds its one-based carrier")
            })?;
            text.push_str(&format!(" {index}"));
        }
    }
    for normal in normals {
        let value = normal.get();
        text.push_str(&format!(" {} {} {}", value.x, value.y, value.z));
    }
    Ok(Some(text))
}

pub(super) struct Faces {
    pub(super) rows: Vec<String>,
    pub(super) indices: HashMap<String, usize>,
    pub(super) ambiguous: HashSet<String>,
    pub(super) emitted: HashSet<String>,
}
pub(super) fn faces(ir: &CadIr, selected: &HashSet<String>) -> Result<Faces, CodecError> {
    // A faceted carrier derived from a triangulation remains a support view
    // when its GUI property has no application object association. The exact
    // forward native home proves this relation without an invented object.
    let triangulation_payloads = ir
        .native
        .namespace("fcstd")
        .map(crate::brep::records::payloads_untracked)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .filter(|payload| {
            crate::brep::Tables::from_payload(payload)
                .is_some_and(|tables| !tables.triangulations.is_empty())
        })
        .map(|payload| payload.id)
        .collect::<HashSet<_>>();
    let is_support = |surface: &cadmpeg_ir::geometry::Surface| {
        surface
            .source_object
            .as_ref()
            .and_then(|source| source.geometry_role)
            == Some(cadmpeg_ir::SourceGeometryRole::Support)
            || (matches!(
                surface.geometry.solved(),
                Some(SolvedSurfaceGeometry::Polygonal(_))
            ) && surface
                .native_ref
                .as_ref()
                .is_some_and(|home| triangulation_payloads.contains(home)))
    };
    let mut out = Faces {
        rows: Vec::new(),
        indices: HashMap::new(),
        ambiguous: HashSet::new(),
        emitted: HashSet::new(),
    };
    let mut mesh_slots = HashMap::new();
    let mut surface_slots = HashMap::new();
    for face in ir
        .model
        .faces
        .iter()
        .filter(|face| selected.contains(face.id.as_str()))
    {
        let meshes = ir
            .model
            .tessellations
            .iter()
            .filter(|mesh| mesh.faces.contains(&face.id))
            .collect::<Vec<_>>();
        if meshes.len() > 1 {
            out.ambiguous.insert(face.id.as_str().to_owned());
            continue;
        }
        let record = if let Some(mesh) = meshes.first() {
            if let Some(surface) = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
            {
                if !is_support(surface) {
                    if let Some(SolvedSurfaceGeometry::Polygonal(value)) = surface.geometry.solved()
                    {
                        if value.vertices() != mesh.vertices()
                            || value.triangles() != mesh.triangles()
                            || Some(value.chordal_deflection()) != mesh.chordal_deflection()
                        {
                            out.ambiguous.insert(face.id.as_str().to_owned());
                            continue;
                        }
                    }
                }
            }
            mesh_record(mesh, None)?
        } else {
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface);
            surface
                .filter(|surface| !is_support(surface))
                .and_then(|surface| surface.geometry.solved())
                .and_then(|geometry| {
                    let SolvedSurfaceGeometry::Polygonal(value) = geometry else {
                        return None;
                    };
                    let mut text = format!(
                        "{} {} 0 0 {}",
                        value.vertices().len(),
                        value.triangles().len(),
                        value.chordal_deflection().get()
                    );
                    for point in value.vertices() {
                        text.push_str(&format!(" {}", super::geometry::point(point.get())));
                    }
                    for triangle in value.triangles() {
                        for index in triangle {
                            let index = index.checked_add(1)?;
                            text.push_str(&format!(" {index}"));
                        }
                    }
                    Some(text)
                })
        };
        if let Some(record) = record {
            if let Some(mesh) = meshes.first() {
                out.emitted.insert(mesh.id.as_str().to_owned());
            }
            let existing = if let Some(mesh) = meshes.first() {
                mesh_slots.get(&mesh.id).copied()
            } else {
                surface_slots.get(&face.surface).copied()
            };
            let index = if let Some(index) = existing {
                index
            } else {
                out.rows.push(record);
                let index = out.rows.len();
                if let Some(mesh) = meshes.first() {
                    mesh_slots.insert(mesh.id.clone(), index);
                } else {
                    surface_slots.insert(face.surface.clone(), index);
                }
                index
            };
            out.indices.insert(face.id.as_str().to_owned(), index);
        }
    }
    Ok(out)
}
