// SPDX-License-Identifier: Apache-2.0
//! Recursive-descent parser for the USDA grammar (spec §2 to §7).
//!
//! Each method consumes one grammar rule and no padding outside it, except
//! the separator rules, which consume their own padding. Padding classes
//! follow §2.1: inline, single-line, and multiline.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::syntax::{
    AttributeField, AttributeSpec, AttributeValue, Dictionary, DictionaryEntry, DictionaryValue,
    Layer, ListOp, MetaItem, MetaValue, Number, Numbers, Packed, PathList, PrimBody, PrimSpec,
    PropertySpec, Reference, RelationshipField, RelationshipSpec, Specifier, SubLayer, TimeSample,
    Value, Variability, Variant, VariantSet,
};

/// The keywords of §2.5.
const KEYWORDS: &[&str] = &[
    "add",
    "AnimationBlock",
    "append",
    "bezier",
    "class",
    "config",
    "connect",
    "curve",
    "custom",
    "customData",
    "default",
    "def",
    "delete",
    "dictionary",
    "displayUnit",
    "doc",
    "held",
    "hermite",
    "inherits",
    "kind",
    "linear",
    "loop",
    "nameChildren",
    "None",
    "none",
    "offset",
    "oscillate",
    "over",
    "payload",
    "permission",
    "post",
    "pre",
    "prefixSubstitutions",
    "prepend",
    "properties",
    "references",
    "relocates",
    "rel",
    "reorder",
    "rootPrims",
    "repeat",
    "reset",
    "scale",
    "sloped",
    "subLayers",
    "suffixSubstitutions",
    "specializes",
    "spline",
    "symmetryArguments",
    "symmetryFunction",
    "timeSamples",
    "uniform",
    "variantSet",
    "variantSets",
    "variants",
    "varying",
];

/// Metadata key that a bare documentation string sets (§4.1).
pub(crate) const DOCUMENTATION_KEY: &str = "documentation";

fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

/// Parses one whole layer. The header line is consumed as text; the caller
/// classifies it.
pub(crate) fn parse_layer(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Layer, CodecError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "usda: byte {} is not valid UTF-8",
            error.valid_up_to()
        ))
    })?;
    let mut parser = Parser {
        ctx,
        text,
        bytes: text.as_bytes(),
        pos: 0,
    };
    parser.layer()
}

struct Parser<'c, 'a, 't> {
    ctx: &'c DecodeContext<'a>,
    text: &'t str,
    bytes: &'t [u8],
    pos: usize,
}

type Parsed<T> = Result<T, CodecError>;

impl<'t> Parser<'_, '_, 't> {
    // ----------------------------------------------------------------------
    // Diagnostics
    // ----------------------------------------------------------------------

    fn fail<T>(&self, expected: &str) -> Parsed<T> {
        Err(self.error_at(self.pos, expected))
    }

    fn error_at(&self, offset: usize, expected: &str) -> CodecError {
        let before = self.bytes.get(..offset).unwrap_or(self.bytes);
        let line = memchr_count(before, b'\n') + 1;
        let column = before
            .iter()
            .rev()
            .take_while(|byte| **byte != b'\n')
            .count()
            + 1;
        CodecError::malformed(format_args!(
            "usda: line {line} column {column}: expected {expected}"
        ))
    }

