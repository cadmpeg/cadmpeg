// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::parameter::{macro_parameter_data, ParameterDefect};

#[test]
fn macro_parameter_data_keeps_language_delimiters_outside_hollerith_payloads() {
    let bytes = b"306,MACRO,621,X,Y;LET $S=3Ha;b;ENDM;comment bytes";
    let data = macro_parameter_data(bytes, b',', b';').unwrap();

    assert_eq!(data.defined_entity_type, 621);
    assert_eq!(data.statement_spans.len(), 3);
    assert_eq!(
        data.statement_spans
            .iter()
            .map(|span| &bytes[span.clone()])
            .collect::<Vec<_>>(),
        vec![
            b"306,MACRO,621,X,Y".as_slice(),
            b"LET $S=3Ha;b".as_slice(),
            b"ENDM".as_slice()
        ]
    );
    assert_eq!(
        data.record_end,
        b"306,MACRO,621,X,Y;LET $S=3Ha;b;ENDM;".len()
    );
}

#[test]
fn macro_parameter_data_requires_the_assigned_type_and_arguments() {
    for (bytes, defect) in [
        (
            b"306,MACRO,599,X;ENDM;".as_slice(),
            ParameterDefect::MacroEntityTypeOutOfRange,
        ),
        (
            b"306,MACRO,621;ENDM;".as_slice(),
            ParameterDefect::MacroArgumentListMissing,
        ),
        (
            b"306,NOT_MACRO,621,X;ENDM;".as_slice(),
            ParameterDefect::MacroHeaderMalformed,
        ),
        (
            b"306,MACRO,621,X;LET X=1;".as_slice(),
            ParameterDefect::MacroTerminatorMissing,
        ),
    ] {
        assert_eq!(
            macro_parameter_data(bytes, b',', b';').unwrap_err().0,
            defect
        );
    }
}

#[test]
fn macro_parameter_data_requires_nonempty_language_statements() {
    let error = macro_parameter_data(b"306,MACRO,621,X;;ENDM;", b',', b';').unwrap_err();
    assert_eq!(error.0, ParameterDefect::MacroStatementEmpty);
}
