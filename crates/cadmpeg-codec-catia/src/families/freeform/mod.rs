// SPDX-License-Identifier: Apache-2.0
//! Freeform decode route composing a5a8 and consolidated NURBS record carriers.

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, Pcurve,
    PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite, Surface,
    SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CurveId, EdgeId, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, RegionId,
    ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::topology::{Body, BodyKind, Edge, Point, Region, Shell, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::assemble::{
    annotate, insert_unresolved_carrier_loss, link_payload_carriers, neutral_model_is_admissible,
    preserve_raw_payload, quintic_jet_pcurve, source_meta,
};
use crate::assemble::{cgm_source, cgm_source_key};
use crate::container::{self, ContainerScan};
use crate::families::FamilyOutput;
use crate::loss::CatiaLossCode;

const EPS_TORUS_FRAME: f64 = 1.0e-12;
const EPS_APEX_ALIGNMENT: f64 = 1.0e-12;

#[derive(Clone)]
struct FreeformSurfaceCarrier {
    pos: usize,
    geometry: SurfaceGeometry,
    source_object: cadmpeg_ir::SourceObjectAssociation,
    source_tag: String,
}

#[derive(Clone)]
pub(crate) struct ConsolidatedRevolutionBinding {
    pub(crate) geometry: SurfaceGeometry,
    pub(crate) profile_sweep: f64,
}

/// Transfer resolved consolidated axis-and-profile revolution carriers.
pub(crate) fn append_consolidated_revolutions(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    resolved: &[crate::families::b2::records::B2ResolvedRevolution],
) -> Vec<ConsolidatedRevolutionBinding> {
    let mut bindings = Vec::new();
    for carrier in resolved {
        let index = carrier.revolution_index;
        let revolution = &carrier.revolution;
        let profile = &carrier.profile;
        let direction_x = Vector3::new(
            revolution.direction_x[0],
            revolution.direction_x[1],
            revolution.direction_x[2],
        );
        let direction_y = Vector3::new(
            revolution.direction_y[0],
            revolution.direction_y[1],
            revolution.direction_y[2],
        );
        let axis = Vector3::new(revolution.axis[0], revolution.axis[1], revolution.axis[2]);
        let origin = Point3::new(
            revolution.origin[0],
            revolution.origin[1],
            revolution.origin[2],
        );
        let transverse_coordinate =
            origin.x * direction_x.x + origin.y * direction_x.y + origin.z * direction_x.z;
        let center = Point3::new(
            transverse_coordinate * direction_x.x
                + profile.center_pair[0] * direction_y.x
                + profile.center_pair[1] * axis.x,
            transverse_coordinate * direction_x.y
                + profile.center_pair[0] * direction_y.y
                + profile.center_pair[1] * axis.y,
            transverse_coordinate * direction_x.z
                + profile.center_pair[0] * direction_y.z
                + profile.center_pair[1] * axis.z,
        );
        let directrix = CurveId(format!(
            "catia:consolidated:surface-revolution-directrix#{index}"
        ));
        annotate(
            annotations,
            &directrix,
            "consolidated_b2_03_19",
            profile.pos as u64,
            format!("circle:{}", profile.record_id),
            Exactness::ByteExact,
        );
        ir.model.curves.push(Curve {
            id: directrix.clone(),
            geometry: CurveGeometry::Circle {
                center,
                axis: direction_x,
                ref_direction: direction_y,
                radius: profile.radius,
            },
            source_object: Some(cgm_source("profile-circle", profile.record_id)),
        });
        let surface = SurfaceId(format!(
            "catia:consolidated:surface-revolution-surface#{index}"
        ));
        let center_offset = Vector3::new(
            center.x - origin.x,
            center.y - origin.y,
            center.z - origin.z,
        );
        let axis_coordinate =
            center_offset.x * axis.x + center_offset.y * axis.y + center_offset.z * axis.z;
        let radial = Vector3::new(
            center_offset.x - axis.x * axis_coordinate,
            center_offset.y - axis.y * axis_coordinate,
            center_offset.z - axis.z * axis_coordinate,
        );
        let major_radius = radial.x.hypot(radial.y).hypot(radial.z);
        let profile_plane_contains_axis =
            (direction_x.x * axis.x + direction_x.y * axis.y + direction_x.z * axis.z).abs()
                <= EPS_TORUS_FRAME;
        let radial_follows_profile_reference = major_radius > 0.0
            && ((radial.x * direction_y.x + radial.y * direction_y.y + radial.z * direction_y.z)
                .abs()
                / major_radius
                - 1.0)
                .abs()
                <= EPS_TORUS_FRAME;
        let torus_geometry = (major_radius > 0.0
            && major_radius.is_finite()
            && profile.radius > 0.0
            && profile.radius.is_finite()
            && profile_plane_contains_axis
            && radial_follows_profile_reference)
            .then(|| {
                let ref_direction = Vector3::new(
                    radial.x / major_radius,
                    radial.y / major_radius,
                    radial.z / major_radius,
                );
                let torus_center = Point3::new(
                    center.x - radial.x,
                    center.y - radial.y,
                    center.z - radial.z,
                );
                SurfaceGeometry::Torus {
                    center: torus_center,
                    axis,
                    ref_direction,
                    major_radius,
                    minor_radius: profile.radius,
                }
            });
        annotate(
            annotations,
            &surface,
            "consolidated_b2_03_2d",
            revolution.pos as u64,
            format!("profile-allocation:{}", revolution.profile_allocation_id),
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: torus_geometry
                .clone()
                .unwrap_or(SurfaceGeometry::Unknown { record: None }),
            source_object: Some(cgm_source(
                "revolution",
                u32::from(revolution.profile_allocation_id),
            )),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("catia:consolidated:surface-revolution#{index}")),
            surface,
            definition: ProceduralSurfaceDefinition::Revolution {
                directrix,
                axis_origin: origin,
                axis_direction: axis,
                angular_interval: [
                    revolution.angular_range[0] / revolution.angular_scale,
                    revolution.angular_range[1] / revolution.angular_scale,
                ],
                angular_parameter_interval: Some(revolution.angular_range),
                parameter_interval: Some(revolution.profile_range),
                transposed: false,
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        if let Some(geometry) = torus_geometry {
            bindings.push(ConsolidatedRevolutionBinding {
                geometry,
                profile_sweep: (revolution.profile_range[1] - revolution.profile_range[0]).abs()
                    / profile.radius,
            });
        }
    }
    bindings
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

fn typed_multi_surface_face_count(graph: &crate::families::b5::graph::B5Graph) -> usize {
    graph
        .face_records
        .values()
        .filter(|face| {
            let Some(&carrier) = face.references.first() else {
                return false;
            };
            let Some(canonical_carrier) =
                crate::families::b5::graph::canonical_surface_id(&graph.surface_aliases, carrier)
            else {
                return false;
            };
            face.references[1..].iter().any(|reference| {
                graph.surfaces.contains_key(reference)
                    && crate::families::b5::graph::canonical_surface_id(
                        &graph.surface_aliases,
                        *reference,
                    )
                    .is_some_and(|candidate| candidate != canonical_carrier)
            })
        })
        .count()
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

pub(crate) fn try_decode_freeform_surfaces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Option<FamilyOutput> {
    let logical_streams = container::logical_record_streams(scan);
    let selection_budget =
        ctx.work_budget(crate::families::b5::graph::MAX_OBJECT_STREAM_SELECTION_WORK as u64);
    let object_selection = crate::families::b5::graph::select_object_stream_population(
        &logical_streams,
        Some(&selection_budget),
    );
    let object_stream_run_count = object_selection.run_count;
    let selected_object_stream_run_count = usize::from(object_selection.selected);
    let object_stream_selection_exhausted = object_selection.exhausted;
    let object_source = object_selection.source;
    let object_frames = object_selection.frames;
    let selected_object_records = object_selection.records;
    let census_object_records = object_selection.census_records;
    let consolidated_records = crate::wire::records::consolidated_records_in_sources(
        &scan.data,
        container::consolidated_record_sources(scan),
    );
    let mut b5_graph = crate::families::b5::graph::parse_from_records_budgeted(
        &object_source,
        &selected_object_records,
        &object_frames,
        true,
        Some(&selection_budget),
    );
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
        let records =
            crate::families::b5::graph::typed_face_records_from_records(&census_object_records);
        (!records.is_empty()).then(|| typed_face_counts(&records, 0))
    };
    let typed_multi_surface_face_count = b5_graph
        .as_ref()
        .map(typed_multi_surface_face_count)
        .unwrap_or_default();
    let typed_edge_records =
        crate::families::b5::graph::typed_edge_records_from_records(&census_object_records);
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
        crate::families::b5::graph::typed_vertex_incidence_links_from_records(
            &census_object_records,
        );
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
    let typed_loop_records =
        crate::families::b5::graph::typed_loop_records_from_records(&census_object_records);
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
        crate::families::b5::graph::typed_class_21_pcurves_from_records(&census_object_records)
            .len();
    let typed_parameter_incidences =
        crate::families::b5::graph::typed_parameter_incidences_from_records(&census_object_records);
    let typed_parameter_incidence_member_count = typed_parameter_incidences
        .values()
        .map(|incidence| incidence.curves.len())
        .sum();
    let typed_vertex_incidence_rosters =
        crate::families::b5::graph::typed_vertex_incidence_rosters_from_records(
            &census_object_records,
        );
    let typed_vertex_incidence_roster_member_count =
        typed_vertex_incidence_rosters.values().map(Vec::len).sum();
    let mut fallback_surfaces = b5_graph
        .is_none()
        .then(|| freeform_surface_carriers(&scan.data, &consolidated_records));
    let b2_nurbs_curves = crate::families::b2::records::b2_nurbs_curves_from_records(
        &scan.data,
        &consolidated_records,
    );
    let b2_nurbs_curve_count = b2_nurbs_curves.len();
    let a5_nurbs_curves = crate::families::a5a8::records::a5_nurbs_curves_from_records(
        &scan.data,
        &consolidated_records,
    );
    let a5_nurbs_curve_count = a5_nurbs_curves.len();
    let b2_spatial_circles = crate::families::b2::records::b2_spatial_circles_from_records(
        &scan.data,
        &consolidated_records,
    );
    let b2_line_profile_count = crate::families::b2::records::b2_line_profiles_from_records(
        &scan.data,
        &consolidated_records,
    )
    .len();
    let resolved_consolidated_revolutions =
        crate::families::b2::records::b2_resolved_revolutions_from_records(
            &scan.data,
            &consolidated_records,
        );
    let resolved_consolidated_revolution_count = resolved_consolidated_revolutions.len();
    let b2_spatial_circle_count = b2_spatial_circles.len();
    if fallback_surfaces.as_ref().is_some_and(Vec::is_empty)
        && crate::families::a5a8::records::a8_freeform_curves(&scan.data).is_empty()
        && b2_nurbs_curves.is_empty()
        && a5_nurbs_curves.is_empty()
        && b2_spatial_circles.is_empty()
        && b2_line_profile_count == 0
        && resolved_consolidated_revolution_count == 0
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
        ) && neutral_model_is_admissible(&mut topology_ir, &unknowns)
    });
    if topology_transferred {
        ir = topology_ir;
        annotations = topology_annotations;
    }
    if !topology_transferred {
        let surfaces = fallback_surfaces
            .take()
            .unwrap_or_else(|| freeform_surface_carriers(&scan.data, &consolidated_records));
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
    let _ = append_consolidated_revolutions(
        &mut ir,
        &mut annotations,
        &resolved_consolidated_revolutions,
    );
    append_a8_rolling_ball_pools(&mut ir, &mut annotations, &scan.data);
    let mut standalone_wires = append_consolidated_line_profiles(
        &mut ir,
        &mut annotations,
        &scan.data,
        &consolidated_records,
    );
    for curve in b2_nurbs_curves {
        let id = CurveId(format!("catia:b2:nurbs-curve#{}", ir.model.curves.len()));
        let parameter_range = [
            *curve.geometry.knots.first().expect("parsed knot vector"),
            *curve.geometry.knots.last().expect("parsed knot vector"),
        ];
        annotate(
            &mut annotations,
            &id,
            "consolidated_b2_03_16",
            curve.pos as u64,
            format!("header_token:{:08x}", curve.header_token),
            Exactness::ByteExact,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Nurbs(curve.geometry),
            source_object: Some(cgm_source_key(
                "b2-nurbs-curve-frame",
                format!("{:010}", curve.pos),
            )),
        });
        standalone_wires.push((id, parameter_range, curve.pos));
    }
    for curve in a5_nurbs_curves {
        let id = CurveId(format!("catia:a5:nurbs-curve#{}", ir.model.curves.len()));
        let parameter_range = [
            *curve.geometry.knots.first().expect("parsed knot vector"),
            *curve.geometry.knots.last().expect("parsed knot vector"),
        ];
        annotate(
            &mut annotations,
            &id,
            "consolidated_a5_13_16",
            curve.pos as u64,
            format!("header_token:{:08x}", curve.header_token),
            Exactness::ByteExact,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Nurbs(curve.geometry),
            source_object: Some(cgm_source_key(
                "a5-nurbs-curve-frame",
                format!("{:010}", curve.pos),
            )),
        });
        standalone_wires.push((id, parameter_range, curve.pos));
    }
    for circle in b2_spatial_circles {
        let id = CurveId(format!("catia:b2:circle#{}", ir.model.curves.len()));
        let parameter_range = [
            circle.range[0] / circle.radius,
            circle.range[1] / circle.radius,
        ];
        annotate(
            &mut annotations,
            &id,
            "consolidated_b2_03_0f",
            circle.pos as u64,
            format!(
                "header_token:{:08x}:range:{:?}:chart_shift:{}",
                circle.header_token, circle.range, circle.chart_shift
            ),
            Exactness::ByteExact,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Circle {
                center: circle.center,
                axis: circle.axis,
                ref_direction: circle.ref_direction,
                radius: circle.radius,
            },
            source_object: Some(cgm_source_key(
                "b2-spatial-circle-frame",
                format!("{:010}", circle.pos),
            )),
        });
        standalone_wires.push((id, parameter_range, circle.pos));
    }
    let wire_topology_transferred = !topology_transferred
        && ir.model.surfaces.is_empty()
        && standalone_wires.len() == ir.model.curves.len()
        && !standalone_wires.is_empty()
        && attach_standalone_wires(&mut ir, &mut annotations, &standalone_wires);
    let mut losses = if wire_topology_transferred {
        Vec::new()
    } else if topology_transferred && b5_complete {
        vec![CatiaLossCode::TopologyB5GaugeSubstituted.note(
            "The B5 reference graph is closed; face sense and body kind use a deterministic topology gauge because their source fields remain unresolved.",
        )]
    } else if topology_transferred {
        vec![CatiaLossCode::TopologyB5SubsetIncomplete.note(
            "A maximal reference-closed B5 face/loop/pcurve/edge subset was transferred; variant nodes and unresolved endpoint lifts remain outside the connected graph.",
        )]
    } else if object_stream_selection_exhausted {
        vec![CatiaLossCode::TopologyObjectStreamWorkSliceExhausted.note(
            "The object-stream graph exceeds the bounded frame-index and record-materialization work slice; its topology remains native.",
        )]
    } else {
        vec![CatiaLossCode::TopologyB5GraphUnclosed.note(
            "Object-stream and consolidated NURBS carriers were decoded, but the face/loop/pcurve/edge graph did not close.",
        )]
    };
    insert_unresolved_carrier_loss(&ir, &mut losses);
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();
    let mut coverage = std::collections::BTreeMap::new();
    coverage.insert(
        "decoded_object_stream_run_count".to_string(),
        object_stream_run_count,
    );
    coverage.insert(
        "selected_object_stream_run_count".to_string(),
        selected_object_stream_run_count,
    );
    coverage.insert(
        "unselected_object_stream_run_count".to_string(),
        object_stream_run_count - selected_object_stream_run_count,
    );
    coverage.insert(
        "exhausted_object_stream_selection_count".to_string(),
        usize::from(object_stream_selection_exhausted),
    );
    coverage.insert(
        "decoded_b2_nurbs_curve_count".to_string(),
        b2_nurbs_curve_count,
    );
    coverage.insert(
        "decoded_a5_nurbs_curve_count".to_string(),
        a5_nurbs_curve_count,
    );
    coverage.insert(
        "decoded_b2_spatial_circle_count".to_string(),
        b2_spatial_circle_count,
    );
    coverage.insert(
        "attached_standalone_wire_edge_count".to_string(),
        usize::from(wire_topology_transferred) * standalone_wires.len(),
    );
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
    if topology_transferred {
        coverage.insert(
            "transferred_object_stream_face_count".to_string(),
            ir.model.faces.len(),
        );
        coverage.insert(
            "transferred_object_stream_loop_count".to_string(),
            ir.model.loops.len(),
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
    if typed_multi_surface_face_count != 0 {
        coverage.insert(
            crate::coverage::TYPED_MULTI_SURFACE_OBJECT_STREAM_FACE_COUNT
                .0
                .to_string(),
            typed_multi_surface_face_count,
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
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses,
            notes: container::summarize(scan).notes,
        },
        annotations,
        unknowns,
        standard_face_population: false,
    })
}

