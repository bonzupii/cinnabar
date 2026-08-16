# Documentation source map

This map records the approved information architecture for the site branch.
Repository sources remain authoritative; route copy summarizes and links to
them rather than becoming a competing specification.

| Public surface | Authoritative source | Branch decision |
| --- | --- | --- |
| `/install/` | `README.md`, `flake.nix`, current install guide | Visitor setup, first project, and LSP only |
| `/contributing/development/` | `CONTAINER_DEVELOPMENT.md`, `AGENTS.md` | Docker, worktrees, editor containers, caches, and the repository gate |
| `/reference/` | CLI implementation, `README.md`, `build.cnb` behavior | Keep the permalink; label it “CLI Reference” |
| `/learn/why-cinnabar/` | `MANIFESTO.md` introduction and principles | Motivation and tradeoffs, with the manifesto linked as normative |
| `/learn/linear-types/` | `MANIFESTO.md` section 7 and linear fixtures | Ownership obligations and branch-sensitive consumption |
| `/learn/borrowing/` | `MANIFESTO.md` sections 5–6 and fixtures | Shared/exclusive borrows and inferred flow-sensitive scopes |
| `/learn/error-handling/` | `MANIFESTO.md` failure rules | `Result`, `Option`, `try`, division, and indexing |
| `/learn/first-program/` | `README.md`, install guide, `tests/fixtures/spec.cnb` | Smallest route from checkout to a checked project |
| `/architecture/` | `ARCHITECTURE.md`, pipeline sources | Anchored chapters on one stable route; raw source linked, not duplicated in full |

Primary navigation is visitor-first: Playground, Learn, Install, CLI Reference,
Architecture, and Roadmap. Manifesto and contributor documentation remain
public routes linked from the learning hub and footer/project context.
