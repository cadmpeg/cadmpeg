//! Relation instance records and scalar roles.

use super::scalars::feature_object_name;
use crate::classification::{native_object_class, NativeClassKind};
use crate::history::is_history_metadata_record;
use crate::layout::feature_input_shifted_scalar_trailer as shifted_trailer;
use crate::records::{
    FeatureInputClass, FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputRelationFamily, FeatureInputRelationInstance, FeatureInputScalar,
    FeatureInputScalarRole,
};
use std::collections::{HashMap, HashSet};

pub(super) fn feature_intervals(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<(u64, u64, String)> {
    let mut starts = histories
        .iter()
        .flat_map(|history| {
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
        })
        .filter_map(|feature| {
            Some((
                feature_object_name(feature, lane)?.offset,
                feature.id.clone(),
            ))
        })
        .collect::<Vec<_>>();
    starts.sort_unstable_by_key(|(offset, _)| *offset);
    starts.dedup_by_key(|(offset, _)| *offset);
    starts
        .iter()
        .enumerate()
        .map(|(index, (start, feature))| {
            (
                *start,
                starts.get(index + 1).map_or(u64::MAX, |(next, _)| *next),
                feature.clone(),
            )
        })
        .collect()
}

fn feature_at_offset(offset: u64, intervals: &[(u64, u64, String)]) -> Option<&str> {
    intervals
        .iter()
        .find(|(start, end, _)| offset > *start && offset < *end)
        .map(|(_, _, feature)| feature.as_str())
}

pub(super) fn relation_declaration_candidates<'a>(
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, u64, String)],
) -> Vec<(
    &'a FeatureInputClass,
    &'a FeatureInputScalar,
    FeatureInputRelationFamily,
)> {
    classes
        .iter()
        .filter_map(|class| {
            let family = relation_family(&class.name)?;
            let scalar = scalars
                .iter()
                .filter(|scalar| scalar.offset > class.offset)
                .min_by_key(|scalar| scalar.offset)?;
            if scalar.offset - class.offset > 128
                || (!intervals.is_empty()
                    && feature_at_offset(class.offset, intervals) != scalar.feature_ref.as_deref())
                || !relation_signature(family, &scalar.operands)
            {
                return None;
            }
            Some((class, scalar, family))
        })
        .collect()
}

pub(super) fn unique_relation_declaration_candidates<'a>(
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, u64, String)],
) -> Vec<(
    &'a FeatureInputClass,
    &'a FeatureInputScalar,
    FeatureInputRelationFamily,
)> {
    let candidates = relation_declaration_candidates(classes, scalars, intervals);
    let mut counts = HashMap::<&str, usize>::new();
    for (_, scalar, _) in &candidates {
        *counts.entry(scalar.id.as_str()).or_default() += 1;
    }
    candidates
        .into_iter()
        .filter(|(_, scalar, _)| counts.get(scalar.id.as_str()) == Some(&1))
        .collect()
}