fn attach_standalone_wires(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    wires: &[(CurveId, [f64; 2], usize)],
) -> bool {
    let plans = wires
        .iter()
        .enumerate()
        .map(|(index, (curve_id, range, pos))| {
            let geometry = &ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)?
                .geometry;
            let start = cadmpeg_ir::eval::curve_point(geometry, range[0])?;
            let end = cadmpeg_ir::eval::curve_point(geometry, range[1])?;
            Some((index, curve_id.clone(), *range, *pos, start, end))
        })
        .collect::<Option<Vec<_>>>();
    let Some(plans) = plans else {
        return false;
    };
    let body_id = BodyId("catia:freeform:wire-body#0".to_string());
    let region_id = RegionId("catia:freeform:wire-region#0".to_string());
    let shell_id = ShellId("catia:freeform:wire-shell#0".to_string());
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotate(
            annotations,
            id,
            "consolidated_curve_wire",
            0,
            "standalone_wire_owner",
            Exactness::Inferred,
        );
    }
    let mut edge_ids = Vec::with_capacity(plans.len());
    for (index, curve_id, range, pos, start, end) in plans {
        let point_ids = [
            PointId(format!("catia:freeform:wire-point#{index}:start")),
            PointId(format!("catia:freeform:wire-point#{index}:end")),
        ];
        let vertex_ids = [
            VertexId(format!("catia:freeform:wire-vertex#{index}:start")),
            VertexId(format!("catia:freeform:wire-vertex#{index}:end")),
        ];
        let edge_id = EdgeId(format!("catia:freeform:wire-edge#{index}"));
        for id in [
            &point_ids[0].0,
            &point_ids[1].0,
            &vertex_ids[0].0,
            &vertex_ids[1].0,
            &edge_id.0,
        ] {
            annotate(
                annotations,
                id,
                "consolidated_curve_wire",
                pos as u64,
                "curve_domain_endpoint",
                Exactness::Derived,
            );
        }
        ir.model.points.extend([
            Point {
                id: point_ids[1].clone(),
                position: end,
                source_object: None,
            },
            Point {
                id: point_ids[0].clone(),
                position: start,
                source_object: None,
            },
        ]);
        ir.model.vertices.extend([
            Vertex {
                id: vertex_ids[1].clone(),
                point: point_ids[1].clone(),
                tolerance: None,
            },
            Vertex {
                id: vertex_ids[0].clone(),
                point: point_ids[0].clone(),
                tolerance: None,
            },
        ]);
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            curve: Some(curve_id),
            start: vertex_ids[0].clone(),
            end: vertex_ids[1].clone(),
            param_range: Some(range),
            tolerance: None,
        });
        edge_ids.push(edge_id);
    }
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Wire,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(Shell {
        id: shell_id,
        region: region_id,
        faces: Vec::new(),
        wire_edges: edge_ids,
        free_vertices: Vec::new(),
    });
    true
}

