//! Byte-level scanning of source text into `NODE_TOKEN` rows.
//!
//! A hand-written scanner that writes tokens straight into the shared
//! arena — there is no separate token type and no intermediate vector of
//! lexemes. It handles the four comment forms (`#`, `#!` doc, `#| |#`
//! block, `#!| |#` block doc), decimal and `0x` integer literals stored as
//! raw bit patterns, double-quoted strings with the five escapes `\n`,
//! `\t`, `\0`, `\"`, `\\`, and the one- and two-character operators.
//!
//! Two decisions here are load-bearing much further down. String literal
//! bytes are interned into the same `names` table identifiers use, so equal
//! literals collapse to a single name id and codegen can emit one `.rodata`
//! global per distinct literal with no side table to keep in step. And
//! unescaped text is copied out as whole `&str` runs rather than byte by
//! byte, so multi-byte characters survive intact.
//!
//! **Invariants:**
//! - Literal accumulation uses `checked_mul`/`checked_add`. An overflowing
//!   literal is a lexical error, never a silently wrapped bit pattern.
//! - Whether a literal fits its type is the typechecker's fact, not this
//!   file's: the lexer does not yet know what type a literal will adopt.
//! - Casing is tokenized, not judged. The resolver enforces the casing
//!   rules, so a mis-cased identifier lexes cleanly and is rejected once,
//!   in the one place that owns that rule.
//! - Block comments do not nest: a nested opener is a hard error, tracked
//!   well enough to still locate the real outer closer.
//! - A string literal does not span lines, so a missing closing quote
//!   reports at the newline rather than consuming the rest of the file.

use crate::ast::*;

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
        lex_comment(names, nodes, bytes, pos, source, file, errors)
    } else if is_ident_start(byte) {
        lex_ident(names, nodes, bytes, pos, file, source, errors)
    } else if is_digit(byte) {
        lex_number(nodes, bytes, pos, file, errors)
    } else if byte == b'"' {
        lex_string(names, nodes, bytes, pos, source, file, errors)
    } else {
        lex_symbol(names, nodes, bytes, pos, source, file, errors)
    }
}

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

fn push_symbol(names: &mut Vec<String>, nodes: &mut Vec<i64>, text: &str, start: usize, end: usize, file: i64) -> usize {
    let name = intern(names, text);
    push_token(nodes, TOK_SYM, name, NONE, start as i64, end as i64, file);
    end
}

fn is_doc_comment(bytes: &[u8], pos: usize) -> bool {
    byte_at(bytes, pos + 1) == b'!'
}

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

fn lex_comment(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    bytes: &[u8],
    pos: usize,
    source: &str,
    file: i64,
    errors: &mut Vec<Diag>,
) -> usize {
    if is_block_opener(bytes, pos) {
        let body = comment_body_start(bytes, pos);
        let end = lex_block_comment(bytes, body, pos, file, errors);
        if is_doc_comment(bytes, pos) && end >= body + 2 {
            push_doc_token(names, nodes, source, body..end - 2, pos..end, file, errors);
        }
        end
    } else if is_doc_comment(bytes, pos) {
        let body = pos + 2;
        let end = lex_line_comment(bytes, body);
        push_doc_token(names, nodes, source, body..end, pos..end, file, errors);
        end
    } else {
        lex_line_comment(bytes, pos + 1)
    }
}

fn push_doc_token(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    source: &str,
    body_range: std::ops::Range<usize>,
    token_range: std::ops::Range<usize>,
    file: i64,
    errors: &mut Vec<Diag>,
) {
    if let Some(body) = slice_text(source, body_range.start, body_range.end, errors) {
        let name = intern(names, body);
        push_token(nodes, TOK_DOC, name, NONE, token_range.start as i64, token_range.end as i64, file);
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
            pos = skip_nested_block(bytes, pos);
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

// Skips past the nested block's matching `|#`; end of input if none exists.
fn skip_nested_block(bytes: &[u8], pos: usize) -> usize {
    let mut scan = pos + 2;
    while byte_at(bytes, scan) != 0 && !is_block_close(bytes, scan) {
        scan += 1;
    }
    if byte_at(bytes, scan) == 0 {
        scan
    } else {
        scan + 2
    }
}

fn is_block_close(bytes: &[u8], pos: usize) -> bool {
    byte_at(bytes, pos) == b'|' && byte_at(bytes, pos + 1) == b'#'
}

fn report_nested_comment(file: i64, pos: usize, errors: &mut Vec<Diag>) {
    push_syntax(errors, "nested block comment is not allowed", file, pos as i64, pos as i64 + 2);
}

fn report_unterminated_comment(file: i64, start: usize, errors: &mut Vec<Diag>) {
    push_syntax(errors, "unterminated block comment", file, start as i64, start as i64 + 2);
}

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
            // Leading underscore marks a discard; Cinnabar has no discards.
            if text.starts_with('_') {
                let message = if text == "_" {
                    "discard pattern '_' is not allowed; bind the value with a real name and use it, or split the match arm so each variant has its own".to_string()
                } else {
                    format!("'{}' begins with an underscore, which marks a value as deliberately unused; bind it with a real name and use it", text)
                };
                push_syntax(errors, &message, file, pos as i64, end as i64);
                return end;
            }
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
            push_syntax(errors, "invalid character in integer literal", file, start as i64, end as i64 + 1);
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
            let next = accumulate_digit(value, digit, 10, "integer literal is too large", (file, start as i64, pos as i64 + 1), errors)?;
            value = next
        }
        pos += 1;
    }
    Some(value as i64)
}

