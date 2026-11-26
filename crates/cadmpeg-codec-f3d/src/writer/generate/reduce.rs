// SPDX-License-Identifier: Apache-2.0
//! Content the source-less generator cannot encode (§3.4, "Content the target
//! cannot hold"). An item with an exact F3D form is converted to it; any other
//! is dropped or degraded with a loss naming it, and the rest of the document
//! is written.
//!
//! [`Reduction::prepare`] removes the families the generator never writes
//! before the first attempt. A face on a carrier F3D has no form for is
//! omitted there, and its body is written open without it; a body with no
//! writable face is left to the body fallback. An attempt that still fails
//! names the stage that failed, and [`Reduction::degrade`] takes the next
//! fallback for that stage:
//! the native Design records, then every native record (W1), then the bodies
//! that cannot be written alone, then every body. An empty design is the last
//! form; only a failure to write that is returned. A failure of the design
//! scope's records or of the ASM history of its timeline items drops the
//! records it names and each record that needs them, then the next part of
//! the design scope's form, then the design scope.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::edge_ranges::{open_edge_ranges, vertex_positions};
use cadmpeg_ir::codec::write::subset_lowering::UnloweredSubset;
use cadmpeg_ir::codec::write::{
    native_records_omitted, output_empty, partial_topology_omitted, record_invalid,
    MissingStateForm,
};
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::curve_point_solved;
use cadmpeg_ir::eval::EvaluationFailure;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::geometry::legacy_cache::FitTolerance;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{EdgeId, Identity, IdentityKeyTail, PointId, ShellId, VertexId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::topology::{
    BodyKind, Color, EdgeCarrier, EdgeEnds, FaceLoops, LoopBoundary, Point, Shell, ShellMember,
    Vertex,
};
use cadmpeg_ir::units::binding_allowance;
use serde_json::Value;

use crate::loss::F3dLossCode;

use super::design::{Culprit, RecordFailure};
use super::native_geometry::{
    native_procedural_curve, native_procedural_surface, ProceduralOwners,
};

/// Native arenas the B-rep stream is written from. Every other arena belongs
/// to the Design, ACT and configuration streams.
const BREP_ARENAS: &[&str] = &[
    "body_native_keys",
    "body_visibilities",
    "creation_timestamps",
    "edge_continuities",
    "edge_ownerships",
    "face_native_keys",
    "face_sidedness",
    "persistent_design_links",
    "persistent_subentity_tags",
    "sketch_curve_links",
    "tolerant_coedge_parameters",
    "tolerant_edge_tails",
    "tolerant_vertex_tails",
    "transform_hints",
    "vertex_ownerships",
    "wire_topologies",
];

/// Native arenas the generator never writes: it has no `MetaStream` record
/// registry for `ACT`, and a mesh-surface sentinel needs the retained ASM record.
const UNWRITTEN_ARENAS: &[&str] = &[
    "act_component_links",
    "act_entities",
    "act_guids",
    "act_registry_channels",
    "act_table_references",
    "mesh_surface_sentinels",
];

/// The stage of an attempt that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// The B-rep stream.
    Brep,
    /// The Design, configuration and appearance streams.
    Design,
    /// The records of the root definition's design scope, with the ASM
    /// history of its timeline items.
    NeutralDesign,
    /// The archive itself: nothing in the document can avoid it.
    Archive,
}

/// A failed attempt and the stage it failed in.
#[derive(Debug)]
pub(super) struct Failure {
    pub(super) stage: Stage,
    pub(super) error: CodecError,
    /// The records the failure names, where it names any.
    pub(super) culprit: Option<Culprit>,
}

impl Failure {
    pub(super) fn brep(error: CodecError) -> Self {
        Self {
            stage: Stage::Brep,
            error,
            culprit: None,
        }
    }

    pub(super) fn design(error: CodecError) -> Self {
        Self {
            stage: Stage::Design,
            error,
            culprit: None,
        }
    }

    pub(super) fn neutral_design(error: CodecError) -> Self {
        Self {
            stage: Stage::NeutralDesign,
            error,
            culprit: None,
        }
    }

    /// A failure of the ASM history, with the feature whose state it
    /// concerns where it names one.
    pub(super) fn history(error: CodecError, cause: Option<FeatureId>) -> Self {
        Self {
            stage: Stage::NeutralDesign,
            error,
            culprit: Some(Culprit::History(cause)),
        }
    }

    /// A failure to write the design scope's records.
    pub(super) fn records(failure: RecordFailure) -> Self {
        Self {
            stage: Stage::NeutralDesign,
            error: failure.error,
            culprit: failure.culprit,
        }
    }

    pub(super) fn archive(error: CodecError) -> Self {
        Self {
            stage: Stage::Archive,
            error,
            culprit: None,
        }
    }
}

/// The document the generator writes, and the losses taken to reach it.
#[derive(Clone)]
pub(super) struct Reduction<'a> {
    ir: Cow<'a, CadIr>,
    losses: Vec<LossNote>,
    design_dropped: bool,
    neutral_dropped: bool,
    /// The form of the features whose states the writer cannot write.
    missing_states: MissingStateForm,
    /// The parts of the design scope's form dropped after a failure.
    fallback: super::design::Fallback,
    /// The faces omitted from each opened body, charged at [`Self::finish`]
    /// for the bodies still written.
    opened: BTreeMap<String, Vec<String>>,
    /// The open edges written over one arc of a curve that passes a vertex
    /// twice, charged at [`Self::finish`] for the edges still written.
    assumed_arcs: Vec<(EdgeId, LossNote)>,
    /// The reduction before the first held state, which the writer takes
    /// again where it no longer writes the held items.
    unheld: Option<Box<Reduction<'a>>>,
}

impl<'a> Reduction<'a> {
    pub(super) fn new(ir: &'a CadIr, missing_states: MissingStateForm) -> Self {
        Self {
            missing_states,
            ir: Cow::Borrowed(ir),
            losses: Vec::new(),
            design_dropped: false,
            neutral_dropped: false,
            fallback: super::design::Fallback::default(),
            opened: BTreeMap::new(),
            assumed_arcs: Vec::new(),
            unheld: None,
        }
    }

    pub(super) fn ir(&self) -> &CadIr {
        &self.ir
    }

    /// Whether the native Design records are written: the document holds an
    /// `f3d` namespace and no fallback has dropped them.
    pub(super) fn writes_design(&self) -> bool {
        !self.design_dropped && self.ir.native.namespace("f3d").is_some()
    }

    /// Whether the root definition's design scope is written: no native
    /// Design record is, the document holds the scope, and no fallback has
    /// dropped it.
    pub(super) fn writes_neutral_design(&self) -> bool {
        !self.writes_design()
            && !self.neutral_dropped
            && super::design::root_scope(&self.ir).is_some()
    }

    /// Whether a fallback dropped the root definition's design scope.
    pub(super) fn drops_neutral_design(&self) -> bool {
        self.neutral_dropped
    }

    /// The parts of the design scope's form dropped after a failure with
    /// them.
    pub(super) fn fallback(&self) -> &super::design::Fallback {
        &self.fallback
    }

