// SPDX-License-Identifier: Apache-2.0
//! Fallible input scans.

use cadmpeg_core::CodecError;

pub(super) fn all<T>(
    values: impl IntoIterator<Item = T>,
    mut predicate: impl FnMut(T) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    for value in values {
        if !predicate(value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn find_map<T, R>(
    values: impl IntoIterator<Item = T>,
    mut project: impl FnMut(T) -> Result<Option<R>, CodecError>,
) -> Result<Option<R>, CodecError> {
    for value in values {
        if let Some(result) = project(value)? {
            return Ok(Some(result));
        }
    }
    Ok(None)
}

pub(super) fn any<T>(
    values: impl IntoIterator<Item = T>,
    mut predicate: impl FnMut(T) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    for value in values {
        if predicate(value)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn find<T>(
    values: impl IntoIterator<Item = T>,
    mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
) -> Result<Option<T>, CodecError> {
    for value in values {
        if predicate(&value)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::CodecError;
    use std::cell::Cell;

    fn counted<T>(visits: &Cell<u32>, value: T) -> T {
        visits.set(visits.get() + 1);
        value
    }

    fn refused<T>(result: Result<T, CodecError>) -> bool {
        result.is_err_and(|error| {
            matches!(error, CodecError::Malformed(message) if message == "scan callback refusal")
        })
    }

    #[test]
    fn validation_scans_stop_at_the_first_decisive_callback() {
        let visits = Cell::new(0);
        assert!(!super::all([1, 2], |_| Ok(counted(&visits, false))).unwrap());
        assert_eq!(visits.replace(0), 1);
        assert_eq!(
            super::find_map([1, 2], |value| Ok(Some(counted(&visits, value)))).unwrap(),
            Some(1)
        );
        assert_eq!(visits.replace(0), 1);
        assert!(super::any([1, 2], |_| Ok(counted(&visits, true))).unwrap());
        assert_eq!(visits.replace(0), 1);
        assert_eq!(
            super::find([1, 2], |value| Ok(counted(&visits, *value == 1))).unwrap(),
            Some(1)
        );
        assert_eq!(visits.get(), 1);
    }

    #[test]
    fn validation_scans_propagate_callback_errors() {
        let refusal = || CodecError::malformed("scan callback refusal");
        assert!(refused(super::all([1], |_| Err(refusal()))));
        assert!(refused(super::find_map([1], |_| Err::<Option<u8>, _>(
            refusal()
        ))));
        assert!(refused(super::any([1], |_| Err(refusal()))));
        assert!(refused(super::find([1], |_| Err(refusal()))));
    }
}
