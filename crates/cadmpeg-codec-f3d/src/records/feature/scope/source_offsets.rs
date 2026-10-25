// SPDX-License-Identifier: Apache-2.0
//! Source offsets of a parameter scope as the pointers of its capture.
//!
//! A pointer has the name of the field that held the offset in the scope's
//! payload. The scope frame length is the capture extent, not a pointer.

use super::{
    DesignBaseFlangeScope, DesignExtrudeScope, DesignJointOriginTransform, DesignParameterScope,
    DesignScopePayload, DesignSketchEntityBinding, DesignSweepScope, DesignWorkPlaneTransform,
};
use crate::records::feature::surface_ops::DesignSurfacePatchScope;
use crate::records::source_offsets::{OffsetVisitor, SourceOffsets};

impl SourceOffsets for DesignParameterScope {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        let source = &mut self.source;
        offsets.offset("byte_offset", &mut source.byte_offset);
        offsets.offset("kind_offset", &mut source.kind_offset);
        offsets.offset(
            "history_state_id_offset",
            &mut source.history_state_id_offset,
        );
        offsets.offset("feature_ordinal_offset", &mut source.feature_ordinal_offset);
        offsets.optional_offset(
            "previous_history_state_id_offset",
            &mut source.previous_history_state_id_offset,
        );
        offsets.offset("reference_count_offset", &mut source.reference_count_offset);
        offsets.offset("paired_byte_offset", &mut source.paired_byte_offset);
        offsets.reference_run("reference_member_offsets", &mut self.reference_members);
        self.payload.visit_source_offsets(offsets);
    }
}

impl SourceOffsets for DesignSketchEntityBinding {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.offset("entity_reference_offset", &mut self.entity_reference_offset);
    }
}

impl SourceOffsets for DesignExtrudeScope {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.optional("extrude_prologue", &mut self.extrude_prologue);
        offsets.optional(
            "fixed_extrude_parameters",
            &mut self.fixed_extrude_parameters,
        );
        offsets.optional("extrude_profile", &mut self.extrude_profile);
    }
}

impl SourceOffsets for DesignSweepScope {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.optional("path_feature_construction", &mut self.construction);
        offsets.optional("sweep_profile", &mut self.sweep_profile);
    }
}

impl SourceOffsets for DesignBaseFlangeScope {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.optional("base_flange_operation", &mut self.base_flange_operation);
        offsets.optional("base_flange_profile", &mut self.base_flange_profile);
    }
}

impl SourceOffsets for DesignSurfacePatchScope {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.optional("surface_patch_normal", &mut self.normal);
    }
}

impl SourceOffsets for DesignWorkPlaneTransform {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.offset(
            "work_plane_transform_offset",
            &mut self.work_plane_transform_offset,
        );
        if let Some(reference) = &mut self.reference {
            offsets.offset(
                "work_plane_reference_offset",
                &mut reference.work_plane_reference_offset,
            );
        }
        offsets.optional("work_plane_construction", &mut self.work_plane_construction);
    }
}

impl SourceOffsets for DesignJointOriginTransform {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.offset(
            "joint_origin_transform_offset",
            &mut self.joint_origin_transform_offset,
        );
        if let Some(reference) = &mut self.reference {
            offsets.offset(
                "joint_origin_reference_offset",
                &mut reference.joint_origin_reference_offset,
            );
        }
    }
}

