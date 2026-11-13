// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignPatchContinuity, DesignRuledSurfaceCorner, DesignRuledSurfaceMethod,
    DesignRuledSurfaceOperation, DesignSurfaceExtendOperation, DesignSurfaceOffsetOperation,
    DesignSurfaceOffsetSupport, DesignSurfacePatchBoundary, DesignSurfacePatchNormalState,
    DesignSurfacePatchScope, DesignSurfaceStitchOperation, DesignSurfaceUntrimLoops,
    DesignSurfaceUntrimOperation,
};

rewrite_native_scalar!(DesignPatchContinuity);
rewrite_native_scalar!(DesignRuledSurfaceCorner);
rewrite_native_scalar!(DesignRuledSurfaceMethod);
rewrite_native_record!(DesignRuledSurfaceOperation, []; {method, method_offset, corner, corner_offset, alternate_face, alternate_face_offset, angle_owner_record_index, distance_owner_record_index, edge_group_record_indices, auxiliary_record_indices, direction_entity_id});
rewrite_native_record!(DesignSurfaceExtendOperation, []; {distance, distance_offset, distance_record_index, boundary_record_index, boundary_reference_record_index, boundary_reference_offset, edge_record_indices, tolerance, tolerance_offset});
rewrite_native_record!(DesignSurfaceOffsetOperation, []; {distance, distance_offset, distance_record_index, support});
rewrite_native_scalar!(DesignSurfaceUntrimLoops);
rewrite_native_record!(DesignSurfaceUntrimOperation, []; {loops, loops_offset, extension, extension_offset, extension_record_index, face_group_record_index, loop_group_record_indices, padded});
rewrite_native_enum!(DesignSurfaceOffsetSupport, []; {
    BoundaryCarrier {boundary_record_index, boundary_reference_record_index, boundary_reference_offset, edge_record_indices, tolerance, tolerance_offset},
    FaceGroups {group_record_indices},
});
rewrite_native_record!(DesignSurfacePatchBoundary, []; {scope_reference_ordinal, record_index, is_seed_selection, continuity, flip, scale, model_reference});
rewrite_native_record!(DesignSurfaceStitchOperation, []; {gap_tolerance, gap_tolerance_offset, tolerance_record_index, settings_record_index});

rewrite_native_scalar!(DesignSurfacePatchNormalState);
rewrite_native_record!(DesignSurfacePatchScope, []; {boundaries, normal});
