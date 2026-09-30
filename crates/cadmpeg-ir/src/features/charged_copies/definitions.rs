// SPDX-License-Identifier: Apache-2.0
//! Caller-admitted copies of feature definitions.

use super::FeatureCopy;
use super::super::{ConfigurationEvaluation, ConfigurationFeatureState, Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

impl FeatureCopy for ConfigurationEvaluation {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Suppressed {  } => Ok(Self::Suppressed {  }),
            Self::Active { outputs } => Ok(Self::Active { outputs: outputs.copy_feature(ctx, purpose)? }),
        }
    }
}

impl FeatureCopy for ConfigurationFeatureState {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { evaluation: self.evaluation.copy_feature(ctx, purpose)?, dependencies: self.dependencies.copy_feature(ctx, purpose)?, definition: self.definition.copy_feature(ctx, purpose)? })
    }
}

impl FeatureCopy for Feature {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { id: self.id.copy_feature(ctx, purpose)?, ordinal: self.ordinal.copy_feature(ctx, purpose)?, name: self.name.copy_feature(ctx, purpose)?, suppressed: self.suppressed.copy_feature(ctx, purpose)?, dependencies: self.dependencies.copy_feature(ctx, purpose)?, source_properties: self.source_properties.copy_feature(ctx, purpose)?, source_tag: self.source_tag.copy_feature(ctx, purpose)?, source_text: self.source_text.copy_feature(ctx, purpose)?, source_content: self.source_content.copy_feature(ctx, purpose)?, evaluation: self.evaluation.copy_feature(ctx, purpose)?, native_ref: self.native_ref.copy_feature(ctx, purpose)? })
    }
}

