import type { Metadata } from "next";
import type { ComponentType, ReactNode } from "react";
import PageHeader from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import ShellBlock from "@/components/ShellBlock";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import {
  BuildIcon,
  CodegenIcon,
  LspIcon,
  RunIcon,
  StaticLinkIcon,
  TestIcon,
} from "@/components/brand/icons";
import { readPageContent } from "@/lib/page-content";
import { REPO_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Install",
  description:
    "Build the Cinnabar compiler with the project's Nix flake, run it from Docker on Windows, set up the language server, and verify a change against the repository's gate.",
  alternates: { canonical: "/install/" },
};

/** One step of the guide, with its own section rule. */
function Step({
  title,
  note,
  icon,
  children,
}: {
  title: string;
  note?: string;
  icon?: ComponentType<{ size?: number; className?: string }>;
  children: ReactNode;
}) {
  return (
    <section className="min-w-0">
      <SectionHeading title={title} note={note} icon={icon} />
      <Reveal className="mt-9 min-w-0">{children}</Reveal>
    </section>
  );
}

/** Prose at the document scale, filling its column. */
function Prose({ children }: { children: string }) {
  return (
    <div className="max-w-[86ch] [&_p:first-child]:mt-0">
      <Markdown>{children}</Markdown>
    </div>
  );
}

export default async function InstallPage() {
  const content = await readPageContent("install");

  /* Links to repository files are added here rather than written into
     content.md, so the copy stays readable and the URL lives in one place. */
  const withRepoLinks = (text: string) =>
    text
      .replace("`build.rs`", `[build.rs](${REPO_URL}/blob/main/build.rs)`)
      .replace("`flake.nix`", `[flake.nix](${REPO_URL}/blob/main/flake.nix)`)
      .replace(
        "`pre_commit_check.sh`",
        `[pre_commit_check.sh](${REPO_URL}/blob/main/pre_commit_check.sh)`,
      );

  return (
    <article className="pb-28">
      <PageHeader
        section="Install"
        note="LLVM 21 · static musl libc"
        icon={BuildIcon}
        title="Build the compiler."
        lede={
          <div className="text-secondary text-[18px] leading-[1.55] tracking-[-0.01em] text-pretty sm:text-[21px] [&_code]:font-mono [&_code]:text-[0.9em]">
            <InlineMarkdown>{content.block("lede")}</InlineMarkdown>
          </div>
        }
      />

      <div className="mx-auto flex max-w-[1400px] flex-col gap-20 px-6 pt-16 sm:px-10 [&>*]:min-w-0">
        <Step title="Nix — the supported path" note="The only setup that is tested" icon={BuildIcon}>
          <Prose>{content.block("nix")}</Prose>
          <ShellBlock
            lines={["nix develop", "cargo build --release"]}
            className="mt-7 max-w-[760px]"
          />
          <div className="mt-8">
            <Prose>{withRepoLinks(content.block("nix-outside"))}</Prose>
          </div>
        </Step>

        <Step title="First program" note="cinnabar init · run · check" icon={RunIcon}>
          <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start [&>*]:min-w-0">
            <div>
              <Prose>{content.block("first-program")}</Prose>
              <ShellBlock
                lines={[
                  "cinnabar init hello",
                  "cinnabar run hello",
                  "cinnabar check hello    # front end only, links nothing",
                ]}
                className="mt-7"
              />
            </div>
            <div>
              <Prose>{content.block("first-program-file")}</Prose>
              <ShellBlock
                lines={[
                  "cargo run -- tests/fixtures/spec.cnb",
                  "cargo run -- tests/fixtures/multi_file/main.cnb --run",
                  "cargo run -- my_program.cnb --dump-ast",
                ]}
                className="mt-7"
              />
            </div>
          </div>
        </Step>

        <Step
          title="Docker Desktop and Windows worktrees"
          note="One reusable Compose service"
          icon={StaticLinkIcon}
        >
          <Prose>{content.block("docker")}</Prose>
          <a
            href={`${REPO_URL}/blob/main/CONTAINER_DEVELOPMENT.md`}
            target="_blank"
            rel="noopener noreferrer"
            className="text-cinnabar-text hover:text-text panel-hover mt-7 inline-block text-[13px] font-bold tracking-[0.1em] uppercase"
          >
            Container development guide →
          </a>
        </Step>

        <Step title="Language server" note="cinnabar-lsp · stdio" icon={LspIcon}>
          <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start [&>*]:min-w-0">
            <Prose>{content.block("lsp")}</Prose>
            <div>
              <ShellBlock lines={["cargo build --release --bin cinnabar-lsp"]} />
              <div className="rule-grid mt-7 flex min-w-0">
                <pre
                  tabIndex={0}
                  className="bg-code-terminal text-term-output w-full overflow-x-auto px-6 py-5 font-mono text-[12.5px] leading-[1.7]"
                >
                  <code>{`vim.lsp.start({
  name = "cinnabar",
  cmd = { "/path/to/cinnabar-lsp" },
  root_dir = vim.fn.getcwd(),
})`}</code>
                </pre>
              </div>
              <div className="mt-6 [&_p]:text-[15px]">
                <Prose>{content.block("lsp-vscode")}</Prose>
              </div>
            </div>
          </div>
        </Step>

        <Step title="Verifying a change" note="nix develop --command" icon={TestIcon}>
          <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start [&>*]:min-w-0">
            <div>
              <Prose>{withRepoLinks(content.block("gate"))}</Prose>
              <ShellBlock
                lines={["nix develop --command ./pre_commit_check.sh"]}
                className="mt-7"
              />
            </div>
            <div>
              <Prose>{content.block("profiles")}</Prose>
              <ShellBlock
                lines={[
                  "nix develop --command cargo test --quiet",
                  "nix develop --command cargo test --quiet --features test-profile-balanced",
                  "nix develop --command cargo test --quiet --features test-profile-smoke",
                ]}
                className="mt-7"
              />
            </div>
          </div>
        </Step>

        <Reveal className="border-hairline bg-panel flex flex-col gap-5 border p-8 sm:p-10">
          <CodegenIcon size={22} className="text-cinnabar-text" />
          <p className="text-bright max-w-[70ch] text-[17px] leading-[1.6] text-pretty">
            On success the compiler prints{" "}
            <code className="border-hairline bg-ground text-bright border px-[5px] py-[2px] font-mono text-[0.875em]">
              Successfully compiled &lt;input&gt; to &apos;&lt;output&gt;&apos;.
            </code>{" "}
            and exits 0. Any failure is rendered as source-located diagnostics and
            exits non-zero. A build either produces its artifact or produces
            diagnostics — never both, and never part of one.
          </p>
        </Reveal>
      </div>
    </article>
  );
}