pub(super) fn relation_instances(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputRelationInstance> {
    let sketch_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag.eq_ignore_ascii_case("Sketch"))
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let intervals = feature_intervals(histories, lane);
    let declaration_candidates =
        relation_declaration_candidates(&lane.classes, &lane.scalars, &intervals);
    let mut candidate_counts = HashMap::<&str, usize>::new();
    for (_, scalar, _) in &declaration_candidates {
        *candidate_counts.entry(scalar.id.as_str()).or_default() += 1;
    }
    let declarations = declaration_candidates
        .into_iter()
        .filter(|(_, scalar, _)| candidate_counts.get(scalar.id.as_str()) == Some(&1))
        .map(|(class, scalar, family)| {
            (
                scalar.id.as_str(),
                (class.offset, family, class.id.as_str()),
            )
        })
        .collect::<HashMap<_, _>>();
    let ambiguous_scalars = candidate_counts
        .into_iter()
        .filter_map(|(scalar, count)| (count > 1).then_some(scalar))
        .collect::<HashSet<_>>();
    let mut groups = Vec::<(
        String,
        FeatureInputRelationFamily,
        String,
        Vec<FeatureInputOperand>,
        Vec<&FeatureInputScalar>,
        usize,
    )>::new();
    for (scalar_index, scalar) in lane.scalars.iter().enumerate() {
        let Some(feature_ref) = scalar
            .feature_ref
            .as_deref()
            .filter(|feature| sketch_features.contains(feature))
        else {
            continue;
        };
        let declaration = declarations.get(scalar.id.as_str());
        let Some((class_offset, family, class_ref)) = declaration else {
            if ambiguous_scalars.contains(scalar.id.as_str()) {
                continue;
            }
            let Some((owner, family, class_ref, operands, group_scalars, last_index)) =
                groups.last()
            else {
                continue;
            };
            let Some(last_scalar) = group_scalars.last() else {
                continue;
            };
            let same_run = owner == feature_ref
                && *last_index + 1 == scalar_index
                && !lane.classes.iter().any(|class| {
                    class.offset > last_scalar.offset
                        && class.offset < scalar.offset
                        && relation_family(&class.name).is_some()
                })
                && operands
                    .iter()
                    .map(|operand| (operand.kind, operand.entity_index))
                    .eq(scalar
                        .operands
                        .iter()
                        .map(|operand| (operand.kind, operand.entity_index)));
            if same_run && scalar.role == FeatureInputScalarRole::Driving {
                if group_scalars.len() == 1 {
                    let (_, _, _, _, scalars, last_index) = groups
                        .last_mut()
                        .expect("a continuation requires an existing relation group");
                    scalars.push(scalar);
                    *last_index = scalar_index;
                } else {
                    groups.push((
                        owner.clone(),
                        *family,
                        class_ref.clone(),
                        scalar.operands.clone(),
                        vec![scalar],
                        scalar_index,
                    ));
                }
            }
            continue;
        };
        // A display scalar can precede the declaration selected by its adjacent
        // driving scalar. The driving scalar's declaration is authoritative for
        // that pair; display-only scalars still stop at a class declaration.
        let promote_class = groups
            .last()
            .is_some_and(|(_, _, group_class, _, scalars, _)| {
                group_class != class_ref
                    && scalars.len() == 1
                    && scalars[0].role == FeatureInputScalarRole::Display
                    && scalar.role == FeatureInputScalarRole::Driving
                    && *class_offset > scalars[0].offset
                    && *class_offset < scalar.offset
            });
        let append = groups.last().is_some_and(
            |(owner, candidate, group_class, operands, scalars, last_index)| {
                owner == feature_ref
                    && (candidate == family || promote_class)
                    && (group_class == class_ref || promote_class)
                    && *last_index + 1 == scalar_index
                    && scalars.len() == 1
                    && operands
                        .iter()
                        .map(|operand| (operand.kind, operand.entity_index))
                        .eq(scalar
                            .operands
                            .iter()
                            .map(|operand| (operand.kind, operand.entity_index)))
            },
        );
        if append {
            let (_, candidate, group_class, _, scalars, last_index) = groups
                .last_mut()
                .expect("append requires an existing relation group");
            if promote_class {
                *candidate = *family;
                *group_class = (*class_ref).to_string();
            }
            scalars.push(scalar);
            *last_index = scalar_index;
        } else {
            groups.push((
                feature_ref.to_string(),
                *family,
                (*class_ref).to_string(),
                scalar.operands.clone(),
                vec![scalar],
                scalar_index,
            ));
        }
    }
    let mut instances = groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (feature_ref, family, class_ref, operands, scalars, _))| {
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let display = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
                    .copied()
                    .collect::<Vec<_>>();
                let offset = scalars[0].offset;
                FeatureInputRelationInstance {
                    id: format!(
                        "sldprt:feature-input:relation-instance#{}:{offset}",
                        lane.id
                            .rsplit_once('#')
                            .map_or(lane.id.as_str(), |(_, key)| key)
                    ),
                    parent: lane.id.clone(),
                    ordinal: ordinal as u32,
                    offset,
                    family,
                    class_ref,
                    feature_ref,
                    scalar_refs: scalars.iter().map(|scalar| scalar.id.clone()).collect(),
                    parameter_scalar_ref: (driving.len() == 1).then(|| driving[0].id.clone()),
                    display_scalar_ref: (display.len() == 1).then(|| display[0].id.clone()),
                    operands,
                }
            },
        )
        .collect::<Vec<_>>();
    bind_detached_relation_drivers(&mut instances, lane);
    bind_circle_dimension_centers(&mut instances, lane);
    instances
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod relation_records_tests {
    use super::*;
    use crate::records::{
        Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole, FeatureInputLane,
        FeatureInputName,
    };
    use std::collections::BTreeMap;

    fn class(offset: u64, name: &str) -> FeatureInputClass {
        FeatureInputClass {
            id: format!("class-{offset}"),
            parent: "lane".into(),
            ordinal: 0,
            offset,
            name: name.into(),
            role: FeatureInputClassRole::SketchConstraint,
        }
    }

    fn scalar(offset: u64, role: FeatureInputScalarRole) -> FeatureInputScalar {
        let operands = [0_u16, 1]
            .into_iter()
            .enumerate()
            .map(|(ordinal, entity_index)| FeatureInputOperand {
                offset: offset + ordinal as u64,
                reference_ref: format!("reference-{offset}-{ordinal}"),
                kind: FeatureInputOperandKind::Native(0x8152),
                entity_index,
                entity_ref: None,
            })
            .collect();
        FeatureInputScalar {
            id: format!("scalar-{offset}"),
            parent: "lane".into(),
            feature_ref: Some("sketch".into()),
            ordinal: 0,
            offset,
            object_id: 1,
            name: "dimension".into(),
            value: 1.0,
            role,
            entity_indices: vec![0, 1],
            operands,
        }
    }

    fn circle_scalar(
        offset: u64,
        name: &str,
        role: FeatureInputScalarRole,
        entity_ref: Option<&str>,
    ) -> FeatureInputScalar {
        FeatureInputScalar {
            id: format!("scalar-{offset}"),
            parent: "lane".into(),
            feature_ref: Some("sketch".into()),
            ordinal: 0,
            offset,
            object_id: 1,
            name: name.into(),
            value: 1.0,
            role,
            entity_indices: vec![0],
            operands: vec![FeatureInputOperand {
                offset: offset + 1,
                reference_ref: format!("reference-{offset}"),
                kind: FeatureInputOperandKind::Native(0x1234),
                entity_index: 0,
                entity_ref: entity_ref.map(str::to_owned),
            }],
        }
    }

    fn lane(classes: Vec<FeatureInputClass>, scalars: Vec<FeatureInputScalar>) -> FeatureInputLane {
        FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes,
            names: Vec::new(),
            scalars,
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        }
    }

    fn sketch_history() -> Vec<FeatureHistory> {
        vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![Feature {
                id: "sketch".into(),
                parent: "history".into(),
                xml_tag: "Sketch".into(),
                tree_parent: None,
                source_id: None,
                parent_source_id: None,
                ordinal: 0,
                name: "Sketch".into(),
                kind: "Sketch".into(),
                input_class: None,
                suppressed: false,
                parameters: BTreeMap::new(),
                dimension_properties: BTreeMap::new(),
                properties: BTreeMap::new(),
                text: None,
                content: Vec::new(),
            }],
        }]
    }

    #[test]
    fn display_scalar_joins_driving_scalar_after_class_declaration() {
        let vertical = class(10, "sgPntPntVertDist");
        let horizontal = class(30, "sgPntPntHorDist");
        let display = scalar(20, FeatureInputScalarRole::Display);
        let driving = scalar(40, FeatureInputScalarRole::Driving);
        let lane = lane(
            vec![vertical, horizontal.clone()],
            vec![display.clone(), driving.clone()],
        );

        let history = sketch_history();
        let instances = relation_instances(&history, &lane);
        let [relation] = instances.as_slice() else {
            panic!("one relation instance");
        };
        assert_eq!(
            relation.family,
            FeatureInputRelationFamily::PointPointHorizontalDistance
        );
        assert_eq!(relation.class_ref, horizontal.id);
        assert_eq!(
            relation.scalar_refs,
            vec![display.id.clone(), driving.id.clone()]
        );
        assert_eq!(relation.display_scalar_ref, Some(display.id));
        assert_eq!(relation.parameter_scalar_ref, Some(driving.id));
    }

    #[test]
    fn display_scalars_do_not_cross_a_class_declaration() {
        let vertical = class(10, "sgPntPntVertDist");
        let horizontal = class(30, "sgPntPntHorDist");
        let first = scalar(20, FeatureInputScalarRole::Display);
        let second = scalar(40, FeatureInputScalarRole::Display);
        let lane = lane(vec![vertical, horizontal], vec![first, second]);

        let relations = relation_instances(&sketch_history(), &lane);
        assert_eq!(relations.len(), 2);
        assert_eq!(
            relations
                .iter()
                .map(|relation| relation.family)
                .collect::<Vec<_>>(),
            vec![
                FeatureInputRelationFamily::PointPointVerticalDistance,
                FeatureInputRelationFamily::PointPointHorizontalDistance,
            ]
        );
    }

    #[test]
    fn ambiguous_relation_declarations_leave_scalar_unbound() {
        let distance = class(10, "sgPntPntDist");
        let vertical = class(20, "sgPntPntVertDist");
        let lane = lane(
            vec![distance, vertical],
            vec![scalar(30, FeatureInputScalarRole::Driving)],
        );

        assert!(relation_instances(&sketch_history(), &lane).is_empty());
    }

    #[test]
    fn relation_declarations_do_not_cross_feature_intervals() {
        let mut history = sketch_history();
        history[0].features[0].id = "first".into();
        history[0].features[0].name = "First".into();
        let mut second = history[0].features[0].clone();
        second.id = "second".into();
        second.name = "Second".into();
        second.ordinal = 1;
        history[0].features.push(second);

        let mut relation_scalar = scalar(120, FeatureInputScalarRole::Driving);
        relation_scalar.feature_ref = Some("second".into());
        let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![relation_scalar]);
        lane.names = vec![
            FeatureInputName {
                id: "name-first".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: None,
                value: "First".into(),
            },
            FeatureInputName {
                id: "name-second".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 100,
                object_id: None,
                value: "Second".into(),
            },
        ];

        assert!(relation_instances(&history, &lane).is_empty());
    }

    #[test]
    fn metadata_records_do_not_split_relation_feature_intervals() {
        let mut history = sketch_history();
        let mut metadata = history[0].features[0].clone();
        metadata.id = "attribute-definition".into();
        metadata.xml_tag = "Feature".into();
        metadata.source_id = Some("-1".into());
        metadata.ordinal = 1;
        metadata.name = "Attribute-Definition".into();
        metadata.kind = "Attribute-Definition".into();
        metadata.input_class = None;
        history[0].features.push(metadata);

        let mut relation_scalar = scalar(120, FeatureInputScalarRole::Driving);
        relation_scalar.feature_ref = Some("sketch".into());
        let mut lane = lane(vec![class(110, "sgPntPntDist")], vec![relation_scalar]);
        lane.names = vec![
            FeatureInputName {
                id: "name-sketch".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: None,
                value: "Sketch".into(),
            },
            FeatureInputName {
                id: "name-attribute".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 100,
                object_id: None,
                value: "Attribute-Definition".into(),
            },
        ];

        let relations = relation_instances(&history, &lane);
        let [relation] = relations.as_slice() else {
            panic!("metadata must not terminate the sketch interval");
        };
        assert_eq!(relation.feature_ref, "sketch");
    }

    #[test]
    fn repeated_driving_scalar_starts_another_anchored_relation() {
        let lane = lane(
            vec![class(10, "sgPntPntDist")],
            vec![
                scalar(20, FeatureInputScalarRole::Display),
                scalar(30, FeatureInputScalarRole::Driving),
                scalar(40, FeatureInputScalarRole::Driving),
            ],
        );

        let relations = relation_instances(&sketch_history(), &lane);
        assert_eq!(relations.len(), 2);
        assert_eq!(
            relations
                .iter()
                .map(|relation| relation.scalar_refs.len())
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(relations.iter().all(|relation| {
            relation.family == FeatureInputRelationFamily::PointPointDistance
                && relation.class_ref == "class-10"
        }));
    }

    #[test]
    fn circle_dimension_binds_driver_inside_declared_entity_handle() {
        let first = circle_scalar(20, "name-first", FeatureInputScalarRole::Display, None);
        let nested_display =
            circle_scalar(40, "name-nested", FeatureInputScalarRole::Display, None);
        let nested_driver = circle_scalar(50, "name-nested", FeatureInputScalarRole::Driving, None);
        let mut lane = lane(
            vec![class(10, "sgCircleDim"), class(31, "sgEntHandle")],
            vec![first.clone(), nested_display.clone(), nested_driver.clone()],
        );
        lane.names = vec![
            FeatureInputName {
                id: "name-first".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 20,
                object_id: None,
                value: "D1".into(),
            },
            FeatureInputName {
                id: "name-nested".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 40,
                object_id: None,
                value: "D4".into(),
            },
        ];

        let instances = relation_instances(&sketch_history(), &lane);
        let [relation] = instances.as_slice() else {
            panic!("one circle relation");
        };
        assert_eq!(relation.scalar_refs, vec![first.id]);
        assert_eq!(relation.display_scalar_ref, Some("scalar-20".into()));
        assert!(relation.parameter_scalar_ref.is_none());
        assert_eq!(
            circle_dimension_handle_driver(relation, &lane).map(|scalar| scalar.id.as_str()),
            Some("scalar-50")
        );
    }

    #[test]
    fn circle_dimension_keeps_ambiguous_handle_drivers_unbound() {
        let first = circle_scalar(20, "name-first", FeatureInputScalarRole::Display, None);
        let d4_display = circle_scalar(40, "name-d4", FeatureInputScalarRole::Display, None);
        let d4_driver = circle_scalar(50, "name-d4", FeatureInputScalarRole::Driving, None);
        let d5_display = circle_scalar(60, "name-d5", FeatureInputScalarRole::Display, None);
        let d5_driver = circle_scalar(70, "name-d5", FeatureInputScalarRole::Driving, None);
        let mut lane = lane(
            vec![class(10, "sgCircleDim"), class(31, "sgEntHandle")],
            vec![first, d4_display, d4_driver, d5_display, d5_driver],
        );
        lane.names = [("name-first", "D1"), ("name-d4", "D4"), ("name-d5", "D5")]
            .into_iter()
            .enumerate()
            .map(|(ordinal, (id, value))| FeatureInputName {
                id: id.into(),
                parent: "lane".into(),
                ordinal: ordinal as u32,
                offset: 20 + ordinal as u64 * 10,
                object_id: None,
                value: value.into(),
            })
            .collect();

        let instances = relation_instances(&sketch_history(), &lane);
        let [relation] = instances.as_slice() else {
            panic!("one circle relation");
        };
        assert_eq!(relation.scalar_refs.len(), 1);
        assert!(relation.parameter_scalar_ref.is_none());
        assert!(circle_dimension_handle_driver(relation, &lane).is_none());
    }
}