impl FeatureCopy for FeatureDefinition {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::PostProcess { operation, refine, fuzzy_tolerance } => Ok(Self::PostProcess { operation: operation.copy_feature(ctx, purpose)?, refine: refine.copy_feature(ctx, purpose)?, fuzzy_tolerance: fuzzy_tolerance.copy_feature(ctx, purpose)? }),
            Self::Operation(value_0) => Ok(Self::Operation(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for FeatureEvaluation {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { definition: self.definition.copy_feature(ctx, purpose)?, outputs: self.outputs.copy_feature(ctx, purpose)? })
    }
}

impl FeatureCopy for FeatureOperation {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::TreeNode { role, children } => Ok(Self::TreeNode { role: role.copy_feature(ctx, purpose)?, children: children.copy_feature(ctx, purpose)? }),
            Self::BaseFeature { bodies } => Ok(Self::BaseFeature { bodies: bodies.copy_feature(ctx, purpose)? }),
            Self::MeshImport { tessellations } => Ok(Self::MeshImport { tessellations: tessellations.copy_feature(ctx, purpose)? }),
            Self::InsertBodies { bodies } => Ok(Self::InsertBodies { bodies: bodies.copy_feature(ctx, purpose)? }),
            Self::InsertComponent { occurrence } => Ok(Self::InsertComponent { occurrence: occurrence.copy_feature(ctx, purpose)? }),
            Self::AssemblyJoint { joint } => Ok(Self::AssemblyJoint { joint: joint.copy_feature(ctx, purpose)? }),
            Self::Form { cages } => Ok(Self::Form { cages: cages.copy_feature(ctx, purpose)? }),
            Self::CosmeticThread { face, diameter, extent } => Ok(Self::CosmeticThread { face: face.copy_feature(ctx, purpose)?, diameter: diameter.copy_feature(ctx, purpose)?, extent: extent.copy_feature(ctx, purpose)? }),
            Self::ReferenceImage { asset, visible, mirror_u, mirror_v, frame, bounds, opacity } => Ok(Self::ReferenceImage { asset: asset.copy_feature(ctx, purpose)?, visible: visible.copy_feature(ctx, purpose)?, mirror_u: mirror_u.copy_feature(ctx, purpose)?, mirror_v: mirror_v.copy_feature(ctx, purpose)?, frame: frame.copy_feature(ctx, purpose)?, bounds: bounds.copy_feature(ctx, purpose)?, opacity: opacity.copy_feature(ctx, purpose)? }),
            Self::Decal { asset, faces, mapping, opacity } => Ok(Self::Decal { asset: asset.copy_feature(ctx, purpose)?, faces: faces.copy_feature(ctx, purpose)?, mapping: mapping.copy_feature(ctx, purpose)?, opacity: opacity.copy_feature(ctx, purpose)? }),
            Self::DatumPrincipalPlane { plane } => Ok(Self::DatumPrincipalPlane { plane: plane.copy_feature(ctx, purpose)? }),
            Self::DatumPlane { frame } => Ok(Self::DatumPlane { frame: frame.copy_feature(ctx, purpose)? }),
            Self::DatumThreePointPlane { frame, points } => Ok(Self::DatumThreePointPlane { frame: frame.copy_feature(ctx, purpose)?, points: points.copy_feature(ctx, purpose)? }),
            Self::DatumOffsetPlane { reference, distance } => Ok(Self::DatumOffsetPlane { reference: reference.copy_feature(ctx, purpose)?, distance: distance.copy_feature(ctx, purpose)? }),
            Self::DatumAxis { origin, direction } => Ok(Self::DatumAxis { origin: origin.copy_feature(ctx, purpose)?, direction: direction.copy_feature(ctx, purpose)? }),
            Self::DatumPoint { position, construction } => Ok(Self::DatumPoint { position: position.copy_feature(ctx, purpose)?, construction: construction.copy_feature(ctx, purpose)? }),
            Self::PointGeometry { position } => Ok(Self::PointGeometry { position: position.copy_feature(ctx, purpose)? }),
            Self::LineSegment { segment } => Ok(Self::LineSegment { segment: segment.copy_feature(ctx, purpose)? }),
            Self::CircularArc { arc } => Ok(Self::CircularArc { arc: arc.copy_feature(ctx, purpose)? }),
            Self::EllipticArc { arc } => Ok(Self::EllipticArc { arc: arc.copy_feature(ctx, purpose)? }),
            Self::Polyline { chain } => Ok(Self::Polyline { chain: chain.copy_feature(ctx, purpose)? }),
            Self::RegularPolygonCurve { sides, circumradius } => Ok(Self::RegularPolygonCurve { sides: sides.copy_feature(ctx, purpose)?, circumradius: circumradius.copy_feature(ctx, purpose)? }),
            Self::PlanarPatch { length, width } => Ok(Self::PlanarPatch { length: length.copy_feature(ctx, purpose)?, width: width.copy_feature(ctx, purpose)? }),
            Self::FaceFromShapes { sources, face_maker } => Ok(Self::FaceFromShapes { sources: sources.copy_feature(ctx, purpose)?, face_maker: face_maker.copy_feature(ctx, purpose)? }),
            Self::DatumCoordinateSystem { frame } => Ok(Self::DatumCoordinateSystem { frame: frame.copy_feature(ctx, purpose)? }),
            Self::Block { dimensions, placement, op } => Ok(Self::Block { dimensions: dimensions.copy_feature(ctx, purpose)?, placement: placement.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)? }),
            Self::EquationCurve { curve } => Ok(Self::EquationCurve { curve: curve.copy_feature(ctx, purpose)? }),
            Self::ProjectedCurve { source, target_faces, direction, bidirectional } => Ok(Self::ProjectedCurve { source: source.copy_feature(ctx, purpose)?, target_faces: target_faces.copy_feature(ctx, purpose)?, direction: direction.copy_feature(ctx, purpose)?, bidirectional: bidirectional.copy_feature(ctx, purpose)? }),
            Self::ProjectOnSurface { sources, support_face, direction, mode, height, offset } => Ok(Self::ProjectOnSurface { sources: sources.copy_feature(ctx, purpose)?, support_face: support_face.copy_feature(ctx, purpose)?, direction: direction.copy_feature(ctx, purpose)?, mode: mode.copy_feature(ctx, purpose)?, height: height.copy_feature(ctx, purpose)?, offset: offset.copy_feature(ctx, purpose)? }),
            Self::CompositeCurve { segments, closed } => Ok(Self::CompositeCurve { segments: segments.copy_feature(ctx, purpose)?, closed: closed.copy_feature(ctx, purpose)? }),
            Self::Helix { axis_origin, axis_direction, radius, shape, revolutions, start_angle, clockwise, segment_turns, construction_style } => Ok(Self::Helix { axis_origin: axis_origin.copy_feature(ctx, purpose)?, axis_direction: axis_direction.copy_feature(ctx, purpose)?, radius: radius.copy_feature(ctx, purpose)?, shape: shape.copy_feature(ctx, purpose)?, revolutions: revolutions.copy_feature(ctx, purpose)?, start_angle: start_angle.copy_feature(ctx, purpose)?, clockwise: clockwise.copy_feature(ctx, purpose)?, segment_turns: segment_turns.copy_feature(ctx, purpose)?, construction_style: construction_style.copy_feature(ctx, purpose)? }),
            Self::HelixNativeAxis { axis_native_ref, axial_rise, pitch, revolutions, start_angle, clockwise } => Ok(Self::HelixNativeAxis { axis_native_ref: axis_native_ref.copy_feature(ctx, purpose)?, axial_rise: axial_rise.copy_feature(ctx, purpose)?, pitch: pitch.copy_feature(ctx, purpose)?, revolutions: revolutions.copy_feature(ctx, purpose)?, start_angle: start_angle.copy_feature(ctx, purpose)?, clockwise: clockwise.copy_feature(ctx, purpose)? }),
            Self::Coil { construction, result } => Ok(Self::Coil { construction: construction.copy_feature(ctx, purpose)?, result: result.copy_feature(ctx, purpose)? }),
            Self::Sphere { center, radius, op } => Ok(Self::Sphere { center: center.copy_feature(ctx, purpose)?, radius: radius.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)? }),
            Self::Torus { center, axis, major_radius, minor_radius, op } => Ok(Self::Torus { center: center.copy_feature(ctx, purpose)?, axis: axis.copy_feature(ctx, purpose)?, major_radius: major_radius.copy_feature(ctx, purpose)?, minor_radius: minor_radius.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)? }),
            Self::Wrap { profile, face, mode } => Ok(Self::Wrap { profile: profile.copy_feature(ctx, purpose)?, face: face.copy_feature(ctx, purpose)?, mode: mode.copy_feature(ctx, purpose)? }),
            Self::Sketch { sketch } => Ok(Self::Sketch { sketch: sketch.copy_feature(ctx, purpose)? }),
            Self::SpatialSketch { sketch } => Ok(Self::SpatialSketch { sketch: sketch.copy_feature(ctx, purpose)? }),
            Self::SketchBlockDefinition { sketch } => Ok(Self::SketchBlockDefinition { sketch: sketch.copy_feature(ctx, purpose)? }),
            Self::SketchBlockInstance { block, placement } => Ok(Self::SketchBlockInstance { block: block.copy_feature(ctx, purpose)?, placement: placement.copy_feature(ctx, purpose)? }),
            Self::StoredGeometry {  } => Ok(Self::StoredGeometry {  }),
            Self::ExtractBody { source } => Ok(Self::ExtractBody { source: source.copy_feature(ctx, purpose)? }),
            Self::DerivedGeometry { source } => Ok(Self::DerivedGeometry { source: source.copy_feature(ctx, purpose)? }),
            Self::ImportedGeometry { path, format } => Ok(Self::ImportedGeometry { path: path.copy_feature(ctx, purpose)?, format: format.copy_feature(ctx, purpose)? }),
            Self::Primitive { solid, op } => Ok(Self::Primitive { solid: solid.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)? }),
            Self::Revolve { construction, op } => Ok(Self::Revolve { construction: construction.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)? }),
            Self::Sweep { shape, path, orientation, transition, transformation, path_tangent, linearize, twist, path_extent, guide_rail, taper, scale, allow_multi_profile_faces } => Ok(Self::Sweep { shape: shape.copy_feature(ctx, purpose)?, path: path.copy_feature(ctx, purpose)?, orientation: orientation.copy_feature(ctx, purpose)?, transition: transition.copy_feature(ctx, purpose)?, transformation: transformation.copy_feature(ctx, purpose)?, path_tangent: path_tangent.copy_feature(ctx, purpose)?, linearize: linearize.copy_feature(ctx, purpose)?, twist: twist.copy_feature(ctx, purpose)?, path_extent: path_extent.copy_feature(ctx, purpose)?, guide_rail: guide_rail.copy_feature(ctx, purpose)?, taper: taper.copy_feature(ctx, purpose)?, scale: scale.copy_feature(ctx, purpose)?, allow_multi_profile_faces: allow_multi_profile_faces.copy_feature(ctx, purpose)? }),
            Self::HelicalSweep { construction, op } => Ok(Self::HelicalSweep { construction: construction.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)? }),
            Self::Binder { sources, construction } => Ok(Self::Binder { sources: sources.copy_feature(ctx, purpose)?, construction: construction.copy_feature(ctx, purpose)? }),
            Self::Rib { construction, op } => Ok(Self::Rib { construction: construction.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)? }),
            Self::SheetMetalBaseFlange { profile, thickness, side } => Ok(Self::SheetMetalBaseFlange { profile: profile.copy_feature(ctx, purpose)?, thickness: thickness.copy_feature(ctx, purpose)?, side: side.copy_feature(ctx, purpose)? }),
            Self::SheetMetalEdgeFlange { edges, height, angle, height_datum, bend_position, width, bend_radius } => Ok(Self::SheetMetalEdgeFlange { edges: edges.copy_feature(ctx, purpose)?, height: height.copy_feature(ctx, purpose)?, angle: angle.copy_feature(ctx, purpose)?, height_datum: height_datum.copy_feature(ctx, purpose)?, bend_position: bend_position.copy_feature(ctx, purpose)?, width: width.copy_feature(ctx, purpose)?, bend_radius: bend_radius.copy_feature(ctx, purpose)? }),
            Self::SheetMetalHem { edges, form, direction, bend_radius } => Ok(Self::SheetMetalHem { edges: edges.copy_feature(ctx, purpose)?, form: form.copy_feature(ctx, purpose)?, direction: direction.copy_feature(ctx, purpose)?, bend_radius: bend_radius.copy_feature(ctx, purpose)? }),
            Self::Fillet { groups } => Ok(Self::Fillet { groups: groups.copy_feature(ctx, purpose)? }),
            Self::FullRoundFillet { groups } => Ok(Self::FullRoundFillet { groups: groups.copy_feature(ctx, purpose)? }),
            Self::FaceBlend { operands, radius } => Ok(Self::FaceBlend { operands: operands.copy_feature(ctx, purpose)?, radius: radius.copy_feature(ctx, purpose)? }),
            Self::Chamfer { groups, flip_direction } => Ok(Self::Chamfer { groups: groups.copy_feature(ctx, purpose)?, flip_direction: flip_direction.copy_feature(ctx, purpose)? }),
            Self::Shell { bodies, removed_faces, thickness, outward, mode, join, resolve_intersections, allow_self_intersections } => Ok(Self::Shell { bodies: bodies.copy_feature(ctx, purpose)?, removed_faces: removed_faces.copy_feature(ctx, purpose)?, thickness: thickness.copy_feature(ctx, purpose)?, outward: outward.copy_feature(ctx, purpose)?, mode: mode.copy_feature(ctx, purpose)?, join: join.copy_feature(ctx, purpose)?, resolve_intersections: resolve_intersections.copy_feature(ctx, purpose)?, allow_self_intersections: allow_self_intersections.copy_feature(ctx, purpose)? }),
            Self::OffsetShape { source, distance, mode, join, resolve_intersections, allow_self_intersections, fill, planar } => Ok(Self::OffsetShape { source: source.copy_feature(ctx, purpose)?, distance: distance.copy_feature(ctx, purpose)?, mode: mode.copy_feature(ctx, purpose)?, join: join.copy_feature(ctx, purpose)?, resolve_intersections: resolve_intersections.copy_feature(ctx, purpose)?, allow_self_intersections: allow_self_intersections.copy_feature(ctx, purpose)?, fill: fill.copy_feature(ctx, purpose)?, planar: planar.copy_feature(ctx, purpose)? }),
            Self::Compound { members } => Ok(Self::Compound { members: members.copy_feature(ctx, purpose)? }),
            Self::RefineShape { source } => Ok(Self::RefineShape { source: source.copy_feature(ctx, purpose)? }),
            Self::ReverseShape { source } => Ok(Self::ReverseShape { source: source.copy_feature(ctx, purpose)? }),
            Self::RuledBetweenCurves { first, second, orientation } => Ok(Self::RuledBetweenCurves { first: first.copy_feature(ctx, purpose)?, second: second.copy_feature(ctx, purpose)?, orientation: orientation.copy_feature(ctx, purpose)? }),
            Self::SectionShape { operands, approximate } => Ok(Self::SectionShape { operands: operands.copy_feature(ctx, purpose)?, approximate: approximate.copy_feature(ctx, purpose)? }),
            Self::MirrorShape { source, plane_origin, plane_normal, plane_reference } => Ok(Self::MirrorShape { source: source.copy_feature(ctx, purpose)?, plane_origin: plane_origin.copy_feature(ctx, purpose)?, plane_normal: plane_normal.copy_feature(ctx, purpose)?, plane_reference: plane_reference.copy_feature(ctx, purpose)? }),
            Self::Thicken { faces, thickness, side } => Ok(Self::Thicken { faces: faces.copy_feature(ctx, purpose)?, thickness: thickness.copy_feature(ctx, purpose)?, side: side.copy_feature(ctx, purpose)? }),
            Self::OffsetSurface { faces, distance } => Ok(Self::OffsetSurface { faces: faces.copy_feature(ctx, purpose)?, distance: distance.copy_feature(ctx, purpose)? }),
            Self::KnitSurface { faces, merge_entities, create_solid, gap_tolerance } => Ok(Self::KnitSurface { faces: faces.copy_feature(ctx, purpose)?, merge_entities: merge_entities.copy_feature(ctx, purpose)?, create_solid: create_solid.copy_feature(ctx, purpose)?, gap_tolerance: gap_tolerance.copy_feature(ctx, purpose)? }),
            Self::SewBodies { bodies, gap_tolerance } => Ok(Self::SewBodies { bodies: bodies.copy_feature(ctx, purpose)?, gap_tolerance: gap_tolerance.copy_feature(ctx, purpose)? }),
            Self::FilledSurface { boundary, support_faces, continuity, merge_result } => Ok(Self::FilledSurface { boundary: boundary.copy_feature(ctx, purpose)?, support_faces: support_faces.copy_feature(ctx, purpose)?, continuity: continuity.copy_feature(ctx, purpose)?, merge_result: merge_result.copy_feature(ctx, purpose)? }),
            Self::TrimSurface { faces, tool, keep } => Ok(Self::TrimSurface { faces: faces.copy_feature(ctx, purpose)?, tool: tool.copy_feature(ctx, purpose)?, keep: keep.copy_feature(ctx, purpose)? }),
            Self::ExtendSurface { faces, distance, method } => Ok(Self::ExtendSurface { faces: faces.copy_feature(ctx, purpose)?, distance: distance.copy_feature(ctx, purpose)?, method: method.copy_feature(ctx, purpose)? }),
            Self::RuledSurface { edges, support_faces, mode, angle, alternate_face, corner } => Ok(Self::RuledSurface { edges: edges.copy_feature(ctx, purpose)?, support_faces: support_faces.copy_feature(ctx, purpose)?, mode: mode.copy_feature(ctx, purpose)?, angle: angle.copy_feature(ctx, purpose)?, alternate_face: alternate_face.copy_feature(ctx, purpose)?, corner: corner.copy_feature(ctx, purpose)? }),
            Self::Draft { faces, anchor, angle, outward } => Ok(Self::Draft { faces: faces.copy_feature(ctx, purpose)?, anchor: anchor.copy_feature(ctx, purpose)?, angle: angle.copy_feature(ctx, purpose)?, outward: outward.copy_feature(ctx, purpose)? }),
            Self::Combine { operands, op, keep_tools } => Ok(Self::Combine { operands: operands.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)?, keep_tools: keep_tools.copy_feature(ctx, purpose)? }),
            Self::BoundaryFill { tools, cells } => Ok(Self::BoundaryFill { tools: tools.copy_feature(ctx, purpose)?, cells: cells.copy_feature(ctx, purpose)? }),
            Self::CutWithSurface { targets, tools, reverse } => Ok(Self::CutWithSurface { targets: targets.copy_feature(ctx, purpose)?, tools: tools.copy_feature(ctx, purpose)?, reverse: reverse.copy_feature(ctx, purpose)? }),
            Self::TrimBodies { operands, keep } => Ok(Self::TrimBodies { operands: operands.copy_feature(ctx, purpose)?, keep: keep.copy_feature(ctx, purpose)? }),
            Self::SplitBody { targets, tools } => Ok(Self::SplitBody { targets: targets.copy_feature(ctx, purpose)?, tools: tools.copy_feature(ctx, purpose)? }),
            Self::SplitFace { targets, tool } => Ok(Self::SplitFace { targets: targets.copy_feature(ctx, purpose)?, tool: tool.copy_feature(ctx, purpose)? }),
            Self::DeleteBody { bodies, mode } => Ok(Self::DeleteBody { bodies: bodies.copy_feature(ctx, purpose)?, mode: mode.copy_feature(ctx, purpose)? }),
            Self::DeleteFace { faces, heal } => Ok(Self::DeleteFace { faces: faces.copy_feature(ctx, purpose)?, heal: heal.copy_feature(ctx, purpose)? }),
            Self::ReplaceFace { operands } => Ok(Self::ReplaceFace { operands: operands.copy_feature(ctx, purpose)? }),
            Self::MoveFace { faces, motion } => Ok(Self::MoveFace { faces: faces.copy_feature(ctx, purpose)?, motion: motion.copy_feature(ctx, purpose)? }),
            Self::MoveBody { bodies, translation, rotation, copies } => Ok(Self::MoveBody { bodies: bodies.copy_feature(ctx, purpose)?, translation: translation.copy_feature(ctx, purpose)?, rotation: rotation.copy_feature(ctx, purpose)?, copies: copies.copy_feature(ctx, purpose)? }),
            Self::Dome { faces, height, elliptical, reverse } => Ok(Self::Dome { faces: faces.copy_feature(ctx, purpose)?, height: height.copy_feature(ctx, purpose)?, elliptical: elliptical.copy_feature(ctx, purpose)?, reverse: reverse.copy_feature(ctx, purpose)? }),
            Self::Flex { axis, mode } => Ok(Self::Flex { axis: axis.copy_feature(ctx, purpose)?, mode: mode.copy_feature(ctx, purpose)? }),
            Self::Scale { bodies, center, factors } => Ok(Self::Scale { bodies: bodies.copy_feature(ctx, purpose)?, center: center.copy_feature(ctx, purpose)?, factors: factors.copy_feature(ctx, purpose)? }),
            Self::Hole { profile, profile_filter, face, direction, placements, shape, extent, bottom, taper_angle, allow_multi_profile_faces } => Ok(Self::Hole { profile: profile.copy_feature(ctx, purpose)?, profile_filter: profile_filter.copy_feature(ctx, purpose)?, face: face.copy_feature(ctx, purpose)?, direction: direction.copy_feature(ctx, purpose)?, placements: placements.copy_feature(ctx, purpose)?, shape: shape.copy_feature(ctx, purpose)?, extent: extent.copy_feature(ctx, purpose)?, bottom: bottom.copy_feature(ctx, purpose)?, taper_angle: taper_angle.copy_feature(ctx, purpose)?, allow_multi_profile_faces: allow_multi_profile_faces.copy_feature(ctx, purpose)? }),
            Self::Pattern { seeds, pattern } => Ok(Self::Pattern { seeds: seeds.copy_feature(ctx, purpose)?, pattern: pattern.copy_feature(ctx, purpose)? }),
            Self::Unresolved { family } => Ok(Self::Unresolved { family: family.copy_feature(ctx, purpose)? }),
            Self::Native { kind, parameters } => Ok(Self::Native { kind: kind.copy_feature(ctx, purpose)?, parameters: parameters.copy_feature(ctx, purpose)? }),
            Self::Extrude { profile, direction, start, extent, op, solid, face_maker, inner_wire_taper, length_along_profile_normal, allow_multi_profile_faces } => Ok(Self::Extrude { profile: profile.copy_feature(ctx, purpose)?, direction: direction.copy_feature(ctx, purpose)?, start: start.copy_feature(ctx, purpose)?, extent: extent.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)?, solid: solid.copy_feature(ctx, purpose)?, face_maker: face_maker.copy_feature(ctx, purpose)?, inner_wire_taper: inner_wire_taper.copy_feature(ctx, purpose)?, length_along_profile_normal: length_along_profile_normal.copy_feature(ctx, purpose)?, allow_multi_profile_faces: allow_multi_profile_faces.copy_feature(ctx, purpose)? }),
            Self::Loft { sections, guidance, op, closed, solid, ruled, linearize, max_degree, allow_multi_profile_faces } => Ok(Self::Loft { sections: sections.copy_feature(ctx, purpose)?, guidance: guidance.copy_feature(ctx, purpose)?, op: op.copy_feature(ctx, purpose)?, closed: closed.copy_feature(ctx, purpose)?, solid: solid.copy_feature(ctx, purpose)?, ruled: ruled.copy_feature(ctx, purpose)?, linearize: linearize.copy_feature(ctx, purpose)?, max_degree: max_degree.copy_feature(ctx, purpose)?, allow_multi_profile_faces: allow_multi_profile_faces.copy_feature(ctx, purpose)? }),
        }
    }
}

