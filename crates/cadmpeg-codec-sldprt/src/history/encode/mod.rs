// SPDX-License-Identifier: Apache-2.0
//! Neutral-feature write encoders for `SolidWorks` history records.

mod datum;
mod format;
mod misc;
mod modify;
mod pattern;
mod sketch;
mod solid;
mod spin;
mod support;
mod surface;

use crate::records::Feature;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureTreeNodeRole};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Native XML kind plus parameter and property maps for a written feature.
pub(super) struct NeutralFeatureEncoding {
    pub(super) kind: String,
    pub(super) parameters: BTreeMap<String, String>,
    pub(super) properties: BTreeMap<String, String>,
}

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

impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        match &self.feature.definition {
            FeatureDefinition::TreeNode {
                role,
                children,
                active_child,
            } => self.encode_tree_node(role, children, active_child),
            FeatureDefinition::CosmeticThread {
                face,
                diameter,
                extent,
            } => self.encode_cosmetic_thread(face, diameter, extent),
            FeatureDefinition::SketchBlockDefinition { sketch } => {
                self.encode_sketch_block_definition(sketch)
            }
            FeatureDefinition::SketchBlockInstance { block, placement } => {
                self.encode_sketch_block_instance(block, placement)
            }
            FeatureDefinition::Native {
                kind,
                parameters,
                properties,
            } => Ok(self.encode_native(kind, parameters, properties)),
            FeatureDefinition::StoredGeometry => Ok(self.encode_stored_geometry()),
            FeatureDefinition::DerivedGeometry { .. } => self.encode_derived_geometry(),
            FeatureDefinition::ImportedGeometry { .. } => self.encode_imported_geometry(),
            FeatureDefinition::Primitive { .. } => self.encode_primitive(),
            FeatureDefinition::DatumPrincipalPlane { plane } => {
                self.encode_datum_principal_plane(plane)
            }
            FeatureDefinition::DatumPlaneUnresolved => self.encode_datum_plane_unresolved(),
            FeatureDefinition::BoundarySurfaceUnresolved => {
                self.encode_boundary_surface_unresolved()
            }
            FeatureDefinition::DatumPlane {
                origin,
                normal,
                u_axis,
            } => self.encode_datum_plane(origin, normal, u_axis),
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } => self.encode_datum_offset_plane(reference, distance),
            FeatureDefinition::TrimSurface {
                faces, tool, keep, ..
            } => self.encode_trim_surface(faces, tool, keep),
            FeatureDefinition::ExtendSurface {
                faces,
                distance,
                method,
            } => self.encode_extend_surface(faces, distance, method),
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                mode,
                angle,
                alternate_face,
                corner,
            } => {
                self.encode_ruled_surface(edges, support_faces, mode, angle, alternate_face, corner)
            }
            FeatureDefinition::DatumAxis { origin, direction } => {
                self.encode_datum_axis(origin, direction)
            }
            FeatureDefinition::DatumPoint { position, .. } => self.encode_datum_point(position),
            FeatureDefinition::DatumCoordinateSystem {
                origin,
                x_axis,
                y_axis,
                z_axis,
            } => self.encode_datum_coordinate_system(origin, x_axis, y_axis, z_axis),
            FeatureDefinition::EquationCurve {
                parameter,
                x_expression,
                y_expression,
                z_expression,
                start,
                end,
            } => self.encode_equation_curve(
                parameter,
                x_expression,
                y_expression,
                z_expression,
                start,
                end,
            ),
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            } => self.encode_projected_curve(source, target_faces, direction, bidirectional),
            FeatureDefinition::CompositeCurve { segments, closed } => {
                self.encode_composite_curve(segments, closed)
            }
            FeatureDefinition::Helix {
                axis_origin,
                axis_direction,
                radius,
                shape,
                revolutions,
                start_angle,
                clockwise,
                segment_turns,
                construction_style,
            } => self.encode_helix(
                axis_origin,
                axis_direction,
                radius,
                shape,
                revolutions,
                start_angle,
                clockwise,
                segment_turns,
                construction_style,
            ),
            FeatureDefinition::HelixNativeAxis {
                axis_native_ref,
                axial_rise,
                pitch,
                revolutions,
                start_angle,
                clockwise,
            } => self.encode_helix_native_axis(
                axis_native_ref,
                axial_rise,
                pitch,
                revolutions,
                start_angle,
                clockwise,
            ),
            FeatureDefinition::Wrap {
                profile,
                face,
                mode,
            } => self.encode_wrap(profile, face, mode),
            FeatureDefinition::Sketch { .. } => self.encode_sketch(),
            FeatureDefinition::SpatialSketch { .. } => self.encode_spatial_sketch(),
            FeatureDefinition::Extrude {
                profile,
                direction,
                start,
                extent,
                op,
                direction_source,
                solid,
                face_maker,
                inner_wire_taper,
                length_along_profile_normal,
                allow_multi_profile_faces,
            } => self.encode_extrude(
                profile,
                direction,
                start,
                extent,
                op,
                direction_source,
                solid,
                face_maker,
                inner_wire_taper,
                length_along_profile_normal,
                allow_multi_profile_faces,
            ),
            FeatureDefinition::Fillet { groups } => self.encode_fillet(groups),
            FeatureDefinition::Chamfer {
                groups,
                flip_direction,
            } => self.encode_chamfer(groups, flip_direction),
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
            FeatureDefinition::Shell {
                bodies,
                removed_faces,
                thickness,
                outward,
                mode,
                join,
                resolve_intersections,
                allow_self_intersections,
            } => self.encode_shell(
                bodies,
                removed_faces,
                thickness,
                outward,
                mode,
                join,
                resolve_intersections,
                allow_self_intersections,
            ),
            FeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } => self.encode_thicken(faces, thickness, side),
            FeatureDefinition::OffsetSurface { faces, distance } => {
                self.encode_offset_surface(faces, distance)
            }
            FeatureDefinition::KnitSurface {
                faces,
                merge_entities,
                create_solid,
                gap_tolerance,
            } => self.encode_knit_surface(faces, merge_entities, create_solid, gap_tolerance),
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                continuity,
                merge_result,
            } => self.encode_filled_surface(boundary, support_faces, continuity, merge_result),
            FeatureDefinition::Draft {
                faces: face_selection,
                anchor,
                angle,
                outward,
            } => self.encode_draft(face_selection, anchor, angle, outward),
            FeatureDefinition::Combine {
                target,
                tools,
                op,
                keep_tools,
            } => self.encode_combine(target, tools, op, keep_tools),
            FeatureDefinition::CutWithSurface {
                targets,
                tools,
                reverse,
            } => self.encode_cut_with_surface(targets, tools, reverse),
            FeatureDefinition::DeleteBody { bodies, mode } => self.encode_delete_body(bodies, mode),
            FeatureDefinition::DeleteFace { faces, heal } => self.encode_delete_face(faces, heal),
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => self.encode_replace_face(targets, replacements),
            FeatureDefinition::MoveFace { faces, motion } => self.encode_move_face(faces, motion),
            FeatureDefinition::MoveBody {
                bodies,
                translation,
                rotation,
                copies,
            } => self.encode_move_body(bodies, translation, rotation, copies),
            FeatureDefinition::Dome {
                faces,
                height,
                elliptical,
                reverse,
            } => self.encode_dome(faces, height, elliptical, reverse),
            FeatureDefinition::Flex { axis, mode } => self.encode_flex(axis, mode),
            FeatureDefinition::Scale {
                bodies,
                center,
                factors,
            } => self.encode_scale(bodies, center, factors),
            FeatureDefinition::Hole {
                profile,
                profile_filter,
                face,
                placements,
                construction,
                exit_kind,
                diameter,
                extent,
                bottom,
                taper_angle,
                allow_multi_profile_faces,
            } => self.encode_hole(
                profile,
                profile_filter,
                face,
                placements,
                construction,
                exit_kind,
                diameter,
                extent,
                bottom,
                taper_angle,
                allow_multi_profile_faces,
            ),
            FeatureDefinition::Revolve { construction, op } => {
                self.encode_revolve(construction, op)
            }
            FeatureDefinition::Sweep {
                section,
                sections,
                path,
                mode,
                orientation,
                transition,
                transformation,
                path_tangent,
                linearize,
                twist,
                path_extent,
                guide_rail,
                taper,
                scale,
                allow_multi_profile_faces,
            } => self.encode_sweep(
                section,
                sections,
                path,
                mode,
                orientation,
                transition,
                transformation,
                path_tangent,
                linearize,
                twist,
                path_extent,
                guide_rail,
                taper,
                scale,
                allow_multi_profile_faces,
            ),
            FeatureDefinition::Loft {
                sections,
                guides,
                centerline,
                op,
                closed,
                solid,
                ruled,
                linearize,
                max_degree,
                allow_multi_profile_faces,
            } => self.encode_loft(
                sections,
                guides,
                centerline,
                op,
                closed,
                solid,
                ruled,
                linearize,
                max_degree,
                allow_multi_profile_faces,
            ),
            FeatureDefinition::Rib { construction, op } => self.encode_rib(construction, op),
            FeatureDefinition::Pattern { seeds, pattern } => self.encode_pattern(seeds, pattern),
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
