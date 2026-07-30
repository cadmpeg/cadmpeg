// SPDX-License-Identifier: Apache-2.0
//! Freeform decode route composing a5a8 and consolidated NURBS record carriers.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, Pcurve,
    PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite, Surface,
    SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CurveId, PcurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use std::collections::{HashMap, HashSet};

use crate::assemble::{
    annotate, insert_unresolved_carrier_loss, link_payload_carriers, neutral_model_is_admissible,
    preserve_raw_payload, quintic_jet_pcurve, source_meta,
};
use crate::assemble::{cgm_source, cgm_source_key};
use crate::container::{self, ContainerScan};
use crate::families::FamilyOutput;

#[derive(Clone)]
struct FreeformSurfaceCarrier {
    pos: usize,
    geometry: SurfaceGeometry,
    source_object: cadmpeg_ir::SourceObjectAssociation,
    source_tag: String,
}

fn typed_face_counts(
    records: &std::collections::BTreeMap<u32, crate::families::b5::graph::B5FaceRecord>,
    resolved_count: usize,
) -> [usize; 4] {
    let controls = records.values().fold([0usize; 3], |mut counts, face| {
        match face.terminal_control {
            Some(0x03) => counts[0] += 1,
            Some(0x05) => counts[1] += 1,
            None => counts[2] += 1,
            Some(_) => unreachable!("the face parser admits only controls 03 and 05"),
        }
        counts
    });
    [
        controls[0],
        controls[1],
        controls[2],
        records
            .len()
            .checked_sub(resolved_count)
            .expect("resolved faces are a subset of typed face records"),
    ]
}

fn loop_metadata_counts<'a>(
    records: impl Iterator<Item = &'a crate::families::b5::graph::B5Loop>,
) -> [usize; 5] {
    records.fold([0usize; 5], |mut counts, loop_| {
        let index = match loop_.metadata.framing_controls {
            [0x03, 0x03] => 0,
            [0x03, 0x05] => 1,
            [0x05, 0x03] => 2,
            [0x05, 0x05] => 3,
            _ => unreachable!("the loop parser admits only controls 03 and 05"),
        };
        counts[index] += 1;
        counts[4] += usize::from(loop_.metadata.extension.is_some());
        counts
    })
}

pub(crate) fn try_decode_freeform_surfaces(scan: &ContainerScan) -> Option<FamilyOutput> {
    let mut b5_graph = crate::families::b5::graph::parse(&scan.data);
    let face_terminal_controls = b5_graph.as_ref().map(|graph| {
        graph.faces.iter().fold([0usize; 3], |mut counts, face| {
            match face.terminal_control {
                Some(0x03) => counts[0] += 1,
                Some(0x05) => counts[1] += 1,
                None => counts[2] += 1,
                Some(_) => unreachable!("the face parser admits only controls 03 and 05"),
            }
            counts
        })
    });
    let typed_face_counts = if let Some(graph) = &b5_graph {
        Some(typed_face_counts(&graph.face_records, graph.faces.len()))
    } else {
        let records = crate::families::b5::graph::typed_face_records(&scan.data);
        (!records.is_empty()).then(|| typed_face_counts(&records, 0))
    };
    let typed_edge_records = crate::families::b5::graph::typed_edge_records(&scan.data);
    let edge_terminal_controls = (!typed_edge_records.is_empty()).then(|| {
        typed_edge_records
            .values()
            .fold([0usize; 8], |mut counts, edge| {
                let index = match edge.terminal_control {
                    0x01 => 0,
                    0x02 => 1,
                    0x21 => 2,
                    0x22 => 3,
                    0x25 => 4,
                    0x26 => 5,
                    0x29 => 6,
                    0x2a => 7,
                    _ => unreachable!("the edge parser admits only declared controls"),
                };
                counts[index] += 1;
                counts
            })
    });
    let typed_vertex_incidence_links =
        crate::families::b5::graph::typed_vertex_incidence_links(&scan.data);
    let vertex_incidence_terminal_controls =
        (!typed_vertex_incidence_links.is_empty()).then(|| {
            typed_vertex_incidence_links
                .values()
                .fold([0usize; 2], |mut counts, link| {
                    match link.terminal_control {
                        0x00 => counts[0] += 1,
                        0x04 => counts[1] += 1,
                        _ => unreachable!(
                            "the vertex-incidence parser admits only controls 00 and 04"
                        ),
                    }
                    counts
                })
        });
    let resolved_loop_metadata_counts = b5_graph
        .as_ref()
        .map(|graph| loop_metadata_counts(graph.loops.values()));
    let typed_loop_records = crate::families::b5::graph::typed_loop_records(&scan.data);
    let typed_loop_metadata_counts = (!typed_loop_records.is_empty()).then(|| {
        let resolved_count = b5_graph.as_ref().map_or(0, |graph| graph.loops.len());
        (
            loop_metadata_counts(typed_loop_records.values()),
            typed_loop_records
                .len()
                .checked_sub(resolved_count)
                .expect("resolved loops are a subset of typed loop records"),
        )
    });
    let class_21_suffix_scalar_count = b5_graph.as_ref().map(|graph| {
        graph
            .pcurves
            .values()
            .filter(|pcurve| pcurve.class_21_suffix_scalar.is_some())
            .count()
    });
    let typed_class_21_pcurve_count =
        crate::families::b5::graph::typed_class_21_pcurves(&scan.data).len();
    let typed_parameter_incidences =
        crate::families::b5::graph::typed_parameter_incidences(&scan.data);
    let typed_parameter_incidence_member_count = typed_parameter_incidences
        .values()
        .map(|incidence| incidence.curves.len())
        .sum();
    let typed_vertex_incidence_rosters =
        crate::families::b5::graph::typed_vertex_incidence_rosters(&scan.data);
    let typed_vertex_incidence_roster_member_count =
        typed_vertex_incidence_rosters.values().map(Vec::len).sum();
    let mut fallback_surfaces = b5_graph
        .is_none()
        .then(|| freeform_surface_carriers(&scan.data));
    if fallback_surfaces.as_ref().is_some_and(Vec::is_empty)
        && crate::families::a5a8::records::a8_freeform_curves(&scan.data).is_empty()
    {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    let payload_id = UnknownId("catia:payload:unknown#freeform".to_string());
    preserve_raw_payload(&mut unknowns, &mut annotations, scan, &payload_id.0);
    let b5_complete = b5_graph.as_ref().is_some_and(|graph| graph.complete);
    let mut topology_ir = ir.clone();
    let mut topology_annotations = annotations.clone();
    let topology_transferred = b5_graph.take().is_some_and(|graph| {
        crate::families::b5::transfer::transfer(
            &mut topology_ir,
            &mut topology_annotations,
            graph,
            &payload_id,
        ) && neutral_model_is_admissible(&topology_ir, &unknowns)
    });
    if topology_transferred {
        ir = topology_ir;
        annotations = topology_annotations;
    }
    if !topology_transferred {
        let surfaces = fallback_surfaces
            .take()
            .unwrap_or_else(|| freeform_surface_carriers(&scan.data));
        for (index, surface) in surfaces.iter().enumerate() {
            let id = SurfaceId(format!("catia:a8:surf#{index}"));
            annotate(
                &mut annotations,
                &id,
                "object_stream_a8_03",
                surface.pos as u64,
                &surface.source_tag,
                Exactness::ByteExact,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: surface.geometry.clone(),
                source_object: Some(surface.source_object.clone()),
            });
        }
    }
    append_a8_rolling_ball_pools(&mut ir, &mut annotations, &scan.data);
    append_consolidated_line_profiles(&mut ir, &mut annotations, &scan.data);
    let mut losses = if topology_transferred && b5_complete {
        vec![LossNote {
            code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: "The B5 reference graph is closed; face sense and body kind use a deterministic topology gauge because their source fields remain unresolved."
                .to_string(),
            provenance: None,
        }]
    } else if topology_transferred {
        vec![LossNote {
            code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "A maximal reference-closed B5 face/loop/pcurve/edge subset was transferred; variant nodes and unresolved endpoint lifts remain outside the connected graph."
                .to_string(),
            provenance: None,
        }]
    } else {
        vec![LossNote {
            code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "Object-stream and consolidated NURBS carriers were decoded, but the face/loop/pcurve/edge graph did not close."
                .to_string(),
            provenance: None,
        }]
    };
    insert_unresolved_carrier_loss(&ir, &mut losses);
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();
    let mut coverage = std::collections::BTreeMap::new();
    if let Some([control_03, control_05, uncounted]) = face_terminal_controls {
        coverage.insert(
            "resolved_object_stream_face_terminal_control_03_count".to_string(),
            control_03,
        );
        coverage.insert(
            "resolved_object_stream_face_terminal_control_05_count".to_string(),
            control_05,
        );
        coverage.insert(
            "resolved_object_stream_uncounted_face_count".to_string(),
            uncounted,
        );
    }
    if let Some([control_03, control_05, uncounted, unresolved]) = typed_face_counts {
        coverage.insert(
            "typed_object_stream_face_terminal_control_03_count".to_string(),
            control_03,
        );
        coverage.insert(
            "typed_object_stream_face_terminal_control_05_count".to_string(),
            control_05,
        );
        coverage.insert(
            "typed_object_stream_uncounted_face_count".to_string(),
            uncounted,
        );
        coverage.insert(
            "typed_unresolved_object_stream_face_count".to_string(),
            unresolved,
        );
    }
    if let Some(counts) = edge_terminal_controls {
        for (control, count) in [0x01, 0x02, 0x21, 0x22, 0x25, 0x26, 0x29, 0x2a]
            .into_iter()
            .zip(counts)
        {
            coverage.insert(
                format!("typed_object_stream_edge_terminal_control_{control:02x}_count"),
                count,
            );
        }
    }
    if let Some([control_00, control_04]) = vertex_incidence_terminal_controls {
        coverage.insert(
            "typed_object_stream_vertex_incidence_terminal_control_00_count".to_string(),
            control_00,
        );
        coverage.insert(
            "typed_object_stream_vertex_incidence_terminal_control_04_count".to_string(),
            control_04,
        );
    }
    if let Some([controls_03_03, controls_03_05, controls_05_03, controls_05_05, extended]) =
        resolved_loop_metadata_counts
    {
        for (controls, count) in [
            ("03_03", controls_03_03),
            ("03_05", controls_03_05),
            ("05_03", controls_05_03),
            ("05_05", controls_05_05),
        ] {
            coverage.insert(
                format!("resolved_object_stream_loop_framing_controls_{controls}_count"),
                count,
            );
        }
        coverage.insert(
            "resolved_object_stream_extended_loop_metadata_count".to_string(),
            extended,
        );
    }
    if let Some((counts, unresolved)) = typed_loop_metadata_counts {
        for (controls, count) in ["03_03", "03_05", "05_03", "05_05"]
            .into_iter()
            .zip(counts[..4].iter().copied())
        {
            coverage.insert(
                format!("typed_object_stream_loop_framing_controls_{controls}_count"),
                count,
            );
        }
        coverage.insert(
            "typed_object_stream_extended_loop_metadata_count".to_string(),
            counts[4],
        );
        coverage.insert(
            "typed_unresolved_object_stream_loop_count".to_string(),
            unresolved,
        );
    }
    if let Some(count) = class_21_suffix_scalar_count {
        coverage.insert(
            "resolved_object_stream_class_21_pcurve_suffix_scalar_count".to_string(),
            count,
        );
    }
    if typed_class_21_pcurve_count != 0 {
        coverage.insert(
            "typed_object_stream_class_21_pcurve_suffix_scalar_count".to_string(),
            typed_class_21_pcurve_count,
        );
    }
    if !typed_parameter_incidences.is_empty() {
        coverage.insert(
            "typed_object_stream_parameter_incidence_count".to_string(),
            typed_parameter_incidences.len(),
        );
        coverage.insert(
            "typed_object_stream_parameter_incidence_member_count".to_string(),
            typed_parameter_incidence_member_count,
        );
    }
    if !typed_vertex_incidence_rosters.is_empty() {
        coverage.insert(
            "typed_object_stream_vertex_incidence_roster_count".to_string(),
            typed_vertex_incidence_rosters.len(),
        );
        coverage.insert(
            "typed_object_stream_vertex_incidence_roster_member_count".to_string(),
            typed_vertex_incidence_roster_member_count,
        );
    }
    Some(FamilyOutput {
        ir,
        report: DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            coverage,
            losses,
            notes: container::summarize(scan).notes,
        },
        annotations,
        unknowns,
    })
}

