// SPDX-License-Identifier: Apache-2.0
//! Native polycurve objects authored from neutral compound constructions.

use std::collections::BTreeSet;

use cadmpeg_ir::geometry::{CompoundCurveConstruction, Curve, ProceduralCurveDefinition};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{
    class_wrapper_with_direct, writable_i32_count, ClassPayload, WritableObjectCurve, WriteError,
};

/// A curve class body and the bytes written directly at its own chunk level.
pub(super) struct CurvePayload {
    pub(super) class: [u8; 16],
    pub(super) body: Vec<u8>,
    pub(super) direct: Vec<u8>,
    pub(super) represented: BTreeSet<CurveId>,
    pub(super) record_count: u64,
}

impl CurvePayload {
    fn leaf((class, body): ClassPayload, id: &CurveId) -> Self {
        Self {
            class,
            direct: body.clone(),
            body,
            represented: BTreeSet::from([id.clone()]),
            record_count: 1,
        }
    }
}

fn construction<'a>(ir: &'a CadIr, curve: &Curve) -> Option<&'a CompoundCurveConstruction> {
    let id = curve.geometry.procedural_construction()?;
    let definition = ir
        .model
        .procedural_curves
        .iter()
        .find(|value| value.id == *id)?;
    match definition.definition() {
        ProceduralCurveDefinition::Compound(value) => Some(value),
        _ => None,
    }
}

pub(super) fn children(ir: &CadIr) -> BTreeSet<CurveId> {
    ir.model
        .curves
        .iter()
        .flat_map(|curve| components(ir, curve))
        .map(|curve| curve.id.clone())
        .collect()
}

pub(super) fn components<'a>(ir: &'a CadIr, curve: &Curve) -> Vec<&'a Curve> {
    let ids: Vec<_> = if let Some(compound) = construction(ir, curve) {
        compound
            .components()
            .iter()
            .map(|row| &row.component)
            .collect()
    } else if let Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Composite { segments, .. }) =
        curve.geometry.solved()
    {
        segments.iter().map(|row| &row.curve).collect()
    } else {
        Vec::new()
    };
    ids.into_iter()
        .filter_map(|id| ir.model.curves.iter().find(|child| child.id == *id))
        .collect()
}

pub(super) fn unrepresented_children<'a>(
    ir: &'a CadIr,
    curve: &Curve,
    represented: &BTreeSet<CurveId>,
) -> Vec<&'a Curve> {
    let mut pending = components(ir, curve);
    let mut seen = BTreeSet::new();
    let mut missing = Vec::new();
    while let Some(child) = pending.pop() {
        if !seen.insert(child.id.clone()) {
            continue;
        }
        if represented.contains(&child.id) {
            pending.extend(components(ir, child));
        } else {
            missing.push(child);
        }
    }
    missing
}

pub(super) fn exact_linear_segments<'a>(
    ir: &'a CadIr,
    curve: &Curve,
    domain: [f64; 2],
) -> Option<Vec<&'a Curve>> {
    use cadmpeg_ir::geometry::SolvedCurveGeometry;

    let compound = construction(ir, curve)?;
    let parameters = compound.parameters();
    let components = compound.components();
    if parameters.len() != components.len() + 1
        || parameters.first()?.get() != domain[0]
        || parameters.last()?.get() != domain[1]
        || parameters.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return None;
    }
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = curve.geometry.solved() else {
        return None;
    };
    if nurbs.degree() != 1 || nurbs.periodic() || nurbs.weights().is_some() {
        return None;
    }
    let expected_knots = std::iter::once(parameters.first()?.get())
        .chain(parameters.iter().map(|parameter| parameter.get()))
        .chain(std::iter::once(parameters.last()?.get()))
        .collect::<Vec<_>>();
    let points = nurbs.control_points();
    if nurbs.knots().as_slice() != expected_knots || points.len() != components.len() + 1 {
        return None;
    }
    let mut segments = Vec::with_capacity(components.len());
    let mut seen = BTreeSet::new();
    for (index, component) in components.iter().enumerate() {
        if !seen.insert(&component.component) {
            return None;
        }
        let child = ir
            .model
            .curves
            .iter()
            .find(|row| row.id == component.component)?;
        if child.geometry.procedural_construction().is_some() {
            return None;
        }
        let Some(SolvedCurveGeometry::Line(line)) = child.geometry.solved() else {
            return None;
        };
        let [start, end] = child.parameter_range?.endpoints();
        let origin = line.origin().get();
        let direction = *line.direction().as_raw();
        if origin.translated(direction, start) != points[index].get()
            || origin.translated(direction, end) != points[index + 1].get()
        {
            return None;
        }
        segments.push(child);
    }
    Some(segments)
}

