use cadmpeg_ir::design::construction::{
    AnyConstruction, Construction, ConstructionInput, ConstructionKind, InputReference, Relation,
};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::selection::{
    MemberOrder, Members, PickDomain, PickTarget, Picked, Picks, Propagation, Selection,
    SelectionRule,
};
use cadmpeg_ir::features::datums::DatumAxis;

pub(super) fn incomplete(axis: &DatumAxis) -> bool {
    axis.geometry()
        .is_none_or(|(_, direction)| direction.unit().is_none())
        || !axis.native_options().is_empty()
        || options(axis.source_settings(), 0)
        || inputs(axis.inputs(), 0)
}
fn options(values: &NativeOptions, depth: usize) -> bool {
    values.values().any(|v| match v {
        NativeOption::Selection(s) => selection(s, depth),
        NativeOption::OrderedSelection(s) => selection(s, depth),
        _ => false,
    })
}
fn inputs(values: &[ConstructionInput], depth: usize) -> bool {
    depth >= 64
        || values.iter().any(|v| {
            v.role.as_ref().is_none_or(|role| match role.relation {
                Relation::Native(_) => true,
                Relation::Offset | Relation::Angle | Relation::AtParameter => {
                    v.reference.is_none() || v.value.is_none()
                }
                Relation::Coordinates => v.value.is_none(),
                Relation::Radius | Relation::Limit => v.reference.is_none() && v.value.is_none(),
                Relation::Coincident
                | Relation::Parallel
                | Relation::Perpendicular
                | Relation::Tangent
                | Relation::Midway
                | Relation::Center
                | Relation::Centroid
                | Relation::AxisOf
                | Relation::Projection
                | Relation::PrincipalNormal
                | Relation::Binormal
                | Relation::Fold => v.reference.is_none(),
            }) || match &v.reference {
                Some(InputReference::Picks(s)) => selection(s, depth + 1),
                Some(InputReference::Built(v)) => match v.as_ref() {
                    AnyConstruction::Point(v) => construction(v, depth + 1),
                    AnyConstruction::Direction(v) => construction(v, depth + 1),
                    AnyConstruction::Axis(v) => construction(v, depth + 1),
                    AnyConstruction::Plane(v) => construction(v, depth + 1),
                    AnyConstruction::Frame(v) => construction(v, depth + 1),
                    AnyConstruction::Curve(v) => construction(v, depth + 1),
                },
                None => false,
            }
        })
}
fn construction<K: ConstructionKind>(value: &Construction<K>, depth: usize) -> bool {
    value.inputs.is_empty() && value.resolved.is_none()
        || inputs(&value.inputs, depth)
        || value
            .mode
            .as_ref()
            .is_some_and(|v| !v.native_options.is_empty() || options(&v.source_settings, depth))
}
fn selection<O: MemberOrder>(value: &Selection<O>, depth: usize) -> bool {
    if depth >= 64 {
        return true;
    }
    match &value.picks {
        Picks::Unread => true,
        Picks::Named(values) => values.as_slice().is_empty() || members(values, depth),
        Picks::Rule {
            terms,
            excluded,
            resolved,
        } => {
            terms.iter().chain(excluded).any(|v| {
                matches!(v.rule, SelectionRule::Native(_))
                    || members(&v.seeds, depth)
                    || matches!(v.propagation, Some(Propagation::Native(_)))
                    || match &v.rule {
                        SelectionRule::Chain { end: Some(v) } => pick(v, depth + 1),
                        SelectionRule::Region { boundary } => members(boundary, depth),
                        SelectionRule::Intersection { with } => members(with, depth),
                        _ => false,
                    }
            }) || resolved.as_ref().is_some_and(|v| members(v, depth))
        }
    }
}
fn members<O: MemberOrder>(values: &Members<Picked, O>, depth: usize) -> bool {
    values.as_slice().iter().any(|v| pick(v, depth + 1))
}
fn pick(value: &Picked, depth: usize) -> bool {
    depth >= 64
        || matches!(value.target, PickTarget::Native { .. })
        || match &value.domain {
            Some(PickDomain::Limits { start, end }) => {
                start.iter().chain(end).any(|v| pick(v, depth + 1))
            }
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::design::construction::{InputRole, ScalarDimension, Subject};
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::features::datums::DatumAxisDraft;
    use cadmpeg_ir::features::geometry::{FeatureDirection3, FinitePoint3};
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::math::{Point3, Vector3};
    #[test]
    fn work_axis_construction_completeness_accounts_for_unknown_rules_and_missing_values() {
        let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
        for (relation, value, expected) in [
            (
                Relation::Native(cadmpeg_core::nonblank_literal!("f3d:unknown")),
                None,
                true,
            ),
            (Relation::Offset, None, true),
            (
                Relation::Offset,
                Some(ScalarDimension::Length(Dimension::Literal(
                    cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
                ))),
                false,
            ),
            (Relation::Coincident, None, false),
        ] {
            let draft = DatumAxisDraft {
                origin: Some(point),
                direction: Some(FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()),
                inputs: vec![ConstructionInput {
                    role: Some(InputRole {
                        subject: Subject::Whole,
                        relation,
                    }),
                    reference: Some(InputReference::Built(Box::new(AnyConstruction::Point(
                        Construction {
                            resolved: Some(point),
                            inputs: Vec::new(),
                            after: Vec::new(),
                            solution: None,
                            mode: None,
                            resolved_in: std::collections::BTreeMap::new(),
                        },
                    )))),
                    value,
                    reversed: None,
                }],
                ..Default::default()
            };
            let FeatureOperation::DatumAxis { axis } = draft.into_operation().unwrap() else {
                panic!("datum axis")
            };
            assert_eq!(incomplete(&axis), expected);
        }
    }

    #[test]
    fn work_axis_completeness_requires_each_relations_reference_and_scalar() {
        let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
        for (relation, scalar, reference, expected) in [
            (Relation::Offset, true, false, true),
            (Relation::Offset, false, true, true),
            (Relation::Offset, true, true, false),
            (Relation::Coordinates, true, false, false),
            (Relation::Coordinates, false, true, true),
            (Relation::Coincident, false, false, true),
            (Relation::Coincident, false, true, false),
            (Relation::Radius, true, false, false),
            (Relation::Radius, false, true, false),
            (Relation::Radius, false, false, true),
            (Relation::Limit, true, false, false),
            (Relation::Limit, false, true, false),
            (Relation::Fold, true, false, true),
        ] {
            let draft = DatumAxisDraft {
                origin: Some(point),
                direction: Some(FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()),
                inputs: vec![ConstructionInput {
                    value: scalar.then(|| {
                        if matches!(relation, Relation::Coordinates) {
                            ScalarDimension::Components {
                                x: Dimension::Literal(
                                    cadmpeg_ir::scalar::FiniteReal::new(0.0).unwrap(),
                                ),
                                y: Dimension::Literal(
                                    cadmpeg_ir::scalar::FiniteReal::new(0.0).unwrap(),
                                ),
                                z: Dimension::Literal(
                                    cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                                ),
                            }
                        } else {
                            ScalarDimension::Length(Dimension::Literal(
                                cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
                            ))
                        }
                    }),
                    role: Some(InputRole {
                        subject: Subject::Whole,
                        relation,
                    }),
                    reference: reference.then(|| {
                        InputReference::Built(Box::new(AnyConstruction::Point(Construction {
                            resolved: Some(point),
                            inputs: Vec::new(),
                            after: Vec::new(),
                            solution: None,
                            mode: None,
                            resolved_in: std::collections::BTreeMap::new(),
                        })))
                    }),
                    reversed: None,
                }],
                ..Default::default()
            };
            let FeatureOperation::DatumAxis { axis } = draft.into_operation().unwrap() else {
                panic!("axis")
            };
            assert_eq!(incomplete(&axis), expected, "{:?}", axis.inputs()[0]);
        }
    }
}