fn lex_hex(nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    let start = pos;
    let end = scan_hex_digits(bytes, pos + 2);
    if end == start + 2 {
        push_syntax(errors, "expected hexadecimal digits after 0x", file, start as i64, end as i64);
        return end;
    }
    if is_ident_char(byte_at(bytes, end)) {
        push_syntax(errors, "invalid digit in hexadecimal literal", file, start as i64, end as i64 + 1);
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
                    let next = accumulate_digit(value, digit, 16, "hexadecimal literal is too large", (file, start as i64, pos as i64 + 1), errors)?;
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
            push_syntax(errors, message, file, start, end);
            None
        }
    }
}

// Scans and interns decoded string bytes; only five escapes accepted.
fn lex_string(names: &mut Vec<String>, nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, source: &str, file: i64, errors: &mut Vec<Diag>) -> usize {
    let start = pos;
    let mut cursor = pos + 1;
    // Runs are copied as whole `&str` slices so multi-byte characters survive.
    let mut run = cursor;
    let mut text = String::new();
    while cursor < bytes.len() {
        let byte = byte_at(bytes, cursor);
        if byte == b'"' {
            if !append_run(&mut text, source, run, cursor, errors) {
                return cursor + 1;
            }
            let end = cursor + 1;
            let name = intern(names, &text);
            push_token(nodes, TOK_STRING, name, NONE, start as i64, end as i64, file);
            return end;
        }
        if byte == b'\n' {
            push_syntax(errors, "unterminated string literal", file, start as i64, cursor as i64);
            return cursor;
        }
        if byte == b'\\' {
            if !append_run(&mut text, source, run, cursor, errors) {
                return cursor + 1;
            }
            // A lone backslash at end of file or before a newline consumes
            // only itself; the newline/EOL arms above report the literal.
            let body = byte_at(bytes, cursor + 1);
            if cursor + 1 >= bytes.len() || body == b'\n' {
                push_syntax(errors, "incomplete escape in string literal", file, cursor as i64, (cursor + 1) as i64);
                cursor += 1;
                run = cursor;
                continue;
            }
            match escape_byte(body) {
                Some(decoded) => {
                    // Every defined escape body is ASCII, so the escape is
                    // exactly two bytes wide.
                    text.push(decoded as char);
                    cursor += 2;
                }
                None => {
                    // An unknown escape body is a character: take its real
                    // UTF-8 width so the next run starts on a boundary.
                    let body_width = match source.get(cursor + 1..).and_then(|rest| rest.chars().next()) {
                        Some(character) => character.len_utf8(),
                        None => 1,
                    };
                    let escape_end = cursor + 1 + body_width;
                    push_syntax(errors, "unknown escape in string literal", file, cursor as i64, escape_end as i64);
                    cursor = escape_end;
                }
            }
            run = cursor;
            continue;
        }
        cursor += 1;
    }
    push_syntax(errors, "unterminated string literal", file, start as i64, cursor as i64);
    cursor
}

// Appends the source text of one unescaped run; both offsets fall on
// character boundaries, so `source.get` cannot panic.
fn append_run(text: &mut String, source: &str, from: usize, to: usize, errors: &mut Vec<Diag>) -> bool {
    match source.get(from..to) {
        Some(part) => {
            text.push_str(part);
            true
        }
        None => {
            push_internal(errors, "string literal run does not fall on a character boundary");
            false
        }
    }
}

