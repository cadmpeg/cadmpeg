// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::dialect::Admission;
use cadmpeg_core::CodecError;

use super::{classify, USD_SDF_1_4_32, USD_UNKNOWN, USD_USDA_1_0, USD_USDA_1_3, USD_USDC_0_8_0};
use crate::detect::{Cookie, Header};
use crate::loss::UsdLossCode;

fn text(cookie: Cookie, version: &str) -> Header {
    Header::Text {
        cookie,
        version: version.to_owned(),
    }
}

#[test]
fn declared_text_versions_are_admitted() {
    for (header, row) in [
        (text(Cookie::Usda, "1.0"), USD_USDA_1_0),
        (text(Cookie::Usda, "1.3"), USD_USDA_1_3),
        (text(Cookie::Sdf, "1.4.32"), USD_SDF_1_4_32),
    ] {
        let (matched, loss) = classify(&header).admit().unwrap();
        assert_eq!(matched.dialect(), &row);
        assert_eq!(matched.admission(), &Admission::Admitted);
        assert!(loss.is_none());
    }
}

#[test]
fn an_undeclared_text_version_is_residual_and_charged() {
    let (matched, loss) = classify(&text(Cookie::Usda, "9.9")).admit().unwrap();
    assert_eq!(matched.dialect(), &USD_UNKNOWN);
    assert_eq!(matched.admission(), &Admission::Residual);
    assert_eq!(
        matched.declared().values().collect::<Vec<_>>(),
        ["#usda 9.9"]
    );
    assert_eq!(
        loss.map(|loss| loss.code),
        Some(UsdLossCode::SourceDialectUnverified.kind())
    );
}

#[test]
fn a_declared_binary_version_is_admitted() {
    let (matched, loss) = classify(&Header::Binary { version: [0, 8, 0] })
        .admit()
        .unwrap();
    assert_eq!(matched.dialect(), &USD_USDC_0_8_0);
    assert_eq!(matched.admission(), &Admission::Admitted);
    assert_eq!(matched.declared().values().collect::<Vec<_>>(), ["0.8.0"]);
    assert!(loss.is_none());
}

#[test]
fn an_undeclared_binary_version_is_identified_and_refused() {
    let Err(CodecError::UnsupportedDialect { dialects, .. }) =
        classify(&Header::Binary { version: [0, 3, 0] }).admit()
    else {
        panic!("an undeclared binary version is refused");
    };
    assert_eq!(dialects.primary().dialect(), &USD_UNKNOWN);
    assert_eq!(dialects.primary().admission(), &Admission::Refused);
}
