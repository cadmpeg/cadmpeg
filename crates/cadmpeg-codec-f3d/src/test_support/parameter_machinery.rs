// SPDX-License-Identifier: Apache-2.0
//! A native store that holds only the Design parameter machinery.

use crate::native::F3dNative;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{
    DesignCompanionPayload, DesignParameter, DesignParameterCompanion,
    DesignParameterDiscriminator, DesignParameterDraft, DesignParameterOwner,
    DesignParameterOwnerWire, DesignParameterSource,
};
use crate::records::references::DesignClassTag;

const STREAM: &str = "f3d:test/BulkStream.dat";

/// An indexed record header in the test Design stream.
pub(crate) fn record_header(record_index: u32, class_tag: &str) -> DesignRecordHeader {
    DesignRecordHeader {
        id: format!(
            "{STREAM}:design-record-header#{}",
            u64::from(record_index) * 100
        ),
        record_index,
        class_tag: DesignClassTag::try_from(class_tag.to_owned()).unwrap(),
        byte_offset: u64::from(record_index) * 100,
    }
}

/// One parameter scope with an owned parameter, its owner and its companion,
/// and the indexed headers of those four records.
pub(crate) fn parameter_machinery_native() -> F3dNative {
    let stream = STREAM;
    let tag = |value: &str| DesignClassTag::try_from(value.to_owned()).unwrap();
    let mut native = F3dNative::default();

    native
        .design_parameter_scopes
        .push(DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#10"),
            DesignFeatureKind::Extrude,
            10,
        ));
    native.design_parameters.push(
        DesignParameter::try_from(DesignParameterDraft {
            id: format!("{stream}:design-parameter#11"),
            byte_offset: 1100,
            class_tag: tag("305"),
            record_index: 11,
            source_ordinal: 0,
            source: DesignParameterSource::new(
                "Linear Dimension-2".into(),
                Some(12),
                Some(crate::records::identity::Located {
                    value: DesignParameterDiscriminator::Code0,
                    offset: 1122,
                }),
            )
            .unwrap(),
            expression: "5 mm".into(),
            expression_offset: 1140,
            source_kind_offset: 1160,
            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 1190,
            }),
            name: "d1".into(),
            name_offset: 1200,
            evaluated_value: 0.5,
            evaluated_value_offset: 1210,
        })
        .unwrap(),
    );
    native.design_parameter_owners.push(
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: format!("{stream}:design-parameter-owner#12"),
            byte_offset: 1200,
            frame_length: 104,
            class_tag: tag("292"),
            record_index: 12,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 0.5,
            evaluated_value_offset: 1240,
            parameter_record_index: 11,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 13,
        })
        .unwrap(),
    );
    native.design_parameter_companions.push(
        DesignParameterCompanion::unbound(
            format!("{stream}:design-parameter-companion#13"),
            1300,
            tag("408"),
            13,
            12,
            std::num::NonZeroU64::new(1).unwrap(),
            1362,
        )
        .bound(DesignCompanionPayload::new(1378, 100, Vec::new())),
    );
    for (record_index, class_tag) in [(10, "268"), (11, "305"), (12, "292"), (13, "408")] {
        native
            .design_record_headers
            .push(record_header(record_index, class_tag));
    }
    native
}
