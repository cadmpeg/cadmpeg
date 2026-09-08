// SPDX-License-Identifier: Apache-2.0
//! Loss counts retained from ASM B-rep admission.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Counts used to construct the B-rep loss report.
#[derive(Default)]
pub struct Stats {
    /// Omitted face counts by null/dangling surface-reference condition.
    pub missing_face_surface_kinds: std::collections::BTreeMap<String, usize>,
    /// Undecoded face-surface counts by owned native construction kind, or by
    /// record head when the record owns no construction subtype.
    pub unknown_surface_kinds: std::collections::BTreeMap<String, usize>,
    /// Faces whose surface record explicitly delegates shape to mesh attributes.
    pub mesh_surface_faces: usize,
    /// Spline surface records whose cached B-spline block was decoded into a
    /// NURBS carrier.
    pub nurbs_surfaces: usize,
    /// Procedural curve records whose cached 3D B-spline block was decoded into
    /// a NURBS carrier.
    pub nurbs_curves: usize,
    /// Undecoded edge-curve counts by full native record name.
    pub procedural_curve_kinds: std::collections::BTreeMap<String, usize>,
    /// Undecoded coedge-pcurve counts by full native record name.
    pub undecoded_pcurve_kinds: std::collections::BTreeMap<String, usize>,
    /// Procedural blends for which only one of two support families resolved.
    pub partial_procedural_supports: usize,
    /// Residual record counts by full record name.
    pub other_record_kinds: std::collections::BTreeMap<String, usize>,
}

impl Stats {
    /// Total count represented by `missing_face_surface_kinds`.
    #[must_use]
    pub fn missing_face_surfaces(&self) -> usize {
        self.missing_face_surface_kinds.values().sum()
    }

    /// Total count represented by `unknown_surface_kinds`.
    #[must_use]
    pub fn unknown_surface_faces(&self) -> usize {
        self.unknown_surface_kinds.values().sum()
    }

    /// Total count represented by `procedural_curve_kinds`.
    #[must_use]
    pub fn procedural_curve_edges(&self) -> usize {
        self.procedural_curve_kinds.values().sum()
    }

    /// Total count represented by `undecoded_pcurve_kinds`.
    #[must_use]
    pub fn undecoded_pcurve_refs(&self) -> usize {
        self.undecoded_pcurve_kinds.values().sum()
    }

    /// Total count represented by `other_record_kinds`.
    #[must_use]
    pub fn other_records(&self) -> usize {
        self.other_record_kinds.values().sum()
    }

    pub(super) fn merge(&mut self, other: Self) {
        macro_rules! add_counts {
            ($($field:ident),+ $(,)?) => {
                $(self.$field += other.$field;)+
            };
        }
        add_counts!(
            mesh_surface_faces,
            nurbs_surfaces,
            nurbs_curves,
            partial_procedural_supports,
        );
        for (target, source) in [
            (
                &mut self.missing_face_surface_kinds,
                other.missing_face_surface_kinds,
            ),
            (&mut self.unknown_surface_kinds, other.unknown_surface_kinds),
            (
                &mut self.procedural_curve_kinds,
                other.procedural_curve_kinds,
            ),
            (
                &mut self.undecoded_pcurve_kinds,
                other.undecoded_pcurve_kinds,
            ),
            (&mut self.other_record_kinds, other.other_record_kinds),
        ] {
            for (kind, count) in source {
                *target.entry(kind).or_default() += count;
            }
        }
    }
}

#[derive(Deserialize)]
struct StatsWire {
    missing_face_surfaces: usize,
    missing_face_surface_kinds: std::collections::BTreeMap<String, usize>,
    unknown_surface_faces: usize,
    unknown_surface_kinds: std::collections::BTreeMap<String, usize>,
    mesh_surface_faces: usize,
    nurbs_surfaces: usize,
    nurbs_curves: usize,
    procedural_curve_edges: usize,
    procedural_curve_kinds: std::collections::BTreeMap<String, usize>,
    undecoded_pcurve_refs: usize,
    undecoded_pcurve_kinds: std::collections::BTreeMap<String, usize>,
    partial_procedural_supports: usize,
    other_records: usize,
    other_record_kinds: std::collections::BTreeMap<String, usize>,
}

impl Serialize for Stats {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_struct("Stats", 14)?;
        wire.serialize_field("missing_face_surfaces", &self.missing_face_surfaces())?;
        wire.serialize_field(
            "missing_face_surface_kinds",
            &self.missing_face_surface_kinds,
        )?;
        wire.serialize_field("unknown_surface_faces", &self.unknown_surface_faces())?;
        wire.serialize_field("unknown_surface_kinds", &self.unknown_surface_kinds)?;
        wire.serialize_field("mesh_surface_faces", &self.mesh_surface_faces)?;
        wire.serialize_field("nurbs_surfaces", &self.nurbs_surfaces)?;
        wire.serialize_field("nurbs_curves", &self.nurbs_curves)?;
        wire.serialize_field("procedural_curve_edges", &self.procedural_curve_edges())?;
        wire.serialize_field("procedural_curve_kinds", &self.procedural_curve_kinds)?;
        wire.serialize_field("undecoded_pcurve_refs", &self.undecoded_pcurve_refs())?;
        wire.serialize_field("undecoded_pcurve_kinds", &self.undecoded_pcurve_kinds)?;
        wire.serialize_field(
            "partial_procedural_supports",
            &self.partial_procedural_supports,
        )?;
        wire.serialize_field("other_records", &self.other_records())?;
        wire.serialize_field("other_record_kinds", &self.other_record_kinds)?;
        wire.end()
    }
}
impl<'de> Deserialize<'de> for Stats {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = StatsWire::deserialize(deserializer)?;
        for (field, map_field, count, kinds) in [
            (
                "missing_face_surfaces",
                "missing_face_surface_kinds",
                wire.missing_face_surfaces,
                &wire.missing_face_surface_kinds,
            ),
            (
                "unknown_surface_faces",
                "unknown_surface_kinds",
                wire.unknown_surface_faces,
                &wire.unknown_surface_kinds,
            ),
            (
                "procedural_curve_edges",
                "procedural_curve_kinds",
                wire.procedural_curve_edges,
                &wire.procedural_curve_kinds,
            ),
            (
                "undecoded_pcurve_refs",
                "undecoded_pcurve_kinds",
                wire.undecoded_pcurve_refs,
                &wire.undecoded_pcurve_kinds,
            ),
            (
                "other_records",
                "other_record_kinds",
                wire.other_records,
                &wire.other_record_kinds,
            ),
        ] {
            if kinds
                .values()
                .try_fold(0_usize, |sum, value| sum.checked_add(*value))
                != Some(count)
            {
                return Err(serde::de::Error::custom(format!(
                    "{field} must equal the sum of {map_field}"
                )));
            }
        }
        Ok(Self {
            missing_face_surface_kinds: wire.missing_face_surface_kinds,
            unknown_surface_kinds: wire.unknown_surface_kinds,
            mesh_surface_faces: wire.mesh_surface_faces,
            nurbs_surfaces: wire.nurbs_surfaces,
            nurbs_curves: wire.nurbs_curves,
            procedural_curve_kinds: wire.procedural_curve_kinds,
            undecoded_pcurve_kinds: wire.undecoded_pcurve_kinds,
            partial_procedural_supports: wire.partial_procedural_supports,
            other_record_kinds: wire.other_record_kinds,
        })
    }
}
