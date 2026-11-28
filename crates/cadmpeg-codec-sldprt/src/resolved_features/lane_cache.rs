// SPDX-License-Identifier: Apache-2.0
//! Per-lane cached native payload scans shared within one decode or projection pass.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::index_from_u64;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::records::{FeatureHistory, FeatureInputLane, SketchInputEntity};

use super::compact_reference_planes::CompactReferencePlaneIndex;
use super::holes::feature_object_byte_ranges;
use super::profiles::{markers_by_owner, TerminalRelationCarriers};
use super::reference_geometry::explicit_reference_plane_frame;
use super::relation_geometry::LaneIds;
use super::relation_table::{handle_declarations, lane_tables, SketchTable};
use super::scalars::FeatureObjectNames;
use super::sketch_lists::{unchecked_record_conflict, ChordConflict, SketchRecordLists};

type PlaneFrame = (Point3, Vector3, Vector3);

/// Cached immutable scans derived from a single [`FeatureInputLane`] and
/// document [`FeatureHistory`] slice.
pub(crate) struct CachedLane<'a> {
    lane: &'a FeatureInputLane,
    histories: &'a [FeatureHistory],
    plane_index: OnceCell<CompactReferencePlaneIndex>,
    record_lists: OnceCell<SketchRecordLists>,
    relation_tables: OnceCell<Vec<SketchTable>>,
    handle_declarations: OnceCell<Vec<(usize, String)>>,
    curve_markers: OnceCell<HashSet<&'a str>>,
    feature_ranges: OnceCell<HashMap<&'a str, (usize, usize, usize)>>,
    names: OnceCell<FeatureObjectNames<'a>>,
    markers_by_feature: OnceCell<HashMap<&'a str, Vec<&'a SketchInputEntity>>>,
    terminal_carriers: OnceCell<TerminalRelationCarriers<'a>>,
    unchecked_conflicts: OnceCell<Vec<(&'a str, ChordConflict)>>,
    lane_ids: OnceCell<LaneIds<'a>>,
    explicit_frames: RefCell<HashMap<(usize, usize), Option<PlaneFrame>>>,
}