pub(super) fn bind_circle_dimension_centers(
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) {
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    for relation in relations.iter_mut().filter(|relation| {
        relation.family == FeatureInputRelationFamily::CircleDiameter
            && relation.operands.len() == 1
    }) {
        let Some(display) = relation
            .display_scalar_ref
            .as_deref()
            .and_then(|id| scalars.get(id).copied())
        else {
            continue;
        };
        let Some(display_name) = names.get(display.name.as_str()) else {
            continue;
        };
        let Some(display_index) = lane
            .scalars
            .iter()
            .position(|scalar| scalar.id == display.id)
        else {
            continue;
        };
        let first = &relation.operands[0];
        let candidates = lane
            .scalars
            .iter()
            .enumerate()
            .filter(|scalar| {
                let scalar = scalar.1;
                scalar.feature_ref == display.feature_ref
                    && names.get(scalar.name.as_str()) == Some(display_name)
                    && matches!(scalar.operands.as_slice(), [candidate, _]
                        if candidate.kind == first.kind
                            && candidate.entity_index == first.entity_index)
            })
            .collect::<Vec<_>>();
        if candidates.first().map(|candidate| candidate.0) != Some(display_index + 1)
            || candidates.windows(2).any(|pair| pair[1].0 != pair[0].0 + 1)
        {
            continue;
        }
        let centers = candidates
            .iter()
            .filter_map(|(_, scalar)| scalar.operands.get(1))
            .map(|operand| {
                (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.clone(),
                )
            })
            .collect::<Vec<_>>();
        let Some(center) = centers.first() else {
            continue;
        };
        if centers.iter().any(|candidate| candidate != center) {
            continue;
        }
        let Some((_, source)) = candidates.iter().find(|(_, scalar)| {
            scalar.operands.get(1).is_some_and(|operand| {
                (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.clone(),
                ) == *center
            })
        }) else {
            continue;
        };
        relation.operands = source.operands.clone();
        for (_, scalar) in candidates {
            if !relation.scalar_refs.contains(&scalar.id) {
                relation.scalar_refs.push(scalar.id.clone());
            }
        }
    }
}

