//! Canonical source formatting for Cinnabar.
//!
//! Indentation and blank lines carry no meaning in Cinnabar, which is what
//! makes a formatter safe to write at this level: it recognizes only the
//! structural syntax needed to choose an indentation level, and leaves
//! tokens and comment contents exactly as written.
//!
//! **Invariants:**
//! - Formatting is not a semantic pass: it resolves no name and infers no
//!   type, deciding only the whitespace between tokens.
//! - Token text and comment bodies survive a round trip unchanged.

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Regular,
    Conditional,
    Match,
    MatchArm,
    Trait,
}

#[derive(Clone, Copy)]
struct PendingBlock {
    kind: BlockKind,
    delimiter_base: i64,
}

fn words(code: &str) -> Vec<&str> {
    code.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|word| !word.is_empty())
        .collect()
}

fn has_word(items: &[&str], expected: &str) -> bool {
    items.contains(&expected)
}

fn top_is(stack: &[BlockKind], expected: BlockKind) -> bool {
    stack.last().is_some_and(|kind| *kind == expected)
}

fn opening_kind(code: &str, stack: &[BlockKind]) -> Option<BlockKind> {
    let items = words(code);
    if has_word(&items, "match") {
        return Some(BlockKind::Match);
    }
    let first = {
        let word = items.first()?;
        *word
    };
    if first == "if" {
        return Some(BlockKind::Conditional);
    }
    if first == "while" || first == "mod" || first == "impl" {
        return Some(BlockKind::Regular);
    }
    if first == "trait" {
        return Some(BlockKind::Trait);
    }
    let is_native = has_word(&items, "nat");
    if has_word(&items, "trait") {
        return Some(BlockKind::Trait);
    }
    if has_word(&items, "mod") || has_word(&items, "impl") {
        return Some(BlockKind::Regular);
    }
    if has_word(&items, "type") && !is_native {
        return Some(BlockKind::Regular);
    }
    if has_word(&items, "fun") && !is_native && !top_is(stack, BlockKind::Trait) {
        return Some(BlockKind::Regular);
    }
    None
}

fn starts_with_word(code: &str, expected: &str) -> bool {
    words(code).first().is_some_and(|word| *word == expected)
}

fn match_arm_has_multiline_body(code: &str) -> bool {
    match code.find("=>") {
        Some(position) => code
            .get(position + 2..)
            .is_some_and(|body| body.trim().is_empty()),
        None => false,
    }
}

// Index past closing quote of literal at `open`.
fn skip_string_literal(bytes: &[u8], open: usize) -> usize {
    let mut idx = open + 1;
    while idx < bytes.len() {
        let byte = match bytes.get(idx) {
            Some(value) => *value,
            None => return bytes.len(),
        };
        if byte == b'\\' {
            idx += 2;
            continue;
        }
        if byte == b'"' {
            return idx + 1;
        }
        idx += 1;
    }
    bytes.len()
}

fn comment_free_code(line: &str, block_comment: &mut bool) -> String {
    let bytes = line.as_bytes();
    let mut code = String::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if *block_comment {
            let closes = bytes.get(idx).is_some_and(|byte| *byte == b'|')
                && bytes.get(idx + 1).is_some_and(|byte| *byte == b'#');
            if closes {
                *block_comment = false;
                idx += 2;
            } else {
                idx += 1;
            }
        } else {
            let starts_doc_block = bytes.get(idx).is_some_and(|byte| *byte == b'#')
                && bytes.get(idx + 1).is_some_and(|byte| *byte == b'!')
                && bytes.get(idx + 2).is_some_and(|byte| *byte == b'|');
            let starts_block = bytes.get(idx).is_some_and(|byte| *byte == b'#')
                && bytes.get(idx + 1).is_some_and(|byte| *byte == b'|');
            if starts_doc_block {
                *block_comment = true;
                idx += 3;
            } else if starts_block {
                *block_comment = true;
                idx += 2;
            } else if bytes.get(idx).is_some_and(|byte| *byte == b'#') {
                break;
            } else if bytes.get(idx).is_some_and(|byte| *byte == b'"') {
                // Literal contents are not code: skip them, emit an empty pair.
                idx = skip_string_literal(bytes, idx);
                code.push_str("\"\"");
            } else {
                match bytes.get(idx) {
                    Some(byte) => code.push(*byte as char),
                    None => break,
                }
                idx += 1;
            }
        }
    }
    code
}

fn delimiter_change(code: &str) -> i64 {
    let mut change = 0i64;
    for ch in code.chars() {
        if ch == '(' || ch == '[' || ch == '{' {
            change += 1;
        } else if ch == ')' || ch == ']' || ch == '}' {
            change -= 1;
        }
    }
    change
}

fn leading_closers(code: &str) -> i64 {
    let mut count = 0i64;
    for ch in code.trim_start().chars() {
        if ch == ')' || ch == ']' || ch == '}' {
            count += 1;
        } else {
            break;
        }
    }
    count
}

fn write_indented(out: &mut String, indent: i64, text: &str) {
    let mut level = 0i64;
    while level < indent {
        out.push_str("  ");
        level += 1;
    }
    out.push_str(text);
    out.push('\n');
}

