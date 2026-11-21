// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignFilletMidpointOwners, DesignFilletScope, DesignFilletSet, DesignFilletSetLaw,
    DesignFilletSetList, DesignFilletSetListForm,
};

rewrite_native_record!(DesignFilletScope, []; {fixed, sets});
rewrite_native_record!(DesignFilletSetList, []; {form, rolling_ball_corner, sets});
rewrite_native_scalar!(DesignFilletSetListForm);
rewrite_native_record!(DesignFilletSet, []; {record_index, byte_offset, frame_length, group_record_index, law, tangency_weight_owner_record_index, tangent_chain, curvature_continuity});
rewrite_native_enum!(DesignFilletSetLaw, []; {
    Constant {radius_owner_record_index},
    Chordal {chord_length_owner_record_index},
    Asymmetric {offset_one_owner_record_index, offset_two_owner_record_index},
    Variable {start_radius_owner_record_index, end_radius_owner_record_index, middle},
});

rewrite_native_record!(DesignFilletMidpointOwners, []; {radius_owner_record_index, position_owner_record_index});
