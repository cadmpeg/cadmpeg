// SPDX-License-Identifier: Apache-2.0
//! A record the archive cannot state is omitted with a loss naming it, and the
//! rest of the document is written.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Point, Vertex};

use crate::loss::RhinoLossCode;
use crate::RhinoArchiveVersion;

use super::{decoded, loss_naming, write_reported};

#[test]
fn a_document_with_nothing_writable_is_an_empty_archive() {
    let (bytes, losses) = write_reported(&CadIr::empty(), RhinoArchiveVersion::V8);
    assert!(super::charges_output_empty(&losses));
    let decoded = decoded(bytes);
    assert!(decoded.geometry.points.is_empty());
    assert!(decoded.geometry.bodies.is_empty());
}

#[test]
fn topology_no_body_owns_is_omitted_by_id() {
    let mut ir = CadIr::empty();
    let point = cadmpeg_ir::ids::PointId::mint("cadir:model:point#orphan").expect("identity");
    let vertex = cadmpeg_ir::ids::VertexId::mint("cadir:model:vertex#orphan").expect("identity");
    ir.geometry.points.push(Point::new(
        point.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
            .expect("a finite position is a point"),
        None,
    ));
    ir.geometry.vertices.push(Vertex {
        id: vertex,
        point,
        tolerance: None,
        native_ref: None,
    });
    let (bytes, losses) = write_reported(&ir, RhinoArchiveVersion::V8);
    loss_naming(
        &losses,
        RhinoLossCode::WriterTopologyOmitted,
        "cadir:model:vertex#orphan",
    );
    assert_eq!(decoded(bytes).geometry.points.len(), 1);
}

#[test]
fn a_procedural_surface_without_a_solved_cache_is_omitted_by_id() {
    let mut ir = CadIr::empty();
    ir.geometry.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("cadir:model:surface#unsolved").expect("identity"),
        geometry: SurfaceGeometry::Procedural {
            construction: cadmpeg_ir::ids::ProceduralSurfaceId::mint(
                "cadir:model:procedural-surface#unsolved",
            )
            .expect("identity"),
            cache: None,
        },
        source_object: None,
        native_ref: None,
    });
    let (bytes, losses) = write_reported(&ir, RhinoArchiveVersion::V8);
    loss_naming(
        &losses,
        RhinoLossCode::WriterSurfaceOmitted,
        "cadir:model:surface#unsolved",
    );
    assert!(decoded(bytes).geometry.surfaces.is_empty());
}

/// A revolved surface decodes as a procedural construction with a solved
/// NURBS cache; the archive states the cache and names the construction.
#[test]
fn a_procedural_construction_is_written_as_its_solved_carrier() {
    let source = decoded(
        include_bytes!("../../../tests/golden/fixtures/rev_surface.3dm").to_vec(),
    );
    let construction = source.geometry.procedural_surfaces[0].id.to_string();
    let (bytes, losses) = write_reported(&source, RhinoArchiveVersion::V8);
    loss_naming(
        &losses,
        RhinoLossCode::WriterProceduralConstructionReduced,
        &construction,
    );
    let written = decoded(bytes);
    assert_eq!(written.geometry.surfaces.len(), 1);
    assert!(written.geometry.procedural_surfaces.is_empty());
}