// The byte a backslash escape denotes, or None when the escape is not one
// the language defines.
fn escape_byte(byte: u8) -> Option<u8> {
    if byte == b'n' {
        Some(b'\n')
    } else if byte == b't' {
        Some(b'\t')
    } else if byte == b'0' {
        Some(0)
    } else if byte == b'"' {
        Some(b'"')
    } else if byte == b'\\' {
        Some(b'\\')
    } else {
        None
    }
}

fn lex_symbol(names: &mut Vec<String>, nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, source: &str, file: i64, errors: &mut Vec<Diag>) -> usize {
    let byte = byte_at(bytes, pos);
    let next = byte_at(bytes, pos + 1);
    match two_char_symbol(byte, next) {
        Some(text) => push_symbol(names, nodes, text, pos, pos + 2, file),
        None => lex_one_char_symbol(names, nodes, bytes, pos, source, file, errors),
    }
}

fn lex_one_char_symbol(names: &mut Vec<String>, nodes: &mut Vec<i64>, bytes: &[u8], pos: usize, source: &str, file: i64, errors: &mut Vec<Diag>) -> usize {
    match one_char_symbol(byte_at(bytes, pos)) {
        Some(text) => push_symbol(names, nodes, text, pos, pos + 1, file),
        None => report_unexpected(source, pos, file, errors),
    }
}

fn char_len_at(source: &str, pos: usize) -> usize {
    match source.get(pos..) {
        Some(rest) => match rest.chars().next() {
            Some(c) => c.len_utf8(),
            None => 1,
        },
        None => 1,
    }
}

