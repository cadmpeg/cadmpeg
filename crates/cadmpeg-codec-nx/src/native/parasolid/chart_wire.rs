// SPDX-License-Identifier: Apache-2.0
//! Original chart fields derived from the checked physical source record.

use super::ParasolidChartRecord;
use crate::intersection::chart_samples::{ChartPreamble, SourceChartData, MISSING_PARAMETER};
use crate::intersection::{ChartFraming, ChartPointLayout, SupportUv};
use cadmpeg_ir::math::Point3;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct ChartWire {
    id: String,
    stream_ordinal: u32,
    xmt: u32,
    count: u32,
    base_parameter: f64,
    base_scale: f64,
    chart_count: u32,
    chordal_error: f64,
    angular_error: f64,
    parameter_errors: [f64; 2],
    points: Vec<[f64; 3]>,
    native_parameters: Option<Vec<f64>>,
    ext_support_uv: SupportUv,
    point_layout: ChartPointLayout,
    framing: ChartFraming,
    inflated_offset: u64,
}
impl From<ParasolidChartRecord> for ChartWire {
    fn from(value: ParasolidChartRecord) -> Self {
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt,
            count: value.data.count(),
            base_parameter: value.preamble.base_parameter(),
            base_scale: value.preamble.base_scale(),
            chart_count: value.data.count(),
            chordal_error: value.preamble.chordal_error(),
            angular_error: value.preamble.angular_error(),
            parameter_errors: [MISSING_PARAMETER, MISSING_PARAMETER],
            points: value
                .data
                .points()
                .iter()
                .map(|point| [point.x, point.y, point.z])
                .collect(),
            native_parameters: value.data.native_parameters().map(<[f64]>::to_vec),
            ext_support_uv: value.data.support_uv(),
            point_layout: value.data.point_layout(),
            framing: value.framing,
            inflated_offset: value.inflated_offset,
        }
    }
}
impl TryFrom<ChartWire> for ParasolidChartRecord {
    type Error = &'static str;
    fn try_from(wire: ChartWire) -> Result<Self, Self::Error> {
        if wire.parameter_errors != [MISSING_PARAMETER, MISSING_PARAMETER] {
            return Err("parameter_errors: both slots must encode the missing-parameter value");
        }
        let points = wire
            .points
            .into_iter()
            .map(|[x, y, z]| Point3::new(x, y, z))
            .collect();
        let data = match (wire.point_layout, wire.native_parameters, wire.ext_support_uv) {
            (ChartPointLayout::Xyz3, None, [None, None]) => SourceChartData::xyz3(points)?,
            (ChartPointLayout::Ext11, Some(parameters), support_uv) => SourceChartData::ext11(points, parameters, support_uv)?,
            _ => return Err("point_layout/native_parameters/ext_support_uv: fields do not match the Hvec layout"),
        };
        if wire.count != data.count() {
            return Err("count: does not match points length");
        }
        if wire.chart_count != data.count() {
            return Err("chart_count: does not match points length");
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: wire.xmt,
            preamble: ChartPreamble::new(
                wire.base_parameter,
                wire.base_scale,
                wire.chordal_error,
                wire.angular_error,
            )?,
            data,
            framing: wire.framing,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_fields_keep_wire_order_and_reject_mismatched_layouts() {
        for (layout, parameters, uv) in [
            ("xyz3", "null", "[null,null]"),
            ("ext11", "[2.0,5.0]", "[[[0.0,1.0],[2.0,3.0]],null]"),
        ] {
            let json = format!(
                r#"{{"id":"chart","stream_ordinal":0,"xmt":1,"count":2,"base_parameter":2.0,"base_scale":1.0,"chart_count":2,"chordal_error":0.01,"angular_error":0.1,"parameter_errors":[-31415800000000.0,-31415800000000.0],"points":[[0.0,0.0,0.0],[1.0,0.0,0.0]],"native_parameters":{parameters},"ext_support_uv":{uv},"point_layout":"{layout}","framing":"direct","inflated_offset":10}}"#
            );
            let chart: ParasolidChartRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&chart).unwrap(), json);
            for (field, invalid) in [
                ("count", serde_json::json!(1)),
                ("chart_count", serde_json::json!(1)),
                ("parameter_errors", serde_json::json!([0.0, 0.0])),
                ("base_scale", serde_json::json!(0.0)),
                ("native_parameters", serde_json::json!([2.0])),
                ("ext_support_uv", serde_json::json!([[[0.0, 0.0]], null])),
            ] {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire[field] = invalid;
                let error = serde_json::from_value::<ParasolidChartRecord>(wire).unwrap_err();
                assert!(error.to_string().contains(field), "{error}");
            }
        }
    }
}