fn same_scalar_operands(left: &FeatureInputScalar, right: &FeatureInputScalar) -> bool {
    left.operands.len() == right.operands.len()
        && left
            .operands
            .iter()
            .zip(&right.operands)
            .all(|(left, right)| {
                left.kind == right.kind
                    && left.entity_index == right.entity_index
                    && left.entity_ref == right.entity_ref
            })
}

pub(super) fn circle_dimension_handle_driver<'a>(
    relation: &FeatureInputRelationInstance,
    lane: &'a FeatureInputLane,
) -> Option<&'a FeatureInputScalar> {
    if relation.family != FeatureInputRelationFamily::CircleDiameter
        || relation.parameter_scalar_ref.is_some()
        || relation.scalar_refs.len() != 1
    {
        return None;
    }
    let mut scalars = lane.scalars.iter().collect::<Vec<_>>();
    scalars.sort_unstable_by_key(|scalar| scalar.offset);
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    let first = relation
        .scalar_refs
        .first()
        .and_then(|id| scalars.iter().find(|scalar| scalar.id == *id))
        .copied()
        .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)?;
    let next_relation_offset = lane
        .classes
        .iter()
        .filter(|class| class.offset > first.offset && relation_family(&class.name).is_some())
        .map(|class| class.offset)
        .min()
        .unwrap_or(u64::MAX);
    let candidates = scalars
        .windows(2)
        .filter_map(|pair| {
            let display = pair[0];
            let driving = pair[1];
            let declared_handle = lane.classes.iter().any(|class| {
                class.name == "sgEntHandle"
                    && class.offset > first.offset
                    && class.offset < display.offset
            });
            let same_feature = display.feature_ref == first.feature_ref
                && driving.feature_ref == first.feature_ref;
            let same_name = matches!(
                (
                    names.get(display.name.as_str()),
                    names.get(driving.name.as_str())
                ),
                (Some(left), Some(right)) if left == right
            );
            let in_relation =
                first.offset < display.offset && driving.offset < next_relation_offset;
            (declared_handle
                && same_feature
                && same_scalar_operands(display, first)
                && same_scalar_operands(driving, first)
                && display.role == FeatureInputScalarRole::Display
                && driving.role == FeatureInputScalarRole::Driving
                && same_name
                && in_relation)
                .then_some(driving)
        })
        .collect::<Vec<_>>();
    if candidates.len() == 1 {
        Some(candidates[0])
    } else {
        None
    }
}

