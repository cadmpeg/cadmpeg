// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignCombineBodySelection, DesignCombineExternalBodyIdentity, DesignCombineForm,
    DesignCombineOperation, DesignCombineTools, DesignExternalVersion,
};

rewrite_native_record!(DesignCombineBodySelection, []; {record_index, external_identity});
rewrite_native_record!(DesignCombineExternalBodyIdentity, []; {selector_asset_id, selector_context_id, occurrence_reference, external_body_reference, external_segment, external_link_name, external_version, tail_values});
rewrite_native_scalar!(DesignCombineForm);
rewrite_native_record!(DesignCombineOperation, []; {form, operation, keep_tools, target_record_index, tools});
rewrite_native_record!(DesignCombineTools, []; {first, additional});
rewrite_native_record!(DesignExternalVersion, []; {property_key, version_urn});
