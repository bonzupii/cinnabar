import { describe, expect, it } from "vitest";
import {
  classifyIdentifier,
  tokenizeCinnabar,
  type Token,
} from "@/lib/cinnabar-syntax";

/** Collapses tokens to `kind:value` pairs, dropping whitespace, for readable assertions. */
function significant(tokens: Token[]): string[] {
  return tokens
    .filter((token) => token.kind !== "text")
    .map((token) => `${token.kind}:${token.value}`);
}

describe("classifyIdentifier", () => {
  it("reads casing as grammar", () => {
    // snake_case: bindings, functions, parameters.
    expect(classifyIdentifier("vec_new")).toBe("identifier");
    expect(classifyIdentifier("acc")).toBe("identifier");
    // PascalCase: types, traits, modules, enum variants.
    expect(classifyIdentifier("MoveCount")).toBe("type");
    expect(classifyIdentifier("Ok")).toBe("type");
    expect(classifyIdentifier("Collections")).toBe("type");
    // SCREAMING_SNAKE_CASE: constants.
    expect(classifyIdentifier("BAD_NEW")).toBe("constant");
    expect(classifyIdentifier("EXPECTED_SUM")).toBe("constant");
  });

  it("keeps the digit-bearing builtin types out of the constant bucket", () => {
    // `I64` carries no lowercase letter, so the casing rule alone would read
    // it as SCREAMING_SNAKE_CASE.
    expect(classifyIdentifier("I64")).toBe("type");
    expect(classifyIdentifier("U8")).toBe("type");
    expect(classifyIdentifier("Usize")).toBe("type");
    expect(classifyIdentifier("Unit")).toBe("type");
  });

  it("treats a lone uppercase letter as a generic parameter", () => {
    expect(classifyIdentifier("T")).toBe("type");
    expect(classifyIdentifier("K")).toBe("type");
  });

  it("recognises every keyword", () => {
    for (const keyword of ["pub", "fun", "val", "var", "match", "end", "try", "impure", "elif"]) {
      expect(classifyIdentifier(keyword)).toBe("keyword");
    }
  });

  it("colours booleans as literals rather than keywords", () => {
    expect(classifyIdentifier("true")).toBe("literal");
    expect(classifyIdentifier("false")).toBe("literal");
  });
});

describe("tokenizeCinnabar", () => {
  it("tokenizes a declaration the way plate 09 sets it", () => {
    expect(significant(tokenizeCinnabar("const BAD_NEW: I64 = 1"))).toEqual([
      "keyword:const",
      "constant:BAD_NEW",
      "punctuation::",
      "type:I64",
      "punctuation:=",
      "literal:1",
    ]);
  });

  it("brightens an applied identifier into a call", () => {
    // `vec_new` is applied; `vec` is not.
    const tokens = significant(tokenizeCinnabar("val vec = vec_new[I64]()"));
    expect(tokens).toContain("function:vec_new");
    expect(tokens).toContain("identifier:vec");
  });

  it("marks caller-declared linear handles structurally, not chromatically", () => {
    const tokens = tokenizeCinnabar("vec_free(vec)", ["vec"]);
    const handle = tokens.find((token) => token.value === "vec");
    expect(handle).toMatchObject({ kind: "identifier", linear: true });
    // The call keeps its own kind and gains no linear rule.
    expect(tokens.find((token) => token.value === "vec_free")).toMatchObject({
      kind: "function",
    });
    expect(tokens.find((token) => token.value === "vec_free")?.linear).toBeUndefined();
  });

  it("handles all four comment forms", () => {
    expect(significant(tokenizeCinnabar("# discarded"))).toEqual(["comment:# discarded"]);
    expect(significant(tokenizeCinnabar("#! attached"))).toEqual([
      "doc-comment:#! attached",
    ]);
    expect(significant(tokenizeCinnabar("#| block |#"))).toEqual([
      "comment:#| block |#",
    ]);
    expect(significant(tokenizeCinnabar("#!| block doc |#"))).toEqual([
      "doc-comment:#!| block doc |#",
    ]);
  });

  it("ends a line comment at the newline, not at the end of the program", () => {
    const tokens = significant(tokenizeCinnabar("# note\nreturn 0"));
    expect(tokens).toEqual(["comment:# note", "keyword:return", "literal:0"]);
  });

  it("closes a block comment at its first terminator, since they do not nest", () => {
    const tokens = significant(tokenizeCinnabar("#| a |# end"));
    expect(tokens).toEqual(["comment:#| a |#", "keyword:end"]);
  });

  it("does not let an unterminated string swallow the rest of the source", () => {
    // A Cinnabar literal cannot span a line, so the highlighter must stop at
    // the newline the same way the lexer does.
    const tokens = significant(tokenizeCinnabar('val a = "oops\nreturn 0'));
    expect(tokens).toEqual([
      "keyword:val",
      "identifier:a",
      "punctuation:=",
      'string:"oops',
      "keyword:return",
      "literal:0",
    ]);
  });

  it("keeps escapes inside a string literal", () => {
    expect(significant(tokenizeCinnabar('"a\\"b"'))).toEqual(['string:"a\\"b"']);
  });

  it("lexes hex literals whole", () => {
    expect(significant(tokenizeCinnabar("0x0D"))).toEqual(["literal:0x0D"]);
    expect(significant(tokenizeCinnabar("0xF0AD"))).toEqual(["literal:0xF0AD"]);
  });

  it("prefers the longest operator so shifts never split", () => {
    expect(significant(tokenizeCinnabar("a << b"))).toEqual([
      "identifier:a",
      "punctuation:<<",
      "identifier:b",
    ]);
    expect(significant(tokenizeCinnabar("x <= y"))).toEqual([
      "identifier:x",
      "punctuation:<=",
      "identifier:y",
    ]);
    expect(significant(tokenizeCinnabar("Ok(v) => v"))).toEqual([
      "type:Ok",
      "punctuation:(",
      "identifier:v",
      "punctuation:)",
      "punctuation:=>",
      "identifier:v",
    ]);
  });

  it("lexes the array rest pattern as one token", () => {
    const tokens = significant(tokenizeCinnabar("[first, rest @ ..]"));
    expect(tokens).toContain("punctuation:..");
    expect(tokens).toContain("punctuation:@");
  });

  it("round-trips the source exactly", () => {
    // Nothing may be dropped: the rendered block must equal the input.
    const source = [
      "pub fun main() impure I64",
      '  val greeting = "hi\\n"   # trailing',
      "  return 0",
      "end",
      "",
    ].join("\n");
    expect(tokenizeCinnabar(source).map((token) => token.value).join("")).toBe(source);
  });
});
