// SPDX-License-Identifier: Apache-2.0
//! Freeform decode route composing a5a8 and consolidated NURBS record carriers.

use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve,
    pcurve::{Pcurve, PcurveGeometry},
    surface_payloads::RevolutionSurfaceConstruction,
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, RecordBounds,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{NonZeroLength, PositiveLength};
use cadmpeg_ir::topology::{Body, BodyKind, Edge, Point, Region, Shell, Vertex};
use cadmpeg_ir::units::OrthonormalFrame3;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::assemble::{
    annotate, insert_unresolved_carrier_loss, link_payload_carriers, neutral_model_is_admissible,
    preserve_raw_payload, quintic_jet_pcurve,
};
use crate::assemble::{cgm_source, cgm_source_key};
use crate::container::{self, ContainerScan};
use crate::families::b5::graph::controls::{
    B5EdgeTerminalControl, B5FramingControl, B5VertexIncidenceControl,
};
use crate::families::{FamilyEntityAdmission, FamilyOutput};
use crate::loss::{identity_statement, CatiaLossCode};
use crate::math::distance;

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
pub(super) struct ConsolidatedRevolutionBinding {
    pub(super) geometry: SurfaceGeometry,
    pub(super) profile_sweep: f64,
}

/// Transfer resolved consolidated axis-and-profile revolution carriers.
pub(super) fn append_consolidated_revolutions(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    resolved: &[crate::families::b2::records::B2ResolvedRevolution],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<Vec<ConsolidatedRevolutionBinding>, cadmpeg_core::CodecError> {
    let mut bindings = Vec::new();
    for carrier in resolved {
        let index = carrier.revolution_index;
        let revolution = &carrier.revolution;
        let profile = &carrier.profile;
        let direction_x = Vector3::from(revolution.profile_frame.axis().get());
        let direction_y = Vector3::from(revolution.profile_frame.reference().get());
        let axis = Vector3::from(revolution.axis.get());
        let origin = revolution.origin.get();
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
        let directrix = CurveId::compose(
            &cadmpeg_ir::identity_namespace!(
                "catia",
                "consolidated",
                "surface-revolution-directrix"
            ),
            index,
        );
        let Some(admitted_center) = FinitePoint3::new(center) else {
            continue;
        };
        let payload = cadmpeg_ir::geometry::analytic::CircleCurve::new(
            admitted_center,
            revolution.profile_frame.into(),
            profile.radius,
        );
        annotate(
            admission.context(),
            annotations,
            &directrix,
            "consolidated_b2_03_19",
            profile.pos as u64,
            format_args!("circle:{}", profile.record_id),
            Exactness::ByteExact)?;
        admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves")?;
        ir.model.curves.push(Curve {
            id: directrix.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(payload)),
            source_object: Some(cgm_source(admission.context(), "profile-circle", profile.record_id)?),
        });
        let surface = SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "consolidated", "surface-revolution-surface"),
            index,
        );
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
        // The profile plane contains the axis: the record's right-handed
        // frame admission holds the profile-circle normal perpendicular to
        // the axis within 2e-12.
        let radial_follows_profile_reference = major_radius > 0.0
            && ((radial.x * direction_y.x + radial.y * direction_y.y + radial.z * direction_y.z)
                .abs()
                / major_radius
                - 1.0)
                .abs()
                <= EPS_TORUS_FRAME;
        let torus_geometry = PositiveLength::new(major_radius)
            .filter(|_| radial_follows_profile_reference)
            .and_then(|major_radius| {
                let ref_direction = Vector3::new(
                    radial.x / major_radius.get(),
                    radial.y / major_radius.get(),
                    radial.z / major_radius.get(),
                );
                let torus_center = Point3::new(
                    center.x - radial.x,
                    center.y - radial.y,
                    center.z - radial.z,
                );
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                    cadmpeg_ir::geometry::analytic::TorusSurface::new(
                        FinitePoint3::new(torus_center)?,
                        OrthonormalFrame3::from_units(
                            revolution.axis.into(),
                            cadmpeg_ir::units::UnitVector3::new(ref_direction)?,
                        )?,
                        major_radius,
                        NonZeroLength::from(profile.radius),
                    ),
                )))
            });
        annotate(
            admission.context(),
            annotations,
            &surface,
            "consolidated_b2_03_2d",
            revolution.pos as u64,
            format_args!("profile-allocation:{}", revolution.profile_allocation_id),
            Exactness::ByteExact)?;
        admission.reserve_entity(&mut ir.model.surfaces, "catia_family_emit_surfaces")?;
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: torus_geometry.clone().unwrap_or(SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Unknown { record: None },
            )),
            source_object: Some(cgm_source(admission.context(),
                "revolution",
                u32::from(revolution.profile_allocation_id),
            )?),
        });
        admission.reserve_entity(&mut ir.model.procedural_surfaces, "catia_family_emit_procedural_surfaces")?;
        let _attached = ir.model.add_procedural_surface(
            surface,
            ProceduralSurface::new(
                ProceduralSurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "consolidated", "surface-revolution"),
                    index,
                ),
                ProceduralSurfaceDefinition::Revolution(RevolutionSurfaceConstruction::legacy(
                    directrix,
                    (revolution.origin, revolution.axis.into()),
                    revolution.angular_interval,
                    Some(revolution.angular_range),
                    Some(revolution.profile_range),
                    false,
                    None,
                )),
                None,
            ),
        );
        if let Some(geometry) = torus_geometry {
            bindings.push(ConsolidatedRevolutionBinding {
                geometry,
                profile_sweep: (revolution.profile_range.upper()
                    - revolution.profile_range.lower())
                .abs()
                    / profile.radius.get(),
            });
        }
    }
    Ok(bindings)
}

fn typed_face_counts(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &std::collections::BTreeMap<u32, crate::families::b5::graph::B5FaceRecord>,
    resolved_faces: &[crate::families::b5::graph::B5Face],
) -> Result<[usize; 4], cadmpeg_core::CodecError> {
    let resolved_ids = crate::resource::collect_set(
        ctx,
        resolved_faces.iter().map(|face| face.object_id),
        "catia_freeform_resolved_face_ids",
    )?;
    Ok(records.values().fold([0usize; 4], |mut counts, face| {
        match face.terminal_control {
            Some(B5FramingControl::Control03) => counts[0] += 1,
            Some(B5FramingControl::Control05) => counts[1] += 1,
            None => counts[2] += 1,
        }
        counts[3] += usize::from(!resolved_ids.contains(&face.object_id));
        counts
    }))
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
            [B5FramingControl::Control03, B5FramingControl::Control03] => 0,
            [B5FramingControl::Control03, B5FramingControl::Control05] => 1,
            [B5FramingControl::Control05, B5FramingControl::Control03] => 2,
            [B5FramingControl::Control05, B5FramingControl::Control05] => 3,
        };
        counts[index] += 1;
        counts[4] += usize::from(loop_.metadata.extension.is_some());
        counts
    })
}

/// Object-stream class code of a `b5 03 5f` face record.
const B5_FACE_CLASS: u8 = 0x5f;

pub(super) fn try_decode_freeform_surfaces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FamilyOutput>, cadmpeg_core::CodecError> {
    (|| -> Option<Result<FamilyOutput, cadmpeg_core::CodecError>> {
        macro_rules! admitted {
            ($value:expr) => {
                match $value {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                }
            };
        }
        let logical_streams = match container::logical_record_streams(ctx, scan) {
            Ok(streams) => streams,
            Err(error) => return Some(Err(error)),
        };
        let selection_budget =
            ctx.work_budget(crate::families::b5::graph::MAX_OBJECT_STREAM_SELECTION_WORK as u64);
        let object_selection = crate::families::b5::graph::select_object_stream_population(
            ctx,
            &logical_streams,
            Some(&selection_budget),
        );
        let object_selection = match object_selection {
            Ok(selection) => selection,
            Err(error) => return Some(Err(error)),
        };
        let (
            object_stream_run_count,
            selected_object_stream_run_count,
            object_stream_selection_exhausted,
            object_source,
            object_frames,
            selected_object_records,
            census_object_records,
        ) = match object_selection {
            crate::families::b5::graph::ObjectStreamSelection::Exhausted { run_count } => (
                run_count,
                0,
                true,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            crate::families::b5::graph::ObjectStreamSelection::Unselected {
                run_count,
                census_records,
            } => (
                run_count,
                0,
                false,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                census_records,
            ),
            crate::families::b5::graph::ObjectStreamSelection::Selected {
                source,
                frames,
                records,
                census_records,
                run_count,
            } => (run_count, 1, false, source, frames, records, census_records),
        };
        let sources = match container::consolidated_record_sources(ctx, scan) {
            Ok(sources) => sources,
            Err(error) => return Some(Err(error)),
        };
        let consolidated_records = match crate::wire::records::consolidated_records_in_sources(
            ctx,
            &scan.data,
            sources,
        ) {
            Ok(records) => records,
            Err(error) => return Some(Err(error)),
        };
        let mut b5_graph = match crate::families::b5::graph::parse_from_records_budgeted(
            ctx,
            &object_source,
            &selected_object_records,
            &object_frames,
            true,
            Some(&selection_budget),
            refusal,
        ) {
            Ok(graph) => graph,
            Err(error) => return Some(Err(error)),
        };
        let face_terminal_controls = b5_graph.as_ref().map(|graph| {
            graph.faces.iter().fold([0usize; 3], |mut counts, face| {
                match face.terminal_control {
                    Some(B5FramingControl::Control03) => counts[0] += 1,
                    Some(B5FramingControl::Control05) => counts[1] += 1,
                    None => counts[2] += 1,
                }
                counts
            })
        });
        let typed_face_counts = if let Some(graph) = &b5_graph {
            Some(admitted!(typed_face_counts(ctx, &graph.face_records, &graph.faces)))
        } else {
            let records = match crate::families::b5::graph::typed_face_records_from_records(
                ctx, &census_object_records,
            ) {
                Ok(records) => records,
                Err(error) => return Some(Err(error)),
            };
            if records.is_empty() { None } else { Some(admitted!(typed_face_counts(ctx, &records, &[]))) }
        };
        let typed_multi_surface_face_count = b5_graph
            .as_ref()
            .map(typed_multi_surface_face_count)
            .unwrap_or_default();
        let typed_edge_records = match crate::families::b5::graph::typed_edge_records_from_records(
            ctx, &census_object_records,
        ) {
            Ok(records) => records,
            Err(error) => return Some(Err(error)),
        };
        let edge_terminal_controls = (!typed_edge_records.is_empty()).then(|| {
            typed_edge_records
                .values()
                .fold([0usize; 8], |mut counts, edge| {
                    let index = match edge.terminal_control {
                        B5EdgeTerminalControl::Control01 => 0,
                        B5EdgeTerminalControl::Control02 => 1,
                        B5EdgeTerminalControl::Control21 => 2,
                        B5EdgeTerminalControl::Control22 => 3,
                        B5EdgeTerminalControl::Control25 => 4,
                        B5EdgeTerminalControl::Control26 => 5,
                        B5EdgeTerminalControl::Control29 => 6,
                        B5EdgeTerminalControl::Control2A => 7,
                    };
                    counts[index] += 1;
                    counts
                })
        });
        let typed_vertex_incidence_links = match
            crate::families::b5::graph::typed_vertex_incidence_links_from_records(
                ctx, &census_object_records,
            ) {
                Ok(records) => records,
                Err(error) => return Some(Err(error)),
            };
        let vertex_incidence_terminal_controls =
            (!typed_vertex_incidence_links.is_empty()).then(|| {
                typed_vertex_incidence_links
                    .values()
                    .fold([0usize; 2], |mut counts, link| {
                        match link.terminal_control {
                            B5VertexIncidenceControl::Control00 => counts[0] += 1,
                            B5VertexIncidenceControl::Control04 => counts[1] += 1,
                        }
                        counts
                    })
            });
        let resolved_loop_metadata_counts = b5_graph
            .as_ref()
            .map(|graph| loop_metadata_counts(graph.loops.values()));
        let typed_loop_records = match crate::families::b5::graph::typed_loop_records_from_records(
            ctx, &census_object_records,
        ) {
            Ok(records) => records,
            Err(error) => return Some(Err(error)),
        };
        let typed_loop_metadata_counts = (!typed_loop_records.is_empty()).then(|| {
            (
                loop_metadata_counts(typed_loop_records.values()),
                typed_loop_records
                    .keys()
                    .filter(|id| {
                        b5_graph
                            .as_ref()
                            .is_none_or(|graph| !graph.loops.contains_key(id))
                    })
                    .count(),
            )
        });
        let class_21_suffix_scalar_count = b5_graph.as_ref().map(|graph| {
            graph
                .pcurves
                .values()
                .filter(|pcurve| pcurve.class_21_suffix_scalar.is_some())
                .count()
        });
        let typed_class_21_pcurve_count = match
            crate::families::b5::graph::typed_class_21_pcurves_from_records(
                ctx, &census_object_records,
            ) {
                Ok(pcurves) => pcurves.len(),
                Err(error) => return Some(Err(error)),
            };
        let typed_parameter_incidences = match
            crate::families::b5::graph::typed_parameter_incidences_from_records(
                ctx, &census_object_records,
            ) {
                Ok(records) => records,
                Err(error) => return Some(Err(error)),
            };
        let typed_parameter_incidence_member_count = typed_parameter_incidences
            .values()
            .map(|incidence| incidence.lanes.len())
            .sum();
        let typed_vertex_incidence_rosters = match
            crate::families::b5::graph::typed_vertex_incidence_rosters_from_records(
                ctx, &census_object_records,
            ) {
                Ok(records) => records,
                Err(error) => return Some(Err(error)),
            };
        let typed_vertex_incidence_roster_member_count =
            typed_vertex_incidence_rosters.values().map(Vec::len).sum();
        let mut fallback_surfaces = if b5_graph.is_none() {
            match freeform_surface_carriers(ctx, &scan.data, &consolidated_records, refusal) {
                Ok(surfaces) => Some(surfaces),
                Err(error) => return Some(Err(error)),
            }
        } else {
            None
        };
        let b2_nurbs_curves = match crate::families::b2::records::b2_nurbs_curves_from_records(
            ctx,
            &scan.data,
            &consolidated_records,
            refusal,
        ) {
            Ok(curves) => curves,
            Err(error) => return Some(Err(error)),
        };
        let b2_nurbs_curve_count = b2_nurbs_curves.len();
        let a5_nurbs_curves = match crate::families::a5a8::records::a5_nurbs_curves_from_records(
            ctx,
            &scan.data,
            &consolidated_records,
            refusal,
        ) {
            Ok(curves) => curves,
            Err(error) => return Some(Err(error)),
        };
        let a5_nurbs_curve_count = a5_nurbs_curves.len();
        let b2_spatial_circles = match crate::resource::collect_vec(
            ctx,
            crate::families::b2::records::b2_spatial_circles_from_records(
                &scan.data,
                &consolidated_records,
            ),
            "catia_freeform_spatial_circles",
        ) {
            Ok(circles) => circles,
            Err(error) => return Some(Err(error)),
        };
        let b2_line_profile_count = crate::families::b2::records::b2_line_profiles_from_records(
            &scan.data,
            &consolidated_records,
        )
        .count();
        let resolved_consolidated_revolutions =
            match crate::families::b2::records::b2_resolved_revolutions_from_records(
                ctx,
                &scan.data,
                &consolidated_records,
            ) {
                Ok(revolutions) => revolutions,
                Err(error) => return Some(Err(error)),
            };
        let resolved_consolidated_revolution_count = resolved_consolidated_revolutions.len();
        let b2_spatial_circle_count = b2_spatial_circles.len();
        let no_a8_jets = if fallback_surfaces.as_ref().is_some_and(Vec::is_empty) {
            match crate::families::a5a8::records::a8_freeform_curves(ctx, &scan.data) {
                Ok(jets) => jets.is_empty(),
                Err(error) => return Some(Err(error)),
            }
        } else {
            false
        };
        if no_a8_jets
            && b2_nurbs_curves.is_empty()
            && a5_nurbs_curves.is_empty()
            && b2_spatial_circles.is_empty()
            && b2_line_profile_count == 0
            && resolved_consolidated_revolution_count == 0
        {
            return None;
        }
        let mut ir = CadIr::empty();
        let mut admission = FamilyEntityAdmission::new(ctx);
        let mut annotations = AnnotationBuilder::new();
        let mut unknowns = Vec::new();
        let payload_id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "payload", "unknown"),
            cadmpeg_ir::identity_key!("freeform"),
        );
        let payload_index = match preserve_raw_payload(
            ctx,
            &mut unknowns,
            &mut annotations,
            scan,
            payload_id.clone(),
        ) {
            Ok(index) => index,
            Err(error) => return Some(Err(error)),
        };
        let b5_complete = b5_graph.as_ref().is_some_and(|graph| graph.complete);
        // The graph moves into the transfer below. Keep the record identities the
        // topology loss notes must name.
        let b5_face_object_ids = admitted!(crate::resource::collect_vec(
            ctx,
            b5_graph.iter().flat_map(|graph| graph.faces.iter().map(|face| face.object_id)),
            "catia_freeform_b5_face_ids",
        ));
        let b5_loop_object_ids = admitted!(crate::resource::collect_vec(
            ctx,
            b5_graph.iter().flat_map(|graph| graph.loops.keys().copied()),
            "catia_freeform_b5_loop_ids",
        ));
        let census_face_object_ids = admitted!(crate::resource::collect_vec(
            ctx,
            census_object_records.iter()
                .filter(|record| record.class == B5_FACE_CLASS)
                .map(|record| record.object_id),
            "catia_freeform_census_face_ids",
        ));
        let mut topology_ir = ir.clone();
        let mut topology_annotations = annotations.clone();
        let topology_transferred = if let Some(graph) = b5_graph.take() {
            let transferred = match crate::families::b5::transfer::transfer(
                &mut topology_ir,
                &mut topology_annotations,
                graph,
                &payload_id,
                refusal,
                &mut admission,
            ) {
                Ok(transferred) => transferred,
                Err(error) => return Some(Err(error)),
            };
            transferred && neutral_model_is_admissible(&mut topology_ir, &unknowns)
        } else {
            false
        };
        if topology_transferred {
            ir = topology_ir;
            annotations = topology_annotations;
        }
        if !topology_transferred {
            let surfaces = match fallback_surfaces.take() {
                Some(surfaces) => surfaces,
                None => {
                    match freeform_surface_carriers(ctx, &scan.data, &consolidated_records, refusal)
                    {
                        Ok(surfaces) => surfaces,
                        Err(error) => return Some(Err(error)),
                    }
                }
            };
            for (index, surface) in surfaces.iter().enumerate() {
                let id = SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "a8", "surf"),
                    index,
                );
                admitted!(annotate(
                    ctx,
                    &mut annotations,
                    &id,
                    "object_stream_a8_03",
                    surface.pos as u64,
                    &surface.source_tag,
                    Exactness::ByteExact));
                if let Err(error) = admission.reserve_entity(&mut ir.model.surfaces, "catia_family_emit_surfaces") {
                    return Some(Err(error));
                }
                ir.model.surfaces.push(Surface {
                    id,
                    geometry: surface.geometry.clone(),
                    source_object: Some(surface.source_object.clone()),
                });
            }
        }
        // The bindings this call returns are read by the standard-family route
        // alone; here the call is made for the curves and surfaces it appends.
        if let Err(error) = append_consolidated_revolutions(
            &mut ir,
            &mut annotations,
            &resolved_consolidated_revolutions,
            &mut admission,
        ) {
            return Some(Err(error));
        }
        if let Err(error) =
            append_a8_rolling_ball_pools(&mut ir, &mut annotations, &scan.data, &mut admission)
        {
            return Some(Err(error));
        }
        let line_profiles = admitted!(consolidated_line_profiles(ctx, &scan.data, &consolidated_records));
        let mut standalone_wires = Vec::new();
        for profile in &line_profiles {
            let id = admitted!(crate::resource::copy_id(
                ctx,
                profile.curve.id.as_str(),
                CurveId::mint,
                "catia_freeform_standalone_wire_id",
            ));
            admitted!(crate::resource::push(
                ctx,
                &mut standalone_wires,
                (id, profile.range, profile.pos),
                "catia_freeform_standalone_wires",
            ));
        }
        if let Err(error) = append_consolidated_line_profiles(
            &mut ir,
            &mut annotations,
            line_profiles,
            &mut admission,
        ) {
            return Some(Err(error));
        }
        for curve in b2_nurbs_curves {
            let id = CurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "b2", "nurbs-curve"),
                ir.model.curves.len(),
            );
            let parameter_range = curve.geometry.full_knot_endpoints();
            admitted!(annotate(
                ctx,
                &mut annotations,
                &id,
                "consolidated_b2_03_16",
                curve.pos as u64,
                format_args!("header_token:{:08x}", curve.header_token),
                Exactness::ByteExact));
            if let Err(error) = admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves") {
                return Some(Err(error));
            }
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve.geometry)),
                source_object: Some(admitted!(cgm_source_key(ctx,
                    "b2-nurbs-curve-frame",
                    format_args!("{:010}", curve.pos),
                ))),
            });
            standalone_wires.push((id, parameter_range.endpoints(), curve.pos));
        }
        for curve in a5_nurbs_curves {
            let id = CurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "a5", "nurbs-curve"),
                ir.model.curves.len(),
            );
            let parameter_range = curve.geometry.full_knot_endpoints();
            admitted!(annotate(
                ctx,
                &mut annotations,
                &id,
                "consolidated_a5_13_16",
                curve.pos as u64,
                format_args!("header_token:{:08x}", curve.header_token),
                Exactness::ByteExact));
            if let Err(error) = admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves") {
                return Some(Err(error));
            }
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve.geometry)),
                source_object: Some(admitted!(cgm_source_key(ctx,
                    "a5-nurbs-curve-frame",
                    format_args!("{:010}", curve.pos),
                ))),
            });
            standalone_wires.push((id, parameter_range.endpoints(), curve.pos));
        }
        for circle in b2_spatial_circles {
            let id = CurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "b2", "circle"),
                ir.model.curves.len(),
            );
            let parameter_range = [
                circle.range.lower() / circle.radius.get(),
                circle.range.upper() / circle.radius.get(),
            ];
            admitted!(annotate(
                ctx,
                &mut annotations,
                &id,
                "consolidated_b2_03_0f",
                circle.pos as u64,
                format_args!(
                    "header_token:{:08x}:range:{:?}:chart_shift:{}",
                    circle.header_token,
                    circle.range.endpoints(),
                    circle.chart_shift.get()
                ),
                Exactness::ByteExact));
            if let Err(error) = admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves") {
                return Some(Err(error));
            }
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::new(
                        circle.center,
                        circle.frame,
                        circle.radius,
                    ),
                )),
                source_object: Some(admitted!(cgm_source_key(ctx,
                    "b2-spatial-circle-frame",
                    format_args!("{:010}", circle.pos),
                ))),
            });
            standalone_wires.push((id, parameter_range, circle.pos));
        }
        let wire_topology_transferred = if !topology_transferred
            && ir.model.surfaces.is_empty()
            && standalone_wires.len() == ir.model.curves.len()
            && !standalone_wires.is_empty()
        {
            match attach_standalone_wires(
                &mut ir,
                &mut annotations,
                &standalone_wires,
                &mut admission,
            ) {
                Ok(transferred) => transferred,
                Err(error) => return Some(Err(error)),
            }
        } else {
            false
        };
        let mut losses = Vec::new();
        if !wire_topology_transferred {
        if topology_transferred && b5_complete {
            admitted!(crate::resource::push_loss(ctx, &mut losses,
                CatiaLossCode::TopologyB5GaugeSubstituted, format_args!(
                "The B5 reference graph is closed; face sense and body kind use a deterministic \
             topology gauge because their source fields remain unresolved. Gauged b5 03 5f face \
             records, by object id ({}): {}.",
                b5_face_object_ids.len(),
                identity_statement(&b5_face_object_ids)
            ), "catia_freeform_topology_loss"));
        } else if topology_transferred {
            admitted!(crate::resource::push_loss(ctx, &mut losses,
                CatiaLossCode::TopologyB5SubsetIncomplete, format_args!(
            "A maximal reference-closed B5 face/loop/pcurve/edge subset was transferred; variant \
             nodes and unresolved endpoint lifts remain outside the connected graph. Transferred \
             b5 03 5f face records, by object id ({}): {}. Transferred b5 03 62 loop records, by \
             object id ({}): {}.",
            b5_face_object_ids.len(),
            identity_statement(&b5_face_object_ids),
            b5_loop_object_ids.len(),
            identity_statement(&b5_loop_object_ids)
        ), "catia_freeform_topology_loss"));
        } else if object_stream_selection_exhausted {
            admitted!(crate::resource::push_loss(ctx, &mut losses,
                CatiaLossCode::TopologyObjectStreamWorkSliceExhausted, format_args!(
            "The object-stream graph exceeds the bounded frame-index and record-materialization \
             work slice; its topology remains native. The {object_stream_run_count} object runs \
             stay inside retained record {payload_id}."
        ), "catia_freeform_topology_loss"));
        } else {
            admitted!(crate::resource::push_loss(ctx, &mut losses,
                CatiaLossCode::TopologyB5GraphUnclosed, format_args!(
                "Object-stream and consolidated NURBS carriers were decoded, but the \
             face/loop/pcurve/edge graph did not close. Unclosed b5 03 5f face records, by object \
             id ({}): {}. The records stay inside retained record {payload_id}.",
                census_face_object_ids.len(),
                identity_statement(&census_face_object_ids)
            ), "catia_freeform_topology_loss"));
        }
        }
        if let Err(error) = insert_unresolved_carrier_loss(ctx, &ir, &mut losses) {
            return Some(Err(error));
        }
        if let Err(error) = link_payload_carriers(ctx, &ir, &mut unknowns[payload_index], &mut annotations) {
            return Some(Err(error));
        }
        let annotations = annotations.build();
        let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
        coverage.record(
            crate::coverage::DECODED_OBJECT_STREAM_RUN_COUNT,
            object_stream_run_count,
        );
        coverage.record(
            crate::coverage::SELECTED_OBJECT_STREAM_RUN_COUNT,
            selected_object_stream_run_count,
        );
        coverage.record(
            crate::coverage::UNSELECTED_OBJECT_STREAM_RUN_COUNT,
            object_stream_run_count - selected_object_stream_run_count,
        );
        coverage.record(
            crate::coverage::EXHAUSTED_OBJECT_STREAM_SELECTION_COUNT,
            usize::from(object_stream_selection_exhausted),
        );
        coverage.record(
            crate::coverage::DECODED_B2_NURBS_CURVE_COUNT,
            b2_nurbs_curve_count,
        );
        coverage.record(
            crate::coverage::DECODED_A5_NURBS_CURVE_COUNT,
            a5_nurbs_curve_count,
        );
        coverage.record(
            crate::coverage::DECODED_B2_SPATIAL_CIRCLE_COUNT,
            b2_spatial_circle_count,
        );
        coverage.record(
            crate::coverage::ATTACHED_STANDALONE_WIRE_EDGE_COUNT,
            usize::from(wire_topology_transferred) * standalone_wires.len(),
        );
        if let Some([control_03, control_05, uncounted]) = face_terminal_controls {
            coverage.record(
                crate::coverage::RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT,
                control_03,
            );
            coverage.record(
                crate::coverage::RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT,
                control_05,
            );
            coverage.record(
                crate::coverage::RESOLVED_OBJECT_STREAM_UNCOUNTED_FACE_COUNT,
                uncounted,
            );
        }
        if topology_transferred {
            coverage.record(
                crate::coverage::TRANSFERRED_OBJECT_STREAM_FACE_COUNT,
                ir.model.faces.len(),
            );
            coverage.record(
                crate::coverage::TRANSFERRED_OBJECT_STREAM_LOOP_COUNT,
                ir.model.loops.len(),
            );
        }
        if let Some([control_03, control_05, uncounted, unresolved]) = typed_face_counts {
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT,
                control_03,
            );
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT,
                control_05,
            );
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_UNCOUNTED_FACE_COUNT,
                uncounted,
            );
            coverage.record(
                crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_FACE_COUNT,
                unresolved,
            );
        }
        if typed_multi_surface_face_count != 0 {
            coverage.record(
                crate::coverage::TYPED_MULTI_SURFACE_OBJECT_STREAM_FACE_COUNT,
                typed_multi_surface_face_count,
            );
        }
        if let Some(counts) = edge_terminal_controls {
            for (key, count) in [
                crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_01_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_02_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_21_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_22_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_25_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_26_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_29_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_2A_COUNT,
            ]
            .into_iter()
            .zip(counts)
            {
                coverage.record(key, count);
            }
        }
        if let Some([control_00, control_04]) = vertex_incidence_terminal_controls {
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_00_COUNT,
                control_00,
            );
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_04_COUNT,
                control_04,
            );
        }
        if let Some([controls_03_03, controls_03_05, controls_05_03, controls_05_05, extended]) =
            resolved_loop_metadata_counts
        {
            for (key, count) in [
                (
                    crate::coverage::RESOLVED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_03_03_COUNT,
                    controls_03_03,
                ),
                (
                    crate::coverage::RESOLVED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_03_05_COUNT,
                    controls_03_05,
                ),
                (
                    crate::coverage::RESOLVED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_03_COUNT,
                    controls_05_03,
                ),
                (
                    crate::coverage::RESOLVED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT,
                    controls_05_05,
                ),
            ] {
                coverage.record(key, count);
            }
            coverage.record(
                crate::coverage::RESOLVED_OBJECT_STREAM_EXTENDED_LOOP_METADATA_COUNT,
                extended,
            );
        }
        if let Some((counts, unresolved)) = typed_loop_metadata_counts {
            for (key, count) in [
                crate::coverage::TYPED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_03_03_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_03_05_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_03_COUNT,
                crate::coverage::TYPED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT,
            ]
            .into_iter()
            .zip(counts[..4].iter().copied())
            {
                coverage.record(key, count);
            }
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_EXTENDED_LOOP_METADATA_COUNT,
                counts[4],
            );
            coverage.record(
                crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_LOOP_COUNT,
                unresolved,
            );
        }
        if let Some(count) = class_21_suffix_scalar_count {
            coverage.record(
                crate::coverage::RESOLVED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT,
                count,
            );
        }
        if typed_class_21_pcurve_count != 0 {
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT,
                typed_class_21_pcurve_count,
            );
        }
        if !typed_parameter_incidences.is_empty() {
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_COUNT,
                typed_parameter_incidences.len(),
            );
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_MEMBER_COUNT,
                typed_parameter_incidence_member_count,
            );
        }
        if !typed_vertex_incidence_rosters.is_empty() {
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_COUNT,
                typed_vertex_incidence_rosters.len(),
            );
            coverage.record(
                crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_MEMBER_COUNT,
                typed_vertex_incidence_roster_member_count,
            );
        }
        Some(Ok(FamilyOutput {
            ir,
            report: DecodeBody {
                transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
                coverage,
                losses,
                notes: Vec::new(),
                transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
            },
            annotations,
            unknowns,
            admitted_model_entities: admission.admitted(),
        }))
    })()
    .transpose()
}

