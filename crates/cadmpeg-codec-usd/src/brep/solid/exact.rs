// SPDX-License-Identifier: Apache-2.0
//! The exact carriers of `CadBrepAnalyticAPI` (spec §18.5). The `BrepArray`
//! carriers are NURBS; these catalogs hold the analytic and NURBS carriers
//! that they are the NURBS forms of, and select one for each face, edge, and
//! edgeuse.

use std::collections::BTreeMap;

use super::{as_index, count, Curves, Refusal};
use crate::brep::{
    AnalyticCurve, AnalyticPcurve, AnalyticSurface, Brep, Catalog, CurveKind, Form, Frame,
    NurbsCurveRecord, PcurveKind, Reference, SurfaceKind,
};

/// The analytic API (spec §18.5).
pub(crate) const ANALYTIC_API: &str = "CadBrepAnalyticAPI";

/// The carrier of an entity: its `BrepArray` carrier, a catalog record, or
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    Brep,
    Analytic(u32),
    Nurbs(u32),
    None,
}

impl Source {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Brep => "brep",
            Self::Analytic(_) => "analytic",
            Self::Nurbs(_) => "nurbs",
            Self::None => "none",
        }
    }

    pub(crate) fn index(self) -> i64 {
        match self {
            Self::Analytic(index) | Self::Nurbs(index) => i64::from(index),
            Self::Brep | Self::None => -1,
        }
    }

    pub(crate) fn parse(token: &str, index: i128) -> Result<Self, Refusal> {
        let index =
            || u32::try_from(index).map_err(|_| format!("a `{token}` carrier index is not a uint"));
        Ok(match token {
            "brep" => Self::Brep,
            "analytic" => Self::Analytic(index()?),
            "nurbs" => Self::Nurbs(index()?),
            "none" => Self::None,
            _ => return Err(format!("unknown carrier type `{token}`")),
        })
    }
}

/// How a `BrepArray` loop runs against the exact loop it is the form of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Same,
    Reversed,
    /// The loop bounds the surface domain of a face with no exact loop.
    Absent,
}

impl Direction {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Reversed => "reversed",
            Self::Absent => "absent",
        }
    }

    pub(crate) fn from_token(token: &str) -> Option<Self> {
        [Self::Same, Self::Reversed, Self::Absent]
            .into_iter()
            .find(|direction| direction.token() == token)
    }
}

/// The kind, frame, and parameters of one analytic record.
type FrameRecord<'a> = (&'a str, Frame, Vec<f64>);

/// Packed frame-positioned analytic records.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Frames {
    pub(crate) kinds: Vec<String>,
    pub(crate) position: Vec<[f64; 3]>,
    pub(crate) axis: Vec<[f64; 3]>,
    pub(crate) ref_direction: Vec<[f64; 3]>,
    pub(crate) param_counts: Vec<u32>,
    pub(crate) params: Vec<f64>,
}

impl Frames {
    fn push(&mut self, kind: &str, frame: &Frame, params: &[f64]) -> Result<(), Refusal> {
        self.kinds.push(kind.to_owned());
        self.position.push(frame.position);
        self.axis.push(frame.axis);
        self.ref_direction.push(frame.ref_direction);
        self.param_counts.push(count(params.len())?);
        self.params.extend(params);
        Ok(())
    }

    /// The kind, frame, and parameters of each record.
    fn unpack(&self, name: &str) -> Result<Vec<FrameRecord<'_>>, Refusal> {
        let n = self.kinds.len();
        if [
            self.position.len(),
            self.axis.len(),
            self.ref_direction.len(),
            self.param_counts.len(),
        ]
        .iter()
        .any(|len| *len != n)
        {
            return Err(format!("{name}: the arrays differ in length"));
        }
        let params = split(&self.params, &self.param_counts, name)?;
        Ok((0..n)
            .zip(params)
            .map(|(index, params)| {
                (
                    self.kinds[index].as_str(),
                    Frame {
                        position: self.position[index],
                        axis: self.axis[index],
                        ref_direction: self.ref_direction[index],
                    },
                    params,
                )
            })
            .collect())
    }
}

