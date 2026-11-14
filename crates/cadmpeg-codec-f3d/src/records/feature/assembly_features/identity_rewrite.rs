// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignComponentInsertConstruction, DesignComponentInsertMatrix,
    DesignCopyPasteComponentOperation, DesignDerivedInstanceAuxiliaryPair,
    DesignDerivedInstanceCarrier, DesignDerivedInstanceConstruction,
    DesignDerivedInstancePlacement, DesignDerivedInstanceReference, DesignDerivedInstanceRelation,
    DesignDerivedInstanceSource, DesignDerivedInstanceSourcePlacement,
};

rewrite_native_record!(DesignComponentInsertConstruction, []; {relation_record_index, carrier_record_index, occurrence_identity, neutron_role, neutron_role_offset, placement});
rewrite_native_record!(DesignComponentInsertMatrix, []; {scope, carrier_offset});
rewrite_native_record!(DesignCopyPasteComponentOperation, []; {relation_record_index, source_occurrence_record_index, copied_occurrence_record_index, component_guid, source_occurrence_guid, copied_occurrence_guid, source_transform, source_transform_offset, copied_transform, copied_transform_offset});
rewrite_native_record!(DesignDerivedInstanceConstruction, []; {reference, placements, auxiliary_pairs, repeated_references, primary_reference, context_placement, trailer_guid, sources});
rewrite_native_record!(DesignDerivedInstanceSource, []; {reference, relation});
rewrite_native_record!(DesignDerivedInstanceSourcePlacement, []; {reference, placement});
rewrite_native_record!(DesignDerivedInstanceAuxiliaryPair, []; {auxiliary, relation});
rewrite_native_record!(DesignDerivedInstanceRelation, []; {carrier_reference, middle_reference, scope_reference, carrier});
rewrite_native_record!(DesignDerivedInstanceCarrier, []; {component_record_index, component_guid, occurrence_guid, transform});
rewrite_native_scalar!(DesignDerivedInstancePlacement);

rewrite_native_record!(DesignDerivedInstanceReference, []; {target, type_guid});
