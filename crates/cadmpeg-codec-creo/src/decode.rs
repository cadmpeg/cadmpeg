// SPDX-License-Identifier: Apache-2.0
//! Decode a `.prt` into an IR document.
//!
//! Creo PSB is, at the current state of the art, a container + namespace spec
//! rather than a geometry decoder (spec §9). Per-instance model-space geometry is
//! gated behind several undecoded PSB layers: the general 8-byte world-coordinate
//! float token, the `0x26` per-instance torus/sphere override region, and the
//! round/fillet feature evaluator. `VisibGeom` stores one *prototype per surface
//! family* (a first-instance template), not per-instance located geometry (spec
//! §4.2). Emitting a prototype surface into the IR would present template values
//! as if they were located model geometry, which the IR's carrier vocabulary
//! cannot honestly qualify — so this codec transfers **no** geometry.
//!
//! What it does instead is an honest structural decode: it enumerates the
//! container (spec §2), reads the byte-backed `srf_array`/`crv_array` namespace
//! counts (spec §4, §5), preserves the PSB geometry sections as
//! [`UnknownRecord`]s so no recognized data is silently dropped, and reports each
//! specific gate as a counted loss note.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::container::{self, role, ContainerScan};

/// Decode a `.prt` reader into an IR + report.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    let ir = build_ir(&scan);
    let report = build_report(&scan, options.container_only);
    Ok(DecodeResult::new(ir, report))
}

/// Build the metadata IR: source attributes plus the PSB geometry sections
/// preserved verbatim as unknown passthrough records.
fn build_ir(scan: &ContainerScan) -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));

    for section in scan.sections.iter().filter(|s| s.role == role::GEOMETRY) {
        let end = (section.offset + section.length).min(scan.data.len());
        let bytes = &scan.data[section.offset..end];
        let id = UnknownId(format!("creo:{}:section#{}", section.name, section.offset));
        annotate(
            &mut annotations,
            &id,
            &section.name,
            section.offset as u64,
            "psb_geometry_section",
            Exactness::Unknown,
        );
        ir.unknowns.push(UnknownRecord {
            id,
            offset: section.offset as u64,
            byte_len: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            data: Some(bytes.to_vec()),
            links: Vec::new(),
        });
    }
    for plane in &scan.datum_planes {
        let id = SurfaceId(format!("creo:datum-plane#{}", plane.id));
        annotate(
            &mut annotations,
            &id,
            "ActDatums",
            plane.offset_in_payload as u64,
            "datum_plane_outline",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(
                    plane.normal[0] * plane.offset,
                    plane.normal[1] * plane.offset,
                    plane.normal[2] * plane.offset,
                ),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    plane.normal[0],
                    plane.normal[1],
                    plane.normal[2],
                )),
            },
        });
    }
    ir.annotations = annotations.build();
    ir
}

fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    source_stream: &str,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let stream = annotations.stream(format!("creo:{source_stream}"));
    annotations.note(id.to_string(), stream, offset).tag(tag);
    annotations.exactness(id, exactness);
}

fn source_meta(scan: &ContainerScan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("version_line".to_string(), scan.version_line.clone());
    attributes.insert("layout".to_string(), scan.layout.token().to_string());
    attributes.insert("file_size".to_string(), scan.data.len().to_string());
    attributes.insert("section_count".to_string(), scan.sections.len().to_string());
    if let Some(c) = scan.census.srf_array_count {
        attributes.insert("srf_array_count".to_string(), c.to_string());
    }
    if let Some(c) = scan.census.crv_array_count {
        attributes.insert("crv_array_count".to_string(), c.to_string());
    }
    if let Some(unit) = &scan.principal_unit {
        attributes.insert("principal_unit".to_string(), unit.clone());
    }
    attributes.insert(
        "decoded_surface_row_count".to_string(),
        scan.surface_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_surface_prototype_count".to_string(),
        scan.surface_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_prototype_count".to_string(),
        scan.curve_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_topology_row_count".to_string(),
        scan.curve_topology_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_half_edge_count".to_string(),
        scan.half_edges.len().to_string(),
    );
    attributes.insert(
        "decoded_loop_count".to_string(),
        scan.loops.len().to_string(),
    );
    attributes.insert(
        "decoded_datum_plane_count".to_string(),
        scan.datum_planes.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_count".to_string(),
        scan.feature_ids.len().to_string(),
    );
    SourceMeta {
        format: "creo".to_string(),
        attributes,
    }
}

/// The loss report. Geometry is never transferred; the report names each gate.
fn build_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let geom_sections = scan
        .sections
        .iter()
        .filter(|s| s.role == role::GEOMETRY)
        .count();

    let mut losses = Vec::new();

    if container_only {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; only the container layer was read."
                .to_string(),
            provenance: None,
        });
    }

    // The namespace census: what is byte-backed and readable.
    let srf = scan
        .census
        .srf_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    let crv = scan
        .census
        .crv_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "PSB container decoded structurally: {} section(s), {} layout, VisibGeom namespace \
             census srf_array={srf} / crv_array={crv}; {} typed surface rows, {} labeled curve \
             prototypes, {} canonical curve-topology rows, and {} closed native loops were decoded. \
             Per-instance parameter bodies remain outside the transferred IR carriers.",
            scan.sections.len(),
            scan.layout.token(),
            scan.surface_rows.len(),
            scan.curve_prototypes.len(),
            scan.curve_topology_rows.len(),
            scan.loops.len(),
        ),
        provenance: None,
    });

    // The core prototype-vs-instance limitation.
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: format!(
            "No model B-rep geometry was transferred. VisibGeom stores one surface prototype per family \
             (a first-instance template), not per-instance located geometry, so its prototype \
             scalars cannot be emitted as model surfaces without mislabeling most instances (spec \
             §4.2). {geom_sections} PSB geometry section(s) were preserved verbatim as unknown \
             records."
        ),
        provenance: None,
    });

    if !scan.datum_planes.is_empty() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
                 these are unbounded reference planes, not model B-rep faces.",
                scan.datum_planes.len()
            ),
            provenance: None,
        });
    }

    // The specific undecoded PSB layers that gate per-instance geometry.
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: "Per-instance model-space coordinates are gated behind undecoded PSB layers: the \
                  general 8-byte world-coordinate float token (only the `0x46`/`0x2d` prefixes are \
                  characterized), the `0x26` per-instance torus/sphere override region, and the \
                  round/fillet feature evaluator that generates most non-planar faces. None were \
                  decoded, so no per-instance surfaces, curves, or vertices were produced."
            .to_string(),
        provenance: None,
    });

    // Topology.
    losses.push(LossNote {
        category: LossCategory::Topology,
        severity: Severity::Blocking,
        message: "Native curve half-edges and closed loops were decoded, but the IR B-rep graph \
                  (body/region/shell/face/loop/coedge/edge/vertex) was not emitted: face-instance \
                  partitioning, surface parameter bindings, curve geometry, and vertex coordinates \
                  remain incomplete."
            .to_string(),
        provenance: None,
    });

    // Features, history, materials.
    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Features, feature history, datums, section recipes, materials, and display data \
                  were not transferred."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "creo".to_string(),
        container_only,
        geometry_transferred: !scan.datum_planes.is_empty(),
        losses,
        notes: summary.notes,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}