fn freeform_surface_carriers(
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
) -> Vec<FreeformSurfaceCarrier> {
    let mut surfaces = crate::families::a5a8::records::resolved_a8_surfaces(data)
        .into_iter()
        .chain(crate::families::a5a8::records::a5_surfaces_from_records(
            data, records,
        ))
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
        crate::families::b2::records::b2_cylinders_from_records(data, records)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: surface.geometry,
                source_object: cgm_source_key("b2-03-28-frame", format!("{:010}", surface.pos)),
                source_tag: format!("b2_03_28:frame_offset:{:010}", surface.pos),
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_embedded_cylinders_from_records(data, records)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: surface.cylinder.geometry,
                source_object: cgm_source("surface", surface.object_id),
                source_tag: format!("b2_03_60:object_id:{:08x}", surface.object_id),
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_cones_from_records(data, records)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: crate::families::b2::records::b2_cone_geometry(&surface),
                source_object: cgm_source_key("b2-03-29-frame", format!("{:010}", surface.pos)),
                source_tag: format!("b2_03_29:frame_offset:{:010}", surface.pos),
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_spheres_from_records(data, records)
            .into_iter()
            .map(|surface| FreeformSurfaceCarrier {
                pos: surface.pos,
                geometry: crate::families::b2::records::b2_sphere_geometry(&surface),
                source_object: cgm_source_key("b2-03-2a-frame", format!("{:010}", surface.pos)),
                source_tag: format!("b2_03_2a:frame_offset:{:010}", surface.pos),
            }),
    );
    surfaces.extend(
        crate::families::b2::records::b2_tori_from_records(data, records)
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

/// Index standard carrier surfaces by their serialized carrier tag.
///
/// A consolidated pcurve support id is admitted as a standard carrier only
/// when that tag selects one decoded surface with known geometry. Duplicate
/// tags and unknown geometry remain explicitly unresolved; an allocation id
/// must not choose a face-local row by emission order.
fn standard_carrier_surface_ids(ir: &CadIr) -> HashMap<u32, Option<SurfaceId>> {
    let mut by_tag = HashMap::new();
    for surface in &ir.model.surfaces {
        let Some(source) = surface.source_object.as_ref() else {
            continue;
        };
        if source.format != "catia" {
            continue;
        }
        let Some(tag) = source
            .object_id
            .strip_prefix("cgm-carrier:")
            .and_then(|value| u32::from_str_radix(value, 16).ok())
        else {
            continue;
        };
        let candidate = (!matches!(surface.geometry, SurfaceGeometry::Unknown { .. }))
            .then(|| surface.id.clone());
        by_tag
            .entry(tag)
            .and_modify(|selected: &mut Option<SurfaceId>| *selected = None)
            .or_insert(candidate);
    }
    by_tag
}

fn standard_carrier_endpoint_loci(
    pcurve: &PcurveGeometry,
    surface: &SurfaceGeometry,
    range: [f64; 2],
) -> Option<[Point3; 2]> {
    let start = cadmpeg_ir::eval::pcurve_uv(pcurve, range[0])?;
    let end = cadmpeg_ir::eval::pcurve_uv(pcurve, range[1])?;
    Some([
        cadmpeg_ir::eval::surface_point(surface, start.u, start.v)?,
        cadmpeg_ir::eval::surface_point(surface, end.u, end.v)?,
    ])
}

/// Transfer every exact consolidated line carrier independently of its parameter chart.
fn append_consolidated_line_profiles(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
) -> Vec<(CurveId, [f64; 2], usize)> {
    let mut standalone_wires = Vec::new();
    for (index, line) in crate::families::b2::records::b2_line_profiles_from_records(data, records)
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
            id: id.clone(),
            geometry: CurveGeometry::Line {
                origin: Point3::new(line.origin[0], line.origin[1], line.origin[2]),
                direction: Vector3::new(line.direction[0], line.direction[1], line.direction[2]),
            },
            source_object: Some(cgm_source_key(
                "b2-03-0e-frame",
                format!("{:010}", line.pos),
            )),
        });
        standalone_wires.push((id, line.range, line.pos));
    }
    standalone_wires
}

