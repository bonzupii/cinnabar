import type { Metadata } from "next";
import type { ComponentType, ReactNode } from "react";
import PageHeader from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import ShellBlock, { PlainWindow } from "@/components/ShellBlock";
import Reveal from "@/components/Reveal";
import { ArrowLink, Callout, Code, Prose } from "@/components/ui";
import {
  BuildIcon,
  CodegenIcon,
  LspIcon,
  RunIcon,
  StaticLinkIcon,
  TestIcon,
} from "@/components/brand/icons";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
import { REPO_URL } from "@/lib/site";

/** Social image copy, rendered by ./og-image/route.tsx. */
export const og = {
  eyebrow: "Getting started",
  title: "Build the compiler.",
  description:
    "LLVM 21 via a Nix flake, a static musl libc, the language server, and the repository's verification gate.",
  alt: "Cinnabar social card — the getting-started guide.",
};

export const metadata: Metadata = {
  title: "Install",
  description:
    "Build the Cinnabar compiler with the project's Nix flake, run it from Docker on Windows, set up the language server, and verify a change against the repository's gate.",
  alternates: { canonical: "/install/" },
  ...ogImageMetadata("/install/", og),
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

/** Two columns that both clamp, so a wide transcript scrolls instead of pushing. */
function SplitStep({ children }: { children: ReactNode }) {
  return (
    <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start [&>*]:min-w-0">
      {children}
    </div>
  );
}

const NEOVIM_SETUP = `vim.lsp.start({
  name = "cinnabar",
  cmd = { "/path/to/cinnabar-lsp" },
  root_dir = vim.fn.getcwd(),
})`;

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
        lede={content.block("lede")}
      />

      <div className="mx-auto flex max-w-[1400px] flex-col gap-20 px-6 pt-16 sm:px-10 [&>*]:min-w-0">
        <Step
          title="Nix — the supported path"
          note="The only setup that is tested"
          icon={BuildIcon}
        >
          <Prose>{content.block("nix")}</Prose>
          <ShellBlock
            lines={["nix develop", "cargo build --release"]}
            cwd="~/src/cinnabar"
            className="mt-7 max-w-[760px]"
          />
          <Prose className="mt-8">{withRepoLinks(content.block("nix-outside"))}</Prose>
        </Step>

        <Step title="First program" note="init · run · check" icon={RunIcon}>
          <SplitStep>
            <div>
              <Prose>{content.block("first-program")}</Prose>
              <ShellBlock
                lines={[
                  "cinnabar init hello",
                  "cinnabar run hello",
                  "cinnabar check hello    # front end only, links nothing",
                ]}
                cwd="~/src"
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
                cwd="~/src/cinnabar"
                className="mt-7"
              />
            </div>
          </SplitStep>
        </Step>

        <Step
          title="Docker Desktop and Windows worktrees"
          note="One reusable Compose service"
          icon={StaticLinkIcon}
        >
          <Prose>{content.block("docker")}</Prose>
          <ArrowLink
            href={`${REPO_URL}/blob/main/CONTAINER_DEVELOPMENT.md`}
            external
            className="mt-7 inline-block"
          >
            Container development guide
          </ArrowLink>
        </Step>

        <Step title="Language server" note="cinnabar-lsp · stdio" icon={LspIcon}>
          <SplitStep>
            <Prose>{content.block("lsp")}</Prose>
            <div>
              <ShellBlock
                lines={["cargo build --release --bin cinnabar-lsp"]}
                cwd="~/src/cinnabar"
              />
              <PlainWindow text={NEOVIM_SETUP} path="init.lua" title="Neovim" className="mt-7" />
              <Prose className="mt-6 [&_p]:text-[15px]">
                {content.block("lsp-vscode")}
              </Prose>
            </div>
          </SplitStep>
        </Step>

        <Step title="Verifying a change" note="nix develop --command" icon={TestIcon}>
          <SplitStep>
            <div>
              <Prose>{withRepoLinks(content.block("gate"))}</Prose>
              <ShellBlock
                lines={["nix develop --command ./pre_commit_check.sh"]}
                cwd="~/src/cinnabar"
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
                cwd="~/src/cinnabar"
                className="mt-7"
              />
            </div>
          </SplitStep>
        </Step>

        <Reveal>
          <Callout>
            <CodegenIcon size={22} className="text-cinnabar-text" />
            <p className="text-bright max-w-[70ch] text-[17px] leading-[1.6] text-pretty">
              On success the compiler prints{" "}
              <Code>Successfully compiled &lt;input&gt; to &apos;&lt;output&gt;&apos;.</Code>{" "}
              and exits 0. Any failure is rendered as source-located diagnostics and
              exits non-zero. A build either produces its artifact or produces
              diagnostics — never both, and never part of one.
            </p>
          </Callout>
        </Reveal>
      </div>
    </article>
  );
}
