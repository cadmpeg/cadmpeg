// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{AssemblyJoint, CopyOnChange, CopyOnChangePolicy, ExternalDocument, JointConnector, JointLimitRange, JointLimits, JointOperand, JointOperands, LinkState, Occurrence, OccurrenceParent, OperandContainer, PairedJointKind, ProductDefinition, ProductDefinitionKind, PrototypeReference};

rewrite_record!(AssemblyJoint, []; {id, operands, suppressed, native_ref});
rewrite_record!(CopyOnChange, []; {policy, source, group, touched});
rewrite_enum!(CopyOnChangePolicy, []; {
    Disabled,
    Enabled,
    Owned,
    Tracking,
    Native(field0),
});
rewrite_enum!(ExternalDocument, []; {
    Path {path},
    DocumentId {document_id},
    Missing {},
});
rewrite_record!(JointConnector, []; {operand, frame, detached});
rewrite_scalar!(JointLimitRange);
rewrite_enum!(JointLimits, []; {
    Minimum {minimum},
    Maximum {maximum},
    Range(field0),
});
rewrite_record!(JointOperand, []; {container, object, subelements});
rewrite_enum!(JointOperands, []; {
    Grounded {connector, offset_frame},
    Pair {kind, connectors, offset_frames},
});
rewrite_record!(LinkState, []; {linked_subelements, element_component, claim_child, copy_on_change});
rewrite_record!(Occurrence, []; {id, prototype, parent, ordinal, transform, linked_prototype, scale, name, visible, link, native_ref});
rewrite_enum!(OccurrenceParent, []; {
    Root {},
    Occurrence {occurrence},
});
rewrite_enum!(OperandContainer, []; {
    Root {},
    Occurrence {occurrence},
    External {external_document},
});
rewrite_enum!(PairedJointKind, []; {
    Fixed {angle, translation_offset, angular_limits, linear_limits},
    Revolute {angle, angular_limits},
    Slider {distance, translation_offset, linear_limits},
    Cylindrical {angle, distance, angular_limits, linear_limits},
    Ball {},
    Distance {distance},
    Parallel {},
    Perpendicular {},
    Angle {angle},
    RackPinion {distance, distance2},
    Screw {distance},
    Gears {distance, distance2},
    Belt {distance, distance2},
    Native {name, angle, translation_offset, distance, distance2, angular_limits, linear_limits},
});
rewrite_record!(ProductDefinition, []; {id, kind, source_name, label, description, part_number, bom_properties, bodies, native_ref});
rewrite_enum!(ProductDefinitionKind, []; {
    Part,
    Group,
    LinkGroup,
    Object,
});
rewrite_enum!(PrototypeReference, []; {
    Local {definition},
    External {document, object},
    Unresolved {},
});