/// Append standalone freeform carriers and return the number of consolidated
/// surface curves bound to existing standard edges.
pub(crate) fn append_freeform_surface_pools(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
    surface_alias_tags: &HashMap<u32, Option<u32>>,
) -> ConsolidatedCurveBindingCounts {
    let mut surfaces = crate::families::a5a8::records::resolved_a8_surfaces(data);
    surfaces.extend(crate::families::a5a8::records::a5_surfaces_from_records(
        data, records,
    ));
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

    let offsets = crate::families::b2::records::b2_offset_supports_from_records(data, records);
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
                u_sense: None,
                v_sense: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: Some([
                Some(offset.domain[0]),
                Some(offset.domain[1]),
                Some(offset.domain[2]),
                Some(offset.domain[3]),
            ]),
        });
    }

    let _ = append_consolidated_line_profiles(ir, annotations, data, records);

    for guide in crate::families::a5a8::records::a5_guide_curves_from_records(data, records) {
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

    for jet in crate::families::a5a8::records::a5_freeform_curves_from_records(data, records) {
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
        let Ok(multiplicities) = alloc_filled(
            jet.knots.len(),
            jet.degree + 1,
            "catia rolling-ball multiplicities",
        ) else {
            continue;
        };
        let surface_index = ir.model.surfaces.len();
        let surface_id = SurfaceId(format!("catia:rolling-ball:surf#{surface_index}"));
        let procedural_id = ProceduralSurfaceId(format!(
            "catia:rolling-ball:construction#{}",
            ir.model.procedural_surfaces.len()
        ));
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
            geometry: SurfaceGeometry::Procedural {
                construction: procedural_id.clone(),
            },
            source_object: None,
        });

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
                multiplicities,
                knots: jet.knots,
                sites,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

    append_a8_rolling_ball_pools(ir, annotations, data);
    append_resolved_consolidated_surface_curves(
        ir,
        annotations,
        data,
        records,
        &surfaces,
        &carrier_ids,
        surface_alias_tags,
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ConsolidatedCarrierKey {
    Cylinder(usize),
    EmbeddedCylinder(usize),
    Cone(usize),
    Sphere(usize),
    Torus(usize),
    Plane(usize),
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
    /// Plane isometry carrying a stored chart onto a target plane's chart.
    Rigid {
        /// Row-major linear part.
        linear: [[f64; 2]; 2],
        /// Translation applied to positions only.
        offset: [f64; 2],
    },
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ConsolidatedCurveBindingCounts {
    pub(crate) standard_edges: usize,
    pub(crate) partner_supports: usize,
    pub(crate) partner_face_pcurve_pairs: usize,
    pub(crate) standard_face_surfaces: usize,
    /// Coedge pcurves bound after the endpoint-lift witness.
    pub(crate) standard_face_pcurves: usize,
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
            Self::Rigid { linear, offset } => [
                linear[0][0] * u + linear[0][1] * v + offset[0],
                linear[1][0] * u + linear[1][1] * v + offset[1],
            ],
        }
    }

    fn derivative(&self, [u, v]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Identity => [u, v],
            Self::Cylinder { radius } => [u / radius, v],
            Self::Cone { cone } => [u / cone.angular_scale, v * cone.half_angle.cos()],
            Self::Torus { torus } => [u / torus.major_scale, v / torus.minor_scale],
            Self::Rigid { linear, .. } => [
                linear[0][0] * u + linear[0][1] * v,
                linear[1][0] * u + linear[1][1] * v,
            ],
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
    records: &[crate::wire::records::ConsolidatedRecord],
    freeform_surfaces: &[crate::families::a5a8::records::FreeformSurface],
    freeform_surface_ids: &[SurfaceId],
    surface_alias_tags: &HashMap<u32, Option<u32>>,
) -> ConsolidatedCurveBindingCounts {
    let standalone = crate::families::b2::records::b2_cylinders_from_records(data, records)
        .into_iter()
        .map(|cylinder| (cylinder.pos, cylinder))
        .collect::<HashMap<_, _>>();
    let embedded = crate::families::b2::records::b2_embedded_cylinders_from_records(data, records)
        .into_iter()
        .map(|value| (value.pos, value))
        .collect::<HashMap<_, _>>();
    let cones = crate::families::b2::records::b2_cones_from_records(data, records)
        .into_iter()
        .map(|cone| (cone.pos, cone))
        .collect::<HashMap<_, _>>();
    let spheres = crate::families::b2::records::b2_spheres_from_records(data, records)
        .into_iter()
        .map(|sphere| (sphere.pos, sphere))
        .collect::<HashMap<_, _>>();
    let tori = crate::families::b2::records::b2_tori_from_records(data, records)
        .into_iter()
        .map(|torus| (torus.pos, torus))
        .collect::<HashMap<_, _>>();
    let planes = crate::families::b2::records::b2_plane_carriers_from_records(data, records)
        .into_iter()
        .map(|plane| (plane.pos, plane))
        .collect::<HashMap<_, _>>();
    let complete_runs =
        crate::families::consolidated::records::consolidated_topology_edge_runs_from_records(
            data, records,
        )
        .into_iter()
        .filter(|run| run.edge.co_parametric && run.identity_chain_consistent)
        .map(|run| (run.edge.pcurves[0].pos, run))
        .collect::<HashMap<_, _>>();

    let mut surface_ids = HashMap::<ConsolidatedCarrierKey, SurfaceId>::new();
    let standard_carrier_surfaces = standard_carrier_surface_ids(ir);
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
    let vertex_tolerances = ir
        .model
        .vertices
        .iter()
        .map(|vertex| (vertex.id.clone(), vertex.tolerance))
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
    let face_tolerances = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.tolerance))
        .collect::<HashMap<_, _>>();
    let coedge_face_tolerances = ir
        .model
        .coedges
        .iter()
        .map(|coedge| {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|value| value.id == coedge.owner_loop)?;
            face_tolerances.get(&loop_.face).copied().flatten()
        })
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
    let mut partner_support_blocks = HashSet::new();

    let mut pending = VecDeque::from(
        crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
            data, records,
        ),
    );
    while let Some(mut resolved) = pending.pop_front() {
        let Some(run) = complete_runs.get(&resolved.block.pcurves[0].pos) else {
            continue;
        };
        let mut sides: [IntcurveSupportSide; 2] = std::array::from_fn(|_| IntcurveSupportSide {
            surface: None,
            pcurve: None,
            pcurve_parameter_range: None,
        });
        let mut standard_endpoint_loci = None;
        for (side, binding) in resolved.supports.iter().enumerate() {
            let pcurve = &resolved.block.pcurves[side];
            let support_tag = match surface_alias_tags.get(&pcurve.support_id) {
                Some(canonical) => canonical.as_ref().copied(),
                None => Some(pcurve.support_id),
            };
            if let Some(surface_id) =
                support_tag.and_then(|tag| standard_carrier_surfaces.get(&tag))
            {
                let Some(surface_id) = surface_id else {
                    continue;
                };
                let Some(surface_geometry) = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
                    .map(|surface| &surface.geometry)
                else {
                    continue;
                };
                let Some(geometry) =
                    consolidated_jet_pcurve(pcurve, &ConsolidatedCarrierChart::Identity)
                else {
                    continue;
                };
                if standard_endpoint_loci.is_none() {
                    standard_endpoint_loci = standard_carrier_endpoint_loci(
                        &geometry,
                        surface_geometry,
                        resolved.block.parameters.range,
                    );
                }
                sides[side] = IntcurveSupportSide {
                    surface: Some(surface_id.clone()),
                    pcurve: Some(geometry),
                    pcurve_parameter_range: None,
                };
                continue;
            }
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
                                u_sense: None,
                                v_sense: None,
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
                Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Sphere { pos }) => {
                    let Some(sphere) = spheres.get(pos) else {
                        continue;
                    };
                    (
                        ConsolidatedCarrierKey::Sphere(*pos),
                        crate::families::b2::records::b2_sphere_geometry(sphere),
                        None,
                        ConsolidatedCarrierChart::Identity,
                        "consolidated_b2_03_2a_sphere",
                        "sphere",
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
                Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Plane { pos }) => {
                    let Some(plane) = planes.get(pos) else {
                        continue;
                    };
                    let Some(carrier) = crate::families::b2::records::b2_plane_geometry(plane)
                    else {
                        continue;
                    };
                    (
                        ConsolidatedCarrierKey::Plane(*pos),
                        carrier,
                        None,
                        ConsolidatedCarrierChart::Identity,
                        "consolidated_b2_03_27_plane",
                        "plane",
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
                        | ConsolidatedCarrierKey::Sphere(pos)
                        | ConsolidatedCarrierKey::Torus(pos)
                        | ConsolidatedCarrierKey::Plane(pos)
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

            let Some(geometry) = consolidated_jet_pcurve(pcurve, &chart) else {
                continue;
            };
            sides[side] = IntcurveSupportSide {
                surface: Some(surface),
                pcurve: Some(geometry),
                pcurve_parameter_range: None,
            };
        }
        if resolved.endpoint_loci.is_none() {
            resolved.endpoint_loci = standard_endpoint_loci;
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
                partner_support_blocks.insert(resolved.block.pcurves[0].pos);
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
                    let standard_partner_geometry = &ir
                        .model
                        .surfaces
                        .iter()
                        .find(|surface| {
                            surface.id == standard_surfaces[1 - *standard_resolved_side]
                        })?
                        .geometry;
                    let partner_pcurve = match &sides[partner].pcurve {
                        Some(pcurve) => pcurve.clone(),
                        None => {
                            // The free side stores its jet in its own carrier's
                            // chart, which is not the standard partner face's
                            // chart. Recover the isometry between them from the
                            // block's shared 3D loci.
                            let Some(chart) = resolved.shared_loci.as_deref().and_then(|loci| {
                                solve_planar_chart_rechart(
                                    &resolved.block.pcurves[partner].points,
                                    loci,
                                    standard_partner_geometry,
                                )
                            }) else {
                                // The free side has no defined chart relation
                                // to a non-planar or unresolved partner.
                                return Some((identity, None));
                            };
                            let mut pcurve =
                                consolidated_jet_pcurve(&resolved.block.pcurves[partner], &chart)?;
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
                    let edge = &ir.model.edges[identity.0];
                    let edge_id = &edge.id;
                    let edge_endpoints = [
                        *vertex_positions.get(&edge.start)?,
                        *vertex_positions.get(&edge.end)?,
                    ];
                    // The shared coincidence bound, widened by whatever the
                    // topology itself declares. A binding accepted here is one
                    // the endpoint-incidence contract also accepts.
                    let edge_allowance = [
                        edge.tolerance,
                        vertex_tolerances.get(&edge.start).copied().flatten(),
                        vertex_tolerances.get(&edge.end).copied().flatten(),
                    ]
                    .into_iter()
                    .flatten()
                    .fold(cadmpeg_ir::units::COINCIDENCE_TOLERANCE, f64::max);
                    let coedges = standard_surfaces
                        .iter()
                        .enumerate()
                        .filter_map(|(side, surface)| {
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
                            // A pcurve binds to a face only when it lifts onto
                            // the edge through that face's carrier. Without the
                            // witness the side's chart is not this face's
                            // chart, and the pcurve does not describe the edge.
                            let surface_geometry = &ir
                                .model
                                .surfaces
                                .iter()
                                .find(|value| &value.id == surface)?
                                .geometry;
                            let face_allowance = coedge_face_tolerances
                                .get(*coedge)
                                .copied()
                                .flatten()
                                .map_or(edge_allowance, |value| edge_allowance.max(value));
                            pcurve_lift_reaches_endpoints(
                                &geometry,
                                surface_geometry,
                                resolved.block.parameters.range,
                                edge_endpoints,
                                face_allowance,
                            )
                            .then_some((*coedge, geometry))
                        })
                        .collect::<Vec<_>>();
                    (!coedges.is_empty()).then(|| ConsolidatedStandardFaceBinding {
                        coedges,
                        standard_surfaces: standard_surfaces.clone(),
                        edge_pcurves: standard_geometries,
                        inferred_partner: inferred_partner
                            .map(|(_, carrier)| (1 - *standard_resolved_side, carrier)),
                    })
                } else {
                    None
                }
            } else {
                None
            };
            Some((identity, partner_pcurves))
        });
        let mut bound_new_standard_surface = false;
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
                        bound_new_standard_surface = true;
                    }
                    sides = std::array::from_fn(|side| IntcurveSupportSide {
                        surface: Some(binding.standard_surfaces[side].clone()),
                        pcurve: Some(binding.edge_pcurves[side].clone()),
                        pcurve_parameter_range: None,
                    });
                }
            }
        }
        if bound_new_standard_surface {
            // The new carrier can make both coedge pcurves resolvable. Replay this
            // block after mutating the face geometry and emit only on that replay.
            pending.push_back(resolved);
            continue;
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
                if partner_pcurves.coedges.len() == 2 {
                    binding_counts.partner_face_pcurve_pairs += 1;
                }
                binding_counts.standard_face_pcurves += partner_pcurves.coedges.len();
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
    binding_counts.partner_supports = partner_support_blocks.len();
    binding_counts
}