fn attach_standalone_wires(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    wires: &[(CurveId, [f64; 2], usize)],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut plans = Vec::new();
    for (index, (curve_id, range, pos)) in wires.iter().enumerate() {
            let Some(geometry) = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .map(|curve| &curve.geometry) else { return Ok(false) };
            let Some(start) = cadmpeg_ir::eval::curve_point(geometry, range[0]).ok() else { return Ok(false) };
            let Some(end) = cadmpeg_ir::eval::curve_point(geometry, range[1]).ok() else { return Ok(false) };
            let carrier_id = crate::resource::copy_id(
                admission.context(), curve_id.as_str(), CurveId::mint,
                "catia_freeform_wire_plan_curve_id",
            )?;
            let Some(carrier) = cadmpeg_ir::topology::EdgeCarrier::new(
                Some(carrier_id), Some(*range),
            ).ok() else { return Ok(false) };
            crate::resource::push(
                admission.context(), &mut plans, (index, carrier, *pos, start, end),
                "catia_freeform_wire_plans",
            )?;
    }
    let body_id = crate::resource::copy_id(admission.context(),
        "catia:freeform:wire-body#0", BodyId::mint, "catia_freeform_wire_body_id")?;
    let region_id = crate::resource::copy_id(admission.context(),
        "catia:freeform:wire-region#0", RegionId::mint, "catia_freeform_wire_region_id")?;
    let shell_id = crate::resource::copy_id(admission.context(),
        "catia:freeform:wire-shell#0", ShellId::mint, "catia_freeform_wire_shell_id")?;
    let mut edge_ids = Vec::new();
    for (index, ..) in &plans {
            let id = crate::resource::compose_index_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "freeform", "wire-edge"),
                *index, EdgeId::mint, "catia_freeform_wire_shell_edge_id",
            )?;
            crate::resource::push(
                admission.context(), &mut edge_ids, id, "catia_freeform_wire_shell_edges",
            )?;
    }
    let shell_owner_id = crate::resource::copy_id(admission.context(),
        shell_id.as_str(), ShellId::mint, "catia_freeform_wire_region_shell_id")?;
    let shell_region_id = crate::resource::copy_id(admission.context(),
        region_id.as_str(), RegionId::mint, "catia_freeform_wire_shell_region_id")?;
    let Ok(shell) = Shell::new(
        shell_id,
        shell_region_id,
        Vec::new(),
        edge_ids,
        Vec::new(),
    ) else {
        return Ok(false);
    };
    for id in [body_id.as_str(), region_id.as_str(), shell_owner_id.as_str()] {
        annotate(
            admission.context(),
            annotations,
            id,
            "consolidated_curve_wire",
            0,
            "standalone_wire_owner",
            Exactness::Inferred)?;
    }
    for (index, carrier, pos, start, end) in plans {
        let point_ids = [
            PointId::mint(crate::resource::format_retained(admission.context(),
                format_args!("catia:freeform:wire-point#{index}:start"),
                "catia_freeform_wire_point_id")?).map_err(cadmpeg_core::CodecError::malformed)?,
            PointId::mint(crate::resource::format_retained(admission.context(),
                format_args!("catia:freeform:wire-point#{index}:end"),
                "catia_freeform_wire_point_id")?).map_err(cadmpeg_core::CodecError::malformed)?,
        ];
        let vertex_ids = [
            VertexId::mint(crate::resource::format_retained(admission.context(),
                format_args!("catia:freeform:wire-vertex#{index}:start"),
                "catia_freeform_wire_vertex_id")?).map_err(cadmpeg_core::CodecError::malformed)?,
            VertexId::mint(crate::resource::format_retained(admission.context(),
                format_args!("catia:freeform:wire-vertex#{index}:end"),
                "catia_freeform_wire_vertex_id")?).map_err(cadmpeg_core::CodecError::malformed)?,
        ];
        let edge_id = crate::resource::compose_index_id(admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "freeform", "wire-edge"),
            index, EdgeId::mint, "catia_freeform_wire_edge_id")?;
        for id in [
            point_ids[0].as_str(),
            point_ids[1].as_str(),
            vertex_ids[0].as_str(),
            vertex_ids[1].as_str(),
            edge_id.as_str(),
        ] {
            annotate(
                admission.context(),
                annotations,
                id,
                "consolidated_curve_wire",
                pos as u64,
                "curve_domain_endpoint",
                Exactness::Derived)?;
        }
        admission.reserve_entity(&mut ir.model.points, "catia_family_emit_points")?;
        admission.reserve_entity(&mut ir.model.points, "catia_family_emit_points")?;
        let vertex_point_ids = [
            crate::resource::copy_id(admission.context(), point_ids[0].as_str(),
                PointId::mint, "catia_freeform_wire_vertex_point_id")?,
            crate::resource::copy_id(admission.context(), point_ids[1].as_str(),
                PointId::mint, "catia_freeform_wire_vertex_point_id")?,
        ];
        let [start_point_id, end_point_id] = point_ids;
        ir.model.points.extend([
            Point::new(end_point_id, end, None),
            Point::new(start_point_id, start, None),
        ]);
        let edge_vertex_ids = [
            crate::resource::copy_id(admission.context(), vertex_ids[0].as_str(),
                VertexId::mint, "catia_freeform_wire_edge_vertex_id")?,
            crate::resource::copy_id(admission.context(), vertex_ids[1].as_str(),
                VertexId::mint, "catia_freeform_wire_edge_vertex_id")?,
        ];
        let [start_vertex_id, end_vertex_id] = vertex_ids;
        let [start_vertex_point_id, end_vertex_point_id] = vertex_point_ids;
        admission.reserve_entity(&mut ir.model.vertices, "catia_family_emit_vertices")?;
        admission.reserve_entity(&mut ir.model.vertices, "catia_family_emit_vertices")?;
        ir.model.vertices.extend([
            Vertex {
                id: end_vertex_id,
                point: end_vertex_point_id,
                tolerance: None,
            },
            Vertex {
                id: start_vertex_id,
                point: start_vertex_point_id,
                tolerance: None,
            },
        ]);
        let [start_edge_vertex_id, end_edge_vertex_id] = edge_vertex_ids;
        admission.reserve_entity(&mut ir.model.edges, "catia_family_emit_edges")?;
        ir.model.edges.push(Edge {
            id: edge_id,
            carrier,
            start: start_edge_vertex_id,
            end: end_edge_vertex_id,
            tolerance: None,
        });
    }
    admission.reserve_entity(&mut ir.model.bodies, "catia_family_emit_bodies")?;
    let region_body_id = crate::resource::copy_id(admission.context(),
        body_id.as_str(), BodyId::mint, "catia_freeform_wire_region_body_id")?;
    let body_region_id = crate::resource::copy_id(admission.context(),
        region_id.as_str(), RegionId::mint, "catia_freeform_wire_body_region_id")?;
    let mut body_regions = Vec::new();
    crate::resource::push(admission.context(), &mut body_regions, body_region_id,
        "catia_freeform_wire_body_regions")?;
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::Wire,
        regions: body_regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    admission.reserve_entity(&mut ir.model.regions, "catia_family_emit_regions")?;
    let mut region_shells = Vec::new();
    crate::resource::push(admission.context(), &mut region_shells, shell_owner_id,
        "catia_freeform_wire_region_shells")?;
    ir.model.regions.push(Region {
        id: region_id,
        body: region_body_id,
        shells: region_shells,
    });
    admission.reserve_entity(&mut ir.model.shells, "catia_family_emit_shells")?;
    ir.model.shells.push(shell);
    Ok(true)
}

fn freeform_surface_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Vec<FreeformSurfaceCarrier>, cadmpeg_core::CodecError> {
    let resolved = crate::families::a5a8::records::resolved_a8_surfaces(ctx, data, refusal)?;
    let a5 = crate::families::a5a8::records::a5_surfaces_from_records(ctx, data, records, refusal)?;
    let mut surfaces = Vec::new();
    for surface in resolved.into_iter().chain(a5) {
        let (source_object, source_tag) = freeform_surface_source(ctx, &surface)?;
        let source_tag = crate::resource::format_retained(ctx,
            format_args!("freeform:{source_tag}"), "catia_freeform_surface_source_tag")?;
        crate::resource::push(ctx, &mut surfaces, FreeformSurfaceCarrier {
            pos: surface.pos,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.geometry)),
            source_object,
            source_tag,
        }, "catia_freeform_surface_carriers")?;
    }
    for surface in crate::families::b2::records::b2_cylinders_from_records(data, records) {
        let source_object = cgm_source_key(ctx, "b2-03-28-frame", format_args!("{:010}", surface.pos))?;
        let source_tag = crate::resource::format_retained(ctx,
            format_args!("b2_03_28:frame_offset:{:010}", surface.pos), "catia_freeform_surface_source_tag")?;
        crate::resource::push(ctx, &mut surfaces, FreeformSurfaceCarrier {
            pos: surface.pos, geometry: surface.surface_geometry(), source_object, source_tag,
        }, "catia_freeform_surface_carriers")?;
    }
    for surface in crate::families::b2::records::b2_embedded_cylinders_from_records(data, records) {
        let source_object = cgm_source(ctx, "surface", surface.object_id)?;
        let source_tag = crate::resource::format_retained(ctx,
            format_args!("b2_03_60:object_id:{:08x}", surface.object_id), "catia_freeform_surface_source_tag")?;
        crate::resource::push(ctx, &mut surfaces, FreeformSurfaceCarrier {
            pos: surface.pos, geometry: surface.cylinder.surface_geometry(), source_object, source_tag,
        }, "catia_freeform_surface_carriers")?;
    }
    for surface in crate::families::b2::records::b2_cones_from_records(data, records) {
        let source_object = cgm_source_key(ctx, "b2-03-29-frame", format_args!("{:010}", surface.pos))?;
        let source_tag = crate::resource::format_retained(ctx,
            format_args!("b2_03_29:frame_offset:{:010}", surface.pos), "catia_freeform_surface_source_tag")?;
        crate::resource::push(ctx, &mut surfaces, FreeformSurfaceCarrier {
            pos: surface.pos, geometry: crate::families::b2::records::b2_cone_geometry(&surface),
            source_object, source_tag,
        }, "catia_freeform_surface_carriers")?;
    }
    for surface in crate::families::b2::records::b2_spheres_from_records(data, records) {
        let source_object = cgm_source_key(ctx, "b2-03-2a-frame", format_args!("{:010}", surface.pos))?;
        let source_tag = crate::resource::format_retained(ctx,
            format_args!("b2_03_2a:frame_offset:{:010}", surface.pos), "catia_freeform_surface_source_tag")?;
        crate::resource::push(ctx, &mut surfaces, FreeformSurfaceCarrier {
            pos: surface.pos, geometry: crate::families::b2::records::b2_sphere_geometry(&surface),
            source_object, source_tag,
        }, "catia_freeform_surface_carriers")?;
    }
    for surface in crate::families::b2::records::b2_tori_from_records(data, records) {
        let source_object = cgm_source_key(ctx, "b2-03-2b-frame", format_args!("{:010}", surface.pos))?;
        let source_tag = crate::resource::format_retained(ctx,
            format_args!("b2_03_2b:frame_offset:{:010}", surface.pos), "catia_freeform_surface_source_tag")?;
        crate::resource::push(ctx, &mut surfaces, FreeformSurfaceCarrier {
            pos: surface.pos, geometry: crate::families::b2::records::b2_torus_geometry(&surface),
            source_object, source_tag,
        }, "catia_freeform_surface_carriers")?;
    }
    Ok(surfaces)
}

