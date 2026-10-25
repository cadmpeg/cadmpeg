// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignDraftOperation, DesignMoveForm, DesignMoveOperation, DesignOffsetFacesOperation,
    DesignShellOperation, DesignThickenOperation,
};

rewrite_native_record!(DesignDraftOperation, []; {angle, angle_record_index, opposite_angle_record_index});
rewrite_native_scalar!(DesignMoveForm);
rewrite_native_record!(DesignMoveOperation, []; {transform, transform_record_index, form});
rewrite_native_record!(DesignOffsetFacesOperation, []; {distance, distance_record_index});
rewrite_native_record!(DesignShellOperation, []; {thickness, thickness_record_index, outward});
rewrite_native_record!(DesignThickenOperation, []; {signed_thickness, thickness_record_index});