fn freeform_surface_carriers(data: &[u8]) -> Vec<FreeformSurfaceCarrier> {
    let mut surfaces = crate::families::a5a8::records::resolved_a8_surfaces(data)
        .into_iter()
        .chain(crate::families::a5a8::records::a5_surfaces(data))
        .map(|surface| {
            let (source_object, source_tag) = freeform_surface_source(&surface);
            FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: surface.geometry,
                source_object,
                source_tag: format!("freeform:{source_tag}"),
            }
        })
        .collect::<Vec<_>>();
    surfaces.extend(
        crate::families::b2::records::b2_cylinders(data)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: surface.geometry,
                source_object: cgm_source_key("b2-03-28-frame", format!("{:010}", surface.pos)),
                source_tag: format!("b2_03_28:frame_offset:{:010}", surface.pos),
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_embedded_cylinders(data)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: surface.cylinder.geometry,
                source_object: cgm_source("surface", surface.object_id),
                source_tag: format!("b2_03_60:object_id:{:08x}", surface.object_id),
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_cones(data)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: crate::families::b2::records::b2_cone_geometry(&surface),
                source_object: cgm_source_key("b2-03-29-frame", format!("{:010}", surface.pos)),
                source_tag: format!("b2_03_29:frame_offset:{:010}", surface.pos),
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_spheres(data)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: crate::families::b2::records::b2_sphere_geometry(&surface),
                source_object: cgm_source_key("b2-03-2a-frame", format!("{:010}", surface.pos)),
                source_tag: format!("b2_03_2a:frame_offset:{:010}", surface.pos),
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_tori(data)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: crate::families::b2::records::b2_torus_geometry(&surface),
                source_object: cgm_source_key("b2-03-2b-frame", format!("{:010}", surface.pos)),
                source_tag: format!("b2_03_2b:frame_offset:{:010}", surface.pos),
            }),
    );
    surfaces
}

fn freeform_surface_source(
    surface: &crate::families::a5a8::records::FreeformSurface,
) -> (cadmpeg_ir::SourceObjectAssociation, String) {
    match surface.identity {
        crate::families::a5a8::records::FreeformSurfaceIdentity::Object(object_id) => (
            cgm_source("surface", object_id),
            format!("object_id:{object_id:08x}"),
        ),
        crate::families::a5a8::records::FreeformSurfaceIdentity::FrameOffset(offset) => (
            cgm_source_key("a5-surface-frame", format!("{offset:010}")),
            format!("frame_offset:{offset:010}"),
        ),
    }
}

/// Transfer every exact consolidated line carrier independently of its parameter chart.
fn append_consolidated_line_profiles(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
) {
    for (index, line) in crate::families::b2::records::b2_line_profiles(data)
        .into_iter()
        .enumerate()
    {
        let id = CurveId(format!("catia:consolidated:line-profile-curve#{index}"));
        annotate(
            annotations,
            &id,
            "consolidated_b2_03_0e",
            line.pos as u64,
            "line_profile_carrier",
            Exactness::ByteExact,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Line {
                origin: Point3::new(line.origin[0], line.origin[1], line.origin[2]),
                direction: Vector3::new(line.direction[0], line.direction[1], line.direction[2]),
            },
            source_object: Some(cgm_source_key(
                "b2-03-0e-frame",
                format!("{:010}", line.pos),
            )),
        });
    }
}

