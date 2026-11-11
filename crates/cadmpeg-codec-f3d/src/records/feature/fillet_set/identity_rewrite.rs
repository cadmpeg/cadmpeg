// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignFilletScope, DesignFilletSet, DesignFilletSetLaw, DesignFilletSetList,
    DesignFilletSetListForm,
};

rewrite_native_record!(DesignFilletScope, []; {fixed, sets});
rewrite_native_record!(DesignFilletSetList, []; {form, sets});
rewrite_native_scalar!(DesignFilletSetListForm);
rewrite_native_record!(DesignFilletSet, []; {record_index, byte_offset, frame_length, group_record_index, law, tangency_weight_owner_record_index, flag});
rewrite_native_enum!(DesignFilletSetLaw, []; {
    Constant {radius_owner_record_index},
    Chordal {chord_length_owner_record_index},
});
