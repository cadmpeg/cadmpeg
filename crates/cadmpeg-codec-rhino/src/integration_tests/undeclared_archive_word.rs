// SPDX-License-Identifier: Apache-2.0
//! Recovery of an archive word no registry row declares.

use super::{assert_valid, decode};
use crate::test_support as support;

#[test]
fn an_undeclared_archive_word_recovers_its_content_under_an_unverified_admission() {
    // Archive word 100 has no row. Its chunks are the same grammar words 2
    // through 90 write, so the document is read rather than refused: the point
    // reaches the model, the report records that no declared strategy was
    // substituted, and the dialect-unverified loss is charged.
    let object = support::object_record(
        1,
        support::POINT_CLASS,
        &support::point_payload([1.0, 2.0, 3.0]),
    );
    let bytes = support::archive_version("100", &[object]);
    let result = decode(bytes);
    assert_eq!(
        result.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    );
    assert_valid(&result);

    let matched = result
        .report()
        .dialects()
        .as_ref()
        .expect("Rhino decode reports dialect layers")
        .primary();
    assert_eq!(matched.dialect().as_str(), "rhino:unknown");
    assert_eq!(
        matched.admission(),
        &cadmpeg_core::dialect::Admission::Residual
    );
    assert_eq!(matched.declared()["archive_version"], "100");
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == crate::loss::RhinoLossCode::SourceDialectUnverified.kind())
            .count(),
        1,
        "the unverified admission is charged exactly once"
    );
}