/// Packed parameter-space analytic records.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PcurveFrames {
    pub(crate) kinds: Vec<String>,
    pub(crate) position: Vec<[f64; 2]>,
    pub(crate) ref_direction: Vec<[f64; 2]>,
    pub(crate) param_counts: Vec<u32>,
    pub(crate) params: Vec<f64>,
    pub(crate) range: Vec<[f64; 2]>,
}

/// `values` split into runs of `counts`.
fn split(values: &[f64], counts: &[u32], name: &str) -> Result<Vec<Vec<f64>>, Refusal> {
    let mut out = Vec::with_capacity(counts.len());
    let mut start = 0_usize;
    for count in counts {
        let end = start
            .checked_add(as_index(*count))
            .ok_or_else(|| format!("{name}: the parameter counts overflow"))?;
        out.push(
            values
                .get(start..end)
                .ok_or_else(|| format!("{name}: the parameters end early"))?
                .to_vec(),
        );
        start = end;
    }
    if start != values.len() {
        return Err(format!("{name}: the parameters do not match their counts"));
    }
    Ok(out)
}

/// The arrays of `CadBrepAnalyticAPI`.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PackedExact {
    pub(crate) surfaces: Frames,
    pub(crate) surface_profiles: Vec<Source>,
    pub(crate) curves: Frames,
    pub(crate) nurbs_curves: Curves<3>,
    pub(crate) nurbs_curve_forms: Vec<Form>,
    pub(crate) pcurves: PcurveFrames,
    pub(crate) nurbs_pcurves: Curves<2>,
    pub(crate) nurbs_pcurve_forms: Vec<Form>,
    pub(crate) face_surfaces: Vec<Source>,
    pub(crate) edge_curves: Vec<Source>,
    pub(crate) edge_ranges: Vec<[f64; 2]>,
    pub(crate) edge_same: Vec<bool>,
    pub(crate) loop_directions: Vec<Direction>,
    pub(crate) edgeuse_pcurves: Vec<Source>,
}

/// The record loop of each packed loop, and the record oriented edge of
/// each packed edgeuse, of one B-rep.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Layout {
    pub(crate) loops: Vec<usize>,
    pub(crate) edgeuses: Vec<usize>,
}

/// The packed index of each record of one B-rep in each catalog.
#[derive(Default)]
struct Packing {
    surfaces: BTreeMap<usize, u32>,
    curves: BTreeMap<Reference, Source>,
    pcurves: BTreeMap<Reference, Source>,
}

impl PackedExact {
    /// Whether any entity has a carrier other than its `BrepArray` carrier.
    pub(crate) fn any(&self) -> bool {
        self.face_surfaces
            .iter()
            .chain(&self.edge_curves)
            .chain(&self.edgeuse_pcurves)
            .any(|source| *source != Source::Brep)
            || self
                .loop_directions
                .iter()
                .any(|direction| *direction != Direction::Same)
    }