    // ----------------------------------------------------------------------
    // Characters and padding (§2.1 to §2.3)
    // ----------------------------------------------------------------------

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.pos
            .checked_add(ahead)
            .and_then(|index| self.bytes.get(index).copied())
    }

    fn rest(&self) -> &'t str {
        self.text.get(self.pos..).unwrap_or("")
    }

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.rest().starts_with(prefix)
    }

    fn advance(&mut self, count: usize) {
        self.pos = self.pos.checked_add(count).unwrap_or(self.bytes.len());
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.advance(1);
            true
        } else {
            false
        }
    }

    fn require(&mut self, byte: u8, expected: &str) -> Parsed<()> {
        if self.eat(byte) {
            Ok(())
        } else {
            self.fail(expected)
        }
    }

    fn is_line_end(byte: u8) -> bool {
        matches!(byte, b'\n' | b'\r')
    }

    /// Consumes one block comment when one starts here.
    fn block_comment(&mut self) -> Parsed<bool> {
        if !self.starts_with("/*") {
            return Ok(false);
        }
        let start = self.pos;
        self.advance(2);
        match self.rest().find("*/") {
            Some(end) => {
                self.advance(end + 2);
                Ok(true)
            }
            None => Err(self.error_at(start, "`*/` to close the block comment")),
        }
    }

    /// Consumes one line comment when one starts here.
    fn line_comment(&mut self) -> bool {
        if self.peek() == Some(b'#') || self.starts_with("//") {
            while let Some(byte) = self.peek() {
                if Self::is_line_end(byte) {
                    break;
                }
                self.advance(1);
            }
            true
        } else {
            false
        }
    }

    fn skip_inline(&mut self) -> Parsed<()> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t') => self.advance(1),
                Some(b'/') if self.block_comment()? => {}
                _ => return Ok(()),
            }
        }
    }

    fn skip_single_line(&mut self) -> Parsed<()> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t') => self.advance(1),
                Some(b'/') if self.block_comment()? => {}
                Some(b'#' | b'/') if self.line_comment() => {}
                _ => return Ok(()),
            }
        }
    }

    fn skip_multiline(&mut self) -> Parsed<()> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.advance(1),
                Some(b'/') if self.block_comment()? => {}
                Some(b'#' | b'/') if self.line_comment() => {}
                _ => return Ok(()),
            }
        }
    }

    /// A token separator: optional block comments, at least one space or
    /// tab, then inline padding (§2.3).
    fn token_separator(&mut self) -> Parsed<()> {
        while self.block_comment()? {}
        if !matches!(self.peek(), Some(b' ' | b'\t')) {
            return self.fail("a space between tokens");
        }
        self.skip_inline()
    }

    /// `=` with optional inline padding on each side.
    fn assignment(&mut self) -> Parsed<()> {
        self.skip_inline()?;
        self.require(b'=', "`=`")?;
        self.skip_inline()
    }

    /// A statement separator and its padding (§3.3). Returns false, consuming
    /// nothing, when no separator follows.
    fn statement_separator(&mut self) -> Parsed<bool> {
        let start = self.pos;
        self.skip_single_line()?;
        match self.peek() {
            Some(b'\n' | b'\r' | b';') => {
                self.advance(1);
                self.skip_multiline()?;
                Ok(true)
            }
            _ => {
                self.pos = start;
                Ok(false)
            }
        }
    }

    /// A list separator `,` and its padding (§3.4). Returns false, consuming
    /// nothing, when no `,` follows.
    fn list_separator(&mut self) -> Parsed<bool> {
        let start = self.pos;
        self.skip_inline()?;
        if self.eat(b',') {
            self.skip_multiline()?;
            Ok(true)
        } else {
            self.pos = start;
            Ok(false)
        }
    }

    // ----------------------------------------------------------------------
    // Words and identifiers (§2.4, §2.5)
    // ----------------------------------------------------------------------

    fn next_char(&self) -> Option<char> {
        self.rest().chars().next()
    }

    /// The UTF-8 identifier starting here, without consuming it.
    fn peek_identifier(&self) -> Option<&'t str> {
        let rest = self.rest();
        let mut chars = rest.char_indices();
        let (_, first) = chars.next()?;
        if first != '_' && !unicode_ident::is_xid_start(first) {
            return None;
        }
        let end = chars
            .find(|(_, character)| !unicode_ident::is_xid_continue(*character))
            .map_or(rest.len(), |(index, _)| index);
        rest.get(..end)
    }

    fn identifier(&mut self, expected: &str) -> Parsed<&'t str> {
        match self.peek_identifier() {
            Some(word) => {
                self.advance(word.len());
                Ok(word)
            }
            None => self.fail(expected),
        }
    }

    /// Whether `keyword` starts here as a whole word.
    fn at_keyword(&self, keyword: &str) -> bool {
        self.peek_identifier() == Some(keyword)
    }

    fn eat_keyword(&mut self, keyword: &str) -> bool {
        if self.at_keyword(keyword) {
            self.advance(keyword.len());
            true
        } else {
            false
        }
    }

    fn require_keyword(&mut self, keyword: &'static str) -> Parsed<()> {
        if self.eat_keyword(keyword) {
            Ok(())
        } else {
            self.fail(keyword)
        }
    }

    /// An identifier that is not a keyword.
    fn keywordless_identifier(&mut self, expected: &str) -> Parsed<&'t str> {
        match self.peek_identifier() {
            Some(word) if !is_keyword(word) => {
                self.advance(word.len());
                Ok(word)
            }
            _ => self.fail(expected),
        }
    }

    /// A type identifier: keywordless identifiers joined by `::` (§2.4).
    fn type_identifier(&mut self, expected: &str) -> Parsed<String> {
        let start = self.pos;
        self.keywordless_identifier(expected)?;
        while self.starts_with("::") {
            self.advance(2);
            self.keywordless_identifier(expected)?;
        }
        Ok(self.slice(start).to_owned())
    }

    fn at_type_identifier(&self) -> bool {
        self.peek_identifier().is_some_and(|word| !is_keyword(word))
    }

    /// A namespaced identifier or a keyword: a property name (§6.1).
    fn property_name(&mut self) -> Parsed<String> {
        let start = self.pos;
        self.identifier("a property name")?;
        while self.peek() == Some(b':') && self.peek_at(1) != Some(b':') {
            let colon = self.pos;
            self.advance(1);
            if self.peek_identifier().is_none() {
                self.pos = colon;
                break;
            }
            self.identifier("a property name part")?;
        }
        Ok(self.slice(start).to_owned())
    }

    fn slice(&self, start: usize) -> &'t str {
        self.text.get(start..self.pos).unwrap_or("")
    }

    // ----------------------------------------------------------------------
    // Literals (§2.6 to §2.9)
    // ----------------------------------------------------------------------

    fn at_number(&self) -> bool {
        match self.peek() {
            Some(b'0'..=b'9') => true,
            Some(b'.') => matches!(self.peek_at(1), Some(b'0'..=b'9')),
            Some(b'-') => {
                matches!(self.peek_at(1), Some(b'0'..=b'9'))
                    || (self.peek_at(1) == Some(b'.')
                        && matches!(self.peek_at(2), Some(b'0'..=b'9')))
                    || self.text.get(self.pos + 1..).is_some_and(|rest| {
                        rest.starts_with("inf")
                            && !rest
                                .get(3..)
                                .and_then(|tail| tail.chars().next())
                                .is_some_and(unicode_ident::is_xid_continue)
                    })
            }
            _ => self.at_keyword("inf") || self.at_keyword("nan"),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.advance(1);
        }
        self.pos - start
    }

    fn number(&mut self) -> Parsed<Number> {
        let start = self.pos;
        if self.eat_keyword("nan") {
            return Ok(Number {
                float: f64::NAN,
                integer: None,
            });
        }
        let negative = self.eat(b'-');
        if self.eat_keyword("inf") {
            let float = if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
            return Ok(Number {
                float,
                integer: None,
            });
        }
        let mut integral = true;
        if self.digits() > 0 {
            if self.eat(b'.') {
                integral = false;
                self.digits();
            }
        } else if self.eat(b'.') {
            integral = false;
            if self.digits() == 0 {
                return self.fail("digits after `.`");
            }
        } else {
            return self.fail("a number");
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            integral = false;
            self.advance(1);
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.advance(1);
            }
            if self.digits() == 0 {
                return self.fail("exponent digits");
            }
        }
        let text = self.slice(start);
        let float = text
            .parse::<f64>()
            .map_err(|_| self.error_at(start, "a number"))?;
        let integer = if integral {
            text.parse::<i128>().ok()
        } else {
            None
        };
        Ok(Number { float, integer })
    }

    fn at_string(&self) -> bool {
        matches!(self.peek(), Some(b'"' | b'\''))
    }

    /// A quoted string with its escapes replaced (§2.7).
    fn string(&mut self) -> Parsed<String> {
        let start = self.pos;
        let Some(quote) = self.peek().filter(|byte| matches!(byte, b'"' | b'\'')) else {
            return self.fail("a quoted string");
        };
        let triple = self.peek_at(1) == Some(quote) && self.peek_at(2) == Some(quote);
        let width = if triple { 3 } else { 1 };
        self.advance(width);
        let content_start = self.pos;
        loop {
            match self.peek() {
                None => return Err(self.error_at(start, "a closing quote")),
                Some(b'\\') => {
                    if matches!(self.peek_at(1), Some(b'\\')) || self.peek_at(1) == Some(quote) {
                        self.advance(2);
                    } else {
                        self.advance(1);
                    }
                }
                Some(byte) if byte == quote => {
                    if !triple {
                        break;
                    }
                    if self.peek_at(1) == Some(quote) && self.peek_at(2) == Some(quote) {
                        break;
                    }
                    self.advance(1);
                }
                Some(byte) if Self::is_line_end(byte) && !triple => {
                    return Err(self.error_at(start, "a closing quote on the same line"));
                }
                Some(_) => {
                    let width = self.next_char().map_or(1, char::len_utf8);
                    self.advance(width);
                }
            }
        }
        let raw = self.text.get(content_start..self.pos).unwrap_or("");
        self.advance(width);
        unescape(raw).ok_or_else(|| self.error_at(start, "a string that is valid UTF-8"))
    }

    fn at_asset(&self) -> bool {
        self.peek() == Some(b'@')
    }

    /// An asset path (§2.8).
    fn asset_path(&mut self) -> Parsed<String> {
        let start = self.pos;
        if self.starts_with("@@@") {
            self.advance(3);
            let mut path = String::new();
            loop {
                if self.starts_with("\\@@@") {
                    path.push_str("@@@");
                    self.advance(4);
                    continue;
                }
                if self.starts_with("@@@") {
                    let run = self.rest().bytes().take_while(|byte| *byte == b'@').count();
                    let kept = run.min(5) - 3;
                    path.extend(std::iter::repeat_n('@', kept));
                    self.advance(kept + 3);
                    return Ok(path);
                }
                match self.next_char() {
                    None => return Err(self.error_at(start, "`@@@` to close the asset path")),
                    Some(character) if character == '\n' || character == '\r' => {
                        return Err(self.error_at(start, "`@@@` on the same line"))
                    }
                    Some(character) => {
                        path.push(character);
                        self.advance(character.len_utf8());
                    }
                }
            }
        }
        self.require(b'@', "an asset path")?;
        let content = self.pos;
        loop {
            match self.peek() {
                Some(b'@') => break,
                None | Some(b'\n' | b'\r') => {
                    return Err(self.error_at(start, "`@` to close the asset path"))
                }
                Some(_) => {
                    let width = self.next_char().map_or(1, char::len_utf8);
                    self.advance(width);
                }
            }
        }
        let path = self.slice(content).to_owned();
        self.advance(1);
        Ok(path)
    }

    fn at_path_ref(&self) -> bool {
        self.peek() == Some(b'<')
    }

    /// A path reference (§2.9), without its angle brackets.
    fn path_ref(&mut self) -> Parsed<String> {
        let start = self.pos;
        self.require(b'<', "a path reference")?;
        let content = self.pos;
        loop {
            match self.peek() {
                Some(b'>') => break,
                None | Some(b'\n' | b'\r') => {
                    return Err(self.error_at(start, "`>` to close the path reference"))
                }
                Some(_) => {
                    let width = self.next_char().map_or(1, char::len_utf8);
                    self.advance(width);
                }
            }
        }
        let path = self.slice(content).trim().to_owned();
        self.advance(1);
        Ok(path)
    }

    // ----------------------------------------------------------------------
    // Values (§7.1)
    // ----------------------------------------------------------------------

    fn typed_value(&mut self) -> Parsed<Value> {
        if self.at_number() {
            return Ok(Value::Number(self.number()?));
        }
        if self.at_string() {
            return Ok(Value::String(self.string()?));
        }
        if self.at_asset() {
            return Ok(Value::Asset(self.asset_path()?));
        }
        if self.at_path_ref() {
            return Ok(Value::Path(self.path_ref()?));
        }
        match self.peek() {
            Some(b'(') => self.tuple(),
            Some(b'[') => self.list(),
            _ if self.at_type_identifier() => {
                Ok(Value::Identifier(self.type_identifier("a value")?))
            }
            _ => self.fail("a value"),
        }
    }

    /// An item inside a tuple or list: an atomic value or a nested tuple or
    /// list.
    fn item_value(&mut self, lists: bool) -> Parsed<Value> {
        match self.peek() {
            Some(b'(') => self.tuple(),
            Some(b'[') if lists => self.list(),
            _ if self.at_number() => Ok(Value::Number(self.number()?)),
            _ if self.at_string() => Ok(Value::String(self.string()?)),
            _ if self.at_asset() => Ok(Value::Asset(self.asset_path()?)),
            _ if self.at_type_identifier() => {
                Ok(Value::Identifier(self.type_identifier("a value")?))
            }
            _ => self.fail("a value"),
        }
    }

    fn tuple(&mut self) -> Parsed<Value> {
        let _depth = self.ctx.enter_nested("usda_tuple_nesting")?;
        self.require(b'(', "`(`")?;
        self.skip_multiline()?;
        let mut items = vec![self.item_value(false)?];
        while self.list_separator()? {
            if self.peek() == Some(b')') {
                break;
            }
            items.push(self.item_value(false)?);
        }
        self.skip_multiline()?;
        self.require(b')', "`,` or `)`")?;
        Ok(Value::Tuple(items))
    }

    fn list(&mut self) -> Parsed<Value> {
        let _depth = self.ctx.enter_nested("usda_list_nesting")?;
        self.require(b'[', "`[`")?;
        let empty = self.pos;
        self.skip_inline()?;
        if self.eat(b']') {
            return Ok(Value::List(Vec::new()));
        }
        self.pos = empty;
        self.skip_multiline()?;
        let mut items = Packer::default();
        items.push(self.item_value(true)?);
        while self.list_separator()? {
            if self.peek() == Some(b']') {
                break;
            }
            items.push(self.item_value(true)?);
        }
        self.skip_multiline()?;
        self.require(b']', "`,` or `]`")?;
        Ok(items.finish())
    }

    /// Statements of `item` up to `close`, inside an opened bracket (§3.3).
    fn statement_sequence<T>(
        &mut self,
        close: u8,
        mut item: impl FnMut(&mut Self) -> Parsed<T>,
    ) -> Parsed<Vec<T>> {
        self.skip_multiline()?;
        let mut items = Vec::new();
        if self.peek() == Some(close) {
            self.advance(1);
            return Ok(items);
        }
        loop {
            items.push(item(self)?);
            let separated = self.statement_separator()?;
            self.skip_multiline()?;
            if self.eat(close) {
                return Ok(items);
            }
            if !separated {
                return self.fail("a line end or `;` between statements");
            }
        }
    }

    /// A comma list of `item` up to `close`, inside an opened bracket (§3.4).
    fn comma_list<T>(
        &mut self,
        close: u8,
        mut item: impl FnMut(&mut Self) -> Parsed<T>,
    ) -> Parsed<Vec<T>> {
        self.skip_multiline()?;
        let mut items = Vec::new();
        if self.eat(close) {
            return Ok(items);
        }
        loop {
            items.push(item(self)?);
            let separated = self.list_separator()?;
            self.skip_multiline()?;
            if self.eat(close) {
                return Ok(items);
            }
            if !separated {
                return self.fail("`,` between list items");
            }
        }
    }

    fn dictionary(&mut self) -> Parsed<Dictionary> {
        let _depth = self.ctx.enter_nested("usda_dictionary_nesting")?;
        self.require(b'{', "`{`")?;
        let entries = self.statement_sequence(b'}', Self::dictionary_entry)?;
        Ok(Dictionary { entries })
    }

    fn dictionary_key(&mut self) -> Parsed<String> {
        if self.at_string() {
            self.string()
        } else {
            Ok(self.identifier("a dictionary key")?.to_owned())
        }
    }

    fn dictionary_entry(&mut self) -> Parsed<DictionaryEntry> {
        if self.eat_keyword("dictionary") {
            self.token_separator()?;
            let key = self.dictionary_key()?;
            self.assignment()?;
            let value = DictionaryValue::Dictionary(self.dictionary()?);
            return Ok(DictionaryEntry { key, value });
        }
        let type_name = self.type_identifier("a dictionary entry type")?;
        let array = self.array_marker()?;
        self.token_separator()?;
        let key = self.dictionary_key()?;
        self.assignment()?;
        let value = self.typed_value()?;
        Ok(DictionaryEntry {
            key,
            value: DictionaryValue::Typed {
                type_name,
                array,
                value,
            },
        })
    }

    /// An optional `[]` after a type name.
    fn array_marker(&mut self) -> Parsed<bool> {
        let start = self.pos;
        self.skip_inline()?;
        if self.eat(b'[') {
            self.skip_inline()?;
            self.require(b']', "`]`")?;
            Ok(true)
        } else {
            self.pos = start;
            Ok(false)
        }
    }

    /// Source text of a balanced bracket group, for spline and array-edit
    /// values that the model keeps as text.
    fn balanced_text(&mut self, open: u8, close: u8) -> Parsed<String> {
        let start = self.pos;
        self.require(open, "an opening bracket")?;
        let mut depth = 1_usize;
        while depth > 0 {
            match self.peek() {
                None => return Err(self.error_at(start, "a closing bracket")),
                Some(b'"' | b'\'') => {
                    self.string()?;
                }
                Some(byte) => {
                    if byte == open {
                        depth += 1;
                    } else if byte == close {
                        depth -= 1;
                    }
                    let width = self.next_char().map_or(1, char::len_utf8);
                    self.advance(width);
                }
            }
        }
        Ok(self.slice(start).to_owned())
    }

    // ----------------------------------------------------------------------
    // Metadata (§4)
    // ----------------------------------------------------------------------

    fn metadata_block(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Parsed<MetaItem>,
    ) -> Parsed<Vec<MetaItem>> {
        let _depth = self.ctx.enter_nested("usda_metadata_nesting")?;
        self.require(b'(', "`(`")?;
        self.statement_sequence(b')', |parser| item(parser))
    }

    fn doc_item(&mut self) -> Parsed<MetaItem> {
        Ok(MetaItem {
            list_op: None,
            key: DOCUMENTATION_KEY.to_owned(),
            value: MetaValue::Value(Value::String(self.string()?)),
        })
    }

    /// `None`, a dictionary, or a typed value: the right side of a generic
    /// key-value item.
    fn generic_meta_value(&mut self) -> Parsed<MetaValue> {
        if self.eat_keyword("None") {
            Ok(MetaValue::None)
        } else if self.peek() == Some(b'{') {
            Ok(MetaValue::Dictionary(self.dictionary()?))
        } else {
            Ok(MetaValue::Value(self.typed_value()?))
        }
    }

    /// `None` or a list value: the right side of a generic list-op item.
    fn list_op_value(&mut self) -> Parsed<MetaValue> {
        if self.eat_keyword("None") {
            Ok(MetaValue::None)
        } else if self.peek() == Some(b'[') {
            Ok(MetaValue::Value(self.list()?))
        } else {
            self.fail("`None` or a list")
        }
    }

    /// The items of §4.1 shared by every metadata block. `prim` admits the
    /// `customData` and `symmetryArguments` keys.
    fn shared_meta_item(&mut self, prim: bool) -> Parsed<Option<MetaItem>> {
        if self.at_string() {
            return self.doc_item().map(Some);
        }
        if self.eat_keyword("doc") {
            self.assignment()?;
            let value = MetaValue::Value(Value::String(self.string()?));
            return Ok(Some(MetaItem {
                list_op: None,
                key: "doc".to_owned(),
                value,
            }));
        }
        for keyword in ["customData", "symmetryArguments"] {
            if prim && self.at_keyword(keyword) {
                self.advance(keyword.len());
                self.assignment()?;
                let value = self.generic_meta_value()?;
                return Ok(Some(MetaItem {
                    list_op: None,
                    key: keyword.to_owned(),
                    value,
                }));
            }
        }
        if let Some(op) = self.peek_identifier().and_then(ListOp::from_keyword) {
            let start = self.pos;
            self.advance(self.peek_identifier().map_or(0, str::len));
            self.token_separator()?;
            if !self.at_type_identifier() {
                self.pos = start;
                return Ok(None);
            }
            let key = self.type_identifier("a metadata key")?;
            self.assignment()?;
            let value = self.list_op_value()?;
            return Ok(Some(MetaItem {
                list_op: Some(op),
                key,
                value,
            }));
        }
        if self.at_type_identifier() {
            let key = self.type_identifier("a metadata key")?;
            self.assignment()?;
            let value = self.generic_meta_value()?;
            return Ok(Some(MetaItem {
                list_op: None,
                key,
                value,
            }));
        }
        Ok(None)
    }

    fn identifier_item(&mut self, key: &'static str, optional: bool) -> Parsed<MetaItem> {
        self.assignment()?;
        let value = if optional && !self.at_type_identifier() {
            None
        } else {
            Some(self.type_identifier("an identifier")?)
        };
        Ok(MetaItem {
            list_op: None,
            key: key.to_owned(),
            value: MetaValue::Identifier(value),
        })
    }

    fn layer_meta_item(&mut self) -> Parsed<MetaItem> {
        if self.eat_keyword("subLayers") {
            self.assignment()?;
            self.require(b'[', "`[`")?;
            let layers = self.comma_list(b']', Self::sublayer)?;
            return Ok(MetaItem {
                list_op: None,
                key: "subLayers".to_owned(),
                value: MetaValue::SubLayers(layers),
            });
        }
        if self.eat_keyword("relocates") {
            return self.relocates();
        }
        match self.shared_meta_item(false)? {
            Some(item) => Ok(item),
            None => self.fail("a layer metadata item"),
        }
    }

    fn sublayer(&mut self) -> Parsed<SubLayer> {
        let asset = self.asset_path()?;
        let (offset, scale) = self.layer_offsets()?;
        Ok(SubLayer {
            asset,
            offset,
            scale,
        })
    }

    /// An optional `( offset = n; scale = n )` block.
    fn layer_offsets(&mut self) -> Parsed<(Option<Number>, Option<Number>)> {
        let start = self.pos;
        self.skip_inline()?;
        if self.peek() != Some(b'(') {
            self.pos = start;
            return Ok((None, None));
        }
        self.advance(1);
        let mut offset = None;
        let mut scale = None;
        self.statement_sequence(b')', |parser| {
            if parser.eat_keyword("offset") {
                parser.assignment()?;
                offset = Some(parser.number()?);
            } else if parser.eat_keyword("scale") {
                parser.assignment()?;
                scale = Some(parser.number()?);
            } else {
                return parser.fail("`offset` or `scale`");
            }
            Ok(())
        })?;
        Ok((offset, scale))
    }

    fn relocates(&mut self) -> Parsed<MetaItem> {
        self.assignment()?;
        self.require(b'{', "`{`")?;
        let pairs = self.comma_list(b'}', |parser| {
            let source = parser.path_ref()?;
            parser.skip_inline()?;
            parser.require(b':', "`:`")?;
            parser.skip_inline()?;
            let target = parser.path_ref()?;
            Ok((source, target))
        })?;
        Ok(MetaItem {
            list_op: None,
            key: "relocates".to_owned(),
            value: MetaValue::Relocates(pairs),
        })
    }

    fn prim_meta_item(&mut self) -> Parsed<MetaItem> {
        let list_op = self.peek_identifier().and_then(ListOp::from_keyword);
        let start = self.pos;
        if list_op.is_some() {
            self.advance(self.peek_identifier().map_or(0, str::len));
            self.token_separator()?;
        }
        for (keyword, arc) in [
            ("references", Arc::References),
            ("payload", Arc::Payload),
            ("inherits", Arc::Inherits),
            ("specializes", Arc::Specializes),
            ("variantSets", Arc::VariantSets),
        ] {
            if self.eat_keyword(keyword) {
                self.assignment()?;
                let value = match arc {
                    Arc::References => MetaValue::References(self.reference_list(true)?),
                    Arc::Payload => MetaValue::Payloads(self.reference_list(false)?),
                    Arc::Inherits | Arc::Specializes => MetaValue::Paths(self.path_list_value()?),
                    Arc::VariantSets => MetaValue::Names(self.name_list()?),
                };
                return Ok(MetaItem {
                    list_op,
                    key: keyword.to_owned(),
                    value,
                });
            }
        }
        if list_op.is_some() {
            self.pos = start;
            return match self.shared_meta_item(true)? {
                Some(item) => Ok(item),
                None => self.fail("a list-op metadata item"),
            };
        }
        if self.eat_keyword("kind") {
            self.assignment()?;
            let value = MetaValue::Value(Value::String(self.string()?));
            return Ok(MetaItem {
                list_op: None,
                key: "kind".to_owned(),
                value,
            });
        }
        if self.eat_keyword("variants") {
            self.assignment()?;
            let value = MetaValue::Dictionary(self.dictionary()?);
            return Ok(MetaItem {
                list_op: None,
                key: "variants".to_owned(),
                value,
            });
        }
        if self.eat_keyword("relocates") {
            return self.relocates();
        }
        for keyword in ["prefixSubstitutions", "suffixSubstitutions"] {
            if self.eat_keyword(keyword) {
                self.assignment()?;
                self.require(b'{', "`{`")?;
                let pairs = self.comma_list(b'}', |parser| {
                    let from = parser.string()?;
                    parser.skip_inline()?;
                    parser.require(b':', "`:`")?;
                    parser.skip_inline()?;
                    Ok((from, parser.string()?))
                })?;
                return Ok(MetaItem {
                    list_op: None,
                    key: keyword.to_owned(),
                    value: MetaValue::StringMap(pairs),
                });
            }
        }
        if let Some(item) = self.permission_or_symmetry()? {
            return Ok(item);
        }
        match self.shared_meta_item(true)? {
            Some(item) => Ok(item),
            None => self.fail("a prim metadata item"),
        }
    }

    fn permission_or_symmetry(&mut self) -> Parsed<Option<MetaItem>> {
        if self.eat_keyword("permission") {
            return self.identifier_item("permission", false).map(Some);
        }
        if self.eat_keyword("symmetryFunction") {
            return self.identifier_item("symmetryFunction", true).map(Some);
        }
        Ok(None)
    }

    fn property_meta_item(&mut self, attribute: bool) -> Parsed<MetaItem> {
        if attribute && self.eat_keyword("displayUnit") {
            return self.identifier_item("displayUnit", false);
        }
        if let Some(item) = self.permission_or_symmetry()? {
            return Ok(item);
        }
        match self.shared_meta_item(true)? {
            Some(item) => Ok(item),
            None => self.fail("a property metadata item"),
        }
    }

    fn reference_list(&mut self, references: bool) -> Parsed<Vec<Reference>> {
        if self.eat_keyword("None") {
            return Ok(Vec::new());
        }
        if self.eat(b'[') {
            return self.comma_list(b']', |parser| parser.reference(references));
        }
        Ok(vec![self.reference(references)?])
    }

    fn reference(&mut self, references: bool) -> Parsed<Reference> {
        let (asset, prim_path) = if self.at_path_ref() {
            (None, Some(self.path_ref()?))
        } else {
            let asset = self.asset_path()?;
            let start = self.pos;
            self.skip_inline()?;
            let path = if self.at_path_ref() {
                Some(self.path_ref()?)
            } else {
                self.pos = start;
                None
            };
            (Some(asset), path)
        };
        let mut reference = Reference {
            asset,
            prim_path,
            offset: None,
            scale: None,
            custom_data: None,
        };
        let start = self.pos;
        self.skip_inline()?;
        if self.peek() != Some(b'(') {
            self.pos = start;
            return Ok(reference);
        }
        self.advance(1);
        self.statement_sequence(b')', |parser| {
            if parser.eat_keyword("offset") {
                parser.assignment()?;
                reference.offset = Some(parser.number()?);
            } else if parser.eat_keyword("scale") {
                parser.assignment()?;
                reference.scale = Some(parser.number()?);
            } else if references && parser.eat_keyword("customData") {
                parser.assignment()?;
                reference.custom_data = Some(parser.dictionary()?);
            } else {
                return parser.fail("a reference parameter");
            }
            Ok(())
        })?;
        Ok(reference)
    }

    fn path_list_value(&mut self) -> Parsed<Vec<String>> {
        match self.path_list()? {
            PathList::None => Ok(Vec::new()),
            PathList::Paths(paths) => Ok(paths),
        }
    }

    /// `None`, one path reference, or a bracketed list of them.
    fn path_list(&mut self) -> Parsed<PathList> {
        if self.eat_keyword("None") {
            return Ok(PathList::None);
        }
        if self.eat(b'[') {
            return Ok(PathList::Paths(self.comma_list(b']', Self::path_ref)?));
        }
        Ok(PathList::Paths(vec![self.path_ref()?]))
    }

    /// A string or a bracketed list of strings (§4.4).
    fn name_list(&mut self) -> Parsed<Vec<String>> {
        if self.eat(b'[') {
            let names = self.comma_list(b']', Self::string)?;
            if names.is_empty() {
                return self.fail("a name");
            }
            return Ok(names);
        }
        Ok(vec![self.string()?])
    }

    // ----------------------------------------------------------------------
    // Layer and prims (§3, §5)
    // ----------------------------------------------------------------------

    fn layer(&mut self) -> Parsed<Layer> {
        if self.peek() != Some(b'#') {
            return self.fail("the `#usda` header");
        }
        while let Some(byte) = self.peek() {
            self.advance(1);
            if Self::is_line_end(byte) {
                break;
            }
        }
        let mut layer = Layer::default();
        self.skip_multiline()?;
        if self.peek() == Some(b'(') {
            layer.metadata = self.metadata_block(Self::layer_meta_item)?;
        }
        loop {
            self.skip_multiline()?;
            if self.at_end() {
                return Ok(layer);
            }
            if self.eat_keyword("reorder") {
                self.token_separator()?;
                self.require_keyword("rootPrims")?;
                self.assignment()?;
                layer.root_order = Some(self.name_list()?);
                self.skip_single_line()?;
                if !self.at_end() && !matches!(self.peek(), Some(b'\n' | b'\r' | b';')) {
                    return self.fail("a line end after `reorder rootPrims`");
                }
                continue;
            }
            layer.prims.push(self.prim_spec()?);
            self.skip_single_line()?;
            if !self.at_end() && !self.peek().is_some_and(Self::is_line_end) {
                return self.fail("a line end after a root prim");
            }
        }
    }

    fn at_prim_spec(&self) -> bool {
        self.at_keyword("def") || self.at_keyword("over") || self.at_keyword("class")
    }

    fn prim_spec(&mut self) -> Parsed<PrimSpec> {
        let _depth = self.ctx.enter_nested("usda_prim_nesting")?;
        self.ctx.charge_entities(1, "usda_prim")?;
        let offset = self.pos;
        let specifier = if self.eat_keyword("def") {
            Specifier::Def
        } else if self.eat_keyword("over") {
            Specifier::Over
        } else if self.eat_keyword("class") {
            Specifier::Class
        } else {
            return self.fail("`def`, `over`, or `class`");
        };
        self.token_separator()?;
        let type_name = if self.at_string() {
            None
        } else {
            let start = self.pos;
            self.type_identifier("a prim type or name")?;
            loop {
                let before = self.pos;
                self.skip_inline()?;
                if !self.eat(b'.') {
                    self.pos = before;
                    break;
                }
                self.skip_inline()?;
                self.type_identifier("a prim type part")?;
            }
            let name: String = self
                .slice(start)
                .chars()
                .filter(|character| !matches!(character, ' ' | '\t'))
                .collect();
            self.token_separator()?;
            Some(name)
        };
        let name = self.string()?;
        self.skip_multiline()?;
        let metadata = if self.peek() == Some(b'(') {
            let items = self.metadata_block(Self::prim_meta_item)?;
            self.skip_multiline()?;
            items
        } else {
            Vec::new()
        };
        self.require(b'{', "`{` to open the prim body")?;
        let body = self.prim_body()?;
        self.require(b'}', "`}` to close the prim body")?;
        Ok(PrimSpec {
            specifier,
            type_name,
            name,
            metadata,
            body,
            offset,
        })
    }

    fn prim_body(&mut self) -> Parsed<PrimBody> {
        let mut body = PrimBody::default();
        loop {
            self.skip_multiline()?;
            if self.peek() == Some(b'}') || self.at_end() {
                return Ok(body);
            }
            if self.at_prim_spec() {
                body.children.push(self.prim_spec()?);
                self.end_line_statement()?;
                continue;
            }
            if self.at_keyword("variantSet") {
                body.variant_sets.push(self.variant_set()?);
                self.end_line_statement()?;
                continue;
            }
            if self.at_keyword("reorder") && self.order_statement(&mut body)? {
                self.end_statement()?;
                continue;
            }
            self.ctx.charge_entities(1, "usda_property")?;
            body.properties.push(self.property_spec()?);
            self.end_statement()?;
        }
    }

    /// The end of a prim or variant-set statement: a line end (§5.2).
    fn end_line_statement(&mut self) -> Parsed<()> {
        self.skip_single_line()?;
        if self.peek().is_some_and(Self::is_line_end) {
            self.advance(1);
            Ok(())
        } else {
            self.fail("a line end after `}`")
        }
    }

    /// The end of a property or order statement: a line end or `;` (§5.2).
    fn end_statement(&mut self) -> Parsed<()> {
        self.skip_single_line()?;
        if matches!(self.peek(), Some(b'\n' | b'\r' | b';')) {
            self.advance(1);
            Ok(())
        } else {
            self.fail("a line end or `;` after the property")
        }
    }

    /// A `reorder nameChildren` or `reorder properties` statement. Returns
    /// false, consuming nothing, when `reorder` starts a list-op property.
    fn order_statement(&mut self, body: &mut PrimBody) -> Parsed<bool> {
        let start = self.pos;
        self.require_keyword("reorder")?;
        self.token_separator()?;
        let children = if self.eat_keyword("nameChildren") {
            true
        } else if self.eat_keyword("properties") {
            false
        } else {
            self.pos = start;
            return Ok(false);
        };
        self.assignment()?;
        let names = self.name_list()?;
        if children {
            body.child_orders.push(names);
        } else {
            body.property_orders.push(names);
        }
        Ok(true)
    }

    fn variant_set(&mut self) -> Parsed<VariantSet> {
        let _depth = self.ctx.enter_nested("usda_variant_set_nesting")?;
        self.require_keyword("variantSet")?;
        self.token_separator()?;
        let name = self.string()?;
        self.skip_inline()?;
        self.require(b'=', "`=`")?;
        self.skip_multiline()?;
        self.require(b'{', "`{` to open the variant set")?;
        let mut variants = Vec::new();
        loop {
            self.skip_multiline()?;
            if self.eat(b'}') {
                return Ok(VariantSet { name, variants });
            }
            let variant_name = self.string()?;
            self.skip_multiline()?;
            let metadata = if self.peek() == Some(b'(') {
                let items = self.metadata_block(Self::prim_meta_item)?;
                self.skip_multiline()?;
                items
            } else {
                Vec::new()
            };
            self.require(b'{', "`{` to open the variant")?;
            let body = self.prim_body()?;
            self.require(b'}', "`}` to close the variant")?;
            variants.push(Variant {
                name: variant_name,
                metadata,
                body,
            });
        }
    }

    // ----------------------------------------------------------------------
    // Properties (§6)
    // ----------------------------------------------------------------------

    fn property_spec(&mut self) -> Parsed<PropertySpec> {
        let offset = self.pos;
        let list_op = self.peek_identifier().and_then(ListOp::from_keyword);
        if list_op.is_some() {
            self.advance(self.peek_identifier().map_or(0, str::len));
            self.token_separator()?;
        }
        let after_op = self.pos;
        let custom = self.eat_keyword("custom");
        if custom {
            self.token_separator()?;
        }
        let varying = self.eat_keyword("varying");
        if varying {
            self.token_separator()?;
        }
        if self.eat_keyword("rel") {
            return self
                .relationship(list_op, custom, varying, offset)
                .map(PropertySpec::Relationship);
        }
        if varying {
            return self.fail("`rel` after `varying`");
        }
        self.pos = after_op;
        self.attribute(list_op, offset).map(PropertySpec::Attribute)
    }

    fn attribute(&mut self, list_op: Option<ListOp>, offset: usize) -> Parsed<AttributeSpec> {
        let custom = self.eat_keyword("custom");
        if custom {
            self.token_separator()?;
        }
        let variability = if self.eat_keyword("uniform") {
            self.token_separator()?;
            Variability::Uniform
        } else if self.eat_keyword("config") {
            self.token_separator()?;
            Variability::Config
        } else {
            Variability::Varying
        };
        let type_name = self.type_identifier("an attribute type")?;
        let array = self.array_marker()?;
        self.token_separator()?;
        let name = self.property_name()?;
        let mut attribute = AttributeSpec {
            list_op,
            custom,
            variability,
            type_name,
            array,
            name,
            field: AttributeField::Declared,
            metadata: Vec::new(),
            offset,
        };
        let before_dot = self.pos;
        self.skip_inline()?;
        if self.eat(b'.') {
            self.skip_inline()?;
            if self.eat_keyword("timeSamples") {
                self.assignment()?;
                attribute.field = AttributeField::TimeSamples(self.time_samples()?);
            } else if self.eat_keyword("connect") {
                self.assignment()?;
                attribute.field = AttributeField::Connect(self.path_list()?);
            } else if self.eat_keyword("spline") {
                self.assignment()?;
                attribute.field = AttributeField::Spline(self.balanced_text(b'{', b'}')?);
            } else {
                return self.fail("`timeSamples`, `connect`, or `spline`");
            }
            if list_op.is_some() && !matches!(attribute.field, AttributeField::Connect(_)) {
                return self.fail("`connect` after a list-op attribute");
            }
            return Ok(attribute);
        }
        self.pos = before_dot;
        if list_op.is_some() {
            return self.fail("`.connect` after a list-op attribute");
        }
        let before_assignment = self.pos;
        self.skip_inline()?;
        if self.eat(b'=') {
            self.skip_inline()?;
            attribute.field = AttributeField::Default(self.attribute_value()?);
        } else {
            self.pos = before_assignment;
        }
        let before_meta = self.pos;
        self.skip_inline()?;
        if self.peek() == Some(b'(') {
            attribute.metadata = self.metadata_block(|parser| parser.property_meta_item(true))?;
        } else {
            self.pos = before_meta;
        }
        Ok(attribute)
    }

    fn attribute_value(&mut self) -> Parsed<AttributeValue> {
        if self.eat_keyword("None") {
            return Ok(AttributeValue::None);
        }
        if self.eat_keyword("AnimationBlock") {
            return Ok(AttributeValue::AnimationBlock);
        }
        if self.at_keyword("edit") {
            let start = self.pos;
            self.advance(4);
            self.skip_inline()?;
            if self.peek() == Some(b'[') {
                let text = self.balanced_text(b'[', b']')?;
                return Ok(AttributeValue::ArrayEdit(format!("edit {text}")));
            }
            self.pos = start;
        }
        Ok(AttributeValue::Value(self.typed_value()?))
    }

    fn time_samples(&mut self) -> Parsed<Vec<TimeSample>> {
        let _depth = self.ctx.enter_nested("usda_time_samples")?;
        self.require(b'{', "`{`")?;
        self.comma_list(b'}', |parser| {
            let time = parser.number()?;
            parser.skip_inline()?;
            parser.require(b':', "`:`")?;
            parser.skip_inline()?;
            let value = parser.attribute_value()?;
            if value == AttributeValue::AnimationBlock {
                return parser.fail("a time-sample value");
            }
            Ok(TimeSample { time, value })
        })
    }

    fn relationship(
        &mut self,
        list_op: Option<ListOp>,
        custom: bool,
        varying: bool,
        offset: usize,
    ) -> Parsed<RelationshipSpec> {
        self.token_separator()?;
        let name = self.property_name()?;
        let mut relationship = RelationshipSpec {
            list_op,
            custom,
            varying,
            name,
            field: RelationshipField::Declared,
            metadata: Vec::new(),
            offset,
        };
        let before = self.pos;
        self.skip_inline()?;
        if list_op.is_none() && self.eat(b'.') {
            self.skip_inline()?;
            self.require_keyword("default")?;
            self.assignment()?;
            relationship.field = RelationshipField::DefaultTarget(self.path_ref()?);
            return Ok(relationship);
        }
        if list_op.is_none() && self.eat(b'[') {
            self.skip_inline()?;
            let target = self.path_ref()?;
            self.skip_inline()?;
            self.require(b']', "`]`")?;
            relationship.field = RelationshipField::BracketTarget(target);
            return Ok(relationship);
        }
        if self.eat(b'=') {
            self.skip_inline()?;
            relationship.field = RelationshipField::Targets(self.path_list()?);
        } else {
            self.pos = before;
        }
        if list_op.is_none() {
            let before_meta = self.pos;
            self.skip_inline()?;
            if self.peek() == Some(b'(') {
                relationship.metadata =
                    self.metadata_block(|parser| parser.property_meta_item(false))?;
            } else {
                self.pos = before_meta;
            }
        }
        Ok(relationship)
    }
}

