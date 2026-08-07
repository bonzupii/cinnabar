//! Cinnabar lexer.
//!
//! Small functions that turn source text into tokens in the node arena.
//! Comments: `#` and `#!` run to end of line; `#| ... |#` and
//! `#!| ... |#` are block comments, and block comments may not nest.
//! Literals are decimal integers and `0x` hexadecimals.

use crate::ast::*;

/// Scans `source` into the token arena.  Returns false when the source is
/// not lexically well formed; every failure is reported in `errors`.
pub fn lex(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    source: &str,
    file: i64,
    errors: &mut Vec<Diag>,
) -> bool {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut pos = 0usize;
    while pos < len {
        pos = lex_byte(names, nodes, bytes, pos, source, file, errors);
    }
    push_token(nodes, TOK_EOF, NONE, NONE, len as i64, len as i64, file);
    errors.is_empty()
}

/// Scans one byte at `pos`, dispatching to the scanner for its class.
fn lex_byte(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    bytes: &[u8],
    pos: usize,
    source: &str,
    file: i64,
    errors: &mut Vec<Diag>,
) -> usize {
    let byte = byte_at(bytes, pos);
    if is_space(byte) {
        pos + 1
    } else if byte == b'\n' {
        push_newline(nodes, pos, file);
        pos + 1
    } else if byte == b'#' {
        lex_comment(bytes, pos, file, errors)
    } else if is_ident_start(byte) {
        lex_ident(names, nodes, bytes, pos, file, source, errors)
    } else if is_digit(byte) {
        lex_number(nodes, bytes, pos, file, errors)
    } else {
        lex_symbol(names, nodes, bytes, pos, file, errors)
    }
}

/// Reads a byte at `pos`, or 0 past the end of the slice.  There is no
/// indexing anywhere in this module.
fn byte_at(bytes: &[u8], pos: usize) -> u8 {
    match bytes.get(pos) {
        Some(byte) => *byte,
        None => 0,
    }
}

fn is_space(byte: u8) -> bool {
    byte == b' ' || byte == b'\t' || byte == b'\r'
}

fn is_letter(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_uppercase()
}

fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

fn is_ident_start(byte: u8) -> bool {
    is_letter(byte) || byte == b'_'
}

fn is_ident_char(byte: u8) -> bool {
    is_letter(byte) || is_digit(byte) || byte == b'_'
}

fn is_hex_digit(byte: u8) -> bool {
    is_digit(byte) || (b'a'..=b'f').contains(&byte) || (b'A'..=b'F').contains(&byte)
}

fn hex_digit(byte: u8) -> Option<u64> {
    if byte.is_ascii_digit() {
        Some((byte - b'0') as u64)
    } else if (b'a'..=b'f').contains(&byte) {
        Some((byte - b'a' + 10) as u64)
    } else if (b'A'..=b'F').contains(&byte) {
        Some((byte - b'A' + 10) as u64)
    } else {
        None
    }
}

fn push_token(
    nodes: &mut Vec<i64>,
    kind: i64,
    name: i64,
    value: i64,
    start: i64,
    end: i64,
    file: i64,
) {
    alloc_node(nodes, &[NODE_TOKEN, file, start, end, kind, name, value, NONE, NONE, NONE]);
}

fn push_newline(nodes: &mut Vec<i64>, pos: usize, file: i64) {
    push_token(nodes, TOK_NL, NONE, NONE, pos as i64, pos as i64 + 1, file);
}

/// Interning an operator symbol and emitting its token; returns the end
/// position for the caller.
fn push_symbol(names: &mut Vec<String>, nodes: &mut Vec<i64>, text: &str, start: usize, end: usize, file: i64) -> usize {
    let name = intern(names, text);
    push_token(nodes, TOK_SYM, name, NONE, start as i64, end as i64, file);
    end
}

// ---------------------------------------------------------------------------
// Comments.
// ---------------------------------------------------------------------------

fn is_doc_comment(bytes: &[u8], pos: usize) -> bool {
    byte_at(bytes, pos + 1) == b'!'
}

/// True when the bytes at `pos` start a block comment: `#|` or `#!|`.
/// The `#` must be at `pos` itself, so scanning inside a comment never
/// mistakes the byte before a `|#` close (or any other `|`) for an
/// opener.
fn is_block_opener(bytes: &[u8], pos: usize) -> bool {
    if byte_at(bytes, pos) != b'#' {
        return false;
    }
    if byte_at(bytes, pos + 1) == b'|' {
        return true;
    }
    byte_at(bytes, pos + 1) == b'!' && byte_at(bytes, pos + 2) == b'|'
}