fn report_unexpected(source: &str, pos: usize, file: i64, errors: &mut Vec<Diag>) -> usize {
    let width = char_len_at(source, pos);
    push_syntax(errors, "unexpected character", file, pos as i64, (pos + width) as i64);
    pos + width
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
    } else if byte == b'%' {
        Some("%")
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
            Some(diag) => assert!(diag.message.contains("nested")),
            None => assert!(false),
        }
    }

    #[test]
    fn nested_comment_skip_keeps_outer_closer() {
        let (kinds, errors) = lex_all("#| outer #| inner |# val x = 1 |#\nval y = 2\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.message.contains("nested")),
            None => assert!(false),
        }
        // The nested block's `|#` must not terminate the outer comment.
        assert_eq!(
            kinds,
            vec![TOK_NL, TOK_IDENT, TOK_IDENT, TOK_SYM, TOK_INT, TOK_NL, TOK_EOF]
        );
    }

    #[test]
    fn rejects_bad_hex() {
        let errors = lex_errors("0xG123\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.message.contains("hex")),
            None => assert!(false),
        }
    }

    #[test]
    fn rejects_unexpected_char() {
        let errors = lex_errors("$100\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.message.contains("unexpected")),
            None => assert!(false),
        }
    }

    #[test]
    fn rejects_unterminated_block_comment() {
        let errors = lex_errors("#| never closed\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.message.contains("unterminated")),
            None => assert!(false),
        }
    }

    // The decoded bytes a source line's single string literal interns to.
    fn lex_string_bytes(source: &str) -> Vec<u8> {
        let mut names: Vec<String> = Vec::new();
        let mut nodes: Vec<i64> = Vec::new();
        let mut errors: Vec<Diag> = Vec::new();
        let ok = lex(&mut names, &mut nodes, source, 0, &mut errors);
        assert!(ok, "unexpected lexical errors: {:?}", errors);
        let mut idx = 0i64;
        while idx < nodes.len() as i64 / NODE_STRIDE {
            if node_tag(&nodes, idx) == NODE_TOKEN && node_a(&nodes, idx) == TOK_STRING {
                let name = node_b(&nodes, idx);
                return name_text(&names, name).into_bytes();
            }
            idx += 1;
        }
        assert!(false, "no string token in {:?}", source);
        Vec::new()
    }

    #[test]
    fn decodes_every_defined_escape() {
        // The five escapes the language defines, each exactly one byte.
        assert_eq!(lex_string_bytes("\"\\n\\t\\0\\\"\\\\\"\n"), vec![10, 9, 0, 34, 92]);
    }

    #[test]
    fn preserves_multibyte_utf8() {
        // UTF-8 source text is copied through unchanged, byte for byte.
        assert_eq!(
            lex_string_bytes("\"é€𝄞\"\n"),
            vec![0xC3, 0xA9, 0xE2, 0x82, 0xAC, 0xF0, 0x9D, 0x84, 0x9E]
        );
    }

    #[test]
    fn interns_equal_literals_to_one_name() {
        // Equal literals share one name id.
        let mut names: Vec<String> = Vec::new();
        let mut nodes: Vec<i64> = Vec::new();
        let mut errors: Vec<Diag> = Vec::new();
        let ok = lex(&mut names, &mut nodes, "\"same\" \"same\" \"other\"\n", 0, &mut errors);
        assert!(ok, "unexpected lexical errors: {:?}", errors);
        let mut ids: Vec<i64> = Vec::new();
        let mut idx = 0i64;
        while idx < nodes.len() as i64 / NODE_STRIDE {
            if node_tag(&nodes, idx) == NODE_TOKEN && node_a(&nodes, idx) == TOK_STRING {
                ids.push(node_b(&nodes, idx));
            }
            idx += 1;
        }
        assert_eq!(ids.len(), 3);
        match (ids.first(), ids.get(1), ids.get(2)) {
            (Some(first), Some(second), Some(third)) => {
                assert_eq!(first, second, "equal literals must intern to one name");
                assert!(first != third, "different literals must intern separately");
            }
            (_first, _second, _third) => assert!(false, "expected three string tokens"),
        }
    }

    #[test]
    fn empty_literal_is_zero_bytes() {
        assert_eq!(lex_string_bytes("\"\"\n"), Vec::<u8>::new());
    }

    #[test]
    fn rejects_unknown_escape() {
        let errors = lex_errors("val x = \"bad \\q escape\"\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.message.contains("unknown escape")),
            None => assert!(false),
        }
    }

    #[test]
    fn rejects_string_spanning_a_line() {
        // A literal that runs off its line reports once and stops at the
        // newline, so a missing quote cannot swallow the rest of the file.
        let errors = lex_errors("val x = \"no closing quote\nval y = 1\n");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.message.contains("unterminated string")),
            None => assert!(false),
        }
    }

    #[test]
    fn rejects_unterminated_string_at_eof() {
        let errors = lex_errors("val x = \"runs to the end");
        assert_eq!(errors.len(), 1);
        match errors.get(0) {
            Some(diag) => assert!(diag.message.contains("unterminated string")),
            None => assert!(false),
        }
    }

    // Every error span must address bytes the source actually has.
    fn assert_spans_addressable(source: &str, errors: &[Diag]) {
        let mut idx = 0usize;
        while idx < errors.len() {
            match errors.get(idx) {
                Some(diag) => {
                    assert!(
                        diag.start >= 0 && diag.end >= diag.start && diag.end <= source.len() as i64,
                        "span {}..{} of '{}' leaves a {}-byte source",
                        diag.start,
                        diag.end,
                        diag.message,
                        source.len()
                    );
                }
                None => break,
            }
            idx += 1;
        }
    }

    #[test]
    fn trailing_backslash_at_eof_stays_inside_the_source() {
        // A trailing backslash at end of file must stay inside the source.
        let source = "val x = \"runs off\\";
        let errors = lex_errors(source);
        assert!(errors.len() > 0, "a literal ending in a backslash must be rejected");
        assert_spans_addressable(source, &errors);
    }

    #[test]
    fn an_undefined_escape_over_a_multibyte_body_reports_the_typo() {
        // A multibyte escape body is spanned by whole characters.
        let source = "val x = \"a\\éb\"\n";
        let errors = lex_errors(source);
        assert_eq!(errors.len(), 1, "expected exactly the typo: {:?}", errors);
        match errors.first() {
            Some(diag) => {
                assert!(
                    diag.message.contains("unknown escape"),
                    "reported something other than the escape: {}",
                    diag.message
                );
                assert!(
                    !diag.message.contains("character boundary"),
                    "a user typo produced an internal invariant diagnostic: {}",
                    diag.message
                );
            }
            None => assert!(false, "no diagnostic reported"),
        }
        assert_spans_addressable(source, &errors);
    }

    #[test]
    fn backslash_before_newline_does_not_swallow_the_next_line() {
        // A newline is never an escape body; it bounds the literal.
        let source = "val x = \"open\\\nval y = 1\n";
        let mut names: Vec<String> = Vec::new();
        let mut nodes: Vec<i64> = Vec::new();
        let mut errors: Vec<Diag> = Vec::new();
        let ok = lex(&mut names, &mut nodes, source, 0, &mut errors);
        assert!(!ok, "an unterminated literal must be rejected");
        assert_spans_addressable(source, &errors);
        let mut saw_following_binding = false;
        let mut idx = 0i64;
        while idx < nodes.len() as i64 / NODE_STRIDE {
            if tok_is_name(&nodes, &names, idx, "y") {
                saw_following_binding = true;
            }
            idx += 1;
        }
        assert!(saw_following_binding, "the line after the literal must still be lexed");
    }
}