fn freeform_surface_source(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &crate::families::a5a8::records::FreeformSurface,
) -> Result<(cadmpeg_ir::SourceObjectAssociation, String), cadmpeg_core::CodecError> {
    Ok(match surface.identity {
        Some(object_id) => (
            cgm_source(ctx, "surface", object_id)?,
            crate::resource::format_retained(ctx, format_args!("object_id:{object_id:08x}"),
                "catia_freeform_surface_source_tag")?,
        ),
        None => (
            cgm_source_key(ctx, "a5-surface-frame", format_args!("{:010}", surface.pos))?,
            crate::resource::format_retained(ctx, format_args!("frame_offset:{:010}", surface.pos),
                "catia_freeform_surface_source_tag")?,
        ),
    })
}

/// Index standard carrier surfaces by their serialized carrier tag.
///
/// A consolidated pcurve support id is admitted as a standard carrier only
/// when that tag selects one decoded surface with known geometry. Duplicate
/// tags and unknown geometry remain explicitly unresolved; an allocation id
/// must not choose a face-local row by emission order.
fn standard_carrier_surface_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
) -> Result<HashMap<u32, Option<SurfaceId>>, cadmpeg_core::CodecError> {
    let mut by_tag = HashMap::new();
    for surface in &ir.model.surfaces {
        let Some(source) = surface.source_object.as_ref() else {
            continue;
        };
        if source.format != cadmpeg_ir::codec_format!(crate::dialect::FORMAT) {
            continue;
        }
        let Some(tag) = source
            .object_id
            .as_str()
            .strip_prefix("cgm-carrier:")
            .and_then(|value| u32::from_str_radix(value, 16).ok())
        else {
            continue;
        };
        if let Some(selected) = by_tag.get_mut(&tag) {
            *selected = None;
            continue;
        }
        let candidate = (!matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        ))
        .then(|| {
            crate::resource::copy_id(
                ctx,
                surface.id.as_str(),
                SurfaceId::mint,
                "catia_freeform_standard_surface_id",
            )
        })
        .transpose()?;
        crate::resource::insert_map(
            ctx,
            &mut by_tag,
            tag,
            candidate,
            "catia_freeform_standard_carrier_tags",
        )?;
    }
    Ok(by_tag)
}

fn standard_carrier_endpoint_loci(
    pcurve: &PcurveGeometry,
    surface: &SurfaceGeometry,
    range: [f64; 2],
) -> Option<[Point3; 2]> {
    let start = cadmpeg_ir::eval::pcurve_uv(pcurve, range[0]).ok()?;
    let end = cadmpeg_ir::eval::pcurve_uv(pcurve, range[1]).ok()?;
    // A non-finite locus is kept as the evaluation reached it.
    let locus = |uv: cadmpeg_ir::units::FinitePoint2| match cadmpeg_ir::eval::surface_point(
        surface, uv.u, uv.v,
    ) {
        Ok(point) => Some(point.get()),
        Err(failure) => failure.non_finite(),
    };
    Some([locus(start)?, locus(end)?])
}

/// One exact consolidated line carrier: the curve it states, the wire interval
/// it owns, and the record position it was read at.
struct ConsolidatedLineProfile {
    curve: Curve,
    range: [f64; 2],
    pos: usize,
}

/// Every exact consolidated line carrier the records state, independently of
/// its parameter chart.
fn consolidated_line_profiles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
) -> Result<Vec<ConsolidatedLineProfile>, cadmpeg_core::CodecError> {
    let mut profiles = Vec::new();
    for (index, line) in crate::families::b2::records::b2_line_profiles_from_records(data, records)
        .into_iter()
        .enumerate()
    {
        let id = CurveId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "consolidated", "line-profile-curve"),
            index,
        );
        let payload =
            cadmpeg_ir::geometry::analytic::LineCurve::new(line.origin, line.direction.into());
        crate::resource::push(ctx, &mut profiles, ConsolidatedLineProfile {
            curve: Curve {
                id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(payload)),
                source_object: Some(cgm_source_key(ctx,
                    "b2-03-0e-frame",
                    format_args!("{:010}", line.pos),
                )?),
            },
            range: line.range.endpoints(),
            pos: line.pos,
        }, "catia_consolidated_line_profiles")?;
    }
    Ok(profiles)
}

/// Transfer every exact consolidated line carrier.
fn append_consolidated_line_profiles(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    profiles: Vec<ConsolidatedLineProfile>,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    for profile in profiles {
        annotate(
            admission.context(),
            annotations,
            &profile.curve.id,
            "consolidated_b2_03_0e",
            profile.pos as u64,
            "line_profile_carrier",
            Exactness::ByteExact)?;
        admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves")?;
        ir.model.curves.push(profile.curve);
    }
    Ok(())
}

/// Append standalone freeform carriers and return the number of consolidated
/// surface curves bound to existing standard edges.
pub(super) fn append_freeform_surface_pools(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
    surface_alias_tags: &HashMap<u32, Option<u32>>,
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<ConsolidatedCurveBindingCounts, cadmpeg_core::CodecError> {
    let mut surfaces =
        crate::families::a5a8::records::resolved_a8_surfaces(admission.context(), data, refusal)?;
    let a5 = crate::families::a5a8::records::a5_surfaces_from_records(
        admission.context(),
        data,
        records,
        refusal,
    )?;
    crate::resource::reserve_vec(admission.context(), &mut surfaces, a5.len(),
        "catia_freeform_surface_pool")?;
    surfaces.extend(a5);
    let mut carrier_ids = Vec::new();
    for surface in &surfaces {
        let (source_object, source_tag) = freeform_surface_source(admission.context(), surface)?;
        let index = ir.model.surfaces.len();
        let id = crate::resource::compose_index_id(admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "freeform", "surf"),
            index,
            SurfaceId::mint, "catia_freeform_surface_pool_id")?;
        crate::resource::push(admission.context(), &mut carrier_ids,
            crate::resource::copy_id(admission.context(), id.as_str(), SurfaceId::mint,
                "catia_freeform_surface_pool_carrier_id")?,
            "catia_freeform_surface_pool_carrier_ids")?;
        annotate(
            admission.context(),
            annotations,
            &id,
            "object_stream_a8_03_or_consolidated_a5_03",
            surface.pos as u64,
            source_tag,
            Exactness::ByteExact)?;
        admission.reserve_entity(&mut ir.model.surfaces, "catia_family_emit_surfaces")?;
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                crate::resource::copy_nurbs_surface(admission.context(), &surface.geometry,
                    "catia_freeform_surface_pool_geometry")?,
            )),
            source_object: Some(source_object),
        });
    }

    let offsets = crate::families::b2::records::b2_offset_supports_from_records(
        admission.context(), data, records)?;
    let bindings = crate::families::b2::records::offset_support_carriers(
        admission.context(), &offsets, &surfaces)?;
    for (offset, carrier) in offsets
        .iter()
        .zip(bindings)
        .filter_map(|(offset, carrier)| Some((offset, carrier?)))
    {
        let surface_index = ir.model.surfaces.len();
        let surface_id = SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "offset", "surf"),
            surface_index,
        );
        annotate(
            admission.context(),
            annotations,
            &surface_id,
            "consolidated_b2_03_31_cache",
            offset.pos as u64,
            format_args!("support_ref:{:08x}", offset.support_id),
            Exactness::Unknown)?;
        admission.reserve_entity(&mut ir.model.surfaces, "catia_family_emit_surfaces")?;
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });

        let procedural_id = ProceduralSurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "offset", "construction"),
            ir.model.procedural_surfaces.len(),
        );
        annotate(
            admission.context(),
            annotations,
            &procedural_id,
            "consolidated_b2_03_31",
            offset.pos as u64,
            format_args!("support_ref:{:08x}", offset.support_id),
            Exactness::ByteExact)?;
        admission.reserve_entity(&mut ir.model.procedural_surfaces, "catia_family_emit_procedural_surfaces")?;
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            ProceduralSurface::new(
                procedural_id,
                ProceduralSurfaceDefinition::Offset(
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
                        crate::resource::copy_id(admission.context(), carrier_ids[carrier].as_str(),
                            SurfaceId::mint, "catia_freeform_offset_carrier_id")?,
                        offset.distance,
                        None,
                        None,
                        false,
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                        None,
                    ),
                ),
                Some(RecordBounds::from_corners(offset.u_range, offset.v_range)),
            ),
        );
    }

    append_consolidated_line_profiles(
        ir,
        annotations,
        consolidated_line_profiles(admission.context(), data, records)?,
        admission,
    )?;

    for guide in crate::families::a5a8::records::a5_guide_curves_from_records(
        admission.context(),
        data,
        records,
    )? {
        let solution = {
            let ctx = admission.context();
            let (mut points, _points_reservation) = crate::resource::temporary_vec(ctx,
                guide.sites.len(), "catia A5 guide points")?;
            let (mut first, _first_reservation) = crate::resource::temporary_vec(ctx,
                guide.sites.len(), "catia A5 guide first jets")?;
            let (mut second, _second_reservation) = crate::resource::temporary_vec(ctx,
                guide.sites.len(), "catia A5 guide second jets")?;
            for site in &guide.sites {
                points.push(site.point.get());
                {
                    let value = site.first_derivative;
                    first.push([value[0], value[1], value[2]]);
                }
                {
                    let value = site.second_derivative;
                    second.push([value[0], value[1], value[2]]);
                }
            }
            let distinct_knots = guide.knots(ctx)?;
            crate::nurbs::quintic_jet_bspline(ctx, guide.degree, &distinct_knots,
                &points, &first, &second)?
        };
        let Some((knots, control_points)) = solution else {
            continue;
        };
        let mut poles = Vec::new();
        crate::resource::reserve_vec(
            admission.context(),
            &mut poles,
            control_points.len(),
            "catia A5 guide poles",
        )?;
        poles.extend(
            control_points
                .into_iter()
                .map(|point| Point3::new(point[0], point[1], point[2])),
        );
        let geometry = NurbsCurve::from_lanes(guide.degree, knots, poles, None, false)?;
        let id = CurveId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "guide", "curve"),
            ir.model.curves.len(),
        );
        annotate(
            admission.context(),
            annotations,
            &id,
            "consolidated_a5_03_39",
            guide.pos as u64,
            format_args!("header_token:{:08x}", guide.header_token),
            Exactness::Derived)?;
        admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves")?;
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
            source_object: None,
        });
    }

    for jet in crate::families::a5a8::records::a5_freeform_curves_from_records(
        admission.context(),
        data,
        records,
    )? {
        for second_limit in [false, true] {
            let Some(curve) = crate::families::a5a8::records::rolling_ball_limit_curve(
                admission.context(),
                &jet,
                second_limit,
                refusal,
            )?
            else {
                continue;
            };
            let side = usize::from(second_limit);
            let id = CurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "rolling-ball", "limit"),
                cadmpeg_ir::ids::IdentityKey::from(jet.pos).colon(side),
            );
            annotate(
                admission.context(),
                annotations,
                &id,
                "consolidated_a5_03_32",
                jet.pos as u64,
                format_args!("limit_{}", side + 1),
                Exactness::Derived)?;
            admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves")?;
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                source_object: None,
            });
        }
        let stations = crate::resource::collect_vec(
            admission.context(),
            jet.sites.iter().map(|sample| cadmpeg_ir::geometry::RollingBallJetStation {
                knot: sample.knot,
                multiplicity: crate::families::a5a8::records::A5FreeformCurve::DEGREE + 1,
                site: crate::families::a5a8::records::rolling_ball_jet_site(
                    &sample.site,
                    sample.first_derivatives,
                    sample.second_derivatives,
                ),
            }),
            "catia_freeform_rolling_ball_stations",
        )?;
        let surface_index = ir.model.surfaces.len();
        let surface_id = SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "rolling-ball", "surf"),
            surface_index,
        );
        let procedural_id = ProceduralSurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "rolling-ball", "construction"),
            ir.model.procedural_surfaces.len(),
        );
        annotate(
            admission.context(),
            annotations,
            &surface_id,
            "consolidated_a5_03_32_cache",
            jet.pos as u64,
            format_args!("header_token:{:08x}", jet.header_token),
            Exactness::Unknown)?;
        admission.reserve_entity(&mut ir.model.surfaces, "catia_family_emit_surfaces")?;
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: procedural_id.clone(),
                cache: None,
            },
            source_object: None,
        });

        annotate(
            admission.context(),
            annotations,
            &procedural_id,
            "consolidated_a5_03_32",
            jet.pos as u64,
            format_args!("header_token:{:08x}", jet.header_token),
            Exactness::ByteExact)?;
        admission.reserve_entity(&mut ir.model.procedural_surfaces, "catia_family_emit_procedural_surfaces")?;
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            procedural_id,
            ProceduralSurfaceDefinition::RollingBallJet(
                cadmpeg_ir::geometry::RollingBallJetStations::from_admitted(
                    crate::families::a5a8::records::A5FreeformCurve::DEGREE,
                    stations,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
            ),
            None,
        ));
    }

    append_a8_rolling_ball_pools(ir, annotations, data, admission)?;
    let counts = append_resolved_consolidated_surface_curves(
        ir,
        annotations,
        data,
        records,
        FreeformSurfacePool {
            surfaces: &surfaces,
            surface_ids: &carrier_ids,
            surface_alias_tags,
        },
        refusal,
        admission,
    )?;
    Ok(counts)
}

type ConsolidatedCarrierKey = (usize, Option<u64>);

enum ConsolidatedCarrierChart<'a> {
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
pub(in crate::families) struct ConsolidatedCurveBindingCounts {
    pub(super) standard_edges: usize,
    pub(super) partner_supports: usize,
    pub(super) partner_face_pcurve_pairs: usize,
    pub(super) standard_face_surfaces: usize,
    /// Coedge pcurves bound after the endpoint-lift witness.
    pub(super) standard_face_pcurves: usize,
    /// Recharts rejected because the derived pcurve coordinates were non-finite.
    pub(super) rechart_numeric_failures: usize,
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
                u / cone.angular_scale.get(),
                (v - cone.slant_range.lower()) * cone.half_angle.get().cos(),
            ],
            Self::Torus { torus } => [u / torus.major_scale.get(), v / torus.minor_scale.get()],
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
            Self::Cone { cone } => [
                u / cone.angular_scale.get(),
                v * cone.half_angle.get().cos(),
            ],
            Self::Torus { torus } => [u / torus.major_scale.get(), v / torus.minor_scale.get()],
            Self::Rigid { linear, .. } => [
                linear[0][0] * u + linear[0][1] * v,
                linear[1][0] * u + linear[1][1] * v,
            ],
        }
    }
}

fn consolidated_jet_pcurve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &crate::wire::records::ConsolidatedPcurve,
    chart: &ConsolidatedCarrierChart<'_>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<PcurveGeometry>, cadmpeg_core::CodecError> {
    let count = pcurve.sites.len();
    let (mut points, _points_reservation) = crate::resource::temporary_vec(ctx,
        count, "catia consolidated pcurve points")?;
    let (mut first, _first_reservation) = crate::resource::temporary_vec(ctx,
        count, "catia consolidated pcurve first jets")?;
    let (mut second, _second_reservation) = crate::resource::temporary_vec(ctx,
        count, "catia consolidated pcurve second jets")?;
    let (mut knots, _knots_reservation) = crate::resource::temporary_vec(ctx,
        count, "catia consolidated pcurve knots")?;
    for site in &pcurve.sites {
        points.push(chart.point(site.point.get()));
        first.push(chart.derivative(site.first_derivatives.get()));
        second.push(chart.derivative(site.second_derivatives.get()));
        knots.push(site.knot.get());
    }
    quintic_jet_pcurve(
        ctx,
        crate::wire::records::ConsolidatedPcurve::DEGREE,
        &knots,
        &points,
        &first,
        &second,
        refusal,
        format_args!("consolidated quintic-jet pcurve record at byte {}", pcurve.pos),
    )
}

/// Transfer resolved consolidated surface curves, reusing an existing
/// pcurve-less standard edge construction when endpoint loci select one.
/// The freeform surface pool a consolidated surface curve binds against.
///
/// The surfaces, the ids they were emitted under, and the alias tags that
/// name them are one pool: the ids are index-aligned with the surfaces, and
/// the alias tags resolve a record's support onto the same pool.
#[derive(Clone, Copy)]
struct FreeformSurfacePool<'a> {
    /// Decoded freeform surfaces in emission order.
    surfaces: &'a [crate::families::a5a8::records::FreeformSurface],
    /// Ids the surfaces were emitted under, index-aligned with `surfaces`.
    surface_ids: &'a [SurfaceId],
    /// Support alias tags that resolve a record's support onto the pool.
    surface_alias_tags: &'a HashMap<u32, Option<u32>>,
}

