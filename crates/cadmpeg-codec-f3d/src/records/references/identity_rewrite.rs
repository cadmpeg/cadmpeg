// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::DesignClassTag;

rewrite_native_record!(DesignClassTag, []; (field0));

rewrite_native_identity_record!(super::DesignMaterialAssignment; body_binding_id);
