// SPDX-License-Identifier: Apache-2.0
//! Expanded-section arenas, feature surface replay associations, and FC05 native records.

use crate::decode::native::CreoArena;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;

use super::coverage::{source_section_ref, surface_family};
use super::native::{emit_uniform, store_arena, UniformArena};
use super::native_records::{
    CreoFc05CircleRecord, CreoFc05CylinderCapPairRecord, CreoFeatureSurfaceReplayAssociation,
    CreoHalfEdgeRef,
};
use super::records::double_xar::CreoDoubleXarTableRecord;
use super::records::{expanded_section_records, CreoPrimitiveScalarArrayRecord};
use super::uniqueness::UniqueById;

pub(super) fn attach_expanded_sections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    // The whole expansion namespace is gated on there being expanded sections at
    // all: with none, the double-xar and primitive-scalar arenas are skipped even
    // when their scan tables are non-empty. Preserve that early return.
    let records = expanded_section_records(scan);
    if records.is_empty() {
        return Ok(());
    }
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::ExpandedSections,
            records: &records,
            id: |record| &record.id,
            stream: |record| &record.name,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.source_offset),
            tag: "unix_compress_expanded_section",
            exactness: Exactness::Derived,
        },
    )?;
    let tables = double_xar_records(scan);
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::DoubleXarTables,
            records: &tables,
            id: |table| &table.id,
            stream: |table| &table.table.section_name,
            offset: |table| cadmpeg_core::decode::u64_from_index(table.table.section_source_offset),
            tag: "model_scalar_dictionary",
            exactness: Exactness::ByteExact,
        },
    )?;
    let primitive_arrays = primitive_scalar_array_records(scan);
    store_arena(ctx, ir, CreoArena::PrimitiveScalarArrays, &primitive_arrays)?;
    Ok(())
}

fn double_xar_records<'a>(scan: &'a ContainerScan<'_>) -> Vec<CreoDoubleXarTableRecord<'a>> {
    let mut records = Vec::new();
    for table in &scan.primitives.double_xar_tables {
        let id = format!(
            "creo:{}:double_xar#{}:{}",
            table.section_name, table.section_source_offset, table.expanded_offset
        );
        records.push(CreoDoubleXarTableRecord { id, table });
    }
    records
}

fn primitive_scalar_array_records<'a>(
    scan: &'a ContainerScan<'_>,
) -> Vec<CreoPrimitiveScalarArrayRecord<'a>> {
    let mut records = Vec::new();
    for array in &scan.primitives.scalar_arrays {
        let id = format!(
            "creo:solid_primdata:scalar_array#{}:{}",
            array.field.as_str(),
            array.offset
        );
        records.push(CreoPrimitiveScalarArrayRecord {
            id,
            field: array.field.as_str(),
            expanded_offset: array.offset,
            count: array.values.len(),
            values: &array.values,
        });
    }
    records
}

pub(super) fn feature_surface_replay_associations(
    scan: &ContainerScan,
) -> Result<Vec<CreoFeatureSurfaceReplayAssociation>, CodecError> {
    let mut associations = Vec::new();
    visit_feature_surface_replays(
        scan,
        |owner_feature_id, table_offset, replay_ordinal, visible, replay| {
            let id = format!(
                "creo:allfeatur:surface_replay#{}:{}:{}:{}",
                owner_feature_id, table_offset, replay_ordinal, visible.id
            );
            associations.push(CreoFeatureSurfaceReplayAssociation {
                id,
                owner_feature_id,
                visible_surface_id: visible.id,
                replay_surface_id: replay.id,
                replay_ordinal,
                surface_family: surface_family(visible.kind),
                table_offset,
            });
            Ok(())
        },
    )?;
    Ok(associations)
}

pub(super) fn feature_surface_replay_association_count(
    scan: &ContainerScan,
) -> Result<usize, CodecError> {
    let mut count = 0usize;
    visit_feature_surface_replays(scan, |_, _, _, _, _| {
        count = count.checked_add(1).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("Creo native surface replay count overflows")
        })?;
        Ok(())
    })?;
    Ok(count)
}

