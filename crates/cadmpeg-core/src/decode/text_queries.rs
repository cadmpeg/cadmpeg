// SPDX-License-Identifier: Apache-2.0
//! Charged borrowed text queries and ASCII case conversion.
use super::{u64_from_index, DecodeContext};
use crate::CodecError;

impl DecodeContext<'_> {
    fn admit_text_search(&self, text: &str, pattern: &str, operation: &'static str) -> Result<(), CodecError> {
        let positions = self.cost_sum(u64_from_index(text.len()), 1, operation)?;
        let pattern = self.cost_sum(u64_from_index(pattern.len()), 1, operation)?;
        self.charge_work(self.cost_product(positions, pattern, operation)?, operation)
    }
    /// Finds a UTF-8 substring after admitting every candidate comparison.
    pub fn find_text(&self, text: &str, pattern: &str, operation: &'static str) -> Result<Option<usize>, CodecError> {
        self.admit_text_search(text, pattern, operation)?;
        Ok(text.find(pattern))
    }
    /// Finds the last UTF-8 substring after admitting every candidate comparison.
    pub fn rfind_text(&self, text: &str, pattern: &str, operation: &'static str) -> Result<Option<usize>, CodecError> {
        self.admit_text_search(text, pattern, operation)?;
        Ok(text.rfind(pattern))
    }
    /// Tests substring membership through the admitted forward search.
    pub fn contains_text(&self, text: &str, pattern: &str, operation: &'static str) -> Result<bool, CodecError> {
        Ok(self.find_text(text, pattern, operation)?.is_some())
    }
    /// Splits at the first UTF-8 substring without allocating either half.
    pub fn split_once<'text>(&self, text: &'text str, pattern: &str, operation: &'static str) -> Result<Option<(&'text str, &'text str)>, CodecError> {
        Ok(self.find_text(text, pattern, operation)?.map(|index| (&text[..index], &text[index + pattern.len()..])))
    }
    /// Splits at the last UTF-8 substring without allocating either half.
    pub fn rsplit_once<'text>(&self, text: &'text str, pattern: &str, operation: &'static str) -> Result<Option<(&'text str, &'text str)>, CodecError> {
        Ok(self.rfind_text(text, pattern, operation)?.map(|index| (&text[..index], &text[index + pattern.len()..])))
    }
    /// Compares a prefix through the one byte-slice equality operation.
    pub fn starts_with(&self, text: &str, prefix: &str, operation: &'static str) -> Result<bool, CodecError> {
        match text.as_bytes().get(..prefix.len()) {
            Some(bytes) => self.equal_bytes(bytes, prefix.as_bytes(), operation),
            None => Ok(false),
        }
    }
    /// Compares a suffix through the one byte-slice equality operation.
    pub fn ends_with(&self, text: &str, suffix: &str, operation: &'static str) -> Result<bool, CodecError> {
        match text.len().checked_sub(suffix.len()) {
            Some(index) => self.equal_bytes(&text.as_bytes()[index..], suffix.as_bytes(), operation),
            None => Ok(false),
        }
    }
    /// Removes a matched prefix without allocating the remaining text.
    pub fn strip_prefix<'text>(&self, text: &'text str, prefix: &str, operation: &'static str) -> Result<Option<&'text str>, CodecError> {
        Ok(if self.starts_with(text, prefix, operation)? { Some(&text[prefix.len()..]) } else { None })
    }
    /// Removes a matched suffix without allocating the remaining text.
    pub fn strip_suffix<'text>(&self, text: &'text str, suffix: &str, operation: &'static str) -> Result<Option<&'text str>, CodecError> {
        Ok(if self.ends_with(text, suffix, operation)? { Some(&text[..text.len() - suffix.len()]) } else { None })
    }
    /// Removes leading and trailing Unicode whitespace after admitting input bytes.
    pub fn trim_text<'text>(&self, text: &'text str, operation: &'static str) -> Result<&'text str, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.trim())
    }
    /// Removes leading Unicode whitespace after admitting input bytes.
    pub fn trim_start_text<'text>(&self, text: &'text str, operation: &'static str) -> Result<&'text str, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.trim_start())
    }
    /// Removes trailing Unicode whitespace after admitting input bytes.
    pub fn trim_end_text<'text>(&self, text: &'text str, operation: &'static str) -> Result<&'text str, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.trim_end())
    }
    /// Converts ASCII letters in place after admitting the complete byte scan.
    pub fn make_ascii_lowercase(&self, text: &mut str, operation: &'static str) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        text.make_ascii_lowercase();
        Ok(())
    }
    /// Converts ASCII letters in place after admitting the complete byte scan.
    pub fn make_ascii_uppercase(&self, text: &mut str, operation: &'static str) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        text.make_ascii_uppercase();
        Ok(())
    }
    /// Copies retained UTF-8 text and converts its ASCII letters to lowercase.
    pub fn to_ascii_lowercase(&self, text: &str, operation: &'static str) -> Result<String, CodecError> {
        let mut output = self.copy_retained_text(text, operation)?;
        self.make_ascii_lowercase(&mut output, operation)?;
        Ok(output)
    }
    /// Copies retained UTF-8 text and converts its ASCII letters to uppercase.
    pub fn to_ascii_uppercase(&self, text: &str, operation: &'static str) -> Result<String, CodecError> {
        let mut output = self.copy_retained_text(text, operation)?;
        self.make_ascii_uppercase(&mut output, operation)?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::CodecError;
    #[test]
    fn borrowed_text_queries_preserve_unicode_boundaries_and_empty_patterns() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        for pattern in ["", "é", "éλ", "x", "λ"] {
            let text = "éλé";
            assert_eq!(ctx.find_text(text, pattern, "find").expect("admission"), text.find(pattern));
            assert_eq!(ctx.rfind_text(text, pattern, "find").expect("admission"), text.rfind(pattern));
            assert_eq!(ctx.split_once(text, pattern, "split").expect("admission"), text.split_once(pattern));
            assert_eq!(ctx.rsplit_once(text, pattern, "split").expect("admission"), text.rsplit_once(pattern));
            assert_eq!(ctx.strip_prefix(text, pattern, "prefix").expect("admission"), text.strip_prefix(pattern));
            assert_eq!(ctx.strip_suffix(text, pattern, "suffix").expect("admission"), text.strip_suffix(pattern));
        }
        assert_eq!(ctx.trim_text("\u{2003}é\n", "trim").expect("admission"), "é");
        assert_eq!(ctx.to_ascii_lowercase("ÉAZλ", "case").expect("admission"), "Éazλ");
        assert_eq!(ctx.to_ascii_uppercase("éazλ", "case").expect("admission"), "éAZλ");
    }
    #[test]
    fn ascii_case_refuses_before_mutation_and_keeps_original_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut text = String::from("ABCé");
        let CodecError::ResourceLimit(first) = ctx.make_ascii_lowercase(&mut text, "case").expect_err("refusal") else { panic!("refusal") };
        assert_eq!(text, "ABCé");
        let CodecError::ResourceLimit(second) = ctx.find_text("a", "a", "search").expect_err("fused") else { panic!("refusal") };
        assert_eq!(first, second);
    }
}