/// Append standalone freeform carriers and return the number of consolidated
/// surface curves bound to existing standard edges.
pub(crate) fn append_freeform_surface_pools(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
) -> ConsolidatedCurveBindingCounts {
    let mut surfaces = crate::families::a5a8::records::resolved_a8_surfaces(data);
    surfaces.extend(crate::families::a5a8::records::a5_surfaces(data));
    let mut carrier_ids = Vec::with_capacity(surfaces.len());
    for surface in &surfaces {
        let (source_object, source_tag) = freeform_surface_source(surface);
        let index = ir.model.surfaces.len();
        let id = SurfaceId(format!("catia:freeform:surf#{index}"));
        carrier_ids.push(id.clone());
        annotate(
            annotations,
            &id,
            "object_stream_a8_03_or_consolidated_a5_03",
            surface.pos as u64,
            source_tag,
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: surface.geometry.clone(),
            source_object: Some(source_object),
        });
    }

    let offsets = crate::families::b2::records::b2_offset_supports(data);
    let bindings = crate::families::b2::records::offset_support_carriers(&offsets, &surfaces);
    for (offset, carrier) in offsets
        .iter()
        .zip(bindings)
        .filter_map(|(offset, carrier)| Some((offset, carrier?)))
    {
        let surface_index = ir.model.surfaces.len();
        let surface_id = SurfaceId(format!("catia:offset:surf#{surface_index}"));
        annotate(
            annotations,
            &surface_id,
            "consolidated_b2_03_31_cache",
            offset.pos as u64,
            format!("support_ref:{:08x}", offset.support_id),
            Exactness::Unknown,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });

        let procedural_id = ProceduralSurfaceId(format!(
            "catia:offset:construction#{}",
            ir.model.procedural_surfaces.len()
        ));
        annotate(
            annotations,
            &procedural_id,
            "consolidated_b2_03_31",
            offset.pos as u64,
            format!("support_ref:{:08x}", offset.support_id),
            Exactness::ByteExact,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Offset {
                support: carrier_ids[carrier].clone(),
                distance: offset.distance,
                u_sense: Some(1),
                v_sense: Some(1),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

    append_consolidated_line_profiles(ir, annotations, data);

    for guide in crate::families::a5a8::records::a5_guide_curves(data) {
        let points = guide
            .sites
            .iter()
            .map(|site| site.point)
            .collect::<Vec<_>>();
        let first = guide
            .first_derivatives
            .iter()
            .map(|value| [value[0], value[1], value[2]])
            .collect::<Vec<_>>();
        let second = guide
            .second_derivatives
            .iter()
            .map(|value| [value[0], value[1], value[2]])
            .collect::<Vec<_>>();
        let Some((knots, control_points)) = crate::nurbs::quintic_jet_bspline3(
            guide.degree,
            &guide.knots,
            &points,
            &first,
            &second,
        ) else {
            continue;
        };
        let id = CurveId(format!("catia:guide:curve#{}", ir.model.curves.len()));
        annotate(
            annotations,
            &id,
            "consolidated_a5_03_39",
            guide.pos as u64,
            format!("header_token:{:08x}", guide.header_token),
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: guide.degree,
                knots,
                control_points: control_points
                    .into_iter()
                    .map(|point| Point3::new(point[0], point[1], point[2]))
                    .collect(),
                weights: None,
                periodic: false,
            }),
            source_object: None,
        });
    }

    for jet in crate::families::a5a8::records::a5_freeform_curves(data) {
        for second_limit in [false, true] {
            let Some(curve) =
                crate::families::a5a8::records::rolling_ball_limit_curve(&jet, second_limit)
            else {
                continue;
            };
            let side = usize::from(second_limit);
            let id = CurveId(format!("catia:rolling-ball:limit#{}:{side}", jet.pos));
            annotate(
                annotations,
                &id,
                "consolidated_a5_03_32",
                jet.pos as u64,
                format!("limit_{}", side + 1),
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Nurbs(curve),
                source_object: None,
            });
        }
        let sites = jet
            .sites
            .iter()
            .zip(&jet.first_derivatives)
            .zip(&jet.second_derivatives)
            .map(|((site, first), second)| RollingBallJetSite {
                first_limit: Point3::new(site.limit1[0], site.limit1[1], site.limit1[2]),
                second_limit: Point3::new(site.limit2[0], site.limit2[1], site.limit2[2]),
                center: Point3::new(site.center[0], site.center[1], site.center[2]),
                angle: site.theta,
                first_derivative: rolling_ball_derivative(*first),
                second_derivative: rolling_ball_derivative(*second),
            })
            .collect::<Vec<_>>();
        if sites.len() != jet.knots.len() {
            continue;
        }
        let surface_index = ir.model.surfaces.len();
        let surface_id = SurfaceId(format!("catia:rolling-ball:surf#{surface_index}"));
        annotate(
            annotations,
            &surface_id,
            "consolidated_a5_03_32_cache",
            jet.pos as u64,
            format!("header_token:{:08x}", jet.header_token),
            Exactness::Unknown,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });

        let procedural_id = ProceduralSurfaceId(format!(
            "catia:rolling-ball:construction#{}",
            ir.model.procedural_surfaces.len()
        ));
        annotate(
            annotations,
            &procedural_id,
            "consolidated_a5_03_32",
            jet.pos as u64,
            format!("header_token:{:08x}", jet.header_token),
            Exactness::ByteExact,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::RollingBallJet {
                degree: jet.degree,
                multiplicities: vec![jet.degree + 1; jet.knots.len()],
                knots: jet.knots,
                sites,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

    append_a8_rolling_ball_pools(ir, annotations, data);
    append_resolved_consolidated_surface_curves(ir, annotations, data, &surfaces, &carrier_ids)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ConsolidatedCarrierKey {
    Cylinder(usize),
    EmbeddedCylinder(usize),
    Cone(usize),
    Torus(usize),
    NurbsOffset(usize, u64),
}

pub(crate) enum ConsolidatedCarrierChart<'a> {
    Identity,
    Cylinder {
        radius: f64,
    },
    Cone {
        cone: &'a crate::families::b2::records::B2Cone,
    },
    Torus {
        torus: &'a crate::families::b2::records::B2Torus,
    },
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ConsolidatedCurveBindingCounts {
    pub(crate) standard_edges: usize,
    pub(crate) partner_supports: usize,
    pub(crate) partner_face_pcurve_pairs: usize,
    pub(crate) standard_face_surfaces: usize,
}

struct ConsolidatedStandardFaceBinding {
    coedges: Vec<(usize, PcurveGeometry)>,
    standard_surfaces: [SurfaceId; 2],
    edge_pcurves: [PcurveGeometry; 2],
    inferred_partner: Option<(usize, usize)>,
}

impl ConsolidatedCarrierChart<'_> {
    fn point(&self, [u, v]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Identity => [u, v],
            Self::Cylinder { radius } => [u / radius, v],
            Self::Cone { cone } => [
                u / cone.angular_scale,
                (v - cone.slant_range[0]) * cone.half_angle.cos(),
            ],
            Self::Torus { torus } => [u / torus.major_scale, v / torus.minor_scale],
        }
    }

    fn derivative(&self, [u, v]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Identity => [u, v],
            Self::Cylinder { radius } => [u / radius, v],
            Self::Cone { cone } => [u / cone.angular_scale, v * cone.half_angle.cos()],
            Self::Torus { torus } => [u / torus.major_scale, v / torus.minor_scale],
        }
    }
}

pub(crate) fn consolidated_jet_pcurve(
    pcurve: &crate::wire::records::ConsolidatedPcurve,
    chart: &ConsolidatedCarrierChart<'_>,
) -> Option<PcurveGeometry> {
    let points = pcurve
        .points
        .iter()
        .copied()
        .map(|point| chart.point(point))
        .collect::<Vec<_>>();
    let first = pcurve
        .first_derivatives
        .iter()
        .copied()
        .map(|derivative| chart.derivative(derivative))
        .collect::<Vec<_>>();
    let second = pcurve
        .second_derivatives
        .iter()
        .copied()
        .map(|derivative| chart.derivative(derivative))
        .collect::<Vec<_>>();
    quintic_jet_pcurve(pcurve.degree, &pcurve.knots, &points, &first, &second)
}

