// SPDX-License-Identifier: Apache-2.0
//! Resolve an Extrude sketch-profile frame on a spatial Sketch to spatial
//! profile indices.
//!
//! The candidate profiles come from the stored selection: the scope's
//! selection groups, the frame's nested region selection, or every profile
//! when the frame has neither. Profiles whose boundaries trace the same
//! geometry as an earlier candidate are one region and are named once. When
//! the far caps of the Extrude transition certify the union of the
//! candidates, the candidates are stated as certified. Candidates from the
//! selection groups or the nested region selection, and the one profile of a
//! Sketch with one profile, are stated as assumed when the transition does
//! not certify them. Every profile of a Sketch with more than one profile, an
//! unresolved ring, and an unresolved group keep the frame native.

use super::cap_regions::spatial::profiles_agree;
use super::{coincident_spatial_profile_geometry, spatial_profile_member_entity};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::{
    DesignSketchProfileOperand, DesignSketchProfileRegion, DesignSketchProfileRegionSelection,
};
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometryDefinition,
};

/// Inputs that bind one spatial profile frame.
pub(super) struct SpatialFrame<'a> {
    pub(super) stream: &'a str,
    pub(super) operand: &'a DesignSketchProfileOperand,
    pub(super) sketch: &'a SpatialSketch,
    pub(super) curve_identities: &'a [SketchCurveIdentity],
    pub(super) entities: &'a [SpatialSketchEntity],
    pub(super) histories: &'a [crate::history_records::AsmHistory],
    pub(super) state_ids: Option<(i64, i64)>,
    pub(super) linear_tolerance: f64,
    pub(super) angular_tolerance: f64,
}

/// The spatial profile indices of a frame, and whether the Extrude
/// transition certified them.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SpatialFrameProfiles {
    /// The far caps of the Extrude transition certify the union of the
    /// profiles.
    Certified(Vec<u32>),
    /// The stored selection, or the only profile of the Sketch, names the
    /// profiles; the transition does not certify them.
    Assumed(Vec<u32>),
}

/// The spatial profile indices of `frame`, from `grouped` when the scope's
/// selection groups resolved them. `None` keeps the frame native.
pub(super) fn certified_profiles(
    frame: &SpatialFrame<'_>,
    grouped: Option<Vec<u32>>,
) -> Option<SpatialFrameProfiles> {
    let (candidates, stored) = match grouped {
        Some(grouped) => (grouped, true),
        None if frame.operand.region_selection.is_some() => (stored_region_profiles(frame)?, true),
        None => {
            let count = u32::try_from(frame.sketch.profiles.len()).ok()?;
            ((0..count).collect(), count == 1)
        }
    };
    let distinct = distinct_profiles(frame, &candidates)?;
    let certified = frame.state_ids.is_some_and(|state_ids| {
        profiles_agree(
            frame.sketch,
            frame.entities,
            &distinct,
            frame.histories,
            state_ids,
            frame.linear_tolerance,
        )
    });
    if certified {
        Some(SpatialFrameProfiles::Certified(distinct))
    } else {
        stored.then_some(SpatialFrameProfiles::Assumed(distinct))
    }
}

/// The profiles that the nested region selection names, in selection order.
///
/// Each selected region is one boundary ring, or a set of atomic regions of
/// one ring each that name every curve of the selected region. Each ring
/// must name exactly the curves of one spatial profile boundary. A selected
/// region with hole rings, a member that names no curve of the Sketch, or a
/// ring that is not one profile boundary leaves the selection unresolved.
fn stored_region_profiles(frame: &SpatialFrame<'_>) -> Option<Vec<u32>> {
    let selection = frame.operand.region_selection.as_ref()?;
    let owner = u32::try_from(frame.operand.entity_id.suffix()).ok()?;
    let mut ordinals = Vec::new();
    for region in &selection.regions {
        if !ordinals.contains(&region.region_ordinal) {
            ordinals.push(region.region_ordinal);
        }
    }
    let mut resolved = Vec::new();
    for ordinal in ordinals {
        let own: Vec<&DesignSketchProfileRegion> = selection
            .regions
            .iter()
            .filter(|region| region.region_ordinal == ordinal)
            .collect();
        let atomic: Vec<_> = selection
            .atomic_regions
            .iter()
            .filter(|atomic| atomic.region_ordinal == ordinal)
            .collect();
        let rings: Vec<&DesignSketchProfileRegion> = if atomic.is_empty() {
            let [ring] = own.as_slice() else {
                return None;
            };
            vec![*ring]
        } else {
            let mut rings = Vec::new();
            for region in atomic {
                let [ring] = region.loops.as_slice() else {
                    return None;
                };
                rings.push(ring);
            }
            let covered = own.iter().flat_map(|ring| &ring.members).all(|member| {
                rings.iter().flat_map(|ring| &ring.members).any(|atomic| {
                    atomic.curve_primary_id == member.curve_primary_id
                        && atomic.curve_secondary_id == member.curve_secondary_id
                })
            });
            if !covered {
                return None;
            }
            rings
        };
        for ring in rings {
            let index = ring_profile(frame, owner, ring)?;
            if !resolved.contains(&index) {
                resolved.push(index);
            }
        }
    }
    (!resolved.is_empty()).then_some(resolved)
}