/// The composition arcs of §4.4 that carry their own value grammar.
#[derive(Clone, Copy)]
enum Arc {
    References,
    Payload,
    Inherits,
    Specializes,
    VariantSets,
}

/// Replaces the escapes of §2.7 in raw string content.
fn unescape(raw: &str) -> Option<String> {
    if !raw.contains('\\') {
        return Some(raw.to_owned());
    }
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        index += 1;
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let Some(&escape) = bytes.get(index) else {
            break;
        };
        index += 1;
        match escape {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'x' => {
                let mut value = 0_u16;
                for _ in 0..2 {
                    match bytes.get(index).and_then(|digit| hex_digit(*digit)) {
                        Some(digit) => {
                            value = value * 16 + u16::from(digit);
                            index += 1;
                        }
                        None => break,
                    }
                }
                out.push(u8::try_from(value).ok()?);
            }
            b'0'..=b'7' => {
                let mut value = 0_u16;
                index -= 1;
                for _ in 0..3 {
                    match bytes.get(index) {
                        Some(digit @ b'0'..=b'7') => {
                            value = value * 8 + u16::from(digit - b'0');
                            index += 1;
                        }
                        _ => break,
                    }
                }
                out.push(u8::try_from(value % 256).ok()?);
            }
            other => out.push(other),
        }
    }
    String::from_utf8(out).ok()
}