/// Transfer resolved consolidated surface curves, reusing an existing
/// pcurve-less standard edge construction when endpoint loci select one.
pub(crate) fn append_resolved_consolidated_surface_curves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
    freeform_surfaces: &[crate::families::a5a8::records::FreeformSurface],
    freeform_surface_ids: &[SurfaceId],
) -> ConsolidatedCurveBindingCounts {
    let standalone = crate::families::b2::records::b2_cylinders(data)
        .into_iter()
        .map(|cylinder| (cylinder.pos, cylinder))
        .collect::<HashMap<_, _>>();
    let embedded = crate::families::b2::records::b2_embedded_cylinders(data)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<HashMap<_, _>>();
    let cones = crate::families::b2::records::b2_cones(data)
        .into_iter()
        .map(|cone| (cone.pos, cone))
        .collect::<HashMap<_, _>>();
    let tori = crate::families::b2::records::b2_tori(data)
        .into_iter()
        .map(|torus| (torus.pos, torus))
        .collect::<HashMap<_, _>>();
    let complete_runs =
        crate::families::consolidated::records::consolidated_topology_edge_runs(data)
            .into_iter()
            .filter(|run| run.edge.co_parametric && run.identity_chain_consistent)
            .map(|run| (run.edge.pcurves[0].pos, run))
            .collect::<HashMap<_, _>>();

    let mut surface_ids = HashMap::<ConsolidatedCarrierKey, SurfaceId>::new();
    let point_positions = ir
        .model
        .points
        .iter()
        .map(|point| (point.id.clone(), point.position))
        .collect::<HashMap<_, _>>();
    let vertex_positions = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| Some((vertex.id.clone(), *point_positions.get(&vertex.point)?)))
        .collect::<HashMap<_, _>>();
    let curve_indices = ir
        .model
        .curves
        .iter()
        .enumerate()
        .map(|(index, curve)| (curve.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.surface.clone()))
        .collect::<HashMap<_, _>>();
    let loop_surfaces = ir
        .model
        .loops
        .iter()
        .filter_map(|loop_| Some((loop_.id.clone(), face_surfaces.get(&loop_.face)?.clone())))
        .collect::<HashMap<_, _>>();
    let coedge_surfaces = ir
        .model
        .coedges
        .iter()
        .map(|coedge| loop_surfaces.get(&coedge.owner_loop).cloned())
        .collect::<Vec<_>>();
    let attachable_edges = ir
        .model
        .edges
        .iter()
        .enumerate()
        .filter_map(|(edge_index, edge)| {
            let curve_id = edge.curve.as_ref()?;
            let curve_index = *curve_indices.get(curve_id)?;
            if !matches!(
                ir.model.curves[curve_index].geometry,
                CurveGeometry::Unknown { .. }
            ) {
                return None;
            }
            let procedures = ir
                .model
                .procedural_curves
                .iter()
                .enumerate()
                .filter_map(|(index, procedure)| {
                    if procedure.curve != *curve_id {
                        return None;
                    }
                    let ProceduralCurveDefinition::Intersection { context, .. } =
                        &procedure.definition
                    else {
                        return None;
                    };
                    let surfaces = std::array::from_fn(|side| {
                        (context.sides[side].pcurve.is_none())
                            .then(|| context.sides[side].surface.clone())
                            .flatten()
                    });
                    let [Some(first), Some(second)] = surfaces else {
                        return None;
                    };
                    Some((index, [first, second]))
                })
                .collect::<Vec<_>>();
            let [(procedure_index, standard_surfaces)] = procedures.as_slice() else {
                return None;
            };
            Some((
                edge_index,
                *procedure_index,
                curve_id.clone(),
                standard_surfaces.clone(),
                [
                    *vertex_positions.get(&edge.start)?,
                    *vertex_positions.get(&edge.end)?,
                ],
            ))
        })
        .collect::<Vec<_>>();
    let mut attached_curves = HashSet::new();
    let mut binding_counts = ConsolidatedCurveBindingCounts::default();

    for resolved in crate::families::consolidated::records::resolve_consolidated_edge_blocks(data) {
        let Some(run) = complete_runs.get(&resolved.block.pcurves[0].pos) else {
            continue;
        };
        let mut sides: [IntcurveSupportSide; 2] = std::array::from_fn(|_| IntcurveSupportSide {
            surface: None,
            pcurve: None,
            pcurve_parameter_range: None,
        });
        for (side, binding) in resolved.supports.iter().enumerate() {
            if let Some(
                crate::families::consolidated::records::ConsolidatedSupportBinding::NurbsCarrier {
                    pos,
                    offset,
                },
            ) = binding
            {
                let Some((carrier_index, _)) = freeform_surfaces
                    .iter()
                    .enumerate()
                    .find(|(_, surface)| surface.pos == *pos)
                else {
                    continue;
                };
                let Some(support) = freeform_surface_ids.get(carrier_index).cloned() else {
                    continue;
                };
                let surface = if *offset == 0.0 {
                    support
                } else {
                    let key = ConsolidatedCarrierKey::NurbsOffset(*pos, offset.to_bits());
                    if let Some(id) = surface_ids.get(&key) {
                        id.clone()
                    } else {
                        let id = SurfaceId(format!(
                            "catia:consolidated:nurbs-offset#{}",
                            ir.model.surfaces.len()
                        ));
                        annotate(
                            annotations,
                            &id,
                            "consolidated_a5_03_34_offset_cache",
                            *pos as u64,
                            "resolved_pcurve_support",
                            Exactness::Unknown,
                        );
                        ir.model.surfaces.push(Surface {
                            id: id.clone(),
                            geometry: SurfaceGeometry::Unknown { record: None },
                            source_object: None,
                        });
                        let procedural_id = ProceduralSurfaceId(format!(
                            "catia:consolidated:nurbs-offset-construction#{}",
                            ir.model.procedural_surfaces.len()
                        ));
                        annotate(
                            annotations,
                            &procedural_id,
                            "consolidated_a5_03_34_constant_normal_offset",
                            *pos as u64,
                            "resolved_pcurve_support",
                            Exactness::Derived,
                        );
                        ir.model.procedural_surfaces.push(ProceduralSurface {
                            id: procedural_id,
                            surface: id.clone(),
                            definition: ProceduralSurfaceDefinition::Offset {
                                support,
                                distance: *offset,
                                u_sense: Some(1),
                                v_sense: Some(1),
                                extension_flags: Vec::new(),
                                revision_form: None,
                            },
                            cache_fit_tolerance: None,
                            record_bounds: None,
                        });
                        surface_ids.insert(key, id.clone());
                        id
                    }
                };
                let pcurve = &resolved.block.pcurves[side];
                let chart = ConsolidatedCarrierChart::Identity;
                let Some(geometry) = consolidated_jet_pcurve(pcurve, &chart) else {
                    continue;
                };
                sides[side] = IntcurveSupportSide {
                    surface: Some(surface),
                    pcurve: Some(geometry),
                    pcurve_parameter_range: None,
                };
                continue;
            }
            let (key, carrier, source_object, chart, annotation_kind, id_kind) = match binding {
                Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Cylinder { pos }) => {
                    let Some(cylinder) = standalone.get(pos) else {
                        continue;
                    };
                    let carrier = cylinder.geometry.clone();
                    let SurfaceGeometry::Cylinder { radius, .. } = carrier else {
                        continue;
                    };
                    if radius <= 0.0 || !radius.is_finite() {
                        continue;
                    }
                    (
                        ConsolidatedCarrierKey::Cylinder(*pos),
                        carrier,
                        None,
                        ConsolidatedCarrierChart::Cylinder { radius },
                        "consolidated_b2_03_28_cylinder",
                        "cylinder",
                    )
                }
                Some(crate::families::consolidated::records::ConsolidatedSupportBinding::EmbeddedCylinder { pos, .. }) => {
                    let Some(value) = embedded.get(pos) else {
                        continue;
                    };
                    let carrier = value.cylinder.geometry.clone();
                    let SurfaceGeometry::Cylinder { radius, .. } = carrier else {
                        continue;
                    };
                    if radius <= 0.0 || !radius.is_finite() {
                        continue;
                    }
                    (
                        ConsolidatedCarrierKey::EmbeddedCylinder(*pos),
                        carrier,
                        Some(cgm_source("surface", value.object_id)),
                        ConsolidatedCarrierChart::Cylinder { radius },
                        "consolidated_b2_03_60_cylinder",
                        "cylinder",
                    )
                }
                Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Cone { pos }) => {
                    let Some(cone) = cones.get(pos) else {
                        continue;
                    };
                    if cone.angular_scale <= 0.0
                        || !cone.angular_scale.is_finite()
                        || !cone.half_angle.is_finite()
                    {
                        continue;
                    }
                    (
                        ConsolidatedCarrierKey::Cone(*pos),
                        crate::families::b2::records::b2_cone_geometry(cone),
                        None,
                        ConsolidatedCarrierChart::Cone { cone },
                        "consolidated_b2_03_29_cone",
                        "cone",
                    )
                }
                Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Torus { pos }) => {
                    let Some(torus) = tori.get(pos) else {
                        continue;
                    };
                    (
                        ConsolidatedCarrierKey::Torus(*pos),
                        crate::families::b2::records::b2_torus_geometry(torus),
                        None,
                        ConsolidatedCarrierChart::Torus { torus },
                        "consolidated_b2_03_2b_torus",
                        "torus",
                    )
                }
                Some(
                    crate::families::consolidated::records::ConsolidatedSupportBinding::Circle { .. }
                    | crate::families::consolidated::records::ConsolidatedSupportBinding::NurbsCarrier { .. },
                )
                | None => continue,
            };
            let surface = if let Some(id) = surface_ids.get(&key) {
                id.clone()
            } else {
                let id = SurfaceId(format!(
                    "catia:consolidated:{id_kind}#{}",
                    ir.model.surfaces.len()
                ));
                annotate(
                    annotations,
                    &id,
                    annotation_kind,
                    match key {
                        ConsolidatedCarrierKey::Cylinder(pos)
                        | ConsolidatedCarrierKey::EmbeddedCylinder(pos)
                        | ConsolidatedCarrierKey::Cone(pos)
                        | ConsolidatedCarrierKey::Torus(pos)
                        | ConsolidatedCarrierKey::NurbsOffset(pos, _) => pos as u64,
                    },
                    "resolved_pcurve_support",
                    Exactness::ByteExact,
                );
                ir.model.surfaces.push(Surface {
                    id: id.clone(),
                    geometry: carrier,
                    source_object,
                });
                surface_ids.insert(key, id.clone());
                id
            };

            let pcurve = &resolved.block.pcurves[side];
            let Some(geometry) = consolidated_jet_pcurve(pcurve, &chart) else {
                continue;
            };
            sides[side] = IntcurveSupportSide {
                surface: Some(surface),
                pcurve: Some(geometry),
                pcurve_parameter_range: None,
            };
        }
        let resolved_sides = sides
            .iter()
            .enumerate()
            .filter(|(_, side)| side.surface.is_some() && side.pcurve.is_some())
            .map(|(side, _)| side)
            .collect::<Vec<_>>();
        let inferred_partner = (|| {
            let [resolved_side] = resolved_sides.as_slice() else {
                return None;
            };
            let partner = 1 - *resolved_side;
            let resolved_geometry = &ir
                .model
                .surfaces
                .iter()
                .find(|surface| Some(&surface.id) == sides[*resolved_side].surface.as_ref())?
                .geometry;
            let partner_pcurve = consolidated_jet_pcurve(
                &resolved.block.pcurves[partner],
                &ConsolidatedCarrierChart::Identity,
            )?;
            let carrier = unique_paired_surface_lift_match(
                sides[*resolved_side].pcurve.as_ref()?,
                resolved_geometry,
                &partner_pcurve,
                resolved.block.parameters.range,
                freeform_surfaces
                    .iter()
                    .enumerate()
                    .map(|(index, surface)| (index, &surface.geometry)),
            );
            if let Some(carrier) = carrier {
                sides[partner] = IntcurveSupportSide {
                    surface: Some(freeform_surface_ids[carrier].clone()),
                    pcurve: Some(partner_pcurve),
                    pcurve_parameter_range: None,
                };
                binding_counts.partner_supports += 1;
                Some((*resolved_side, carrier))
            } else {
                None
            }
        })();
        let exact_side_count = sides
            .iter()
            .filter(|side| side.surface.is_some() && side.pcurve.is_some())
            .count();
        if exact_side_count == 0 {
            continue;
        }
        let attachment = resolved.endpoint_loci.as_ref().and_then(|loci| {
            unique_endpoint_pair_match(
                *loci,
                attachable_edges
                    .iter()
                    .filter(|(_, _, curve, _, _)| !attached_curves.contains(curve))
                    .map(|(edge, procedure, curve, surfaces, endpoints)| {
                        (
                            (*edge, *procedure, curve.clone(), surfaces.clone()),
                            *endpoints,
                        )
                    }),
            )
        });
        let attachment = attachment.and_then(|(identity, reversed)| {
            if reversed {
                let reversed_pcurves = sides
                    .iter()
                    .map(|side| match &side.pcurve {
                        Some(pcurve) => crate::nurbs::reverse_pcurve_geometry(
                            pcurve,
                            resolved.block.parameters.range,
                        )
                        .map(Some),
                        None => Some(None),
                    })
                    .collect::<Option<Vec<_>>>()?;
                for (side, pcurve) in sides.iter_mut().zip(reversed_pcurves) {
                    side.pcurve = pcurve;
                }
            }
            let (_, _, _, standard_surfaces) = &identity;
            let resolved_sides = sides
                .iter()
                .enumerate()
                .filter(|(_, side)| side.surface.is_some() && side.pcurve.is_some())
                .map(|(side, _)| side)
                .collect::<Vec<_>>();
            let resolved_side = inferred_partner
                .map(|(resolved_side, _)| resolved_side)
                .or_else(|| {
                    let [resolved_side] = resolved_sides.as_slice() else {
                        return None;
                    };
                    Some(*resolved_side)
                });
            let partner_pcurves = if let Some(resolved_side) = resolved_side {
                let resolved_surface = sides[resolved_side].surface.as_ref()?;
                let resolved_geometry = &ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| &surface.id == resolved_surface)?
                    .geometry;
                let matches = standard_surfaces
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| {
                        ir.model
                            .surfaces
                            .iter()
                            .find(|surface| &surface.id == *id)
                            .is_some_and(|surface| {
                                same_surface_locus(&surface.geometry, resolved_geometry)
                            })
                    })
                    .map(|(side, _)| side)
                    .collect::<Vec<_>>();
                if let [standard_resolved_side] = matches.as_slice() {
                    let partner = 1 - resolved_side;
                    if let Some((_, carrier)) = inferred_partner {
                        let standard_partner = &standard_surfaces[1 - *standard_resolved_side];
                        let standard_partner_geometry = &ir
                            .model
                            .surfaces
                            .iter()
                            .find(|surface| &surface.id == standard_partner)?
                            .geometry;
                        if !matches!(standard_partner_geometry, SurfaceGeometry::Unknown { .. })
                            && standard_partner_geometry != &freeform_surfaces[carrier].geometry
                        {
                            return Some((identity, None));
                        }
                    }
                    let partner_pcurve = match &sides[partner].pcurve {
                        Some(pcurve) => pcurve.clone(),
                        None => {
                            let mut pcurve = consolidated_jet_pcurve(
                                &resolved.block.pcurves[partner],
                                &ConsolidatedCarrierChart::Identity,
                            )?;
                            if reversed {
                                pcurve = crate::nurbs::reverse_pcurve_geometry(
                                    &pcurve,
                                    resolved.block.parameters.range,
                                )?;
                            }
                            pcurve
                        }
                    };
                    let standard_surface_geometry = &ir
                        .model
                        .surfaces
                        .iter()
                        .find(|surface| surface.id == standard_surfaces[*standard_resolved_side])?
                        .geometry;
                    let resolved_pcurve = rechart_equivalent_surface_pcurve(
                        sides[resolved_side].pcurve.as_ref()?,
                        resolved_geometry,
                        standard_surface_geometry,
                    )?;
                    let standard_geometries = [resolved_pcurve, partner_pcurve];
                    let standard_geometries = if *standard_resolved_side == 0 {
                        standard_geometries
                    } else {
                        [
                            standard_geometries[1].clone(),
                            standard_geometries[0].clone(),
                        ]
                    };
                    let edge_id = &ir.model.edges[identity.0].id;
                    let coedges = standard_surfaces
                        .iter()
                        .enumerate()
                        .map(|(side, surface)| {
                            let candidates = ir
                                .model
                                .coedges
                                .iter()
                                .enumerate()
                                .filter(|(index, coedge)| {
                                    coedge.edge == *edge_id
                                        && coedge.pcurves.is_empty()
                                        && coedge_surfaces[*index].as_ref() == Some(surface)
                                })
                                .map(|(index, _)| index)
                                .collect::<Vec<_>>();
                            let [coedge] = candidates.as_slice() else {
                                return None;
                            };
                            let mut geometry = standard_geometries[side].clone();
                            if matches!(
                                ir.model.coedges[*coedge].sense,
                                cadmpeg_ir::topology::Sense::Reversed
                            ) {
                                geometry = crate::nurbs::reverse_pcurve_geometry(
                                    &geometry,
                                    resolved.block.parameters.range,
                                )?;
                            }
                            Some((*coedge, geometry))
                        })
                        .collect::<Option<Vec<_>>>();
                    coedges.filter(|coedges| coedges.len() == 2).map(|coedges| {
                        ConsolidatedStandardFaceBinding {
                            coedges,
                            standard_surfaces: standard_surfaces.clone(),
                            edge_pcurves: standard_geometries,
                            inferred_partner: inferred_partner
                                .map(|(_, carrier)| (1 - *standard_resolved_side, carrier)),
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            };
            Some((identity, partner_pcurves))
        });
        if let Some((_, Some(binding))) = attachment.as_ref() {
            if let Some((standard_partner_side, carrier)) = binding.inferred_partner {
                let surface_id = &binding.standard_surfaces[standard_partner_side];
                if let Some(surface) = ir
                    .model
                    .surfaces
                    .iter_mut()
                    .find(|surface| &surface.id == surface_id)
                {
                    if matches!(surface.geometry, SurfaceGeometry::Unknown { .. }) {
                        surface.geometry = freeform_surfaces[carrier].geometry.clone();
                        annotations.derived(&surface.id, "geometry");
                        binding_counts.standard_face_surfaces += 1;
                    }
                    sides = std::array::from_fn(|side| IntcurveSupportSide {
                        surface: Some(binding.standard_surfaces[side].clone()),
                        pcurve: Some(binding.edge_pcurves[side].clone()),
                        pcurve_parameter_range: None,
                    });
                }
            }
        }
        let context = IntcurveSupportContext {
            sides,
            parameter_range: resolved.block.parameters.range,
            discontinuities: std::array::from_fn(|_| Vec::new()),
        };
        let definition = if exact_side_count == 2 {
            ProceduralCurveDefinition::Intersection {
                context,
                discontinuity_flag: false,
            }
        } else {
            ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric,
                context,
                tail: None,
            }
        };
        if let Some(((edge_index, procedure_index, curve_id, _), partner_pcurves)) = attachment {
            attached_curves.insert(curve_id.clone());
            binding_counts.standard_edges += 1;
            if let Some(partner_pcurves) = partner_pcurves {
                binding_counts.partner_face_pcurve_pairs += 1;
                for (coedge_index, geometry) in partner_pcurves.coedges {
                    let pcurve_id = PcurveId(format!(
                        "catia:consolidated:standard-pcurve#{}",
                        ir.model.pcurves.len()
                    ));
                    annotate(
                        annotations,
                        &pcurve_id,
                        "consolidated_edge_run",
                        run.edge.pcurves[0].pos as u64,
                        "resolved_face_side_pcurve",
                        Exactness::Derived,
                    );
                    ir.model.pcurves.push(Pcurve {
                        id: pcurve_id.clone(),
                        geometry,
                        wrapper_reversed: None,
                        native_tail_flags: None,
                        parameter_range: Some(resolved.block.parameters.range),
                        fit_tolerance: None,
                    });
                    ir.model.coedges[coedge_index]
                        .pcurves
                        .push(cadmpeg_ir::topology::PcurveUse {
                            pcurve: pcurve_id,
                            isoparametric: None,
                            parameter_range: None,
                        });
                    annotations.derived(&ir.model.coedges[coedge_index].id, "pcurves");
                }
            }
            ir.model.edges[edge_index].param_range = Some(resolved.block.parameters.range);
            let procedural = &mut ir.model.procedural_curves[procedure_index];
            procedural.definition = definition;
            procedural.cache_fit_tolerance = None;
            annotate(
                annotations,
                &procedural.id,
                "consolidated_edge_run",
                run.edge.pcurves[0].pos as u64,
                "resolved_surface_curve_bound_to_standard_edge",
                Exactness::Derived,
            );
            annotations
                .derived(&procedural.id, "curve")
                .derived(&procedural.id, "definition");
        } else {
            let curve_id = CurveId(format!(
                "catia:consolidated:curve#{}",
                ir.model.curves.len()
            ));
            annotate(
                annotations,
                &curve_id,
                "consolidated_edge_run",
                run.edge.pcurves[0].pos as u64,
                "procedural_curve_cache",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Unknown { record: None },
                source_object: None,
            });
            let procedural_id = ProceduralCurveId(format!(
                "catia:consolidated:construction#{}",
                ir.model.procedural_curves.len()
            ));
            annotate(
                annotations,
                &procedural_id,
                "consolidated_edge_run",
                run.edge.pcurves[0].pos as u64,
                "resolved_surface_curve",
                Exactness::Derived,
            );
            annotations
                .derived(&procedural_id, "curve")
                .derived(&procedural_id, "definition");
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id,
                definition,
                cache_fit_tolerance: None,
            });
        }
    }
    binding_counts
}