fn append_resolved_consolidated_surface_curves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
    records: &[crate::wire::records::ConsolidatedRecord],
    pool: FreeformSurfacePool<'_>,
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<ConsolidatedCurveBindingCounts, cadmpeg_core::CodecError> {
    let FreeformSurfacePool {
        surfaces: freeform_surfaces,
        surface_ids: freeform_surface_ids,
        surface_alias_tags,
    } = pool;
    let ctx = admission.context();
    let mut standalone = HashMap::new();
    for cylinder in crate::families::b2::records::b2_cylinders_from_records(data, records) {
        crate::resource::insert_map(
            ctx,
            &mut standalone,
            cylinder.pos,
            cylinder,
            "catia_freeform_standalone_cylinders",
        )?;
    }
    let mut embedded = HashMap::new();
    for value in crate::families::b2::records::b2_embedded_cylinders_from_records(data, records) {
        crate::resource::insert_map(
            ctx,
            &mut embedded,
            value.pos,
            value,
            "catia_freeform_embedded_cylinders",
        )?;
    }
    let mut cones = HashMap::new();
    for cone in crate::families::b2::records::b2_cones_from_records(data, records) {
        crate::resource::insert_map(ctx, &mut cones, cone.pos, cone, "catia_freeform_cones")?;
    }
    let mut spheres = HashMap::new();
    for sphere in crate::families::b2::records::b2_spheres_from_records(data, records) {
        crate::resource::insert_map(
            ctx,
            &mut spheres,
            sphere.pos,
            sphere,
            "catia_freeform_spheres",
        )?;
    }
    let mut tori = HashMap::new();
    for torus in crate::families::b2::records::b2_tori_from_records(data, records) {
        crate::resource::insert_map(ctx, &mut tori, torus.pos, torus, "catia_freeform_tori")?;
    }
    let mut planes = HashMap::new();
    for plane in crate::families::b2::records::b2_plane_carriers_from_records(ctx, data, records)? {
        crate::resource::insert_map(ctx, &mut planes, plane.pos, plane, "catia_freeform_planes")?;
    }
    let mut complete_runs = HashMap::new();
    for run in crate::families::consolidated::records::consolidated_topology_edge_runs_from_records(
        ctx, data, records,
    )? {
        crate::resource::insert_map(
            ctx,
            &mut complete_runs,
            run.edge.pcurves[0].pos,
            run,
            "catia_freeform_complete_runs",
        )?;
    }

    let mut surface_ids = HashMap::<ConsolidatedCarrierKey, SurfaceId>::new();
    let standard_carrier_surfaces = standard_carrier_surface_ids(ctx, ir)?;
    let mut point_positions = HashMap::new();
    for point in &ir.model.points {
        let id = crate::resource::copy_id(
            ctx,
            point.id.as_str(),
            PointId::mint,
            "catia_freeform_point_position_id",
        )?;
        crate::resource::insert_map(
            ctx,
            &mut point_positions,
            id,
            point.position().get(),
            "catia_freeform_point_positions",
        )?;
    }
    let mut vertex_positions = HashMap::new();
    let mut vertex_tolerances = HashMap::new();
    for vertex in &ir.model.vertices {
        if let Some(position) = point_positions.get(&vertex.point) {
            let id = crate::resource::copy_id(
                ctx,
                vertex.id.as_str(),
                VertexId::mint,
                "catia_freeform_vertex_position_id",
            )?;
            crate::resource::insert_map(
                ctx,
                &mut vertex_positions,
                id,
                *position,
                "catia_freeform_vertex_positions",
            )?;
        }
        let id = crate::resource::copy_id(
            ctx,
            vertex.id.as_str(),
            VertexId::mint,
            "catia_freeform_vertex_tolerance_id",
        )?;
        crate::resource::insert_map(
            ctx,
            &mut vertex_tolerances,
            id,
            vertex.tolerance,
            "catia_freeform_vertex_tolerances",
        )?;
    }
    let mut curve_indices = HashMap::new();
    for (index, curve) in ir.model.curves.iter().enumerate() {
        let id = crate::resource::copy_id(
            ctx,
            curve.id.as_str(),
            CurveId::mint,
            "catia_freeform_curve_index_id",
        )?;
        crate::resource::insert_map(
            ctx,
            &mut curve_indices,
            id,
            index,
            "catia_freeform_curve_indices",
        )?;
    }
    let mut face_surfaces = HashMap::new();
    let mut face_tolerances = HashMap::new();
    for face in &ir.model.faces {
        let face_id = crate::resource::copy_id(
            ctx,
            face.id.as_str(),
            FaceId::mint,
            "catia_freeform_face_surface_id",
        )?;
        let surface_id = crate::resource::copy_id(
            ctx,
            face.surface.as_str(),
            SurfaceId::mint,
            "catia_freeform_face_surface_value",
        )?;
        crate::resource::insert_map(
            ctx,
            &mut face_surfaces,
            face_id,
            surface_id,
            "catia_freeform_face_surfaces",
        )?;
        let face_id = crate::resource::copy_id(
            ctx,
            face.id.as_str(),
            FaceId::mint,
            "catia_freeform_face_tolerance_id",
        )?;
        crate::resource::insert_map(
            ctx,
            &mut face_tolerances,
            face_id,
            face.tolerance,
            "catia_freeform_face_tolerances",
        )?;
    }
    let mut loop_surfaces = HashMap::new();
    for loop_ in &ir.model.loops {
        let Some(surface_id) = face_surfaces.get(&loop_.face) else {
            continue;
        };
        let loop_id = crate::resource::copy_id(
            ctx,
            loop_.id.as_str(),
            LoopId::mint,
            "catia_freeform_loop_surface_id",
        )?;
        let surface_id = crate::resource::copy_id(
            ctx,
            surface_id.as_str(),
            SurfaceId::mint,
            "catia_freeform_loop_surface_value",
        )?;
        crate::resource::insert_map(
            ctx,
            &mut loop_surfaces,
            loop_id,
            surface_id,
            "catia_freeform_loop_surfaces",
        )?;
    }
    let mut coedge_surfaces = Vec::new();
    let mut coedge_face_tolerances = Vec::new();
    for coedge in &ir.model.coedges {
        let surface = loop_surfaces
            .get(&coedge.owner_loop)
            .map(|id| {
                crate::resource::copy_id(
                    ctx,
                    id.as_str(),
                    SurfaceId::mint,
                    "catia_freeform_coedge_surface_id",
                )
            })
            .transpose()?;
        crate::resource::push(
            ctx,
            &mut coedge_surfaces,
            surface,
            "catia_freeform_coedge_surfaces",
        )?;
        let tolerance = ir
            .model
            .loops
            .iter()
            .find(|value| value.id == coedge.owner_loop)
            .and_then(|loop_| face_tolerances.get(&loop_.face).copied().flatten());
        crate::resource::push(
            ctx,
            &mut coedge_face_tolerances,
            tolerance,
            "catia_freeform_coedge_face_tolerances",
        )?;
    }
    let mut attachable_edges = Vec::new();
    for (edge_index, edge) in ir.model.edges.iter().enumerate() {
        let candidate = (|| {
            let curve_id = edge.curve()?;
            let curve_index = *curve_indices.get(curve_id)?;
            let CurveGeometry::Procedural {
                construction,
                cache: Some(cache),
            } = &ir.model.curves[curve_index].geometry
            else {
                return None;
            };
            if !matches!(cache, SolvedCurveGeometry::Unknown { .. }) {
                return None;
            }
            let mut procedure_match = None;
            for (index, procedure) in ir.model.procedural_curves.iter().enumerate() {
                if procedure.id != *construction {
                    continue;
                }
                let ProceduralCurveDefinition::Intersection { context, .. } =
                    procedure.definition()
                else {
                    continue;
                };
                let sides = context.sides();
                let (Some(first), Some(second)) = (
                    (sides[0].pcurve.is_none())
                        .then_some(sides[0].surface.as_ref())
                        .flatten(),
                    (sides[1].pcurve.is_none())
                        .then_some(sides[1].surface.as_ref())
                        .flatten(),
                ) else {
                    continue;
                };
                if procedure_match.replace((index, [first, second])).is_some() {
                    return None;
                }
            }
            let (procedure_index, standard_surfaces) = procedure_match?;
            Some((
                edge_index,
                procedure_index,
                curve_id,
                standard_surfaces,
                [
                    *vertex_positions.get(&edge.start)?,
                    *vertex_positions.get(&edge.end)?,
                ],
            ))
        })();
        let Some((edge_index, procedure_index, curve_id, surfaces, points)) = candidate else {
            continue;
        };
        let curve_id = crate::resource::copy_id(
            ctx,
            curve_id.as_str(),
            CurveId::mint,
            "catia_freeform_attachable_curve_id",
        )?;
        let standard_surfaces = [
            crate::resource::copy_id(
                ctx,
                surfaces[0].as_str(),
                SurfaceId::mint,
                "catia_freeform_attachable_first_surface",
            )?,
            crate::resource::copy_id(
                ctx,
                surfaces[1].as_str(),
                SurfaceId::mint,
                "catia_freeform_attachable_second_surface",
            )?,
        ];
        crate::resource::push(
            ctx,
            &mut attachable_edges,
            (
                edge_index,
                procedure_index,
                curve_id,
                standard_surfaces,
                points,
            ),
            "catia_freeform_attachable_edges",
        )?;
    }
    let mut attached_curves = HashSet::new();
    let mut binding_counts = ConsolidatedCurveBindingCounts::default();
    let mut partner_support_blocks = HashSet::new();

    let mut pending = VecDeque::new();
    for resolved in
        crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
            ctx, data, records, refusal,
        )?
    {
        crate::resource::push_back(ctx, &mut pending, resolved, "catia_freeform_pending_edges")?;
    }
    while let Some(mut resolved) = pending.pop_front() {
        let Some(run) = complete_runs.get(&resolved.block.pcurves[0].pos) else {
            continue;
        };
        let mut sides: [IntcurveSupportSide; 2] = std::array::from_fn(|_| IntcurveSupportSide {
            surface: None,
            pcurve: None,
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
                let Some(geometry) = consolidated_jet_pcurve(
                    admission.context(),
                    pcurve,
                    &ConsolidatedCarrierChart::Identity,
                    refusal,
                )?
                else {
                    continue;
                };
                if standard_endpoint_loci.is_none() {
                    standard_endpoint_loci = standard_carrier_endpoint_loci(
                        &geometry,
                        surface_geometry,
                        resolved.block.parameters.range.endpoints(),
                    );
                }
                sides[side] = IntcurveSupportSide {
                    surface: Some(crate::resource::copy_id(
                        admission.context(),
                        surface_id.as_str(),
                        SurfaceId::mint,
                        "catia_freeform_standard_support_id",
                    )?),
                    pcurve: Some(geometry.into()),
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
                let Some(support) = freeform_surface_ids.get(carrier_index) else {
                    continue;
                };
                let support = crate::resource::copy_id(
                    admission.context(),
                    support.as_str(),
                    SurfaceId::mint,
                    "catia_freeform_nurbs_support_id",
                )?;
                let surface = if offset.get() == 0.0 {
                    support
                } else {
                    let key = (*pos, Some(offset.get().to_bits()));
                    if let Some(id) = surface_ids.get(&key) {
                        crate::resource::copy_id(
                            admission.context(),
                            id.as_str(),
                            SurfaceId::mint,
                            "catia_freeform_offset_surface_lookup_id",
                        )?
                    } else {
                        let id = crate::resource::compose_index_id(
                            admission.context(),
                            &cadmpeg_ir::identity_namespace!(
                                "catia",
                                "consolidated",
                                "nurbs-offset"
                            ),
                            ir.model.surfaces.len(),
                            SurfaceId::mint,
                            "catia_freeform_nurbs_offset_id",
                        )?;
                        annotate(
                            admission.context(),
                            annotations,
                            &id,
                            "consolidated_a5_03_34_offset_cache",
                            *pos as u64,
                            "resolved_pcurve_support",
                            Exactness::Unknown)?;
                        admission.reserve_entity(
                            &mut ir.model.surfaces,
                            "catia_freeform_offset_surfaces",
                        )?;
                        let surface_owner_id = crate::resource::copy_id(
                            admission.context(),
                            id.as_str(),
                            SurfaceId::mint,
                            "catia_freeform_offset_surface_owner_id",
                        )?;
                        ir.model.surfaces.push(Surface {
                            id: surface_owner_id,
                            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                                record: None,
                            }),
                            source_object: None,
                        });
                        let procedural_id = crate::resource::compose_index_id(
                            admission.context(),
                            &cadmpeg_ir::identity_namespace!(
                                "catia",
                                "consolidated",
                                "nurbs-offset-construction"
                            ),
                            ir.model.procedural_surfaces.len(),
                            ProceduralSurfaceId::mint,
                            "catia_freeform_nurbs_offset_construction_id",
                        )?;
                        annotate(
                            admission.context(),
                            annotations,
                            &procedural_id,
                            "consolidated_a5_03_34_constant_normal_offset",
                            *pos as u64,
                            "resolved_pcurve_support",
                            Exactness::Derived)?;
                        admission.reserve_entity(
                            &mut ir.model.procedural_surfaces,
                            "catia_freeform_offset_procedural_surfaces",
                        )?;
                        let procedural_owner_id = crate::resource::copy_id(
                            admission.context(),
                            id.as_str(),
                            SurfaceId::mint,
                            "catia_freeform_offset_procedural_owner_id",
                        )?;
                        let _attached = ir.model.add_procedural_surface(
                            procedural_owner_id,
                            ProceduralSurface::new(
                                procedural_id,
                                ProceduralSurfaceDefinition::Offset(
                                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
                                        support,
                                        *offset,
                                        None,
                                        None,
                                        false,
                                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                                        None,
                                    ),
                                ),
                                None,
                            ),
                        );
                        let stored_id = crate::resource::copy_id(
                            admission.context(),
                            id.as_str(),
                            SurfaceId::mint,
                            "catia_freeform_offset_surface_index_id",
                        )?;
                        crate::resource::insert_map(
                            admission.context(),
                            &mut surface_ids,
                            key,
                            stored_id,
                            "catia_freeform_offset_surface_index",
                        )?;
                        id
                    }
                };
                let chart = ConsolidatedCarrierChart::Identity;
                let Some(geometry) =
                    consolidated_jet_pcurve(admission.context(), pcurve, &chart, refusal)?
                else {
                    continue;
                };
                sides[side] = IntcurveSupportSide {
                    surface: Some(surface),
                    pcurve: Some(geometry.into()),
                };
                continue;
            }
            let (key, carrier, source_object, chart, annotation_kind, namespace) = match binding {
                Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Cylinder { pos }) => {
                    let Some(cylinder) = standalone.get(pos) else {
                        continue;
                    };
                    let carrier = cylinder.surface_geometry();
                    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = carrier else {
                        continue;
                    };
 let radius = cylinder_surface.radius().get();
                    (
                        (*pos, None),
                        carrier,
                        None,
                        ConsolidatedCarrierChart::Cylinder { radius },
                        "consolidated_b2_03_28_cylinder",
                        cadmpeg_ir::identity_namespace!("catia", "consolidated", "cylinder"),
                    )
                }
            Some(crate::families::consolidated::records::ConsolidatedSupportBinding::EmbeddedCylinder { pos, .. }) => {
                let Some(value) = embedded.get(pos) else {
                    continue;
                };
                let carrier = value.cylinder.surface_geometry();
                let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) = carrier else {
                    continue;
                };
                let radius = cylinder.radius().get();
                (
                    (*pos, None),
                    carrier,
                    Some(cgm_source(admission.context(), "surface", value.object_id)?),
                    ConsolidatedCarrierChart::Cylinder { radius },
                    "consolidated_b2_03_60_cylinder",
                    cadmpeg_ir::identity_namespace!("catia", "consolidated", "cylinder"),
                )
            }
            Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Cone { pos }) => {
                let Some(cone) = cones.get(pos) else {
                    continue;
                };
                (
                    (*pos, None),
                    crate::families::b2::records::b2_cone_geometry(cone),
                    None,
                    ConsolidatedCarrierChart::Cone { cone },
                    "consolidated_b2_03_29_cone",
                    cadmpeg_ir::identity_namespace!("catia", "consolidated", "cone"),
                )
            }
            Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Sphere { pos }) => {
                let Some(sphere) = spheres.get(pos) else {
                    continue;
                };
                (
                    (*pos, None),
                    crate::families::b2::records::b2_sphere_geometry(sphere),
                    None,
                    ConsolidatedCarrierChart::Identity,
                    "consolidated_b2_03_2a_sphere",
                    cadmpeg_ir::identity_namespace!("catia", "consolidated", "sphere"),
                )
            }
            Some(crate::families::consolidated::records::ConsolidatedSupportBinding::Torus { pos }) => {
                let Some(torus) = tori.get(pos) else {
                    continue;
                };
                (
                    (*pos, None),
                    crate::families::b2::records::b2_torus_geometry(torus),
                    None,
                    ConsolidatedCarrierChart::Torus { torus },
                    "consolidated_b2_03_2b_torus",
                    cadmpeg_ir::identity_namespace!("catia", "consolidated", "torus"),
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
                    (*pos, None),
                    carrier,
                    None,
                    ConsolidatedCarrierChart::Identity,
                    "consolidated_b2_03_27_plane",
                    cadmpeg_ir::identity_namespace!("catia", "consolidated", "plane"),
                )
            }
            Some(
                crate::families::consolidated::records::ConsolidatedSupportBinding::Circle { .. }
                | crate::families::consolidated::records::ConsolidatedSupportBinding::NurbsCarrier { .. },
            )
            | None => continue,
        };
            let surface = if let Some(id) = surface_ids.get(&key) {
                crate::resource::copy_id(
                    admission.context(),
                    id.as_str(),
                    SurfaceId::mint,
                    "catia_freeform_carrier_surface_lookup_id",
                )?
            } else {
                let id = crate::resource::compose_index_id(
                    admission.context(),
                    &namespace,
                    ir.model.surfaces.len(),
                    SurfaceId::mint,
                    "catia_freeform_carrier_surface_id",
                )?;
                annotate(
                    admission.context(),
                    annotations,
                    &id,
                    annotation_kind,
                    key.0 as u64,
                    "resolved_pcurve_support",
                    Exactness::ByteExact)?;
                admission
                    .reserve_entity(&mut ir.model.surfaces, "catia_freeform_carrier_surfaces")?;
                let surface_owner_id = crate::resource::copy_id(
                    admission.context(),
                    id.as_str(),
                    SurfaceId::mint,
                    "catia_freeform_carrier_surface_owner_id",
                )?;
                ir.model.surfaces.push(Surface {
                    id: surface_owner_id,
                    geometry: carrier,
                    source_object,
                });
                let stored_id = crate::resource::copy_id(
                    admission.context(),
                    id.as_str(),
                    SurfaceId::mint,
                    "catia_freeform_carrier_surface_index_id",
                )?;
                crate::resource::insert_map(
                    admission.context(),
                    &mut surface_ids,
                    key,
                    stored_id,
                    "catia_freeform_carrier_surface_index",
                )?;
                id
            };

            let Some(geometry) =
                consolidated_jet_pcurve(admission.context(), pcurve, &chart, refusal)?
            else {
                continue;
            };
            sides[side] = IntcurveSupportSide {
                surface: Some(surface),
                pcurve: Some(geometry.into()),
            };
        }
        if resolved.endpoint_loci.is_none() {
            resolved.endpoint_loci = standard_endpoint_loci;
        }
        let resolved_side = match sides
            .each_ref()
            .map(|side| side.surface.is_some() && side.pcurve.is_some())
        {
            [true, false] => Some(0),
            [false, true] => Some(1),
            _ => None,
        };
        let inferred_partner = if let Some(resolved_side) = resolved_side {
            let partner = 1 - resolved_side;
            let resolved_geometry = ir
                .model
                .surfaces
                .iter()
                .find(|surface| Some(&surface.id) == sides[resolved_side].surface.as_ref())
                .map(|surface| &surface.geometry);
            if let (Some(resolved_geometry), Some(resolved_pcurve)) =
                (resolved_geometry, sides[resolved_side].pcurve.as_ref())
            {
                if let Some(partner_pcurve) = consolidated_jet_pcurve(
                    admission.context(),
                    &resolved.block.pcurves[partner],
                    &ConsolidatedCarrierChart::Identity,
                    refusal,
                )? {
                    let mut candidates = Vec::new();
                    crate::resource::reserve_vec(
                        admission.context(),
                        &mut candidates,
                        freeform_surfaces.len(),
                        "catia consolidated partner surface candidates",
                    )?;
                    for (index, surface) in freeform_surfaces.iter().enumerate() {
                        candidates.push((
                            index,
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                                crate::resource::copy_nurbs_surface(
                                    admission.context(),
                                    &surface.geometry,
                                    "catia consolidated partner surface copy",
                                )?,
                            )),
                        ));
                    }
                    let carrier = unique_paired_surface_lift_match(
                        &resolved_pcurve.geometry,
                        resolved_geometry,
                        &partner_pcurve,
                        resolved.block.parameters.range.endpoints(),
                        candidates
                            .iter()
                            .map(|(index, geometry)| (*index, geometry)),
                    );
                    if let Some(carrier) = carrier {
                        sides[partner] = IntcurveSupportSide {
                            surface: Some(crate::resource::copy_id(
                                admission.context(),
                                freeform_surface_ids[carrier].as_str(),
                                SurfaceId::mint,
                                "catia_freeform_partner_surface_id",
                            )?),
                            pcurve: Some(partner_pcurve.into()),
                        };
                        crate::resource::insert_set(
                            admission.context(),
                            &mut partner_support_blocks,
                            resolved.block.pcurves[0].pos,
                            "catia_freeform_partner_support_blocks",
                        )?;
                        Some((resolved_side, carrier))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let exact_side_count = sides
            .iter()
            .filter(|side| side.surface.is_some() && side.pcurve.is_some())
            .count();
        if exact_side_count == 0 {
            continue;
        }
        let selected_attachment = resolved.endpoint_loci.as_ref().and_then(|loci| {
            unique_endpoint_pair_match(
                *loci,
                attachable_edges
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, _, curve, _, _))| !attached_curves.contains(curve))
                    .map(|(index, (_, _, _, _, endpoints))| (index, *endpoints)),
            )
        });
        let attachment = selected_attachment
            .map(|(index, reversed)| {
                let (edge, procedure, curve, surfaces, _) = &attachable_edges[index];
                Ok::<_, cadmpeg_core::CodecError>((
                    (
                        *edge,
                        *procedure,
                        crate::resource::copy_id(
                            admission.context(),
                            curve.as_str(),
                            CurveId::mint,
                            "catia_freeform_selected_curve_id",
                        )?,
                        [
                            crate::resource::copy_id(
                                admission.context(),
                                surfaces[0].as_str(),
                                SurfaceId::mint,
                                "catia_freeform_selected_first_surface",
                            )?,
                            crate::resource::copy_id(
                                admission.context(),
                                surfaces[1].as_str(),
                                SurfaceId::mint,
                                "catia_freeform_selected_second_surface",
                            )?,
                        ],
                    ),
                    reversed,
                ))
            })
            .transpose()?;
        macro_rules! option_or_none {
            ($value:expr) => {
                match $value {
                    Some(value) => value,
                    None => return Ok(None),
                }
            };
        }
        let attachment = match attachment {
            Some((identity, reversed)) => (|| -> Result<Option<_>, cadmpeg_core::CodecError> {
                if reversed {
                    let reversed_pcurves = sides.each_ref().map(|side| match &side.pcurve {
                        Some(pcurve) => crate::nurbs::reverse_pcurve_geometry(
                            admission.context(),
                            &pcurve.geometry,
                            resolved.block.parameters.range.endpoints(),
                            refusal,
                            &format!(
                                "consolidated surface-curve pcurve of the edge block at byte {} reversed onto its edge",
                                resolved.block.pcurves[0].pos
                            ),
                        ).map(|geometry| geometry.map(Some)),
                        None => Ok(Some(None)),
                    });
                    let [first, second] = reversed_pcurves;
                    let [Some(first), Some(second)] = [first?, second?] else {
                        return Ok(None);
                    };
                    for (side, pcurve) in sides.iter_mut().zip([first, second]) {
                        side.pcurve = pcurve.map(Into::into);
                    }
                }
                let (_, _, _, standard_surfaces) = &identity;
                let unique_resolved_side = match sides
                    .each_ref()
                    .map(|side| side.surface.is_some() && side.pcurve.is_some())
                {
                    [true, false] => Some(0),
                    [false, true] => Some(1),
                    _ => None,
                };
                let resolved_side = inferred_partner
                    .map(|(resolved_side, _)| resolved_side)
                    .or(unique_resolved_side);
                let partner_pcurves = if let Some(resolved_side) = resolved_side {
                    let resolved_surface = option_or_none!(sides[resolved_side].surface.as_ref());
                    let resolved_geometry = &option_or_none!(ir
                        .model
                        .surfaces
                        .iter()
                        .find(|surface| &surface.id == resolved_surface))
                    .geometry;
                    let matches = standard_surfaces.each_ref().map(|id| {
                        ir.model
                            .surfaces
                            .iter()
                            .find(|surface| &surface.id == id)
                            .is_some_and(|surface| {
                                same_surface_locus(&surface.geometry, resolved_geometry)
                            })
                    });
                    let standard_resolved_side = match matches {
                        [true, false] => Some(0),
                        [false, true] => Some(1),
                        _ => None,
                    };
                    if let Some(standard_resolved_side) = standard_resolved_side {
                        let partner = 1 - resolved_side;
                        if let Some((_, carrier)) = inferred_partner {
                            let standard_partner = &standard_surfaces[1 - standard_resolved_side];
                            let standard_partner_geometry = &option_or_none!(ir
                                .model
                                .surfaces
                                .iter()
                                .find(|surface| &surface.id == standard_partner))
                            .geometry;
                            if !matches!(
                                standard_partner_geometry,
                                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                            ) && *standard_partner_geometry
                                != SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                                    crate::resource::copy_nurbs_surface(
                                        admission.context(),
                                        &freeform_surfaces[carrier].geometry,
                                        "catia_freeform_matched_partner_surface",
                                    )?,
                                ))
                            {
                                return Ok(Some((identity, None)));
                            }
                        }
                        let standard_partner_geometry =
                            &option_or_none!(ir.model.surfaces.iter().find(|surface| {
                                surface.id == standard_surfaces[1 - standard_resolved_side]
                            }))
                            .geometry;
                        let partner_pcurve = match &sides[partner].pcurve {
                            Some(pcurve) => crate::resource::copy_pcurve_geometry(
                                admission.context(),
                                &pcurve.geometry,
                                "catia_freeform_partner_pcurve_copy",
                            )?,
                            None => {
                                // The free side stores its jet in its own carrier's
                                // chart, which is not the standard partner face's
                                // chart. Recover the isometry between them from the
                                // block's shared 3D loci.
                                let partner_points = crate::resource::collect_vec(
                                    admission.context(),
                                    resolved.block.pcurves[partner]
                                        .sites
                                        .iter()
                                        .map(|site| site.point.get()),
                                    "catia_freeform_partner_chart_points",
                                )?;
                                let Some(chart) = (match resolved.shared_loci.as_deref() {
                                    Some(loci) => solve_planar_chart_rechart(
                                        admission.context(),
                                        &partner_points,
                                        loci,
                                        standard_partner_geometry,
                                    )?,
                                    None => None,
                                }) else {
                                    // The free side has no defined chart relation
                                    // to a non-planar or unresolved partner.
                                    return Ok(Some((identity, None)));
                                };
                                let Some(mut pcurve) = consolidated_jet_pcurve(
                                    admission.context(),
                                    &resolved.block.pcurves[partner],
                                    &chart,
                                    refusal,
                                )?
                                else {
                                    return Ok(None);
                                };
                                if reversed {
                                    pcurve = option_or_none!(crate::nurbs::reverse_pcurve_geometry(
                                    admission.context(),
                                    &pcurve,
                                    resolved.block.parameters.range.endpoints(),
                                    refusal,
                                    &format!(
                                        "consolidated partner pcurve of the edge block at byte {} reversed onto its edge",
                                        resolved.block.pcurves[0].pos
                                    ),
                                )?);
                                }
                                pcurve
                            }
                        };
                        let standard_surface_geometry = &option_or_none!(ir
                            .model
                            .surfaces
                            .iter()
                            .find(
                                |surface| surface.id == standard_surfaces[standard_resolved_side]
                            ))
                        .geometry;
                        let resolved_pcurve = match rechart_equivalent_surface_pcurve(
                            admission.context(),
                            &option_or_none!(sides[resolved_side].pcurve.as_ref()).geometry,
                            resolved_geometry,
                            standard_surface_geometry,
                        ) {
                            Ok(Some(pcurve)) => pcurve,
                            Ok(None) => return Ok(None),
                            Err(RechartFailure::NonFinite) => {
                                binding_counts.rechart_numeric_failures += 1;
                                return Ok(None);
                            }
                            Err(RechartFailure::Resource(error)) => return Err(error),
                        };
                        let standard_geometries = if standard_resolved_side == 0 {
                            [resolved_pcurve, partner_pcurve]
                        } else {
                            [partner_pcurve, resolved_pcurve]
                        };
                        let edge = &ir.model.edges[identity.0];
                        let edge_id = &edge.id;
                        let edge_endpoints = [
                            *option_or_none!(vertex_positions.get(&edge.start)),
                            *option_or_none!(vertex_positions.get(&edge.end)),
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
                        .map(cadmpeg_ir::scalar::PositiveReal::get)
                        .fold(cadmpeg_ir::units::COINCIDENCE_TOLERANCE, f64::max);
                        let mut coedges = Vec::new();
                        for (side, surface) in standard_surfaces.iter().enumerate() {
                            let mut selected_coedge = None;
                            for (index, coedge) in ir.model.coedges.iter().enumerate() {
                                if coedge.edge == *edge_id
                                    && coedge.pcurves.is_empty()
                                    && coedge_surfaces[index].as_ref() == Some(surface)
                                    && selected_coedge.replace(index).is_some()
                                {
                                    selected_coedge = None;
                                    break;
                                }
                            }
                            let Some(coedge) = selected_coedge else {
                                continue;
                            };
                            let mut geometry = crate::resource::copy_pcurve_geometry(
                                admission.context(),
                                &standard_geometries[side],
                                "catia_freeform_standard_coedge_pcurve_copy",
                            )?;
                            if matches!(
                                ir.model.coedges[coedge].sense,
                                cadmpeg_ir::topology::Sense::Reversed
                            ) {
                                let Some(reversed_geometry) = crate::nurbs::reverse_pcurve_geometry(
                                    admission.context(),
                                    &geometry,
                                    resolved.block.parameters.range.endpoints(),
                                    refusal,
                                    &format!(
                                        "standard pcurve of the edge block at byte {} reversed onto coedge {coedge}",
                                        resolved.block.pcurves[0].pos
                                    ),
                                )? else { continue };
                                geometry = reversed_geometry;
                            }
                            // A pcurve binds to a face only when it lifts onto
                            // the edge through that face's carrier. Without the
                            // witness the side's chart is not this face's
                            // chart, and the pcurve does not describe the edge.
                            let Some(surface_geometry) = ir
                                .model
                                .surfaces
                                .iter()
                                .find(|value| &value.id == surface)
                                .map(|value| &value.geometry)
                            else {
                                continue;
                            };
                            let Some(solved_surface) = surface_geometry.solved() else {
                                continue;
                            };
                            let face_allowance = coedge_face_tolerances
                                .get(coedge)
                                .copied()
                                .flatten()
                                .map_or(edge_allowance, |value| edge_allowance.max(value.get()));
                            if pcurve_lift_reaches_endpoints(
                                &geometry,
                                solved_surface,
                                resolved.block.parameters.range.endpoints(),
                                edge_endpoints,
                                face_allowance,
                            ) {
                                crate::resource::push(
                                    admission.context(),
                                    &mut coedges,
                                    (coedge, geometry),
                                    "catia_freeform_standard_face_coedges",
                                )?;
                            }
                        }
                        if coedges.is_empty() {
                            None
                        } else {
                            Some(ConsolidatedStandardFaceBinding {
                                coedges,
                                standard_surfaces: [
                                    crate::resource::copy_id(
                                        admission.context(),
                                        standard_surfaces[0].as_str(),
                                        SurfaceId::mint,
                                        "catia_freeform_binding_surface_id",
                                    )?,
                                    crate::resource::copy_id(
                                        admission.context(),
                                        standard_surfaces[1].as_str(),
                                        SurfaceId::mint,
                                        "catia_freeform_binding_surface_id",
                                    )?,
                                ],
                                edge_pcurves: standard_geometries,
                                inferred_partner: inferred_partner
                                    .map(|(_, carrier)| (1 - standard_resolved_side, carrier)),
                            })
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                Ok(Some((identity, partner_pcurves)))
            })()?,
            None => None,
        };
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
                    if matches!(
                        surface.geometry,
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                    ) {
                        surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                            crate::resource::copy_nurbs_surface(
                                admission.context(),
                                &freeform_surfaces[carrier].geometry,
                                "catia_freeform_bound_partner_surface",
                            )?,
                        ));
                        crate::resource::derived_annotation(admission.context(), annotations, &surface.id, "geometry", "catia_annotation_field")?;
                        binding_counts.standard_face_surfaces += 1;
                        bound_new_standard_surface = true;
                    }
                    let copy_side = |side: usize| {
                        Ok::<_, cadmpeg_core::CodecError>(IntcurveSupportSide {
                            surface: Some(crate::resource::copy_id(
                                admission.context(),
                                binding.standard_surfaces[side].as_str(),
                                SurfaceId::mint,
                                "catia_freeform_replayed_support_surface",
                            )?),
                            pcurve: Some(
                                crate::resource::copy_pcurve_geometry(
                                    admission.context(),
                                    &binding.edge_pcurves[side],
                                    "catia_freeform_replayed_support_pcurve",
                                )?
                                .into(),
                            ),
                        })
                    };
                    sides = [copy_side(0)?, copy_side(1)?];
                }
            }
        }
        if bound_new_standard_surface {
            // The new carrier can make both coedge pcurves resolvable. Replay this
            // block after mutating the face geometry and emit only on that replay.
            crate::resource::push_back(
                admission.context(),
                &mut pending,
                resolved,
                "catia_freeform_pending_edges",
            )?;
            continue;
        }
        let context = IntcurveSupportContext::over_interval(sides, resolved.block.parameters.range);
        let definition = if exact_side_count == 2 {
            ProceduralCurveDefinition::Intersection {
                context,
                discontinuity_flag: false,
                cache: None,
            }
        } else {
            ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric {
                    context,
                    tail: None,
                },
            }
        };
        if let Some(((edge_index, procedure_index, curve_id, _), partner_pcurves)) = attachment {
            let attached_id = crate::resource::copy_id(
                admission.context(),
                curve_id.as_str(),
                CurveId::mint,
                "catia_freeform_attached_curve_id",
            )?;
            crate::resource::insert_set(
                admission.context(),
                &mut attached_curves,
                attached_id,
                "catia_freeform_attached_curves",
            )?;
            binding_counts.standard_edges += 1;
            if let Some(partner_pcurves) = partner_pcurves {
                if partner_pcurves.coedges.len() == 2 {
                    binding_counts.partner_face_pcurve_pairs += 1;
                }
                binding_counts.standard_face_pcurves += partner_pcurves.coedges.len();
                for (coedge_index, geometry) in partner_pcurves.coedges {
                    let pcurve_id = crate::resource::compose_index_id(
                        admission.context(),
                        &cadmpeg_ir::identity_namespace!(
                            "catia",
                            "consolidated",
                            "standard-pcurve"
                        ),
                        ir.model.pcurves.len(),
                        PcurveId::mint,
                        "catia_freeform_standard_pcurve_id",
                    )?;
                    annotate(
                        admission.context(),
                        annotations,
                        &pcurve_id,
                        "consolidated_edge_run",
                        run.edge.pcurves[0].pos as u64,
                        "resolved_face_side_pcurve",
                        Exactness::Derived)?;
                    admission
                        .reserve_entity(&mut ir.model.pcurves, "catia_freeform_standard_pcurves")?;
                    let pcurve_owner_id = crate::resource::copy_id(
                        admission.context(),
                        pcurve_id.as_str(),
                        PcurveId::mint,
                        "catia_freeform_standard_pcurve_owner_id",
                    )?;
                    ir.model.pcurves.push(Pcurve {
                        id: pcurve_owner_id,
                        geometry,
                        metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                            None,
                            Some(cadmpeg_ir::units::FiniteVector::from(
                                resolved.block.parameters.range,
                            )),
                            None,
                        ),
                    });
                    crate::resource::push(
                        admission.context(),
                        &mut ir.model.coedges[coedge_index].pcurves,
                        cadmpeg_ir::topology::PcurveUse {
                            pcurve: pcurve_id,
                            isoparametric: None,
                            parameter_range: None,
                        },
                        "catia_freeform_standard_coedge_pcurve_uses",
                    )?;
                    crate::resource::derived_annotation(admission.context(), annotations, &ir.model.coedges[coedge_index].id, "pcurves", "catia_annotation_field")?;
                }
            }
            ir.model.edges[edge_index].set_param_range(Some(
                cadmpeg_ir::topology::ParameterInterval::from(resolved.block.parameters.range),
            ));
            let procedural = &mut ir.model.procedural_curves[procedure_index];
            procedural.replace_definition(definition);
            annotate(
                admission.context(),
                annotations,
                &procedural.id,
                "consolidated_edge_run",
                run.edge.pcurves[0].pos as u64,
                "resolved_surface_curve_bound_to_standard_edge",
                Exactness::Derived)?;
            crate::resource::derived_annotation(admission.context(), annotations, &procedural.id, "curve", "catia_annotation_field")?;
            crate::resource::derived_annotation(admission.context(), annotations, &procedural.id, "definition", "catia_annotation_field")?;
        } else {
            let curve_id = crate::resource::compose_index_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "consolidated", "curve"),
                ir.model.curves.len(),
                CurveId::mint,
                "catia_freeform_resolved_curve_id",
            )?;
            annotate(
                admission.context(),
                annotations,
                &curve_id,
                "consolidated_edge_run",
                run.edge.pcurves[0].pos as u64,
                "procedural_curve_cache",
                Exactness::Unknown)?;
            admission.reserve_entity(&mut ir.model.curves, "catia_freeform_resolved_curves")?;
            let curve_owner_id = crate::resource::copy_id(
                admission.context(),
                curve_id.as_str(),
                CurveId::mint,
                "catia_freeform_resolved_curve_owner_id",
            )?;
            ir.model.curves.push(Curve {
                id: curve_owner_id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: None,
            });
            let procedural_id = crate::resource::compose_index_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "consolidated", "construction"),
                ir.model.procedural_curves.len(),
                ProceduralCurveId::mint,
                "catia_freeform_resolved_construction_id",
            )?;
            annotate(
                admission.context(),
                annotations,
                &procedural_id,
                "consolidated_edge_run",
                run.edge.pcurves[0].pos as u64,
                "resolved_surface_curve",
                Exactness::Derived)?;
            crate::resource::derived_annotation(admission.context(), annotations, &procedural_id, "curve", "catia_annotation_field")?;
            crate::resource::derived_annotation(admission.context(), annotations, &procedural_id, "definition", "catia_annotation_field")?;
            admission.reserve_entity(
                &mut ir.model.procedural_curves,
                "catia_freeform_resolved_procedural_curves",
            )?;
            let _attached = ir
                .model
                .add_procedural_curve(curve_id, ProceduralCurve::new(procedural_id, definition));
        }
    }
    binding_counts.partner_supports = partner_support_blocks.len();
    Ok(binding_counts)
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sites: &[[f64; 2]],
    loci: &[Point3],
    target: &SurfaceGeometry,
) -> Result<Option<ConsolidatedCarrierChart<'static>>, cadmpeg_core::CodecError> {
    if !matches!(
        target,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
    ) || sites.len() != loci.len()
    {
        return Ok(None);
    }
    // Target-chart image of each locus. A locus off the plane has no image,
    // because the plane inverse discards the normal component.
    let images = crate::resource::collect_options(
        ctx,
        loci.iter().map(|locus| {
            let uv = cadmpeg_ir::math::Point2::from(cadmpeg_ir::eval::analytic_surface_parameters(
                target, *locus,
            )?);
            let back = cadmpeg_ir::eval::surface_point(target, uv.u, uv.v).ok()?;
            ((back.x - locus.x)
                .hypot(back.y - locus.y)
                .hypot(back.z - locus.z)
                <= CONSOLIDATED_SITE_TOLERANCE)
                .then_some([uv.u, uv.v])
        }),
        "catia_freeform_chart_images",
    )?;
    let Some(images) = images else { return Ok(None) };
    let count = images.len();
    if count < 2 {
        return Ok(None);
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
    let centered = |values: &[[f64; 2]], center: [f64; 2]| -> Result<Option<Vec<[f64; 2]>>, cadmpeg_core::CodecError> {
        if values
            .iter()
            .flatten()
            .chain(&center)
            .any(|value| !value.is_finite())
        {
            return Ok(None);
        }
        let mut offsets = crate::resource::collect_vec(
            ctx,
            values.iter().map(|value| [value[0] - center[0], value[1] - center[1]]),
            "catia_freeform_chart_offsets",
        )?;
        let mut scale = offsets
            .iter()
            .flatten()
            .fold(0.0_f64, |scale, value| scale.max(value.abs()));
        if !scale.is_finite() {
            let frame = values
                .iter()
                .flatten()
                .chain(&center)
                .fold(0.0_f64, |scale, value| scale.max(value.abs()));
            offsets = crate::resource::collect_vec(
                ctx,
                values.iter().map(|value| {
                    [
                        value[0] / frame - center[0] / frame,
                        value[1] / frame - center[1] / frame,
                    ]
                }),
                "catia_freeform_chart_rescaled_offsets",
            )?;
            scale = offsets
                .iter()
                .flatten()
                .fold(0.0_f64, |scale, value| scale.max(value.abs()));
        }
        if scale == 0.0 {
            return Ok(None);
        }
        Ok(Some(crate::resource::collect_vec(
            ctx,
            offsets.into_iter().map(|value| [value[0] / scale, value[1] / scale]),
            "catia_freeform_chart_normalized_offsets",
        )?))
    };
    let Some(stored_offsets) = centered(sites, stored_center)? else { return Ok(None) };
    let Some(image_offsets) = centered(&images, image_center)? else { return Ok(None) };
    let (mut dot, mut cross) = (0.0, 0.0);
    let (mut reflected_dot, mut reflected_cross) = (0.0, 0.0);
    for ([su, sv], [iu, iv]) in stored_offsets.into_iter().zip(image_offsets) {
        dot += su * iu + sv * iv;
        cross += su * iv - sv * iu;
        reflected_dot += su * iu - sv * iv;
        reflected_cross += su * iv + sv * iu;
    }
    let candidates = [(dot, cross, 1.0), (reflected_dot, reflected_cross, -1.0)];
    let mut admissible = None;
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
            if admissible.is_some() {
                return Ok(None);
            }
            admissible = Some(chart);
        }
    }
    Ok(admissible)
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
    surface: &SolvedSurfaceGeometry,
    range: [f64; 2],
    endpoints: [Point3; 2],
    allowance: f64,
) -> bool {
    if matches!(surface, SolvedSurfaceGeometry::Unknown { .. }) {
        return false;
    }
    // A non-finite lift is measured as a finite one is.
    let lift = |parameter| {
        let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter).ok()?;
        match cadmpeg_ir::eval::surface_point_solved(surface, uv.u, uv.v) {
            Ok(point) => Some(point.get()),
            Err(failure) => failure.non_finite(),
        }
    };
    let (Some(start), Some(end)) = (lift(range[0]), lift(range[1])) else {
        return false;
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
    let close = |left: Point3, right: Point3| distance(left, right) < TOLERANCE;
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
    let ordinary_midpoint = parameter_range[0] + (parameter_range[1] - parameter_range[0]) * 0.5;
    let midpoint = if ordinary_midpoint.is_finite() {
        ordinary_midpoint
    } else {
        cadmpeg_ir::math::interpolate(parameter_range[0], parameter_range[1], 0.5)?.get()
    };
    let parameters = [parameter_range[0], midpoint, parameter_range[1]];
    let resolved_lift = |parameter| {
        let uv = cadmpeg_ir::eval::pcurve_uv(resolved_pcurve, parameter).ok()?;
        cadmpeg_ir::eval::surface_point(resolved_surface, uv.u, uv.v).ok()
    };
    let resolved_loci = [
        resolved_lift(parameters[0])?,
        resolved_lift(parameters[1])?,
        resolved_lift(parameters[2])?,
    ];
    let partner_uv = [
        cadmpeg_ir::eval::pcurve_uv(partner_pcurve, parameters[0]).ok()?,
        cadmpeg_ir::eval::pcurve_uv(partner_pcurve, parameters[1]).ok()?,
        cadmpeg_ir::eval::pcurve_uv(partner_pcurve, parameters[2]).ok()?,
    ];
    let mut matches = candidates.filter_map(|(identity, surface)| {
        resolved_loci
            .iter()
            .zip(&partner_uv)
            .all(|(resolved, uv)| {
                cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v).is_ok_and(|partner| {
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
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface_2)),
    ) = (left, right)
    else {
        return false;
    };
    let left_origin = cone_surface.origin();
    let left_axis = cone_surface.frame().axis().as_raw();
    let left_reference = cone_surface.frame().reference().as_raw();
    let left_radius = cone_surface.radius().get();
    let left_ratio = cone_surface.ratio().get();
    let left_angle = cone_surface.half_angle().get();
    let right_origin = cone_surface_2.origin();
    let right_axis = cone_surface_2.frame().axis().as_raw();
    let right_reference = cone_surface_2.frame().reference().as_raw();
    let right_radius = cone_surface_2.radius().get();
    let right_ratio = cone_surface_2.ratio().get();
    let right_angle = cone_surface_2.half_angle().get();
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
    let left_apex = apex(*left_origin, *left_axis, left_radius);
    let right_apex = apex(*right_origin, *right_axis, right_radius);
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

#[derive(Debug)]
enum RechartFailure {
    NonFinite,
    Resource(cadmpeg_core::CodecError),
}

fn rechart_equivalent_surface_pcurve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &PcurveGeometry,
    source: &SurfaceGeometry,
    target: &SurfaceGeometry,
) -> Result<Option<PcurveGeometry>, RechartFailure> {
    if source == target {
        return crate::resource::copy_pcurve_geometry(
            ctx,
            pcurve,
            "catia_freeform_equivalent_pcurve_copy",
        )
        .map(Some)
        .map_err(RechartFailure::Resource);
    }
    let (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface_2)),
    ) = (source, target)
    else {
        return Ok(None);
    };
    let source_origin = cone_surface.origin();
    let source_axis = cone_surface.frame().axis().as_raw();
    let target_origin = cone_surface_2.origin();
    if !same_surface_locus(source, target) {
        return Ok(None);
    }
    let v_shift = (source_origin.x - target_origin.x) * source_axis.x
        + (source_origin.y - target_origin.y) * source_axis.y
        + (source_origin.z - target_origin.z) * source_axis.z;
    if !v_shift.is_finite() {
        return Err(RechartFailure::NonFinite);
    }
    match pcurve {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin().as_raw();
            let shifted_origin =
                cadmpeg_ir::units::FinitePoint2::new(Point2::new(origin.u, origin.v + v_shift))
                    .ok_or(RechartFailure::NonFinite)?;
            Ok(Some(PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::new(
                    shifted_origin,
                    *line_pcurve.direction(),
                ),
            )))
        }
        PcurveGeometry::Nurbs { nurbs } => {
            use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;
            let knots = crate::resource::copy_knot_vector(
                ctx,
                nurbs.knots(),
                "catia_freeform_rechart_knots",
            )
            .map_err(RechartFailure::Resource)?;
            let mut poles = match nurbs.pole_rows() {
                PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
                    points: crate::resource::copy_retained_slice(
                        ctx,
                        points,
                        "catia_freeform_rechart_poles",
                    )
                    .map_err(RechartFailure::Resource)?,
                },
                PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
                    points: crate::resource::copy_retained_slice(
                        ctx,
                        points,
                        "catia_freeform_rechart_poles",
                    )
                    .map_err(RechartFailure::Resource)?,
                },
            };
            match &mut poles {
                PcurveNurbsPoles::Polynomial { points } => {
                    for point in points {
                        let raw = point.get();
                        *point = cadmpeg_ir::units::FinitePoint2::new(Point2::new(
                            raw.u,
                            raw.v + v_shift,
                        ))
                        .ok_or(RechartFailure::NonFinite)?;
                    }
                }
                PcurveNurbsPoles::Rational { points } => {
                    for pole in points {
                        let raw = pole.point.get();
                        pole.point = cadmpeg_ir::units::FinitePoint2::new(Point2::new(
                            raw.u,
                            raw.v + v_shift,
                        ))
                        .ok_or(RechartFailure::NonFinite)?;
                    }
                }
            }
            let shifted = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_admitted_parts(
                nurbs.degree(),
                knots,
                poles,
                nurbs.periodic(),
            )
            .map_err(|_| RechartFailure::NonFinite)?;
            Ok(Some(PcurveGeometry::Nurbs { nurbs: shifted }))
        }
        _ => Ok(None),
    }
}

