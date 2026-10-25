// SPDX-License-Identifier: Apache-2.0
//! One current Pcurve constructor shared by native and original-source inputs.
use super::*;
use crate::brep::source::projection::PcurveInputs;

pub(super) struct Projection<'a> {
    pub(super) id: PcurveId,
    pub(super) native_ref: Option<String>,
    pub(super) payload: &'a str,
    pub(super) curve: usize,
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    request: Projection<'_>,
    inputs: PcurveInputs,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Pcurve>, CodecError> {
    let geometry = match pcurve_geometry(ctx, &inputs.curve) {
        Ok(geometry) => geometry,
        Err(PcurveGeometryError::Resource(error)) => return Err(error),
        Err(error) => {
            losses.push(pcurve_loss(request.payload, request.curve, Some(&error)));
            return Ok(None);
        }
    };
    let Some(geometry) =
        geometry.and_then(|geometry| transformed_pcurve_geometry(geometry, inputs.affine))
    else {
        losses.push(pcurve_loss(request.payload, request.curve, None));
        return Ok(None);
    };
    let range = normalize_pcurve_parameter_range(&geometry, Some(inputs.parameter_range));
    Ok(Some(Pcurve {
        native_ref: request.native_ref,
        id: request.id,
        geometry,
        metadata: PcurveMetadata::general(None, range.map(Into::into), None),
    }))
}