    /// Appends the exact carriers of one B-rep: `exact` is the B-rep and
    /// `converted` its NURBS form, packed with `layout`. With no `exact`,
    /// each entity has its `BrepArray` carrier.
    pub(crate) fn push(
        &mut self,
        exact: Option<&Brep>,
        converted: &Brep,
        layout: &Layout,
    ) -> Result<(), Refusal> {
        let mut packing = Packing::default();
        for index in 0..converted.faces.len() {
            let source = match exact.and_then(|exact| Some((exact, exact.faces.get(index)?))) {
                Some((exact, record)) => match record.surface {
                    (Catalog::Analytic, surface) => {
                        Source::Analytic(self.surface(exact, surface, &mut packing)?)
                    }
                    (Catalog::Nurbs, _) => Source::Brep,
                },
                None => Source::Brep,
            };
            self.face_surfaces.push(source);
        }
        for (index, edge) in converted.edges.iter().enumerate() {
            let record = exact.and_then(|exact| Some((exact, exact.edges.get(index)?)));
            let source = match record {
                Some((exact, record)) => match record.curve {
                    Some((Catalog::Analytic | Catalog::Nurbs, _))
                        if !same_curve(exact, record, converted, index) =>
                    {
                        let curve = record.curve.ok_or("an edge has no curve")?;
                        self.curve(exact, curve, &mut packing)?
                    }
                    _ => Source::Brep,
                },
                None => Source::Brep,
            };
            match (source, record) {
                (Source::Brep, _) | (_, None) => {
                    self.edge_ranges.push(edge.range);
                    self.edge_same.push(edge.same_sense);
                }
                (_, Some((_, record))) => {
                    self.edge_ranges.push(record.range);
                    self.edge_same.push(record.same_sense);
                }
            }
            self.edge_curves.push(source);
        }
        for loop_ in &layout.loops {
            let direction = match exact {
                None => Direction::Same,
                Some(exact) => match exact.loops.get(*loop_) {
                    None => Direction::Absent,
                    Some(record) => {
                        let reversed = record.oriented_edges.first().is_some_and(|oriented| {
                            exact
                                .oriented_edges
                                .get(*oriented)
                                .map(|edge| edge.orientation)
                                != converted
                                    .oriented_edges
                                    .get(*oriented)
                                    .map(|edge| edge.orientation)
                        });
                        if reversed {
                            Direction::Reversed
                        } else {
                            Direction::Same
                        }
                    }
                },
            };
            self.loop_directions.push(direction);
        }
        for oriented in &layout.edgeuses {
            let record =
                exact.and_then(|exact| Some((exact, exact.oriented_edges.get(*oriented)?)));
            let source = match record {
                Some((exact, record)) => {
                    let written = converted.oriented_edges.get(*oriented);
                    let kept = written.is_some_and(|written| {
                        written.orientation == record.orientation
                            && pcurve(converted, written.pcurve).is_some()
                            && pcurve(converted, written.pcurve) == pcurve(exact, record.pcurve)
                    });
                    match record.pcurve {
                        _ if kept => Source::Brep,
                        None => Source::None,
                        Some(reference) => self.pcurve(exact, reference, &mut packing)?,
                    }
                }
                None => Source::Brep,
            };
            self.edgeuse_pcurves.push(source);
        }
        Ok(())
    }

    fn surface(
        &mut self,
        exact: &Brep,
        index: usize,
        packing: &mut Packing,
    ) -> Result<u32, Refusal> {
        if let Some(packed) = packing.surfaces.get(&index) {
            return Ok(*packed);
        }
        let record = exact
            .analytic_surfaces
            .get(index)
            .ok_or("a face names no analytic surface")?;
        let profile = match record.profile {
            Some(profile) => self.curve(exact, profile, packing)?,
            None => Source::None,
        };
        let packed = count(self.surface_profiles.len())?;
        self.surfaces
            .push(record.kind.token(), &record.frame, &record.params)?;
        self.surface_profiles.push(profile);
        packing.surfaces.insert(index, packed);
        Ok(packed)
    }

    fn curve(
        &mut self,
        exact: &Brep,
        reference: Reference,
        packing: &mut Packing,
    ) -> Result<Source, Refusal> {
        if let Some(packed) = packing.curves.get(&reference) {
            return Ok(*packed);
        }
        let packed = match reference {
            (Catalog::Analytic, index) => {
                let record = exact
                    .analytic_curves
                    .get(index)
                    .ok_or("a reference names no analytic curve")?;
                let packed = count(self.curves.kinds.len())?;
                self.curves
                    .push(record.kind.token(), &record.frame, &record.params)?;
                Source::Analytic(packed)
            }
            (Catalog::Nurbs, index) => {
                let record = exact
                    .nurbs_curves
                    .get(index)
                    .ok_or("a reference names no NURBS curve")?;
                let packed = count(self.nurbs_curves.len())?;
                self.nurbs_curves.push(record)?;
                self.nurbs_curve_forms.push(record.form);
                Source::Nurbs(packed)
            }
        };
        packing.curves.insert(reference, packed);
        Ok(packed)
    }

    fn pcurve(
        &mut self,
        exact: &Brep,
        reference: Reference,
        packing: &mut Packing,
    ) -> Result<Source, Refusal> {
        if let Some(packed) = packing.pcurves.get(&reference) {
            return Ok(*packed);
        }
        let packed = match reference {
            (Catalog::Analytic, index) => {
                let record = exact
                    .analytic_pcurves
                    .get(index)
                    .ok_or("an oriented edge names no analytic pcurve")?;
                let pcurves = &mut self.pcurves;
                let packed = count(pcurves.kinds.len())?;
                pcurves.kinds.push(record.kind.token().to_owned());
                pcurves.position.push(record.position);
                pcurves.ref_direction.push(record.ref_direction);
                pcurves.param_counts.push(count(record.params.len())?);
                pcurves.params.extend(&record.params);
                pcurves.range.push(record.range);
                Source::Analytic(packed)
            }
            (Catalog::Nurbs, index) => {
                let record = exact
                    .nurbs_pcurves
                    .get(index)
                    .ok_or("an oriented edge names no NURBS pcurve")?;
                let packed = count(self.nurbs_pcurves.len())?;
                self.nurbs_pcurves.push(record)?;
                self.nurbs_pcurve_forms.push(record.form);
                Source::Nurbs(packed)
            }
        };
        packing.pcurves.insert(reference, packed);
        Ok(packed)
    }
}