fn unique_endpoint_pair_match<T>(
    loci: [Point3; 2],
    candidates: impl Iterator<Item = (T, [Point3; 2])>,
) -> Option<(T, bool)> {
    const TOLERANCE: f64 = 2e-3;
    let close = |left: Point3, right: Point3| {
        (left.x - right.x)
            .hypot(left.y - right.y)
            .hypot(left.z - right.z)
            < TOLERANCE
    };
    let mut matches = candidates.filter_map(|(identity, endpoints)| {
        let forward = close(loci[0], endpoints[0]) && close(loci[1], endpoints[1]);
        let reversed = close(loci[0], endpoints[1]) && close(loci[1], endpoints[0]);
        (forward != reversed).then_some((identity, reversed))
    });
    let winner = matches.next()?;
    matches.next().is_none().then_some(winner)
}

fn unique_paired_surface_lift_match<'a, T>(
    resolved_pcurve: &PcurveGeometry,
    resolved_surface: &SurfaceGeometry,
    partner_pcurve: &PcurveGeometry,
    parameter_range: [f64; 2],
    candidates: impl Iterator<Item = (T, &'a SurfaceGeometry)>,
) -> Option<T> {
    const TOLERANCE: f64 = 2e-3;
    let midpoint = parameter_range[0] + (parameter_range[1] - parameter_range[0]) * 0.5;
    let parameters = [parameter_range[0], midpoint, parameter_range[1]];
    let resolved_lift = |parameter| {
        let uv = cadmpeg_ir::eval::pcurve_uv(resolved_pcurve, parameter)?;
        cadmpeg_ir::eval::surface_point(resolved_surface, uv.u, uv.v)
    };
    let resolved_loci = [
        resolved_lift(parameters[0])?,
        resolved_lift(parameters[1])?,
        resolved_lift(parameters[2])?,
    ];
    let partner_uv = [
        cadmpeg_ir::eval::pcurve_uv(partner_pcurve, parameters[0])?,
        cadmpeg_ir::eval::pcurve_uv(partner_pcurve, parameters[1])?,
        cadmpeg_ir::eval::pcurve_uv(partner_pcurve, parameters[2])?,
    ];
    let mut matches = candidates.filter_map(|(identity, surface)| {
        resolved_loci
            .iter()
            .zip(&partner_uv)
            .all(|(resolved, uv)| {
                cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v).is_some_and(|partner| {
                    (resolved.x - partner.x)
                        .hypot(resolved.y - partner.y)
                        .hypot(resolved.z - partner.z)
                        < TOLERANCE
                })
            })
            .then_some(identity)
    });
    let winner = matches.next()?;
    matches.next().is_none().then_some(winner)
}