pub(super) fn bind_detached_relation_drivers(
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) {
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    let claimed = relations
        .iter()
        .flat_map(|relation| &relation.scalar_refs)
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut drivers = HashMap::<(String, String), Vec<&FeatureInputScalar>>::new();
    for scalar in lane.scalars.iter().filter(|scalar| {
        scalar.role == FeatureInputScalarRole::Driving
            && scalar.operands.is_empty()
            && !claimed.contains(scalar.id.as_str())
    }) {
        let (Some(feature), Some(name)) = (
            scalar.feature_ref.as_deref(),
            names.get(scalar.name.as_str()).copied(),
        ) else {
            continue;
        };
        drivers
            .entry((feature.to_string(), name.to_string()))
            .or_default()
            .push(scalar);
    }
    let mut candidates = HashMap::<(String, String), Vec<usize>>::new();
    for (index, relation) in relations.iter().enumerate() {
        if relation.parameter_scalar_ref.is_some() {
            continue;
        }
        let relation_names = relation
            .scalar_refs
            .iter()
            .filter_map(|id| scalars.get(id.as_str()))
            .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
            .filter_map(|scalar| names.get(scalar.name.as_str()).copied())
            .collect::<HashSet<_>>();
        if relation_names.len() != 1 {
            continue;
        }
        let name = *relation_names
            .iter()
            .next()
            .expect("one display scalar name");
        candidates
            .entry((relation.feature_ref.clone(), name.to_string()))
            .or_default()
            .push(index);
    }
    for (key, relation_indices) in candidates {
        let [relation_index] = relation_indices.as_slice() else {
            continue;
        };
        let Some([driver]) = drivers.get(&key).map(Vec::as_slice) else {
            continue;
        };
        let relation = &mut relations[*relation_index];
        relation.scalar_refs.push(driver.id.clone());
        relation.parameter_scalar_ref = Some(driver.id.clone());
    }
}