/// Whether the `BrepArray` curve of edge `index` of `converted` is the
/// NURBS curve of `record` over its range and in its direction.
fn same_curve(
    exact: &Brep,
    record: &crate::brep::EdgeRecord,
    converted: &Brep,
    index: usize,
) -> bool {
    let Some((Catalog::Nurbs, curve)) = record.curve else {
        return false;
    };
    let Some(written) = converted.edges.get(index) else {
        return false;
    };
    record.same_sense
        && written.range == record.range
        && exact.nurbs_curves.get(curve).is_some()
        && written
            .curve
            .and_then(|(_, index)| converted.nurbs_curves.get(index))
            == exact.nurbs_curves.get(curve)
}

/// The NURBS pcurve that `reference` names in `brep`.
fn pcurve(brep: &Brep, reference: Option<Reference>) -> Option<&NurbsCurveRecord<2>> {
    match reference? {
        (Catalog::Nurbs, index) => brep.nurbs_pcurves.get(index),
        (Catalog::Analytic, _) => None,
    }
}

/// The records of the catalogs of `CadBrepAnalyticAPI`.
#[derive(Debug, Default)]
pub(crate) struct Catalogs {
    surfaces: Vec<(AnalyticSurface, Source)>,
    curves: Vec<AnalyticCurve>,
    nurbs_curves: Vec<NurbsCurveRecord<3>>,
    pcurves: Vec<AnalyticPcurve>,
    nurbs_pcurves: Vec<NurbsCurveRecord<2>>,
}