fn comment_body_start(bytes: &[u8], pos: usize) -> usize {
    if byte_at(bytes, pos + 1) == b'!' {
        pos + 3
    } else {
        pos + 2
    }
}

/// Scans a comment starting at `pos` (which points at `#`).
fn lex_comment(bytes: &[u8], pos: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    if is_block_opener(bytes, pos) {
        let body = comment_body_start(bytes, pos);
        lex_block_comment(bytes, body, pos, file, errors)
    } else if is_doc_comment(bytes, pos) {
        lex_line_comment(bytes, pos + 2)
    } else {
        lex_line_comment(bytes, pos + 1)
    }
}

fn lex_line_comment(bytes: &[u8], mut pos: usize) -> usize {
    while byte_at(bytes, pos) != b'\n' && byte_at(bytes, pos) != 0 {
        pos += 1;
    }
    pos
}

fn lex_block_comment(bytes: &[u8], mut pos: usize, start: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    while byte_at(bytes, pos) != 0 {
        if is_block_opener(bytes, pos) {
            report_nested_comment(file, pos, errors);
            pos += 1;
            continue;
        }
        if is_block_close(bytes, pos) {
            return pos + 2;
        }
        pos += 1;
    }
    report_unterminated_comment(file, start, errors);
    pos
}

fn is_block_close(bytes: &[u8], pos: usize) -> bool {
    byte_at(bytes, pos) == b'|' && byte_at(bytes, pos + 1) == b'#'
}

fn report_nested_comment(file: i64, pos: usize, errors: &mut Vec<Diag>) {
    push_error(errors, "nested block comment is not allowed", file, pos as i64, pos as i64 + 2);
}

fn report_unterminated_comment(file: i64, start: usize, errors: &mut Vec<Diag>) {
    push_error(errors, "unterminated block comment", file, start as i64, start as i64 + 2);
}

// ---------------------------------------------------------------------------
// Identifiers.
// ---------------------------------------------------------------------------

fn lex_ident(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    bytes: &[u8],
    pos: usize,
    file: i64,
    source: &str,
    errors: &mut Vec<Diag>,
) -> usize {
    let end = scan_ident_end(bytes, pos);
    match slice_text(source, pos, end, errors) {
        Some(text) => {
            let name = intern(names, text);
            push_token(nodes, TOK_IDENT, name, NONE, pos as i64, end as i64, file);
            end
        }
        None => end,
    }
}

fn scan_ident_end(bytes: &[u8], mut end: usize) -> usize {
    while is_ident_char(byte_at(bytes, end)) {
        end += 1;
    }
    end
}