impl<'a> CachedLane<'a> {
    fn new(histories: &'a [FeatureHistory], lane: &'a FeatureInputLane) -> Self {
        Self {
            lane,
            histories,
            plane_index: OnceCell::new(),
            record_lists: OnceCell::new(),
            relation_tables: OnceCell::new(),
            handle_declarations: OnceCell::new(),
            curve_markers: OnceCell::new(),
            feature_ranges: OnceCell::new(),
            names: OnceCell::new(),
            markers_by_feature: OnceCell::new(),
            terminal_carriers: OnceCell::new(),
            unchecked_conflicts: OnceCell::new(),
            lane_ids: OnceCell::new(),
            explicit_frames: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn plane_index(&self) -> &CompactReferencePlaneIndex {
        self.plane_index
            .get_or_init(|| CompactReferencePlaneIndex::new(&self.lane.native_payload))
    }

    pub(crate) fn record_lists(&self) -> &SketchRecordLists {
        self.record_lists
            .get_or_init(|| SketchRecordLists::new(&self.lane.native_payload))
    }

    pub(crate) fn relation_tables(&self) -> &[SketchTable] {
        self.relation_tables
            .get_or_init(|| lane_tables(&self.lane.native_payload))
    }

    pub(crate) fn handle_declarations(&self) -> &[(usize, String)] {
        self.handle_declarations
            .get_or_init(|| handle_declarations(&self.lane.native_payload))
    }

    pub(crate) fn curve_markers(&self) -> &HashSet<&'a str> {
        self.curve_markers.get_or_init(|| {
            let lists = self.record_lists();
            self.lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    usize::try_from(marker.offset())
                        .is_ok_and(|offset| lists.is_curve(&self.lane.native_payload, offset))
                })
                .map(SketchInputEntity::id)
                .collect()
        })
    }

    pub(crate) fn feature_ranges(&self) -> &HashMap<&'a str, (usize, usize, usize)> {
        self.feature_ranges
            .get_or_init(|| feature_object_byte_ranges(self.histories, self.lane))
    }

    pub(crate) fn names(&self) -> &FeatureObjectNames<'a> {
        self.names
            .get_or_init(|| FeatureObjectNames::new(&self.lane.names))
    }

    pub(crate) fn markers_by_feature(&self) -> &HashMap<&'a str, Vec<&'a SketchInputEntity>> {
        self.markers_by_feature
            .get_or_init(|| markers_by_owner(&self.lane.sketch_entities))
    }

    pub(crate) fn terminal_carriers(&self) -> &TerminalRelationCarriers<'a> {
        self.terminal_carriers
            .get_or_init(|| TerminalRelationCarriers::new(self.lane))
    }

    pub(crate) fn unchecked_conflicts(&self) -> &[(&'a str, ChordConflict)] {
        self.unchecked_conflicts.get_or_init(|| {
            let lists = self.record_lists();
            self.lane
                .sketch_entities
                .iter()
                .filter_map(|marker| {
                    let offset = index_from_u64(marker.offset())?;
                    let conflict =
                        unchecked_record_conflict(&self.lane.native_payload, lists, offset)?;
                    Some((marker.id(), conflict))
                })
                .collect()
        })
    }

    pub(crate) fn lane_ids(&self) -> &LaneIds<'a> {
        self.lane_ids.get_or_init(|| LaneIds::new(self.lane))
    }

    pub(crate) fn explicit_frame(&self, start: usize, end: usize) -> Option<PlaneFrame> {
        if let Some(&cached) = self.explicit_frames.borrow().get(&(start, end)) {
            return cached;
        }
        let frame = self
            .lane
            .native_payload
            .get(start..end)
            .and_then(|object| explicit_reference_plane_frame(object).ok().flatten());
        self.explicit_frames
            .borrow_mut()
            .insert((start, end), frame);
        frame
    }
}

/// Per-pass cache of lane scans keyed by lane pointer identity within `lanes`.
pub(crate) struct LaneScanCache<'a> {
    entries: Vec<CachedLane<'a>>,
    by_ptr: HashMap<usize, usize>,
}

impl<'a> LaneScanCache<'a> {
    pub(crate) fn new(histories: &'a [FeatureHistory], lanes: &'a [FeatureInputLane]) -> Self {
        let mut entries = Vec::with_capacity(lanes.len());
        let mut by_ptr = HashMap::with_capacity(lanes.len());
        for (index, lane) in lanes.iter().enumerate() {
            by_ptr.insert(std::ptr::from_ref(lane).addr(), index);
            entries.push(CachedLane::new(histories, lane));
        }
        Self { entries, by_ptr }
    }

    pub(crate) fn get(&self, lane: &FeatureInputLane) -> Option<&CachedLane<'a>> {
        self.by_ptr
            .get(&std::ptr::from_ref(lane).addr())
            .and_then(|&index| self.entries.get(index))
    }
}

/// Optional per-pass lane scan cache and precomputed datum plane frames.
#[derive(Clone, Copy, Default)]
pub(crate) struct CachedLaneContext<'cache, 'frames> {
    pub(crate) cache: Option<&'cache LaneScanCache<'cache>>,
    pub(crate) plane_frames:
        Option<&'frames HashMap<usize, HashMap<u32, super::curves::SketchPlaneFrame>>>,
}

/// Feature input lanes paired with optional cached scans and plane frames.
#[derive(Clone, Copy)]
pub(crate) struct CachedLanes<'lanes, 'cache, 'frames> {
    pub(crate) lanes: &'lanes [FeatureInputLane],
    pub(crate) context: CachedLaneContext<'cache, 'frames>,
}
