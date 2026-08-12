// SPDX-License-Identifier: Apache-2.0
//! Neutral-feature write encoders for `SolidWorks` history records.

mod datum;
mod misc;
mod modify;
mod sketch;
mod solid;
mod surface;

use crate::records::Feature;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureTreeNodeRole};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) type NeutralFeatureEncoding =
    (String, BTreeMap<String, String>, BTreeMap<String, String>);

pub(super) struct NeutralFeatureEncoder<'context, 'feature_key, 'source> {
    pub(super) feature: &'context cadmpeg_ir::features::Feature,
    pub(super) existing: Option<&'context Feature>,
    pub(super) principal_planes_by_record:
        &'context HashMap<String, cadmpeg_ir::features::PrincipalPlane>,
    pub(super) record_sources: &'context HashMap<String, String>,
    pub(super) retained_tree_node_roles: &'context HashMap<String, FeatureTreeNodeRole>,
    pub(super) feature_sources: &'context HashMap<&'feature_key FeatureId, &'source str>,
    pub(super) sketch_sources: &'context HashMap<cadmpeg_ir::sketches::SketchId, String>,
    pub(super) parent_sources: &'context HashMap<FeatureId, String>,
    pub(super) resolved_parameter_names: &'context HashMap<String, HashSet<String>>,
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "Per-feature encoders use one fallible dispatch interface."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        match &self.feature.definition {
            FeatureDefinition::TreeNode { .. } => self.encode_tree_node(),
            FeatureDefinition::CosmeticThread { .. } => self.encode_cosmetic_thread(),
            FeatureDefinition::SketchBlockDefinition { .. } => {
                self.encode_sketch_block_definition()
            }
            FeatureDefinition::SketchBlockInstance { .. } => self.encode_sketch_block_instance(),
            FeatureDefinition::Native { .. } => self.encode_native(),
            FeatureDefinition::StoredGeometry => self.encode_stored_geometry(),
            FeatureDefinition::DerivedGeometry { .. } => self.encode_derived_geometry(),
            FeatureDefinition::ImportedGeometry { .. } => self.encode_imported_geometry(),
            FeatureDefinition::Primitive { .. } => self.encode_primitive(),
            FeatureDefinition::DatumPrincipalPlane { .. } => self.encode_datum_principal_plane(),
            FeatureDefinition::DatumPlaneUnresolved => self.encode_datum_plane_unresolved(),
            FeatureDefinition::BoundarySurfaceUnresolved => {
                self.encode_boundary_surface_unresolved()
            }
            FeatureDefinition::DatumPlane { .. } => self.encode_datum_plane(),
            FeatureDefinition::DatumOffsetPlane { .. } => self.encode_datum_offset_plane(),
            FeatureDefinition::TrimSurface { .. } => self.encode_trim_surface(),
            FeatureDefinition::ExtendSurface { .. } => self.encode_extend_surface(),
            FeatureDefinition::RuledSurface { .. } => self.encode_ruled_surface(),
            FeatureDefinition::DatumAxis { .. } => self.encode_datum_axis(),
            FeatureDefinition::DatumPoint { .. } => self.encode_datum_point(),
            FeatureDefinition::DatumCoordinateSystem { .. } => {
                self.encode_datum_coordinate_system()
            }
            FeatureDefinition::EquationCurve { .. } => self.encode_equation_curve(),
            FeatureDefinition::ProjectedCurve { .. } => self.encode_projected_curve(),
            FeatureDefinition::CompositeCurve { .. } => self.encode_composite_curve(),
            FeatureDefinition::Helix { .. } => self.encode_helix(),
            FeatureDefinition::HelixNativeAxis { .. } => self.encode_helix_native_axis(),
            FeatureDefinition::Wrap { .. } => self.encode_wrap(),
            FeatureDefinition::Sketch { .. } => self.encode_sketch(),
            FeatureDefinition::SpatialSketch { .. } => self.encode_spatial_sketch(),
            FeatureDefinition::Extrude { .. } => self.encode_extrude(),
            FeatureDefinition::Fillet { .. } => self.encode_fillet(),
            FeatureDefinition::Chamfer { .. } => self.encode_chamfer(),
            FeatureDefinition::OffsetShape { .. } => self.encode_offset_shape(),
            FeatureDefinition::PostProcess { .. } => self.encode_post_process(),
            FeatureDefinition::PointGeometry { .. }
            | FeatureDefinition::LineSegment { .. }
            | FeatureDefinition::CircularArc { .. }
            | FeatureDefinition::EllipticArc { .. }
            | FeatureDefinition::Polyline { .. }
            | FeatureDefinition::RegularPolygonCurve { .. }
            | FeatureDefinition::PlanarPatch { .. }
            | FeatureDefinition::FaceFromShapes { .. } => self.encode_curve_geometry(),
            FeatureDefinition::Compound { .. }
            | FeatureDefinition::RefineShape { .. }
            | FeatureDefinition::ReverseShape { .. }
            | FeatureDefinition::RuledBetweenCurves { .. }
            | FeatureDefinition::SectionShape { .. }
            | FeatureDefinition::MirrorShape { .. }
            | FeatureDefinition::ProjectOnSurface { .. } => self.encode_shape_operation(),
            FeatureDefinition::Shell { .. } => self.encode_shell(),
            FeatureDefinition::Thicken { .. } => self.encode_thicken(),
            FeatureDefinition::OffsetSurface { .. } => self.encode_offset_surface(),
            FeatureDefinition::KnitSurface { .. } => self.encode_knit_surface(),
            FeatureDefinition::FilledSurface { .. } => self.encode_filled_surface(),
            FeatureDefinition::Draft { .. } => self.encode_draft(),
            FeatureDefinition::Combine { .. } => self.encode_combine(),
            FeatureDefinition::CutWithSurface { .. } => self.encode_cut_with_surface(),
            FeatureDefinition::DeleteBody { .. } => self.encode_delete_body(),
            FeatureDefinition::DeleteFace { .. } => self.encode_delete_face(),
            FeatureDefinition::ReplaceFace { .. } => self.encode_replace_face(),
            FeatureDefinition::MoveFace { .. } => self.encode_move_face(),
            FeatureDefinition::MoveBody { .. } => self.encode_move_body(),
            FeatureDefinition::Dome { .. } => self.encode_dome(),
            FeatureDefinition::Flex { .. } => self.encode_flex(),
            FeatureDefinition::Scale { .. } => self.encode_scale(),
            FeatureDefinition::Hole { .. } => self.encode_hole(),
            FeatureDefinition::Revolve { .. } => self.encode_revolve(),
            FeatureDefinition::Sweep { .. } => self.encode_sweep(),
            FeatureDefinition::Loft { .. } => self.encode_loft(),
            FeatureDefinition::Rib { .. } => self.encode_rib(),
            FeatureDefinition::Pattern { .. } => self.encode_pattern(),
            FeatureDefinition::HelicalSweep { .. } => self.encode_helical_sweep(),
            FeatureDefinition::Binder { .. } => self.encode_binder(),
            FeatureDefinition::DatumPointUnresolved
            | FeatureDefinition::DatumCoordinateSystemUnresolved
            | FeatureDefinition::Block { .. }
            | FeatureDefinition::ExtractBody { .. }
            | FeatureDefinition::LoftUnresolved
            | FeatureDefinition::FreeformSurfaceUnresolved
            | FeatureDefinition::DraftUnresolved
            | FeatureDefinition::FaceBlend { .. }
            | FeatureDefinition::SewBodies { .. }
            | FeatureDefinition::TrimBodies { .. } => self.encode_explicitly_unsupported(),
            _ => self.encode_unsupported(),
        }
    }
}
