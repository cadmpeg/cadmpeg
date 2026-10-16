// SPDX-License-Identifier: Apache-2.0
use super::Slots;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis,
    AsmHistoricalCylinder, AsmHistoricalEdge, AsmHistoricalOptionalCarrierBinding,
    AsmHistoricalPersistentSubentityTag, AsmHistoricalPlane, AsmHistoricalPoint,
    AsmHistoricalRelation, AsmHistoricalSurfaceAxis, AsmHistoricalSurfaceRadius,
    AsmHistoricalTopology,
};
use std::collections::BTreeMap;

fn group<T>(rows: Vec<T>, key: impl Fn(&T) -> i64) -> BTreeMap<i64, Vec<T>> {
    let mut groups = BTreeMap::<i64, Vec<T>>::new();
    for row in rows {
        groups.entry(key(&row)).or_default().push(row);
    }
    groups
}

fn ordered_rows<T>(rows: &[T], key: impl Fn(&T) -> i64) -> bool {
    rows.windows(2).all(|pair| key(&pair[0]) <= key(&pair[1]))
}

fn project_family<T: Clone>(current: &BTreeMap<i64, Vec<T>>, keys: &Slots) -> Vec<T> {
    if keys.len() < current.len() {
        keys.iter()
            .filter_map(|key| current.get(key))
            .flatten()
            .cloned()
            .collect()
    } else {
        current
            .iter()
            .filter(|(key, _)| keys.contains(key))
            .flat_map(|(_, rows)| rows.iter().cloned())
            .collect()
    }
}

fn replace_family<T: Clone>(
    current: &mut BTreeMap<i64, Vec<T>>,
    rows: &[T],
    keys: &Slots,
    key: impl Fn(&T) -> i64,
) {
    if current.len() < keys.len() {
        current.retain(|slot, _| !keys.contains(slot));
    } else {
        for slot in keys {
            current.remove(slot);
        }
    }
    if rows.len() < keys.len() {
        for row in rows {
            let slot = key(row);
            if keys.contains(&slot) {
                current.entry(slot).or_default().push(row.clone());
            }
        }
    } else {
        for slot in keys {
            let start = rows.partition_point(|row| key(row) < *slot);
            let end = rows.partition_point(|row| key(row) <= *slot);
            if start != end {
                current.insert(*slot, rows[start..end].to_vec());
            }
        }
    }
}

macro_rules! indexed_topology {
    (entities { $($entity:ident: $ety:ty => $ekey:expr),* $(,)? }
     containers { $($container:ident: $cty:ty => $ckey:expr),* $(,)? }) => {
        #[derive(Default)]
        pub(super) struct IndexedTopology {
            $($entity: BTreeMap<i64, Vec<$ety>>),*,
            $($container: BTreeMap<i64, Vec<$cty>>),*
        }
        impl From<AsmHistoricalTopology> for IndexedTopology {
            fn from(topology: AsmHistoricalTopology) -> Self {
                Self {
                    $($entity: group(topology.$entity, $ekey)),*,
                    $($container: group(topology.$container, $ckey)),*
                }
            }
        }
        impl IndexedTopology {
            pub(super) fn ordered(topology: &AsmHistoricalTopology) -> bool {
                $(ordered_rows(&topology.$entity, $ekey))&&*
                    && $(ordered_rows(&topology.$container, $ckey))&&*
            }
            pub(super) fn bodies(&self) -> impl Iterator<Item = &i64> { self.bodies.keys() }
            pub(super) fn materialize(&self) -> AsmHistoricalTopology {
                AsmHistoricalTopology {
                    $($entity: self.$entity.values().flatten().cloned().collect()),*,
                    $($container: self.$container.values().flatten().cloned().collect()),*
                }
            }
            pub(super) fn project(&self, keys: &Slots, containers: bool) -> AsmHistoricalTopology {
                let mut out = AsmHistoricalTopology::default();
                if containers {
                    $(out.$container = project_family(&self.$container, keys);)*
                } else {
                    $(out.$entity = project_family(&self.$entity, keys);)*
                }
                out
            }
            pub(super) fn replace(&mut self, part: AsmHistoricalTopology, keys: &Slots, containers: bool) {
                if containers {
                    $(replace_family(&mut self.$container, &part.$container, keys, $ckey);)*
                } else {
                    $(replace_family(&mut self.$entity, &part.$entity, keys, $ekey);)*
                }
            }
        }
    };
}

