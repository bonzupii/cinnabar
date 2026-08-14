import type { ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSlug from "rehype-slug";
import CodeBlock from "@/components/CodeBlock";
import { PlainWindow } from "@/components/ShellBlock";

/*
 * Renders markdown in the brand's type scale (plate 06).
 *
 * Nothing here sets a measure. Line length is a layout decision that belongs
 * to the page — a lede beside a heading, a full-width document body and a
 * caption in a narrow column all want different widths from the same
 * component, so each block element fills whatever container it is given.
 *
 * Fenced blocks tagged `cinnabar` go through the Cinnabar Dark highlighter;
 * every other block is set in mono without colour, because the theme has no
 * palette for shell or Rust and inventing one would break plate 14's last
 * misuse rule ("Do not add colours to the syntax theme").
 *
 * `rehype-slug` supplies the heading ids the documents' own cross-links
 * (`MANIFESTO.md#7-linear-types`) depend on.
 */

/**
 * Titles a fenced block that is not Cinnabar source.
 *
 * The documents tag blocks as `bash`, `lua`, `rust` and so on, and some not at
 * all. Whatever the fence says becomes the window title, so a reader can tell
 * a shell session from a diagram without the site inventing a theme for each
 * language — which plate 14's last misuse rule forbids anyway.
 */
function plainTitle(language: string | undefined): { path: string; title: string } {
  if (!language || language === "text") {
    return { path: "output", title: "Output" };
  }
  const titles: Record<string, string> = {
    bash: "Shell",
    sh: "Shell",
    console: "Shell",
    lua: "Editor config",
    rust: "Rust",
    toml: "Configuration",
    json: "Configuration",
  };
  return { path: language, title: titles[language] ?? language.toUpperCase() };
}

const INLINE_CODE =
  "border-hairline bg-panel text-bright border px-[5px] py-[2px] font-mono text-[0.875em] break-words";

/** Shared renderers; the two entry points differ only in block spacing. */
function components(inline: boolean): Components {
  return {
    h2: ({ children, id }) => (
      <h2
        id={id}
        className="border-hairline text-text mt-20 scroll-mt-24 border-t pt-10 text-[28px] leading-tight font-bold tracking-[-0.025em] break-words first:mt-0 sm:text-[34px]"
      >
        {children}
      </h2>
    ),
    h3: ({ children, id }) => (
      <h3
        id={id}
        className="text-text mt-12 scroll-mt-24 text-[19px] leading-snug font-bold tracking-[-0.015em] break-words sm:text-[22px]"
      >
        {children}
      </h3>
    ),
    h4: ({ children, id }) => (
      <h4
        id={id}
        className="text-bright mt-10 scroll-mt-24 text-[16px] font-bold tracking-[-0.01em] break-words"
      >
        {children}
      </h4>
    ),
    p: ({ children }) =>
      inline ? (
        <p className="[&:not(:first-child)]:mt-4">{children}</p>
      ) : (
        <p className="text-secondary mt-5 text-[16.5px] leading-[1.75] break-words text-pretty">
          {children}
        </p>
      ),
    a: ({ children, href }) => {
      const external = href?.startsWith("http");
      return (
        <a
          href={href}
          target={external ? "_blank" : undefined}
          rel={external ? "noopener noreferrer" : undefined}
          className="text-cinnabar-text decoration-cinnabar-text/40 hover:decoration-cinnabar-text underline underline-offset-[3px]"
        >
          {children}
        </a>
      );
    },
    ul: ({ children }) => (
      <ul className="mt-5 flex list-none flex-col gap-3 pl-0">{children}</ul>
    ),
    ol: ({ children }) => (
      <ol className="text-secondary marker:text-label mt-5 flex list-decimal flex-col gap-3 pl-6 marker:font-mono marker:text-[13px]">
        {children}
      </ol>
    ),
    li: ({ children }) => (
      <li className="text-secondary relative pl-6 text-[16.5px] leading-[1.7] break-words before:absolute before:top-[0.62em] before:left-0 before:h-[6px] before:w-[6px] before:bg-[color:var(--hairline-strong)] before:content-[''] [ol_&]:pl-0 [ol_&]:before:hidden">
        {children}
      </li>
    ),
    strong: ({ children }) => (
      <strong className="text-text font-bold">{children}</strong>
    ),
    em: ({ children }) => <em className="text-bright not-italic">{children}</em>,
    blockquote: ({ children }) => (
      <blockquote className="border-cinnabar text-bright my-8 border-l-2 pl-6 [&>p]:text-[17px]">
        {children}
      </blockquote>
    ),
    hr: () => <hr className="border-hairline my-16 border-t" />,
    table: ({ children }) => (
      <div className="rule-grid my-8 block overflow-x-auto">
        <table className="bg-ground w-full border-collapse text-left">{children}</table>
      </div>
    ),
    thead: ({ children }) => <thead className="bg-panel">{children}</thead>,
    th: ({ children }) => (
      <th className="border-hairline text-label border-b px-5 py-3 font-mono text-[10px] font-medium tracking-[0.16em] whitespace-nowrap uppercase">
        {children}
      </th>
    ),
    td: ({ children }) => (
      <td className="border-hairline text-secondary border-b px-5 py-3 align-top text-[14px] leading-relaxed break-words">
        {children}
      </td>
    ),
    code: ({ className, children }) => {
      const text = String(children ?? "").replace(/\n$/, "");
      const language = /language-([\w-]+)/.exec(className ?? "")?.[1];
      // Inline code never spans lines; a fenced block either declares a
      // language or contains a newline. That distinction is enough, and avoids
      // depending on the `inline` prop react-markdown dropped.
      const isBlock = language !== undefined || text.includes("\n");

      if (!isBlock) return <code className={INLINE_CODE}>{children}</code>;
      if (language === "cinnabar") {
        return <CodeBlock code={text} path="fixture.cnb" className="my-8" />;
      }
      const plain = plainTitle(language);
      return (
        <PlainWindow
          text={text}
          path={plain.path}
          title={plain.title}
          className="my-8"
        />
      );
    },
    // `code` above renders the whole block, so the wrapper is redundant.
    pre: ({ children }) => <>{children}</>,
  };
}

const BLOCK_COMPONENTS = components(false);
const INLINE_COMPONENTS = components(true);

/** Document-style markdown, with its own type scale and vertical rhythm. */
export default function Markdown({ children }: { children: string }): ReactNode {
  return (
    <div className="min-w-0">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeSlug]}
        components={BLOCK_COMPONENTS}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}

/**
 * Markdown that inherits the surrounding typography.
 *
 * For copy that is styled by its container — a hero lede at 27px, a caption at
 * 13px — where the point of using markdown is emphasis and inline code, not a
 * type scale of its own.
 */
export function InlineMarkdown({ children }: { children: string }): ReactNode {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[rehypeSlug]}
      components={INLINE_COMPONENTS}
    >
      {children}
    </ReactMarkdown>
  );
}