impl SourceOffsets for DesignScopePayload {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        match self {
            Self::Sketch(value)
            | Self::Esquisse(value)
            | Self::Skizze(value)
            | Self::Esboco(value) => {
                if let Some(value) = value {
                    value.visit_source_offsets(offsets);
                }
            }
            Self::Assemble(value) | Self::AsBuilt(value) => {
                offsets.optional("assembly_alignment", value);
            }
            Self::Extrude(value) | Self::Extrusion(value) | Self::Extrusao(value) => {
                if let Some(value) = value {
                    value.visit_source_offsets(offsets);
                }
            }
            Self::Fillet(value)
            | Self::Conge(value)
            | Self::Abrundung(value)
            | Self::Arredondamento(value) => {
                offsets.optional("fixed_fillet_parameters", value);
            }
            Self::Chamfer(value) | Self::Chanfrein(value) => {
                offsets.optional("fixed_chamfer_parameters", value);
            }
            Self::Combine(value) => offsets.optional("combine_operation", value),
            Self::Draft(value) => offsets.optional("draft_operation", value),
            Self::CPattern(value) | Self::CircularPattern(value) | Self::ReseauC(value) => {
                offsets.optional("circular_pattern_construction", value);
            }
            Self::RPattern(value) | Self::RectangularPattern(value) => {
                offsets.optional("rectangular_pattern_construction", value);
            }
            Self::Mirror(value) | Self::SymetrieMiroir(value) => {
                offsets.optional("mirror_construction", value);
            }
            Self::Move(value) => offsets.optional("move_operation", value),
            Self::OffsetFaces(value) | Self::DecalerLesFaces(value) => {
                offsets.optional("direct_face_operation", value);
            }
            Self::Shell(value) | Self::Schale(value) => {
                offsets.optional("direct_face_operation", value);
            }
            Self::Thicken(value) => offsets.optional("direct_face_operation", value),
            Self::SpirePrimitive(value) | Self::CoilPrimitive(value) => {
                if let Some(value) = value {
                    value.visit_source_offsets(offsets);
                }
            }
            Self::Sweep(value) => {
                if let Some(value) = value {
                    value.visit_source_offsets(offsets);
                }
            }
            Self::SurfacePatch(value) => value.visit_source_offsets(offsets),
            Self::SurfaceExtend(value) => offsets.optional("surface_extend_operation", value),
            Self::SurfaceOffset(value) => offsets.optional("surface_offset_operation", value),
            Self::Hole(value) => offsets.optional("hole_construction", value),
            Self::Scale(value) | Self::Massstab(value) => {
                offsets.optional("scale_operation", value);
            }
            Self::Thread(value) => offsets.optional("thread_construction", value),
            Self::EdgeFlange(value) => offsets.optional("edge_flange_operation", value),
            Self::Hem(value) => offsets.optional("hem_operation", value),
            Self::BaseFlange(value) => {
                if let Some(value) = value {
                    value.visit_source_offsets(offsets);
                }
            }
            Self::ComponentInsert(value) => {
                offsets.optional("component_insert_construction", value);
            }
            Self::CopyPaste(value) => offsets.optional("copy_paste_component_operation", value),
            Self::JointOrigin(value) => {
                if let Some(value) = value {
                    value.visit_source_offsets(offsets);
                }
            }
            Self::WorkPlane(value) => {
                if let Some(value) = value {
                    value.visit_source_offsets(offsets);
                }
            }
            Self::WorkAxis(value) => offsets.optional("work_axis_construction", value),
            Self::WorkPoint(value) => offsets.optional("work_point_construction", value),
            Self::DerivedInstance(value) => {
                offsets.optional("derived_instance_construction", value);
            }
            Self::BaseFeature(value) => offsets.optional("base_feature_construction", value),
            Self::CopyPasteBodies(value) => offsets.optional("copy_paste_bodies_operation", value),
            Self::Revolve(value) => offsets.optional("path_feature_construction", value),
            Self::Loft(value) => offsets.optional("path_feature_construction", value),
            Self::Pipe(value) => offsets.optional("path_feature_construction", value),
            Self::SpherePrimitive(value) => offsets.optional("solid_primitive", value),
            Self::TorusPrimitive(value) => offsets.optional("solid_primitive", value),
            Self::BoxPrimitive(value) => offsets.optional("solid_primitive", value),
            Self::CylinderPrimitive(value) => offsets.optional("solid_primitive", value),
            Self::SurfaceStitch(value) => offsets.record("surface_stitch_operation", value),
            Self::SurfaceRuled(value) => offsets.record("ruled_surface_operation", value),
            Self::ReplaceFace
            | Self::SurfaceTrim
            | Self::BoundaryFill
            | Self::Split
            | Self::Canvas
            | Self::Decal
            | Self::BaseMeshFeature
            | Self::CustomFeature
            | Self::Form
            | Self::SplitFace
            | Self::DeleteFace
            | Self::SurfaceDeleteFace
            | Self::RemoveBody
            | Self::Face
            | Self::Native(_) => {}
        }
    }
}
