// SPDX-License-Identifier: Apache-2.0
//! Skamp incidence, symmetry, and fixed-coordinate helpers.

use std::collections::{BTreeMap, BTreeSet};

use super::super::sketch_transfer::{
    active_complete_section_skamps, saved_section_entity_fallback_allowed, section_saved_entity,
    section_skamp_is_line, section_skamp_is_point, unique_bounded_curve_segment,
    unique_centered_line_segment, unique_circle_segment, unique_point_segment,
    unique_reference_line_segment,
};

pub(crate) fn section_line_fixed_coordinate(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<usize> {
    let segment = unique_section_skamp_segment(definition, segment.external_id)?;
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    section_line_entity_fixed_coordinate(definition, segment.external_id)
}

pub(crate) fn section_line_entity_fixed_coordinate(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<usize> {
    section_line_entity_fixed_coordinate_with_mode(definition, entity_id, false)
}

pub(crate) fn section_line_entity_fixed_coordinate_with_unique_rows(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
) -> Option<usize> {
    section_line_entity_fixed_coordinate_with_mode(definition, entity_id, true)
}

fn section_line_entity_fixed_coordinate_with_mode(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
    include_unique_rows: bool,
) -> Option<usize> {
    let mut adjacency = BTreeMap::<u32, Vec<(u32, usize)>>::new();
    for skamp in active_complete_section_skamps(definition) {
        let (parity, first, second) = match (skamp.kind, skamp.items.as_slice()) {
            (5 | 7, [first, second]) if first.sense == 0 && second.sense == 0 => {
                ((skamp.kind == 5) as usize, first, second)
            }
            _ => continue,
        };
        if !section_skamp_is_line(definition, first) || !section_skamp_is_line(definition, second) {
            continue;
        }
        adjacency
            .entry(first.entity_id)
            .or_default()
            .push((second.entity_id, parity));
        adjacency
            .entry(second.entity_id)
            .or_default()
            .push((first.entity_id, parity));
    }
    let mut parities = BTreeMap::from([(entity_id, 0usize)]);
    let mut pending = std::collections::VecDeque::from([entity_id]);
    while let Some(entity_id) = pending.pop_front() {
        let parity = parities[&entity_id];
        for &(neighbor, edge_parity) in adjacency.get(&entity_id).into_iter().flatten() {
            let neighbor_parity = parity ^ edge_parity;
            match parities.get(&neighbor) {
                Some(stored) if *stored != neighbor_parity => return None,
                Some(_) => {}
                None => {
                    parities.insert(neighbor, neighbor_parity);
                    pending.push_back(neighbor);
                }
            }
        }
    }
    let mut coordinates = BTreeSet::new();
    for (entity_id, parity) in parities {
        coordinates.extend(
            section_line_direct_fixed_coordinates_with_mode(
                definition,
                entity_id,
                include_unique_rows,
            )
            .into_iter()
            .map(|coordinate| coordinate ^ parity),
        );
    }
    coordinates
        .first()
        .copied()
        .filter(|_| coordinates.len() == 1)
}

fn section_line_direct_fixed_coordinates_with_mode(
    definition: &crate::feature::FeatureDefinition,
    entity_id: u32,
    include_unique_rows: bool,
) -> BTreeSet<usize> {
    let segment = if include_unique_rows {
        unique_decoded_section_segment(definition, entity_id)
    } else {
        unique_section_skamp_segment(definition, entity_id)
    };
    let mut coordinates = segment
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .and_then(|segment| segment.vertical_horizontal)
        .and_then(|selector| match selector {
            0 => Some(0),
            1 => Some(1),
            _ => None,
        })
        .into_iter()
        .collect::<BTreeSet<_>>();
    coordinates.extend(
        unique_reference_line_segment(definition, entity_id)
            .and_then(|segment| segment.vertical_horizontal)
            .and_then(|selector| match selector {
                0 => Some(0),
                1 => Some(1),
                _ => None,
            }),
    );
    coordinates.extend(
        active_complete_section_skamps(definition).filter_map(|skamp| {
            match (skamp.kind, skamp.items.as_slice()) {
                (1, [item]) if item.sense == 0 && item.entity_id == entity_id => Some(1),
                (2, [item]) if item.sense == 0 && item.entity_id == entity_id => Some(0),
                _ => None,
            }
        }),
    );
    if let Some(crate::feature::FeatureSavedEntity::Line(line)) =
        section_saved_entity(definition, entity_id)
    {
        let [[Some(x0), Some(y0), _], [Some(x1), Some(y1), _]] = line.endpoints else {
            return coordinates;
        };
        let scale = [x0, y0, x1, y1]
            .into_iter()
            .map(f64::abs)
            .fold(1.0, f64::max);
        let tolerance = 1e-9 * scale;
        match [(x0 - x1).abs() <= tolerance, (y0 - y1).abs() <= tolerance] {
            [true, false] => {
                coordinates.insert(0);
            }
            [false, true] => {
                coordinates.insert(1);
            }
            _ => {}
        }
    }
    coordinates
}

pub(crate) fn section_skamp_point_on_line(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, u32, usize)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let selected_point_id = |item: &crate::feature::FeatureSkampItem| {
        section_skamp_selected_point_id(definition, item).or_else(|| {
            section_skamp_selected_point_id_with_ordinary_segment(
                definition,
                item,
                unique_decoded_section_segment(definition, item.entity_id),
            )
        })
    };
    let line_for_item = |item: &crate::feature::FeatureSkampItem| {
        unique_section_skamp_segment(definition, item.entity_id).or_else(|| {
            unique_decoded_section_segment(definition, item.entity_id)
                .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        })
    };
    let pair = match skamp.kind {
        3 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                let line = line_for_item(line_item)?;
                (line_item.sense == 0 && line.kind == crate::feature::FeatureSegmentKind::Line)
                    .then_some((line, selected_point_id(point_item)?))
            }),
        9 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                let line = line_for_item(line_item)?;
                if line_item.sense != 0
                    || point_item.sense != 0
                    || line.kind != crate::feature::FeatureSegmentKind::Line
                    || !section_skamp_is_point(definition, point_item)
                {
                    return None;
                }
                Some((line, selected_point_id(point_item)?))
            }),
        _ => None,
    }?;
    let coordinate = if definition.segments.as_ref()?.is_complete() {
        section_line_fixed_coordinate(definition, pair.0)?
    } else {
        section_line_entity_fixed_coordinate_with_unique_rows(definition, pair.0.external_id)?
    };
    Some((pair.0.point_ids[0], pair.1, coordinate))
}