/// The occurrences of `needle` in `haystack`.
fn memchr_count(haystack: &[u8], needle: u8) -> usize {
    haystack.split(|byte| *byte == needle).count() - 1
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

/// The shape of a list item that packs: a number, a tuple of numbers, or a
/// tuple of equal tuples of numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemShape {
    Scalar,
    Tuple(usize),
    Matrix(usize, usize),
}

impl ItemShape {
    const fn width(self) -> usize {
        match self {
            Self::Scalar => 1,
            Self::Tuple(count) => count,
            Self::Matrix(rows, columns) => rows * columns,
        }
    }
}

/// The shape and numbers of an item that packs.
fn packable(value: &Value) -> Option<(ItemShape, Vec<Number>)> {
    let numbers = |items: &[Value]| -> Option<Vec<Number>> {
        items
            .iter()
            .map(|item| match item {
                Value::Number(number) => Some(*number),
                _ => None,
            })
            .collect()
    };
    match value {
        Value::Number(number) => Some((ItemShape::Scalar, vec![*number])),
        Value::Tuple(items) => {
            if let Some(numbers) = numbers(items) {
                return Some((ItemShape::Tuple(items.len()), numbers));
            }
            let mut out = Vec::new();
            let mut columns = None;
            for row in items {
                let Value::Tuple(row) = row else {
                    return None;
                };
                if *columns.get_or_insert(row.len()) != row.len() {
                    return None;
                }
                out.extend(numbers(row)?);
            }
            Some((ItemShape::Matrix(items.len(), columns?), out))
        }
        _ => None,
    }
}

