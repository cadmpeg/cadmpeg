// SPDX-License-Identifier: Apache-2.0
//! Original chart fields derived from the checked physical source record.

use super::ParasolidChartRecord;
use crate::intersection::chart_samples::{ChartPreamble, SourceChartData, MISSING_PARAMETER};
use crate::intersection::{ChartFraming, ChartPointLayout};
use cadmpeg_ir::math::Point3;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
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
    ext_support_uv: [Option<Vec<[f64; 2]>>; 2],
    point_layout: ChartPointLayout,
    framing: ChartFraming,
    inflated_offset: u64,
}

struct ChartPoints<'a>(&'a SourceChartData);
struct ChartParameters<'a>(&'a SourceChartData);

impl Serialize for ChartPoints<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let count = usize::try_from(self.0.count()).map_err(serde::ser::Error::custom)?;
        let mut points = serializer.serialize_seq(Some(count))?;
        for index in 0..count {
            let point = self.0.point_at(index).ok_or_else(|| {
                serde::ser::Error::custom("chart point missing from counted source")
            })?;
            points.serialize_element(&[point.x, point.y, point.z])?;
        }
        points.end()
    }
}

impl Serialize for ChartParameters<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let count = usize::try_from(self.0.count()).map_err(serde::ser::Error::custom)?;
        let mut parameters = serializer.serialize_seq(Some(count))?;
        for index in 0..count {
            let value = self.0.native_parameter_at(index).ok_or_else(|| {
                serde::ser::Error::custom("chart parameter missing from counted source")
            })?;
            parameters.serialize_element(&value)?;
        }
        parameters.end()
    }
}

#[derive(Serialize)]
struct ChartRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    xmt: u32,
    count: u32,
    base_parameter: f64,
    base_scale: f64,
    chart_count: u32,
    chordal_error: f64,
    angular_error: f64,
    parameter_errors: [f64; 2],
    points: ChartPoints<'a>,
    native_parameters: Option<ChartParameters<'a>>,
    ext_support_uv: [Option<&'a [cadmpeg_ir::units::FiniteVector<2>]>; 2],
    point_layout: ChartPointLayout,
    framing: ChartFraming,
    inflated_offset: u64,
}

impl Serialize for ParasolidChartRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let support_uv = self
            .data
            .support_uv_ref()
            .map(|lane| lane.map(|lane| lane.as_slice()));
        ChartRef {
            id: &self.id,
            stream_ordinal: self.stream_ordinal,
            xmt: self.xmt,
            count: self.data.count(),
            base_parameter: self.preamble.base_parameter(),
            base_scale: self.preamble.base_scale(),
            chart_count: self.data.count(),
            chordal_error: self.preamble.chordal_error().get(),
            angular_error: self.preamble.angular_error(),
            parameter_errors: [MISSING_PARAMETER, MISSING_PARAMETER],
            points: ChartPoints(&self.data),
            native_parameters: (self.data.point_layout() == ChartPointLayout::Ext11)
                .then_some(ChartParameters(&self.data)),
            ext_support_uv: support_uv,
            point_layout: self.data.point_layout(),
            framing: self.framing,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
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
            chordal_error: value.preamble.chordal_error().get(),
            angular_error: value.preamble.angular_error(),
            parameter_errors: [MISSING_PARAMETER, MISSING_PARAMETER],
            points: value
                .data
                .points()
                .iter()
                .map(|point| [point.x, point.y, point.z])
                .collect(),
            native_parameters: value.data.native_parameters(),
            ext_support_uv: value
                .data
                .support_uv()
                .map(|lane| lane.map(|lane| lane.iter().map(|pair| **pair).collect())),
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
    use super::super::ParasolidChartRecord;

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
            assert_eq!(
                serde_json::to_vec(&chart).unwrap(),
                serde_json::to_vec(&super::ChartWire::from(chart.clone())).unwrap()
            );
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

    #[test]
    fn chart_native_limit_refuses_before_nested_column_copies() {
        let json = r#"{"id":"nx:parasolid:chart#0","stream_ordinal":0,"xmt":1,"count":2,"base_parameter":2.0,"base_scale":1.0,"chart_count":2,"chordal_error":0.01,"angular_error":0.1,"parameter_errors":[-31415800000000.0,-31415800000000.0],"points":[[0.0,0.0,0.0],[1.0,0.0,0.0]],"native_parameters":[2.0,5.0],"ext_support_uv":[[[0.0,1.0],[2.0,3.0]],null],"point_layout":"ext11","framing":"direct","inflated_offset":10}"#;
        let chart: ParasolidChartRecord = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &chart,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