/// Tolerance in millimetres for consolidated definition-site agreement.
const CONSOLIDATED_SITE_TOLERANCE: f64 = 2e-3;

/// Solve the plane isometry that carries a consolidated side's stored chart
/// onto `target`'s chart.
///
/// A consolidated side stores its definition sites in the carrier's own
/// orthonormal chart. When the shared 3D loci of the block lie on `target` and
/// `target` is a plane, both charts are isometric parameterizations of one
/// plane, so a single rigid 2D motion relates them. The motion is recovered
/// from the index-aligned site correspondence and is accepted only when it
/// reproduces every site.
fn solve_planar_chart_rechart(
    sites: &[[f64; 2]],
    loci: &[Point3],
    target: &SurfaceGeometry,
) -> Option<ConsolidatedCarrierChart<'static>> {
    if !matches!(target, SurfaceGeometry::Plane { .. }) || sites.len() != loci.len() {
        return None;
    }
    // Target-chart image of each locus. A locus off the plane has no image,
    // because the plane inverse discards the normal component.
    let images = loci
        .iter()
        .map(|locus| {
            let uv = cadmpeg_ir::eval::analytic_surface_parameters(target, *locus)?;
            let back = cadmpeg_ir::eval::surface_point(target, uv.u, uv.v)?;
            ((back.x - locus.x)
                .hypot(back.y - locus.y)
                .hypot(back.z - locus.z)
                <= CONSOLIDATED_SITE_TOLERANCE)
                .then_some([uv.u, uv.v])
        })
        .collect::<Option<Vec<_>>>()?;
    let count = images.len();
    if count < 2 {
        return None;
    }
    let scale = 1.0 / count as f64;
    let mean = |values: &[[f64; 2]]| {
        values.iter().fold([0.0, 0.0], |acc, value| {
            [acc[0] + value[0] * scale, acc[1] + value[1] * scale]
        })
    };
    let stored_center = mean(sites);
    let image_center = mean(&images);
    // Two-dimensional orthogonal Procrustes. `dot` and `cross` accumulate the
    // rotation's cosine and sine lanes; the reflected solution swaps the sign
    // of the image's second chart coordinate.
    let (mut dot, mut cross) = (0.0, 0.0);
    let (mut reflected_dot, mut reflected_cross) = (0.0, 0.0);
    for (stored, image) in sites.iter().zip(&images) {
        let [su, sv] = [stored[0] - stored_center[0], stored[1] - stored_center[1]];
        let [iu, iv] = [image[0] - image_center[0], image[1] - image_center[1]];
        dot += su * iu + sv * iv;
        cross += su * iv - sv * iu;
        reflected_dot += su * iu - sv * iv;
        reflected_cross += su * iv + sv * iu;
    }
    let candidates = [(dot, cross, 1.0), (reflected_dot, reflected_cross, -1.0)];
    let mut admissible = Vec::new();
    for (dot, cross, determinant) in candidates {
        let norm = dot.hypot(cross);
        if !norm.is_finite() || norm <= f64::EPSILON {
            continue;
        }
        let (cosine, sine) = (dot / norm, cross / norm);
        // A reflection about the target chart's first axis follows the
        // rotation, so its second column changes sign.
        let linear = [[cosine, -sine * determinant], [sine, cosine * determinant]];
        let offset = [
            image_center[0] - (linear[0][0] * stored_center[0] + linear[0][1] * stored_center[1]),
            image_center[1] - (linear[1][0] * stored_center[0] + linear[1][1] * stored_center[1]),
        ];
        let chart = ConsolidatedCarrierChart::Rigid { linear, offset };
        let residual = sites
            .iter()
            .zip(&images)
            .map(|(stored, image)| {
                let mapped = chart.point(*stored);
                (mapped[0] - image[0]).hypot(mapped[1] - image[1])
            })
            .fold(0.0f64, f64::max);
        if !residual.is_finite() {
            continue;
        }
        if residual <= CONSOLIDATED_SITE_TOLERANCE {
            admissible.push(chart);
        }
    }
    if admissible.len() == 1 {
        admissible.pop()
    } else {
        None
    }
}