fn slice_text<'a>(source: &'a str, start: usize, end: usize, errors: &mut Vec<Diag>) -> Option<&'a str> {
    match source.get(start..end) {
        Some(text) => Some(text),
        None => {
            push_internal(errors, "slice out of range");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Numbers.
// ---------------------------------------------------------------------------

fn lex_number(nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    if byte_at(bytes, pos) == b'0' && byte_at(bytes, pos + 1) == b'x' {
        lex_hex(nodes, bytes, pos, file, errors)
    } else {
        lex_decimal(nodes, bytes, pos, file, errors)
    }
}

fn lex_decimal(nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    let start = pos;
    let end = scan_digits(bytes, pos);
    if let Some(value) = decimal_value(bytes, start, end, file, errors) {
        if is_ident_char(byte_at(bytes, end)) {
            push_error(errors, "invalid character in integer literal", file, start as i64, end as i64);
        } else {
            push_token(nodes, TOK_INT, NONE, value, start as i64, end as i64, file);
        }
    }
    end
}

fn scan_digits(bytes: &[u8], mut end: usize) -> usize {
    while is_digit(byte_at(bytes, end)) {
        end += 1;
    }
    end
}

fn decimal_value(bytes: &[u8], start: usize, end: usize, file: i64, errors: &mut Vec<Diag>) -> Option<i64> {
    let mut value: u64 = 0;
    let mut pos = start;
    while pos < end {
        let digit = (byte_at(bytes, pos) - b'0') as u64;
        {
            let next = accumulate_digit(value, digit, 10, "integer literal is too large", (file, start as i64, pos as i64), errors)?;
            value = next
        }
        pos += 1;
    }
    if value > i64::MAX as u64 {
        push_error(errors, "integer literal is too large", file, start as i64, end as i64);
        return None;
    }
    Some(value as i64)
}

fn lex_hex(nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    let start = pos;
    let end = scan_hex_digits(bytes, pos + 2);
    if end == start + 2 {
        push_error(errors, "expected hexadecimal digits after 0x", file, start as i64, end as i64);
        return end;
    }
    if is_ident_char(byte_at(bytes, end)) {
        push_error(errors, "invalid digit in hexadecimal literal", file, start as i64, end as i64);
        return end;
    }
    if let Some(value) = hex_value(bytes, start + 2, end, file, errors) { push_token(nodes, TOK_HEX, NONE, value, start as i64, end as i64, file) }
    end
}

fn scan_hex_digits(bytes: &[u8], mut end: usize) -> usize {
    while is_hex_digit(byte_at(bytes, end)) {
        end += 1;
    }
    end
}

fn hex_value(bytes: &[u8], start: usize, end: usize, file: i64, errors: &mut Vec<Diag>) -> Option<i64> {
    let mut value: u64 = 0;
    let mut pos = start;
    while pos < end {
        match hex_digit(byte_at(bytes, pos)) {
            Some(digit) => {
                {
                    let next = accumulate_digit(value, digit, 16, "hexadecimal literal is too large", (file, start as i64, pos as i64), errors)?;
                    value = next
                }
            }
            None => {
                push_internal(errors, "hex digit invariant broken");
                return None;
            }
        }
        pos += 1;
    }
    Some(value as i64)
}

/// Multiplies `value` by `radix` and adds `digit`, reporting an overflow
/// as a diagnostic.
fn accumulate_digit(
    value: u64,
    digit: u64,
    radix: u64,
    message: &str,
    span: (i64, i64, i64),
    errors: &mut Vec<Diag>,
) -> Option<u64> {
    let (file, start, end) = span;
    match value.checked_mul(radix).and_then(|v| v.checked_add(digit)) {
        Some(next) => Some(next),
        None => {
            push_error(errors, message, file, start, end);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Symbols.
// ---------------------------------------------------------------------------

fn lex_symbol(names: &mut Vec<String>, nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    let byte = byte_at(bytes, pos);
    let next = byte_at(bytes, pos + 1);
    match two_char_symbol(byte, next) {
        Some(text) => push_symbol(names, nodes, text, pos, pos + 2, file),
        None => lex_one_char_symbol(names, nodes, bytes, pos, file, errors),
    }
}

fn lex_one_char_symbol(names: &mut Vec<String>, nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    match one_char_symbol(byte_at(bytes, pos)) {
        Some(text) => push_symbol(names, nodes, text, pos, pos + 1, file),
        None => report_unexpected(file, pos, errors),
    }
}

fn report_unexpected(file: i64, pos: usize, errors: &mut Vec<Diag>) -> usize {
    push_error(errors, "unexpected character", file, pos as i64, pos as i64 + 1);
    pos + 1
}

fn two_char_symbol(byte: u8, next: u8) -> Option<&'static str> {
    if byte == b'<' {
        less_symbol(next)
    } else if byte == b'>' {
        greater_symbol(next)
    } else if byte == b'=' {
        equal_symbol(next)
    } else if byte == b'!' {
        not_symbol(next)
    } else if byte == b'&' {
        and_symbol(next)
    } else if byte == b'|' {
        or_symbol(next)
    } else if byte == b'.' {
        dot_symbol(next)
    } else {
        None
    }
}

fn less_symbol(next: u8) -> Option<&'static str> {
    if next == b'<' {
        Some("<<")
    } else if next == b'=' {
        Some("<=")
    } else {
        None
    }
}

fn greater_symbol(next: u8) -> Option<&'static str> {
    if next == b'>' {
        Some(">>")
    } else if next == b'=' {
        Some(">=")
    } else {
        None
    }
}

fn equal_symbol(next: u8) -> Option<&'static str> {
    if next == b'=' {
        Some("==")
    } else if next == b'>' {
        Some("=>")
    } else {
        None
    }
}

fn not_symbol(next: u8) -> Option<&'static str> {
    if next == b'=' {
        Some("!=")
    } else {
        None
    }
}

fn and_symbol(next: u8) -> Option<&'static str> {
    if next == b'&' {
        Some("&&")
    } else {
        None
    }
}

