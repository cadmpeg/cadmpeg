// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignPatchContinuity, DesignRuledSurfaceCorner, DesignRuledSurfaceMethod,
    DesignRuledSurfaceOperation, DesignSurfaceExtendMethod, DesignSurfaceExtendOperation,
    DesignSurfaceOffsetOperation, DesignSurfaceOffsetSupport, DesignSurfacePatchBoundary,
    DesignSurfacePatchNormalState, DesignSurfacePatchScope, DesignSurfaceStitchOperation,
};

rewrite_native_scalar!(DesignPatchContinuity);
rewrite_native_scalar!(DesignRuledSurfaceCorner);
rewrite_native_scalar!(DesignRuledSurfaceMethod);
rewrite_native_record!(DesignRuledSurfaceOperation, []; {method, corner, alternate_face, angle_owner_record_index, distance_owner_record_index, edge_group_record_indices, auxiliary_record_indices, direction_entity_id});
rewrite_native_scalar!(DesignSurfaceExtendMethod);
rewrite_native_record!(DesignSurfaceExtendOperation, []; {distance, distance_record_index, method, boundary_record_index, boundary_reference_record_index, edge_record_indices, tolerance});
rewrite_native_record!(DesignSurfaceOffsetOperation, []; {distance, distance_record_index, support});
rewrite_native_enum!(DesignSurfaceOffsetSupport, []; {
    BoundaryCarrier {boundary_record_index, boundary_reference_record_index, boundary_reference_offset, edge_record_indices, tolerance, tolerance_offset},
    FaceGroups {group_record_indices},
});
rewrite_native_record!(DesignSurfacePatchBoundary, []; {scope_reference_ordinal, record_index, is_seed_selection, continuity, flip, scale, model_reference});
rewrite_native_record!(DesignSurfaceStitchOperation, []; {gap_tolerance, tolerance_record_index, settings_record_index});

rewrite_native_scalar!(DesignSurfacePatchNormalState);
rewrite_native_record!(DesignSurfacePatchScope, []; {boundaries, normal});