fn same_surface_locus(left: &SurfaceGeometry, right: &SurfaceGeometry) -> bool {
    if left == right {
        return true;
    }
    let (
        SurfaceGeometry::Cone {
            origin: left_origin,
            axis: left_axis,
            ref_direction: left_reference,
            radius: left_radius,
            ratio: left_ratio,
            half_angle: left_angle,
        },
        SurfaceGeometry::Cone {
            origin: right_origin,
            axis: right_axis,
            ref_direction: right_reference,
            radius: right_radius,
            ratio: right_ratio,
            half_angle: right_angle,
        },
    ) = (left, right)
    else {
        return false;
    };
    if left_axis != right_axis
        || left_reference != right_reference
        || left_ratio.to_bits() != right_ratio.to_bits()
        || left_angle.to_bits() != right_angle.to_bits()
    {
        return false;
    }
    let tangent = left_angle.tan();
    if !tangent.is_finite() || tangent == 0.0 {
        return false;
    }
    let apex = |origin: Point3, axis: Vector3, radius: f64| {
        Point3::new(
            origin.x - axis.x * radius / tangent,
            origin.y - axis.y * radius / tangent,
            origin.z - axis.z * radius / tangent,
        )
    };
    let left_apex = apex(*left_origin, *left_axis, *left_radius);
    let right_apex = apex(*right_origin, *right_axis, *right_radius);
    let scale = [
        left_apex.x,
        left_apex.y,
        left_apex.z,
        right_apex.x,
        right_apex.y,
        right_apex.z,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0f64, f64::max);
    (left_apex.x - right_apex.x)
        .hypot(left_apex.y - right_apex.y)
        .hypot(left_apex.z - right_apex.z)
        <= 1e-12 * scale
}

