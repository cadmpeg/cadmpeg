// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignHoleConstruction, DesignHoleExtent, DesignHoleFaceSelection, DesignHoleScope,
    DesignHoleTangentPoint,
};

rewrite_native_record!(DesignHoleConstruction, []; {point_record_index, position, direction, point_parameters, reference_type, tangent_point_data, input_records, face_selection});
rewrite_native_record!(DesignHoleFaceSelection, []; {record_index, class_tag, asset_id, context_id, identity_record_index, primary_identity, secondary, historical_face_candidates, next_record_index});
rewrite_native_record!(DesignHoleTangentPoint, []; {prefix, data});

rewrite_native_record!(DesignHoleScope, []; {point, extent});
rewrite_native_record!(DesignHoleExtent, []; {leading_word, extent_control, saved_non_distance_depth});