fn append_a8_rolling_ball_pools(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    data: &[u8],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    for jet in crate::families::a5a8::records::a8_freeform_curves(admission.context(), data)? {
        let Some(definition) =
            crate::families::a5a8::records::rolling_ball_jet_definition(admission.context(), &jet)?
        else {
            continue;
        };
        let surface_id = SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "a8-rolling-ball", "surf"),
            ir.model.surfaces.len(),
        );
        let procedural_id = ProceduralSurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "a8-rolling-ball", "construction"),
            ir.model.procedural_surfaces.len(),
        );
        annotate(
            admission.context(),
            annotations,
            &surface_id,
            "object_stream_a8_03_32_cache",
            jet.pos as u64,
            format_args!("object_id:{:08x}", jet.object_id),
            Exactness::Unknown)?;
        admission.reserve_entity(&mut ir.model.surfaces, "catia_family_emit_surfaces")?;
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: procedural_id.clone(),
                cache: None,
            },
            source_object: Some(cgm_source(admission.context(), "surface", jet.object_id)?),
        });

        annotate(
            admission.context(),
            annotations,
            &procedural_id,
            "object_stream_a8_03_32",
            jet.pos as u64,
            format_args!(
                "object_id:{:08x}:multiplicities:{:?}",
                jet.object_id,
                jet.multiplicities(admission.context())?
            ),
            Exactness::ByteExact)?;
        admission.reserve_entity(&mut ir.model.procedural_surfaces, "catia_family_emit_procedural_surfaces")?;
        ir.model
            .procedural_surfaces
            .push(ProceduralSurface::new(procedural_id, definition, None));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn freeform_spatial_circle_collection_refuses_before_growth() {
        let mut bytes = vec![0xb2, 0x03, 0x0f, 112, 0x05];
        let cosine = 0.696_706_709_347_165_3_f64;
        let sine = 0.717_356_090_899_522_8_f64;
        for value in [
            17.0,
            23.0,
            13.0,
            cosine,
            -sine,
            0.0,
            sine,
            cosine,
            -0.0,
            7.0,
            0.0,
            11.2,
            1.0,
            -16.391_148_575_128_55,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            crate::resource::collect_vec(
                ctx,
                crate::families::b2::records::b2_spatial_circles_from_records(&bytes, &records),
                "catia_freeform_spatial_circles",
            )
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_freeform_spatial_circles")
        );
    }

    use super::{
        append_consolidated_line_profiles, append_freeform_surface_pools,
        append_resolved_consolidated_surface_curves, attach_standalone_wires,
        consolidated_line_profiles, freeform_surface_carriers, pcurve_lift_reaches_endpoints,
        rechart_equivalent_surface_pcurve, same_surface_locus,
        standard_carrier_surface_ids, typed_face_counts, unique_endpoint_pair_match,
        unique_paired_surface_lift_match, ConsolidatedCarrierChart, FreeformSurfacePool,
        RechartFailure,
    };
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        nurbs::NurbsCurve, pcurve::PcurveGeometry, Curve, CurveGeometry, IntcurveSupportContext,
        IntcurveSupportSide, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
        SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ProceduralCurveId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};
    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::HashMap;

    fn solve_planar_chart_rechart(
        sites: &[[f64; 2]],
        loci: &[Point3],
        target: &SurfaceGeometry,
    ) -> Option<ConsolidatedCarrierChart<'static>> {
        crate::test_support::with_service_context(|ctx| {
            super::solve_planar_chart_rechart(ctx, sites, loci, target)
        })
        .expect("service chart resource budget")
    }

    #[test]
    fn consolidated_jet_pcurve_workspace_refuses_materialized_limit() {
        use crate::wire::records::{ConsolidatedPcurve, ConsolidatedPcurveSite};
        let site = |knot, u| ConsolidatedPcurveSite {
            knot: cadmpeg_ir::scalar::FiniteReal::new(knot).expect("finite knot"),
            point: cadmpeg_ir::units::FiniteVector::new([u, 0.0]).expect("finite point"),
            first_derivatives: cadmpeg_ir::units::FiniteVector::new([1.0, 0.0])
                .expect("finite first jet"),
            second_derivatives: cadmpeg_ir::units::FiniteVector::new([0.0, 0.0])
                .expect("finite second jet"),
        };
        let pcurve = ConsolidatedPcurve {
            pos: 16,
            support_id: 1,
            extrapolation_sites: 0,
            sites: vec![site(0.0, 0.0), site(1.0, 1.0)],
            range: cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, 1.0])
                .expect("increasing range"),
            tail: Vec::new(),
        };
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            super::consolidated_jet_pcurve(ctx, &pcurve,
                &ConsolidatedCarrierChart::Identity, &mut crate::nurbs::LaneRefusals::new())
        };
        assert!(crate::test_support::with_service_context(run)
            .expect("service profile admits consolidated jet").is_some());
        assert!(matches!(crate::test_support::with_materialized_limit(0, run),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia consolidated pcurve points"));
    }

    fn with_admission<T>(run: impl FnOnce(&mut super::FamilyEntityAdmission<'_, '_>) -> T) -> T {
        crate::test_support::with_service_context(|ctx| {
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            run(&mut admission)
        })
    }

    #[test]
    fn freeform_surface_pool_entity_limit_refuses_before_curve_append() {
        let bytes = crate::test_support::test_a5a8::a5_freeform_curve_stream();
        crate::test_support::with_entity_limit(0, |ctx| {
            let mut ir = CadIr::empty();
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            let Err(error) = append_freeform_surface_pools(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                &HashMap::new(),
                &mut crate::nurbs::LaneRefusals::new(),
                &mut admission,
            ) else {
                panic!("an A5 limiting curve exceeds the zero-entity allowance");
            };
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                        && limit.operation == "admit CATIA family model entity"
            ));
            assert_eq!(ir.model.entity_count(), 0);
        });
    }

    #[test]
    fn freeform_wire_entity_limit_refuses_before_endpoint_append() {
        let mut ir = CadIr::empty();
        let curve_id = CurveId::mint("catia:test:curve#wire").expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                    None,
                    false,
                )
                .expect("valid linear wire curve"),
            )),
            source_object: None,
        });
        crate::test_support::with_entity_limit(0, |ctx| {
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            let error = attach_standalone_wires(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &[(curve_id, [0.0, 1.0], 0)],
                &mut admission,
            )
            .expect_err("the first endpoint exceeds the zero-entity allowance");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                        && limit.operation == "admit CATIA family model entity"
            ));
            assert!(ir.model.points.is_empty());
        });
    }

    #[test]
    fn consolidated_revolution_entity_limit_refuses_before_directrix_append() {
        let bytes = crate::test_support::test_b2::b2_resolved_revolution_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let resolved = crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_resolved_revolutions_from_records(
                ctx, &bytes, &records,
            )
        })
        .expect("service decode");
        assert_eq!(resolved.len(), 1);
        crate::test_support::with_entity_limit(0, |ctx| {
            let mut ir = CadIr::empty();
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            let Err(error) = super::append_consolidated_revolutions(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &resolved,
                &mut admission,
            ) else {
                panic!("the directrix exceeds the zero-entity allowance");
            };
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                        && limit.operation == "admit CATIA family model entity"
            ));
            assert_eq!(ir.model.entity_count(), 0);
        });
    }

    #[test]
    fn typed_face_counts_partition_the_parsed_record_identities() {
        use crate::families::b5::graph::{parse, parse_from_records, B5Record};
        use crate::test_support::test_b5::{append_b5_record, b5_closed_triangle_stream};

        let mut bytes = b5_closed_triangle_stream();
        append_b5_record(
            &mut bytes,
            0x5f,
            902,
            &[0x82, 0x18, 100, 0, 0x18, 0xe7, 0x03, 0x03],
        );
        let graph = crate::test_support::with_service_context(|ctx| {
            parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
        })
        .expect("service resource budget")
        .expect("one resolved and one unresolved face");
        assert_eq!(graph.face_records.len(), 2);
        assert_eq!(graph.faces.len(), 1);
        assert!(graph
            .faces
            .iter()
            .all(|face| graph.face_records.contains_key(&face.object_id)));
        assert_eq!(
            crate::test_support::with_service_context(|ctx| typed_face_counts(ctx, &graph.face_records, &graph.faces)).expect("service face count"),
            [1, 1, 0, 1]
        );
        assert_eq!(crate::test_support::with_service_context(|ctx| typed_face_counts(ctx, &graph.face_records, &[])).expect("service face count"), [1, 1, 0, 2]);
        let limited = crate::test_support::with_collection_limit(0, |ctx| typed_face_counts(ctx, &graph.face_records, &graph.faces));
        assert!(matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_freeform_resolved_face_ids"));

        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x5f,
            object_id: 902,
            payload: vec![0x82, 0x18, 100, 0, 0x18, 0xe7, 0x03, 0x03],
        };
        assert!(
            crate::test_support::with_service_context(|ctx| parse_from_records(
                ctx,
                &[],
                std::slice::from_ref(&record),
                &[],
                false,
                &mut crate::nurbs::LaneRefusals::new()
            ))
            .expect("service resource budget")
            .is_some()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| parse_from_records(
                ctx,
                &[],
                &[record.clone(), record],
                &[],
                false,
                &mut crate::nurbs::LaneRefusals::new()
            ))
            .expect("service resource budget")
            .is_none()
        );
    }

    #[test]
    fn object_stream_selection_uses_the_unique_topology_root_run() {
        let topology = crate::test_support::test_b5::b5_closed_triangle_stream();
        let mut unrelated = vec![0xb5, 0x03, 0x5e, 0x01];
        unrelated.extend_from_slice(&99u32.to_le_bytes());
        unrelated.push(0x00);

        let selection = crate::test_support::with_service_context(|ctx| {
            crate::families::b5::graph::select_object_stream_population(
                ctx,
                &[unrelated, topology.clone()],
                None,
            )
        })
        .expect("service collection budget");
        assert_eq!(selection.run_count(), 2);
        assert!(selection.selected());
        assert_eq!(selection.source(), topology);
    }

    #[test]
    fn object_stream_selection_refuses_multiple_topology_root_runs() {
        let topology = crate::test_support::test_b5::b5_closed_triangle_stream();
        let selection = crate::test_support::with_service_context(|ctx| {
            crate::families::b5::graph::select_object_stream_population(
                ctx,
                &[topology.clone(), topology],
                None,
            )
        })
        .expect("service collection budget");

        assert_eq!(selection.run_count(), 2);
        assert!(!selection.selected());
        assert!(selection.source().is_empty());
    }

    #[test]
    fn object_stream_selection_stops_before_materializing_over_budget_records() {
        let topology = crate::test_support::test_b5::b5_closed_triangle_stream();
        let budget = cadmpeg_core::decode::WorkBudget::new(1);

        let selection = crate::test_support::with_service_context(|ctx| {
            crate::families::b5::graph::select_object_stream_population(
                ctx,
                &[topology],
                Some(&budget),
            )
        })
        .expect("service collection budget");

        assert_eq!(selection.run_count(), 1);
        assert!(!selection.selected());
        assert!(selection.source().is_empty());
        assert!(selection.records().is_empty());
        assert!(selection.exhausted());
        assert!(budget.exhausted());
    }

    #[test]
    fn standalone_clamped_curve_becomes_a_valid_wire_edge() {
        let mut ir = CadIr::empty();
        let curve_id = CurveId::mint("catia:test:curve#0".to_string()).expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(2.0, 3.0, 5.0), Point3::new(7.0, 11.0, 13.0)],
                    None,
                    false,
                )
                .expect("valid linear NURBS"),
            )),
            source_object: None,
        });
        assert!(with_admission(|admission| attach_standalone_wires(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[(curve_id, [0.0, 1.0], 17)],
            admission
        ))
        .expect("service limits admit freeform model records"));
        assert_eq!(
            ir.model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Wire
        );
        assert_eq!(
            ir.model.shells[0].wire_edges(),
            [ir.model.edges[0].id.clone()]
        );
        assert_eq!(
            ir.model.points[1].position().get(),
            Point3::new(2.0, 3.0, 5.0)
        );
        assert_eq!(
            ir.model.points[0].position().get(),
            Point3::new(7.0, 11.0, 13.0)
        );
        ir.finalize();
        let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new());
        assert!(validation.is_ok(), "{:?}", validation.findings);
    }

    #[test]
    fn rejected_later_wire_leaves_all_topology_arenas_unchanged() {
        let mut ir = CadIr::empty();
        let curve_id = CurveId::mint("catia:test:curve#0").expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                    None,
                    false,
                )
                .expect("valid linear NURBS"),
            )),
            source_object: None,
        });
        let before = ir.model.clone();
        assert!(!with_admission(|admission| attach_standalone_wires(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[(curve_id.clone(), [0.0, 1.0], 0), (curve_id, [1.0, 0.0], 1)],
            admission
        ))
        .expect("service limits admit freeform model records"));
        assert_eq!(ir.model, before);
    }

    #[test]
    fn rejected_later_wire_refuses_before_plan_collection_growth() {
        let mut ir = CadIr::empty();
        let curve_id = CurveId::mint("catia:test:curve#0").expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                    None,
                    false,
                ).expect("valid linear NURBS"),
            )),
            source_object: None,
        });
        let wires = [(curve_id.clone(), [0.0, 1.0], 0), (curve_id, [1.0, 0.0], 1)];
        let before = ir.model.clone();
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            attach_standalone_wires(&mut ir, &mut AnnotationBuilder::new(), &wires, &mut admission)
        });
        assert!(matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_freeform_wire_plans"));
        assert_eq!(ir.model, before);
    }

    #[test]
    fn standalone_wire_owner_id_refuses_before_retained_copy() {
        let mut ir = CadIr::empty();
        let curve_id = CurveId::mint("catia:test:curve#0").expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                    None,
                    false,
                ).expect("valid linear NURBS"),
            )),
            source_object: None,
        });
        let wires = [(curve_id.clone(), [0.0, 1.0], 0)];
        let limit = curve_id.as_str().len() as u64;
        let limited = crate::test_support::with_retained_limit(limit, |ctx| {
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            attach_standalone_wires(&mut ir, &mut AnnotationBuilder::new(), &wires, &mut admission)
        });
        assert!(matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_freeform_wire_body_id"));
        assert!(ir.model.bodies.is_empty());
    }

    #[test]
    fn consolidated_line_profile_retains_its_stored_wire_interval() {
        let mut ir = CadIr::empty();
        let bytes = crate::test_support::test_b2::b2_line_profile_stream();
        let profiles = crate::test_support::with_service_context(|ctx|
            consolidated_line_profiles(ctx, &bytes, &crate::wire::records::consolidated_records(&bytes)))
            .expect("service profile admits line profiles");
        let wires = profiles
            .iter()
            .map(|profile| (profile.curve.id.clone(), profile.range, profile.pos))
            .collect::<Vec<_>>();
        with_admission(|admission| {
            append_consolidated_line_profiles(
                &mut ir,
                &mut AnnotationBuilder::new(),
                profiles,
                admission,
            )
        })
        .expect("service limits admit freeform model records");
        assert_eq!(wires.len(), 1);
        assert!(with_admission(|admission| attach_standalone_wires(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &wires,
            admission
        ))
        .expect("service limits admit freeform model records"));
        assert_eq!(
            ir.model.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([-4.0, 9.0])
        );
        let expected_start = Point3::new(1.0, -0.4, -0.2);
        let expected_end = Point3::new(1.0, 7.4, 10.2);
        for (actual, expected) in [
            (ir.model.points[1].position().get(), expected_start),
            (ir.model.points[0].position().get(), expected_end),
        ] {
            assert!((actual.x - expected.x).abs() < 1.0e-12);
            assert!((actual.y - expected.y).abs() < 1.0e-12);
            assert!((actual.z - expected.z).abs() < 1.0e-12);
        }
        ir.finalize();
        let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new());
        assert!(validation.is_ok(), "{:?}", validation.findings);
    }

    #[test]
    fn consolidated_line_profile_refuses_collection_limit() {
        let bytes = crate::test_support::test_b2::b2_line_profile_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let refused = crate::test_support::with_collection_limit(0, |ctx| {
            consolidated_line_profiles(ctx, &bytes, &records)
        });
        assert!(matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_consolidated_line_profiles"));
        let service = crate::test_support::with_service_context(|ctx| {
            consolidated_line_profiles(ctx, &bytes, &records)
        }).expect("service profile admits line profile");
        assert_eq!(service.len(), 1);
    }

    #[test]
    fn freeform_surface_pool_refuses_carrier_collection_limit() {
        let bytes = crate::test_support::test_a5a8::a5_surface_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let mut ir = CadIr::empty();
            let mut annotations = AnnotationBuilder::new();
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            append_freeform_surface_pools(&mut ir, &mut annotations, &bytes, &records,
                &HashMap::new(), &mut crate::nurbs::LaneRefusals::new(), &mut admission)
                .map(|_| ir.model.surfaces.len())
        };
        let mut refused_at_carrier = false;
        for limit in 0..96 {
            let result = crate::test_support::with_collection_limit(limit, &run);
            if matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_freeform_surface_pool_carrier_ids") {
                refused_at_carrier = true;
                break;
            }
        }
        assert!(refused_at_carrier);
        assert_eq!(crate::test_support::with_service_context(run)
            .expect("service profile admits freeform surface pool"), 1);
    }

    #[test]
    fn a5_guide_scratch_points_refuse_materialized_limit() {
        let bytes = crate::test_support::test_a5a8::a5_guide_curve_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let mut ir = CadIr::empty();
            let mut annotations = AnnotationBuilder::new();
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            append_freeform_surface_pools(&mut ir, &mut annotations, &bytes, &records,
                &HashMap::new(), &mut crate::nurbs::LaneRefusals::new(), &mut admission)
                .map(|_| ir.model.curves.len())
        };
        let refused = crate::test_support::with_materialized_limit(0, &run);
        assert!(matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia A5 guide points"));
        assert_eq!(crate::test_support::with_service_context(run)
            .expect("service profile admits A5 guide curve"), 1);
    }

    #[test]
    fn the_surface_pool_route_appends_the_line_profiles_the_standalone_route_appends() {
        let bytes = crate::test_support::test_b2::b2_line_profile_stream();
        let records = crate::wire::records::consolidated_records(&bytes);

        let mut standalone = CadIr::empty();
        with_admission(|admission| {
            append_consolidated_line_profiles(
                &mut standalone,
                &mut AnnotationBuilder::new(),
                consolidated_line_profiles(admission.context(), &bytes, &records)?,
                admission,
            )
        })
        .expect("service limits admit freeform model records");

        let mut pooled = CadIr::empty();
        with_admission(|admission| {
            append_freeform_surface_pools(
                &mut pooled,
                &mut AnnotationBuilder::new(),
                &bytes,
                &records,
                &HashMap::new(),
                &mut crate::nurbs::LaneRefusals::new(),
                admission,
            )
        })
        .expect("valid source object identity");

        assert_eq!(standalone.model.curves.len(), 1);
        assert_eq!(pooled.model.curves.len(), standalone.model.curves.len());
        assert_eq!(
            pooled
                .model
                .curves
                .iter()
                .map(|curve| curve.id.clone())
                .collect::<Vec<_>>(),
            standalone
                .model
                .curves
                .iter()
                .map(|curve| curve.id.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rolling_ball_pool_retains_both_exact_limiting_curves() {
        let mut ir = CadIr::empty();
        let bytes = crate::test_support::test_a5a8::a5_freeform_curve_stream();
        with_admission(|admission| {
            append_freeform_surface_pools(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                &HashMap::new(),
                &mut crate::nurbs::LaneRefusals::new(),
                admission,
            )
        })
        .expect("valid source object identity");

        assert!(matches!(
            ir.model.curves.as_slice(),
            [Curve {
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(first)),
                ..
            }, Curve {
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(second)),
                ..
            }] if first.degree() == 5
                && second.degree() == 5
                && first.pole_rows().raw_points().first() == Some(&Point3::new(1.0, 0.0, 0.0))
                && second.pole_rows().raw_points().first() == Some(&Point3::new(0.0, 1.0, 0.0))
        ));
    }

    #[test]
    fn freeform_fallback_distinguishes_grouped_and_standalone_cylinders() {
        let mut bytes = crate::test_support::test_b2::b2_cylinder_stream();
        bytes.extend_from_slice(&crate::test_support::test_b2::b2_embedded_cylinder_stream());

        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = crate::test_support::with_service_context(|ctx| {
            freeform_surface_carriers(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        });
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
        let plane = |z| {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, z),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            ))
        };
        let pcurve = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
            )
            .expect("valid LinePcurve fixture"),
        );
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
    fn paired_surface_lifts_keep_a_finite_midpoint_in_a_wide_parameter_range() {
        let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid plane"),
        ));
        let pcurve = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
            )
            .expect("valid pcurve"),
        );
        assert_eq!(
            unique_paired_surface_lift_match(
                &pcurve,
                &plane,
                &pcurve,
                [-f64::MAX, f64::MAX],
                [(7, &plane)].into_iter(),
            ),
            Some(7)
        );
    }

    #[test]
    fn cone_locus_equality_accepts_only_the_same_apex_shift() {
        let cone = |origin, radius| {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    origin,
                    Vector3::new(-1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    radius,
                    1.0,
                    std::f64::consts::FRAC_PI_4,
                )
                .expect("valid ConeSurface fixture"),
            ))
        };
        let apex_form = cone(Point3::new(111.0, 0.0, 0.0), 0.0);
        let shifted = cone(Point3::new(107.5, 0.0, 0.0), 3.5);
        let other = cone(Point3::new(107.0, 0.0, 0.0), 3.5);
        assert!(same_surface_locus(&apex_form, &shifted));
        assert!(!same_surface_locus(&apex_form, &other));
    }

    #[test]
    fn equivalent_cone_pcurve_moves_to_the_target_axial_origin() {
        let cone = |origin, radius| {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    origin,
                    Vector3::new(-1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    radius,
                    1.0,
                    std::f64::consts::FRAC_PI_4,
                )
                .expect("valid ConeSurface fixture"),
            ))
        };
        let source = cone(Point3::new(107.5, 0.0, 0.0), 3.5);
        let target = cone(Point3::new(111.0, 0.0, 0.0), 0.0);
        let pcurve = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.25, 1.5),
                Point2::new(2.0, -0.5),
            )
            .expect("valid LinePcurve fixture"),
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                rechart_equivalent_surface_pcurve(ctx, &pcurve, &source, &target)
            })
            .expect("finite pcurve rechart"),
            Some(PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.25, 5.0),
                    Point2::new(2.0, -0.5)
                )
                .expect("valid LinePcurve fixture")
            ))
        );
    }

    #[test]
    fn equivalent_cone_rechart_reports_overflow_for_finite_nurbs_poles() {
        let cone = |origin, radius| {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    origin,
                    Vector3::new(-1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    radius,
                    1.0,
                    std::f64::consts::FRAC_PI_4,
                )
                .expect("valid ConeSurface fixture"),
            ))
        };
        let shift = f64::MAX * 0.5;
        let source = cone(
            Point3::new(-shift / std::f64::consts::FRAC_PI_4.tan(), 0.0, 0.0),
            shift,
        );
        let target = cone(Point3::new(0.0, 0.0, 0.0), 0.0);
        assert!(same_surface_locus(&source, &target));
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    Point2::new(0.0, f64::MAX * 0.75),
                    Point2::new(1.0, f64::MAX * 0.75),
                ],
                None,
                false,
            )
            .expect("finite NURBS fixture"),
        };
        assert!(matches!(
            crate::test_support::with_service_context(|ctx| {
                rechart_equivalent_surface_pcurve(ctx, &pcurve, &source, &target)
            }),
            Err(RechartFailure::NonFinite),
        ));
    }

    #[test]
    fn equivalent_surface_rechart_refuses_pcurve_copy_limit() {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
                None,
                false,
            )
            .expect("valid pcurve fixture"),
        };
        let refused = crate::test_support::with_collection_limit(5, |ctx| {
            rechart_equivalent_surface_pcurve(ctx, &pcurve, &surface, &surface)
        });
        assert!(matches!(
            refused,
            Err(RechartFailure::Resource(
                cadmpeg_core::CodecError::ResourceLimit(_)
            ))
        ));
        let copied = crate::test_support::with_service_context(|ctx| {
            rechart_equivalent_surface_pcurve(ctx, &pcurve, &surface, &surface)
        })
        .expect("service budget");
        assert_eq!(copied, Some(pcurve));
    }

    #[test]
    fn consolidated_surface_curve_reuses_one_matching_unresolved_edge() {
        let mut ir = CadIr::empty();
        let points = [
            Point3::new(1.0, 4.0, 3.0),
            Point3::new(2.0, 2.0 + 2.0 * 0.5f64.cos(), 3.0 + 2.0 * 0.5f64.sin()),
        ];
        let mut bytes = crate::test_support::test_b2::b2_cylinder_stream();
        for point in points {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [point.x, point.y, point.z] {
                bytes.extend_from_slice(&(value as f32).to_le_bytes());
            }
        }
        let mut edge_run =
            crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 139, 142);
        let second_pcurve = crate::test_support::test_a5a8::a5_pcurve_stream().len();
        for (offset, value) in [10.0f64, 11.0, 20.0, 21.0].into_iter().enumerate() {
            let start = second_pcurve + 33 + 8 * offset;
            edge_run[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&edge_run);
        let cylinder = crate::families::b2::records::b2_cylinders(&bytes)
            .into_iter()
            .next()
            .expect("one exact cylinder")
            .surface_geometry();

        for (index, position) in points.into_iter().enumerate() {
            ir.model.points.push(Point::new(
                PointId::mint(format!("catia:test:point#point%23{index}"))
                    .expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(position)
                    .expect("a finite position is a point"),
                None,
            ));
            ir.model.vertices.push(Vertex {
                id: VertexId::mint(format!("catia:test:vertex#vertex%23{index}"))
                    .expect("identity grammar"),
                point: PointId::mint(format!("catia:test:point#point%23{index}"))
                    .expect("identity grammar"),
                tolerance: None,
            });
        }
        let curve_id =
            CurveId::mint("catia:test:curve#standard-curve".to_string()).expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId::mint("catia:test:edge#standard-edge".to_string())
                .expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([0.0, 1.0]),
            )
            .expect("valid edge carrier"),
            start: VertexId::mint("catia:test:vertex#vertex%231".to_string())
                .expect("identity grammar"),
            end: VertexId::mint("catia:test:vertex#vertex%230".to_string())
                .expect("identity grammar"),
            tolerance: None,
        });
        let support_ids = [
            SurfaceId::mint("catia:test:surface#support%230".to_string())
                .expect("identity grammar"),
            SurfaceId::mint("catia:test:surface#support%231".to_string())
                .expect("identity grammar"),
        ];
        ir.model.surfaces.push(Surface {
            id: support_ids[0].clone(),
            geometry: cylinder,
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: support_ids[1].clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        for (side, support_id) in support_ids.iter().enumerate() {
            let face_id =
                FaceId::mint(format!("catia:test:face#face%23{side}")).expect("identity grammar");
            let loop_id =
                LoopId::mint(format!("catia:test:loop#loop%23{side}")).expect("identity grammar");
            let coedge_id = CoedgeId::mint(format!("catia:test:coedge#coedge%23{side}"))
                .expect("identity grammar");
            ir.model.faces.push(Face {
                id: face_id.clone(),
                shell: ShellId::mint("catia:test:shell#shell".to_string())
                    .expect("identity grammar"),
                surface: support_id.clone(),
                sense: Sense::Forward,
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
                name: None,
                color: None,
                tolerance: None,
            });
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(vec![coedge_id.clone()], Vec::new())
                        .expect("valid loop ring"),
                ),
            });
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id,
                edge: EdgeId::mint("catia:test:edge#standard-edge".to_string())
                    .expect("identity grammar"),
                radial_next: CoedgeId::mint(format!("catia:test:coedge#coedge%23{}", 1 - side))
                    .expect("identity grammar"),
                sense: if side == 0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
            });
        }
        let _attached = ir.model.add_procedural_curve(
            curve_id.clone(),
            ProceduralCurve::new(
                ProceduralCurveId::mint(
                    "catia:test:proceduralcurve#standard-intersection".to_string(),
                )
                .expect("identity grammar"),
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::try_new(
                        std::array::from_fn(|side| IntcurveSupportSide {
                            surface: Some(support_ids[side].clone()),
                            pcurve: None,
                        }),
                        [0.0, 1.0],
                        std::array::from_fn(|_| Vec::new()),
                    )
                    .expect("valid IntcurveSupportContext fixture"),
                    discontinuity_flag: false,
                    cache: None,
                },
            ),
        );

        let attached = with_admission(|admission| {
            append_resolved_consolidated_surface_curves(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                FreeformSurfacePool {
                    surfaces: &[],
                    surface_ids: &[],
                    surface_alias_tags: &HashMap::new(),
                },
                &mut crate::nurbs::LaneRefusals::new(),
                admission,
            )
        })
        .expect("valid source object identity");
        assert_eq!(attached.standard_edges, 1);
        assert_eq!(attached.partner_face_pcurve_pairs, 0);
        assert_eq!(ir.model.pcurves.len(), 0);
        assert_eq!(ir.model.coedges[0].pcurves.len(), 0);
        assert_eq!(ir.model.coedges[1].pcurves.len(), 0);
        assert_eq!(ir.model.curves.len(), 1);
        assert_eq!(ir.model.edges[0].curve(), Some(&curve_id));
        let ProceduralCurveDefinition::SurfaceCurve { family } =
            ir.model.procedural_curves[0].definition()
        else {
            panic!("one exact support remains a parametric surface curve");
        };
        let context = family.context();
        assert_eq!(
            context
                .sides()
                .iter()
                .filter(|side| side.pcurve.is_some())
                .count(),
            1
        );
        let start = cadmpeg_ir::eval::pcurve_uv(
            &context.sides()[0]
                .pcurve
                .as_ref()
                .expect("first pcurve")
                .geometry,
            context.parameter_range().endpoints()[0],
        )
        .expect("reversed pcurve start");
        assert_eq!([start.u, start.v], [0.5, 1.0]);
        assert_eq!(
            ir.model.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([0.0, 1.0])
        );
    }

    #[test]
    fn consolidated_pcurve_uses_unique_standard_carrier_tag() {
        let bytes = crate::test_support::test_a5_bound::a5_native_edge_run_stream_with_support(
            6, 139, 142, 0x1234,
        );
        let mut ir = CadIr::empty();
        let surface_id = SurfaceId::mint("catia:test:surface#standard-carrier".to_string())
            .expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: Some(crate::test_support::with_service_context(|ctx| crate::assemble::cgm_source(ctx, "carrier", 0x1234)).expect("service profile admits source object")),
        });

        let counts = with_admission(|admission| {
            append_resolved_consolidated_surface_curves(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                FreeformSurfacePool {
                    surfaces: &[],
                    surface_ids: &[],
                    surface_alias_tags: &HashMap::new(),
                },
                &mut crate::nurbs::LaneRefusals::new(),
                admission,
            )
        })
        .expect("valid source object identity");

        assert_eq!(counts.standard_edges, 0);
        let [procedural] = ir.model.procedural_curves.as_slice() else {
            panic!("one consolidated surface curve");
        };
        let context = match procedural.definition() {
            ProceduralCurveDefinition::Intersection { context, .. } => context,
            ProceduralCurveDefinition::SurfaceCurve { family } => family.context(),
            _ => panic!("consolidated surface-curve construction"),
        };
        assert!(context
            .sides()
            .iter()
            .all(|side| { side.surface.as_ref() == Some(&surface_id) && side.pcurve.is_some() }));
        let start = cadmpeg_ir::eval::pcurve_uv(
            &context.sides()[0]
                .pcurve
                .as_ref()
                .expect("standard pcurve")
                .geometry,
            0.0,
        )
        .expect("standard pcurve start");
        assert_eq!(start, Point2::new(0.0, 0.0));
    }

    #[test]
    fn consolidated_pcurve_uses_unique_canonical_surface_alias_tag() {
        let bytes = crate::test_support::test_a5_bound::a5_native_edge_run_stream_with_support(
            6, 139, 142, 0x5678,
        );
        let mut ir = CadIr::empty();
        let surface_id = SurfaceId::mint("catia:test:surface#standard-carrier".to_string())
            .expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: Some(crate::test_support::with_service_context(|ctx| crate::assemble::cgm_source(ctx, "carrier", 0x1234)).expect("service profile admits source object")),
        });

        let counts = with_admission(|admission| {
            append_resolved_consolidated_surface_curves(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                FreeformSurfacePool {
                    surfaces: &[],
                    surface_ids: &[],
                    surface_alias_tags: &HashMap::from([(0x5678, Some(0x1234))]),
                },
                &mut crate::nurbs::LaneRefusals::new(),
                admission,
            )
        })
        .expect("valid source object identity");

        assert_eq!(counts.standard_edges, 0);
        let [procedural] = ir.model.procedural_curves.as_slice() else {
            panic!("one consolidated surface curve");
        };
        let context = match procedural.definition() {
            ProceduralCurveDefinition::Intersection { context, .. } => context,
            ProceduralCurveDefinition::SurfaceCurve { family } => family.context(),
            _ => panic!("consolidated surface-curve construction"),
        };
        assert!(context
            .sides()
            .iter()
            .all(|side| { side.surface.as_ref() == Some(&surface_id) && side.pcurve.is_some() }));
    }

    #[test]
    fn standard_carrier_index_rejects_duplicate_or_unknown_geometry() {
        let mut ir = CadIr::empty();
        for (id, geometry) in [
            (
                "known-0",
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
            ),
            (
                "known-1",
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                )),
            ),
            (
                "unknown",
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            ),
        ] {
            ir.model.surfaces.push(Surface {
                id: SurfaceId::mint(format!("catia:test:surface#{id}")).expect("identity grammar"),
                geometry,
                source_object: Some(crate::test_support::with_service_context(|ctx| crate::assemble::cgm_source(ctx, "carrier", 0x1234)).expect("service profile admits source object")),
            });
        }

        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                standard_carrier_surface_ids(ctx, &ir).expect("service decode")
            })
            .get(&0x1234),
            Some(&None)
        );
    }

    #[test]
    fn standard_carrier_tag_index_refuses_collection_limit() {
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#tagged".to_owned()).expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: Some(crate::test_support::with_service_context(|ctx| crate::assemble::cgm_source(ctx, "carrier", 0x1234)).expect("service profile admits source object")),
        });
        let refused = crate::test_support::with_collection_limit(0, |ctx| {
            standard_carrier_surface_ids(ctx, &ir)
        });
        assert!(matches!(
            refused,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        let indexed =
            crate::test_support::with_service_context(|ctx| standard_carrier_surface_ids(ctx, &ir))
                .expect("service budget");
        assert_eq!(indexed.get(&0x1234), Some(&None));
    }

    #[test]
    fn consolidated_plane_support_transfers_both_surface_curve_sides() {
        let plane_stream = crate::test_support::test_b2::b2_plane_carrier_stream();
        let plane_end = crate::families::b2::records::b2_plane_carriers(&plane_stream)[0].end;
        let mut bytes = plane_stream[..plane_end].to_vec();
        let points = [Point3::new(10.0, 20.0, 0.0), Point3::new(11.0, 20.0, 1.0)];
        for point in points {
            bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [point.x, point.y, point.z] {
                bytes.extend_from_slice(&(value as f32).to_le_bytes());
            }
        }
        bytes.extend_from_slice(
            &crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 139, 142),
        );

        let mut ir = CadIr::empty();
        for (index, position) in points.into_iter().enumerate() {
            ir.model.points.push(Point::new(
                PointId::mint(format!("catia:test:point#point%23{index}"))
                    .expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(position)
                    .expect("a finite position is a point"),
                None,
            ));
            ir.model.vertices.push(Vertex {
                id: VertexId::mint(format!("catia:test:vertex#vertex%23{index}"))
                    .expect("identity grammar"),
                point: PointId::mint(format!("catia:test:point#point%23{index}"))
                    .expect("identity grammar"),
                tolerance: None,
            });
        }
        let curve_id = CurveId::mint("catia:test:curve#standard-plane-curve".to_string())
            .expect("identity grammar");
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: EdgeId::mint("catia:test:edge#standard-plane-edge".to_string())
                .expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id.clone())),
            start: VertexId::mint("catia:test:vertex#vertex%230".to_string())
                .expect("identity grammar"),
            end: VertexId::mint("catia:test:vertex#vertex%231".to_string())
                .expect("identity grammar"),
            tolerance: None,
        });
        let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(10.0, 20.0, 0.0),
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ));
        let support_ids = [
            SurfaceId::mint("catia:test:surface#standard-plane%230".to_string())
                .expect("identity grammar"),
            SurfaceId::mint("catia:test:surface#standard-plane%231".to_string())
                .expect("identity grammar"),
        ];
        for support_id in &support_ids {
            ir.model.surfaces.push(Surface {
                id: support_id.clone(),
                geometry: plane.clone(),
                source_object: None,
            });
        }
        let _attached = ir.model.add_procedural_curve(
            curve_id,
            ProceduralCurve::new(
                ProceduralCurveId::mint(
                    "catia:test:proceduralcurve#standard-plane-intersection".to_string(),
                )
                .expect("identity grammar"),
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::try_new(
                        std::array::from_fn(|side| IntcurveSupportSide {
                            surface: Some(support_ids[side].clone()),
                            pcurve: None,
                        }),
                        [0.0, 1.0],
                        std::array::from_fn(|_| Vec::new()),
                    )
                    .expect("valid IntcurveSupportContext fixture"),
                    discontinuity_flag: false,
                    cache: None,
                },
            ),
        );

        let attached = with_admission(|admission| {
            append_resolved_consolidated_surface_curves(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &bytes,
                &crate::wire::records::consolidated_records(&bytes),
                FreeformSurfacePool {
                    surfaces: &[],
                    surface_ids: &[],
                    surface_alias_tags: &HashMap::new(),
                },
                &mut crate::nurbs::LaneRefusals::new(),
                admission,
            )
        })
        .expect("valid source object identity");
        assert_eq!(attached.standard_edges, 1);
        assert_eq!(
            ir.model.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([0.0, 1.0])
        );
        let ProceduralCurveDefinition::Intersection { context, .. } =
            ir.model.procedural_curves[0].definition()
        else {
            panic!("plane support keeps an intersection construction");
        };
        assert!(context.sides().iter().all(|side| {
            side.surface
                .as_ref()
                .is_some_and(|id| id.as_str().starts_with("catia:consolidated:plane#"))
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
        let target = SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin, normal, u_axis)
                .expect("valid PlaneSurface fixture"),
        );
        // Sites in the target chart, deliberately not collinear so the
        // isometry between the charts is uniquely determined.
        let target_sites = [[0.0, 0.0], [3.0, 1.0], [5.0, -2.0], [8.0, 4.0]];
        let loci = target_sites
            .iter()
            .map(|[u, v]| {
                cadmpeg_ir::eval::surface_point_solved(&target, *u, *v)
                    .expect("plane evaluates")
                    .get()
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
        (SurfaceGeometry::Solved(target), stored, loci)
    }

    #[test]
    fn planar_rechart_refuses_before_absent_chart_candidate() {
        let (target, stored, loci) = foreign_plane_chart_sites(0.4, [1.0, 2.0]);
        let scaled = stored
            .iter()
            .map(|[u, v]| [*u * 1.5, *v])
            .collect::<Vec<_>>();
        assert!(solve_planar_chart_rechart(&scaled, &loci, &target).is_none());
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            super::solve_planar_chart_rechart(ctx, &scaled, &loci, &target)
        });
        assert!(matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_freeform_chart_images"));
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
                    < 1.0e-9,
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
            (derivative[0].hypot(derivative[1]) - 1.0).abs() < 1.0e-12,
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
            &SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })
        )
        .is_none());
        // Two sites leave both orientation choices valid. Three collinear
        // sites have the same ambiguity, so neither admits a unique chart.
        assert!(solve_planar_chart_rechart(&stored[..2], &loci[..2], &target).is_none());
        let collinear_sites = [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]];
        let collinear_loci = collinear_sites
            .iter()
            .map(|[u, v]| {
                cadmpeg_ir::eval::surface_point(&target, *u, *v)
                    .expect("plane")
                    .get()
            })
            .collect::<Vec<_>>();
        assert!(solve_planar_chart_rechart(&collinear_sites, &collinear_loci, &target).is_none());
    }

    #[test]
    fn endpoint_lift_witness_refuses_a_pcurve_from_a_foreign_chart() {
        let (target, stored, loci) = foreign_plane_chart_sites(0.9, [-6.0, 3.5]);
        let endpoints = [*loci.first().expect("sites"), *loci.last().expect("sites")];
        let range = [0.0, 1.0];
        let line_through = |first: [f64; 2], last: [f64; 2]| PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                1,
                vec![range[0], range[0], range[1], range[1]],
                vec![
                    Point2::new(first[0], first[1]),
                    Point2::new(last[0], last[1]),
                ],
                None,
                false,
            )
            .expect("valid endpoint witness pcurve"),
        };
        let chart = solve_planar_chart_rechart(&stored, &loci, &target).expect("isometry");
        let first = *stored.first().expect("sites");
        let last = *stored.last().expect("sites");
        let recharted = line_through(chart.point(first), chart.point(last));
        assert!(
            pcurve_lift_reaches_endpoints(
                &recharted,
                target.solved().expect("solved carrier"),
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
                target.solved().expect("solved carrier"),
                range,
                endpoints,
                cadmpeg_ir::units::COINCIDENCE_TOLERANCE
            ),
            "a pcurve stored in a foreign chart has no witness on this carrier"
        );
        // The witness is independent of endpoint order.
        assert!(pcurve_lift_reaches_endpoints(
            &recharted,
            target.solved().expect("solved carrier"),
            range,
            [endpoints[1], endpoints[0]],
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        ));
        // A carrier with no geometry has no chart and admits no witness.
        assert!(!pcurve_lift_reaches_endpoints(
            &naive,
            &SolvedSurfaceGeometry::Unknown { record: None },
            range,
            endpoints,
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        ));
    }

    #[test]
    fn freeform_fallback_retains_exact_consolidated_spheres() {
        let bytes = crate::test_support::test_b2::b2_sphere_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = crate::test_support::with_service_context(|ctx| {
            freeform_surface_carriers(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        });
        assert!(matches!(carriers.as_slice(), [carrier]
                if matches!(carrier.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface))
                if {
                    let center = sphere_surface.center().get();
        let axis = sphere_surface.frame().axis().as_raw();
        let ref_direction = sphere_surface.frame().reference().as_raw();
                    (sphere_surface.radius().get() == 5.0)
                        && (center == Point3::new(1.0, 2.0, 3.0)
                            && *axis == Vector3::new(0.0, 0.0, 1.0)
                            && *ref_direction == Vector3::new(1.0, 0.0, 0.0))
                })));
    }

    #[test]
    fn freeform_surface_carrier_refuses_collection_limit() {
        let bytes = crate::test_support::test_b2::b2_sphere_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let refused = crate::test_support::with_collection_limit(0, |ctx| {
            freeform_surface_carriers(ctx, &bytes, &records, &mut crate::nurbs::LaneRefusals::new())
        });
        assert!(matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_freeform_surface_carriers"));
        let service = crate::test_support::with_service_context(|ctx| {
            freeform_surface_carriers(ctx, &bytes, &records, &mut crate::nurbs::LaneRefusals::new())
        }).expect("service profile admits sphere carrier");
        assert_eq!(service.len(), 1);
    }

    #[test]
    fn freeform_fallback_retains_exact_consolidated_tori() {
        let bytes = crate::test_support::test_b2::b2_torus_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = crate::test_support::with_service_context(|ctx| {
            freeform_surface_carriers(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        });
        assert!(matches!(carriers.as_slice(), [carrier]
                if matches!(carrier.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface))
                if {
                    let center = torus_surface.center().get();
        let axis = torus_surface.frame().axis().as_raw();
        let ref_direction = torus_surface.frame().reference().as_raw();
                    (torus_surface.major_radius().get() == 7.0)
                        && (torus_surface.minor_radius().get() == 2.0)
                        && (center == Point3::new(1.0, 2.0, 3.0)
                            && *axis == Vector3::new(0.0, 0.0, 1.0)
                            && *ref_direction == Vector3::new(1.0, 0.0, 0.0))
                })));
    }

    /// A cone record that is read builds its carrier. The frame witness has a
    /// componentwise cross-product deviation of `1e-9` and `|t1·axis| ≈ 1.41e-9`;
    /// the overflow witness has a finite apex whose carrier origin, the axis
    /// point at the slant start, is not finite. The record read refuses both, so
    /// the freeform carriers are built without them.
    #[test]
    fn a_cone_record_is_refused_when_read_or_builds_its_freeform_carrier() {
        let set = |stream: &mut Vec<u8>, index: usize, values: &[f64]| {
            for (offset, value) in values.iter().enumerate() {
                let start = 5 + 8 * (index + offset);
                stream[start..start + 8].copy_from_slice(&value.to_le_bytes());
            }
        };
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let mut frame_witness = crate::test_support::test_b2::b2_cone_stream();
        set(
            &mut frame_witness,
            3,
            &[s, s, 0.0, -s, s, 0.0, 1.0e-9, 1.0e-9, 1.0],
        );
        let mut overflow_witness = crate::test_support::test_b2::b2_cone_stream();
        set(&mut overflow_witness, 0, &[f64::MAX, 0.0, 0.0]);
        set(
            &mut overflow_witness,
            3,
            &[0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
        );
        set(&mut overflow_witness, 16, &[1.0e308, 1.5e308]);
        for bytes in [frame_witness, overflow_witness] {
            let records = crate::wire::records::consolidated_records(&bytes);
            let carriers = crate::test_support::with_service_context(|ctx| {
                freeform_surface_carriers(
                    ctx,
                    &bytes,
                    &records,
                    &mut crate::nurbs::LaneRefusals::new(),
                )
                .expect("service decode")
            });
            assert!(carriers.is_empty());
            assert!(
                crate::families::b2::records::b2_cones_from_records(&bytes, &records)
                    .next()
                    .is_none()
            );
        }
    }

    #[test]
    fn a_revolution_frame_admitted_when_read_converts_to_a_torus_without_a_second_axis_test() {
        // A right-handed frame whose profile-circle normal deviates from the
        // axis cross product by 0.9e-12 in each component. The record's
        // frame admission holds it, and the normal meets the axis at about
        // 1.56e-12, above 1e-12.
        let (a, b) = (std::f64::consts::FRAC_1_SQRT_2, 1.0 / 3.0_f64.sqrt());
        let deviation = 0.9e-12;
        let axis = [b, b, b];
        let direction_y = [a, -a, 0.0];
        let direction_x = [
            -a * b + deviation,
            -a * b + deviation,
            2.0 * a * b + deviation,
        ];
        let normal_meets_axis =
            direction_x[0] * axis[0] + direction_x[1] * axis[1] + direction_x[2] * axis[2];
        assert!(normal_meets_axis > 1.0e-12 && normal_meets_axis < 2.0e-12);

        let mut bytes = crate::test_support::test_b2::b2_resolved_revolution_stream();
        let frame_start = bytes.len() - 0xae + 3;
        let frame = [[0.0; 3], direction_x, direction_y, axis].concat();
        for (index, value) in frame.into_iter().enumerate() {
            let at = frame_start + 8 * index;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        let records = crate::wire::records::consolidated_records(&bytes);
        let resolved = crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_resolved_revolutions_from_records(
                ctx, &bytes, &records,
            )
        })
        .expect("service decode");
        assert_eq!(resolved.len(), 1);

        let bindings = with_admission(|admission| {
            super::append_consolidated_revolutions(
                &mut CadIr::empty(),
                &mut AnnotationBuilder::default(),
                &resolved,
                admission,
            )
        })
        .expect("service limits admit freeform model records");
        let [binding] = bindings.as_slice() else {
            panic!("the admitted revolution converts to one torus");
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus)) = &binding.geometry else {
            panic!("the conversion is a torus");
        };
        assert!((torus.major_radius().get() - 4.0).abs() <= 1.0e-9);
        assert_eq!(torus.minor_radius().get(), 3.0);
    }

    #[test]
    fn a_torus_reference_tilted_off_the_axis_by_rounding_keeps_the_perpendicularity_refusal() {
        // The record admits an axis whose squared length is 1 + 9.8e-13, a
        // frame whose second direction crosses the axis to the first within
        // 4.9e-13, and an axis origin 1e5 along the axis. The torus
        // reference is the radial part of the profile-center offset over
        // its length: the axial part leaves 2·4.9e-13·1e5 ≈ 9.8e-8 of radial
        // length along the axis, so the reference meets the axis at about
        // 2.45e-8, above the 1e-9 of OrthonormalFrame3::from_units. Every
        // other condition of the conversion holds. With an exact unit axis
        // the same revolution converts.
        let deviation = 4.9e-13;
        let convert = |axis_z: f64| {
            let mut bytes = crate::test_support::test_b2::b2_resolved_revolution_stream();
            let frame_start = bytes.len() - 0xae + 3;
            let frame = [
                [0.0, 0.0, 1.0e5],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, axis_z],
            ]
            .concat();
            for (index, value) in frame.into_iter().enumerate() {
                let at = frame_start + 8 * index;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
            let records = crate::wire::records::consolidated_records(&bytes);
            let resolved = crate::test_support::with_service_context(|ctx| {
                crate::families::b2::records::b2_resolved_revolutions_from_records(
                    ctx, &bytes, &records,
                )
            })
            .expect("service decode");
            assert_eq!(resolved.len(), 1);
            with_admission(|admission| {
                super::append_consolidated_revolutions(
                    &mut CadIr::empty(),
                    &mut AnnotationBuilder::default(),
                    &resolved,
                    admission,
                )
            })
            .expect("service limits admit freeform model records")
        };
        assert!(convert(1.0 + deviation).is_empty());
        let bindings = convert(1.0);
        let [binding] = bindings.as_slice() else {
            panic!("the exact unit axis converts to one torus");
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus)) = &binding.geometry else {
            panic!("the conversion is a torus");
        };
        assert_eq!(torus.major_radius().get(), 4.0);
    }

    #[test]
    fn freeform_fallback_retains_range_origin_cylinder_carriers() {
        let bytes = crate::test_support::test_b2::b2_range_origin_cylinder_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = crate::test_support::with_service_context(|ctx| {
            freeform_surface_carriers(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        });
        assert!(matches!(carriers.as_slice(), [carrier]
                if matches!(carrier.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
                if {
                    let origin = cylinder_surface.origin().get();
        let axis = cylinder_surface.frame().axis().as_raw();
        let ref_direction = cylinder_surface.frame().reference().as_raw();
                    (cylinder_surface.radius().get() == 4.0)
                        && (origin == Point3::new(0.0, 0.0, 0.0)
                            && *axis == Vector3::new(0.0, 1.0, 0.0)
                            && *ref_direction == Vector3::new(0.0, 0.0, 1.0))
                })));
    }

    #[test]
    fn large_planar_sites_recover_the_identity_chart() {
        use cadmpeg_ir::{
            geometry::{analytic::PlaneSurface, SolvedSurfaceGeometry, SurfaceGeometry},
            math::{Point3, Vector3},
        };
        let target = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
            )
            .expect("orthonormal target plane"),
        ));
        for (a, origin) in [(1., 0.), (1e200, 0.), (1., 1e10)] {
            let sites = [
                [origin + a, origin],
                [origin, origin + a],
                [origin - a, origin],
                [origin, origin - a],
            ];
            let loci = sites.map(|p| Point3::new(p[0], p[1], 0.));
            let chart = solve_planar_chart_rechart(&sites, &loci, &target)
                .expect("planar chart for the four sites");
            for point in sites {
                assert_eq!(chart.point(point), point);
            }
        }
    }

    /// A unit-radius cone about +Z whose cross-section radius overflows at
    /// v = 1e308, and the pcurve from its overflowing section at t = 0 to
    /// its unit circle at t = 1.
    fn overflowing_cone_lift() -> (SurfaceGeometry, PcurveGeometry) {
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    1.0,
                    1.0,
                    1.5,
                )
                .expect("valid ConeSurface fixture"),
            )),
            PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 1.0e308),
                    Point2::new(0.0, -1.0e308),
                )
                .expect("valid LinePcurve fixture"),
            ),
        )
    }

    #[test]
    fn standard_carrier_endpoint_loci_keep_an_overflowing_lift() {
        let (cone, pcurve) = overflowing_cone_lift();
        let loci = super::standard_carrier_endpoint_loci(&pcurve, &cone, [0.0, 1.0])
            .expect("both ends lift");
        assert!(!loci[0].is_finite());
        assert_eq!(loci[1], Point3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn a_pcurve_lift_with_an_overflowing_end_is_measured_at_its_finite_end() {
        let (cone, pcurve) = overflowing_cone_lift();
        assert!(pcurve_lift_reaches_endpoints(
            &pcurve,
            cone.solved().expect("solved carrier"),
            [0.0, 1.0],
            [Point3::new(5.0, 5.0, 5.0), Point3::new(1.0, 0.0, 0.0)],
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE,
        ));
    }

    /// The overflowing cone lift with the cone under the identity placement.
    fn placed_overflowing_cone_lift() -> (SurfaceGeometry, PcurveGeometry) {
        let (cone, pcurve) = overflowing_cone_lift();
        let SurfaceGeometry::Solved(cone) = cone else {
            panic!("the cone fixture is solved");
        };
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
                cadmpeg_ir::geometry::PlacedSurface::try_new(
                    Box::new(cone),
                    cadmpeg_ir::transform::Transform::identity(),
                )
                .expect("valid PlacedSurface fixture"),
            )),
            pcurve,
        )
    }

    #[test]
    fn standard_carrier_endpoint_loci_keep_an_overflowing_placed_lift() {
        let (cone, pcurve) = placed_overflowing_cone_lift();
        let loci = super::standard_carrier_endpoint_loci(&pcurve, &cone, [0.0, 1.0])
            .expect("both ends lift");
        assert!(!loci[0].is_finite());
        assert_eq!(loci[1], Point3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn a_pcurve_lift_with_an_overflowing_placed_end_is_measured_at_its_finite_end() {
        let (cone, pcurve) = placed_overflowing_cone_lift();
        assert!(pcurve_lift_reaches_endpoints(
            &pcurve,
            cone.solved().expect("solved carrier"),
            [0.0, 1.0],
            [Point3::new(5.0, 5.0, 5.0), Point3::new(1.0, 0.0, 0.0)],
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE,
        ));
    }
}