fn rechart_equivalent_surface_pcurve(
    pcurve: &PcurveGeometry,
    source: &SurfaceGeometry,
    target: &SurfaceGeometry,
) -> Option<PcurveGeometry> {
    if source == target {
        return Some(pcurve.clone());
    }
    let (
        SurfaceGeometry::Cone {
            origin: source_origin,
            axis: source_axis,
            ..
        },
        SurfaceGeometry::Cone {
            origin: target_origin,
            ..
        },
    ) = (source, target)
    else {
        return None;
    };
    if !same_surface_locus(source, target) {
        return None;
    }
    let v_shift = (source_origin.x - target_origin.x) * source_axis.x
        + (source_origin.y - target_origin.y) * source_axis.y
        + (source_origin.z - target_origin.z) * source_axis.z;
    if !v_shift.is_finite() {
        return None;
    }
    match pcurve {
        PcurveGeometry::Line { origin, direction } => Some(PcurveGeometry::Line {
            origin: Point2::new(origin.u, origin.v + v_shift),
            direction: *direction,
        }),
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => Some(PcurveGeometry::Nurbs {
            degree: *degree,
            knots: knots.clone(),
            control_points: control_points
                .iter()
                .map(|point| Point2::new(point.u, point.v + v_shift))
                .collect(),
            weights: weights.clone(),
            periodic: *periodic,
        }),
        _ => None,
    }
}

pub(crate) fn append_a8_rolling_ball_pools(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
) {
    for jet in crate::families::a5a8::records::a8_freeform_curves(data) {
        let Some(definition) = crate::families::a5a8::records::rolling_ball_jet_definition(&jet)
        else {
            continue;
        };
        let surface_id = SurfaceId(format!(
            "catia:a8-rolling-ball:surf#{}",
            ir.model.surfaces.len()
        ));
        annotate(
            annotations,
            &surface_id,
            "object_stream_a8_03_32_cache",
            jet.pos as u64,
            format!("object_id:{:08x}", jet.object_id),
            Exactness::Unknown,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: Some(cgm_source("surface", jet.object_id)),
        });

        let procedural_id = ProceduralSurfaceId(format!(
            "catia:a8-rolling-ball:construction#{}",
            ir.model.procedural_surfaces.len()
        ));
        annotate(
            annotations,
            &procedural_id,
            "object_stream_a8_03_32",
            jet.pos as u64,
            format!(
                "object_id:{:08x}:multiplicities:{:?}",
                jet.object_id, jet.multiplicities
            ),
            Exactness::ByteExact,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition,
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }
}