fn visit_feature_surface_replays(
    scan: &ContainerScan,
    mut emit: impl FnMut(
        u32,
        usize,
        usize,
        &crate::surface::SurfaceRow,
        &crate::surface::SurfaceRow,
    ) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let visible_rows = UniqueById::new(&scan.surfaces.rows, |row| row.id);
    let replay_rows = UniqueById::new(&scan.surfaces.nonvisible_rows, |row| row.id);
    for table in &scan.features.entity_tables {
        let owner_feature_id = table.feature_id;
        let visible_count = table
            .entries
            .iter()
            .take_while(|entry| entry.class_id() == 254)
            .count();
        if visible_count == 0 {
            continue;
        }
        let Some(visible) = table.entries[..visible_count]
            .iter()
            .map(|entry| visible_rows.get(entry.entity_id))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        // A visible row of another feature matches no replay window.
        if visible.iter().any(|row| row.feature_id != owner_feature_id) {
            continue;
        }
        let replay = table.entries[visible_count..]
            .iter()
            .map(|entry| {
                (entry.class_id() == 214)
                    .then(|| replay_rows.get(entry.entity_id))
                    .flatten()
                    .filter(|row| row.feature_id == owner_feature_id)
            })
            .collect::<Vec<_>>();
        let pattern = visible.iter().map(|row| row.kind).collect::<Vec<_>>();
        let symbols = replay
            .iter()
            .map(|row| row.map(|row| row.kind))
            .collect::<Vec<_>>();
        for (replay_ordinal, start) in disjoint_occurrences(&pattern, &symbols)
            .into_iter()
            .enumerate()
        {
            for (visible, replay) in visible.iter().zip(&replay[start..start + visible_count]) {
                let replay = replay.ok_or_else(|| {
                    CodecError::malformed("matched nonvisible replay row disappeared")
                })?;
                emit(
                    owner_feature_id,
                    table.offset,
                    replay_ordinal,
                    visible,
                    replay,
                )?;
            }
        }
    }
    Ok(())
}

/// Start positions of the leftmost non-overlapping occurrences of `pattern` in
/// `text`, in order, where a `None` symbol matches nothing.
///
/// Knuth-Morris-Pratt with the match state cleared after each occurrence, so
/// the scan is linear in `pattern` and `text`.
fn disjoint_occurrences<T: PartialEq>(pattern: &[T], text: &[Option<T>]) -> Vec<usize> {
    let mut starts = Vec::new();
    if pattern.is_empty() {
        return starts;
    }
    let mut failure = vec![0usize; pattern.len()];
    let mut matched = 0;
    for index in 1..pattern.len() {
        while matched > 0 && pattern[index] != pattern[matched] {
            matched = failure[matched - 1];
        }
        if pattern[index] == pattern[matched] {
            matched += 1;
        }
        failure[index] = matched;
    }
    matched = 0;
    for (index, symbol) in text.iter().enumerate() {
        let Some(symbol) = symbol else {
            matched = 0;
            continue;
        };
        while matched > 0 && *symbol != pattern[matched] {
            matched = failure[matched - 1];
        }
        if *symbol == pattern[matched] {
            matched += 1;
        }
        if matched == pattern.len() {
            starts.push(index + 1 - matched);
            matched = 0;
        }
    }
    starts
}

pub(super) fn affected_kind(kind: crate::feature::rows::AffectedIdKind) -> &'static str {
    match kind {
        crate::feature::rows::AffectedIdKind::Geometry => "geometry",
        crate::feature::rows::AffectedIdKind::Edges => "edges",
        crate::feature::rows::AffectedIdKind::StrongParents => "strong_parents",
        crate::feature::rows::AffectedIdKind::Parents => "parents",
        crate::feature::rows::AffectedIdKind::Contours => "contours",
        crate::feature::rows::AffectedIdKind::Quilts => "quilts",
    }
}

pub(super) fn extent_source(source: crate::feature::rows::ReplayExtentSource) -> &'static str {
    match source {
        crate::feature::rows::ReplayExtentSource::Explicit => "explicit",
        crate::feature::rows::ReplayExtentSource::Inherited => "inherited",
    }
}

pub(super) fn half_edge_ref(id: crate::topology::HalfEdgeId) -> CreoHalfEdgeRef {
    CreoHalfEdgeRef {
        curve_id: id.curve_id,
        side: id.side,
    }
}