/// The items of a list, packed while every item is a number or a tuple of
/// numbers of one shape. While every number is an integer literal, both
/// its exact integer and its binary64 value are kept.
#[derive(Default)]
struct Packer {
    shape: Option<ItemShape>,
    floats: Vec<f64>,
    integers: Option<Vec<i128>>,
    items: Option<Vec<Value>>,
}

impl Packer {
    fn push(&mut self, value: Value) {
        if let Some(items) = &mut self.items {
            items.push(value);
            return;
        }
        let packs =
            packable(&value).filter(|(shape, _)| self.shape.is_none_or(|known| known == *shape));
        let Some((shape, numbers)) = packs else {
            let mut items = self.unpacked();
            items.push(value);
            self.items = Some(items);
            return;
        };
        if self.shape.is_none() {
            self.integers = Some(Vec::new());
        }
        self.shape = Some(shape);
        for number in numbers {
            self.floats.push(number.float);
            match (number.integer, &mut self.integers) {
                (Some(integer), Some(integers)) => integers.push(integer),
                _ => self.integers = None,
            }
        }
    }

    /// The items pushed so far as values.
    fn unpacked(&mut self) -> Vec<Value> {
        let Some(shape) = self.shape else {
            return Vec::new();
        };
        let integers = self.integers.take();
        let numbers: Vec<Number> = self
            .floats
            .drain(..)
            .enumerate()
            .map(|(index, float)| Number {
                float,
                integer: integers
                    .as_ref()
                    .and_then(|integers| integers.get(index).copied()),
            })
            .collect();
        let tuple = |numbers: &[Number]| {
            Value::Tuple(
                numbers
                    .iter()
                    .map(|number| Value::Number(*number))
                    .collect(),
            )
        };
        numbers
            .chunks(shape.width().max(1))
            .map(|chunk| match shape {
                ItemShape::Scalar => Value::Number(chunk[0]),
                ItemShape::Tuple(_) => tuple(chunk),
                ItemShape::Matrix(_, columns) => {
                    Value::Tuple(chunk.chunks(columns.max(1)).map(tuple).collect())
                }
            })
            .collect()
    }

    fn finish(self) -> Value {
        if let Some(items) = self.items {
            return Value::List(items);
        }
        let Some(shape) = self.shape else {
            return Value::List(Vec::new());
        };
        let numbers = match self.integers {
            Some(integers) => Numbers::Literals {
                integers,
                floats: self.floats,
            },
            None => Numbers::Reals(self.floats),
        };
        Value::Packed(Packed {
            numbers,
            width: shape.width(),
        })
    }
}