indexed_topology! {
    entities {
        faces: i64 => |row: &i64| *row,
        loops: i64 => |row: &i64| *row,
        coedges: i64 => |row: &i64| *row,
        edges: i64 => |row: &i64| *row,
        vertices: i64 => |row: &i64| *row,
        points: i64 => |row: &i64| *row,
        surfaces: i64 => |row: &i64| *row,
        surface_radii: AsmHistoricalSurfaceRadius => |row: &AsmHistoricalSurfaceRadius| row.surface,
        surface_cylinders: AsmHistoricalCylinder => |row: &AsmHistoricalCylinder| row.surface,
        surface_planes: AsmHistoricalPlane => |row: &AsmHistoricalPlane| row.surface,
        surface_axes: AsmHistoricalSurfaceAxis => |row: &AsmHistoricalSurfaceAxis| row.surface,
        curves: i64 => |row: &i64| *row,
        curve_axes: AsmHistoricalCurveAxis => |row: &AsmHistoricalCurveAxis| row.curve,
        pcurves: i64 => |row: &i64| *row,
        persistent_subentity_tags: AsmHistoricalPersistentSubentityTag => |row: &AsmHistoricalPersistentSubentityTag| row.entity_ref,
        face_loops: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        loop_coedges: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        coedge_topology: AsmHistoricalCoedge => |row: &AsmHistoricalCoedge| row.coedge,
        edge_vertices: AsmHistoricalEdge => |row: &AsmHistoricalEdge| row.edge,
        face_surfaces: AsmHistoricalCarrierBinding => |row: &AsmHistoricalCarrierBinding| row.entity,
        edge_curves: AsmHistoricalOptionalCarrierBinding => |row: &AsmHistoricalOptionalCarrierBinding| row.entity,
        coedge_pcurves: AsmHistoricalOptionalCarrierBinding => |row: &AsmHistoricalOptionalCarrierBinding| row.entity,
        vertex_points: AsmHistoricalCarrierBinding => |row: &AsmHistoricalCarrierBinding| row.entity,
        point_positions: AsmHistoricalPoint => |row: &AsmHistoricalPoint| row.point,
    }
    containers {
        bodies: i64 => |row: &i64| *row,
        regions: i64 => |row: &i64| *row,
        shells: i64 => |row: &i64| *row,
        body_regions: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        region_shells: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_faces: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_wire_edges: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_free_vertices: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_records::{AsmHistoricalTransition, AsmStateSnapshot};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    #[test]
    fn keyed_chain_matches_full_diffs_with_repeats_and_tag_groups() {
        let tag = |ordinal| AsmHistoricalPersistentSubentityTag {
            entity_kind: AsmHistoricalEntityKind::Face,
            entity_ref: 1,
            selector: 7,
            token: "face".into(),
            design_references: vec![8],
            ordinal,
        };
        let mut newer = AsmHistoricalTopology {
            faces: vec![1],
            surfaces: vec![10, 10, 20],
            curves: vec![40, 40],
            persistent_subentity_tags: vec![tag(0), tag(1)],
            ..Default::default()
        };
        let mut indexed = IndexedTopology::from(newer.clone());
        let mut newer_versions = BTreeMap::from([(1, 1), (10, 10), (20, 20), (40, 40)]);
        let keys = Slots::from([1, 10, 40]);
        for state_id in [2, 1] {
            let mut older = newer.clone();
            older.surfaces.retain(|slot| *slot != 10);
            older.surfaces.insert(0, 10);
            older.curves.clear();
            older.persistent_subentity_tags = vec![tag(2)];
            let mut older_versions = newer_versions.clone();
            for slot in &keys {
                older_versions.insert(*slot, *slot + state_id * 100);
            }
            let newer_part = indexed.project(&keys, false);
            indexed.replace(older.clone(), &keys, false);
            let older_part = indexed.project(&keys, false);
            assert_eq!(indexed.materialize(), older);
            let old = AsmStateSnapshot {
                state_id,
                topology: &older,
                versions: &older_versions,
            };
            let new = AsmStateSnapshot {
                state_id: state_id + 1,
                topology: &newer,
                versions: &newer_versions,
            };
            assert_eq!(
                super::super::transition_for_changes(
                    AsmStateSnapshot {
                        topology: &older_part,
                        ..old
                    },
                    AsmStateSnapshot {
                        topology: &newer_part,
                        ..new
                    },
                    &keys,
                ),
                AsmHistoricalTransition::between(Some(old), new),
            );
            newer = older;
            newer_versions = older_versions;
        }
    }
    #[test]
    fn sparse_edit_preserves_other_slots() {
        let mut indexed = IndexedTopology::from(AsmHistoricalTopology {
            surfaces: vec![10, 20, 30],
            ..Default::default()
        });
        let keys = Slots::from([20]);
        assert_eq!(indexed.project(&keys, false).surfaces, vec![20]);
        indexed.replace(
            AsmHistoricalTopology {
                surfaces: vec![20, 20],
                ..Default::default()
            },
            &keys,
            false,
        );
        assert_eq!(indexed.materialize().surfaces, vec![10, 20, 20, 30]);
        assert_eq!(indexed.project(&keys, false).surfaces, vec![20, 20]);
    }
}