fn or_symbol(next: u8) -> Option<&'static str> {
    if next == b'|' {
        Some("||")
    } else {
        None
    }
}

fn dot_symbol(next: u8) -> Option<&'static str> {
    if next == b'.' {
        Some("..")
    } else {
        None
    }
}

fn one_char_symbol(byte: u8) -> Option<&'static str> {
    math_symbol(byte)
        .or(bit_symbol(byte))
        .or(group_symbol(byte))
        .or(punct_symbol(byte))
        .or(relational_symbol(byte))
}

fn math_symbol(byte: u8) -> Option<&'static str> {
    if byte == b'+' {
        Some("+")
    } else if byte == b'-' {
        Some("-")
    } else if byte == b'*' {
        Some("*")
    } else if byte == b'/' {
        Some("/")
    } else {
        None
    }
}

fn bit_symbol(byte: u8) -> Option<&'static str> {
    if byte == b'^' {
        Some("^")
    } else if byte == b'|' {
        Some("|")
    } else if byte == b'&' {
        Some("&")
    } else {
        None
    }
}

fn group_symbol(byte: u8) -> Option<&'static str> {
    if byte == b'(' {
        Some("(")
    } else if byte == b')' {
        Some(")")
    } else if byte == b'[' {
        Some("[")
    } else if byte == b']' {
        Some("]")
    } else {
        None
    }
}

fn punct_symbol(byte: u8) -> Option<&'static str> {
    if byte == b',' {
        Some(",")
    } else if byte == b':' {
        Some(":")
    } else if byte == b'.' {
        Some(".")
    } else if byte == b'@' {
        Some("@")
    } else if byte == b';' {
        Some(";")
    } else {
        None
    }
}

fn relational_symbol(byte: u8) -> Option<&'static str> {
    if byte == b'<' {
        Some("<")
    } else if byte == b'>' {
        Some(">")
    } else if byte == b'=' {
        Some("=")
    } else if byte == b'!' {
        Some("!")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_all(source: &str) -> (Vec<i64>, Vec<Diag>) {
        let mut names: Vec<String> = Vec::new();
        let mut nodes: Vec<i64> = Vec::new();
        let mut errors: Vec<Diag> = Vec::new();
        let ok = lex(&mut names, &mut nodes, source, 0, &mut errors);
        let mut kinds: Vec<i64> = Vec::new();
        let mut idx = 0i64;
        while idx < nodes.len() as i64 / NODE_STRIDE {
            if node_tag(&nodes, idx) == NODE_TOKEN {
                kinds.push(node_a(&nodes, idx));
            }
            idx += 1;
        }
        assert!(ok || errors.len() > 0);
        (kinds, errors)
    }

    /// Lexes `source` and returns only its diagnostics, for tests that
    /// assert rejection behavior and never need the token stream.
    fn lex_errors(source: &str) -> Vec<Diag> {
        let mut names: Vec<String> = Vec::new();
        let mut nodes: Vec<i64> = Vec::new();
        let mut errors: Vec<Diag> = Vec::new();
        let ok = lex(&mut names, &mut nodes, source, 0, &mut errors);
        assert!(ok || errors.len() > 0);
        errors
    }

    #[test]
    fn lexes_comments_and_literals() {
        // `=` is a symbol token, not an identifier; the block comment
        // produces no tokens at all.
        let (kinds, errors) = lex_all("#| block |#\nval x = 0x1F\n");
        assert_eq!(errors.len(), 0);
        assert_eq!(
            kinds,
            vec![TOK_NL, TOK_IDENT, TOK_IDENT, TOK_SYM, TOK_HEX, TOK_NL, TOK_EOF]
        );
    }

    #[test]
    fn rejects_nested_block_comment() {
        let errors = lex_errors("#| outer #| inner |# |#\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.0.contains("nested")),
            None => assert!(false),
        }
    }

    #[test]
    fn rejects_bad_hex() {
        let errors = lex_errors("0xG123\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.0.contains("hex")),
            None => assert!(false),
        }
    }

    #[test]
    fn rejects_unexpected_char() {
        let errors = lex_errors("$100\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.0.contains("unexpected")),
            None => assert!(false),
        }
    }

    #[test]
    fn rejects_unterminated_block_comment() {
        let errors = lex_errors("#| never closed\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.0.contains("unterminated")),
            None => assert!(false),
        }
    }
}
