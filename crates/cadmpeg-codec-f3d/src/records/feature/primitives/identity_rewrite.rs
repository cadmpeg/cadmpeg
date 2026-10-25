// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignBoxPrimitive, DesignCylinderPrimitive, DesignSpherePrimitive, DesignTorusPrimitive,
};

rewrite_native_record!(DesignBoxPrimitive, []; {length, length_record_index, width, width_record_index, height, height_record_index, offset_x, offset_x_record_index, offset_y, offset_y_record_index, operation});
rewrite_native_scalar!(DesignCylinderPrimitive);
rewrite_native_record!(DesignSpherePrimitive, []; {transform, diameter, diameter_record_index, operation});
rewrite_native_record!(DesignTorusPrimitive, []; {transform, major_diameter, major_diameter_record_index, minor_diameter, minor_diameter_record_index, operation});
