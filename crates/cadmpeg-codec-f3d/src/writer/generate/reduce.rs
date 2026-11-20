// SPDX-License-Identifier: Apache-2.0
//! Content the source-less generator cannot encode (§3.4, "Content the target
//! cannot hold"). An item with an exact F3D form is converted to it; any other
//! is dropped or degraded with a loss naming it, and the rest of the document
//! is written.
//!
//! [`Reduction::prepare`] removes the families the generator never writes
//! before the first attempt. An attempt that still fails names the stage that
//! failed, and [`Reduction::degrade`] takes the next fallback for that stage:
//! the native Design records, then every native record (W1), then the bodies
//! that cannot be written alone, then every body. An empty design is the last
//! form; only a failure to write that is returned. A failure of the design
//! scope's records takes one fallback: the design scope is not written.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    evaluation_missing, native_records_omitted, output_empty, record_invalid,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::curve_point_solved;
use cadmpeg_ir::eval::EvaluationFailure;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{EdgeId, PointId, VertexId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::topology::{
    Color, EdgeCarrier, EdgeEnds, FaceLoops, LoopBoundary, Point, ShellMember, Vertex,
};
use serde_json::Value;

use crate::loss::F3dLossCode;

use super::native_geometry::{native_procedural_curve, native_procedural_surface};

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
    /// The records of the root definition's design scope.
    NeutralDesign,
    /// The archive itself: nothing in the document can avoid it.
    Archive,
}

/// A failed attempt and the stage it failed in.
#[derive(Debug)]
pub(super) struct Failure {
    pub(super) stage: Stage,
    pub(super) error: CodecError,
}

impl Failure {
    pub(super) fn brep(error: CodecError) -> Self {
        Self {
            stage: Stage::Brep,
            error,
        }
    }

    pub(super) fn design(error: CodecError) -> Self {
        Self {
            stage: Stage::Design,
            error,
        }
    }

    pub(super) fn neutral_design(error: CodecError) -> Self {
        Self {
            stage: Stage::NeutralDesign,
            error,
        }
    }

    pub(super) fn archive(error: CodecError) -> Self {
        Self {
            stage: Stage::Archive,
            error,
        }
    }
}

/// The document the generator writes, and the losses taken to reach it.
pub(super) struct Reduction<'a> {
    ir: Cow<'a, CadIr>,
    losses: Vec<LossNote>,
    design_dropped: bool,
    neutral_dropped: bool,
    /// The parts of the design scope's form dropped after a failure.
    fallback: super::design::Fallback,
}

