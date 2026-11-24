// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    AngularGeneral, AssemblyJoint, DriveAxis, DriveMode, ExternalDocument, FrameAxis,
    GeneralTolerance, GeneratedFrom, InstanceBodies, JointConnector, JointDrive, JointLimitRange,
    JointLimits, JointOperand, JointOperands, LinearGeneral, LinearGeneralClass, MemberAction,
    MemberRule, NestedOverride, Occurrence, OperandContainer, PairedJointKind, PlaneSense,
    ProductDefinition, ProductDefinitionKind, Representation, RepresentationContent,
    RepresentationDetail, RepresentationMember, Storage, TangentContact,
};

rewrite_record!(AssemblyJoint, []; {id, operands, suppressed, driving_parameter, drives, native_ref});
rewrite_scalar!(FrameAxis);
rewrite_scalar!(DriveAxis);
rewrite_scalar!(DriveMode);
rewrite_record!(JointDrive, []; {axis, mode, target_position, target_velocity, stiffness, damping, max_force});
rewrite_scalar!(ExternalDocument);
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
    Pair {kind, connectors, offset_frames, frame_sense},
});
rewrite_record!(Occurrence, []; {id, prototype, transform, suppressed, visible, representation, overrides, name, native_ref});
rewrite_record!(NestedOverride, []; {path, transform, suppressed, visible, representation});
rewrite_record!(InstanceBodies, []; {occurrence, bodies});
rewrite_enum!(OperandContainer, []; {
    Root {},
    Occurrence {occurrence, occurrence_path},
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
    RackPinion {distance, distance2, ratio, coupled_joints},
    Screw {distance},
    Gears {distance, distance2, ratio, coupled_joints},
    Belt {distance, distance2},
    PlaneCoincidence {sense, offset},
    AxisCoincidence {},
    PointOnAxis {distance},
    Insert {sense, offset},
    Tangent {contact},
    Native {name, angle, translation_offset, distance, distance2, angular_limits, linear_limits},
});
rewrite_enum!(PlaneSense, []; {
    Opposed,
    Aligned,
});
rewrite_enum!(TangentContact, []; {
    Outside,
    Inside,
});
rewrite_record!(ProductDefinition, []; {id, kind, source_name, label, description, part_number, bom_properties, generated_from, storage, bodies, occurrences, instance_bodies, representations, sketches, units, general_tolerance, active, native_ref});
rewrite_scalar!(ProductDefinitionKind);
rewrite_record!(GeneratedFrom, []; {definition, configuration});
rewrite_enum!(Storage, []; {
    Embedded,
    File {document, source},
    External(field0),
});
rewrite_scalar!(GeneralTolerance);
rewrite_scalar!(LinearGeneral);
rewrite_scalar!(AngularGeneral);
rewrite_scalar!(LinearGeneralClass);
rewrite_record!(Representation, []; {name, content, features, default, native_ref});
rewrite_enum!(RepresentationContent, []; {
    Bodies(field0),
    Members(field0),
    Document(field0),
    Unread,
});
rewrite_record!(MemberRule, []; {unlisted, members});
rewrite_record!(RepresentationMember, []; {path, action});
rewrite_enum!(MemberAction, []; {
    Include,
    Exclude,
    Substitute(field0),
    ReplaceBy(field0),
    Detail(field0),
    Native(field0),
});
rewrite_scalar!(RepresentationDetail);