/// Format source using Cinnabar's single canonical whitespace style.
/// Tokens and comment contents are preserved; the result always ends in one
/// newline and formatting the result again is a no-op.
pub fn format_source(source: &str) -> String {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::new();
    let mut stack: Vec<BlockKind> = Vec::new();
    let mut delimiter_depth = 0i64;
    let mut pending: Option<PendingBlock> = None;
    let mut in_block_comment = false;
    let mut previous_blank = true;

    for raw_line in normalized.lines() {
        let text = raw_line.trim_end();
        if text.trim().is_empty() {
            if in_block_comment {
                out.push_str(text);
                out.push('\n');
                previous_blank = false;
            } else if !previous_blank {
                out.push('\n');
                previous_blank = true;
            }
            continue;
        }

        let started_in_block_comment = in_block_comment;
        let mut line_block_comment = in_block_comment;
        let code = comment_free_code(text, &mut line_block_comment);
        let trimmed_code = code.trim();
        if started_in_block_comment && trimmed_code.is_empty() {
            out.push_str(text);
            out.push('\n');
            in_block_comment = line_block_comment;
            previous_blank = false;
            continue;
        }
        let is_end = starts_with_word(trimmed_code, "end");
        let is_branch = starts_with_word(trimmed_code, "elif")
            || starts_with_word(trimmed_code, "else");
        let is_arm = trimmed_code.contains("=>")
            && (top_is(&stack, BlockKind::Match) || top_is(&stack, BlockKind::MatchArm));

        if top_is(&stack, BlockKind::MatchArm) && (is_arm || is_end) {
            stack.pop();
        }
        if is_end {
            stack.pop();
        }

        let branch_dedent = if is_branch && top_is(&stack, BlockKind::Conditional) {
            1i64
        } else {
            0i64
        };
        let closer_dedent = leading_closers(trimmed_code).min(delimiter_depth);
        let structural_indent = stack.len() as i64 - branch_dedent;
        let indent = (structural_indent + delimiter_depth - closer_dedent).max(0);
        write_indented(&mut out, indent, text.trim_start());
        previous_blank = false;

        delimiter_depth = (delimiter_depth + delimiter_change(trimmed_code)).max(0);
        if let Some(waiting) = pending
            && delimiter_depth <= waiting.delimiter_base
        {
            stack.push(waiting.kind);
            pending = None;
        }

        if is_arm && match_arm_has_multiline_body(trimmed_code) {
            stack.push(BlockKind::MatchArm);
        } else if !is_end && !is_branch && pending.is_none()
            && let Some(kind) = opening_kind(trimmed_code, &stack)
        {
            let change = delimiter_change(trimmed_code);
            if change > 0 {
                pending = Some(PendingBlock {
                    kind,
                    delimiter_base: delimiter_depth - change,
                });
            } else {
                stack.push(kind);
            }
        }

        in_block_comment = line_block_comment;
    }

    while out.ends_with("\n\n") {
        out.pop();
    }
    if out.is_empty() || !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_source;

    #[test]
    fn formats_nested_control_flow_and_match_arms() {
        let source = "fun choose(flag: Bool) I64\nif flag\nmatch flag\ntrue =>\nreturn 1\nfalse => return 0\nend\nelse\nreturn 2\nend\nend\n";
        let expected = "fun choose(flag: Bool) I64\n  if flag\n    match flag\n      true =>\n        return 1\n      false => return 0\n    end\n  else\n    return 2\n  end\nend\n";
        assert_eq!(format_source(source), expected);
        assert_eq!(format_source(expected), expected);
    }

    #[test]
    fn preserves_block_comment_contents_and_formats_multiline_declarations() {
        let source = "pub mod Demo\npub nat fun call(\nvalue: I64\n) I64\n#!|\n  preserved indentation\n|#\npub trait Check\npub fun check(value: &Self) Bool\nend\nend\n";
        let expected = "pub mod Demo\n  pub nat fun call(\n    value: I64\n  ) I64\n  #!|\n  preserved indentation\n|#\n  pub trait Check\n    pub fun check(value: &Self) Bool\n  end\nend\n";
        assert_eq!(format_source(source), expected);
    }

    #[test]
    fn ignores_structure_words_inside_trailing_multiline_comments() {
        let source = "fun main() I64 #| comment starts here\nend and match are comment text\n|#\nreturn 0\nend\n";
        let expected = "fun main() I64 #| comment starts here\nend and match are comment text\n|#\n  return 0\nend\n";
        assert_eq!(format_source(source), expected);
    }

    #[test]
    fn formats_code_after_multiline_comment_closer() {
        let source = "fun main() I64\n#|\ncomment text\n|# if true\nreturn 1\nend\nreturn 0\nend\n";
        let expected = "fun main() I64\n  #|\ncomment text\n  |# if true\n    return 1\n  end\n  return 0\nend\n";
        assert_eq!(format_source(source), expected);
        assert_eq!(format_source(expected), expected);
    }

    #[test]
    fn ignores_structure_words_inside_string_literals() {
        // `"match"` must not open a block, `"end"` must not close one.
        let source = "fun main() I64\nprint(\"match\")\nprint(\"end\")\nreturn 0\nend\n";
        let expected = "fun main() I64\n  print(\"match\")\n  print(\"end\")\n  return 0\nend\n";
        assert_eq!(format_source(source), expected);
        assert_eq!(format_source(expected), expected);
    }

    #[test]
    fn ignores_comment_and_bracket_characters_inside_string_literals() {
        // `#` and brackets inside a literal are text; escaped quotes do not end it.
        let source = "fun main() I64\nprint(\"# ( not code\")\nprint(\"say \\\"hi\\\"\")\nreturn 0\nend\n";
        let expected = "fun main() I64\n  print(\"# ( not code\")\n  print(\"say \\\"hi\\\"\")\n  return 0\nend\n";
        assert_eq!(format_source(source), expected);
        assert_eq!(format_source(expected), expected);
    }

    #[test]
    fn preserves_literal_contents_verbatim() {
        // Literal bytes, including internal whitespace, come through unchanged.
        let source = "fun main() I64\nval text = \"  spaced   out  \"\nreturn 0\nend\n";
        let expected = "fun main() I64\n  val text = \"  spaced   out  \"\n  return 0\nend\n";
        assert_eq!(format_source(source), expected);
    }
}