impl<'a> Reduction<'a> {
    pub(super) fn new(ir: &'a CadIr) -> Self {
        Self {
            ir: Cow::Borrowed(ir),
            losses: Vec::new(),
            design_dropped: false,
            neutral_dropped: false,
            fallback: super::design::Fallback::default(),
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

    /// The parts of the design scope's form dropped after a failure with
    /// them.
    pub(super) fn fallback(&self) -> &super::design::Fallback {
        &self.fallback
    }

    /// The losses, with `export:output.empty` when no geometry is written.
    pub(super) fn finish(mut self) -> Vec<LossNote> {
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
        self.drop_unwritten_native();
        if !self.writes_design() {
            self.drop_design_only_arenas();
        }
        self.drop_unwritten_arenas();
        self.strip_carrier_metadata();
        self.complete_vertex_free_edges()?;
        self.drop_pole_loops();
        self.state_open_edge_ranges()?;
        self.write_attributes_in_their_native_form();
        self.reduce_procedural_carriers();
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
        let Failure { stage, error } = failure;
        if matches!(error, CodecError::ResourceLimit(_) | CodecError::Io(_)) {
            return Err(error);
        }
        let reason = error.to_string();
        let progressed = match stage {
            Stage::Archive => false,
            Stage::NeutralDesign => {
                self.drop_design_part(&reason) || self.drop_neutral_design(&reason)
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
        let subds = std::mem::take(&mut ir.geometry.subds);
        let assets = std::mem::take(&mut ir.model.assets);
        let tessellations = std::mem::take(&mut ir.model.tessellations);
        self.losses.extend(named_loss(
            F3dLossCode::WriterSubdOmitted,
            "subdivision surface",
            "the source-less F3D generator has no T-spline form",
            subds.iter().map(|row| row.id.to_string()),
        ));
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

    /// An open edge that states no parameter range spans its vertices; the
    /// F3D edge record stores the range, so the writer states the vertices'
    /// parameters on the carrier, the exact form of the same edge. An edge
    /// whose vertices do not invert onto its carrier keeps no range.
    fn state_open_edge_ranges(&mut self) -> Result<(), CodecError> {
        let geometry = &self.ir.geometry;
        let open = geometry
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                edge.param_range().is_none()
                    && edge.curve().is_some()
                    && edge
                        .ends
                        .as_ref()
                        .is_some_and(|ends| ends.start != ends.end)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if open.is_empty() {
            return Ok(());
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(&self.ir);
        let points = geometry
            .points
            .iter()
            .map(|point| (point.id.as_str(), point.position().get()))
            .collect::<HashMap<_, _>>();
        let positions = geometry
            .vertices
            .iter()
            .filter_map(|vertex| Some((vertex.id.as_str(), *points.get(vertex.point.as_str())?)))
            .collect::<HashMap<_, _>>();
        let curves = geometry
            .curves
            .iter()
            .map(|curve| (curve.id.as_str(), curve))
            .collect::<HashMap<_, _>>();
        let mut ranges = Vec::new();
        for edge_index in open {
            let edge = &geometry.edges[edge_index];
            let (Some(curve), Some(ends)) = (edge.curve(), edge.ends.as_ref()) else {
                continue;
            };
            let (Some(start), Some(end)) = (
                positions.get(ends.start.as_str()),
                positions.get(ends.end.as_str()),
            ) else {
                continue;
            };
            let invert = |point, seed| {
                cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
                    &ctx, &index, curve, point, seed,
                )
                .map(|parameter| parameter.map(cadmpeg_ir::scalar::FiniteReal::get))
            };
            let Some(first) = invert(*start, 0.0)? else {
                continue;
            };
            let Some(mut last) = invert(*end, first)? else {
                continue;
            };
            let periodic = curves.get(curve.as_str()).is_some_and(|curve| {
                matches!(
                    curve.geometry.solved(),
                    Some(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_))
                )
            });
            if last <= first && periodic {
                last += std::f64::consts::TAU;
            }
            if last > first {
                ranges.push((edge_index, curve.clone(), [first, last]));
            }
        }
        drop(index);
        let geometry = &mut self.ir.to_mut().geometry;
        for (edge_index, curve, range) in ranges {
            if let Ok(carrier) = EdgeCarrier::new(Some(curve), Some(range)) {
                geometry.edges[edge_index].carrier = carrier;
            }
        }
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
                FaceLoops::Unspecified { .. } => None,
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
                FaceLoops::Unspecified { loops } => loops.retain(|id| !poles.contains(id)),
                FaceLoops::Classified { inner, .. } => inner.retain(|id| !poles.contains(id)),
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

    /// Write the design scope without its next droppable part (its sketch
    /// dimensions, its `Extrude` item, its `Sketch` items, in that order):
    /// the design plan charges what the part held.
    fn drop_design_part(&mut self, reason: &str) -> bool {
        self.writes_neutral_design() && self.fallback.drop_next(&self.ir, reason)
    }

    /// Write no design scope: its features and parameters are charged.
    fn drop_neutral_design(&mut self, reason: &str) -> bool {
        if !self.writes_neutral_design() {
            return false;
        }
        let (features, parameters) = super::design::scope_records(&self.ir);
        self.neutral_dropped = true;
        self.losses.extend(evaluation_missing(
            "F3D",
            &format!("stores no design for these features ({reason})"),
            features,
        ));
        self.losses.extend(named_loss(
            F3dLossCode::WriterParameterOmitted,
            "parameter",
            &format!("the F3D writer writes no design ({reason})"),
            parameters,
        ));
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
        self.retain(kept, reason)?;
        Ok(())
    }

    /// Keep `kept` bodies and what they use; report the geometry no written
    /// body uses, and drop the native records that name a removed entity (W1).
    fn retain(
        &mut self,
        kept: &BTreeSet<&str>,
        reason: &str,
    ) -> Result<BTreeSet<String>, CodecError> {
        let ir = self.ir.to_mut();
        let removed = retain_bodies(ir, kept)?;
        if removed.is_empty() {
            return Ok(removed);
        }
        self.losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
            "{} topology and geometry record(s) of omitted bodies, or used by no written body, were not written ({reason}): {}",
            removed.len(),
            removed.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
        )));
        let dropped = self.remove_native_records_naming(&removed);
        self.losses.extend(native_records_omitted(
            "F3D",
            "that name topology or geometry the write omits",
            dropped,
        ));
        Ok(removed)
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
            FaceLoops::Unspecified { loops: members } => members.retain(|id| !loops.contains(id)),
            FaceLoops::Classified { inner, .. } => inner.retain(|id| !loops.contains(id)),
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
                native_procedural_surface(&mut scratch, ir, surface, nurbs)
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
                native_procedural_curve(&mut scratch, ir, &curve.id, nurbs)
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
fn geometry_records_by_arena(geometry: &cadmpeg_ir::document::Topology) -> Vec<String> {
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
