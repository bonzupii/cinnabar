import type { Metadata } from "next";
import type { ComponentType, ReactNode } from "react";
import DataTable from "@/components/DataTable";
import Disclosure from "@/components/Disclosure";
import { InlineMarkdown } from "@/components/Markdown";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import ShellBlock, { PlainWindow } from "@/components/ShellBlock";
import Reveal from "@/components/Reveal";
import { ArrowLink, Callout, MarkedList, Prose } from "@/components/ui";
import {
  BuildIcon,
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
    "LLVM 21 via a Nix flake, a static musl libc, the language server, the WSL2 path for Windows, and the repository's verification gate.",
  alt: "Cinnabar social card — the getting-started guide.",
};

export const metadata: Metadata = {
  title: "Install",
  description:
    "Build the Cinnabar compiler with the project's Nix flake, run the same toolchain under WSL2 on Windows, wire up VS Code, and verify a change against the repository's gate.",
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
    <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start *:min-w-0">
      {children}
    </div>
  );
}

const NEOVIM_SETUP = `vim.lsp.start({
  name = "cinnabar",
  cmd = { "/path/to/cinnabar-lsp" },
  root_dir = vim.fn.getcwd(),
})`;

/* The attached-container configuration, verbatim from CONTAINER_DEVELOPMENT.md. */
const VSCODE_CONFIG = `{
  "workspaceFolder": "/workspace",
  "extensions": [
    "rust-lang.rust-analyzer"
  ],
  "settings": {
    "terminal.integrated.defaultProfile.linux": "bash",
    "rust-analyzer.server.path": "/workspace/container/bin/rust-analyzer-nix"
  },
  "remoteUser": "root"
}`;

/* Every Compose call takes the generated environment file and nothing else. */
const COMPOSE = 'docker compose --env-file "container/local/main/worktree.env"';

/** What survives a service recreation — CONTAINER_DEVELOPMENT.md's own table. */
const VOLUMES = [
  ["Selected checkout", "/workspace", "Host bind mount"],
  ["Nix store and database", "/nix", "Shared volume"],
  ["Nix flake fetch cache", "/root/.cache/nix", "Shared volume"],
  ["Cargo home", "/root/.cargo", "Shared volume"],
  ["Rust build output", "/workspace/target", "Volume, per worktree"],
  ["VS Code Server", "/root/.vscode-server", "Shared volume"],
  ["Gate log", "/workspace/pre_commit.log", "Selected host checkout"],
] as const;