impl PackedExact {
    /// The catalog records, after a check that each entity array has one
    /// element for each of `faces`, `edges`, `loops`, and `edgeuses`.
    pub(crate) fn unpack(
        &self,
        faces: usize,
        edges: usize,
        loops: usize,
        edgeuses: usize,
    ) -> Result<Catalogs, Refusal> {
        if self.face_surfaces.len() != faces
            || self.edge_curves.len() != edges
            || self.edge_ranges.len() != edges
            || self.edge_same.len() != edges
            || self.loop_directions.len() != loops
            || self.edgeuse_pcurves.len() != edgeuses
        {
            return Err(format!(
                "{ANALYTIC_API}: an entity array does not have one element for each entity"
            ));
        }
        if self.surface_profiles.len() != self.surfaces.kinds.len() {
            return Err("cad:surface:analytic: a surface has no profile element".to_owned());
        }
        let surfaces = self
            .surfaces
            .unpack("cad:surface:analytic")?
            .into_iter()
            .zip(&self.surface_profiles)
            .map(|((kind, frame, params), profile)| {
                let kind = SurfaceKind::from_token(kind)
                    .ok_or_else(|| format!("unknown analytic surface type `{kind}`"))?;
                Ok((
                    AnalyticSurface {
                        kind,
                        frame,
                        params,
                        profile: None,
                    },
                    *profile,
                ))
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        let curves = self
            .curves
            .unpack("cad:curve3d:analytic")?
            .into_iter()
            .map(|(kind, frame, params)| {
                let kind = CurveKind::from_token(kind)
                    .ok_or_else(|| format!("unknown analytic curve type `{kind}`"))?;
                Ok(AnalyticCurve {
                    kind,
                    frame,
                    params,
                })
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        let mut nurbs_curves = self.nurbs_curves.unpack("cad:curve3d:nurb")?;
        for (record, form) in nurbs_curves.iter_mut().zip(&self.nurbs_curve_forms) {
            record.form = *form;
        }
        let mut nurbs_pcurves = self.nurbs_pcurves.unpack("cad:curveUv:nurb")?;
        for (record, form) in nurbs_pcurves.iter_mut().zip(&self.nurbs_pcurve_forms) {
            record.form = *form;
        }
        let packed = &self.pcurves;
        let n = packed.kinds.len();
        if [
            packed.position.len(),
            packed.ref_direction.len(),
            packed.param_counts.len(),
            packed.range.len(),
        ]
        .iter()
        .any(|len| *len != n)
        {
            return Err("cad:curveUv:analytic: the arrays differ in length".to_owned());
        }
        let params = split(&packed.params, &packed.param_counts, "cad:curveUv:analytic")?;
        let pcurves = params
            .into_iter()
            .enumerate()
            .map(|(index, params)| {
                let kind = &packed.kinds[index];
                Ok(AnalyticPcurve {
                    kind: PcurveKind::from_token(kind)
                        .ok_or_else(|| format!("unknown analytic pcurve type `{kind}`"))?,
                    position: packed.position[index],
                    ref_direction: packed.ref_direction[index],
                    params,
                    range: packed.range[index],
                })
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        Ok(Catalogs {
            surfaces,
            curves,
            nurbs_curves,
            pcurves,
            nurbs_pcurves,
        })
    }
}

/// The record index of each packed entity of one decoded B-rep.
pub(crate) struct Decoded<'a> {
    /// The packed face of each record face.
    pub(crate) faces: &'a [usize],
    pub(crate) edges: &'a BTreeMap<usize, usize>,
    pub(crate) loops: &'a [(usize, usize)],
    pub(crate) edgeuses: &'a [(usize, usize)],
}

/// The record index in `brep` of each catalog record copied into it.
#[derive(Default)]
struct Copied {
    surfaces: BTreeMap<u32, usize>,
    curves: BTreeMap<Source, usize>,
    pcurves: BTreeMap<Source, usize>,
}

/// Replaces the `BrepArray` carriers of the decoded `brep` with the exact
/// carriers that `packed` selects.
pub(crate) fn restore(
    brep: &mut Brep,
    packed: &PackedExact,
    catalogs: &Catalogs,
    decoded: &Decoded<'_>,
) -> Result<(), Refusal> {
    let mut copied = Copied::default();
    for (record, face) in decoded.faces.iter().enumerate() {
        if let Source::Analytic(index) = packed.face_surfaces[*face] {
            let surface = copy_surface(brep, catalogs, index, &mut copied)?;
            brep.faces[record].surface = (Catalog::Analytic, surface);
        }
    }
    for (edge, record) in decoded.edges {
        let source = packed.edge_curves[*edge];
        match source {
            Source::Brep => continue,
            Source::None => return Err("an edge has no curve".to_owned()),
            Source::Analytic(_) | Source::Nurbs(_) => {}
        }
        let curve = copy_curve(brep, catalogs, source, &mut copied)?;
        let written = &mut brep.edges[*record];
        written.curve = Some(curve);
        written.range = packed.edge_ranges[*edge];
        written.same_sense = packed.edge_same[*edge];
    }
    for (loop_, record) in decoded.loops {
        match packed.loop_directions[*loop_] {
            Direction::Same => continue,
            Direction::Absent => {
                for face in &mut brep.faces {
                    face.loops.retain(|index| index != record);
                }
                continue;
            }
            Direction::Reversed => {}
        }
        let oriented = brep.loops[*record].oriented_edges.clone();
        brep.loops[*record].oriented_edges.reverse();
        for index in oriented {
            let edge = &mut brep.oriented_edges[index];
            edge.orientation = !edge.orientation;
            if let Some((Catalog::Nurbs, pcurve)) = edge.pcurve {
                if let Some(curve) = brep.nurbs_pcurves.get_mut(pcurve) {
                    *curve = crate::brep::convert::reversed(curve);
                }
            }
        }
    }
    for (edgeuse, record) in decoded.edgeuses {
        let source = packed.edgeuse_pcurves[*edgeuse];
        brep.oriented_edges[*record].pcurve = match source {
            Source::Brep => continue,
            Source::None => None,
            Source::Analytic(_) | Source::Nurbs(_) => {
                Some(copy_pcurve(brep, catalogs, source, &mut copied)?)
            }
        };
    }
    Ok(())
}

fn copy_surface(
    brep: &mut Brep,
    catalogs: &Catalogs,
    index: u32,
    copied: &mut Copied,
) -> Result<usize, Refusal> {
    if let Some(record) = copied.surfaces.get(&index) {
        return Ok(*record);
    }
    let (surface, profile) = catalogs
        .surfaces
        .get(as_index(index))
        .ok_or("cad:face:surfaceIndex names no analytic surface")?;
    let mut surface = surface.clone();
    surface.profile = match profile {
        Source::None | Source::Brep => None,
        Source::Analytic(_) | Source::Nurbs(_) => {
            Some(copy_curve(brep, catalogs, *profile, copied)?)
        }
    };
    brep.analytic_surfaces.push(surface);
    let record = brep.analytic_surfaces.len() - 1;
    copied.surfaces.insert(index, record);
    Ok(record)
}

fn copy_curve(
    brep: &mut Brep,
    catalogs: &Catalogs,
    source: Source,
    copied: &mut Copied,
) -> Result<Reference, Refusal> {
    if let Some(record) = copied.curves.get(&source) {
        return Ok(match source {
            Source::Analytic(_) => (Catalog::Analytic, *record),
            _ => (Catalog::Nurbs, *record),
        });
    }
    let reference = match source {
        Source::Analytic(index) => {
            let curve = catalogs
                .curves
                .get(as_index(index))
                .ok_or("a reference names no analytic curve")?;
            brep.analytic_curves.push(curve.clone());
            (Catalog::Analytic, brep.analytic_curves.len() - 1)
        }
        Source::Nurbs(index) => {
            let curve = catalogs
                .nurbs_curves
                .get(as_index(index))
                .ok_or("a reference names no NURBS curve")?;
            brep.nurbs_curves.push(curve.clone());
            (Catalog::Nurbs, brep.nurbs_curves.len() - 1)
        }
        Source::Brep | Source::None => return Err("a reference names no curve".to_owned()),
    };
    copied.curves.insert(source, reference.1);
    Ok(reference)
}

fn copy_pcurve(
    brep: &mut Brep,
    catalogs: &Catalogs,
    source: Source,
    copied: &mut Copied,
) -> Result<Reference, Refusal> {
    if let Some(record) = copied.pcurves.get(&source) {
        return Ok(match source {
            Source::Analytic(_) => (Catalog::Analytic, *record),
            _ => (Catalog::Nurbs, *record),
        });
    }
    let reference = match source {
        Source::Analytic(index) => {
            let curve = catalogs
                .pcurves
                .get(as_index(index))
                .ok_or("an edgeuse names no analytic pcurve")?;
            brep.analytic_pcurves.push(curve.clone());
            (Catalog::Analytic, brep.analytic_pcurves.len() - 1)
        }
        Source::Nurbs(index) => {
            let curve = catalogs
                .nurbs_pcurves
                .get(as_index(index))
                .ok_or("an edgeuse names no NURBS pcurve")?;
            brep.nurbs_pcurves.push(curve.clone());
            (Catalog::Nurbs, brep.nurbs_pcurves.len() - 1)
        }
        Source::Brep | Source::None => return Err("an edgeuse names no pcurve".to_owned()),
    };
    copied.pcurves.insert(source, reference.1);
    Ok(reference)
}

/// The arrays of `CadBrepAnalyticAPI` on `prim`.
pub(crate) fn read(
    prim: &crate::stage::Prim<'_>,
    issues: &mut crate::schema::Issues,
) -> Result<PackedExact, Refusal> {
    use crate::schema::{integers, reals, tokens, vectors3};
    let uints = |issues: &mut crate::schema::Issues, name: &str| -> Result<Vec<u32>, Refusal> {
        integers(prim, name, issues)
            .unwrap_or_default()
            .into_iter()
            .map(|value| u32::try_from(value).map_err(|_| format!("{name}: a count is not a uint")))
            .collect()
    };
    let pairs = |issues: &mut crate::schema::Issues, name: &str| -> Vec<[f64; 2]> {
        reals(prim, name, 2, issues)
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|pair| [pair[0], pair[1]])
            .collect()
    };
    let words = |issues: &mut crate::schema::Issues, name: &str| -> Vec<String> {
        tokens(prim, name, issues).unwrap_or_default()
    };
    let sources =
        |issues: &mut crate::schema::Issues, name: &str| -> Result<Vec<Source>, Refusal> {
            let kinds = tokens(prim, &format!("{name}Type"), issues).unwrap_or_default();
            let indices = integers(prim, &format!("{name}Index"), issues).unwrap_or_default();
            if kinds.len() != indices.len() {
                return Err(format!("{name}Type and {name}Index differ in length"));
            }
            kinds
                .iter()
                .zip(indices)
                .map(|(kind, index)| Source::parse(kind, index))
                .collect()
        };
    let flags = |issues: &mut crate::schema::Issues,
                 name: &str,
                 yes: &str,
                 no: &str|
     -> Result<Vec<bool>, Refusal> {
        tokens(prim, name, issues)
            .unwrap_or_default()
            .into_iter()
            .map(|token| match token.as_str() {
                value if value == yes => Ok(true),
                value if value == no => Ok(false),
                _ => Err(format!("{name}: unknown token `{token}`")),
            })
            .collect()
    };
    let forms = |issues: &mut crate::schema::Issues, name: &str| -> Result<Vec<Form>, Refusal> {
        tokens(prim, name, issues)
            .unwrap_or_default()
            .iter()
            .map(|token| {
                Form::from_token(token).ok_or_else(|| format!("{name}: unknown form `{token}`"))
            })
            .collect()
    };
    let frames = |issues: &mut crate::schema::Issues, prefix: &str| -> Result<Frames, Refusal> {
        Ok(Frames {
            kinds: tokens(prim, &format!("{prefix}:type"), issues).unwrap_or_default(),
            position: vectors3(prim, &format!("{prefix}:position"), issues).unwrap_or_default(),
            axis: vectors3(prim, &format!("{prefix}:axis"), issues).unwrap_or_default(),
            ref_direction: vectors3(prim, &format!("{prefix}:refDirection"), issues)
                .unwrap_or_default(),
            param_counts: uints(issues, &format!("{prefix}:paramCounts"))?,
            params: reals(prim, &format!("{prefix}:params"), 1, issues).unwrap_or_default(),
        })
    };
    let curves3 = |issues: &mut crate::schema::Issues| -> Result<Curves<3>, Refusal> {
        let name = |field: &str| format!("cad:curve3d:nurb:{field}");
        Ok(Curves {
            control_vertices: vectors3(prim, &name("controlVertices"), issues).unwrap_or_default(),
            vertex_count: uints(issues, &name("vertexCount"))?,
            order: uints(issues, &name("order"))?,
            knots: reals(prim, &name("knots"), 1, issues).unwrap_or_default(),
            weights: reals(prim, &name("weights"), 1, issues).unwrap_or_default(),
        })
    };
    let curves2 = |issues: &mut crate::schema::Issues| -> Result<Curves<2>, Refusal> {
        let name = |field: &str| format!("cad:curveUv:nurb:{field}");
        Ok(Curves {
            control_vertices: pairs(issues, &name("controlVertices")),
            vertex_count: uints(issues, &name("vertexCount"))?,
            order: uints(issues, &name("order"))?,
            knots: reals(prim, &name("knots"), 1, issues).unwrap_or_default(),
            weights: reals(prim, &name("weights"), 1, issues).unwrap_or_default(),
        })
    };
    Ok(PackedExact {
        surfaces: frames(issues, "cad:surface:analytic")?,
        surface_profiles: sources(issues, "cad:surface:analytic:profile")?,
        curves: frames(issues, "cad:curve3d:analytic")?,
        nurbs_curves: curves3(issues)?,
        nurbs_curve_forms: forms(issues, "cad:curve3d:nurb:form")?,
        pcurves: PcurveFrames {
            kinds: words(issues, "cad:curveUv:analytic:type"),
            position: pairs(issues, "cad:curveUv:analytic:position"),
            ref_direction: pairs(issues, "cad:curveUv:analytic:refDirection"),
            param_counts: uints(issues, "cad:curveUv:analytic:paramCounts")?,
            params: reals(prim, "cad:curveUv:analytic:params", 1, issues).unwrap_or_default(),
            range: pairs(issues, "cad:curveUv:analytic:range"),
        },
        nurbs_pcurves: curves2(issues)?,
        nurbs_pcurve_forms: forms(issues, "cad:curveUv:nurb:form")?,
        face_surfaces: sources(issues, "cad:face:surface")?,
        edge_curves: sources(issues, "cad:edge:curve")?,
        edge_ranges: pairs(issues, "cad:edge:range"),
        edge_same: flags(issues, "cad:edge:sense", "same", "opposite")?,
        loop_directions: tokens(prim, "cad:loop:direction", issues)
            .unwrap_or_default()
            .iter()
            .map(|token| {
                Direction::from_token(token)
                    .ok_or_else(|| format!("cad:loop:direction: unknown token `{token}`"))
            })
            .collect::<Result<_, Refusal>>()?,
        edgeuse_pcurves: sources(issues, "cad:edgeuse:pcurve")?,
    })
}

/// Removes the loops that no face holds, and the oriented edges, edges, and
/// vertices that only they reach.
pub(crate) fn prune(brep: &mut Brep) {
    fn kept<T: Clone>(values: &[T], keep: &[bool]) -> Vec<T> {
        values
            .iter()
            .zip(keep)
            .filter(|(_, keep)| **keep)
            .map(|(value, _)| value.clone())
            .collect()
    }
    fn renumbered(keep: &[bool]) -> Vec<usize> {
        let mut next = 0;
        keep.iter()
            .map(|keep| {
                let index = next;
                next += usize::from(*keep);
                index
            })
            .collect()
    }
    let mut keep_loops = vec![false; brep.loops.len()];
    for face in &brep.faces {
        for loop_ in &face.loops {
            if let Some(keep) = keep_loops.get_mut(*loop_) {
                *keep = true;
            }
        }
    }
    if keep_loops.iter().all(|keep| *keep) {
        return;
    }
    let mut keep_oriented = vec![false; brep.oriented_edges.len()];
    let mut keep_vertices = vec![false; brep.vertices.len()];
    for (loop_, record) in brep.loops.iter().enumerate() {
        if !keep_loops[loop_] {
            continue;
        }
        for oriented in &record.oriented_edges {
            keep_oriented[*oriented] = true;
        }
        if let Some(vertex) = record.vertex {
            keep_vertices[vertex] = true;
        }
    }
    let mut keep_edges = vec![false; brep.edges.len()];
    for (oriented, record) in brep.oriented_edges.iter().enumerate() {
        if keep_oriented[oriented] {
            keep_edges[record.edge] = true;
        }
    }
    for (edge, record) in brep.edges.iter().enumerate() {
        if keep_edges[edge] {
            for vertex in record.vertices {
                keep_vertices[vertex] = true;
            }
        }
    }
    let (loops, oriented, edges, vertices) = (
        renumbered(&keep_loops),
        renumbered(&keep_oriented),
        renumbered(&keep_edges),
        renumbered(&keep_vertices),
    );
    for face in &mut brep.faces {
        for loop_ in &mut face.loops {
            *loop_ = loops[*loop_];
        }
    }
    brep.loops = kept(&brep.loops, &keep_loops);
    for record in &mut brep.loops {
        for index in &mut record.oriented_edges {
            *index = oriented[*index];
        }
        if let Some(vertex) = &mut record.vertex {
            *vertex = vertices[*vertex];
        }
    }
    brep.oriented_edges = kept(&brep.oriented_edges, &keep_oriented);
    for record in &mut brep.oriented_edges {
        record.edge = edges[record.edge];
    }
    brep.edges = kept(&brep.edges, &keep_edges);
    for record in &mut brep.edges {
        record.vertices = record.vertices.map(|vertex| vertices[vertex]);
    }
    brep.vertices = kept(&brep.vertices, &keep_vertices);
    let pad = |values: &mut Vec<Option<String>>, len: usize| values.resize(len, None);
    pad(&mut brep.identities.edges, keep_edges.len());
    pad(&mut brep.identities.vertices, keep_vertices.len());
    brep.identities.edges = kept(&brep.identities.edges, &keep_edges);
    brep.identities.vertices = kept(&brep.identities.vertices, &keep_vertices);
    let tolerances = &mut brep.tolerances;
    tolerances.edges.resize(keep_edges.len(), None);
    tolerances.vertices.resize(keep_vertices.len(), None);
    tolerances.edges = kept(&tolerances.edges, &keep_edges);
    tolerances.vertices = kept(&tolerances.vertices, &keep_vertices);
}
