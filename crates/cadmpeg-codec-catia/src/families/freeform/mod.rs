// SPDX-License-Identifier: Apache-2.0
//! Freeform decode route composing a5a8 and consolidated NURBS record carriers.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, PcurveGeometry,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    RollingBallJetDerivative, RollingBallJetSite, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use std::collections::HashMap;

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
    let typed_face_counts = b5_graph.as_ref().map(|graph| {
        let controls = graph
            .face_records
            .values()
            .fold([0usize; 3], |mut counts, face| {
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
            graph
                .face_records
                .len()
                .checked_sub(graph.faces.len())
                .expect("resolved faces are a subset of typed face records"),
        ]
    });
    let edge_terminal_controls = b5_graph.as_ref().map(|graph| {
        graph.edges.values().fold([0usize; 8], |mut counts, edge| {
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
    let vertex_incidence_terminal_controls = b5_graph.as_ref().map(|graph| {
        graph
            .vertex_incidence_links
            .values()
            .fold([0usize; 2], |mut counts, link| {
                match link.terminal_control {
                    0x00 => counts[0] += 1,
                    0x04 => counts[1] += 1,
                    _ => unreachable!("the vertex-incidence parser admits only controls 00 and 04"),
                }
                counts
            })
    });
    let loop_metadata_counts = b5_graph.as_ref().map(|graph| {
        graph.loops.values().fold([0usize; 5], |mut counts, loop_| {
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
    });
    let class_21_suffix_scalar_count = b5_graph.as_ref().map(|graph| {
        graph
            .pcurves
            .values()
            .filter(|pcurve| pcurve.class_21_suffix_scalar.is_some())
            .count()
    });
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
        loop_metadata_counts
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
    if let Some(count) = class_21_suffix_scalar_count {
        coverage.insert(
            "resolved_object_stream_class_21_pcurve_suffix_scalar_count".to_string(),
            count,
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

pub(crate) fn append_freeform_surface_pools(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
) {
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
    append_resolved_consolidated_surface_curves(ir, annotations, data, &surfaces, &carrier_ids);
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ConsolidatedCarrierKey {
    Cylinder(usize),
    EmbeddedCylinder(usize),
    Cone(usize),
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
        }
    }

    fn derivative(&self, [u, v]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Identity => [u, v],
            Self::Cylinder { radius } => [u / radius, v],
            Self::Cone { cone } => [u / cone.angular_scale, v * cone.half_angle.cos()],
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

pub(crate) fn append_resolved_consolidated_surface_curves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
    freeform_surfaces: &[crate::families::a5a8::records::FreeformSurface],
    freeform_surface_ids: &[SurfaceId],
) {
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
    let complete_runs =
        crate::families::consolidated::records::consolidated_topology_edge_runs(data)
            .into_iter()
            .filter(|run| run.edge.co_parametric && run.identity_chain_consistent)
            .map(|run| (run.edge.pcurves[0].pos, run))
            .collect::<HashMap<_, _>>();

    let mut surface_ids = HashMap::<ConsolidatedCarrierKey, SurfaceId>::new();

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
        let resolved_side_count = sides.iter().filter(|side| side.surface.is_some()).count();
        if resolved_side_count == 0 {
            continue;
        }
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
        let context = IntcurveSupportContext {
            sides,
            parameter_range: resolved.block.parameters.range,
            discontinuities: std::array::from_fn(|_| Vec::new()),
        };
        let definition = if resolved_side_count == 2 {
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
        ir.model.procedural_curves.push(ProceduralCurve {
            id: procedural_id,
            curve: curve_id,
            definition,
            cache_fit_tolerance: None,
        });
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
    use super::freeform_surface_carriers;
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

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
