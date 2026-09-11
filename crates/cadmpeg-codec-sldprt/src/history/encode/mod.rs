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
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureId, FeatureOperation, FeatureTreeNodeRole, UnresolvedFamily,
};
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
        match self.feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::TreeNode { role, children }) => {
                self.encode_tree_node(role, children, children.active_child().as_ref())
            }
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                face,
                diameter,
                extent,
            }) => self.encode_cosmetic_thread(face, diameter, extent),
            FeatureDefinition::Operation(FeatureOperation::SketchBlockDefinition { sketch }) => {
                self.encode_sketch_block_definition(sketch)
            }
            FeatureDefinition::Operation(FeatureOperation::SketchBlockInstance {
                block,
                placement,
            }) => self.encode_sketch_block_instance(block, placement),
            FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters }) => {
                Ok(self.encode_native(kind, parameters))
            }
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}) => {
                Ok(self.encode_stored_geometry())
            }
            FeatureDefinition::Operation(FeatureOperation::DerivedGeometry { .. }) => {
                self.encode_derived_geometry()
            }
            FeatureDefinition::Operation(FeatureOperation::ImportedGeometry { .. }) => {
                self.encode_imported_geometry()
            }
            FeatureDefinition::Operation(FeatureOperation::Primitive { .. }) => {
                self.encode_primitive()
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane }) => {
                self.encode_datum_principal_plane(plane)
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            }) => self.encode_datum_plane_unresolved(),
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::BoundarySurface,
            }) => self.encode_boundary_surface_unresolved(),
            FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => {
                self.encode_datum_plane(frame)
            }
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference,
                distance,
            }) => self.encode_datum_offset_plane(reference, distance),
            FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                faces,
                tool,
                keep,
                ..
            }) => self.encode_trim_surface(faces, tool, keep),
            FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
                faces,
                distance,
                method,
            }) => self.encode_extend_surface(faces, distance, method),
            FeatureDefinition::Operation(FeatureOperation::RuledSurface {
                edges,
                support_faces,
                mode,
                angle,
                alternate_face,
                corner,
            }) => {
                self.encode_ruled_surface(edges, support_faces, mode, angle, alternate_face, corner)
            }
            FeatureDefinition::Operation(FeatureOperation::DatumAxis { origin, direction }) => {
                self.encode_datum_axis(origin, direction)
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint { position, .. }) => {
                self.encode_datum_point(position)
            }
            FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { frame }) => {
                self.encode_datum_coordinate_system(frame)
            }
            FeatureDefinition::Operation(FeatureOperation::EquationCurve { curve }) => {
                self.encode_equation_curve(curve)
            }
            FeatureDefinition::Operation(FeatureOperation::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            }) => self.encode_projected_curve(source, target_faces, direction, bidirectional),
            FeatureDefinition::Operation(FeatureOperation::CompositeCurve { segments, closed }) => {
                self.encode_composite_curve(segments, closed)
            }
            FeatureDefinition::Operation(FeatureOperation::Helix {
                axis_origin,
                axis_direction,
                radius,
                shape,
                revolutions,
                start_angle,
                clockwise,
                segment_turns,
                construction_style,
            }) => self.encode_helix(
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
            FeatureDefinition::Operation(FeatureOperation::HelixNativeAxis {
                axis_native_ref,
                axial_rise,
                pitch,
                revolutions,
                start_angle,
                clockwise,
            }) => self.encode_helix_native_axis(
                axis_native_ref.as_str(),
                axial_rise,
                pitch,
                revolutions,
                start_angle,
                clockwise,
            ),
            FeatureDefinition::Operation(FeatureOperation::Wrap {
                profile,
                face,
                mode,
            }) => self.encode_wrap(profile, face, mode),
            FeatureDefinition::Operation(FeatureOperation::Sketch { .. }) => self.encode_sketch(),
            FeatureDefinition::Operation(FeatureOperation::SpatialSketch { .. }) => {
                self.encode_spatial_sketch()
            }
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile,
                direction,
                start,
                extent,
                op,
                solid,
                face_maker,
                inner_wire_taper,
                length_along_profile_normal,
                allow_multi_profile_faces,
            }) => self.encode_extrude(
                profile,
                direction,
                start,
                extent,
                op,
                solid,
                face_maker,
                inner_wire_taper,
                length_along_profile_normal,
                allow_multi_profile_faces,
            ),
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                self.encode_fillet(groups)
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer {
                groups,
                flip_direction,
            }) => self.encode_chamfer(groups, flip_direction),
            FeatureDefinition::Operation(FeatureOperation::OffsetShape { .. }) => {
                self.encode_offset_shape()
            }
            FeatureDefinition::PostProcess { .. } => self.encode_post_process(),
            FeatureDefinition::Operation(
                FeatureOperation::PointGeometry { .. }
                | FeatureOperation::LineSegment { .. }
                | FeatureOperation::CircularArc { .. }
                | FeatureOperation::EllipticArc { .. }
                | FeatureOperation::Polyline { .. }
                | FeatureOperation::RegularPolygonCurve { .. }
                | FeatureOperation::PlanarPatch { .. }
                | FeatureOperation::FaceFromShapes { .. },
            ) => self.encode_curve_geometry(),
            FeatureDefinition::Operation(
                FeatureOperation::Compound { .. }
                | FeatureOperation::RefineShape { .. }
                | FeatureOperation::ReverseShape { .. }
                | FeatureOperation::RuledBetweenCurves { .. }
                | FeatureOperation::SectionShape { .. }
                | FeatureOperation::MirrorShape { .. }
                | FeatureOperation::ProjectOnSurface { .. },
            ) => self.encode_shape_operation(),
            FeatureDefinition::Operation(FeatureOperation::Shell {
                bodies,
                removed_faces,
                thickness,
                outward,
                mode,
                join,
                resolve_intersections,
                allow_self_intersections,
            }) => self.encode_shell(
                bodies,
                removed_faces,
                thickness,
                outward,
                mode,
                join,
                resolve_intersections,
                allow_self_intersections,
            ),
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces,
                thickness,
                side,
            }) => self.encode_thicken(faces, thickness, side),
            FeatureDefinition::Operation(FeatureOperation::OffsetSurface { faces, distance }) => {
                self.encode_offset_surface(faces, distance)
            }
            FeatureDefinition::Operation(FeatureOperation::KnitSurface {
                faces,
                merge_entities,
                create_solid,
                gap_tolerance,
            }) => self.encode_knit_surface(faces, merge_entities, create_solid, gap_tolerance),
            FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                boundary,
                support_faces,
                continuity,
                merge_result,
            }) => self.encode_filled_surface(boundary, support_faces, continuity, merge_result),
            FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: face_selection,
                anchor,
                angle,
                outward,
            }) => self.encode_draft(face_selection, anchor, angle, outward),
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands,

                op,
                keep_tools,
            }) => {
                let target = operands.target();
                let tools = operands.tools();
                self.encode_combine(target, tools, op, keep_tools)
            }
            FeatureDefinition::Operation(FeatureOperation::CutWithSurface {
                targets,
                tools,
                reverse,
            }) => self.encode_cut_with_surface(targets, tools, reverse),
            FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, mode }) => {
                self.encode_delete_body(bodies, mode)
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteFace { faces, heal }) => {
                self.encode_delete_face(faces, heal)
            }
            FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }) => {
                let targets = operands.targets();
                let replacements = operands.replacements();
                self.encode_replace_face(targets, replacements)
            }
            FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, motion }) => {
                self.encode_move_face(faces, motion)
            }
            FeatureDefinition::Operation(FeatureOperation::MoveBody {
                bodies,
                translation,
                rotation,
                copies,
            }) => self.encode_move_body(bodies, translation, rotation, copies),
            FeatureDefinition::Operation(FeatureOperation::Dome {
                faces,
                height,
                elliptical,
                reverse,
            }) => self.encode_dome(faces, height, elliptical, reverse),
            FeatureDefinition::Operation(FeatureOperation::Flex { axis, mode }) => {
                self.encode_flex(axis, mode)
            }
            FeatureDefinition::Operation(FeatureOperation::Scale {
                bodies,
                center,
                factors,
            }) => self.encode_scale(bodies, center, factors),
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile,
                profile_filter,
                face,
                direction: _,
                placements,
                shape,

                extent,
                bottom,
                taper_angle,
                allow_multi_profile_faces,
            }) => {
                let construction = shape.construction();
                let exit_kind = shape.exit_kind();
                let diameter = &shape.diameter();
                self.encode_hole(
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
                )
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { construction, op }) => {
                self.encode_revolve(construction, op)
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,

                path,

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
            }) => {
                let section = shape.section();
                let sections = shape.sections();
                let mode = &shape.mode();
                self.encode_sweep(
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
                )
            }
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections,
                guidance,
                op,
                closed,
                solid,
                ruled,
                linearize,
                max_degree,
                allow_multi_profile_faces,
            }) => self.encode_loft(
                sections,
                guidance,
                op,
                closed,
                solid,
                ruled,
                linearize,
                max_degree,
                allow_multi_profile_faces,
            ),
            FeatureDefinition::Operation(FeatureOperation::Rib { construction, op }) => {
                self.encode_rib(construction, op)
            }
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) => {
                self.encode_pattern(seeds, pattern)
            }
            FeatureDefinition::Operation(FeatureOperation::HelicalSweep { .. }) => {
                self.encode_helical_sweep()
            }
            FeatureDefinition::Operation(FeatureOperation::Binder { .. }) => self.encode_binder(),
            FeatureDefinition::Operation(
                FeatureOperation::Unresolved {
                    family:
                        UnresolvedFamily::DatumPoint
                        | UnresolvedFamily::DatumCoordinateSystem
                        | UnresolvedFamily::Loft
                        | UnresolvedFamily::FreeformSurface
                        | UnresolvedFamily::Draft,
                }
                | FeatureOperation::Block { .. }
                | FeatureOperation::ExtractBody { .. }
                | FeatureOperation::FaceBlend { .. }
                | FeatureOperation::SewBodies { .. }
                | FeatureOperation::TrimBodies { .. },
            ) => self.encode_explicitly_unsupported(),
            FeatureDefinition::Operation(_) => self.encode_unsupported(),
        }
    }
}