pub(super) fn fc05_circle_records<'a>(
    scan: &'a ContainerScan<'_>,
) -> Vec<CreoFc05CircleRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.curves.fc05_circles {
        let id = format!("creo:curve:fc05_circle#{}", record.curve_id);
        records.push(CreoFc05CircleRecord {
            id,
            curve_id: record.curve_id,
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
            sample_direction_row_frame: record.sample_direction_row_frame.get(),
            angle_parameter: record.angle_parameter,
            cap_ordinate_row_frame: record.cap_ordinate_row_frame,
            point_count: record.point_count,
            max_residual: record.max_residual,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

pub(super) fn fc05_cylinder_cap_pair_records<'a>(
    scan: &'a ContainerScan<'_>,
) -> Vec<CreoFc05CylinderCapPairRecord<'a>> {
    let mut records = Vec::new();
    for record in &scan.curves.fc05_cylinder_cap_pairs {
        let id = format!("creo:surface:fc05_cylinder_cap_pair#{}", record.surface_id);
        records.push(CreoFc05CylinderCapPairRecord {
            id,
            surface_id: record.surface_id,
            cap_edges: &record.cap_edges,
            center_row_frame: record.center_row_frame,
            radius_mm: record.radius_mm,
            reference_direction_row_frame: record.reference_direction_row_frame,
            parameter_sign: record.parameter_sense.as_i8(),
            cap_ordinates_row_frame: &record.cap_ordinates_row_frame,
            offset: record.offset,
            source_section: source_section_ref(scan, record.offset),
        });
    }
    records
}

#[cfg(test)]
mod tests {
    use super::{
        double_xar_records, fc05_circle_records, fc05_cylinder_cap_pair_records,
        feature_surface_replay_association_count, feature_surface_replay_associations,
        primitive_scalar_array_records,
    };

    fn primitive_scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.primitives
            .double_xar_tables
            .push(crate::container::ModelDoubleXarTable {
                section_name: "Body".to_string(),
                section_source_offset: 0,
                expanded_offset: 0,
                body_offset: 0,
                logical_count: 0,
                entries: Vec::new(),
            });
        scan.primitives
            .scalar_arrays
            .push(crate::primdata::PrimitiveScalarArray {
                field: crate::primdata::PrimitiveArrayField::Points,
                offset: 0,
                values: vec![cadmpeg_ir::scalar::FiniteReal::new(2.5).expect("finite scalar")],
            });
        scan.curves.fc05_circles.push(crate::curve::Fc05Circle {
            curve_id: 20,
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length(
                [1.0, 0.0],
            )
            .expect("unit sample direction")
            .0,
            angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
                sense: crate::curve::ParameterSense::Increasing,
                reference_direction_row_frame: [1.0, 0.0],
            },
            cap_ordinate_row_frame: Some(-5.0),
            point_count: 8,
            max_residual: 0.0,
            offset: 0,
        });
        scan.curves
            .fc05_cylinder_cap_pairs
            .push(crate::curve::Fc05CylinderCapPair {
                surface_id: 10,
                cap_edges: vec![
                    crate::curve::Fc05CapEdge {
                        curve_id: 20,
                        cap_plane_id: 11,
                        cap_ordinate_row_frame: -5.0,
                    },
                    crate::curve::Fc05CapEdge {
                        curve_id: 21,
                        cap_plane_id: 12,
                        cap_ordinate_row_frame: 7.0,
                    },
                ],
                center_row_frame: [3.0, 4.0],
                radius_mm: 2.0,
                reference_direction_row_frame: [1.0, 0.0],
                parameter_sense: crate::curve::ParameterSense::Increasing,
                cap_ordinates_row_frame: vec![-5.0, 7.0],
                offset: 0,
            });
        scan
    }

    fn replay_scan() -> crate::container::ContainerScan<'static> {
        use crate::feature::entity::{
            EntryPayload, FeatureEntityTable, FeatureEntityTableEntry, PlainClass,
        };
        let mut scan = crate::test_support::empty_container_scan();
        let entry = |entity_id, class_id| FeatureEntityTableEntry {
            entity_id,
            payload: EntryPayload::Plain {
                class: PlainClass::new(class_id).expect("plain entry class"),
            },
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
        scan.features.entity_tables.push(FeatureEntityTable::new(
            4,
            913,
            vec![entry(7, 254), entry(9, 214)],
            &std::collections::BTreeSet::from([7, 9]),
            0,
        ));
        let row = |id| crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        };
        scan.surfaces.rows.push(row(7));
        scan.surfaces.nonvisible_rows.push(row(9));
        scan
    }

    #[test]
    fn native_surface_replay_row_records_visible_and_replay_surfaces() {
        let scan = replay_scan();
        let records = feature_surface_replay_associations(&scan).expect("replay associations");
        let record = serde_json::to_value(&records[0]).expect("record JSON");
        assert_eq!(record["id"], "creo:allfeatur:surface_replay#4:0:0:7");
        assert_eq!(record["visible_surface_id"], 7);
        assert_eq!(record["replay_surface_id"], 9);
        assert_eq!(record["surface_family"], "plane");
        assert_eq!(
            feature_surface_replay_association_count(&scan).expect("association count"),
            1
        );
    }

    type Association = (u32, usize, usize, u32, u32);

    /// The association rule as a window slid over the replay entries, with
    /// every row looked up by a scan of its namespace.
    fn windowed_associations(scan: &crate::container::ContainerScan<'_>) -> Vec<Association> {
        use crate::surface::unique_surface_row;
        let mut associations = Vec::new();
        for table in &scan.features.entity_tables {
            let owner = table.feature_id;
            let visible_count = table
                .entries
                .iter()
                .take_while(|entry| entry.class_id() == 254)
                .count();
            let visible_entries = &table.entries[..visible_count];
            if visible_count == 0
                || visible_entries
                    .iter()
                    .any(|entry| unique_surface_row(&scan.surfaces.rows, entry.entity_id).is_none())
            {
                continue;
            }
            let replay_entries = &table.entries[visible_count..];
            let mut ordinal = 0;
            let mut cursor = 0;
            while cursor + visible_count <= replay_entries.len() {
                let window = &replay_entries[cursor..cursor + visible_count];
                let rows = visible_entries
                    .iter()
                    .zip(window)
                    .map(|(visible, replay)| {
                        let visible = unique_surface_row(&scan.surfaces.rows, visible.entity_id)?;
                        let replay = (replay.class_id() == 214)
                            .then(|| {
                                unique_surface_row(&scan.surfaces.nonvisible_rows, replay.entity_id)
                            })
                            .flatten()?;
                        (visible.feature_id == owner
                            && replay.feature_id == owner
                            && visible.kind == replay.kind)
                            .then_some((visible.id, replay.id))
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(rows) = rows {
                    for (visible, replay) in rows {
                        associations.push((owner, table.offset, ordinal, visible, replay));
                    }
                    ordinal += 1;
                    cursor += visible_count;
                } else {
                    cursor += 1;
                }
            }
        }
        associations
    }

    fn indexed_associations(scan: &crate::container::ContainerScan<'_>) -> Vec<Association> {
        let mut associations = Vec::new();
        super::visit_feature_surface_replays(scan, |owner, offset, ordinal, visible, replay| {
            associations.push((owner, offset, ordinal, visible.id, replay.id));
            Ok(())
        })
        .expect("visit replays");
        associations
    }

    fn replay_fixture(
        tables: Vec<(u32, Vec<(u32, u32)>)>,
        visible: Vec<(u32, u32, crate::surface::SurfaceKind)>,
        nonvisible: Vec<(u32, u32, crate::surface::SurfaceKind)>,
    ) -> crate::container::ContainerScan<'static> {
        use crate::feature::entity::{
            EntryPayload, FeatureEntityTable, FeatureEntityTableEntry, PlainClass,
        };
        let mut scan = crate::test_support::empty_container_scan();
        for (offset, (feature_id, entries)) in tables.into_iter().enumerate() {
            let entries = entries
                .into_iter()
                .map(|(entity_id, class_id)| FeatureEntityTableEntry {
                    entity_id,
                    payload: EntryPayload::Plain {
                        class: PlainClass::new(class_id).expect("plain entry class"),
                    },
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                })
                .collect();
            scan.features.entity_tables.push(FeatureEntityTable::new(
                feature_id,
                913,
                entries,
                &std::collections::BTreeSet::new(),
                offset,
            ));
        }
        let row = |(id, feature_id, kind)| crate::surface::SurfaceRow {
            id,
            kind,
            feature_id,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        };
        scan.surfaces.rows = visible.into_iter().map(row).collect();
        scan.surfaces.nonvisible_rows = nonvisible.into_iter().map(row).collect();
        scan
    }

    #[test]
    fn indexed_replay_association_matches_the_window_rule() {
        use crate::surface::SurfaceKind;
        let kinds = [SurfaceKind::Plane, SurfaceKind::Cylinder, SurfaceKind::Cone];
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |bound: u32| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            u32::try_from(state % u64::from(bound)).expect("bounded draw")
        };
        let mut compared = 0;
        for _ in 0..400 {
            let mut tables = Vec::new();
            for _ in 0..=next(3) {
                let owner = 1 + next(2);
                let visible_count = next(4);
                let mut entries = Vec::new();
                for _ in 0..visible_count {
                    entries.push((next(6), 254));
                }
                for _ in 0..next(16) {
                    let class = if next(8) == 0 { 7 } else { 214 };
                    entries.push((100 + next(6), class));
                }
                tables.push((owner, entries));
            }
            let mut rows = |base: u32| {
                (0..8)
                    .map(|_| {
                        (
                            base + next(6),
                            1 + next(2),
                            kinds[usize::try_from(next(3)).expect("index")],
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let visible = rows(0);
            let nonvisible = rows(100);
            let scan = replay_fixture(tables, visible, nonvisible);
            let expected = windowed_associations(&scan);
            compared += expected.len();
            assert_eq!(indexed_associations(&scan), expected);
        }
        assert!(compared > 0, "the generated tables produce associations");
    }

    #[test]
    fn long_replay_tables_associate_every_window() {
        use crate::surface::SurfaceKind;
        let visible_count = 2_000_u32;
        let windows = 20_u32;
        let mut entries = (0..visible_count).map(|id| (id, 254)).collect::<Vec<_>>();
        entries.extend((0..visible_count * windows).map(|index| (100_000 + index, 214)));
        let visible = (0..visible_count)
            .map(|id| (id, 4, SurfaceKind::Plane))
            .collect();
        let nonvisible = (0..visible_count * windows)
            .map(|index| (100_000 + index, 4, SurfaceKind::Plane))
            .collect();
        let scan = replay_fixture(vec![(4, entries)], visible, nonvisible);
        let associations = indexed_associations(&scan);
        assert_eq!(
            associations.len(),
            usize::try_from(visible_count * windows).expect("count")
        );
        assert_eq!(
            associations.last().map(|association| association.2),
            Some(usize::try_from(windows - 1).expect("ordinal"))
        );
    }

    #[test]
    fn native_double_xar_row_records_table() {
        let scan = primitive_scan();
        let records = double_xar_records(&scan);
        let value = serde_json::to_value(&records[0]).expect("record JSON");
        assert_eq!(value["id"], "creo:Body:double_xar#0:0");
        assert_eq!(value["count"], 0);
    }

    #[test]
    fn native_scalar_array_row_records_values() {
        let scan = primitive_scan();
        let records = primitive_scalar_array_records(&scan);
        let value = serde_json::to_value(&records[0]).expect("record JSON");
        assert_eq!(value["id"], "creo:solid_primdata:scalar_array#pts:0");
        assert_eq!(value["field"], "pts");
        assert_eq!(value["values"], serde_json::json!([2.5]));
    }

    #[test]
    fn native_fc05_circle_row_records_circle() {
        let scan = primitive_scan();
        let records = fc05_circle_records(&scan);
        let value = serde_json::to_value(&records[0]).expect("record JSON");
        assert_eq!(value["id"], "creo:curve:fc05_circle#20");
        assert_eq!(value["radius_mm"], 2.0);
        assert_eq!(value["point_count"], 8);
    }

    #[test]
    fn native_fc05_cap_pair_row_records_caps() {
        let scan = primitive_scan();
        let records = fc05_cylinder_cap_pair_records(&scan);
        let value = serde_json::to_value(&records[0]).expect("record JSON");
        assert_eq!(value["id"], "creo:surface:fc05_cylinder_cap_pair#10");
        assert_eq!(value["curve_ids"], serde_json::json!([20, 21]));
        assert_eq!(value["cap_plane_ids"], serde_json::json!([11, 12]));
        assert_eq!(
            value["curve_cap_ordinates_row_frame"],
            serde_json::json!([-5.0, 7.0])
        );
        assert_eq!(
            value["cap_ordinates_row_frame"],
            serde_json::json!([-5.0, 7.0])
        );
    }
}
