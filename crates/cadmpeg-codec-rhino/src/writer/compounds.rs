// SPDX-License-Identifier: Apache-2.0
//! Native polycurve objects authored from neutral compound constructions.

use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{CompoundCurveConstruction, Curve, ProceduralCurveDefinition};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::CadIr;

use super::{class_wrapper_with_direct, native_i32_count, ClassPayload, WritableObjectCurve};

/// A curve class body and the bytes written directly at its own chunk level.
pub(super) struct CurvePayload {
    pub(super) class: [u8; 16],
    pub(super) body: Vec<u8>,
    pub(super) direct: Vec<u8>,
}

impl CurvePayload {
    fn leaf((class, body): ClassPayload) -> Self {
        Self {
            class,
            direct: body.clone(),
            body,
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
        .procedural_curves
        .iter()
        .flat_map(|value| match value.definition() {
            ProceduralCurveDefinition::Compound(value) => value
                .components()
                .iter()
                .map(|component| component.component.clone())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

pub(super) fn components<'a>(ir: &'a CadIr, curve: &Curve) -> Vec<&'a Curve> {
    construction(ir, curve).map_or_else(Vec::new, |compound| {
        compound
            .components()
            .iter()
            .filter_map(|component| {
                ir.model
                    .curves
                    .iter()
                    .find(|child| child.id == component.component)
            })
            .collect()
    })
}

pub(super) fn validate(ir: &CadIr) -> Result<(), CodecError> {
    let mut incoming = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.clone(), 0_usize))
        .collect::<std::collections::BTreeMap<_, _>>();
    if incoming.len() != ir.model.curves.len() {
        // (C) Distinct records cannot declare the same neutral identity.
        return Err(CodecError::malformed("duplicate neutral curve identity"));
    }
    for curve in &ir.model.curves {
        if let Some(id) = curve.geometry.procedural_construction() {
            if !ir
                .model
                .procedural_curves
                .iter()
                .any(|record| record.id == *id)
            {
                // (C) A held procedural carrier has a dangling construction.
                return Err(CodecError::malformed(format!(
                    "curve {} construction {id} is missing",
                    curve.id
                )));
            }
        }
        if let Some(compound) = construction(ir, curve) {
            for child in compound.components() {
                let count = incoming.get_mut(&child.component).ok_or_else(|| {
                    // (C) A construction refers to a curve absent from the document.
                    CodecError::malformed(format!("compound child {} is missing", child.component))
                })?;
                *count += 1;
            }
        }
    }
    let mut ready = incoming
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(id) = ready.pop() {
        visited += 1;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == id)
            .ok_or_else(|| CodecError::malformed("compound curve identity is missing"))?;
        for child in components(ir, curve) {
            let count = incoming
                .get_mut(&child.id)
                .ok_or_else(|| CodecError::malformed("compound child identity is missing"))?;
            *count -= 1;
            if *count == 0 {
                ready.push(child.id.clone());
            }
        }
    }
    if visited != incoming.len() {
        // (C) A cycle has no standalone root and cannot be hidden by omission.
        return Err(CodecError::malformed("cyclic compound curve construction"));
    }
    Ok(())
}

pub(super) fn payload(
    ir: &CadIr,
    curve: &Curve,
    active: &mut BTreeSet<CurveId>,
) -> Result<(CurvePayload, bool), CodecError> {
    let Some(compound) = construction(ir, curve) else {
        return WritableObjectCurve::try_new(curve)
            .and_then(|value| value.payload())
            .map(|payload| (CurvePayload::leaf(payload), false));
    };
    if !active.insert(curve.id.clone()) {
        // (C) The neutral construction graph contains a cycle.
        return Err(CodecError::malformed("cyclic compound curve construction"));
    }
    let result = compound_payload(ir, compound, active);
    active.remove(&curve.id);
    match result {
        Err(CodecError::UnsupportedContent(_)) if curve.geometry.solved().is_some() => {
            WritableObjectCurve::try_new(curve)
                .and_then(|value| value.payload())
                .map(|payload| (CurvePayload::leaf(payload), true))
        }
        other => other,
    }
}

fn compound_payload(
    ir: &CadIr,
    compound: &CompoundCurveConstruction,
    active: &mut BTreeSet<CurveId>,
) -> Result<(CurvePayload, bool), CodecError> {
    if active.len() > crate::curves::MAX_CURVE_DEPTH {
        return Err(CodecError::UnsupportedContent(
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
        return Err(CodecError::UnsupportedContent(
            "compound curve parameters do not define native polycurve child intervals".into(),
        ));
    }
    let mut data = vec![0x10];
    data.extend(
        native_i32_count(
            components.len(),
            "polycurve child count exceeds the native field".into(),
        )?
        .to_le_bytes(),
    );
    data.extend(0_i32.to_le_bytes());
    data.extend(0_i32.to_le_bytes());
    data.extend([0_u8; 48]);
    data.extend(
        native_i32_count(
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
    let mut reduced = false;
    for component in components {
        let child = ir
            .model
            .curves
            .iter()
            .find(|value| value.id == component.component)
            .ok_or_else(|| {
                CodecError::malformed(format!("compound child {} is missing", component.component))
            })?;
        let (child, child_reduced) = payload(ir, child, active)?;
        reduced |= child_reduced;
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
        },
        reduced,
    ))
}
