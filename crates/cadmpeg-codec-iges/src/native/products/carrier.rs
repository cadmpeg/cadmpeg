// SPDX-License-Identifier: Apache-2.0
//! Product-owned wire topology for bounded source curve members.

use crate::ids::{self, Stem};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::eval::EvaluationFailure;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind, Edge, EdgeCarrier, Point, Region, Shell, Vertex};
use cadmpeg_ir::CadIr;

pub(super) fn project(
    ir: &mut CadIr,
    definition: &str,
    member: &str,
    link: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<BodyId>, CodecError> {
    let stem =
        Stem::directory(super::source_sequence(definition)?).child(super::source_sequence(member)?);
    let body = ids::body(&stem);
    if ir.model.bodies.iter().any(|record| record.id == body) {
        return Ok(Some(body));
    }
    let Some(curve) = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == link)
    else {
        return Ok(None);
    };
    let (Some(range), Some(geometry)) = (curve.parameter_range, curve.geometry.solved()) else {
        return Ok(None);
    };
    let mut endpoints = Vec::new();
    for parameter in range.endpoints() {
        match cadmpeg_ir::eval::decode::curve_point(ctx, geometry, parameter) {
            Ok(point) => endpoints.push(point),
            Err(EvaluationFailure::ResourceLimit(limit)) => return Err(limit.into()),
            Err(
                EvaluationFailure::NoValue
                | EvaluationFailure::NonFinite(_)
                | EvaluationFailure::WorkExhausted,
            ) => return Ok(None),
        }
    }
    let curve = curve.id.clone();
    let closed = endpoints[0] == endpoints[1];
    ctx.charge_entities(if closed { 6 } else { 8 }, "iges_product_member_topology")?;
    let region = ids::region(&stem);
    let shell = ids::shell(&stem);
    let edge = ids::edge(&stem);
    let start = ids::vertex(&stem.slot(0));
    let end = if closed {
        start.clone()
    } else {
        ids::vertex(&stem.slot(1))
    };
    for (ordinal, position) in endpoints
        .into_iter()
        .take(if closed { 1 } else { 2 })
        .enumerate()
    {
        let point = ids::point(&stem.slot(ordinal));
        ir.model
            .points
            .push(Point::new(point.clone(), position, None));
        ir.model.vertices.push(Vertex {
            id: ids::vertex(&stem.slot(ordinal)),
            point,
            tolerance: None,
        });
    }
    ir.model.edges.push(Edge {
        id: edge.clone(),
        carrier: EdgeCarrier::Bounded(curve, range.into()),
        start,
        end,
        tolerance: None,
    });
    ir.model.shells.push(
        Shell::new(
            shell.clone(),
            region.clone(),
            Vec::new(),
            vec![edge],
            Vec::new(),
        )
        .map_err(CodecError::malformed)?,
    );
    ir.model.regions.push(Region {
        id: region.clone(),
        body: body.clone(),
        shells: vec![shell],
    });
    ir.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Wire,
        regions: vec![region],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    Ok(Some(body))
}