pub(crate) fn rolling_ball_derivative(values: [f64; 10]) -> RollingBallJetDerivative {
    RollingBallJetDerivative {
        first_limit: Vector3::new(values[0], values[1], values[2]),
        second_limit: Vector3::new(values[3], values[4], values[5]),
        center: Vector3::new(values[6], values[7], values[8]),
        angle: values[9],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        append_freeform_surface_pools, append_resolved_consolidated_surface_curves,
        freeform_surface_carriers, rechart_equivalent_surface_pcurve, same_surface_locus,
        unique_endpoint_pair_match, unique_paired_surface_lift_match,
    };
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, PcurveGeometry,
        ProceduralCurve, ProceduralCurveDefinition, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ProceduralCurveId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Sense, Vertex};
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::AnnotationBuilder;

    #[test]
    fn rolling_ball_pool_retains_both_exact_limiting_curves() {
        let mut ir = CadIr::empty(Units::default());
        append_freeform_surface_pools(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &crate::tests::a5_freeform_curve_stream(),
        );

        assert!(matches!(
            ir.model.curves.as_slice(),
            [Curve {
                geometry: CurveGeometry::Nurbs(first),
                ..
            }, Curve {
                geometry: CurveGeometry::Nurbs(second),
                ..
            }] if first.degree == 5
                && second.degree == 5
                && first.control_points.first() == Some(&Point3::new(1.0, 0.0, 0.0))
                && second.control_points.first() == Some(&Point3::new(0.0, 1.0, 0.0))
        ));
    }

    #[test]
    fn freeform_fallback_distinguishes_grouped_and_standalone_cylinders() {
        let mut bytes = crate::tests::b2_cylinder_stream();
        bytes.extend_from_slice(&crate::tests::b2_embedded_cylinder_stream());

        let carriers = freeform_surface_carriers(&bytes);
        assert_eq!(carriers.len(), 2);
        assert!(carriers[0].source_tag.starts_with("b2_03_28:"));
        assert!(carriers[1].source_tag.starts_with("b2_03_60:"));
    }

    #[test]
    fn endpoint_pair_binding_requires_one_unordered_match() {
        let loci = [Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)];
        let unique = unique_endpoint_pair_match(
            loci,
            [
                (
                    7,
                    [Point3::new(4.0, 5.0, 6.001), Point3::new(1.0, 2.0, 3.001)],
                ),
                (8, [Point3::new(0.0, 0.0, 0.0), Point3::new(4.0, 5.0, 6.0)]),
            ]
            .into_iter(),
        );
        assert_eq!(unique, Some((7, true)));

        let ambiguous = unique_endpoint_pair_match(
            loci,
            [
                (7, loci),
                (
                    8,
                    [Point3::new(1.0, 2.0, 3.001), Point3::new(4.0, 5.0, 6.001)],
                ),
            ]
            .into_iter(),
        );
        assert_eq!(ambiguous, None);
    }

    #[test]
    fn paired_surface_lifts_require_one_matching_carrier() {
        let plane = |z| SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, z),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let pcurve = PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(1.0, 0.0),
        };
        let resolved = plane(0.0);
        let matching = plane(0.001);
        let distant = plane(1.0);
        assert_eq!(
            unique_paired_surface_lift_match(
                &pcurve,
                &resolved,
                &pcurve,
                [0.0, 1.0],
                [(7, &matching), (8, &distant)].into_iter(),
            ),
            Some(7)
        );
        assert_eq!(
            unique_paired_surface_lift_match(
                &pcurve,
                &resolved,
                &pcurve,
                [0.0, 1.0],
                [(7, &matching), (9, &matching)].into_iter(),
            ),
            None
        );
    }

    #[test]
    fn cone_locus_equality_accepts_only_the_same_apex_shift() {
        let cone = |origin, radius| SurfaceGeometry::Cone {
            origin,
            axis: Vector3::new(-1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius,
            ratio: 1.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        };
        let apex_form = cone(Point3::new(111.0, 0.0, 0.0), 0.0);
        let shifted = cone(Point3::new(107.5, 0.0, 0.0), 3.5);
        let other = cone(Point3::new(107.0, 0.0, 0.0), 3.5);
        assert!(same_surface_locus(&apex_form, &shifted));
        assert!(!same_surface_locus(&apex_form, &other));
    }

    #[test]
    fn equivalent_cone_pcurve_moves_to_the_target_axial_origin() {
        let cone = |origin, radius| SurfaceGeometry::Cone {
            origin,
            axis: Vector3::new(-1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius,
            ratio: 1.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        };
        let source = cone(Point3::new(107.5, 0.0, 0.0), 3.5);
        let target = cone(Point3::new(111.0, 0.0, 0.0), 0.0);
        let pcurve = PcurveGeometry::Line {
            origin: Point2::new(0.25, 1.5),
            direction: Point2::new(2.0, -0.5),
        };
        assert_eq!(
            rechart_equivalent_surface_pcurve(&pcurve, &source, &target),
            Some(PcurveGeometry::Line {
                origin: Point2::new(0.25, 5.0),
                direction: Point2::new(2.0, -0.5),
            })
        );
    }

    #[test]
    fn consolidated_surface_curve_reuses_one_matching_unresolved_edge() {
        let mut ir = CadIr::empty(Units::default());
        let points = [
            Point3::new(1.0, 4.0, 3.0),
            Point3::new(2.0, 2.0 + 2.0 * 0.5f64.cos(), 3.0 + 2.0 * 0.5f64.sin()),
        ];
        let mut bytes = crate::tests::b2_cylinder_stream();
        for point in points {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [point.x, point.y, point.z] {
                bytes.extend_from_slice(&(value as f32).to_le_bytes());
            }
        }
        let mut edge_run = crate::tests::a5_native_edge_run_stream(6, 139, 142);
        let second_pcurve = crate::tests::a5_pcurve_stream().len();
        for (offset, value) in [10.0f64, 11.0, 20.0, 21.0].into_iter().enumerate() {
            let start = second_pcurve + 33 + 8 * offset;
            edge_run[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&edge_run);
        let cylinder = crate::families::b2::records::b2_cylinders(&bytes)
            .into_iter()
            .next()
            .expect("one exact cylinder")
            .geometry;

        for (index, position) in points.into_iter().enumerate() {
            ir.model.points.push(Point {
                id: PointId(format!("point#{index}")),
                position,
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: VertexId(format!("vertex#{index}")),
                point: PointId(format!("point#{index}")),
                tolerance: None,
            });
        }
        let curve_id = CurveId("standard-curve".to_string());
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId("standard-edge".to_string()),
            curve: Some(curve_id.clone()),
            start: VertexId("vertex#1".to_string()),
            end: VertexId("vertex#0".to_string()),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        });
        let support_ids = [
            SurfaceId("support#0".to_string()),
            SurfaceId("support#1".to_string()),
        ];
        ir.model.surfaces.push(Surface {
            id: support_ids[0].clone(),
            geometry: cylinder,
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: support_ids[1].clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        for (side, support_id) in support_ids.iter().enumerate() {
            let face_id = FaceId(format!("face#{side}"));
            let loop_id = LoopId(format!("loop#{side}"));
            let coedge_id = CoedgeId(format!("coedge#{side}"));
            ir.model.faces.push(Face {
                id: face_id.clone(),
                shell: ShellId("shell".to_string()),
                surface: support_id.clone(),
                sense: Sense::Forward,
                loops: vec![loop_id.clone()],
                name: None,
                color: None,
                tolerance: None,
            });
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id,
                boundary_role: LoopBoundaryRole::Unspecified,
                coedges: vec![coedge_id.clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id,
                edge: EdgeId("standard-edge".to_string()),
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: CoedgeId(format!("coedge#{}", 1 - side)),
                sense: if side == 0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
        }
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId("standard-intersection".to_string()),
            curve: curve_id.clone(),
            definition: ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: std::array::from_fn(|side| IntcurveSupportSide {
                        surface: Some(support_ids[side].clone()),
                        pcurve: None,
                        pcurve_parameter_range: None,
                    }),
                    parameter_range: [0.0, 1.0],
                    discontinuities: std::array::from_fn(|_| Vec::new()),
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });

        let attached = append_resolved_consolidated_surface_curves(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &bytes,
            &[],
            &[],
        );
        assert_eq!(attached.standard_edges, 1);
        assert_eq!(attached.partner_face_pcurve_pairs, 1);
        assert_eq!(ir.model.pcurves.len(), 2);
        assert!(ir
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.len() == 1));
        assert_eq!(ir.model.curves.len(), 1);
        assert_eq!(ir.model.edges[0].curve.as_ref(), Some(&curve_id));
        let ProceduralCurveDefinition::SurfaceCurve { context, .. } =
            &ir.model.procedural_curves[0].definition
        else {
            panic!("one exact support remains a parametric surface curve");
        };
        assert_eq!(
            context
                .sides
                .iter()
                .filter(|side| side.pcurve.is_some())
                .count(),
            1
        );
        let start = cadmpeg_ir::eval::pcurve_uv(
            context.sides[0].pcurve.as_ref().expect("first pcurve"),
            context.parameter_range[0],
        )
        .expect("reversed pcurve start");
        assert_eq!([start.u, start.v], [0.5, 1.0]);
        assert_eq!(ir.model.edges[0].param_range, Some([0.0, 1.0]));
    }

    #[test]
    fn freeform_fallback_retains_exact_consolidated_spheres() {
        let carriers = freeform_surface_carriers(&crate::tests::b2_sphere_stream());
        assert!(matches!(
            carriers.as_slice(),
            [carrier]
                if matches!(
                    carrier.geometry,
                    SurfaceGeometry::Sphere {
                        center,
                        axis,
                        ref_direction,
                        radius: 5.0,
                    } if center == Point3::new(1.0, 2.0, 3.0)
                        && axis == Vector3::new(0.0, 0.0, 1.0)
                        && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                )
        ));
    }

    #[test]
    fn freeform_fallback_retains_exact_consolidated_tori() {
        let carriers = freeform_surface_carriers(&crate::tests::b2_torus_stream());
        assert!(matches!(
            carriers.as_slice(),
            [carrier]
                if matches!(
                    carrier.geometry,
                    SurfaceGeometry::Torus {
                        center,
                        axis,
                        ref_direction,
                        major_radius: 7.0,
                        minor_radius: 2.0,
                    } if center == Point3::new(1.0, 2.0, 3.0)
                        && axis == Vector3::new(0.0, 0.0, 1.0)
                        && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                )
        ));
    }

    #[test]
    fn freeform_fallback_retains_range_origin_cylinder_carriers() {
        let carriers = freeform_surface_carriers(&crate::tests::b2_range_origin_cylinder_stream());
        assert!(matches!(
            carriers.as_slice(),
            [carrier]
                if matches!(
                    carrier.geometry,
                    SurfaceGeometry::Cylinder {
                        origin,
                        axis,
                        ref_direction,
                        radius: 4.0,
                    } if origin == Point3::new(0.0, 0.0, 0.0)
                        && axis == Vector3::new(0.0, 1.0, 0.0)
                        && ref_direction == Vector3::new(0.0, 0.0, 1.0)
                )
        ));
    }
}
