// SPDX-License-Identifier: Apache-2.0
//! Coedge runs: maximal boundary half-edge sequences between kept vertices.

use super::geom::{Slot, NONE};
use super::loops::Loops;
use super::mesh::Mesh;
use super::MeshBrepError;

/// One coedge: a chain of boundary half-edges of one face loop.
pub(super) struct Run {
    pub(super) start: usize,
    pub(super) end: usize,
    /// The run on the other side of the same edge, or the run itself when the
    /// edge has only one coedge.
    pub(super) partner: usize,
    /// Cleared when the face that owned the run was removed.
    pub(super) live: bool,
}

/// All runs, plus each face loop as a ring of run indices.
pub(super) struct RunSet {
    pub(super) runs: Vec<Run>,
    /// Per region, per loop, the runs in ring order.
    pub(super) rings: Vec<Vec<Vec<usize>>>,
}

/// Split every loop at the kept vertices and pair the runs across edges.
///
/// Returns `None` when the kept set does not give a consistent B-rep: a loop
/// without a kept vertex, a run that starts and ends at one vertex, or two
/// sides of an edge that do not agree on their extent. The caller then
/// retries with every boundary vertex kept, where each run is one half-edge.
pub(super) fn build(
    mesh: &Mesh,
    twin: &[Option<usize>],
    loops: &Loops,
    keep: &[bool],
) -> Result<Option<RunSet>, MeshBrepError> {
    let half_edges = mesh.tris.len() * 3;
    let mut run_of = (0..half_edges).map(|_| NONE).collect::<Vec<_>>();
    let mut members: Vec<Vec<usize>> = Vec::new();
    let mut rings = Vec::with_capacity(loops.regions.len());
    for region in &loops.regions {
        let mut region_rings = Vec::with_capacity(region.loops.len());
        for ring in &region.loops {
            let mut origin = None;
            for (position, &he) in ring.iter().enumerate() {
                if *keep.slot(mesh.from(he)?)? {
                    origin = Some(position);
                    break;
                }
            }
            let Some(origin) = origin else {
                return Ok(None);
            };
            let mut ring_runs = Vec::new();
            for offset in 0..ring.len() {
                let he = *ring.slot((origin + offset) % ring.len())?;
                if *keep.slot(mesh.from(he)?)? {
                    ring_runs.push(members.len());
                    members.push(Vec::new());
                }
                let run = members.len() - 1;
                members.slot_mut(run)?.push(he);
                *run_of.slot_mut(he)? = run;
            }
            region_rings.push(ring_runs);
        }
        rings.push(region_rings);
    }
    let mut runs = Vec::with_capacity(members.len());
    for hes in &members {
        let first = *hes.first().ok_or(MeshBrepError::Internal("empty run"))?;
        let last = *hes.last().ok_or(MeshBrepError::Internal("empty run"))?;
        let (start, end) = (mesh.from(first)?, mesh.to(last)?);
        if start == end {
            return Ok(None);
        }
        let partner = match *twin.slot(first)? {
            Some(other) => *run_of.slot(other)?,
            None => runs.len(),
        };
        runs.push(Run {
            start,
            end,
            partner,
            live: true,
        });
    }
    for (index, run) in runs.iter().enumerate() {
        let hes = members.slot(index)?;
        if run.partner == NONE {
            return Ok(None);
        }
        let other = runs.slot(run.partner)?;
        let hes_other = members.slot(run.partner)?;
        if other.partner != index {
            return Ok(None);
        }
        if run.partner == index {
            if hes.iter().any(|&he| matches!(twin.get(he), Some(Some(_)))) {
                return Ok(None);
            }
            continue;
        }
        if other.start != run.end || other.end != run.start || hes.len() != hes_other.len() {
            return Ok(None);
        }
        for &he in hes {
            let Some(opposite) = *twin.slot(he)? else {
                return Ok(None);
            };
            if *run_of.slot(opposite)? != run.partner {
                return Ok(None);
            }
        }
    }
    Ok(Some(RunSet { runs, rings }))
}

impl RunSet {
    /// Remove every face whose only loop has two runs and join the faces on
    /// both sides of it.
    ///
    /// Such a face has no area: its two coedges run between the same two
    /// vertices in opposite directions, so the face is a sliver that the
    /// collinear merge reduced to a digon. The coedge across each of its two
    /// edges becomes the partner of the other one, which makes the neighbours
    /// share one edge. A coedge whose partner was the digon's own other coedge
    /// disappears with the face; a coedge that had no partner stays unpaired.
    /// Returns the number of faces removed.
    pub(super) fn collapse_digons(&mut self) -> Result<usize, MeshBrepError> {
        let mut collapsed = 0;
        for region in 0..self.rings.len() {
            let [ring] = self.rings.slot(region)?.as_slice() else {
                continue;
            };
            let &[first, second] = ring.as_slice() else {
                continue;
            };
            let across_first = self.runs.slot(first)?.partner;
            let across_second = self.runs.slot(second)?.partner;
            self.runs.slot_mut(first)?.live = false;
            self.runs.slot_mut(second)?.live = false;
            self.rings.slot_mut(region)?.clear();
            collapsed += 1;
            let outside = |run: usize| run != first && run != second;
            match (outside(across_first), outside(across_second)) {
                (true, true) => {
                    self.runs.slot_mut(across_first)?.partner = across_second;
                    self.runs.slot_mut(across_second)?.partner = across_first;
                }
                (true, false) => self.runs.slot_mut(across_first)?.partner = across_first,
                (false, true) => self.runs.slot_mut(across_second)?.partner = across_second,
                (false, false) => {}
            }
        }
        Ok(collapsed)
    }

    /// Remove the faces of every closed shell of fewer than four faces, which
    /// encloses no volume. `component_of_region` gives the shell of each
    /// region. Returns the number of shells removed.
    pub(super) fn drop_flat_shells(
        &mut self,
        component_of_region: &[usize],
    ) -> Result<usize, MeshBrepError> {
        let mut faces = std::collections::BTreeMap::<usize, usize>::new();
        let mut open = std::collections::BTreeSet::new();
        for (region, region_rings) in self.rings.iter().enumerate() {
            if region_rings.is_empty() {
                continue;
            }
            let component = *component_of_region.slot(region)?;
            *faces.entry(component).or_insert(0) += 1;
            for &run in region_rings.iter().flatten() {
                if self.runs.slot(run)?.partner == run {
                    open.insert(component);
                }
            }
        }
        let flat: std::collections::BTreeSet<usize> = faces
            .iter()
            .filter(|&(component, &count)| count < 4 && !open.contains(component))
            .map(|(&component, _)| component)
            .collect();
        for region in 0..self.rings.len() {
            if flat.contains(component_of_region.slot(region)?) {
                let runs: Vec<usize> = self.rings.slot(region)?.iter().flatten().copied().collect();
                for run in runs {
                    self.runs.slot_mut(run)?.live = false;
                }
                self.rings.slot_mut(region)?.clear();
            }
        }
        Ok(flat.len())
    }
}
