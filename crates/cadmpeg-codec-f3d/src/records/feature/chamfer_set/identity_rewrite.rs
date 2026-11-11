// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignChamferScope, DesignChamferSet, DesignChamferSetList, DesignChamferSetListForm,
    DesignChamferSetSpec,
};

rewrite_native_record!(DesignChamferScope, []; {fixed, sets});
rewrite_native_record!(DesignChamferSetList, []; {form, entry_count, sets});
rewrite_native_scalar!(DesignChamferSetListForm);
rewrite_native_record!(DesignChamferSet, []; {record_index, byte_offset, frame_length, group_record_index, spec, auxiliary_record_index, flag});
rewrite_native_enum!(DesignChamferSetSpec, []; {
    EqualDistance {distance_owner_record_index},
    TwoDistances {first_owner_record_index, second_owner_record_index},
    DistanceAngle {distance_owner_record_index, angle_owner_record_index},
});