/// Does `pcurve`, mapped through `surface`, reach `endpoints` over `range`?
///
/// A consolidated side binds to a standard face only when its stored chart is
/// that face carrier's chart. This is the local geometric witness of that: the
/// pcurve's parameter-interval extremes must lift onto the edge's vertex
/// positions within `allowance`. Either endpoint assignment satisfies it,
/// because pcurve parameter direction is independent of edge sense.
///
/// A carrier with no geometry has no chart and therefore admits no witness.
fn pcurve_lift_reaches_endpoints(
    pcurve: &PcurveGeometry,
    surface: &SurfaceGeometry,
    range: [f64; 2],
    endpoints: [Point3; 2],
    allowance: f64,
) -> bool {
    if matches!(surface, SurfaceGeometry::Unknown { .. }) {
        return false;
    }
    let lift = |parameter| {
        let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter)?;
        cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v)
    };
    let (Some(start), Some(end)) = (lift(range[0]), lift(range[1])) else {
        return false;
    };
    let distance = |left: Point3, right: Point3| {
        (left.x - right.x)
            .hypot(left.y - right.y)
            .hypot(left.z - right.z)
    };
    let forward = distance(start, endpoints[0]).max(distance(end, endpoints[1]));
    let reversed = distance(start, endpoints[1]).max(distance(end, endpoints[0]));
    forward.min(reversed) <= allowance
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
        <= EPS_APEX_ALIGNMENT * scale
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
        let procedural_id = ProceduralSurfaceId(format!(
            "catia:a8-rolling-ball:construction#{}",
            ir.model.procedural_surfaces.len()
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
            geometry: SurfaceGeometry::Procedural {
                construction: procedural_id.clone(),
            },
            source_object: Some(cgm_source("surface", jet.object_id)),
        });

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
    use super::*;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve,
        PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, Surface, SurfaceGeometry,
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
    fn object_stream_selection_uses_the_unique_topology_root_run() {
        let topology = crate::test_support::b5_closed_triangle_stream();
        let mut unrelated = vec![0xb5, 0x03, 0x5e, 0x01];
        unrelated.extend_from_slice(&99u32.to_le_bytes());
        unrelated.push(0x00);

        let selection = crate::families::b5::graph::select_object_stream_population(
            &[unrelated, topology.clone()],
            None,
        );
        assert_eq!(selection.run_count, 2);
        assert!(selection.selected);
        assert_eq!(selection.source, topology);
    }

    #[test]
    fn object_stream_selection_refuses_multiple_topology_root_runs() {
        let topology = crate::test_support::b5_closed_triangle_stream();
        let selection = crate::families::b5::graph::select_object_stream_population(
            &[topology.clone(), topology],
            None,
        );

        assert_eq!(selection.run_count, 2);
        assert!(!selection.selected);
        assert!(selection.source.is_empty());
    }

    #[test]
    fn object_stream_selection_stops_before_materializing_over_budget_records() {
        let topology = crate::test_support::b5_closed_triangle_stream();
        let budget = cadmpeg_core::decode::WorkBudget::new(1);

        let selection =
            crate::families::b5::graph::select_object_stream_population(&[topology], Some(&budget));

        assert_eq!(selection.run_count, 1);
        assert!(!selection.selected);
        assert!(selection.source.is_empty());
        assert!(selection.records.is_empty());
        assert!(selection.exhausted);
        assert!(budget.exhausted());
    }

    #[test]
    fn standalone_clamped_curve_becomes_a_valid_wire_edge() {
        let mut ir = CadIr::empty(Units::default());
        let curve_id = CurveId("catia:test:curve#0".to_string());
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point3::new(2.0, 3.0, 5.0), Point3::new(7.0, 11.0, 13.0)],
                weights: None,
                periodic: false,
            }),
            source_object: None,
        });
        assert!(attach_standalone_wires(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[(curve_id, [0.0, 1.0], 17)],
        ));
        assert_eq!(
            ir.model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Wire
        );
        assert_eq!(
            ir.model.shells[0].wire_edges,
            [ir.model.edges[0].id.clone()]
        );
        assert_eq!(ir.model.points[1].position, Point3::new(2.0, 3.0, 5.0));
        assert_eq!(ir.model.points[0].position, Point3::new(7.0, 11.0, 13.0));
        ir.finalize();
        let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new());
        assert!(validation.is_ok(), "{:?}", validation.findings);
    }

    #[test]
    fn consolidated_line_profile_retains_its_stored_wire_interval() {
        let mut ir = CadIr::empty(Units::default());
        let bytes = crate::test_support::b2_line_profile_stream();
        let wires = append_consolidated_line_profiles(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &bytes,
            &crate::wire::records::consolidated_records(&bytes),
        );
        assert_eq!(wires.len(), 1);
        assert!(attach_standalone_wires(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &wires,
        ));
        assert_eq!(ir.model.edges[0].param_range, Some([-4.0, 9.0]));
        let expected_start = Point3::new(1.0, -0.4, -0.2);
        let expected_end = Point3::new(1.0, 7.4, 10.2);
        for (actual, expected) in [
            (ir.model.points[1].position, expected_start),
            (ir.model.points[0].position, expected_end),
        ] {
            assert!((actual.x - expected.x).abs() < 1e-12);
            assert!((actual.y - expected.y).abs() < 1e-12);
            assert!((actual.z - expected.z).abs() < 1e-12);
        }
        ir.finalize();
        let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new());
        assert!(validation.is_ok(), "{:?}", validation.findings);
    }

    #[test]
    fn rolling_ball_pool_retains_both_exact_limiting_curves() {
        let mut ir = CadIr::empty(Units::default());
        let bytes = crate::test_support::a5_freeform_curve_stream();
        append_freeform_surface_pools(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &bytes,
            &crate::wire::records::consolidated_records(&bytes),
            &HashMap::new(),
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
        let mut bytes = crate::test_support::b2_cylinder_stream();
        bytes.extend_from_slice(&crate::test_support::b2_embedded_cylinder_stream());

        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = freeform_surface_carriers(&bytes, &records);
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
        let mut bytes = crate::test_support::b2_cylinder_stream();
        for point in points {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [point.x, point.y, point.z] {
                bytes.extend_from_slice(&(value as f32).to_le_bytes());
            }
        }
        let mut edge_run = crate::test_support::a5_native_edge_run_stream(6, 139, 142);
        let second_pcurve = crate::test_support::a5_pcurve_stream().len();
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
            &crate::wire::records::consolidated_records(&bytes),
            &[],
            &[],
            &HashMap::new(),
        );
        assert_eq!(attached.standard_edges, 1);
        assert_eq!(attached.partner_face_pcurve_pairs, 0);
        assert_eq!(ir.model.pcurves.len(), 0);
        assert_eq!(ir.model.coedges[0].pcurves.len(), 0);
        assert_eq!(ir.model.coedges[1].pcurves.len(), 0);
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
    fn consolidated_pcurve_uses_unique_standard_carrier_tag() {
        let bytes =
            crate::test_support::a5_native_edge_run_stream_with_support(6, 139, 142, 0x1234);
        let mut ir = CadIr::empty(Units::default());
        let surface_id = SurfaceId("standard-carrier".to_string());
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: Some(crate::assemble::cgm_source("carrier", 0x1234)),
        });

        let counts = append_resolved_consolidated_surface_curves(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &bytes,
            &crate::wire::records::consolidated_records(&bytes),
            &[],
            &[],
            &HashMap::new(),
        );

        assert_eq!(counts.standard_edges, 0);
        let [ProceduralCurve {
            definition:
                ProceduralCurveDefinition::Intersection { context, .. }
                | ProceduralCurveDefinition::SurfaceCurve { context, .. },
            ..
        }] = ir.model.procedural_curves.as_slice()
        else {
            panic!("one consolidated surface curve");
        };
        assert!(context
            .sides
            .iter()
            .all(|side| { side.surface.as_ref() == Some(&surface_id) && side.pcurve.is_some() }));
        let start = cadmpeg_ir::eval::pcurve_uv(
            context.sides[0].pcurve.as_ref().expect("standard pcurve"),
            0.0,
        )
        .expect("standard pcurve start");
        assert_eq!(start, Point2::new(0.0, 0.0));
    }

    #[test]
    fn consolidated_pcurve_uses_unique_canonical_surface_alias_tag() {
        let bytes =
            crate::test_support::a5_native_edge_run_stream_with_support(6, 139, 142, 0x5678);
        let mut ir = CadIr::empty(Units::default());
        let surface_id = SurfaceId("standard-carrier".to_string());
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: Some(crate::assemble::cgm_source("carrier", 0x1234)),
        });

        let counts = append_resolved_consolidated_surface_curves(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &bytes,
            &crate::wire::records::consolidated_records(&bytes),
            &[],
            &[],
            &HashMap::from([(0x5678, Some(0x1234))]),
        );

        assert_eq!(counts.standard_edges, 0);
        let [ProceduralCurve {
            definition:
                ProceduralCurveDefinition::Intersection { context, .. }
                | ProceduralCurveDefinition::SurfaceCurve { context, .. },
            ..
        }] = ir.model.procedural_curves.as_slice()
        else {
            panic!("one consolidated surface curve");
        };
        assert!(context
            .sides
            .iter()
            .all(|side| { side.surface.as_ref() == Some(&surface_id) && side.pcurve.is_some() }));
    }

    #[test]
    fn standard_carrier_index_rejects_duplicate_or_unknown_geometry() {
        let mut ir = CadIr::empty(Units::default());
        for (id, geometry) in [
            (
                "known-0",
                SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
            ),
            (
                "known-1",
                SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
            ),
            ("unknown", SurfaceGeometry::Unknown { record: None }),
        ] {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(id.to_string()),
                geometry,
                source_object: Some(crate::assemble::cgm_source("carrier", 0x1234)),
            });
        }

        assert_eq!(standard_carrier_surface_ids(&ir).get(&0x1234), Some(&None));
    }

    #[test]
    fn consolidated_plane_support_transfers_both_surface_curve_sides() {
        let plane_stream = crate::test_support::b2_plane_carrier_stream();
        let plane_end = crate::families::b2::records::b2_plane_carriers(&plane_stream)[0].end;
        let mut bytes = plane_stream[..plane_end].to_vec();
        let points = [Point3::new(10.0, 20.0, 0.0), Point3::new(11.0, 20.0, 1.0)];
        for point in points {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [point.x, point.y, point.z] {
                bytes.extend_from_slice(&(value as f32).to_le_bytes());
            }
        }
        bytes.extend_from_slice(&crate::test_support::a5_native_edge_run_stream(6, 139, 142));

        let mut ir = CadIr::empty(Units::default());
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
        let curve_id = CurveId("standard-plane-curve".to_string());
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId("standard-plane-edge".to_string()),
            curve: Some(curve_id.clone()),
            start: VertexId("vertex#0".to_string()),
            end: VertexId("vertex#1".to_string()),
            param_range: None,
            tolerance: None,
        });
        let plane = SurfaceGeometry::Plane {
            origin: Point3::new(10.0, 20.0, 0.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let support_ids = [
            SurfaceId("standard-plane#0".to_string()),
            SurfaceId("standard-plane#1".to_string()),
        ];
        for support_id in &support_ids {
            ir.model.surfaces.push(Surface {
                id: support_id.clone(),
                geometry: plane.clone(),
                source_object: None,
            });
        }
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId("standard-plane-intersection".to_string()),
            curve: curve_id,
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
            &crate::wire::records::consolidated_records(&bytes),
            &[],
            &[],
            &HashMap::new(),
        );
        assert_eq!(attached.standard_edges, 1);
        assert_eq!(ir.model.edges[0].param_range, Some([0.0, 1.0]));
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &ir.model.procedural_curves[0].definition
        else {
            panic!("plane support keeps an intersection construction");
        };
        assert!(context.sides.iter().all(|side| {
            side.surface
                .as_ref()
                .is_some_and(|id| id.0.starts_with("catia:consolidated:plane#"))
                && side.pcurve.is_some()
        }));
    }

    /// A plane whose chart origin and axes differ from the stored chart used by
    /// a consolidated side, together with definition sites on it. The stored
    /// chart is the target chart turned by `angle` about the site origin and
    /// shifted by `shift`, which is what a foreign carrier's chart looks like.
    fn foreign_plane_chart_sites(
        angle: f64,
        shift: [f64; 2],
    ) -> (SurfaceGeometry, Vec<[f64; 2]>, Vec<Point3>) {
        let origin = Point3::new(7.0, -2.0, 11.0);
        let u_axis = Vector3::new(0.0, 1.0, 0.0);
        let normal = Vector3::new(1.0, 0.0, 0.0);
        let target = SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        };
        // Sites in the target chart, deliberately not collinear so the
        // isometry between the charts is uniquely determined.
        let target_sites = [[0.0, 0.0], [3.0, 1.0], [5.0, -2.0], [8.0, 4.0]];
        let loci = target_sites
            .iter()
            .map(|[u, v]| {
                cadmpeg_ir::eval::surface_point(&target, *u, *v).expect("plane evaluates")
            })
            .collect::<Vec<_>>();
        let (cosine, sine) = (angle.cos(), angle.sin());
        let stored = target_sites
            .iter()
            .map(|[u, v]| {
                [
                    cosine * u - sine * v + shift[0],
                    sine * u + cosine * v + shift[1],
                ]
            })
            .collect::<Vec<_>>();
        (target, stored, loci)
    }

    #[test]
    fn planar_rechart_recovers_a_foreign_consolidated_chart() {
        let angle = 0.7;
        let shift = [12.5, -4.25];
        let (target, stored, loci) = foreign_plane_chart_sites(angle, shift);
        let chart = solve_planar_chart_rechart(&stored, &loci, &target)
            .expect("an isometric stored chart recharts onto the target plane");
        for (site, locus) in stored.iter().zip(&loci) {
            let [u, v] = chart.point(*site);
            let lifted = cadmpeg_ir::eval::surface_point(&target, u, v).expect("plane evaluates");
            assert!(
                (lifted.x - locus.x)
                    .hypot(lifted.y - locus.y)
                    .hypot(lifted.z - locus.z)
                    < 1e-9,
                "recharted site must lift onto its definition locus"
            );
        }
        // The naive binding this replaces reads the stored chart as the
        // target's own, which lands far from the definition loci.
        let naive = ConsolidatedCarrierChart::Identity;
        let [u, v] = naive.point(stored[0]);
        let lifted = cadmpeg_ir::eval::surface_point(&target, u, v).expect("plane evaluates");
        assert!(
            (lifted.x - loci[0].x)
                .hypot(lifted.y - loci[0].y)
                .hypot(lifted.z - loci[0].z)
                > 1.0,
            "the unrecharted stored chart must not be mistaken for the target chart"
        );
        // The linear part carries derivatives without the translation.
        let derivative = chart.derivative([1.0, 0.0]);
        assert!(
            (derivative[0].hypot(derivative[1]) - 1.0).abs() < 1e-12,
            "an isometry preserves derivative magnitude"
        );
    }

    #[test]
    fn planar_rechart_declines_a_chart_that_is_not_an_isometry() {
        let (target, stored, loci) = foreign_plane_chart_sites(0.4, [1.0, 2.0]);
        // Scale one chart axis. No rigid motion reproduces the sites, so no
        // binding may be claimed.
        let scaled = stored
            .iter()
            .map(|[u, v]| [*u * 1.5, *v])
            .collect::<Vec<_>>();
        assert!(solve_planar_chart_rechart(&scaled, &loci, &target).is_none());
        // Loci off the plane have no image in its chart.
        let lifted_loci = loci
            .iter()
            .map(|locus| Point3::new(locus.x + 4.0, locus.y, locus.z))
            .collect::<Vec<_>>();
        assert!(solve_planar_chart_rechart(&stored, &lifted_loci, &target).is_none());
        // A non-plane target has no affine chart to solve against.
        assert!(solve_planar_chart_rechart(
            &stored,
            &loci,
            &SurfaceGeometry::Unknown { record: None }
        )
        .is_none());
        // Two sites leave both orientation choices valid. Three collinear
        // sites have the same ambiguity, so neither admits a unique chart.
        assert!(solve_planar_chart_rechart(&stored[..2], &loci[..2], &target).is_none());
        let collinear_sites = [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]];
        let collinear_loci = collinear_sites
            .iter()
            .map(|[u, v]| cadmpeg_ir::eval::surface_point(&target, *u, *v).expect("plane"))
            .collect::<Vec<_>>();
        assert!(solve_planar_chart_rechart(&collinear_sites, &collinear_loci, &target).is_none());
    }

    #[test]
    fn endpoint_lift_witness_refuses_a_pcurve_from_a_foreign_chart() {
        let (target, stored, loci) = foreign_plane_chart_sites(0.9, [-6.0, 3.5]);
        let endpoints = [*loci.first().expect("sites"), *loci.last().expect("sites")];
        let range = [0.0, 1.0];
        let line_through = |first: [f64; 2], last: [f64; 2]| PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![range[0], range[0], range[1], range[1]],
            control_points: vec![
                Point2::new(first[0], first[1]),
                Point2::new(last[0], last[1]),
            ],
            weights: None,
            periodic: false,
        };
        let chart = solve_planar_chart_rechart(&stored, &loci, &target).expect("isometry");
        let first = *stored.first().expect("sites");
        let last = *stored.last().expect("sites");
        let recharted = line_through(chart.point(first), chart.point(last));
        assert!(
            pcurve_lift_reaches_endpoints(
                &recharted,
                &target,
                range,
                endpoints,
                cadmpeg_ir::units::COINCIDENCE_TOLERANCE
            ),
            "the recharted pcurve lifts onto the edge's vertex positions"
        );
        let naive = line_through(first, last);
        assert!(
            !pcurve_lift_reaches_endpoints(
                &naive,
                &target,
                range,
                endpoints,
                cadmpeg_ir::units::COINCIDENCE_TOLERANCE
            ),
            "a pcurve stored in a foreign chart has no witness on this carrier"
        );
        // The witness is independent of endpoint order.
        assert!(pcurve_lift_reaches_endpoints(
            &recharted,
            &target,
            range,
            [endpoints[1], endpoints[0]],
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        ));
        // A carrier with no geometry has no chart and admits no witness.
        assert!(!pcurve_lift_reaches_endpoints(
            &naive,
            &SurfaceGeometry::Unknown { record: None },
            range,
            endpoints,
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        ));
    }

    #[test]
    fn freeform_fallback_retains_exact_consolidated_spheres() {
        let bytes = crate::test_support::b2_sphere_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = freeform_surface_carriers(&bytes, &records);
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
        let bytes = crate::test_support::b2_torus_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = freeform_surface_carriers(&bytes, &records);
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
        let bytes = crate::test_support::b2_range_origin_cylinder_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = freeform_surface_carriers(&bytes, &records);
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