const DOCKER_SAFETY = [
  "Allocate at least 8 GiB to Docker Desktop; 12 GiB is preferable for the full gate.",
  "Never share a target cache key between concurrently active worktrees.",
  "Never run two branches through the single service at once.",
  "Never rewrite a host .git pointer to a Linux path.",
  "Do not run docker compose down --volumes during normal work — it deletes the reusable caches.",
  "Read pre_commit.log from the selected host checkout after the gate exits.",
] as const;

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

      <div className="mx-auto flex max-w-350 flex-col gap-20 px-6 pt-16 sm:px-10 *:min-w-0">
        <Step
          title="Nix — the supported path"
          note="The only setup that is tested"
          icon={BuildIcon}
        >
          <Prose>{content.block("nix")}</Prose>
          <ShellBlock
            lines={["nix develop", "cargo build --release"]}
            cwd="~/src/cinnabar"
            className="mt-7 max-w-190"
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

        {/*
          The default Windows path, and the shorter of the two by a wide margin:
          there is no service to configure, select or retarget, so the whole
          Compose apparatus below has no counterpart here.
        */}
        <Step
          title="Windows — WSL2"
          note="The default on Windows"
          icon={StaticLinkIcon}
        >
          <Prose>{content.block("wsl")}</Prose>
          <ShellBlock
            lines={[
              "sh <(curl -fsSL https://releases.nixos.org/nix/nix-2.35.1/install) --no-daemon",
              `git clone ${REPO_URL}.git ~/dev/cinnabar`,
              "cd ~/dev/cinnabar && nix develop --command ./pre_commit_check.sh",
            ]}
            cwd="~"
            title="In the distro — never under /mnt/c"
            className="mt-7 max-w-190"
          />
        </Step>

        {/*
          Expanded from three sentences and a link. CONTAINER_DEVELOPMENT.md is
          the source for every command here — a Windows contributor should be
          able to reach a running gate without leaving the page.
        */}
        <Step
          title="Windows fallback — Docker Desktop"
          note="One reusable Compose service"
          icon={StaticLinkIcon}
        >
          <Prose>{content.block("docker")}</Prose>

          <Prose className="mt-8">{content.block("docker-volumes")}</Prose>
          <DataTable headings={["Data", "Container path", "Lifetime"]} data={VOLUMES} />

          <div className="mt-10 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start *:min-w-0">
            <div>
              <Prose>{content.block("docker-configure")}</Prose>
              <ShellBlock
                lines={[
                  './container/configure-worktree.sh --worktree "$PWD" --cache-key main',
                ]}
                cwd="~/src/cinnabar"
                title="Configure a checkout"
                className="mt-7"
              />
            </div>
            <div>
              <Prose>{content.block("docker-select")}</Prose>
              <ShellBlock
                lines={[
                  `${COMPOSE} config`,
                  `${COMPOSE} up -d --build`,
                  `${COMPOSE} exec dev nix develop`,
                  `${COMPOSE} exec dev nix develop --command ./pre_commit_check.sh`,
                  "cat pre_commit.log",
                ]}
                cwd="~/src/cinnabar"
                title="Select and start"
                className="mt-7"
              />
            </div>
          </div>

          <Prose className="mt-10">{content.block("docker-switch")}</Prose>
          <ShellBlock
            lines={[
              "./container/configure-worktree.sh --worktree /c/path/to/cinnabar-feature --cache-key feature",
              'docker compose --env-file "container/local/feature/worktree.env" up -d --build',
            ]}
            cwd="~/src/cinnabar"
            title="A linked worktree"
            className="mt-7"
          />

          <Prose className="mt-10">{content.block("docker-safety")}</Prose>
          <MarkedList items={DOCKER_SAFETY} className="mt-5" />

          {/* Only a reader changing the container setup needs this. */}
          <Disclosure summary="Verification checklist" className="mt-10">
            <Prose className="mt-4">{content.block("docker-verify")}</Prose>
            <ShellBlock
              lines={[
                "git status --short --branch",
                "nix --version",
                "nix develop --command rust-analyzer --version",
                "nix develop --command ./pre_commit_check.sh",
              ]}
              cwd="/workspace"
              title="Inside the container"
              className="mt-7"
            />
          </Disclosure>

          <ArrowLink
            href={`${REPO_URL}/blob/main/CONTAINER_DEVELOPMENT.md`}
            external
            className="mt-8 inline-block"
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
              <PlainWindow
                text={NEOVIM_SETUP}
                path="init.lua"
                title="Neovim"
                className="mt-7"
              />
            </div>
          </SplitStep>
        </Step>

        {/*
          VS Code gets its own step rather than one line under the language
          server: attaching to the container, the rust-analyzer wrapper and the
          Cinnabar extension are things a reader has to do in order.
        */}
        <Step title="VS Code" note="WSL remote or attached container" icon={LspIcon}>
          <Prose>{content.block("vscode-attach")}</Prose>

          <div className="mt-9 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start *:min-w-0">
            <Prose>{content.block("vscode-config")}</Prose>
            <PlainWindow
              text={VSCODE_CONFIG}
              path="nameConfigs/dev.json"
              title="Attached container configuration"
            />
          </div>

          <div className="mt-10 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start *:min-w-0">
            <Prose>{content.block("vscode-analyzer")}</Prose>
            <ShellBlock
              lines={[
                'docker exec dev ps -eo pid,ppid,args | grep -E "rust-analyzer" | grep -v grep',
              ]}
              cwd="~/src/cinnabar"
              title="Which rust-analyzer is running"
            />
          </div>

          <div className="mt-10 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start *:min-w-0">
            <Prose>{content.block("vscode-extension")}</Prose>
            <ShellBlock
              lines={[
                "nix develop --command ./container/install-vscode-extension.sh",
                `${COMPOSE} exec dev nix develop --command ./container/install-vscode-extension.sh`,
                `docker exec dev sh -c 'readlink /proc/$(pgrep -f "cinnabar[-]lsp --stdio" | head -1)/exe'`,
              ]}
              cwd="~/src/cinnabar"
              title="Install it — WSL2, then the container, then check the server is not stale"
            />
          </div>
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

        {/*
          The page's closing statement, in the shape the site's other callouts
          already use: an eyebrow, the claim, then how it is made good. It was
          a lone 22px icon on its own line above a paragraph, which anchored
          nothing and left the box reading half-empty — and the icon carried no
          meaning the eyebrow does not carry in words, so it is gone rather
          than moved. Compare the Single-Fact Rule on /architecture/ and the
          horizon on /roadmap/, both eyebrow-first.

          The prose is markdown rather than JSX because the sentence quotes
          what the compiler prints, and a literal in backticks in content.md is
          the same string the compiler emits — where the JSX spelled it with
          five HTML entities.
        */}
        <Reveal>
          <Callout>
            <Eyebrow>What a build leaves you with</Eyebrow>
            <h2 className="text-text max-w-[46ch] text-[28px] leading-tight font-bold tracking-tight text-balance sm:text-[36px]">
              {content.block("outcome-title")}
            </h2>
            <div className="text-bright max-w-[80ch] text-[17px] leading-[1.6] text-pretty">
              <InlineMarkdown>{content.block("outcome")}</InlineMarkdown>
            </div>
          </Callout>
        </Reveal>
      </div>
    </article>
  );
}