fn relation_family(name: &str) -> Option<FeatureInputRelationFamily> {
    match native_object_class(name).kind {
        NativeClassKind::SketchRelation(family) => Some(family),
        _ => None,
    }
}

pub(super) fn relation_signature(
    family: FeatureInputRelationFamily,
    operands: &[FeatureInputOperand],
) -> bool {
    use FeatureInputOperandKind::{Native, D6, E1};
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    if family == CircleDiameter {
        return matches!(
            operands,
            [operand]
                if matches!(operand.kind, Native(_))
        );
    }
    let [first, second] = operands else {
        return false;
    };
    match family {
        PointPointDistance => {
            (first.kind == D6 && second.kind == D6)
                || (first.kind == Native(0x81b2) && second.kind == Native(0x81b2))
                || (first.kind == Native(0x8152) && second.kind == Native(0x8152))
                || (first.kind == Native(0x837b) && second.kind == Native(0x837b))
                || (first.kind == Native(0xbc7c) && second.kind == Native(0xbc7c))
        }
        LineLineDistance => {
            (first.kind == E1 && second.kind == E1)
                || (first.kind == Native(0x8386) && second.kind == Native(0x8386))
                || (first.kind == Native(0xbc87) && second.kind == Native(0xbc87))
        }
        PointLineDistance => {
            (first.kind == D6 && second.kind == E1)
                || (first.kind == Native(0x837b) && second.kind == Native(0x8386))
                || (first.kind == Native(0xbc7c) && second.kind == Native(0xbc87))
        }
        PointPointHorizontalDistance | PointPointVerticalDistance => {
            (first.kind == Native(0x8152) && second.kind == Native(0x8152))
                || (first.kind == Native(0x8dcb) && second.kind == Native(0x8dcb))
        }
        Angle => first.kind == Native(0x8dda) && second.kind == Native(0x8dda),
        CircleDiameter => unreachable!("handled as a unary relation"),
    }
}