pub(crate) fn section_skamp_saved_point_on_line(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, usize, f64)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let (line_item, point_id) = match skamp.kind {
        3 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                if line_item.sense != 0 {
                    return None;
                }
                Some((
                    line_item,
                    section_skamp_selected_point_id(definition, point_item)?,
                ))
            }),
        9 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                if line_item.sense != 0
                    || point_item.sense != 0
                    || !section_skamp_is_point(definition, point_item)
                {
                    return None;
                }
                Some((
                    line_item,
                    section_skamp_selected_point_id(definition, point_item)?,
                ))
            }),
        _ => None,
    }?;
    if !saved_section_entity_fallback_allowed(definition, line_item.entity_id) {
        return None;
    }
    let crate::feature::FeatureSavedEntity::Line(line) =
        section_saved_entity(definition, line_item.entity_id)?
    else {
        return None;
    };
    let coordinate = section_line_entity_fixed_coordinate(definition, line_item.entity_id)?;
    Some((
        point_id,
        coordinate,
        saved_line_fixed_coordinate_value(line, coordinate)?,
    ))
}

#[derive(Clone, Copy)]
pub(crate) enum SectionSymmetryAxis {
    Point(u32),
    Value(f64),
}

pub(crate) fn section_skamp_axis_symmetry(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(
    SectionSymmetryAxis,
    SectionPointSource,
    SectionPointSource,
    usize,
)> {
    let (14, [axis_item, first_item, second_item]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    (axis_item.sense == 0 && section_skamp_is_line(definition, axis_item)).then_some(())?;
    let unique_row = unique_decoded_section_segment(definition, axis_item.entity_id);
    let coordinate =
        section_line_entity_fixed_coordinate_with_unique_rows(definition, axis_item.entity_id)?;
    let axis = if let Some(segment) = unique_section_skamp_segment(definition, axis_item.entity_id)
    {
        SectionSymmetryAxis::Point(segment.point_ids[0])
    } else if let Some(segment) = unique_row {
        SectionSymmetryAxis::Point(segment.point_ids[0])
    } else {
        let crate::feature::FeatureSavedEntity::Line(line) =
            section_saved_entity(definition, axis_item.entity_id)?
        else {
            return None;
        };
        SectionSymmetryAxis::Value(saved_line_fixed_coordinate_value(line, coordinate)?)
    };
    Some((
        axis,
        section_skamp_incidence_point(definition, first_item)?,
        section_skamp_incidence_point(definition, second_item)?,
        coordinate,
    ))
}

pub(crate) fn section_skamp_point_symmetry(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, SectionPointSource, SectionPointSource)> {
    let (14, [center, first, second]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    Some((
        section_skamp_point_entity_id(definition, center)?,
        section_skamp_incidence_point(definition, first)?,
        section_skamp_incidence_point(definition, second)?,
    ))
}

pub(crate) fn saved_line_fixed_coordinate_value(
    line: &crate::feature::FeatureSavedLine,
    coordinate: usize,
) -> Option<f64> {
    let [Some(first), Some(second)] =
        [line.endpoints[0][coordinate], line.endpoints[1][coordinate]]
    else {
        return None;
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    ((first - second).abs() <= 1e-9 * scale).then_some(first)
}

#[derive(Clone, Copy)]
pub(crate) enum SectionPointSource {
    Point(u32),
    Value([f64; 2]),
}

pub(crate) fn unique_section_skamp_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSegment> {
    definition.segments.as_ref()?.segment(external_id)
}

pub(crate) fn unique_decoded_section_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSegment> {
    definition.segments.as_ref()?.unique_segment(external_id)
}

pub(crate) fn section_segment_rows(
    definition: &crate::feature::FeatureDefinition,
) -> &[crate::feature::FeatureSegment] {
    definition
        .segments
        .as_ref()
        .map_or(&[], |table| table.rows.as_slice())
}

pub(crate) fn complete_section_segment_rows(
    definition: &crate::feature::FeatureDefinition,
) -> &[crate::feature::FeatureSegment] {
    definition
        .segments
        .as_ref()
        .filter(|table| table.is_complete())
        .map_or(&[], |table| table.rows.as_slice())
}

pub(crate) fn section_skamp_point_entity_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    if let Some(point) = unique_point_segment(definition, item.entity_id) {
        return (item.sense == 0).then_some(point.point_id);
    }
    let segment = unique_decoded_section_segment(definition, item.entity_id)?;
    (item.sense == 0 && segment.kind == crate::feature::FeatureSegmentKind::Point)
        .then_some(segment.point_ids[0])
}

pub(crate) fn section_skamp_selected_point_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    let ordinary_segment = unique_section_skamp_segment(definition, item.entity_id).or_else(|| {
        unique_decoded_section_segment(definition, item.entity_id)
            .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Point)
    });
    section_skamp_selected_point_id_with_ordinary_segment(definition, item, ordinary_segment)
}

pub(crate) fn section_skamp_selected_point_id_with_ordinary_segment(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
    ordinary_segment: Option<&crate::feature::FeatureSegment>,
) -> Option<u32> {
    if let Some(segment) = unique_centered_line_segment(definition, item.entity_id) {
        return match item.sense {
            2 => Some(0),
            3 => Some(1),
            4 => Some(segment.center_id),
            _ => None,
        };
    }
    if let Some(segment) = unique_reference_line_segment(definition, item.entity_id) {
        return match item.sense {
            2 => segment.point_ids[0],
            3 => segment.point_ids[1],
            _ => None,
        };
    }
    if let Some(segment) = unique_bounded_curve_segment(definition, item.entity_id) {
        return match item.sense {
            2 => Some(segment.point_ids[0]),
            3 => Some(segment.point_ids[1]),
            _ => None,
        };
    }
    if let Some(point) = unique_point_segment(definition, item.entity_id) {
        return matches!(item.sense, 0 | 4).then_some(point.point_id);
    }
    if let Some(circle) = unique_circle_segment(definition, item.entity_id) {
        return (item.sense == 4).then_some(circle.center_id);
    }
    let segment = ordinary_segment?;
    if segment.kind == crate::feature::FeatureSegmentKind::Point {
        return matches!(item.sense, 0 | 4).then_some(segment.point_ids[0]);
    }
    match item.sense {
        2 => Some(segment.point_ids[0]),
        3 => Some(segment.point_ids[1]),
        4 => segment.center_id,
        _ => None,
    }
}

pub(crate) fn section_skamp_selected_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SectionPointSource> {
    section_skamp_selected_point_id(definition, item)
        .map(SectionPointSource::Point)
        .or_else(|| saved_section_point(definition, item).map(SectionPointSource::Value))
}

pub(crate) fn section_skamp_incidence_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SectionPointSource> {
    section_skamp_selected_point(definition, item).or_else(|| {
        section_skamp_selected_point_id_with_ordinary_segment(
            definition,
            item,
            unique_decoded_section_segment(definition, item.entity_id),
        )
        .map(SectionPointSource::Point)
    })
}

pub(crate) fn saved_section_point(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<[f64; 2]> {
    if !saved_section_entity_fallback_allowed(definition, item.entity_id) {
        return None;
    }
    let coordinates = match (
        section_saved_entity(definition, item.entity_id)?,
        item.sense,
    ) {
        (crate::feature::FeatureSavedEntity::Line(line), 2) => line.endpoints[0],
        (crate::feature::FeatureSavedEntity::Line(line), 3) => line.endpoints[1],
        (crate::feature::FeatureSavedEntity::Arc(arc), 2) => arc.endpoints[0],
        (crate::feature::FeatureSavedEntity::Arc(arc), 3) => arc.endpoints[1],
        (crate::feature::FeatureSavedEntity::Arc(arc), 4) => arc.center,
        (crate::feature::FeatureSavedEntity::Circle(circle), 4) => circle.center,
        (crate::feature::FeatureSavedEntity::Conic(conic), 4) => {
            let frame = conic.local_system?;
            [Some(frame[9]), Some(frame[10]), Some(frame[11])]
        }
        _ => return None,
    };
    let [Some(u), Some(v), _] = coordinates else {
        return None;
    };
    (u.is_finite() && v.is_finite()).then_some([u, v])
}

#[cfg(test)]
mod tests {
    use super::{
        section_line_entity_fixed_coordinate,
        section_line_entity_fixed_coordinate_with_unique_rows, section_skamp_axis_symmetry,
        section_skamp_point_entity_id, section_skamp_point_on_line, section_skamp_point_symmetry,
        section_skamp_selected_point_id, SectionPointSource,
    };

    fn point_definition(
        declared_count: u32,
        rows: Vec<crate::feature::FeatureSegment>,
        point_rows: Vec<crate::feature::FeaturePointSegment>,
    ) -> crate::feature::FeatureDefinition {
        crate::feature::FeatureDefinition {
            id: 1,
            owner_feature_id: None,
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::FeatureSegmentTable {
                declared_count,
                has_elided_prototype: false,
                entity_ref: None,
                rows,
                circle_rows: Vec::new(),
                point_rows,
                centered_line_rows: Vec::new(),
                reference_line_rows: Vec::new(),
                bounded_curve_rows: Vec::new(),
                conic_rows: Vec::new(),
                opaque_rows: Vec::new(),
                offset: 0,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        }
    }

    fn ordinary_point(
        external_id: u32,
        point_id: u32,
        offset: usize,
    ) -> crate::feature::FeatureSegment {
        crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Point,
            directions: [None; 3],
            point_ids: [point_id; 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset,
        }
    }

    #[test]
    fn unique_ordinary_point_rows_supply_sense_zero_ids_in_incomplete_tables() {
        let incomplete = point_definition(2, vec![ordinary_point(7, 42, 1)], Vec::new());
        assert_eq!(
            section_skamp_point_entity_id(
                &incomplete,
                &crate::feature::FeatureSkampItem {
                    entity_id: 7,
                    sense: 0,
                },
            ),
            Some(42)
        );
        assert_eq!(
            section_skamp_selected_point_id(
                &incomplete,
                &crate::feature::FeatureSkampItem {
                    entity_id: 7,
                    sense: 4,
                },
            ),
            Some(42)
        );

        let duplicate = point_definition(
            2,
            vec![ordinary_point(7, 42, 1), ordinary_point(7, 43, 2)],
            Vec::new(),
        );
        assert_eq!(
            section_skamp_point_entity_id(
                &duplicate,
                &crate::feature::FeatureSkampItem {
                    entity_id: 7,
                    sense: 0,
                },
            ),
            None
        );
        assert_eq!(
            section_skamp_selected_point_id(
                &duplicate,
                &crate::feature::FeatureSkampItem {
                    entity_id: 7,
                    sense: 4,
                },
            ),
            None
        );

        let cross_family_duplicate = point_definition(
            1,
            vec![ordinary_point(7, 42, 1)],
            vec![crate::feature::FeaturePointSegment {
                point_id: 44,
                external_id: 7,
                offset: 2,
            }],
        );
        assert_eq!(
            section_skamp_point_entity_id(
                &cross_family_duplicate,
                &crate::feature::FeatureSkampItem {
                    entity_id: 7,
                    sense: 0,
                },
            ),
            None
        );
        assert_eq!(
            section_skamp_selected_point_id(
                &cross_family_duplicate,
                &crate::feature::FeatureSkampItem {
                    entity_id: 7,
                    sense: 4,
                },
            ),
            None
        );
    }

    #[test]
    fn incomplete_unique_rows_supply_point_symmetry_sources() {
        let line = |external_id, point_ids| crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids,
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: external_id as usize,
        };
        let definition = point_definition(
            4,
            vec![ordinary_point(5, 9, 1), line(10, [1, 2]), line(11, [3, 4])],
            Vec::new(),
        );
        let skamp = crate::feature::FeatureSkamp {
            id: 14,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 5,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 10,
                    sense: 2,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 11,
                    sense: 3,
                },
            ],
            offset: 0,
        };
        let Some((center, first, second)) = section_skamp_point_symmetry(&definition, &skamp)
        else {
            panic!("point-symmetry sources");
        };
        assert_eq!(center, 9);
        assert!(matches!(first, SectionPointSource::Point(1)));
        assert!(matches!(second, SectionPointSource::Point(4)));

        let mut duplicate = definition.clone();
        duplicate
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(line(10, [6, 7]));
        assert!(section_skamp_point_symmetry(&duplicate, &skamp).is_none());

        let mut cross_family = definition;
        cross_family
            .segments
            .as_mut()
            .expect("segments")
            .point_rows
            .push(crate::feature::FeaturePointSegment {
                point_id: 99,
                external_id: 10,
                offset: 99,
            });
        assert!(section_skamp_point_symmetry(&cross_family, &skamp).is_none());
    }

    #[test]
    fn incomplete_unique_rows_supply_axis_symmetry_sources() {
        let line = |external_id, point_ids| crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids,
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: external_id as usize,
        };
        let mut definition =
            point_definition(3, vec![line(10, [1, 2]), line(11, [3, 4])], Vec::new());
        definition.order_table = Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 99,
                internal_id: 20,
                bitmask: 0,
                offset: 1,
            }],
            offset: 0,
        });
        definition.saved_section = Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Line(
                crate::feature::FeatureSavedLine {
                    entity_id: 20,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(0.0), Some(0.0), Some(0.0)],
                        [Some(0.0), Some(2.0), Some(0.0)],
                    ],
                    body: Vec::new(),
                    offset: 2,
                },
            )],
            offset: 2,
        });
        let skamp = crate::feature::FeatureSkamp {
            id: 14,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 10,
                    sense: 2,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 11,
                    sense: 3,
                },
            ],
            offset: 0,
        };
        let Some((axis, first, second, coordinate)) =
            section_skamp_axis_symmetry(&definition, &skamp)
        else {
            panic!("axis-symmetry sources");
        };
        assert!(matches!(axis, super::SectionSymmetryAxis::Value(0.0)));
        assert!(matches!(first, SectionPointSource::Point(1)));
        assert!(matches!(second, SectionPointSource::Point(4)));
        assert_eq!(coordinate, 0);

        let mut duplicate = definition;
        duplicate
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(line(10, [5, 6]));
        assert!(section_skamp_axis_symmetry(&duplicate, &skamp).is_none());

        let mut incomplete_axis = point_definition(
            4,
            vec![line(99, [8, 9]), line(10, [1, 2]), line(11, [3, 4])],
            Vec::new(),
        );
        incomplete_axis.segments.as_mut().expect("segments").rows[0].vertical_horizontal = Some(0);
        assert_eq!(
            section_line_entity_fixed_coordinate(&incomplete_axis, 99),
            None
        );
        assert_eq!(
            section_line_entity_fixed_coordinate_with_unique_rows(&incomplete_axis, 99),
            Some(0)
        );
        let Some((axis, first, second, coordinate)) =
            section_skamp_axis_symmetry(&incomplete_axis, &skamp)
        else {
            panic!("incomplete axis-symmetry sources");
        };
        assert!(matches!(axis, super::SectionSymmetryAxis::Point(8)));
        assert!(matches!(first, SectionPointSource::Point(1)));
        assert!(matches!(second, SectionPointSource::Point(4)));
        assert_eq!(coordinate, 0);

        let mut duplicate_axis = incomplete_axis.clone();
        duplicate_axis
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(line(99, [10, 11]));
        let mut duplicate_axis_skamp = skamp.clone();
        duplicate_axis_skamp.items[0].entity_id = 99;
        assert!(section_skamp_axis_symmetry(&duplicate_axis, &duplicate_axis_skamp).is_none());

        let mut conflicting_orientation = incomplete_axis;
        conflicting_orientation.relations = Some(crate::feature::FeatureRelationTable {
            declared_count: 1,
            entity_ref: None,
            rows: Vec::new(),
            skamps: vec![crate::feature::FeatureSkamp {
                id: 1,
                kind: 1,
                flags: 0,
                status: 1,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                }],
                offset: 0,
            }],
            skamp_header: Some(crate::feature::FeatureSolverTableHeader {
                declared_count: 1,
                entity_ref: 0,
                offset: 0,
            }),
            triples: Vec::new(),
            triples_header: None,
            offset: 0,
        });
        assert!(section_line_entity_fixed_coordinate_with_unique_rows(
            &conflicting_orientation,
            99
        )
        .is_none());
        assert!(section_skamp_axis_symmetry(&conflicting_orientation, &skamp).is_none());
    }

    #[test]
    fn incomplete_unique_rows_supply_point_on_line_sources() {
        let line = |external_id, point_ids, vertical_horizontal| crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids,
            center_id: None,
            arc_orientation: None,
            vertical_horizontal,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: external_id as usize,
        };
        let definition = point_definition(
            4,
            vec![line(10, [1, 2], Some(1)), line(20, [3, 4], None)],
            vec![crate::feature::FeaturePointSegment {
                point_id: 5,
                external_id: 30,
                offset: 30,
            }],
        );
        assert!(!definition
            .segments
            .as_ref()
            .expect("segments")
            .is_complete());

        let type_three = crate::feature::FeatureSkamp {
            id: 3,
            kind: 3,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 10,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 20,
                    sense: 2,
                },
            ],
            offset: 0,
        };
        assert_eq!(
            section_skamp_point_on_line(&definition, &type_three),
            Some((1, 3, 1))
        );

        let mut unary_orientation = definition.clone();
        unary_orientation.segments.as_mut().expect("segments").rows[0].vertical_horizontal = None;
        unary_orientation.relations = Some(crate::feature::FeatureRelationTable {
            declared_count: 1,
            entity_ref: None,
            rows: Vec::new(),
            skamps: vec![crate::feature::FeatureSkamp {
                id: 1,
                kind: 1,
                flags: 0,
                status: 1,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 10,
                    sense: 0,
                }],
                offset: 0,
            }],
            skamp_header: Some(crate::feature::FeatureSolverTableHeader {
                declared_count: 1,
                entity_ref: 0,
                offset: 0,
            }),
            triples: Vec::new(),
            triples_header: None,
            offset: 0,
        });
        assert_eq!(
            section_skamp_point_on_line(&unary_orientation, &type_three),
            Some((1, 3, 1))
        );

        let type_nine = crate::feature::FeatureSkamp {
            id: 9,
            kind: 9,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 10,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 30,
                    sense: 0,
                },
            ],
            offset: 0,
        };
        assert_eq!(
            section_skamp_point_on_line(&definition, &type_nine),
            Some((1, 5, 1))
        );

        let mut duplicate = definition.clone();
        duplicate
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(line(20, [6, 7], None));
        assert!(section_skamp_point_on_line(&duplicate, &type_three).is_none());

        let mut cross_family = definition.clone();
        cross_family
            .segments
            .as_mut()
            .expect("segments")
            .point_rows
            .push(crate::feature::FeaturePointSegment {
                point_id: 8,
                external_id: 20,
                offset: 31,
            });
        assert!(section_skamp_point_on_line(&cross_family, &type_three).is_none());

        let mut missing_selector = definition;
        missing_selector.segments.as_mut().expect("segments").rows[0].vertical_horizontal = None;
        assert!(section_skamp_point_on_line(&missing_selector, &type_three).is_none());
    }
}