    /// The losses, with `export:output.empty` when no geometry is written.
    pub(super) fn finish(mut self) -> Vec<LossNote> {
        for (body, faces) in std::mem::take(&mut self.opened) {
            let Some(row) = self
                .ir
                .geometry
                .bodies
                .iter()
                .find(|row| row.id.as_str() == body)
            else {
                continue;
            };
            let kind = match row.kind {
                BodyKind::Solid => "solid",
                BodyKind::Sheet => "sheet",
                BodyKind::Wire => "wire",
                BodyKind::General => "general",
            };
            self.losses.push(F3dLossCode::WriterFaceOmitted.note(format!(
                "body {body} is written as a {kind} body without {} face(s) whose carriers F3D has no form for: {}",
                faces.len(),
                faces.join(", ")
            )));
        }
        let written = self
            .ir
            .geometry
            .edges
            .iter()
            .map(|edge| &edge.id)
            .collect::<HashSet<_>>();
        self.losses.extend(
            std::mem::take(&mut self.assumed_arcs)
                .into_iter()
                .filter(|(edge, _)| written.contains(edge))
                .map(|(_, note)| note),
        );
        let geometry = &self.ir.geometry;
        if geometry.faces.is_empty()
            && !geometry.shells.iter().any(|shell| {
                shell.wire_edges().next().is_some() || shell.free_vertices().next().is_some()
            })
        {
            self.losses.push(output_empty(
                "the F3D design is written with no body: the document holds no geometry the writer can encode",
            ));
        }
        let geometry = &self.ir.geometry;
        if geometry.bodies.is_empty()
            && geometry.regions.is_empty()
            && geometry.shells.is_empty()
            && geometry.faces.is_empty()
        {
            let unwritten = geometry_records_by_arena(geometry);
            if !unwritten.is_empty() {
                self.losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                    "the design holds no body, region, shell or face, so its B-rep stream is the ASM header alone and these geometry records were not written: {}",
                    unwritten.join("; ")
                )));
            }
        } else {
            self.losses.extend(unranged_open_edges(geometry));
        }
        self.losses
    }

    /// Remove every item the generator has no form for. Idempotent: it runs
    /// again after each fallback.
    pub(super) fn prepare(&mut self) -> Result<(), CodecError> {
        self.omit_partial_bodies()?;
        self.drop_unwritten_native();
        self.drop_rolled_back_native_design();
        if !self.writes_design() {
            self.drop_design_only_arenas();
        }
        self.drop_unwritten_arenas();
        self.strip_carrier_metadata();
        self.complete_vertex_free_edges()?;
        self.drop_pole_loops();
        self.write_point_edges_curveless();
        self.state_open_edge_ranges()?;
        self.write_attributes_in_their_native_form();
        self.reduce_procedural_carriers();
        self.open_shells_over_unwritable_carriers()?;
        self.drop_unwritable_pcurves();
        self.allocate_appearance_guids();
        self.drop_unwritable_appearances();
        self.separate_shared_appearance_guids();
        Ok(())
    }

    /// Take the next fallback for `failure`. Returns the failure's error when
    /// it is terminal or no fallback remains.
    pub(super) fn degrade(
        &mut self,
        failure: Failure,
        writes_alone: impl Fn(&CadIr, &crate::native::F3dNative, Stage) -> bool,
    ) -> Result<(), CodecError> {
        let Failure {
            stage,
            error,
            culprit,
        } = failure;
        if matches!(error, CodecError::ResourceLimit(_) | CodecError::Io(_)) {
            return Err(error);
        }
        let reason = error.to_string();
        let progressed = match stage {
            Stage::Archive => false,
            Stage::NeutralDesign => {
                let named = match culprit {
                    Some(culprit) => self.drop_records(&reason, &culprit)?,
                    None => false,
                };
                named || self.drop_design_part(&reason) || self.drop_neutral_design(&reason)
            }
            Stage::Brep => {
                self.drop_unwritable_bodies(&reason, Stage::Brep, &writes_alone)?
                    || self.drop_native_design(&reason)
                    || self.drop_native(&reason)
                    || self.drop_every_body(&reason)?
            }
            Stage::Design => {
                self.drop_native_design(&reason)
                    || self.drop_native(&reason)
                    || self.drop_unwritable_bodies(&reason, Stage::Design, &writes_alone)?
                    || self.drop_every_body(&reason)?
            }
        };
        if !progressed {
            return Err(error);
        }
        self.prepare()
    }

    fn drop_unwritten_native(&mut self) {
        let Some(namespace) = self.ir.native.namespace("f3d") else {
            return;
        };
        if !UNWRITTEN_ARENAS.iter().any(|arena| {
            namespace
                .arenas()
                .get(*arena)
                .is_some_and(|rows| !rows.is_empty())
        }) {
            return;
        }
        let dropped = self.remove_native_arenas(|arena| UNWRITTEN_ARENAS.contains(&arena));
        self.losses.extend(native_records_omitted(
            "F3D",
            "that the source-less generator has no form for (ACT records and mesh-surface sentinels need the retained MetaStream registry and ASM record)",
            dropped,
        ));
    }

    /// Neutral records the generator writes only from native Design records.
    /// A row that a parameter of the written design scope restates is written
    /// or charged with that parameter.
    fn drop_design_only_arenas(&mut self) {
        let model = &self.ir.model;
        if model.parameters.is_empty()
            && model.configurations.is_empty()
            && model.assembly_joints.is_empty()
        {
            return;
        }
        let restated = if self.writes_neutral_design() {
            super::design::restated_parameters(&self.ir)
                .into_iter()
                .map(str::to_owned)
                .collect()
        } else {
            BTreeSet::new()
        };
        let model = &mut self.ir.to_mut().model;
        let parameters = std::mem::take(&mut model.parameters);
        let configurations = std::mem::take(&mut model.configurations);
        let joints = std::mem::take(&mut model.assembly_joints);
        let reason = "the source-less F3D generator writes it only from native Design records";
        self.losses.extend(named_loss(
            F3dLossCode::WriterParameterOmitted,
            "parameter",
            reason,
            parameters
                .iter()
                .map(|row| row.id.to_string())
                .filter(|id| !restated.contains(id)),
        ));
        self.losses.extend(named_loss(
            F3dLossCode::WriterConfigurationOmitted,
            "configuration",
            reason,
            configurations.iter().map(|row| row.id.to_string()),
        ));
        self.losses.extend(named_loss(
            F3dLossCode::WriterJointOmitted,
            "assembly joint",
            reason,
            joints.iter().map(|row| row.id.to_string()),
        ));
    }

    fn drop_unwritten_arenas(&mut self) {
        let ir = &self.ir;
        if ir.geometry.subds.is_empty()
            && ir.model.assets.is_empty()
            && ir.model.tessellations.is_empty()
        {
            return;
        }
        let ir = self.ir.to_mut();
        // A cage's body is not written without its cage (§5.6).
        let cage_bodies = cadmpeg_ir::codec::write::remove_cage_bodies(ir);
        let subds = std::mem::take(&mut ir.geometry.subds);
        let assets = std::mem::take(&mut ir.model.assets);
        let tessellations = std::mem::take(&mut ir.model.tessellations);
        self.losses.extend(
            named_loss(
                F3dLossCode::WriterSubdOmitted,
                "subdivision surface",
                "the source-less F3D generator has no T-spline form",
                subds.iter().map(|row| row.id.to_string()),
            )
            .map(|mut loss| {
                loss.message
                    .push_str(&cadmpeg_ir::codec::write::cage_bodies_clause(&cage_bodies));
                loss
            }),
        );
        self.losses.extend(named_loss(
            F3dLossCode::WriterAssetOmitted,
            "document asset",
            "the source-less F3D generator writes no asset",
            assets.iter().map(|row| row.id.to_string()),
        ));
        self.losses.extend(named_loss(
            F3dLossCode::WriterTessellationOmitted,
            "tessellation",
            "the display mesh is not written: Fusion tessellates the B-rep it reads",
            tessellations.iter().map(|row| row.id.to_string()),
        ));
    }

    /// Source-object associations, face tolerances and coedge use curves the
    /// B-rep stream has no field for.
    fn strip_carrier_metadata(&mut self) {
        let geometry = &self.ir.geometry;
        let tolerant = if geometry
            .coedges
            .iter()
            .any(|coedge| coedge.use_curve.is_some())
        {
            self.tolerant_use_curve_coedges()
        } else {
            HashSet::new()
        };
        let associations = geometry
            .surfaces
            .iter()
            .filter(|row| row.source_object.is_some())
            .map(|row| row.id.to_string())
            .chain(
                geometry
                    .curves
                    .iter()
                    .filter(|row| row.source_object.is_some())
                    .map(|row| row.id.to_string()),
            )
            .collect::<Vec<_>>();
        let tolerances = geometry
            .faces
            .iter()
            .filter(|face| face.tolerance.is_some())
            .map(|face| face.id.to_string())
            .chain(
                geometry
                    .coedges
                    .iter()
                    .filter(|coedge| {
                        coedge.use_curve.is_some() && !tolerant.contains(coedge.id.as_str())
                    })
                    .map(|coedge| coedge.id.to_string()),
            )
            .collect::<BTreeSet<_>>();
        if associations.is_empty() && tolerances.is_empty() {
            return;
        }
        let geometry = &mut self.ir.to_mut().geometry;
        for surface in &mut geometry.surfaces {
            surface.source_object = None;
        }
        for curve in &mut geometry.curves {
            curve.source_object = None;
        }
        for face in &mut geometry.faces {
            face.tolerance = None;
        }
        for coedge in &mut geometry.coedges {
            if tolerances.contains(coedge.id.as_str()) {
                coedge.use_curve = None;
            }
        }
        self.losses.extend(named_loss(
            F3dLossCode::WriterSourceAssociationOmitted,
            "carrier source-object association",
            "the F3D B-rep stream has no field for it",
            associations,
        ));
        self.losses.extend(named_loss(
            F3dLossCode::WriterGeometryOmitted,
            "face tolerance or coedge use curve",
            "the source-less F3D generator writes neither without its native tolerant record; the target uses the model tolerance",
            tolerances,
        ));
    }

    fn tolerant_use_curve_coedges(&self) -> HashSet<String> {
        let Some(native) = self
            .ir
            .native
            .namespace("f3d")
            .and_then(|namespace| crate::native::F3dNative::load(namespace).ok())
        else {
            return HashSet::new();
        };
        native
            .tolerant_coedge_parameters
            .iter()
            .filter(|parameters| {
                matches!(
                    parameters.extension,
                    cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                        target: None,
                        ..
                    }
                )
            })
            .map(|parameters| parameters.coedge.as_str().to_owned())
            .collect()
    }

    /// An empty display name is written as no name. A translucent direct
    /// colour is written opaque, the form F3D's colour attribute holds, with a
    /// loss naming the entity.
    fn write_attributes_in_their_native_form(&mut self) {
        let geometry = &self.ir.geometry;
        let translucent = |color: &Option<Color>| color.is_some_and(|color| color.a() != 1.0);
        let empty = |name: &Option<String>| name.as_deref() == Some("");
        if !geometry
            .bodies
            .iter()
            .any(|body| translucent(&body.color) || empty(&body.name))
            && !geometry
                .faces
                .iter()
                .any(|face| translucent(&face.color) || empty(&face.name))
        {
            return;
        }
        let opaque = |color: &mut Option<Color>| -> bool {
            let Some(current) = *color else { return false };
            if current.a() == 1.0 {
                return false;
            }
            *color = Color::new(current.r(), current.g(), current.b(), 1.0);
            true
        };
        let mut made_opaque = Vec::new();
        let geometry = &mut self.ir.to_mut().geometry;
        for body in &mut geometry.bodies {
            if empty(&body.name) {
                body.name = None;
            }
            if opaque(&mut body.color) {
                made_opaque.push(body.id.to_string());
            }
        }
        for face in &mut geometry.faces {
            if empty(&face.name) {
                face.name = None;
            }
            if opaque(&mut face.color) {
                made_opaque.push(face.id.to_string());
            }
        }
        self.losses.extend(named_loss(
            F3dLossCode::WriterAppearanceOmitted,
            "direct colour transparency",
            "the F3D colour attribute holds opaque RGB; the colour is written opaque",
            made_opaque,
        ));
    }

    /// A closed edge on a point carrier with no stated range collapses to its
    /// vertex. ASM stores a collapsed edge with a null curve reference
    /// (asm.md §6.3, `degenerate_curve`), so such an edge whose vertex lies at
    /// the point within the coincidence allowance is written curveless.
    fn write_point_edges_curveless(&mut self) {
        let linear = self.ir.tolerances.linear;
        let geometry = &self.ir.geometry;
        let vertices = vertex_positions(geometry);
        let points = geometry
            .curves
            .iter()
            .filter_map(|curve| match curve.geometry.solved() {
                Some(SolvedCurveGeometry::Degenerate(point)) => {
                    Some((curve.id.as_str(), point.point().get()))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let collapsed = geometry
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                let (Some(curve), Some(ends), None) =
                    (edge.curve(), edge.ends.as_ref(), edge.param_range())
                else {
                    return false;
                };
                let (Some(point), Some((position, tolerance))) = (
                    points.get(curve.as_str()),
                    vertices.get(ends.start.as_str()),
                ) else {
                    return false;
                };
                ends.start == ends.end
                    && position.distance(*point)
                        <= binding_allowance(
                            linear,
                            &[
                                edge.tolerance.map(cadmpeg_ir::scalar::PositiveLength::get),
                                *tolerance,
                            ],
                        )
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if collapsed.is_empty() {
            return;
        }
        let geometry = &mut self.ir.to_mut().geometry;
        for index in collapsed {
            geometry.edges[index].carrier = EdgeCarrier::Free;
        }
    }

    /// An open edge that states no parameter range spans its vertices; the
    /// F3D edge record stores the range, so the writer states the vertices'
    /// parameters on the carrier ([`open_edge_ranges`], with the coincidence
    /// allowance of f3d.md §2: the edge's, the vertex's own and the cache-fit
    /// tolerances). A raised vertex tolerance is charged
    /// `geometry.vertex-tolerance-widened`, and an edge over one arc of a
    /// curve that passes a vertex twice `writer.edge-range-assumed` if it is
    /// still written at [`Self::finish`]. A line or conic edge ranged between
    /// its vertices' feet charges a loss naming the distance and, because the
    /// output's validation finds the vertex off its curve,
    /// `export:record.invalid` naming the edge (cadir-v11.md §3.4).
    fn state_open_edge_ranges(&mut self) -> Result<(), CodecError> {
        let owners = ProceduralOwners::new(&self.ir);
        let Some(plan) = open_edge_ranges(
            &self.ir,
            |curve| {
                owners
                    .curve(curve)
                    .last()
                    .and_then(|construction| construction.cache_fit_tolerance())
                    .map(FitTolerance::get)
            },
            |edge| self.names_edge_uses(edge),
        )?
        else {
            return Ok(());
        };
        drop(owners);
        let widened = plan
            .widened
            .iter()
            .map(|(vertex, tolerance)| {
                F3dLossCode::VertexToleranceWidened.note(format!(
                    "vertex {vertex}: tolerance raised to {:.6} mm, its distance from the end of an edge's curve",
                    tolerance.get()
                ))
            })
            .collect::<Vec<_>>();
        let feet = plan
            .off_curve
            .iter()
            .map(|(edge, off)| {
                F3dLossCode::WriterGeometryOmitted.note(format!(
                    "edge {edge}: {}; its range is stated from the vertices' feet on the curve",
                    off.iter()
                        .map(|(vertex, distance)| format!(
                            "vertex {vertex} lies {distance:.6} mm off its curve"
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
            .collect::<Vec<_>>();
        let arcs = plan
            .assumed_arcs
            .iter()
            .map(|(edge, [first, last])| {
                (
                    edge.clone(),
                    F3dLossCode::WriterEdgeRangeAssumed.note(format!(
                        "edge {edge}: its curve passes a vertex of the edge on two arcs; the \
                         edge runs from {first:.6} to {last:.6} on it, the shortest arc \
                         the writer can state that passes neither vertex between its ends"
                    )),
                )
            })
            .collect::<Vec<_>>();
        let invalid = plan
            .off_curve
            .iter()
            .map(|(edge, _)| edge.to_string())
            .collect::<Vec<_>>();
        plan.apply(self.ir.to_mut());
        self.losses.extend(widened);
        self.losses.extend(feet);
        self.assumed_arcs.extend(arcs);
        if !invalid.is_empty() {
            self.losses.push(record_invalid(
                EntityKind::Edge,
                format!(
                    "{} open edge(s) are written with a vertex off their curve, ranged between the vertices' feet: {}",
                    invalid.len(),
                    invalid.join(", ")
                ),
            ));
        }
        Ok(())
    }

    /// Whether a use curve or a native record states `edge`'s direction: a
    /// coedge of the edge carries a use curve, or an F3D record names the
    /// edge or one of its coedges.
    fn names_edge_uses(&self, edge: &cadmpeg_ir::topology::Edge) -> bool {
        let coedges = self
            .ir
            .geometry
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .collect::<Vec<_>>();
        if coedges.iter().any(|coedge| coedge.use_curve.is_some()) {
            return true;
        }
        let Some(namespace) = self.ir.native.namespace("f3d") else {
            return false;
        };
        let names = coedges
            .iter()
            .map(|coedge| coedge.id.to_string())
            .chain([edge.id.to_string()])
            .collect::<BTreeSet<_>>();
        namespace.arenas().values().flatten().any(|record| {
            record
                .fields()
                .is_ok_and(|fields| fields.values().any(|value| mentions(value, &names)))
        })
    }

    /// A body that does not state every region, shell, face, or loop is not
    /// written: F3D's body records hold no partial form (§3.4, Content the
    /// target cannot hold).
    fn omit_partial_bodies(&mut self) -> Result<(), CodecError> {
        let geometry = &self.ir.geometry;
        let lists = geometry.child_lists();
        let (partial, kept): (Vec<_>, Vec<_>) = geometry.bodies.iter().partition(|body| {
            lists.complete_body_regions(&body.id).is_none_or(|regions| {
                regions
                    .iter()
                    .any(|region| lists.complete_shells(region).is_none())
            })
        });
        if partial.is_empty() {
            return Ok(());
        }
        let losses = partial_topology_omitted("F3D", geometry, "the body is not written");
        let kept = kept
            .into_iter()
            .map(|body| body.id.to_string())
            .collect::<Vec<_>>();
        let kept = kept.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let unreachable = retain_bodies(self.ir.to_mut(), &kept)?;
        self.losses.extend(losses);
        self.charge_removed(
            &unreachable,
            &BTreeSet::new(),
            "their bodies do not state every region, shell, face, or loop",
        );
        Ok(())
    }

    /// A loop that is one vertex at a surface singularity has no ASM record:
    /// the face's surface states the pole. Each such loop that is not a face's
    /// classified outer loop is dropped, with its vertex where nothing else
    /// uses it, and a loss naming the loops.
    fn drop_pole_loops(&mut self) {
        let geometry = &self.ir.geometry;
        let outer = geometry
            .faces
            .iter()
            .filter_map(|face| match &face.loops {
                FaceLoops::Classified { outer, .. } => Some(outer.as_str()),
                FaceLoops::PartialClassified { outer, .. } => {
                    outer.as_ref().map(cadmpeg_ir::ids::LoopId::as_str)
                }
                FaceLoops::Unspecified { .. } | FaceLoops::PartialUnspecified { .. } => None,
            })
            .collect::<HashSet<_>>();
        let poles = geometry
            .loops
            .iter()
            .filter(|row| {
                matches!(row.boundary, LoopBoundary::Vertex { .. })
                    && !outer.contains(row.id.as_str())
            })
            .map(|row| row.id.clone())
            .collect::<HashSet<_>>();
        if poles.is_empty() {
            return;
        }
        let geometry = &mut self.ir.to_mut().geometry;
        for face in &mut geometry.faces {
            match &mut face.loops {
                FaceLoops::Unspecified { loops } | FaceLoops::PartialUnspecified { loops } => {
                    loops.retain(|id| !poles.contains(id));
                }
                FaceLoops::Classified { inner, .. }
                | FaceLoops::PartialClassified { inner, .. } => {
                    inner.retain(|id| !poles.contains(id));
                }
            }
        }
        geometry.loops.retain(|row| !poles.contains(&row.id));
        let used = geometry
            .edges
            .iter()
            .flat_map(|edge| edge.vertices().cloned().collect::<Vec<_>>())
            .chain(
                geometry
                    .shells
                    .iter()
                    .flat_map(|shell| shell.free_vertices().cloned().collect::<Vec<_>>()),
            )
            .chain(geometry.loops.iter().filter_map(|row| match &row.boundary {
                LoopBoundary::Vertex { vertex, .. } => Some(vertex.clone()),
                LoopBoundary::Ring(_) => None,
            }))
            .collect::<HashSet<_>>();
        geometry.vertices.retain(|vertex| used.contains(&vertex.id));
        let points = geometry
            .vertices
            .iter()
            .map(|vertex| vertex.point.clone())
            .collect::<HashSet<_>>();
        geometry.points.retain(|point| points.contains(&point.id));
        self.losses.extend(named_loss(
            F3dLossCode::WriterGeometryOmitted,
            "pole loop",
            "ASM states a surface singularity by its surface and stores no vertex loop for it",
            poles.iter().map(ToString::to_string),
        ));
    }

    /// A vertex-free edge (§5.6) is written in the exact form F3D stores, one
    /// vertex at its closed curve's start. An edge whose curve does not close
    /// there is omitted with a loss, with the loop it alone bounds.
    fn complete_vertex_free_edges(&mut self) -> Result<(), CodecError> {
        let geometry = &self.ir.geometry;
        if geometry.edges.iter().all(|edge| edge.ends.is_some()) {
            return Ok(());
        }
        let tolerance = self.ir.tolerances.linear.get();
        let curves = geometry
            .curves
            .iter()
            .map(|curve| (curve.id.as_str(), curve))
            .collect::<HashMap<_, _>>();
        let mut starts = Vec::new();
        let mut omitted = Vec::new();
        for (index, edge) in geometry.edges.iter().enumerate() {
            if edge.ends.is_some() {
                continue;
            }
            let start = closed_curve_start(edge, &curves, tolerance)?.and_then(|position| {
                let point = PointId::mint(format!("{}/start-point", edge.id))
                    .map_err(|error| error.to_string())?;
                let vertex = VertexId::mint(format!("{}/start-vertex", edge.id))
                    .map_err(|error| error.to_string())?;
                Ok((point, vertex, position))
            });
            match start {
                Ok(start) => starts.push((index, start)),
                Err(reason) => omitted.push((edge.id.clone(), reason)),
            }
        }
        let geometry = &mut self.ir.to_mut().geometry;
        for (index, (point, vertex, position)) in starts {
            geometry
                .points
                .push(Point::new(point.clone(), position, None));
            let edge = &mut geometry.edges[index];
            geometry.vertices.push(Vertex {
                id: vertex.clone(),
                point,
                tolerance: edge.tolerance,
                native_ref: None,
            });
            edge.ends = Some(EdgeEnds::new(vertex.clone(), vertex));
        }
        for (edge, reason) in omitted {
            if omit_vertex_free_edge(geometry, &edge) {
                self.losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                    "edge {edge} has no vertex and was omitted with the loop it alone bounds: {reason}"
                )));
            }
        }
        Ok(())
    }

    /// A subset curve that cannot be restated on the parameter of the curve
    /// it trims is written as the solved geometry its cache holds, and the
    /// construction is dropped.
    pub(super) fn reduce_unlowered_subsets(&mut self, unlowered: Vec<UnloweredSubset>) {
        if unlowered.is_empty() {
            return;
        }
        let geometry = &mut self.ir.to_mut().geometry;
        for subset in unlowered {
            let Some(curve) = geometry
                .curves
                .iter_mut()
                .find(|curve| curve.id == subset.curve)
            else {
                continue;
            };
            let Some(solved) = curve.geometry.solved_cache().cloned() else {
                continue;
            };
            curve.geometry = CurveGeometry::Solved(solved);
            geometry
                .procedural_curves
                .retain(|row| row.id != subset.construction);
            self.losses
                .push(F3dLossCode::WriterProceduralReduced.note(format!(
                "procedural construction {} was reduced to the solved geometry of carrier {}: {}",
                subset.construction, subset.curve, subset.error
            )));
        }
    }

    /// A procedural carrier whose construction has no native encoding is
    /// written as the solved geometry its cache holds, and the construction is
    /// dropped. Repeats until every remaining construction encodes, since a
    /// reduced support changes how its dependents encode.
    fn reduce_procedural_carriers(&mut self) {
        loop {
            let reductions = procedural_reductions(&self.ir);
            if reductions.is_empty() {
                return;
            }
            let geometry = &mut self.ir.to_mut().geometry;
            let mut surfaces = HashSet::new();
            let mut curves = HashSet::new();
            for reduction in &reductions {
                match &reduction.carrier {
                    Carrier::Surface(id) => {
                        surfaces.insert(id.clone());
                    }
                    Carrier::Curve(id) => {
                        curves.insert(id.clone());
                    }
                    Carrier::None => {}
                }
            }
            let constructions = reductions
                .iter()
                .map(|reduction| reduction.construction.as_str())
                .collect::<HashSet<_>>();
            for surface in &mut geometry.surfaces {
                if surfaces.contains(surface.id.as_str()) {
                    if let Some(solved) = surface.geometry.solved_cache().cloned() {
                        surface.geometry = SurfaceGeometry::Solved(solved);
                    }
                }
            }
            for curve in &mut geometry.curves {
                if curves.contains(curve.id.as_str()) {
                    if let Some(solved) = curve.geometry.solved_cache().cloned() {
                        curve.geometry = CurveGeometry::Solved(solved);
                    }
                }
            }
            geometry
                .procedural_surfaces
                .retain(|row| !constructions.contains(row.id.as_str()));
            geometry
                .procedural_curves
                .retain(|row| !constructions.contains(row.id.as_str()));
            for reduction in reductions {
                let message = match &reduction.carrier {
                    Carrier::None => format!(
                        "procedural construction {} was not written: {}",
                        reduction.construction, reduction.reason
                    ),
                    Carrier::Surface(id) | Carrier::Curve(id) => format!(
                        "procedural construction {} was reduced to the solved geometry of carrier {id}: {}",
                        reduction.construction, reduction.reason
                    ),
                };
                self.losses
                    .push(F3dLossCode::WriterProceduralReduced.note(message));
            }
        }
    }

    /// A face on a carrier F3D has no form for is omitted with its loops and
    /// coedges, and the radial rings close over them. The records only those
    /// faces used go, each touched shell is split into its connected parts,
    /// and the body takes the kind its incidence derives.
    fn open_shells_over_unwritable_carriers(&mut self) -> Result<(), CodecError> {
        let unwritable = unwritable_faces(&self.ir);
        if unwritable.is_empty() {
            return Ok(());
        }
        let faces = unwritable
            .values()
            .flatten()
            .map(|(face, _)| face.clone())
            .collect::<HashSet<_>>();
        let geometry = &self.ir.geometry;
        let loops = geometry
            .loops
            .iter()
            .filter(|row| faces.contains(row.face.as_str()))
            .map(|row| row.id.to_string())
            .chain(
                geometry
                    .faces
                    .iter()
                    .filter(|face| faces.contains(face.id.as_str()))
                    .flat_map(|face| face.loops.stated())
                    .map(ToString::to_string),
            )
            .collect::<HashSet<_>>();
        let coedges = geometry
            .coedges
            .iter()
            .filter(|coedge| loops.contains(coedge.owner_loop.as_str()))
            .map(|coedge| coedge.id.to_string())
            .collect::<HashSet<_>>();
        let radial = geometry
            .coedges
            .iter()
            .map(|coedge| (coedge.id.as_str(), &coedge.radial_next))
            .collect::<HashMap<_, _>>();
        let closed = geometry
            .coedges
            .iter()
            .enumerate()
            .filter(|(_, coedge)| {
                !coedges.contains(coedge.id.as_str())
                    && coedges.contains(coedge.radial_next.as_str())
            })
            .map(|(index, coedge)| {
                let mut next = &coedge.radial_next;
                for _ in 0..=coedges.len() {
                    if !coedges.contains(next.as_str()) {
                        break;
                    }
                    match radial.get(next.as_str()) {
                        Some(&following) => next = following,
                        None => break,
                    }
                }
                let next = if coedges.contains(next.as_str()) {
                    coedge.id.clone()
                } else {
                    next.clone()
                };
                (index, next)
            })
            .collect::<Vec<_>>();
        let touched = geometry
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .map(|face| face.shell.to_string())
            .collect::<BTreeSet<_>>();

        let geometry = &mut self.ir.to_mut().geometry;
        for (index, next) in closed {
            geometry.coedges[index].radial_next = next;
        }
        geometry
            .faces
            .retain(|face| !faces.contains(face.id.as_str()));
        geometry
            .loops
            .retain(|row| !loops.contains(row.id.as_str()));
        geometry
            .coedges
            .retain(|coedge| !coedges.contains(coedge.id.as_str()));
        let occupied = geometry
            .faces
            .iter()
            .map(|face| face.shell.to_string())
            .collect::<HashSet<_>>();
        let empty_shells = geometry
            .shells
            .iter()
            .filter(|shell| {
                touched.contains(shell.id.as_str())
                    && !occupied.contains(shell.id.as_str())
                    && shell.members.is_empty()
            })
            .map(|shell| shell.id.to_string())
            .collect::<HashSet<_>>();
        geometry
            .shells
            .retain(|shell| !empty_shells.contains(shell.id.as_str()));
        let mut empty_regions = HashSet::new();
        for region in &mut geometry.regions {
            let before = region.stated_shells().count();
            region.retain_stated_shells(|shell| !empty_shells.contains(shell.as_str()));
            if region.stated_shells().next().is_none() && before != 0 {
                empty_regions.insert(region.id.to_string());
            }
        }
        geometry
            .regions
            .retain(|region| !empty_regions.contains(region.id.as_str()));
        let omitted = faces
            .into_iter()
            .chain(loops)
            .chain(coedges)
            .chain(empty_shells)
            .chain(empty_regions)
            .collect::<BTreeSet<_>>();

        let bodies = self
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.to_string())
            .collect::<Vec<_>>();
        let kept = bodies.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let unreachable = retain_bodies(self.ir.to_mut(), &kept)?;
        split_disconnected_shells(&mut self.ir.to_mut().geometry, &touched);
        let kinds = super::preconditions::derived_body_kinds(&self.ir.geometry);
        for (body, kind) in self.ir.to_mut().geometry.bodies.iter_mut().zip(kinds) {
            if unwritable.contains_key(body.id.as_str()) {
                body.kind = kind;
            }
        }
        self.charge_removed(
            &unreachable,
            &omitted,
            "their faces are on carriers F3D has no form for",
        );
        for (body, faces) in unwritable {
            self.opened.entry(body).or_default().extend(
                faces
                    .into_iter()
                    .map(|(face, carrier)| format!("face {face} ({carrier})")),
            );
        }
        Ok(())
    }

    /// A pcurve with no native form is omitted with its coedge uses; Fusion
    /// recomputes a missing pcurve from the face and edge.
    fn drop_unwritable_pcurves(&mut self) {
        let unwritable = super::smbh::unwritable_pcurves(&self.ir.geometry);
        if unwritable.is_empty() {
            return;
        }
        let ids = unwritable
            .iter()
            .map(|(id, _)| id.as_str().to_owned())
            .collect::<HashSet<_>>();
        let geometry = &mut self.ir.to_mut().geometry;
        for coedge in &mut geometry.coedges {
            coedge
                .pcurves
                .retain(|use_| !ids.contains(use_.pcurve.as_str()));
        }
        geometry
            .pcurves
            .retain(|pcurve| !ids.contains(pcurve.id.as_str()));
        let mut by_reason = std::collections::BTreeMap::<String, BTreeSet<String>>::new();
        for (id, error) in unwritable {
            by_reason
                .entry(error.to_string())
                .or_default()
                .insert(id.to_string());
        }
        for (reason, ids) in by_reason {
            self.losses.push(F3dLossCode::WriterPcurveOmitted.note(format!(
                "{} pcurve(s) were omitted with their coedge uses, and Fusion recomputes them ({reason}): {}",
                ids.len(),
                ids.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
    }

    /// A Protein asset is keyed by a GUID. An appearance that states none (a
    /// source of another format) takes one the writer allocates from its id,
    /// the same for the same id on every write.
    fn allocate_appearance_guids(&mut self) {
        if self
            .ir
            .model
            .appearances
            .iter()
            .all(|appearance| appearance.visual_guid.is_some() || appearance.asset_guid.is_some())
        {
            return;
        }
        let mut allocated = Vec::new();
        for appearance in &mut self.ir.to_mut().model.appearances {
            if appearance.visual_guid.is_none() && appearance.asset_guid.is_none() {
                let guid = allocated_guid(appearance.id.as_str());
                appearance.asset_guid = Some(guid.clone());
                if crate::materials::encode_protein(appearance).is_ok() {
                    allocated.push(format!("{} as {guid}", appearance.id));
                }
            }
        }
        if !allocated.is_empty() {
            self.losses.push(F3dLossCode::WriterAppearanceGuidAllocated.note(format!(
                "{} appearance(s) state no asset GUID; each Protein asset is written under a GUID the writer allocates from the appearance id: {}",
                allocated.len(),
                allocated.join(", ")
            )));
        }
    }

    fn drop_unwritable_appearances(&mut self) {
        let unwritable = self
            .ir
            .model
            .appearances
            .iter()
            .filter_map(|appearance| {
                crate::materials::encode_protein(appearance)
                    .err()
                    .map(|error| (appearance.id.to_string(), error))
            })
            .collect::<Vec<_>>();
        if unwritable.is_empty() {
            return;
        }
        let ids = unwritable
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<HashSet<_>>();
        let appearances = std::mem::take(&mut self.ir.to_mut().model.appearances);
        self.ir.to_mut().model.appearances = appearances
            .into_iter()
            .filter(|appearance| !ids.contains(appearance.id.as_str()))
            .collect();
        for (id, error) in unwritable {
            self.losses.push(
                F3dLossCode::WriterAppearanceOmitted
                    .note(format!("appearance {id} was not written: {error}")),
            );
        }
    }

    /// The written design keys each Protein asset by its GUID, and one GUID
    /// holds one asset. An F3Z source holds an appearance table per document,
    /// where one GUID can name two different assets. A later appearance whose
    /// asset differs from the first one written under its GUID takes a GUID the
    /// writer allocates from its id.
    fn separate_shared_appearance_guids(&mut self) {
        let mut written = HashMap::<String, Vec<u8>>::new();
        let mut separate = Vec::new();
        for (index, appearance) in self.ir.model.appearances.iter().enumerate() {
            let Some(guid) = appearance
                .visual_guid
                .as_deref()
                .or(appearance.asset_guid.as_deref())
            else {
                continue;
            };
            let Ok(asset) = crate::materials::encode_protein(appearance) else {
                continue;
            };
            match written.get(guid) {
                Some(first) if *first != asset => separate.push(index),
                Some(_) => {}
                None => {
                    written.insert(guid.to_owned(), asset);
                }
            }
        }
        if separate.is_empty() {
            return;
        }
        let appearances = &mut self.ir.to_mut().model.appearances;
        let mut replaced = Vec::new();
        for index in separate {
            let Some(appearance) = appearances.get_mut(index) else {
                continue;
            };
            let guid = allocated_guid(appearance.id.as_str());
            let slot = if appearance.visual_guid.is_some() {
                &mut appearance.visual_guid
            } else {
                &mut appearance.asset_guid
            };
            if let Some(stated) = slot.replace(guid.clone()) {
                replaced.push(format!("{} from {stated} to {guid}", appearance.id));
            }
        }
        if !replaced.is_empty() {
            self.losses.push(F3dLossCode::WriterAppearanceGuidAllocated.note(format!(
                "{} appearance(s) state a GUID that an earlier appearance holds for a different asset; each is written under a GUID the writer allocates from its id: {}",
                replaced.len(),
                replaced.join(", ")
            )));
        }
    }

    /// W1 for the Design stream: drop every native record the B-rep stream is
    /// not written from. The neutral records written only from them go too.
    fn drop_native_design(&mut self, reason: &str) -> bool {
        if !self.writes_design() {
            return false;
        }
        let dropped = self.remove_native_arenas(|arena| !BREP_ARENAS.contains(&arena));
        if dropped.is_empty() {
            return false;
        }
        self.design_dropped = true;
        self.losses.extend(native_records_omitted(
            "F3D",
            &format!("of the Design stream, because the source-less generator cannot write them consistently ({reason})"),
            dropped,
        ));
        true
    }

    /// A native design whose timeline is rolled back is written from its
    /// neutral records, which give the features after the marker the form
    /// that `missing_states` selects (cadir-v11.md §3.4, Features without
    /// their state). The generator writes no feature scope from native
    /// records.
    fn drop_rolled_back_native_design(&mut self) {
        if !self.writes_design() {
            return;
        }
        let first = match rolled_back_from(&self.ir) {
            Some(Member::Feature(feature)) => feature.to_string(),
            Some(Member::Container(container)) => container.to_string(),
            None => return,
        };
        let form = match self.missing_states {
            MissingStateForm::GeometryTrue => "geometry-true",
            MissingStateForm::HistoryTrue => "history-true",
        };
        let reason = format!(
            "of the Design stream, because the generator writes the design's timeline, rolled back before {first}, from its neutral records in the {form} form"
        );
        let dropped = self.remove_native_arenas(|arena| !BREP_ARENAS.contains(&arena));
        self.design_dropped = true;
        self.losses
            .extend(native_records_omitted("F3D", &reason, dropped));
    }

    /// Write the design scope without its next droppable part (its sketch
    /// dimensions, its `Extrude` item, its `Sketch` items, in that order):
    /// the design plan charges what the part held.
    fn drop_design_part(&mut self, reason: &str) -> bool {
        let dropped = self.writes_neutral_design()
            && self
                .fallback
                .drop_next(&self.ir, self.missing_states, reason);
        if dropped && self.fallback.drops_items() {
            self.unhold();
        }
        dropped
    }

    /// Drop the records `culprit` names, and each record that needs them.
    /// Where the history-true run ends at an item, the document keeps the
    /// state the failed attempt wrote for the items before it.
    fn drop_records(&mut self, reason: &str, culprit: &Culprit) -> Result<bool, CodecError> {
        if !self.writes_neutral_design() {
            return Ok(false);
        }
        let earlier = self.fallback.clone();
        if !self
            .fallback
            .drop_records(&self.ir, self.missing_states, culprit, reason)
        {
            return Ok(false);
        }
        if self.fallback.drops_items() {
            self.unhold();
        } else {
            self.hold(&earlier)?;
        }
        Ok(true)
    }

    /// Write the state the history-true design holds where the current
    /// fallback ends its run at a feature that `earlier` does not: the
    /// bodies outside that state are not written.
    fn hold(&mut self, earlier: &super::design::Fallback) -> Result<(), CodecError> {
        let Some(scope) = super::design::root_scope_at(&self.ir) else {
            return Ok(());
        };
        let Some(held) = super::design::held_state(
            &self.ir,
            scope,
            self.missing_states,
            earlier,
            &self.fallback,
        ) else {
            return Ok(());
        };
        if self.unheld.is_none() {
            self.unheld = Some(Box::new(self.clone()));
        }
        let reason = format!(
            "the history-true design writes the state of the items before {} at its timeline marker",
            held.before
        );
        let dropped = self
            .ir
            .geometry
            .bodies
            .iter()
            .filter(|body| !held.state.bodies.contains(&body.id))
            .map(|body| body.id.as_str().to_owned())
            .collect::<Vec<_>>();
        let kept = held
            .state
            .bodies
            .iter()
            .map(|body| body.as_str().to_owned())
            .collect::<Vec<_>>();
        self.ir.to_mut().design.scopes[scope].current = vec![held.state];
        let kept = kept.iter().map(String::as_str).collect();
        self.retain_reporting_bodies(&kept, &dropped, &reason)
    }

    /// Take again the reduction before the first held state, with the
    /// current fallback: the design no longer writes the held items, so
    /// every body the hold removed is written.
    fn unhold(&mut self) {
        if let Some(unheld) = self.unheld.take() {
            let fallback = std::mem::take(&mut self.fallback);
            *self = *unheld;
            self.fallback = fallback;
        }
    }

    /// Write no design scope: every design scope, container, feature and
    /// parameter is charged by name, also where the document's v10 design
    /// records keep the export wrapper from charging the design arenas.
    fn drop_neutral_design(&mut self, reason: &str) -> bool {
        if !self.writes_neutral_design() {
            return false;
        }
        self.unhold();
        self.neutral_dropped = true;
        self.losses
            .extend(super::design::dropped_records(&self.ir, reason));
        true
    }

    /// W1 for every native record: write the neutral document alone.
    fn drop_native(&mut self, reason: &str) -> bool {
        if self.ir.native.namespace("f3d").is_none() {
            return false;
        }
        self.design_dropped = true;
        let namespace = self.ir.to_mut().native.0.remove("f3d").unwrap_or_default();
        let dropped = namespace
            .arenas()
            .values()
            .flatten()
            .map(|record| record.id().to_owned())
            .collect::<Vec<_>>();
        self.losses.extend(native_records_omitted(
            "F3D",
            &format!(
                "because the source-less generator cannot write the document with them ({reason})"
            ),
            dropped,
        ));
        true
    }

    /// Drop each body the stage cannot write on its own, with or without the
    /// native records. A body that writes alone only without them blames the
    /// native records, and the native fallbacks run first.
    fn drop_unwritable_bodies(
        &mut self,
        reason: &str,
        stage: Stage,
        writes_alone: &impl Fn(&CadIr, &crate::native::F3dNative, Stage) -> bool,
    ) -> Result<bool, CodecError> {
        let bodies = self
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.as_str().to_owned())
            .collect::<Vec<_>>();
        if bodies.is_empty() {
            return Ok(false);
        }
        let reach = Reach::new(&self.ir.geometry)?;
        let native = self
            .ir
            .native
            .namespace("f3d")
            .and_then(|namespace| crate::native::F3dNative::load(namespace).ok());
        let without_native = crate::native::F3dNative::default();
        let writes = |group: &[String], native: &crate::native::F3dNative| {
            let mut alone = CadIr::empty();
            alone.tolerances = self.ir.tolerances;
            alone.geometry = reach.subset(
                &self.ir.geometry,
                &reach.from(group.iter().map(String::as_str)),
            );
            writes_alone(&alone, native, stage)
        };
        // Bisect: a group that writes clears its bodies; a failing group is
        // halved until each failing body stands alone.
        let mut unwritable = Vec::new();
        let mut native_blamed = false;
        let mut pending = vec![bodies.as_slice()];
        while let Some(group) = pending.pop() {
            if writes(group, native.as_ref().unwrap_or(&without_native)) {
                continue;
            }
            if let [body] = group {
                if native.is_some() && writes(group, &without_native) {
                    native_blamed = true;
                } else {
                    unwritable.push(body.clone());
                }
                continue;
            }
            let (first, second) = group.split_at(group.len() / 2);
            pending.push(second);
            pending.push(first);
        }
        if unwritable.is_empty() {
            if native_blamed {
                return Ok(false);
            }
            let kept = bodies.iter().map(String::as_str).collect();
            let removed = self.retain(&kept, reason)?;
            return Ok(!removed.is_empty());
        }
        let unwritable_set = unwritable
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let kept = bodies
            .iter()
            .map(String::as_str)
            .filter(|body| !unwritable_set.contains(body))
            .collect();
        self.retain_reporting_bodies(&kept, &unwritable, reason)?;
        Ok(true)
    }

    fn drop_every_body(&mut self, reason: &str) -> Result<bool, CodecError> {
        let bodies = self
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.as_str().to_owned())
            .collect::<Vec<_>>();
        let geometry = &self.ir.geometry;
        if bodies.is_empty()
            && geometry.faces.is_empty()
            && geometry.surfaces.is_empty()
            && geometry.curves.is_empty()
            && geometry.points.is_empty()
            && geometry.pcurves.is_empty()
        {
            return Ok(false);
        }
        self.retain_reporting_bodies(&BTreeSet::new(), &bodies, reason)?;
        Ok(true)
    }

    fn retain_reporting_bodies(
        &mut self,
        kept: &BTreeSet<&str>,
        dropped_bodies: &[String],
        reason: &str,
    ) -> Result<(), CodecError> {
        for body in dropped_bodies {
            self.losses
                .push(F3dLossCode::WriterBodyOmitted.note(format!(
                "body {body} was not written, with the topology and geometry only it uses: {reason}"
            )));
        }
        let mut removed = retain_bodies(self.ir.to_mut(), kept)?;
        let omitted = dropped_bodies
            .iter()
            .filter(|body| removed.remove(body.as_str()))
            .cloned()
            .collect();
        self.charge_removed(&removed, &omitted, reason);
        Ok(())
    }

    /// Keep `kept` bodies and what they use; report the geometry no written
    /// body uses, and drop the native records that name a removed entity (W1).
    fn retain(
        &mut self,
        kept: &BTreeSet<&str>,
        reason: &str,
    ) -> Result<BTreeSet<String>, CodecError> {
        let removed = retain_bodies(self.ir.to_mut(), kept)?;
        self.charge_removed(&removed, &BTreeSet::new(), reason);
        Ok(removed)
    }

    /// Report the `unreachable` records no written body uses, and drop the
    /// native records that name them or the `omitted` records (W1).
    fn charge_removed(
        &mut self,
        unreachable: &BTreeSet<String>,
        omitted: &BTreeSet<String>,
        reason: &str,
    ) {
        if !unreachable.is_empty() {
            self.losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "{} topology and geometry record(s) of omitted bodies, or used by no written body, were not written ({reason}): {}",
                unreachable.len(),
                unreachable.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
            )));
        }
        let named = unreachable.union(omitted).cloned().collect::<BTreeSet<_>>();
        if named.is_empty() {
            return;
        }
        let dropped = self.remove_native_records_naming(&named);
        self.losses.extend(native_records_omitted(
            "F3D",
            "that name topology or geometry the write omits",
            dropped,
        ));
    }

    /// Remove the named native arenas; returns the removed record ids.
    fn remove_native_arenas(&mut self, remove: impl Fn(&str) -> bool) -> Vec<String> {
        let Some(namespace) = self.ir.to_mut().native.0.get_mut("f3d") else {
            return Vec::new();
        };
        let names = namespace
            .arenas()
            .keys()
            .filter(|name| remove(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let mut dropped = Vec::new();
        for name in names {
            if let Some(records) = namespace.arenas_mut().remove(&name) {
                dropped.extend(records.iter().map(|record| record.id().to_owned()));
            }
        }
        dropped
    }

    /// W1: a native record that holds a reference to a removed entity is
    /// dropped. Returns the dropped record ids.
    fn remove_native_records_naming(&mut self, removed: &BTreeSet<String>) -> Vec<String> {
        let Some(namespace) = self.ir.to_mut().native.0.get_mut("f3d") else {
            return Vec::new();
        };
        let mut dropped = Vec::new();
        for records in namespace.arenas_mut().values_mut() {
            records.retain(|record| {
                let names_removed = record.fields().map_or(true, |fields| {
                    fields.values().any(|value| mentions(value, removed))
                });
                if names_removed {
                    dropped.push(record.id().to_owned());
                }
                !names_removed
            });
        }
        dropped
    }
}

/// The start of `edge`'s curve, where the curve closes over the edge's range
/// (or its closed traversal) within `tolerance`, or why it has none. A
/// resource refusal of the evaluation is the outer error.
fn closed_curve_start(
    edge: &cadmpeg_ir::topology::Edge,
    curves: &HashMap<&str, &cadmpeg_ir::geometry::Curve>,
    tolerance: f64,
) -> Result<Result<FinitePoint3, String>, CodecError> {
    let Some(curve) = edge.curve() else {
        return Ok(Err("it has no curve".into()));
    };
    let Some(solved) = curves
        .get(curve.as_str())
        .and_then(|curve| curve.geometry.solved())
    else {
        return Ok(Err(format!("its curve {curve} has no solved form")));
    };
    let [start, end] = match edge.param_range() {
        Some(range) => range.get(),
        None => match solved.closed_traversal_range() {
            Some(range) => range.endpoints(),
            None => return Ok(Err(format!("its curve {curve} has no closed traversal"))),
        },
    };
    let point =
        |parameter| match curve_point_solved(EvaluationAdmission::Standard, solved, parameter) {
            Ok(point) => Ok(Some(point)),
            Err(EvaluationFailure::ResourceLimit(limit)) => Err(CodecError::ResourceLimit(limit)),
            Err(_) => Ok(None),
        };
    let (Some(first), Some(last)) = (point(start)?, point(end)?) else {
        return Ok(Err(format!(
            "its curve {curve} does not evaluate at its ends"
        )));
    };
    let (a, b) = (first.get(), last.get());
    let gap = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt();
    if gap > tolerance {
        return Ok(Err(format!(
            "its curve {curve} does not close over the edge"
        )));
    }
    Ok(Ok(first))
}

/// Remove a vertex-free edge, its coedges, the pcurves only they use, and the
/// loops they alone form. Returns false, changing nothing, where a loop holds
/// other coedges or is a face's classified outer loop.
fn omit_vertex_free_edge(geometry: &mut cadmpeg_ir::document::Topology, edge: &EdgeId) -> bool {
    let coedges = geometry
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == *edge)
        .map(|coedge| (coedge.id.clone(), coedge.owner_loop.clone()))
        .collect::<Vec<_>>();
    let coedge_ids = coedges.iter().map(|(id, _)| id).collect::<HashSet<_>>();
    let loops = coedges
        .iter()
        .map(|(_, owner)| owner)
        .collect::<HashSet<_>>();
    for row in geometry.loops.iter().filter(|row| loops.contains(&row.id)) {
        let LoopBoundary::Ring(ring) = &row.boundary else {
            return false;
        };
        if ring
            .coedges()
            .iter()
            .any(|coedge| !coedge_ids.contains(coedge))
        {
            return false;
        }
        let outer = geometry.faces.iter().any(|face| {
            face.id == row.face
                && matches!(&face.loops, FaceLoops::Classified { outer, .. } if *outer == row.id)
        });
        if outer {
            return false;
        }
    }
    let pcurves = geometry
        .coedges
        .iter()
        .filter(|coedge| coedge_ids.contains(&coedge.id))
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.clone()))
        .filter(|pcurve| {
            !geometry.coedges.iter().any(|coedge| {
                !coedge_ids.contains(&coedge.id)
                    && coedge.pcurves.iter().any(|use_| use_.pcurve == *pcurve)
            })
        })
        .collect::<HashSet<_>>();
    let loops = loops.into_iter().cloned().collect::<HashSet<_>>();
    let coedge_ids = coedge_ids.into_iter().cloned().collect::<HashSet<_>>();
    for face in &mut geometry.faces {
        match &mut face.loops {
            FaceLoops::Unspecified { loops: members }
            | FaceLoops::PartialUnspecified { loops: members } => {
                members.retain(|id| !loops.contains(id));
            }
            FaceLoops::Classified { inner, .. } => inner.retain(|id| !loops.contains(id)),
            FaceLoops::PartialClassified { outer, inner } => {
                inner.retain(|id| !loops.contains(id));
                if outer.as_ref().is_some_and(|id| loops.contains(id)) {
                    *outer = None;
                }
            }
        }
    }
    for shell in &mut geometry.shells {
        shell
            .members
            .retain(|member| !matches!(member, ShellMember::WireEdge { id } if id == edge));
    }
    geometry.loops.retain(|row| !loops.contains(&row.id));
    geometry
        .coedges
        .retain(|coedge| !coedge_ids.contains(&coedge.id));
    geometry
        .pcurves
        .retain(|pcurve| !pcurves.contains(&pcurve.id));
    geometry.edges.retain(|row| row.id != *edge);
    true
}

/// The faces of each body on a carrier F3D has no form for, with that carrier,
/// for the bodies that also hold a writable face.
fn unwritable_faces(ir: &CadIr) -> BTreeMap<String, Vec<(String, String)>> {
    let geometry = &ir.geometry;
    let owners = ProceduralOwners::new(ir);
    let surfaces = geometry
        .surfaces
        .iter()
        .filter(|surface| {
            matches!(
                super::smbh::surface_has_no_form(ir, &owners, surface),
                Ok(true)
            )
        })
        .map(|surface| surface.id.as_str())
        .collect::<HashSet<_>>();
    let curves = geometry
        .curves
        .iter()
        .filter(|curve| matches!(super::smbh::curve_has_no_form(&owners, curve), Ok(true)))
        .map(|curve| curve.id.as_str())
        .collect::<HashSet<_>>();
    if surfaces.is_empty() && curves.is_empty() {
        return BTreeMap::new();
    }
    let edge_curves = geometry
        .edges
        .iter()
        .filter_map(|edge| {
            let curve = edge.curve()?.as_str();
            curves.contains(curve).then_some((edge.id.as_str(), curve))
        })
        .collect::<HashMap<_, _>>();
    let coedge_edges = geometry
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge.edge.as_str()))
        .collect::<HashMap<_, _>>();
    let loops = geometry
        .loops
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect::<HashMap<_, _>>();
    let region_bodies = geometry
        .regions
        .iter()
        .map(|region| (region.id.as_str(), region.body.as_str()))
        .collect::<HashMap<_, _>>();
    let shell_bodies = geometry
        .shells
        .iter()
        .filter_map(|shell| {
            Some((
                shell.id.as_str(),
                *region_bodies.get(shell.region.as_str())?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut written = HashSet::new();
    let mut unwritable = BTreeMap::<String, Vec<(String, String)>>::new();
    for face in &geometry.faces {
        let Some(&body) = shell_bodies.get(face.shell.as_str()) else {
            continue;
        };
        let carrier = if surfaces.contains(face.surface.as_str()) {
            Some(format!("surface {}", face.surface))
        } else {
            face.loops
                .stated()
                .filter_map(|id| loops.get(id.as_str()))
                .flat_map(|row| row.coedges())
                .filter_map(|coedge| coedge_edges.get(coedge.as_str()))
                .find_map(|edge| {
                    edge_curves
                        .get(edge)
                        .map(|curve| format!("edge {edge} on curve {curve}"))
                })
        };
        match carrier {
            Some(carrier) => unwritable
                .entry(body.to_owned())
                .or_default()
                .push((face.id.to_string(), carrier)),
            None => {
                written.insert(body);
            }
        }
    }
    unwritable.retain(|body, _| written.contains(body.as_str()));
    unwritable
}

/// Split each of `shells` into the face sets that share no edge or vertex.
/// The set holding the shell's first face keeps the shell; each other set
/// takes a new shell in the same region, listed after it.
fn split_disconnected_shells(
    geometry: &mut cadmpeg_ir::document::Topology,
    shells: &BTreeSet<String>,
) {
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum Incidence<'a> {
        Edge(&'a str),
        Vertex(&'a str),
    }
    let edges = geometry
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let coedge_edges = geometry
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge.edge.as_str()))
        .collect::<HashMap<_, _>>();
    let loops = geometry
        .loops
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect::<HashMap<_, _>>();
    let mut used = geometry
        .shells
        .iter()
        .map(|shell| shell.id.to_string())
        .collect::<HashSet<_>>();
    let mut moves = Vec::new();
    let mut created = Vec::new();
    for shell in geometry
        .shells
        .iter()
        .filter(|shell| shells.contains(shell.id.as_str()))
    {
        let members = geometry
            .faces
            .iter()
            .enumerate()
            .filter(|(_, face)| face.shell == shell.id)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if members.len() < 2 {
            continue;
        }
        let mut parents = (0..members.len()).collect::<Vec<_>>();
        let mut first = HashMap::new();
        for (local, &index) in members.iter().enumerate() {
            for row in geometry.faces[index]
                .loops
                .stated()
                .filter_map(|id| loops.get(id.as_str()))
            {
                let mut incidences = row
                    .vertices()
                    .map(|vertex| Incidence::Vertex(vertex.as_str()))
                    .collect::<Vec<_>>();
                for coedge in row.coedges() {
                    let Some(&edge) = coedge_edges.get(coedge.as_str()) else {
                        continue;
                    };
                    incidences.push(Incidence::Edge(edge));
                    if let Some(edge) = edges.get(edge) {
                        incidences.extend(
                            edge.vertices()
                                .map(|vertex| Incidence::Vertex(vertex.as_str())),
                        );
                    }
                }
                for incidence in incidences {
                    let owner = *first.entry(incidence).or_insert(local);
                    let (a, b) = (
                        component_root(&mut parents, owner),
                        component_root(&mut parents, local),
                    );
                    parents[a.max(b)] = a.min(b);
                }
            }
        }
        let mut components = HashMap::new();
        let mut ordinal = 0usize;
        let mut after = shell.id.clone();
        for (local, &index) in members.iter().enumerate() {
            let root = component_root(&mut parents, local);
            if root == 0 {
                continue;
            }
            let id = if let Some(id) = components.get(&root) {
                ShellId::clone(id)
            } else {
                let id = loop {
                    ordinal += 1;
                    let candidate = ShellId::from(Identity::from(shell.id.clone()).with_key_tail(
                        &IdentityKeyTail::percent_encode(&format!("/component-{ordinal}")),
                    ));
                    if used.insert(candidate.to_string()) {
                        break candidate;
                    }
                };
                created.push((id.clone(), shell.region.clone(), after));
                after = id.clone();
                components.insert(root, id.clone());
                id
            };
            moves.push((index, id));
        }
    }
    for (index, shell) in moves {
        geometry.faces[index].shell = shell;
    }
    for (id, region, after) in created {
        if let Some(row) = geometry.regions.iter_mut().find(|row| row.id == region) {
            row.insert_shell_after(&after, id.clone());
        }
        geometry.shells.push(Shell::new(id, region));
    }
}

/// The root of `node`, halving the path on the way.
fn component_root(parents: &mut [usize], mut node: usize) -> usize {
    while parents[node] != node {
        parents[node] = parents[parents[node]];
        node = parents[node];
    }
    node
}

/// A hyphenated GUID allocated from `id`: the first 16 bytes of its SHA-256.
fn allocated_guid(id: &str) -> String {
    use sha2::Digest as _;
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = sha2::Sha256::digest(id.as_bytes());
    let hex = digest[..16]
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(DIGITS[usize::from(nibble)]))
        .collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn mentions(value: &Value, removed: &BTreeSet<String>) -> bool {
    match value {
        Value::String(text) => removed.contains(text),
        Value::Array(items) => items.iter().any(|item| mentions(item, removed)),
        Value::Object(fields) => fields.values().any(|item| mentions(item, removed)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// One loss listing every id of a dropped family, sorted after the count.
pub(super) fn named_loss<I: IntoIterator<Item = String>>(
    code: F3dLossCode,
    family: &str,
    reason: &str,
    ids: I,
) -> Option<LossNote> {
    let mut ids = ids.into_iter().collect::<Vec<_>>();
    if ids.is_empty() {
        return None;
    }
    ids.sort_unstable();
    Some(code.note(format!(
        "{} {family} record(s) were not written, because {reason}: {}",
        ids.len(),
        ids.join(", ")
    )))
}

enum Carrier {
    Surface(String),
    Curve(String),
    None,
}

struct ProceduralReduction {
    construction: String,
    carrier: Carrier,
    reason: String,
}

/// Every construction the generator cannot write on its carrier, where the
/// carrier holds a solved form to write instead, or no carrier uses it.
fn procedural_reductions(ir: &CadIr) -> Vec<ProceduralReduction> {
    let geometry = &ir.geometry;
    let owners = ProceduralOwners::new(ir);
    let mut reductions = Vec::new();
    let mut scratch = Vec::new();
    let mut surface_carriers = HashMap::<&str, Vec<&cadmpeg_ir::geometry::Surface>>::new();
    for surface in &geometry.surfaces {
        if let Some(construction) = surface.geometry.procedural_construction() {
            surface_carriers
                .entry(construction.as_str())
                .or_default()
                .push(surface);
        }
    }
    for procedural in &geometry.procedural_surfaces {
        let carriers = surface_carriers
            .get(procedural.id.as_str())
            .map_or(&[][..], Vec::as_slice);
        let Some((surface, rest)) = carriers.split_first() else {
            reductions.push(ProceduralReduction {
                construction: procedural.id.to_string(),
                carrier: Carrier::None,
                reason: "no surface carries it".into(),
            });
            continue;
        };
        if !rest.is_empty() {
            for extra in rest
                .iter()
                .filter(|extra| extra.geometry.solved_cache().is_some())
            {
                reductions.push(ProceduralReduction {
                    construction: procedural.id.to_string(),
                    carrier: Carrier::Surface(extra.id.to_string()),
                    reason: "more than one surface carries it".into(),
                });
            }
            continue;
        }
        scratch.clear();
        let reason = match surface.geometry.solved_cache() {
            Some(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
                native_procedural_surface(&mut scratch, ir, &owners, surface, nurbs)
                    .err()
                    .map(|error| error.to_string())
            }
            Some(
                SolvedSurfaceGeometry::Plane(_)
                | SolvedSurfaceGeometry::Cylinder(_)
                | SolvedSurfaceGeometry::Cone(_)
                | SolvedSurfaceGeometry::Sphere(_)
                | SolvedSurfaceGeometry::Torus(_),
            ) => Some("F3D retains no construction on an analytic carrier".into()),
            _ => None,
        };
        if let Some(reason) = reason {
            reductions.push(ProceduralReduction {
                construction: procedural.id.to_string(),
                carrier: Carrier::Surface(surface.id.to_string()),
                reason,
            });
        }
    }
    let mut curve_carriers = HashMap::<&str, Vec<&cadmpeg_ir::geometry::Curve>>::new();
    for curve in &geometry.curves {
        if let Some(construction) = curve.geometry.procedural_construction() {
            curve_carriers
                .entry(construction.as_str())
                .or_default()
                .push(curve);
        }
    }
    for procedural in &geometry.procedural_curves {
        let carriers = curve_carriers
            .get(procedural.id.as_str())
            .map_or(&[][..], Vec::as_slice);
        let Some((curve, rest)) = carriers.split_first() else {
            reductions.push(ProceduralReduction {
                construction: procedural.id.to_string(),
                carrier: Carrier::None,
                reason: "no curve carries it".into(),
            });
            continue;
        };
        if !rest.is_empty() {
            for extra in rest
                .iter()
                .filter(|extra| extra.geometry.solved_cache().is_some())
            {
                reductions.push(ProceduralReduction {
                    construction: procedural.id.to_string(),
                    carrier: Carrier::Curve(extra.id.to_string()),
                    reason: "more than one curve carries it".into(),
                });
            }
            continue;
        }
        scratch.clear();
        let reason = match curve.geometry.solved_cache() {
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                native_procedural_curve(&mut scratch, ir, &owners, &curve.id, nurbs, false)
                    .err()
                    .map(|error| error.to_string())
            }
            Some(_) => Some("F3D retains no construction on an analytic carrier".into()),
            None => None,
        };
        if let Some(reason) = reason {
            reductions.push(ProceduralReduction {
                construction: procedural.id.to_string(),
                carrier: Carrier::Curve(curve.id.to_string()),
                reason,
            });
        }
    }
    reductions
}

/// What each topology and geometry record reaches: the records it names and
/// the records its owner lists (a body's regions, a region's shells, a
/// shell's faces, a face's loops, a loop's coedges).
struct Reach {
    adjacency: HashMap<String, Vec<String>>,
    every: BTreeSet<String>,
    /// Each record's arena (in [`for_each_geometry_arena`] order) and row.
    rows: HashMap<String, (usize, usize)>,
}

macro_rules! for_each_geometry_arena {
    ($apply:ident) => {
        $apply!(
            bodies,
            regions,
            shells,
            faces,
            loops,
            coedges,
            edges,
            vertices,
            points,
            surfaces,
            curves,
            pcurves,
            procedural_surfaces,
            procedural_curves
        )
    };
}

/// Each non-empty geometry arena: its name, row count and record ids.
pub(super) fn geometry_records_by_arena(geometry: &cadmpeg_ir::document::Topology) -> Vec<String> {
    let mut arenas = Vec::new();
    macro_rules! list {
        ($($field:ident),*) => {$(
            if !geometry.$field.is_empty() {
                arenas.push(format!(
                    "{} {}: {}",
                    geometry.$field.len(),
                    stringify!($field),
                    geometry
                        .$field
                        .iter()
                        .map(|row| row.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        )*};
    }
    for_each_geometry_arena!(list);
    arenas
}

/// The records of `geometry` that `kept` does not hold unchanged, by arena:
/// each arena's name, row count and record ids.
pub(super) fn geometry_records_outside(
    geometry: &cadmpeg_ir::document::Topology,
    kept: &cadmpeg_ir::document::Topology,
) -> Vec<String> {
    let mut arenas = Vec::new();
    macro_rules! list {
        ($($field:ident),*) => {$(
            let outside = geometry
                .$field
                .iter()
                .filter(|row| !kept.$field.contains(row))
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>();
            if !outside.is_empty() {
                arenas.push(format!(
                    "{} {}: {}",
                    outside.len(),
                    stringify!($field),
                    outside.join(", ")
                ));
            }
        )*};
    }
    for_each_geometry_arena!(list);
    arenas
}

/// The F3D edge record stores a parameter range, and an open edge on a curve
/// with none is written with `[0, 1]`: a range its vertices do not fix, which
/// the output's validation finds.
fn unranged_open_edges(geometry: &cadmpeg_ir::document::Topology) -> Option<LossNote> {
    let edges = geometry
        .edges
        .iter()
        .filter(|edge| {
            edge.param_range().is_none()
                && edge.curve().is_some()
                && edge
                    .ends
                    .as_ref()
                    .is_some_and(|ends| ends.start != ends.end)
        })
        .map(|edge| edge.id.as_str())
        .collect::<Vec<_>>();
    if edges.is_empty() {
        return None;
    }
    Some(record_invalid(
        EntityKind::Edge,
        format!(
            "{} open edge(s) are written with the parameter range [0, 1]: the writer finds no range on the curve from the start vertex to the end vertex: {}",
            edges.len(),
            edges.join(", ")
        ),
    ))
}

impl Reach {
    fn new(geometry: &cadmpeg_ir::document::Topology) -> Result<Self, CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
        let mut adjacency = HashMap::<String, Vec<String>>::new();
        let mut every = BTreeSet::new();
        let mut rows = HashMap::new();
        let mut arena_ordinal = 0usize;
        macro_rules! forward {
            ($($field:ident),*) => {$(
                for (row_ordinal, row) in geometry.$field.iter().enumerate() {
                    let owner = row.id.as_str();
                    every.insert(owner.to_owned());
                    rows.insert(owner.to_owned(), (arena_ordinal, row_ordinal));
                    row.visit_identity_references(&ctx, &mut |target| {
                        if target != owner {
                            adjacency.entry(owner.to_owned()).or_default().push(target.to_owned());
                        }
                        Ok(())
                    })?;
                }
                arena_ordinal += 1;
            )*};
        }
        for_each_geometry_arena!(forward);
        macro_rules! owned {
            ($($field:ident => $owner:ident),*) => {$(
                for row in &geometry.$field {
                    adjacency
                        .entry(row.$owner.as_str().to_owned())
                        .or_default()
                        .push(row.id.as_str().to_owned());
                }
            )*};
        }
        owned!(regions => body, shells => region, faces => shell, loops => face, coedges => owner_loop);
        Ok(Self {
            adjacency,
            every,
            rows,
        })
    }

    fn from<'b>(&self, bodies: impl IntoIterator<Item = &'b str>) -> HashSet<String> {
        let mut reachable = bodies
            .into_iter()
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let mut pending = reachable.iter().cloned().collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            for adjacent in self.adjacency.get(&id).into_iter().flatten() {
                if reachable.insert(adjacent.clone()) {
                    pending.push(adjacent.clone());
                }
            }
        }
        reachable
    }

    /// The records of `geometry` in `reachable`, in arena order.
    fn subset(
        &self,
        geometry: &cadmpeg_ir::document::Topology,
        reachable: &HashSet<String>,
    ) -> cadmpeg_ir::document::Topology {
        let mut selected = Vec::<Vec<usize>>::new();
        for id in reachable {
            if let Some(&(arena, row)) = self.rows.get(id) {
                if selected.len() <= arena {
                    selected.resize_with(arena + 1, Vec::new);
                }
                selected[arena].push(row);
            }
        }
        for rows in &mut selected {
            rows.sort_unstable();
        }
        let mut subset = cadmpeg_ir::document::Topology::default();
        let mut arena = 0usize;
        macro_rules! copy {
            ($($field:ident),*) => {$(
                subset.$field = selected
                    .get(arena)
                    .into_iter()
                    .flatten()
                    .map(|&row| geometry.$field[row].clone())
                    .collect();
                arena += 1;
            )*};
        }
        for_each_geometry_arena!(copy);
        subset
    }
}

/// Keep the bodies in `kept` and every topology and geometry record they
/// reach; remove the rest. Returns the removed ids.
fn retain_bodies(ir: &mut CadIr, kept: &BTreeSet<&str>) -> Result<BTreeSet<String>, CodecError> {
    let reach = Reach::new(&ir.geometry)?;
    let reachable = reach.from(kept.iter().copied());
    let removed = reach
        .every
        .iter()
        .filter(|id| !reachable.contains(*id))
        .cloned()
        .collect::<BTreeSet<_>>();
    if removed.is_empty() {
        return Ok(removed);
    }
    let geometry = &mut ir.geometry;
    macro_rules! retain {
        ($($field:ident),*) => {$(
            geometry.$field.retain(|row| reachable.contains(row.id.as_str()));
        )*};
    }
    for_each_geometry_arena!(retain);
    Ok(removed)
}

/// The member a timeline marker of `ir` stands before, where it is a member
/// of the marker's container.
fn rolled_back_from(ir: &CadIr) -> Option<&Member> {
    ir.design
        .containers
        .iter()
        .find_map(|container| match container {
            Container::Ordered {
                members,
                rollback: Some(Rollback::Before(before)),
                ..
            } if members.contains(before) => Some(before),
            _ => None,
        })
}