pub(super) fn scalar_role(payload: &[u8], trailer_offset: usize) -> FeatureInputScalarRole {
    let shifted_layout = shifted_value_only_scalar_trailer(payload, trailer_offset);
    let fixed_layout = payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 21)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 24..trailer_offset + 29) == Some(&[0, 0, 0, 2, 0]);
    let role_offset = if shifted_layout {
        trailer_offset + shifted_trailer::ROLE
    } else if compact_scalar_layout(payload, trailer_offset) {
        trailer_offset + 27
    } else if fixed_layout {
        trailer_offset + 29
    } else if legacy_scalar_layout(payload, trailer_offset) {
        trailer_offset + 30
    } else {
        return FeatureInputScalarRole::Native;
    };
    match payload.get(role_offset) {
        Some(0) => FeatureInputScalarRole::Driving,
        Some(1) => FeatureInputScalarRole::Display,
        _ => FeatureInputScalarRole::Native,
    }
}

pub(super) fn shifted_value_only_scalar_trailer(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(
        trailer_offset + shifted_trailer::ZERO_PREFIX..trailer_offset + shifted_trailer::OBJECT_ID,
    ) == Some(&[0, 0, 0])
        && payload
            .get(
                trailer_offset + shifted_trailer::ZERO_OBJECT_TAIL
                    ..trailer_offset + shifted_trailer::LAYOUT_MARKER,
            )
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(
            trailer_offset + shifted_trailer::LAYOUT_MARKER..trailer_offset + shifted_trailer::ROLE,
        ) == Some(&[1, 0, 0, 0, 2, 0])
        && payload
            .get(trailer_offset + shifted_trailer::ROLE)
            .is_some_and(|role| *role <= 1)
        && payload
            .get(trailer_offset + shifted_trailer::ZERO_TAIL..trailer_offset + shifted_trailer::LEN)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
}

pub(super) fn shifted_value_only_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    shifted_value_only_scalar_trailer(payload, trailer_offset)
        && payload
            .get(trailer_offset + 35..trailer_offset + 47)
            .is_some_and(|cell| {
                cell[0..2] != [0, 0]
                    && cell[0..2] != [0xff, 0xff]
                    && cell[4..8] == [0xff; 4]
                    && cell[8..12] == [0; 4]
            })
}

pub(super) fn compact_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    !shifted_value_only_scalar_layout(payload, trailer_offset)
        && payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 21)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 21..trailer_offset + 27) == Some(&[1, 0, 0, 0, 2, 0])
        && payload
            .get(trailer_offset + 28..trailer_offset + 35)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 39..trailer_offset + 43) == Some(&[0xff; 4])
        && payload.get(trailer_offset + 47..trailer_offset + 51) == Some(&[0xff; 4])
}

pub(super) fn legacy_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 24)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 24..trailer_offset + 30) == Some(&[0x0f, 0, 0, 0, 2, 0])
}