pub(super) fn unrooted_members(ir: &CadIr) -> BTreeSet<CurveId> {
    let mut incoming = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.clone(), 0_usize))
        .collect::<std::collections::BTreeMap<_, _>>();
    for curve in &ir.model.curves {
        for child in components(ir, curve) {
            if let Some(count) = incoming.get_mut(&child.id) {
                *count += 1;
            }
        }
    }
    let mut ready = incoming
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    while let Some(id) = ready.pop() {
        let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == id) else {
            continue;
        };
        for child in components(ir, curve) {
            if let Some(count) = incoming.get_mut(&child.id) {
                *count -= 1;
                if *count == 0 {
                    ready.push(child.id.clone());
                }
            }
        }
    }
    incoming
        .into_iter()
        .filter_map(|(id, count)| (count != 0).then_some(id))
        .collect()
}

pub(super) fn payload(
    ir: &CadIr,
    curve: &Curve,
    active: &mut BTreeSet<CurveId>,
) -> Result<(CurvePayload, Vec<LossNote>), WriteError> {
    use crate::loss::RhinoLossCode;
    let Some(compound) = construction(ir, curve) else {
        let (value, conversion) = WritableObjectCurve::try_new(curve)?;
        let payload = value.payload()?;
        let mut notes: Vec<LossNote> = conversion.into_iter().collect();
        if let Some(construction) = curve.geometry.procedural_construction() {
            notes.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                "curve {} procedural construction {construction} was reduced to a held solved carrier",
                curve.id,
            )));
        }
        return Ok((CurvePayload::leaf(payload, &curve.id), notes));
    };
    if !active.insert(curve.id.clone()) {
        // (C) The neutral construction graph contains a cycle.
        return Err(WriteError::Omit(
            "cyclic compound curve construction".into(),
        ));
    }
    let result = compound_payload(ir, compound, active);
    active.remove(&curve.id);
    match result {
        Err(WriteError::Omit(_)) if curve.geometry.solved().is_some() => {
            let (value, conversion) = WritableObjectCurve::try_new(curve)?;
            let payload = value.payload()?;
            let mut notes: Vec<LossNote> = conversion.into_iter().collect();
            if let Some(construction) = curve.geometry.procedural_construction() {
                notes.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                    "curve {} compound construction {construction} was reduced to a held solved carrier",
                    curve.id
                )));
            }
            Ok((CurvePayload::leaf(payload, &curve.id), notes))
        }
        Ok((mut payload, notes)) => {
            payload.represented.insert(curve.id.clone());
            Ok((payload, notes))
        }
        Err(error) => Err(error),
    }
}

fn compound_payload(
    ir: &CadIr,
    compound: &CompoundCurveConstruction,
    active: &mut BTreeSet<CurveId>,
) -> Result<(CurvePayload, Vec<LossNote>), WriteError> {
    if active.len() > crate::curves::MAX_CURVE_DEPTH {
        return Err(WriteError::Omit(
            "compound curve nesting exceeds the native decoder's supported depth".into(),
        ));
    }
    let components = compound.components();
    let parameters = compound.parameters();
    if parameters.len() != components.len() + 1
        || components
            .iter()
            .zip(parameters)
            .any(|(component, parameter)| component.parameter != *parameter)
        || parameters
            .windows(2)
            .any(|pair| pair[0].get() >= pair[1].get())
    {
        // (S-entity) Other compound parameter conventions have no native polycurve spelling.
        return Err(WriteError::Omit(
            "compound curve parameters do not define native polycurve child intervals".into(),
        ));
    }
    let mut data = vec![0x10];
    data.extend(
        writable_i32_count(
            components.len(),
            "polycurve child count exceeds the native field".into(),
        )?
        .to_le_bytes(),
    );
    data.extend(0_i32.to_le_bytes());
    data.extend(0_i32.to_le_bytes());
    data.extend([0_u8; 48]);
    data.extend(
        writable_i32_count(
            parameters.len(),
            "polycurve parameter count exceeds the native field".into(),
        )?
        .to_le_bytes(),
    );
    for parameter in parameters {
        data.extend(parameter.get().to_le_bytes());
    }
    // Complete child chunks do not contribute to their parent's CRC (§4.1).
    let direct = data.clone();
    let mut reduced = Vec::new();
    let mut represented = BTreeSet::new();
    let mut record_count = 1_u64;
    for component in components {
        let Some(child) = ir
            .model
            .curves
            .iter()
            .find(|value| value.id == component.component)
        else {
            // (S-entity) The named child has no curve record; the enclosing
            // construction omits with a per-record loss naming it.
            return Err(WriteError::Omit(format!(
                "compound child {} is missing",
                component.component
            )));
        };
        let (child, child_reduced) = payload(ir, child, active)?;
        reduced.extend(child_reduced);
        represented.extend(child.represented.iter().cloned());
        record_count = record_count
            .checked_add(child.record_count)
            .ok_or_else(|| WriteError::Omit("compound curve record count exceeds u64".into()))?;
        data.extend(class_wrapper_with_direct(
            child.class,
            &child.body,
            &child.direct,
        )?);
    }
    Ok((
        CurvePayload {
            class: crate::curves::POLYCURVE.to_wire(),
            body: data,
            direct,
            represented,
            record_count,
        },
        reduced,
    ))
}
