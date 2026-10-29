// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignLoftConstruction, DesignPipeConstruction, DesignPipePointSelection,
    DesignRevolveConstruction, DesignSweepConstruction,
};

rewrite_native_record!(DesignLoftConstruction, []; {operation});
rewrite_native_record!(DesignPipeConstruction, []; {operation, is_hollow, opaque_post_operation, record_indexes, point_selection});
rewrite_native_record!(DesignPipePointSelection, []; {point_data});
rewrite_native_scalar!(DesignRevolveConstruction);
rewrite_native_record!(DesignSweepConstruction, []; {operation, values, record_indexes});