/// Whether every selected region of `selection` is one loop, or atomic
/// regions of one loop each.
pub(super) fn regions_without_holes(selection: &DesignSketchProfileRegionSelection) -> bool {
    let mut ordinals = Vec::new();
    for region in &selection.regions {
        if !ordinals.contains(&region.region_ordinal) {
            ordinals.push(region.region_ordinal);
        }
    }
    ordinals.into_iter().all(|ordinal| {
        let mut atomic = selection
            .atomic_regions
            .iter()
            .filter(|atomic| atomic.region_ordinal == ordinal)
            .peekable();
        if atomic.peek().is_some() {
            atomic.all(|atomic| atomic.loops.len() == 1)
        } else {
            selection
                .regions
                .iter()
                .filter(|region| region.region_ordinal == ordinal)
                .count()
                == 1
        }
    })
}

/// The spatial profile whose boundary curves are exactly the curves `ring`
/// names. A member names a boundary curve directly, or names a curve that
/// traces the same geometry as that boundary curve.
fn ring_profile(
    frame: &SpatialFrame<'_>,
    owner: u32,
    ring: &DesignSketchProfileRegion,
) -> Option<u32> {
    let mut named: Vec<&SpatialSketchEntity> = Vec::new();
    for member in &ring.members {
        named.push(spatial_profile_member_entity(
            frame.stream,
            owner,
            member,
            frame.sketch,
            frame.curve_identities,
            frame.entities,
        )?);
    }
    let first = named.first()?;
    let mut matches = frame
        .sketch
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| {
            profile
                .boundary()
                .iter()
                .any(|use_| &use_.entity == first.id())
        });
    let (index, profile) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let mut boundary = Vec::new();
    for use_ in profile.boundary() {
        let mut curves = frame
            .entities
            .iter()
            .filter(|entity| entity.sketch == frame.sketch.id && entity.id() == &use_.entity);
        let curve = curves.next()?;
        if curves.next().is_some() {
            return None;
        }
        boundary.push(curve);
    }
    let traces = |member: &SpatialSketchEntity, curve: &SpatialSketchEntity| {
        member.id() == curve.id() || same_curve(frame, &member.geometry, &curve.geometry)
    };
    let exact = named
        .iter()
        .all(|member| boundary.iter().any(|curve| traces(member, curve)))
        && boundary
            .iter()
            .all(|curve| named.iter().any(|member| traces(member, curve)));
    exact.then(|| u32::try_from(index).ok()).flatten()
}

/// `candidates` without each profile whose boundary traces the same geometry
/// as an earlier kept candidate. `None` when a candidate index is out of
/// range or a boundary curve is missing.
pub(super) fn distinct_profiles(frame: &SpatialFrame<'_>, candidates: &[u32]) -> Option<Vec<u32>> {
    let mut kept: Vec<u32> = Vec::new();
    for candidate in candidates {
        let profile = frame
            .sketch
            .profiles
            .get(usize::try_from(*candidate).ok()?)?;
        let mut duplicate = false;
        for earlier in &kept {
            let earlier = frame.sketch.profiles.get(usize::try_from(*earlier).ok()?)?;
            if same_boundary(frame, profile.boundary(), earlier.boundary())? {
                duplicate = true;
                break;
            }
        }
        if !duplicate && !kept.contains(candidate) {
            kept.push(*candidate);
        }
    }
    (!kept.is_empty()).then_some(kept)
}

/// Whether two profile boundaries have the same number of curves and each
/// curve of one traces the geometry of a curve of the other.
fn same_boundary(
    frame: &SpatialFrame<'_>,
    first: &[cadmpeg_ir::sketches::SpatialSketchEntityUse],
    second: &[cadmpeg_ir::sketches::SpatialSketchEntityUse],
) -> Option<bool> {
    if first.len() != second.len() {
        return Some(false);
    }
    let geometry = |id: &SpatialSketchEntityId| {
        let mut matches = frame
            .entities
            .iter()
            .filter(|entity| entity.sketch == frame.sketch.id && entity.id() == id);
        let entity = matches.next()?;
        matches.next().is_none().then_some(&entity.geometry)
    };
    for (one, other) in [(first, second), (second, first)] {
        for use_ in one {
            let curve = geometry(&use_.entity)?;
            let mut traced = false;
            for candidate in other {
                if same_curve(frame, curve, geometry(&candidate.entity)?) {
                    traced = true;
                    break;
                }
            }
            if !traced {
                return Some(false);
            }
        }
    }
    Some(true)
}

/// Whether two boundary curves trace the same point set: lines with the same
/// end points in either order, or coincident full circles.
fn same_curve(
    frame: &SpatialFrame<'_>,
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
) -> bool {
    let tolerance = frame.linear_tolerance;
    match (first.definition(), second.definition()) {
        (
            SpatialSketchGeometryDefinition::Line { start, end },
            SpatialSketchGeometryDefinition::Line {
                start: other_start,
                end: other_end,
            },
        ) => {
            let near = |a: cadmpeg_ir::math::Point3, b: cadmpeg_ir::math::Point3| {
                a.vector_from(b).norm() <= tolerance
            };
            let (start, end, other_start, other_end) =
                (start.get(), end.get(), other_start.get(), other_end.get());
            (near(start, other_start) && near(end, other_end))
                || (near(start, other_end) && near(end, other_start))
        }
        _ => coincident_spatial_profile_geometry(first, second, tolerance, frame.angular_tolerance),
    }
}

#[cfg(test)]
mod tests;
